use super::*;
use crate::{
    object_tree::{LevelStatus, Totals, TreePage, TreeRow, TreeSort},
    tree_cache,
    workspace::TreeStatus,
};

const CHECK_WIDTH: f32 = 42.0;
const SIZE_WIDTH: f32 = 92.0;
const COUNT_WIDTH: f32 = 80.0;
const SHARE_WIDTH: f32 = 124.0;
const ROW_HEIGHT: f32 = 40.0;

pub(super) fn objects(store: Store) -> View {
    let rows = computed(move || store.read(|state| state.tree.rows.clone()));
    let list_ref = element_ref();
    let scroll_ref = list_ref.clone();
    let location = computed(move || {
        store.read(|state| {
            (
                state.active_id.clone(),
                state.bucket.clone(),
                state.prefix.clone(),
                state.tree.options.pages.get(&state.prefix).copied(),
                state.tree.options.search_pages.len(),
            )
        })
    });
    effect(move || {
        location.with(|_| ());
        scroll_ref.scroll_to(nsh_core::element::point(px(0.0), px(0.0)), false);
    });
    let list = virtual_list(
        move || rows.with(|rows| rows.len()),
        move |position| {
            dynamic(move || {
                rows.with(|rows| {
                    rows.get(position.index())
                        .map(|entry| store.read(|state| item(store, state, entry)))
                })
            })
        },
    )
    .keys(move || {
        rows.with(|rows| {
            rows.iter()
                .map(|entry| match entry {
                    TreeRow::Entry { item, .. } => (0, item.key.key.clone(), item.key.folder),
                    TreeRow::Pager { parent, .. } => (1, parent.clone(), false),
                    TreeRow::Level { parent, .. } => (2, parent.clone(), false),
                })
                .collect::<Vec<_>>()
        })
    })
    .estimated_item_size(px(ROW_HEIGHT))
    .element_ref(&list_ref)
    .test_id("tree-objects")
    .role(AccessibilityRole::Tree)
    .flex_1()
    .min_h_0()
    .empty(dynamic(move || {
        store.read(|state| scan_state(store, state))
    }));

    column()
        .flex_1()
        .min_w_0()
        .h(objects_height)
        .child(dynamic(move || store.read(|state| activity(store, state))))
        .child(when(
            move || rows.with(|rows| !rows.is_empty()),
            move || dynamic(move || store.read(|state| header(store, state))),
        ))
        .child(list)
}

pub(super) fn summary(state: &Workspace) -> Option<View> {
    let Some(totals) = state.tree.totals else {
        return (!state.search.is_empty() && !state.bucket.is_empty()).then(|| {
            row()
                .min_w_0()
                .child(label(t("统计完成前，搜索范围为已加载的目录页。"), 11.0, MUTED).truncate())
        });
    };
    let scanned_at = state.tree.scanned_at?;
    let scan_time = chrono::DateTime::from_timestamp_millis(scanned_at)?
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M");
    Some(
        row()
            .flex_1()
            .min_w_0()
            .justify_between()
            .gap(px(12.0))
            .child(
                label(
                    crate::lf!(
                        "{count} 个对象 · {size}",
                        count = totals.objects,
                        size = model::bytes(totals.bytes)
                    ),
                    11.0,
                    MUTED,
                )
                .test_id("tree-totals"),
            )
            .child(
                row()
                    .min_w_0()
                    .gap(px(10.0))
                    .when(
                        state.tree.dirty || tree_cache::expired(scanned_at),
                        |status| {
                            status.child(
                                label(t("缓存待刷新"), 11.0, DANGER).test_id("tree-cache-stale"),
                            )
                        },
                    )
                    .child(
                        label(crate::lf!("上次扫描 {}", scan_time), 11.0, MUTED)
                            .truncate()
                            .test_id("tree-cache-time"),
                    ),
            ),
    )
}

fn progress_label(totals: Totals) -> String {
    crate::lf!(
        "已扫描 {count} 个对象 · {size}",
        count = totals.objects,
        size = model::bytes(totals.bytes)
    )
}

