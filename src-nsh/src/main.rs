#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod appearance;
mod backend;
mod i18n;
mod model;
mod object_tree;
mod ui;
mod updater;
mod workspace;
pub use s3_studio_core::lf;

use nsh_core::{desktop::MainWindowOptions, element::Root, prelude::*};
use s3_studio_core::transfers::UpdateGuard;
use std::process::ExitCode;

#[cfg(target_os = "linux")]
use nsh_platform_gpui as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(windows)]
use nsh_platform_windows as platform;

const APP_NAME: &str = "S3 Studio";
const APP_ID: &str = "com.s3studio.desktop";

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    i18n::initialize();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let preview = std::env::args_os().any(|arg| arg == "--preview");
    let appearance = appearance::preference();
    let mut window = MainWindowOptions::default();
    window.attributes.min_size = Some(Some(nsh_core::element::size(px(960.0), px(680.0))));
    let root = studio_root(preview)
        .appearance(appearance)
        .main_window(window);
    run(root, appearance)
}

fn studio_root(preview: bool) -> Root {
    Root::new::<Studio>(move || StudioProps { preview }).app_id(APP_ID)
}

#[derive(Clone, PartialEq, Props)]
struct StudioProps {
    #[prop(default)]
    preview: bool,
}

#[component]
fn Studio(props: StudioProps) -> impl IntoElement {
    match backend::Backend::open(props.preview) {
        Ok((backend, receiver)) => ui::workspace(workspace::Store::new(backend, receiver)),
        Err(error) => view()
            .flex_1()
            .p(px(24.0))
            .child(text(i18n::t("无法定位应用数据目录")))
            .child(text(error.to_string()))
            .into_element(),
    }
}

#[cfg(not(target_os = "linux"))]
fn run(root: Root, appearance: Appearance) -> ExitCode {
    platform::run_app(
        platform::AppSource::Rust(root),
        platform::AppOptions {
            app_id: APP_ID.into(),
            title: APP_NAME.into(),
            size: (1280, 860),
            appearance,
            ..Default::default()
        },
    )
}

#[cfg(target_os = "linux")]
struct RestartRequests(futures::channel::mpsc::UnboundedSender<UpdateGuard>);

#[cfg(target_os = "linux")]
fn run(root: Root, appearance: Appearance) -> ExitCode {
    use futures::{StreamExt, channel::mpsc};
    use std::{cell::Cell, rc::Rc};

    let (send, mut receive) = mpsc::unbounded();
    let opened = Rc::new(Cell::new(false));
    let result = opened.clone();
    let options = platform::AppOptions {
        app_id: APP_ID.into(),
        title: APP_NAME.into(),
        size: (1280, 860),
        appearance,
        context: platform::AppOptions::default()
            .context
            .with(RestartRequests(send)),
        ..Default::default()
    };
    gpui_platform::application()
        .with_restart_arguments(std::env::args_os().skip(1).collect())
        .run(move |cx| {
            cx.set_app_identity(APP_ID, APP_NAME);
            if let Some(path) = std::env::var_os("APPIMAGE") {
                cx.set_restart_path(path.into());
            }
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            match platform::open_app_window(platform::AppSource::Rust(root), options, cx) {
                Ok(_) => opened.set(true),
                Err(error) => {
                    eprintln!("S3 Studio: {error}");
                    cx.quit();
                    return;
                }
            }
            cx.spawn(async move |cx| {
                if let Some(guard) = receive.next().await {
                    let _ = cx.update(|cx| {
                        // Keep the transfer reservation through application shutdown.
                        cx.on_app_quit(move |_| {
                            let _guard = &guard;
                            async {}
                        })
                        .detach();
                        cx.restart();
                    });
                }
            })
            .detach();
        });
    if result.get() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(target_os = "linux")]
fn restart_after_update(guard: UpdateGuard) {
    if let Some(requests) = nsh_core::element::engine_context::<RestartRequests>() {
        let _ = requests.0.unbounded_send(guard);
    }
}

#[cfg(not(target_os = "linux"))]
fn restart_after_update(_guard: UpdateGuard) {
    nsh_core::element::desktop::restart();
}

#[cfg(test)]
mod tests;
