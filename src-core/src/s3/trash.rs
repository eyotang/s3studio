use super::*;
use std::collections::HashSet;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashObject {
    pub key: String,
    pub version_id: String,
    pub deleted: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor {
    pub key: String,
    pub version: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashPage {
    pub objects: Vec<TrashObject>,
    pub cursor: Option<Cursor>,
    pub versioning: bool,
}

#[derive(Serialize, Deserialize, Default)]
pub struct RestoreResult {
    pub restored: Vec<String>,
    pub failed: Vec<(String, String)>,
}

pub async fn list(
    store: &AccountStore,
    account: String,
    bucket: String,
    cursor: Option<Cursor>,
) -> Result<TrashPage, String> {
    let client = bucket_client(store, &account, &bucket).await?;
    list_with_client(&client, &bucket, cursor).await
}

async fn list_with_client(
    client: &Client,
    bucket: &str,
    cursor: Option<Cursor>,
) -> Result<TrashPage, String> {
    if cursor.is_none() {
        let status = client
            .get_bucket_versioning()
            .bucket(bucket)
            .send()
            .await
            .map_err(failure)?;
        if status.status().is_none() {
            return Ok(TrashPage {
                objects: vec![],
                cursor: None,
                versioning: false,
            });
        }
    }
    list_page(client, bucket, cursor).await
}

async fn list_page(
    client: &Client,
    bucket: &str,
    cursor: Option<Cursor>,
) -> Result<TrashPage, String> {
    let response = client
        .list_object_versions()
        .bucket(bucket)
        .max_keys(200)
        .set_key_marker(cursor.as_ref().map(|c| c.key.clone()))
        .set_version_id_marker(cursor.as_ref().and_then(|c| c.version.clone()))
        .send()
        .await
        .map_err(failure)?;
    let next = next_cursor(&response)?;
    if next.is_some() && next == cursor {
        return Err(crate::i18n::t("服务器返回了重复的分页标记。").into());
    }
    let objects = response
        .delete_markers()
        .iter()
        .filter(|marker| marker.is_latest() == Some(true))
        .filter_map(|marker| {
            Some(TrashObject {
                key: marker.key()?.into(),
                version_id: marker.version_id()?.into(),
                deleted: marker
                    .last_modified()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            })
        })
        .collect();
    Ok(TrashPage {
        objects,
        cursor: next,
        versioning: true,
    })
}

pub(super) fn next_cursor(
    response: &aws_sdk_s3::operation::list_object_versions::ListObjectVersionsOutput,
) -> Result<Option<Cursor>, String> {
    if response.is_truncated() != Some(true) {
        return Ok(None);
    }
    let key = response
        .next_key_marker()
        .filter(|key| !key.is_empty())
        .ok_or(crate::i18n::t("服务端未返回有效的分页标记。"))?;
    Ok(Some(Cursor {
        key: key.into(),
        version: response.next_version_id_marker().map(str::to_owned),
    }))
}

pub async fn restore(
    store: &AccountStore,
    account: String,
    bucket: String,
    objects: Vec<TrashObject>,
) -> Result<RestoreResult, String> {
    let client = bucket_client(store, &account, &bucket).await?;
    let mut result = RestoreResult::default();
    let mut seen = HashSet::new();
    for object in objects {
        if !seen.insert(object.key.clone()) {
            continue;
        }
        match restore_one(&client, &bucket, &object).await {
            Ok(()) => result.restored.push(object.key),
            Err(error) => result.failed.push((object.key, error)),
        }
    }
    Ok(result)
}

async fn restore_one(client: &Client, bucket: &str, object: &TrashObject) -> Result<(), String> {
    let changed = crate::i18n::t("对象状态已变化，请刷新回收站后重试。");
    let missing = crate::i18n::t("历史数据版本已不存在，无法还原此对象。");
    if object.key.is_empty() || object.version_id.is_empty() {
        return Err(changed.into());
    }
    // Establish that data still exists before deleting any markers. Retain only
    // observed marker IDs, so a concurrent deletion cannot be undone by this request.
    let mut cursor: Option<Cursor> = None;
    let mut cursors = HashSet::new();
    let mut markers = HashSet::new();
    loop {
        let response = client
            .list_object_versions()
            .bucket(bucket)
            .prefix(&object.key)
            .max_keys(200)
            .set_key_marker(cursor.as_ref().map(|c| c.key.clone()))
            .set_version_id_marker(cursor.as_ref().and_then(|c| c.version.clone()))
            .send()
            .await
            .map_err(failure)?;
        if cursor.is_none()
            && !response.delete_markers().iter().any(|m| {
                m.key() == Some(object.key.as_str())
                    && m.is_latest() == Some(true)
                    && m.version_id() == Some(object.version_id.as_str())
            })
        {
            return Err(changed.into());
        }
        markers.extend(
            response
                .delete_markers()
                .iter()
                .filter(|m| m.key() == Some(object.key.as_str()))
                .filter_map(|m| m.version_id().map(str::to_owned)),
        );
        if response
            .versions()
            .iter()
            .any(|v| v.key() == Some(object.key.as_str()))
        {
            break;
        }
        let next = next_cursor(&response)?;
        let Some(next) = next.filter(|c| c.key == object.key) else {
            return Err(missing.into());
        };
        if !cursors.insert((next.key.clone(), next.version.clone())) {
            return Err(changed.into());
        }
        cursor = Some(next);
    }
    let mut marker = object.version_id.clone();
    loop {
        if !markers.remove(&marker) {
            return Err(changed.into());
        }
        // Always specify a known delete-marker version. Never delete object data,
        // and never issue a plain DELETE which would create a new marker.
        client
            .delete_object()
            .bucket(bucket)
            .key(&object.key)
            .version_id(&marker)
            .send()
            .await
            .map_err(failure)?;
        let current = client
            .list_object_versions()
            .bucket(bucket)
            .prefix(&object.key)
            .max_keys(1)
            .send()
            .await
            .map_err(failure)?;
        if current
            .versions()
            .iter()
            .any(|v| v.key() == Some(object.key.as_str()) && v.is_latest() == Some(true))
        {
            return Ok(());
        }
        marker = current
            .delete_markers()
            .iter()
            .find(|m| m.key() == Some(object.key.as_str()) && m.is_latest() == Some(true))
            .and_then(|m| m.version_id())
            .ok_or(changed)?
            .into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock(
        steps: Vec<(&'static str, &'static str, &'static str)>,
    ) -> (Client, std::thread::JoinHandle<()>) {
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
        let thread = std::thread::spawn(move || {
            for (method, path, response) in steps {
                let request = server
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
                    .expect("expected S3 request");
                assert_eq!(request.method().as_str(), method);
                if method == "DELETE" {
                    assert!(request.url().starts_with("/bucket/folder/a.txt?"));
                }
                assert!(
                    request.url().contains(path),
                    "{} does not contain {path}",
                    request.url()
                );
                request
                    .respond(tiny_http::Response::from_string(response))
                    .unwrap();
            }
            assert!(
                server
                    .recv_timeout(Duration::from_millis(100))
                    .unwrap()
                    .is_none(),
                "unexpected extra S3 request"
            );
        });
        (client, thread)
    }

    fn item() -> TrashObject {
        TrashObject {
            key: "folder/a.txt".into(),
            version_id: "m2".into(),
            deleted: String::new(),
        }
    }

    #[tokio::test]
    async fn unversioned_bucket_reports_unsupported_without_listing_or_mutating() {
        let (client, server) = mock(vec![("GET", "versioning", "<VersioningConfiguration/>")]);
        let page = list_with_client(&client, "bucket", None).await.unwrap();
        assert!(!page.versioning);
        assert!(page.objects.is_empty());
        server.join().unwrap();
    }

    #[tokio::test]
    async fn lists_only_current_delete_markers_and_preserves_both_pagination_markers() {
        let (client, server) = mock(vec![
            (
                "GET",
                "versions",
                r#"<ListVersionsResult><IsTruncated>true</IsTruncated><NextKeyMarker>folder/a.txt</NextKeyMarker><NextVersionIdMarker>m2</NextVersionIdMarker><DeleteMarker><Key>folder/a.txt</Key><VersionId>m2</VersionId><IsLatest>true</IsLatest></DeleteMarker><DeleteMarker><Key>live.txt</Key><VersionId>old</VersionId><IsLatest>false</IsLatest></DeleteMarker></ListVersionsResult>"#,
            ),
            (
                "GET",
                "version-id-marker=m2",
                r#"<ListVersionsResult><IsTruncated>false</IsTruncated></ListVersionsResult>"#,
            ),
        ]);
        let first = list_page(&client, "bucket", None).await.unwrap();
        assert_eq!(first.objects.len(), 1);
        assert_eq!(first.objects[0].version_id, "m2");
        assert_eq!(first.cursor.as_ref().unwrap().key, "folder/a.txt");
        let second = list_page(&client, "bucket", first.cursor).await.unwrap();
        assert!(second.cursor.is_none());
        server.join().unwrap();
    }

    #[tokio::test]
    async fn restore_handles_consecutive_markers_and_null_version_without_deleting_data() {
        let (client, server) = mock(vec![
            (
                "GET",
                "versions",
                r#"<ListVersionsResult><IsTruncated>true</IsTruncated><NextKeyMarker>folder/a.txt</NextKeyMarker><NextVersionIdMarker>m2</NextVersionIdMarker><DeleteMarker><Key>folder/a.txt</Key><VersionId>m2</VersionId><IsLatest>true</IsLatest></DeleteMarker></ListVersionsResult>"#,
            ),
            (
                "GET",
                "version-id-marker=m2",
                r#"<ListVersionsResult><IsTruncated>false</IsTruncated><DeleteMarker><Key>folder/a.txt</Key><VersionId>null</VersionId><IsLatest>false</IsLatest></DeleteMarker><Version><Key>folder/a.txt</Key><VersionId>data</VersionId><IsLatest>false</IsLatest><Size>9000000000</Size></Version></ListVersionsResult>"#,
            ),
            ("DELETE", "versionId=m2", ""),
            (
                "GET",
                "max-keys=1",
                r#"<ListVersionsResult><DeleteMarker><Key>folder/a.txt</Key><VersionId>null</VersionId><IsLatest>true</IsLatest></DeleteMarker></ListVersionsResult>"#,
            ),
            ("DELETE", "versionId=null", ""),
            (
                "GET",
                "max-keys=1",
                r#"<ListVersionsResult><Version><Key>folder/a.txt</Key><VersionId>data</VersionId><IsLatest>true</IsLatest></Version></ListVersionsResult>"#,
            ),
        ]);
        restore_one(&client, "bucket", &item()).await.unwrap();
        server.join().unwrap();
    }

    #[tokio::test]
    async fn restore_refuses_missing_data_and_changed_current_versions() {
        for response in [
            r#"<ListVersionsResult><DeleteMarker><Key>folder/a.txt</Key><VersionId>m2</VersionId><IsLatest>true</IsLatest></DeleteMarker></ListVersionsResult>"#,
            r#"<ListVersionsResult><Version><Key>folder/a.txt</Key><VersionId>new</VersionId><IsLatest>true</IsLatest></Version></ListVersionsResult>"#,
        ] {
            let (client, server) = mock(vec![("GET", "versions", response)]);
            assert!(restore_one(&client, "bucket", &item()).await.is_err());
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn restore_does_not_remove_a_concurrent_delete_marker() {
        let (client, server) = mock(vec![
            (
                "GET",
                "versions",
                r#"<ListVersionsResult><DeleteMarker><Key>folder/a.txt</Key><VersionId>m2</VersionId><IsLatest>true</IsLatest></DeleteMarker><Version><Key>folder/a.txt</Key><VersionId>data</VersionId><IsLatest>false</IsLatest></Version></ListVersionsResult>"#,
            ),
            ("DELETE", "versionId=m2", ""),
            (
                "GET",
                "max-keys=1",
                r#"<ListVersionsResult><DeleteMarker><Key>folder/a.txt</Key><VersionId>concurrent</VersionId><IsLatest>true</IsLatest></DeleteMarker></ListVersionsResult>"#,
            ),
        ]);
        assert!(restore_one(&client, "bucket", &item()).await.is_err());
        server.join().unwrap();
    }
}
