//! 「提交并推送…」对话框。
//!
//! 三件事按顺序摆在一屏里：**变更摘要**（这次要提交什么）→ **提交信息**
//! （预填上次提交标题）→ **提交后拉取**（一个勾）。理由见设计文档 3.7：
//! 这是一个多步的低频操作，不该挤进同步卡那三把靠位置记忆的键里。
//!
//! 执行走 core 的 `sync::commit_push_with`——和拉取/推送同一条 `run_net_op`
//! 链路，代理回退与凭证注入都在里面，界面不自己拼 git 子进程。

use gpui_kit::component::button::ButtonCustomVariant;
use gpui_kit::base::v_flex;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Disableable as _, Sizable, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, AppContext, Context, Div, Entity, Focusable as _, InteractiveElement,
    ParentElement, StatefulInteractiveElement, Styled, Window,
};
use qingbird_core::repos::ChangeEntry;

use super::{dlg_cancel, footer, shell};
use crate::changes::changes_list;
use crate::state::*;
use crate::widgets::{ghost_variant, sunken_box};
use crate::tokens::*;

/// 变更摘要最多列几条。超出的在末尾给"还有 N 个…"。
/// 提交前要核对的是"该带的都带了"，不是逐个翻四百行文件名
const MAX_SUMMARY: usize = 60;

impl Shell {
    /// 打开「提交并推送…」。变更摘要在打开那一刻取**快照**：
    /// 对话框开着的时候 git 状态不会自己变，每帧重扫只会让列表闪
    ///
    /// `path` 给 `None` = 用当前选中。侧栏右键菜单要把对话框开在**它右键的那一行**
    /// 上：改用 `reveal_repo` 顺带切选中会让"提交 B"变成"切到 B 再提交"，
    /// 用户回到主区时选中已经换了，是他自己没要求的一次切换
    pub(crate) fn open_commit_dlg(
        &mut self,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = path.or_else(|| self.selected.clone()) else {
            return;
        };
        let changes: Vec<ChangeEntry> = self
            .view(&path)
            .map(|v| v.changes.clone())
            .unwrap_or_default();
        if changes.is_empty() {
            window.push_notification("没有可提交的改动", cx);
            return;
        }
        // 预填上次提交标题。用户多半是在改上一件事，从空开始想措辞
        // 比接着上一句改难得多（一条本地 git 命令，不走网络）
        let subject = qingbird_core::repos::last_commit_subject(&path);
        let message = cx.new(|cx| InputState::new(window, cx).placeholder("写一句这次改了什么"));
        if !subject.is_empty() {
            // set_value 要一个活的 `Window`，而 `Entity::update` 的闭包只给
            // `Context`。所以每一轮都从外层**重新借**一次 window——
            // 直接 `move` 进闭包会在第一轮就把 `window` 移走
            let w: &mut Window = window;
            message.update(cx, move |st, cx| st.set_value(subject, w, cx));
        }

        self.commit_dlg = Some(CommitDialog {
            path: path.clone(),
            message,
            // 默认勾上：本地落后时 push 必被拒（non-fast-forward），而"忘了先拉"
            // 是最常见的卡点。不勾也行，那条路留给只想提交的用户
            pull_first: true,
            changes,
            running: false,
            error: None,
        });
        // 焦点直接落在提交信息上——这是这个对话框里唯一必须动手的字段
        let fh = self
            .commit_dlg
            .as_ref()
            .map(|d| d.message.clone())
            .map(|m| m.read(cx).focus_handle(cx));
        if let Some(fh) = fh {
            window.focus(&fh, cx);
        }
        self.build_commit_dlg(window, cx);
    }
}

impl Shell {
    /// 对话框本体（官方 Dialog）。Shell 只提供状态。
    ///
    /// ⚠️ **整个 builder 每帧重跑**（`dialogs::shell` 的头注释有出处）：
    /// content 与 footer 都是每帧新的，状态每帧从 `Entity<Shell>` 读。
    /// 提交期间对话框不关（失败要留在原地看输出），转圈由日志区与结论行承担
    fn build_commit_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        let (path, message) = match self.commit_dlg.as_ref() {
            Some(d) => (d.path.clone(), d.message.clone()),
            None => return,
        };
        shell(
            window,
            cx,
            |_| "提交并推送".into(),
            None,
            {
                let this = this.clone();
                let message = message.clone();
                move |c: DialogContent, _w, cx: &mut App| {
                    let st = this.read(cx).commit_state();
                    c.child(commit_body(&cx.theme().clone(), &st, &message, &this))
                }
            },
            move |cx| commit_footer(&this, &path, &message, ghost_variant(cx)),
        );
    }
}

