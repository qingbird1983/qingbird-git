//! 「一键清理构建缓存」确认框。
//!
//! **这个框存在的理由只有一个：删 `target` 的代价不对称。** 删掉省 800 MB，
//! 代价是下一次 `cargo build` 从零重来几十分钟；而误点的代价是"我那几个小时"。
//! 逐项删已经挂在列表上了（用户可能只想删 `dist`），一键是给"我就是要全清"
//! 的人准备的快路——**快路更要先刹车**。
//!
//! 框里逐项列出删什么、放多少：只给一个「确定」而不给清单，等于让用户凭
//! 上一个界面的印象做不可逆的决定。清单按体积从大到小排，最贵的那项在
//! 最上面，用户扫一眼就知道自己在放弃什么。
//!
//! 执行走 core 的 `space::delete_cache_dirs`（逐项独立：一项被占用而删不掉，
//! 不连坐掉同一批里其它能删的）。**点确认就关框**——这一页没有"停在原地看
//! 输出"的必要，进度与结论由日志区和状态条承担，用户想看随时能翻上去。

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::{ActiveTheme, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    assets::IconName as AssetIcon, div, px, App, Context, Div, Entity, FontWeight, IntoElement,
    ParentElement, Styled, Window,
};
use qingbird_core::space;

use crate::state::*;
use crate::tokens::*;
use crate::widgets::{ghost_variant, meta_text};

/// 清单最多列几项。超出的并成一行「还有 N 个…」——
/// 确认框的作用是让用户知道**大概要放弃什么**，不是给一张完整对账单
const MAX_ITEMS: usize = 8;

impl Shell {
    /// 打开确认框。**体积从大到小排**，取自打开那一刻的 `report` 快照
    pub(crate) fn open_clean_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        let Some(rep) = self.space.report.as_ref() else {
            return;
        };
        let mut items: Vec<(String, u64)> = rep
            .dirs
            .iter()
            .filter(|d| d.is_cache && d.size > 0)
            .map(|d| (d.name.clone(), d.size))
            .collect();
        if items.is_empty() {
            return;
        }
        items.sort_by(|a, b| b.1.cmp(&a.1));

        self.clean_dlg = Some(CleanDialog { path, items });
        self.build_clean_dlg(window, cx);
    }

    /// 对话框本体（官方 Dialog）。Shell 只提供状态
    fn build_clean_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        let (path, items) = match self.clean_dlg.as_ref() {
            Some(d) => (d.path.clone(), d.items.clone()),
            None => return,
        };
        let total: u64 = items.iter().map(|(_, b)| *b).sum();
        let count = items.len();

        window.open_dialog(cx, move |dialog, _window, cx| {
            let t = cx.theme().clone();
            dialog
                .title("清理构建缓存")
                .content({
                    let t = t.clone();
                    // `content` 闭包是 `Fn` 不是 `FnOnce`，每帧重画一次——
                    // 所以 `items` 只能**借**，不能 move 出去
                    let items = items.clone();
                    move |c: DialogContent, _w, _cx: &mut App| {
                        c.child(clean_body(&t, &items, total))
                    }
                })
                .footer(clean_footer(&this, &path, count, ghost_variant(cx)))
        });
    }
}

/// 框正文：一行总量 + 逐项清单 + 一句后果提醒
fn clean_body(t: &Theme, items: &[(String, u64)], total: u64) -> Div {
    let head = items.len();
    let shown: Vec<&(String, u64)> = items.iter().take(MAX_ITEMS).collect();
    let rest = head.saturating_sub(shown.len());

    v_flex()
        .w_full()
        .gap(px(GAP))
        .child(
            h_flex()
                .w_full()
                .items_baseline()
                .gap(px(GAP_TIGHT))
                .child(
                    div()
                        .text_size(px(fs_num()))
                        .font_family(t.mono_font_family.clone())
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.colors.foreground)
                        .child(space::human_size(total)),
                )
                .child(meta_text(
                    if head == 1 {
                        "1 个目录将被删除".to_string()
                    } else {
                        format!("{head} 个目录将被删除")
                    },
                    t,
                ))
                .into_any_element(),
        )
        .child(
            v_flex()
                .w_full()
                .gap(px(GAP_TIGHT))
                .children(shown.into_iter().map(|(name, bytes)| {
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap(px(GAP_TIGHT))
                        .child(
                            div()
                                .w(px(14.0))
                                .flex_shrink_0()
                                .text_color(orange(t))
                                .child(icon_sm(AssetIcon::Folder)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(fs_sec()))
                                .font_family(t.mono_font_family.clone())
                                .text_color(body(t))
                                .child(name.clone()),
                        )
                        .child(meta_text(space::human_bytes(*bytes), t))
                }))
                .when_some((rest > 0).then_some(rest), |el, n| {
                    el.child(meta_text(format!("还有 {n} 个较小的目录…"), t))
                })
                .into_any_element(),
        )
        .child(
            div()
                .w_full()
                .rounded(px(R_MD))
                .border_1()
                .border_color(orange(t))
                .bg(orange_bg(t))
                .px(px(GAP))
                .py(px(GAP_TIGHT))
                .text_size(px(fs_meta()))
                .text_color(orange_text(t))
                .child(
                    "删掉的都是可以重新生成的构建产物与依赖缓存，源码不受影响。\
                     但下一次构建 / 启动要重新跑一遍——target 大的话会等很久。",
                ),
        )
}

/// 框底按钮：取消 + 确认删除
///
/// **确认键用 danger 而不是 primary**：这是一个不可逆、代价高的动作，
/// 放在主色按钮上等于在鼓励用户点它
fn clean_footer(this: &Entity<Shell>, path: &str, count: usize, gv: ButtonCustomVariant) -> Div {
    let cancel = this.clone();
    let go = this.clone();
    let p = path.to_string();
    h_flex()
        .w_full()
        .justify_end()
        .gap(px(GAP))
        .child(
            Button::new("clean-cancel")
                .label("取消")
                .custom(gv)
                .on_click(move |_, window, cx| {
                    cancel.update(cx, |shell, _| shell.clean_dlg = None);
                    window.close_dialog(cx);
                }),
        )
        .child(
            Button::new("clean-go")
                .label(format!("删除这 {count} 个目录"))
                .danger()
                .with_size(Size::Small)
                .on_click(move |_, window, cx| {
                    go.update(cx, |shell, cx| shell.clean_cache_all(&p, cx));
                    // 不等结果就关：这一页没有"停在原地看输出"的必要，
                    // 进度与结论都在日志区和状态条上
                    window.close_dialog(cx);
                }),
        )
}
