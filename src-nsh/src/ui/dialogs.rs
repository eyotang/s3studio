use super::*;
use crate::{appearance, i18n, model};
use nsh_base::{SelectItem, combobox, dialog};
use s3_studio_core::accounts::S3Option;

pub(super) fn active(store: Store) -> AnyElement {
    let active = computed(move || {
        store.read(|state| {
            state
                .dialog
                .clone()
                .map(|dialog| (dialog, state.dialog_revision))
        })
    });
    dynamic(move || {
        active.get().map(|(current, revision)| {
            dialog()
                .key(revision.to_string())
                .label(move || {
                    store.read(|state| {
                        SharedString::from(state.dialog.as_ref().map_or("", Dialog::title))
                    })
                })
                .on_cancel(store.action(Workspace::close_dialog))
                .child(
                    view()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .p(px(18.0))
                        .child(
                            view()
                                .absolute()
                                .inset_0()
                                .bg(SCRIM.opacity(0.32))
                                .on_press(store.action(Workspace::close_dialog)),
                        )
                        .child(
                            scroll()
                                .test_id("modal")
                                .w(px(if matches!(current, Dialog::Trash | Dialog::Versions) {
                                    700.0
                                } else if matches!(current, Dialog::Account(_)) {
                                    510.0
                                } else {
                                    460.0
                                }))
                                .max_w(percentage(1.0))
                                .max_h(percentage(1.0))
                                .rounded(px(14.0))
                                .border_1()
                                .border_color(LINE)
                                .bg(SURFACE)
                                .shadow_lg()
                                .child(dynamic(move || {
                                    if matches!(current, Dialog::Update | Dialog::CancelTransfer(_))
                                    {
                                        store.read_transfers(|state| body(store, state, &current))
                                    } else {
                                        store.read(|state| body(store, state, &current))
                                    }
                                })),
                        ),
                )
        })
    })
}

fn body(store: Store, state: &Workspace, dialog: &Dialog) -> View {
    if state.dialog.as_ref() != Some(dialog) {
        return column();
    }
    let disabled = state.is_busy();
    let title = if dialog == &Dialog::Update {
        state.update_status.version.as_ref().map_or_else(
            || dialog.title().into(),
            |version| format!("S3 Studio {version}"),
        )
    } else {
        dialog.title().to_owned()
    };
    let mut body = column().gap(px(13.0)).p(px(24.0)).child(
        row()
            .justify_between()
            .gap(px(10.0))
            .child(
                row()
                    .gap(px(9.0))
                    .flex_1()
                    .min_w_0()
                    .when(dialog == &Dialog::About, |row| row.child(brand(37.0)))
                    .child(
                        label(title, 20.0, INK)
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD),
                    ),
            )
            .child(
                icon_button("close-dialog", t("关闭弹窗"), "x")
                    .disabled(disabled || (dialog == &Dialog::Update && state.update_pending))
                    .on_press(store.action(Workspace::close_dialog)),
            ),
    );
    if let Some(copy) = description(state, dialog) {
        body = body.child(label(copy, 12.0, MUTED));
    }
    body.child(form(store, state, dialog))
        .when(!state.dialog_error.is_empty(), |body| {
            body.child(error_notice(&state.dialog_error))
        })
        .child(divider())
        .child(actions(store, state, dialog))
}

