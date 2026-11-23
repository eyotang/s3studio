use super::trash::Cursor;
use super::*;
use aws_sdk_s3::types::{BucketVersioningStatus, CompletedMultipartUpload, CompletedPart};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectVersion {
    pub id: String,
    pub modified: String,
    pub size: Option<i64>,
    pub latest: bool,
    pub deleted: bool,
}

#[derive(Serialize, Deserialize)]
pub struct VersionPage {
    pub versions: Vec<ObjectVersion>,
    pub cursor: Option<Cursor>,
}

pub async fn list(
    store: &AccountStore,
    account: String,
    bucket: String,
    key: String,
    cursor: Option<Cursor>,
) -> Result<VersionPage, String> {
    let client = bucket_client(store, &account, &bucket).await?;
    list_page(&client, &bucket, &key, cursor).await
}

async fn list_page(
    client: &Client,
    bucket: &str,
    key: &str,
    cursor: Option<Cursor>,
) -> Result<VersionPage, String> {
    let response = client
        .list_object_versions()
        .bucket(bucket)
        .prefix(key)
        .max_keys(200)
        .set_key_marker(cursor.as_ref().map(|c| c.key.clone()))
        .set_version_id_marker(cursor.as_ref().and_then(|c| c.version.clone()))
        .send()
        .await
        .map_err(failure)?;
    let next = trash::next_cursor(&response)?.filter(|c| c.key == key);
    if next.is_some() && next == cursor {
        return Err(crate::i18n::t("服务器返回了重复的分页标记。").into());
    }
    let mut versions: Vec<_> = response
        .versions()
        .iter()
        .filter(|v| v.key() == Some(key))
        .filter_map(|v| {
            Some(ObjectVersion {
                id: v.version_id()?.into(),
                modified: v
                    .last_modified()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                size: v.size(),
                latest: v.is_latest() == Some(true),
                deleted: false,
            })
        })
        .collect();
    versions.extend(
        response
            .delete_markers()
            .iter()
            .filter(|v| v.key() == Some(key))
            .filter_map(|v| {
                Some(ObjectVersion {
                    id: v.version_id()?.into(),
                    modified: v
                        .last_modified()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                    size: None,
                    latest: v.is_latest() == Some(true),
                    deleted: true,
                })
            }),
    );
    versions.sort_by(|a, b| {
        b.latest
            .cmp(&a.latest)
            .then_with(|| b.modified.cmp(&a.modified))
    });
    Ok(VersionPage {
        versions,
        cursor: next,
    })
}

pub async fn restore(
    store: &AccountStore,
    account: String,
    bucket: String,
    key: String,
    version: String,
    expected: String,
) -> Result<(), String> {
    let client = bucket_client(store, &account, &bucket).await?;
    restore_with_client(&client, &bucket, &key, &version, &expected).await
}

async fn restore_with_client(
    client: &Client,
    bucket: &str,
    key: &str,
    version: &str,
    expected: &str,
) -> Result<(), String> {
    let changed = crate::i18n::t("对象状态已变化，请刷新版本列表后重试。");
    if key.is_empty() || version.is_empty() || expected.is_empty() || version == expected {
        return Err(changed.into());
    }
    let status = client
        .get_bucket_versioning()
        .bucket(bucket)
        .send()
        .await
        .map_err(failure)?;
    if status.status() != Some(&BucketVersioningStatus::Enabled) {
        return Err(crate::i18n::t("请先在存储服务中启用版本控制，再按版本还原。").into());
    }
    let current = client
        .list_object_versions()
        .bucket(bucket)
        .prefix(key)
        .max_keys(1)
        .send()
        .await
        .map_err(failure)?;
    let etag = if let Some(v) = current.versions().iter().find(|v| {
        v.key() == Some(key) && v.is_latest() == Some(true) && v.version_id() == Some(expected)
    }) {
        Some(v.e_tag().ok_or(changed)?.to_owned())
    } else if current.delete_markers().iter().any(|v| {
        v.key() == Some(key) && v.is_latest() == Some(true) && v.version_id() == Some(expected)
    }) {
        None
    } else {
        return Err(changed.into());
    };
    let source = client
        .head_object()
        .bucket(bucket)
        .key(key)
        .version_id(version)
        .send()
        .await
        .map_err(failure)?;
    let size = source.content_length().filter(|s| *s >= 0).ok_or(changed)? as u64;
    let source_etag = source.e_tag().ok_or(changed)?;
    let from = format!(
        "{}?versionId={}",
        copy_source(bucket, key),
        percent_encoding::utf8_percent_encode(version, percent_encoding::NON_ALPHANUMERIC)
    );
    if size <= 5 * 1024 * 1024 * 1024 {
        client
            .copy_object()
            .bucket(bucket)
            .key(key)
            .copy_source(from)
            .copy_source_if_match(source_etag)
            .set_if_match(etag.clone())
            .set_if_none_match(etag.is_none().then(|| "*".into()))
            .send()
            .await
            .map_err(failure)?;
        return Ok(());
    }
    multipart_restore(client, bucket, key, version, &from, &source, etag, size).await
}

