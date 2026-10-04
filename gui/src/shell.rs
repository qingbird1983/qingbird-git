//! 窗口壳：标题栏 / 导航轨 / 状态条，仓库选中与状态刷新。
//!
//! `Render for Shell` 在这里，四块面板由 sidebar / overview / detail 提供。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::motion::{transition, Transition};
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::resizable::{
    h_resizable, resizable_panel, ResizablePanelGroup, ResizableState,
};
use gpui_kit::component::separator::Separator;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::text::TextViewState;
use gpui_kit::component::table::{TableEvent, TableState};
use gpui_kit::component::{ActiveTheme, Selectable as _, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, img, px, Action, AnyElement, AppContext, Context, Div, FontWeight, InteractiveElement,
    IntoElement, KeyBinding, ParentElement, Pixels, Render, ScrollHandle,
    StatefulInteractiveElement, Styled, Window,
};
use qingbird_core::fork;
use qingbird_core::net;
use qingbird_core::repos::{self};
use qingbird_core::run_cmd::{self, Slot};
use qingbird_core::store::RepoEntry;
use qingbird_core::types::RepoStatus;
use std::collections::HashMap;

use crate::detail::DetailTab;
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
        };

        // 侧栏宽度只有一个真源：`ResizableState` 里第 0 号面板的宽度。
        // 折叠 / 展开 / 切到总览都只是把"目标宽度"改成 0 或 sidebar_w，
        // 中间值由官方 transition 补出来——不要在别处再存一份宽度。
        let motion = cx.theme().motion_tokens();
        let policy = Transition::new(motion.duration_fast).easing(motion.easing_move.clone());
        let target_w: Pixels = if self.sidebar_collapsed || self.page == Page::Overview {
            px(0.0)
        } else {
            px(self.sidebar_w)
        };
        // id 是稳定字符串：transition 的状态挂在它上面，id 一变动画就从头开始
        let anim_w: Pixels = transition("sidebar-width", target_w, policy, window, cx);
        // 补间中 = 当前值与目标值对不上。`cx.reduce_motion()` 为真时官方
        // transition 直接返回终值，这里自然落到 false，不补间
        let animating = (anim_w - target_w).abs() > px(0.5);

        // 只在可见或正在滑出时挂面板。宽度到 0 就把面板整个摘掉，
        // 这才是"不留空槽位"——留一个 0 宽面板在 flex 里，主区虽然吃掉了它的
        // 份额，可第 1 号面板上那个拖拽把手还挂在 x=0，一条按了没用的 4px 命中区
        let sidebar_mounted =
            (self.page == Page::Repos && !self.sidebar_collapsed) || anim_w > px(0.5);

        if sidebar_mounted && animating {
            self.split.update(cx, |s, cx| {
                // 补间期间由 transition 说话。**不在非补间时写**：
                // 那时宽度归拖拽，写进去会把用户刚拖出来的宽度一帧拽回去
                if s.sizes().len() < 2 {
                    // 面板这一帧才挂上。状态里还没有第 0 号面板，
                    // `resize_panel` 是空操作，面板会退回 `initial_size`（= 全宽）
                    // 闪一下再缩回去。直接按当前动画值插入，省掉那一帧
                    s.insert_panel(Some(anim_w), Some(0), cx);
                } else {
                    s.resize_panel(0, anim_w, window, cx);
                }
            });
        }

        // 下限放开成 0 的两种情形：正在补间（滑进滑出都要经过 200px 以下），
        // 或者目标就是 0。少了后一条，折叠停在仓库页时 min_w 会把面板按回 200px，
        // 凭空多出一条空槽——正好是这一步要消灭的东西。
        // 非折叠状态下下限照常夹住，拖宽才拖不到 0
        let min_w: Pixels = if animating || target_w <= px(0.5) {
            px(0.0)
        } else {
            px(SIDEBAR_MIN_W)
        };

        let mut group = h_resizable("body-split").with_state(&self.split);
        if sidebar_mounted {
            group = group.child(
                resizable_panel()
                    .size_range(min_w..px(SIDEBAR_MAX_W))
                    .size(px(self.sidebar_w))
                    // 官方面板默认 flex_grow: 1，会把主区挤走；定宽面板要自己关掉
                    .flex_none()
                    .child(self.sidebar(&t, window, cx)),
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
    /// 组好窗口的其余部分：导航轨 / 主区分组 / 检视抽屉 / 状态条。
    /// `body` 已经是「侧栏（可选）+ 主区」的 resizable 分组。
    fn finish_frame(
        &mut self,
        t: Theme,
        body: ResizablePanelGroup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
        main_group = main_group.child(resizable_panel().child(body));
        // 检视抽屉只在仓库页有——总览页没有当前仓库，这一列里没有东西可看
        if self.page == Page::Repos && self.inspector_open && self.selected.is_some() {
            main_group = main_group.child(
                resizable_panel()
                    .size_range(px(INSPECTOR_MIN_W)..px(INSPECTOR_MAX_W))
                    // 只在这一列**首次挂上**、状态里还没有它那一格时起作用
                    // （`panel.rs:341-349`：`panel_state.size` 为 None 才吃
                    // `initial_size`）。之后就归 `ResizableState` 自己管，
                    // 每帧再写一遍会把用户正拖的宽度一帧拽回去
                    .size(px(self.inspector_w))
                    .flex_none()
                    .child(self.inspector(&t, cx)),
            );
        }
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
        v_flex()
            .size_full()
            .min_h_0()
            .bg(t.colors.background)
            .text_color(t.colors.foreground)
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
                    .child(self.nav_rail(&t, cx))
                    .child(Separator::vertical().color(t.colors.border))
                    .child(div().flex_1().min_w_0().min_h_0().child(main_group)),
            )
            .child(self.status_bar(&t, cx))
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
            .h(px(TITLE_BAR_H))
            .flex_shrink_0()
            .items_center()
            .bg(t.colors.popover)
            .border_b_1()
            .border_color(t.colors.border)
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
                            .text_size(px(FS_BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(body(t))
                            .child("青鸟 Git"),
                    )
                    // 总览页没有侧栏可折，藏起来免得点了个没反应的东西
                    .when(self.page == Page::Repos, |bar| {
                        bar.child(self.sidebar_toggle(t, cx))
                    }),
            )
            // ③ 拖拽区：左右两组之间的**空白**才是拖窗口的地方
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(gpui_kit::WindowControlArea::Drag),
            )
            .on_double_click(|_, window, _| window.zoom_window())
            .child(self.window_controls(supported, maximized, cx))
            .into_any_element()
    }

    /// 侧栏展开 / 收起开关。
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
        // 图标随折叠态翻：这颗键的图标就是"它现在会做什么"的说明
        let icon = if self.sidebar_collapsed {
            AssetIcon::PanelLeftOpen
        } else {
            AssetIcon::PanelLeftClose
        };
        let label = if self.sidebar_collapsed {
            "展开仓库侧栏"
        } else {
            "收起仓库侧栏"
        };
        Button::new("titlebar-sidebar-toggle")
            .icon(icon_sm(icon).text_color(tier3(t)))
            .custom(ghost_variant(cx))
            .with_size(Size::Small)
            .tooltip(label)
            .accessibility_label(label)
            .on_click(cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)))
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
                    th.secondary_hover,
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
                    th.secondary_hover,
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
    /// 折叠 / 展开侧栏（`Ctrl B` 与标题栏按钮共这一个入口）。
    /// 折叠态与宽度都落盘，下次启动回到用户自己摆的样子
    pub(crate) fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        let w = qingbird_core::store::clamp_sidebar_w(w.as_f32());
        if (w - self.sidebar_w).abs() < f32::EPSILON {
            return;
        }
        self.sidebar_w = w;
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
        self.persist_inspector_layout();
        cx.notify();
    }

    /// 落盘检视列版式（宽度 + 展开态）。与 `persist_sidebar_layout` 一对
    pub(crate) fn persist_inspector_layout(&self) {
        let _ = qingbird_core::store::set_inspector_layout(self.inspector_w, self.inspector_open);
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
    /// 顶部 `NAV_PAD_TOP` 是一条对齐基线、底部 `NAV_PAD_BOTTOM` 是"独立一组的气口"。
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
            .bg(t.colors.popover)
            .items_center()
            .px(px(NAV_PAD))
            .pt(px(NAV_PAD_TOP))
            .pb(px(NAV_PAD_BOTTOM))
            // ⚠️ 这一处用 `GAP_LG`（12）而不是 `GAP_DENSE`，**是有意的**：
            // `GAP_DENSE`（2）治的是「一串同类**行**相邻两项」（列表行、色块分段），
            // 那些行本来就是一整块、行内还有内边距，所以贴着才连得起来。
            // 导航轨上是**三个各自独立的按钮**——各有各的命中区、各有各的选中底块，
            // 贴着排读起来是"糊成一片"，实机按 4px 排出来用户就说"快粘到一起了"。
            // 它们之间要的是"两个不同的东西"那种能分开的气口
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
            .child(div().flex_1())
            .child(
                Button::new("nav-settings")
                    .icon(nav_icon(AssetIcon::Settings).text_color(tier3(t)))
                    .custom(ghost_variant(cx))
                    .with_size(Size::Large)
                    .tooltip("设置")
                    .accessibility_label("设置")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_settings_dlg(window, cx)),
                    ),
            )
    }

    /// 导航按钮。选中态仍交给官方 `Button::selected`（它管焦点环、a11y 与
    /// 「打开态也画成选中」这条语义），但**底色由我们自己给**（D60）。
    ///
    /// ⚠️ 官方 ghost 选中的底是 `button.active.background`，而 `theme.json`
    /// 没配这一项 → 落基座兜底 `input.mix_oklab(transparent, 0.7)`，比规范板的
    /// 选中面深一大截。所以用 `tokens::selected_mask` 盖掉。
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
        };
        Button::new(id)
            .icon(nav_icon(icon).text_color(ink))
            .custom(ghost_variant(cx))
            .selected(active)
            .when(active, |this| this.bg(selected_mask(t)))
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
            sidebar_w: qingbird_core::store::clamp_sidebar_w(saved.sidebar_w),
            split: cx.new(|_| ResizableState::default()),
            // 右栏两件都从配置读，与侧栏同一口径：宽度夹过、展开态是用户自己的选择。
            // 默认展开（core 的 `inspector_open` 是 `default_true`）——
            // 收起的右栏只有仓库头那颗按钮能叫回来，默认为收等于新装用户看不到这列
            inspector_open: saved.inspector_open,
            inspector_w: qingbird_core::store::clamp_inspector_w(saved.inspector_w),
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
            reveal: None,
            pending_reveal: None,
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
            history: HistoryState::default(),
            history_table,
            ov_table,
            ov_rows: Vec::new(),
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
            clone_dlg: None,
            cmd_dlg: None,
            settings_dlg: None,
            sync_cfg,
            palette,
            palette_actions: Vec::new(),
            palette_open: false,
        };
        // 全部走官方 action + key binding，不自己 `on_key_down` 判键：
        // 自己判的那份不知道当前焦点在输入框还是按钮上，会在打字时把侧栏折了。
        // `context` 传 None = 全局生效；命令面板自己那个 Dialog 上下文里
        // ↑↓ 与回车由 `command::init` 绑，不在这里重复占键
        cx.bind_keys([
            KeyBinding::new("ctrl-b", ToggleSidebar, None),
            KeyBinding::new("ctrl-i", ToggleInspector, None),
            // `Ctrl .` 停当前操作。键位跟着"停"这个动作走，和日志工具行那个
            // 停止按钮共用 `stop_current`——两处不该各调一次 core 的取消入口
            KeyBinding::new("ctrl-.", StopOp, None),
            KeyBinding::new("ctrl-k", OpenPalette, None),
            KeyBinding::new("ctrl-t", OpenTerminal, None),
            // ⚠️ **顺序即语义**：`1` `2` `3` 必须与详情页同步卡从左到右同序。
            // 那个顺序声明在 `state.rs::SYNC_SLOTS`（连同文案一起）——
            // 改键位要三处一起改：这里、那边的顺序、侧栏 / 状态条的文案。
            // `shell.rs` 的单测 `the_three_number_keys_match_the_sync_card_order` 钉住这个序
            KeyBinding::new("1", SyncPull, None),
            KeyBinding::new("2", SyncPush, None),
            KeyBinding::new("3", SyncCheck, None),
            KeyBinding::new("down", NextRepo, None),
            KeyBinding::new("up", PrevRepo, None),
            KeyBinding::new("escape", DismissTop, None),
        ]);
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

