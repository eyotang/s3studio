use super::*;
use gpui::base::FocusTrapElement;
impl Workspace {
    fn field(&self, key: &'static str, title: &str) -> AnyElement {
        let Some(input) = self.form.inputs.get(key) else {
            return div().into_any_element();
        };
        col()
            .flex_1()
            .min_w_0()
            .mb(px(16.))
            .child(label(title.to_owned(), 12., 0x778294))
            .child(
                Input::new(input)
                    .id(key)
                    .aria_label(title.to_owned())
                    .mt(px(7.))
                    .h(px(38.))
                    .text_size(px(12.))
                    .bg(rgb(0xfcfdfe))
                    .border_color(rgb(0xe0e5ec))
                    .rounded(px(7.)),
            )
            .into_any_element()
    }
    pub(super) fn render_dialog(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(dialog) = self.dialog.clone() else {
            return div().into_any_element();
        };
        let account = matches!(dialog, Dialog::Account(_));
        let description = match &dialog {
            Dialog::Account(_) => {
                crate::i18n::t("所有凭据仅保存于本机系统密钥库，账户数量没有限制。").into()
            }
            Dialog::Folder => crate::i18n::t("在当前目录中创建一个新的文件夹。").into(),
            Dialog::Bucket => crate::i18n::t("在当前账户和区域中创建新的 S3 存储桶。").into(),
            Dialog::Copy => crate::i18n::t("输入目标对象的完整路径，复制到当前存储桶。").into(),
            Dialog::Delete => {
                let chosen = self.chosen();
                let folders = chosen.iter().filter(|o| o.folder).count();
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
            Dialog::Upload => crate::i18n::t(
                "接下来选择本地文件。当前目录中同名的对象将被覆盖；大文件自动分片上传。",
            )
            .into(),
            Dialog::RemoveAccount => {
                crate::i18n::t("删除本机保存的账户配置和凭据，云端文件不受影响。").into()
            }
            Dialog::Help => crate::i18n::t("一个基于 Tauri 的跨平台 S3 工作空间。").into(),
            Dialog::About => crate::i18n::t("云端文件，井然有序。").into(),
            Dialog::TransferSettings => crate::i18n::t(
                "设置整个应用的并发上限，所有上传和下载任务共同使用。建议从默认 8 路开始。",
            )
            .into(),
            Dialog::CancelTransfer(id) => crate::lf!(
                "{} 的断点将被清理；已经存在的源文件和目标文件不受影响。",
                self.transfers
                    .iter()
                    .find(|t| &t.id == id)
                    .map(|t| t.name.as_str())
                    .unwrap_or("")
            ),
            Dialog::Update => crate::lf!(
                "当前版本 {}{}",
                self.update_status.current_version,
                if self.update_status.version.is_some() {
                    crate::i18n::t(" · 有新版本可用")
                } else {
                    ""
                }
            ),
        };
        let mut modal = col()
            .id("modal")
            .relative()
            .w(px(if account { 510. } else { 430. }))
            .max_h(window.viewport_size().height - px(50.))
            .overflow_y_scroll()
            .p(px(30.))
            .rounded(px(19.))
            .bg(rgba(0xfffffff9))
            .border_1()
            .border_color(rgba(0xffffffc9))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div().absolute().top(px(19.)).right(px(19.)).child(
                    icon_button(
                        "close-dialog",
                        if dialog == Dialog::Update {
                            crate::i18n::t("关闭更新窗口")
                        } else {
                            crate::i18n::t("关闭弹窗")
                        },
                        "x",
                    )
                    .disabled(self.is_busy() || (dialog == Dialog::Update && self.update_pending))
                    .on_click(cx.listener(|this, _, window, cx| this.close_dialog(window, cx))),
                ),
            )
            .when(!matches!(dialog, Dialog::CancelTransfer(_)), |s| {
                s.child(
                    row()
                        .size(px(50.))
                        .justify_center()
                        .rounded(px(14.))
                        .mb(px(19.))
                        .text_color(rgb(0x4696f5))
                        .bg(linear_gradient(
                            145.,
                            linear_color_stop(rgb(0xe3efff), 0.),
                            linear_color_stop(rgb(0xf2f7ff), 1.),
                        ))
                        .when(dialog == Dialog::About, |s| {
                            s.child(img("brand.svg").size(px(50.)))
                        })
                        .when(dialog != Dialog::About, |s| {
                            s.child(icon(
                                if account {
                                    "key-round"
                                } else if dialog == Dialog::Delete {
                                    "trash-2"
                                } else if dialog == Dialog::Update {
                                    "sparkles"
                                } else {
                                    "cloud"
                                },
                                26.,
                            ))
                        }),
                )
            })
            .child(
                label(
                    if dialog == Dialog::Update {
                        self.update_status
                            .version
                            .as_ref()
                            .map(|v| format!("S3 Studio {v}"))
                            .unwrap_or_else(|| crate::i18n::t("软件更新").into())
                    } else {
                        dialog.title().into()
                    },
                    21.,
                    0x252731,
                )
                .font_weight(FontWeight::SEMIBOLD)
                .mb(px(9.)),
            )
            .child(
                label(description, 12., 0x99a2b1)
                    .line_height(relative(1.8))
                    .mb(px(23.)),
            );
        match &dialog{
            Dialog::Account(id)=>{
                modal=modal.child(self.field("name",crate::i18n::t("账户名称"))).child(row().gap(px(13.)).items_start().child(self.field("accessKey","Access Key ID")).child(self.field("region","Region")))
                    .child(self.field("secretKey","Secret Access Key")).child(self.field("sessionToken",crate::i18n::t("Session Token  可选 · 临时凭据"))).child(self.field("endpoint",crate::i18n::t("Endpoint  可选 · S3 兼容服务")))
                    .child(row().gap(px(5.)).mb(px(16.)).child(Checkbox::new("pathStyle").accessibility_label(crate::i18n::t("使用路径风格访问")).checked(self.form.path_style).xsmall().on_click(cx.listener(|this,checked,_,cx|{this.form.path_style = *checked;cx.notify();}))).child(label(crate::i18n::t("使用路径风格访问"),10.,0x778294)).child(label(crate::i18n::t("MinIO 等服务推荐开启"),9.,0xadb5c3)))
                    .when(id.is_some(),|s|s.child(button("remove-account",crate::i18n::t("移除账户")).text_size(px(10.)).text_color(rgb(0xd18282)).justify_start().child(icon("trash-2",14.)).child(crate::i18n::t("移除账户")).on_click(cx.listener(|this,_,window,cx|this.open_dialog(Dialog::RemoveAccount,window,cx)))));
            },
            Dialog::Folder=>modal=modal.child(self.field("name",crate::i18n::t("文件夹名称"))),Dialog::Bucket=>modal=modal.child(self.field("name",crate::i18n::t("存储桶名称"))),Dialog::Copy=>modal=modal.child(self.field("name",crate::i18n::t("目标对象键"))),
            Dialog::TransferSettings=>modal=modal.child(self.preferences(cx)).child(self.field("concurrency",crate::i18n::t("并发数量"))).child(label(crate::i18n::t("支持 1–32 路。设置自动保存；降低上限时，已开始的分片会先完成，后续分片按新上限执行。"),12.,0x99a2b1).line_height(relative(1.8)).mb(px(23.))),
            Dialog::Delete=>modal=modal.child(col().id("delete-list").max_h(px(160.)).overflow_y_scroll().px(px(14.)).py(px(10.)).rounded(px(8.)).bg(rgb(0xfafbfd)).children(self.chosen().iter().map(|o|row().py(px(7.)).gap(px(7.)).child(file_icon(o,20.)).child(label(model::object_name(&o.key,&self.prefix).to_owned(),11.,0x808d9f))))),
            Dialog::Help=>modal=modal.children([
                crate::i18n::t("① 添加账户：填写 Access Key、Secret Key 和 Region。MinIO 等服务可以配置 Endpoint 和路径风格。"),
                crate::i18n::t("② 选择存储桶，双击文件夹浏览。选择文件后可下载、复制、分享或删除。"),
                crate::i18n::t("③ 使用 ⌘ / Ctrl + F 搜索，Escape 关闭弹窗或取消选择。"),
                crate::i18n::t("删除目录会同时删除其中所有文件和子目录。分享链接有效期 1 小时。传输活动支持暂停、继续和失败重试；关闭应用后，再次打开可从断点继续。"),
                crate::i18n::t("首次使用需具备相应的 S3 权限；Ubuntu 需启动支持 Secret Service 的密钥库。"),
            ].into_iter().map(|s|label(s,12.,0x8591a4).line_height(relative(1.9)).my(px(6.)))),
            Dialog::Update=>modal=modal.child(self.render_update_status()),
            Dialog::About=>modal=modal.child(label(crate::lf!("版本 {}", env!("CARGO_PKG_VERSION")),14.,0x4b5362).id("about-version"))
                .child(label(crate::i18n::t("跨平台 S3 文件工作空间"),12.,0x8591a4).mt(px(12.))),
            _=>{}
        }
        if !self.dialog_error.is_empty() {
            modal = modal.child(
                label(self.dialog_error.clone(), 11., 0xc8716c)
                    .line_height(relative(1.7))
                    .p(px(10.))
                    .rounded(px(7.))
                    .bg(rgb(0xfff2f1))
                    .border_1()
                    .border_color(rgb(0xffe2df)),
            );
        }
        if dialog == Dialog::Help {
            modal = modal.child(
                primary("help-start", crate::i18n::t("开始使用"))
                    .mt(px(20.))
                    .w_full()
                    .on_click(cx.listener(|this, _, window, cx| this.close_dialog(window, cx))),
            );
        } else if dialog == Dialog::About {
            modal = modal.child(
                row()
                    .justify_end()
                    .gap(px(9.))
                    .mt(px(24.))
                    .child(
                        secondary("about-check-update", crate::i18n::t("检查更新…")).on_click(
                            cx.listener(|this, _, window, cx| {
                                this.check_for_updates(&crate::CheckForUpdates, window, cx)
                            }),
                        ),
                    )
                    .child(primary("about-close", crate::i18n::t("关闭")).on_click(
                        cx.listener(|this, _, window, cx| this.close_dialog(window, cx)),
                    )),
            );
        } else if dialog == Dialog::Update {
            let status = &self.update_status;
            let mut actions =
                row()
                    .justify_end()
                    .gap(px(9.))
                    .border_t_1()
                    .border_color(rgb(0xedf0f4))
                    .pt(px(19.))
                    .mt(px(22.))
                    .child(
                        secondary("check-update", crate::i18n::t("检查更新"))
                            .disabled(
                                self.update_pending
                                    || status.installing()
                                    || status.mode == "disabled"
                                    || status.phase == "checking",
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update_action("check_app_update", cx)
                            })),
                    );
            if status.version.is_some() {
                actions = actions.child(
                    primary(
                        "install-update",
                        if status.installing() {
                            crate::i18n::t("正在更新…")
                        } else if status.mode == "inApp" {
                            crate::i18n::t("下载并安装")
                        } else {
                            crate::i18n::t("打开下载页")
                        },
                    )
                    .disabled(
                        self.update_pending
                            || self.is_busy()
                            || (status.mode == "inApp" && self.transfers.iter().any(model::active)),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.update_action(
                            if this.update_status.mode == "inApp" {
                                "install_app_update"
                            } else {
                                "open_update_download_page"
                            },
                            cx,
                        )
                    })),
                );
            }
            modal = modal.child(actions);
        } else {
            modal = modal.child(
                row()
                    .justify_end()
                    .gap(px(9.))
                    .border_t_1()
                    .border_color(rgb(0xedf0f4))
                    .pt(px(19.))
                    .mt(px(22.))
                    .child(
                        secondary(
                            "cancel-dialog",
                            if matches!(dialog, Dialog::CancelTransfer(_)) {
                                crate::i18n::t("保留任务")
                            } else {
                                crate::i18n::t("取消")
                            },
                        )
                        .disabled(self.is_busy())
                        .on_click(cx.listener(|this, _, window, cx| this.close_dialog(window, cx))),
                    )
                    .child(
                        primary(
                            "submit-dialog",
                            if self.is_busy() {
                                crate::i18n::t("处理中…")
                            } else {
                                dialog.submit_label()
                            },
                        )
                        .disabled(self.is_busy())
                        .when(dialog.destructive(), |s| s.bg(rgb(0xe55656)))
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            );
        }
        div()
            .id("modal-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p(px(25.))
            .bg(rgba(0x1c29433d))
            .on_click(cx.listener(|this, _, window, cx| this.close_dialog(window, cx)))
            .child(modal.focus_trap("modal-trap", &self.modal_focus))
            .into_any_element()
    }
    fn preferences(&self, cx: &mut Context<Self>) -> Div {
        let languages = [
            (
                "settings-language-system",
                crate::i18n::t("跟随系统"),
                crate::i18n::Language::System,
            ),
            (
                "settings-language-zh",
                "简体中文",
                crate::i18n::Language::Chinese,
            ),
            (
                "settings-language-en",
                "English",
                crate::i18n::Language::English,
            ),
        ];
        let themes = [
            (
                "theme-system",
                crate::i18n::t("跟随系统"),
                crate::appearance::Preference::System,
            ),
            (
                "theme-light",
                crate::i18n::t("亮色"),
                crate::appearance::Preference::Light,
            ),
            (
                "theme-dark",
                crate::i18n::t("暗色"),
                crate::appearance::Preference::Dark,
            ),
        ];
        col()
            .gap(px(12.))
            .mb(px(24.))
            .child(label(crate::i18n::t("语言"), 12., MUTED))
            .child(
                row()
                    .gap(px(6.))
                    .children(languages.into_iter().map(|(id, text, language)| {
                        secondary(id, text)
                            .when(crate::i18n::preference() == language, |s| {
                                s.border_color(gpui::rgb(BLUE))
                            })
                            .disabled(self.is_busy())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.change_language(language, window, cx)
                            }))
                    })),
            )
            .child(label(crate::i18n::t("主题"), 12., MUTED))
            .child(
                row()
                    .gap(px(6.))
                    .children(themes.into_iter().map(|(id, text, theme)| {
                        secondary(id, text)
                            .when(crate::appearance::preference() == theme, |s| {
                                s.border_color(gpui::rgb(BLUE))
                            })
                            .disabled(self.is_busy())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Err(error) = crate::appearance::select(theme, window, cx) {
                                    this.dialog_error = error.to_string();
                                }
                                cx.notify();
                            }))
                    })),
            )
            .child(label(
                crate::i18n::t("语言和主题立即生效并自动保存。"),
                11.,
                MUTED,
            ))
    }
    fn render_update_status(&self) -> AnyElement {
        let status = &self.update_status;
        let error = if self.update_error.is_empty() {
            status.error.as_deref().unwrap_or("")
        } else {
            &self.update_error
        };
        let message = if status.mode == "disabled" {
            crate::i18n::t("开发模式与网页预览不检查更新。安装发布版后即可使用。")
        } else if status.phase == "checking" {
            crate::i18n::t("正在检查新版本…")
        } else if status.phase == "installing" {
            crate::i18n::t("签名校验通过，正在安装更新…")
        } else if status.phase == "downloading" {
            crate::i18n::t("正在下载并校验更新包…")
        } else if status.last_checked.is_some() && status.version.is_none() && error.is_empty() {
            crate::i18n::t("已是最新版本。")
        } else {
            ""
        };
        let mut content = col()
            .gap(px(10.))
            .child(label(message, 13., 0x6c7480).line_height(relative(1.65)));
        if status.phase == "downloading" {
            let progress = status
                .total
                .filter(|total| *total > 0)
                .map(|total| (status.downloaded as f32 / total as f32).clamp(0., 1.))
                .unwrap_or(0.);
            content = content
                .child(
                    div()
                        .h(px(6.))
                        .w_full()
                        .bg(rgb(0xe7ebf1))
                        .rounded(px(3.))
                        .child(
                            div()
                                .h_full()
                                .w(relative(progress))
                                .bg(rgb(BLUE))
                                .rounded(px(3.)),
                        ),
                )
                .child(label(
                    format!(
                        "{}{}",
                        model::bytes(status.downloaded),
                        status
                            .total
                            .map(|total| format!(" / {}", model::bytes(total)))
                            .unwrap_or_default()
                    ),
                    12.,
                    0x6c7480,
                ));
        }
        if let Some(time) = status
            .last_checked
            .and_then(|t| chrono::DateTime::from_timestamp_millis(t as i64))
        {
            content = content.child(label(
                crate::lf!(
                    "上次检查：{}",
                    time.with_timezone(&chrono::Local)
                        .format("%Y/%-m/%-d %H:%M:%S")
                ),
                11.,
                0x99a2b1,
            ));
        }
        if let Some(notes) = &status.notes {
            content = content.child(
                div()
                    .id("update-notes")
                    .max_h(px(220.))
                    .overflow_y_scroll()
                    .p(px(14.))
                    .rounded(px(8.))
                    .bg(rgb(0xf7f9fc))
                    .child(label(notes.clone(), 12., 0x6c7480).line_height(relative(1.7))),
            );
        }
        if !error.is_empty() {
            content = content.child(
                label(error.to_owned(), 11., 0xc8716c)
                    .p(px(10.))
                    .bg(rgb(0xfff2f1)),
            );
        }
        if status.version.is_some() && !status.installing() {
            content = content.child(
                label(
                    if status.mode == "inApp" {
                        crate::i18n::t("确认后下载更新包，通过签名校验后安装并重启应用。")
                    } else {
                        crate::i18n::t(
                            "当前安装方式需前往发布页下载新版本。账户配置与传输断点会保留。",
                        )
                    },
                    12.,
                    0x99a2b1,
                )
                .line_height(relative(1.8)),
            );
        }
        if status.version.is_some()
            && status.mode == "inApp"
            && self.transfers.iter().any(model::active)
        {
            content = content.child(
                label(
                    crate::i18n::t("请先暂停或完成所有传输，再安装更新。"),
                    11.,
                    0xc8716c,
                )
                .p(px(10.))
                .bg(rgb(0xfff2f1)),
            );
        }
        content.into_any_element()
    }
}
