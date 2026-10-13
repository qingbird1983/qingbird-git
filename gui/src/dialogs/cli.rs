//! 阶段三的两颗 CLI 确认框：**卸载**（OpPlan 四段信息层级）与**共享缓存清理**。
//!
//! ## 为什么这两颗框不走「点确认就关」
//!
//! `purge.rs` / `clean.rs` 那两颗是「点确认就关、结论进日志区」——它们的活
//! 一两秒内完。CLI 这颗不是：`winget uninstall` 一个大家伙是**分钟级**的，
//! 而设计 §11 的状态机里 **Running 是一个独立状态**（Idle → Planning →
//! AwaitingConfirm → Running → Success/Failed）。所以确认之后**框留着**，
//! 按钮转禁、确认键变「执行中…」；跑完由 render 那一帧收掉
//! （`Shell::pending_cli_dlg_close`——后台任务里没有 `&mut Window`，
//! 与 `pending_batch_note` 同一条路）。
//!
//! ## 卸载框的四段信息层级（设计 §10.1）
//!
//! ① 同时移除清单（`plan.affected`）→ ② 等宽命令预览（`plan.steps`）→
//! ③ 风险徽章（`plan.risk`）→ ④ 可勾选清理清单（Cache 默认勾 /
//! Config·Data 禁勾 + 红字 / 底部「本次可释放约 N MB」）。
//! 四段全部从 core 的 [`OpPlan`](qingbird_core::cli::OpPlan) 来——
//! **界面不自己判一次风险、不自己合计一次字节**（同一属性两个出处）。
//!
//! ## 每帧现读
//!
//! `content` 的 builder 是 `Fn`、**每帧重跑**，而闭包捕获的是快照——
//! 所以这里用 `purge.rs` 那套写法：builder 里 `this.read(cx)` 现读。
//! 不这么写的后果很隐蔽：勾选行为是对的、画面上的方块不动。

use std::collections::HashSet;

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::{ActiveTheme, Disableable as _, Sizable, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    assets::IconName as AssetIcon, div, px, App, Context, Div, Entity, FontWeight, IntoElement,
    InteractiveElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
};
use qingbird_core::cli::{provider_of, OpPlan, PackageDir, RiskLevel};
use qingbird_core::space::human_bytes;

use super::{dlg_cancel, footer, shell};
use crate::state::*;
use crate::tokens::*;
use crate::widgets::{ghost_variant, list_box, meta_text, pill, sunken_box};

/// 共享缓存清单最多列几条（与 `dialogs/clean.rs` 同一定额：确认框不是对账单）
const MAX_SHARED: usize = 8;

impl Shell {
    /// 打开卸载确认框（详情卡那颗「卸载…」）。
    ///
    /// 快照取自**当下清单**：执行前 `run_cli_uninstall` 还会按 id 再核一遍
    /// （对话框开着的时候后台可能重扫）。`Cache` 目录**默认全勾**
    /// （设计 §8：删了不影响功能）；`Config` / `Data` 压根不进这个集合
    /// （禁勾，见 `DirKind::auto_deletable`）。
    pub(crate) fn open_cli_uninstall_dlg(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pkg) = self.cli.packages.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        let picked: HashSet<String> = pkg
            .dirs
            .iter()
            .filter(|d| d.kind.auto_deletable())
            .map(|d| d.path.clone())
            .collect();
        self.cli_uninstall_dlg = Some(CliUninstallDialog {
            pkg,
            picked,
            running: false,
        });
        self.build_cli_uninstall_dlg(window, cx);
    }

    fn build_cli_uninstall_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        shell(
            window,
            cx,
            |_| "卸载 CLI 工具".into(),
            None,
            {
                let this = this.clone();
                move |c: DialogContent, _w, cx: &mut App| {
                    // 每帧现读，见文件头
                    let Some(d) = this.read(cx).cli_uninstall_dlg.clone() else {
                        return c;
                    };
                    let t = cx.theme().clone();
                    c.child(uninstall_body(&t, &d, &this))
                }
            },
            move |cx| uninstall_footer(&this, cx, ghost_variant(cx)),
        );
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

