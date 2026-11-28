use super::*;
use crate::model::FileAction;

pub(super) fn description(action: FileAction) -> Option<&'static str> {
    use FileAction::*;
    Some(match action {
        UploadFolder => t("保留所选文件夹及其子目录。同名对象将被覆盖。"),
        Download => t("保留路径层级。未选择项目时下载当前目录，目标目录中的同名文件将被替换。"),
        CopyTo | MoveTo => t("选择目标工作空间，填写目标桶和目录。已有同名目标时停止。"),
        DeleteVersions => t("这些版本将永久删除，无法恢复。"),
        Properties => t("仅修改填写的属性。元数据和标签使用 JSON，值设为 null 可移除对应键。"),
        Sync => t("比较当前 S3 目录与本地目录，只传新增和内容不同的文件。同名文件按所选方向更新。"),
        Edit => t("选择可信的编辑器打开工作副本。保存后自动上传，远端内容变化时保留本地副本。"),
        Filter => t("筛选当前目录及子目录。日期使用 UTC，边界包含在内。"),
        _ => return None,
    })
}

pub(super) fn filters(store: Store, state: &Workspace, storage: bool) -> View {
    column()
        .gap(px(7.0))
        .child(field(store, state, "pattern", t("路径匹配"), false))
        .child(
            row()
                .gap(px(8.0))
                .child(field(store, state, "after", t("开始日期"), false).flex_1())
                .child(field(store, state, "before", t("结束日期"), false).flex_1()),
        )
        .child(
            row()
                .gap(px(8.0))
                .child(field(store, state, "minSize", t("最小字节数"), false).flex_1())
                .child(field(store, state, "maxSize", t("最大字节数"), false).flex_1()),
        )
        .when(storage, |v| {
            v.child(field(store, state, "storageClass", t("存储类型"), false))
        })
}

