//! 定时同步对话框：档位 / 每天几点 / 检测后自动拉取 / 上次结果 / 保存。
//!
//! **2026-10-05 从「同步设置」整页搬进一个弹窗。** 那一页只有七个档位按钮
//! 加一个时间框、一颗勾选，摊满整页却只占一条窄列，而概览页正为高度发愁。
//! 用户口径是「定时任务有个入口，单独面板设置就行了」——它是**设置**不是
//! **动作**，不该在概览页常驻占一行；入口放在同步段右上角，点开是这个窗。
//!
//! **三项一个「保存」按钮，不做"点一下存一项"。** 这几项会互相影响
//! （档位一变，间隔与时间点谁生效就变了），逐项存的话用户改到一半关掉，
//! 留下的就是半套配置——而他记得的"我设过了"和实际生效的不是一回事。
//!
//! 编辑期间改动只落在 `Shell::sync_cfg` 这份草稿上，落盘只在按「保存」时发生。
//! 草稿在**切仓库时重建**（`Shell::load_sync_cfg`）：概览页是全局的，
//! 用户在 A 仓库改了一半没保存、切到 B 仓库再切回来，不该看到 A 的半成品。
//!
//! 规范化在 core 那边（`store::normalize_auto_sync`）。显示与存盘走同一个
//! 换算函数（`SyncCfg::to_auto_sync`），不各算一套。
//!
//! **所有展示函数收 `&Entity<Shell>` 而不是 `&mut Context<Shell>`**：
//! 官方 `Dialog` 的 `content` / `footer` 闭包只给 `&mut App` 与 `&Entity`，
//! 拿不到 `Context<Self>`。这一整页（`presets_row` / `when_daily` /
//! `last_result`）因此都改成从 Entity 回写——原来那个页签版本收的是
//! `&mut Context<Shell>`，搬过来时统一，别让两套写法并存

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, Context, Div, Entity, IntoElement, ParentElement, SharedString,
    Styled, Window,
};
// 明确带包路径：同名的 `crate::scheduler` 是 GUI 侧那个定时器模块，
// 这里要的是 core 里的判定与结论类型
use qingbird_core::scheduler::{self, AutoSyncKind};
use qingbird_core::store::RepoEntry;

use crate::state::*;
use crate::tokens::*;
use crate::widgets::{ghost_variant, meta_text};

/// 档位总数：不定时 + 5 个间隔 + 每天定时
const PRESET_COUNT: usize = SYNC_MINS_PRESETS.len() + 2;

impl Shell {
    /// 打开定时同步弹窗。内容是 `Shell::sync_cfg` 那份草稿的实时视图，
    /// 所以编辑期间每敲一个字这窗就跟着变——不另存一份
    ///
    /// **开窗时无条件重建草稿。** 这份草稿只服务这一个窗（概览页那边只剩
    /// 一颗入口按钮），而"没点保存就关掉、重开又是原样"正是它该有的行为。
    /// 判断"草稿是不是过期"要拿配置跟草稿逐项比，而那份判据一旦有盲区，
    /// 用户看到的就是这个仓库的定时设置配成了另一个仓库的——
    /// 重建一次比判一次便宜，也对
    pub(crate) fn open_sync_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.load_sync_cfg(window, cx);
        let this = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let t = cx.theme().clone();
            dialog
                .title("定时同步")
                .content({
                    // 克隆在 `.content({})` 这一层里做，外层那个 `this`
                    // 还要给 `.footer(..)` 用——直接 move 进 content 闭包
                    // 的话 footer 那一行就编译不过（Entity 不 Copy）
                    let t = t.clone();
                    let this = this.clone();
                    move |c: DialogContent, _w, cx: &mut App| {
                        let t = t.clone();
                        let this = this.clone();
                        c.child(sync_body(&t, &this, cx))
                    }
                })
                .footer(sync_footer(&this, ghost_variant(cx)))
        });
    }
}

