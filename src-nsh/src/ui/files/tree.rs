use super::*;
use crate::{
    object_tree::{ObjectTree, Totals, TreeRow, TreeSort},
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
                let row = rows.with(|rows| rows.get(position.index()).copied())?;
                store.read(|state| {
                    state
                        .tree
                        .snapshot()
                        .map(|tree| item(store, state, tree, row))
                })
            })
        },
    )
    .keys(move || rows.with(|rows| rows.iter().map(|row| row.id).collect::<Vec<_>>()))
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
        .child(when(
            move || rows.with(|rows| !rows.is_empty()),
            move || dynamic(move || store.read(|state| header(store, state))),
        ))
        .child(list)
}

pub(super) fn summary(state: &Workspace) -> Option<Text> {
    let totals = state.tree.snapshot()?.totals();
    Some(
        label(
            crate::lf!(
                "{count} 个对象 · {size}",
                count = totals.objects,
                size = model::bytes(totals.bytes),
            ),
            11.0,
            MUTED,
        )
        .test_id("tree-totals"),
    )
}

fn scan_state(store: Store, state: &Workspace) -> View {
    let panel = column()
        .min_h(px(215.0))
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .p(px(16.0));
    match &state.tree.status {
        TreeStatus::Scanning(Totals { bytes, objects }) => panel
            .test_id("tree-scanning")
            .child(icon("loader-circle", 25.0, ACCENT))
            .child(label(t("正在扫描目录及其子目录"), 15.0, TEXT).font_weight(FontWeight::MEDIUM))
            .child(label(
                crate::lf!(
                    "已扫描 {count} 个对象 · {size}",
                    count = objects,
                    size = model::bytes(*bytes),
                ),
                11.0,
                MUTED,
            ))
            .child(
                secondary("tree-cancel-scan", t("取消扫描"))
                    .on_press(store.action(Workspace::cancel_tree_scan)),
            ),
        TreeStatus::Cancelled | TreeStatus::Failed(_) => panel
            .child(match &state.tree.status {
                TreeStatus::Failed(error) => error_notice(error),
                _ => row().child(label(t("已取消扫描"), 15.0, TEXT)),
            })
            .child(
                secondary("tree-rescan", t("重新扫描"))
                    .disabled(state.is_busy())
                    .on_press(store.action(|state| state.load_objects(false))),
            ),
        TreeStatus::Idle | TreeStatus::Ready(_) => empty_state(store, state),
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
        .when(state.tree.sort == sort, |b| {
            b.child(icon(
                if state.tree.ascending {
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

fn item(store: Store, state: &Workspace, tree: &ObjectTree, entry: TreeRow) -> View {
    let id = entry.id;
    let node = tree.node(id);
    let selected = state.tree.selected.contains(&id);
    let folder = node.folder();
    let kind = if folder { "folder" } else { "file" };
    let name = node.name();
    let disabled = state.is_busy();
    let fraction = tree.parent_fraction(id);
    let share = if fraction > 0.0 && fraction < 0.001 {
        "<0.1%".into()
    } else {
        format!("{:.1}%", fraction * 100.0)
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
                        &format!("tree-check-{kind}-{}", node.key),
                        crate::lf!("选择 {}", name),
                        selected,
                        false,
                    )
                    .disabled(disabled)
                    .on_change(move |_: &bool| store.change(|state| state.select_tree_node(id))),
                ),
        )
        .child(
            row()
                .test_id(&format!("tree-{kind}-{}", node.key))
                .role(AccessibilityRole::TreeItem)
                .label(name.to_owned())
                .accessibility_level(entry.depth as u32 + 1)
                .accessibility_state(nsh_core::AccessibilityState {
                    expanded: entry.expandable.then_some(entry.expanded),
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
                .pl(px(entry.depth.min(8) as f32 * 14.0))
                .gap(px(6.0))
                .child(
                    row()
                        .w(px(14.0))
                        .flex_shrink_0()
                        .when(entry.expandable, |r| {
                            r.child(icon(
                                if entry.expanded {
                                    "chevron-down"
                                } else {
                                    "chevron-right"
                                },
                                13.0,
                                MUTED,
                            ))
                        }),
                )
                .child(object_icon(&node.key, folder, 16.0))
                .child(label(name.to_owned(), 12.0, TEXT).min_w_0().truncate())
                .on_press(store.action(move |state| {
                    if entry.expandable {
                        state.toggle_tree_folder(id);
                    } else {
                        state.inspect_tree_node(id);
                    }
                })),
        )
        .child(stat_column(SIZE_WIDTH, model::bytes(node.totals.bytes)))
        .child(stat_column(COUNT_WIDTH, node.totals.objects.to_string()))
        .child(
            row()
                .w(px(SHARE_WIDTH))
                .flex_shrink_0()
                .justify_end()
                .pr(px(12.0))
                .gap(px(6.0))
                .child(progress(fraction as f32, if folder { FOLDER } else { ACCENT }).w(px(49.0)))
                .child(
                    row()
                        .w(px(44.0))
                        .justify_end()
                        .child(label(share, 11.0, MUTED)),
                ),
        )
}

fn stat_column(width: f32, value: String) -> View {
    row()
        .w(px(width))
        .flex_shrink_0()
        .justify_end()
        .pr(px(10.0))
        .child(label(value, 11.0, MUTED).truncate())
}