/// 对话框的只读快照（只带展示与判定要用的字段）。
///
/// `content` 闭包里只有 `&mut App`，改状态得走 `Entity::update`；
/// 展示函数一律收不可变的一份，读写两个口就分开了
struct CommitState {
    pull_first: bool,
    running: bool,
    error: Option<String>,
    changes: Vec<ChangeEntry>,
}

impl Shell {
    fn commit_state(&self) -> CommitState {
        match self.commit_dlg.as_ref() {
            Some(d) => CommitState {
                pull_first: d.pull_first,
                running: d.running,
                error: d.error.clone(),
                changes: d.changes.clone(),
            },
            None => CommitState {
                pull_first: true,
                running: false,
                error: None,
                changes: Vec::new(),
            },
        }
    }
}

fn commit_body(
    t: &Theme,
    st: &CommitState,
    message: &Entity<InputState>,
    this: &Entity<Shell>,
) -> Div {
    v_flex()
        .w_full()
        .gap(px(GAP_LG))
        .child(
            v_flex()
                .w_full()
                .gap(px(GAP_TIGHT))
                .child(
                    div()
                        .text_size(px(fs_meta()))
                        .text_color(tier4(t))
                        .child(format!("变更摘要 · {} 个文件", st.changes.len())),
                )
                .child(
                    // 只读内容 → `sunken_box`（`card-sunken` 底 + `line_soft` 边）。
                    // 原来这里自己写了 `border_color(t.colors.border)`——那是
                    // **主描边**档（#D7E0E4），比卡边还重一档，坐在全是 `track`
                    // 边的卡里就是"这一圈的边比卡的边粗"
                    sunken_box(div().w_full().max_h(px(220.0)), t)
                        .id("commit-changes-scroll")
                        .overflow_y_scroll()
                        .py(px(GAP_TIGHT))
                        .child(changes_list(
                            "commit-changes-list",
                            &st.changes,
                            t,
                            MAX_SUMMARY,
                        )),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .gap(px(GAP_TIGHT))
                .child(
                    div()
                        .text_size(px(fs_meta()))
                        .text_color(tier4(t))
                        .child("提交信息"),
                )
                .child(Input::new(message).w_full().small()),
        )
        // 「提交后拉取」。官方 Checkbox 的 on_change 只给 `&mut App`，
        // 所以回写走 `Entity` —— 与命令卡溢出菜单同一个套路
        .child(
            Checkbox::new("commit-pull-first")
                .checked(st.pull_first)
                .label("提交后先拉一次（本地落后时推送才不会被拒）")
                .disabled(st.running)
                .on_click({
                    let this = this.clone();
                    move |checked, _, cx| {
                        this.update(cx, |shell, cx| {
                            if let Some(d) = shell.commit_dlg.as_mut() {
                                d.pull_first = *checked;
                            }
                            cx.notify();
                        });
                    }
                }),
        )
        .when_some(st.error.clone(), |b, e| {
            b.child(
                div()
                    .w_full()
                    .text_size(px(fs_sec()))
                    .text_color(t.colors.danger)
                    .child(e),
            )
        })
}

fn commit_footer(this: &Entity<Shell>, path: &str, message: &Entity<InputState>, gv: ButtonCustomVariant) -> Div {
    let go = this.clone();
    let p = path.to_string();
    let msg = message.clone();
    footer(
        None,
        [
            dlg_cancel(
                "commit-cancel",
                "取消",
                this,
                |shell, _| shell.commit_dlg = None,
                gv,
            ),
            Button::new("commit-go")
                .label("提交并推送")
                .primary()
                .on_click(move |_, window, cx| {
                    let text = msg.read(cx).value().to_string();
                    let done = go.update(cx, |shell, cx| shell.run_commit(p.clone(), text, cx));
                    // 成功才关。失败留在原地，错误与输出都在日志区，
                    // 用户填好的提交信息也还在
                    if done {
                        window.close_dialog(cx);
                    }
                }),
        ],
    )
}
