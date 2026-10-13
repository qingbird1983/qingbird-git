//! CLI 工具屏（阶段一：只读扫描展示）。
//!
//! 沿用仓库屏的布局语言（设计 §10.1，2026-10-13 用户拍板）：**左栏工具列表
//! → 右栏详情**，操作习惯零学习成本。
//!
//! ⚠️ **内外距与左栏宽度一律照仓库屏那两栏，不自己定**（2026-10-13 实机返工）：
//! - 左栏**不是这一屏自己拼的定宽 `div`**——它是壳层 `body-split` 的 **0 号面板**，
//!   与仓库侧栏同一个位置、同一套机制（宽度来源同一个 `sidebar_w`、同一条
//!   `SIDEBAR_W_RANGE`、朝向气口同一个 `side_shell` 的 `mr(GAP_TIGHT)`）。
//!   自己拼一个的结果量出来是**宽 4px、右移 1px**，所以这里只提供**内容**
//!   （[`Shell::cli_list`]），外壳与宽度全归壳层。折叠态由 `cli_collapsed` 管
//!   （与 `sidebar_collapsed` 各一份，见 `state.rs`）。
//! - 左栏内容 = [`side_shell`]（仓库侧栏同一颗：`card` + `p(GAP)`/`pt(GAP_LG)`/
//!   `pb(GAP)`/`mt(GAP)` + 朝向气口 `mr(GAP_TIGHT)`）。树照 `sidebar()` 用负 `mx`
//!   铺到卡边、行用 `SIDE_GUTTER` 缩进——**两个数都从 `tokens` 取**，
//!   一个都不许在这儿写。
//! - 右栏照 `detail_page`：`pt(GAP)` + `gap(GAP_TIGHT)`；两栏顶边靠
//!   `side_shell.mt(GAP)` ↔ 这一列的 `pt(GAP)` **同源**对齐。
//!
//! 右栏（预留的 AI 助理栏）也在：这一屏与仓库屏同走 `insp-split`
//! （2026-10-13 用户令「100% 要」），顶栏那颗开关与仓库屏同一颗。
//!
//! 图标（阶段二落地）：三级降里【② 从 exe 提图标】+【③ 终字符块】两级都在——
//! 提到了就画那张 PNG（`cli.icons`，由 `ops::scan_cli` 在后台逐颗填），
//! 没提到就画终字符块。**① Provider 自报**那一级没接：三个 provider 都没有
//! 图标字段，写它就是一个永不触发的分支（见 `crates/core/src/cli/icon.rs` 模块头）。
//!
//! ⛔ **提图标那一路不许挡住首屏**：磁盘缓存由 `ops::scan_cli` 在**进屏那一刻**
//! 一次 `read_dir` 灌进来（只管已有图的那几颗），真正的提取在后台逐颗跑、
//! 提完一颗 notify 一次。所以形成这一屏的两条路是「先画块 / 已经有图先画图」→
//! 「提到图了换上去」，**没有任何一步等磁盘或 GDI**。

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::Button;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::{Disableable as _, Sizable as _, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, img, percentage, px, AnyElement, Context, Div, FontWeight, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement, Styled,
};
use qingbird_core::cli::{CliPackage, ProviderId};

use crate::sidebar::{side_shell, Facing};
use crate::state::Shell;
use crate::tokens::*;
use crate::widgets::*;

/// 「没有这个值」的记号。与总览表格里"没有这个数"那几列**同一个记号**
/// （同一含义两个记号，又是一处会漂的东西）
const NONE: &str = "–";

