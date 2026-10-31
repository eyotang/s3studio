use crate::{
    backend::Backend,
    model::{Dialog, Sort, bytes, visible},
    workspace::Workspace,
};
use gpui::test::{TestAppContextExt, TestWindowExt};
use gpui::{AppContext, Bounds, TestAppContext, WindowBounds, WindowOptions, point, px, size};
use std::time::Duration;

async fn setup(cx: &mut TestAppContext) -> (gpui::AnyWindowHandle, gpui::Entity<Workspace>) {
    cx.executor().allow_parking();
    let (backend, receiver) = Backend::open(true).unwrap();
    let result = cx.update(|cx| {
        gpui::init(cx);
        crate::ui::configure_theme(cx);
        gpui::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(1280.), px(860.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| Workspace::new(backend, receiver, window, cx)),
        )
        .unwrap()
    });
    let view = result.1.clone();
    cx.wait_for(result.0, Duration::from_secs(10), move |_, cx| {
        view.read(cx).objects.len() == 9
    })
    .await;
    result
}
#[gpui::test]
async fn app_menu_actions_open_about_and_existing_update_dialog(cx: &mut TestAppContext) {
    let (window, view) = setup(cx).await;
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(crate::About), cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        assert_eq!(view.read(cx).dialog, Some(Dialog::About));
        window.click("about-close", cx);
        window.dispatch_action(Box::new(crate::CheckForUpdates), cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        assert_eq!(view.read(cx).dialog, Some(Dialog::Update));
        assert_eq!(window.find("check-update").disabled(), Some(true));
        assert!(
            !view.read(cx).update_pending,
            "Preview menu must not request updates"
        );
        window.click("close-dialog", cx);
        window.dispatch_action(Box::new(crate::About), cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.click("about-check-update", cx);
        assert_eq!(view.read(cx).dialog, Some(Dialog::Update));
        view.update(cx, |view, cx| {
            view.update_status.mode = "manual";
            view.update_status.phase = "idle";
            view.check_for_updates(&crate::CheckForUpdates, window, cx);
            assert!(
                view.update_pending,
                "Menu must invoke the existing update command"
            );
            view.show_about(&crate::About, window, cx);
            assert_eq!(
                view.dialog,
                Some(Dialog::Update),
                "Pending updates must stay visible"
            );
        });
    })
    .unwrap();
}

#[gpui::test]
async fn app_menu_preserves_unsaved_account_form(cx: &mut TestAppContext) {
    let (window, view) = setup(cx).await;
    cx.update_window(window, |_, window, cx| {
        window.click("add-account", cx);
        window.click("name", cx);
        window.input("Unsaved account", cx);
        window.dispatch_action(Box::new(crate::About), cx);
        window.dispatch_action(Box::new(crate::CheckForUpdates), cx);
    })
    .unwrap();
    cx.update_window(window, |_, _, cx| {
        let view = view.read(cx);
        assert_eq!(view.dialog, Some(Dialog::Account(None)));
        assert_eq!(
            view.form.inputs["name"].read(cx).value().as_str(),
            "Unsaved account"
        );
        assert!(!view.update_pending);
    })
    .unwrap();
}

#[gpui::test]
async fn new_account_defaults_to_path_style_and_edit_preserves_false(cx: &mut TestAppContext) {
    let (window, view) = setup(cx).await;
    cx.update_window(window, |_, window, cx| {
        window.click("add-account", cx);
        assert_eq!(window.find("pathStyle").checked(), Some(true));
        window.click("pathStyle", cx);
        assert!(!view.read(cx).form.path_style);
        window.click("cancel-dialog", cx);
        window.click("account-settings", cx);
        assert_eq!(window.find("pathStyle").checked(), Some(false));
    })
    .unwrap();
}
#[gpui::test]
async fn search_selection_and_view_switch_match_web(cx: &mut TestAppContext) {
    let (window, view) = setup(cx).await;
    cx.update_window(window, |_, window, cx| {
        window.click("object-search", cx);
        window.input("guidelines", cx);
        assert_eq!(view.read(cx).visible(cx).len(), 1);
        window.click("clear-search", cx);
        assert_eq!(view.read(cx).visible(cx).len(), 9);
        window.click(("object-check", 3_usize), cx);
        assert_eq!(view.read(cx).selected.len(), 1);
        window.click("grid-view", cx);
        assert!(view.read(cx).grid);
        window.click("list-view", cx);
        assert!(!view.read(cx).grid);
        window.click("delete-selection", cx);
        assert_eq!(view.read(cx).dialog, Some(Dialog::Delete));
        window.click("cancel-dialog", cx);
        assert_eq!(view.read(cx).objects.len(), 9);
    })
    .unwrap();
}
#[gpui::test]
async fn folder_navigation_and_details_ignore_stale_requests(cx: &mut TestAppContext) {
    let (window, view) = setup(cx).await;
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.navigate(view.bucket.clone(), "Brand assets/".into(), window, cx)
        });
    })
    .unwrap();
    let check = view.clone();
    cx.wait_for(window, Duration::from_secs(10), move |_, cx| {
        check.read(cx).objects.len() == 2
    })
    .await;
    cx.update_window(window, |_, window, cx| window.click("root-directory", cx))
        .unwrap();
    let check = view.clone();
    cx.wait_for(window, Duration::from_secs(10), move |_, cx| {
        check.read(cx).objects.len() == 9
    })
    .await;
    cx.update_window(window, |_, window, cx| {
        window.click(("more", 3_usize), cx);
        assert!(view.read(cx).show_info);
    })
    .unwrap();
    let check = view.clone();
    cx.wait_for(window, Duration::from_secs(10), move |_, cx| {
        check.read(cx).info.is_some()
    })
    .await;
    cx.update_window(window, |_, window, cx| {
        assert_eq!(
            view.read(cx).info.as_ref().unwrap().content_type,
            "application/octet-stream"
        );
        window.click("close-info", cx);
        assert!(!view.read(cx).show_info);
    })
    .unwrap();
}
#[test]
fn formatting_and_directory_priority_match_existing_behavior() {
    assert_eq!(bytes(0), "0 B");
    assert_eq!(bytes(2488000), "2.4 MB");
    assert_eq!(bytes(1024), "1 KB");
    let objects: Vec<s3_studio_core::s3::S3Object> = serde_json::from_value(serde_json::json!([
        {"key":"file10","size":1,"modified":"","storageClass":"STANDARD","folder":false},
        {"key":"dir/","size":0,"modified":"","storageClass":"STANDARD","folder":true},
        {"key":"file2","size":2,"modified":"","storageClass":"STANDARD","folder":false}
    ]))
    .unwrap();
    let result = visible(&objects, "", Sort::Name, true);
    assert_eq!(
        result.iter().map(|o| o.key.as_str()).collect::<Vec<_>>(),
        vec!["dir/", "file2", "file10"]
    );
    assert!(visible(&objects, "", Sort::Size, false)[0].folder);
}

