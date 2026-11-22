use crate::{
    accounts::{get_s3_defaults, read_secret, Account, AccountStore, S3Connection, S3Defaults},
    transfers::Engine,
};
use aws_credential_types::Credentials;
use aws_sdk_s3::{
    config::{
        timeout::TimeoutConfig, BehaviorVersion, Region, RequestChecksumCalculation,
        ResponseChecksumValidation,
    },
    operation::{
        head_object::HeadObjectOutput, list_objects_v2::builders::ListObjectsV2FluentBuilder,
    },
    presigning::PresigningConfig,
    primitives::ByteStream,
    types::{
        BucketLocationConstraint, CreateBucketConfiguration, Delete, EncodingType, Object,
        ObjectIdentifier,
    },
    Client,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    time::Duration,
};

fn resolve_connection(
    account: &Account,
    defaults: &S3Defaults,
    bucket: &S3Connection,
) -> (S3Connection, bool) {
    let endpoint = if bucket.endpoint.is_empty() {
        defaults.endpoint_for(account)
    } else {
        &bucket.endpoint
    };
    let region = if bucket.region.is_empty() {
        defaults.region_for(account)
    } else {
        &bucket.region
    };
    let pinned = !endpoint.is_empty()
        || !bucket.region.is_empty()
        || (account.region.is_empty() && !defaults.region.is_empty());
    (
        S3Connection {
            endpoint: endpoint.into(),
            region: region.into(),
        },
        pinned,
    )
}

/// Local cache identity. A credential edit changes the account revision without
/// storing either credentials or a derivative of them in the object cache.
pub async fn directory_cache_namespace(
    store: &AccountStore,
    id: &str,
    bucket: &str,
) -> Result<String, String> {
    let account = store.account(id).await?;
    let defaults = get_s3_defaults(store).await;
    let empty = S3Connection::default();
    let (connection, pinned) = resolve_connection(
        &account,
        &defaults,
        account.bucket_connections.get(bucket).unwrap_or(&empty),
    );
    serde_json::to_string(&(
        account.cache_revision,
        connection,
        pinned,
        account.path_style,
    ))
    .map_err(failure)
}

async fn client(account: &Account, connection: &S3Connection) -> Result<Client, String> {
    let secret = read_secret(account.id.clone()).await?;
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
        .region(Region::new(connection.region.clone()))
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
    if !connection.endpoint.is_empty() {
        config = config.endpoint_url(&connection.endpoint);
    }
    Ok(Client::from_conf(config.build()))
}

pub async fn bucket_client(store: &AccountStore, id: &str, bucket: &str) -> Result<Client, String> {
    let account = store.account(id).await?;
    let defaults = get_s3_defaults(store).await;
    let empty = S3Connection::default();
    let (connection, use_configured_connection) = resolve_connection(
        &account,
        &defaults,
        account.bucket_connections.get(bucket).unwrap_or(&empty),
    );
    let client = client(&account, &connection).await?;
    if use_configured_connection {
        return Ok(client);
    }
    let default_region = connection.region;
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
    #[serde(flatten)]
    pub connection: S3Connection,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
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

pub const TREE_PAGE_SIZE: usize = 200;

pub async fn list_buckets(store: &AccountStore, account_id: String) -> Result<Vec<Bucket>, String> {
    let account = store.account(&account_id).await?;
    let defaults = get_s3_defaults(store).await;
    let (connection, _) = resolve_connection(&account, &defaults, &S3Connection::default());
    let client = client(&account, &connection).await?;
    let mut buckets = match list_remote_buckets(&client).await {
        Ok(buckets) => buckets,
        // A bucket can live on a different service than the account listing endpoint.
        Err(_) if !account.bucket_connections.is_empty() => Vec::new(),
        Err(error) => return Err(error),
    };
    let mut connections = account.bucket_connections;
    for bucket in &mut buckets {
        if let Some(connection) = connections.remove(&bucket.name) {
            bucket.connection = connection;
        }
    }
    buckets.extend(connections.into_iter().map(|(name, connection)| Bucket {
        name,
        created: String::new(),
        connection,
    }));
    buckets.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(buckets)
}

async fn list_remote_buckets(client: &Client) -> Result<Vec<Bucket>, String> {
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
                    connection: S3Connection::default(),
                });
            }
        }
        token = response.continuation_token().map(str::to_owned);
        if token.is_none() {
            break;
        }
    }
    Ok(buckets)
}

