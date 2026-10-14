//! CLI 工具屏（阶段一只读扫描 → 阶段三带上管理动作）。
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
//!
//! 管理动作（阶段三）：详情卡卡底「升级 / 卸载…」、共享缓存汇总每颗一条
//! 「清理」+ 整批「全部清理…」、右栏底部操作日志（收起态）。三者的执行编排在
//! `ops.rs` 的 CLI 块，确认框在 `dialogs/cli.rs`——**这一文件只画键与状态**，
//! 风险判据 / 计划装配 / 执行全在 core（同一属性两个出处是本项目最贵的错）。

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::separator::Separator;
use gpui_kit::component::{Disableable as _, Sizable as _, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, img, percentage, px, AnyElement, Context, Div, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled,
};
use qingbird_core::cli::{CliPackage, PackageDir, ProviderId};
use qingbird_core::space::human_bytes;

use crate::log::{log_state_line, output_stream, should_follow_bottom};
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
                    "装了 winget / scoop / npm / pipx / cargo 里的工具后会出现在这里",
                    t,
                )
                .py(px(GAP_XL)),
            );
        } else {
            for (ix, id) in ProviderId::all().iter().enumerate() {
                if counts[ix] == 0 {
                    continue;
                }
                // 组内有可更新项时，组标题行尾多点一颗（设计 §10.1「组内有可更新项时
                // 行尾点数」）——折叠着也知道这一组里有东西该升
                let updatable = self
                    .cli
                    .packages
                    .iter()
                    .any(|p| p.provider == *id && p.latest.is_some());
                tree = tree.child(self.cli_group_header(*id, counts[ix], ix, updatable, t, cx));
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
    ///
    /// `updatable` = 组内有包可更新：行尾一颗橙点（设计 §10.1）。**不报个数**——
    /// 组标题那一行已经有一个计数徽章了，再并一个数就是两套计数
    fn cli_group_header(
        &self,
        id: ProviderId,
        n: usize,
        ix: usize,
        updatable: bool,
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
            .when(updatable, |row| row.child(update_dot(t)))
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
            // 可更新时行尾一颗小圆点（`p.latest.is_some()`，设计 §10.1「可更新工具行
            // 右侧小圆点徽章」）。**阶段三接上了**：`latest` 由 winget 的清单免费带回、
            // npm 的 `outdated` 单独查（`ops::check_cli_updates`），其余三家查不了 ⇒
            // 它们的 `latest` 恒 `None`，点不画——不画骗人的点
            .when(p.latest.is_some(), |row| row.child(update_dot(t)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.cli.selected = Some(id.clone());
                // 刚选中的那颗，它的数据目录可能还没量过体积（惰性，设计 §4）——
                // 点上就把它那几条补量（已经量过的不会重来）
                this.spawn_dir_sizes(cx, false);
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
    /// 外形照 `detail_page`：`flex_1` + `pt(GAP)`，**底部不给外边距**
    /// （与左栏一起贴到状态条上沿，用户 2026-10-08 定）。
    ///
    /// 块间距取 `GAP_LG`（"两个不同东西"那一档）而不是 `GAP_TIGHT`：这一列装着
    /// 详情卡 + 共享缓存汇总 + 操作日志三块，4px 让白卡与灰盒看着像连体
    pub(crate) fn cli_detail(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let mut col = v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .min_h_0()
            .pt(px(GAP))
            .gap(px(GAP_LG));
        match self
            .cli
            .packages
            .iter()
            .find(|p| Some(p.id.as_str()) == self.cli.selected.as_deref())
        {
            Some(p) => col = col.child(self.cli_detail_card(p, t, cx)),
            None => {
                col = col.child(
                    card(t).child(
                        empty_state("还没有选中工具", "左边点一个工具就能看它的信息", t)
                            .py(px(GAP_XL)),
                    ),
                )
            }
        }
        // 共享缓存汇总：**紧跟在详情卡下面**。没有共享缓存就整块不画。
        //
        // ⚠️ 原来这里在它前面垫了一个 `div().flex_1()`（"把汇总条压到这一列的底部"），
        // 2026-10-13 用户指着帧上那块空白问"是留给更新卸载的吗"——**它不是预留，
        // 是那个垫子撑出来的洞**。设计 §9 只说共享缓存上右栏那条汇总，
        // 没说中间要空一大截；阶段三的升级/卸载键挂在**详情卡自己身上**
        // （卡底），操作日志在这一列尾巴，都不占这块空。
        if let Some(box_) = self.shared_box(t, cx) {
            col = col.child(box_);
        }
        // 操作日志：这一列的尾巴。**默认收起**（设计 §10.1「收起态」）——
        // 一屏的主角是工具与目录，日志是跑完才看的东西
        col.child(self.cli_log_box(t, cx))
    }

    /// 详情卡：名称 / 版本 / 来源 / 桌面关联 / 命令 / 安装目录 / 数据目录，
    /// **卡底两颗管理键**（升级 / 卸载…，设计 §10.1 ASCII 最后那行）。
    ///
    /// 升级键**只在真有新版时画**：`latest` 为 `None` 是"已最新**或未查询**"
    /// （模型注释），画一颗升不了的键是骗人的键（v4 铁律）。
    /// 卸不了的那种（provider 给不出命令）由 `run_cli_uninstall` 收成一句
    /// 通知——键照画，因为五个 provider 都有卸载命令，blocked 只可能发生在
    /// id 认不出的坏数据上
    fn cli_detail_card(&self, p: &CliPackage, t: &Theme, cx: &mut Context<Self>) -> Div {
        let version = p.version.clone().unwrap_or_else(|| NONE.into());
        let commands = if p.executables.is_empty() {
            NONE.into()
        } else {
            p.executables.join("、")
        };
        let install = p.install_path.clone().unwrap_or_else(|| NONE.into());
        // 有新版时头行右侧那颗「可更新 x.y.z」徽章（设计 §10.1 ASCII 第一行）。
        // 走全站那套 warn-wash 胶囊——「可更新」在侧栏 / 总览也是这一档，
        // 同一语义同色
        let mut head = CardHeader::new(p.name.clone())
            .leading(self.tool_icon(p, TOOL_ICON_DETAIL, t))
            // 来源（哪个包管理器）作为标题右侧的附注
            .note(p.provider.label(), false);
        if let Some(v) = &p.latest {
            head = head.right(
                pill(&format!("可更新 {v}"), orange_text(t), wash_bg(WARN_WASH, t)).into_any_element(),
            );
        }
        let busy = self.cli_busy_any();
        let mine = self.cli.busy.contains(&p.id);
        // 升级键还多一道判据：**provider 此刻给得出升级命令**
        // （`upgrade_step` 是纯字符串拼接、不起进程，每帧问一次无副作用）。
        // 不依赖"有 latest 就等于升得了"那个巧合——cargo 哪天接上 outdated
        // 数据时，这条判据自动跟上，不用回来改这里
        let can_upgrade = p.latest.is_some()
            && qingbird_core::cli::provider_of(p.provider)
                .and_then(|pr| pr.upgrade_step(p))
                .is_some();
        let id = p.id.clone();
        card(t)
            .child(head.build(t))
            .child(kv("版本", &version, t))
            .child(kv("来源", p.provider.label(), t))
            // 桌面关联（core 按 PE 子系统判的两档之一）：**它是给用户看的卸载前提**
            // ——「命令行 + 桌面程序」这一颗不能只当命令行卸。判不出证据时是「未确定」，
            // 那句话照写：**"没证据"本身也是一句该让用户知道的话**
            .child(kv("桌面关联", p.kind.label(), t))
            .child(kv("命令", &commands, t))
            .child(kv("安装目录", &install, t))
            .child(dirs_section(&p.dirs, t))
            // 卡底两颗管理键。右对齐（与仓库详情卡的操作排同向）
            .child(
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .justify_end()
                    .gap(px(GAP))
                    .pt(px(GAP_TIGHT))
                    .when(can_upgrade, |row| {
                        let id = p.id.clone();
                        row.child(
                            Button::new("cli-upgrade")
                                .label(if mine { "升级中…" } else { "升级" })
                                .icon(icon_sm(AssetIcon::ArrowUp).text_color(tier3(t)))
                                .with_size(Size::Small)
                                // 忙态禁言：**任何一个** CLI 操作在跑就禁
                                // （设计 §11 串行），不只是这一颗在跑——
                                // 两颗一起跑会在同一个包管理器上撞车
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.upgrade_cli_pkg(&id, cx);
                                })),
                        )
                    })
                    .child(
                        Button::new("cli-uninstall")
                            .label(if mine { "卸载中…" } else { "卸载…" })
                            .icon(icon_sm(AssetIcon::Trash).text_color(tier3(t)))
                            // **danger**：它开的是一颗不可逆动作的确认框，
                            // 用主色等于鼓励用户点（同 `dialogs/clean.rs` 那条判据）
                            .danger()
                            .with_size(Size::Small)
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_cli_uninstall_dlg(&id, window, cx);
                            })),
                    ),
            )
    }

    /// 右栏那条「共享缓存」汇总（设计 §9）——**紧跟在详情卡下面**，不压到底部。
    ///
    /// 每行尾巴一颗「清理」（阶段三）：一颗一条地清，也走确认框
    /// （`open_cli_clean_dlg`）——**删 18.8 GB 不给一次「你确定」不能接受**，
    /// 与 `dialogs/clean.rs` 同一条判据。⌘K「清理共享缓存」是整批的那条路
    fn shared_box(&self, t: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        if self.cli.shared.is_empty() {
            return None;
        }
        let mut rows = v_flex().w_full().gap(px(GAP_DENSE));
        for d in self.cli.shared.iter() {
            rows = rows.child(self.shared_row(d, t, cx));
        }
        Some(sunken_box(
            v_flex()
                .w_full()
                // ⚠️ 内边距取 `GAP_XL`——与上面那张白卡**同一档**。这一块是那一列的
                // 同级孩子、不是"卡内盒"（卡内盒那一档是 `GAP`），用 `GAP` 会让它的字
                // 比卡里的字左移 8px：用户 2026-10-13 说的"文字太靠边、没让出边留白"
                // 就是这 8px（帧上量过：白卡内容 x=314、这一块 x=306）
                .p(px(GAP_XL))
                // 标题与行之间是"两个不同东西" ⇒ `GAP_LG`（卡内块间距同一档）
                .gap(px(GAP_LG))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap(px(GAP))
                        .child(
                            div()
                                .text_size(px(fs_meta()))
                                .text_color(tier3(t))
                                .child("共享缓存"),
                        )
                        .child(div().flex_1())
                        // 整批清的那颗（⌘K 同一条路）。**与逐颗那颗分开**：
                        // 一颗是"我只想清 npm 缓存"，一颗是"全清"——代价差一个量级
                        .child(
                            Button::new("cli-clean-shared-all")
                                .label("全部清理…")
                                .with_size(Size::Small)
                                .disabled(self.cli_busy_any())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let targets = this.cli.shared.clone();
                                    this.open_cli_clean_dlg(targets, window, cx);
                                })),
                        ),
                )
                .child(rows),
            t,
        ))
    }