/// 此刻的计划（对话框开着的时候**按帧重算**：勾选一变合计就变）。
///
/// provider 认不出时给 `OpPlan::blocked`（steps 空）——调用方据此
/// **不开执行**（一颗确认了却什么都没做的键比没有更糟）。
fn plan_of(d: &CliUninstallDialog) -> OpPlan {
    let picked: Vec<PackageDir> = d
        .pkg
        .dirs
        .iter()
        .filter(|x| d.picked.contains(&x.path))
        .cloned()
        .collect();
    match provider_of(d.pkg.provider) {
        Some(p) => p.plan_uninstall(&d.pkg, &picked),
        None => OpPlan::blocked(&d.pkg, "认不出这个包是从哪个包管理器装的"),
    }
}

/// 风险徽章：三档各一个色（低 = 成功档 / 中 = 警示档 / 高 = 危险档）。
///
/// 颜色走全站那三套 wash（`OK_WASH` / `WARN_WASH` / `BAD_WASH`）——
/// 不新调一个色：「可更新」「失败」在别处也是这三档，同一语义同色
fn risk_pill(risk: RiskLevel, t: &Theme) -> Div {
    let (fg, bg) = match risk {
        RiskLevel::Safe => (t.colors.success, wash_bg(OK_WASH, t)),
        RiskLevel::Moderate => (orange_text(t), wash_bg(WARN_WASH, t)),
        RiskLevel::High => (t.colors.danger, wash_bg(BAD_WASH, t)),
    };
    pill(&format!("风险 {}", risk.label()), fg, bg)
}