async fn multipart_restore(
    client: &Client,
    bucket: &str,
    key: &str,
    version: &str,
    from: &str,
    source: &aws_sdk_s3::operation::head_object::HeadObjectOutput,
    etag: Option<String>,
    size: u64,
) -> Result<(), String> {
    // Multipart copy must explicitly preserve user metadata and object tags.
    let tags = client
        .get_object_tagging()
        .bucket(bucket)
        .key(key)
        .version_id(version)
        .send()
        .await
        .map_err(failure)?;
    let tagging = tags
        .tag_set()
        .iter()
        .map(|tag| {
            format!(
                "{}={}",
                percent_encoding::utf8_percent_encode(
                    tag.key(),
                    percent_encoding::NON_ALPHANUMERIC
                ),
                percent_encoding::utf8_percent_encode(
                    tag.value(),
                    percent_encoding::NON_ALPHANUMERIC
                )
            )
        })
        .collect::<Vec<_>>()
        .join("&");
    let expires = source
        .expires_string()
        .map(|value| {
            aws_smithy_types::DateTime::from_str(
                value,
                aws_smithy_types::date_time::Format::HttpDate,
            )
        })
        .transpose()
        .map_err(failure)?;
    let created = client
        .create_multipart_upload()
        .bucket(bucket)
        .key(key)
        .set_metadata(source.metadata().cloned())
        .set_content_type(source.content_type().map(str::to_owned))
        .set_content_encoding(source.content_encoding().map(str::to_owned))
        .set_content_language(source.content_language().map(str::to_owned))
        .set_content_disposition(source.content_disposition().map(str::to_owned))
        .set_cache_control(source.cache_control().map(str::to_owned))
        .set_expires(expires)
        .set_storage_class(source.storage_class().cloned())
        .set_tagging((!tagging.is_empty()).then_some(tagging))
        .send()
        .await
        .map_err(failure)?;
    let upload = created
        .upload_id()
        .ok_or(crate::i18n::t("服务未返回 multipart UploadId。"))?;
    let result = async {
        let part_size = (512 * 1024 * 1024u64).max(size.div_ceil(10_000));
        let mut parts = Vec::new();
        for number in 0..size.div_ceil(part_size) {
            let start = number * part_size;
            let end = (start + part_size).min(size) - 1;
            let copied = client
                .upload_part_copy()
                .bucket(bucket)
                .key(key)
                .upload_id(upload)
                .part_number((number + 1) as i32)
                .copy_source(from)
                .set_copy_source_if_match(source.e_tag().map(str::to_owned))
                .copy_source_range(format!("bytes={start}-{end}"))
                .send()
                .await
                .map_err(failure)?;
            let etag = copied
                .copy_part_result()
                .and_then(|p| p.e_tag())
                .ok_or(crate::i18n::t("服务端分片缺少 ETag。"))?;
            parts.push(
                CompletedPart::builder()
                    .part_number((number + 1) as i32)
                    .e_tag(etag)
                    .build(),
            );
        }
        client
            .complete_multipart_upload()
            .bucket(bucket)
            .key(key)
            .upload_id(upload)
            .multipart_upload(
                CompletedMultipartUpload::builder()
                    .set_parts(Some(parts))
                    .build(),
            )
            .set_if_match(etag.clone())
            .set_if_none_match(etag.is_none().then(|| "*".into()))
            .send()
            .await
            .map_err(failure)?;
        Ok(())
    }
    .await;
    match result {
        Ok(()) => Ok(()),
        Err(error) => match client
            .abort_multipart_upload()
            .bucket(bucket)
            .key(key)
            .upload_id(upload)
            .send()
            .await
        {
            Ok(_) => Err(error),
            Err(cleanup) => Err(crate::lf!(
                "{error}；分片清理失败：{}",
                failure(cleanup),
                error = error
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> (Client, tiny_http::Server) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let client = Client::from_conf(
            aws_sdk_s3::Config::builder()
                .behavior_version(BehaviorVersion::latest())
                .region(Region::new("us-east-1"))
                .credentials_provider(Credentials::new("test", "test", None, None, "test"))
                .force_path_style(true)
                .endpoint_url(format!("http://{}", server.server_addr()))
                .retry_config(
                    aws_sdk_s3::config::retry::RetryConfig::standard().with_max_attempts(1),
                )
                .build(),
        );
        (client, server)
    }
    fn request(server: &tiny_http::Server, method: &str, query: &str) -> tiny_http::Request {
        let request = server
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .expect("S3 request");
        assert_eq!(request.method().as_str(), method);
        assert!(
            request.url().contains(query),
            "{} should contain {query}",
            request.url()
        );
        request
    }
    fn header(request: &tiny_http::Request, name: &str) -> String {
        request
            .headers()
            .iter()
            .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
            .unwrap()
            .value
            .to_string()
    }
    fn respond(request: tiny_http::Request, xml: &str) {
        request
            .respond(tiny_http::Response::from_string(xml))
            .unwrap();
    }
    fn no_more(server: &tiny_http::Server) {
        assert!(server
            .recv_timeout(Duration::from_millis(100))
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn version_listing_filters_prefix_neighbors_and_includes_delete_markers() {
        let (client, server) = server();
        let thread = std::thread::spawn(move || {
            respond(
                request(&server, "GET", "versions"),
                r#"<ListVersionsResult><IsTruncated>true</IsTruncated><NextKeyMarker>file</NextKeyMarker><NextVersionIdMarker>old</NextVersionIdMarker><Version><Key>file</Key><VersionId>old</VersionId><IsLatest>false</IsLatest><Size>12</Size></Version><Version><Key>file-other</Key><VersionId>unrelated</VersionId><IsLatest>true</IsLatest></Version><DeleteMarker><Key>file</Key><VersionId>deleted</VersionId><IsLatest>true</IsLatest></DeleteMarker></ListVersionsResult>"#,
            );
            respond(
                request(&server, "GET", "version-id-marker=old"),
                r#"<ListVersionsResult><IsTruncated>true</IsTruncated><NextKeyMarker>file-other</NextKeyMarker><NextVersionIdMarker>other</NextVersionIdMarker></ListVersionsResult>"#,
            );
        });
        let page = list_page(&client, "bucket", "file", None).await.unwrap();
        assert_eq!(page.versions.len(), 2);
        assert!(page.versions[0].deleted);
        assert_eq!(page.versions[1].size, Some(12));
        assert!(list_page(&client, "bucket", "file", page.cursor)
            .await
            .unwrap()
            .cursor
            .is_none());
        thread.join().unwrap();
    }

    #[tokio::test]
    async fn restore_copies_the_exact_encoded_version_and_checks_current_etag() {
        let (client, server) = server();
        let thread = std::thread::spawn(move || {
            respond(
                request(&server, "GET", "versioning"),
                "<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>",
            );
            respond(
                request(&server, "GET", "versions"),
                r#"<ListVersionsResult><Version><Key>folder/a b.txt</Key><VersionId>current</VersionId><IsLatest>true</IsLatest><ETag>&quot;current-etag&quot;</ETag></Version></ListVersionsResult>"#,
            );
            request(&server, "HEAD", "versionId=old%2B%2F%3D")
                .respond(tiny_http::Response::new(
                    tiny_http::StatusCode(200),
                    vec![tiny_http::Header::from_bytes("ETag", "\"old-etag\"").unwrap()],
                    std::io::empty(),
                    Some(12),
                    None,
                ))
                .unwrap();
            let copy = request(&server, "PUT", "/folder/a%20b.txt");
            assert_eq!(
                header(&copy, "x-amz-copy-source"),
                "bucket/folder/a%20b%2Etxt?versionId=old%2B%2F%3D"
            );
            assert_eq!(header(&copy, "x-amz-copy-source-if-match"), "\"old-etag\"");
            assert_eq!(header(&copy, "if-match"), "\"current-etag\"");
            respond(
                copy,
                "<CopyObjectResult><ETag>\"new-etag\"</ETag></CopyObjectResult>",
            );
            no_more(&server);
        });
        restore_with_client(&client, "bucket", "folder/a b.txt", "old+/=", "current")
            .await
            .unwrap();
        thread.join().unwrap();
    }

    #[tokio::test]
    async fn restore_refuses_suspended_versioning_and_stale_current_versions() {
        for suspended in [true, false] {
            let (client, server) = server();
            let thread = std::thread::spawn(move || {
                respond(
                    request(&server, "GET", "versioning"),
                    if suspended {
                        "<VersioningConfiguration><Status>Suspended</Status></VersioningConfiguration>"
                    } else {
                        "<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>"
                    },
                );
                if !suspended {
                    respond(
                        request(&server, "GET", "versions"),
                        r#"<ListVersionsResult><Version><Key>file</Key><VersionId>concurrent</VersionId><IsLatest>true</IsLatest></Version></ListVersionsResult>"#,
                    );
                }
                no_more(&server);
            });
            assert!(
                restore_with_client(&client, "bucket", "file", "old", "current")
                    .await
                    .is_err()
            );
            thread.join().unwrap();
        }
    }

    #[tokio::test]
    async fn large_version_restore_copies_ranges_and_creates_a_new_current_version() {
        let (client, server) = server();
        let size = 5 * 1024 * 1024 * 1024u64 + 1;
        let thread = std::thread::spawn(move || {
            respond(
                request(&server, "GET", "versioning"),
                "<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>",
            );
            respond(
                request(&server, "GET", "versions"),
                r#"<ListVersionsResult><DeleteMarker><Key>file</Key><VersionId>deleted</VersionId><IsLatest>true</IsLatest></DeleteMarker></ListVersionsResult>"#,
            );
            request(&server, "HEAD", "versionId=old")
                .respond(
                    tiny_http::Response::new(
                        tiny_http::StatusCode(200),
                        vec![tiny_http::Header::from_bytes("ETag", "source").unwrap()],
                        std::io::empty(),
                        Some(size as usize),
                        None,
                    )
                    .with_chunked_threshold(usize::MAX),
                )
                .unwrap();
            respond(
                request(&server, "GET", "tagging"),
                "<Tagging><TagSet/></Tagging>",
            );
            respond(request(&server, "POST", "uploads"), "<InitiateMultipartUploadResult><UploadId>large-copy</UploadId></InitiateMultipartUploadResult>");
            let chunk = 512 * 1024 * 1024;
            for number in 0..size.div_ceil(chunk) {
                let part = request(&server, "PUT", &format!("partNumber={}", number + 1));
                assert_eq!(
                    header(&part, "x-amz-copy-source"),
                    "bucket/file?versionId=old"
                );
                assert_eq!(
                    header(&part, "x-amz-copy-source-range"),
                    format!(
                        "bytes={}-{}",
                        number * chunk,
                        ((number + 1) * chunk).min(size) - 1
                    )
                );
                respond(part, "<CopyPartResult><ETag>part</ETag></CopyPartResult>");
            }
            let complete = request(&server, "POST", "uploadId=large-copy");
            assert_eq!(header(&complete, "if-none-match"), "*");
            respond(
                complete,
                "<CompleteMultipartUploadResult><ETag>new</ETag></CompleteMultipartUploadResult>",
            );
            no_more(&server);
        });
        restore_with_client(&client, "bucket", "file", "old", "deleted")
            .await
            .unwrap();
        thread.join().unwrap();
    }

    #[tokio::test]
    async fn multipart_restore_preserves_metadata_and_aborts_on_conditional_conflict() {
        let (client, server) = server();
        let thread = std::thread::spawn(move || {
            respond(request(&server,"GET","tagging"), "<Tagging><TagSet><Tag><Key>team</Key><Value>design</Value></Tag></TagSet></Tagging>");
            let create = request(&server, "POST", "uploads");
            assert_eq!(header(&create, "x-amz-meta-owner"), "design");
            assert_eq!(header(&create, "content-type"), "text/plain");
            assert_eq!(header(&create, "x-amz-tagging"), "team=design");
            respond(create,"<InitiateMultipartUploadResult><UploadId>copy-upload</UploadId></InitiateMultipartUploadResult>");
            let part = request(&server, "PUT", "partNumber=1");
            assert_eq!(header(&part, "x-amz-copy-source-range"), "bytes=0-11");
            assert_eq!(header(&part, "x-amz-copy-source-if-match"), "source");
            respond(
                part,
                "<CopyPartResult><ETag>part-etag</ETag></CopyPartResult>",
            );
            let complete = request(&server, "POST", "uploadId=copy-upload");
            assert_eq!(header(&complete, "if-none-match"), "*");
            complete
                .respond(
                    tiny_http::Response::from_string(
                        "<Error><Code>PreconditionFailed</Code></Error>",
                    )
                    .with_status_code(412),
                )
                .unwrap();
            respond(request(&server, "DELETE", "uploadId=copy-upload"), "");
            no_more(&server);
        });
        let source = aws_sdk_s3::operation::head_object::HeadObjectOutput::builder()
            .content_type("text/plain")
            .content_length(12)
            .e_tag("source")
            .metadata("owner", "design")
            .build();
        assert!(multipart_restore(
            &client,
            "bucket",
            "file",
            "old",
            "bucket/file?versionId=old",
            &source,
            None,
            12
        )
        .await
        .is_err());
        thread.join().unwrap();
    }
}
