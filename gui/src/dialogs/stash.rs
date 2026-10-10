//! 32-2：「收起来的改动」清单。
//!
//! ## 它要堵的那个坑
//!
//! 拉取前青鸟替用户 `git stash push -u`，收尾 `stash pop`。**pop 没成功时那些文件
//! 就留在 stash 里**，而整个 GUI 从来没有一处读 `stash list`——于是"该去哪儿找"
//! 没有答案，用户以为东西没了。这一屏就是那个答案。
//!
//! ## 为什么入口是"点那颗橙色三角"而不是气泡里的一颗键
//!
//! 机制与实测依据写在 `widgets::warn_dot` 的注释上（官方气泡的宽限期只有 300ms，
//! 鼠标移进气泡那一路它就消失了）。形态没变——还是"只在出事那一格说话"，
//! 变的只是点哪儿。
//!
//! ## 为什么每行自带动作
//!
//! 见 `state::StashDialog` 的注释：`Dialog` 的 footer 定不住"一个都没选"那一档，
//! 而这一屏里有**不可逆**的那一半（丢掉）。动作与对象并在同一行，
//! 结构上就不存在"选了却没处执行"和"执行了却不知道点的是哪条"。
//!
//! ## 清单是快照，动手前 core 会再比一次原文
//!
//! `stash drop` 之后所有序号整体前移。界面这里存的是打开/每次动手之后重读的那份，
//! 而 `stash::apply_one` / `drop_one` 都要求把**消息原文**一起传进去比对
//! （`stash::resolve`），对不上就一条都不动。**序号只用来定位，身份是消息**。

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::{ActiveTheme, Disableable as _, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, Context, Div, Entity, FontWeight, ParentElement, SharedString, Styled, Window,
};
use qingbird_core::stash::{self, StashEntry};

use crate::state::*;
use crate::tokens::*;
use crate::widgets::{ghost_variant, list_box, meta_text, sunken_box};

/// 一行左边那颗图标。**取回与丢掉共用同一颗（存档箱）**：这一列说的是"这一条是什么"，
/// 不是"这里有什么动作"——动作已经各写在各那颗键上了
fn entry_icon(e: &StashEntry, t: &Theme) -> Div {
    div()
        .flex_shrink_0()
        .text_color(if e.auto { orange(t) } else { tier3(t) })
        .child(icon_sm(gpui_kit::assets::IconName::Archive))
}

/// 一行里那句主句：**谁存的 + 存的时候在哪个分支**。
///
/// 抽成纯函数是为了单测能钉住那几档（青鸟藏的 / 用户自己存的 / 分支解析不出来 /
/// 说明为空）。这类文案留在渲染函数里就只能靠抓帧看，而抓帧只证明"渲染成那样了"
pub(crate) fn entry_label(e: &StashEntry) -> String {
    let note = stash::human_note(&e.note);
    // 空说明**不许显示成空白**：那一行看着像渲染坏了
    let note = if note.is_empty() { "（没有说明）" } else { note };
    let text = if e.auto {
        note.to_string()
    } else {
        format!("你自己存的：{note}")
    };
    if e.branch.is_empty() {
        text
    } else {
        format!("{text} · {}", e.branch)
    }
}

/// 这一行是不是正在等二次确认。**比的是（序号 + 原文）两样**，与 core 那道
/// `resolve` 同一个判据——只比序号的话，清单被换掉之后确认框会挂到别的条目身上
fn is_confirming(confirm: &Option<(u32, String)>, e: &StashEntry) -> bool {
    confirm.as_ref() == Some(&(e.index, e.subject.clone()))
}

