use super::*;
use crate::{
    backend::{Backend, Message},
    model::{Sort, bytes, object_name, visible},
    updater::Status,
    workspace::Store,
};
use futures::channel::mpsc::UnboundedSender;
use nsh_core::platform::{Idiom, Os};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use std::{sync::mpsc, time::Duration};

fn desktop_config(root: Root) -> EngineConfig {
    let mut config = EngineConfig::new(root);
    config.viewport = nsh_core::element::size(px(1280.0), px(860.0));
    config.environment.idiom = Idiom::Desktop;
    config.environment.os = if cfg!(target_os = "macos") {
        Os::MacOs
    } else if cfg!(windows) {
        Os::Windows
    } else {
        Os::Linux
    };
    config
}

fn preview() -> Headless {
    let mut app = Headless::start(desktop_config(studio_root(true))).unwrap();
    app.wait_for("id:object-Brand assets/").unwrap();
    app
}

const WAIT: Duration = Duration::from_secs(5);

#[test]
fn search_selection_grid_and_delete_confirmation() {
    let mut app = preview();
    app.type_text("id:object-search", "guidelines").unwrap();
    app.wait_for("id:object-Brand guidelines.pdf").unwrap();
    app.wait_for_gone("id:object-Brand assets/", WAIT).unwrap();
    app.press("id:object-check-Brand guidelines.pdf").unwrap();
    assert!(app.screen().enabled("id:copy-selection").unwrap());
    app.press("id:grid-view").unwrap();
    app.wait_for("id:card-Brand guidelines.pdf").unwrap();
    app.press("id:list-view").unwrap();
    app.press("id:delete-selection").unwrap();
    app.wait_for("id:delete-list").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:delete-list", WAIT).unwrap();
    app.press("id:clear-search").unwrap();
    app.wait_for("id:object-Brand assets/").unwrap();
}

#[test]
fn switches_buckets_and_shows_details_for_the_selected_object() {
    let mut app = preview();
    app.press("id:bucket-application-backups").unwrap();
    app.wait_for("id:object-Brand assets/").unwrap();
    app.type_text("id:object-search", "guidelines").unwrap();
    app.wait_for("id:more-Brand guidelines.pdf").unwrap();
    app.press("id:more-Brand guidelines.pdf").unwrap();
    app.wait_for("application/octet-stream").unwrap();
    assert!(app.screen().enabled("id:inspector-download").unwrap());
    app.press("id:close-info").unwrap();
    app.wait_for_gone("id:object-inspector", WAIT).unwrap();
}

#[test]
fn account_form_preserves_drafts_and_requires_credentials() {
    let mut app = preview();
    app.press("id:add-account").unwrap();
    app.wait_for("id:name").unwrap();
    app.type_text("id:name", "Unsaved account").unwrap();
    app.press("id:pathStyle").unwrap();
    assert_eq!(
        app.screen().input_text("id:name").unwrap(),
        Some("Unsaved account")
    );
    app.press("id:submit-dialog").unwrap();
    app.wait_for(i18n::t("请填写完整的账户名称、Access Key 和 Secret Key。"))
        .unwrap();
    app.type_text("id:name", " draft").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:name", WAIT).unwrap();
    app.press("id:account-settings").unwrap();
    assert_eq!(
        app.screen().input_text("id:name").unwrap(),
        Some("Design workspace")
    );
    assert_eq!(app.screen().input_text("id:accessKey").unwrap(), Some(""));
    app.press("id:remove-account").unwrap();
    app.wait_for(i18n::t("移除这个账户？")).unwrap();
    app.press("id:cancel-dialog").unwrap();
}

#[test]
fn bucket_options_can_be_added_reused_and_removed() {
    let mut app = preview();
    app.press("id:create-bucket").unwrap();
    app.type_text("id:region", "eu-west-2").unwrap();
    app.press("id:add-s3-option-region").unwrap();
    app.wait_for_gone("id:add-s3-option-region", WAIT).unwrap();
    app.press("id:cancel-dialog").unwrap();

    app.press("id:bucket-settings").unwrap();
    app.press("id:s3-options-region").unwrap();
    app.wait_for("eu-west-2").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for_gone("eu-west-2", WAIT).unwrap();
    app.press("id:s3-options-region").unwrap();
    app.press("eu-west-2").unwrap();
    assert_eq!(
        app.screen().input_text("id:region").unwrap(),
        Some("eu-west-2")
    );
    app.press("id:cancel-dialog").unwrap();

    app.press("id:app-settings").unwrap();
    for id in [
        "s3-defaults",
        "settings-language-system",
        "settings-language-zh",
        "settings-language-en",
        "settings-language-ko",
        "settings-language-ja",
        "settings-language-fr",
        "settings-language-de",
        "settings-language-ru",
        "theme-system",
        "theme-light",
        "theme-dark",
        "concurrency",
    ] {
        app.wait_for(&format!("id:{id}")).unwrap();
    }
    app.press("id:s3-defaults").unwrap();
    app.press("id:delete-s3-option-region-eu-west-2").unwrap();
    app.press("id:submit-dialog").unwrap();
    app.wait_for(i18n::t("设置保存成功")).unwrap();
}

