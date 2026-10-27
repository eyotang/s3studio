#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]
extern crate gpui_kit as gpui;
mod backend;
mod model;
mod ui;
mod updater;
mod workspace;
use gpui::{prelude::*, *};
use std::borrow::Cow;
actions!(s3_studio, [Quit, Hide, HideOthers, ShowAll]);
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
    let _ = rustls::crypto::ring::default_provider().install_default();
    let app_icon = std::sync::Arc::new(
        image::load_from_memory(include_bytes!("../../src-tauri/icons/128x128.png"))?.into_rgba8(),
    );
    let preview = std::env::args().any(|a| a == "--preview");
    let (backend, receiver) = backend::Backend::open(preview)?;
    gpui::application().with_assets(Assets).run(move |cx| {
        cx.set_app_identity("com.s3studio.desktop", "S3 Studio");
        gpui::init(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &Hide, cx| cx.hide());
        cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
        cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
        if cfg!(target_os = "macos") {
            use gpui::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
            cx.bind_keys([
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("cmd-h", Hide, None),
                KeyBinding::new("cmd-alt-h", HideOthers, None),
            ]);
            cx.set_menus([
                Menu::new("S3 Studio").items([
                    MenuItem::os_submenu("Services", SystemMenuType::Services),
                    MenuItem::separator(),
                    MenuItem::action("Hide S3 Studio", Hide),
                    MenuItem::action("Hide Others", HideOthers),
                    MenuItem::action("Show All", ShowAll),
                    MenuItem::separator(),
                    MenuItem::action("Quit S3 Studio", Quit),
                ]),
                Menu::new("Edit").items([
                    MenuItem::action("Undo", Undo),
                    MenuItem::action("Redo", Redo),
                    MenuItem::separator(),
                    MenuItem::action("Cut", Cut),
                    MenuItem::action("Copy", Copy),
                    MenuItem::action("Paste", Paste),
                    MenuItem::action("Select All", SelectAll),
                ]),
            ]);
        }
        gpui::component::Theme::change(gpui::component::ThemeMode::Light, None, cx);
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
