//! 窗口壳：标题栏 / 导航轨 / 状态条，仓库选中与状态刷新。
//!
//! `Render for Shell` 在这里，四块面板由 sidebar / overview / detail 提供。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::motion::spring;
use gpui_kit::base::{h_flex, v_flex, GlobalState, TextSelection};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::resizable::{
    h_resizable, resizable_panel, ResizablePanelGroup, ResizableState,
};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::text::TextViewState;
use gpui_kit::component::table::{TableEvent, TableState};
use gpui_kit::component::{ActiveTheme, Selectable as _, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, img, px, AnyElement, AppContext, Context, Div, FontWeight, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Pixels, Render, ScrollHandle,
    StatefulInteractiveElement, Styled, Window,
};
use qingbird_core::fork;
use qingbird_core::net;
use qingbird_core::repos::{self};
use qingbird_core::run_cmd::{self, Slot};
use qingbird_core::store::{self, CloseAction, RepoEntry};
use qingbird_core::types::RepoStatus;
use std::collections::HashMap;

use crate::detail::DetailTab;
use crate::keys::{
    DismissTop, NextRepo, OpenPalette, OpenTerminal, PrevRepo, StopOp, SyncCheck, SyncPull,
    SyncPush, ToggleInspector, ToggleSidebar,
};
use crate::ops::sync_cfg_from;
use crate::state::*;
use crate::tokens::*;
use crate::widgets::*;

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        // 定位要在这一帧布局之前消费：`scroll_to_top_of_item` 设的偏移量
        // 由本帧的布局读取，晚一帧就会看到一次跳
        self.apply_reveal(cx);
        // 克隆成功后的完整定位（切页 + 展开侧栏 + 选中 + 重扫）。
        // 同样必须在这一帧兑现：后台任务里凑不齐 `&mut Shell` + `&mut Window`
        if let Some(path) = self.pending_reveal.take() {
            self.reveal_repo(path, window, cx);
        }
        // 「浏览…」拉起系统目录选择器（阶段 21-2）。同样在 render 里兑现：
        // 系统选择器要活着的 `&mut Window` 回填输入框，而按钮那个 `on_click`
        // 回调里窗口已经被 `update_window_id` take 走了——
        // `cx.update_window` 会把整个闭包丢掉，点着没反应
        //
        // ⚠️ **先读再置 false，不要顺手写 `self.pending_folder.take()`**：
        // 那是 `Option::take`——2026-10-09 之前这个字段是 `bool`，`bool` 没有 `take`，
        // 编译器会报「`bool` is not an iterator」，一个**看不出指向哪**的错。
        // 现在它已经是 `Option<FolderPick>`（哪个弹窗要选目录必须随标记走），
        // `take()` 才是对的，而 `match` 保证以后新增变体时这里不会漏一支
        if let Some(target) = self.pending_folder.take() {
            match target {
                FolderPick::Clone => self.pick_clone_folder(window, cx),
                FolderPick::AddRepo => self.pick_repo_path_folder(window, cx),
            }
        }
        // 批量跑完那一句通知，同样在这一帧兑现。为什么不走后台任务里那条
        // `win` + `update_window` 的老路，理由写在 `Shell::pending_batch_note` 上
        if let Some(note) = self.pending_batch_note.take() {
            window.push_notification(note, cx);
        }
        // CLI 管理操作跑完：收确认框 + 弹一句结论。后台任务里没有 `&mut Window`
        // （`close_dialog` / `push_notification` 都挂在它上面），与上面同一条路
        if self.pending_cli_dlg_close {
            self.pending_cli_dlg_close = false;
            window.close_dialog(cx);
        }
        if let Some(note) = self.pending_cli_note.take() {
            window.push_notification(note, cx);
        }
        // 关窗询问面板（阶段 22）。同样在 render 里兑现：
        // `on_window_should_close` 的回调跑在 Win32 的 `WM_CLOSE` 处理过程中，
        // 在那儿拉模态浮层等于消息循环嵌套，而它还必须同步返回一个值
        // 决定这一次关闭放不放行。回调里只判档 + 置标记，这里弹窗。
        if self.pending_close_prompt {
            self.pending_close_prompt = false;
            self.open_close_prompt(window, cx);
        }
        // 「完全退出」档的兑现。**同样不在回调里就地 `quit()`**：
        // `QuitMode::Explicit` 下关窗只是销毁窗口、进程留着，
        // 真正结束进程的是 `App::quit()`，而它得在正常路径上调用。
        //
        // `Context<T>` 实现了 `Deref<Target = App>`（`app/context.rs:27-40`），
        // 所以这里 `cx.quit()` 就是 `App::quit()`。
        if self.pending_app_quit {
            self.pending_app_quit = false;
            cx.quit();
        }
        // 「复制地址」的对号反馈到期翻回去。
        //
        // 翻回去由 `arm_copy_expiry` 排的那个定时任务负责（它在 `mark_copied`
        // 时就排好了，醒来清 `copied` 并 notify），这里**不再每帧重复排**——
        // 每帧排一个两秒后的任务等于每秒排六百个。
        //
        // 下面这一段只兜一个洞：`Shell` 是从配置反序列化不出来的，
        // 而 `copied`/`copied_until` 也只在内存里，所以理论上不会出现
        // "有 `copied` 但没有任务在跑"的状态。留着它是因为代价只有一次
        // 比较，而漏掉的话表现是**对号永远挂着**——一个静默的坏状态。
        if self.copied.is_some() && qingbird_core::store::now_secs() >= self.copied_until {
            self.copied = None;
            // `cx.notify()` 不用调：这一帧正在重画，改完状态本帧就生效
        }
        // 总览的行快照每帧同步一次。委托只存索引读 `ov_rows`，
        // 靠这一行保证它永远是最新的——两处各自更新必然会差一帧
        if self.page == Page::Overview {
            self.sync_ov_rows();
            // 列宽跟着窗口走。放在行快照之后：它要读上一帧表格实测的可用宽度，
            // 与行数无关，但同一帧里做完两件事比跨帧清楚
            self.sync_ov_col_widths(window, cx);
            // 冷启动就停在总览页，而活跃度扫描只挂在「点导航轨进总览」那条路上。
            // 少这一次，柱状图是一排 0 高度——看着像"近两周没人提交"，不是像 bug
            if self.ov_need_scan {
                self.ov_need_scan = false;
                self.scan_overview_activity(cx);
            }
        }

        // 主区：按页面与仓库归属路由
        let main: AnyElement = match self.page {
            Page::Overview => self.overview_page(&t, cx).into_any_element(),
            Page::Repos => match self.selected.clone() {
                // 两类仓库进**同一个**详情页骨架，只有页签序列按归属裁剪不同
                Some(path) => self.detail_page(&path, &t, cx).into_any_element(),
                None => empty_main(&t, window).into_any_element(),
            },
            // CLI 工具屏：左栏工具列表 + 右栏详情，仿仓库屏骨架（阶段一为新屏）。
            // **左栏不在这儿给**——它是 `body-split` 的 0 号面板，与仓库侧栏
            // 同一个位置、同一套机制（见下面 `sidebar_mounted` 那一段）
            Page::CliTools => self.cli_detail(&t, cx).into_any_element(),
        };

        // 侧栏宽度只有一个真源：`ResizableState` 里第 0 号面板的宽度。
        // 折叠 / 展开 / 切到总览都只是把"目标宽度"改成 0 或 sidebar_w，
        // 中间值由弹簧补出来——不要在别处再存一份宽度。
        //
        // ⚠️ **回弹走 `spring()` 而不是 `transition()`**（阶段 21-6）：
        // 用户要的是「缓冲回弹」——到位后有一点点过冲再收住，那正是弹簧的
        // 行为；`transition(..).easing(easing_move)` 是匀速到位后停住。
        //
        // `spring_move` 的类型是 `Spring` 而**不是** `Easing`，所以它**喂不进**
        // `Transition::easing(..)`（那是编译期就挡住的，不是运行期行为差异）。
        // 基座给了独立的 `spring(id, target, policy, ..)`（`motion.rs:573`）。
        //
        // 顺带一个它比 transition 强的地方：中途改目标时**保留当前速度重新导向**，
        // 而不会从头开始——用户连着点两下开合，动效不会"重置一下"。
        //
        // ⚠️⚠️ **用自订的 `spring_bounce()` 而不是官方 `spring_move`**（阶段 25）：
        // 官方那条 ζ=0.85 ⇒ 过冲 0.63% ⇒ 侧栏 220px 行程上只有 **1.4px**，
        // 弹簧确实在跑、用户确实看不到回弹（2026-10-05 实机反馈）。
        // 数字与推导见 `tokens.rs` 动效段，那儿有单测钉住"过冲必须看得见"。
        // ⚠️ **左栏由两屏共用同一套面板机制**：仓库页是仓库侧栏，CLI 屏是工具列表。
        // 这就是"边距与仓库版面一比一"的落法——**不去微调数值，而是让 CLI 左栏
        // 就是同一种面板**：宽度来源同一个 `self.sidebar_w`、上下限同一条
        // `SIDEBAR_W_RANGE`、朝向气口同一个 `side_shell` 的 `mr(GAP_TIGHT)`。
        // 自己拼一个定宽 `div` 是本项目最贵的那类错——2026-10-13 实机量出来：
        // 那个 div 的卡比仓库侧栏**宽 4px、右移 1px**（358 vs 353 白面），
        // 而肉眼几乎看不出、`cargo check` 与单测一个字都不报。
        // 收起态**两屏各存一份**（`sidebar_collapsed` / `cli_collapsed`），
        // 见 `state.rs::cli_collapsed`；判据写成"这两屏"而不是枚举出别的屏
        let target_w: Pixels = if self.left_collapsed() || !self.has_left_col() {
            px(0.0)
        } else {
            px(self.sidebar_w)
        };
        // id 带代次后缀：**拖拽松手后换一份新状态，让它就地静止在新宽度上**，
        // 而不是从旧宽度弹过去（那正是「拉大拉小时弹簧一下」的成因）。
        // 详见 `state.rs::sidebar_gen`——包括"为什么换 id 不会泄漏 keyed state"。
        let anim_w: Pixels =
            spring(format!("sidebar-width-{}", self.sidebar_gen), target_w, spring_bounce(), window, cx)
                // 夹住下界：ζ=0.6 收拢时**会真的过冲到负宽度**（约 -21px）。
                // 显式夹成 0，而不是指望 `ResizableState` 内部替我们夹——
                // "隐式行为"改成"显式规则"是这里唯一该做的选择。
                .max(px(0.0));
        // 补间中 = 当前值与目标值对不上。`cx.reduce_motion()` 为真时官方
        // transition 直接返回终值，这里自然落到 false，不补间
        let animating = (anim_w - target_w).abs() > px(0.5);

        // 只在可见或正在滑出时挂面板。宽度到 0 就把面板整个摘掉，
        // 这才是"不留空槽位"——留一个 0 宽面板在 flex 里，主区虽然吃掉了它的
        // 份额，可第 1 号面板上那个拖拽把手还挂在 x=0，一条按了没用的 4px 命中区
        let sidebar_mounted = (self.has_left_col() && !self.left_collapsed()) || anim_w > px(0.5);

        // ⚠️ **挂载那一帧的插入要用「意图宽」，而且不能只在补间时跑**（2026-10-09 帧上量出来的成因）。
        //
        // 原来这一整支挂在 `animating` 里。从总览切回仓库页时弹簧本来就静止
        // （`anim_w == target_w` ⇒ `animating` 是 false）⇒ **两个分支一条都不跑**，
        // 面板于是由基座 `sync_panels_count` 补成占位值，再由 `update_panel_size`
        // 量"还没缩之前的整组宽"写进去（实测 1232）⇒ 随后被 `size_range` 夹到上限
        // `qingbird_core::store::SIDEBAR_W_RANGE.1` = **360** ⇒ 而那条写只在"还等于占位值"时才发生，**就此锁死**。
        // 后果：左栏 360 / 中间 872，用户设的 `sidebar_w` = 263 **从来没生效过**，
        // 而中间那 872 刚好压在他给的 870 上，所以这一条一直没被发现。
        //
        // 补间期间仍然由弹簧说话：**不在非补间时写**——那时宽度归拖拽，
        // 每帧写会把用户刚拖出来的宽度一帧拽回去（阶段 25 那条「弹簧一下」）。
        if sidebar_mounted {
            // ⚠️ 挂载那一帧插的尺寸 = **弹簧此刻的值**，不是意图宽
            // （2026-10-09 每帧打点量出来的，路线表 31.4 补二十七）。
            //
            // 原来这里写的是 `px(self.sidebar_w)`。静止时两者是同一个数，看不出差别；
            // 而**展开途中**弹簧还在 0 附近，那一帧却把面板挂成整宽 ⇒ 画面是
            // 「啪一下全宽 → 缩回弹簧位 → 再滑出来」，用户报的就是这一闪
            // （他问「滑动和弹簧回弹是被删了还是接线断了」）。
            // 打点实测：弹簧本身**一直在跑**，过冲 287.9 / 目标 263 = **+9.5%**，
            // 正是 `tokens::spring_bounce()` 的 ζ=0.6 该有的量——坏的不是弹簧，
            // 是挂载那一帧抢先把结果写死了。
            //
            // 这条改动**不影响补二十五那个修复**：那一修要的是"非补间时也要挂"，
            // 而静止时 `anim_w == self.sidebar_w`，插进去的还是同一个数。
            self.split.update(cx, |s, cx| {
                if s.sizes().len() < 2 {
                    s.insert_panel(Some(anim_w), Some(0), cx);
                } else if animating {
                    s.resize_panel(0, anim_w, window, cx);
                }
            });
        }
        // 每帧把"中间那一格"（挂侧栏时是下标 1，没挂就只有 0 号这一格）交回给 flex。
        // 这一句才是"左栏抢宽"的根，理由与越界处理见 `keep_main_flexible`
        self.split.update(cx, |s, cx| {
            let flex_ix = if sidebar_mounted { 1 } else { 0 };
            Self::keep_main_flexible(s, flex_ix, cx);
        });

        // 下限放开成 0 的两种情形：正在补间（滑进滑出都要经过 200px 以下），
        // 或者目标就是 0。少了后一条，折叠停在仓库页时 min_w 会把面板按回最小宽，
        // 凭空多出一条空槽——正好是这一步要消灭的东西。
        // 非折叠状态下下限照常夹住，拖宽才拖不到 0
        //
        // ⚠️ 下限**不是** `SIDEBAR_MIN_W` 那个 200：200 是"文字放得下"拍的数，
        // 帧上量到侧栏拖到最小那一站，底部「克隆仓库」那颗按钮的盒子压过卡右缘 8px。
        // 现在这一档由那两颗按钮的构成算，且**跟着字号档走**（推导见 `sidebar_min_w`）
        let min_w: Pixels = if animating || target_w <= px(0.5) {
            px(0.0)
        } else {
            px(sidebar_min_w(fs_sec()))
        };

        let mut group = h_resizable("body-split").with_state(&self.split);
        if sidebar_mounted {
            group = group.child(
                resizable_panel()
                    .size_range(min_w..px(qingbird_core::store::SIDEBAR_W_RANGE.1))
                    .size(px(self.sidebar_w))
                    // 官方面板默认 flex_grow: 1，会把主区挤走；定宽面板要自己关掉
                    .flex_none()
                    // 这一颗就是"两屏的左栏"：仓库页给仓库侧栏，CLI 屏给工具列表。
                    // 两者的卡外壳 / 内距 / 顶距 / 朝向气口都出自同一颗 `side_shell`
                    .child(if self.page == Page::CliTools {
                        self.cli_list(&t, cx).into_any_element()
                    } else {
                        self.sidebar(&t, window, cx).into_any_element()
                    }),
            );
        }
        // 主区不带 size：不写 flex_basis，让它吃掉剩下的全部宽度。
        // `on_resize` 只在拖拽松手时触发（不是每帧），正合"松手才落盘"的语义
        let group = group.child(resizable_panel().child(main)).on_resize({
            let this = cx.entity();
            move |state, _, cx| {
                let w = state.read(cx).sizes().first().copied();
                this.update(cx, |this, cx| {
                    if let Some(w) = w {
                        this.commit_sidebar_width(w, cx);
                    }
                });
            }
        });
        self.finish_frame(t, group, window, cx)
    }
}

