//! 「上次批量结果」对话框：`app_log`（跨仓库的批次汇总槽）唯一的读出口。
//!
//! ## 为什么要有这个对话框
//!
//! 批量跑完只弹一条通知，而通知是会消失的。汇总那一行里有"3 个跳过（工作区
//! 有改动，需切到该仓库确认后再拉）"这样的解释——它一消失，"哪几个没成、
//! 为什么"就再也没地方能看到了。逐仓库的日志在 `logs` 里，但看它得挨个切
//! 过去，几十个仓库时没人会这么查。
//!
//! 所以 `ops::run_batch` 除了最后那一句汇总，还把**没成的每一个**都记进
//! `app_log`，这里把它们再摊开一次。
//!
//! **不挂在状态条那一格上**：`UI-DESIGN.md` 6.1 已经把那一格写成
//! "进度（不是入口，只是进度显示）"。让它可点等于让文档那一行自己打自己。
//! 入口放 ⌘K——6.1 指定的低频层通道。

use gpui_kit::component::button::ButtonCustomVariant;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::{ActiveTheme, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, ClipboardItem, Context, Div, Entity, FontWeight, ParentElement, ScrollHandle,
    Styled, Window,
};
use std::rc::Rc;

use crate::log::output_stream;
use crate::state::*;
use crate::widgets::ghost_variant;
use crate::tokens::*;

impl Shell {
    /// 打开批量结果。**没有记录时不走这条路**——⌘K 里那一项已经置灰，
    /// 这里再兜一次是为了不给"空对话框"留入口
    pub(crate) fn open_batch_log_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.app_log.lines.is_empty() {
            return;
        }
        let this = cx.entity();
        // 滚动句柄在**打开对话框时**取一次并捕获进闭包：它是 `Rc` 句柄，
        // 克隆体与 `Shell` 上那个指向同一份状态，整段闭包生命周期都有效。
        //
        // ⚠️ **不要在 `content` 闭包里 `this.read(..).batch_log_scroll`**
        // ——那一行每帧都在跑，虽然对话框这条路不持有 Shell 的锁（不会 panic），
        // 但它让"渲染期读 Entity"这件事有了一个看起来合法的先例。
        // 阶段 21-5 的侧栏筛选键就是照着这个先例写的，当场闪退。
        // 用 `Rc` 包一层：`Fn` 闭包只能持有**共享**引用，而 `ScrollHandle`
        // 不是 `Copy`。`Rc` 克隆是廉价的引用计数，且里面的句柄**始终是同一份**
        let scroll = std::rc::Rc::new(this.read(cx).batch_log_scroll.clone());
        window.open_dialog(cx, move |dialog, _window, cx| {
            let t = cx.theme().clone();
            dialog
                .title("上次批量结果")
                .content({
                    let t = t.clone();
                    let this = this.clone();
                    // 与上面同一个道理：`this` 要 clone 之后才能被 `Fn` 闭包持有
                    let scroll = scroll.clone();
                    move |c: DialogContent, _w, cx: &mut App| {
                        // 每帧现读：这个对话框开着的时候批次可能还在跑，
                        // 快照会在开着的这几秒里一直是旧的
                        let lg = this.read(cx).app_log.clone();
                        c.child(batch_log_body(&scroll, &t, &lg))
                    }
                })
                .footer(batch_log_footer(&this, ghost_variant(cx)))
        });
    }
}

/// 滚动句柄**由调用方给**（打开对话框时取一次，见上面那处注释）。
/// 这个对话框与概览页的日志共用同一份 `LogState`，句柄必须分开，
/// 否则在这里滚一下会把底下的日志视口一起拽走
fn batch_log_body(scroll: &Rc<ScrollHandle>, t: &Theme, lg: &LogState) -> Div {
    // 汇总是最后写进去的那一行，成功与否看它前面的 `·` / `✗` 有没有出错
    let ok = lg.verdict.as_ref().is_some_and(|(ok, _)| *ok);
    v_flex()
        .w_full()
        .gap(px(GAP))
        .child(
            h_flex()
                .w_full()
                .gap(px(GAP_TIGHT))
                .items_center()
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(FS_SEC))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if ok { t.colors.success } else { t.colors.danger })
                        .child(if ok { "✓" } else { "✗" }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(FS_SEC))
                        .text_color(tier3(t))
                        .child(
                            lg.verdict
                                .as_ref()
                                .map(|(_, m)| m.clone())
                                .unwrap_or_else(|| "还没跑完".to_string()),
                        ),
                )
                .when(lg.ts > 0, |r| {
                    r.child(crate::widgets::meta_text(rel_time(lg.ts), t))
                }),
        )
        .child(
            div()
                .text_size(px(FS_META))
                .text_color(tier4(t))
                .child("只列没成的仓库与那一句汇总。完整的 git 输出在每个仓库自己的日志里（选中它 → 检视抽屉第一页签）。"),
        )
        .child(output_stream(
            "batch-log-stream",
                            &scroll,
            t,
            lg,
            "这一批没有留下逐仓库的记录",
        ))}

fn batch_log_footer(this: &Entity<Shell>, gv: ButtonCustomVariant) -> Div {
    let copy = this.clone();
    h_flex()
        .w_full()
        .justify_between()
        .gap(px(GAP))
        .child(
            Button::new("batch-log-copy")
                .label("复制")
                .custom(gv)
                .with_size(Size::Small)
                .tooltip("复制这份汇总")
                .on_click(move |_, window, cx| {
                    let text = copy.read(cx).app_log.lines.join("\n");
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    window.push_notification("已复制批量结果", cx);
                }),
        )
        .child(
            Button::new("batch-log-close")
                .label("关闭")
                .primary()
                .on_click(|_, window, cx| {
                    window.close_dialog(cx);
                }),
        )
}
