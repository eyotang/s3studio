use crate::accounts::{read_secret, AccountStore};
use aws_credential_types::Credentials;
use aws_sdk_s3::{
    config::{
        timeout::TimeoutConfig, BehaviorVersion, Region, RequestChecksumCalculation,
        ResponseChecksumValidation,
    },
    presigning::PresigningConfig,
    primitives::ByteStream,
    types::{BucketLocationConstraint, CreateBucketConfiguration, Delete, ObjectIdentifier},
    Client,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    time::Duration,
};

async fn client(store: &AccountStore, id: &str) -> Result<(Client, String), String> {
    let account = store
        .accounts
        .lock()
        .await
        .iter()
        .find(|a| a.id == id)
        .cloned()
        .ok_or(crate::i18n::t("账户不存在。"))?;
    let secret = read_secret(id.to_owned()).await?;
    let credentials = Credentials::new(
        secret.access_key,
        secret.secret_key,
        if secret.session_token.is_empty() {
            None
        } else {
            Some(secret.session_token)
        },
        None,
        "S3Studio",
    );
    let mut config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new(account.region.clone()))
        .credentials_provider(credentials)
        .force_path_style(account.path_style)
        .timeout_config(
            TimeoutConfig::builder()
                .connect_timeout(Duration::from_secs(10))
                .read_timeout(Duration::from_secs(30))
                .build(),
        )
        .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
        .response_checksum_validation(ResponseChecksumValidation::WhenRequired);
    if !account.endpoint.is_empty() {
        config = config.endpoint_url(account.endpoint);
    }
    Ok((Client::from_conf(config.build()), account.region))
}

pub async fn bucket_client(store: &AccountStore, id: &str, bucket: &str) -> Result<Client, String> {
    let (client, default_region) = client(store, id).await?;
    if store
        .accounts
        .lock()
        .await
        .iter()
        .any(|account| account.id == id && !account.endpoint.is_empty())
    {
        return Ok(client);
    }
    let cache_key = (id.to_owned(), bucket.to_owned());
    let cached = store.bucket_regions.lock().await.get(&cache_key).cloned();
    let region = if let Some(region) = cached {
        region
    } else {
        // AWS returns x-amz-bucket-region even on many permission / wrong-region errors.
        let region = match client.head_bucket().bucket(bucket).send().await {
            Ok(response) => response.bucket_region().map(str::to_owned),
            Err(error) => error
                .raw_response()
                .and_then(|response| response.headers().get("x-amz-bucket-region"))
                .map(str::to_owned),
        }
        .unwrap_or(default_region.clone());
        store
            .bucket_regions
            .lock()
            .await
            .insert(cache_key, region.clone());
        region
    };
    if region == default_region {
        Ok(client)
    } else {
        Ok(Client::from_conf(
            client
                .config()
                .to_builder()
                .region(Region::new(region))
                .build(),
        ))
    }
}

