use super::failure;
use crate::i18n::t;
use aws_sdk_s3::Client;
use base64::Engine as _;
use bytes::Bytes;
use parquet::{
    errors::ParquetError,
    file::reader::{ChunkReader, FileReader, Length, SerializedFileReader},
};
use serde::{Deserialize, Serialize};
use std::io::{self, Cursor, Read};
use tokio::io::AsyncReadExt;

const PAGE: u64 = 8192;
const ROWS: usize = 60;
const MAX_IMAGE: u64 = 48 * 1024 * 1024;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Preview {
    pub text: String,
    pub image: Option<String>,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub next: bool,
}

#[derive(Clone)]
struct ObjectReader {
    client: Client,
    bucket: String,
    key: String,
    etag: String,
    version: Option<String>,
    size: u64,
    runtime: tokio::runtime::Handle,
}
impl ObjectReader {
    async fn range(&self, start: u64, len: u64) -> Result<Bytes, String> {
        if start.checked_add(len).is_none_or(|end| end > self.size) {
            return Err(t("范围无效").into());
        }
        if len == 0 {
            return Ok(Bytes::new());
        }
        let read = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .if_match(&self.etag)
            .set_version_id(self.version.clone())
            .range(format!("bytes={start}-{}", start + len - 1))
            .send()
            .await
            .map_err(failure)?;
        if read.content_length() != Some(len as i64) {
            return Err(t("对象大小无效。").into());
        }
        let mut data = Vec::new();
        read.body
            .into_async_read()
            .take(len + 1)
            .read_to_end(&mut data)
            .await
            .map_err(failure)?;
        if data.len() as u64 != len {
            return Err(t("对象大小无效。").into());
        }
        Ok(data.into())
    }
}
impl Length for ObjectReader {
    fn len(&self) -> u64 {
        self.size
    }
}
struct RangeStream {
    object: ObjectReader,
    next: u64,
    buffer: Cursor<Bytes>,
}
impl Read for RangeStream {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if self.buffer.position() == self.buffer.get_ref().len() as u64 {
            if self.next >= self.object.size {
                return Ok(0);
            }
            let size = (256 * 1024).min(self.object.size - self.next);
            let bytes = self
                .object
                .runtime
                .block_on(self.object.range(self.next, size))
                .map_err(io::Error::other)?;
            self.next += size;
            self.buffer = Cursor::new(bytes);
        }
        Read::read(&mut self.buffer, out)
    }
}
impl ChunkReader for ObjectReader {
    type T = RangeStream;
    fn get_read(&self, start: u64) -> parquet::errors::Result<Self::T> {
        Ok(RangeStream {
            object: self.clone(),
            next: start,
            buffer: Cursor::new(Bytes::new()),
        })
    }
    fn get_bytes(&self, start: u64, length: usize) -> parquet::errors::Result<Bytes> {
        if length > 128 * 1024 * 1024 {
            return Err(ParquetError::General(
                t("表格行组过大，请下载源文件").into(),
            ));
        }
        self.runtime
            .block_on(self.range(start, length as u64))
            .map_err(ParquetError::General)
    }
}

