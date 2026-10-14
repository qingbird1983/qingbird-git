//! CLI 的确认层：**共享缓存清理**确认框 + **卸载审计态**的状态开关。
//!
//! ## 卸载审计为什么不在对话框里（v8 定稿，2026-10-14）
//!
//! 卸载审计（四段信息层级：同时移除清单 / 命令预览 / 风险徽章+合计 /
//! 可勾选清理清单）从中模态框**上浮**到 CLI 屏中栏 ② 卡的一态
//! （`cli_tab::update_card` 开头那个分支）——审计页是"动了会怎样"的读法，
//! 盖一层框反而把它与日志、磁盘占用隔开。本文件只剩 `open_cli_audit`
//! （快照 + 默认勾选，**不再 build 框**）与共享缓存清理框的全套。
//!
//! ## 为什么清理框不走「点确认就关」
//!
//! `purge.rs` / `clean.rs` 那两颗是「点确认就关、结论进日志区」——它们的活
//! 一两秒内完。CLI 这颗不是：`npm cache clean` 一个大家伙是分钟级的，
//! 而设计 §11 的状态机里 **Running 是一个独立状态**（Idle → Planning →
//! AwaitingConfirm → Running → Success/Failed）。所以确认之后**框留着**，
//! 按钮转禁、确认键变「执行中…」；跑完由 render 那一帧收掉
//! （`Shell::pending_cli_dlg_close`——后台任务里没有 `&mut Window`，
//! 与 `pending_batch_note` 同一条路）。卸载审计态沿用同一条收场路径。
//!
//! ## 每帧现读
//!
//! `content` 的 builder 是 `Fn`、**每帧重跑**，而闭包捕获的是快照——
//! 所以这里用 `purge.rs` 那套写法：builder 里 `this.read(cx)` 现读。
//! 不这么写的后果很隐蔽：勾选行为是对的、画面上的方块不动。

use std::collections::HashSet;

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::{ActiveTheme, Disableable as _, Sizable, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, Context, Div, Entity, FontWeight, InteractiveElement, ParentElement,
    StatefulInteractiveElement, Styled, Window,
};
use qingbird_core::cli::PackageDir;
use qingbird_core::space::human_bytes;

use super::{dlg_cancel, footer, shell};
use crate::state::*;
use crate::tokens::*;
use crate::widgets::{ghost_variant, list_box, meta_text};

/// 共享缓存清单最多列几条（与 `dialogs/clean.rs` 同一定额：确认框不是对账单）
const MAX_SHARED: usize = 8;

impl Shell {
    /// 打开卸载确认框（详情卡那颗「卸载…」）。
    ///
    /// 快照取自**当下清单**：执行前 `run_cli_uninstall` 还会按 id 再核一遍
    /// （对话框开着的时候后台可能重扫）。`Cache` 目录**默认全勾**
    /// （设计 §8：删了不影响功能）；`Config` / `Data` 压根不进这个集合
    /// （禁勾，见 `DirKind::auto_deletable`）。
    pub(crate) fn open_cli_audit(&mut self, id: &str) {
        let Some(pkg) = self.cli.packages.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        let picked: HashSet<String> = pkg
            .dirs
            .iter()
            .filter(|d| d.kind.auto_deletable())
            .map(|d| d.path.clone())
            .collect();
        self.cli_audit = Some(CliUninstallAudit {
            pkg,
            picked,
            running: false,
        });
    }

    /// 打开共享缓存清理确认框。一颗一条地清（汇总条那颗「清理」），
    /// ⌘K「清理共享缓存」整批地清（`targets` 多条）——同一个框
    pub(crate) fn open_cli_clean_dlg(
        &mut self,
        targets: Vec<PackageDir>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if targets.is_empty() {
            return;
        }
        self.cli_clean_dlg = Some(CliCleanDialog {
            targets,
            running: false,
        });
        self.build_cli_clean_dlg(window, cx);
    }

    fn build_cli_clean_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        shell(
            window,
            cx,
            |_| "清理共享缓存".into(),
            None,
            {
                let this = this.clone();
                move |c: DialogContent, _w, cx: &mut App| {
                    let Some(d) = this.read(cx).cli_clean_dlg.clone() else {
                        return c;
                    };
                    let t = cx.theme().clone();
                    c.child(clean_body(&t, &d))
                }
            },
            move |cx| clean_footer(&this, cx, ghost_variant(cx)),
        );
    }
}