impl Shell {
    /// 左栏**内容**：工具列表（按 provider 分组，可折叠）+ 底部「重新扫描」。
    ///
    /// ⚠️ **宽度与外壳的位置不在这里**：它是 `body-split` 的 0 号面板（见模块头），
    /// 与仓库侧栏共用同一套几何。所以这里只画 `side_shell` 里的三段。
    ///
    /// 结构照 `sidebar()`：`side_shell` → [树] → 分隔线 → 脚。
    /// **没有标题行**（仓库侧栏也没有——分组标题已经说出这一列是什么）。
    pub(crate) fn cli_list(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        // 每个 provider 有几个——没有的**不画组标题**（"npm · 0" 是噪音）
        let counts: Vec<usize> = ProviderId::all()
            .iter()
            .map(|id| self.cli.packages.iter().filter(|p| p.provider == *id).count())
            .collect();
        let total: usize = counts.iter().sum();

        // 树：照 `sidebar()` 那三条（负 `mx` 铺到卡边 + `SIDE_GUTTER` 缩进 + 气口）。
        // 行没有左侧品牌竖条，但那两条缩进数照抄——**行与仓库行左对齐靠的就是它们**
        let mut tree = v_flex()
            .id("cli-list-tree")
            .flex_1()
            .min_h_0()
            .mx(px(-GAP))
            .px(px(SIDE_GUTTER))
            .py(px(GAP_TIGHT))
            .gap(px(GAP_DENSE))
            .overflow_y_scroll();

        // 某台 provider 失败的一句话。**一行提示，不是"整屏没扫到"**——
        // 别的 provider 的结果照常出
        if !self.cli.errors.is_empty() {
            tree = tree.child(
                div()
                    .px(px(SIDE_GUTTER))
                    .pb(px(GAP_DENSE))
                    .text_size(px(fs_meta()))
                    .text_color(orange_text(t))
                    .child(self.cli.errors.join(" · ")),
            );
        }

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

        side_shell(t, Facing::Left)
            .child(tree)
            .child(Separator::horizontal().color(edge(t)))
            .child(self.cli_list_footer(t, cx))
    }

    /// 左栏底部：「重新扫描」。位置与内距照 `sidebar_footer`（`px/py(GAP_LG)`）。
    fn cli_list_footer(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .px(px(GAP_LG))
            .py(px(GAP_LG))
            .gap(px(GAP))
            .child(
                Button::new("cli-rescan")
                    .flex_1()
                    .icon(icon_sm(AssetIcon::RefreshCw).text_color(tier3(t)))
                    .label(if self.cli.scanning {
                        "扫描中…"
                    } else {
                        "重新扫描"
                    })
                    .with_size(Size::Small)
                    .tooltip("重新扫描本机的 CLI 工具")
                    // 扫描进行中禁点：免得连点堆起几条并发的 `npm ls`。
                    // ⚠️ 用 `disabled` 而**不是** `.loading(true)`——后者会让按钮
                    // 完全不响应指针（AGENTS 记过），语义也不对（loading 答的是
                    // "这个操作在跑"，而这里要表达的是"等它"）
                    .disabled(self.cli.scanning)
                    .on_click(cx.listener(|this, _, _, cx| this.scan_cli(cx))),
            )
    }

    /// 一个 provider 的分组标题：折叠箭头 + `名字 · 个数`，点一下折叠/展开。
    /// 几何与 [`cli_row`](Self::cli_row) 同一套（`SIDE_ROW_H` + `px(SIDE_GUTTER)`）。
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
            .rounded(px(r_lg(t)))
            .text_color(tier4(t))
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

