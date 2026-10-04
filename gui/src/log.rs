//! 日志区：表头 / 状态行 / 进度条 / 输出流 / 结论行。
//!
//! **2026-10-05 起它是概览页的第一段，也是那一屏占最大、最重要的一块。**
//! 走过的三步：检视抽屉第一页签 → 概览页下半 → 提到最上的独立一段。
//! 每一步都是因为右栏与中栏撞了车（同一份数据两个入口），
//! 而不是它自己变好或变坏了。
//!
//! **表头挂五颗键，是这一段存在的理由。** 复制 / 清空 / 终端 / 开发运行 /
//! 编译构建 全在这一行上：它们是「从刚才那次操作接着往下走」的五种走法。
//! 拆散到别处，每一种都要用户自己先找到它、再点一次
//!
//! **浅色输出流，不要深色代码块。** 早期版本拿墨色底当终端用，但这一屏
//! 的读者是在看 git 的结论，不是在读代码——深色块在整屏灰白里是一块黑洞，
//! 而且中文在里面掉字重。等宽字体只需要在这一小段里用。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Disableable as _, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, ClipboardItem, Context, Div, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled,
};
use qingbird_core::run_cmd::Slot;

use crate::os;
use crate::shell::{OpenTerminal, StopOp};
use crate::state::*;
use crate::widgets::{card, ghost_variant, meta_text, vdivider, CardHeader};
use crate::tokens::*;

/// 表头上**常显**的两条命令键。**这一对就是全部的常驻命令入口**：
/// 「常用命令」段 2026-10-05 撤掉了（同一对入口摆两遍），测试 / 安装依赖 /
/// 钉住的命令与它们的编辑入口都在 ⌘K（`palette.rs` 的 `RunSlot` 与 `EditCmds`）
pub(crate) const QUICK_SLOTS: [(Slot, &str); 2] =
    [(Slot::Dev, "开发运行"), (Slot::Build, "编译构建")];

impl Shell {
    /// 日志区本体。**概览页第一段**（`detail_body` 的 Overview 分支）。
    ///
    /// 两类仓库（我的 / 第三方）**走的是同一个组件、同一套状态机**：
    /// 变的只是"跑什么会往里写"，而那是 `ops.rs` 的事。
    ///
    /// **高度由外层管**：这里只给 `flex_1 + min_h_0`，`detail.rs` 负责
    /// 让它吃掉剩余高度（`LOG_MIN_H` 是它的下限）
    pub(crate) fn log_panel(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let Some(path) = self.selected.clone() else {
            return card(t)
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .child(meta_text("先选一个仓库", t));
        };
        let lg = self.logs.get(&path).unwrap_or(&LOG_EMPTY);

        card(t)
            .flex_1()
            .min_h_0()
            .child(self.log_header(path.as_str(), lg, t, cx))
            .child(self.log_status_row(t, lg, cx))
            .when(lg.running, |col| col.child(progress_bar(t, lg)))
            .child(output_stream(
                "log-stream",
                t,
                lg,
                "点同步三键或命令行的按钮，输出会实时回显在这里",
            ))
            .child(self.log_verdict_row(t, lg))
    }
}

// ================= 表头 =================

impl Shell {
    /// 表头：左边「日志」，右边五颗键。
    ///
    /// **五颗键用一条竖细线分成两组**：左边三颗（复制 / 清空 / 终端）是
    /// "看刚才那次结果"，右边两颗（开发运行 / 编译构建）是"接着再跑一个"。
    /// 不分组的话五颗同规格的键排一行，视线扫过去分不出哪颗是哪颗
    fn log_header(&self, path: &str, lg: &LogState, t: &Theme, cx: &mut Context<Self>) -> Div {
        let text = lg.lines.join("\n");
        let has_text = !text.is_empty();
        let mut hdr = CardHeader::new("日志")
            .right(
                Button::new("log-copy")
                    .label("复制")
                    .custom(ghost_variant(cx))
                    .disabled(!has_text)
                    .with_size(Size::XSmall)
                    .tooltip("复制全部输出")
                    .on_click(cx.listener(move |_, _, window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        window.push_notification("已复制全部输出", cx);
                    }))
                    .into_any_element(),
            )
            .right(
                Button::new("log-clear")
                    .label("清空")
                    .custom(ghost_variant(cx))
                    .disabled(!has_text)
                    .with_size(Size::XSmall)
                    .tooltip("清空输出（不影响正在跑的操作）")
                    .on_click(cx.listener(|this, _, _, cx| this.clear_log(cx)))
                    .into_any_element(),
            )
            .right(vdivider(t).into_any_element())
            .right(
                // 官方 `tooltip_with_action` 会自己去 keymap 里查 `Ctrl T`
                // 并画成 `Kbd` 角标。手写「（Ctrl T）」进文案的话，
                // 键位一改那句话就成了假提示
                Button::new("log-term")
                    .label("终端")
                    .custom(ghost_variant(cx))
                    .with_size(Size::XSmall)
                    .tooltip_with_action("在终端里 cd 到这个仓库", &OpenTerminal, None)
                    .accessibility_label("在终端中打开这个仓库")
                    .on_click({
                        let path = path.to_string();
                        cx.listener(move |_, _, window, cx| {
                            if let Err(e) = os::open_terminal(&path) {
                                window.push_notification(format!("打开终端失败：{e}"), cx);
                            }
                        })
                    })
                    .into_any_element(),
            );
        for (slot, label) in QUICK_SLOTS {
            let cmd = self.effective_cmd(path, slot);
            hdr = hdr.right(
                Button::new(SharedString::from(format!("log-cmd-{}", slot.key())))
                    .label(label)
                    .custom(ghost_variant(cx))
                    .with_size(Size::XSmall)
                    .loading(lg.running && lg.op == Some(Op::Cmd(slot)))
                    .disabled(lg.running)
                    .tooltip(SharedString::from(if cmd.is_empty() {
                        "这个仓库还没认出这条命令，按 Ctrl K 搜「编辑常用命令」里填".to_string()
                    } else {
                        cmd
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.run_slot(slot, cx)))
                    .into_any_element(),
            );
        }
        hdr.build(t)
    }
}

