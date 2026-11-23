use super::palette::*;
use nsh_base::{
    ButtonElement, CheckboxElement, button as base_button, checkbox as base_checkbox, input,
};
use nsh_core::{element::ImageSrc, prelude::*};

pub fn row() -> View {
    view().flex_row().items_center()
}

pub fn column() -> View {
    view().flex_col()
}

pub fn label(value: impl IntoReactive<SharedString>, size: f32, ink: AdaptiveColor) -> Text {
    text(value).text_size(px(size)).text_color(ink)
}

pub fn button(id: &str, title: impl Into<SharedString>) -> ButtonElement {
    base_button()
        .test_id(id)
        .accessibility_label(title.into())
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(7.0))
        .rounded(px(7.0))
        .flex_shrink_0()
        .cursor(Cursor::Pointer)
        .hover(|s| s.bg(SUBTLE))
        .active(|s| s.opacity(0.78))
        .focus_visible(|s| s.outline(px(2.0)).outline_color(ACCENT))
        .disabled_style(|s| s.opacity(0.45))
}

pub fn icon_button(id: &str, title: impl Into<SharedString>, image: &'static str) -> ButtonElement {
    let title = title.into();
    button(id, title)
        .size(px(30.0))
        .child(icon(image, 16.0, MUTED))
}

pub fn secondary(id: &str, title: impl Into<SharedString>) -> ButtonElement {
    let title = title.into();
    button(id, title.clone())
        .min_h(px(34.0))
        .px(px(11.0))
        .border_1()
        .border_color(LINE)
        .bg(SURFACE)
        .child(label(title, 12.0, TEXT))
}

pub fn primary(id: &str, title: impl Into<SharedString>, destructive: bool) -> ButtonElement {
    let title = title.into();
    let fill = if destructive { DANGER_FILL } else { BLUE };
    button(id, title.clone())
        .min_h(px(36.0))
        .px(px(15.0))
        .bg(fill)
        .hover(|s| s.bg(if destructive { DANGER_FILL } else { BLUE_HOVER }))
        .child(label(title, 12.0, ON_BLUE).font_weight(FontWeight::MEDIUM))
}

pub fn input_field(
    id: &str,
    name: impl Into<SharedString>,
    hint: impl IntoReactive<String>,
    value: impl IntoReactive<String>,
) -> Input {
    input()
        .test_id(id)
        .label(name.into())
        .placeholder(hint)
        .value(value)
        .auto_correct(false)
        .h(px(36.0))
        .min_w_0()
        .px(px(10.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(LINE)
        .focus(|s| s.border_color(ACCENT))
        .bg(SUBTLE)
        .text_color(TEXT)
        .placeholder_color(|| color(MUTED))
        .text_size(px(12.0))
}

pub fn checkbox(
    id: &str,
    title: impl Into<SharedString>,
    checked: bool,
    mixed: bool,
) -> CheckboxElement {
    base_checkbox()
        .test_id(id)
        .accessibility_label(title.into())
        .checked(checked)
        .indeterminate(mixed)
        .focus_visible(|s| s.outline(px(2.0)).outline_color(ACCENT))
        .cursor(Cursor::Pointer)
        .indicator_style(move |s| {
            s.size(px(15.0))
                .border_1()
                .border_color(if checked || mixed { BLUE } else { LINE })
                .rounded(px(4.0))
                .bg(if checked || mixed { BLUE } else { SURFACE })
                .items_center()
                .justify_center()
        })
        .when(checked || mixed, |control| {
            control.indicator(label(if mixed { "−" } else { "✓" }, 11.0, ON_BLUE))
        })
}

pub fn panel() -> View {
    column()
        .rounded(px(11.0))
        .border_1()
        .border_color(LINE)
        .bg(SURFACE)
}

pub fn divider() -> View {
    view().h(px(1.0)).w_full().flex_shrink_0().bg(LINE)
}

pub fn error_notice(error: &str) -> View {
    row()
        .gap(px(8.0))
        .px(px(14.0))
        .py(px(9.0))
        .rounded(px(6.0))
        .bg(DANGER_TINT)
        .child(icon("info", 16.0, DANGER))
        .child(label(error.to_owned(), 12.0, DANGER).flex_1().min_w_0())
}

pub fn progress(fraction: f32, fill: AdaptiveColor) -> View {
    view().h(px(5.0)).w_full().rounded_full().bg(LINE).child(
        view()
            .h_full()
            .w(percentage(fraction.clamp(0.0, 1.0)))
            .bg(fill)
            .rounded_full(),
    )
}

pub fn brand(size: f32) -> impl IntoElement {
    svg(ImageSrc::svg_markup(include_str!(
        "../../../assets/brand/s3-studio-logo.svg"
    )))
    .size(px(size))
    .flex_shrink_0()
}

macro_rules! icons {
    ($name:expr, $($icon:literal),+ $(,)?) => {
        match $name {
            $($icon => include_str!(concat!("../../assets/icons/", $icon, ".svg")),)+
            _ => include_str!("../../assets/icons/file.svg"),
        }
    };
}

pub fn icon(name: &'static str, size: f32, ink: AdaptiveColor) -> impl IntoElement {
    let markup = icons!(
        name,
        "arrow-down",
        "arrow-down-to-line",
        "arrow-up",
        "arrow-up-to-line",
        "arrow-up-down",
        "check",
        "chevron-down",
        "chevron-right",
        "circle-question-mark",
        "cloud",
        "copy",
        "database",
        "ellipsis",
        "ellipsis-vertical",
        "file-archive",
        "file-code-corner",
        "file-image",
        "file-text",
        "folder",
        "folder-plus",
        "grip-vertical",
        "history",
        "hard-drive",
        "info",
        "layout-grid",
        "link",
        "list",
        "list-tree",
        "loader-circle",
        "pause",
        "play",
        "plus",
        "refresh-cw",
        "rotate-ccw",
        "server",
        "settings-2",
        "trash",
        "recycle-bin",
        "x",
    );
    svg(ImageSrc::svg_markup(markup))
        .size(px(size))
        .flex_shrink_0()
        .text_color(ink)
}