/// `Ctrl B`。走官方 action + key binding，而不是自己 `on_key_down` 判键：
/// 自己判的那份不知道当前焦点在输入框还是按钮上，会在打字时把侧栏折了
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct ToggleSidebar;

/// `Ctrl I`：折叠 / 展开检视抽屉
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct ToggleInspector;

/// `Ctrl .`：停掉当前正在跑的操作
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct StopOp;

/// `Ctrl K`：命令面板（全局，两屏都生效）
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct OpenPalette;

/// `Ctrl T`：在当前仓库开终端
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct OpenTerminal;

/// `1` / `2` / `3`：同步卡上**从左到右**的拉取 · 推送 · 检测。
///
/// 键位按**屏幕位置**对位而不是按操作顺序：用户是先看见按钮再按位置的键，
/// 顺序一改快捷键与眼睛看到的东西就对不上了
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct SyncPull;
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct SyncPush;
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct SyncCheck;

/// `↓` / `↑`：在当前这一组里上下切仓库（到头就停，不绕回）
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct NextRepo;
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct PrevRepo;

/// `Esc`：收起当前这一屏自己开的临时东西。
/// 浮层（对话框 / Sheet / 右键菜单）的 Esc 由官方那几层自己吃掉，
/// 走到这里的只可能是本壳自己画的那一样
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct DismissTop;

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
}