fn uninstall_body(t: &Theme, d: &CliUninstallDialog, this: &Entity<Shell>) -> Div {
    let plan = plan_of(d);
    let col = v_flex().w_full().gap(px(GAP_LG));

    // ① 同时移除清单（设计 §10.1 第一段）。话全部来自 core 的
    // `affected_notes`——四档桌面关联四句话，说错方向不是显示问题
    // （`CliWithDesktop` 说成"只卸命令行"会让用户以为不碰桌面程序）
    let affected = v_flex()
        .w_full()
        .gap(px(GAP_TIGHT))
        .children(plan.affected.iter().map(|a| {
            h_flex()
                .w_full()
                .items_start()
                .gap(px(GAP_TIGHT))
                .child(
                    div()
                        .flex_shrink_0()
                        .pt(px(2.0))
                        .text_color(if plan.risk == RiskLevel::High {
                            t.colors.danger
                        } else {
                            tier3(t)
                        })
                        .child(icon_sm(if plan.risk == RiskLevel::High {
                            AssetIcon::TriangleAlert
                        } else {
                            AssetIcon::CircleAlert
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(fs_sec()))
                        .line_height(px(18.0))
                        .text_color(body(t))
                        .child(a.clone()),
                )
        }));

    // ② 等宽命令预览（第二段）。**不可编辑**——它是给用户看"将要发生什么"的，
    // 不是一个可以改的输入框（改了就不等于实际执行的那条）
    let preview = sunken_box(
        v_flex()
            .w_full()
            .p(px(GAP))
            .gap(px(GAP_DENSE))
            .font_family(t.mono_font_family.clone())
            .children(plan.steps.iter().map(|s| {
                div()
                    .w_full()
                    .text_size(px(fs_meta()))
                    .text_color(body(t))
                    .child(s.preview())
            })),
        t,
    );

    // ③ 风险徽章（第三段）。与预览并排：一个说"危不危险"，一个说"具体干什么"
    let risk_row = h_flex()
        .w_full()
        .items_center()
        .gap(px(GAP))
        .child(risk_pill(plan.risk, t))
        .child(div().flex_1())
        .child(meta_text(
            match plan.reclaimable {
                // 合计字节：**有一项没量过就整栏是 None**（core 的口径，
                // 拿量过的那些加出一个数 = 把"我还不知道"说成结论）
                Some(b) => format!("本次可释放约 {}", human_bytes(b)),
                None => "本次可释放：计算中…".to_string(),
            },
            t,
        ));

    // ④ 可勾选清理清单（第四段）。`Cache` 默认勾、可改；
    // `Config` / `Data` 禁勾——**不是把勾选框画灰**（官方 `Checkbox` 没有
    // 禁用档，画一个点了没反应的方块比不画更糟），是换一颗锁图标 + 红字原因
    let mut dirs = v_flex().w_full().gap(px(GAP_DENSE));
    if d.pkg.dirs.is_empty() {
        dirs = dirs.child(meta_text("没有发现这颗工具的数据目录".to_string(), t));
    }
    for dir in d.pkg.dirs.iter() {
        dirs = dirs.child(dir_pick_row(t, this, d, dir));
    }

    col.child(affected)
        .child(preview)
        .child(risk_row)
        .child(
            v_flex()
                .w_full()
                .gap(px(GAP_TIGHT))
                .child(
                    div()
                        .text_size(px(fs_meta()))
                        .text_color(tier3(t))
                        .child("卸载时顺手清理"),
                )
                .child(dirs),
        )
}

/// 清理清单里的一行：可勾（Cache）/ 禁勾（Config·Data 换锁图标 + 红字）。
///
/// `Checked` 每帧从 `d.picked` 现读——见文件头「每帧现读」
fn dir_pick_row(
    t: &Theme,
    this: &Entity<Shell>,
    d: &CliUninstallDialog,
    dir: &PackageDir,
) -> Div {
    let checked = d.picked.contains(&dir.path);
    let warn = dir.kind.note_is_warning();
    h_flex()
        .w_full()
        .items_center()
        .gap(px(GAP))
        // 左槽：勾选框或锁。**两种"不给勾"是不同的两件事**——Cache 是
        // "可清理、默认勾上"，Config/Data 是"不许清、给理由"
        .child(if dir.kind.auto_deletable() {
            Checkbox::new(SharedString::from(format!("cli-dir-cb-{}", dir.path)))
                .checked(checked)
                .on_click({
                    let this = this.clone();
                    let path = dir.path.clone();
                    move |checked, _, cx| {
                        this.update(cx, |this, cx| {
                            if let Some(d) = this.cli_uninstall_dlg.as_mut() {
                                if *checked {
                                    d.picked.insert(path.clone());
                                } else {
                                    d.picked.remove(&path);
                                }
                            }
                            cx.notify();
                        });
                    }
                })
                .into_any_element()
        } else {
            div()
                .w(px(16.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .text_color(tier4(t))
                .child(icon_sm(AssetIcon::Lock))
                .into_any_element()
        })
        .child(
            div()
                .w(px(FORM_LABEL_W))
                .flex_shrink_0()
                .text_size(px(fs_meta()))
                .text_color(tier3(t))
                .child(dir.kind.label()),
        )
        .child(
            div()
                .w(px(DIR_SIZE_W))
                .flex_shrink_0()
                .text_size(px(fs_meta()))
                .text_color(body(t))
                // `None` = 还没量（`size_text` 的口径：没量过与量到 0 是两句话）
                .child(crate::cli_tab::size_text(dir.size)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(fs_meta()))
                .font_family(t.mono_font_family.clone())
                .text_color(tier3(t))
                .child(dir.path.clone()),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(fs_meta()))
                .text_color(if warn { t.colors.danger } else { tier3(t) })
                .child(dir.kind.note()),
        )
}

/// 卸载框底栏：取消 + 卸载（danger）。
///
/// ⚠️ **执行中不禁用整排、只禁确认键**：取消随时能点（Esc 同理）——
/// 拦着不给走是把用户关在框里。后台任务照跑，收框由 render 那一帧做
/// （`pending_cli_dlg_close` 的注释：官方 `close_dialog` 空栈上是安全的）。
///
/// ⚠️ **确认之后不关框**（与 purge / clean 相反）：见文件头第一段。
///
/// `running` 每帧现读（`cx` 是 `shell` 那个 builder 闭包收到的 `&App`）：
/// footer 的闭包是 `Fn`、**每帧重调**（`dialogs/mod.rs::shell` 头注释：
/// 官方 root 每帧重跑整个 builder，不只是 content）⇒ 禁用态跟着状态走
fn uninstall_footer(this: &Entity<Shell>, cx: &App, gv: ButtonCustomVariant) -> Div {
    let running = this
        .read(cx)
        .cli_uninstall_dlg
        .as_ref()
        .is_some_and(|d| d.running);
    let go = this.clone();
    footer(
        None,
        [
            dlg_cancel(
                "cli-uninstall-cancel",
                "取消",
                this,
                |shell, _| shell.cli_uninstall_dlg = None,
                gv,
            ),
            Button::new("cli-uninstall-go")
                .label(if running { "执行中…" } else { "卸载" })
                .danger()
                .with_size(Size::Small)
                .disabled(running)
                .on_click(move |_, _, cx| {
                    go.update(cx, |shell, cx| shell.run_cli_uninstall(cx));
                }),
        ],
    )
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
