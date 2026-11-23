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
fn search_clear_is_inside_the_field_and_returns_keyboard_focus() {
    let mut app = preview();
    let input = "id:object-search";
    let clear = "id:clear-search";
    assert!(!app.screen().matches("id:locate-path").unwrap());
    assert!(!app.screen().matches(clear).unwrap());
    app.type_text(input, "guidelines").unwrap();
    app.wait_for(clear).unwrap();

    let layout = app.screen().layout_nodes();
    let node = |id| {
        layout
            .iter()
            .find(|node| node.test_id.as_deref() == Some(id))
            .unwrap()
    };
    let field = node("object-search-field");
    let input_node = node("object-search");
    let clear_node = node("clear-search");
    assert_eq!(input_node.parent, field.id);
    assert_eq!(clear_node.parent, field.id);
    assert!(input_node.bounds.right() <= clear_node.bounds.origin.x);
    assert!(field.bounds.contains(&clear_node.bounds.center()));
    assert!(clear_node.bounds.right() < field.bounds.right());

    app.press(clear).unwrap();
    app.wait_for_gone(clear, WAIT).unwrap();
    assert_eq!(app.screen().input_text(input).unwrap(), Some(""));
    app.press_key("w").unwrap();
    assert_eq!(app.screen().input_text(input).unwrap(), Some("w"));
}

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
fn enter_locates_and_selects_an_object_outside_the_first_directory_page() {
    let mut app = preview();
    app.press("id:tree-view").unwrap();
    app.wait_for("id:tree-totals").unwrap();
    let key = "Brand assets/Guides/Usage.pdf";
    app.type_text("id:object-search", key).unwrap();
    app.press_key("enter").unwrap();
    app.wait_for("id:breadcrumb-Brand assets/Guides/").unwrap();
    app.wait_for("id:object-Brand assets/Guides/Typography.txt")
        .unwrap();
    let row = format!("id:object-{key}");
    app.wait_for(&row).unwrap();
    app.wait_for("id:object-inspector").unwrap();
    assert!(app.screen().enabled("id:copy-selection").unwrap());
    assert!(!app.screen().matches("id:tree-objects").unwrap());

    let layout = app.screen().layout_nodes();
    let breadcrumb = |id| {
        layout
            .iter()
            .find(|node| node.test_id.as_deref() == Some(id))
            .unwrap()
            .bounds
    };
    let root = breadcrumb("root-directory");
    let parent = breadcrumb("breadcrumb-Brand assets/");
    let current = breadcrumb("breadcrumb-Brand assets/Guides/");
    assert!(root.right() < parent.origin.x && parent.right() < current.origin.x);
    assert_eq!(root.center().y, parent.center().y);
    assert_eq!(parent.center().y, current.center().y);

    app.press("id:load-more").unwrap();
    app.wait_for_gone("id:load-more", WAIT).unwrap();
    assert_eq!(app.screen().count(&row).unwrap(), 1);
    assert!(app.screen().enabled("id:copy-selection").unwrap());
}

#[test]
fn search_opens_an_exact_directory_and_can_continue_typing_an_object_key() {
    let mut app = preview();
    let search = "id:object-search";
    let directory = "Brand assets/";
    app.type_text(search, directory).unwrap();
    app.wait_for("id:breadcrumb-Brand assets/").unwrap();
    app.wait_for("id:object-Brand assets/Logo.svg").unwrap();
    assert_eq!(app.screen().input_text(search).unwrap(), Some(directory));

    app.type_text(search, "Guides/Usage.pdf").unwrap();
    app.wait_for("id:breadcrumb-Brand assets/Guides/").unwrap();
    app.wait_for("id:object-Brand assets/Guides/Usage.pdf")
        .unwrap();
    app.wait_for("id:object-Brand assets/Guides/Typography.txt")
        .unwrap();
    app.wait_for("id:object-inspector").unwrap();
    assert!(app.screen().enabled("id:copy-selection").unwrap());
    assert!(!app.screen().matches("id:modal").unwrap());
}