struct Events(mpsc::Sender<UnboundedSender<Message>>);

#[component]
fn TestWorkspace() -> impl IntoElement {
    let (backend, receiver) = Backend::open(true).unwrap();
    engine_context::<Events>()
        .unwrap()
        .0
        .send(backend.event_sender())
        .unwrap();
    ui::workspace(Store::new(backend, receiver))
}

#[test]
fn update_dialog_tracks_progress_and_blocks_install_during_transfers() {
    let (events, receiver) = mpsc::channel();
    let mut config = desktop_config(Root::of::<TestWorkspace>());
    config.context = config.context.with(Events(events));
    let mut app = Headless::start(config).unwrap();
    let sender = receiver.recv_timeout(WAIT).unwrap();
    app.wait_for("id:object-Brand assets/").unwrap();
    app.scroll("id:workspace-body", px(500.0)).unwrap();
    app.press("id:update").unwrap();
    app.wait_for("id:check-update").unwrap();
    assert!(!app.screen().enabled("id:check-update").unwrap());
    let available = Status {
        revision: 10,
        mode: "inApp",
        phase: "available",
        version: Some("0.4.9".into()),
        notes: Some("Release notes".into()),
        ..Default::default()
    };
    let mut transfer: s3_studio_core::transfers::Transfer =
        serde_json::from_value(serde_json::json!({
            "id":"example", "name":"archive.zip", "accountId":"demo-aws",
            "bucket":"studio-assets", "key":"archive.zip", "direction":"upload",
            "status":"running", "bytes":512, "total":1024
        }))
        .unwrap();
    sender
        .unbounded_send(Message::Transfer(transfer.clone()))
        .unwrap();
    sender
        .unbounded_send(Message::Update(available.clone()))
        .unwrap();
    app.wait_for("id:install-update").unwrap();
    assert!(!app.screen().enabled("id:install-update").unwrap());
    transfer.status = "paused".into();
    sender.unbounded_send(Message::Transfer(transfer)).unwrap();
    app.wait_for_gone(i18n::t("请先暂停或完成所有传输，再安装更新。"), WAIT)
        .unwrap();
    assert!(app.screen().enabled("id:install-update").unwrap());
    sender
        .unbounded_send(Message::Update(Status {
            revision: 9,
            ..Default::default()
        }))
        .unwrap();
    sender
        .unbounded_send(Message::Update(Status {
            revision: 11,
            phase: "downloading",
            downloaded: 512,
            total: Some(1024),
            ..available
        }))
        .unwrap();
    app.wait_for("512 B / 1 KB").unwrap();
    assert!(!app.screen().enabled("id:close-dialog").unwrap());
    assert!(!app.screen().enabled("id:install-update").unwrap());
}

#[test]
fn formatting_and_directory_priority_match_existing_behavior() {
    assert_eq!(bytes(0), "0 B");
    assert_eq!(bytes(1024), "1 KB");
    assert_eq!(bytes(1280), "1.3 KB");
    assert_eq!(bytes(2488000), "2.4 MB");
    assert_eq!(bytes(1024_u64.pow(4)), "1 TB");
    assert_eq!(object_name("design/Archive/", "design/"), "Archive");
    assert_eq!(object_name("design/Archive//", "design/"), "Archive/");
    let objects: Vec<s3_studio_core::s3::S3Object> = serde_json::from_value(serde_json::json!([
        {"key":"file10","size":1,"modified":"2026-10-03T08:00:00Z","storageClass":"STANDARD","folder":false},
        {"key":"Archive/","size":0,"modified":"","storageClass":"STANDARD","folder":true},
        {"key":"file2","size":2,"modified":"2026-10-01T08:00:00Z","storageClass":"STANDARD","folder":false}
    ]))
    .unwrap();
    let result = visible(&objects, "", Sort::Name, true);
    assert_eq!(
        result.iter().map(|o| o.key.as_str()).collect::<Vec<_>>(),
        vec!["Archive/", "file2", "file10"]
    );
    let descending = visible(&objects, "", Sort::Size, false);
    assert_eq!(
        descending.iter().map(|o| o.size).collect::<Vec<_>>(),
        vec![0, 2, 1]
    );
    assert_eq!(
        visible(&objects, "", Sort::Modified, false)[1].key,
        "file10"
    );
    assert_eq!(visible(&objects, "FILE", Sort::Size, true)[0].key, "file10");
    assert_eq!(visible(&objects, "ARCHIVE", Sort::Name, true).len(), 1);
    assert!(visible(&objects, "missing", Sort::Name, true).is_empty());
    assert_eq!(objects[0].key, "file10");
}
