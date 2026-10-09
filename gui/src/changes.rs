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
use gpui_kit::component::scroll::ScrollableElement as _;
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
            // **四页的左右留白在 2026-10-09 第 1 条之后落在卡里面**（`TAB_PAD` 那一档），
            // 外层这一列只给 `pt(GAP)` 那一条上气口——从前这里注释说"外层已给 pt/pr"，
            // 那是旧的落法，pr 早就不在外层了（注释留在原地会让人以为不用再补）
            .child(
                // 白卡 + 卡内表头，与日志 / 说明 / 空间 / 历史同一套
                crate::widgets::card(t)
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    // 表头左缘与下面的文件行同一档（36），推导见 `widgets::tab_head`
                    .child(crate::widgets::tab_head(hdr, t))
                    .child(changes_list("changes-list", &changes, t, usize::MAX)),
            )
            .into_any_element()
    }
}

/// 一行的高度。悬停底色跟着行高走，写成一个值才不会两行文字高度不同而闪
const ROW_H: f32 = 26.0;

/// 状态词那一列的宽。**由字号派生，不许写死**（2026-10-09 用户第 2 条）。
///
/// 原来这里是一颗 `const MARK_W = 40.0`，而那一列要放的**最长**那句是
/// 「已暂存·新增」= 6 个全角字。40 给完 14 的图标和 4 的气口只剩 22，
/// 六个字要 66 ⇒ **状态词整条画到右边的文件路径上面去了**（帧上就是
/// 「二级小字压在一级字上」）。这一列是 `flex_shrink_0` 的定宽列，
/// 里面的文字既没有 `min_w_0` 也没有 `truncate`，**溢出是静默的**：
/// `cargo check` 零错、单测全绿、抓帧看着也只是"这列有点糊"。
///
/// 取「最长那句的字数 × 字号」而不是目测一个宽档：中文墨迹 ≈ 1 个字号宽，
/// 字号是三档可切的（`fs_meta` 11 / 12 / 14），写死的数在中档就开始吃右边。
/// 与 `row_h()` / `sidebar_min_w()` 同一条纪律——**外框是内容的函数，不是内容的猜测**。
fn mark_w(fs: f32) -> f32 {
    ICON_SM + GAP_TIGHT + STATUS_WORD_CHARS * fs
}
/// 状态词最长那一句的字数（「已暂存·新增」）
const STATUS_WORD_CHARS: f32 = 6.0;

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

    // ⚠️ **探出那一格挂在外面这颗裸 Div 上，不挂在滚动 Div 自己身上**
    // （2026-10-09 实测：`mr(-5)` 直接写在带 `overflow_y_scrollbar()` 的那一格上
    // **不生效**，条停在 1332..1337；换成"裸 Div 探出 + 滚动格在里面铺满"
    // 之后落到 1337..1342，与说明 / 历史 / 空间同一档。
    // 系数与三张对照表都在 `tokens::SCROLL_BLEED`）
    scroll_cell(
        div()
            .flex_1()
            .min_h_0()
            .child(
                v_flex()
                    .w_full()
                    .h_full()
                    // ⚠️ **这一格是「变更」清单唯一的出口**（2026-10-09 查第 1 条时
                    // 顺带查出来的功能缺陷）：原来这里只有 `.id(id)` 加一串行，
                    // **没有任何滚动容器**，而外面那张卡是 `overflow_hidden`
                    // ⇒ 文件一多，**第 N 行之后的改动用户永远够不着**
                    // （`cargo check` 零错、单测全绿、抓帧看着也只是"列了十几行"）。
                    // 用 `overflow_y_scrollbar()` 而不是光 `overflow_y_scroll`：
                    // 后者在这一层**不画条**（`settings.rs:791` 那颗早就为同一条改过，
                    // 注释写着「用户看得见"下面还有"，而不是滚不动也看不出为什么」）
                    .id(id)
                    .overflow_y_scrollbar()
                    // **左右留白与滑块落位与说明页同一档**（用户第 1 条）：
                    // 左 = 卡内距 16 + 这里 12 + 行自己带的 8 = **36**
                    .pl(px(GAP_LG))
                    // 右 = 卡内距 16 + 探出抵消后的 `TAB_PAD` 20；
                    // 这一格的行自己带 `px(GAP)`(8) ⇒ `pr = TAB_PAD_RIGHT − 8`
                    .pr(px(TAB_PAD_RIGHT - GAP))
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
                    }),
            ),
    )
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
        .w(px(mark_w(fs_meta())))
        .flex_shrink_0()
        .items_center()
        .gap(px(GAP_TIGHT))
        .child(div().text_color(ink).child(icon_sm(icon)))
        .child(
            div()
                // 兜底：列宽已经按最长那句算过，这两层是**第二道闸**——
                // 将来状态词再加长字（或换更长的说法）时这里会截断而不是盖到路径上。
                // 与卡头附注那一格同一道理：`h_flex` 的定宽列里不给 `min_w_0`，
                // 文字就往外溢，而溢出**不报错也不影响布局**
                .min_w_0()
                .truncate()
                .text_size(px(fs_meta()))
                .text_color(tier4(t))
                .child(word.to_string()),
        )
}