pub async fn read(
    client: &Client,
    bucket: &str,
    key: &str,
    page: u64,
    hex: bool,
) -> Result<Preview, String> {
    let head = client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(failure)?;
    let size = head
        .content_length()
        .filter(|n| *n >= 0)
        .ok_or(t("对象大小无效。"))? as u64;
    if size == 0 {
        return Ok(Preview::default());
    }
    let object = ObjectReader {
        client: client.clone(),
        bucket: bucket.into(),
        key: key.into(),
        size,
        etag: head.e_tag().ok_or(t("服务端分片缺少 ETag。"))?.into(),
        version: head.version_id().map(str::to_owned),
        runtime: tokio::runtime::Handle::current(),
    };
    let ext = key.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if !hex && matches!(ext.as_str(), "csv" | "parquet") {
        return tokio::task::spawn_blocking(move || table(object, &ext, page))
            .await
            .map_err(failure)?;
    }
    let image = !hex
        && matches!(
            ext.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
        );
    if image && size > MAX_IMAGE {
        return Err(t("图片预览上限为 48 MiB，可下载源文件或切换十六进制").into());
    }
    let start = if image {
        0
    } else {
        page.checked_mul(PAGE).ok_or(t("范围无效"))?
    };
    if start >= size {
        return Err(t("范围无效").into());
    }
    let length = if image { size } else { PAGE.min(size - start) };
    let data = object.range(start, length).await?;
    let mut out = Preview {
        next: start + length < size,
        ..Preview::default()
    };
    if image {
        return tokio::task::spawn_blocking(move || thumbnail(data))
            .await
            .map_err(failure)?;
    } else if hex {
        out.text = data
            .chunks(16)
            .enumerate()
            .map(|(i, chunk)| {
                format!(
                    "{:08x}  {}",
                    start + i as u64 * 16,
                    chunk
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    } else {
        out.text = String::from_utf8_lossy(&data).into_owned();
        if ext == "json" && start == 0 && length == size {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&data) {
                out.text = serde_json::to_string_pretty(&v).map_err(failure)?;
            }
        }
    }
    Ok(out)
}

fn thumbnail(data: Bytes) -> Result<Preview, String> {
    let mut reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(failure)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(failure)?;
    let mut png = Cursor::new(Vec::new());
    img.thumbnail(700, 460)
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(failure)?;
    Ok(Preview {
        text: format!("{} × {}", img.width(), img.height()),
        image: Some(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png.into_inner())
        )),
        ..Preview::default()
    })
}
fn cell(text: &str) -> String {
    text.chars().take(512).collect()
}
fn table(object: ObjectReader, ext: &str, page: u64) -> Result<Preview, String> {
    let mut out = Preview::default();
    let mut skip = usize::try_from(page)
        .ok()
        .and_then(|p| p.checked_mul(ROWS))
        .ok_or(t("范围无效"))?;
    if ext == "csv" {
        let source = object.get_read(0).map_err(failure)?;
        // Bound memory for malformed CSV with an unterminated quoted record.
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(RecordLimit {
                inner: source,
                remaining: 8 * 1024 * 1024,
            });
        out.columns = reader
            .headers()
            .map_err(failure)?
            .iter()
            .take(32)
            .map(cell)
            .collect();
        for index in 0..skip.saturating_add(ROWS + 1) {
            reader.get_mut().remaining = 8 * 1024 * 1024;
            let mut record = csv::StringRecord::new();
            if !reader.read_record(&mut record).map_err(failure)? {
                break;
            }
            if index < skip {
                continue;
            }
            if out.rows.len() == ROWS {
                out.next = true;
                break;
            }
            out.rows.push(record.iter().take(32).map(cell).collect());
        }
    } else {
        let reader = SerializedFileReader::new(object).map_err(failure)?;
        for index in 0..reader.num_row_groups() {
            let group = reader.get_row_group(index).map_err(failure)?;
            let count = usize::try_from(group.metadata().num_rows()).map_err(failure)?;
            if skip >= count {
                skip -= count;
                continue;
            }
            if group.metadata().total_byte_size() > 128 * 1024 * 1024 {
                return Err(t("表格行组过大，请下载源文件").into());
            }
            for row in group.get_row_iter(None).map_err(failure)?.skip(skip) {
                if out.rows.len() == ROWS {
                    out.next = true;
                    return Ok(out);
                }
                let row = row.map_err(failure)?;
                if out.columns.is_empty() {
                    out.columns = row
                        .get_column_iter()
                        .take(32)
                        .map(|(k, _)| cell(k))
                        .collect();
                }
                out.rows.push(
                    row.get_column_iter()
                        .take(32)
                        .map(|(_, v)| cell(&v.to_string()))
                        .collect(),
                );
            }
            skip = 0;
        }
    }
    Ok(out)
}
struct RecordLimit<R> {
    inner: R,
    remaining: usize,
}
impl<R: Read> Read for RecordLimit<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other(t("表格行组过大，请下载源文件")));
        }
        let max = buf.len().min(self.remaining);
        let n = self.inner.read(&mut buf[..max])?;
        self.remaining -= n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use tiny_http::{Header, Response, Server};

    async fn preview_page(name: &str, data: Vec<u8>, page: u64, hex: bool) -> Preview {
        let server = Server::http("127.0.0.1:0").unwrap();
        let client = crate::s3::tests::test_client(format!("http://{}", server.server_addr()));
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let worker = std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let Some(req) = server
                    .recv_timeout(std::time::Duration::from_millis(50))
                    .unwrap()
                else {
                    continue;
                };
                let etag = Header::from_bytes("ETag", "\"preview\"").unwrap();
                if req.method().as_str() == "HEAD" {
                    req.respond(Response::from_data(data.clone()).with_header(etag))
                        .unwrap();
                } else {
                    assert!(req
                        .headers()
                        .iter()
                        .any(|h| h.field.equiv("If-Match") && h.value.as_str() == "\"preview\""));
                    let range = req
                        .headers()
                        .iter()
                        .find(|h| h.field.equiv("Range"))
                        .unwrap()
                        .value
                        .as_str()
                        .strip_prefix("bytes=")
                        .unwrap();
                    let (a, b) = range.split_once('-').unwrap();
                    let (a, b) = (a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap());
                    req.respond(
                        Response::from_data(data[a..=b].to_vec())
                            .with_status_code(206)
                            .with_header(etag),
                    )
                    .unwrap();
                }
            }
        });
        let result = read(&client, "bucket", name, page, hex).await;
        stop.store(true, Ordering::Relaxed);
        worker.join().unwrap();
        result.unwrap()
    }
    #[tokio::test]
    async fn renders_csv_quotes_parquet_rows_and_hex_pages() {
        let csv = preview_page("rows.csv", b"id,name\n1,\"Doe, Jane\"\n".to_vec(), 0, false).await;
        assert_eq!(csv.columns, ["id", "name"]);
        assert_eq!(csv.rows[0], ["1", "Doe, Jane"]);
        let mut parquet = Vec::new();
        {
            let schema = parquet::schema::parser::parse_message_type(
                "message test { REQUIRED INT32 count; }",
            )
            .unwrap();
            let mut writer = parquet::file::writer::SerializedFileWriter::new(
                &mut parquet,
                Arc::new(schema),
                Default::default(),
            )
            .unwrap();
            let mut group = writer.next_row_group().unwrap();
            let mut column = group.next_column().unwrap().unwrap();
            column
                .typed::<parquet::data_type::Int32Type>()
                .write_batch(&[10, 20], None, None)
                .unwrap();
            column.close().unwrap();
            group.close().unwrap();
            writer.close().unwrap();
        }
        let table = preview_page("rows.parquet", parquet, 0, false).await;
        assert_eq!(table.columns, ["count"]);
        assert_eq!(table.rows[1], ["20"]);
        let hex = preview_page("object.bin", vec![0xab; 8193], 1, true).await;
        assert!(hex.text.contains("00002000  ab"));
        assert!(!hex.next);
    }
}