impl Shell {
    /// 点仓库名后面那颗橙色三角进来。**清单在这一步现读**（一条 `git stash list`，
    /// 几十毫秒），不缓存进 `Shell`——缓存了就会漂成"屏上三条、点开四条"
    pub(crate) fn open_stash_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        let entries = stash::list(&path);
        // ⚠️ 空清单**照样开**：入口那颗点读的是上一次状态扫描给的数，而现在这一条
        // 命令读的是 git。两者对不上时（比如他刚在终端里 pop 掉），给一个写着
        // "清单已经空了"的对话框是**说清发生了什么**；什么都不弹则是"点了没反应"，
        // 而"点了没反应"在这个项目里已经被误判成功能坏了过一次
        self.stash_dlg = Some(StashDialog {
            path,
            entries,
            confirm: None,
            busy: false,
        });
        self.build_stash_dlg(window, cx);
    }

    fn build_stash_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        let t = cx.theme().clone();
        window.open_dialog(cx, move |dialog, _window, cx| {
            dialog
                .title("收起来的改动")
                .content({
                    let t = t.clone();
                    let this = this.clone();
                    move |c: DialogContent, _w, cx: &mut App| {
                        // 每帧现读（与 `purge.rs`、`batch_log.rs` 同一套）：
                        // builder 捕获的是快照，不现读的话点了「丢掉」行却还挂在屏上
                        let Some(d) = this.read(cx).stash_dlg.clone() else {
                            return c;
                        };
                        c.child(stash_body(&t, &d, &this))
                    }
                })
                .footer({
                    let gv = ghost_variant(cx);
                    stash_footer(&this, gv)
                })
        });
    }
}

fn stash_body(t: &Theme, d: &StashDialog, this: &Entity<Shell>) -> Div {
    let col = v_flex().w_full().gap(px(GAP_LG));
    if d.entries.is_empty() {
        // 空态：主句一句、副句一句（v4 §7「空态」）。**不许用等宽字体容器装中文**
        return col
            .child(
                div()
                    .w_full()
                    .text_size(px(fs_body()))
                    .text_color(body(t))
                    .child("清单已经空了——没有待取回的改动。"),
            )
            .child(meta_text(
                "刚才这里还有几条的话，说明它们已经被取回或在别处清掉了。".to_string(),
                t,
            ));
    }
    col.child(
        sunken_box(
            v_flex()
                .w_full()
                .gap(px(GAP_DENSE))
                .child(
                    div()
                        .text_size(px(fs_sec()))
                        .text_color(body(t))
                        .child("取回 = 把这一条放回工作区，放回之后它仍留在清单里。"),
                )
                .child(meta_text(
                    "要清掉请点「丢掉」；丢掉不可撤销，会先问一次并打出那一行的原文。".to_string(),
                    t,
                )),
            t,
        ),
    )
    .child(
        list_box(v_flex().w_full().gap(px(GAP_DENSE)).p(px(GAP)), t).children(
            d.entries
                .iter()
                .map(|e| stash_row(t, this, e, is_confirming(&d.confirm, e), d.busy)),
        ),
    )
}

