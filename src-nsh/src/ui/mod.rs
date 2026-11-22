mod controls;
mod dialogs;
mod files;
mod palette;
mod transfers;

use crate::{
    i18n::t,
    model::{Dialog, Sort},
    workspace::{Store, Workspace},
};
use controls::*;
use nsh_core::prelude::*;
use palette::*;

pub fn workspace(store: Store) -> AnyElement {
    let search = element_ref();
    let search_shortcut = search.clone();
    on_key("mod+f", move |_| {
        if store.read(|state| state.dialog.is_none()) {
            search_shortcut.focus();
        }
    });
    #[cfg(target_os = "macos")]
    {
        let search_shortcut = search.clone();
        on_key("ctrl+f", move |_| {
            if store.read(|state| state.dialog.is_none()) {
                search_shortcut.focus();
            }
        });
        let menu_state = computed(move || {
            store.read(|state| (state.can_open_app_dialog(), crate::i18n::current()))
        });
        effect(move || app_menu(store, menu_state.get().0));
    }
    on_key_with(
        "escape",
        KeyOptions {
            when: nsh_core::keymap::When::Always,
            ..Default::default()
        },
        move |_| {
            store.change(|state| {
                if state.dialog.is_some() {
                    state.close_dialog();
                } else if !state.is_busy() {
                    state.clear_selection();
                }
            });
        },
    );

    let width = computed(|| window().width.0);
    let inset = move || px(if width.get() <= 1050.0 { 22.0 } else { 34.0 });
    row()
        .size_full()
        .items_stretch()
        .bg(BACKGROUND)
        .child(sidebar(store, width))
        .child(
            column()
                .flex_1()
                .min_w_0()
                .child(dynamic(move || store.read(|state| header(state, inset()))))
                .child(
                    scroll().test_id("workspace-body").flex_1().min_h_0().child(
                        column()
                            .gap(px(18.0))
                            .px(inset)
                            .pt(px(19.0))
                            .pb(px(12.0))
                            .child(dynamic(move || {
                                store.read(|state| bucket_heading(store, state))
                            }))
                            .child(files::browser(store, search))
                            .child(dynamic(move || {
                                store.read_transfers(|state| transfers::panel(store, state))
                            }))
                            .child(footer(store)),
                    ),
                ),
        )
        .child(dynamic(move || {
            store.read(|state| {
                (!state.toast.is_empty()).then(|| {
                    view()
                        .absolute()
                        .bottom(px(20.0))
                        .left_0()
                        .right_0()
                        .items_center()
                        .pointer_events(PointerEvents::None)
                        .child(
                            row()
                                .gap(px(8.0))
                                .px(px(16.0))
                                .py(px(12.0))
                                .rounded(px(9.0))
                                .bg(SURFACE)
                                .border_1()
                                .border_color(LINE)
                                .shadow_md()
                                .child(icon("check", 16.0, OK))
                                .child(label(state.toast.clone(), 12.0, TEXT)),
                        )
                })
            })
        }))
        .child(dialogs::active(store))
        .into_element()
}

