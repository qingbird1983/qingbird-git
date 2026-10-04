//! 命令编辑对话框：四个固定槽位 + 三个钉住命令（每个「文案 / 命令」两格）。
//!
//! **十个字段用一张标签表 + 一个平行数组**（`state::CMD_FIELDS`），不写十个具名字段。
//! 具名的话「读出来 → 交给 `set_repo_cmds` / `set_pinned_cmds`」那段要写十遍，
//! 漏一处的后果是某个槽位的命令被存到别的槽位上——那是个很难被发现的 bug：
//! 命令能跑、结果也对，只是跑的始终是另一条。
//!
//! 钉住命令的**文案必须单独一格**，不能只存命令让 core 兜底：
//! 已经存过自定义文案的仓库，光把命令存回去会把它改回"前两个词"，
//! 那是在用户没要求的情况下抹掉他起过的名字。
//!
//! 全部字段**一次预填当前值**（`InputState::set_value`）。这里「留空 = 清空」，
//! 与「编辑仓库」对话框里「留空 = 不改」是**相反**的约定，所以副标题写明了。

use gpui_kit::component::button::ButtonCustomVariant;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Sizable, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, AppContext, Context, Div, Entity, Focusable as _, FontWeight, ParentElement,
    Styled, Window,
};
use qingbird_core::run_cmd::{self, Slot};

use crate::state::*;
use crate::widgets::ghost_variant;
use crate::tokens::*;

/// 四个固定槽位按这个顺序排。⌘K「当前仓库」组里的「跑 X」按 `Dev / Build /
/// Test / Install` 排，**两处顺序不同是故意的**：这里排的是"检查 → 构建 →
/// 开发 → 装依赖"这条排查动线，日志表头那两颗与 ⌘K 排的是"开发 → 构建"
/// 这条日常动线
const SLOTS: [Slot; 4] = [Slot::Test, Slot::Build, Slot::Dev, Slot::Install];

impl Shell {
    /// 打开命令编辑。`focus` 决定焦点落在哪个槽位上——
    /// 从命令行右上角的「⋯」进来时落在那一条，从卡片头的「编辑」进来时落在第一个
    pub(crate) fn open_cmds_dlg(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
        focus: Option<Slot>,
    ) {
        let Some(entry) = self.entry(&path).cloned() else {
            return;
        };
        let detected = self.view(&path).map(|v| v.detected.clone());

        // 前四格：存了就用存的，没存就预填自动识别的结果（不落盘）
        let mut values: Vec<String> = SLOTS
            .iter()
            .map(|s| {
                let saved = s.cmd(&entry).trim().to_string();
                if !saved.is_empty() {
                    return saved;
                }
                match (s, &detected) {
                    (Slot::Test, Some(d)) => d.test_cmd.clone(),
                    (Slot::Build, Some(d)) => d.build_cmd.clone(),
                    (Slot::Dev, Some(d)) => d.dev_cmd.clone(),
                    (Slot::Install, Some(d)) => d.install_cmd.clone(),
                    _ => String::new(),
                }
            })
            .collect();
        // 后六格：三个钉住命令的「文案 / 命令」成对
        for i in 0..run_cmd::MAX_PINNED_CMDS {
            let c = entry.pinned_cmds.get(i);
            values.push(c.map(|c| c.label.clone()).unwrap_or_default());
            values.push(c.map(|c| c.cmd.clone()).unwrap_or_default());
        }

        let inputs: Vec<Entity<InputState>> = values
            .iter()
            .map(|_| cx.new(|cx| InputState::new(window, cx).placeholder("留空即清空")))
            .collect();
        // 值要在 Entity 建好之后才塞进去：`InputState::new` 只接 placeholder
        for (ent, v) in inputs.iter().zip(values.iter()) {
            if !v.is_empty() {
                let v = v.clone();
                // set_value 要一个活的 `Window`，而 `Entity::update` 的闭包
                // 只给 `Context`。所以每一轮都从外层**重新借**一次 window——
                // 直接 `move` 进闭包会在第一轮就把 `window` 移走
                let w: &mut Window = window;
                ent.update(cx, move |st, cx| st.set_value(v, w, cx));
            }
        }

        self.cmd_dlg = Some(CmdDialog {
            path: path.clone(),
            inputs,
            error: None,
        });
        if let Some(ix) = focus.and_then(|s| focus_ix(s)).and_then(|i| {
            self.cmd_dlg
                .as_ref()
                .and_then(|d| d.inputs.get(i))
                .map(|e| e.read(cx).focus_handle(cx))
        }) {
            window.focus(&ix, cx);
        }
        self.build_cmds_dlg(window, cx);
    }
}

