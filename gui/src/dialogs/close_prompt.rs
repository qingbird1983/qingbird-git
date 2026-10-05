//! 关窗询问面板（阶段 22）。
//!
//! ## 为什么这块能成立：一个查错了的结论
//!
//! 2026-10-05 之前，项目文档里写着「**Windows 上没有拦截关闭的钩子，
//! 别再花时间找"阻止关闭"的那个钩子了，它不存在**」。**那条是错的。**
//! 错因是只查了两层：组件层的 `TitleBar::on_close_window`（源码第一行就是
//! `if cfg!(target_os = "linux")`，Windows 上确实不写进去）与
//! `Window::remove_window`（确实没有 prevent 参数）。
//!
//! **真正那一层在 `PlatformWindow` 上，而且是通的**：
//! - `gpui-pre/src/window.rs:6658` `pub fn on_window_should_close(..)`
//! - `gpui-pre-windows/src/events.rs:110` 消息泵里 `WM_CLOSE => self.handle_close_msg()`
//! - 同文件 `:336-341` 回调返回 `false` 时回 `Some(0)`，系统收到非零 LRESULT
//!   且 DefWindowProc 不跑 ⇒ **窗口不关**
//!
//! 顺带更正另一条：「自己 `on_click` 调 `remove_window()` 会让托盘唤不醒」
//! **也不对**。托盘唤起是 `tray.rs::show_or_open`，它只看 `cx.windows()` 里
//! 有没有窗口：有就 `activate_window()`，没有就 `open_main_window()` **新开一个**——
//! **不依赖窗口当初是怎么关的**。
//!
//! ## 为什么在那个回调里只置标记，不直接拉起弹窗
//!
//! `on_window_should_close` 的回调签名是 `Fn(&mut Window, &mut App) -> bool`，
//! 而它跑在 **Win32 的 `WM_CLOSE` 处理过程中**。在那儿拉一个模态浮层意味着
//! 消息循环嵌套，而回调还必须**同步返回一个值**决定这一次关闭放不放行。
//!
//! 所以拆成两拍（本项目的老套路，与 `pending_folder` / `pending_reveal` 同源）：
//! ① 回调里判档：关到托盘 → 放行；完全退出 → 阻止 + 置 `pending_app_quit`；
//!    每次都问 → **阻止这一次关闭** + 置 `pending_close_prompt`；
//! ② `Render::render` 下一帧兑现标记，那时 `&mut self` 与 `&mut Window` 都在手边。
//!
//! 「每次都问」这一档是**默认**（`store::close_action_of` 的兜底方向），
//! 理由与那条判据的注释一样：兜错到"直接执行"是不可撤销的那种错。

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::{ActiveTheme as _, Sizable, Size, Theme, WindowExt as _};
use gpui_kit::{
    div, px, App, Context, Entity, ParentElement as _, IntoElement as _, Styled as _, Window,
};
use qingbird_core::store::{self, CLOSE_EXIT, CLOSE_TRAY};

use crate::state::*;
use crate::tokens::*;
use crate::widgets::ghost_variant;

/// 面板宽度。官方默认 448（`dialog.rs:219`）是给"确认一下"的，
/// 这里有两行说明加一颗勾选，448 够用，不额外放宽——**弹窗越宽越像结论页**
const DLG_W: f32 = 420.0;