fn activity(store: Store, state: &Workspace) -> Option<View> {
    let bar = row().px(px(12.0)).py(px(4.0)).gap(px(8.0));
    let retry = || {
        button("tree-rescan", t("重新扫描"))
            .disabled(state.is_busy())
            .px(px(6.0))
            .h(px(26.0))
            .child(label(t("重新扫描"), 11.0, ACCENT))
            .on_press(store.action(Workspace::refresh_objects))
    };
    Some(match &state.tree.status {
        TreeStatus::Scanning(totals) => bar
            .test_id("tree-scanning")
            .child(icon("loader-circle", 15.0, ACCENT))
            .child(label(progress_label(*totals), 11.0, MUTED))
            .child(
                button("tree-cancel-scan", t("取消扫描"))
                    .px(px(6.0))
                    .h(px(26.0))
                    .child(label(t("取消扫描"), 11.0, ACCENT))
                    .on_press(store.action(Workspace::cancel_tree_scan)),
            ),
        TreeStatus::Cancelled if state.tree.totals.is_none() => bar
            .child(label(t("已取消扫描"), 11.0, MUTED))
            .child(retry()),
        TreeStatus::Failed(error) => bar.child(error_notice(error)).child(retry()),
        _ if !state.tree.query_error.is_empty() => bar
            .child(error_notice(&state.tree.query_error))
            .child(retry()),
        _ if state.tree.querying() || matches!(state.tree.status, TreeStatus::Opening) => {
            bar.child(label(t("正在读取目录缓存…"), 11.0, MUTED).test_id("tree-querying"))
        }
        _ => return None,
    })
}

fn scan_state(store: Store, state: &Workspace) -> View {
    if state.tree.querying() || matches!(state.tree.status, TreeStatus::Opening) {
        column()
            .min_h(px(175.0))
            .justify_center()
            .items_center()
            .child(label(t("正在加载文件"), 15.0, MUTED))
    } else {
        empty_state(store, state)
    }
}

fn header(store: Store, state: &Workspace) -> View {
    row()
        .h(px(37.0))
        .bg(SUBTLE)
        .border_y(px(1.0))
        .border_color(LINE)
        .child(view().w(px(CHECK_WIDTH)).flex_shrink_0())
        .child(
            sort_header(store, state, TreeSort::Name, "tree-sort-name", t("名称"))
                .flex_1()
                .min_w_0(),
        )
        .child(
            sort_header(store, state, TreeSort::Size, "tree-sort-size", t("大小"))
                .w(px(SIZE_WIDTH))
                .justify_end()
                .pr(px(10.0)),
        )
        .child(
            sort_header(
                store,
                state,
                TreeSort::Objects,
                "tree-sort-count",
                t("对象数"),
            )
            .w(px(COUNT_WIDTH))
            .justify_end()
            .pr(px(10.0)),
        )
        .child(
            row()
                .w(px(SHARE_WIDTH))
                .flex_shrink_0()
                .justify_end()
                .pr(px(12.0))
                .child(label(t("占父目录"), 11.0, MUTED).truncate()),
        )
}

fn sort_header(
    store: Store,
    state: &Workspace,
    sort: TreeSort,
    id: &str,
    title: &str,
) -> nsh_base::ButtonElement {
    button(id, title.to_owned())
        .h(px(32.0))
        .justify_start()
        .gap(px(4.0))
        .child(label(title.to_owned(), 11.0, MUTED).truncate())
        .when(state.tree.options.sort == sort, |b| {
            b.child(icon(
                if state.tree.options.ascending {
                    "arrow-up"
                } else {
                    "arrow-down"
                },
                11.0,
                ACCENT,
            ))
        })
        .on_press(store.action(move |state| state.sort_tree(sort)))
}

