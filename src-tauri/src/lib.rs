mod accounts;
mod s3;
mod transfer_commands;
mod transfers;

use accounts::AccountStore;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            app.manage(AccountStore::open(dir.join("accounts.json"))?);
            let handle = app.handle().clone();
            app.manage(transfers::Engine::open(
                dir.join("transfers.json"),
                std::sync::Arc::new(move |transfer| {
                    let _ = handle.emit("transfer", transfer);
                }),
            )?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            accounts::list_accounts,
            accounts::save_account,
            accounts::remove_account,
            s3::list_buckets,
            s3::list_objects,
            s3::create_bucket,
            s3::create_folder,
            s3::delete_objects,
            s3::copy_object,
            s3::object_info,
            s3::presign_object,
            transfer_commands::upload_files,
            transfer_commands::download_object,
            transfer_commands::list_transfers,
            transfer_commands::get_transfer_settings,
            transfer_commands::set_transfer_settings,
            transfer_commands::pause_transfer,
            transfer_commands::resume_transfer,
            transfer_commands::cancel_transfer,
            transfer_commands::clear_finished_transfers,
        ])
        .run(tauri::generate_context!())
        .expect("无法启动 S3 Studio");
}