impl Shell {
    /// 这一屏有没有**左栏**（仓库页 = 仓库侧栏，CLI 屏 = 工具列表）。
    ///
    /// 两屏共用同一套面板机制（`body-split` 的 0 号面板 + `self.sidebar_w` 宽度来源），
    /// 所以"宽多少 / 内距多少 / 朝向气口多少"三处各只有一个出处，几何天然一比一。
    /// 写成 `matches!` 而不是枚举"没有左栏的那些屏"：加第四屏时这里天然跟上
    /// （旧写法就是枚举出来的，CLI 屏一进来侧栏立刻弹了出来）
    fn has_left_col(&self) -> bool {
        matches!(self.page, Page::Repos | Page::CliTools)
    }

    /// 左栏此刻的收起态。**两屏各一份**（设计 §10.1「两者互不干扰」）：
    /// 在 CLI 屏收起工具列表，不该把仓库页的侧栏也一起收掉
    fn left_collapsed(&self) -> bool {
        match self.page {
            Page::CliTools => self.cli_collapsed,
            _ => self.sidebar_collapsed,
        }
    }

    /// 这一屏有没有**右栏**（检视列 / 预留的 AI 助理栏）可开。
    ///
    /// 仓库页要选中一个仓库才有它（那一列读的是"当前仓库"这个对象）；
    /// CLI 屏是它自己的对象，常驻可开（2026-10-13 用户令：CLI 屏 100% 要这一栏）
    fn has_inspector(&self) -> bool {
        match self.page {
            Page::CliTools => true,
            Page::Repos => self.selected.is_some(),
            Page::Overview => false,
        }
    }

