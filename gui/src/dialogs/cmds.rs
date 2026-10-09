//! 命令编辑对话框：四个固定槽位（每行「自动 / 启动」两颗键）+ 三个钉住命令
//! （每个「文案 / 命令」两格）。
//!
//! **每行两颗键是老版就有、重写时漏掉的**（2026-10-09 用户第 8 条）：
//! 「自动」= 把这一格填回自动识别的结果并保存（不运行），「启动」= 保存这一屏
//! 并后台跑这一条。两版之间不是"改版删了功能"——`git show da6a3fb^:src/components/CmdPanel.tsx`
//! 里 `restoreDetected` / `saveAndRun` 都在，GPUI 这一版**从来没写过**这两颗，
//! 只留了一个总「保存」。所以这里是**补齐**，不是回滚。
//!
//! **钉住命令那六格不做「+ 添加按钮 / 移除」**：老版是动态行，而 core 的
//! `sanitize_pinned` 本来就把「命令为空」的行丢掉、文案为空时自动取前两个词
//! ⇒ 「清空命令那一格」在这两版里都是移除，动态行只是把同一件事换个入口。
//! 副标题那句话因此写成了「命令留空即移除这一颗」，让约定在界面上说出来。
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
use gpui_kit::component::{ActiveTheme, Disableable as _, Size, Sizable, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, AppContext, Context, Div, Entity, Focusable as _, FontWeight, Hsla,
    ParentElement, SharedString, Styled, Window,
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

        // 四格**已存的**命令（去首尾空白）。留一份快照给状态词与「自动」用
        let saved: Vec<String> = SLOTS
            .iter()
            .map(|s| s.cmd(&entry).trim().to_string())
            .collect();
        // 四格**识别到的**命令。识别失败（`detected` 为 None，档案还没扫过）时四格全空，
        // 于是「自动」四颗都是灰的——这比让它们点了没反应诚实
        let detected_cmds: Vec<String> = SLOTS
            .iter()
            .map(|s| match (s, &detected) {
                (Slot::Test, Some(d)) => d.test_cmd.clone(),
                (Slot::Build, Some(d)) => d.build_cmd.clone(),
                (Slot::Dev, Some(d)) => d.dev_cmd.clone(),
                (Slot::Install, Some(d)) => d.install_cmd.clone(),
                _ => String::new(),
            })
            .collect();
        // 前四格的初值：存了用存的，没存用识别的（**不落盘**）
        let mut values: Vec<String> = (0..SLOTS.len())
            .map(|i| {
                if saved[i].is_empty() {
                    detected_cmds[i].clone()
                } else {
                    saved[i].clone()
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
            saved,
            detected: detected_cmds,
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
                    // 幽灵按钮的悬停底色要从活的 `App` 上取，而 content 的闭包里
                    // 那个 `cx` 是**每一帧重新给**的 ⇒ 在这里算一次带进去
                    // （与 `repo.rs` 的 footer 同一写法）
                    let gv = ghost_variant(cx);
                    move |c: DialogContent, _w, cx: &mut App| {
                        let st = this.read(cx).cmd_state();
                        c.child(cmds_body(&t, &st, &this, gv, cx))
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
    /// 打开那一刻四槽位**已存的**命令，状态词「未保存 / 已自定义」拿它比
    saved: Vec<String>,
    /// 四槽位**识别到的**命令，「自动」那颗键回填它
    detected: Vec<String>,
    /// 这一条仓库正有命令在后台跑 ⇒ 四行的「自动 / 启动」全部灰掉。
    /// 老版就是这条纪律（`busy = running || !!cmdRunning`）：一边跑一边再点
    /// 会把第二条塞进同一个日志窗口，看起来像"输出混了"
    busy: bool,
}

impl Shell {
    fn cmd_state(&self) -> CmdState {
        match self.cmd_dlg.as_ref() {
            Some(d) => CmdState {
                inputs: d.inputs.clone(),
                error: d.error.clone(),
                saved: d.saved.clone(),
                detected: d.detected.clone(),
                busy: self.log().running,
            },
            None => CmdState {
                inputs: Vec::new(),
                error: None,
                saved: Vec::new(),
                detected: Vec::new(),
                busy: false,
            },
        }
    }
}

/// 一行右侧那句状态。**先分"配置里有没有"，再分"框里的是不是它"**，四档互斥：
///
/// | 存着的 | 框里的 | 说 |
/// |---|---|---|
/// | 非空 | 与存着的一致 | 「已自定义」 |
/// | 非空 | 不一致（含被清空） | 「未保存」 |
/// | 空 | 空 · 识别到了 | 「自动识别：x」（这条能点「自动」拿） |
/// | 空 | 与识别的一致 | 「自动识别：x」（预填进来的，**还没进配置**） |
/// | 空 | 空 · 也没识别到 | 「未识别到命令」 |
/// | 空 | 别的 | 「未保存」 |
///
/// ⚠️ **第一刀切在「存着的」而不是「框里的」**：老版（React 的 `CmdPanel`）先比
/// `cur != saved`，而它那里识别结果是**异步**灌进来的，于是"打开面板、一个字没动"
/// 的四格里有三格写着「未保存」——那是在说假话：用户没保存是因为他还没点。
/// 这一版四格在**打开时**就同步预填好了（`open_cmds_dlg`），所以「预填但没存」是
/// **默认状态而不是改动**，必须落到「自动识别」那一档，否则这一屏从头到尾都在
/// 管用户叫"你有改动没存"
///
/// 抽成纯函数是因为它要读**三个**来源（当前值 / 快照 / 识别结果），
/// 摆在渲染闭包里写就是"只有抓帧能验"的六条分支；收成函数后每档各有单测
fn slot_state_word(cur: &str, saved: &str, detected: &str) -> String {
    let cur = cur.trim();
    if !saved.is_empty() {
        return if cur == saved {
            "已自定义".to_string()
        } else {
            "未保存".to_string()
        };
    }
    if detected.is_empty() {
        return if cur.is_empty() {
            "未识别到命令".to_string()
        } else {
            "未保存".to_string()
        };
    }
    if cur.is_empty() || cur == detected {
        format!("自动识别：{detected}")
    } else {
        "未保存".to_string()
    }
}

/// 四槽位那一行的状态词用哪一档的灰：「未保存」是**要他动手**的信号，
/// 其余三档只是说明。与总览「状态」列同一条纪律——中性态不许穿行动色
fn slot_word_color(word: &str, t: &Theme) -> Hsla {
    if word == "未保存" {
        orange_text(t)
    } else {
        tier4(t)
    }
}

fn cmds_body(
    t: &Theme,
    st: &CmdState,
    this: &Entity<Shell>,
    gv: ButtonCustomVariant,
    cx: &mut App,
) -> Div {
    let mut body = v_flex().w_full().gap(px(GAP)).child(
        div()
            .text_size(px(fs_meta()))
            .text_color(tier4(t))
            .child("留空即清空这一条。前四格预填的是自动识别的结果，不点保存不会写进配置。"),
    );
    for ix in 0..SLOTS.len() {
        body = body.child(slot_field(t, st, ix, this, gv, cx));
    }
    body = body.child(
        div()
            .pt(px(GAP_TIGHT))
            .text_size(px(fs_meta()))
            .font_weight(FontWeight::MEDIUM)
            .text_color(tier3(t))
            .child("钉住的自定义命令（上限 3 个 · 命令留空即移除这一颗）"),
    );
    for ix in SLOTS.len()..CMD_FIELDS.len() {
        body = body.child(plain_field(t, st, ix));
    }
    body.when_some(st.error.clone(), |b, e| {
        b.child(
            div()
                .w_full()
                .text_size(px(fs_sec()))
                .text_color(t.colors.danger)
                .child(e),
        )
    })
}

/// 一个标签 + 一个输入框，右侧没有键。钉住命令那六格用这个
fn plain_field(t: &Theme, st: &CmdState, ix: usize) -> Div {
    v_flex()
        .w_full()
        .gap(px(GAP_TIGHT))
        .child(
            div()
                .text_size(px(fs_meta()))
                .text_color(tier4(t))
                .child(CMD_FIELDS[ix].to_string()),
        )
        .child(Input::new(&st.inputs[ix]).w_full().small())
}

/// 四槽位之一：标签 + 状态词 一行，输入框 + 「自动」+「启动」一行。
///
/// ⚠️ **输入框那一行必须是 `h_flex` 且输入框带 `w_full().flex_1().min_w_0()`**——
/// 这一列没有 `w_full` 的话，里面那颗 `flex_1` 会塌成 0 宽（AGENTS 里
/// `FormRows` 那条踩过的同款），帧上的症状是「地址格只剩一个椭圆」。
fn slot_field(
    t: &Theme,
    st: &CmdState,
    ix: usize,
    this: &Entity<Shell>,
    gv: ButtonCustomVariant,
    cx: &mut App,
) -> Div {
    let input = &st.inputs[ix];
    let cur = input.read(cx).value().to_string();
    let saved = st.saved.get(ix).map(String::as_str).unwrap_or("");
    let detected = st
        .detected
        .get(ix)
        .map(String::as_str)
        .unwrap_or("")
        .to_string();
    let word = slot_state_word(&cur, saved, &detected);

    v_flex()
        .w_full()
        .gap(px(GAP_TIGHT))
        .child(
            h_flex()
                .w_full()
                .gap(px(GAP_TIGHT))
                .items_center()
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(fs_meta()))
                        .text_color(tier3(t))
                        .child(CMD_FIELDS[ix].to_string()),
                )
                // 状态词这一格**必须能截断**：它可能是「自动识别：pnpm run build --foo…」
                // 这种长句，而定宽标签在左边、右边那两颗键在下一行——这一格不给
                // `min_w_0` + `truncate`，长句会把这一行的右边界顶穿
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(fs_meta()))
                        .text_color(slot_word_color(&word, t))
                        .child(word),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .gap(px(GAP))
                .items_center()
                .child(Input::new(input).w_full().flex_1().min_w_0().small())
                .child(
                    Button::new(SharedString::from(format!("cmds-auto-{ix}")))
                        .label("自动")
                        .custom(gv)
                        .with_size(Size::Small)
                        .flex_shrink_0()
                        .disabled(st.busy || detected.is_empty())
                        .tooltip("填入自动识别到的这条并保存（不运行）")
                        .accessibility_label("填入自动识别的命令")
                        .on_click({
                            let this = this.clone();
                            move |_, window, cx| {
                                this.update(cx, |shell, cx| {
                                    shell.cmds_restore(ix, window, cx)
                                });
                            }
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("cmds-run-{ix}")))
                        .label("启动")
                        .secondary()
                        .with_size(Size::Small)
                        .flex_shrink_0()
                        // 空的那条没有可跑的东西。`loading` 不给这颗键——
                        // 它不是"这个操作在进行中别再点"，而是"我来起跑"，
                        // 而 D64 那条纪律是进行中**不许再点**，用 `disabled(busy)` 表达
                        .disabled(st.busy || cur.trim().is_empty())
                        .tooltip("保存并后台运行这一条")
                        .accessibility_label("保存并运行这一条命令")
                        .on_click({
                            let this = this.clone();
                            move |_, window, cx| {
                                let started =
                                    this.update(cx, |shell, cx| shell.cmds_run(ix, cx));
                                // 只有真的起跑了才关窗：保存失败要留在原地看错误
                                if started {
                                    window.close_dialog(cx);
                                }
                            }
                        }),
                ),
        )
}

impl Shell {
    /// 「自动」：把这一格填回自动识别的结果**并保存**，不运行。
    /// 其余三格与钉住命令原样带着走（`save_cmds` 一次写十个字段，
    /// 所以这里必须把**全部**当前值读齐再交出去——少交一格就是抹掉一格）
    pub(crate) fn cmds_restore(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.cmd_dlg.as_ref() else {
            return;
        };
        let (Some(detected), Some(input)) = (d.detected.get(ix).cloned(), d.inputs.get(ix).cloned())
        else {
            return;
        };
        if detected.trim().is_empty() {
            return;
        }
        let inputs = d.inputs.clone();
        let path = d.path.clone();
        input.update(cx, |st, cx| st.set_value(detected, window, cx));
        let vals: Vec<String> = inputs
            .iter()
            .map(|e| e.read(cx).value().to_string())
            .collect();
        self.save_cmds(path, vals, cx);
    }

    /// 「启动」：先保存十个字段，保存成了才起跑这一条并关窗。
    /// 返回 `true` = 已经起跑（调用方据此关弹窗）
    pub(crate) fn cmds_run(&mut self, ix: usize, cx: &mut Context<Self>) -> bool {
        let Some(d) = self.cmd_dlg.as_ref() else {
            return false;
        };
        let Some(slot) = SLOTS.get(ix).copied() else {
            return false;
        };
        let inputs = d.inputs.clone();
        let path = d.path.clone();
        let vals: Vec<String> = inputs.iter().map(|e| e.read(cx).value().to_string()).collect();
        if vals.get(ix).map(|v| v.trim().is_empty()).unwrap_or(true) {
            return false;
        }
        if !self.save_cmds(path.clone(), vals, cx) {
            return false;
        }
        // 跑的是**这一条仓库**，不是"当前选中的那条"：弹窗开着时选中项可以被切走
        self.run_slot_at(&path, slot, cx);
        true
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    // 「自动 / 启动 / 状态词」这一行的四档判据。它要同时读三个来源
    // （输入框当前值 / 打开时的快照 / 识别结果），摆在渲染闭包里就只剩抓帧能验，
    // 而抓帧一次只看得见一档。抽成纯函数后四档各有其名
    #[test]
    fn slot_state_word_prefers_unsaved_only_over_a_saved_value() {
        // 有存值、框里改了 → 「未保存」
        assert_eq!(slot_state_word("pnpm t", "npm t", "vitest"), "未保存");
        // 存值被清空也是改动，不许退回「未识别到命令」
        assert_eq!(slot_state_word("   ", "npm t", ""), "未保存");
        // 首尾空白不算改动（保存时一律 `trim`，不 trim 会永远停在「未保存」）
        assert_eq!(slot_state_word("npm t  ", "npm t", ""), "已自定义");
    }

    #[test]
    fn slot_state_word_distinguishes_saved_from_detected() {
        assert_eq!(slot_state_word("npm t", "npm t", "vitest"), "已自定义");
        assert_eq!(slot_state_word("vitest", "", "vitest"), "自动识别：vitest");
        assert_eq!(slot_state_word("", "", ""), "未识别到命令");
    }

    #[test]
    fn prefill_but_not_saved_says_detected_not_unsaved() {
        // **这一档是这一屏的默认状态**：打开面板时识别结果已经填进框里，
        // 用户一个字没动。老版先比 `cur != saved`，于是这种帧上满屏「未保存」，
        // 而副标题明写着"不点保存不会写进配置"——两句互相打脸
        assert_eq!(
            slot_state_word("vitest run", "", "vitest run"),
            "自动识别：vitest run"
        );
        // 识别到了、那一格是空的（他自己清掉了）：仍然告诉他有什么可拿
        assert_eq!(slot_state_word("", "", "vitest run"), "自动识别：vitest run");
        // 识别到了、框里是**另一条** → 这才是他改的
        assert_eq!(slot_state_word("make", "", "vitest run"), "未保存");
    }
}