fn description(state: &Workspace, dialog: &Dialog) -> Option<String> {
    Some(match dialog {
        Dialog::Versions => {
            t("选择历史数据版本还原为新的当前版本；已有版本保留，当前内容将被替换。").into()
        }
        Dialog::Trash => {
            t("显示已删除对象，还原后回到原路径。未启用版本控制或历史数据已清理的对象无法恢复。")
                .into()
        }
        Dialog::RemoveS3Option(..) => {
            t("确认删除此全局选项？如果它是当前默认值，将同时清空默认值；保存设置后生效。").into()
        }
        Dialog::ClearTransfers => {
            t("确认清除所有已完成及已取消的传输记录？只清除记录，不删除本地文件或存储桶对象。")
                .into()
        }
        Dialog::Account(_) => t("所有凭据仅保存于本机系统密钥库，账户数量没有限制。").into(),
        Dialog::Bucket => {
            t("在当前工作空间的 Endpoint 创建存储桶；Region 留空时使用工作空间或全局默认值。")
                .into()
        }
        Dialog::LocatePath => {
            t("输入从桶根目录开始的路径，或用 s3:// 指定存储桶。目录路径可用 / 结尾。").into()
        }
        Dialog::Copy => t("输入目标对象的完整路径，复制到当前存储桶。").into(),
        Dialog::Delete => {
            let chosen = state.chosen();
            let folders = chosen.iter().filter(|object| object.folder).count();
            if folders > 0 {
                crate::lf!(
                    "将递归删除 {folders} 个目录及其中所有文件和子目录，并删除 {} 个所选文件。未启用版本控制的对象无法恢复。",
                    chosen.len() - folders,
                    folders = folders
                )
            } else {
                crate::lf!(
                    "即将删除 {} 个文件。未启用版本控制的对象无法恢复。",
                    chosen.len()
                )
            }
        }
        Dialog::Upload => {
            t("接下来选择本地文件。当前目录中同名的对象将被覆盖；大文件自动分片上传。").into()
        }
        Dialog::RemoveAccount(id) => crate::lf!(
            "仅删除「{name}」保存在本机的工作空间配置和凭据。",
            name = state
                .account_by_id(id)
                .map_or("", |account| account.name.as_str())
        ),
        Dialog::S3Defaults => {
            t("设置全局默认值，并管理账户和存储桶可选择的 Endpoint 和 Region。").into()
        }
        Dialog::CancelTransfer(id) => crate::lf!(
            "{} 的断点将被清理；已经存在的源文件和目标文件不受影响。",
            state
                .transfers
                .iter()
                .find(|transfer| transfer.id == *id)
                .map_or("", |transfer| transfer.name.as_str())
        ),
        Dialog::Update => crate::lf!(
            "当前版本 {}{}",
            state.update_status.current_version,
            if state.update_status.version.is_some() {
                t(" · 有新版本可用")
            } else {
                ""
            }
        ),
        Dialog::Folder | Dialog::About | Dialog::Help | Dialog::TransferSettings => return None,
    })
}