pub async fn list_objects(
    store: &AccountStore,
    account_id: String,
    bucket: String,
    prefix: String,
    token: Option<String>,
) -> Result<ObjectPage, String> {
    let client = bucket_client(store, &account_id, &bucket).await?;
    fetch_objects(&client, &bucket, &prefix, token).await
}

fn object_listing(
    client: &Client,
    bucket: &str,
    prefix: &str,
    token: Option<String>,
) -> ListObjectsV2FluentBuilder {
    client
        .list_objects_v2()
        .bucket(bucket)
        .prefix(prefix)
        .max_keys(1000)
        .set_continuation_token(token)
}

fn listed_object(item: &Object, key: String, folder: bool) -> S3Object {
    S3Object {
        key,
        size: item.size().unwrap_or(0),
        modified: item
            .last_modified()
            .map(ToString::to_string)
            .unwrap_or_default(),
        storage_class: item
            .storage_class()
            .map(|s| s.as_str().into())
            .unwrap_or_default(),
        folder,
    }
}

fn directory_entry(key: String) -> S3Object {
    S3Object {
        key,
        size: 0,
        modified: String::new(),
        storage_class: String::new(),
        folder: true,
    }
}

async fn fetch_objects(
    client: &Client,
    bucket: &str,
    prefix: &str,
    token: Option<String>,
) -> Result<ObjectPage, String> {
    let response = object_listing(client, bucket, prefix, token)
        .delimiter("/")
        .send()
        .await
        .map_err(failure)?;
    let mut objects = Vec::new();
    for item in response.common_prefixes() {
        if let Some(key) = item.prefix() {
            objects.push(directory_entry(key.into()));
        }
    }
    for item in response.contents() {
        if let Some(key) = item.key() {
            if key != prefix {
                objects.push(listed_object(item, key.into(), key.ends_with('/')));
            }
        }
    }
    Ok(ObjectPage {
        objects,
        next_token: response.next_continuation_token().map(str::to_owned),
    })
}

pub async fn list_tree_level(
    store: &AccountStore,
    account_id: String,
    bucket: String,
    prefix: String,
    token: Option<String>,
) -> Result<ObjectPage, String> {
    let client = bucket_client(store, &account_id, &bucket).await?;
    fetch_tree_level(&client, &bucket, &prefix, token).await
}

async fn fetch_tree_level(
    client: &Client,
    bucket: &str,
    prefix: &str,
    token: Option<String>,
) -> Result<ObjectPage, String> {
    let page = object_listing(client, bucket, prefix, token)
        .max_keys(TREE_PAGE_SIZE as i32)
        .delimiter("/")
        .encoding_type(EncodingType::Url)
        .send()
        .await
        .map_err(failure)?;
    let encoded = page.encoding_type() == Some(&EncodingType::Url);
    let invalid_key = || crate::i18n::t("服务端返回了无效的对象键或大小。");
    let mut folders = BTreeSet::new();
    for item in page.common_prefixes() {
        let key = decode_listed_key(item.prefix().ok_or_else(invalid_key)?, encoded)?;
        let offset = key
            .strip_prefix(prefix)
            .and_then(|suffix| suffix.find('/'))
            .ok_or_else(invalid_key)?;
        folders.insert(key[..prefix.len() + offset + 1].to_owned());
    }
    let mut objects = Vec::new();
    let mut files = BTreeSet::new();
    for item in page.contents() {
        let object = scanned_object(item, prefix, encoded)?;
        if let Some(offset) = object.key[prefix.len()..].find('/') {
            folders.insert(object.key[..prefix.len() + offset + 1].to_owned());
        } else if !object.folder && files.insert(object.key.clone()) {
            // A nonempty object whose key equals this prefix is a file in it.
            objects.push(object);
        }
    }
    objects.extend(folders.into_iter().map(directory_entry));
    let next = page.next_continuation_token();
    let next_token = if page.is_truncated().unwrap_or(next.is_some()) {
        Some(
            next.filter(|token| !token.is_empty())
                .ok_or_else(|| crate::i18n::t("服务端未返回有效的分页标记。"))?
                .to_owned(),
        )
    } else {
        None
    };
    Ok(ObjectPage {
        objects,
        next_token,
    })
}

