#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]
extern crate gpui_kit as gpui;
mod backend;
mod i18n;
pub use s3_studio_core::lf;
mod appearance;
mod model;
mod ui;
mod updater;
mod workspace;
use gpui::{prelude::*, *};
use std::borrow::Cow;
actions!(
    s3_studio,
    [
        Quit,
        Hide,
        HideOthers,
        ShowAll,
        About,
        CheckForUpdates,
        OpenSettings
    ]
);
struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match path{
        "brand.svg"=>Ok(Some(Cow::Borrowed(include_bytes!("../../public/brand/s3-studio-logo.svg")))),
        "folder-filled.svg"=>Ok(Some(Cow::Borrowed(br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path fill="currentColor" stroke="currentColor" stroke-width="1.3" d="M20 20H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2Z"/></svg>"#))),
        _=>gpui::assets::AllAssets.load(path),
    }
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui::assets::AllAssets.list(path)
    }
}
fn main() -> anyhow::Result<()> {
    i18n::initialize();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let app_icon = std::sync::Arc::new(
        image::load_from_memory(include_bytes!("../../src-tauri/icons/128x128.png"))?.into_rgba8(),
    );
    let preview = std::env::args().any(|a| a == "--preview");
    let (backend, receiver) = backend::Backend::open(preview)?;
    gpui::application()
        .with_restart_arguments(std::env::args_os().skip(1).collect())
        .with_assets(Assets)
        .run(move |cx| {
            #[cfg(target_os = "linux")]
            if let Some(path) = std::env::var_os("APPIMAGE") {
                cx.set_restart_path(std::path::PathBuf::from(path));
            }
            cx.set_app_identity("com.s3studio.desktop", "S3 Studio");
            gpui::init(cx);
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.on_action(|_: &Hide, cx| cx.hide());
            cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
            cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
            if cfg!(target_os = "macos") {
                cx.bind_keys([
                    KeyBinding::new("cmd-q", Quit, None),
                    KeyBinding::new("cmd-,", OpenSettings, None),
                    KeyBinding::new("cmd-h", Hide, None),
                    KeyBinding::new("cmd-alt-h", HideOthers, None),
                ]);
                set_app_menus(cx);
            }
            appearance::initialize();
            ui::configure_theme(cx);
            let options = WindowOptions {
                icon: Some(app_icon),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1280.), px(860.)),
                    cx,
                ))),
                window_min_size: Some(size(px(960.), px(680.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("S3 Studio".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            match gpui::open_window(options, cx, move |window, cx| {
                cx.new(|cx| workspace::Workspace::new(backend, receiver, window, cx))
            }) {
                Ok((_window, _view)) => cx.activate(true),
                Err(error) => {
                    eprintln!("无法启动 S3 Studio：{error}");
                    cx.quit();
                }
            }
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
        });
    Ok(())
}
#[cfg(test)]
mod tests;

pub fn set_app_menus(cx: &App) {
    if !cfg!(target_os = "macos") {
        return;
    }
    use gpui::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
    cx.set_menus([
        Menu::new("S3 Studio").items([
            MenuItem::action(crate::i18n::t("关于 S3 Studio"), About),
            MenuItem::action(crate::i18n::t("检查更新…"), CheckForUpdates),
            MenuItem::action(i18n::t("设置…"), OpenSettings),
            MenuItem::separator(),
            MenuItem::os_submenu(i18n::t("服务"), SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(i18n::t("隐藏 S3 Studio"), Hide),
            MenuItem::action(i18n::t("隐藏其他应用"), HideOthers),
            MenuItem::action(i18n::t("显示全部"), ShowAll),
            MenuItem::separator(),
            MenuItem::action(i18n::t("退出 S3 Studio"), Quit),
        ]),
        Menu::new(i18n::t("编辑")).items([
            MenuItem::action(i18n::t("撤销"), Undo),
            MenuItem::action(i18n::t("重做"), Redo),
            MenuItem::separator(),
            MenuItem::action(i18n::t("剪切"), Cut),
            MenuItem::action(i18n::t("复制"), Copy),
            MenuItem::action(i18n::t("粘贴"), Paste),
            MenuItem::action(i18n::t("全选"), SelectAll),
        ]),
    ]);
}