fn form(store: Store, state: &Workspace, dialog: &Dialog) -> Option<AnyElement> {
    let body = match dialog {
        Dialog::RemoveS3Option(field, value) => column()
            .child(label(if field == "endpoint" { "Endpoint" } else { "Region" }, 12.0, MUTED))
            .child(label(value.clone(), 12.0, TEXT).selectable(true)),
        Dialog::Trash => trash_contents(store, state),
        Dialog::Versions => version_contents(store, state),
        Dialog::Account(_) => column().gap(px(11.0))
            .child(field(store, state, "name", t("账户名称"), true))
            .child(connection(store, state, S3Option::Endpoint, "Endpoint", false))
            .child(connection(store, state, S3Option::Region, "Region", false))
            .child(row().gap(px(10.0)).items_stretch()
                .child(field(store, state, "accessKey", "Access Key ID", false).flex_1())
                .child(field(store, state, "secretKey", "Secret Access Key", false).flex_1()))
            .child(field(store, state, "sessionToken", t("Session Token  可选 · 临时凭据"), false))
            .child(row().gap(px(7.0)).flex_wrap()
                .child(checkbox("pathStyle", t("使用路径风格访问"), state.form.path_style, false)
                    .disabled(state.is_busy()).gap(px(7.0)).items_center()
                    .child(label(t("使用路径风格访问"), 11.0, TEXT))
                    .on_change(move |checked: &bool| store.change(|state| state.form.path_style = *checked)))
                .child(label(t("MinIO 等服务推荐开启"), 10.0, MUTED))),
        Dialog::Folder => field(store, state, "name", t("文件夹名称"), true),
        Dialog::LocatePath => column().gap(px(10.0))
            .child(field(store, state, "path", t("完整路径"), true))
            .child(checkbox("locate-object-only", t("仅定位对象"), state.form.locate_object_only, false)
                .disabled(state.is_busy()).gap(px(7.0)).items_center()
                .child(label(t("仅定位对象"), 11.0, TEXT))
                .on_change(move |checked: &bool| store.change(|state| {
                    state.form.locate_object_only = *checked;
                    state.dialog_error.clear();
                }))),
        Dialog::Copy => field(store, state, "name", t("目标对象键"), true),
        Dialog::Bucket | Dialog::S3Defaults => column().gap(px(11.0))
            .when(dialog == &Dialog::Bucket, |body| body.child(field(store, state, "name", t("存储桶名称"), true)))
            .when(dialog == &Dialog::Bucket, |body| body.child(column().gap(px(4.0))
                .child(label("Endpoint", 12.0, TEXT))
                .child(label(state.account().map(|account| state.form.s3_options.endpoint_for(account))
                    .filter(|endpoint| !endpoint.is_empty()).unwrap_or(t("使用默认值")).to_owned(), 12.0, MUTED))))
            .when(dialog == &Dialog::S3Defaults, |body| body.child(connection(store, state, S3Option::Endpoint, "Endpoint", true)))
            .child(connection(store, state, S3Option::Region, "Region", false)),
        Dialog::TransferSettings => preferences(store, state),
        Dialog::Delete => column().child(
            scroll().test_id("delete-list").max_h(px(158.0)).bg(SUBTLE).rounded(px(7.0))
                .child(column().p(px(9.0)).gap(px(6.0)).child(state.chosen().iter().map(|object| {
                    row().gap(px(7.0))
                        .child(icon(if object.folder { "folder" } else { "file" }, 16.0, if object.folder { FOLDER } else { MUTED }))
                        .child(label(model::object_name(&object.key, &state.prefix).to_owned(), 11.0, TEXT).flex_1().min_w_0())
                }).collect::<Vec<_>>())),
        ),
        Dialog::Help => column().gap(px(10.0)).child([
            t("① 添加账户：填写 Access Key 和 Secret Key。MinIO 等服务可以配置 Endpoint 和路径风格。"),
            t("② 选择存储桶，双击文件夹浏览。选择文件后可下载、复制、分享或删除。"),
            t("③ 使用 ⌘ / Ctrl + F 搜索，Escape 关闭弹窗或取消选择。"),
            t("删除目录会同时删除其中所有文件和子目录。分享链接有效期 1 小时。传输活动支持暂停、继续和失败重试；关闭应用后，再次打开可从断点继续。"),
            t("首次使用需具备相应的 S3 权限；Ubuntu 需启动支持 Secret Service 的密钥库。"),
        ].map(|message| label(message, 12.0, TEXT)))
            .child(row().gap(px(8.0))
                .child(label(t("设置"), 12.0, TEXT))
                .child(label(nsh_core::element::key_label("mod+,"), 12.0, MUTED))),
        Dialog::About => column().gap(px(6.0))
            .child(label(crate::lf!("版本 {}", env!("CARGO_PKG_VERSION")), 14.0, TEXT).test_id("about-version"))
            .child(label(t("跨平台 S3 文件工作空间"), 12.0, MUTED)),
        Dialog::Update => update_status(state),
        Dialog::Upload | Dialog::RemoveAccount(_) | Dialog::CancelTransfer(_) | Dialog::ClearTransfers => return None,
    };
    Some(body.into_element())
}

fn field(
    store: Store,
    state: &Workspace,
    key: &'static str,
    title: &'static str,
    focus: bool,
) -> View {
    column()
        .min_w_0()
        .gap(px(4.0))
        .child(label(title, 12.0, TEXT))
        .child(form_input(store, state, key, title, focus))
}

