use crate::{
    accounts::AccountStore,
    transfers::{Engine, Settings, Transfer},
};
use tauri::State;
use tauri::{AppHandle, Emitter};

#[tauri::command]
pub async fn list_transfers(engine: State<'_, Engine>) -> Result<Vec<Transfer>, String> {
    s3_studio_core::transfer_commands::list_transfers(&engine).await
}

#[tauri::command]
pub async fn get_transfer_settings(engine: State<'_, Engine>) -> Result<Settings, String> {
    s3_studio_core::transfer_commands::get_transfer_settings(&engine).await
}

#[tauri::command]
pub async fn set_transfer_settings(
    app: AppHandle,
    engine: State<'_, Engine>,
    concurrency: usize,
) -> Result<Settings, String> {
    let settings =
        s3_studio_core::transfer_commands::set_transfer_settings(&engine, concurrency).await?;
    let _ = app.emit("transfer-settings", &settings);
    Ok(settings)
}

#[tauri::command]
pub async fn pause_transfer(engine: State<'_, Engine>, transfer_id: String) -> Result<(), String> {
    s3_studio_core::transfer_commands::pause_transfer(&engine, transfer_id).await
}

#[tauri::command]
pub async fn resume_transfer(
    engine: State<'_, Engine>,
    store: State<'_, AccountStore>,
    transfer_id: String,
) -> Result<(), String> {
    s3_studio_core::transfer_commands::resume_transfer(&engine, &store, transfer_id).await
}

#[tauri::command]
pub async fn cancel_transfer(
    engine: State<'_, Engine>,
    store: State<'_, AccountStore>,
    transfer_id: String,
) -> Result<(), String> {
    s3_studio_core::transfer_commands::cancel_transfer(&engine, &store, transfer_id).await
}

#[tauri::command]
pub async fn clear_finished_transfers(engine: State<'_, Engine>) -> Result<(), String> {
    s3_studio_core::transfer_commands::clear_finished_transfers(&engine).await
}

#[tauri::command]
pub async fn upload_files(
    engine: State<'_, Engine>,
    store: State<'_, AccountStore>,
    account_id: String,
    bucket: String,
    prefix: String,
) -> Result<usize, String> {
    s3_studio_core::transfer_commands::upload_files(&engine, &store, account_id, bucket, prefix)
        .await
}

#[tauri::command]
pub async fn download_object(
    engine: State<'_, Engine>,
    store: State<'_, AccountStore>,
    account_id: String,
    bucket: String,
    key: String,
) -> Result<bool, String> {
    s3_studio_core::transfer_commands::download_object(&engine, &store, account_id, bucket, key)
        .await
}