// ================= 状态行 =================

impl Shell {
    /// 「正在拉取…」/「跑测试 · 2 分钟前」+ 停止按钮。
    /// 停止按钮只在跑的时候出现——平时摆一个点不动的停止键只会让人去试它
    fn log_status_row(&self, t: &Theme, lg: &LogState, cx: &mut Context<Self>) -> Div {
        let text = if lg.running {
            match lg.op {
                Some(Op::Cmd(s)) => format!("{} 进行中…", s.default_label()),
                Some(Op::Pull) => "拉取中…".to_string(),
                Some(Op::Push) => "推送中…".to_string(),
                Some(Op::Check) => "检测更新中…".to_string(),
                Some(Op::Rebase(_)) => "变基中…".to_string(),
                Some(Op::UndoRebase) => "撤销变基中…".to_string(),
                Some(Op::Commit) => "正在提交并推送".to_string(),
                None => "进行中…".to_string(),
            }
        } else if lg.ts == 0 {
            "还没有操作记录".to_string()
        } else {
            match lg.op {
                Some(Op::Cmd(s)) => format!("{} · {}", s.default_label(), rel_time(lg.ts)),
                Some(Op::Pull) => format!("拉取 · {}", rel_time(lg.ts)),
                Some(Op::Push) => format!("推送 · {}", rel_time(lg.ts)),
                Some(Op::Check) => format!("检测更新 · {}", rel_time(lg.ts)),
                Some(Op::Rebase(_)) => format!("变基 · {}", rel_time(lg.ts)),
                Some(Op::UndoRebase) => format!("撤销变基 · {}", rel_time(lg.ts)),
                Some(Op::Commit) => format!("提交并推送 · {}", rel_time(lg.ts)),
                None => rel_time(lg.ts),
            }
        };

        let spin = Spinner::new().color(tier3(t)).with_size(Size::XSmall);
        h_flex()
            .w_full()
            .h(px(20.0))
            .flex_shrink_0()
            .gap(px(GAP_TIGHT))
            .items_center()
            .when(lg.running, |row| row.child(spin))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(FS_SEC))
                    .text_color(if lg.running { tier3(t) } else { tier4(t) })
                    .child(text),
            )
            .when(lg.running, |row| {
                row.child(
                    Button::new("log-stop")
                        .label("停止")
                        .custom(ghost_variant(cx))
                        .with_size(Size::XSmall)
                        .tooltip_with_action("停止当前操作", &StopOp, None)
                        .accessibility_label("停止当前操作")
                        .on_click(cx.listener(|this, _, window, cx| this.stop_current(window, cx))),
                )
            })
    }
}

/// 进度条。**有 `percent` 走确定态，没有走不定态**——
/// 跑命令给不出可信的百分比，造一个只会让人以为快好了，然后越等越焦。
/// 官方 `Progress::loading(true)` 自带来回扫的动画，不另写 `with_animation`
///
/// ⚠️ 官方 `Progress` **没有实现 `FluentBuilder`**，所以这里不能用 `.when()`
/// 分支，只能先算出 `value` 再一次成型
fn progress_bar(t: &Theme, lg: &LogState) -> impl IntoElement {
    let bar = Progress::new("log-progress")
        .w_full()
        .with_size(Size::XSmall)
        .color(t.colors.accent)
        .loading(lg.percent.is_none())
        .accessibility_label(lg.stage.clone().unwrap_or_else(|| "操作进行中".to_string()));
    match lg.percent {
        Some(pct) => bar.value(pct).into_any_element(),
        None => bar.into_any_element(),
    }
}

// ================= 输出流 =================

