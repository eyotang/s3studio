use crate::accounts::AccountStore;
pub use s3_studio_core::s3::*;
use tauri::State;

#[tauri::command]
pub async fn list_buckets(
    store: State<'_, AccountStore>,
    account_id: String,
) -> Result<Vec<Bucket>, String> {
    s3_studio_core::s3::list_buckets(&store, account_id).await
}

#[tauri::command]
pub async fn list_objects(
    store: State<'_, AccountStore>,
    account_id: String,
    bucket: String,
    prefix: String,
    token: Option<String>,
) -> Result<ObjectPage, String> {
    s3_studio_core::s3::list_objects(&store, account_id, bucket, prefix, token).await
}

#[tauri::command]
pub async fn create_bucket(
    store: State<'_, AccountStore>,
    account_id: String,
    name: String,
) -> Result<(), String> {
    s3_studio_core::s3::create_bucket(&store, account_id, name).await
}

#[tauri::command]
pub async fn create_folder(
    store: State<'_, AccountStore>,
    account_id: String,
    bucket: String,
    key: String,
) -> Result<(), String> {
    s3_studio_core::s3::create_folder(&store, account_id, bucket, key).await
}

#[tauri::command]
pub async fn delete_objects(
    store: State<'_, AccountStore>,
    account_id: String,
    bucket: String,
    keys: Vec<String>,
    folders: Vec<String>,
) -> Result<usize, String> {
    s3_studio_core::s3::delete_objects(&store, account_id, bucket, keys, folders).await
}

#[tauri::command]
pub async fn copy_object(
    store: State<'_, AccountStore>,
    account_id: String,
    bucket: String,
    key: String,
    destination: String,
) -> Result<(), String> {
    s3_studio_core::s3::copy_object(&store, account_id, bucket, key, destination).await
}

#[tauri::command]
pub async fn object_info(
    store: State<'_, AccountStore>,
    account_id: String,
    bucket: String,
    key: String,
) -> Result<ObjectInfo, String> {
    s3_studio_core::s3::object_info(&store, account_id, bucket, key).await
}

#[tauri::command]
pub async fn presign_object(
    store: State<'_, AccountStore>,
    account_id: String,
    bucket: String,
    key: String,
    expires: u64,
) -> Result<String, String> {
    s3_studio_core::s3::presign_object(&store, account_id, bucket, key, expires).await
}