fn decode_listed_key(key: &str, encoded: bool) -> Result<String, String> {
    if encoded {
        Ok(percent_encoding::percent_decode_str(key)
            .decode_utf8()
            .map_err(|_| crate::i18n::t("服务端返回了无法解码的对象键。"))?
            .into_owned())
    } else {
        Ok(key.to_owned())
    }
}

fn scanned_object(item: &Object, prefix: &str, encoded: bool) -> Result<S3Object, String> {
    let key = item
        .key()
        .ok_or_else(|| crate::i18n::t("服务端列出的对象缺少键名。"))?;
    let key = decode_listed_key(key, encoded)?;
    if !key.starts_with(prefix) || key.is_empty() || item.size().is_none_or(|size| size < 0) {
        return Err(crate::i18n::t("服务端返回了无效的对象键或大小。").into());
    }
    let folder = key.ends_with('/') && item.size() == Some(0);
    Ok(listed_object(item, key, folder))
}

/// Enumerates current objects beneath a prefix using one flat, paginated listing.
pub async fn scan_objects(
    store: &AccountStore,
    account_id: &str,
    bucket: &str,
    prefix: &str,
    on_page: impl FnMut(Vec<S3Object>) -> Result<(), String>,
) -> Result<(), String> {
    let client = bucket_client(store, account_id, bucket).await?;
    scan_listing(&client, bucket, prefix, on_page).await
}

async fn scan_listing(
    client: &Client,
    bucket: &str,
    prefix: &str,
    mut on_page: impl FnMut(Vec<S3Object>) -> Result<(), String>,
) -> Result<(), String> {
    let mut token = None;
    let mut seen_tokens = BTreeSet::new();
    loop {
        let page = object_listing(client, bucket, prefix, token)
            .encoding_type(EncodingType::Url)
            .send()
            .await
            .map_err(failure)?;
        let encoded = page.encoding_type() == Some(&EncodingType::Url);
        let mut objects = Vec::with_capacity(page.contents().len());
        for item in page.contents() {
            objects.push(scanned_object(item, prefix, encoded)?);
        }
        let next = page.next_continuation_token();
        if !page.is_truncated().unwrap_or(next.is_some()) {
            on_page(objects)?;
            return Ok(());
        }
        let next = next
            .filter(|next| !next.is_empty() && seen_tokens.insert((*next).to_owned()))
            .ok_or_else(|| crate::i18n::t("服务端未返回有效的分页标记，无法完成大小统计。"))?;
        on_page(objects)?;
        token = Some(next.to_owned());
    }
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
    connection: S3Connection,
) -> Result<(), String> {
    if !valid_bucket(&name) {
        return Err(crate::i18n::t(
            "存储桶名称需为 3–63 个小写字母、数字、点或连字符，且符合 S3 命名规则。",
        )
        .into());
    }
    let connection = connection.normalized()?;
    let _change = store.connection_changes.lock().await;
    let account = store.account(&account_id).await?;
    if account.bucket_connections.contains_key(&name) {
        return Err(crate::i18n::t("此账户已有同名存储桶，请编辑现有存储桶的连接设置。").into());
    }
    let defaults = get_s3_defaults(store).await;
    let (resolved, _) = resolve_connection(&account, &defaults, &connection);
    let client = client(&account, &resolved).await?;
    create_bucket_at(&client, &name, &resolved.region).await?;
    if !connection.is_empty() {
        store
            .save_bucket_connection(&account_id, &name, connection)
            .await
            .map_err(|e| crate::lf!("存储桶已创建，但连接设置保存失败：{e}", e = e))?;
    }
    Ok(())
}

async fn create_bucket_at(client: &Client, name: &str, region: &str) -> Result<(), String> {
    let mut request = client.create_bucket().bucket(name);
    if region != "us-east-1" {
        request = request.create_bucket_configuration(
            CreateBucketConfiguration::builder()
                .location_constraint(BucketLocationConstraint::from(region))
                .build(),
        );
    }
    request.send().await.map_err(failure)?;
    Ok(())
}