/// 共享缓存汇总里的一行：名字 · 路径 · 体积 · 「清理」。
///
/// 名字是**这一条的身份**（`npm 缓存` 与 `pnpm store` 是两样东西），
/// 所以它排在最前面、用的是正文色；路径压到后面去。
/// 「清理」在行尾（阶段三）：一颗一条地清，也过确认框——大删除不许一键直达
///
/// ⛔ **它是 `&self` 方法，render 期直读 `self`，不许 `cx.entity()` + `read(cx)`**：
/// render 时 Shell 正被 update 借着，再 read 同一个 entity 直接 panic
/// （"cannot read Shell while it is already being updated"，gpui entity_map.rs:156）。
/// 2026-10-14 抓帧当场崩在这——**进 CLI 屏（共享缓存非空时）必现**，
/// 而 cargo check / 单测一个都不报。这一版当初写成自由函数（没有 self 可读）
/// 才绕去 read entity；改成方法就是了，`cx` 只留给那颗键的 listener 用
fn shared_row(&self, d: &PackageDir, t: &Theme, cx: &mut Context<Shell>) -> Div {
    let this_busy = self.cli.shared_busy.contains(&d.path);
    let any_busy = self.cli_busy_any();
    // 行尾那颗键要找回来得按路径对（清单会因重扫变，下标会错位）
    let path = d.path.clone();
    h_flex()
        .w_full()
        .items_center()
        .gap(px(GAP))
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(fs_meta()))
                .text_color(body(t))
                .child(d.label.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(fs_meta()))
                .text_color(tier3(t))
                .child(d.path.clone()),
        )
        .child(
            div()
                .w(px(DIR_SIZE_W))
                .flex_shrink_0()
                .text_size(px(fs_meta()))
                .text_color(body(t))
                .child(size_text(d.size)),
        )
        .child(
            Button::new(SharedString::from(format!("cli-clean-{path}")))
                .label(if this_busy { "清理中…" } else { "清理" })
                .with_size(Size::Small)
                // 任何一个 CLI 操作在跑就禁（设计 §11 串行）——共享缓存清理
                // 与卸载 / 升级共用同一把串行锁
                .disabled(any_busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    // 只清这一颗：目标清单就一条。**按路径从当下清单里取**——
                    // 不用行里那个快照（重扫之后大小可能变了，而确认框要报新数）
                    let target = this.cli.shared.iter().find(|x| x.path == path).cloned();
                    if let Some(one) = target {
                        this.open_cli_clean_dlg(vec![one], window, cx);
                    }
                })),
        )
}

    /// 右栏底部那块「操作日志」（设计 §10.1：**收起态**，复用 `log.rs` 的输出流）。
    ///
    /// 两档：
    /// - **收起**（默认）：一条可点的摘要——「操作日志」+ 最后一句结论
    ///   （或「还没有操作记录」）。一屏的主角是工具与目录，日志是跑完才看的
    /// - **展开**：`output_stream`（等宽输出流，全仓唯一那份）+ 结论行。
    ///   `flex_1 + min_h_0` 吃掉这一列剩下的高度（同仓库详情页日志卡那套）
    ///
    /// ⚠️ **滚动句柄必须用 `cli_log_scroll`**：gpui 的滚动状态按 `ElementId` 存，
    /// 与仓库日志共用一个 id 会共用一个滚动句柄——在 CLI 屏滚一下，
    /// 概览页的日志会跟着跳（`cli_log_scroll` 字段注释同一条）
    ///
    /// 返回 `AnyElement`：收起态那颗条要点得动（`.id()` 之后是 `Stateful<Div>`），
    /// 展开态是张卡——两个分支不同类型，收 `AnyElement` 由调用方 `child` 收
    fn cli_log_box(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let lg = &self.cli_log;
        if !self.cli_log_open {
            // 收起态那条。**结论从 `verdict` 取**（跑完才有）——没有就说"还没有记录"，
            // 不说"运行中"（那一句归展开态的状态行）
            let tail = match &lg.verdict {
                Some((_, msg)) => msg.clone(),
                None => "还没有操作记录".to_string(),
            };
            return h_flex()
                .id("cli-log-collapsed")
                .w_full()
                .flex_shrink_0()
                .items_center()
                .gap(px(GAP))
                .h(px(SIDE_ROW_H))
                .px(px(GAP_XL))
                .rounded(px(r_lg(t)))
                .text_color(tier3(t))
                .hover(|s| s.bg(hover_face(t)))
                .child(
                    icon_sm(AssetIcon::ChevronRight)
                        .text_color(tier4(t))
                        .rotate(percentage(90.0 / 360.0)),
                )
                .child(
                    div()
                        .text_size(px(fs_meta()))
                        .font_weight(FontWeight::MEDIUM)
                        .child("操作日志"),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(fs_meta()))
                        .text_color(tier4(t))
                        .child(tail),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cli_log_open = true;
                    cx.notify();
                }))
                .into_any_element()
        }
        // 展开态。**贴底跟随**与仓库日志同一判据（`should_follow_bottom`）：
        // 用户自己滚回底部后跟随自动恢复
        let stick = should_follow_bottom(
            self.cli_log_scroll.offset().y.to_f64() as f32,
            self.cli_log_scroll.max_offset().y.to_f64() as f32,
        );
        let panel = card(t)
            .flex_1()
            .min_h_0()
            .child(
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(GAP))
                    .h(px(SIDE_ROW_H))
                    .px(px(GAP_XL))
                    .child(
                        div()
                            .text_size(px(fs_meta()))
                            .text_color(tier3(t))
                            .child("操作日志"),
                    )
                    .child(div().flex_1())
                    .child(meta_text(
                        log_state_line(lg.running, None, lg.ts, ""),
                        t,
                    ))
                    .child(
                        div()
                            .id("cli-log-collapse")
                            .cursor_pointer()
                            .child(
                                icon_sm(AssetIcon::ChevronRight)
                                    .text_color(tier4(t))
                                    .rotate(percentage(270.0 / 360.0)),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cli_log_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(output_stream(
                "cli-log-stream",
                &self.cli_log_scroll,
                t,
                lg,
                Some("还没有操作记录"),
                "升级 / 卸载 / 清理共享缓存的结果会记在这里",
            ))
            .child(self.log_verdict_row(t, lg));
        if stick {
            self.cli_log_scroll.scroll_to_bottom();
        }
        panel.into_any_element()
    }
}

