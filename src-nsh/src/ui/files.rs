mod tree;

use super::*;
use crate::{
    model::{self, FileAction},
    workspace::ObjectView,
};
use s3_studio_core::s3::S3Object;

pub(super) fn browser(store: Store, search: ElementRef) -> View {
    let visible = computed(move || store.read(Workspace::visible));
    let grid = computed(move || store.read(|state| state.object_view == ObjectView::Grid));
    let tree = computed(move || store.read(|state| state.object_view == ObjectView::Tree));
    let show_info = computed(move || store.read(|state| state.show_info));
    let narrow = computed(move || window().width.0 <= 1050.0);
    let compact = computed(move || narrow.get() || show_info.get());

    controls::panel()
        .child(dynamic(move || store.read(|state| toolbar(store, state))))
        .child(divider())
        .child(
            row()
                .px(px(15.0))
                .py(px(9.0))
                .gap(px(10.0))
                .justify_between()
                .child(search_field(store, search))
                .child(dynamic(move || {
                    store.read(|s| {
                        s.files.filter.is_some().then(|| {
                            secondary("clear-advanced-filter", t("清除筛选"))
                                .on_press(store.action(Workspace::clear_file_filter))
                        })
                    })
                }))
                .child(dynamic(move || {
                    store.read(|state| selection_actions(store, state))
                })),
        )
        .child(dynamic(move || {
            store.read(|state| {
                (!state.error.is_empty()).then(|| {
                    error_notice(&state.error).mx(px(10.0)).mb(px(8.0)).child(
                        icon_button("close-error", t("关闭错误"), "x")
                            .on_press(store.action(|state| state.error.clear())),
                    )
                })
            })
        }))
        .child(
            row()
                .items_stretch()
                .child(when(
                    move || !tree.get(),
                    move || objects(store, visible, grid, compact),
                ))
                .child(when(tree, move || tree::objects(store)))
                .child(when(show_info, move || {
                    scroll()
                        .test_id("object-inspector")
                        .w(move || px(if narrow.get() { 198.0 } else { 224.0 }))
                        .flex_shrink_0()
                        .border_l_1()
                        .border_color(LINE)
                        .bg(SUBTLE)
                        .child(dynamic(move || store.read(|state| inspector(store, state))))
                })),
        )
        .child(divider())
        .child(dynamic(move || {
            store.read(|state| {
                let footer = row().justify_between().px(px(15.0)).h(px(33.0)).bg(SUBTLE);
                if state.object_view == ObjectView::Tree {
                    return footer.child(tree::summary(state));
                }
                footer
                    .child(label(
                        crate::lf!(
                            "{} 个项目{}{}",
                            visible.with(Vec::len),
                            if state.search_query().is_empty() {
                                ""
                            } else {
                                t(" · 搜索结果")
                            },
                            if state.selected.is_empty() {
                                String::new()
                            } else {
                                crate::lf!(" · 已选择 {} 项", state.selected.len())
                            },
                        ),
                        11.0,
                        MUTED,
                    ))
                    .child(label(
                        crate::lf!(
                            "{} · 当前已加载",
                            model::bytes(state.objects.iter().map(|o| o.size.max(0) as u128).sum())
                        ),
                        11.0,
                        MUTED,
                    ))
            })
        }))
}