#[cfg(target_os = "macos")]
fn app_menu(store: Store, enabled: bool) {
    use nsh_core::desktop::MenuRole;
    use nsh_core::element::desktop::{MenuItem, set_application_menu};
    let role = |role, name| MenuItem::role(role).label(t(name));
    let services = t("服务");
    set_application_menu(vec![
        MenuItem::submenu(
            "S3 Studio",
            [
                MenuItem::new(t("关于 S3 Studio"))
                    .disabled(!enabled)
                    .on_select(store.open(Dialog::About)),
                MenuItem::new(t("检查更新…"))
                    .disabled(!enabled)
                    .on_select(store.action(Workspace::check_for_updates)),
                MenuItem::new(t("设置…"))
                    .shortcut("mod+,")
                    .disabled(!enabled)
                    .on_select(store.open(Dialog::TransferSettings)),
                MenuItem::separator(),
                MenuItem::submenu(services, []),
                MenuItem::separator(),
                role(MenuRole::Hide, "隐藏 S3 Studio"),
                role(MenuRole::HideOthers, "隐藏其他应用"),
                role(MenuRole::ShowAll, "显示全部"),
                MenuItem::separator(),
                role(MenuRole::Quit, "退出 S3 Studio"),
            ],
        ),
        MenuItem::submenu(
            t("编辑"),
            [
                role(MenuRole::Undo, "撤销"),
                role(MenuRole::Redo, "重做"),
                MenuItem::separator(),
                role(MenuRole::Cut, "剪切"),
                role(MenuRole::Copy, "复制"),
                role(MenuRole::Paste, "粘贴"),
                role(MenuRole::SelectAll, "全选"),
            ],
        ),
    ]);
    // AppKit fills this submenu with the system's installed Services.
    run_on_main_thread(move || {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSApplication;
        use objc2_foundation::NSString;
        let Some(main_thread) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(main_thread);
        let submenu = app
            .mainMenu()
            .and_then(|bar| bar.itemAtIndex(0))
            .and_then(|item| item.submenu())
            .and_then(|menu| menu.itemWithTitle(&NSString::from_str(services)))
            .and_then(|item| item.submenu());
        app.setServicesMenu(submenu.as_deref());
    });
}