/// 输出流：浅色底 + 1px 边 + 等宽字体。
///
/// **那圈里框只在有内容时画**（2026-10-05）。它的作用是把等宽输出与白卡分开；
/// 空态时它只剩一个套在白卡里的空盒子，所以空态走另一条分支，只留居中提示语。
///
/// **错误行只给前缀标色，不整段染红。** 整段红会把整块输出变成一片警示墙，
/// 真正要看的报错那几行反而被淹掉；只标 `✗` 前缀，视线自然落到行首
///
/// `pub(crate)` 且收空态文案与元素 id：批量汇总（`app_log`）也是一份 `LogState`，
/// 也要用这一段渲染。两处各画一份的话，改配色 / 间距只改一处，
/// 另一个对话框会留在旧样式上。
///
/// **id 必须由调用方给**：gpui 的滚动状态是按 `ElementId` 全局存的，
/// 两个元素共用一个 id 会共用同一个滚动句柄——在批量结果对话框里滚一下，
/// 底下检视抽屉的日志会跟着跳
pub(crate) fn output_stream(
    id: &'static str,
    t: &Theme,
    lg: &LogState,
    empty_hint: &str,
) -> impl IntoElement {
    let mut area = v_flex()
        .id(id)
        .w_full()
        .flex_1()
        .min_h(px(120.0))
        .min_h_0()
        .overflow_y_scroll()
        .p(px(GAP))
        .gap(px(GAP_TIGHT))
        .rounded(px(R_MD))
        // 浅色底 + 细边，跟白卡片之间靠这一层边界分开，不靠阴影堆层次
        .bg(t.colors.muted)
        .border_1()
        .border_color(t.colors.border)
        .font_family(t.mono_font_family.clone());

    if lg.lines.is_empty() {
        // 空态**不画里框**：那圈 1px 边 + 浅灰底在有内容时干的是"把等宽输出与
        // 白卡分开"，空的时候它只剩一个套在白卡里的空盒子，白卡上凭空多出一层。
        // 这里只留一句居中的提示语，底就是卡底。
        // ⚠️ 也**不继承等宽字体**：`area` 上挂的是 `mono_font_family`，
        // 而 Windows 默认等宽是 Consolas、不含汉字，提示语一落进去就掉字重
        // （读起来像换了一款字）
        return v_flex()
            .flex_1()
            .min_h(px(120.0))
            .min_h_0()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_size(px(FS_META))
                    .text_color(tier4(t))
                    .child(empty_hint.to_string()),
            )
            .into_any_element();
    }
    for line in &lg.lines {
        // 前缀单独占一列：只给**标记**上色，文字仍用正文色。
        // 整段染红会把整块输出变成一片警示墙，真正的报错那几行反而被淹掉；
        // 只标行首，视线自然落到那里
        let (mark, color) = match line.chars().next() {
            Some('✗') => (Some('✗'), t.colors.danger),
            Some('✓') => (Some('✓'), t.colors.success),
            _ => (None, tier3(t)),
        };
        let body = match mark {
            Some(_) => {
                line[line.char_indices().nth(1).map_or(line.len(), |(i, _)| i)..].trim_start()
            }
            None => line.as_str(),
        };
        area = area.child(
            h_flex()
                .w_full()
                .gap(px(GAP_TIGHT))
                .items_start()
                .child(
                    div()
                        .w(px(10.0))
                        .flex_shrink_0()
                        .text_size(px(FS_META))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color)
                        .child(mark.map(|c| c.to_string()).unwrap_or_default()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(FS_META))
                        .text_color(if mark.is_some() { tier3(t) } else { color })
                        .child(body.to_string()),
                ),
        );
    }
    area.into_any_element()
}

// ================= 结论行 =================

impl Shell {
    /// 结论行：成功 ✓ / 失败 ✗ + 走代理标记。
    /// 「走代理」单列是因为它是"连上了但绕了一圈"——混在成功里会让人以为一切正常
    fn log_verdict_row(&self, t: &Theme, lg: &LogState) -> Div {
        let mut row = v_flex().w_full().flex_shrink_0().gap(px(GAP_TIGHT));

        if let Some(e) = &lg.error {
            row = row.child(
                h_flex()
                    .w_full()
                    .gap(px(GAP_TIGHT))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(FS_SEC))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.colors.danger)
                            .child("✗"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(FS_SEC))
                            .text_color(t.colors.danger)
                            .child(e.clone()),
                    ),
            );
        }

        if let Some((ok, msg)) = &lg.verdict {
            let color = if *ok {
                t.colors.success
            } else {
                t.colors.danger
            };
            row = row.child(
                h_flex()
                    .w_full()
                    .gap(px(GAP_TIGHT))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(FS_SEC))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color)
                            .child(if *ok { "✓" } else { "✗" }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(FS_SEC))
                            .text_color(tier3(t))
                            .child(msg.clone()),
                    )
                    .when_some(lg.used_proxy.clone(), |r, proxy| {
                        // 地址直接写在胶囊上：这一列只有 320px，
                        // 塞不下"一个图标 + 一句 tooltip 之外的信息"，
                        // 而代理地址恰恰是排查网络时最需要的那串字符
                        r.child(crate::widgets::pill(
                            &format!("代理 {proxy}"),
                            orange_text(t),
                            orange_bg(t),
                        ))
                    }),
            );
        }

        row
    }
}