/// 对话框正文。**只读快照 + Entity 回写**：`content` 闭包里只有 `&mut App`，
/// 读写两个口就分开了
fn sync_body(t: &Theme, this: &Entity<Shell>, cx: &mut App) -> Div {
    let (mode, auto_pull, daily_input, error) = {
        let s = this.read(cx);
        (
            s.sync_cfg.mode,
            s.sync_cfg.auto_pull,
            s.sync_cfg.daily_input.clone(),
            s.sync_cfg.error.clone(),
        )
    };
    let daily = daily_input.read(cx).value().to_string();
    let daily_ok = this.read(cx).sync_cfg.daily_ok(&daily);
    let last = this
        .read(cx)
        .selected
        .as_deref()
        .and_then(|p| this.read(cx).entry(p))
        .cloned();

    v_flex()
        .w_full()
        .gap(px(GAP))
        .child(
            div()
                .w_full()
                .text_size(px(FS_META))
                .text_color(tier4(t))
                .child("关掉之后不再自动检测。要接上定时，先在仓库头确认有上游分支。"),
        )
        .child(presets_row(mode, this))
        .when_some(
            when_daily(mode, &daily_input, daily_ok, t),
            |b, row| b.child(row),
        )
        .child(
            h_flex()
                .w_full()
                .flex_wrap()
                .items_center()
                .gap(px(GAP_TIGHT))
                .child(
                    Checkbox::new("sync-auto-pull")
                        .checked(auto_pull)
                        .label("检测到更新后自动拉取")
                        .on_click({
                            let this = this.clone();
                            move |checked, _, cx| {
                                this.update(cx, |shell, cx| {
                                    shell.sync_cfg.auto_pull = *checked;
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(meta_text(
                    if auto_pull {
                        "自动拉取（--ff-only，不改写本地历史）"
                    } else {
                        "只检测并提醒，不动工作区"
                    },
                    t,
                )),
        )
        .child(last_result(last.as_ref(), t))
        .when_some(error, |b, e| {
            b.child(
                div()
                    .w_full()
                    .text_size(px(FS_SEC))
                    .text_color(t.colors.danger)
                    .child(e),
            )
        })
}

fn sync_footer(this: &Entity<Shell>, gv: ButtonCustomVariant) -> Div {
    let save = this.clone();
    h_flex()
        .w_full()
        .justify_end()
        .gap(px(GAP))
        .child(
            Button::new("sync-cancel")
                .label("取消")
                .custom(gv)
                .on_click(move |_, window, cx| window.close_dialog(cx)),
        )
        .child(Button::new("sync-save").label("保存").primary().on_click({
            let save = save.clone();
            move |_, window, cx| {
                // 存成了才关窗。存不成时错误留在窗里，用户要看得见
                // 「时间格式不对」这句话——关掉窗他就只剩一个"没反应"的窗
                let ok = save.update(cx, |shell, cx| {
                    shell.save_sync_settings(&mut *window, cx)
                });
                if ok {
                    window.close_dialog(cx);
                }
            }
        }))
}

// ================= 档位 =================

/// 档位一排按钮。**选中态用官方 `toggled`**，不要自己画选中底色——
/// 手搓的没有焦点环与按压反馈
fn presets_row(mode: usize, this: &Entity<Shell>) -> Div {
    h_flex()
        .w_full()
        .flex_wrap()
        .gap(px(GAP_TIGHT))
        .children((0..PRESET_COUNT).map(|ix| {
            let this = this.clone();
            Button::new(SharedString::from(format!("sync-preset-{ix}")))
                .label(preset_label(ix))
                .toggled(mode == ix)
                .with_size(Size::Small)
                .on_click(move |_, _, cx| {
                    this.update(cx, |shell, cx| {
                        shell.sync_cfg.mode = ix;
                        shell.sync_cfg.error = None;
                        cx.notify();
                    });
                })
        }))
}

fn preset_label(ix: usize) -> String {
    match ix {
        SyncCfg::OFF => "不定时".to_string(),
        SyncCfg::DAILY => "每天定时".to_string(),
        i => format!("每 {} 分钟", SYNC_MINS_PRESETS[i - 1]),
    }
}

/// 「每天几点」的时间输入框。只在选中那一档时才画——
/// 摆着一个用不上的输入框，用户会以为它也生效
fn when_daily(
    mode: usize,
    input: &Entity<gpui_kit::component::input::InputState>,
    ok: bool,
    t: &Theme,
) -> Option<Div> {
    if mode != SyncCfg::DAILY {
        return None;
    }
    Some(
        h_flex()
            .w_full()
            .items_center()
            .gap(px(GAP))
            .child(
                div()
                    .text_size(px(FS_SEC))
                    .text_color(tier3(t))
                    .child("每天几点"),
            )
            .child(Input::new(input).w(px(120.0)).small())
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(FS_META))
                    .text_color(if ok { tier4(t) } else { t.colors.danger })
                    .child(if ok {
                        "本地时区，24 小时制"
                    } else {
                        "时间格式要对：24 小时制的 HH:MM"
                    }),
            ),
    )
}

// ================= 上次结果 =================

/// 上一次自动同步的结论与时间。
///
/// **kind 的解析与中文说法一律走 core 的 `parse_summary` / `label()`**，
/// 这里不再自己维护一份映射表：两套界面共用同一份 `config.json`，
/// 而这一行短码的取值只有 core 那一处说了算。界面自己再抄一份映射，
/// 改了一处忘了另一处，用户看到的是"上次失败"还是"上次跳过"随机。
///
/// 查不到就退回显示原始串，**不猜 kind**——猜错会把一次失败显示成成功
fn last_result(e: Option<&RepoEntry>, t: &Theme) -> Div {
    let Some(e) = e else {
        return hint(t, "这个仓库不在列表里。");
    };
    let raw = e.last_auto_result.trim();
    let mut rows: Vec<gpui_kit::AnyElement> = Vec::new();
    if raw.is_empty() {
        rows.push(hint(t, "还没有跑过自动同步。").into_any_element());
    } else {
        let (ink, text) = match scheduler::parse_summary(raw) {
            Some((kind, msg)) => {
                let ink = match kind {
                    AutoSyncKind::Ok => t.colors.success,
                    AutoSyncKind::Behind => orange_text(t),
                    AutoSyncKind::Pulled => t.colors.success,
                    AutoSyncKind::Skip => tier3(t),
                    AutoSyncKind::Error => t.colors.danger,
                };
                (ink, format!("上次{}：{msg}", kind.label()))
            }
            None => (tier3(t), raw.to_string()),
        };
        rows.push(
            h_flex()
                .w_full()
                .flex_wrap()
                .items_center()
                .gap(px(GAP_TIGHT))
                .child(div().text_color(ink).child(icon_sm(AssetIcon::Clock)))
                .child(div().text_size(px(FS_META)).text_color(ink).child(text))
                .into_any_element(),
        );
    }
    // 「上次自动检查」这一行以前永远不显示，因为 `last_auto_check` 恒为 0
    // （调度器没搬）。现在它会被真实刷新，调度器跑完一次就能看到时间
    if e.last_auto_check > 0 {
        rows.push(
            hint(
                t,
                &format!("上次检查：{}（本地时区）", rel_time(e.last_auto_check)),
            )
            .into_any_element(),
        );
    }
    v_flex()
        .w_full()
        .gap(px(GAP_DENSE))
        .children(rows)
}

fn hint(t: &Theme, text: &str) -> Div {
    div()
        .w_full()
        .text_size(px(FS_META))
        .text_color(tier4(t))
        .child(text.to_string())
}
