use crate::{
    accounts::AccountStore,
    s3::bucket_client,
    transfers::{Direction, Engine, Settings, Transfer},
};

pub async fn list_transfers(engine: &Engine) -> Result<Vec<Transfer>, String> {
    Ok(engine.list().await)
}
pub async fn get_transfer_settings(engine: &Engine) -> Result<Settings, String> {
    Ok(engine.settings().await)
}
pub async fn set_transfer_settings(
    engine: &Engine,
    concurrency: usize,
) -> Result<Settings, String> {
    let settings = engine.set_settings(concurrency).await?;
    Ok(settings)
}
pub async fn pause_transfer(engine: &Engine, transfer_id: String) -> Result<(), String> {
    engine.pause(&transfer_id).await
}
pub async fn resume_transfer(
    engine: &Engine,
    store: &AccountStore,
    transfer_id: String,
) -> Result<(), String> {
    let job = engine.job(&transfer_id).await?;
    let client = bucket_client(&store, &job.view.account_id, &job.view.bucket).await?;
    engine.start(&transfer_id, client).await
}
pub async fn cancel_transfer(
    engine: &Engine,
    store: &AccountStore,
    transfer_id: String,
) -> Result<(), String> {
    let job = engine.job(&transfer_id).await?;
    let client = bucket_client(&store, &job.view.account_id, &job.view.bucket).await?;
    engine.cancel(&transfer_id, client).await
}
pub async fn clear_finished_transfers(engine: &Engine) -> Result<(), String> {
    engine.clear_finished().await
}

pub async fn upload_files(
    engine: &Engine,
    store: &AccountStore,
    account_id: String,
    bucket: String,
    prefix: String,
) -> Result<usize, String> {
    let Some(files) = rfd::AsyncFileDialog::new()
        .set_title("选择上传到 S3 的文件")
        .pick_files()
        .await
    else {
        return Ok(0);
    };
    let client = bucket_client(&store, &account_id, &bucket).await?;
    let mut count = 0;
    let mut errors = Vec::new();
    for file in files {
        let result = async {
            let id = engine
                .enqueue(
                    Direction::Upload,
                    account_id.clone(),
                    bucket.clone(),
                    format!("{prefix}{}", file.file_name()),
                    file.path().to_owned(),
                )
                .await?;
            engine.start(&id, client.clone()).await?;
            Ok::<_, String>(())
        }
        .await;
        match result {
            Ok(()) => count += 1,
            Err(error) => errors.push(format!("{}: {error}", file.file_name())),
        }
    }
    if errors.is_empty() {
        Ok(count)
    } else {
        Err(format!(
            "已添加 {count} 个上传任务，部分失败：{}",
            errors.join("；")
        ))
    }
}
pub async fn download_object(
    engine: &Engine,
    store: &AccountStore,
    account_id: String,
    bucket: String,
    key: String,
) -> Result<bool, String> {
    if key.is_empty() || key.ends_with('/') {
        return Err("请选择文件下载。".into());
    }
    let name = key
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .unwrap_or("download");
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "\\/:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("保存 S3 对象")
        .set_file_name(name)
        .save_file()
        .await
    else {
        return Ok(false);
    };
    let client = bucket_client(&store, &account_id, &bucket).await?;
    let id = engine
        .enqueue(
            Direction::Download,
            account_id,
            bucket,
            key,
            file.path().to_owned(),
        )
        .await?;
    engine.start(&id, client).await?;
    Ok(true)
}
