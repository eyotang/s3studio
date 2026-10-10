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
use serde::Serialize;
use std::{collections::HashMap, time::Duration};
use tauri::State;

async fn client(store: &AccountStore, id: &str) -> Result<(Client, String), String> {
    let account = store
        .accounts
        .lock()
        .await
        .iter()
        .find(|a| a.id == id)
        .cloned()
        .ok_or("账户不存在。")?;
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

pub(crate) async fn bucket_client(
    store: &AccountStore,
    id: &str,
    bucket: &str,
) -> Result<Client, String> {
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

#[derive(Serialize)]
pub struct Bucket {
    name: String,
    created: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct S3Object {
    key: String,
    size: i64,
    modified: String,
    storage_class: String,
    folder: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectPage {
    objects: Vec<S3Object>,
    next_token: Option<String>,
}

#[tauri::command]
pub async fn list_buckets(
    store: State<'_, AccountStore>,
    account_id: String,
) -> Result<Vec<Bucket>, String> {
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

#[tauri::command]
pub async fn list_objects(
    store: State<'_, AccountStore>,
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

#[tauri::command]
pub async fn create_bucket(
    store: State<'_, AccountStore>,
    account_id: String,
    name: String,
) -> Result<(), String> {
    if !valid_bucket(&name) {
        return Err(
            "存储桶名称需为 3–63 个小写字母、数字、点或连字符，且符合 S3 命名规则。".into(),
        );
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

#[tauri::command]
pub async fn create_folder(
    store: State<'_, AccountStore>,
    account_id: String,
    bucket: String,
    key: String,
) -> Result<(), String> {
    if !key.ends_with('/') || key.len() < 2 || key.as_bytes().len() > 1024 {
        return Err("文件夹路径无效。".into());
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

#[tauri::command]
pub async fn delete_objects(
    store: State<'_, AccountStore>,
    account_id: String,
    bucket: String,
    keys: Vec<String>,
) -> Result<(), String> {
    if keys.is_empty() || keys.iter().any(|key| key.is_empty() || key.ends_with('/')) {
        return Err("请选择文件；文件夹暂不支持递归删除。".into());
    }
    let client = bucket_client(&store, &account_id, &bucket).await?;
    for chunk in keys.chunks(1000) {
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
            .bucket(&bucket)
            .delete(delete)
            .send()
            .await
            .map_err(failure)?;
        if !result.errors().is_empty() {
            return Err(format!(
                "部分对象删除失败，请刷新列表确认：{}",
                result
                    .errors()
                    .iter()
                    .map(|e| format!(
                        "{}: {}",
                        e.key().unwrap_or(""),
                        e.message().unwrap_or("未知错误")
                    ))
                    .collect::<Vec<_>>()
                    .join("；")
            ));
        }
    }
    Ok(())
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

#[tauri::command]
pub async fn copy_object(
    store: State<'_, AccountStore>,
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
        return Err("请输入不同且有效的目标对象键（最多 1024 字节）。".into());
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
        Ok(_) => return Err("目标对象已存在，请换一个对象键。".into()),
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectInfo {
    size: i64,
    content_type: String,
    etag: String,
    modified: String,
    metadata: HashMap<String, String>,
}

#[tauri::command]
pub async fn object_info(
    store: State<'_, AccountStore>,
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

#[tauri::command]
pub async fn presign_object(
    store: State<'_, AccountStore>,
    account_id: String,
    bucket: String,
    key: String,
    expires: u64,
) -> Result<String, String> {
    if !(60..=604800).contains(&expires) {
        return Err("有效期需为 60 秒到 7 天。".into());
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
