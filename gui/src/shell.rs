//! 应用壳：导航轨 / 仓库侧栏（我的·第三方分组）/ 主区仓库头。
//!
//! 静态骨架版：数据先硬编码对齐设计稿（Screen 1/3），后续接入 qingbird-core 实时状态。

use gpui::{div, px, Div, FontWeight, Hsla, IntoElement, ParentElement, Styled, Window};
use gpui::{Context, Render};
use guise::prelude::*;

// 设计规范板 token（与 theme.json 同源；theme.json 管全局语义，这里只放少量语义色附加态）。
// color!() 非 const，用函数返回
fn warn_fg() -> Hsla { color!("#DD5B00") }
fn warn_bg() -> Hsla { color!("#FDEFE4") }
fn slate_fg() -> Hsla { color!("#5B7A99") }
fn slate_bg() -> Hsla { color!("#EDF1F5") }
fn ok_dot() -> Hsla { color!("#2E9E44") }
fn ink_name() -> Hsla { color!("#37352F") }
fn label_dim() -> Hsla { color!("#A4A097") }

pub struct Shell;

impl Shell {
    pub fn new() -> Self {
        Shell
    }
}

/// 竖分隔线（列与列之间）
fn vline(t: &Theme) -> impl IntoElement {
    div().w(px(1.0)).h_full().bg(t.border().hsla())
}

/// 导航轨上的圆形图标钮（selected 时画浅灰药丸底）
fn nav_item(icon: IconName, selected: bool, t: &Theme) -> Div {
    let base = div()
        .w(px(36.0))
        .h(px(36.0))
        .rounded(px(8.0))
        .flex()
        .items_center()
        .justify_center();
    if selected {
        base.bg(t.surface_hover().hsla())
            .child(Icon::new(icon).size(Size::Lg))
    } else {
        base.child(
            Icon::new(icon)
                .size(Size::Lg)
                .color(ColorName::Dark),
        )
    }
}

fn badge(text: &str, fg: Hsla, bg: Hsla) -> Div {
    div()
        .h(px(18.0))
        .px(px(6.0))
        .rounded(px(9.0))
        .bg(bg)
        .flex()
        .items_center()
        .child(
            div()
                .text_size(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .child(text.to_string()),
        )
}

fn dot(color: Hsla) -> Div {
    div().w(px(7.0)).h(px(7.0)).rounded_full().bg(color)
}

/// 侧栏里的一行仓库
fn repo_row(name: &str, status: Option<Hsla>, badge_text: Option<(&str, Hsla, Hsla)>) -> Div {
    let row = div()
        .w_full()
        .h(px(38.0))
        .px(px(10.0))
        .rounded(px(8.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(match status {
            Some(c) => dot(c).into_any_element(),
            None => div().into_any_element(),
        })
        .child(
            div()
                .text_size(px(13.0))
                .text_color(ink_name())
                .child(name.to_string()),
        )
        .child(div().flex_1());
    match badge_text {
        Some((text, fg, bg)) => row.child(badge(text, fg, bg)),
        None => row,
    }
}

/// 分组小标题（「我的仓库 · 2」）
fn group_label(text: &str) -> Div {
    div()
        .px(px(2.0))
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(label_dim())
        .child(text.to_string())
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.global::<Theme>();

        // ── 导航轨 ─────────────────────────────────────────────
        let nav = div()
            .w(px(56.0))
            .h_full()
            .bg(t.surface().hsla())
            .flex()
            .flex_col()
            .items_center()
            .pt(px(10.0))
            .pb(px(10.0))
            .gap(px(6.0))
            .child(nav_item(IconName::FolderGit2, true, &t))
            .child(nav_item(IconName::LayoutGrid, false, &t))
            .child(div().flex_1())
            .child(nav_item(IconName::Plus, false, &t))
            .child(nav_item(IconName::Radar, false, &t))
            .child(nav_item(IconName::Settings, false, &t));

        // ── 仓库侧栏 ───────────────────────────────────────────
        let search = div()
            .w_full()
            .h(px(30.0))
            .px(px(9.0))
            .rounded(px(6.0))
            .bg(t.surface_hover().hsla())
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .child(Icon::new(IconName::Search).color(ColorName::Dark))
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(t.dimmed().hsla())
                    .child("筛选名称或分支"),
            );

        let sidebar = div()
            .w(px(264.0))
            .h_full()
            .bg(t.surface().hsla())
            .flex()
            .flex_col()
            .px(px(12.0))
            .py(px(12.0))
            .gap(px(8.0))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(px(2.0))
                    .child(group_label("仓库"))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(label_dim())
                            .child("5"),
                    ),
            )
            .child(search)
            .child(group_label("我的仓库 · 2"))
            .child(repo_row(
                "qingbird-git",
                Some(slate_fg()),
                Some(("未提交", slate_fg(), slate_bg())),
            ))
            .child(repo_row("qingbird-md", Some(ok_dot()), None))
            .child(group_label("第三方仓库 · 3"))
            .child(
                repo_row(
                    "element-plus",
                    Some(warn_fg()),
                    Some(("可更新", warn_fg(), warn_bg())),
                )
                .bg(t.surface_hover().hsla()),
            )
            .child(repo_row("vue", Some(ok_dot()), None))
            .child(repo_row("tauri", Some(ok_dot()), None));

        // ── 主区：仓库头 + 内容占位 ────────────────────────────
        let head_left = div()
            .flex()
            .flex_col()
            .gap(px(5.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_size(px(18.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text().hsla())
                            .child("qingbird-git"),
                    )
                    .child(
                        div()
                            .h(px(22.0))
                            .px(px(8.0))
                            .gap(px(4.0))
                            .rounded(px(6.0))
                            .bg(t.surface_hover().hsla())
                            .flex()
                            .items_center()
                            .child(Icon::new(IconName::GitBranch).color(ColorName::Dark))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(t.dimmed().hsla())
                                    .child("main"),
                            ),
                    )
                    .child(badge("未提交 2 文件", slate_fg(), slate_bg())),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(t.dimmed().hsla())
                    .child("我的仓库 · 本地路径 D:\\AIwork\\qingbird-git · 上次推送 2 小时前"),
            );

        let head_right = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .child(
                Button::new("detect", "检测更新").variant(Variant::Outline),
            )
            .child(
                Button::new("pull", "拉取更新")
                    .variant(Variant::Filled)
                    .color(color!("#5B7A99")),
            );

        let main_area = div()
            .flex_1()
            .h_full()
            .bg(t.body().hsla())
            .flex()
            .flex_col()
            .p(px(24.0))
            .gap(px(14.0))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(head_left)
                    .child(head_right),
            )
            .child(
                // 内容占位：真实内容按设计稿 Screen 3 逐屏填充
                div()
                    .flex_1()
                    .w_full()
                    .rounded(px(12.0))
                    .bg(t.surface().hsla())
                    .border_1()
                    .border_color(t.border().hsla())
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(t.dimmed().hsla())
                            .child("主区内容 · 按设计稿逐屏实现"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(t.dimmed().hsla())
                            .child("Screen 3：常用命令 / 磁盘占用 / Git 附带卡"),
                    ),
            );

        // ── 三列拼装 ───────────────────────────────────────────
        div()
            .size_full()
            .flex()
            .bg(t.body().hsla())
            .text_color(t.text().hsla())
            .child(nav)
            .child(vline(&t))
            .child(sidebar)
            .child(vline(&t))
            .child(main_area)
    }
}