#[test]
fn search_filters_partial_paths_and_locates_a_pasted_s3_uri() {
    let mut config = desktop_config(studio_root(true));
    config.clock = nsh_core::time::Clock::manual(std::time::SystemTime::UNIX_EPOCH);
    let mut app = Headless::start(config).unwrap();
    app.wait_for("id:object-Brand assets/").unwrap();
    app.press("id:tree-view").unwrap();
    app.wait_for("id:tree-cache-time").unwrap();
    let search = "id:object-search";
    app.type_text(search, "Brand assets/Guides/Usage").unwrap();
    let tree_result = "id:tree-file-Brand assets/Guides/Usage.pdf";
    app.wait_for(tree_result).unwrap();
    app.run_for(Duration::from_secs(1)).unwrap();
    assert!(app.screen().matches(tree_result).unwrap());
    assert!(!app.screen().matches("id:modal").unwrap());

    let mut paste = nsh_core::PasteEvent::default();
    paste.text = "s3://application-backups/Brand assets/Guides/Usage.pdf".into();
    app.clear(search).unwrap();
    app.paste(search, &paste).unwrap();
    app.wait_for("id:object-Brand assets/Guides/Usage.pdf")
        .unwrap();
    app.wait_for("id:object-inspector").unwrap();
    assert!(
        app.screen()
            .selected("id:bucket-application-backups")
            .unwrap()
    );
    assert_eq!(
        app.screen().input_text(search).unwrap(),
        Some(paste.text.as_str())
    );
}

#[test]
fn locator_opens_a_directory_in_another_bucket_and_keeps_location_on_a_miss() {
    let mut app = preview();
    app.type_text(
        "id:object-search",
        "s3://application-backups/Brand assets/Guides/",
    )
    .unwrap();
    app.press_key("shift+enter").unwrap();
    app.wait_for("id:path").unwrap();
    app.press("id:submit-dialog").unwrap();
    let directory = "id:breadcrumb-Brand assets/Guides/";
    app.wait_for(directory).unwrap();
    app.wait_for("id:object-Brand assets/Guides/Typography.txt")
        .unwrap();
    assert!(
        app.screen()
            .selected("id:bucket-application-backups")
            .unwrap()
    );

    app.type_text("id:object-search", "brand assets/Guides/")
        .unwrap();
    app.press_key("enter").unwrap();
    app.wait_for(i18n::t("找不到指定的目录或对象。")).unwrap();
    app.press_key("escape").unwrap();
    app.wait_for(directory).unwrap();
}

#[test]
fn locator_can_select_a_slash_ending_key_as_an_object() {
    let mut app = preview();
    let path = "Product design/Archive/";
    app.type_text("id:object-search", path).unwrap();
    app.press_key("shift+enter").unwrap();
    app.wait_for("id:path").unwrap();
    assert_eq!(app.screen().input_text("id:path").unwrap(), Some(path));
    app.press("id:locate-object-only").unwrap();
    app.press("id:submit-dialog").unwrap();
    app.wait_for("id:object-Product design/Archive/").unwrap();
    app.wait_for("id:object-inspector").unwrap();
    assert!(app.screen().enabled("id:copy-selection").unwrap());
}

#[test]
fn tree_view_expands_levels_and_displays_parent_size_shares() {
    let mut app = preview();
    app.press("id:tree-view").unwrap();
    app.wait_for("id:tree-totals").unwrap();
    app.wait_for("id:tree-cache-time").unwrap();
    let folder = "id:tree-folder-Brand assets/";
    let guides = "id:tree-folder-Brand assets/Guides/";
    let file = "id:tree-file-Brand assets/Guides/Usage.pdf";
    app.wait_for(folder).unwrap();
    app.wait_for("24.6 MB").unwrap();
    assert!(!app.screen().matches(guides).unwrap());
    assert_eq!(
        app.screen().accessibility_state(folder).unwrap().expanded,
        Some(false)
    );
    app.press(folder).unwrap();
    app.wait_for(guides).unwrap();
    app.wait_for("97.5%").unwrap();
    assert_eq!(app.screen().accessibility_level(guides).unwrap(), Some(2));
    assert!(!app.screen().matches(file).unwrap());
    app.press(guides).unwrap();
    app.wait_for(file).unwrap();
    assert_eq!(app.screen().accessibility_level(file).unwrap(), Some(3));
    app.press("id:tree-sort-count").unwrap();
    app.wait_for_gone("id:tree-querying", WAIT).unwrap();
    app.wait_for(file).unwrap();
    app.press(guides).unwrap();
    app.wait_for_gone(file, WAIT).unwrap();
    app.press("id:list-view").unwrap();
    app.wait_for("id:object-Brand assets/").unwrap();
    app.press("id:tree-view").unwrap();
    app.wait_for("id:tree-cache-time").unwrap();
    app.wait_for(folder).unwrap();
}