fn form_input(
    store: Store,
    state: &Workspace,
    key: &'static str,
    title: &'static str,
    focus: bool,
) -> Input {
    let field = &state.form.inputs[key];
    input_field(
        key,
        title,
        i18n::translate_placeholder(&field.placeholder),
        move || store.read_draft(|state| state.input(key).to_owned()),
    )
    .masked(field.masked)
    .auto_focus(focus)
    .readonly(state.is_busy())
    .on_change(move |value: &str| store.edit(key, value))
    .on_submit(move |value: &str| {
        store.change(|state| {
            state.set_input(key, value);
            state.submit();
        })
    })
}

fn connection(
    store: Store,
    state: &Workspace,
    option: S3Option,
    title: &'static str,
    focus: bool,
) -> View {
    let key = option.key();
    let saved = state.form.s3_options.options(option);
    let normalized = move |state: &Workspace| {
        let value = state.input(key);
        option
            .normalize(value.to_owned())
            .unwrap_or_else(|_| value.trim().to_owned())
    };
    let options = move || {
        store.read_draft(|state| {
            let mut values = state.form.s3_options.options(option).to_vec();
            let current = normalized(state);
            if !current.is_empty() && !values.contains(&current) {
                values.push(current);
            }
            std::iter::once(SelectItem::new("", t("使用默认值")))
                .chain(
                    values
                        .into_iter()
                        .map(|item| SelectItem::new(item.clone(), item)),
                )
                .collect::<Vec<_>>()
        })
    };
    let _ = focus;
    column()
        .gap(px(4.0))
        .child(label(title, 12.0, TEXT))
        .child(
            combobox()
                .test_id(&format!("s3-options-{key}"))
                .input_test_id(key)
                .items(options)
                .value(move || Some(vec![SharedString::from(store.read_draft(normalized))]))
                .placeholder(i18n::translate_placeholder(
                    &state.form.inputs[key].placeholder,
                ))
                .disabled(state.is_busy())
                .accessibility_label(title)
                .w_full()
                .rounded(px(7.0))
                .border_1()
                .border_color(LINE)
                .bg(SUBTLE)
                .input_style(|s| {
                    s.flex_1()
                        .min_w_0()
                        .h(px(36.0))
                        .px(px(10.0))
                        .text_size(px(12.0))
                        .text_color(TEXT)
                })
                .trigger_button(
                    row()
                        .size(px(36.0))
                        .flex_shrink(0.0)
                        .justify_center()
                        .child(icon("chevron-down", 15.0, MUTED)),
                )
                .offset(px(4.0))
                .popup_style(|s| {
                    s.bg(SURFACE)
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(LINE)
                        .p(px(4.0))
                        .shadow_md()
                })
                .item_class("rounded-md focus:bg-[#f7f9fc] dark:focus:bg-[#2a2e38] selected:bg-[#e7f1ff] dark:selected:bg-[#233d5a] selected:text-[#2166b7] dark:selected:text-[#97c7ff]")
                .item_style(|s| {
                    s.p(px(7.0))
                        .text_size(px(12.0))
                        .text_color(TEXT)
                })
                .on_query_change(move |value: &SharedString| store.edit(key, value.as_str()))
                .on_change(move |values: &Vec<SharedString>| {
                    if let Some(value) = values.first() {
                        store.change(|state| state.set_connection_value(option, value.as_str()));
                    }
                })
                .footer(when(
                    move || {
                        store.read_draft(|state| {
                            let value = normalized(state);
                            !value.is_empty()
                                && !state.form.s3_options.options(option).contains(&value)
                        })
                    },
                    move || {
                        secondary(&format!("add-s3-option-{key}"), t("新增"))
                            .disabled(move || store.read(Workspace::is_busy))
                            .on_press(store.action(move |state| state.add_s3_option(option)))
                    },
                )),
        )
        .when(
            state.dialog == Some(Dialog::S3Defaults) && !saved.is_empty(),
            |body| {
                body.child(
                    scroll().max_h(px(91.0)).bg(SUBTLE).rounded(px(6.0)).child(
                        column().px(px(5.0)).child(
                            saved
                                .iter()
                                .map(|value| {
                                    let value = value.clone();
                                    row()
                                        .key(value.clone())
                                        .gap(px(6.0))
                                        .child(
                                            label(value.clone(), 11.0, MUTED)
                                                .flex_1()
                                                .min_w_0()
                                                .truncate(),
                                        )
                                        .child(
                                            icon_button(
                                                &format!("delete-s3-option-{key}-{value}"),
                                                format!("{} · {value}", t("删除选项")),
                                                "trash",
                                            )
                                            .disabled(state.is_busy())
                                            .on_press(
                                                store.action(move |state| {
                                                    state.remove_s3_option(option, &value)
                                                }),
                                            ),
                                        )
                                })
                                .collect::<Vec<_>>(),
                        ),
                    ),
                )
            },
        )
}

