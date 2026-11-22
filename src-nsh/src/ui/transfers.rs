use super::*;
use crate::model;
use s3_studio_core::transfers::{Direction, Transfer};

pub(super) fn panel(store: Store, state: &Workspace) -> View {
    let active = state
        .transfers
        .iter()
        .filter(|transfer| model::active(transfer))
        .count();
    let paused = state
        .transfers
        .iter()
        .filter(|transfer| transfer.status == "paused")
        .count();
    let summary = if active > 0 {
        crate::lf!("{active} 项正在传输", active = active)
    } else if paused > 0 {
        crate::lf!("{paused} 项已暂停", paused = paused)
    } else {
        t("一切已就绪").into()
    };
    controls::panel()
        .child(
            row()
                .px(px(11.0))
                .gap(px(6.0))
                .child(
                    button("transfers-toggle", t("传输活动"))
                        .flex_1()
                        .min_w_0()
                        .h(px(43.0))
                        .justify_between()
                        .px(px(5.0))
                        .selected(state.expanded)
                        .child(
                            row()
                                .gap(px(7.0))
                                .child(icon("arrow-down-to-line", 16.0, MUTED))
                                .child(
                                    label(t("传输活动"), 12.0, TEXT)
                                        .font_weight(FontWeight::MEDIUM),
                                ),
                        )
                        .child(
                            row()
                                .gap(px(8.0))
                                .child(label(summary, 10.0, MUTED))
                                .child(
                                    label(
                                        crate::lf!("{} 路并发", state.settings.concurrency),
                                        10.0,
                                        ACCENT,
                                    )
                                    .px(px(6.0))
                                    .py(px(3.0))
                                    .rounded(px(5.0))
                                    .bg(SELECTED),
                                )
                                .child(icon(
                                    if state.expanded {
                                        "chevron-down"
                                    } else {
                                        "chevron-right"
                                    },
                                    14.0,
                                    MUTED,
                                )),
                        )
                        .on_press(store.action(|state| state.expanded = !state.expanded)),
                )
                .child(
                    icon_button("transfer-settings", t("传输设置"), "settings-2")
                        .disabled(!state.can_open_app_dialog())
                        .on_press(store.open(Dialog::TransferSettings)),
                ),
        )
        .when(!state.transfer_error.is_empty(), |panel| {
            panel.child(
                error_notice(&state.transfer_error)
                    .mx(px(9.0))
                    .mb(px(7.0))
                    .child(
                        icon_button("close-transfer-error", t("关闭传输错误"), "x")
                            .on_press(store.action(|state| state.transfer_error.clear())),
                    ),
            )
        })
        .when(state.expanded, |panel| {
            panel.child(divider()).child(list(store, state))
        })
}

fn list(store: Store, state: &Workspace) -> View {
    if state.transfers.is_empty() {
        return row()
            .px(px(16.0))
            .py(px(16.0))
            .gap(px(8.0))
            .child(icon("check", 16.0, OK))
            .child(label(t("文件已就绪，随时开始。"), 11.0, MUTED));
    }
    column()
        .child(
            scroll().test_id("transfers-list").max_h(px(262.0)).child(
                column().child(
                    state
                        .transfers
                        .iter()
                        .map(|transfer| transfer_row(store, state, transfer))
                        .collect::<Vec<_>>(),
                ),
            ),
        )
        .when(
            state
                .transfers
                .iter()
                .any(|transfer| matches!(transfer.status.as_str(), "done" | "cancelled")),
            |panel| {
                panel.child(
                    row().justify_end().p(px(8.0)).child(
                        secondary("clear-transfers", t("清除已完成及已取消的记录"))
                            .disabled(state.is_busy())
                            .on_press(store.action(Workspace::clear_finished)),
                    ),
                )
            },
        )
}

fn transfer_row(store: Store, state: &Workspace, transfer: &Transfer) -> View {
    let id = &transfer.id;
    let disabled = state.is_busy() || state.pending_transfers.contains(id);
    let status = transfer.status.as_str();
    let ratio = if transfer.total > 0 {
        (transfer.bytes as f32 / transfer.total as f32).clamp(0.0, 1.0)
    } else if status == "done" {
        1.0
    } else {
        0.0
    };
    let direction = if transfer.direction == Direction::Upload {
        t("上传")
    } else {
        t("下载")
    };
    let speed = if status == "running" && transfer.speed > 0 {
        format!(" · {}/s", model::bytes(transfer.speed.into()))
    } else {
        String::new()
    };
    let (status_icon, ink) = match status {
        "done" => ("check", OK),
        "paused" => ("pause", MUTED),
        "cancelled" => ("x", MUTED),
        "error" => ("info", DANGER),
        _ => ("loader-circle", ACCENT),
    };
    let controls = row()
        .gap(px(4.0))
        .flex_shrink_0()
        .when(
            matches!(status, "queued" | "preparing" | "running"),
            |row| {
                let id = id.clone();
                row.child(
                    icon_button(
                        &format!("pause-{id}"),
                        crate::lf!("暂停 {}", transfer.name),
                        "pause",
                    )
                    .disabled(disabled)
                    .on_press(
                        store.action(move |state| state.control("pause_transfer", id.clone())),
                    ),
                )
            },
        )
        .when(matches!(status, "paused" | "error"), |row| {
            let id = id.clone();
            let (action, glyph) = if status == "error" {
                (t("重试"), "rotate-ccw")
            } else {
                (t("继续"), "play")
            };
            row.child(
                icon_button(
                    &format!("resume-{id}"),
                    format!("{action} {}", transfer.name),
                    glyph,
                )
                .disabled(disabled)
                .on_press(store.action(move |state| state.control("resume_transfer", id.clone()))),
            )
        })
        .when(
            !matches!(status, "done" | "cancelled" | "pausing" | "finalizing"),
            |row| {
                row.child(
                    icon_button(
                        &format!("cancel-transfer-{id}"),
                        crate::lf!("取消 {}", transfer.name),
                        "x",
                    )
                    .disabled(disabled)
                    .on_press(store.open(Dialog::CancelTransfer(id.clone()))),
                )
            },
        );
    row()
        .key(id.clone())
        .px(px(15.0))
        .py(px(10.0))
        .gap(px(10.0))
        .border_b_1()
        .border_color(LINE)
        .child(icon(status_icon, 17.0, ink))
        .child(
            column()
                .flex_1()
                .min_w_0()
                .gap(px(4.0))
                .child(label(transfer.name.clone(), 12.0, TEXT).truncate())
                .child(label(
                    format!(
                        "{direction} · {} / {}{speed}",
                        model::bytes(transfer.bytes.into()),
                        model::bytes(transfer.total.into())
                    ),
                    10.0,
                    MUTED,
                ))
                .child(progress(
                    ratio,
                    match status {
                        "paused" => MUTED,
                        "done" => OK,
                        _ => BLUE,
                    },
                ))
                .when_some(transfer.error.as_ref(), |column, error| {
                    column.child(label(error.clone(), 11.0, DANGER))
                }),
        )
        .child(
            column()
                .items_end()
                .gap(px(4.0))
                .child(label(model::status_label(status), 11.0, ink))
                .when(status == "running" && transfer.total > 0, |column| {
                    column.child(label(format!("{:.0}%", ratio * 100.0), 10.0, MUTED))
                }),
        )
        .child(controls)
}