/// 「可更新」那颗小圆点（`latest.is_some()` 时画，工具行行尾与组标题行尾）。
///
/// **尺寸走 `DOT` 档**——与状态条那颗「运行中」橙点同径：同屏里两颗都可能
/// 出现，一般大才像一套东西。颜色走 `orange`（「可更新」徽章是 warn-wash +
/// 橙字，这颗点用橙本体——同一个语义的两处呈现）
fn update_dot(t: &Theme) -> Div {
    div()
        .w(px(DOT))
        .h(px(DOT))
        .flex_shrink_0()
        .rounded_full()
        .bg(orange(t))
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

/// 右栏详情里那块「数据目录」（阶段二）。**只读**：勾选与清理是阶段三的事，
/// 这一块只把"这颗工具在盘上留了什么、多大、能不能删"说清楚。
///
/// 一条都没发现时也要写一句：**空着会被读成"还在算"**，而真相是"规则库里没有它"
/// （我们宁可少报也不按名字猜，见 `crates/core/src/cli/rules.toml` 头那两个反例）
fn dirs_section(dirs: &[PackageDir], t: &Theme) -> Div {
    let col = v_flex().w_full().gap(px(GAP_TIGHT)).child(
        div()
            .text_size(px(fs_meta()))
            .text_color(tier3(t))
            .child("数据目录"),
    );
    if dirs.is_empty() {
        return col.child(sunken_box(
            v_flex().w_full().p(px(GAP)).child(
                div()
                    .text_size(px(fs_meta()))
                    .text_color(tier3(t))
                    .child("没有发现这颗工具的独立数据目录"),
            ),
            t,
        ));
    }
    let mut inner = v_flex().w_full().p(px(GAP)).gap(px(GAP_DENSE));
    for d in dirs.iter() {
        inner = inner.child(dir_row(d, t));
    }
    col.child(sunken_box(inner, t))
}

/// 数据目录里的一行：类别 · 体积 · 路径 · 那句话。
fn dir_row(d: &PackageDir, t: &Theme) -> Div {
    h_flex()
        .w_full()
        .items_center()
        .gap(px(GAP))
        .child(
            div()
                .w(px(FORM_LABEL_W))
                .flex_shrink_0()
                .text_size(px(fs_meta()))
                .text_color(tier3(t))
                .child(d.kind.label()),
        )
        .child(
            div()
                .w(px(DIR_SIZE_W))
                .flex_shrink_0()
                .text_size(px(fs_meta()))
                .text_color(body(t))
                .child(size_text(d.size)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(fs_meta()))
                .text_color(tier3(t))
                .child(d.path.clone()),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(fs_meta()))
                .text_color(if d.kind.note_is_warning() {
                    t.colors.danger
                } else {
                    tier3(t)
                })
                .child(d.kind.note()),
        )
}

/// 体积那一格的字。
///
/// ⚠️ **`None`（还没量）与 `Some(0)`（量过、里面就是空的）是两句话**——
/// 这正是 Q39 / 32-11 那一族：**"量到 0" 不许渲染成"没量"**，反过来更不许：
/// 没量过写成"0 B"就是把"我还不知道"说成了一个结论
pub(crate) fn size_text(size: Option<u64>) -> String {
    match size {
        Some(b) => human_bytes(b),
        None => "计算中…".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::monogram;
    use super::size_text;

    /// 体积那一格：**没量过**与**量到 0** 必须是两句话
    #[test]
    fn size_text_separates_not_measured_from_zero() {
        assert_eq!(size_text(None), "计算中…");
        assert_eq!(size_text(Some(0)), "0 B", "量过就是量过——里面空也是 0 B");
        let kb = size_text(Some(1536));
        assert!(kb.contains("KB"), "1536 字节该读成 KB 档，实际 {kb}");
        assert_ne!(kb, "计算中…");
    }

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