pub async fn set_bucket_connection(
    store: &AccountStore,
    engine: &Engine,
    account_id: String,
    bucket: String,
    connection: S3Connection,
) -> Result<S3Connection, String> {
    if bucket.is_empty() {
        return Err(crate::i18n::t("请填写此字段。").into());
    }
    let connection = connection.normalized()?;
    let _change = store.connection_changes.lock().await;
    let account = store.account(&account_id).await?;
    if account
        .bucket_connections
        .get(&bucket)
        .cloned()
        .unwrap_or_default()
        == connection
    {
        return Ok(connection);
    }
    if engine.has_unfinished_bucket(&account_id, &bucket).await {
        return Err(crate::i18n::t("请先完成或取消此存储桶的传输，再修改连接设置。").into());
    }
    store
        .save_bucket_connection(&account_id, &bucket, connection.clone())
        .await?;
    Ok(connection)
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
        || keys.iter().any(|key| key.is_empty() || key.len() > 1024)
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
            let page = object_listing(client, bucket, root, token)
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

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PathTarget {
    Directory {
        prefix: String,
    },
    Object {
        object: S3Object,
        info: Option<ObjectInfo>,
    },
}

pub async fn locate_path(
    store: &AccountStore,
    account_id: &str,
    bucket: &str,
    key: &str,
    object_only: bool,
) -> Result<PathTarget, String> {
    if key.is_empty() && !object_only {
        return Ok(PathTarget::Directory {
            prefix: String::new(),
        });
    }
    if key.is_empty() || key.len() > 1024 {
        return Err(crate::i18n::t("对象键无效。").into());
    }
    let client = bucket_client(store, account_id, bucket).await?;
    locate_with_client(&client, bucket, key, object_only).await
}

async fn locate_with_client(
    client: &Client,
    bucket: &str,
    key: &str,
    object_only: bool,
) -> Result<PathTarget, String> {
    if object_only || !key.ends_with('/') {
        let exact = match client.head_object().bucket(bucket).key(key).send().await {
            Ok(head) => Some(PathTarget::Object {
                object: S3Object {
                    folder: false,
                    ..head_listing(key.to_owned(), &head)?
                },
                info: Some(head_info(&head)),
            }),
            Err(error) => match error
                .raw_response()
                .map(|response| response.status().as_u16())
            {
                Some(404) => None,
                Some(400 | 403) => {
                    // General-purpose S3 listings are ordered by key: an exact
                    // match is the first entry under its own prefix.
                    first_path_entry(client, bucket, key)
                        .await?
                        .filter(|object| object.key == key)
                        .map(|object| PathTarget::Object {
                            object: S3Object {
                                folder: false,
                                ..object
                            },
                            info: None,
                        })
                }
                _ => return Err(failure(error)),
            },
        };
        if let Some(exact) = exact {
            return Ok(exact);
        }
    }
    if !object_only {
        let prefix = if key.ends_with('/') {
            key.to_owned()
        } else {
            format!("{key}/")
        };
        if prefix.len() <= 1024 && first_path_entry(client, bucket, &prefix).await?.is_some() {
            return Ok(PathTarget::Directory { prefix });
        }
    }
    Err(crate::i18n::t("找不到指定的目录或对象。").into())
}

async fn first_path_entry(
    client: &Client,
    bucket: &str,
    prefix: &str,
) -> Result<Option<S3Object>, String> {
    let mut token = None;
    let mut seen_tokens = BTreeSet::new();
    loop {
        let page = object_listing(client, bucket, prefix, token)
            .max_keys(1)
            .encoding_type(EncodingType::Url)
            .send()
            .await
            .map_err(failure)?;
        if let Some(item) = page.contents().first() {
            return scanned_object(
                item,
                prefix,
                page.encoding_type() == Some(&EncodingType::Url),
            )
            .map(Some);
        }
        let next = page.next_continuation_token();
        if !page.is_truncated().unwrap_or(next.is_some()) {
            return Ok(None);
        }
        token = Some(
            next.filter(|next| !next.is_empty() && seen_tokens.insert((*next).to_owned()))
                .ok_or_else(|| crate::i18n::t("服务端未返回有效的分页标记。"))?
                .to_owned(),
        );
    }
}

pub async fn object_info(
    store: &AccountStore,
    account_id: String,
    bucket: String,
    key: String,
) -> Result<ObjectInfo, String> {
    let result = head_object(store, &account_id, &bucket, &key).await?;
    Ok(head_info(&result))
}

fn head_info(result: &HeadObjectOutput) -> ObjectInfo {
    ObjectInfo {
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
    }
}

async fn head_object(
    store: &AccountStore,
    account_id: &str,
    bucket: &str,
    key: &str,
) -> Result<HeadObjectOutput, String> {
    bucket_client(store, account_id, bucket)
        .await?
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(failure)
}

pub async fn object_listing_entry(
    store: &AccountStore,
    account_id: &str,
    bucket: &str,
    key: String,
) -> Result<S3Object, String> {
    let result = head_object(store, account_id, bucket, &key).await?;
    head_listing(key, &result)
}

fn head_listing(key: String, result: &HeadObjectOutput) -> Result<S3Object, String> {
    let size = result
        .content_length()
        .filter(|&size| size >= 0)
        .ok_or_else(|| crate::i18n::t("服务端返回了无效的对象键或大小。"))?;
    Ok(S3Object {
        folder: size == 0 && key.ends_with('/'),
        key,
        size,
        modified: result
            .last_modified()
            .map(ToString::to_string)
            .unwrap_or_default(),
        storage_class: result
            .storage_class()
            .map(|class| class.as_str().to_owned())
            .unwrap_or_else(|| "STANDARD".into()),
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
    async fn locates_an_exact_object_with_its_original_key_and_metadata() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let client = test_client(format!("http://{}", server.server_addr()));
        let thread = std::thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            assert_eq!(request.method(), &tiny_http::Method::Head);
            assert_eq!(request.url(), "/test-bucket/design//%E5%9B%BE%20%252F.csv");
            request
                .respond(tiny_http::Response::new(
                    tiny_http::StatusCode(200),
                    vec![tiny_http::Header::from_bytes("Content-Type", "text/csv").unwrap()],
                    std::io::empty(),
                    Some(42),
                    None,
                ))
                .unwrap();
        });
        let key = "design//图 %2F.csv";
        let target = locate_with_client(&client, "test-bucket", key, false)
            .await
            .unwrap();
        thread.join().unwrap();
        let PathTarget::Object {
            object,
            info: Some(info),
        } = target
        else {
            panic!("expected the exact object and its metadata");
        };
        assert_eq!(object.key, key);
        assert_eq!(object.size, 42);
        assert!(!object.folder);
        assert_eq!(info.content_type, "text/csv");
    }

    #[tokio::test]
    async fn locates_a_directory_with_or_without_a_trailing_slash() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let client = test_client(format!("http://{}", server.server_addr()));
        let thread = std::thread::spawn(move || {
            let head = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            assert_eq!(head.method(), &tiny_http::Method::Head);
            assert_eq!(head.url(), "/test-bucket/design/guides");
            head.respond(tiny_http::Response::empty(404)).unwrap();
            for _ in 0..2 {
                let list = server
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap()
                    .unwrap();
                assert_eq!(list.method(), &tiny_http::Method::Get);
                assert!(list.url().contains("prefix=design%2Fguides%2F"));
                assert!(list.url().contains("max-keys=1"));
                list.respond(tiny_http::Response::from_string(
                    "<ListBucketResult><Contents><Key>design/guides/file.pdf</Key><Size>9</Size></Contents></ListBucketResult>",
                )).unwrap();
            }
        });
        for key in ["design/guides", "design/guides/"] {
            let target = locate_with_client(&client, "test-bucket", key, false)
                .await
                .unwrap();
            assert!(
                matches!(target, PathTarget::Directory { prefix } if prefix == "design/guides/")
            );
        }
        thread.join().unwrap();
    }

    #[tokio::test]
    async fn list_only_lookup_requires_the_exact_key_and_supports_slash_ending_objects() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let client = test_client(format!("http://{}", server.server_addr()));
        let thread = std::thread::spawn(move || {
            for returned_key in ["design%2Fobject%2F", "design%2Fobject%2Fchild"] {
                let head = server
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap()
                    .unwrap();
                assert_eq!(head.method(), &tiny_http::Method::Head);
                assert_eq!(head.url(), "/test-bucket/design/object/");
                head.respond(tiny_http::Response::empty(403)).unwrap();
                let list = server
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap()
                    .unwrap();
                assert!(list.url().contains("prefix=design%2Fobject%2F"));
                assert!(list.url().contains("encoding-type=url"));
                list.respond(tiny_http::Response::from_string(format!(
                    "<ListBucketResult><EncodingType>url</EncodingType><Contents><Key>{returned_key}</Key><Size>24</Size></Contents></ListBucketResult>",
                ))).unwrap();
            }
        });
        let target = locate_with_client(&client, "test-bucket", "design/object/", true)
            .await
            .unwrap();
        assert!(matches!(target, PathTarget::Object { object, info: None }
            if !object.folder && object.key == "design/object/" && object.size == 24));
        let missing = locate_with_client(&client, "test-bucket", "design/object/", true)
            .await
            .err()
            .unwrap();
        assert_eq!(missing, crate::i18n::t("找不到指定的目录或对象。"));
        thread.join().unwrap();
    }

    #[tokio::test]
    async fn bucket_settings_persist_and_keep_explicit_values_after_option_removal() {
        use crate::accounts::{set_s3_defaults, S3Option};
        use crate::transfers::Direction;
        let dir = std::env::temp_dir().join(format!("s3-bucket-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("accounts.json");
        std::fs::write(
            &path,
            r#"[{"id":"test","name":"Test","region":"eu-west-1","pathStyle":true}]"#,
        )
        .unwrap();
        let store = AccountStore::open(path.clone()).unwrap();
        let engine = Engine::open(dir.join("transfers.json"), std::sync::Arc::new(|_| {})).unwrap();
        let defaults = set_s3_defaults(
            &store,
            S3Defaults {
                endpoint: "https://global.example".into(),
                region: "us-east-1".into(),
                endpoints: vec!["https://bucket.example/Case".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let bucket = "test-bucket";
        let connection = set_bucket_connection(
            &store,
            &engine,
            "test".into(),
            bucket.into(),
            S3Connection {
                endpoint: " https://bucket.example/Case/ ".into(),
                region: String::new(),
            },
        )
        .await
        .unwrap();
        let mut options = defaults.clone();
        options.remove_option(S3Option::Endpoint, &connection.endpoint);
        let saved = set_s3_defaults(&store, options).await.unwrap();
        assert_eq!(saved.endpoints, ["https://global.example"]);

        let reopened = AccountStore::open(path).unwrap();
        let account = reopened.account("test").await.unwrap();
        assert_eq!(account.bucket_connections[bucket], connection);
        let (resolved, pinned) = resolve_connection(&account, &saved, &connection);
        assert!(pinned);
        assert_eq!(resolved.endpoint, "https://bucket.example/Case");
        assert_eq!(resolved.region, "eu-west-1");
        engine
            .enqueue(
                Direction::Download,
                "test".into(),
                bucket.into(),
                "file".into(),
                dir.join("file"),
            )
            .await
            .unwrap();
        assert!(set_bucket_connection(
            &reopened,
            &engine,
            "test".into(),
            bucket.into(),
            S3Connection::default()
        )
        .await
        .is_err());
        assert_eq!(
            reopened.account("test").await.unwrap().bucket_connections[bucket],
            connection
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn creates_bucket_with_selected_endpoint_and_region() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let client = test_client(format!("http://{}", server.server_addr()));
        let client = Client::from_conf(
            client
                .config()
                .to_builder()
                .region(Region::new("eu-west-2"))
                .build(),
        );
        let server_thread = std::thread::spawn(move || {
            let mut request = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            assert_eq!(request.method(), &tiny_http::Method::Put);
            assert_eq!(request.url(), "/test-bucket/");
            assert!(request
                .headers()
                .iter()
                .any(|header| header.field.equiv("Authorization")
                    && header.value.as_str().contains("/eu-west-2/s3/")));
            let mut body = String::new();
            request.as_reader().read_to_string(&mut body).unwrap();
            assert!(body.contains("<LocationConstraint>eu-west-2</LocationConstraint>"));
            request.respond(tiny_http::Response::empty(200)).unwrap();
        });
        create_bucket_at(&client, "test-bucket", "eu-west-2")
            .await
            .unwrap();
        server_thread.join().unwrap();
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
    async fn flat_scan_follows_pages_and_decodes_object_keys_once() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let client = test_client(format!("http://{}", server.server_addr()));
        let thread = std::thread::spawn(move || {
            let first = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            assert!(first.url().contains("list-type=2"));
            assert!(first.url().contains("prefix=design%2F"));
            assert!(first.url().contains("encoding-type=url"));
            assert!(!first.url().contains("delimiter="));
            first.respond(tiny_http::Response::from_string(r#"<ListBucketResult><EncodingType>url</EncodingType><IsTruncated>true</IsTruncated><NextContinuationToken>a+/=</NextContinuationToken><Contents><Key>design%2Fphotos%2F%E6%A0%B7%E7%89%87%20a%252Fb.png</Key><Size>42</Size><StorageClass>GLACIER</StorageClass></Contents></ListBucketResult>"#)).unwrap();
            let second = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            assert!(second.url().contains("continuation-token=a%2B%2F%3D"));
            assert!(!second.url().contains("delimiter="));
            second.respond(tiny_http::Response::from_string(r#"<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>design/photos/</Key><Size>0</Size></Contents><Contents><Key>design/photos/a%2Fb/</Key><Size>12</Size></Contents></ListBucketResult>"#)).unwrap();
        });
        let mut pages = Vec::new();
        scan_listing(&client, "test-bucket", "design/", |page| {
            pages.push(page);
            Ok(())
        })
        .await
        .unwrap();
        thread.join().unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0][0].key, "design/photos/样片 a%2Fb.png");
        assert_eq!(pages[0][0].size, 42);
        assert_eq!(pages[0][0].storage_class, "GLACIER");
        assert_eq!(pages[1][0].key, "design/photos/");
        assert!(pages[1][0].folder);
        assert_eq!(pages[1][1].key, "design/photos/a%2Fb/");
        assert!(!pages[1][1].folder);
    }

    #[tokio::test]
    async fn tree_level_lists_only_one_layer_and_preserves_slash_ending_files() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let client = test_client(format!("http://{}", server.server_addr()));
        let thread = std::thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            assert!(request.url().contains("prefix=design%2F"));
            assert!(request.url().contains("delimiter=%2F"));
            assert!(request.url().contains("max-keys=200"));
            assert!(request.url().contains("encoding-type=url"));
            request.respond(tiny_http::Response::from_string(r#"<ListBucketResult><EncodingType>url</EncodingType><IsTruncated>true</IsTruncated><NextContinuationToken>next</NextContinuationToken><CommonPrefixes><Prefix>design%2Fphotos%2F</Prefix></CommonPrefixes><Contents><Key>design%2F</Key><Size>12</Size></Contents><Contents><Key>design%2Fphotos%2Fdeep%2Ffile.png</Key><Size>9</Size></Contents><Contents><Key>design%2Fnotes%252F.txt</Key><Size>2</Size></Contents></ListBucketResult>"#)).unwrap();
        });
        let page = fetch_tree_level(&client, "test-bucket", "design/", None)
            .await
            .unwrap();
        thread.join().unwrap();
        assert_eq!(page.next_token.as_deref(), Some("next"));
        assert_eq!(page.objects.len(), 3);
        assert_eq!(page.objects[0].key, "design/");
        assert!(!page.objects[0].folder);
        assert_eq!(page.objects[0].size, 12);
        assert_eq!(page.objects[1].key, "design/notes%2F.txt");
        assert_eq!(page.objects[2].key, "design/photos/");
        assert!(page.objects[2].folder);
    }

    #[tokio::test]
    async fn flat_scan_rejects_a_repeated_continuation_token() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let client = test_client(format!("http://{}", server.server_addr()));
        let thread = std::thread::spawn(move || {
            for _ in 0..2 {
                let request = server
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap()
                    .unwrap();
                request.respond(tiny_http::Response::from_string(r#"<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>again</NextContinuationToken><Contents><Key>design/a</Key><Size>5</Size></Contents></ListBucketResult>"#)).unwrap();
            }
        });
        let mut pages = 0;
        assert!(scan_listing(&client, "test-bucket", "design/", |_| {
            pages += 1;
            Ok(())
        })
        .await
        .is_err());
        thread.join().unwrap();
        assert_eq!(pages, 1);
    }

    #[tokio::test]
    async fn deletes_an_explicit_slash_ending_object_without_listing_its_prefix() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let client = test_client(format!("http://{}", server.server_addr()));
        let thread = std::thread::spawn(move || {
            let mut request = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            assert_eq!(request.method(), &tiny_http::Method::Post);
            assert!(request.url().contains("?delete"));
            let mut body = String::new();
            request.as_reader().read_to_string(&mut body).unwrap();
            assert!(body.contains("<Key>design/object/</Key>"));
            request
                .respond(tiny_http::Response::from_string("<DeleteResult/>"))
                .unwrap();
        });
        let count = delete_selection(
            &client,
            "test-bucket",
            vec!["design/object/".into()],
            vec![],
        )
        .await
        .unwrap();
        assert_eq!(count, 1);
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
