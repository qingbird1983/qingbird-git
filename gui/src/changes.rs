//! 未提交变更。**提交对话框的「变更摘要」与中栏「变更」页签共用这一份清单**。
//!
//! 两处数据同源、格式也同源。写两份的话，改一次排序规则就得记得改两处——
//! 漏一处，两屏就会对同一份改动给出不同的答案。
//!
//! 排序在界面这边做（core 只给 git 的原始顺序），而且**顺序是定的**：
//! 已暂存优先，其余按路径排。git 的原始顺序随文件系统变，
//! 同一份改动两次打开对话框列出来的次序不一样，会让人以为多了或少了东西。
//!
//! 2026-10-05：`changes_panel` 从检视抽屉搬进中栏。右栏那三个页签
//! （日志 / 变更 / 历史 / 空间）与中栏的历史、空间是同一份数据、同一套内容，
//! 两处并排就是两个入口看同一件事；日志那一格则整块搬去中栏概览页底部。

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable, Size, Theme};
use gpui_kit::{
    div, px, Context, Div, InteractiveElement, IntoElement, ParentElement, Styled,
};
use qingbird_core::repos::ChangeEntry;

use gpui_kit::prelude::FluentBuilder as _;

use crate::state::*;
use crate::tokens::*;
use crate::widgets::{ghost_variant, CardHeader};

impl Shell {
    /// 「变更」页签正文：文件数 + 清单 + 一把「提交并推送…」。
    ///
    /// **这一页是只读的**：改工作区（暂存、丢弃、编辑）不在这里做。
    /// 那是 git 自己的事，而要改就得给出每行的状态与操作，一页塞不下；
    /// 真正的写入口是「提交并推送…」那把键，它开对话框逐项确认
    pub(crate) fn changes_panel(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let changes = self
            .selected
            .as_deref()
            .and_then(|p| self.view(p))
            .map(|v| v.changes.clone())
            .unwrap_or_default();
        let third = self.selected_is_third();
        let count = if changes.is_empty() {
            "0 个文件".to_string()
        } else {
            format!("{} 个文件待提交", changes.len())
        };
        let mut hdr = CardHeader::new(count);
        // 提交是改自己的历史，第三方仓库不出现这把键
        if !third && !changes.is_empty() {
            hdr = hdr.right(
                Button::new("changes-commit")
                    .label("提交并推送…")
                    .custom(ghost_variant(cx))
                    .with_size(Size::XSmall)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_commit_dlg(None, window, cx)
                    }))
                    .into_any_element(),
            );
        }
        // 阶段 17：**只给第三方**。自己的仓库要做这件事有正经工具链，
        // 在这里给一条「一键丢工作区」的路是帮倒忙
        if third && !changes.is_empty() {
            hdr = hdr.right(
                Button::new("changes-purge")
                    .label("清掉多出来的文件…")
                    .custom(ghost_variant(cx))
                    .with_size(Size::XSmall)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_purge_dlg(window, cx)
                    }))
                    .into_any_element(),
            );
        }
        v_flex()
            .flex_1()
            .min_h_0()
            // 页级留白**不在这里**——主区外层（`detail_page`）已经给了 `pt`/`pr`，
            // 这一层再套一圈 `p(GAP_LG)` 就是同一条边距写两次，四张页签会因此
            // 比头卡窄 24px（2026-10-08 用户定四边缩进时一并收掉）
            .child(
                // 白卡 + 卡内表头，与日志 / 说明 / 空间 / 历史同一套
                crate::widgets::card(t)
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(hdr.build(t))
                    .child(changes_list("changes-list", &changes, t, usize::MAX)),
            )
            .into_any_element()
    }
}

/// 一行的高度。悬停底色跟着行高走，写成一个值才不会两行文字高度不同而闪
const ROW_H: f32 = 26.0;

/// 状态词那一列的宽度
const MARK_W: f32 = 40.0;

/// 变更清单。超过 `max` 条时截断，并在末尾补一行"还有 N 个"——
/// 对话框里把四百个文件全列出来没法看，而用户要判断的是"这次要提交什么"，
/// 不是逐个核对
///
/// `id` 由调用方给：对话框与抽屉页签同时在场时两个滚动容器不能撞 id
pub(crate) fn changes_list(
    id: &'static str,
    changes: &[ChangeEntry],
    t: &Theme,
    max: usize,
) -> impl IntoElement {
    if changes.is_empty() {
        return v_flex()
            .w_full()
            .items_center()
            .gap(px(GAP_TIGHT))
            .py(px(GAP_XL))
            .child(
                div()
                    .text_size(px(fs_body()))
                    .text_color(t.colors.success)
                    .child("工作区干净，没有未提交的改动"),
            )
            .into_any_element();
    }

    let mut sorted: Vec<&ChangeEntry> = changes.iter().collect();
    sorted.sort_by(|a, b| {
        b.staged
            .cmp(&a.staged)
            .then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase()))
    });
    let shown: Vec<&ChangeEntry> = sorted.iter().take(max).copied().collect();
    let rest = sorted.len().saturating_sub(shown.len());

    v_flex()
        .w_full()
        .id(id)
        .children(shown.iter().map(|c| change_row(c, t)))
        .when(rest > 0, |list| {
            list.child(
                div()
                    .w_full()
                    .h(px(ROW_H))
                    .px(px(GAP))
                    .flex_shrink_0()
                    .items_center()
                    .text_size(px(fs_meta()))
                    .text_color(tier4(t))
                    .child(format!("还有 {rest} 个文件…")),
            )
        })
        .into_any_element()
}

fn change_row(c: &ChangeEntry, t: &Theme) -> Div {
    h_flex()
        .h(px(ROW_H))
        .px(px(GAP))
        .flex_shrink_0()
        .items_center()
        .gap(px(GAP_TIGHT))
        .child(status_mark(c, t))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(fs_sec()))
                .font_family(t.mono_font_family.clone())
                .text_color(body(t))
                .truncate()
                .child(c.path.clone()),
        )
}

/// 左边那一列：状态字形 + 状态词。
///
/// **不用 git 的两字母代码。** `M` / `A` / `??` 只有天天敲 git 的人认得，
/// 而这一列要回答的是"这次提交带不带它"——那是暂存 / 未暂存 / 未跟踪三件事，
/// 写成人话才不用去查 git 文档
fn status_mark(c: &ChangeEntry, t: &Theme) -> Div {
    let (icon, ink, word) = if c.untracked {
        (AssetIcon::FilePlus, tier4(t), "未跟踪")
    } else if c.staged {
        let word = match c.status.chars().next().unwrap_or('M') {
            'A' => "已暂存·新增",
            'D' => "已暂存·删除",
            'R' => "已暂存·改名",
            _ => "已暂存·修改",
        };
        (AssetIcon::Check, t.colors.success, word)
    } else {
        let word = match c.status.chars().last().unwrap_or('M') {
            'D' => "未暂存·删除",
            _ => "未暂存",
        };
        (AssetIcon::CircleDot, orange(t), word)
    };
    h_flex()
        .w(px(MARK_W))
        .flex_shrink_0()
        .items_center()
        .gap(px(GAP_TIGHT))
        .child(div().text_color(ink).child(icon_sm(icon)))
        .child(
            div()
                .text_size(px(fs_meta()))
                .text_color(tier4(t))
                .child(word.to_string()),
        )
}
