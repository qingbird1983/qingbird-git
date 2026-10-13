//! CLI 工具屏（阶段一：只读扫描展示）。
//!
//! 沿用仓库屏的布局语言（设计 §10.1，2026-10-13 用户拍板）：**左栏工具列表
//! → 右栏详情**，操作习惯零学习成本。
//!
//! ⚠️ **一点与仓库屏不同**：CLI 屏的左栏是它**自己的列表列**，不是仓库侧栏。
//! 它不读 `side_focus`、不驱动 `sidebar`、宽度也不走那条 resizable——
//! 两者互不干扰。所以这一屏的整个主区就是一个 `h_flex`：左列表 + 右详情。
//!
//! 图标（阶段一）：设计里的三级降策略（Provider 自报 / 从 exe 提图标 / 终字符块）
//! 中，**前两级依赖 `windows` crate，归阶段二**；阶段一对每个工具都画一个
//! 终端图标占位（等于第三级"终字符块"的近似）。

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::Button;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::{Disableable as _, Sizable as _, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, percentage, px, AnyElement, Context, Div, FontWeight, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement, Styled,
};
use qingbird_core::cli::{CliPackage, ProviderId};

use crate::state::Shell;
use crate::tokens::*;
use crate::widgets::*;

/// 「没有这个值」的记号。与总览表格里"没有这个数"那几列**同一个记号**
/// （同一含义两个记号，又是一处会漂的东西）
const NONE: &str = "–";

impl Shell {
    /// CLI 屏整屏：左栏工具列表 + 右栏详情。
    ///
    /// 外层 `h_flex` 自己带 `p(GAP)`——内距落在**行**上、不落在每个子列上，
    /// 这样两列的顶边由同一个数对齐（与仓库屏"侧栏 `mt(GAP)` / 主区 `pt(GAP)`"
    /// 是同一个意思，只是这里行是我自己的，就一次给在行上）
    pub(crate) fn cli_page(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        h_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .min_h_0()
            // ⚠️ **`items_stretch` 不能省**（AGENTS：h_flex 与 v_flex 交叉轴不对称）：
            // 默认 `items_center` 会让两列各拿内容高并被垂直居中，整屏上下各留一大块空
            .items_stretch()
            .bg(t.colors.background)
            .p(px(GAP))
            .gap(px(GAP_TIGHT))
            .child(self.cli_list(t, cx))
            .child(self.cli_detail(t, cx))
    }

    /// 左栏：工具列表（按 provider 分组，可折叠）+ 底部「重新扫描」。
    fn cli_list(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        // 每个 provider 有几个——没有的**不画组标题**（"npm · 0" 是噪音）
        let counts: Vec<usize> = ProviderId::all()
            .iter()
            .map(|id| self.cli.packages.iter().filter(|p| p.provider == *id).count())
            .collect();
        let total: usize = counts.iter().sum();

        // 头：标题 + 一句状态（扫描中 / 几个）
        let note = if self.cli.scanning {
            "扫描中…".to_string()
        } else {
            format!("{total} 个工具")
        };
        let mut head = CardHeader::new("CLI 工具").note(note, false);
        if !self.cli.errors.is_empty() {
            head = head.note(self.cli.errors.join(" · "), true);
        }

        // 身：分组 + 行。滚动容器要 `id`（`overflow_y_scroll` 只在 Stateful 上）
        let mut tree = v_flex()
            .id("cli-list-tree")
            .flex_1()
            .min_h_0()
            .gap(px(GAP_DENSE))
            .overflow_y_scroll();
        if total == 0 {
            tree = tree.child(
                empty_state(
                    "未发现 CLI 工具",
                    "装了 scoop / npm / pipx 里的工具后会出现在这里",
                    t,
                )
                .py(px(GAP_XL)),
            );
        } else {
            for (ix, id) in ProviderId::all().iter().enumerate() {
                if counts[ix] == 0 {
                    continue;
                }
                tree = tree.child(self.cli_group_header(*id, counts[ix], ix, t, cx));
                if self.cli.group_open[ix] {
                    for p in self.cli.packages.iter().filter(|p| p.provider == *id) {
                        tree = tree.child(self.cli_row(p, t, cx));
                    }
                }
            }
        }

        card_shell(
            v_flex()
                .w(px(CLI_LIST_W))
                .flex_none()
                .min_h_0()
                .gap(px(GAP_TIGHT)),
            t,
        )
        .child(head.build(t))
        .child(tree)
        .child(Separator::horizontal().color(edge(t)))
        .child(self.cli_list_footer(t, cx))
    }

    /// 左栏底部：一颗「重新扫描」。位置同仓库屏侧栏底部的动作键。
    fn cli_list_footer(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .py(px(GAP_TIGHT))
            .child(
                Button::new("cli-rescan")
                    .flex_1()
                    .icon(icon_sm(AssetIcon::RefreshCw).text_color(tier3(t)))
                    .label("重新扫描")
                    .with_size(Size::Small)
                    .tooltip("重新扫描本机的 CLI 工具")
                    // 扫描进行中禁点：免得连点堆起几条并发的 `npm ls`。
                    // ⚠️ 这里用 `disabled` 而**不是** `.loading(true)`——
                    // 后者会让按钮完全不响应指针（AGENTS 记过），语义也不对
                    // （loading 是"这个操作在跑"的指示器，而这里要表达的是"等它"）
                    .disabled(self.cli.scanning)
                    .on_click(cx.listener(|this, _, _, cx| this.scan_cli(cx))),
            )
    }

