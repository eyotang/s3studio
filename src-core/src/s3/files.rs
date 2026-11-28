use super::{bucket_client, copy_source, failure, scan_listing, S3Object};
use crate::{accounts::AccountStore, i18n::t};
use aws_sdk_s3::{
    operation::head_object::HeadObjectOutput,
    types::{
        CompletedMultipartUpload, CompletedPart, MetadataDirective, StorageClass, Tag, Tagging,
    },
    Client,
};
use chrono::{DateTime, NaiveDate, Utc};
use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Default, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Filter {
    pub pattern: String,
    pub after: String,
    pub before: String,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub storage_class: String,
}

pub struct Matcher {
    glob: Option<GlobMatcher>,
    after: Option<DateTime<Utc>>,
    before: Option<DateTime<Utc>>,
    spec: Filter,
}

impl Filter {
    pub fn compile(&self) -> Result<Matcher, String> {
        let date = |s: &str, end: bool| -> Result<Option<DateTime<Utc>>, String> {
            if s.is_empty() {
                return Ok(None);
            }
            if let Ok(day) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                let instant = if end {
                    day.and_hms_nano_opt(23, 59, 59, 999_999_999)
                } else {
                    day.and_hms_opt(0, 0, 0)
                };
                return Ok(instant.map(|d| d.and_utc()));
            }
            DateTime::parse_from_rfc3339(s)
                .map(|d| Some(d.with_timezone(&Utc)))
                .map_err(failure)
        };
        let after = date(&self.after, false)?;
        let before = date(&self.before, true)?;
        if after.zip(before).is_some_and(|(a, b)| a > b)
            || self.min_size.zip(self.max_size).is_some_and(|(a, b)| a > b)
        {
            return Err(t("范围无效").into());
        }
        let glob = if self.pattern.is_empty() {
            None
        } else {
            Some(Glob::new(&self.pattern).map_err(failure)?.compile_matcher())
        };
        Ok(Matcher {
            glob,
            after,
            before,
            spec: self.clone(),
        })
    }
}
impl Matcher {
    pub fn matches(&self, key: &str, size: u64, modified: &str, class: &str) -> bool {
        let day = DateTime::parse_from_rfc3339(modified)
            .ok()
            .map(|d| d.with_timezone(&Utc));
        self.glob.as_ref().is_none_or(|g| g.is_match(key))
            && self.spec.min_size.is_none_or(|n| size >= n)
            && self.spec.max_size.is_none_or(|n| size <= n)
            && self.after.is_none_or(|a| day.is_some_and(|d| d >= a))
            && self.before.is_none_or(|b| day.is_some_and(|d| d <= b))
            && (self.spec.storage_class.is_empty()
                || self.spec.storage_class.eq_ignore_ascii_case(class))
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Selection {
    pub key: String,
    pub folder: bool,
}

// Keep real trailing-slash keys distinct from virtual common prefixes.
pub async fn expand(
    client: &Client,
    bucket: &str,
    selections: &[Selection],
    filter: &Filter,
) -> Result<Vec<S3Object>, String> {
    let matcher = filter.compile()?;
    let mut items = BTreeMap::new();
    let mut prefixes = BTreeSet::<String>::new();
    let mut sorted = selections.to_vec();
    sorted.sort_by(|a, b| a.key.cmp(&b.key).then(b.folder.cmp(&a.folder)));
    for selection in sorted {
        if prefixes
            .iter()
            .next_back()
            .is_some_and(|p| selection.key.starts_with(p))
        {
            continue;
        }
        if selection.folder {
            if !selection.key.is_empty() && !selection.key.ends_with('/') {
                return Err(t("对象键无效。").into());
            }
            scan_listing(client, bucket, &selection.key, |page| {
                for object in page {
                    if matcher.matches(
                        &object.key,
                        object.size.max(0) as u64,
                        &object.modified,
                        &object.storage_class,
                    ) {
                        items.insert(object.key.clone(), object);
                    }
                }
                Ok(())
            })
            .await?;
            prefixes.insert(selection.key);
        } else if !items.contains_key(&selection.key) {
            let h = client
                .head_object()
                .bucket(bucket)
                .key(&selection.key)
                .send()
                .await
                .map_err(failure)?;
            let obj = super::head_listing(selection.key.clone(), &h)?;
            if matcher.matches(&obj.key, obj.size as u64, &obj.modified, &obj.storage_class) {
                items.insert(selection.key, obj);
            }
        }
    }
    Ok(items.into_values().collect())
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    pub account: String,
    pub bucket: String,
    pub path: String,
}

pub async fn relocate(
    store: &AccountStore,
    account: &str,
    bucket: &str,
    selections: &[Selection],
    target: &Destination,
    moving: bool,
    exact: bool,
) -> Result<usize, String> {
    if selections.is_empty() || target.bucket.is_empty() || (exact && selections.len() != 1) {
        return Err(t("请选择文件或文件夹").into());
    }
    let _change = store.connection_changes.lock().await;
    let source = bucket_client(store, account, bucket).await?;
    let dest = bucket_client(store, &target.account, &target.bucket).await?;
    let defaults = crate::accounts::get_s3_defaults(store).await;
    let same_service = defaults.endpoint_for(&store.account(account).await?)
        == defaults.endpoint_for(&store.account(&target.account).await?);
    relocate_objects(
        &source,
        bucket,
        &dest,
        target,
        selections,
        moving,
        exact,
        same_service && bucket == target.bucket,
        same_service && account == target.account,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn relocate_objects(
    source: &Client,
    bucket: &str,
    dest: &Client,
    target: &Destination,
    selections: &[Selection],
    moving: bool,
    exact: bool,
    same_bucket: bool,
    server_copy: bool,
) -> Result<usize, String> {
    let mut targets = BTreeMap::new();
    let mut sources = BTreeSet::new();
    let mut roots = selections.to_vec();
    roots.sort_by(|a, b| a.key.cmp(&b.key).then(b.folder.cmp(&a.folder)));
    let mut folder_roots = BTreeSet::<String>::new();
    for selection in &roots {
        if folder_roots
            .iter()
            .next_back()
            .is_some_and(|p| selection.key.starts_with(p))
        {
            continue;
        }
        let leaf = selection
            .key
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("");
        let mut base = if exact {
            target.path.clone()
        } else if target.path.is_empty() {
            leaf.into()
        } else {
            format!("{}/{}", target.path.trim_end_matches('/'), leaf)
        };
        if selection.folder {
            if !base.is_empty() && !base.ends_with('/') {
                base.push('/');
            }
            if same_bucket && (base.starts_with(&selection.key) || selection.key.starts_with(&base))
            {
                return Err(t("源路径与目标路径重叠").into());
            }
            folder_roots.insert(selection.key.clone());
        }
        for object in expand(
            &source,
            bucket,
            std::slice::from_ref(selection),
            &Filter::default(),
        )
        .await?
        {
            let suffix = object
                .key
                .strip_prefix(&selection.key)
                .ok_or(t("对象键无效。"))?;
            let key = format!("{base}{suffix}");
            if key.is_empty() || key.len() > 1024 || (!selection.folder && key.ends_with('/')) {
                return Err(t("对象键无效。").into());
            }
            sources.insert(object.key.clone());
            if targets.insert(key, object.key).is_some() {
                return Err(t("多个对象映射到同一目标").into());
            }
        }
    }
    if same_bucket && targets.keys().any(|k| sources.contains(k)) {
        return Err(t("源路径与目标路径重叠").into());
    }
    let mut count = 0;
    for (new, old) in targets {
        let result = async {
            match dest
                .head_object()
                .bucket(&target.bucket)
                .key(&new)
                .send()
                .await
            {
                Ok(_) => return Err(t("目标对象已存在，请换一个对象键。").into()),
                Err(e) if e.raw_response().is_some_and(|r| r.status().as_u16() == 404) => {}
                Err(e) => return Err(failure(e)),
            }
            let head = source
                .head_object()
                .bucket(bucket)
                .key(&old)
                .send()
                .await
                .map_err(failure)?;
            copy_file(
                &source,
                bucket,
                &old,
                &head,
                &dest,
                &target.bucket,
                &new,
                server_copy,
                None,
            )
            .await?;
            if moving {
                let current = source
                    .head_object()
                    .bucket(bucket)
                    .key(&old)
                    .if_match(head.e_tag().ok_or(t("服务端分片缺少 ETag。"))?)
                    .send()
                    .await
                    .map_err(failure)?;
                if current.version_id() != head.version_id()
                    || current.last_modified() != head.last_modified()
                    || current.content_length() != head.content_length()
                {
                    return Err(t("对象状态已变化，请刷新版本列表后重试。").into());
                }
                source
                    .delete_object()
                    .bucket(bucket)
                    .key(&old)
                    .if_match(head.e_tag().ok_or(t("服务端分片缺少 ETag。"))?)
                    .send()
                    .await
                    .map_err(failure)?;
            }
            Ok::<(), String>(())
        }
        .await;
        if let Err(e) = result {
            return Err(format!("{} {count}\n{old} → {new}\n{e}", t("已完成")));
        }
        count += 1;
    }
    Ok(count)
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Properties {
    pub content_type: Option<String>,
    pub cache_control: Option<String>,
    pub storage_class: Option<String>,
    pub metadata: BTreeMap<String, Option<String>>,
    pub tags: BTreeMap<String, Option<String>>,
}
fn merge(values: &mut HashMap<String, String>, patch: &BTreeMap<String, Option<String>>) {
    for (k, v) in patch {
        if let Some(v) = v {
            values.insert(k.clone(), v.clone());
        } else {
            values.remove(k);
        }
    }
}
pub(super) async fn tags(
    client: &Client,
    bucket: &str,
    key: &str,
    head: &HeadObjectOutput,
) -> Result<HashMap<String, String>, String> {
    let r = client
        .get_object_tagging()
        .bucket(bucket)
        .key(key)
        .set_version_id(head.version_id().map(str::to_owned))
        .send()
        .await
        .map_err(failure)?;
    Ok(r.tag_set()
        .iter()
        .map(|t| (t.key().to_owned(), t.value().to_owned()))
        .collect())
}
pub async fn properties(
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<serde_json::Value, String> {
    let h = client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(failure)?;
    let tags = match tags(client, bucket, key, &h).await {
        Ok(tags) => serde_json::json!(tags),
        Err(e) => serde_json::json!(e),
    };
    Ok(
        serde_json::json!({"Content-Type":h.content_type(),"Cache-Control":h.cache_control(),"StorageClass":h.storage_class().map(|s|s.as_str()).unwrap_or("STANDARD"),"Metadata":h.metadata(),"Tags":tags}),
    )
}
fn tag_list(values: HashMap<String, String>) -> Result<Tagging, String> {
    let tags = values
        .into_iter()
        .map(|(k, v)| Tag::builder().key(k).value(v).build().map_err(failure))
        .collect::<Result<Vec<_>, _>>()?;
    Tagging::builder()
        .set_tag_set(Some(tags))
        .build()
        .map_err(failure)
}

pub async fn update_properties(
    client: &Client,
    bucket: &str,
    items: &[Selection],
    patch: &Properties,
) -> Result<usize, String> {
    let headers = patch.content_type.is_some()
        || patch.cache_control.is_some()
        || patch.storage_class.is_some()
        || !patch.metadata.is_empty();
    if !headers && patch.tags.is_empty() {
        return Err(t("请填写要修改的属性").into());
    }
    let mut done = 0;
    for o in expand(client, bucket, items, &Filter::default()).await? {
        let result = async {
            let h = client
                .head_object()
                .bucket(bucket)
                .key(&o.key)
                .send()
                .await
                .map_err(failure)?;
            if headers {
                copy_file(
                    client,
                    bucket,
                    &o.key,
                    &h,
                    client,
                    bucket,
                    &o.key,
                    true,
                    Some(patch),
                )
                .await?;
            } else {
                let mut values = tags(client, bucket, &o.key, &h).await?;
                merge(&mut values, &patch.tags);
                let version = h.version_id().map(str::to_owned);
                if values.is_empty() {
                    client
                        .delete_object_tagging()
                        .bucket(bucket)
                        .key(&o.key)
                        .set_version_id(version)
                        .send()
                        .await
                        .map_err(failure)?;
                } else {
                    client
                        .put_object_tagging()
                        .bucket(bucket)
                        .key(&o.key)
                        .set_version_id(version)
                        .tagging(tag_list(values)?)
                        .send()
                        .await
                        .map_err(failure)?;
                }
            }
            Ok::<(), String>(())
        }
        .await;
        result.map_err(|e| format!("{} {done}\n{}\n{e}", t("已完成"), o.key))?;
        done += 1;
    }
    Ok(done)
}

#[allow(clippy::too_many_arguments)]
async fn copy_file(
    src: &Client,
    bucket: &str,
    key: &str,
    h: &HeadObjectOutput,
    dst: &Client,
    to_bucket: &str,
    to_key: &str,
    server: bool,
    patch: Option<&Properties>,
) -> Result<(), String> {
    let size = h
        .content_length()
        .filter(|s| *s >= 0)
        .ok_or(t("对象大小无效。"))? as u64;
    let etag = h.e_tag().ok_or(t("服务端分片缺少 ETag。"))?;
    let mut from = copy_source(bucket, key);
    if let Some(version) = h.version_id() {
        from.push_str("?versionId=");
        from.push_str(
            &percent_encoding::utf8_percent_encode(version, percent_encoding::NON_ALPHANUMERIC)
                .to_string(),
        );
    }
    let exists = patch.is_some();
    let condition = exists.then(|| etag.to_owned());
    let absent = (!exists).then(|| "*".to_owned());
    let single = server && size <= 5 * 1024u64.pow(3);
    let defaults = Properties::default();
    let p = patch.unwrap_or(&defaults);
    let class = p
        .storage_class
        .as_deref()
        .map(StorageClass::from)
        .or_else(|| h.storage_class().cloned());
    let mut metadata = h.metadata().cloned().unwrap_or_default();
    merge(&mut metadata, &p.metadata);
    let content_type = p
        .content_type
        .clone()
        .or_else(|| h.content_type().map(str::to_owned));
    let cache_control = p
        .cache_control
        .clone()
        .or_else(|| h.cache_control().map(str::to_owned));
    let mut encoded_tags = None;
    if !single || !p.tags.is_empty() {
        let mut values = tags(src, bucket, key, h).await?;
        merge(&mut values, &p.tags);
        encoded_tags = Some(
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(values)
                .finish(),
        );
    }
    let expires = h
        .expires_string()
        .map(|v| {
            aws_smithy_types::DateTime::from_str(v, aws_smithy_types::date_time::Format::HttpDate)
        })
        .transpose()
        .map_err(failure)?;
    macro_rules! headers {
        ($request:expr) => {
            $request
                .set_metadata(Some(metadata))
                .set_content_type(content_type)
                .set_cache_control(cache_control)
                .set_content_encoding(h.content_encoding().map(str::to_owned))
                .set_content_language(h.content_language().map(str::to_owned))
                .set_content_disposition(h.content_disposition().map(str::to_owned))
                .set_expires(expires)
                .set_website_redirect_location(h.website_redirect_location().map(str::to_owned))
        };
    }
    if single {
        let mut req = dst
            .copy_object()
            .bucket(to_bucket)
            .key(to_key)
            .copy_source(from)
            .copy_source_if_match(etag)
            .set_if_match(condition)
            .set_if_none_match(absent)
            .set_storage_class(class);
        if exists {
            req = headers!(req)
                .metadata_directive(MetadataDirective::Replace)
                .set_server_side_encryption(h.server_side_encryption().cloned())
                .set_ssekms_key_id(h.ssekms_key_id().map(str::to_owned));
        }
        if let Some(tags) = encoded_tags {
            req = req
                .tagging_directive(aws_sdk_s3::types::TaggingDirective::Replace)
                .tagging(tags);
        }
        req.send().await.map_err(failure)?;
        return Ok(());
    }
    let tagging = encoded_tags.filter(|s| !s.is_empty());
    if size == 0 {
        headers!(dst.put_object().bucket(to_bucket).key(to_key))
            .set_tagging(tagging)
            .set_storage_class(class)
            .set_if_none_match(absent)
            .send()
            .await
            .map_err(failure)?;
        return Ok(());
    }
    let mut create = headers!(dst.create_multipart_upload().bucket(to_bucket).key(to_key))
        .set_storage_class(class)
        .set_tagging(tagging);
    if exists {
        create = create
            .set_server_side_encryption(h.server_side_encryption().cloned())
            .set_ssekms_key_id(h.ssekms_key_id().map(str::to_owned));
    }
    let created = create.send().await.map_err(failure)?;
    let id = created
        .upload_id()
        .ok_or(t("服务未返回 multipart UploadId。"))?;
    let result = async {
        let chunk = (32 * 1024 * 1024u64).max(size.div_ceil(10_000));
        let mut parts = Vec::new();
        for (i, start) in (0..size).step_by(chunk as usize).enumerate() {
            let len = chunk.min(size - start);
            let range = format!("bytes={start}-{}", start + len - 1);
            let number = (i + 1) as i32;
            let part_etag = if server {
                let copied = dst
                    .upload_part_copy()
                    .bucket(to_bucket)
                    .key(to_key)
                    .upload_id(id)
                    .part_number(number)
                    .copy_source(&from)
                    .copy_source_if_match(etag)
                    .copy_source_range(range)
                    .send()
                    .await
                    .map_err(failure)?;
                copied
                    .copy_part_result()
                    .and_then(|p| p.e_tag())
                    .map(str::to_owned)
            } else {
                let data = src
                    .get_object()
                    .bucket(bucket)
                    .key(key)
                    .set_version_id(h.version_id().map(str::to_owned))
                    .if_match(etag)
                    .range(range)
                    .send()
                    .await
                    .map_err(failure)?;
                if data.content_length() != Some(len as i64) {
                    return Err(t("对象大小无效。").into());
                }
                let uploaded = dst
                    .upload_part()
                    .bucket(to_bucket)
                    .key(to_key)
                    .upload_id(id)
                    .part_number(number)
                    .content_length(len as i64)
                    .body(data.body)
                    .send()
                    .await
                    .map_err(failure)?;
                uploaded.e_tag().map(str::to_owned)
            }
            .ok_or(t("服务端分片缺少 ETag。"))?;
            parts.push(
                CompletedPart::builder()
                    .part_number(number)
                    .e_tag(part_etag)
                    .build(),
            );
        }
        dst.complete_multipart_upload()
            .bucket(to_bucket)
            .key(to_key)
            .upload_id(id)
            .set_if_match(condition)
            .set_if_none_match(absent)
            .multipart_upload(
                CompletedMultipartUpload::builder()
                    .set_parts(Some(parts))
                    .build(),
            )
            .send()
            .await
            .map_err(failure)?;
        Ok::<(), String>(())
    }
    .await;
    if let Err(err) = result {
        dst.abort_multipart_upload()
            .bucket(to_bucket)
            .key(to_key)
            .upload_id(id)
            .send()
            .await
            .map_err(|e| format!("{err}\n{}", failure(e)))?;
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s3::tests::test_client;
    use tiny_http::{Header, Response, Server};
    #[test]
    fn combines_utc_dates_size_class_and_glob() {
        let f = Filter {
            pattern: "*.csv".into(),
            after: "2026-11-01".into(),
            before: "2026-11-01".into(),
            min_size: Some(5),
            max_size: Some(5),
            storage_class: "standard".into(),
        }
        .compile()
        .unwrap();
        assert!(f.matches("sales/a.csv", 5, "2026-10-31T21:00:00-03:00", "STANDARD"));
        assert!(!f.matches("sales/a.csv", 6, "2026-11-01T00:00:00Z", "STANDARD"));
        assert!(!f.matches("sales/a.json", 5, "2026-11-01T00:00:00Z", "STANDARD"));
    }
    #[tokio::test]
    async fn failed_cross_account_move_never_deletes_source() {
        let server = Server::http("127.0.0.1:0").unwrap();
        let src = test_client(format!("http://{}", server.server_addr()));
        let dest = src.clone();
        let worker = std::thread::spawn(move || {
            for (method, path) in [
                ("HEAD", "/source/file"),
                ("HEAD", "/target/file"),
                ("HEAD", "/source/file"),
                ("GET", "/source/file?tagging"),
                ("POST", "/target/file?uploads"),
            ] {
                let req = server
                    .recv_timeout(std::time::Duration::from_secs(4))
                    .unwrap()
                    .unwrap();
                assert_eq!(req.method().as_str(), method);
                assert!(req.url().starts_with(path), "{:?}", req.url());
                let response = match method {
                    "HEAD" if path.contains("target") => {
                        Response::from_string("").with_status_code(404)
                    }
                    "HEAD" => Response::from_string("data")
                        .with_header(Header::from_bytes("ETag", "\"old\"").unwrap()),
                    "GET" => Response::from_string("<Tagging><TagSet/></Tagging>"),
                    _ => Response::from_string("<Error><Code>AccessDenied</Code></Error>")
                        .with_status_code(403),
                };
                req.respond(response).unwrap();
            }
            assert!(server
                .recv_timeout(std::time::Duration::from_millis(150))
                .unwrap()
                .is_none());
        });
        let target = Destination {
            account: "other".into(),
            bucket: "target".into(),
            path: "".into(),
        };
        let selection = [Selection {
            key: "file".into(),
            folder: false,
        }];
        assert!(relocate_objects(
            &src, "source", &dest, &target, &selection, true, false, false, false
        )
        .await
        .is_err());
        worker.join().unwrap();
    }
    #[tokio::test]
    async fn relay_commits_destination_before_conditional_source_delete() {
        let server = Server::http("127.0.0.1:0").unwrap();
        let client = test_client(format!("http://{}", server.server_addr()));
        let worker = std::thread::spawn(move || {
            let mut committed = false;
            for step in 0..10 {
                let mut req = server
                    .recv_timeout(std::time::Duration::from_secs(4))
                    .unwrap()
                    .unwrap();
                let method = req.method().as_str().to_owned();
                let checked = |name: &'static str, val: &str| {
                    req.headers()
                        .iter()
                        .any(|h| h.field.equiv(name) && h.value.as_str() == val)
                };
                let response = match step {
                    0 | 2 | 8 => {
                        assert_eq!(method, "HEAD");
                        if step == 8 {
                            assert!(committed && checked("If-Match", "\"source\""));
                        }
                        Response::from_string("payload")
                            .with_header(Header::from_bytes("ETag", "\"source\"").unwrap())
                    }
                    1 => {
                        assert_eq!(method, "HEAD");
                        Response::from_string("").with_status_code(404)
                    }
                    3 => {
                        assert!(req.url().contains("tagging"));
                        Response::from_string("<Tagging><TagSet><Tag><Key>team</Key><Value>qa</Value></Tag></TagSet></Tagging>")
                    }
                    4 => {
                        assert_eq!(method, "POST");
                        assert!(checked("x-amz-tagging", "team=qa"));
                        Response::from_string("<InitiateMultipartUploadResult><UploadId>u</UploadId></InitiateMultipartUploadResult>")
                    }
                    5 => {
                        assert_eq!(method, "GET");
                        assert!(checked("If-Match", "\"source\""));
                        Response::from_string("payload").with_status_code(206)
                    }
                    6 => {
                        assert_eq!(method, "PUT");
                        let mut body = Vec::new();
                        req.as_reader().read_to_end(&mut body).unwrap();
                        assert!(body.windows(7).any(|s| s == b"payload"));
                        Response::from_string("")
                            .with_header(Header::from_bytes("ETag", "\"part\"").unwrap())
                    }
                    7 => {
                        assert_eq!(method, "POST");
                        assert!(checked("If-None-Match", "*"));
                        committed = true;
                        Response::from_string("<CompleteMultipartUploadResult><ETag>&quot;dest&quot;</ETag></CompleteMultipartUploadResult>")
                    }
                    _ => {
                        assert_eq!(method, "DELETE");
                        assert!(committed && checked("If-Match", "\"source\""));
                        Response::from_string("").with_status_code(204)
                    }
                };
                req.respond(response).unwrap();
            }
        });
        let target = Destination {
            account: "other".into(),
            bucket: "dest".into(),
            path: "new".into(),
        };
        let items = [Selection {
            key: "old".into(),
            folder: false,
        }];
        assert_eq!(
            relocate_objects(&client, "source", &client, &target, &items, true, true, false, false)
                .await
                .unwrap(),
            1
        );
        worker.join().unwrap();
    }
}