fn preferences(store: Store, state: &Workspace) -> View {
    let language = i18n::preference();
    let languages = i18n::Language::ALL.map(|option| {
        let code = if option == i18n::Language::Chinese {
            "zh"
        } else {
            option.code()
        };
        secondary(&format!("settings-language-{code}"), option.name())
            .disabled(state.is_busy())
            .selected(option == language)
            .selected_style(|s| s.border_color(ACCENT).bg(SELECTED))
            .on_press(store.action(move |state| {
                if let Err(error) = i18n::select(option) {
                    state.dialog_error = error.to_string();
                }
            }))
    });
    let themes = [
        ("theme-system", t("跟随系统"), Appearance::System),
        ("theme-light", t("亮色"), Appearance::Light),
        ("theme-dark", t("暗色"), Appearance::Dark),
    ]
    .map(|(id, name, preference)| {
        secondary(id, name)
            .disabled(state.is_busy())
            .selected(state.appearance == preference)
            .selected_style(|s| s.border_color(ACCENT).bg(SELECTED))
            .on_press(
                store.action(move |state| match appearance::select(preference) {
                    Ok(()) => state.appearance = preference,
                    Err(error) => state.dialog_error = error.to_string(),
                }),
            )
    });

    column()
        .gap(px(9.0))
        .when(state.s3_defaults.is_some(), |body| {
            body.child(
                secondary("s3-defaults", t("S3 全局默认值"))
                    .disabled(state.is_busy())
                    .on_press(store.open(Dialog::S3Defaults)),
            )
        })
        .child(label(t("语言"), 12.0, TEXT))
        .child(row().gap(px(5.0)).flex_wrap().child(languages))
        .child(label(t("主题"), 12.0, TEXT))
        .child(row().gap(px(5.0)).flex_wrap().child(themes))
        .child(label(t("语言和主题立即生效并自动保存。"), 11.0, MUTED))
        .child(field(store, state, "concurrency", t("并发数量"), true))
        .child(label(
            t("支持 1–32 路。点击保存设置后生效；降低上限时，已开始的分片会先完成。"),
            11.0,
            MUTED,
        ))
}