    /// 组好窗口的其余部分：导航轨 / 主区分组 / 检视抽屉 / 状态条。
    /// `body` 已经是「左栏（可选）+ 主区」的 resizable 分组。
    fn finish_frame(
        &mut self,
        t: Theme,
        body: ResizablePanelGroup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 「中间那一列不足 870 就把窗口向右撑大」这条规则落在这里，
        // **不在下面的构建链里**：那条链一路借着 `self`（`title_bar` / `nav_rail` /
        // `status_bar` 都在借），而撑窗口要 `&mut self` 改那两个记账字段。
        // 判据与三条守则是 `ensure_shell_width`，注释在那儿
        //
        // 两栏"开不开"用**目标态**判（折叠态 / 抽屉开关），不用 `*_mounted`：
        // 后者在补间里会跟着弹簧值来回翻转，窗口就会一边开抽屉一边长一下缩一下
        self.ensure_shell_width(
            self.has_left_col() && !self.left_collapsed(),
            self.has_inspector() && self.inspector_open,
            window,
        );
        // ⚠️ `items_stretch()` 不能省。`h_flex()` 装的是 `items_center()`（基座
        // `styled.rs` 的 `h_flex` 文档明写「centers its children on the cross axis」，
        // 而 `v_flex()` 不装任何交叉轴对齐 = stretch，**两者不对称**）。
        // 不加这一句：nav_rail / 主区 / 检视抽屉三个子项各拿**内容高度**并被
        // 垂直居中，主区整体缩成内容高、飘在行中间——表现为整屏上方一大块空洞、
        // 概览页被挤成一小条（2026-10-03 实机首轮就撞上，三个主板面同时炸）。
        //
        // 「主区 + 检视列」是一个**独立的 resizable 分组**（`insp_split`），
        // 与侧栏那组（`split`）分开持有：侧栏那组只在仓库页有、且总览页整组不挂，
        // 混进同一组会让面板下标随页面变，下标与列的对应关系就不再固定。
        // 面板数量由基座的 `sync_panels_count` 每帧自动对齐
        // （`resizable/panel.rs:151`），所以收起那一列不用手动同步状态。
        //
        // **主区面板不带 `size`**：让 `flex_grow` 吃掉除检视列以外的全部宽度；
        // 检视列 `.flex_none()` 是必须的——官方面板默认 `flex_grow: 1`，
        // 不关掉它会把主区挤走（`resizable/panel.rs:318-322` 与官方文档同一句）
        let mut main_group = h_resizable("insp-split").with_state(&self.insp_split);
        // 0 号面板 = 「侧栏 + 主区」那一整组，**不带 `size`**：宽度由 `flex_grow`
        // 给。它为什么还需要上面那段 `reset_panel` 才不溢出，见那里
        main_group = main_group.child(resizable_panel().child(body));
        //
        // ⚠️ **检视列走的是与侧栏同一套补间**（阶段 21-6）。原先这里是
        // `if … { child(...) }` **一帧到位**：用户说「以前有点动效」，
        // 实测左栏那条动效一直在、右栏这条**从来就没有**——
        // 两栏手感不一致的根源就在这一行。
        // ⚠️ **与左栏同一套的三处（阶段 25）**，别只改一处：
        // ① `spring_move`（ζ=0.85 ⇒ 过冲 0.63% ⇒ 320px 行程上 2px，**看不见**）
        //    → 改自订的 `spring_bounce()`（ζ=0.60 ⇒ 约 30px，一眼能看出弹了一下）
        // ② id 带代次后缀：拖拽松手后换一份新状态**就地静止在新宽度**，
        //    而不是从旧宽度弹过去——不换的话 `resize_panel` 会把面板
        //    **拽回去再弹过来**，每次拖拽必现（用户 2026-10-05 实机报的
        //    「右栏拉大拉小时会弹簧一下」）
        // ③ 显式夹住 0：ζ=0.6 收拢时会真的过冲到负宽度
        let insp_target: Pixels = if self.has_inspector() && self.inspector_open {
            px(self.inspector_w)
        } else {
            px(0.0)
        };
        let insp_anim_w: Pixels =
            spring(format!("inspector-width-{}", self.insp_gen), insp_target, spring_bounce(), window, cx)
                .max(px(0.0));
        let insp_animating = (insp_anim_w - insp_target).abs() > px(0.5);
        // 与左栏 `sidebar_mounted` 同一处理：宽度到 0 才把面板整个摘掉。
        // 留一个 0 宽面板在 flex 里，那条按了没用的拖拽把手还挂在原处
        let inspector_mounted = insp_anim_w > px(0.5);
        // ⚠️ **Q23（抽屉一开、主区右半截看不见）已修，成因不是"内容压不动"，是这两列的分配本身错了**。
        //
        // 帧上量到的旧数（临时 `QB_GEOM_DEBUG` 自报，核完已删）：抽屉展开那一帧
        // `split = [630, 282]`、`insp_split = [1232, 320]` ⇒ 中间那一列只剩 **282px**，
        // 而左栏拿走了 630（用户设的是 263）。主区不是被裁掉一条，是被压掉三分之二。
        //
        // **根因**：一个分组里只要每一格都留下了宽度偏好（`panel.size = Some(..)`），
        // 基座就在容器宽度变化时按**百分比**重分整组（`adjust_to_container_size`）。
        // 挂上右栏 = 外层容器变了 → 中间那格被按比例缩小；内层容器又跟着变小 →
        // 左栏也被按比例重分一次。两次都是**比例**，所以 `sidebar_w` / `inspector_w`
        // 这两个数只在第一次挂载时管用，之后每开合一次就漂一次。
        //
        // **修法 = `keep_main_flexible`**：每帧把"中间那一格"的偏好交回 flex
        // （基座自己给了这条早退：「Flex already fits the container, so there is
        // nothing here to adjust」）。整组不做比例重分 ⇒ 两栏的宽恒等于用户设的数，
        // 窗口变宽变窄全部由中间那一列吸收。
        //
        // 四条无效改法与两条基座事实登记在路线表 **31.4 补二十四**，别再查一遍。
        // 其中两条仍然成立，是这次改动的地基：
        // ① 面板的运行时约束（`min_w` / `max_w` / `flex_basis`）写在 `refine_style`
        //    **之后**（`resizable/panel.rs:330` 与 `:334-336`）⇒ **调用方盖不住**，
        //    往 `resizable_panel()` 上写 `min_w_0` 是空操作；
        // ② 面板实测尺寸**一旦被量出来就锁死**：`update_panel_size` 只在
        //    `sizes[ix]` 还等于占位值 `PANEL_MIN_SIZE` 时才写（`mod.rs:211-214`），
        //    `panel.rs:350-351` 再拿它去写 `flex_basis` ⇒ 所以要**每帧**清，
        //    不是只在挂载那一帧清一次

        // 检视抽屉在有"当前对象"的那两屏都有（仓库页 / CLI 屏）——
        // 总览页没有当前仓库，这一列里没有东西可看，`has_inspector` 判的就是它
        if inspector_mounted {
            // ⚠️ 与左栏**同一处修法**（AGENTS 那条"两处用了同一套写法必须一起改"）：
            // 挂载那一帧插的尺寸 = **弹簧此刻的值**，理由与实测数字写在左栏
            // `sidebar_mounted` 那一段（31.4 补二十七）——静止时两者同一个数，
            // 补二十五那条"非补间也要挂"的修复不受影响。
            //
            // 早先这里写的是意图宽 `px(self.inspector_w)`，打点量到挂载帧
            // `sizes=[931, 320]` 而弹簧那一刻在 309 ⇒ 抽屉也会先弹一下全宽。
            //
            // ⚠️ **下标是写死的 1，不是 `len() - 1`**（2026-10-05 实机闪退）。
            // 这一组里**主区永远占 0 号位**（`main_group` 先挂它，检视列第二个挂），
            // 所以检视列的下标恒为 1。早先写 `sizes().len().saturating_sub(1)`——
            // 面板还没挂上的那一帧 `len()` 是 1，减出 **0**，于是把检视列
            // **插到 0 号位顶掉了主区**；基座下一帧按子元素个数对齐，列数与下标就此错位，
            // 表现是**点导航轨第二颗按钮直接闪退**（事件日志 `ucrtbase.dll` +
            // 异常码 `0xc0000409`，即 `/GS` 栈保护命中——不是 Rust panic，
            // 所以 stderr 一个字都没有）
            self.insp_split.update(cx, |s, cx| {
                if s.sizes().len() < 2 {
                    s.insert_panel(Some(insp_anim_w), Some(1), cx);
                } else if insp_animating {
                    s.resize_panel(1, insp_anim_w, window, cx);
                }
            });
            main_group = main_group.child(
                resizable_panel()
                    // 补间中下限放开成 0（滑进滑出都要经过窄于最小宽的一段），
                    // 与左栏 `min_w` 那条同一理由：不放开的话收起时会停在最小宽，
                    // 凭空多出一条空槽——正好是这一步要消灭的东西
                    .size_range(if insp_animating {
                        px(0.0)..px(qingbird_core::store::INSPECTOR_W_RANGE.1)
                    } else {
                        px(qingbird_core::store::INSPECTOR_W_RANGE.0)
                            ..px(qingbird_core::store::INSPECTOR_W_RANGE.1)
                    })
                    // 只在这一列**首次挂上**、状态里还没有它那一格时起作用
                    // （`panel.rs:341-349`：`panel_state.size` 为 None 才吃
                    // `initial_size`）。之后就归 `ResizableState` 自己管，
                    // 每帧再写一遍会把用户正拖的宽度一帧拽回去
                    .size(px(self.inspector_w))
                    .flex_none()
                    .child(self.inspector(&t, cx)),
            );
        }
        // 与内层**同一句话、同一个理由**（AGENTS 那条"两处用了同一套写法必须一起改"）：
        // 外层这一组里"中间那一格"永远是 0 号（主区先挂，检视列第二个挂）。
        // 放在 `if` 外面是有意的——挂载分支里的 `insert_panel` 会把整组重分成 Some，
        // 必须**在它之后**清才清得掉
        self.insp_split.update(cx, |s, cx| Self::keep_main_flexible(s, 0, cx));
        // `on_resize` 只在拖拽松手时触发（不是每帧），正合"松手才落盘"的语义
        let main_group = main_group.on_resize({
            let this = cx.entity();
            move |state, _, cx| {
                let w = state.read(cx).sizes().last().copied();
                this.update(cx, |this, cx| {
                    if let Some(w) = w {
                        this.commit_inspector_width(w, cx);
                    }
                });
            }
        });
        // 落点提示要用到"落进哪一组"，而 `.when()` 的闭包里只有那个 div、拿不到
        // `self`（它是被 `&mut self` 借走的）。所以在链条**外面**算一次：
        // 判据只有这一处，`on_drag_move` / `on_drop` 各自也调 `Shell::drop_target`，
        // 三处同源（D78）
        let drop_group = self.drop_target();
        v_flex()
            .size_full()
            .min_h_0()
            .bg(t.colors.background)
            .text_color(t.colors.foreground)
            // 落点提示卡是 `.absolute().inset_0()`，没有定位上下文它会贴到窗口原点
            .relative()
            // ---------- 从资源管理器拖文件夹进来（阶段 25，补旧 Tauri 版）----------
            //
            // 挂根 div = 拖到窗口任意位置都能放，与旧版一致。原理与那一长串
            // 注释在 `drag_drop.rs` 模块头（冒泡阶段 + 命中即触发 + stop_propagation，
            // 本项目没有任何子元素注册 drop 监听，所以事件一路冒到根）。
            //
            // ⚠️ "任意位置"指的是**仓库页那一屏**。总览页不收（D78，判据是
            // `state::drop_target`），三处闸门都调它——**改一处记得看另外两处**。
            //
            // ⚠️ **类型参数必须是 `ExternalPaths`，不是 `PathBuf`**：写错一行不报错，
            // 只是永远不触发。`PathBuf` 是本应用自己往外拖文件时的 payload。
            .on_drag_move(cx.listener(
                |this, _: &gpui_kit::DragMoveEvent<gpui_kit::ExternalPaths>, _, cx| {
                    // 总览页不收（D78）：这一屏没有"当前分类"，别画落点提示。
                    // 提示卡是 `.absolute().inset_0()`，画出来就是在**承诺**能放。
                    if this.drop_target().is_none() {
                        return;
                    }
                    if !this.dragging {
                        this.dragging = true;
                        cx.notify();
                    }
                },
            ))
            .on_drop(cx.listener(
                |this, e: &gpui_kit::ExternalPaths, window, cx| {
                    // **松手这一下必须把标志落回去**：漏了的话提示卡会一直挂着，
                    // 而它是 `.absolute().inset_0()`，正好盖住整个界面
                    this.dragging = false;
                    match this.drop_target() {
                        Some(g) => this.on_paths_dropped(e.paths(), g, window, cx),
                        // Windows 的复制光标是平台按窗口给的（改不掉），所以用户
                        // 会以为能放。**说清去哪放**，比静默吃掉强
                        None => window.push_notification(
                            "总览页不能添加仓库，请到「我的仓库」或「第三方仓库」页拖入",
                            cx,
                        ),
                    }
                },
            ))
            .on_file_drop_exit(cx.listener(|this, _: &gpui_kit::FileDropEvent, _, cx| {
                if this.dragging {
                    this.dragging = false;
                    cx.notify();
                }
            }))
            // 必须 `track_focus`，否则这个节点不在派发路径上（`state.rs` 的 `focus` 字段有详述）
            .track_focus(&self.focus)
            // ⚠️ Action 监听器必须挂在**元素**上，不能用 `Context::on_action`。
            // 后者（window.rs:6677）第一行是 `debug_assert_paint()`，只允许在
            // `DrawPhase::Paint` 期间调；而 `DrawPhase::Paint` 只在
            // `Window::draw_roots` 的 `root_element.paint(..)` 那段被设上（window.rs:3571），
            // `Render::render` 跑的是 `Prepaint`。所以在 `new()` 或 `render` 里挂：
            // debug 构建直接 panic（"this method can only be called during paint"，
            // 2026-10-03 首次实机启动就是这个），release 下 `debug_assert!` 是空操作、
            // 监听器只活一帧（`next_frame` 每帧清空）——快捷键会集体失灵且界面看不出异常。
            // 元素级则由 `Div::paint_keyboard_listeners` 在 paint 里统一注册，
            // 天然每帧重挂，不需要额外维护。gpui-component 自己就是这么写的。
            .on_action(
                cx.listener(|this, _: &ToggleSidebar, window, cx| this.toggle_sidebar(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleInspector, _, cx| this.toggle_inspector(cx)))
            .on_action(cx.listener(|this, _: &StopOp, window, cx| this.stop_current(window, cx)))
            .on_action(
                cx.listener(|this, _: &OpenPalette, window, cx| this.open_palette(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &OpenTerminal, window, cx| this.hotkey_terminal(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SyncPull, window, cx| {
                    this.hotkey_sync(Op::Pull, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &SyncPush, window, cx| {
                    this.hotkey_sync(Op::Push, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &SyncCheck, window, cx| {
                this.hotkey_sync(Op::Check, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NextRepo, window, cx| this.move_repo(1, window, cx)))
            .on_action(cx.listener(|this, _: &PrevRepo, window, cx| this.move_repo(-1, window, cx)))
            .on_action(cx.listener(|this, _: &DismissTop, window, cx| this.dismiss_top(window, cx)))
            .child(self.title_bar(&t, window, cx))
            // 分组外面那层 `flex_1 + min_w_0` 不能省：基座 `ResizablePanelGroup`
            // 渲染出来是 `size_full()`，在 `h_flex` 里直接当子项会占满整行、
            // 把导航轨顶出去。包一层由 flex 分配宽度的容器，它就只吃剩余宽度
            .child(
                h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    // 轨与右侧的同灰分界由 `nav_rail` 自己的 `border_r_1`（`track(t)`）
                    // 来画。这里原来还有一条 `Separator::vertical()`——同灰之后
                    // 它和那条边框就是**挨着的两条发丝线**（用户抱怨的"像有边框"
                    // 正是这一类），所以删掉，只留一个出处
                    .child(self.nav_rail(&t, cx))
                    // **主区外层与窗口右缘那 9px 只写在这一处**（2026-10-08 用户定四边缩进）。
                    // 原来它写在 `overview_page` 与 `detail_page` 各一份，于是**检视抽屉那一列
                    // 没人管**——抽屉的 `popover` 底一路铺到窗口右缘，"右 9"在仓库页实测是 0。
                    // 收到壳层这一格之后，总览页 / 仓库页 / 抽屉三处共用同一个出处，
                    // 而左缘仍然贴导航轨（他要的"左 41 = 轨 40 + 边框 1 + 主区 0"）
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .pr(px(GAP))
                            .child(main_group),
                    ),
            )
            .child(self.status_bar(&t, cx))
            // 落点提示卡。**放最后**：它是整窗绝对定位的一层，
            // 前面那些都是正常流式内容，先挂它会被后来的兄弟盖住。
            //
            // ⚠️ 条件是 `dragging && drop_target().is_some()`，不是光看 `dragging`：
            // `dragging` 是"平台正在拖一个外部路径进来"，提示卡是**能不能放**的
            // 承诺。这两件事在总览页会分开——Windows 的复制光标照给，提示卡不给。
            .when(
                self.dragging && drop_group.is_some(),
                |this| match drop_group {
                    Some(g) => this.child(crate::drag_drop::drop_overlay(&t, g)),
                    None => this,
                },
            )
            .into_any_element()
    }
}

// ================= 标题栏 =================

impl Shell {
    /// 标题栏。**自研，不用官方 `TitleBar`**（B 方案，用户已拍板）。
    ///
    /// ## 为什么组件库做不到
    ///
    /// 官方 `TitleBar` 把**所有** `children` 塞进一个带
    /// `window_control_area(WindowControlArea::Drag)` 的 `h_flex`
    /// （`title_bar.rs:371-398`）。Windows 侧 `handle_hit_test_msg` 把它翻成
    /// `HTCAPTION`（`gpui-pre-windows/src/events.rs:976`）——**非客户区命中**，
    /// 点击直接交给系统去拖窗口，**永远不派发到子元素**。
    ///
    /// 表现就是侧栏开关画得出来、位置也对，但 `on_click` 一次都不触发。
    /// 官方公开 API 只有 `new` / `h` / `bg` / `border_color` / `pl` / `child` /
    /// `children` / `on_close_window`，**没有任何"这块别算拖拽区"的开关**。
    /// 备选是"把开关挪到标题栏之下另起一行"，但那会多占 40px 主区高度，
    /// 已否。官方 `WindowControls` 也是私有 enum（`title_bar.rs:248`），
    /// 导不出来复用，所以那三个键一并接管。
    ///
    /// ## 自研时接住了官方的三样（别弄丢）
    ///
    /// ① **窗口控件只声明 area、交给系统**（官方 `title_bar.rs:220-222`，
    ///    它的 `on_click` 只在 `is_linux` 那条臂里，`:223-242`）。
    /// ② **图标用现成的**：`WindowMinimize` / `WindowMaximize` /
    ///    `WindowRestore` / `WindowClose` 都在 lucide 那一套里
    ///    （`title_bar.rs:148-151`），悬停面同源（`:175-183`）。
    /// ③ **拖拽区**：基座规范明写 "Preserve its drag region"
    ///    （`design-guides.md:467`）。左边这一组排除在拖拽区外，
    ///    **右边空白仍是拖拽区**；双击缩放补 `zoom_window()`
    ///    （官方只在 Linux/macOS 挂，`:344-349`）。
    ///
    /// ## 开关为什么在「青鸟 Git」**文字后面**
    ///
    /// 图标和标题是一个整体（"我是谁"），开关是它们的附属控件（"我在哪"）。
    /// 三者必须并成**同一个** `h_flex`：官方那行是 `justify_between()`，
    /// 分成两个子项会被推到两头去，开关就会贴到窗口控件上。
    pub(crate) fn title_bar(
        &self,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // ⚠️ `on_double_click` 来自 `InteractiveElementExt`（gpui-component 的
        // 扩展 trait，官方 `title_bar.rs:345` 用的就是它）。**它只在
        // `Stateful<E>` 上有**——所以这行必须先 `.id("title-bar")`，
        // 裸 `Div` 上是 E0599。
        use gpui_kit::component::InteractiveElementExt as _;
        // 最大化/还原图标要跟着窗口实际状态翻，而这两个读数**只能从 `Window` 拿**：
        // `Context<T>` 没有 `window()`（E0599），所以 `title_bar` 得收 `&mut Window`。
        // 官方也是从 `render` 那一层往下传（`title_bar.rs:286` 读 `window.is_maximized()`）
        let supported = window.window_controls();
        let maximized = window.is_maximized();
        h_flex()
            // ⚠️ 这个 `id` 不能省：它不只是稳定标识，更是 `on_double_click`
            // 的前置条件（见上面那行 `use` 的注释）
            .id("title-bar")
            .w_full()
            .h(px(title_bar_h()))
            .flex_shrink_0()
            .items_center()
            .bg(t.colors.background)
            .border_b_1()
            .border_color(track(t))
            // 左边距按轨宽反推，让图标与导航轨那一列的中心线对齐
            .pl(px(TITLE_BAR_PAD_L))
            // 左组：图标 + 标题 + 开关（开关在文字**之后**）
            .child(
                h_flex()
                    .items_center()
                    .gap(px(GAP_TIGHT))
                    .flex_shrink_0()
                    .h_full()
                    // 应用图标（老 Tauri 版那张 128px 位图，不是 lucide 的 Bird）
                    .child(img(crate::app_assets::APP_ICON_ASSET).size(px(APP_ICON)))
                    .child(
                        div()
                            .text_size(px(fs_body()))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(body(t))
                            .child("青鸟 Git"),
                    )
                    // 总览页没有左栏可折，藏起来免得点了个没反应的东西。
                    // 仓库页与 CLI 屏都挂（后者折的是工具列表）——两屏顶栏一比一
                    .when(self.has_left_col(), |bar| {
                        bar.child(self.sidebar_toggle(t, cx))
                    }),
            )
            // ③ 拖拽区：左右两组之间的**空白**才是拖窗口的地方
            //
            // ⚠️ **这里必须挂 `on_mouse_down` 收掉文字选中（D76）**。
            // 机制是「按下和抬起落在两个不同的派发域」：
            // `WindowControlArea::Drag` 在 Windows 侧被 `handle_hit_test_msg`
            // 翻成 `HTCAPTION`（`gpui-pre-windows/src/events.rs:976`），
            // 而 `handle_nc_mouse_down_msg` **会把非客户区的按下翻成 client
            // 事件派发、且不消费**（`:1071`）——所以 `on_mouse_down` 挂得上
            // （这也是官方 `TitleBar` 的按钮只差在抬起那一步，见上面的注释）。
            // 但随后 `DefWindowProc` 接管、`SC_MOVE` 进入系统移动循环并
            // **捕获鼠标**，抬起被那个循环吃掉 ⇒ **永远不会有
            // `WM_NCLBUTTONUP`** ⇒ `MouseUpEvent` 不来。
            // 而框选的收尾只挂在那个 `MouseUpEvent` 上
            // （`text_selection.rs:2807`/`:2834` 调 `end()`），
            // 门是 `is_selecting`（`:1904`）。⇒ **卡在 `true` 不动**，
            // 之后每个鼠标移动（`pressed_button: None`，`:1045`）都在扩选区。
            //
            // 表现就是用户报的：说明页里在标题栏中间点一下，
            // 再把鼠标移进文档区，就变成"框选"，
            // 而**真正要框选本该是在文字上按住左键**。
            //
            // 两条一起上，缺一不可：
            // ① `TextSelection::end` 收掉**在途**的选中（拦"已经卡住"）
            // ② `suppress_text_selection` 让这一次按下**不再起跑**新的
            //    （基座原话：「控件自己拥有按下或拖拽交互时用它，
            //    同一个指针事件就不该同时起一个应用级文字选中」）
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(gpui_kit::WindowControlArea::Drag)
                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                        TextSelection::end(window, cx);
                        GlobalState::suppress_text_selection(cx);
                    }),
            )
            .on_double_click(|_, window, _| window.zoom_window())
            // 右栏开关紧挨着主题键（用户 2026-10-09 的令）。条件与左栏那颗同一档：
            // `has_left_col()` 是"这一屏有左栏"，而**有左栏的两屏都有右栏**
            // （仓库页选中一个仓库就有、CLI 屏常驻有）⇒ 两屏顶栏一比一。
            // 总览页两栏都没有，两颗一起藏
            .when(self.has_left_col(), |bar| {
                bar.child(self.inspector_toggle(t, cx))
            })
            .child(self.theme_toggle(t, cx))
            .child(self.window_controls(supported, maximized, cx))
            .into_any_element()
    }

    /// 三颗标题栏开关共用的那副形状（审查台账 §1 第五条：同段三份）。
    /// 各自不同的只有三样：id、图标随状态翻、点击做什么——共用的
    /// "ghost + Small + tier3 + 气泡与读屏标签同一句"收在这里。
    /// 少数派在调用方接着加（检视列那颗的 `tooltip_with_action` 键位角标）。
    fn titlebar_toggle(
        &self,
        id: &'static str,
        icon: AssetIcon,
        label: &'static str,
        t: &Theme,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Shell, &gpui_kit::ClickEvent, &mut Window, &mut Context<Self>) + 'static,
    ) -> Button {
        ghost_btn(id, icon, tier3(t), Size::Small, label, cx, cx.listener(on_click))
    }

    /// 明暗主题切换（阶段 21-4）。**放在窗口控件之前**，也就是紧挨着最小化键。
    ///
    /// ## 为什么不是"移动"而是"新增"
    ///
    /// 阶段 20 只把开关做进了**设置对话框**（`dialogs/settings.rs:763` 的
    /// `Switch`），标题栏上原来那颗是**侧栏开关**、与主题无关。
    /// 所以这一条是新增一个常驻入口，不是把某个已有按钮挪过来。
    ///
    /// ## 为什么放在右端而不是跟着左边那组
    ///
    /// 左边那组（图标 + 「青鸟 Git」+ 侧栏开关）是**应用身份**，
    /// 主题是**全局偏好**——和右边那三颗窗口控件同属"这台窗口的操作"。
    /// 用户把它放在最小化键前面，理由就是"它是一个开关，
    /// 挨着另外几个开关/窗口控件更顺手"，这一条照办。
    ///
    /// ⚠️ **不能挂 `window_control_area(Drag)`**——那会让整颗键点不动
    /// （AGENTS.md 记着官方 `TitleBar` 的那个坑：Windows 侧翻成 `HTMINBUTTON`
    /// 那一类，非客户区命中不派发到子元素）。它是个普通 `Button`，
    /// 标题栏那行的拖拽区是**左右两组之间的空白**，这一颗在空白左边，
    /// 不受影响。
    fn theme_toggle(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        // 图标随**当前主题**翻：现在亮着就摆月亮（点了变暗），
        // 现在暗着就摆太阳。这是"按下会发生什么"的说法，与侧栏开关那颗
        // "按了会收起来"同一个约定
        let (icon, label) = if t.is_dark() {
            (AssetIcon::Sun, "切到浅色主题")
        } else {
            (AssetIcon::Moon, "切到深色主题")
        };
        self.titlebar_toggle("titlebar-theme-toggle", icon, label, t, cx, |_, _, window, cx| {
            let dark = !cx.theme().is_dark();
            Shell::toggle_theme_static(dark, window, cx);
        })
        .into_any_element()
    }
    /// 左栏展开 / 收起开关。**仓库页与 CLI 屏共用这一颗**（两屏顶栏一比一）：
    /// 仓库页折仓库侧栏，CLI 屏折工具列表——图标与文案两样都跟着屏走。
    ///
    /// **为什么用裸 `Button` 而不用官方 `SidebarToggleButton`**：
    /// 那个组件内部写死了 `Button::new("collapse").custom(ghost_variant(cx)).small()` +
    /// `Icon::new(icon).size_4()`（`sidebar/mod.rs:313`/`:378`），
    /// **图标没有 `text_color` 入口**（公开 API 只有 `new` / `side` /
    /// `collapsed` / `accessibility_label` / `on_click`）。
    /// 实机后果：那个图标继承到深色，在标题栏里渲成**一块实心深底**，
    /// 灰白底上一颗糊掉的方块——2026-10-04 抓帧确认。
    ///
    /// 换 `Button` 不是"自研"，是换官方件的**档位**：图标档、颜色档、
    /// 悬停提示都变成可配的，而"图标随折叠态在 Close/Open 之间翻"
    /// 这一条我们自己照搬（`collapsed` 那一行），没丢语义。
    /// 组件库做不到的是**调一个图标颜色**，不是"画一颗开关按钮"。
    fn sidebar_toggle(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        // 图标随折叠态翻：这颗键的图标就是"它现在会做什么"的说明。
        // 文案按屏说清折的是哪个——同一颗键在 CLI 屏折的是工具列表
        let cli = self.page == Page::CliTools;
        let (icon, label) = if self.left_collapsed() {
            (
                AssetIcon::PanelLeftOpen,
                if cli { "展开工具列表" } else { "展开仓库侧栏" },
            )
        } else {
            (
                AssetIcon::PanelLeftClose,
                if cli { "收起工具列表" } else { "收起仓库侧栏" },
            )
        };
        self.titlebar_toggle("titlebar-sidebar-toggle", icon, label, t, cx, |this, _, window, cx| {
            this.toggle_sidebar(window, cx)
        })
        .into_any_element()
    }

    /// 右栏（检视列）展开 / 收起开关。**从抽屉自己的标题行搬上来**
    /// （用户 2026-10-09：「右栏 展开收缩 按钮 移到顶栏那个 明暗主题 按钮的边上」）。
    ///
    /// 搬上来一次解决三件事：
    /// ① 左栏那颗开关本来就在这排，两栏的开关并排 = 同一类操作同一个位置；
    /// ② 抽屉里的标题行整行要撤（这一列改成与左栏同款的悬浮卡，卡里不再有一行"检视"），
    ///    键留在里面就没有落脚处；
    /// ③ 原来"收起后"的入口挂在仓库头右上角（`inspector_reopen`），
    ///    那是**条件出现**的第二颗键——它和这颗键是同一个功能的两个出处，
    ///    本项目为"同帧两颗同 id"付过一次 debug panic 的学费（见 AGENTS.md）。
    ///    现在常驻一颗，`repo_head.rs` 那一颗随之删掉。
    fn inspector_toggle(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let (icon, label) = if self.inspector_open {
            (AssetIcon::PanelRightClose, "收起右栏")
        } else {
            (AssetIcon::PanelRightOpen, "展开右栏")
        };
        self.titlebar_toggle("titlebar-inspector-toggle", icon, label, t, cx, |this, _, _, cx| {
            this.toggle_inspector(cx)
        })
        // 键位角标：官方会自己去 keymap 里查 `ToggleInspector` 画成 `Kbd`，
        // 覆盖上面那颗不带角标的气泡（手写「（Ctrl …）」进文案的话，
        // 键位一改那句话就成了假提示）
        .tooltip_with_action(label, &ToggleInspector, None)
        .into_any_element()
    }

    /// 窗口控件：最小化 / 最大化（还原）/ 关闭。
    ///
    /// ## 点击**一律不自己处理**（①）
    ///
    /// 三块都只挂 `window_control_area`，让 `handle_hit_test_msg` 翻成
    /// `HTMINBUTTON` / `HTMAXBUTTON` / `HTCLOSE`（`events.rs:978-984`），
    /// 由系统发对应的窗口消息。
    ///
    /// ⚠️ 自己在 `on_click` 里调 `remove_window()` 看着一样，行为不一样：
    /// 那条路绕过 `WM_CLOSE`，而 `tray.rs` 的托盘唤起与 `QuitMode::Explicit`
    /// 全按"系统关窗 → 销毁窗口 → 进程留着"设计，绕过去托盘就唤不醒了。
    ///
    /// ## 图标与悬停面（②）
    ///
    /// 图标取官方同款（`title_bar.rs:148-151`），最大化/还原按
    /// `window.is_maximized()`（`window.rs:2374`）翻。系统不支持某键时
    /// 官方也不画图标（`:282-291` 的 `when(supported.., ..)`），这里照做
    ///
    /// ⚠️ 窗口状态要用 `Window` 拿，**不能从 `Context` 拿**：`Context<T>`
    /// 没有 `window()` 方法（E0599，`help` 里提示的 `windows` 是另一回事：
    /// 它返回全部窗口的集合）。所以这两个读数由调用方 `title_bar` 传进来
    fn window_controls(
        &self,
        supported: gpui_kit::WindowControls,
        maximized: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let th = cx.theme().clone();
        h_flex()
            .id("window-controls")
            .items_center()
            .flex_shrink_0()
            .h_full()
            .when(supported.minimize, |row| {
                row.child(window_control_button(
                    "min",
                    Some(AssetIcon::WindowMinimize),
                    hover_face(&th),
                    gpui_kit::WindowControlArea::Min,
                ))
            })
            .when(supported.maximize, |row| {
                row.child(window_control_button(
                    "max",
                    Some(if maximized {
                        AssetIcon::WindowRestore
                    } else {
                        AssetIcon::WindowMaximize
                    }),
                    hover_face(&th),
                    gpui_kit::WindowControlArea::Max,
                ))
            })
            // 关闭键永远给：系统说"不支持"的通常是别的键
            .child(window_control_button(
                "close",
                Some(AssetIcon::WindowClose),
                th.danger,
                gpui_kit::WindowControlArea::Close,
            ))
            .into_any_element()
    }
}

/// 一颗窗口控件键：**只有外观与 area，没有点击处理**
/// （理由见 `Shell::window_controls` 的 ①）。
///
/// `id` 固定为 `min` / `max` / `close`，**不能省**：gpui 在 paint 阶段靠节点的
/// id 把它登记进 `window_control_hitboxes`（`window.rs:5130`），
/// `handle_hit_test_msg` 再靠这个 id 判断鼠标落在不在这一区。没有 id，
/// 系统永远收不到 `HTMINBUTTON` / `HTCLOSE`，键就成了一块死区。
///
/// 返回 `Stateful<Div>`（`.id(..)` 的产物）而不是 `Div`：带 `id` 的节点
/// 就是 `Stateful`，签名写 `Div` 会 E0308
///
/// `icon` 为 `None` 时**照旧占满宽度**（系统不支持时官方也是这么留位的），
/// 宽度是系统约定，不能因为不画图标就缩
fn window_control_button(
    id: &'static str,
    icon: Option<AssetIcon>,
    hover_bg: gpui_kit::Hsla,
    area: gpui_kit::WindowControlArea,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    use gpui_kit::component::{Icon, Sizable as _};
    div()
        .id(id)
        .w(px(TITLE_BAR_CTRL_W))
        .h_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .window_control_area(area)
        .when_some(icon, |this, name| this.child(Icon::new(name).small()))
        .hover(move |style| style.bg(hover_bg))
}

impl Shell {
    /// 折叠 / 展开左栏（`Ctrl B` 与标题栏那颗键共这一个入口）。
    /// 仓库页折的是仓库侧栏——折叠态与宽度都落盘，下次启动回到用户自己摆的样子；
    /// CLI 屏折的是它自己的工具列表（收起态两屏各存一份、不落盘）
    pub(crate) fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == Page::CliTools {
            self.cli_collapsed = !self.cli_collapsed;
            cx.notify();
            return;
        }
        if self.page != Page::Repos {
            // 总览页没有侧栏。按 `Ctrl B` 却什么都不发生会像坏了，
            // 不如把它当成"我要看仓库"——切回仓库页并展开
            self.go(Nav::Group(self.side_focus), window, cx);
            return;
        }
        self.sidebar_collapsed = !self.sidebar_collapsed;
        self.persist_sidebar_layout();
        cx.notify();
    }

    /// 拖宽结束后把新宽度记下来。不记的话折叠再展开会弹回旧宽度，
    /// 用户会以为拖拽没生效
    fn commit_sidebar_width(&mut self, w: Pixels, cx: &mut Context<Self>) {
        // core 那一道管的是"落盘值别离谱"，GUI 这一道管的是"底部两键别被挤穿"
        let w = qingbird_core::store::clamp_sidebar_w(w.as_f32())
            .max(sidebar_min_w(fs_sec()));
        if (w - self.sidebar_w).abs() < f32::EPSILON {
            return;
        }
        self.sidebar_w = w;
        // **必须换弹簧的 id 代次**（2026-10-05 实机：「侧栏在拉大拉小时会弹簧一下」）。
        // 不换的话弹簧的内部位置还停在拖之前的宽度，松手这一刻它会从旧宽度
        // 起步往新宽度弹，而 `resize_panel` 每帧把面板按在弹簧值上——
        // 面板被**拽回去再弹过来**。整段回抽稳定复现，不是偶发抖动。
        // 换 id = 换一份初值 `position = target` 的全新状态，就地静止。
        // 详见 `state.rs::sidebar_gen`
        self.sidebar_gen = self.sidebar_gen.wrapping_add(1);
        self.persist_sidebar_layout();
        cx.notify();
    }

    /// 落盘失败不弹窗：侧栏宽度存不住，下次开还是这个宽度，
    /// 用户自己再拖一次就行，不值得打断他
    fn persist_sidebar_layout(&self) {
        let _ = qingbird_core::store::set_sidebar_layout(self.sidebar_w, self.sidebar_collapsed);
    }

    /// 拖宽检视列结束后把新宽度记下来。理由与 `commit_sidebar_width` 相同：
    /// 不记的话收起再展开会弹回旧宽度，用户会以为拖拽没生效
    pub(crate) fn commit_inspector_width(&mut self, w: Pixels, cx: &mut Context<Self>) {
        let w = qingbird_core::store::clamp_inspector_w(w.as_f32());
        if (w - self.inspector_w).abs() < f32::EPSILON {
            return;
        }
        self.inspector_w = w;
        // **与 `commit_sidebar_width` 同一件事**（2026-10-05 实机：左栏修完，
        // 用户立刻报「右栏还是会弹簧一下」——同一个 bug 的第二份拷贝）。
        // 不换弹簧 id 的后果一样：面板被拽回旧宽度再弹过来，每次拖拽必现
        self.insp_gen = self.insp_gen.wrapping_add(1);
        self.persist_inspector_layout();
        cx.notify();
    }

    /// 落盘检视列版式（宽度 + 展开态）。与 `persist_sidebar_layout` 一对
    pub(crate) fn persist_inspector_layout(&self) {
        let _ = qingbird_core::store::set_inspector_layout(self.inspector_w, self.inspector_open);
    }

    /// 把「中间那一格」的宽度偏好交回给 flex——**两栏抢宽的根就治在这一句**。
    ///
    /// 基座在容器宽度变化时（拖窗口 / 挂上或摘掉一栏）会把这一组里**每一格**按
    /// 各自的比例重分一次（`resizable/mod.rs:353 adjust_to_container_size`）。
    /// 它的早退条件写在原文里：
    /// 「A panel with no size preference is laid out by flex …
    /// **Flex already fits the container, so there is nothing here to adjust.**」
    /// ⇒ 这一组里**只要有一格没有宽度偏好**，整组就不做比例重分。
    ///
    /// 这正是这里要的：两栏的宽是用户设的数（`sidebar_w` / `inspector_w`），
    /// 窗口变宽变窄都该由中间那一列吸收。不清的话每开合一次、每拖一次窗口，
    /// 两栏就按百分比漂一次——帧上量到过 `split = [630, 282]`，
    /// 左栏 630 而用户设的是 **263**。
    ///
    /// `reset_panel` 只清「偏好」，`sizes` 里那一格保留原值
    /// （基座文档原话 "while preserving its current size"），所以拖拽把手不受影响：
    /// 拖的时候基座算出来的中间格宽度 = 容器 − 侧栏，**与 flex 自己算的那个值同一个数**，
    /// 清不清都在同一处（阶段 31.4 补二十五帧上拖过侧栏把手，跟随正常）。
    ///
    /// ⚠️ **必须每帧清，不能只在挂载那一帧清一次**：`update_panel_size` 会在
    /// 每一格被量出来时把偏好写回去（`mod.rs:211-214`），挂载那一帧清掉、
    /// 下一帧 prepaint 又给填回去了。
    ///
    /// 越界不 panic：`reset_panel` 内部是 `self.panels[ix] = ..` 直接下标，
    /// 而侧栏收起那一帧这一组只有一格，所以先判长度
    fn keep_main_flexible(
        state: &mut ResizableState,
        flex_ix: usize,
        cx: &mut Context<ResizableState>,
    ) {
        if state.sizes().len() > flex_ix {
            state.reset_panel(flex_ix, cx);
        }
    }

    /// 两栏（或多一栏）展开时把窗口**向右撑大**，让中间那一列不低于 `MAIN_MIN_W`。
    ///
    /// 用户 2026-10-09 的令：「检测中间最小宽度，小于 870 就自动向右将窗口撑大展开」。
    /// ⚠️ **下限取的是 950 不是 870**：870 那一档中间列照样把右边那排键切掉，
    /// 是帧上一档 20px 压出来的（948 干净 / 928 开切），推导与表都在 `tokens::MAIN_MIN_W`。
    /// 三条守则各挡一种把这条规则用坏的方式：
    /// ① **目标宽只由算式给**：撑到哪 = `required_shell_w(开没开, 意图宽)`，
    ///    **不是**"当前宽 + 缺口"再迭代一次。当前宽只用来判"够不够"，
    ///    不参与算目标——一旦让它参与就是正反馈环：撑一次算一次、
    ///    每次又想再撑一点，窗口会自己往右爬；
    /// ② **一次只发一个目标**（`shell_grow_pending`）：`Window::resize` 在 Windows 侧
    ///    是 spawn 出去的 `SetWindowPos`（`gpui-pre-windows/window.rs:652-674`），
    ///    生效前一两帧 `window.bounds()` 读到的还是旧宽，不记目标就会每帧重发、
    ///    `shell_grown_w` 被重复累加，收起一栏时把窗口减得比原来还窄；
    /// ③ **只还我们自己加的那一段**（`shell_grown_w`），且**下限钉在当前需求上**——
    ///    还回去之后中间列照样 ≥ 870，绝不会因为"还原"把内容压窄。
    ///    ⚠️ 已知的一处不完美：撑宽之后用户自己又拖过窗口，这里减掉的就不正好是
    ///    我们加的那一段；因为下限钉在需求上，最坏是"回到刚好放得下的宽"，
    ///    不会出现内容被压。真要精确得再记一格"撑之前的宽"，为这一处边缘情况
    ///    多一个字段不值当，先记在这儿。
    fn ensure_shell_width(
        &mut self,
        side_on: bool,
        insp_on: bool,
        window: &mut Window,
    ) {
        let required = required_shell_w(side_on, self.sidebar_w, insp_on, self.inspector_w);
        let cur_w = window.bounds().size.width.as_f32();
        let cur_h = window.bounds().size.height.as_f32();

        // `shell_grow_pending` 的语义是"欠着一次还没生效的窗口宽请求"（撑宽与收窄都算）。
        // **兑现了就清**：不清的话"同一个目标只发一次"那条守则会把下一次该发的挡掉——
        // 最典型的是用户自己把窗口拖窄之后我们该再撑回去，却被上一次的目标挡住
        if (cur_w - self.shell_grow_pending).abs() < 1.5 {
            self.shell_grow_pending = 0.0;
        }

        if cur_w + 0.5 >= required {
            // 够宽（这一支同时管"这一屏压根不该为两栏撑窗口"：总览页与两栏都收时
            // 需求 919，本来就落在窗口下限以内）。这里必须**顺手把撑出去的那一段还回去**。
            // 第一版把还原只写在"需求 ≤ 窗口下限"那一支里，于是收起抽屉时需求是 1182
            // （仍然 > 1080）⇒ 那一支根本不进 ⇒ **窗口停在撑过的 1502 不回**，
            // 症状是"抽屉关了右边多一条空"。判据②就是 `target/verify_grow.py` 抓的
            self.give_shell_width_back(cur_w, required, cur_h, window);
            return;
        }
        // ② 同一个目标只发一次（生效之前的那几帧 `cur_w` 还是旧的）
        if (self.shell_grow_pending - required).abs() < 0.5 {
            return;
        }
        // ① 加宽的量按"当前缺口"累计，不按需求累计——用户在中间自己拖过窗口也对得上
        self.shell_grown_w += required - cur_w;
        self.shell_grow_pending = required;
        // 不用 `cx.notify()`：`SetWindowPos` 必带一次 WM_SIZE，窗口自己会再来一帧。
        // 参数里没有 `cx` 就是有意的——这里**不该**手动催帧，催了就是第二条路径
        window.resize(gpui_kit::size(px(required), px(cur_h)));
    }

    /// 把为两栏撑出去的那一段还给窗口。见 `ensure_shell_width` 守则 ③
    fn give_shell_width_back(&mut self, cur_w: f32, required: f32, cur_h: f32, window: &mut Window) {
        if self.shell_grown_w <= 0.0 {
            return;
        }
        // 下限钉在当前需求与窗口下限上：还回去之后中间那一列照样 ≥ 870，
        // 绝不会因为"还原"把内容压窄
        let target = (cur_w - self.shell_grown_w).max(required).max(WIN_MIN_W);
        let give_back = (cur_w - target).max(0.0);
        if give_back <= 0.5 {
            // ⚠️ **一段都还不出去时，一个字段都不许改**，尤其不许把 `shell_grown_w` 清零。
            // 抽屉还开着的那些帧正好是这种帧（需求 == 当前宽）。第一版写成
            // "先清零、再看要不要 resize" ⇒ 等到真收起抽屉，欠着的那一段早被抹平，
            // 窗口永远停在 1502（判据②第一次跑红就是它）
            return;
        }
        if (self.shell_grow_pending - target).abs() < 0.5 {
            return; // 这个收窄目标已经发出去了，正在路上
        }
        // 按**实际还出去的**那一段减，不整个抹平：被需求夹住没还完的留着，
        // 下次需求再降还能接着还
        self.shell_grown_w -= give_back;
        self.shell_grow_pending = target;
        window.resize(gpui_kit::size(px(target), px(cur_h)));
    }
}

// ================= 导航轨 =================

/// 导航轨按钮里的图标。
///
/// **为什么不给它写尺寸。** 官方 `Button` 会用按钮档反推图标档再覆盖一遍
/// （`button.rs:756-761`：`icon.with_size(icon_size)`），而 `icon_size`
/// 只在 `content_style(..)` 那条路上能被外面塞进去，公开 API 给不了。
/// 所以这里传进去的任何 `Size` 都会被丢弃——**写了不生效，
/// 留着就是一句会骗人的话**（AGENTS.md「基座的 builder 方法」那条）。
/// 真正决定图标多大的是按钮档 `Size::Large`，见 `nav_item` 的注释。
fn nav_icon(name: AssetIcon) -> gpui_kit::component::Icon {
    gpui_kit::component::Icon::new(name)
}

impl Shell {
    /// 只有三个导航按钮 + 设置（D2：导航轨只回答"我在看哪一屏"）。
    /// 添加 / 扫描这类功能入口不往这儿塞——功能入口塞进导航轨是这版界面
    /// 上一轮最大的问题：轨上一排图标分不清哪些是切屏、哪些是动作。
    ///
    /// 轨宽与内边距是**三档**（D58 / D59）：横向 `NAV_PAD` 被"装得下按钮"钉死、
    /// 顶部 `NAV_PAD_TOP` 是一条对齐基线、底部 `NAV_PAD_BOTTOM` 是轨末端的余量。
    /// 三者**不能共用一个数**——共用过一次（6），实机看着"logo 和三颗图标糊成一片"；
    /// 只修顶部又漏了底部，齿轮直接坐在状态条上沿。`tokens::tests` 钉住了这条。
    pub(crate) fn nav_rail(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        debug_assert_eq!(
            NAV_W - 2.0 * NAV_PAD,
            32.0,
            "导航轨的内容盒（{NAV_W} − 2×{NAV_PAD}）必须装得下 ghost 图标按钮并居中；\
             实测按钮渲染成 26px、内容盒 32px，左右各多出 3px"
        );
        v_flex()
            .w(px(NAV_W))
            .h_full()
            .flex_shrink_0()
            .bg(t.colors.background)
            // 同灰之后轨与主区没有颜色差了，分界只剩这一条发丝线
            // （`track` = 卡间发丝线的同一个出处）
            .border_r_1()
            .border_color(track(t))
            .items_center()
            .px(px(NAV_PAD))
            .pt(px(NAV_PAD_TOP))
            .pb(px(NAV_PAD_BOTTOM))
            // ⚠️ 这一档**不许顺手收到 4**：`GAP_DENSE`(2) 治的是一串同类**行**
            // 的呼吸量（列表行、色块分段），导航轨上是**三个各自独立的按钮**——
            // 各有各的命中区、各有各的选中底块，贴着排读起来是"糊成一片"，
            // 实机按 4px 排出来用户就说"快粘到一起了"
            .gap(px(GAP_LG))
            .child(self.nav_item(AssetIcon::LayoutDashboard, Nav::Overview, "总览", t, cx))
            .child(self.nav_item(
                AssetIcon::GitBranch,
                Nav::Group(Group::Mine),
                "我的仓库",
                t,
                cx,
            ))
            .child(self.nav_item(
                AssetIcon::Users,
                Nav::Group(Group::Third),
                "第三方仓库",
                t,
                cx,
            ))
            // 第 4 个入口：CLI 工具屏（阶段一新增）。与前两颗不同——
            // 它真的切屏，不是"驱动侧栏"那一路
            .child(self.nav_item(AssetIcon::Terminal, Nav::CliTools, "CLI 工具", t, cx))
            // 设置键**沉在轨底**（2026-10-08 用户令：「左导航条 设置 按钮恢复到左下角」）。
            // 阶段 31.4 一度把它提到图标组下面 + 补一条发丝线分组，那条改动当轮就作废了：
            // 分组线用的是 `track`，与画布只差 1/255 **根本看不见**，等于把设置键白挪了一格。
            // ⚠️ `flex_1` 这颗垫圈必须在设置键**前面**，整组才顶对齐、设置键才真沉底
            .child(div().flex_1())
            .child(
                ghost_btn(
                    "nav-settings",
                    AssetIcon::Settings,
                    tier3(t),
                    Size::Large,
                    "设置",
                    cx,
                    cx.listener(|this, _, window, cx| this.open_settings_dlg(window, cx)),
                )
                .into_any_element(),
            )
    }

    /// 导航按钮。选中态仍交给官方 `Button::selected`（它管焦点环、a11y 与
    /// 「打开态也画成选中」这条语义），但**底色由我们自己给**（D60）。
    ///
    /// ⚠️ 官方 ghost 选中的底是 `button.active.background`，而 `theme.json`
    /// 没配这一项 → 落基座兜底 `input.mix_oklab(transparent, 0.7)`，比规范板的
    /// 选中面深一大截。所以用 `tokens::wash` 盖掉。
    /// **能盖掉是因为官方在每个状态层的最后重放调用方样式**
    /// （`button.rs:733`、`:795` 的 `refine_style(&instance_style)`）——
    /// 这条顺序是"编译得过但不写注释就会失效"的那类知识，改基座大版本时先复核它。
    ///
    /// **`when(active, ..)` 不能省**：同一份 `instance_style` 在常态层也会被重放，
    /// 无条件 `.bg()` 会让三颗按钮统统带上底。
    ///
    /// **图标尺寸为什么是 `Size::Large`。** 官方 `Button` 的图标档是从按钮档
    /// 推出来的（`button.rs:615-618`）：`Size::Size(v)` → `v × 0.75`，
    /// 其余档原样传给图标。而 icon-only 按钮的外框是
    /// `XSmall=20 / Small=24 / Medium=Large=32`（`button.rs:661-666`）——
    /// **`Large` 的外框和 `Medium` 一样是 32px，图标却从 16px 变 24px**。
    /// 这正是要的那一档：框不变、图标变大，没有第二个档能这么调。
    ///
    /// ⚠️ **不要用 `Size::Size(px(..))` 放大**（AGENTS.md 已记）：那条路会让
    /// 官方把文字也一起放大到 16px、图标放大到 size×0.75，整屏图标过大。
    pub(crate) fn nav_item(
        &self,
        icon: AssetIcon,
        nav: Nav,
        label: &'static str,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = match nav {
            Nav::Overview => self.page == Page::Overview,
            Nav::Group(g) => self.page == Page::Repos && self.side_focus == g,
            Nav::CliTools => self.page == Page::CliTools,
        };
        let ink = if active {
            t.colors.foreground
        } else {
            tier3(t)
        };
        let id: &'static str = match nav {
            Nav::Overview => "nav-overview",
            Nav::Group(Group::Mine) => "nav-mine",
            Nav::Group(Group::Third) => "nav-third",
            Nav::CliTools => "nav-cli",
        };
        Button::new(id)
            .icon(nav_icon(icon).text_color(ink))
            .custom(ghost_variant(cx))
            .selected(active)
            .when(active, |this| this.bg(wash(t)))
            // ⚠️ **不要在这里加 `.hover(..)` 改悬停面**（Q12 已开，见路线表）。
            // 想改也改不动：官方在 `render` 里先注册自己的悬停层（`button.rs:713`），
            // 调用方再来一次会撞上 `div.rs:843` 的 `debug_assert!(hover_style already set)`
            // ——**debug 构建直接 panic**。正解是换 `ButtonVariant::Custom`，
            // 那是重新设计这颗按钮（焦点环 Edge→Inside、描边逻辑都变），不是改个色。
            .with_size(Size::Large)
            .tooltip(label)
            .accessibility_label(label)
            .on_click(cx.listener(move |this, _, window, cx| this.go(nav, window, cx)))
            .into_any_element()
    }
}

impl Shell {
    /// 导航轨点击的唯一入口。第 2、3 个按钮不是"切页面"而是"驱动侧栏"（D2）：
    /// 停在总览时点它们要同时切到仓库页，否则点了没反应会被当成坏了。
    pub(crate) fn go(&mut self, nav: Nav, window: &mut Window, cx: &mut Context<Self>) {
        match nav {
            Nav::Overview => {
                // 只切屏，不动 `sidebar_collapsed`。总览没有侧栏是"那屏没有侧栏"，
                // 不是"用户把侧栏折了"——把两者混成一个值，用户回仓库页会发现
                // 自己刚摆好的侧栏不见了
                self.page = Page::Overview;
                // 活跃度每个仓库一条 `git log`，只在真的进总览时扫
                self.scan_overview_activity(cx);
            }
            Nav::Group(g) => {
                self.page = Page::Repos;
                self.side_focus = g;
                // 点"我的仓库 / 第三方仓库"就是要看仓库，侧栏必须跟着出来。
                // 重新落一次盘：这个展开是用户刚做出的选择
                let was_collapsed = self.sidebar_collapsed;
                self.sidebar_collapsed = false;
                if was_collapsed {
                    self.persist_sidebar_layout();
                }
                // 导航按钮点的是"组"，组开着才看得见里面的仓库。
                // 只切焦点不展开的话，用户会看到侧栏里那组仍折着，像按钮没生效
                let group_was_closed = !self.group_open[g.ix()];
                self.group_open[g.ix()] = true;
                // 已经在这一组里就只切焦点、不动选中：否则点两下会莫名换掉当前仓库
                if !self.selected_in_group(g) {
                    match self.first_in_group(g) {
                        Some(path) => {
                            self.reveal = Some(path.clone());
                            self.select(path, window, cx);
                        }
                        // 这一组一个仓库都没有：清选中，主区给空态。
                        // 留着另一组的选中会让导航高亮与主区内容对不上
                        None => self.select_none(),
                    }
                } else if group_was_closed {
                    // 刚把这一组展开：把它选中的那行滚到眼前，
                    // 否则展开完视线还停在组标题上，不知道自己落到了哪儿
                    self.reveal = self.selected.clone();
                }
            }
            Nav::CliTools => {
                // 真·切屏。CLI 屏既没有仓库侧栏、也没有"我的/第三方"焦点，
                // 所以不动 `side_focus`、不展开任何组
                self.page = Page::CliTools;
                // 进这一屏就扫一遍（与"进总览扫活跃度"同一套路数）。
                // 扫描要跑几条外部命令，走后台；界面先拿缓存画
                self.scan_cli(cx);
            }
        }
        cx.notify();
    }