// The display chain includes service error codes (AccessDenied, wrong region, etc.)
// without exposing the SDK request, credentials or signed headers.
fn failure(error: impl std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(e) = source {
        let value = e.to_string();
        if !message.contains(&value) {
            message.push_str(": ");
            message.push_str(&value);
        }
        source = e.source();
    }
    message
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Bucket {
    pub name: String,
    pub created: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct S3Object {
    pub key: String,
    pub size: i64,
    pub modified: String,
    pub storage_class: String,
    pub folder: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectPage {
    pub objects: Vec<S3Object>,
    pub next_token: Option<String>,
}

pub async fn list_buckets(store: &AccountStore, account_id: String) -> Result<Vec<Bucket>, String> {
    let (client, _) = client(&store, &account_id).await?;
    let mut token = None;
    let mut buckets = Vec::new();
    loop {
        let response = client
            .list_buckets()
            .set_continuation_token(token)
            .send()
            .await
            .map_err(failure)?;
        for bucket in response.buckets() {
            if let Some(name) = bucket.name() {
                buckets.push(Bucket {
                    name: name.into(),
                    created: bucket
                        .creation_date()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                });
            }
        }
        token = response.continuation_token().map(str::to_owned);
        if token.is_none() {
            break;
        }
    }
    buckets.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(buckets)
}

pub async fn list_objects(
    store: &AccountStore,
    account_id: String,
    bucket: String,
    prefix: String,
    token: Option<String>,
) -> Result<ObjectPage, String> {
    let client = bucket_client(&store, &account_id, &bucket).await?;
    fetch_objects(&client, &bucket, &prefix, token).await
}

async fn fetch_objects(
    client: &Client,
    bucket: &str,
    prefix: &str,
    token: Option<String>,
) -> Result<ObjectPage, String> {
    let response = client
        .list_objects_v2()
        .bucket(bucket)
        .prefix(prefix)
        .delimiter("/")
        .max_keys(1000)
        .set_continuation_token(token)
        .send()
        .await
        .map_err(failure)?;
    let mut objects = Vec::new();
    for item in response.common_prefixes() {
        if let Some(key) = item.prefix() {
            objects.push(S3Object {
                key: key.into(),
                size: 0,
                modified: "".into(),
                storage_class: "".into(),
                folder: true,
            });
        }
    }
    for item in response.contents() {
        if let Some(key) = item.key() {
            if key != prefix {
                objects.push(S3Object {
                    key: key.into(),
                    size: item.size().unwrap_or(0),
                    modified: item
                        .last_modified()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                    storage_class: item
                        .storage_class()
                        .map(|s| s.as_str().into())
                        .unwrap_or_default(),
                    folder: key.ends_with('/'),
                });
            }
        }
    }
    Ok(ObjectPage {
        objects,
        next_token: response.next_continuation_token().map(str::to_owned),
    })
}

fn valid_bucket(name: &str) -> bool {
    (3..=63).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
        && name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && name
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && !name.contains("..")
        && !name.contains(".-")
        && !name.contains("-.")
        && name.parse::<std::net::Ipv4Addr>().is_err()
        && !["xn--", "sthree-", "amzn-s3-demo-"]
            .iter()
            .any(|p| name.starts_with(p))
        && !["-s3alias", "--ol-s3", ".mrap", "--x-s3", "--table-s3"]
            .iter()
            .any(|s| name.ends_with(s))
}

pub async fn create_bucket(
    store: &AccountStore,
    account_id: String,
    name: String,
) -> Result<(), String> {
    if !valid_bucket(&name) {
        return Err(crate::i18n::t(
            "存储桶名称需为 3–63 个小写字母、数字、点或连字符，且符合 S3 命名规则。",
        )
        .into());
    }
    let (client, region) = client(&store, &account_id).await?;
    let mut request = client.create_bucket().bucket(name);
    if region != "us-east-1" {
        request = request.create_bucket_configuration(
            CreateBucketConfiguration::builder()
                .location_constraint(BucketLocationConstraint::from(region.as_str()))
                .build(),
        );
    }
    request.send().await.map_err(failure)?;
    Ok(())
}

pub async fn create_folder(
    store: &AccountStore,
    account_id: String,
    bucket: String,
    key: String,
) -> Result<(), String> {
    if !key.ends_with('/') || key.len() < 2 || key.as_bytes().len() > 1024 {
        return Err(crate::i18n::t("文件夹路径无效。").into());
    }
    let client = bucket_client(&store, &account_id, &bucket).await?;
    client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(ByteStream::from_static(b""))
        .send()
        .await
        .map_err(failure)?;
    Ok(())
}

pub async fn delete_objects(
    store: &AccountStore,
    account_id: String,
    bucket: String,
    keys: Vec<String>,
    folders: Vec<String>,
) -> Result<usize, String> {
    let client = bucket_client(&store, &account_id, &bucket).await?;
    delete_selection(&client, &bucket, keys, folders).await
}

async fn delete_selection(
    client: &Client,
    bucket: &str,
    keys: Vec<String>,
    folders: Vec<String>,
) -> Result<usize, String> {
    if (keys.is_empty() && folders.is_empty())
        || keys
            .iter()
            .any(|key| key.is_empty() || key.ends_with('/') || key.len() > 1024)
        || folders
            .iter()
            .any(|prefix| prefix.len() < 2 || !prefix.ends_with('/') || prefix.len() > 1024)
    {
        return Err(crate::i18n::t("请选择有效文件或目录，不能删除存储桶根目录。").into());
    }
    let mut roots = Vec::<String>::new();
    for folder in folders.into_iter().collect::<BTreeSet<_>>() {
        if !roots.iter().any(|root| folder.starts_with(root)) {
            roots.push(folder);
        }
    }
    let mut selected = keys.into_iter().collect::<BTreeSet<_>>();
    // Enumerate the entire snapshot before deleting: a listing failure cannot
    // leave a partially deleted directory, and concurrent new uploads are excluded.
    for root in &roots {
        let mut token = None;
        let mut seen = BTreeSet::new();
        loop {
            let page = client
                .list_objects_v2()
                .bucket(bucket)
                .prefix(root)
                .max_keys(1000)
                .set_continuation_token(token)
                .send()
                .await
                .map_err(failure)?;
            for object in page.contents() {
                let key = object
                    .key()
                    .ok_or(crate::i18n::t("服务端列出的对象缺少键名。"))?;
                if !key.starts_with(root) {
                    return Err(
                        crate::i18n::t("服务端返回了所选目录以外的对象，已停止删除。").into(),
                    );
                }
                selected.insert(key.to_owned());
            }
            if !page.is_truncated().unwrap_or(false) {
                break;
            }
            let next = page
                .next_continuation_token()
                .ok_or(crate::i18n::t("服务端未返回目录分页标记，已停止删除。"))?
                .to_owned();
            if next.is_empty() || !seen.insert(next.clone()) {
                return Err(crate::i18n::t("服务端目录分页标记重复，已停止删除。").into());
            }
            token = Some(next);
        }
    }
    let (markers, files): (Vec<_>, Vec<_>) =
        selected.into_iter().partition(|key| key.ends_with('/'));
    let mut deleted = 0;
    // Folder markers are removed only after every file batch succeeds.
    for chunk in files.chunks(1000).chain(markers.chunks(1000)) {
        let identifiers = chunk
            .iter()
            .map(|key| {
                ObjectIdentifier::builder()
                    .key(key)
                    .build()
                    .map_err(failure)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let delete = Delete::builder()
            .set_objects(Some(identifiers))
            .quiet(true)
            .build()
            .map_err(failure)?;
        let result = client
            .delete_objects()
            .bucket(bucket)
            .delete(delete)
            .send()
            .await
            .map_err(|error| {
                crate::lf!(
                    "已完成 {} 个对象的删除，本批请求失败，请刷新列表确认：{}",
                    deleted,
                    failure(error)
                )
            })?;
        if !result.errors().is_empty() {
            return Err(crate::lf!(
                "已删除至少 {} 个对象，部分对象删除失败，请刷新列表确认：{}",
                deleted + chunk.len().saturating_sub(result.errors().len()),
                result
                    .errors()
                    .iter()
                    .take(10)
                    .map(|e| format!(
                        "{}: {}",
                        e.key().unwrap_or(""),
                        e.message().unwrap_or(crate::i18n::t("未知错误"))
                    ))
                    .collect::<Vec<_>>()
                    .join("；")
            ));
        }
        deleted += chunk.len();
    }
    Ok(deleted)
}

fn copy_source(bucket: &str, key: &str) -> String {
    // S3 requires URI-encoded CopySource, including spaces, Unicode, ? and #.
    format!(
        "{}/{}",
        bucket,
        key.split('/')
            .map(|part| percent_encoding::utf8_percent_encode(
                part,
                percent_encoding::NON_ALPHANUMERIC
            )
            .to_string())
            .collect::<Vec<_>>()
            .join("/")
    )
}

pub async fn copy_object(
    store: &AccountStore,
    account_id: String,
    bucket: String,
    key: String,
    destination: String,
) -> Result<(), String> {
    if destination.is_empty()
        || destination.len() > 1024
        || destination == key
        || destination.ends_with('/')
    {
        return Err(crate::i18n::t("请输入不同且有效的目标对象键（最多 1024 字节）。").into());
    }
    let client = bucket_client(&store, &account_id, &bucket).await?;
    // Refuse to overwrite an existing destination, including permission errors.
    match client
        .head_object()
        .bucket(&bucket)
        .key(&destination)
        .send()
        .await
    {
        Ok(_) => return Err(crate::i18n::t("目标对象已存在，请换一个对象键。").into()),
        Err(e) if e.raw_response().is_some_and(|r| r.status().as_u16() == 404) => {}
        Err(e) => return Err(failure(e)),
    }
    client
        .copy_object()
        .bucket(&bucket)
        .key(destination)
        .copy_source(copy_source(&bucket, &key))
        .send()
        .await
        .map_err(failure)?;
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectInfo {
    pub size: i64,
    pub content_type: String,
    pub etag: String,
    pub modified: String,
    pub metadata: HashMap<String, String>,
}

pub async fn object_info(
    store: &AccountStore,
    account_id: String,
    bucket: String,
    key: String,
) -> Result<ObjectInfo, String> {
    let client = bucket_client(&store, &account_id, &bucket).await?;
    let result = client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(failure)?;
    Ok(ObjectInfo {
        size: result.content_length().unwrap_or(0),
        content_type: result
            .content_type()
            .unwrap_or("application/octet-stream")
            .into(),
        etag: result.e_tag().unwrap_or("").into(),
        modified: result
            .last_modified()
            .map(ToString::to_string)
            .unwrap_or_default(),
        metadata: result.metadata().cloned().unwrap_or_default(),
    })
}

pub async fn presign_object(
    store: &AccountStore,
    account_id: String,
    bucket: String,
    key: String,
    expires: u64,
) -> Result<String, String> {
    if !(60..=604800).contains(&expires) {
        return Err(crate::i18n::t("有效期需为 60 秒到 7 天。").into());
    }
    let client = bucket_client(&store, &account_id, &bucket).await?;
    let request = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .presigned(PresigningConfig::expires_in(Duration::from_secs(expires)).map_err(failure)?)
        .await
        .map_err(failure)?;
    Ok(request.uri().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfers::part_size;
    fn test_client(endpoint: String) -> Client {
        Client::from_conf(
            aws_sdk_s3::Config::builder()
                .behavior_version(BehaviorVersion::latest())
                .region(Region::new("us-east-1"))
                .credentials_provider(Credentials::new(
                    "test-access",
                    "test-secret",
                    None,
                    None,
                    "test",
                ))
                .force_path_style(true)
                .endpoint_url(endpoint)
                .build(),
        )
    }
    #[tokio::test]
    async fn signed_listing_parses_folders_and_passes_pagination_token() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", server.server_addr());
        let thread = std::thread::spawn(move || {
            let first = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            assert!(first.url().contains("list-type=2"));
            assert!(first.url().contains("prefix=design%2F"));
            assert!(first
                .headers()
                .iter()
                .any(|h| h.field.equiv("Authorization")
                    && h.value.as_str().starts_with("AWS4-HMAC-SHA256")));
            first.respond(tiny_http::Response::from_string(r#"<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><IsTruncated>true</IsTruncated><NextContinuationToken>page-two</NextContinuationToken><CommonPrefixes><Prefix>design/images/</Prefix></CommonPrefixes><Contents><Key>design/</Key><Size>0</Size></Contents><Contents><Key>design/a.json</Key><Size>42</Size><LastModified>2026-10-09T08:00:00Z</LastModified><StorageClass>STANDARD</StorageClass></Contents></ListBucketResult>"#)).unwrap();
            let second = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            assert!(second.url().contains("continuation-token=page-two"));
            second.respond(tiny_http::Response::from_string(r#"<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><IsTruncated>false</IsTruncated><Contents><Key>design/b.json</Key><Size>12</Size></Contents></ListBucketResult>"#)).unwrap();
        });
        let client = test_client(endpoint);
        let first = fetch_objects(&client, "test-bucket", "design/", None)
            .await
            .unwrap();
        assert_eq!(first.objects.len(), 2);
        assert!(first.objects[0].folder);
        assert_eq!(first.objects[1].size, 42);
        let second = fetch_objects(&client, "test-bucket", "design/", first.next_token)
            .await
            .unwrap();
        assert_eq!(second.objects[0].key, "design/b.json");
        assert!(second.next_token.is_none());
        thread.join().unwrap();
    }
    #[tokio::test]
    async fn presigned_links_contain_signature_expiration_and_encoded_key() {
        let client = test_client("https://s3.example.com".into());
        let request = client
            .get_object()
            .bucket("test-bucket")
            .key("设计/a b.png")
            .presigned(PresigningConfig::expires_in(Duration::from_secs(3600)).unwrap())
            .await
            .unwrap();
        let url = request.uri();
        assert!(url.contains("X-Amz-Signature="));
        assert!(url.contains("X-Amz-Expires=3600"));
        assert!(url.contains("a%20b.png"));
    }

    #[tokio::test]
    async fn recursive_delete_lists_all_pages_deduplicates_and_deletes_markers_last() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let client = test_client(format!("http://{}", server.server_addr()));
        let thread = std::thread::spawn(move || {
            let first = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            assert!(first.url().contains("prefix=photos%2F"));
            assert!(!first.url().contains("delimiter"));
            let contents = (0..1000)
                .map(|i| {
                    format!("<Contents><Key>photos/sub/{i:04}.jpg</Key><Size>1</Size></Contents>")
                })
                .collect::<String>();
            first.respond(tiny_http::Response::from_string(format!("<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>page2</NextContinuationToken>{contents}</ListBucketResult>"))).unwrap();
            let second = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            assert!(second.url().contains("continuation-token=page2"));
            second.respond(tiny_http::Response::from_string("<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>photos/extra.jpg</Key></Contents><Contents><Key>photos/</Key></Contents><Contents><Key>photos/sub/</Key></Contents></ListBucketResult>")).unwrap();
            let mut deleted = BTreeSet::new();
            for expected in [1000, 2, 2] {
                let mut request = server
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap()
                    .unwrap();
                assert_eq!(request.method(), &tiny_http::Method::Post);
                assert!(request.url().contains("delete"));
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).unwrap();
                let keys = body
                    .split("<Key>")
                    .skip(1)
                    .map(|s| s.split("</Key>").next().unwrap())
                    .collect::<Vec<_>>();
                assert_eq!(keys.len(), expected);
                let markers = deleted.len() == 1002;
                for key in keys {
                    assert_eq!(key.ends_with('/'), markers);
                    assert!(deleted.insert(key.to_owned()));
                }
                request
                    .respond(tiny_http::Response::from_string("<DeleteResult/>"))
                    .unwrap();
            }
            assert!(deleted.contains("outside.txt"));
            assert_eq!(deleted.len(), 1004);
        });
        let count = delete_selection(
            &client,
            "bucket",
            vec!["photos/sub/0000.jpg".into(), "outside.txt".into()],
            vec!["photos/".into(), "photos/sub/".into(), "photos/".into()],
        )
        .await
        .unwrap();
        assert_eq!(count, 1004);
        thread.join().unwrap();
    }

    #[tokio::test]
    async fn recursive_delete_rejects_root_and_outside_keys_before_deleting() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let client = test_client(format!("http://{}", server.server_addr()));
        for root in ["", "/", "photos"] {
            assert!(
                delete_selection(&client, "bucket", vec![], vec![root.into()])
                    .await
                    .is_err()
            );
        }
        let thread = std::thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            request.respond(tiny_http::Response::from_string("<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>photos-other/a.jpg</Key></Contents></ListBucketResult>")).unwrap();
            assert!(server
                .recv_timeout(Duration::from_millis(100))
                .unwrap()
                .is_none());
        });
        assert!(
            delete_selection(&client, "bucket", vec![], vec!["photos/".into()])
                .await
                .unwrap_err()
                .contains("目录以外")
        );
        thread.join().unwrap();
    }

    #[tokio::test]
    async fn recursive_delete_reports_partial_failure_and_keeps_directory_markers() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let client = test_client(format!("http://{}", server.server_addr()));
        let thread = std::thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            request.respond(tiny_http::Response::from_string("<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>photos/a.jpg</Key></Contents><Contents><Key>photos/</Key></Contents></ListBucketResult>")).unwrap();
            let mut request = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            let mut body = String::new();
            request.as_reader().read_to_string(&mut body).unwrap();
            assert!(!body.contains("<Key>photos/</Key>"));
            request.respond(tiny_http::Response::from_string("<DeleteResult><Error><Key>photos/a.jpg</Key><Code>AccessDenied</Code><Message>denied</Message></Error></DeleteResult>")).unwrap();
            assert!(server
                .recv_timeout(Duration::from_millis(100))
                .unwrap()
                .is_none());
        });
        let error = delete_selection(&client, "bucket", vec![], vec!["photos/".into()])
            .await
            .unwrap_err();
        assert!(error.contains("部分对象删除失败"));
        assert!(error.contains("photos/a.jpg"));
        thread.join().unwrap();
    }
    #[test]
    fn bucket_validation_matches_s3_rules() {
        for name in ["test-bucket", "my.assets-123"] {
            assert!(valid_bucket(name));
        }
        for name in [
            "ab",
            "Invalid",
            "-bucket",
            "bucket-",
            "a..b",
            "127.0.0.1",
            "xn--test",
            "test--x-s3",
        ] {
            assert!(!valid_bucket(name), "{name}");
        }
    }
    #[test]
    fn copy_source_encodes_unicode_spaces_and_reserved_characters() {
        let source = copy_source("bucket", "设计/a b?#.png");
        assert_eq!(source, "bucket/%E8%AE%BE%E8%AE%A1/a%20b%3F%23%2Epng");
    }
    #[test]
    fn multipart_respects_part_count_and_minimum_part_size() {
        for size in [17 * 1024 * 1024, 500 * 1024 * 1024, 5 * 1024_u64.pow(4)] {
            assert!(part_size(size) >= 5 * 1024 * 1024);
            assert!(size.div_ceil(part_size(size)) <= 10_000);
        }
    }
}