impl Shell {
    /// 拉起关窗询问面板。**由 `Render::render` 兑现标记时调用**，
    /// 不是由 `on_window_should_close` 回调直接调（理由见模块头）
    pub(crate) fn open_close_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        // 每次开面板重置勾选：**上一次勾过不等于这一次也要记**。
        // 用户主动勾上才叫"记住"，否则它会变成一个"永远记住第一次"
        self.close_remember = false;
        window.open_dialog(cx, move |dialog, _window, cx| {
            let t = cx.theme().clone();
            dialog
                .title("关闭窗口")
                .w(px(DLG_W))
                .content({
                    let t = t.clone();
                    let this = this.clone();
                    move |c: DialogContent, _w: &mut Window, cx: &mut App| {
                        let st = this.read(cx);
                        let remember = st.close_remember;
                        c.child(Self::close_prompt_body(&t, remember, &this))
                    }
                })
                .footer(Self::close_prompt_footer(&this, ghost_variant(cx)))
                // Esc / 点遮罩 / 右上角 X 一律当"取消"：窗口照常留着
                .on_close({
                    let this = this.clone();
                    move |_, window, cx| {
                        this.update(cx, |this, cx| this.cancel_close(window, cx));
                    }
                })
        });
    }

    /// 正文：两行说明 + 「记住我的选择」
    ///
    /// 返回 `AnyElement` 而不是 `DialogContent`——后者只能由 `Dialog::content`
    /// 那个闭包自己产出；这里给的是它往里 `child` 的东西。
    fn close_prompt_body(
        t: &Theme,
        remember: bool,
        this: &Entity<Self>,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .w_full()
            .child(
                div()
                    .text_size(px(FS_BODY))
                    .text_color(body(t))
                    .child("关闭窗口之后，这个程序还在后台跑：托盘图标留着，定时自动同步照常触发。"),
            )
            .child(
                div()
                    .mt(px(GAP_TIGHT))
                    .text_size(px(FS_SEC))
                    .text_color(tier3(t))
                    .child("真的要关掉整个程序（后台同步也会停）吗？"),
            )
            .child(
                div()
                    .mt(px(GAP))
                    .border_t_1()
                    .border_color(edge(t))
                    .pt(px(GAP))
                    .child(Checkbox::new("close-remember").checked(remember).label("记住我的选择，下次直接这么关").on_click({
                        let this = this.clone();
                        move |checked, _window, cx| {
                            this.update(cx, |this, _| this.close_remember = *checked)
                        }
                    })),
            )
            .into_any_element()
    }

    /// 底部三颗键：**两个动作键平级，右边是主路径；「取消」在左边当退路**
    fn close_prompt_footer(
        this: &Entity<Self>,
        gv: ButtonCustomVariant,
    ) -> impl gpui_kit::IntoElement {        h_flex()
            .w_full()
            .items_center()
            // 左边一颗取消，右边两颗动作键——与项目里 `dlg_footer` 同口径
            .child(
                Button::new("close-cancel")
                    .label("取消")
                    .custom(gv.clone())
                    .with_size(Size::Medium)
                    .on_click({
                        let this = this.clone();
                        move |_, window, cx| {
                            this.update(cx, |this, cx| this.cancel_close(window, cx))
                        }
                    }),
            )
            .child(div().flex_1())
            .child(
                Button::new("close-quit")
                    .label("完全退出")
                    .danger()
                    .with_size(Size::Medium)
                    .on_click({
                        let this = this.clone();
                        move |_, window, cx| {
                            this.update(cx, |this, cx| this.quit_app(window, cx))
                        }
                    }),
            )
            .child(
                Button::new("close-tray")
                    .label("关闭到托盘")
                    .primary()
                    .with_size(Size::Medium)
                    .on_click({
                        let this = this.clone();
                        move |_, window, cx| {
                            this.update(cx, |this, cx| this.close_to_tray(window, cx))
                        }
                    }),
            )
    }

    /// 设置面板里改关窗档。**即时落盘 + 同步缓存**
    ///
    /// ⚠️ **落盘与改缓存必须一起做**：`on_window_should_close` 的回调读的是
    /// `Shell::close_action` 那份**缓存**（它在 Win32 消息处理里，一次
    /// `store::load()` 就是一次同步读盘）。只落盘不改缓存，用户在设置里
    /// 拨了开关、点 X 走的还是旧档——**看着改了、行为没变**，最难查的那一类。
    pub(crate) fn set_close_action(&mut self, raw: &str, cx: &mut Context<Self>) {
        let mut cfg = store::load();
        cfg.settings.close_action = raw.to_string();
        let _ = store::save(&cfg);
        self.close_action = store::close_action_of(raw);
        // 设置对话框开着的话，把快照里那份一起跟上，
        // 否则那一页会一直显示改之前的档
        if let Some(d) = self.settings_dlg.as_mut() {
            d.close_action = self.close_action;
        }
        cx.notify();
    }

    /// 勾了「记住」才落盘。**不勾就什么都不改**——用户说的就是
    /// "打勾下次就不询问"，没打勾时下次照问
    fn persist_close_action(&mut self, raw: &str, cx: &mut Context<Self>) {
        if !self.close_remember {
            return;
        }
        let mut cfg = store::load();
        cfg.settings.close_action = raw.to_string();
        let _ = store::save(&cfg);
        self.close_action = store::close_action_of(raw);
        cx.notify();
    }

    /// 「关闭到托盘」：窗口销毁、进程留着，托盘图标能把它叫回来
    fn close_to_tray(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.persist_close_action(CLOSE_TRAY, cx);
        window.close_dialog(cx);
        // ⚠️ 这里调 `remove_window()` 而不是"再发一次 WM_CLOSE"：
        // 我们已经把 `on_window_should_close` 装上了，再走一次系统关闭
        // 会**第二次进那个回调**，弹窗就循环了。
        // 托盘唤起不受影响——`tray.rs::show_or_open` 只看 `cx.windows()` 有没有窗口
        window.remove_window();
    }

    /// 「完全退出」：勾了就记住 `exit` 档，下次点 X 直接退
    fn quit_app(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.persist_close_action(CLOSE_EXIT, cx);
        window.close_dialog(cx);
        // 置标记而不是就地 `cx.quit()`：**这个回调是 `on_click`**，
        // 与 `pending_folder` 那条同源（渲染/事件路径上直接动 App 的全局状态不稳），
        // 下一帧由 `Render::render` 兑现
        self.pending_app_quit = true;
        cx.notify();
    }

    /// 「取消」：什么都不做，窗口照常留着
    fn cancel_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_remember = false;
        window.close_dialog(cx);
    }
}