    /// 清掉选中（分组为空时用）。不复用 `select`：那个函数要落盘 touch 记录，
    /// 空分组没有仓库可 touch
    pub(crate) fn select_none(&mut self) {
        self.selected = None;
        self.fork = None;
        self.fork_backup = None;
        self.fork_diff = false;
        self.fork_confirm = false;
    }
}

impl Shell {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let repos = repos::list_repos();
        // 自动识别只读文件、不派生 git 子进程，放启动路径上不卡界面
        let mut views = HashMap::new();
        for r in &repos {
            views.insert(
                r.path.clone(),
                RepoView {
                    detected: run_cmd::detect_cmds(&r.path),
                    ..RepoView::blank()
                },
            );
        }
        let selected = repos.first().map(|r| r.path.clone());
        let side_filter = cx.new(|cx| InputState::new(window, cx).placeholder("筛选名称或分支"));
        // 总览表格：官方 DataTable。委托只带一个回指 `Shell` 的 Entity，
        // 数据在 `Shell::ov_rows`——每帧由 `sync_ov_rows` 压一次
        let ov_table = {
            let this = cx.entity();
            cx.new(|cx| TableState::new(crate::overview::OvDelegate::new(this), window, cx))
        };
        // 提交列表：官方 List。和总览表格一样，委托只带一个回指 `Shell` 的
        // Entity，数据在 `Shell::history`——不存第二份，两处必然会对不上
        let history_table = {
            let this = cx.entity();
            cx.new(|cx| {
                gpui_kit::component::list::ListState::new(
                    crate::history_tab::CommitDelegate::new(this),
                    window,
                    cx,
                )
            })
        };
        // 命令面板：官方 CommandState。查询词、高亮项、焦点都在它身上，
        // 整个应用只建这一个——重建等于每次打开都把上次搜到哪儿扔掉
        let palette = { cx.new(|cx| gpui_kit::component::command::CommandState::new(window, cx)) };
        // 侧栏版式跟着用户走。启动首屏是总览（D1），那屏不显示侧栏，
        // 但用户的折叠选择要留着——回仓库页时回到他上次摆的样子
        let saved = qingbird_core::store::load().settings;
        // 同步设置草稿要在结构体字面量之外建：它内部要 `cx.new` 建一个
        // InputState 的 Entity，放字面量里会让 window 的借用拖到函数尾
        let sync_cfg = sync_cfg_from(None, window, cx);
        let mut this = Self {
            win: window.window_handle(),
            // 壳层根节点自己的焦点句柄。**必须真的去聚焦**（下面 focus 那一行），
            // 光建不聚焦等于没建：Action 派发是从焦点节点往上冒泡的，
            // 一个焦点都没有时落到的是 dispatch tree 的根节点（gpui-kit 那层
            // `Root`），壳层的监听器在更深一层，一次都收不到。
            // 详见 `state.rs` 里 `focus` 字段的注释
            focus: cx.focus_handle(),
            page: Page::Overview,
            side_focus: Group::Mine,
            sidebar_collapsed: saved.sidebar_collapsed,
            sidebar_w: qingbird_core::store::clamp_sidebar_w(saved.sidebar_w)
                // 老配置里可能存着旧下限 200（那一档底部两键会穿出去），
                // 启动就按当前字号档抬到该在的位置，与 `commit_sidebar_width` 同一道夹
                .max(sidebar_min_w(fs_sec())),
            // 启动这一屏是总览页，两栏都没挂 ⇒ 既没撑过窗口，也没有欠着的加宽目标
            shell_grown_w: 0.0,
            shell_grow_pending: 0.0,
            // 弹簧 id 的后缀。启动这一份状态的目标就是 `sidebar_w`，本来就是静止的，
            // 所以 0 就对（见 `state.rs` 里 `sidebar_gen` 的注释）
            sidebar_gen: 0,
            dragging: false,
            split: cx.new(|_| ResizableState::default()),
            // 右栏两件都从配置读，与侧栏同一口径：宽度夹过、展开态是用户自己的选择。
            // 默认展开（core 的 `inspector_open` 是 `default_true`）——
            // 收起的右栏只有仓库头那颗按钮能叫回来，默认为收等于新装用户看不到这列
            inspector_open: saved.inspector_open,
            inspector_w: qingbird_core::store::clamp_inspector_w(saved.inspector_w),
            // 与 `sidebar_gen` 同理：启动这一份的目标就是 `inspector_w`，静止的，0 就对。
            // **两个弹簧的 id 必须各自独立**（见 `state.rs::insp_gen`）
            insp_gen: 0,
            // 模块可见性（阶段 19）：同样是"启动读一次、设置里改完写回来"。
            // **不存合并后的结果**——归属基线（第三方天生没有「空间」与分叉条）
            // 每次问的时候现并，免得基线改了就有一份旧的躺在内存里
            hidden_mine: saved.hidden_modules_mine.clone(),
            hidden_third: saved.hidden_modules_third.clone(),
            insp_split: cx.new(|_| ResizableState::default()),
            copied: None,
            copied_until: 0,
            addr_hover: None,
            readme_state: cx.new(|cx| TextViewState::markdown("", cx)),
            readme_task: None,
            repos,
            views,
            selected,
            side_filter,
            filter_open: false,
            // 两组默认都展开：仓库一般不多，展开着省一次点击；
            // 真嫌挤可以自己点标题折起来
            group_open: [true, true],
            tree_scroll: ScrollHandle::new(),
            space_scroll: ScrollHandle::new(),
            log_scroll: ScrollHandle::new(),
            batch_log_scroll: ScrollHandle::new(),
            reveal: None,
            pending_reveal: None,
            pending_folder: None,
            pending_batch_note: None,
            pending_cli_dlg_close: false,
            pending_cli_note: None,
            // 关窗那三档**只在构造期读一次**：`on_window_should_close` 的回调
            // 每次点 X 都会跑，而那个回调在消息处理里，多一次 `store::load()`
            // 就是一次同步读盘。改档的地方（面板 / 设置）会同步更新这个字段
            close_action: store::close_action_of(&store::load().settings.close_action),
            pending_close_prompt: false,
            close_remember: false,
            pending_app_quit: false,
            scanning: false,
            last_scan_ts: 0,
            scan: None,
            seq: 0,
            logs: HashMap::new(),
            op_logs: HashMap::new(),
            detail_tab: 0,
            space: SpaceState::default(),
            space_seq: 0,
            space_task: None,
            profile: ProfileState::default(),
            profile_seq: 0,
            profile_task: None,
            cli: CliState::default(),
            cli_seq: 0,
            cli_task: None,
            cli_size_task: None,
            // 左栏默认展开，与仓库侧栏同一档：收起的左栏是一屏的读法，
            // 不是偏好，所以不进配置、每次进屏都是展开的
            cli_collapsed: false,
            history: HistoryState::default(),
            history_table,
            ov_table,
            ov_rows: Vec::new(),
            ov_sort: crate::state::OvSort::LastUsed,
            // ⚠️ `None` 的意思是**还没读过这张表 / 读不出来**，与"读出来是空的"
            // 是两件事（判据见 `Shell::last_sync` 那段）。启动到这里为止那颗片是
            // 禁用的 ⇒ 不会出现"刚打开就把整张表说成全都久未同步"那一格假话
            last_sync: None,
            ov_stale_only: false,
            ov_stale_counts: (0, 0),
            ov_activity: Vec::new(),
            ov_seq: 0,
            ov_task: None,
            ov_need_scan: true,
            batch: None,
            app_log: LogState::default(),
            dlg: None,
            fork: None,
            fork_backup: None,
            fork_diff: false,
            fork_confirm: false,
            commit_dlg: None,
            clean_dlg: None,
            purge_dlg: None,
            stash_dlg: None,
            clone_dlg: None,
            cmd_dlg: None,
            settings_dlg: None,
            cli_audit: None,
            cli_clean_dlg: None,
            cli_log: LogState::default(),
            cli_log_scroll: ScrollHandle::new(),
            cli_upgrade: None,
            upd: Default::default(),
            sync_cfg,
            palette,
            palette_actions: Vec::new(),
            palette_open: false,
        };
        // 关窗询问（阶段 22）。**这个钩子在 Windows 上是真的**——
        // 详见 `dialogs/close_prompt.rs` 模块头那段更正：
        // `window.rs:6658` 挂上 → Windows 消息泵 `events.rs:110` 的
        // `WM_CLOSE => self.handle_close_msg()` 调它 → `events.rs:336-341`
        // 返回 `false` 时回 `Some(0)`，DefWindowProc 不跑，窗口就不关。
        //
        // **回调里只判档 + 置标记，绝不拉浮层**：它跑在 Win32 的消息处理里，
        // 而弹窗必须能在用户点完那颗键之后继续活。
        //
        // 三档（`store::close_action_of`，默认 `Ask`）：
        // ① `Tray` → 返回 `true` 放行，系统照旧销毁窗口、进程留着（今天的行为）
        // ② `Ask` → 返回 `false` **这一次**关不掉，置标记，下一帧弹面板
        // ③ `Exit` → 也返回 `false` + 置 `pending_app_quit`：
        //    **不能放行**——放行只是销毁窗口，`QuitMode::Explicit` 下
        //    进程会留着，那就成了"点了完全退出却还在托盘里"的那种幽灵
        window.on_window_should_close(cx, {
            let this = cx.entity();
            move |_window, cx| {
                let mut allow = false;
                this.update(cx, |this, cx| match this.close_action {
                    CloseAction::Tray => allow = true,
                    // ⚠️ **置了标记就必须 `cx.notify()`**（2026-10-05 实机撞出来的）：
                    // `Entity::update` **不会**自动安排下一帧。少了它，标记躺在
                    // 字段里没人消费——表现是 `exit` 档点了 X 既不关窗也不退出
                    // （`WM_CLOSE` 已被这次返回的 `false` 挡住了），
                    // 而 `cargo check` 零错、单测全绿、**抓帧完全正常**。
                    // 与 `pending_folder` / `pending_reveal` 同一条纪律
                    CloseAction::Ask => {
                        this.pending_close_prompt = true;
                        cx.notify();
                    }
                    CloseAction::Exit => {
                        this.pending_app_quit = true;
                        cx.notify();
                    }
                });
                allow
            }
        });
        // 全部走官方 action + key binding，不自己 `on_key_down` 判键：
        // 自己判的那份不知道当前焦点在输入框还是按钮上，会在打字时把侧栏折了。
        // `context` 传 None = 全局生效；命令面板自己那个 Dialog 上下文里
        // ↑↓ 与回车由 `command::init` 绑，不在这里重复占键
        //
        // ⚠️ **绑定由 `keys.rs` 那张表生成**（阶段 30）。早先这里是 11 行
        // `KeyBinding::new` 写死，设置页要列出/改/还原它们时就必须有一份能枚举的
        // 数据 ⇒ 抽成表。表是唯一真源，**别在这里再补一条绑定**——补的那条不进表，
        // 设置里就没有它的入口，用户也改不了它（而编译器不会报）。
        //
        // 用户改过键的那几项，`keys::bindings` 会先给旧键补一条 `NoAction`
        // 再注册新键，所以这里读一遍配置就够，**不要在别处重复覆盖**
        let overrides = qingbird_core::store::load().settings.keybindings;
        cx.bind_keys(crate::keys::bindings(&overrides));
        // 过滤框每敲一个字都要重画侧栏。官方 Input 的 value() 是纯读取，
        // 不订阅就永远停在旧值上——这是"输入框接了但过滤不生效"的典型原因
        cx.subscribe(&this.side_filter, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        // Action 监听器不在这里挂，见 impl Shell::bind_actions 的注释：
        // `Window::on_action` 只允许在 paint 阶段调用，而构造期不是 paint。
        // 快捷键的 `bind_keys` 可以在构造时挂（全局键位表，不挑阶段），
        // 但收到 Action 之后要做什么这一半必须每帧重挂
        //
        // 点总览表格的一行 = 切仓库页 + 展开侧栏 + 展开它那组 + 滚到那一行。
        // 与侧栏行点击、右键「打开」、⌘K 切仓库共用 `reveal_repo`。
        // 订阅回调的第 1 个参数就是 `&mut Shell`，第 4 个是 `&mut Context<Shell>`——
        // 所以这里能直接改状态，不必绕回 `Entity`
        //
        // ⚠️ **只能置一次性标记，不能在这里直接调 `reveal_repo`。**
        // 早先的写法是 `cx.update_window(win, |view, window, cx| view.downcast::<Shell>())`，
        // 它静默失败、点行不跳转：`App::update_window_id` 靠
        // `cx.windows.get_mut(id).and_then(Option::take)` 取窗口（app.rs:2888），
        // 而这个订阅恰恰跑在**该窗口自己的事件处理里**——窗口此刻已经被 take 走了，
        // 于是 `update(None)` 把整个闭包丢掉，返回 `Err("window not found")`，
        // 外面那个 `.ok()` 又把错误吞了。三处都不出声，表现为"行选中了但没反应"。
        //
        // `reveal_repo` 要 `&mut Window`，而订阅回调拿不到；`render` 里拿得到。
        // 所以置 `pending_reveal`，下一帧由 `render` 兑现——与克隆成功后那条路同一条
        cx.subscribe(&this.ov_table, |this, _t, event: &TableEvent, cx| {
            if let TableEvent::SelectRow(ix) = event {
                if let Some(path) = this.ov_rows.get(*ix).map(|r| r.path.clone()) {
                    this.pending_reveal = Some(path);
                    cx.notify();
                }
            }
        })
        .detach();
        // 目录树**没有**官方 TreeState 可挂：懒加载的文件夹在扫之前
        // `is_folder()` 恒为 false，基座点不开（理由与查证见 `state.rs::TreeNode`）
        // **先把缓存灌满**（阶段 16）：侧栏徽标、总览统计一上来就有内容。
        // 然后只对当前选中的那个仓库真去问 git——打开 Locowrk 那种 98 GB 的
        // 仓库时，靠缓存页面立刻稳，那个仓库自己在后台刷。
        //
        // ⚠️ 三个后台扫描都**先灌缓存再刷**（见 `ops.rs` 里的两处
        // `scan_cache::load_cache`）。所以这里照常起任务是安全的：
        // 库里有数就先摆数、库里没数才出骨架屏，两条路都试过了。
        // 17 个仓库全刷一遍才是"启动慢"，所以只刷选中的这一个
        // （用户 2026-10-05 拍板的这一档）
        this.load_caches(cx);
        if let Some(sel) = this.selected.clone() {
            this.refresh_one(sel.clone(), cx);
            this.scan_space(cx);
            this.scan_profile(cx);
            // 落盘的操作历史：**只读选中的这一个**（17 个仓库全读一遍
            // 是启动时白开 17 次 SQLite 连接，而界面一次只显示一条）。
            // 切仓库时 `select` 会补上那一个
            this.load_op_log(&sel);
        }
        // 启动时的选中项是构造时就定下的，**没有走过 `select`**，所以这条链上
        // 灌地址条与同步设置草稿的那两步得在这里自己补。不补的话第一屏的
        // 地址条停在占位符上、「同步设置」页签显示的是上一份（其实是空）草稿，
        // 而用户完全没做过任何操作就会以为是坏了
        this.load_sync_cfg(window, cx);
        // 启动就把焦点落在壳层根节点上。理由见 `state.rs` 里 `focus` 字段的注释：
        // Action 派发走「焦点节点 → 逐级上冒」，没有焦点时落到的是 dispatch tree
        // 的根节点（gpui-kit 包的那层 `Root`），壳层挂在更深处的监听器收不到任何派发
        this.focus.focus(window, cx);
        this
    }
}

impl Shell {
    /// 抽屉开关。标题栏的按钮、面板头那颗折叠键与 `Ctrl I` 共这一个入口。
    /// 展开态落盘，理由与侧栏一致
    pub(crate) fn toggle_inspector(&mut self, cx: &mut Context<Self>) {
        self.inspector_open = !self.inspector_open;
        self.persist_inspector_layout();
        cx.notify();
    }
}

// ================= 快捷键的动作实现 =================
//
// 四个 `1·2·3` / `Ctrl T` 都只是**判一下这一屏有没有这个对象**，
// 然后交给主面板那几个按钮已经在用的函数——快捷键不该是"第二份拉取逻辑"
// （上一版的前端就因为按钮与快捷键各写一份而漂过行为）

impl Shell {
    /// 同步卡的三个键。`1·2·3` 与 ⌘K「当前仓库」那三条都走这里，
    /// 再往里转发到 `run_sync`——执行层只有一个入口，
    /// 快捷键与命令面板都不许是第二份拉取逻辑
    pub(crate) fn hotkey_sync(&mut self, op: Op, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != Page::Repos {
            return;
        }
        if self.selected.is_none() {
            window.push_notification("先选一个仓库", cx);
            return;
        }
        // 第三方那把「推送」键在界面上就是一块不可点的占位（设计 3.5 硬边界）。
        // 快捷键不能比界面多给一把——说一声比悄悄跑掉强
        if op == Op::Push && self.selected_is_third() {
            window.push_notification("第三方仓库不能推送", cx);
            return;
        }
        self.run_sync(op, cx);
    }