fn sidebar(store: Store, width: Memo<f32>) -> View {
    let bucket_query = move || store.read(|state| state.bucket_search.clone());
    column()
        .w(move || {
            px(if width.get() <= 1050.0 {
                215.0
            } else if width.get() >= 1450.0 {
                268.0
            } else {
                248.0
            })
        })
        .flex_shrink_0()
        .pt(px(27.0))
        .pb(px(14.0))
        .px(px(15.0))
        .border_r_1()
        .border_color(LINE)
        .bg(SIDEBAR)
        .child(
            row()
                .gap(px(10.0))
                .pl(px(8.0))
                .mb(px(30.0))
                .child(brand(42.0))
                .child(label("S3 Studio", 20.0, INK).font_weight(FontWeight::SEMIBOLD)),
        )
        .child(
            row()
                .justify_between()
                .pl(px(8.0))
                .child(label(move || t("工作空间"), 11.0, MUTED).font_weight(FontWeight::SEMIBOLD))
                .child(dynamic(move || {
                    store.read(|state| {
                        icon_button("add-account", t("添加账户"), "plus")
                            .disabled(state.is_busy())
                            .on_press(store.open(Dialog::Account(None)))
                    })
                })),
        )
        .child(scroll().test_id("accounts").max_h(percentage(0.30)).child(
            column().gap(px(4.0)).child(dynamic(move || {
                store.read(|state| account_buttons(store, state))
            })),
        ))
        .child(dynamic(move || {
            store.read(|state| {
                button("add-account-bottom", t("添加账户"))
                    .h(px(34.0))
                    .justify_start()
                    .px(px(8.0))
                    .disabled(state.is_busy())
                    .child(icon("plus", 15.0, MUTED))
                    .child(label(t("添加账户"), 12.0, MUTED))
                    .on_press(store.open(Dialog::Account(None)))
            })
        }))
        .child(divider().my(px(10.0)))
        .child(dynamic(move || {
            store.read(|state| {
                row()
                    .justify_between()
                    .pl(px(8.0))
                    .child(
                        row()
                            .gap(px(8.0))
                            .child(
                                label(t("存储桶"), 11.0, MUTED).font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(label(state.buckets.len().to_string(), 10.0, MUTED)),
                    )
                    .child(
                        icon_button("create-bucket", t("创建存储桶"), "plus")
                            .disabled(state.account().is_none() || state.is_busy())
                            .on_press(store.open(Dialog::Bucket)),
                    )
            })
        }))
        .child(when(
            move || store.read(|state| !state.buckets.is_empty()),
            move || {
                input_field(
                    "bucket-search",
                    t("搜索存储桶"),
                    t("查找存储桶"),
                    bucket_query,
                )
                .label(move || store.read(|_| SharedString::from(t("搜索存储桶"))))
                .placeholder(move || store.read(|_| t("查找存储桶").to_owned()))
                .h(px(30.0))
                .mb(px(6.0))
                .on_change(move |value: &str| {
                    store.change(|state| state.bucket_search = value.into())
                })
            },
        ))
        .child(scroll().test_id("buckets").flex_1().min_h(px(35.0)).child(
            column().gap(px(4.0)).child(dynamic(move || {
                store.read(|state| bucket_buttons(store, state))
            })),
        ))
        .child(dynamic(move || {
            store.read(|state| {
                column()
                    .gap(px(8.0))
                    .pt(px(10.0))
                    .child(
                        secondary("app-settings", t("设置"))
                            .disabled(!state.can_open_app_dialog())
                            .on_press(store.open(Dialog::TransferSettings)),
                    )
                    .child(divider())
                    .child(
                        row()
                            .justify_between()
                            .child(
                                button("account-settings", t("账户设置"))
                                    .h(px(32.0))
                                    .px(px(7.0))
                                    .child(icon("settings-2", 15.0, MUTED))
                                    .child(label(t("账户设置"), 11.0, TEXT))
                                    .disabled(state.account().is_none() || state.is_busy())
                                    .on_press(
                                        store.open(Dialog::Account(Some(state.active_id.clone()))),
                                    ),
                            )
                            .child(
                                icon_button("help", t("使用帮助"), "circle-question-mark")
                                    .disabled(state.is_busy())
                                    .on_press(store.open(Dialog::Help)),
                            ),
                    )
            })
        }))
}

fn account_buttons(store: Store, state: &Workspace) -> Vec<AnyElement> {
    let defaults = state.s3_defaults.as_ref();
    state
        .accounts
        .iter()
        .map(|account| {
            let id = account.id.clone();
            let active = id == state.active_id;
            let aws = defaults
                .map_or(account.endpoint.as_str(), |d| d.endpoint_for(account))
                .is_empty();
            button(&format!("account-{id}"), account.name.clone())
                .key(id.clone())
                .selected(active)
                .selected_style(|s| s.bg(SURFACE).border_color(LINE))
                .border_1()
                .border_color(AdaptiveColor::TRANSPARENT)
                .justify_start()
                .px(px(8.0))
                .py(px(9.0))
                .disabled(state.is_busy())
                .child(
                    row()
                        .size(px(32.0))
                        .justify_center()
                        .rounded(px(8.0))
                        .bg(if aws { SUBTLE } else { SELECTED })
                        .child(icon(
                            if aws { "cloud" } else { "server" },
                            18.0,
                            if aws { ARCHIVE } else { IMAGE },
                        )),
                )
                .child(
                    column()
                        .flex_1()
                        .min_w_0()
                        .gap(px(3.0))
                        .child(
                            label(account.name.clone(), 12.0, INK)
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(label(
                            if aws { "Amazon S3" } else { "S3 Compatible" },
                            10.0,
                            MUTED,
                        )),
                )
                .when(active, |b| b.child(dot()))
                .on_press(store.action(move |state| state.select_account(id.clone())))
                .into_element()
        })
        .collect()
}

fn bucket_buttons(store: Store, state: &Workspace) -> Vec<AnyElement> {
    if state.bucket_loading || state.buckets.is_empty() {
        let message = if state.bucket_loading {
            t("正在连接…")
        } else if state.account().is_some() {
            t("暂无存储桶")
        } else {
            t("添加账户以开始")
        };
        return vec![label(message, 11.0, MUTED).p(px(9.0)).into_element()];
    }
    state
        .buckets
        .iter()
        .filter(|bucket| bucket.name.contains(&state.bucket_search))
        .map(|bucket| {
            let name = bucket.name.clone();
            let selected = name == state.bucket;
            let ink = if selected { ACCENT } else { MUTED };
            button(&format!("bucket-{name}"), name.clone())
                .key(name.clone())
                .h(px(36.0))
                .px(px(8.0))
                .justify_start()
                .selected(selected)
                .selected_style(|s| s.bg(SELECTED))
                .disabled(state.is_busy())
                .child(icon("database", 16.0, ink))
                .child(label(name.clone(), 12.0, ink).truncate().flex_1().min_w_0())
                .when(selected, |b| b.child(icon("chevron-right", 13.0, ink)))
                .on_press(store.action(move |state| state.navigate(name.clone(), String::new())))
                .into_element()
        })
        .collect()
}

fn dot() -> View {
    view().size(px(6.0)).rounded_full().bg(OK).flex_shrink_0()
}

fn header(state: &Workspace, inset: Points) -> View {
    let name = state
        .account()
        .map_or_else(|| t("你的工作空间").into(), |account| account.name.clone());
    column()
        .child(
            row()
                .h(px(50.0))
                .px(inset)
                .gap(px(8.0))
                .bg(SURFACE)
                .border_b_1()
                .border_color(LINE)
                .child(dot())
                .child(label(name, 12.0, TEXT).truncate()),
        )
        .when(state.backend.preview, |header| {
            header.child(
                row()
                    .px(inset)
                    .py(px(8.0))
                    .gap(px(7.0))
                    .bg(SELECTED)
                    .child(icon("info", 14.0, ACCENT))
                    .child(
                        label(
                            t("交互预览 · 当前展示示例数据，桌面应用支持连接真实 S3 账户。"),
                            11.0,
                            ACCENT,
                        )
                        .flex_1()
                        .min_w_0(),
                    ),
            )
        })
}

fn bucket_heading(store: Store, state: &Workspace) -> View {
    let title = if state.bucket.is_empty() {
        t("欢迎来到 S3 Studio").into()
    } else {
        state.bucket.clone()
    };
    let region = state
        .current_bucket()
        .filter(|bucket| !bucket.connection.region.is_empty())
        .map(|bucket| bucket.connection.region.clone())
        .or_else(|| {
            state.account().map(|account| {
                state
                    .s3_defaults
                    .as_ref()
                    .unwrap_or(&s3_studio_core::accounts::S3Defaults::default())
                    .region_for(account)
                    .to_owned()
            })
        })
        .unwrap_or_else(|| t("未连接").into());
    column()
        .gap(px(5.0))
        .child(
            row()
                .justify_between()
                .gap(px(12.0))
                .child(
                    label(title, 26.0, INK)
                        .truncate()
                        .flex_1()
                        .min_w_0()
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .child(
                    primary("upload", t("上传文件"), false)
                        .disabled(state.disabled())
                        .on_press(store.open(Dialog::Upload)),
                ),
        )
        .child(
            row()
                .gap(px(8.0))
                .child(icon("database", 15.0, MUTED))
                .child(
                    label(
                        if state.bucket.is_empty() {
                            t("尚未选择存储桶")
                        } else {
                            &state.bucket
                        }
                        .to_owned(),
                        11.0,
                        MUTED,
                    )
                    .truncate(),
                )
                .child(dot())
                .child(label(region, 11.0, MUTED))
                .when(state.current_bucket().is_some(), |r| {
                    r.child(
                        icon_button("bucket-settings", t("存储桶设置"), "settings-2")
                            .disabled(state.is_busy())
                            .on_press(store.open(Dialog::BucketSettings)),
                    )
                }),
        )
}

fn footer(store: Store) -> View {
    let can_open = move || store.read(Workspace::can_open_app_dialog);
    row().justify_end().child(
        button("update", t("检查应用更新"))
            .px(px(6.0))
            .h(px(26.0))
            .disabled(move || !can_open())
            .child(label(
                concat!("S3 Studio v", env!("CARGO_PKG_VERSION")),
                11.0,
                MUTED,
            ))
            .child(icon("refresh-cw", 12.0, MUTED))
            .child(when(
                move || store.read(|state| state.update_status.version.is_some()),
                move || {
                    label(
                        move || store.read(|_| SharedString::from(t("新版本"))),
                        11.0,
                        ACCENT,
                    )
                },
            ))
            .on_press(store.open(Dialog::Update)),
    )
}
