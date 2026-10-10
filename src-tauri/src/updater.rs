use crate::transfers::Engine;
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{utils::config::BundleType, AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::{Update, UpdaterExt};

const MANIFEST: &str = "https://github.com/eyotang/s3studio/releases/latest/download/latest.json";
const RELEASES: &str = "https://github.com/eyotang/s3studio/releases";
const MAX_MANIFEST: usize = 128 * 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    revision: u64,
    mode: &'static str,
    phase: &'static str,
    current_version: String,
    version: Option<String>,
    notes: Option<String>,
    downloaded: u64,
    total: Option<u64>,
    last_checked: Option<u64>,
    error: Option<String>,
}
pub struct UpdaterState {
    status: Mutex<Status>,
    pending: Mutex<Option<Update>>,
    operation: tokio::sync::Mutex<()>,
}
fn mode(debug: bool, bundle: Option<BundleType>) -> &'static str {
    if debug {
        return "disabled";
    }
    match bundle {
        Some(BundleType::Nsis | BundleType::Msi | BundleType::AppImage) => "inApp",
        _ => "manual",
    }
}
impl UpdaterState {
    pub fn new() -> Arc<Self> {
        let mode = mode(
            cfg!(debug_assertions),
            tauri::utils::platform::bundle_type(),
        );
        Arc::new(Self {
            status: Mutex::new(Status {
                revision: 0,
                mode,
                phase: if mode == "disabled" {
                    "disabled"
                } else {
                    "idle"
                },
                current_version: env!("CARGO_PKG_VERSION").into(),
                version: None,
                notes: None,
                downloaded: 0,
                total: None,
                last_checked: None,
                error: None,
            }),
            pending: Mutex::new(None),
            operation: tokio::sync::Mutex::new(()),
        })
    }
    fn snapshot(&self) -> Status {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn edit(&self, app: &AppHandle, edit: impl FnOnce(&mut Status)) -> Status {
        let mut state = self.status.lock().unwrap_or_else(|e| e.into_inner());
        edit(&mut state);
        state.revision += 1;
        let status = state.clone();
        let _ = app.emit("app-update", &status);
        status
    }
}
fn stable_version(value: &str) -> Result<semver::Version, String> {
    let version = semver::Version::parse(value).map_err(|_| "更新版本号无效。")?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err("仅接受正式版本更新。".into());
    }
    Ok(version)
}
#[derive(Deserialize)]
struct Manifest {
    version: String,
    notes: Option<String>,
}
fn parse_manifest(bytes: &[u8], current: &str) -> Result<Option<Manifest>, String> {
    if bytes.len() > MAX_MANIFEST {
        return Err("更新清单过大。".into());
    }
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|_| "更新清单格式无效。")?;
    let newer = stable_version(&manifest.version)? > stable_version(current)?;
    if manifest
        .notes
        .as_ref()
        .is_some_and(|notes| notes.len() > 32 * 1024)
    {
        return Err("更新说明过长。".into());
    }
    Ok(newer.then_some(manifest))
}
async fn manual_check(current: &str) -> Result<Option<Manifest>, String> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(concat!("S3-Studio/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client
        .get(MANIFEST)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.to_string())?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_MANIFEST as u64)
    {
        return Err("更新清单过大。".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if bytes.len() + chunk.len() > MAX_MANIFEST {
            return Err("更新清单过大。".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_manifest(&bytes, current)
}
async fn check(app: AppHandle, user_initiated: bool) -> Result<Status, String> {
    let state = app.state::<Arc<UpdaterState>>().inner().clone();
    let _operation = state
        .operation
        .try_lock()
        .map_err(|_| "更新操作正在进行中。")?;
    let previous = state.snapshot();
    if previous.mode == "disabled" {
        return Ok(previous);
    }
    state.edit(&app, |status| {
        status.phase = "checking";
        status.error = None;
    });
    let result: Result<Option<Manifest>, String> = async {
        if previous.mode == "inApp" {
            let update = app
                .updater_builder()
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|e| e.to_string())?
                .check()
                .await
                .map_err(|e| e.to_string())?;
            if let Some(mut update) = update {
                stable_version(&update.version)?;
                let allowed = format!("{RELEASES}/download/v{}/", update.version);
                if !update.download_url.as_str().starts_with(&allowed) {
                    return Err("更新包地址不属于官方发布仓库。".into());
                }
                update.timeout = Some(Duration::from_secs(5 * 60));
                let manifest = Manifest {
                    version: update.version.clone(),
                    notes: update.body.clone(),
                };
                *state.pending.lock().unwrap() = Some(update);
                Ok(Some(manifest))
            } else {
                *state.pending.lock().unwrap() = None;
                Ok(None)
            }
        } else {
            manual_check(&previous.current_version).await
        }
    }
    .await;
    match result {
        Ok(available) => Ok(state.edit(&app, |status| {
            status.phase = if available.is_some() {
                "available"
            } else {
                "idle"
            };
            status.version = available.as_ref().map(|manifest| manifest.version.clone());
            status.notes = available.and_then(|manifest| manifest.notes);
            status.last_checked = Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            );
            status.error = None;
        })),
        Err(error) => {
            state.edit(&app, |status| {
                status.phase = previous.phase;
                status.error = if user_initiated {
                    Some(format!("暂时无法检查更新：{error}"))
                } else {
                    None
                };
            });
            if user_initiated {
                Err(format!("暂时无法检查更新：{error}"))
            } else {
                Ok(state.snapshot())
            }
        }
    }
}
pub fn schedule(app: AppHandle) {
    if app.state::<Arc<UpdaterState>>().snapshot().mode == "disabled" {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            let _ = check(app.clone(), false).await;
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });
}
#[tauri::command]
pub fn get_app_update_status(state: State<'_, Arc<UpdaterState>>) -> Status {
    state.snapshot()
}
#[tauri::command]
pub async fn check_app_update(app: AppHandle) -> Result<Status, String> {
    check(app, true).await
}
#[tauri::command]
pub async fn install_app_update(app: AppHandle) -> Result<(), String> {
    let state = app.state::<Arc<UpdaterState>>().inner().clone();
    let _operation = state
        .operation
        .try_lock()
        .map_err(|_| "更新操作正在进行中。")?;
    if state.snapshot().mode != "inApp" {
        return Err("当前安装方式需从发布页下载更新。".into());
    }
    let _guard = app.state::<Engine>().reserve_for_update().await?;
    let update = state
        .pending
        .lock()
        .unwrap()
        .clone()
        .ok_or("请先检查可用更新。")?;
    state.edit(&app, |status| {
        status.phase = "downloading";
        status.downloaded = 0;
        status.total = None;
        status.error = None;
    });
    let progress_state = state.clone();
    let progress_app = app.clone();
    let result: Result<(), String> = async {
        let bytes = update
            .download(
                move |length, total| {
                    progress_state.edit(&progress_app, |status| {
                        status.downloaded = status.downloaded.saturating_add(length as u64);
                        status.total = total;
                    });
                },
                || {},
            )
            .await
            .map_err(|e| e.to_string())?;
        state.edit(&app, |status| status.phase = "installing");
        tauri::async_runtime::spawn_blocking(move || update.install(bytes))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        state.edit(&app, |status| {
            status.phase = "available";
            status.error = Some(format!("更新未完成，当前版本保持不变：{error}"));
        });
        return Err(error);
    }
    app.restart();
}
#[tauri::command]
pub fn open_update_download_page(app: AppHandle) -> Result<(), String> {
    let status = app.state::<Arc<UpdaterState>>().snapshot();
    let url = if let Some(version) = status.version {
        stable_version(&version)?;
        format!("{RELEASES}/tag/v{version}")
    } else {
        format!("{RELEASES}/latest")
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_accepts_only_new_stable_versions() {
        assert!(
            parse_manifest(br#"{"version":"0.2.4","notes":"New release"}"#, "0.2.3")
                .unwrap()
                .is_some()
        );
        assert!(parse_manifest(br#"{"version":"0.2.3"}"#, "0.2.3")
            .unwrap()
            .is_none());
        assert!(parse_manifest(br#"{"version":"0.2.2"}"#, "0.2.3")
            .unwrap()
            .is_none());
        for version in ["../evil", "v0.2.4", "0.2.4-beta.1", "0.2.4+build"] {
            assert!(stable_version(version).is_err());
        }
        assert!(parse_manifest(&vec![b' '; MAX_MANIFEST + 1], "0.2.3").is_err());
    }
    #[test]
    fn distribution_modes_preserve_installer_compatibility() {
        assert_eq!(mode(true, Some(BundleType::Nsis)), "disabled");
        for bundle in [BundleType::Nsis, BundleType::Msi, BundleType::AppImage] {
            assert_eq!(mode(false, Some(bundle)), "inApp");
        }
        for bundle in [BundleType::App, BundleType::Deb] {
            assert_eq!(mode(false, Some(bundle)), "manual");
        }
        assert_eq!(mode(false, None), "manual");
    }
}
