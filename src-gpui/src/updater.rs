// Host adaptation of src-tauri/src/updater.rs; update policy and installer engine are unchanged.
use crate::backend::Message;
use futures::channel::mpsc::UnboundedSender;
use s3_studio_core::transfers::Engine;
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::utils::config::BundleType;
use tauri_plugin_updater::{Update, UpdaterBuilder};

const MANIFEST: &str = "https://github.com/eyotang/s3studio/releases/latest/download/latest.json";
const RELEASES: &str = "https://github.com/eyotang/s3studio/releases";
const MAX_MANIFEST: usize = 128 * 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub revision: u64,
    pub mode: &'static str,
    pub phase: &'static str,
    pub current_version: String,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub last_checked: Option<u64>,
    pub error: Option<String>,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            revision: 0,
            mode: "disabled",
            phase: "disabled",
            current_version: env!("CARGO_PKG_VERSION").into(),
            version: None,
            notes: None,
            downloaded: 0,
            total: None,
            last_checked: None,
            error: None,
        }
    }
}
impl Status {
    pub fn installing(&self) -> bool {
        matches!(self.phase, "downloading" | "installing")
    }
}
pub struct UpdaterState {
    status: Mutex<Status>,
    sender: UnboundedSender<Message>,
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
    pub fn new(preview: bool, sender: UnboundedSender<Message>) -> Arc<Self> {
        let mode = mode(
            preview || cfg!(debug_assertions),
            tauri::utils::platform::bundle_type(),
        );
        Arc::new(Self {
            sender,
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
    pub fn snapshot(&self) -> Status {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn edit(&self, edit: impl FnOnce(&mut Status)) -> Status {
        let mut state = self.status.lock().unwrap_or_else(|e| e.into_inner());
        edit(&mut state);
        state.revision += 1;
        let status = state.clone();
        let _ = self.sender.unbounded_send(Message::Update(status.clone()));
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
pub async fn check(state: Arc<UpdaterState>, user_initiated: bool) -> Result<Status, String> {
    let _operation = state
        .operation
        .try_lock()
        .map_err(|_| "更新操作正在进行中。")?;
    let previous = state.snapshot();
    if previous.mode == "disabled" {
        return Ok(previous);
    }
    state.edit(|status| {
        status.phase = "checking";
        status.error = None;
    });
    let result: Result<Option<Manifest>, String> = async {
        if previous.mode == "inApp" {
            let config: serde_json::Value =
                serde_json::from_str(include_str!("../../src-tauri/tauri.conf.json"))
                    .map_err(|e| e.to_string())?;
            let config = serde_json::from_value(config["plugins"]["updater"].clone())
                .map_err(|e| e.to_string())?;
            let update =
                UpdaterBuilder::standalone(stable_version(&previous.current_version)?, config)
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
                *state.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(update);
                Ok(Some(manifest))
            } else {
                *state.pending.lock().unwrap_or_else(|e| e.into_inner()) = None;
                Ok(None)
            }
        } else {
            manual_check(&previous.current_version).await
        }
    }
    .await;
    match result {
        Ok(available) => Ok(state.edit(|status| {
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
            state.edit(|status| {
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
pub async fn schedule(state: Arc<UpdaterState>) {
    if state.snapshot().mode == "disabled" {
        return;
    }
    tokio::time::sleep(Duration::from_secs(20)).await;
    loop {
        let _ = check(state.clone(), false).await;
        tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
    }
}
pub async fn install_app_update(state: Arc<UpdaterState>, engine: &Engine) -> Result<(), String> {
    let _operation = state
        .operation
        .try_lock()
        .map_err(|_| "更新操作正在进行中。")?;
    if state.snapshot().mode != "inApp" {
        return Err("当前安装方式需从发布页下载更新。".into());
    }
    let guard = engine.reserve_for_update().await?;
    let update = state
        .pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or("请先检查可用更新。")?;
    state.edit(|status| {
        status.phase = "downloading";
        status.downloaded = 0;
        status.total = None;
        status.error = None;
    });
    let progress_state = state.clone();
    let result: Result<(), String> = async {
        let bytes = update
            .download(
                move |length, total| {
                    progress_state.edit(|status| {
                        status.downloaded = status.downloaded.saturating_add(length as u64);
                        status.total = total;
                    });
                },
                || {},
            )
            .await
            .map_err(|e| e.to_string())?;
        state.edit(|status| status.phase = "installing");
        tokio::task::spawn_blocking(move || update.install(bytes))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        state.edit(|status| {
            status.phase = "available";
            status.error = Some(format!("更新未完成，当前版本保持不变：{error}"));
        });
        return Err(error);
    }
    let _ = state.sender.unbounded_send(Message::Restart(guard));
    Ok(())
}
pub fn download_page(status: &Status) -> Result<String, String> {
    if let Some(version) = &status.version {
        stable_version(version)?;
        Ok(format!("{RELEASES}/tag/v{version}"))
    } else {
        Ok(format!("{RELEASES}/latest"))
    }
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
        assert!(
            parse_manifest(br#"{"version":"0.2.3"}"#, "0.2.3")
                .unwrap()
                .is_none()
        );
        assert!(
            parse_manifest(br#"{"version":"0.2.2"}"#, "0.2.3")
                .unwrap()
                .is_none()
        );
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
    #[test]
    fn release_links_accept_only_stable_versions() {
        let mut status = Status::default();
        assert_eq!(
            download_page(&status).unwrap(),
            format!("{RELEASES}/latest")
        );
        status.version = Some("0.3.1".into());
        assert_eq!(
            download_page(&status).unwrap(),
            format!("{RELEASES}/tag/v0.3.1")
        );
        status.version = Some("../other".into());
        assert!(download_page(&status).is_err());
    }
    #[test]
    fn preview_check_never_contacts_update_service() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (sender, _) = futures::channel::mpsc::unbounded();
        let state = UpdaterState::new(true, sender);
        let status = runtime.block_on(check(state, true)).unwrap();
        assert_eq!(status.phase, "disabled");
        assert_eq!(status.revision, 0);
        assert!(status.last_checked.is_none());
    }
    #[test]
    #[ignore = "Reads the official release manifest over HTTPS"]
    fn official_release_manifest_is_readable() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(manual_check(env!("CARGO_PKG_VERSION")))
            .unwrap();
    }
}