fn actions(store: Store, state: &Workspace, dialog: &Dialog) -> View {
    let actions = row().justify_end().gap(px(7.0)).flex_wrap();
    match dialog {
        Dialog::Versions => actions
            .when(state.versions.from_trash, |row| {
                row.child(
                    secondary("versions-back", t("返回回收站"))
                        .disabled(state.is_busy())
                        .on_press(store.action(Workspace::open_trash)),
                )
            })
            .child(
                secondary("versions-close", t("关闭"))
                    .disabled(state.is_busy())
                    .on_press(store.action(Workspace::close_dialog)),
            )
            .child(
                primary("versions-restore", t("还原所选版本"), false)
                    .disabled(
                        state.is_busy()
                            || state.versions.loading
                            || state.versions.selected.is_none()
                            || state.versions.expected.is_empty(),
                    )
                    .on_press(store.action(Workspace::restore_version)),
            ),
        Dialog::Trash => actions
            .child(
                secondary("trash-close", t("关闭"))
                    .disabled(state.is_busy())
                    .on_press(store.action(Workspace::close_dialog)),
            )
            .child(
                primary("trash-restore", t("还原所选对象"), false)
                    .disabled(
                        state.is_busy() || state.trash.loading || state.trash.selected.is_empty(),
                    )
                    .on_press(store.action(Workspace::restore_trash)),
            ),
        Dialog::Help => actions.child(
            primary("help-start", t("开始使用"), false)
                .on_press(store.action(Workspace::close_dialog)),
        ),
        Dialog::About => actions
            .child(
                secondary("about-check-update", t("检查更新…"))
                    .on_press(store.action(Workspace::check_for_updates)),
            )
            .child(
                primary("about-close", t("关闭"), false)
                    .on_press(store.action(Workspace::close_dialog)),
            ),
        Dialog::Update => {
            let status = &state.update_status;
            let in_app = status.mode == "inApp";
            actions
                .child(
                    secondary("check-update", t("检查更新"))
                        .disabled(
                            state.update_pending
                                || status.installing()
                                || status.mode == "disabled"
                                || status.phase == "checking",
                        )
                        .on_press(store.action(|state| state.update_action("check_app_update"))),
                )
                .when(status.version.is_some(), |actions| {
                    actions.child(
                        primary(
                            "install-update",
                            if status.installing() {
                                t("正在更新…")
                            } else if in_app {
                                t("下载并安装")
                            } else {
                                t("打开下载页")
                            },
                            false,
                        )
                        .disabled(
                            state.update_pending
                                || state.is_busy()
                                || (in_app && state.transfers.iter().any(model::active)),
                        )
                        .on_press(store.action(move |state| {
                            state.update_action(if in_app {
                                "install_app_update"
                            } else {
                                "open_update_download_page"
                            })
                        })),
                    )
                })
        }
        _ => actions
            .child(
                secondary(
                    "cancel-dialog",
                    if matches!(dialog, Dialog::CancelTransfer(_)) {
                        t("保留任务")
                    } else {
                        t("取消")
                    },
                )
                .disabled(state.is_busy())
                .on_press(store.action(Workspace::close_dialog)),
            )
            .child(
                primary(
                    "submit-dialog",
                    if state.is_busy() {
                        t("处理中…")
                    } else {
                        dialog.submit_label()
                    },
                    dialog.destructive(),
                )
                .disabled(state.is_busy())
                .on_press(store.action(Workspace::submit)),
            ),
    }
}

fn update_status(state: &Workspace) -> View {
    let status = &state.update_status;
    let error = if state.update_error.is_empty() {
        status.error.as_deref().unwrap_or("")
    } else {
        &state.update_error
    };
    let message = match (status.mode, status.phase) {
        ("disabled", _) => t("开发模式与网页预览不检查更新。安装发布版后即可使用。"),
        (_, "checking") => t("正在检查新版本…"),
        (_, "installing") => t("完整性校验通过，正在安装更新…"),
        (_, "downloading") => t("正在下载并校验更新包…"),
        _ if status.last_checked.is_some() && status.version.is_none() && error.is_empty() => {
            t("已是最新版本。")
        }
        _ => "",
    };
    column()
        .gap(px(9.0))
        .when(!message.is_empty(), |body| {
            body.child(label(message, 12.0, TEXT))
        })
        .when(status.phase == "downloading", |body| {
            let fraction = status
                .total
                .filter(|total| *total > 0)
                .map_or(0.0, |total| status.downloaded as f32 / total as f32);
            body.child(progress(fraction, BLUE)).child(label(
                format!(
                    "{}{}",
                    model::bytes(status.downloaded.into()),
                    status
                        .total
                        .map(|size| format!(" / {}", model::bytes(size.into())))
                        .unwrap_or_default()
                ),
                11.0,
                MUTED,
            ))
        })
        .when_some(
            status
                .last_checked
                .and_then(|time| chrono::DateTime::from_timestamp_millis(time as i64)),
            |body, time| {
                body.child(label(
                    crate::lf!(
                        "上次检查：{}",
                        time.with_timezone(&chrono::Local)
                            .format("%Y/%-m/%-d %H:%M:%S")
                    ),
                    11.0,
                    MUTED,
                ))
            },
        )
        .when_some(status.notes.as_ref(), |body, notes| {
            body.child(
                scroll()
                    .test_id("update-notes")
                    .max_h(px(215.0))
                    .rounded(px(7.0))
                    .bg(SUBTLE)
                    .child(label(notes.clone(), 12.0, TEXT).selectable(true).p(px(9.0))),
            )
        })
        .when(!error.is_empty(), |body| body.child(error_notice(error)))
        .when(status.version.is_some() && !status.installing(), |body| {
            body.child(label(
                if status.mode == "inApp" {
                    t("确认后下载更新包，通过完整性校验后安装并重启应用。")
                } else {
                    t("当前安装方式需前往发布页下载新版本。账户配置与传输断点会保留。")
                },
                11.0,
                MUTED,
            ))
        })
        .when(
            status.version.is_some()
                && status.mode == "inApp"
                && state.transfers.iter().any(model::active),
            |body| body.child(error_notice(t("请先暂停或完成所有传输，再安装更新。"))),
        )
}

