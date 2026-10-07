//! 检视抽屉：仓库页右侧常驻列，可收起、可拉伸。
//!
//! **这里不是官方 `Sheet`。** `Sheet` 是模态浮层——它带遮罩、会吃掉整个窗口的
//! 点击、并劫持 Esc 与焦点（`BaseSheet` + `focus_handle` + `overlay`）。设计文档第 1.1 节
//! 写的是"抽屉可收起，收起后内容区获得空间""收起不会丢失日志状态——它只是不可见"，
//! 这两条全都要求它是版面里的一列，不是盖在版面上的一层。
//! 官方 `Sheet` 留给真正需要"盖住界面"的地方：提交 diff。
//!
//! **2026-10-05：正文清空，标成「开发中」。** 原来这里有四个页签
//! （日志 / 变更 / 历史 / 空间），其中历史与空间和中栏的页签是**同一份数据、
//! 同一套内容**——两个入口看同一件事；变更升格成了中栏页签；日志整块搬去了
//! 中栏概览页的下半段。四个页签全撤之后这一列空了。
//!
//! **但模块留着**（用户口径："暂时保留右栏的模块，先标成开发中，我未来规划给 AI 用"）。
//! 留着的是三样：**列本身**（宽度 / 可收起 / 标题行 / 折叠键）、
//! **落盘的列宽设置**（`store::Settings::inspector_w`）、
//! 以及它作为"主区之外的第二列"在版面里的位置。将来往里放 AI 相关的
//! 会话 / 建议 / 差异审阅时，这套骨架可以直接用；把列删掉将来要重建，
//! 而重建时那三样都会发现要重新做一遍。
//!
//! **占位不放"敬请期待"这类空话。** 写清楚这一列将来放什么、以及现在那些
//! 内容去了哪，用户才知道去中栏找，而不是以为功能被删了

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::empty::{Empty, EmptyContent, EmptyHeader, EmptyTitle};
use gpui_kit::component::separator::Separator;
use gpui_kit::component::{Sizable, Size, Theme};
use gpui_kit::{
    div, px, AnyElement, Context, Div, FontWeight, IntoElement, ParentElement, Styled,
};

use crate::keys::ToggleInspector;
use crate::state::*;
use crate::tokens::*;
use crate::widgets::ghost_variant;

// ================= 检视抽屉（右侧常驻列） =================

impl Shell {
    /// 右侧常驻列：标题行（检视 + 仓库名 + 折叠键）/ 分隔线 / 正文。
    /// 收起不丢任何状态——它只是不渲染
    ///
    /// **不自己设宽度**：这一列是 `shell.rs` 里 `h_resizable("insp-split")`
    /// 的第 1 号面板，宽度由 `ResizableState` 派给 `flex_basis`，
    /// 自己再写一个 `w(px(..))` 会和它打架（每帧以谁为准说不清）。
    /// `self.inspector_w` 只在**首次挂上**、状态里还没有它那一格时当初值用
    pub(crate) fn inspector(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let name = self
            .selected
            .as_deref()
            .map(|p| self.title(p))
            .unwrap_or_default();

        v_flex()
            .w_full()
            .h_full()
            .bg(t.colors.popover)
            .child(
                h_flex()
                    .w_full()
                    .h(px(40.0))
                    .flex_shrink_0()
                    .px(px(GAP_LG))
                    .gap(px(GAP))
                    .items_center()
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap(px(GAP_TIGHT))
                            .child(
                                div()
                                    .text_size(px(fs_body()))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(t.colors.foreground)
                                    .child("检视"),
                            )
                            .child(
                                div()
                                    .text_size(px(fs_meta()))
                                    .text_color(tier4(t))
                                    .truncate()
                                    .child(if name.is_empty() {
                                        "未选中仓库".to_string()
                                    } else {
                                        name
                                    }),
                            ),
                    )
                    .child(div().flex_1())
                    // 收起 / 展开都走 `toggle_inspector`：那才是会落盘的那个入口
                    .child(
                        Button::new("inspector-collapse")
                            .icon(icon_sm(AssetIcon::PanelRightClose).text_color(tier3(t)))
                            .custom(ghost_variant(cx))
                            .with_size(Size::Small)
                            .tooltip_with_action("收起检视", &ToggleInspector, None)
                            .accessibility_label("收起检视面板")
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_inspector(cx))),
                    ),
            )
            .child(Separator::horizontal().color(t.colors.border))
            .child(self.inspector_placeholder(t))
    }

    /// 「开发中」占位。**用官方 `Empty`**——它自带图标位与三层文案层级，
    /// 手搓一个居中的灰字块在深浅两种主题下都不如它稳
    fn inspector_placeholder(&self, t: &Theme) -> Div {
        v_flex()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .p(px(GAP_LG))
            .child(
                Empty::new()
                    .border_0()
                    .header(EmptyHeader::new().title(EmptyTitle::new().child("检视面板 · 开发中")))
                    .content(EmptyContent::new().child(
                        v_flex()
                            .items_center()
                            .gap(px(GAP_TIGHT))
                            .child(
                                div()
                                    .text_size(px(fs_meta()))
                                    .text_color(tier4(t))
                                    .child("这一列留给 AI 助手：会话、提交建议与差异审阅"),
                            )
                            .child(
                                div()
                                    .text_size(px(fs_meta()))
                                    .text_color(tier4(t))
                                    .child("变更 / 历史 / 空间 / 日志 已移到中栏页签与概览底部"),
                            ),
                    )),
            )
    }

    /// 收起后在仓库头右上角留一个回来的入口。收起与展开都得有路，
    /// 否则用户收一次就再也找不回来
    ///
    /// ⚠️ **这一颗全app只能挂一次**（现在挂在 `repo_head.rs` 那一行）。
    /// 2026-10-05 踩过：在 `sync_card` 的状态行里又挂了一颗，两个调用点都是
    /// `Button::new("inspector-open")`，而 gpui 的 `GlobalElementId` **只由
    /// 「有 id 的祖先」串起来**——这两颗的父链里没有任何一层带 id 能区分它们，
    /// 于是算出同一个 `NodeId`，`a11y.rs` 的 `can_push` 判定重复：
    /// debug 构建直接 panic（`Duplicate a11y node id`），release 构建静默
    /// 把第二个节点从无障碍树里丢掉。
    ///
    /// **为什么平时跑看不出来**：那棵树**只在系统/读屏软件接入时才建**
    /// （`a11y.rs:212` 的 `active_flag`，由 `window.rs:1626` 的平台回调置位，
    /// 没有环境变量能强开）。所以这个 bug 在开发时是"偶发"的——
    /// 真要复现，让一个 UIA 客户端碰一下窗口即可（见
    /// `scripts/uia_probe.py`）。**新增任何"收起了才出现"的入口时，
    /// 先确认它在这一个页面上只挂一次。**
    pub(crate) fn inspector_reopen(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        Button::new("inspector-open")
            .icon(icon_sm(AssetIcon::PanelRightOpen).text_color(tier3(t)))
            .custom(ghost_variant(cx))
            .with_size(Size::Small)
            .tooltip_with_action("展开检视", &ToggleInspector, None)
            .accessibility_label("展开检视面板")
            .on_click(cx.listener(|this, _, _, cx| this.toggle_inspector(cx)))
            .into_any_element()
    }
}