#[test]
fn tree_search_reveals_matches_and_clears_hidden_selections() {
    let mut app = preview();
    app.press("id:tree-view").unwrap();
    app.wait_for("id:tree-totals").unwrap();
    app.type_text("id:object-search", "usage").unwrap();
    let guides = "id:tree-folder-Brand assets/Guides/";
    let file = "id:tree-file-Brand assets/Guides/Usage.pdf";
    app.wait_for(file).unwrap();
    assert!(
        !app.screen()
            .matches("id:tree-file-Brand assets/Logo.svg")
            .unwrap()
    );
    app.press("id:tree-check-file-Brand assets/Guides/Usage.pdf")
        .unwrap();
    assert!(app.screen().enabled("id:copy-selection").unwrap());
    app.press("id:delete-selection").unwrap();
    app.wait_for("id:delete-list").unwrap();
    app.press_key("escape").unwrap();
    app.press(guides).unwrap();
    app.wait_for_gone(file, WAIT).unwrap();
    app.wait_for_gone("id:delete-selection", WAIT).unwrap();
    app.press(guides).unwrap();
    app.wait_for(file).unwrap();
    app.press(file).unwrap();
    app.wait_for("application/octet-stream").unwrap();
    assert!(app.screen().enabled("id:inspector-download").unwrap());
    app.press("id:clear-search").unwrap();
    app.wait_for_gone(file, WAIT).unwrap();
    app.wait_for("id:tree-totals").unwrap();
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
    assert!(!app.screen().matches("id:account-settings").unwrap());
    assert!(!app.screen().matches("id:add-account-bottom").unwrap());
    app.press("id:account-menu-demo-aws").unwrap();
    app.press("id:account-edit-demo-aws").unwrap();
    assert_eq!(
        app.screen().input_text("id:name").unwrap(),
        Some("Design workspace")
    );
    assert_eq!(app.screen().input_text("id:accessKey").unwrap(), Some(""));
    assert!(!app.screen().matches("id:remove-account").unwrap());
    app.press("id:cancel-dialog").unwrap();
}

#[test]
fn workspace_menu_opens_the_target_settings_and_confirms_local_removal() {
    let mut app = preview();
    app.press("id:bucket-team-documents").unwrap();
    let menu = "id:account-menu-demo-minio";
    let edit = "id:account-edit-demo-minio";
    let delete = "id:account-delete-demo-minio";
    app.press(menu).unwrap();
    app.wait_for(edit).unwrap();
    assert!(app.screen().selected("id:account-demo-aws").unwrap());
    app.press(edit).unwrap();
    app.wait_for("id:name").unwrap();
    assert_eq!(
        app.screen().input_text("id:name").unwrap(),
        Some("Local development")
    );
    app.press("id:cancel-dialog").unwrap();

    app.press(menu).unwrap();
    app.wait_for(delete).unwrap();
    app.press(delete).unwrap();
    app.wait_for(i18n::t("删除工作空间配置？")).unwrap();
    app.wait_for(&crate::lf!(
        "仅删除「{name}」保存在本机的工作空间配置和凭据。",
        name = "Local development"
    ))
    .unwrap();
    app.press("id:cancel-dialog").unwrap();
    app.wait_for_gone("id:modal", WAIT).unwrap();
    assert!(app.screen().matches("id:account-demo-minio").unwrap());
    assert!(app.screen().selected("id:bucket-team-documents").unwrap());
}