/// 槽位 → 字段下标。`Pinned(i)` 指到**命令**那一格（不是文案）：
/// 用户点某一行的「⋯」是为了改那条命令，文案不是他来的原因
fn focus_ix(slot: Slot) -> Option<usize> {
    match slot {
        Slot::Pinned(i) => Some(4 + i * 2 + 1),
        other => SLOTS.iter().position(|x| *x == other),
    }
}

impl Shell {
    fn build_cmds_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        let path = match self.cmd_dlg.as_ref() {
            Some(d) => d.path.clone(),
            None => return,
        };
        window.open_dialog(cx, move |dialog, _window, cx| {
            let t = cx.theme().clone();
            dialog
                .title("编辑命令")
                .content({
                    let t = t.clone();
                    let this = this.clone();
                    move |c: DialogContent, _w, cx: &mut App| {
                        let st = this.read(cx).cmd_state();
                        c.child(cmds_body(&t, &st))
                    }
                })
                .footer(cmds_footer(&this, &path, ghost_variant(cx)))
        });
    }
}

/// 对话框的只读快照。`content` 闭包里只有 `&mut App`，展示函数一律收
/// 不可变的一份，读写两个口就分开了
pub(crate) struct CmdState {
    inputs: Vec<Entity<InputState>>,
    error: Option<String>,
}

impl Shell {
    fn cmd_state(&self) -> CmdState {
        match self.cmd_dlg.as_ref() {
            Some(d) => CmdState {
                inputs: d.inputs.clone(),
                error: d.error.clone(),
            },
            None => CmdState {
                inputs: Vec::new(),
                error: None,
            },
        }
    }
}

fn cmds_body(t: &Theme, st: &CmdState) -> Div {
    let field = |label: &str, input: &Entity<InputState>| -> Div {
        v_flex()
            .w_full()
            .gap(px(GAP_TIGHT))
            .child(
                div()
                    .text_size(px(FS_META))
                    .text_color(tier4(t))
                    .child(label.to_string()),
            )
            .child(Input::new(input).w_full().small())
    };

    let mut body = v_flex().w_full().gap(px(GAP)).child(
        div()
            .text_size(px(FS_META))
            .text_color(tier4(t))
            .child("留空即清空这一条。前四格预填的是自动识别的结果，不点保存不会写进配置。"),
    );
    for ix in 0..CMD_FIELDS.len() {
        if ix == 4 {
            body = body.child(
                div()
                    .pt(px(GAP_TIGHT))
                    .text_size(px(FS_META))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(tier3(t))
                    .child("钉住的自定义命令（上限 3 个）"),
            );
        }
        body = body.child(field(CMD_FIELDS[ix], &st.inputs[ix]));
    }
    body.when_some(st.error.clone(), |b, e| {
        b.child(
            div()
                .w_full()
                .text_size(px(FS_SEC))
                .text_color(t.colors.danger)
                .child(e),
        )
    })
}

fn cmds_footer(this: &Entity<Shell>, path: &str, gv: ButtonCustomVariant) -> Div {
    let cancel = this.clone();
    let save = this.clone();
    let p = path.to_string();
    h_flex()
        .w_full()
        .justify_end()
        .gap(px(GAP))
        .child(
            Button::new("cmds-cancel")
                .label("取消")
                .custom(gv)
                .on_click(move |_, window, cx| {
                    cancel.update(cx, |shell, _| shell.cmd_dlg = None);
                    window.close_dialog(cx);
                }),
        )
        .child(Button::new("cmds-save").label("保存").primary().on_click({
            let save = save.clone();
            move |_, window, cx| {
                let vals: Vec<String> = save
                    .read(cx)
                    .cmd_dlg
                    .as_ref()
                    .map(|d| {
                        d.inputs
                            .iter()
                            .map(|e| e.read(cx).value().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                if save.update(cx, |shell, cx| shell.save_cmds(p.clone(), vals, cx)) {
                    window.close_dialog(cx);
                }
            }
        }))
}