fn trash_contents(store: Store, state: &Workspace) -> View {
    let trash = &state.trash;
    let all = !trash.objects.is_empty()
        && trash
            .objects
            .iter()
            .all(|o| trash.selected.contains(&o.key));
    column()
        .gap(px(10.0))
        .child(
            row()
                .justify_between()
                .child(label(state.bucket.clone(), 12.0, TEXT))
                .child(
                    secondary("trash-refresh", t("刷新"))
                        .disabled(state.is_busy() || trash.loading)
                        .on_press(store.action(|s| s.load_trash(false))),
                ),
        )
        .when(!trash.notice.is_empty(), |body| {
            body.child(label(trash.notice.clone(), 12.0, OK))
        })
        .when(trash.versioning == Some(false), |body| {
            body.child(label(
                t("此存储桶未启用版本控制，无法查看可还原对象。"),
                12.0,
                MUTED,
            ))
        })
        .when(!trash.objects.is_empty(), |body| {
            body.child(
                checkbox(
                    "trash-select-all",
                    t("全选"),
                    all,
                    !all && !trash.selected.is_empty(),
                )
                .disabled(state.is_busy() || trash.loading)
                .gap(px(7.0))
                .child(label(t("全选"), 12.0, TEXT))
                .on_change(move |checked: &bool| {
                    store.change(|s| {
                        s.trash.selected = if *checked {
                            s.trash.objects.iter().map(|o| o.key.clone()).collect()
                        } else {
                            Default::default()
                        };
                    })
                }),
            )
            .child(
                scroll().max_h(px(320.0)).child(
                    column().gap(px(6.0)).child(
                        trash
                            .objects
                            .iter()
                            .map(|object| {
                                let key = object.key.clone();
                                row()
                                    .gap(px(8.0))
                                    .p(px(7.0))
                                    .bg(SUBTLE)
                                    .rounded(px(5.0))
                                    .child(
                                        checkbox(
                                            &format!("trash-check-{key}"),
                                            key.clone(),
                                            trash.selected.contains(&key),
                                            false,
                                        )
                                        .disabled(state.is_busy() || trash.loading)
                                        .on_change(
                                            move |checked: &bool| {
                                                store.change(|s| {
                                                    if *checked {
                                                        s.trash.selected.insert(key.clone());
                                                    } else {
                                                        s.trash.selected.remove(&key);
                                                    }
                                                })
                                            },
                                        ),
                                    )
                                    .child(
                                        icon_button(
                                            &format!("trash-versions-{}", object.key),
                                            t("版本历史"),
                                            "history",
                                        )
                                        .disabled(state.is_busy())
                                        .on_press({
                                            let key = object.key.clone();
                                            store.action(move |s| s.open_versions(key.clone()))
                                        }),
                                    )
                                    .child(icon("file", 16.0, MUTED))
                                    .child(
                                        column()
                                            .flex_1()
                                            .min_w_0()
                                            .gap(px(3.0))
                                            .child(label(object.key.clone(), 12.0, TEXT).truncate())
                                            .child(label(
                                                format!("{}: {}", t("删除时间"), object.deleted),
                                                11.0,
                                                MUTED,
                                            )),
                                    )
                            })
                            .collect::<Vec<_>>(),
                    ),
                ),
            )
        })
        .when(trash.loading, |body| {
            body.child(label(t("正在加载文件"), 12.0, MUTED))
        })
        .when(
            !trash.loading && trash.objects.is_empty() && trash.versioning == Some(true),
            |body| {
                body.child(label(
                    if trash.cursor.is_some() {
                        t("当前页没有已删除对象，可继续加载。")
                    } else {
                        t("回收站为空。")
                    },
                    12.0,
                    MUTED,
                ))
            },
        )
        .when(trash.cursor.is_some(), |body| {
            body.child(
                secondary("trash-load-more", t("加载更多对象"))
                    .disabled(state.is_busy() || trash.loading)
                    .on_press(store.action(|s| s.load_trash(true))),
            )
        })
}