    /// `Ctrl T`。与仓库头那颗终端图标、⌘K「在终端中打开」、右键菜单同一条路
    fn hotkey_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != Page::Repos {
            return;
        }
        let Some(path) = self.selected.clone() else {
            window.push_notification("先选一个仓库", cx);
            return;
        };
        if let Err(e) = crate::os::open_terminal(&path) {
            window.push_notification(e, cx);
        }
    }

    /// `↑↓`：上下切仓库（到头就停，不绕回）。
    ///
    /// **两屏都生效**，但候选集不同：仓库页是"当前这一组、且没被过滤掉"的
    /// 侧栏行，总览页是表格里那几行（那一屏没有侧栏，表格就是"当前看谁"）。
    /// 两边最后都走 `reveal_repo`——切页、展开侧栏、展开那组、滚到那一行，
    /// 与点侧栏行、点总览行、⌘K 切仓库是同一个入口
    fn move_repo(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        // CLI 屏不是仓库列表，↑↓ 在这里没有对象可移（阶段一不做屏内选择键）。
        // 不拦的话 `else` 那支会拿仓库列表去切选中，人眼在 CLI 屏上"点了没反应"
        // 而侧栏里的选中偷偷换了
        if self.page == Page::CliTools {
            return;
        }
        let list: Vec<String> = if self.page == Page::Overview {
            self.ov_rows.iter().map(|r| r.path.clone()).collect()
        } else {
            let filter = self.filter_text(cx);
            self.repos
                .iter()
                .filter(|r| r.is_third_party == self.side_focus.is_third())
                .filter(|r| crate::sidebar::matches_filter(r, &self.status_of(&r.path), &filter))
                .map(|r| r.path.clone())
                .collect()
        };
        if list.is_empty() {
            return;
        }
        let cur = self
            .selected
            .as_deref()
            .and_then(|p| list.iter().position(|x| x == p));
        let next = step_repo_index(cur, delta, list.len());
        self.reveal_repo(list[next].clone(), window, cx);
    }