#[gpui::test]
async fn update_dialog_preserves_modes_progress_and_revision_order(cx: &mut TestAppContext) {
    let (window, view) = setup(cx).await;
    cx.update_window(window, |_, window, cx| {
        window.scroll(
            "object-search",
            gpui::ScrollDelta::Pixels(point(px(0.), px(-1200.))),
            cx,
        );
        window.click("update", cx);
        assert_eq!(view.read(cx).dialog, Some(Dialog::Update));
        assert_eq!(window.find("check-update").disabled(), Some(true));
        let available = crate::updater::Status {
            revision: 10,
            mode: "manual",
            phase: "available",
            version: Some("0.3.1".into()),
            notes: Some("Release notes".into()),
            ..Default::default()
        };
        view.update(cx, |view, cx| {
            view.accept_update(available.clone());
            cx.notify();
        });
        window.render_frame(cx);
        assert_ne!(window.find("check-update").disabled(), Some(true));
        assert_ne!(window.find("install-update").disabled(), Some(true));
        view.update(cx, |view, cx| {
            let mut older = available.clone();
            older.revision = 9;
            older.version = None;
            view.accept_update(older);
            assert_eq!(view.update_status.version.as_deref(), Some("0.3.1"));
            view.accept_update(crate::updater::Status {
                revision: 11,
                mode: "inApp",
                phase: "downloading",
                downloaded: 512,
                total: Some(1024),
                ..available
            });
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(window.find("close-dialog").disabled(), Some(true));
        assert_eq!(window.find("check-update").disabled(), Some(true));
        assert_eq!(window.find("install-update").disabled(), Some(true));
        view.update(cx, |view, cx| view.close_dialog(window, cx));
        assert_eq!(view.read(cx).dialog, Some(Dialog::Update));
        view.update(cx, |view, cx| {
            let mut status = view.update_status.clone();
            status.revision = 12;
            status.phase = "available";
            status.error = Some("更新未完成，当前版本保持不变：测试错误".into());
            view.accept_update(status);
            cx.notify();
        });
        window.render_frame(cx);
        assert_ne!(window.find("check-update").disabled(), Some(true));
        window.click("close-dialog", cx);
        assert!(view.read(cx).dialog.is_none());
    })
    .unwrap();
}

#[gpui::test]
async fn active_transfers_block_update_install_and_pending_blocks_close(cx: &mut TestAppContext) {
    let (window, view) = setup(cx).await;
    cx.update_window(window, |_, window, cx| {
        window.click("help", cx);
        window.click("help-start", cx);
        assert!(view.read(cx).dialog.is_none());
        view.update(cx, |view, cx| {
            view.transfers.push(
                serde_json::from_value(serde_json::json!({
                    "id":"test-transfer", "name":"test.zip", "accountId":"demo-aws",
                    "bucket":"studio-assets", "key":"test.zip", "direction":"upload",
                    "status":"running", "bytes":1, "total":2
                }))
                .unwrap(),
            );
            view.accept_update(crate::updater::Status {
                revision: 10,
                mode: "inApp",
                phase: "available",
                version: Some("0.3.1".into()),
                ..Default::default()
            });
            view.open_dialog(Dialog::Update, window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("install-update").disabled(), Some(true));
        view.update(cx, |view, cx| {
            view.transfers[0].status = "paused".into();
            cx.notify();
        });
        window.render_frame(cx);
        assert_ne!(window.find("install-update").disabled(), Some(true));
        view.update(cx, |view, cx| {
            view.update_pending = true;
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(window.find("close-dialog").disabled(), Some(true));
        view.update(cx, |view, cx| view.close_dialog(window, cx));
        assert_eq!(view.read(cx).dialog, Some(Dialog::Update));
    })
    .unwrap();
}

#[gpui::test]
async fn settings_menu_and_sidebar_share_global_preferences(cx: &mut TestAppContext) {
    let (window, view) = setup(cx).await;
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(crate::OpenSettings), cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        assert_eq!(view.read(cx).dialog, Some(Dialog::TransferSettings));
        for id in [
            "settings-language-system",
            "settings-language-zh",
            "settings-language-en",
            "theme-system",
            "theme-light",
            "theme-dark",
            "concurrency",
        ] {
            window.find(id);
        }
        window.click("close-dialog", cx);
        window.click("app-settings", cx);
    })
    .unwrap();
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).dialog, Some(Dialog::TransferSettings));
        assert!(view.read(cx).form.inputs.contains_key("concurrency"));
    })
    .unwrap();
}