fn search_field(store: Store, search: ElementRef) -> View {
    let focused = signal(false);
    effect(move || {
        if focused.get() {
            on_key_with(
                "shift+enter",
                KeyOptions {
                    when: nsh_core::keymap::When::Always,
                    for_focus: true,
                    ..Default::default()
                },
                move |_| {
                    store.change(|state| {
                        if state.dialog.is_none() && !state.bucket.is_empty() {
                            state.open_dialog(Dialog::LocatePath);
                        }
                    });
                },
            );
        }
    });

    row()
        .test_id("object-search-field")
        .flex_1()
        .min_w_0()
        .max_w(px(470.0))
        .h(px(36.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(LINE)
        .focus_within(|style| style.border_color(ACCENT))
        .bg(SUBTLE)
        .child(
            input_field(
                "object-search",
                t("搜索当前目录"),
                t("搜索当前目录，或输入完整路径…"),
                move || store.read(|state| state.search.clone()),
            )
            .element_ref(&search)
            .label(move || {
                store.read(|state| {
                    SharedString::from(if state.object_view == ObjectView::Tree {
                        t("搜索对象树")
                    } else {
                        t("搜索当前目录")
                    })
                })
            })
            .placeholder(move || {
                store.read(|state| {
                    if state.object_view == ObjectView::Tree {
                        t("搜索对象树，或输入完整路径…")
                    } else {
                        t("搜索当前目录，或输入完整路径…")
                    }
                    .to_owned()
                })
            })
            .flex_1()
            .h_full()
            .border(px(0.0))
            .bg(AdaptiveColor::TRANSPARENT)
            .on_focus(move || focused.set(true))
            .on_blur(move || focused.set(false))
            .on_change(move |value: &str| store.set_search(value))
            .on_submit(move |value: &str| store.change(|state| state.locate_search(value))),
        )
        .child(when(
            move || store.read(|state| !state.search.is_empty()),
            move || {
                icon_button("clear-search", t("清除搜索"), "x")
                    .mr(px(3.0))
                    .hover(|style| style.bg(LINE))
                    .on_press(move || {
                        store.set_search("");
                        search.focus();
                    })
            },
        ))
}

fn toolbar(store: Store, state: &Workspace) -> View {
    let mut crumbs = row().h(px(34.0)).gap(px(3.0)).flex_shrink_0().child(
        button("root-directory", t("根目录"))
            .h(px(30.0))
            .px(px(5.0))
            .disabled(state.bucket.is_empty() || state.is_busy())
            .child(icon("hard-drive", 15.0, MUTED))
            .child(label(t("根目录"), 11.0, MUTED))
            .on_press(store.action(|state| state.navigate(state.bucket.clone(), String::new()))),
    );
    // Empty S3 path components have their own clickable slash.
    let mut prefix = String::new();
    for part in state.prefix.split_inclusive('/') {
        prefix.push_str(part);
        let target = prefix.clone();
        let title = if part == "/" {
            part
        } else {
            crumbs = crumbs.child(label("/", 12.0, MUTED).flex_shrink_0());
            part.strip_suffix('/').unwrap_or(part)
        };
        crumbs = crumbs.child(
            button(&format!("breadcrumb-{target}"), title.to_owned())
                .h(px(30.0))
                .px(px(4.0))
                .disabled(state.is_busy())
                .child(label(title.to_owned(), 11.0, TEXT).flex_shrink_0())
                .on_press(
                    store.action(move |state| state.navigate(state.bucket.clone(), target.clone())),
                ),
        );
    }
    row()
        .h(px(48.0))
        .px(px(12.0))
        .gap(px(8.0))
        .child(
            scroll()
                .test_id("object-breadcrumbs")
                .horizontal()
                .h(px(34.0))
                .flex_1()
                .min_w_0()
                .child(crumbs),
        )
        .child(
            row()
                .gap(px(4.0))
                .flex_shrink_0()
                .child(file_menu(store, state))
                .child(
                    icon_button("new-folder", t("新建文件夹"), "folder-plus")
                        .disabled(state.disabled())
                        .on_press(store.open(Dialog::Folder)),
                )
                .child(
                    icon_button("refresh", t("刷新"), "refresh-cw")
                        .disabled(state.disabled())
                        .on_press(store.action(Workspace::refresh_objects)),
                )
                .child(
                    icon_button("list-view", t("列表视图"), "list")
                        .selected(state.object_view == ObjectView::List)
                        .selected_style(|s| s.bg(SELECTED))
                        .disabled(state.is_busy())
                        .on_press(store.action(|state| state.set_object_view(ObjectView::List))),
                )
                .child(
                    icon_button("grid-view", t("网格视图"), "layout-grid")
                        .selected(state.object_view == ObjectView::Grid)
                        .selected_style(|s| s.bg(SELECTED))
                        .disabled(state.is_busy())
                        .on_press(store.action(|state| state.set_object_view(ObjectView::Grid))),
                )
                .child(
                    icon_button("tree-view", t("树状视图"), "list-tree")
                        .selected(state.object_view == ObjectView::Tree)
                        .selected_style(|s| s.bg(SELECTED))
                        .disabled(state.is_busy())
                        .on_press(store.action(|state| state.set_object_view(ObjectView::Tree))),
                )
                .child(
                    icon_button("toggle-info", t("对象详情"), "info")
                        .selected(state.show_info)
                        .selected_style(|s| s.bg(SELECTED))
                        .on_press(store.action(|state| {
                            state.show_info = !state.show_info;
                            state.selection_changed();
                        })),
                ),
        )
}

fn file_menu(store: Store, state: &Workspace) -> impl IntoElement {
    use FileAction::*;
    let selected = state.selection_count();
    let single = state.first().is_some_and(|o| !o.folder);
    let mut popup = menu()
        .align(Align::End)
        .offset(px(4.0))
        .popup_style(sidebar_menu_style)
        .trigger(secondary("file-actions", t("文件操作")).disabled(state.disabled()));
    for (id, action, icon, enabled) in [
        ("upload-folder", UploadFolder, "arrow-up-to-line", true),
        ("download", Download, "arrow-down-to-line", true),
        ("copy", CopyTo, "copy", true),
        ("move", MoveTo, "folder", true),
        ("rename", Rename, "file", selected == 1),
        ("preview", Preview, "info", single),
        ("edit", Edit, "file-code-corner", single),
        ("sync", Sync, "refresh-cw", true),
        ("filter", Filter, "list", true),
        ("share", Share, "link", selected > 0),
        ("properties", Properties, "settings-2", selected > 0),
    ] {
        popup = popup.child(
            sidebar_menu_item(&format!("files-{id}"), action.title(), icon, TEXT)
                .disabled(!enabled)
                .on_select(store.open(Dialog::Files(action))),
        );
    }
    popup
}

fn selection_actions(store: Store, state: &Workspace) -> Option<View> {
    let count = state.selection_count();
    if count == 0 {
        return None;
    }
    let single_file = state.first().is_some_and(|first| !first.folder);
    let busy = state.is_busy() || (state.object_view == ObjectView::Tree && state.tree.querying());
    let disabled = busy || !single_file;
    Some(
        row()
            .gap(px(4.0))
            .flex_shrink_0()
            .child(label(crate::lf!("已选 {} 项", count), 11.0, MUTED).mr(px(4.0)))
            .child(
                icon_button(
                    "download-selection",
                    t("下载所选文件"),
                    "arrow-down-to-line",
                )
                .disabled(busy)
                .on_press(store.action(|s| {
                    if s.first().is_some_and(|o| !o.folder) {
                        s.download()
                    } else {
                        s.open_dialog(Dialog::Files(FileAction::Download))
                    }
                })),
            )
            .child(
                icon_button("versions-selection", t("版本历史"), "history")
                    .disabled(disabled || count != 1)
                    .on_press(store.action(|s| {
                        if let Some(object) = s.first() {
                            s.open_versions(object.key.clone());
                        }
                    })),
            )
            .child(
                icon_button("copy-selection", t("复制所选对象"), "copy")
                    .disabled(busy)
                    .on_press(store.action(|s| {
                        if s.first().is_some_and(|o| !o.folder) {
                            s.open_dialog(Dialog::Copy)
                        } else {
                            s.open_dialog(Dialog::Files(FileAction::CopyTo))
                        }
                    })),
            )
            .child(
                icon_button("share-selection", t("复制分享链接"), "link")
                    .disabled(busy)
                    .on_press(store.action(|s| {
                        if s.first().is_some_and(|o| !o.folder) {
                            s.share()
                        } else {
                            s.open_dialog(Dialog::Files(FileAction::Share))
                        }
                    })),
            )
            .child(
                icon_button("delete-selection", t("删除所选项目"), "trash")
                    .disabled(busy)
                    .on_press(store.open(Dialog::Delete)),
            ),
    )
}

fn objects(
    store: Store,
    visible: Memo<Vec<S3Object>>,
    grid: Memo<bool>,
    compact: Memo<bool>,
) -> View {
    let root = element_ref();
    let location = computed(move || {
        store.read(|state| {
            (
                state.active_id.clone(),
                state.bucket.clone(),
                state.prefix.clone(),
                state.located.as_ref().map(|(_, revision)| *revision),
            )
        })
    });
    let list_ref = root.clone();
    effect(move || {
        let _ = location.get();
        list_ref.scroll_to(nsh_core::element::point(px(0.0), px(0.0)), false);
    });
    let trash = computed(move || store.read(show_trash));
    let list = virtual_list(
        move || visible.with(Vec::len) + usize::from(trash.get()),
        move |position| {
            dynamic(move || {
                let index = position.index();
                if trash.get() && index == 0 {
                    return Some(store.read(|state| trash_entry(store, state, grid.get())));
                }
                let index = index - usize::from(trash.get());
                let grid = grid.get();
                let compact = compact.get();
                visible.with(|objects| {
                    let object = objects.get(index)?;
                    Some(store.read(|state| {
                        if grid {
                            card(store, state, object)
                        } else {
                            object_row(store, state, object, compact)
                        }
                    }))
                })
            })
        },
    )
    .keys(move || {
        visible.with(|objects| {
            trash
                .get()
                .then(|| (true, String::new()))
                .into_iter()
                .chain(objects.iter().map(|object| (false, object.key.clone())))
                .collect::<Vec<_>>()
        })
    })
    .estimated_item_size(move || px(if grid.get() { 135.0 } else { 43.0 }))
    .columns(move || {
        if !grid.get() {
            return 1;
        }
        let width = window().width.0;
        let sidebar = if width <= 1050.0 {
            215.0
        } else if width >= 1450.0 {
            268.0
        } else {
            248.0
        };
        let inspector = store.read(|state| if state.show_info { 224.0 } else { 0.0 });
        ((width - sidebar - inspector - 75.0) / 150.0)
            .floor()
            .max(1.0) as u32
    })
    .content_padding_x(move || px(if grid.get() { 9.0 } else { 0.0 }))
    .element_ref(&root)
    .test_id("objects")
    .flex_1()
    .min_h_0()
    .empty(dynamic(move || {
        store.read(|state| empty_state(store, state))
    }))
    .footer(dynamic(move || {
        store.read(|state| {
            state.next_token.as_ref().map(|_| {
                column()
                    .items_center()
                    .gap(px(5.0))
                    .py(px(10.0))
                    .child(
                        secondary(
                            "load-more",
                            if state.loading {
                                t("加载中…")
                            } else {
                                t("加载更多对象")
                            },
                        )
                        .disabled(state.loading || state.is_busy())
                        .on_press(store.action(|state| state.load_objects(true))),
                    )
                    .child(label(t("搜索和排序适用于已加载的对象"), 10.0, MUTED))
            })
        })
    }));
    column()
        .flex_1()
        .min_w_0()
        .h(objects_height)
        .child(when(
            move || !grid.get() && visible.with(|objects| !objects.is_empty()),
            move || {
                dynamic(move || {
                    visible.with(|objects| {
                        store.read(|state| table_header(store, state, objects, compact.get()))
                    })
                })
            },
        ))
        .child(list)
}

fn show_trash(state: &Workspace) -> bool {
    !state.bucket.is_empty() && state.prefix.is_empty() && state.search_query().is_empty()
}

fn trash_entry(store: Store, state: &Workspace, grid: bool) -> AnyElement {
    button(
        "open-trash",
        if state.trash_availability_error.is_empty() {
            t("回收站").to_owned()
        } else {
            format!("{}: {}", t("回收站"), state.trash_availability_error)
        },
    )
    .h(px(if grid {
        135.0
    } else if state.object_view == ObjectView::Tree {
        40.0
    } else {
        43.0
    }))
    .w_full()
    .justify_start()
    .px(px(if grid { 18.0 } else { 42.0 }))
    .gap(px(9.0))
    .border_b_1()
    .border_color(LINE)
    .hover(|style| style.bg(SUBTLE))
    .disabled(state.is_busy() || state.trash_available != Some(true))
    .child(icon(
        "recycle-bin",
        if grid { 28.0 } else { 20.0 },
        if state.trash_available == Some(true) {
            ACCENT
        } else {
            MUTED
        },
    ))
    .child(label(t("回收站"), 12.0, TEXT))
    .on_press(store.action(Workspace::open_trash))
    .into_element()
}

fn objects_height() -> Points {
    px((window().height.0 - 478.0).clamp(250.0, 620.0))
}

fn empty_state(store: Store, state: &Workspace) -> View {
    let loading = state.loading && state.objects.is_empty();
    let title = if loading {
        t("正在加载文件")
    } else if !state.search_query().is_empty() {
        t("没有找到匹配的文件")
    } else if state.account().is_none() {
        t("一切从连接开始")
    } else if state.bucket.is_empty() {
        t("创建你的第一个存储桶")
    } else {
        t("这里还没有文件")
    };
    column()
        .min_h(px(215.0))
        .items_center()
        .justify_center()
        .gap(px(12.0))
        .p(px(16.0))
        .child(
            row()
                .size(px(57.0))
                .justify_center()
                .rounded(px(15.0))
                .bg(SELECTED)
                .child(icon(
                    if loading { "loader-circle" } else { "cloud" },
                    28.0,
                    ACCENT,
                )),
        )
        .child(label(title, 15.0, TEXT).font_weight(FontWeight::MEDIUM))
        .when(state.account().is_none() && !loading, |empty| {
            empty.child(
                primary("first-account", t("连接第一个账户"), false)
                    .disabled(state.is_busy())
                    .on_press(store.open(Dialog::Account(None))),
            )
        })
}

fn table_header(store: Store, state: &Workspace, visible: &[S3Object], compact: bool) -> View {
    let count = visible
        .iter()
        .filter(|object| state.selected.contains(&object.key))
        .count();
    let all = count == visible.len();
    row()
        .h(px(37.0))
        .bg(SUBTLE)
        .border_y(px(1.0))
        .border_color(LINE)
        .child(
            row().w(px(42.0)).justify_center().flex_shrink_0().child(
                checkbox("select-all", t("选择当前可见对象"), all, count > 0 && !all)
                    .disabled(state.is_busy())
                    .on_change(move |next: &bool| {
                        store.change(|state| {
                            state.selected = if *next {
                                state
                                    .visible()
                                    .into_iter()
                                    .map(|object| object.key)
                                    .collect()
                            } else {
                                Default::default()
                            };
                            state.selection_changed();
                        })
                    }),
            ),
        )
        .child(
            row()
                .flex_1()
                .min_w_0()
                .child(
                    sort_button(store, state, Sort::Name, "sort-name", t("名称"))
                        .flex_1()
                        .min_w_0(),
                )
                .child(sort_button(store, state, Sort::Size, "sort-size", t("大小")).w(px(87.0)))
                .child(
                    sort_button(store, state, Sort::Modified, "sort-modified", t("修改时间"))
                        .w(px(104.0)),
                )
                .when(!compact, |r| {
                    r.child(label(t("存储类型"), 11.0, MUTED).w(px(106.0)))
                }),
        )
        .child(view().w(px(42.0)).flex_shrink_0())
}

fn sort_button(
    store: Store,
    state: &Workspace,
    sort: Sort,
    id: &str,
    title: &str,
) -> nsh_base::ButtonElement {
    button(id, title.to_owned())
        .h(px(32.0))
        .justify_start()
        .child(label(title.to_owned(), 11.0, MUTED).truncate())
        .when(state.sort == sort, |b| {
            b.child(icon(
                if state.ascending {
                    "arrow-up"
                } else {
                    "arrow-down"
                },
                11.0,
                ACCENT,
            ))
        })
        .on_press(store.action(move |state| state.sort_by(sort)))
}

fn object_row(store: Store, state: &Workspace, object: &S3Object, compact: bool) -> AnyElement {
    let key = object.key.clone();
    let check_key = key.clone();
    let info_key = key.clone();
    let name = model::object_name(&key, &state.prefix).to_owned();
    let selected = state.selected.contains(&key);
    let disabled = state.is_busy();
    row()
        .h(px(43.0))
        .border_b_1()
        .border_color(LINE)
        .bg(if selected { SELECTED } else { SURFACE })
        .child(
            row().w(px(42.0)).justify_center().flex_shrink_0().child(
                checkbox(
                    &format!("object-check-{key}"),
                    crate::lf!("选择 {}", name),
                    selected,
                    false,
                )
                .disabled(disabled)
                .on_change(move |_: &bool| store.change(|state| state.toggle(&check_key))),
            ),
        )
        .child(
            row()
                .flex_1()
                .min_w_0()
                .h_full()
                .gap(px(7.0))
                .test_id(&format!("object-{key}"))
                .role(AccessibilityRole::Button)
                .label(name.clone())
                .disabled(disabled)
                .cursor(Cursor::Pointer)
                .hover(|s| s.bg(SUBTLE))
                .focus_visible(|s| s.outline(px(2.0)).outline_color(ACCENT))
                .on_press(object_press(store, key.clone(), object.folder))
                .child(
                    row()
                        .flex_1()
                        .min_w_0()
                        .gap(px(9.0))
                        .child(file_icon(object, 18.0))
                        .child(
                            label(name.clone(), 12.0, TEXT)
                                .truncate()
                                .flex_1()
                                .min_w_0(),
                        ),
                )
                .child({
                    let key = key.clone();
                    let folder = object.folder;
                    let bytes = object.size.max(0) as u128;
                    dynamic(move || {
                        store.read_sizes(|state| {
                            let size = if folder {
                                state
                                    .tree
                                    .list_sizes
                                    .get(&key)
                                    .map(|size| model::bytes(*size))
                                    .unwrap_or_else(|| {
                                        if state.tree.calculating() {
                                            t("计算大小中…").into()
                                        } else {
                                            "—".into()
                                        }
                                    })
                            } else {
                                model::bytes(bytes)
                            };
                            label(size, 11.0, MUTED)
                                .test_id(&format!("object-size-{key}"))
                                .w(px(80.0))
                                .flex_shrink_0()
                                .truncate()
                        })
                    })
                })
                .child(
                    label(
                        if object.folder {
                            "—".into()
                        } else {
                            model::date(&object.modified)
                        },
                        11.0,
                        MUTED,
                    )
                    .w(px(97.0)),
                )
                .when(!compact, |r| {
                    let storage_class = if object.folder {
                        t("文件夹")
                    } else if object.storage_class.is_empty() {
                        "STANDARD"
                    } else {
                        &object.storage_class
                    };
                    r.child(
                        label(storage_class.to_owned(), 10.0, MUTED)
                            .w(px(99.0))
                            .truncate(),
                    )
                }),
        )
        .child(
            icon_button(
                &format!("more-{key}"),
                crate::lf!("查看 {}", name),
                "ellipsis",
            )
            .mx(px(6.0))
            .disabled(disabled)
            .on_press(store.action(move |state| state.inspect(info_key.clone()))),
        )
        .into_element()
}

fn card(store: Store, state: &Workspace, object: &S3Object) -> AnyElement {
    let key = object.key.clone();
    let name = model::object_name(&key, &state.prefix).to_owned();
    let selected = state.selected.contains(&key);
    column()
        .p(px(5.0))
        .h(px(135.0))
        .child(
            column()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(7.0))
                .min_w_0()
                .px(px(8.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(LINE)
                .bg(if selected { SELECTED } else { SUBTLE })
                .hover(|s| s.border_color(ACCENT))
                .focus_visible(|s| s.outline(px(2.0)).outline_color(ACCENT))
                .role(AccessibilityRole::Button)
                .label(name.clone())
                .disabled(state.is_busy())
                .test_id(&format!("card-{key}"))
                .on_press(object_press(store, key, object.folder))
                .child(file_icon(object, 34.0))
                .child(label(name, 12.0, TEXT).max_w(percentage(1.0)).truncate())
                .child(label(
                    if object.folder {
                        t("文件夹").into()
                    } else {
                        model::bytes(object.size.max(0) as u128)
                    },
                    10.0,
                    MUTED,
                ))
                .when(selected, |card| {
                    card.child(
                        view()
                            .absolute()
                            .top(px(8.0))
                            .right(px(8.0))
                            .child(icon("check", 14.0, ACCENT)),
                    )
                }),
        )
        .into_element()
}

fn object_press(store: Store, key: String, folder: bool) -> impl Fn(&PressEvent) {
    move |event| {
        store.change(|state| {
            if event.clicks >= 2 {
                if folder {
                    state.navigate(state.bucket.clone(), key.clone());
                } else {
                    state.inspect(key.clone());
                }
            } else {
                state.toggle(&key);
            }
        })
    }
}

fn inspector(store: Store, state: &Workspace) -> View {
    let mut panel = column().p(px(12.0)).gap(px(10.0)).child(
        row()
            .justify_between()
            .child(label(t("对象详情"), 12.0, TEXT).font_weight(FontWeight::MEDIUM))
            .child(
                icon_button("close-info", t("关闭详情"), "x").on_press(store.action(|state| {
                    state.show_info = false;
                    state.selection_changed();
                })),
            ),
    );
    let Some(object) = state.first() else {
        return panel.child(label(t("选择一个对象以查看详情。"), 11.0, MUTED));
    };
    let info = state.info.as_ref();
    let folder_totals =
        if object.folder && state.object_view == ObjectView::Tree && state.tree.totals.is_some() {
            state
                .tree
                .selected
                .first()
                .and_then(|key| state.tree.entry(key))
                .map(|item| item.totals)
        } else {
            None
        };
    let mut entries = vec![
        (t("对象键").into(), object.key.clone()),
        (
            t("大小").into(),
            if object.folder {
                folder_totals.map_or_else(
                    || {
                        if state.object_view == ObjectView::Tree && state.tree.calculating() {
                            t("计算大小中…").into()
                        } else {
                            t("文件夹").into()
                        }
                    },
                    |totals| model::bytes(totals.bytes),
                )
            } else {
                model::bytes(info.map_or(object.size, |info| info.size).max(0) as u128)
            },
        ),
        (
            t("最后修改").into(),
            model::date(
                info.map(|info| info.modified.as_str())
                    .filter(|time| !time.is_empty())
                    .unwrap_or(&object.modified),
            ),
        ),
        (
            t("存储类型").into(),
            if object.storage_class.is_empty() {
                "—".into()
            } else {
                object.storage_class.clone()
            },
        ),
    ];
    if let Some(totals) = folder_totals {
        entries.insert(2, (t("对象数").into(), totals.objects.to_string()));
    }
    if let Some(info) = info {
        entries.push(("Content-Type".into(), info.content_type.clone()));
        entries.push(("ETag".into(), info.etag.clone()));
        let mut metadata: Vec<_> = info.metadata.iter().collect();
        metadata.sort_by_key(|(key, _)| *key);
        entries.extend(
            metadata
                .into_iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
    panel = panel
        .child(
            column()
                .items_center()
                .gap(px(8.0))
                .py(px(8.0))
                .child(file_icon(&object, 35.0))
                .child(
                    label(
                        model::object_name(&object.key, &state.prefix).to_owned(),
                        12.0,
                        TEXT,
                    )
                    .max_w(percentage(1.0))
                    .truncate(),
                ),
        )
        .child(
            entries
                .into_iter()
                .map(|(key, value)| {
                    column()
                        .gap(px(2.0))
                        .min_w_0()
                        .child(label(key, 10.0, MUTED).truncate())
                        .child(label(value, 11.0, TEXT).selectable(true))
                })
                .collect::<Vec<_>>(),
        )
        .when(!state.info_error.is_empty(), |panel| {
            panel.child(label(state.info_error.clone(), 11.0, DANGER))
        });
    panel.when(!object.folder, |panel| {
        panel.child(
            secondary("inspector-download", t("下载文件"))
                .disabled(state.is_busy())
                .on_press(store.action(Workspace::download)),
        )
    })
}

fn file_icon(object: &S3Object, size: f32) -> impl IntoElement {
    object_icon(&object.key, object.folder, size)
}

fn object_icon(key: &str, folder: bool, size: f32) -> impl IntoElement + use<> {
    let (name, ink) = if folder {
        ("folder", FOLDER)
    } else {
        match key
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "png" | "jpg" | "jpeg" | "webp" | "svg" => ("file-image", IMAGE),
            "zip" | "gz" | "tar" => ("file-archive", ARCHIVE),
            "json" | "js" | "ts" | "html" => ("file-code-corner", CODE),
            "pdf" | "md" | "txt" => ("file-text", MUTED),
            _ => ("file", MUTED),
        }
    };
    icon(name, size, ink)
}