    /// 一行工具：终端图标 + 名字。
    ///
    /// 自绘而行（不走官方 `SidebarMenuItem`）：那个件把行高写死 28px、且渲染期
    /// 读 `Shell` 会 panic。几何与 `sidebar::repo_row` 的**内层脸**同一套
    /// （`SIDE_ROW_H` + `px(SIDE_GUTTER)` + `r_lg` + 选中 `wash`）。
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
            .child(self.tool_icon(p, TOOL_ICON, t))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(fs_sec()))
                    .text_color(if selected { body(t) } else { tier3(t) })
                    .font_weight(if selected {
                        FontWeight::BOLD
                    } else {
                        FontWeight::MEDIUM
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

    /// 这一颗工具的工具图标：提到了就画那张 PNG，没提到就画终字符块。
    ///
    /// `size` 只走 `TOOL_ICON`（左栏行）/ `TOOL_ICON_DETAIL`（右栏详情头部）两档——
    /// **同一个包在两处是同一张图、两个尺寸**，不是两张图（设计 §10.1「同源复用」）。
    ///
    /// 图交给 gpui 自己读：`img(PathBuf)` → `Resource::Path` → `fs::read` → 解码
    /// （`elements/img.rs:634`），不经我们的资产源。**也没有"读到旧的"这一档**：
    /// 文件名里带着那颗 exe 的版本指纹（mtime + 大小），换版就是换名字。
    fn tool_icon(&self, p: &CliPackage, size: f32, t: &Theme) -> AnyElement {
        match self.cli.icons.get(&p.id) {
            Some(path) => img(path.clone())
                .size(px(size))
                .flex_shrink_0()
                .into_any_element(),
            None => monogram_block(&p.name, size, t),
        }
    }

    /// 右栏**主区**：选中工具的只读详情。没选中就给一句提示。
    ///
    /// ⚠️ **它是 `body-split` 的 1 号面板（主区）**，不是这一屏自己拼的一行——
    /// 与仓库页的 `detail_page` 占同一个位置、走同一条 `keep_main_flexible`。
    ///
    /// 外形照 `detail_page`：`flex_1` + `pt(GAP)` + `gap(GAP_TIGHT)`，
    /// **底部不给外边距**（与左栏一起贴到状态条上沿，用户 2026-10-08 定）
    pub(crate) fn cli_detail(&self, t: &Theme, _cx: &mut Context<Self>) -> Div {
        let mut col = v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .min_h_0()
            .pt(px(GAP))
            .gap(px(GAP_TIGHT));
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
                    .leading(self.tool_icon(p, TOOL_ICON_DETAIL, t))
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

/// 三级降的第 ③ 级：**终字符块**（设计 §10.1）——工具名前两个字符画一块。
///
/// 底走 `wash`（`tokens` 里那一条的注释写的就是"选中面、**图标底块**、信息徽章底"，
/// 即 v4 §5.3 靠底块面积撑重量的手法）、圆角走 `R_MD`（那一档的注释是
/// "键帽、图标小底"）。
///
/// ⚠️ **它是同一个位置上的另一种画法，不是另一个尺寸**：宽高都由调用方给
/// （与 `img()` 那一支同吃 `TOOL_ICON` / `TOOL_ICON_DETAIL`），两档之间只差缩放的量。
///
/// 字色给 `tier3`（中性档）而不是 `brand`：它表达的是"这颗工具没有自带图标"，
/// 不是一个状态、更不是行动项——**中性态不许穿行动色**（阶段 16 那条）。
pub(crate) fn monogram_block(name: &str, size: f32, t: &Theme) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .flex_shrink_0()
        .rounded(px(R_MD))
        .bg(wash(t))
        .flex()
        .items_center()
        .justify_center()
        // 行盒 = 块高：11px 的字放进 16px 的块里，不设行高会被默认那档顶出格
        .line_height(px(size))
        .text_size(px(fs_meta()))
        .text_color(tier3(t))
        .child(monogram(name))
        .into_any_element()
}

/// 终字符块里那两个字符。
///
/// 取**包名最后一段**的前两个字符：scoped 包名（`@larksuite/cli`）整串的头两个
/// 字符是 `@l`，画出来谁都不认识；末段 `cli` → `cl` 才对得上用户嘴里那个名字。
///
/// 取不到 ASCII 字母数字就给 `?`——**不硬塞汉字**：16px 的块里一个汉字约 11px 宽，
/// 两个字就被切一半，而那正是本项目最恨的"看着像坏了"（查不出、只能靠帧发现）。
///
/// 抽成纯函数是为了能单测：它是**这一屏唯一一处把名字翻译成别的东西**的地方，
/// 猜错的表现只是"块里两个字符不对"，抓帧也未必看得清。
pub(crate) fn monogram(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let chars: String = last
        .chars()
        .skip_while(|c| !c.is_ascii_alphanumeric())
        .take_while(|c| c.is_ascii_alphanumeric())
        .take(2)
        .collect();
    if chars.is_empty() {
        "?".to_string()
    } else {
        chars
    }
}

#[cfg(test)]
mod tests {
    use super::monogram;

    /// 终字符块取哪两个字符：**scoped 包名要取最后一段**
    /// （`@larksuite/cli` 上的 `@l` 是错的，`cl` 才对）
    #[test]
    fn monogram_takes_the_last_name_segment() {
        assert_eq!(monogram("ripgrep"), "ri");
        assert_eq!(monogram("rg"), "rg");
        assert_eq!(monogram("@larksuite/cli"), "cl");
        assert_eq!(monogram("@mimo-ai/cli"), "cl");
        assert_eq!(monogram("feishu-cli"), "fe");
        // 前导非字母（`@`、`_`、数字前的一串符号）要跳过去再取
        assert_eq!(monogram("@wenyan-md/core"), "co");
        assert_eq!(monogram("__x-tool"), "x");
    }

    /// 纯中文 / 空名字：退成 `?`，**不把汉字塞进 16px 的块里**（会被切一半）
    #[test]
    fn monogram_falls_back_for_names_without_ascii() {
        assert_eq!(monogram("青鸟工具"), "?");
        assert_eq!(monogram(""), "?");
        assert_eq!(monogram("@scope/中文"), "?");
    }
}
