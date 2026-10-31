use super::*;
impl Workspace {
    pub(super) fn render_transfers(&self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.transfers.iter().filter(|t| model::active(t)).count();
        let paused = self
            .transfers
            .iter()
            .filter(|t| t.status == "paused")
            .count();
        let mut panel = col()
            .mt(px(23.))
            .border_1()
            .border_color(rgb(0xe7e9ee))
            .rounded(px(11.))
            .bg(rgba(0xffffffaa))
            .overflow_hidden()
            .flex_shrink_0()
            .child(
                row()
                    .gap(px(10.))
                    .child(
                        button("transfers-toggle", crate::i18n::t("传输活动"))
                            .flex_1()
                            .justify_between()
                            .py(px(15.))
                            .px(px(19.))
                            .text_color(rgb(0x818b9c))
                            .text_size(px(11.))
                            .child(
                                row()
                                    .gap(px(8.))
                                    .text_color(rgb(0x69778c))
                                    .child(icon("arrow-down-to-line", 16.))
                                    .child(crate::i18n::t("传输活动"))
                                    .when(active > 0, |s| {
                                        s.child(
                                            label(active.to_string(), 10., BLUE)
                                                .px(px(5.))
                                                .py(px(2.))
                                                .rounded(px(4.))
                                                .bg(rgb(0xeaf2ff)),
                                        )
                                    }),
                            )
                            .child(
                                row()
                                    .gap(px(14.))
                                    .child(label(
                                        if active > 0 {
                                            crate::lf!("{active} 项正在传输", active = active)
                                        } else if paused > 0 {
                                            crate::lf!("{paused} 项已暂停", paused = paused)
                                        } else {
                                            crate::i18n::t("一切已就绪").into()
                                        },
                                        9.,
                                        MUTED,
                                    ))
                                    .child(
                                        label(
                                            crate::lf!("{} 路并发", self.settings.concurrency),
                                            10.,
                                            0x3670b5,
                                        )
                                        .px(px(8.))
                                        .py(px(3.))
                                        .rounded(px(6.))
                                        .bg(rgb(0xeef5ff)),
                                    )
                                    .child(icon(
                                        if self.expanded {
                                            "chevron-down"
                                        } else {
                                            "chevron-right"
                                        },
                                        15.,
                                    )),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.expanded = !this.expanded;
                                cx.notify();
                            })),
                    )
                    .child(
                        icon_button(
                            "transfer-settings",
                            crate::i18n::t("传输设置"),
                            "settings-2",
                        )
                        .size(px(30.))
                        .rounded(px(9.))
                        .text_color(rgb(BLUE))
                        .bg(rgb(0xeef5ff))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_dialog(Dialog::TransferSettings, window, cx)
                        })),
                    ),
            );
        if !self.transfer_error.is_empty() {
            panel = panel.child(
                row()
                    .px(px(18.))
                    .py(px(12.))
                    .gap(px(8.))
                    .bg(rgb(0xfff3f2))
                    .text_color(rgb(0xbe625e))
                    .child(icon("info", 16.))
                    .child(label(self.transfer_error.clone(), 11., 0xbe625e).flex_1())
                    .child(
                        icon_button("close-transfer-error", crate::i18n::t("关闭传输错误"), "x")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.transfer_error.clear();
                                cx.notify();
                            })),
                    ),
            );
        }
        if self.expanded {
            if self.transfers.is_empty() {
                panel = panel.child(
                    row()
                        .border_t_1()
                        .border_color(rgb(0xf0f1f5))
                        .py(px(22.))
                        .px(px(25.))
                        .gap(px(14.))
                        .child(
                            row()
                                .size(px(32.))
                                .justify_center()
                                .rounded_full()
                                .bg(rgb(0xf0f6f4))
                                .text_color(rgb(0x8ab9aa))
                                .child(icon("check", 18.)),
                        )
                        .child(
                            col()
                                .child(label(
                                    crate::i18n::t("文件已就绪，随时开始。"),
                                    11.,
                                    0x8993a3,
                                ))
                                .child(
                                    label(
                                        crate::i18n::t("支持并发传输、暂停和应用重启后继续。"),
                                        10.,
                                        0xb0b7c2,
                                    )
                                    .mt(px(5.)),
                                ),
                        )
                        .child(label("↑  ↓", 32., 0xe3e8f1).ml_auto()),
                );
            } else {
                panel=panel.child(col().id("transfers-list").max_h(px(270.)).overflow_y_scroll().children(self.transfers.iter().enumerate().map(|(i,t)|{
                let id=t.id.clone();let cancel_id=id.clone();let percent=if t.total>0{(t.bytes as f32/t.total as f32*100.).min(100.)}else if t.status=="done"{100.}else{0.};
                let (status_icon,color)=if model::active(t){("loader-circle",0x8b98aa)}else{match t.status.as_str(){"done"=>("circle-check",0x66a790),"paused"=>("pause",0x8b98aa),"cancelled"=>("x",0x8b98aa),_=>("info",0xd87878)}};
                let mut controls=row().w(px(66.)).flex_shrink_0().justify_end().gap(px(4.));
                if matches!(t.status.as_str(),"running"|"preparing"|"queued"){let pause_id=id.clone();controls=controls.child(icon_button(("pause",i),&crate::lf!("暂停 {}",t.name),"pause").disabled(self.pending_transfers.contains(&id)).on_click(cx.listener(move|this,_,_,cx|this.control("pause_transfer",pause_id.clone(),cx))));}
                if matches!(t.status.as_str(),"paused"|"error"){let resume_id=id.clone();controls=controls.child(icon_button(("resume",i),&format!("{} {}",if t.status=="error"{crate::i18n::t("重试")}else{crate::i18n::t("继续")},t.name),if t.status=="error"{"rotate-ccw"}else{"play"}).disabled(self.pending_transfers.contains(&id)).on_click(cx.listener(move|this,_,_,cx|this.control("resume_transfer",resume_id.clone(),cx))));}
                if !matches!(t.status.as_str(),"done"|"cancelled"|"finalizing"|"pausing"){controls=controls.child(icon_button(("cancel-transfer",i),&crate::lf!("取消 {}",t.name),"x").disabled(self.pending_transfers.contains(&id)).on_click(cx.listener(move|this,_,window,cx|this.open_dialog(Dialog::CancelTransfer(cancel_id.clone()),window,cx))));}
                row().px(px(20.)).py(px(12.)).border_t_1().border_color(rgb(LINE)).gap(px(12.)).child(icon(status_icon,17.).text_color(rgb(color)))
                    .child(col().flex_1().min_w_0().child(label(t.name.clone(),10.,0x8b98aa).truncate())
                        .child(label(format!("{} · {} / {}{}",if t.direction==s3_studio_core::transfers::Direction::Upload{crate::i18n::t("上传")}else{crate::i18n::t("下载")},model::bytes(t.bytes),model::bytes(t.total),if t.status=="running"&&t.speed>0{format!(" · {}/s",model::bytes(t.speed))}else{String::new()}),9.,0x8b98aa).mt(px(4.)))
                        .when_some(t.error.clone(),|s,e|s.child(label(e,9.,0xc63b40).mt(px(4.))))
                        .child(div().mt(px(3.)).h(px(4.)).w_full().bg(rgb(0xe8edf3)).rounded(px(3.)).child(div().h_full().w(relative(percent/100.)).rounded(px(3.)).bg(rgb(match t.status.as_str(){"paused"=>0x97a6b9,"done"=>0x36a780,_=>BLUE})))))
                    .child(col().w(px(100.)).flex_shrink_0().items_end().gap(px(5.)).child(label(model::status_label(&t.status),11.,0x65758b)).when(t.status=="running"&&t.total>0,|s|s.child(label(format!("{}%",percent.round()),10.,0x8b99ab))))
                    .child(controls)
            })).when(self.transfers.iter().any(|t|matches!(t.status.as_str(),"done"|"cancelled")),|s|s.child(button("clear-transfers",crate::i18n::t("清除已完成及已取消的记录")).text_size(px(9.)).text_color(rgb(0xa3adbc)).mt(px(10.)).mb(px(12.)).mr(px(20.)).ml_auto().child(crate::i18n::t("清除已完成及已取消的记录")).on_click(cx.listener(|this,_,_,cx|this.clear_finished(cx))))));
            }
        }
        panel.into_any_element()
    }
}