pub(super) fn form(store: Store, s: &Workspace, action: FileAction) -> View {
    use FileAction::*;
    let body = match action {
        UploadFolder | Download | Filter => filters(store, s, action != UploadFolder),
        CopyTo | MoveTo => {
            let mut accounts = row().gap(px(4.0)).flex_wrap();
            for account in &s.accounts {
                let id = account.id.clone();
                accounts = accounts.child(
                    secondary(&format!("target-{id}"), account.name.clone())
                        .selected(s.input("targetAccount") == id)
                        .selected_style(|s| s.bg(SELECTED))
                        .on_press(store.action(move |s| s.set_input("targetAccount", &id))),
                );
            }
            column()
                .gap(px(8.0))
                .child(label(t("目标工作空间"), 12.0, TEXT))
                .child(accounts)
                .when(s.selection_count() == 0, |v| {
                    v.child(label(
                        format!("{} · s3://{}/{}", t("当前目录"), s.bucket, s.prefix),
                        12.0,
                        TEXT,
                    ))
                })
                .child(field(store, s, "targetBucket", t("目标存储桶"), false))
                .child(field(store, s, "path", t("目标目录"), false))
        }
        Rename => field(store, s, "path", t("新名称"), true),
        Preview => {
            let mut body = column().gap(px(8.0)).child(label(
                s.first().map(|o| o.key).unwrap_or_default(),
                12.0,
                TEXT,
            ));
            if let Some(p) = &s.files.preview {
                if let Some(src) = &p.image {
                    body = body.child(
                        image(nsh_core::element::ImageSrc::from(src.clone()))
                            .w_full()
                            .h(px(315.0)),
                    );
                }
                if !p.columns.is_empty() {
                    let line = |cells: &[String], header: bool| {
                        row()
                            .gap(px(6.0))
                            .p(px(4.0))
                            .bg(if header { SUBTLE } else { SURFACE })
                            .child(
                                cells
                                    .iter()
                                    .map(|c| {
                                        label(c.clone(), 12.0, TEXT)
                                            .w(px(145.0))
                                            .flex_shrink_0()
                                            .selectable(true)
                                    })
                                    .collect::<Vec<_>>(),
                            )
                    };
                    body =
                        body.child(scroll().max_h(px(300.0)).child(
                            scroll().horizontal().child(
                                column().child(line(&p.columns, true)).child(
                                    p.rows.iter().map(|r| line(r, false)).collect::<Vec<_>>(),
                                ),
                            ),
                        ));
                }
                body = body.child(
                    scroll()
                        .max_h(px(330.0))
                        .child(label(p.text.clone(), 12.0, TEXT).selectable(true)),
                );
            }
            body.child(
                row()
                    .gap(px(5.0))
                    .child(
                        secondary("preview-prev", t("上一页"))
                            .disabled(s.is_busy() || s.files.page == 0)
                            .on_press(store.action(|s| {
                                s.files.page = s.files.page.saturating_sub(1);
                                s.fetch_preview();
                            })),
                    )
                    .child(label((s.files.page + 1).to_string(), 12.0, TEXT))
                    .child(
                        secondary("preview-next", t("下一页"))
                            .disabled(
                                s.is_busy() || !s.files.preview.as_ref().is_some_and(|p| p.next),
                            )
                            .on_press(store.action(|s| {
                                s.files.page += 1;
                                s.fetch_preview();
                            })),
                    )
                    .child(
                        secondary(
                            "preview-mode",
                            if s.files.hex {
                                t("自动预览")
                            } else {
                                t("十六进制")
                            },
                        )
                        .disabled(s.is_busy())
                        .on_press(store.action(|s| {
                            s.files.page = 0;
                            s.files.hex = !s.files.hex;
                            s.fetch_preview();
                        })),
                    ),
            )
        }
        Edit => column().child(label(
            s.first().map(|o| o.key).unwrap_or_default(),
            12.0,
            TEXT,
        )),
        Share => column()
            .gap(px(6.0))
            .child(field(store, s, "minutes", t("链接有效期（分钟）"), true))
            .child(field(store, s, "until", t("或指定到期时间"), false)),
        Properties => column()
            .gap(px(6.0))
            .child(field(store, s, "contentType", "Content-Type", false))
            .child(field(store, s, "cacheControl", "Cache-Control", false))
            .child(field(store, s, "class", t("存储类型"), false))
            .child(field(store, s, "metadata", t("自定义元数据"), false))
            .child(field(store, s, "tags", t("对象标签"), false)),
        DownloadVersions | DeleteVersions => column().child(
            label(
                format!(
                    "{}\n{}",
                    s.versions.key,
                    s.versions
                        .marked
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                12.0,
                TEXT,
            )
            .selectable(true),
        ),
        Sync => {
            let mut saved = row().gap(px(4.0)).flex_wrap();
            for (name, job) in &s.files.jobs {
                if job.account == s.active_id && job.bucket == s.bucket && job.prefix == s.prefix {
                    let name = name.clone();
                    saved = saved.child(
                        secondary(&format!("sync-job-{name}"), name.clone())
                            .on_press(store.action(move |s| s.load_sync_job(&name))),
                    );
                }
            }
            column()
                .gap(px(7.0))
                .child(label(format!("s3://{}/{}", s.bucket, s.prefix), 12.0, TEXT))
                .child(saved)
                .child(field(store, s, "job", t("任务名称"), true))
                .child(
                    row()
                        .gap(px(6.0))
                        .child(field(store, s, "local", t("本地目录"), false).flex_1())
                        .child(
                            secondary("sync-folder", t("选择目录"))
                                .on_press(store.action(Workspace::choose_local)),
                        ),
                )
                .child(
                    row()
                        .gap(px(6.0))
                        .child(
                            secondary("sync-upload", t("本地 → S3"))
                                .selected(!s.files.sync_download)
                                .selected_style(|s| s.bg(SELECTED))
                                .on_press(store.action(|s| s.files.sync_download = false)),
                        )
                        .child(
                            secondary("sync-download", t("S3 → 本地"))
                                .selected(s.files.sync_download)
                                .selected_style(|s| s.bg(SELECTED))
                                .on_press(store.action(|s| s.files.sync_download = true)),
                        ),
                )
                .child(filters(store, s, s.files.sync_download))
        }
    };
    body.when(!s.files.notice.is_empty(), |v| {
        v.child(
            scroll()
                .max_h(px(150.0))
                .child(label(s.files.notice.clone(), 12.0, TEXT).selectable(true)),
        )
    })
}
pub(super) fn actions(store: Store, state: &Workspace, action: FileAction) -> View {
    let mut row = row().gap(px(6.0)).justify_end().flex_wrap();
    if action != FileAction::Preview {
        row = row.child(
            secondary("file-close", t("关闭"))
                .disabled(state.is_busy())
                .on_press(store.action(Workspace::close_dialog)),
        );
    }
    if action == FileAction::Sync {
        row = row
            .child(
                secondary("sync-save", t("保存任务"))
                    .disabled(state.is_busy())
                    .on_press(store.action(|s| s.sync_action("files_sync_save"))),
            )
            .child(
                secondary("sync-run", t("执行同步"))
                    .disabled(state.is_busy())
                    .on_press(store.action(|s| s.sync_action("files_sync_run"))),
            );
    }
    let title = match action {
        FileAction::Preview => t("关闭"),
        FileAction::Sync => t("比较目录"),
        FileAction::Edit => t("打开或停止编辑"),
        _ => action.title(),
    };
    row.child(
        primary("file-submit", title, action == FileAction::DeleteVersions)
            .disabled(state.is_busy())
            .on_press(store.action(Workspace::submit)),
    )
}
