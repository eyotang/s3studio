use super::{failure, files::tags};
use crate::{folders::digest, i18n::t};
use aws_sdk_s3::{primitives::ByteStream, Client};
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};
use tokio::io::AsyncWriteExt;

pub struct EditSession {
    pub path: PathBuf,
    bucket: String,
    key: String,
    etag: String,
    stamp: (u64, SystemTime),
    hash: Vec<u8>,
    head: aws_sdk_s3::operation::head_object::HeadObjectOutput,
    tags: String,
}
async fn stamp(path: &Path) -> Result<(u64, SystemTime), String> {
    let m = tokio::fs::symlink_metadata(path).await.map_err(failure)?;
    if !m.is_file() {
        return Err(t("保存路径无效。").into());
    }
    Ok((m.len(), m.modified().map_err(failure)?))
}
impl EditSession {
    pub async fn prepare(
        client: &Client,
        bucket: &str,
        key: &str,
        dir: &Path,
    ) -> Result<Self, String> {
        let h = client
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(failure)?;
        let etag = h.e_tag().ok_or(t("服务端分片缺少 ETag。"))?.to_owned();
        if h.content_length()
            .is_none_or(|s| !(0..=5 * 1024i64.pow(3)).contains(&s))
        {
            return Err(t("编辑文件上限为 5 GiB").into());
        }
        let tags = tags(client, bucket, key, &h).await?;
        let tags = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(tags)
            .finish();
        std::fs::create_dir_all(dir).map_err(failure)?;
        let workspace = tempfile::Builder::new()
            .prefix("edit-")
            .tempdir_in(dir)
            .map_err(failure)?;
        let extension = Path::new(key)
            .extension()
            .and_then(|s| s.to_str())
            .filter(|s| s.len() <= 16 && s.chars().all(|c| c.is_ascii_alphanumeric()));
        let filename = extension.map_or_else(|| "document".into(), |s| format!("document.{s}"));
        let path = workspace.path().join(filename);
        let read = client
            .get_object()
            .bucket(bucket)
            .key(key)
            .if_match(&etag)
            .set_version_id(h.version_id().map(str::to_owned))
            .send()
            .await
            .map_err(failure)?;
        let mut file = tokio::fs::File::create(&path).await.map_err(failure)?;
        let bytes = tokio::io::copy(&mut read.body.into_async_read(), &mut file)
            .await
            .map_err(failure)?;
        if Some(bytes as i64) != h.content_length() {
            return Err(t("对象大小无效。").into());
        }
        file.flush().await.map_err(failure)?;
        drop(file);
        let session = Self {
            bucket: bucket.into(),
            key: key.into(),
            etag,
            stamp: stamp(&path).await?,
            hash: digest(tokio::fs::File::open(&path).await.map_err(failure)?).await?,
            head: h,
            tags,
            path,
        };
        // Preserve the working copy across editor and application restarts.
        let _directory = workspace.keep();
        Ok(session)
    }
    pub async fn changed(&self) -> Result<bool, String> {
        match tokio::fs::symlink_metadata(&self.path).await {
            Ok(m) if m.is_file() => Ok((m.len(), m.modified().map_err(failure)?) != self.stamp),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Ok(_) => Err(t("保存路径无效。").into()),
            Err(e) => Err(failure(e)),
        }
    }
    pub async fn save_if_changed(&mut self, client: &Client) -> Result<bool, String> {
        let observed = stamp(&self.path).await?;
        if observed == self.stamp {
            return Ok(false);
        }
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        if observed != stamp(&self.path).await? {
            return Ok(false);
        }
        let tmp = tempfile::NamedTempFile::new_in(self.path.parent().ok_or(t("保存路径无效。"))?)
            .map_err(failure)?;
        tokio::fs::copy(&self.path, tmp.path())
            .await
            .map_err(failure)?;
        if observed != stamp(&self.path).await? {
            return Ok(false);
        }
        let hash = digest(tokio::fs::File::open(tmp.path()).await.map_err(failure)?).await?;
        if hash == self.hash {
            self.stamp = observed;
            return Ok(false);
        }
        let h = &self.head;
        let result = client
            .put_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .if_match(&self.etag)
            .set_metadata(h.metadata().cloned())
            .set_content_type(h.content_type().map(str::to_owned))
            .set_content_encoding(h.content_encoding().map(str::to_owned))
            .set_content_disposition(h.content_disposition().map(str::to_owned))
            .set_content_language(h.content_language().map(str::to_owned))
            .set_cache_control(h.cache_control().map(str::to_owned))
            .set_expires(
                h.expires_string()
                    .map(|v| {
                        aws_smithy_types::DateTime::from_str(
                            v,
                            aws_smithy_types::date_time::Format::HttpDate,
                        )
                    })
                    .transpose()
                    .map_err(failure)?,
            )
            .set_website_redirect_location(h.website_redirect_location().map(str::to_owned))
            .set_storage_class(h.storage_class().cloned())
            .set_server_side_encryption(h.server_side_encryption().cloned())
            .set_ssekms_key_id(h.ssekms_key_id().map(str::to_owned))
            .set_tagging((!self.tags.is_empty()).then(|| self.tags.clone()))
            .body(ByteStream::from_path(tmp.path()).await.map_err(failure)?)
            .send()
            .await
            .map_err(failure)?;
        self.etag = result.e_tag().ok_or(t("服务端分片缺少 ETag。"))?.into();
        self.hash = hash;
        self.stamp = observed;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_http::{Header, Response, Server};
    #[tokio::test]
    async fn editor_uses_conditional_write_and_preserves_conflicting_local_copy() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::http("127.0.0.1:0").unwrap();
        let client = crate::s3::tests::test_client(format!("http://{}", server.server_addr()));
        let worker = std::thread::spawn(move || {
            for expected in ["HEAD", "tags", "GET", "PUT"] {
                let req = server
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap()
                    .unwrap();
                if expected == "tags" {
                    assert!(req.url().contains("tagging"));
                } else {
                    assert_eq!(req.method().as_str(), expected);
                }
                let res = match expected {
                    "tags" => Response::from_string("<Tagging><TagSet/></Tagging>"),
                    "PUT" => {
                        assert!(req
                            .headers()
                            .iter()
                            .any(|h| h.field.equiv("If-Match") && h.value.as_str() == "\"base\""));
                        Response::from_string("<Error><Code>PreconditionFailed</Code></Error>")
                            .with_status_code(412)
                    }
                    _ => Response::from_string("original")
                        .with_header(Header::from_bytes("ETag", "\"base\"").unwrap()),
                };
                req.respond(res).unwrap();
            }
        });
        let mut session = EditSession::prepare(&client, "bucket", "document.txt", dir.path())
            .await
            .unwrap();
        tokio::fs::write(&session.path, "changed locally")
            .await
            .unwrap();
        assert!(session.save_if_changed(&client).await.is_err());
        assert_eq!(
            tokio::fs::read_to_string(&session.path).await.unwrap(),
            "changed locally"
        );
        worker.join().unwrap();
    }
}