#[test]
fn bucket_options_can_be_added_reused_and_removed() {
    let mut app = preview();
    assert!(!app.screen().matches("ap-southeast-1").unwrap());
    app.press("id:create-bucket").unwrap();
    app.press("id:region").unwrap();
    app.wait_for(i18n::t("使用默认值")).unwrap();
    app.type_text("id:region", "eu-west-2").unwrap();
    app.press("id:add-s3-option-region").unwrap();
    app.wait_for_gone("id:add-s3-option-region", WAIT).unwrap();
    app.press("id:cancel-dialog").unwrap();

    app.right_click("id:bucket-application-backups").unwrap();
    assert!(
        !app.screen()
            .matches("id:bucket-settings-application-backups")
            .unwrap()
    );
    assert!(app.screen().selected("id:bucket-studio-assets").unwrap());
    app.press("id:create-bucket").unwrap();
    assert!(!app.screen().matches("id:endpoint").unwrap());
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

    assert!(!app.screen().matches("id:app-settings").unwrap());
    assert!(!app.screen().matches("id:transfers-panel").unwrap());
    app.press("id:transfers-toggle").unwrap();
    app.wait_for("id:transfers-panel").unwrap();
    assert!(!app.screen().matches("id:transfer-settings").unwrap());
    app.press("id:transfers-toggle").unwrap();
    app.wait_for_gone("id:transfers-panel", WAIT).unwrap();
    app.press_key("mod+,").unwrap();
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
struct OpenUpdate;
struct ScanGate(std::sync::Arc<tokio::sync::Semaphore>);

#[component]
fn TestWorkspace() -> impl IntoElement {
    let (mut backend, receiver) = Backend::open(true).unwrap();
    if let Some(events) = engine_context::<Events>() {
        events.0.send(backend.event_sender()).unwrap();
    }
    if let Some(gate) = engine_context::<ScanGate>() {
        backend.hold_preview_scan(gate.0.clone());
    }
    let store = Store::new(backend, receiver);
    if engine_context::<OpenUpdate>().is_some() {
        store.change(|state| state.open_dialog(model::Dialog::Update));
    }
    ui::workspace(store)
}

fn paused_scan() -> (Headless, std::sync::Arc<tokio::sync::Semaphore>) {
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
    let mut config = desktop_config(Root::of::<TestWorkspace>());
    config.context = config.context.with(ScanGate(gate.clone()));
    let mut app = Headless::start(config).unwrap();
    app.wait_for("id:object-Brand assets/").unwrap();
    app.press("id:tree-view").unwrap();
    app.wait_for("id:tree-scanning").unwrap();
    (app, gate)
}

#[test]
fn lists_and_expands_a_cold_tree_before_the_size_scan_finishes() {
    let (mut app, gate) = paused_scan();
    let folder = "id:tree-folder-Brand assets/";
    let guides = "id:tree-folder-Brand assets/Guides/";
    let file = "id:tree-file-Brand assets/Guides/Usage.pdf";
    let pending = "id:tree-size-pending-Brand assets/";

    app.wait_for(pending).unwrap();
    assert!(!app.screen().matches("id:tree-totals").unwrap());
    app.press(folder).unwrap();
    app.wait_for(guides).unwrap();
    app.press(guides).unwrap();
    app.wait_for(file).unwrap();
    app.press("id:tree-check-file-Brand assets/Guides/Usage.pdf")
        .unwrap();
    assert!(app.screen().enabled("id:download-selection").unwrap());

    app.press("id:list-view").unwrap();
    app.wait_for("id:object-Brand assets/").unwrap();
    app.press("id:tree-view").unwrap();
    app.wait_for(file).unwrap();
    app.wait_for(pending).unwrap();
    gate.add_permits(1);
    app.wait_for("id:tree-totals").unwrap();
    app.wait_for_gone(pending, WAIT).unwrap();
    app.wait_for(file).unwrap();
    app.wait_for("97.5%").unwrap();

    app.press("id:refresh").unwrap();
    app.wait_for("id:tree-scanning").unwrap();
    assert!(app.screen().matches("id:tree-totals").unwrap());
    assert!(!app.screen().matches(pending).unwrap());
    app.press("id:tree-cancel-scan").unwrap();
    app.wait_for_gone("id:tree-scanning", WAIT).unwrap();
    app.wait_for("id:tree-totals").unwrap();
}

#[test]
fn canceled_size_scan_keeps_browsing_available() {
    let (mut app, gate) = paused_scan();
    app.press("id:bucket-application-backups").unwrap();
    app.wait_for("id:tree-size-pending-Brand assets/").unwrap();
    app.press("id:tree-cancel-scan").unwrap();
    app.wait_for_gone("id:tree-size-pending-Brand assets/", WAIT)
        .unwrap();
    app.press("id:tree-folder-Brand assets/").unwrap();
    app.wait_for("id:tree-file-Brand assets/Logo.svg").unwrap();
    app.press("id:tree-rescan").unwrap();
    app.wait_for("id:tree-size-pending-Brand assets/").unwrap();
    // The original bucket now keeps scanning in the background as well.
    gate.add_permits(2);
    app.wait_for("id:tree-totals").unwrap();
}

#[test]
fn update_dialog_tracks_progress_and_blocks_install_during_transfers() {
    let (events, receiver) = mpsc::channel();
    let mut config = desktop_config(Root::of::<TestWorkspace>());
    config.context = config.context.with(Events(events)).with(OpenUpdate);
    let mut app = Headless::start(config).unwrap();
    let sender = receiver.recv_timeout(WAIT).unwrap();
    app.wait_for("id:object-Brand assets/").unwrap();
    assert!(!app.screen().matches("id:update").unwrap());
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
    assert_eq!(bytes(1024_u128.pow(4)), "1 TB");
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

#[test]
fn list_view_shows_folder_totals_without_opening_tree_view() {
    let mut app = preview();
    app.wait_for("id:object-size-Brand assets/").unwrap();
    app.wait_for("24.6 MB").unwrap();
    app.type_text("id:object-search", "Brand assets/").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for("id:object-size-Brand assets/Guides/").unwrap();
    app.wait_for("24 MB").unwrap();
}

#[test]
fn list_sizes_finish_without_dismissing_an_open_form() {
    let (mut app, gate) = paused_scan();
    app.press("id:list-view").unwrap();
    app.wait_for("id:object-size-Brand assets/").unwrap();
    app.wait_for(i18n::t("计算大小中…")).unwrap();
    app.press("id:add-account").unwrap();
    app.wait_for("id:name").unwrap();
    app.type_text("id:name", "Keep this draft").unwrap();
    gate.add_permits(1);
    app.wait_for("24.6 MB").unwrap();
    assert_eq!(
        app.screen().input_text("id:name").unwrap(),
        Some("Keep this draft")
    );
}

#[test]
fn bucket_scan_finishes_in_background_and_is_reused_on_return() {
    let (mut app, gate) = paused_scan();
    app.press("id:bucket-application-backups").unwrap();
    app.wait_for("id:tree-scanning").unwrap();
    // Only the first bucket's waiting scan may finish. The second stays paused.
    gate.add_permits(1);
    app.press("id:bucket-studio-assets").unwrap();
    app.wait_for("id:tree-totals").unwrap();
    app.wait_for("24.6 MB").unwrap();
    assert!(!app.screen().matches("id:tree-scanning").unwrap());
    app.press("id:bucket-application-backups").unwrap();
    app.wait_for("id:tree-scanning").unwrap();
    app.press("id:bucket-studio-assets").unwrap();
    app.wait_for("id:tree-totals").unwrap();
    assert!(!app.screen().matches("id:tree-scanning").unwrap());
}

#[test]
fn returning_to_a_directory_restores_all_loaded_pages() {
    let mut app = preview();
    let search = "id:object-search";
    app.type_text(search, "Brand assets/Guides/").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for("id:object-Brand assets/Guides/Typography.txt")
        .unwrap();
    app.press("id:load-more").unwrap();
    app.wait_for("id:object-Brand assets/Guides/Usage.pdf")
        .unwrap();
    app.wait_for_gone("id:load-more", WAIT).unwrap();
    app.press("id:root-directory").unwrap();
    app.wait_for("id:object-Brand assets/").unwrap();
    app.type_text(search, "Brand assets/Guides/").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for("id:object-Brand assets/Guides/Typography.txt")
        .unwrap();
    app.wait_for("id:object-Brand assets/Guides/Usage.pdf")
        .unwrap();
    assert!(!app.screen().matches("id:load-more").unwrap());
}

#[test]
fn dragging_workspace_handle_reorders_without_switching_accounts() {
    use nsh_core::geometry::point;
    let mut app = preview();
    let first = "id:account-demo-aws";
    let second = "id:account-demo-minio";
    app.hover(first).unwrap();
    app.drag("id:account-drag-demo-aws", point(px(0.0), px(58.0)))
        .unwrap();
    // Wait for the persistence response before inspecting the reordered layout.
    app.wait_until("workspace order changes", WAIT, |screen| {
        let nodes = screen.layout_nodes();
        let y = |id: &str| {
            nodes
                .iter()
                .find(|n| n.test_id.as_deref() == Some(id))
                .unwrap()
                .bounds
                .origin
                .y
                .0
        };
        y("account-demo-aws") > y("account-demo-minio")
    })
    .unwrap();
    assert!(app.screen().selected(first).unwrap());
    app.hover(second).unwrap();
    app.drag("id:account-drag-demo-minio", point(px(0.0), px(58.0)))
        .unwrap();
    app.wait_until("workspace order changes", WAIT, |screen| {
        let nodes = screen.layout_nodes();
        let y = |id: &str| {
            nodes
                .iter()
                .find(|n| n.test_id.as_deref() == Some(id))
                .unwrap()
                .bounds
                .origin
                .y
                .0
        };
        y("account-demo-aws") < y("account-demo-minio")
    })
    .unwrap();
    assert!(app.screen().selected(first).unwrap());
}

#[test]
fn recycle_bin_restores_checked_objects_and_stays_out_of_subdirectories() {
    let mut app = preview();
    app.press("id:open-trash").unwrap();
    app.wait_for("id:trash-check-deleted/report.pdf").unwrap();
    assert!(!app.screen().enabled("id:trash-restore").unwrap());
    app.press("id:trash-check-deleted/report.pdf").unwrap();
    app.press("id:trash-restore").unwrap();
    app.wait_for(&format!("{}: 1", i18n::t("已还原"))).unwrap();
    app.wait_for_gone("id:trash-check-deleted/report.pdf", WAIT)
        .unwrap();
    app.press("id:trash-close").unwrap();
    app.type_text("id:object-search", "Brand assets/").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for("id:breadcrumb-Brand assets/").unwrap();
    assert!(!app.screen().matches("id:open-trash").unwrap());
}

#[test]
fn transfer_activity_animates_only_while_tasks_are_active_without_opening_panel() {
    let (events, receiver) = mpsc::channel();
    let mut config = desktop_config(Root::of::<TestWorkspace>());
    config.context = config.context.with(Events(events));
    let mut app = Headless::start(config).unwrap();
    let sender = receiver.recv_timeout(WAIT).unwrap();
    app.wait_for("id:object-Brand assets/").unwrap();
    assert!(!app.screen().matches("id:transfer-animation").unwrap());
    let mut transfer: s3_studio_core::transfers::Transfer = serde_json::from_value(serde_json::json!({
        "id":"activity-test","name":"archive.zip","accountId":"demo-aws","bucket":"studio-assets",
        "key":"archive.zip","direction":"download","status":"running","bytes":512,"total":1024
    })).unwrap();
    sender
        .unbounded_send(Message::Transfer(transfer.clone()))
        .unwrap();
    app.wait_for("id:transfer-animation").unwrap();
    assert!(!app.screen().matches("id:transfers-panel").unwrap());
    for status in ["paused", "running", "error", "queued", "done"] {
        transfer.status = status.into();
        sender
            .unbounded_send(Message::Transfer(transfer.clone()))
            .unwrap();
        if matches!(status, "running" | "queued") {
            app.wait_for("id:transfer-animation").unwrap();
        } else {
            app.wait_for_gone("id:transfer-animation", WAIT).unwrap();
        }
    }
}