/// 共享缓存清理框正文：合计一行 + 逐项清单 + 一句后果。
///
/// **不给"影响包数"**（设计 §13 风险表原话是"影响包数提示"）：共享缓存那一格
/// 没有 provider 字段（`rules.toml` 的 shared 规则只有 label），按 label 猜
/// "这是 npm 的"是编一个映射——而后果本身是确定的，说后果不说个数
fn clean_body(t: &Theme, d: &CliCleanDialog) -> Div {
    let measured: Option<u64> = d
        .targets
        .iter()
        .try_fold(0u64, |acc, x| x.size.map(|s| acc + s));
    let shown: Vec<&PackageDir> = d.targets.iter().take(MAX_SHARED).collect();
    let rest = d.targets.len().saturating_sub(shown.len());

    v_flex()
        .w_full()
        .gap(px(GAP))
        .child(
            h_flex()
                .w_full()
                .items_baseline()
                .gap(px(GAP_TIGHT))
                .child(
                    div()
                        .text_size(px(fs_num()))
                        .font_family(t.mono_font_family.clone())
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.colors.foreground)
                        .child(match measured {
                            Some(b) => human_bytes(b),
                            None => "计算中…".to_string(),
                        }),
                )
                .child(meta_text(
                    if d.targets.len() == 1 {
                        "1 个目录的内容将被清空".to_string()
                    } else {
                        format!("{} 个目录的内容将被清空", d.targets.len())
                    },
                    t,
                )),
        )
        .child(
            list_box(
                v_flex()
                    .w_full()
                    .max_h(px(220.0))
                    .gap(px(GAP_DENSE))
                    .p(px(GAP)),
                t,
            )
            .id("cli-clean-list")
            .overflow_y_scroll()
            .children(shown.into_iter().map(|x| {
                h_flex()
                    .w_full()
                    .items_center()
                    .gap(px(GAP_TIGHT))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(fs_meta()))
                            .text_color(body(t))
                            .child(if x.label.is_empty() {
                                "共享缓存".to_string()
                            } else {
                                x.label.clone()
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(fs_meta()))
                            .font_family(t.mono_font_family.clone())
                            .text_color(tier3(t))
                            .child(x.path.clone()),
                    )
                    .child(
                        div()
                            .w(px(DIR_SIZE_W))
                            .flex_shrink_0()
                            .text_size(px(fs_meta()))
                            .text_color(body(t))
                            .child(crate::cli_tab::size_text(x.size)),
                    )
            }))
            .when(rest > 0, |el| {
                el.child(meta_text(format!("还有 {rest} 个…"), t))
            }),
        )
        .child(
            div()
                .w_full()
                .rounded(px(R_MD))
                .border_1()
                .border_color(orange(t))
                .bg(wash_bg(WARN_WASH, t))
                .px(px(GAP))
                .py(px(GAP_TIGHT))
                .text_size(px(fs_meta()))
                .text_color(orange_text(t))
                .child(
                    "只清内容、保留目录本身。共享缓存由同一个包管理器的所有包共用：\
                     清掉后已安装的工具照常可用，下次安装 / 更新要重新下载。",
                ),
        )
}

/// 清理框底栏。**danger**：可再生不等于没代价（本机 npm 缓存 18.8 GB，
/// 重下一次是几十分钟的网）
fn clean_footer(this: &Entity<Shell>, cx: &App, gv: ButtonCustomVariant) -> Div {
    let running = this
        .read(cx)
        .cli_clean_dlg
        .as_ref()
        .is_some_and(|d| d.running);
    let go = this.clone();
    footer(
        None,
        [
            dlg_cancel(
                "cli-clean-cancel",
                "取消",
                this,
                |shell, _| shell.cli_clean_dlg = None,
                gv,
            ),
            Button::new("cli-clean-go")
                .label(if running { "执行中…" } else { "清理" })
                .danger()
                .with_size(Size::Small)
                .disabled(running)
                .on_click(move |_, _, cx| {
                    go.update(cx, |shell, cx| shell.run_cli_clean(cx));
                }),
        ],
    )
}