    /// `Esc`。**只收本壳自己画的那一样**——侧栏筛选框。
    /// 对话框 / Sheet / 右键菜单的 Esc 都被官方那几层先吃掉了，
    /// 轮不到这里；真要在这里 `close_dialog` 反而会把用户的对话框抢关掉
    fn dismiss_top(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.filter_open {
            return;
        }
        self.filter_open = false;
        let sf = self.side_filter.clone();
        let w: &mut Window = window;
        sf.update(cx, move |st, cx| st.set_value(String::new(), w, cx));
        cx.notify();
    }
}

impl Shell {
    pub(crate) fn entry(&self, path: &str) -> Option<&RepoEntry> {
        self.repos.iter().find(|r| r.path == path)
    }
}

impl Shell {
    pub(crate) fn view(&self, path: &str) -> Option<&RepoView> {
        self.views.get(path)
    }

    /// 这一台仓库的 git 状态**问过没有**。
    ///
    /// 这是「我还没问过 git」与「我问过了、这个文件夹不是仓库」唯一的分界，
    /// 而 `RepoStatus` 自己答不出来（两条都是 `is_repo: false`）——
    /// 判据见 `RepoView::has_status`。**同步卡主句与头卡「云端」那一格都读它**，
    /// 别在那两处各写一遍 `view(path).map(..)`：判两遍迟早漂。
    pub(crate) fn status_read(&self, path: &str) -> bool {
        self.view(path)
            .map(|v| v.has_status)
            .unwrap_or(false)
    }
}