impl Shell {
    /// 当前仓库的显示名：有自定义名用自定义名
    pub(crate) fn title(&self, path: &str) -> String {
        self.entry(path)
            .map(|r| {
                if r.alias.trim().is_empty() {
                    r.name.clone()
                } else {
                    r.alias.trim().to_string()
                }
            })
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
                                    (
                                        p.clone(),
                                        repos::read_status(p),
                                        // 一条 `git status --porcelain` 同时给个数和文件名。
                                        // 以前这里跑的是 `changed_files`，跑的是同一条命令、
                                        // 只把结果数了一下行——提交对话框与检视抽屉要的
                                        // 变更清单就是这批输出的另一半，等于白丢
                                        repos::changed_entries(p),
                                        // `git log -1` 只读 HEAD 不走历史，O(1)，
                                        // 放在状态扫描这一轮里顺手算，不另开一趟
                                        repos::last_commit_ts(p),
                                    )
                                })
                            })
                            .collect();
                        handles
                            .into_iter()
                            .map(|h| {
                                h.join().unwrap_or_else(|_| {
                                    (String::new(), RepoStatus::default(), Vec::new(), 0)
                                })
                            })
                            .collect::<Vec<_>>()
                    });
                    // 分叉信息只对选中仓库算（几条本地 git 命令），和状态扫描同轮完成；
                    // 没分叉就是 None，分叉卡不出现
                    let fork = sel.as_ref().and_then(|sel_path| {
                        results
                            .iter()
                            .find(|(p, _, _, _)| p == sel_path)
                            .filter(|(_, st, _, _)| st.ahead > 0 && st.behind > 0)
                            .and_then(|(p, st, _, _)| {
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
                for (path, status, changes, last_commit) in scan {
                    let v = this.views.entry(path.clone()).or_insert_with(RepoView::blank);
                    v.status = status;
                    v.changed = changes.len() as u32;
                    v.changes = changes;
                    v.last_commit_ts = last_commit;
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
                    .w(px(6.0))
                    .h(px(6.0))
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
                    (None, None) => format!("{} 个仓库", self.repos.len()),
                },
                t,
            ));

        // 中区：当前选中路径。总览页没有"当前仓库"这个对象，就说这屏是什么
        let center = match (self.page, self.selected.as_deref()) {
            (Page::Repos, Some(path)) => meta_text(path, t),
            (Page::Repos, None) => meta_text("未选中仓库", t),
            (Page::Overview, _) => meta_text("总览", t),
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
            .h(px(STATUS_BAR_H))
            .left(left)
            .child(center)
            .right(right)
            .into_any_element()
    }

    /// 「上次扫描 · 刚刚」：静态文字 + 点击重扫。做成文字而不是一颗图标按钮，
    /// 是因为"数据多旧"本身就是用户想知道的事，单独一个刷新图标把这句话藏起来了。
    fn rescan_hint(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let text = if self.scanning {
            "扫描中…".to_string()
        } else if self.last_scan_ts == 0 {
            "尚未扫描".to_string()
        } else {
            format!("上次扫描 · {}", rel_time(self.last_scan_ts))
        };
        h_flex()
            .id("status-rescan")
            .h(px(20.0))
            .px(px(GAP_TIGHT))
            .rounded(px(R_SM))
            .items_center()
            .gap(px(GAP_TIGHT))
            .cursor_pointer()
            .child(icon_sm(AssetIcon::RefreshCw).text_color(if self.scanning {
                tier4(t)
            } else {
                tier3(t)
            }))
            .child(meta_text(text, t))
            .when(!self.scanning, |it| {
                it.hover(move |style| style.bg(t.colors.list_hover))
            })
            .on_click(cx.listener(|this, _, _, cx| {
                if !this.scanning {
                    this.refresh_status(cx);
                }
            }))
            .into_any_element()
    }
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