fn item(store: Store, state: &Workspace, entry: &TreeRow) -> View {
    let disabled = state.is_busy() || state.tree.querying();
    let TreeRow::Entry {
        item,
        parent_bytes,
        depth,
        expandable,
        expanded,
    } = entry
    else {
        return level_controls(store, entry, disabled);
    };
    let (depth, expandable, expanded) = (*depth, *expandable, *expanded);
    let selected = state.tree.selected.contains(&item.key);
    let folder = item.key.folder;
    let kind = if folder { "folder" } else { "file" };
    let name = item.key.name();
    let check_key = item.key.clone();
    let press_key = item.key.clone();
    let fraction = parent_bytes.map(|parent| {
        if parent == 0 {
            0.0
        } else {
            item.totals.bytes as f64 / parent as f64
        }
    });
    let share = fraction.map_or_else(
        || "—".into(),
        |fraction| {
            if fraction > 0.0 && fraction < 0.001 {
                "<0.1%".into()
            } else {
                format!("{:.1}%", fraction * 100.0)
            }
        },
    );
    let unknown = folder && parent_bytes.is_none();
    let calculating = unknown && state.tree.calculating();
    let size = if calculating {
        t("计算大小中…").into()
    } else if unknown {
        "—".into()
    } else {
        model::bytes(item.totals.bytes)
    };
    row()
        .h(px(ROW_HEIGHT))
        .border_b_1()
        .border_color(LINE)
        .bg(if selected { SELECTED } else { SURFACE })
        .child(
            row()
                .w(px(CHECK_WIDTH))
                .flex_shrink_0()
                .justify_center()
                .child(
                    checkbox(
                        &format!("tree-check-{kind}-{}", item.key.key),
                        crate::lf!("选择 {}", name),
                        selected,
                        false,
                    )
                    .disabled(disabled)
                    .on_change(move |_: &bool| {
                        store.change(|state| state.select_tree_node(check_key.clone()))
                    }),
                ),
        )
        .child(
            row()
                .test_id(&format!("tree-{kind}-{}", item.key.key))
                .role(AccessibilityRole::TreeItem)
                .label(name.to_owned())
                .accessibility_level(depth as u32 + 1)
                .accessibility_state(nsh_core::AccessibilityState {
                    expanded: expandable.then_some(expanded),
                    disabled,
                    ..Default::default()
                })
                .disabled(disabled)
                .cursor(Cursor::Pointer)
                .hover(|s| s.bg(SUBTLE))
                .focus_visible(|s| s.outline(px(2.0)).outline_color(ACCENT))
                .h_full()
                .flex_1()
                .min_w_0()
                .justify_start()
                .pl(px(depth.min(8) as f32 * 14.0))
                .gap(px(6.0))
                .child(row().w(px(14.0)).flex_shrink_0().when(expandable, |r| {
                    r.child(icon(
                        if expanded {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        },
                        13.0,
                        MUTED,
                    ))
                }))
                .child(object_icon(&item.key.key, folder, 16.0))
                .child(label(name.to_owned(), 12.0, TEXT).min_w_0().truncate())
                .on_press(store.action(move |state| {
                    if expandable {
                        state.toggle_tree_folder(press_key.key.clone());
                    } else {
                        state.inspect_tree_node(press_key.clone());
                    }
                })),
        )
        .child(stat_column(SIZE_WIDTH, size).when(calculating, |cell| {
            cell.test_id(&format!("tree-size-pending-{}", item.key.key))
        }))
        .child(stat_column(
            COUNT_WIDTH,
            if unknown {
                "—".into()
            } else {
                item.totals.objects.to_string()
            },
        ))
        .child(
            row()
                .w(px(SHARE_WIDTH))
                .flex_shrink_0()
                .justify_end()
                .pr(px(12.0))
                .gap(px(6.0))
                .when(fraction.is_some(), |cell| {
                    cell.child(
                        progress(
                            fraction.unwrap_or_default() as f32,
                            if folder { FOLDER } else { ACCENT },
                        )
                        .w(px(49.0)),
                    )
                })
                .child(
                    row()
                        .w(px(44.0))
                        .justify_end()
                        .child(label(share, 11.0, MUTED)),
                ),
        )
}

fn level_controls(store: Store, entry: &TreeRow, disabled: bool) -> View {
    let bar = |depth: usize| {
        row()
            .h(px(ROW_HEIGHT))
            .gap(px(6.0))
            .pl(px(CHECK_WIDTH + depth.min(8) as f32 * 14.0))
            .pr(px(12.0))
            .border_b_1()
            .border_color(LINE)
            .bg(SUBTLE)
    };
    match entry {
        TreeRow::Pager {
            parent,
            depth,
            previous,
            next,
        } => {
            let mut controls = bar(*depth);
            for (id, title, page) in [("prev", t("上一页"), previous), ("next", t("下一页"), next)]
            {
                if let Some(page) = page.clone() {
                    controls = controls.child(
                        button(&format!("tree-{id}-{parent}"), title.to_owned())
                            .disabled(disabled)
                            .px(px(8.0))
                            .h(px(30.0))
                            .child(label(title, 11.0, ACCENT))
                            .on_press(store.action(move |state| state.page_tree(page.clone()))),
                    );
                }
            }
            controls
        }
        TreeRow::Level {
            parent,
            depth,
            status,
        } => match status {
            LevelStatus::Loading => bar(*depth)
                .child(icon("loader-circle", 13.0, MUTED))
                .child(label(t("正在加载文件"), 11.0, MUTED)),
            LevelStatus::Empty => bar(*depth).child(label(t("这里还没有文件"), 11.0, MUTED)),
            LevelStatus::Failed { error, page } => {
                let retry = TreePage::Directory {
                    parent: parent.clone(),
                    page: *page,
                };
                bar(*depth)
                    .child(
                        label(error.clone(), 11.0, DANGER)
                            .truncate()
                            .min_w_0()
                            .flex_1(),
                    )
                    .child(
                        button(&format!("tree-retry-{parent}"), t("重试"))
                            .disabled(disabled)
                            .px(px(6.0))
                            .h(px(26.0))
                            .child(label(t("重试"), 11.0, ACCENT))
                            .on_press(store.action(move |state| state.page_tree(retry.clone()))),
                    )
            }
        },
        TreeRow::Entry { .. } => unreachable!(),
    }
}

fn stat_column(width: f32, value: String) -> View {
    row()
        .w(px(width))
        .flex_shrink_0()
        .justify_end()
        .pr(px(10.0))
        .child(label(value, 11.0, MUTED).truncate())
}