impl Shell {
    /// 当前仓库的显示名：有自定义名用自定义名
    pub(crate) fn title(&self, path: &str) -> String {
        self.entry(path)
            .map(qingbird_core::scheduler::display_name)
            .unwrap_or_default()
    }
}

impl Shell {
    /// 某槽位实际要跑的命令：仓库里存了就用存的，没存就用自动识别的
    /// （与旧版一致：识别结果只预填，不落盘）
    pub(crate) fn effective_cmd(&self, path: &str, slot: Slot) -> String {
        if let Some(e) = self.entry(path) {
            let saved = slot.cmd(e).trim();
            if !saved.is_empty() {
                return saved.to_string();
            }
        }
        let Some(v) = self.view(path) else {
            return String::new();
        };
        let d = &v.detected;
        match slot {
            Slot::Test => d.test_cmd.clone(),
            Slot::Build => d.build_cmd.clone(),
            Slot::Dev => d.dev_cmd.clone(),
            Slot::Install => d.install_cmd.clone(),
            Slot::Pinned(_) => String::new(),
        }
    }
}

impl Shell {
    // ---------- 状态扫描 ----------

    /// 重扫所有仓库的 git 状态。每个仓库要派生 4~5 条 git 命令，串起来好几秒，
    /// 所以整个扫描丢进后台线程池，仓库之间并发，界面不等着。
    ///
    /// **只在"用户主动要求扫全部"时用**（⌘K 重扫、状态条点一下、托盘扫描全部、
    /// 批量同步跑完）。启动与切仓库走 [`Self::refresh_one`]——否则打开
    /// Locowrk 那种 98 GB 的仓库就要等它重扫一遍，页面稳不下来。
    pub(crate) fn refresh_status(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<String> = self.repos.iter().map(|r| r.path.clone()).collect();
        self.refresh_many(paths, true, cx);
    }

    /// 只重扫**一个**仓库。切仓库、单个操作跑完都走这里。
    ///
    /// ⚠️ **不要设 `self.scanning`**：那个标志会让状态条显示"尚未扫描"，
    /// 而一个仓库的后台刷新**不该让整屏看起来在等**。全量 `refresh_many`
    /// 才设。
    pub(crate) fn refresh_one(&mut self, path: String, cx: &mut Context<Self>) {
        if path.is_empty() {
            return;
        }
        self.refresh_many(vec![path], false, cx);
    }

    /// 真正的扫描实现。`all` = 这是不是"扫全部"（决定要不要亮"尚未扫描"）。
    fn refresh_many(&mut self, paths: Vec<String>, all: bool, cx: &mut Context<Self>) {
        self.seq += 1;
        let seq = self.seq;
        // 丢掉上一次的句柄（= 取消），免得两轮结果打架
        self.scan = None;
        if all {
            self.scanning = true;
        }
        cx.notify();

        if paths.is_empty() {
            if all {
                self.scanning = false;
                // 一个仓库都没有时状态条不该一直挂着"尚未扫描"——这轮本身就是成功的空扫描
                self.last_scan_ts = qingbird_core::store::now_secs();
            }
            return;
        }
        let sel = self.selected.clone();
        self.scan = Some(cx.spawn(async move |this, cx| {
            let scan = cx
                .background_executor()
                .spawn(async move {
                    let results: Vec<_> = std::thread::scope(|s| {
                        let handles: Vec<_> = paths
                            .iter()
                            .map(|p| {
                                s.spawn(move || {
                                    // 32-12：**一次扫描、一份输出**。这里以前各叫一次
                                    // `read_status` 与 `changed_entries`，而两者都要问
                                    // `git status`——同一条命令在同一轮里跑两遍。
                                    // `scan_repo` 用一条 `--porcelain=v2 --branch
                                    // --ahead-behind` 同时供出状态与清单，
                                    // 每颗仓库的 git 子进程 7 条 → 3 条
                                    let (st, changes) = repos::scan_repo(p);
                                    (p.clone(), st, changes)
                                })
                            })
                            .collect();
                        handles
                            .into_iter()
                            .map(|h| {
                                h.join().unwrap_or_else(|_| {
                                    (String::new(), RepoStatus::default(), Vec::new())
                                })
                            })
                            .collect::<Vec<_>>()
                    });
                    // 分叉信息只对选中仓库算（几条本地 git 命令），和状态扫描同轮完成；
                    // 没分叉就是 None，分叉卡不出现
                    let fork = sel.as_ref().and_then(|sel_path| {
                        results
                            .iter()
                            .find(|(p, _, _)| p == sel_path)
                            .filter(|(_, st, _)| st.ahead > 0 && st.behind > 0)
                            .and_then(|(p, st, _)| {
                                fork::fork_info(p, st, &|args: &[&str]| net::run_local(p, args))
                            })
                    });
                    (results, fork)
                })
                .await;
            let (scan, fork) = scan;
            this.update(cx, |this, cx| {
                if this.seq != seq {
                    return; // 已经有更新的一轮在跑了，这批过期
                }
                if all {
                    this.scanning = false;
                }
                // 状态条要显示"上次扫描"，得在真的扫完时记时间，
                // 不能在发起扫描时记——那会把失败的一轮也报成"刚刚扫过"
                if all {
                    this.last_scan_ts = qingbird_core::store::now_secs();
                }
                for (path, status, changes) in scan {
                    let v = this.views.entry(path.clone()).or_insert_with(RepoView::blank);
                    v.status = status;
                    // 问过 git 了，答案就在这上面 ⇒ 「待扫描」那一档到此结束。
                    // ⚠️ 这一格**不判成败**：`read_status` 不返回 `Result`，跑完就有
                    // 一份读数，读不出仓库的那个文件夹在 core 侧就是 `is_repo: false`——
                    // 那种情况下界面说「未识别为 Git 仓库」是真话。
                    // 它答的只是"有没有一份真实读数"，与读数内容好不好无关
                    v.has_status = true;
                    v.changed = changes.len() as u32;
                    v.changes = changes;
                    // **扫完就落库**（阶段 16）。刷新过的那个仓库状态一定是最新的，
                    // 不落库的话下次启动又要从头问一遍 git
                    this.persist_view_cache(&path);
                }
                if fork.is_some() {
                    this.fork = fork;
                }
                cx.notify();
            })
            .ok();
        }));
    }
}

impl Shell {
    pub(crate) fn select(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.as_deref() == Some(path.as_str()) {
            return;
        }
        self.selected = Some(path.clone());
        // **不清日志**：日志按仓库路径存（D7），切回来还在。
        // 之前这里是 `out = OutState::default()`，切一次仓库就把上一个的输出冲掉了，
        // 跑到一半切走再切回来只能重跑一遍
        // 分叉状态跟仓库走：换仓库全部作废（备份分支只在"刚变基完"那一刻有意义）
        self.fork = None;
        self.fork_backup = None;
        self.fork_diff = false;
        self.fork_confirm = false;
        repos::touch_repo(&path);
        // ⚠️ **状态扫描必须跟着选中走**。`refresh_one` 的注释写着"切仓库走这里"，
        // 而启动那条链（`open_main_window` 里）也确实调了它——**唯独 `select` 漏了**。
        // 后果实测在 2026-10-07 的抓帧里：点第二个仓库，占用与语言技术栈都有
        // （`scan_space` / `scan_profile` 在跑），状态那一整块却是空的，于是同步卡
        // 说「这个文件夹不是 Git 仓库」、头卡说「该仓库还没有配置远端」、状态条说
        // 「尚未扫描」——**三句都是假话，真话是"我还没问过 git"**。
        // 顺序照启动那条链一致（先刷状态，再空间、档案），别在这里换。
        self.refresh_one(path.clone(), cx);
        self.scan_space(cx);
        self.scan_profile(cx);
        // 落盘的操作历史跟着仓库走。**切回来时重新读而不是留着旧的**：
        // 上一次在这个窗口里跑的操作已经更新过内存那份，可"这个仓库在
        // 别的窗口跑过"这种情况读不到——而那正是重启一次之后的情形
        self.load_op_log(&path);
        // 停在「历史」页签时切仓库要跟着重扫。`scan_history` 自己按路径判重，
        // 所以「切到没扫过的仓库」才真起 git 进程，切回扫过的那个直接给缓存
        if self.cur_detail_tab() == DetailTab::History {
            self.scan_history(cx);
        }
        // 同步设置那一页的草稿跟仓库走：换个仓库还摆着上一个的半成品，
        // 用户会以为设置是共用的
        self.load_sync_cfg(window, cx);
        cx.notify();
    }
}
impl Shell {
    /// 从列表移除仓库（只删记录，不动磁盘文件）
    ///
    /// 收 `window` 是因为选中项可能因此换到别人，而地址条与同步设置草稿
    /// 都得跟着换——那两步要一个活的 `&mut Window` 才能改 `InputState`
    pub(crate) fn remove_repo_at(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = repos::remove_repo(&path);
        self.repos = repos::list_repos();
        self.views.remove(&path);
        // 库里那一行也要删，否则**孤儿行会一直攒着**：仓库移出列表了，
        // 它的体积和 README 还躺在缓存里，而这个路径将来可能被重新加回来
        // （同一个仓库被加两次时旧数会直接冒出来，看起来像"没重新扫"）
        qingbird_core::scan_cache::drop_cache(&path);
        if self.selected.as_deref() == Some(path.as_str()) {
            self.selected = self.repos.first().map(|r| r.path.clone());
            // ⚠️ 一个仓库都不剩时上面两次 `scan_space` / `scan_profile` 会
            // 直接 return，**被移走那个仓库的体积与 README 还留在屏幕上**。
            // 这里显式清空
            if self.selected.is_none() {
                self.space = SpaceState::default();
                self.profile = ProfileState::default();
            }
            self.scan_space(cx);
            self.scan_profile(cx);
            // 与 `select` 同一套收尾：换人之后地址条 / 草稿 / 历史都得重灌，
            // 否则抽屉里摆着上一个仓库的远端地址
            self.load_sync_cfg(window, cx);
            if self.cur_detail_tab() == DetailTab::History {
                self.scan_history(cx);
            }
        }
        self.logs.remove(&path);
        // 操作历史跟着 `logs` 一起从内存里摘掉。
        // ⚠️ **库里的那些行故意留着**：`drop_cache` 只清 `scan_cache` 表，
        // `op_history` 是"这个路径上次发生过什么"，将来同一个路径被加回来
        // （换台机器 clone 的同一个仓库就是同一个绝对路径）时，
        // 那句"上次拉取 · 3 天前"仍然成立。清掉反而是丢信息
        self.op_logs.remove(&path);
        self.dlg = None;
        cx.notify();
    }
}