/// 一行：图标 + 主句 + 时间 + 两颗键（等确认时换成另外两颗）
fn stash_row(
    t: &Theme,
    this: &Entity<Shell>,
    e: &StashEntry,
    confirming: bool,
    busy: bool,
) -> Div {
    let idx = e.index;
    let subject = e.subject.clone();
    v_flex()
        .w_full()
        .min_h(px(row_h(fs_body())))
        .items_center()
        .gap(px(GAP_TIGHT))
        .py(px(GAP_TIGHT))
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap(px(GAP))
                .child(entry_icon(e, t))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(fs_body()))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.colors.foreground)
                        .child(entry_label(e)),
                )
                .child(meta_text(e.when.clone(), t))
                .when(!confirming, |row| {
                    row.child(
                        Button::new(SharedString::from(format!("stash-apply-{idx}")))
                            .label("取回")
                            .secondary()
                            .with_size(Size::Small)
                            .flex_shrink_0()
                            .disabled(busy)
                            .tooltip("把这一条放回工作区（放回后仍留在清单里）")
                            .accessibility_label("取回这一条")
                            .on_click({
                                let this = this.clone();
                                let subject = subject.clone();
                                move |_, _window, cx| {
                                    this.update(cx, |shell, cx| {
                                        shell.run_stash_apply(idx, &subject, cx)
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("stash-drop-{idx}")))
                            .label("丢掉")
                            .danger()
                            .with_size(Size::Small)
                            .flex_shrink_0()
                            .disabled(busy)
                            .tooltip("从清单里删掉这一条，不可撤销")
                            .accessibility_label("丢掉这一条")
                            .on_click({
                                let this = this.clone();
                                let subject = subject.clone();
                                move |_, _window, cx| {
                                    this.update(cx, |shell, cx| {
                                        shell.ask_stash_drop(idx, &subject, cx)
                                    });
                                }
                            }),
                    )
                })
                .when(confirming, |row| {
                    row.child(
                        Button::new(SharedString::from(format!("stash-drop-yes-{idx}")))
                            .label("确认丢掉")
                            .danger()
                            .with_size(Size::Small)
                            .flex_shrink_0()
                            .disabled(busy)
                            .accessibility_label("确认丢掉这一条")
                            .on_click({
                                let this = this.clone();
                                let subject = subject.clone();
                                move |_, _window, cx| {
                                    this.update(cx, |shell, cx| {
                                        shell.run_stash_drop(idx, &subject, cx)
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("stash-drop-no-{idx}")))
                            .label("再想想")
                            .secondary()
                            .with_size(Size::Small)
                            .flex_shrink_0()
                            .accessibility_label("取消这次丢掉")
                            .on_click({
                                let this = this.clone();
                                move |_, _window, cx| {
                                    this.update(cx, |shell, cx| shell.cancel_stash_drop(cx));
                                }
                            }),
                    )
                }),
        )
        // ⚠️ **原文只在等确认时打出来**：它是 git 那一行的整截，平时摆在行里只会把
        // 清单撑乱；而"要删的东西长什么样"必须在**按下确认之前**看见，不是之后
        .when(confirming, |row| {
            row.child(
                div()
                    .w_full()
                    .text_size(px(fs_meta()))
                    .text_color(tier3(t))
                    .child(format!("git stash 里那一行的原文：{subject}")),
            )
        })
}

/// 底栏：**只一颗「关闭」**。
///
/// 这里不给"全部取回"那种省事的键：一次取多条会互相冲突（同一批文件被两条暂存
/// 各自改过），而冲突之后 stash 还留着、工作区却半新半旧——**那正是这一屏要消灭的
/// 状态**。要取几条就点几次，每次都能看见结果
fn stash_footer(this: &Entity<Shell>, gv: ButtonCustomVariant) -> Div {
    // `on_click` 要 `'static`，所以这里必须 clone 一份 owned 的句柄
    // （purge / cmds 那两处同一写法）
    let this = this.clone();
    h_flex()
        .w_full()
        .justify_end()
        .gap(px(GAP))
        .child(
            Button::new("stash-close")
                .label("关闭")
                .custom(gv)
                .on_click(move |_, window, cx| {
                    this.update(cx, |shell, _| shell.stash_dlg = None);
                    window.close_dialog(cx);
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::entry_label;
    use qingbird_core::stash::{StashEntry, MSG_PULL};

    fn e(auto: bool, branch: &str, note: &str) -> StashEntry {
        StashEntry {
            index: 0,
            branch: branch.into(),
            note: note.into(),
            when: "2026-10-10 14:02".into(),
            auto,
            subject: format!("On {branch}: {note}"),
        }
    }

    #[test]
    fn ours_says_what_it_parked_and_where() {
        let got = entry_label(&e(true, "main", MSG_PULL));
        assert_eq!(got, "拉取前自动暂存 · main", "前缀要剥掉、分支要带上：{got}");
    }

    #[test]
    fn theirs_is_labeled_as_theirs() {
        // 「你自己存的」这五个字是 Q33 选"列全部"之后必须说的话——
        // 不标出来，用户会以为这几条也是青鸟藏的，然后来找我们
        let got = entry_label(&e(false, "dev", "WIP api"));
        assert_eq!(got, "你自己存的：WIP api · dev");
    }

    #[test]
    fn a_missing_branch_or_note_degrades_instead_of_blanking() {
        // 分支解析不出来 ⇒ 只说说明；说明为空 ⇒ 给「（没有说明）」，
        // **不许留一个空白格**（那一行看着像渲染坏了）
        assert_eq!(entry_label(&e(true, "", MSG_PULL)), "拉取前自动暂存");
        assert_eq!(entry_label(&e(false, "", "")), "你自己存的：（没有说明）");
    }

    #[test]
    fn the_confirm_gate_needs_both_the_number_and_the_text() {
        use super::is_confirming;
        let one = e(true, "main", MSG_PULL);
        let same_number_other_text = StashEntry {
            subject: "On main: 别的".into(),
            note: "别的".into(),
            ..one.clone()
        };
        assert!(is_confirming(&Some((0, one.subject.clone())), &one));
        // 序号对、原文不对 ⇒ 不算在确认这一条（清单被换掉了）
        assert!(!is_confirming(&Some((0, one.subject.clone())), &same_number_other_text));
        assert!(!is_confirming(&None, &one));
    }
}