fn version_contents(store: Store, state: &Workspace) -> View {
    let versions = &state.versions;
    column()
        .gap(px(10.0))
        .child(label(versions.key.clone(), 12.0, TEXT))
        .child(
            secondary("versions-refresh", t("刷新"))
                .disabled(state.is_busy() || versions.loading)
                .on_press(store.action(|s| s.load_versions(false))),
        )
        .when(!versions.notice.is_empty(), |body| {
            body.child(label(versions.notice.clone(), 12.0, OK))
        })
        .child(
            scroll().max_h(px(350.0)).child(
                column().gap(px(6.0)).child(
                    versions
                        .entries
                        .iter()
                        .map(|version| {
                            let id = version.id.clone();
                            row()
                                .gap(px(8.0))
                                .p(px(8.0))
                                .bg(SUBTLE)
                                .rounded(px(5.0))
                                .child(
                                    checkbox(
                                        &format!("version-check-{id}"),
                                        id.clone(),
                                        versions.selected.as_ref() == Some(&id),
                                        false,
                                    )
                                    .disabled(
                                        state.is_busy()
                                            || versions.loading
                                            || version.deleted
                                            || version.latest,
                                    )
                                    .on_change(
                                        move |checked: &bool| {
                                            store.change(|s| {
                                                s.versions.selected = checked.then(|| id.clone())
                                            })
                                        },
                                    ),
                                )
                                .child(
                                    column()
                                        .gap(px(4.0))
                                        .flex_1()
                                        .min_w_0()
                                        .child(label(
                                            format!("{}: {}", t("版本 ID"), version.id),
                                            11.0,
                                            TEXT,
                                        ))
                                        .child(label(
                                            format!(
                                                "{} · {}{}",
                                                version.modified,
                                                if version.deleted {
                                                    t("删除标记").into()
                                                } else {
                                                    model::bytes(
                                                        version.size.unwrap_or(0).max(0) as u128
                                                    )
                                                },
                                                if version.latest {
                                                    format!(" · {}", t("当前版本"))
                                                } else {
                                                    String::new()
                                                }
                                            ),
                                            11.0,
                                            MUTED,
                                        )),
                                )
                        })
                        .collect::<Vec<_>>(),
                ),
            ),
        )
        .when(versions.loading, |body| {
            body.child(label(t("正在加载文件"), 12.0, MUTED))
        })
        .when(!versions.loading && versions.entries.is_empty(), |body| {
            body.child(label(t("没有可查看的版本。"), 12.0, MUTED))
        })
        .when(versions.cursor.is_some(), |body| {
            body.child(
                secondary("versions-load-more", t("加载更多对象"))
                    .disabled(state.is_busy() || versions.loading)
                    .on_press(store.action(|s| s.load_versions(true))),
            )
        })
}