impl Shell {
    /// 窗口底部状态条（官方 `StatusBar`）：仓库数 · 当前选中路径 · 定时同步 · 上次扫描。
    ///
    /// 为什么扫描入口在这里而不在导航轨：导航轨只回答"我在看哪一屏"（D2），
    /// 而"重新扫一遍所有仓库"是个动作。"上次扫描 · 2 分钟前"本身就是这个动作
    /// 最好的标签——它既回答了"这些数据什么时候的"，点一下又能刷新。
    pub(crate) fn status_bar(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        // 状态条报的是"**有没有活**"，不是"当前选中的仓库有没有活"——
        // 用户在总览页盯着侧栏时，想知道的是这机器上还有没有东西在跑
        let running = self
            .logs
            .iter()
            .find(|(_, l)| l.running)
            .map(|(p, _)| p.clone());
        // 跑批量时这一格报批次进度而不是"运行中 · 某仓库"：
        // 批量横跨多个仓库，报一个仓库名会让人以为只动了一个
        let batch = self.batch_note();
        let left = h_flex()
            .items_center()
            .gap(px(GAP_TIGHT))
            .child(
                div()
                    .w(px(DOT))
                    .h(px(DOT))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(match (&batch, running.is_some()) {
                        // 跑完的批次报结论时不该还亮着"运行中"的橙点
                        (Some(_), false) => t.colors.success,
                        _ if running.is_some() => orange(t),
                        _ => t.colors.success,
                    }),
            )
            .child(meta_text(
                match (&batch, &running) {
                    (Some(b), _) => b.clone(),
                    (None, Some(path)) => format!("运行中 · {}", self.title(path)),
                    (None, None) => match self.page {
                        // 这一格报的是**当前这一屏的对象数**：CLI 屏报工具数，
                        // 别的屏报仓库数。与"右页脚刷新谁"同一条——底栏跟着屏走
                        Page::CliTools => format!("{} 个 CLI 工具", self.cli.packages.len()),
                        _ => format!("{} 个仓库", self.repos.len()),
                    },
                },
                t,
            ));

        // 中区：当前选中路径。总览页没有"当前仓库"这个对象，就说这屏是什么
        let center = match (self.page, self.selected.as_deref()) {
            (Page::Repos, Some(path)) => meta_text(path, t),
            (Page::Repos, None) => meta_text("未选中仓库", t),
            (Page::Overview, _) => meta_text("总览", t),
            (Page::CliTools, _) => meta_text("CLI 工具", t),
        };

        let right = h_flex()
            .items_center()
            .gap(px(GAP_LG))
            .child(meta_text(
                match self.selected.as_deref() {
                    Some(path) => self
                        .entry(path)
                        .map(auto_sync_text)
                        .unwrap_or_else(|| "定时同步：未开启".into()),
                    None => "定时同步：未开启".into(),
                },
                t,
            ))
            .child(self.rescan_hint(t, cx));

        StatusBar::new()
            .h(px(status_bar_h()))
            // **状态条上沿那根发丝线拿掉**（2026-10-08 用户令）。它是官方
            // `StatusBar::render` 自己挂的 `.border_t_1()` + `status_bar_border`
            // （`status_bar.rs:89-90`）。压得住是因为官方把 `refine_style(&self.style)`
            // 排在**那些层之后**——调用方的样式最后重放。
            // ⚠️ 同一条机制在 `.py(0)` 上**没有生效**（帧上逐像素没动，见上面 Q22 那句），
            // 所以"refine 排在后面"不等于"每个属性都压得住"：每压一次都要抓帧量
            .border_t_0()
            // ⚠️ **`.h()` 在 gpui 里是下限，不是定高**（2026-10-08 变异实测：这一档给 50
            // 帧上就是 50，给 28 却是 33——被内容加官方自带的 `.py_1()` 撑过了下限）。
            // 试过在这里补 `.py(px(0.0))` 想把那 8 压掉：**帧上逐像素没动**
            // （`refine_style` 压不住它），那一句已经删了，别再加回来。
            // ⇒ 状态条实测 33 而 `status_bar_h()` 报 28，这 5px 未定位，记成 **Q22**
            .left(left)
            .child(center)
            .right(right)
            .into_any_element()
    }

    /// 「上次扫描 · 刚刚」：静态文字 + 点击重扫。
    ///
    /// 做成文字而不是一颗图标按钮，是因为"数据多旧"本身就是用户想知道的事，
    /// 单独一个刷新图标把这句话藏起来了。
    ///
    /// ⚠️ **这一格刷的是当前这一屏的东西**：仓库页刷仓库状态，CLI 屏重扫工具。
    /// 两屏各有自己的"扫过没有"（`last_scan_ts` / `cli.scanned_at`），所以
    /// **文字、图标语气、点击目标三样一起按屏分**——只改点击不改文字，
    /// 就会出现"写着『尚未扫描』、点下去刷的是另一边"那种两处对不上的话
    fn rescan_hint(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let on_cli = self.page == Page::CliTools;
        let (scanning, last_ts) = if on_cli {
            (self.cli.scanning, self.cli.scanned_at)
        } else {
            (self.scanning, self.last_scan_ts)
        };
        let text = if scanning {
            "扫描中…".to_string()
        } else if last_ts == 0 {
            "尚未扫描".to_string()
        } else {
            format!("上次扫描 · {}", rel_time(last_ts))
        };
        h_flex()
            .id("status-rescan")
            // 阶段 28：跟着 `meta_text` 那档字号走。原来写死 20，
            // 而这一行带的是 `fs_sec()`——中档 14px 装在 20 高里上下只剩 3px
            .h(px(row_h(fs_sec())))
            .px(px(GAP_TIGHT))
            .rounded(px(R_SM))
            .items_center()
            .gap(px(GAP_TIGHT))
            .cursor_pointer()
            .child(icon_sm(AssetIcon::RefreshCw).text_color(if scanning {
                tier4(t)
            } else {
                tier3(t)
            }))
            .child(meta_text(text, t))
            .when(!scanning, |it| {
                it.hover(move |style| style.bg(hover_face(t)))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                // 与界面那句文案同源：CLI 屏重扫工具，别的屏刷仓库状态
                if on_cli {
                    if !this.cli.scanning {
                        this.scan_cli(cx);
                    }
                } else if !this.scanning {
                    this.refresh_status(cx);
                }
            }))
            .into_any_element()
    }
}

/// 「这两栏（或多一栏）都展开时，窗口客户区至少要多宽」。
///
/// 用户 2026-10-09 给的规则是**展开时检测中间那一列不足 870 就向右把窗口撑大**，
/// 这条算式就是"那一下要撑到哪"的唯一出处。每一项都来自一个真源，
/// **不许在这里写裸数**（写了就是第二个出处，这类账本项目交过四次学费）：
///
/// | 项 | 来源 |
/// |---|---|
/// | 导航轨 + 那条右分界 | `NAV_W` + 1（`nav_rail` 的 `border_r_1`） |
/// | 窗口右缘留白 | `GAP`（只在 `finish_frame` 那一处写） |
/// | 左栏 / 右栏 | 用户自己设的 `sidebar_w` / `inspector_w`（**就是面板宽，不再另加缝**） |
/// | 中间那一列 | `MAIN_MIN_W`（**帧上量的 950**，不是用户口头给的 870，推导在 tokens 里） |
///
/// ⚠️ **列与列之间没有"再扣一条缝"这一项**。第一版在这里每项加了 6px，
/// 理由是"帧上卡间实测 6 = `GAP_TIGHT` + 两条 1px 边框"——那 6px 量的是
/// **同一列里两张卡之间**，它本来就长在面板自己的 `flex_basis` 里
/// （抽屉开那一帧实测：客户区 1514 = 轨 41 + 左栏 263 + 中间 883 + 右栏 320 + 右缘 8，
/// 一像素不多不少，而中间那 883 比目标 870 正好多出 2×6）。
/// 再扣一次就是**把同一个 6px 数两遍**，窗口会无故多宽 12px。
///
/// ⚠️ **纯函数、开环**（D56 那条教训）：输入里**没有**"窗口现在多宽"，
/// 只有"两栏开不开 + 那两个意图宽"。一旦让当前宽度参与进来就是正反馈环——
/// 撑一次算一次、每次又想再撑一点，窗口会自己往右爬。
///
/// ⚠️ 这条算式**只有在两栏的宽真的等于那两个意图值时才成立**。让宽漂回意图值的
/// 是 `keep_main_flexible`，两者是一套，不是两个功能
pub(crate) fn required_shell_w(side_on: bool, side_w: f32, insp_on: bool, insp_w: f32) -> f32 {
    let mut w = NAV_W + 1.0 + GAP + MAIN_MIN_W;
    if side_on {
        w += side_w;
    }
    if insp_on {
        w += insp_w;
    }
    w
}

/// `↑↓` 切仓库时下一步落在第几项。
///
/// **到头就停，不绕回**——列表首尾相接会让用户按到底之后突然跳回第一个，
/// 在"我正在往下翻仓库"这件事上是最反直觉的一种行为。
///
/// `cur = None`（选中的不在候选集里：刚切组还没落选中、或刚进总览）时
/// **↓ 从头、↑ 从尾**。⚠️ 旧 React 版这里 ↑ 也是从头，于是第一次按 `↑`
/// 会跳到列表中间；那是**故意改的**，别改回去。
///
/// 调用方保证 `total > 0`（`move_repo` 里先判了 `list.is_empty()`）
fn step_repo_index(cur: Option<usize>, delta: i32, total: usize) -> usize {
    match cur {
        Some(ix) => (ix as i32 + delta).clamp(0, total as i32 - 1) as usize,
        None if delta > 0 => 0,
        None => total - 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两栏都开时，**用户设的那两个宽必须真的进到算式里**——
    /// 这条挡的是"算式里用常量替了 state"：那样窗口撑到的宽度永远按默认宽算，
    /// 用户把左栏拖到 360 之后中间列照样被压，而画面看不出来是算式没跟着走。
    #[test]
    fn the_two_panel_widths_actually_enter_the_formula() {
        let base = required_shell_w(true, 263.0, true, 320.0);
        let wider_side = required_shell_w(true, 360.0, true, 320.0);
        let wider_insp = required_shell_w(true, 263.0, true, 560.0);
        assert!(
            (wider_side - base - 97.0).abs() < 0.01,
            "左栏从 263 拖到 360，需要的窗口宽应该多 97，实得 {}",
            wider_side - base
        );
        assert!(
            (wider_insp - base - 240.0).abs() < 0.01,
            "右栏从 320 拖到 560，应该多 240，实得 {}",
            wider_insp - base
        );
    }

    /// **总览页与"两栏都收"绝不该把窗口撑大**：那一屏没有左栏也没有右栏，
    /// 算出来的需求宽必须落在窗口下限以内，否则一进总览窗口就被撑一次。
    /// （撑不撑的判据是 `required <= WIN_MIN_W`，见 `ensure_shell_width` 第一条）
    #[test]
    fn nothing_open_never_forces_the_window_wider() {
        assert!(
            required_shell_w(false, 263.0, false, 320.0) <= WIN_MIN_W,
            "两栏都收时的需求宽 {} 已经顶到窗口下限 {} 之上，进总览页会被撑",
            required_shell_w(false, 263.0, false, 320.0),
            WIN_MIN_W
        );
        // 只开一栏的需求宽必须**介于**"都收"与"都开"之间：
        // 少开一栏就少一份宽，多开一栏就多一份——这两条挡的是"栏开没开没进算式"
        assert!(required_shell_w(true, 263.0, false, 320.0) > required_shell_w(false, 263.0, false, 320.0));
        assert!(required_shell_w(true, 263.0, true, 320.0) > required_shell_w(true, 263.0, false, 320.0));
    }

    #[test]
    fn stepping_stops_at_the_ends_instead_of_wrapping() {
        assert_eq!(step_repo_index(Some(0), -1, 5), 0, "已在顶部再按上，停在顶部");
        assert_eq!(step_repo_index(Some(4), 1, 5), 4, "已在底部再按下，停在底部");
        assert_eq!(step_repo_index(Some(1), 1, 5), 2);
        assert_eq!(step_repo_index(Some(1), -1, 5), 0);
    }

    /// 候选集里没有选中项时，**方向决定去哪一端**。旧 React 版 ↑ 也从头，
    /// 第一次按 ↑ 会落到列表中间——那条已经被有意改掉了
    #[test]
    fn with_nothing_selected_the_direction_picks_the_end() {
        assert_eq!(step_repo_index(None, 1, 3), 0, "没选中时向下 → 第一项");
        assert_eq!(step_repo_index(None, -1, 3), 2, "没选中时向上 → 最后一项");
    }

    #[test]
    fn a_single_row_list_never_moves() {
        assert_eq!(step_repo_index(Some(0), 1, 1), 0);
        assert_eq!(step_repo_index(Some(0), -1, 1), 0);
    }

    /// 键位与语义不许错位：`1/2/3` 必须是**拉取 / 推送 / 检测**，
    /// 且与详情页同步卡从左到右同序（设计 6.1 把三键定成"按位置记忆"）。
    /// 错位了用户按 `2` 却跑成检测，而界面上没有任何东西会告诉他
    #[test]
    fn the_three_number_keys_match_the_sync_card_order() {
        assert_eq!(SYNC_SLOTS, [Op::Pull, Op::Push, Op::Check]);
        assert_eq!(SYNC_SLOTS.len(), SYNC_SLOT_LABELS.len());
        assert_eq!(SYNC_SLOT_LABELS, ["拉取", "推送", "检测"]);
    }
}
