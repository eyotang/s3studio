mod dialogs;
mod transfers;
use crate::{
    model::{self, Dialog, Sort},
    workspace::Workspace,
};
use gpui::base::Button;
use gpui::component::{Icon, Sizable, checkbox::Checkbox, input::Input};
use gpui::{prelude::*, *};
use s3_studio_core::s3::S3Object;

pub(super) const BLUE: u32 = 0x007aff;
pub(super) const MUTED: u32 = 0x92949f;
pub(super) const LINE: u32 = 0xebecf0;
pub fn configure_theme(cx: &mut App) {
    gpui::component::Theme::change(gpui::component::ThemeMode::Light, None, cx);
    gpui::component::Theme::update(cx, |theme| {
        theme.primary = rgb(0x007aff).into();
        theme.primary_hover = rgb(0x006deb).into();
        theme.primary_active = rgb(0x006deb).into();
        theme.primary_foreground = rgb(0xffffff).into();
        theme.ring = rgb(0x007aff).into();
        theme.foreground = rgb(0x4b5362).into();
        theme.muted_foreground = rgb(0xaeb5c2).into();
        theme.input = rgb(0xe0e5ec).into();
    });
}

pub(super) fn row() -> Div {
    div().flex().items_center()
}
pub(super) fn col() -> Div {
    div().flex().flex_col()
}
pub(super) fn label(value: impl Into<SharedString>, size: f32, color: u32) -> Div {
    div()
        .text_size(px(size))
        .text_color(rgb(color))
        .child(value.into())
}
pub(super) fn icon(name: &str, size: f32) -> Icon {
    Icon::default()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
}
pub(super) fn dot() -> Div {
    div()
        .size(px(5.))
        .rounded_full()
        .bg(rgb(0x36b68a))
        .flex_shrink_0()
}
pub(super) fn button(id: impl Into<ElementId>, text: &str) -> Button {
    Button::new(id)
        .accessibility_label(text.to_owned())
        .flex()
        .items_center()
        .justify_center()
        .gap(px(7.))
        .rounded(px(6.))
        .text_size(px(12.))
        .cursor_pointer()
        .styles(|s| s.disabled(|s| s.opacity(0.4)))
}
pub(super) fn icon_button(id: impl Into<ElementId>, text: &str, name: &str) -> Button {
    button(id, text)
        .size(px(28.))
        .text_color(rgb(0x9299a6))
        .child(icon(name, 17.))
}
pub(super) fn primary(id: impl Into<ElementId>, text: &str) -> Button {
    button(id, text)
        .px(px(15.))
        .py(px(10.))
        .rounded(px(8.))
        .bg(rgb(BLUE))
        .text_color(rgb(0xffffff))
        .hover(|s| s.bg(rgb(0x006deb)))
        .child(text.to_owned())
}
pub(super) fn secondary(id: impl Into<ElementId>, text: &str) -> Button {
    button(id, text)
        .px(px(15.))
        .py(px(9.))
        .rounded(px(7.))
        .border_1()
        .border_color(rgb(0xe1e5ec))
        .bg(rgb(0xffffff))
        .text_color(rgb(0x737e90))
        .child(text.to_owned())
}
pub(super) fn file_icon(object: &S3Object, size: f32) -> AnyElement {
    let ext = object.key.rsplit('.').next().unwrap_or("").to_lowercase();
    let (name, color) = if object.folder {
        ("folder", 0x83b9f7)
    } else {
        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "webp" | "svg" => ("file-image", 0xb593d7),
            "zip" | "gz" | "tar" => ("file-archive", 0xd9ab73),
            "json" | "js" | "ts" | "html" => ("file-code-2", 0x76b4b0),
            "pdf" | "md" | "txt" => ("file-text", 0x91a4c0),
            _ => ("file", MUTED),
        }
    };
    if object.folder {
        svg()
            .path("folder-filled.svg")
            .size(px(size))
            .text_color(rgb(color))
            .into_any_element()
    } else {
        icon(name, size).text_color(rgb(color)).into_any_element()
    }
}
impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = window.viewport_size();
        let width = f32::from(size.width);
        let narrow = width <= 1050.;
        let wide = width >= 1450.;
        let sidebar = if narrow {
            215.
        } else if wide {
            268.
        } else {
            248.
        };
        let padding = if narrow {
            23.
        } else if wide {
            48.
        } else {
            34.
        };
        let content = self.render_workspace(window, cx, narrow, wide, padding);
        let mut root = row()
            .id("s3-workspace")
            .size_full()
            .relative()
            .items_stretch()
            .text_color(rgb(0x252731))
            .text_size(px(13.))
            .font_weight(FontWeight::NORMAL)
            .line_height(relative(1.2))
            .font_family(if cfg!(target_os = "macos") {
                ".AppleSystemUIFont"
            } else {
                "Segoe UI"
            })
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &crate::LanguageSystem, window, cx| {
                this.change_language(crate::i18n::Language::System, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::LanguageChinese, window, cx| {
                this.change_language(crate::i18n::Language::Chinese, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::LanguageEnglish, window, cx| {
                this.change_language(crate::i18n::Language::English, window, cx)
            }))
            .when(self.can_open_app_dialog(), |root| {
                root.on_action(cx.listener(Self::show_about))
                    .on_action(cx.listener(Self::check_for_updates))
            })
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = &event.keystroke;
                if (key.modifiers.platform || key.modifiers.control) && key.key == "f" {
                    this.search.update(cx, |s, cx| s.focus(window, cx));
                    cx.stop_propagation();
                }
                if key.key == "escape" && !this.is_busy() {
                    this.close_dialog(window, cx);
                    this.selected.clear();
                    this.selection_changed(cx);
                }
            }))
            .child(self.render_sidebar(cx, narrow, wide, sidebar))
            .child(content);
        if !self.toast.is_empty() {
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(28.))
                    .left_0()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        row()
                            .gap(px(10.))
                            .px(px(22.))
                            .py(px(14.))
                            .bg(rgba(0xfffffff5))
                            .border_1()
                            .border_color(rgb(0xe5eaf1))
                            .rounded(px(12.))
                            .shadow_lg()
                            .text_size(px(12.))
                            .text_color(rgb(0x547e70))
                            .child(icon("circle-check", 18.))
                            .child(self.toast.clone()),
                    ),
            );
        }
        if self.dialog.is_some() {
            root = root.child(self.render_dialog(window, cx));
        }
        root
    }
}
impl Workspace {
    fn render_sidebar(
        &self,
        cx: &mut Context<Self>,
        narrow: bool,
        wide: bool,
        width: f32,
    ) -> AnyElement {
        let inset = if narrow {
            12.
        } else if wide {
            23.
        } else {
            17.
        };
        let mut side = col()
            .w(px(width))
            .h_full()
            .flex_shrink_0()
            .pt(px(32.))
            .pb(px(16.))
            .px(px(inset))
            .border_r_1()
            .border_color(rgb(0xe1e3e9))
            .bg(linear_gradient(
                145.,
                linear_color_stop(rgb(0xeef0f5), 0.),
                linear_color_stop(rgb(0xf3f4f8), 1.),
            ))
            .child(
                row()
                    .mx(px(if narrow { 6. } else { 10. }))
                    .gap(px(11.))
                    .mb(px(if wide { 50. } else { 42. }))
                    .child(
                        div()
                            .size(px(40.))
                            .flex_shrink_0()
                            .child(img("brand.svg").size(px(48.))),
                    )
                    .child(
                        col()
                            .child(
                                label("S3 Studio", if narrow { 18. } else { 19. }, 0x252731)
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(
                                label(crate::i18n::t("云端文件，井然有序。"), 10., 0x9395a0)
                                    .mt(px(5.)),
                            ),
                    ),
            )
            .child(
                row()
                    .justify_between()
                    .px(px(11.))
                    .mb(px(10.))
                    .child(
                        label(crate::i18n::t("工作空间"), 11., 0x90939f)
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(
                        icon_button("add-account", crate::i18n::t("添加账户"), "plus")
                            .size(px(23.))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_dialog(Dialog::Account(None), window, cx)
                            })),
                    ),
            );
        side = side
            .child(
                col()
                    .id("accounts")
                    .max_h(relative(0.31))
                    .overflow_y_scroll()
                    .children(self.accounts.iter().map(|a| {
                        let id = a.id.clone();
                        let active = self.active_id == id;
                        button(SharedString::from(format!("account-{id}")), &a.name)
                            .w_full()
                            .justify_start()
                            .px(px(10.))
                            .py(px(11.))
                            .gap(px(10.))
                            .rounded(px(9.))
                            .mb(px(5.))
                            .disabled(self.is_busy())
                            .when(active, |s| {
                                s.bg(rgb(0xffffff)).border_1().border_color(rgb(0xe3e5eb))
                            })
                            .child(
                                row()
                                    .size(px(31.))
                                    .flex_shrink_0()
                                    .justify_center()
                                    .rounded(px(8.))
                                    .bg(rgb(if a.endpoint.is_empty() {
                                        0xfff0dd
                                    } else {
                                        0xeeebff
                                    }))
                                    .text_color(rgb(if a.endpoint.is_empty() {
                                        0xe99a2b
                                    } else {
                                        0x8c7acd
                                    }))
                                    .child(icon(
                                        if a.endpoint.is_empty() {
                                            "cloud"
                                        } else {
                                            "server"
                                        },
                                        18.,
                                    )),
                            )
                            .child(
                                col()
                                    .flex_1()
                                    .min_w_0()
                                    .items_start()
                                    .child(
                                        label(a.name.clone(), 12., 0x252731)
                                            .truncate()
                                            .font_weight(FontWeight::SEMIBOLD),
                                    )
                                    .child(
                                        label(
                                            if a.endpoint.is_empty() {
                                                "Amazon S3"
                                            } else {
                                                "S3 Compatible"
                                            },
                                            10.,
                                            0xa1a3ad,
                                        )
                                        .mt(px(4.)),
                                    ),
                            )
                            .when(active, |s| s.child(dot().mr(px(3.))))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_account(id.clone(), window, cx)
                            }))
                    })),
            )
            .child(
                button("add-account-bottom", crate::i18n::t("添加账户"))
                    .w_full()
                    .justify_start()
                    .px(px(12.))
                    .py(px(10.))
                    .text_color(rgb(0x7c8390))
                    .child(icon("plus", 15.))
                    .child(crate::i18n::t("添加账户"))
                    .child(
                        label(crate::i18n::t("无限数量"), 9., 0x9ea3ad)
                            .ml_auto()
                            .border_1()
                            .border_color(rgb(0xdcdee5))
                            .rounded(px(4.))
                            .px(px(4.))
                            .py(px(2.)),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_dialog(Dialog::Account(None), window, cx)
                    })),
            )
            .child(div().h(px(1.)).bg(rgb(0xdfe2e9)).my(px(23.)).mx(px(10.)))
            .child(
                row()
                    .justify_between()
                    .px(px(11.))
                    .mb(px(10.))
                    .child(
                        row()
                            .gap(px(6.))
                            .child(label(crate::i18n::t("存储桶"), 11., 0x90939f))
                            .child(
                                label(self.buckets.len().to_string(), 10., 0x90939f)
                                    .bg(rgb(0xe2e4eb))
                                    .rounded(px(4.))
                                    .px(px(5.)),
                            ),
                    )
                    .child(
                        icon_button("create-bucket", crate::i18n::t("创建存储桶"), "plus")
                            .size(px(23.))
                            .disabled(self.account().is_none() || self.is_busy())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_dialog(Dialog::Bucket, window, cx)
                            })),
                    ),
            );
        if !self.buckets.is_empty() {
            side = side.child(
                row()
                    .mx(px(10.))
                    .mb(px(9.))
                    .rounded(px(6.))
                    .bg(rgba(0xe7e9f080))
                    .px(px(6.))
                    .h(px(31.))
                    .gap(px(6.))
                    .text_color(rgb(0xa4a7b1))
                    .child(icon("search", 13.))
                    .child(
                        Input::new(&self.bucket_search)
                            .id("bucket-search")
                            .aria_label(crate::i18n::t("搜索存储桶"))
                            .appearance(false)
                            .text_size(px(11.))
                            .h(px(29.)),
                    ),
            );
        }
        let query = self.bucket_search.read(cx).value();
        side = side
            .child(
                col()
                    .id("buckets")
                    .flex_1()
                    .min_h(px(40.))
                    .overflow_y_scroll()
                    .when(self.bucket_loading, |s| {
                        s.child(label(crate::i18n::t("正在连接…"), 11., 0x9499a4).p(px(12.)))
                    })
                    .when(!self.bucket_loading && self.buckets.is_empty(), |s| {
                        s.child(
                            label(
                                if self.account().is_some() {
                                    crate::i18n::t("暂无存储桶")
                                } else {
                                    crate::i18n::t("添加账户以开始")
                                },
                                11.,
                                0x9499a4,
                            )
                            .p(px(12.)),
                        )
                    })
                    .children(
                        self.buckets
                            .iter()
                            .filter(|b| b.name.contains(query.as_str()))
                            .map(|b| {
                                let name = b.name.clone();
                                let selected = name == self.bucket;
                                button(SharedString::from(format!("bucket-{name}")), &name)
                                    .w_full()
                                    .justify_start()
                                    .gap(px(9.))
                                    .px(px(12.))
                                    .py(px(11.))
                                    .rounded(px(7.))
                                    .mb(px(4.))
                                    .text_color(rgb(if selected { 0x2876db } else { 0x777e8e }))
                                    .when(selected, |s| s.bg(rgb(0xdae8fc)))
                                    .child(icon("database", 16.))
                                    .child(div().flex_1().truncate().child(name.clone()))
                                    .when(selected, |s| s.child(icon("chevron-right", 13.)))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.navigate(name.clone(), String::new(), window, cx)
                                    }))
                            }),
                    ),
            )
            .child(
                col()
                    .px(px(9.))
                    .when(!cfg!(target_os = "macos"), |s| {
                        s.child(label(crate::i18n::t("语言"), 10., 0x777f8f)).child(
                            row().gap(px(4.)).mt(px(7.)).children(
                                [
                                    (
                                        "language-system",
                                        crate::i18n::t("跟随系统"),
                                        crate::i18n::Language::System,
                                    ),
                                    ("language-zh", "中文", crate::i18n::Language::Chinese),
                                    ("language-en", "English", crate::i18n::Language::English),
                                ]
                                .into_iter()
                                .map(|(id, text, language)| {
                                    secondary(id, text)
                                        .px(px(7.))
                                        .py(px(6.))
                                        .text_size(px(10.))
                                        .when(crate::i18n::preference() == language, |s| {
                                            s.text_color(rgb(BLUE)).border_color(rgb(BLUE))
                                        })
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.change_language(language, window, cx)
                                        }))
                                }),
                            ),
                        )
                    })
                    .child(
                        row()
                            .gap(px(10.))
                            .py(px(18.))
                            .border_t_1()
                            .border_color(rgb(0xdfe2e9))
                            .text_color(rgb(0x8e96a5))
                            .child(icon("shield-check", 17.))
                            .child(
                                col()
                                    .child(label(
                                        crate::i18n::t("只属于你的工作空间"),
                                        10.,
                                        0x777f8f,
                                    ))
                                    .child(
                                        label(crate::i18n::t("凭据保存在系统密钥库"), 9., 0x9ea4af)
                                            .mt(px(4.)),
                                    ),
                            ),
                    )
                    .child(
                        row()
                            .justify_between()
                            .pt(px(15.))
                            .border_t_1()
                            .border_color(rgb(0xdfe2e9))
                            .text_color(rgb(0x818795))
                            .child(
                                button("account-settings", crate::i18n::t("账户设置"))
                                    .disabled(self.account().is_none() || self.is_busy())
                                    .child(icon("settings-2", 16.))
                                    .child(crate::i18n::t("账户设置"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_dialog(
                                            Dialog::Account(Some(this.active_id.clone())),
                                            window,
                                            cx,
                                        )
                                    })),
                            )
                            .child(
                                icon_button("help", crate::i18n::t("使用帮助"), "circle-help")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_dialog(Dialog::Help, window, cx)
                                    })),
                            ),
                    ),
            );
        side.into_any_element()
    }
    fn render_workspace(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
        narrow: bool,
        wide: bool,
        padding: f32,
    ) -> AnyElement {
        let account = self.account();
        let name = account
            .map(|a| a.name.clone())
            .unwrap_or_else(|| crate::i18n::t("你的工作空间").into());
        let mut workspace = col().flex_1().min_w_0().h_full().bg(rgb(0xfafafb)).child(
            row()
                .h(px(68.))
                .flex_shrink_0()
                .px(px(if narrow { 23. } else { 34. }))
                .justify_between()
                .border_b_1()
                .border_color(rgb(LINE))
                .bg(rgba(0xffffffa8))
                .child(
                    row()
                        .gap(px(9.))
                        .text_size(px(12.))
                        .text_color(rgb(0x7d8290))
                        .child(dot())
                        .child(name.clone())
                        .child(icon("chevron-down", 13.).text_color(rgb(0xb0b4be))),
                )
                .child(
                    row()
                        .gap(px(20.))
                        .when(!narrow, |s| {
                            s.child(label("macOS · Windows · Linux", 10., 0xadb1bb))
                        })
                        .child(
                            row()
                                .size(px(29.))
                                .justify_center()
                                .rounded_full()
                                .bg(rgb(0xf0eafa))
                                .border_2()
                                .border_color(rgb(0xffffff))
                                .child(label(
                                    name.chars()
                                        .next()
                                        .unwrap_or('S')
                                        .to_uppercase()
                                        .to_string(),
                                    11.,
                                    0xa398b9,
                                )),
                        ),
                ),
        );
        if self.backend.preview {
            workspace = workspace.child(
                row()
                    .px(px(if narrow { 23. } else { 34. }))
                    .py(px(8.))
                    .gap(px(8.))
                    .bg(rgb(0xf0f6ff))
                    .border_b_1()
                    .border_color(rgb(0xe3edfb))
                    .text_color(rgb(0x6684ac))
                    .child(icon("info", 14.))
                    .child(label(
                        crate::i18n::t(
                            "交互预览 · 当前展示示例数据，桌面应用支持连接真实 S3 账户。",
                        ),
                        10.,
                        0x6684ac,
                    ))
                    .child(
                        label("PREVIEW", 8., 0x6684ac)
                            .ml_auto()
                            .border_1()
                            .border_color(rgb(0xccdef8))
                            .rounded(px(4.))
                            .px(px(5.))
                            .py(px(2.)),
                    ),
            );
        }
        let body = col()
            .id("workspace-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(padding))
            .pt(px(if narrow {
                27.
            } else if wide {
                45.
            } else {
                35.
            }))
            .child(
                row()
                    .justify_between()
                    .gap(px(16.))
                    .mb(px(if wide { 34. } else { 26. }))
                    .child(
                        col()
                            .child(label("O B J E C T   S T O R A G E", 9., 0x9da4b1).mb(px(9.)))
                            .child(
                                row().child(
                                    label(
                                        if self.bucket.is_empty() {
                                            crate::i18n::t("欢迎来到 S3 Studio").into()
                                        } else {
                                            self.bucket.clone()
                                        },
                                        if narrow { 25. } else { 28. },
                                        0x252731,
                                    )
                                    .font_weight(FontWeight::SEMIBOLD),
                                ),
                            )
                            .child(
                                label(
                                    if self.bucket.is_empty() {
                                        crate::i18n::t("连接你的云存储，从一个清晰的工作空间开始。")
                                    } else {
                                        crate::i18n::t("让每一份文件，都有自己的位置。")
                                    },
                                    12.,
                                    0x9299a6,
                                )
                                .mt(px(10.)),
                            ),
                    )
                    .child(
                        button("upload", crate::i18n::t("上传文件"))
                            .bg(rgb(BLUE))
                            .text_color(rgb(0xffffff))
                            .px(px(15.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .disabled(self.disabled())
                            .child(icon("arrow-up-from-line", 17.))
                            .child(crate::i18n::t("上传文件"))
                            .child(label("+", 17., 0x80bdff).ml(px(13.)))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_dialog(Dialog::Upload, window, cx)
                            })),
                    ),
            )
            .child(
                row()
                    .gap(px(13.))
                    .mb(px(22.))
                    .text_size(px(11.))
                    .text_color(rgb(0x989fac))
                    .child(
                        row()
                            .gap(px(6.))
                            .text_color(rgb(0x778397))
                            .child(icon("database", 16.))
                            .child(if self.bucket.is_empty() {
                                crate::i18n::t("尚未选择存储桶").into()
                            } else {
                                self.bucket.clone()
                            }),
                    )
                    .child(div().w(px(1.)).h(px(11.)).bg(rgb(0xdfe3eb)))
                    .child(
                        row().gap(px(5.)).child(dot()).child(
                            account
                                .map(|a| a.region.clone())
                                .unwrap_or_else(|| crate::i18n::t("未连接").into()),
                        ),
                    )
                    .child(div().w(px(1.)).h(px(11.)).bg(rgb(0xdfe3eb)))
                    .child(if self.backend.preview {
                        crate::i18n::t("示例工作空间")
                    } else {
                        crate::i18n::t("安全直连")
                    })
                    .when(!narrow, |s| {
                        s.child(
                            label(
                                crate::lf!(
                                    "{} 个文件{}",
                                    self.objects.iter().filter(|o| !o.folder).count(),
                                    if self.next_token.is_some() {
                                        crate::i18n::t("（已加载）")
                                    } else {
                                        ""
                                    }
                                ),
                                11.,
                                0x989ba6,
                            )
                            .ml_auto(),
                        )
                    }),
            )
            .child(self.render_files(window, cx, narrow))
            .child(self.render_transfers(cx))
            .child(
                row()
                    .justify_between()
                    .py(px(22.))
                    .px(px(1.))
                    .text_size(px(10.))
                    .text_color(rgb(0xb0b6c2))
                    .child(
                        row()
                            .gap(px(5.))
                            .child(icon("shield-check", 13.))
                            .child(crate::i18n::t("本地凭据 · 直接连接 · 不限账户")),
                    )
                    .child(
                        button("update", crate::i18n::t("检查应用更新"))
                            .text_size(px(10.))
                            .child("S3 Studio")
                            .child(label(
                                format!("v{}", self.update_status.current_version),
                                10.,
                                0xc3c8d1,
                            ))
                            .child(icon(
                                if self.update_status.version.is_some() {
                                    "sparkles"
                                } else {
                                    "refresh-cw"
                                },
                                12.,
                            ))
                            .when(self.update_status.version.is_some(), |s| {
                                s.text_color(rgb(BLUE)).child(crate::i18n::t("新版本"))
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_dialog(Dialog::Update, window, cx)
                            })),
                    ),
            );
        workspace.child(body).into_any_element()
    }
    fn render_files(&self, window: &Window, cx: &mut Context<Self>, narrow: bool) -> AnyElement {
        let visible = self.visible(cx);
        let query = self.search.read(cx).value();
        let first = self.first();
        let mut crumbs = row().gap(px(6.)).child(
            button("root-directory", crate::i18n::t("根目录"))
                .text_color(rgb(0x7e8796))
                .p(px(4.))
                .disabled(self.bucket.is_empty())
                .child(icon("hard-drive", 16.))
                .child(crate::i18n::t("根目录"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.navigate(this.bucket.clone(), String::new(), window, cx)
                })),
        );
        let mut prefix = String::new();
        for part in self.prefix.split('/').filter(|s| !s.is_empty()) {
            prefix.push_str(part);
            prefix.push('/');
            let target = prefix.clone();
            crumbs = crumbs
                .child(icon("chevron-right", 13.).text_color(rgb(0xc2c6cf)))
                .child(
                    button(SharedString::from(format!("breadcrumb-{prefix}")), part)
                        .text_color(rgb(0x404654))
                        .child(part.to_owned())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.navigate(this.bucket.clone(), target.clone(), window, cx)
                        })),
                );
        }
        let toolbar = row()
            .h(px(58.))
            .px(px(20.))
            .justify_between()
            .border_b_1()
            .border_color(rgb(LINE))
            .gap(px(10.))
            .child(crumbs)
            .child(
                row()
                    .gap(px(7.))
                    .child(
                        icon_button("new-folder", crate::i18n::t("新建文件夹"), "folder-plus")
                            .disabled(self.disabled())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_dialog(Dialog::Folder, window, cx)
                            })),
                    )
                    .child(
                        icon_button("refresh", crate::i18n::t("刷新"), "refresh-cw")
                            .disabled(self.disabled())
                            .on_click(cx.listener(|this, _, _, cx| this.load_objects(false, cx))),
                    )
                    .child(div().w(px(1.)).h(px(16.)).mx(px(4.)).bg(rgb(0xe5e8ef)))
                    .child(
                        row()
                            .p(px(2.))
                            .rounded(px(6.))
                            .bg(rgb(0xf3f4f6))
                            .border_1()
                            .border_color(rgb(0xeceef2))
                            .child(
                                icon_button("list-view", crate::i18n::t("列表视图"), "list")
                                    .w(px(27.))
                                    .h(px(23.))
                                    .when(!self.grid, |s| {
                                        s.bg(rgb(0xffffff)).text_color(rgb(0x697486))
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.grid = false;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                icon_button("grid-view", crate::i18n::t("网格视图"), "layout-grid")
                                    .w(px(27.))
                                    .h(px(23.))
                                    .when(self.grid, |s| {
                                        s.bg(rgb(0xffffff)).text_color(rgb(0x697486))
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.grid = true;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        icon_button("toggle-info", crate::i18n::t("对象详情"), "info")
                            .when(self.show_info, |s| {
                                s.text_color(rgb(BLUE)).bg(rgb(0xf0f6ff))
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_info = !this.show_info;
                                this.selection_changed(cx);
                            })),
                    ),
            );
        let mut search = row()
            .px(px(20.))
            .py(px(13.))
            .gap(px(10.))
            .justify_between()
            .child(
                row()
                    .flex_1()
                    .max_w(px(360.))
                    .h(px(40.))
                    .px(px(10.))
                    .gap(px(8.))
                    .bg(rgb(0xf8f9fb))
                    .border_1()
                    .border_color(rgb(0xeceef3))
                    .rounded(px(7.))
                    .text_color(rgb(0xa2aaba))
                    .child(icon("search", 17.))
                    .child(
                        Input::new(&self.search)
                            .id("object-search")
                            .aria_label(crate::i18n::t("搜索当前目录"))
                            .appearance(false)
                            .text_size(px(12.))
                            .h(px(36.)),
                    )
                    .when(query.is_empty(), |s| {
                        s.child(
                            label("⌘ / Ctrl F", 10., 0xabb1bd)
                                .border_1()
                                .border_color(rgb(0xe1e5ed))
                                .rounded(px(3.))
                                .px(px(4.))
                                .py(px(2.))
                                .whitespace_nowrap(),
                        )
                    })
                    .when(!query.is_empty(), |s| {
                        s.child(
                            icon_button("clear-search", crate::i18n::t("清除搜索"), "x")
                                .size(px(14.))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.search.update(cx, |s, cx| s.set_value("", window, cx));
                                    cx.notify();
                                })),
                        )
                    }),
            );
        if self.selected.is_empty() {
            if !narrow {
                search = search.child(label(crate::i18n::t("双击文件夹进入"), 10., 0xb2b7c2));
            }
        } else {
            let disabled = self.is_busy() || first.as_ref().is_none_or(|o| o.folder);
            search = search.child(
                row()
                    .gap(px(6.))
                    .child(
                        label(crate::lf!("已选 {} 项", self.selected.len()), 10., 0x828e9f)
                            .mr(px(6.)),
                    )
                    .child(
                        icon_button(
                            "download-selection",
                            crate::i18n::t("下载所选文件"),
                            "arrow-down-to-line",
                        )
                        .disabled(disabled)
                        .on_click(cx.listener(|this, _, _, cx| this.download(cx))),
                    )
                    .child(
                        icon_button("copy-selection", crate::i18n::t("复制所选对象"), "copy")
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_dialog(Dialog::Copy, window, cx)
                            })),
                    )
                    .child(
                        icon_button("share-selection", crate::i18n::t("复制分享链接"), "link")
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.share(cx))),
                    )
                    .child(
                        icon_button(
                            "delete-selection",
                            crate::i18n::t("删除所选项目"),
                            "trash-2",
                        )
                        .text_color(rgb(0xd87878))
                        .disabled(self.is_busy())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_dialog(Dialog::Delete, window, cx)
                        })),
                    ),
            );
        }
        let mut panel = col()
            .rounded(px(12.))
            .border_1()
            .border_color(rgb(0xe6e8ed))
            .bg(rgb(0xffffff))
            .overflow_hidden()
            .flex_shrink_0()
            .child(toolbar)
            .child(search);
        if !self.error.is_empty() {
            panel = panel.child(
                row()
                    .px(px(18.))
                    .py(px(12.))
                    .gap(px(8.))
                    .bg(rgb(0xfff3f2))
                    .text_color(rgb(0xbe625e))
                    .text_size(px(11.))
                    .child(icon("info", 17.))
                    .child(div().flex_1().child(self.error.clone()))
                    .child(
                        icon_button("close-error", crate::i18n::t("关闭错误"), "x").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.error.clear();
                                cx.notify();
                            }),
                        ),
                    ),
            );
        }
        let height = (f32::from(window.viewport_size().height) - 555.).max(240.);
        let mut objects = col()
            .id("objects")
            .flex_1()
            .min_w_0()
            .min_h(px(240.))
            .max_h(px(height))
            .overflow_y_scroll();
        if visible.is_empty() {
            let loading = self.loading && self.objects.is_empty();
            let title = if loading {
                crate::i18n::t("正在加载文件")
            } else if !query.is_empty() {
                crate::i18n::t("没有找到匹配的文件")
            } else if self.account().is_none() {
                crate::i18n::t("一切从连接开始")
            } else if self.bucket.is_empty() {
                crate::i18n::t("创建你的第一个存储桶")
            } else {
                crate::i18n::t("这里还没有文件")
            };
            let detail = if loading {
                crate::i18n::t("正在安全连接你的存储空间…")
            } else if !query.is_empty() {
                crate::i18n::t("试试其他关键词。")
            } else if self.account().is_none() {
                crate::i18n::t("支持 AWS、MinIO 和其他 S3 兼容服务，账户数量不限。")
            } else {
                crate::i18n::t("添加文件，为你的工作腾出更多空间。")
            };
            objects = objects.child(
                col()
                    .min_h(px(350.))
                    .items_center()
                    .justify_center()
                    .p(px(30.))
                    .child(
                        row()
                            .size(px(80.))
                            .justify_center()
                            .rounded(px(22.))
                            .bg(rgb(0xeaf3ff))
                            .text_color(rgb(0x83b8fc))
                            .child(icon(if loading { "loader-circle" } else { "cloud" }, 36.)),
                    )
                    .child(label(title, 16., 0x66758a).mt(px(15.)))
                    .child(label(detail, 11., 0xa3acba).mt(px(9.)).mb(px(20.)))
                    .when(self.account().is_none() && !loading, |s| {
                        s.child(
                            primary("first-account", crate::i18n::t("连接第一个账户")).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.open_dialog(Dialog::Account(None), window, cx)
                                }),
                            ),
                        )
                    }),
            );
        } else if self.grid {
            objects = objects.child(self.render_grid(&visible, window, narrow, cx));
        } else {
            objects = objects.child(self.render_table(&visible, narrow, height, cx));
        }
        if self.next_token.is_some() {
            objects = objects.child(
                col()
                    .items_center()
                    .gap(px(7.))
                    .p(px(20.))
                    .child(
                        secondary(
                            "load-more",
                            if self.loading {
                                crate::i18n::t("加载中…")
                            } else {
                                crate::i18n::t("加载更多对象")
                            },
                        )
                        .disabled(self.loading)
                        .on_click(cx.listener(|this, _, _, cx| this.load_objects(true, cx))),
                    )
                    .child(label(
                        crate::i18n::t("搜索和排序适用于已加载的对象"),
                        9.,
                        0xaaaaaa,
                    )),
            );
        }
        let mut content = row().items_stretch().child(objects);
        if self.show_info {
            content = content.child(self.render_inspector(narrow, cx));
        }
        panel = panel.child(content).child(
            row()
                .justify_between()
                .border_t_1()
                .border_color(rgb(LINE))
                .px(px(20.))
                .py(px(12.))
                .bg(rgb(0xfdfdfe))
                .child(label(
                    crate::lf!(
                        "{} 个项目{}{}",
                        visible.len(),
                        if query.is_empty() {
                            ""
                        } else {
                            crate::i18n::t(" · 搜索结果")
                        },
                        if self.selected.is_empty() {
                            String::new()
                        } else {
                            crate::lf!(" · 已选择 {} 项", self.selected.len())
                        }
                    ),
                    10.,
                    0xadb3be,
                ))
                .child(label(
                    crate::lf!(
                        "{} · 当前已加载",
                        model::bytes(self.objects.iter().map(|o| o.size.max(0) as u64).sum())
                    ),
                    10.,
                    0xadb3be,
                )),
        );
        panel.into_any_element()
    }
    fn render_table(
        &self,
        objects: &[S3Object],
        narrow: bool,
        height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let all = objects.iter().all(|o| self.selected.contains(&o.key));
        let header = row()
            .h(px(35.))
            .border_y_1()
            .border_color(rgb(0xf1f2f5))
            .bg(rgb(0xfafbfc))
            .child(
                div().w(px(46.)).pl(px(20.)).flex_shrink_0().child(
                    Checkbox::new("select-all")
                        .accessibility_label(crate::i18n::t("选择当前可见对象"))
                        .checked(all)
                        .xsmall()
                        .on_click(cx.listener(|this, checked, _, cx| {
                            this.selected = if *checked {
                                this.visible(cx).iter().map(|o| o.key.clone()).collect()
                            } else {
                                Default::default()
                            };
                            this.selection_changed(cx);
                        })),
                ),
            )
            .children(
                [
                    (Sort::Name, crate::i18n::t("名称"), 0.43),
                    (Sort::Size, crate::i18n::t("大小"), 0.13),
                    (Sort::Modified, crate::i18n::t("修改时间"), 0.19),
                ]
                .into_iter()
                .map(|(sort, text, width)| {
                    div().w(relative(width)).child(
                        button(SharedString::from(format!("sort-{sort:?}")), text)
                            .justify_start()
                            .text_size(px(10.))
                            .text_color(rgb(0x9ca4b2))
                            .child(text)
                            .when(self.sort == sort, |s| {
                                s.child(icon(
                                    if self.ascending {
                                        "arrow-up"
                                    } else {
                                        "arrow-down"
                                    },
                                    12.,
                                ))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| this.sort_by(sort, cx))),
                    )
                }),
            )
            .when(!narrow, |s| {
                s.child(label(crate::i18n::t("存储类型"), 10., 0x9ca4b2).flex_1())
            })
            .child(div().w(px(44.)));
        col()
            .child(header.flex_shrink_0())
            .child(
                col()
                    .id("table-rows")
                    .max_h(px(height - 35.))
                    .overflow_y_scroll()
                    .children(objects.iter().enumerate().map(|(i, o)| {
                        let key = o.key.clone();
                        let is_folder = o.folder;
                        let selected = self.selected.contains(&key);
                        let check_key = key.clone();
                        let more_key = key.clone();
                        row()
                            .id(SharedString::from(format!("object-{key}")))
                            .min_h(px(48.))
                            .border_b_1()
                            .border_color(rgb(0xf4f5f7))
                            .text_size(px(11.))
                            .cursor_pointer()
                            .when(selected, |s| s.bg(rgb(0xedf5ff)))
                            .hover(|s| s.bg(rgb(0xf8faff)))
                            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                                if event.click_count() == 2 {
                                    if is_folder {
                                        this.navigate(this.bucket.clone(), key.clone(), window, cx);
                                    } else {
                                        this.inspect(key.clone(), cx);
                                    }
                                } else {
                                    this.toggle(&key, cx);
                                }
                            }))
                            .child(
                                div()
                                    .w(px(46.))
                                    .pl(px(20.))
                                    .flex_shrink_0()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .child(
                                        Checkbox::new(("object-check", i))
                                            .accessibility_label(crate::lf!(
                                                "选择 {}",
                                                model::object_name(&o.key, &self.prefix)
                                            ))
                                            .checked(selected)
                                            .xsmall()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.toggle(&check_key, cx);
                                            })),
                                    ),
                            )
                            .child(
                                row()
                                    .w(relative(0.43))
                                    .gap(px(11.))
                                    .child(file_icon(o, 20.))
                                    .child(
                                        label(
                                            model::object_name(&o.key, &self.prefix).to_owned(),
                                            12.,
                                            0x4b5362,
                                        )
                                        .flex_1(),
                                    ),
                            )
                            .child(
                                label(
                                    if o.folder {
                                        "—".into()
                                    } else {
                                        model::bytes(o.size.max(0) as u64)
                                    },
                                    11.,
                                    MUTED,
                                )
                                .w(relative(0.13)),
                            )
                            .child(
                                label(
                                    if o.folder {
                                        "—".into()
                                    } else {
                                        model::date(&o.modified)
                                    },
                                    11.,
                                    MUTED,
                                )
                                .w(relative(0.19)),
                            )
                            .when(!narrow, |s| {
                                s.child(
                                    row().flex_1().child(
                                        label(
                                            if o.folder {
                                                crate::i18n::t("文件夹").into()
                                            } else if o.storage_class.is_empty() {
                                                "STANDARD".into()
                                            } else {
                                                o.storage_class.clone()
                                            },
                                            9.,
                                            if o.folder { 0xb2b7c2 } else { 0x9ba6b7 },
                                        )
                                        .px(px(5.))
                                        .py(px(3.))
                                        .rounded(px(4.))
                                        .when(
                                            !o.folder,
                                            |s| {
                                                s.border_1()
                                                    .border_color(rgb(0xe8ecf1))
                                                    .bg(rgb(0xfcfcfd))
                                            },
                                        ),
                                    ),
                                )
                            })
                            .child(
                                icon_button(
                                    ("more", i),
                                    &crate::lf!(
                                        "查看 {}",
                                        model::object_name(&o.key, &self.prefix)
                                    ),
                                    "ellipsis",
                                )
                                .mr(px(10.))
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.inspect(more_key.clone(), cx);
                                    },
                                )),
                            )
                    })),
            )
            .into_any_element()
    }
    fn render_grid(
        &self,
        objects: &[S3Object],
        window: &Window,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let width = f32::from(window.viewport_size().width);
        let (sidebar, padding) = if narrow {
            (215., 23.)
        } else if width >= 1450. {
            (268., 48.)
        } else {
            (248., 34.)
        };
        let inspector = if self.show_info {
            if narrow { 210. } else { 230. }
        } else {
            0.
        };
        let columns = ((width - sidebar - padding * 2. - inspector - 40. + 15.) / 160.)
            .floor()
            .max(1.) as u16;
        div()
            .grid()
            .grid_cols(columns)
            .items_stretch()
            .gap(px(15.))
            .px(px(20.))
            .py(px(18.))
            .children(objects.iter().enumerate().map(|(i, o)| {
                let key = o.key.clone();
                let folder = o.folder;
                let selected = self.selected.contains(&key);
                button(("card", i), model::object_name(&o.key, &self.prefix))
                    .relative()
                    .flex_col()
                    .min_w_0()
                    .min_h(px(148.))
                    .py(px(20.))
                    .px(px(10.))
                    .gap(px(6.))
                    .rounded(px(9.))
                    .bg(rgb(if selected { 0xedf5ff } else { 0xfafbfd }))
                    .border_1()
                    .border_color(rgb(if selected { 0xb9d8ff } else { 0xeff1f5 }))
                    .child(div().pt(px(4.)).pb(px(14.)).child(file_icon(o, 42.)))
                    .child(label(
                        model::object_name(&o.key, &self.prefix).to_owned(),
                        10.,
                        0x252731,
                    ))
                    .child(label(
                        if folder {
                            crate::i18n::t("文件夹").into()
                        } else {
                            model::bytes(o.size.max(0) as u64)
                        },
                        9.,
                        0x9ca6b5,
                    ))
                    .when(selected, |s| {
                        s.child(
                            div()
                                .absolute()
                                .top(px(8.))
                                .right(px(8.))
                                .child(icon("circle-check", 16.).text_color(rgb(BLUE))),
                        )
                    })
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if event.click_count() == 2 {
                            if folder {
                                this.navigate(this.bucket.clone(), key.clone(), window, cx);
                            } else {
                                this.show_info = true;
                                this.selection_changed(cx);
                            }
                        } else {
                            this.toggle(&key, cx);
                        }
                    }))
            }))
            .into_any_element()
    }
    fn render_inspector(&self, narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = col()
            .w(px(if narrow { 210. } else { 235. }))
            .flex_shrink_0()
            .p(px(18.))
            .bg(rgb(0xfdfdfe))
            .border_l_1()
            .border_color(rgb(LINE))
            .child(
                row()
                    .justify_between()
                    .child(label(crate::i18n::t("对象详情"), 11., 0x758297))
                    .child(
                        icon_button("close-info", crate::i18n::t("关闭详情"), "x")
                            .size(px(16.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_info = false;
                                cx.notify();
                            })),
                    ),
            );
        if let Some(o) = self.first() {
            panel = panel.child(
                col()
                    .items_center()
                    .gap(px(18.))
                    .pt(px(28.))
                    .pb(px(15.))
                    .child(file_icon(&o, 44.))
                    .child(label(
                        model::object_name(&o.key, &self.prefix).to_owned(),
                        12.,
                        0x252731,
                    )),
            );
            let mut entries = vec![
                (crate::i18n::t("对象键").into(), o.key.clone()),
                (
                    crate::i18n::t("大小").into(),
                    if o.folder {
                        crate::i18n::t("文件夹").into()
                    } else {
                        model::bytes(
                            self.info.as_ref().map(|i| i.size).unwrap_or(o.size).max(0) as u64
                        )
                    },
                ),
                (
                    crate::i18n::t("最后修改").into(),
                    model::date(
                        self.info
                            .as_ref()
                            .map(|i| i.modified.as_str())
                            .filter(|s| !s.is_empty())
                            .unwrap_or(&o.modified),
                    ),
                ),
                (
                    crate::i18n::t("存储类型").into(),
                    if o.storage_class.is_empty() {
                        "—".into()
                    } else {
                        o.storage_class.clone()
                    },
                ),
            ];
            if let Some(info) = &self.info {
                entries.push(("Content-Type".into(), info.content_type.clone()));
                entries.push(("ETag".into(), info.etag.clone()));
                entries.extend(info.metadata.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
            panel = panel
                .child(
                    col()
                        .mt(px(16.))
                        .mb(px(22.))
                        .children(entries.into_iter().map(|(key, value)| {
                            col()
                                .mt(px(14.))
                                .child(label(key, 10., 0xa3adbb).mb(px(5.)))
                                .child(label(value, 10., 0x758091))
                        })),
                )
                .when(!self.info_error.is_empty(), |s| {
                    s.child(label(self.info_error.clone(), 11., 0xd87878))
                })
                .when(!o.folder, |s| {
                    s.child(
                        secondary("inspector-download", crate::i18n::t("下载文件"))
                            .w_full()
                            .disabled(self.is_busy())
                            .on_click(cx.listener(|this, _, _, cx| this.download(cx))),
                    )
                });
        } else {
            panel = panel
                .child(label(crate::i18n::t("选择一个对象以查看详情。"), 11., MUTED).mt(px(12.)));
        }
        panel.into_any_element()
    }
}