    /// 一个 provider 的分组标题：折叠箭头 + `名字 · 个数`，点一下折叠/展开。
    fn cli_group_header(
        &self,
        id: ProviderId,
        n: usize,
        ix: usize,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.cli.group_open[ix];
        h_flex()
            .id(format!("cli-group-{ix}"))
            .h(px(SIDE_ROW_H))
            .flex_shrink_0()
            .items_center()
            .gap(px(GAP))
            .px(px(SIDE_GUTTER))
            .text_color(tier4(t))
            .rounded(px(r_lg(t)))
            .hover(|s| s.bg(hover_face(t)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.cli.group_open[ix] = !this.cli.group_open[ix];
                cx.notify();
            }))
            .child(
                icon_sm(AssetIcon::ChevronRight)
                    .text_color(tier4(t))
                    .rotate(percentage(if open { 90.0 } else { 0.0 } / 360.0)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(fs_meta()))
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("{} · {n}", id.label())),
            )
            .into_any_element()
    }

    /// 一行工具：终端图标 + 名字 + （可更新时）一个小圆点。
    ///
    /// 自绘而行（不走官方 `SidebarMenuItem`）：那个件把行高写死 28px、
    /// 且渲染期读 `Shell` 会 panic（见 `sidebar.rs::repo_row` 与 `group_header`）。
    /// 这里与 `sidebar::repo_row` 是同一副形状的**第二处**，但**两屏各管各的**
    /// （CLI 屏不是仓库侧栏），不强行并成一颗——并了就得塞一堆"这一屏用不用"的开关。
    fn cli_row(&self, p: &CliPackage, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.cli.selected.as_deref() == Some(p.id.as_str());
        let id = p.id.clone();
        h_flex()
            .id(format!("cli-row-{}", p.id))
            .h(px(SIDE_ROW_H))
            .flex_shrink_0()
            .items_center()
            .gap(px(GAP))
            .px(px(SIDE_GUTTER))
            .rounded(px(r_lg(t)))
            .text_color(tier3(t))
            .when(selected, |s| s.bg(wash(t)))
            .hover(|s| s.bg(if selected { wash(t) } else { hover_face(t) }))
            .child(icon_sm(AssetIcon::Terminal).text_color(tier3(t)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(fs_sec()))
                    .text_color(if selected { body(t) } else { tier3(t) })
                    .font_weight(if selected {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .child(p.name.clone()),
            )
            // 可更新时行尾会有一个小圆点（`p.latest.is_some()`）——**阶段一不画**：
            // 那时 `latest` 恒为 None（不查更新），画一条永不触发的分支等于留一句
            // 会骗人的话。阶段三把 `latest` 接上时再加回（连它的尺寸档一起进 tokens）
            .on_click(cx.listener(move |this, _, _, cx| {
                this.cli.selected = Some(id.clone());
                cx.notify();
            }))
            .into_any_element()
    }

    /// 右栏：选中工具的只读详情。没选中就给一句提示。
    fn cli_detail(&self, t: &Theme, _cx: &mut Context<Self>) -> Div {
        let mut col = v_flex().flex_1().min_w_0().h_full().min_h_0().gap(px(GAP_TIGHT));
        match self
            .cli
            .packages
            .iter()
            .find(|p| Some(p.id.as_str()) == self.cli.selected.as_deref())
        {
            Some(p) => col = col.child(self.cli_detail_card(p, t)),
            None => {
                col = col.child(
                    card(t).child(
                        empty_state("还没有选中工具", "左边点一个工具就能看它的信息", t)
                            .py(px(GAP_XL)),
                    ),
                )
            }
        }
        col
    }

    /// 详情卡：名称 / 版本 / 来源 / 命令 / 安装目录。
    ///
    /// **阶段一只读**：升级 / 卸载按钮属阶段三，这里不画（v4 铁律：
    /// 不演拿不到的效果——一颗点了没反应的键比没有更糟）。
    fn cli_detail_card(&self, p: &CliPackage, t: &Theme) -> Div {
        let version = p.version.clone().unwrap_or_else(|| NONE.into());
        let commands = if p.executables.is_empty() {
            NONE.into()
        } else {
            p.executables.join("、")
        };
        let install = p.install_path.clone().unwrap_or_else(|| NONE.into());
        card(t)
            .child(
                CardHeader::new(p.name.clone())
                    .leading(icon_sm(AssetIcon::Terminal).text_color(tier3(t)).into_any_element())
                    // 来源（哪个包管理器）作为标题右侧的附注
                    .note(p.provider.label(), false)
                    .build(t),
            )
            .child(kv("版本", &version, t))
            .child(kv("来源", p.provider.label(), t))
            .child(kv("命令", &commands, t))
            .child(kv("安装目录", &install, t))
    }
}

/// 详情卡里的一行「标签 · 值」。
///
/// 标签列宽走 `FORM_LABEL_W` 那一个档（不在这儿再写一个数）。
/// ⚠️ 值那一列**左对齐**（不做 `justify_end`）：它装的是路径与命令名这类要读的
/// 长文本，推到最右反而看不出标签与值的从属关系
fn kv(label: &str, value: &str, t: &Theme) -> Div {
    h_flex()
        .w_full()
        .items_start()
        .gap(px(GAP_LG))
        .child(
            div()
                .w(px(FORM_LABEL_W))
                .flex_shrink_0()
                .text_size(px(fs_body()))
                .text_color(tier3(t))
                .child(label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(fs_body()))
                .text_color(body(t))
                .child(value.to_string()),
        )
}
