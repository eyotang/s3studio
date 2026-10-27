pub use s3_studio_core::accounts::*;
use tauri::State;

#[tauri::command]
pub async fn list_accounts(store: State<'_, AccountStore>) -> Result<Vec<Account>, String> {
    s3_studio_core::accounts::list_accounts(&store).await
}

#[tauri::command]
pub async fn save_account(
    store: State<'_, AccountStore>,
    input: AccountInput,
) -> Result<Account, String> {
    s3_studio_core::accounts::save_account(&store, input).await
}

#[tauri::command]
pub async fn remove_account(
    store: State<'_, AccountStore>,
    engine: State<'_, crate::transfers::Engine>,
    account_id: String,
) -> Result<(), String> {
    s3_studio_core::accounts::remove_account(&store, &engine, account_id).await
}
