//! 阶段 17：第三方仓库「多出来的文件」逐项勾选框。
//!
//! ## 为什么要逐项、为什么默认一个都不勾
//!
//! 这一屏要做的是**不可逆的删除**。用户点进来的心理是"这个仓库脏了，帮我收拾一下"，
//! 而他在终端里随手留下的那个 `analysis.log`、那个没写完的脚本、那份拷进来的
//! 资料，**只有他自己知道该不该留**。所以：
//!
//! - **一个都不预勾**——预勾等于替他决定。`init` 向导那套"常见构建产物默认勾上"
//!   是反过来的理由：写 `.gitignore` 可逆且无害
//! - **两半分列**——`untracked` 勾了是永久删除，`tracked` 勾了是恢复成云端版本。
//!   可逆性完全不同，摆成一列用户不会去想"这条删了能撤回吗"
//!
//! ## 谁会看到它
//!
//! **只有第三方仓库**（17-4）。变更页签的表头上那把「清理多出来的文件…」在
//! 自己的仓库里不出现：自己的仓库要做这件事有正经工具链，在这里给一条
//! 「一键丢工作区」的路是帮倒忙。
//!
//! ## 状态每帧现读
//!
//! `content` 的 builder 收的是 `Fn`、**每帧重画一次**。但闭包里捕获的是快照，
//! 快照不会跟着勾选变——所以这里用 `batch_log.rs` 那套写法：
//! 在 builder 里 `this.read(cx)` 现读。不这么写的后果很隐蔽：**行为是对的
//! （勾上的确实进了清单），画面上的方块却不动**，用户会以为没点上而反复点。

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::{ActiveTheme, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    assets::IconName as AssetIcon, div, px, App, Context, Div, Entity, IntoElement,
    InteractiveElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
};
use qingbird_core::purge;

use super::{dlg_cancel, footer, shell};
use crate::state::*;
use crate::tokens::*;
use crate::widgets::{ghost_variant, list_box, meta_text};

/// 每组最多列几项。超出的并成一行「还有 N 个…」——
/// 这一屏的作用是让用户看清**将要发生什么**，不是给一张完整对账单。
/// 真的一个仓库多出几百个文件时，把窗口撑开不如让用户看见"数量很多"这件事
const MAX_PER_GROUP: usize = 8;

impl Shell {
    /// 打开勾选框。清单从**当下的** `changes` 拆开，拆法是 core 的
    /// `split_for_purge`——分流判据只有一份，不在界面这边再写一遍
    pub(crate) fn open_purge_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        // 17-4：**只给第三方**。这一条不能只靠"入口键不画"来保证——
        // 万一有人从别的路调进来，闸门得在这一层
        if !self.selected_is_third() {
            return;
        }
        let changes = self
            .view(&path)
            .map(|v| v.changes.clone())
            .unwrap_or_default();
        let (untracked, tracked) = purge::split_for_purge(&changes);
        if untracked.is_empty() && tracked.is_empty() {
            return;
        }
        self.purge_dlg = Some(PurgeDialog {
            path,
            untracked,
            tracked,
            // **默认全不勾**。理由写在这个结构体的字段注释里
            picked: std::collections::HashSet::new(),
        });
        self.build_purge_dlg(window, cx);
    }

    fn build_purge_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        shell(
            window,
            cx,
            |_| "清掉多出来的文件".into(),
            None,
            {
                let this = this.clone();
                move |c: DialogContent, _w, cx: &mut App| {
                    // 每帧现读，见文件头
                    let Some(d) = this.read(cx).purge_dlg.clone() else {
                        return c;
                    };
                    c.child(purge_body(&cx.theme().clone(), &d, &this))
                }
            },
            move |cx| purge_footer(&this, ghost_variant(cx)),
        );
    }
}

/// 一组清单的小标题 + 一条说明。**两类分列就是这个标题的差别**
fn group_head(t: &Theme, title: &str, note: &str, danger: bool) -> Div {
    h_flex()
        .w_full()
        .items_center()
        .gap(px(GAP_TIGHT))
        .child(
            div()
                .text_color(if danger { t.colors.danger } else { tier3(t) })
                .child(if danger {
                    icon_sm(AssetIcon::Trash)
                } else {
                    icon_sm(AssetIcon::Undo2)
                }),
        )
        .child(
            div()
                .text_size(px(fs_sec()))
                .text_color(body(t))
                .child(title.to_string()),
        )
        .child(div().flex_1())
        .child(meta_text(note.to_string(), t))
}

/// 一行：勾选框 + 路径 + 「文件 / 文件夹」
fn pick_row(
    t: &Theme,
    this: &Entity<Shell>,
    path: &str,
    checked: bool,
    danger: bool,
) -> Div {
    h_flex()
        .w_full()
        .h(px(26.0))
        .items_center()
        .gap(px(GAP_TIGHT))
        .child(
            Checkbox::new(SharedString::from(format!("purge-cb-{path}")))
                .checked(checked)
                .on_click({
                    let this = this.clone();
                    let path = path.to_string();
                    move |checked, _, cx| {
                        this.update(cx, |this, cx| {
                            if let Some(d) = this.purge_dlg.as_mut() {
                                if *checked {
                                    d.picked.insert(path.clone());
                                } else {
                                    d.picked.remove(&path);
                                }
                            }
                            cx.notify();
                        });
                    }
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(fs_sec()))
                .font_family(t.mono_font_family.clone())
                .text_color(if danger { t.colors.danger } else { body(t) })
                .child(path.to_string()),
        )
        .child(meta_text(
            if path.ends_with('/') || path.ends_with('\\') {
                "文件夹".to_string()
            } else {
                "文件".to_string()
            },
            t,
        ))
}

/// 一组的 N 个勾选行（超出并成一句）
///
/// 返回 `impl IntoElement` 而不是 `Div`：**`overflow_y_scroll` / `id` 只在
/// `Stateful<E>` 上**，加了它们类型就变了（`Div` → `Stateful<Div>`）。
fn group_list(
    t: &Theme,
    this: &Entity<Shell>,
    id: &'static str,
    paths: &[String],
    picked: &std::collections::HashSet<String>,
    danger: bool,
) -> impl IntoElement {
    let shown: Vec<&String> = paths.iter().take(MAX_PER_GROUP).collect();
    let rest = paths.len().saturating_sub(shown.len());
    // 与 `dialogs/init.rs` 同一类东西（装可勾选行的盒）⇒ 走同一颗 `list_box`。
    // 原来这两处是逐行相同的两份拷贝，各自写着 `.bg(background)` + `.border(border)`
    list_box(
        v_flex()
            .w_full()
            .max_h(px(220.0))
            .gap(px(GAP_DENSE))
            .p(px(GAP)),
        t,
    )
    .id(id)
    .overflow_y_scroll()
    .children(shown.into_iter().map(|p| {
            pick_row(t, this, p, picked.contains(p), danger)
        }))
        .when(rest > 0, |el| {
            el.child(meta_text(format!("还有 {rest} 项…"), t))
        })
}

fn purge_body(t: &Theme, d: &PurgeDialog, this: &Entity<Shell>) -> Div {
    let mut col = v_flex().w_full().gap(px(GAP_LG)).child(
        div()
            .w_full()
            .text_size(px(fs_body()))
            .line_height(px(20.0))
            .text_color(tier3(t))
            .child("第三方仓库本地会无意间多出文件，而本项目不替它们提交——\
                    勾掉不想留的，让它回到干净。"),
    );

    if !d.untracked.is_empty() {
        let n = d.untracked.len();
        col = col
            .child(group_head(
                t,
                &format!("未跟踪 · {n} 项"),
                "勾 = 永久删除，无法撤销",
                true,
            ))
            .child(group_list(
                t,
                this,
                "purge-untracked",
                &d.untracked,
                &d.picked,
                true,
            ));
    }
    if !d.tracked.is_empty() {
        let n = d.tracked.len();
        col = col
            .child(group_head(
                t,
                &format!("已跟踪但被改动 · {n} 项"),
                "勾 = 恢复成云端版本，可逆",
                false,
            ))
            .child(group_list(
                t,
                this,
                "purge-tracked",
                &d.tracked,
                &d.picked,
                false,
            ));
    }

    if !d.picked.is_empty() {
        let del = d
            .untracked
            .iter()
            .filter(|p| d.picked.contains(*p))
            .count();
        let res = d.picked.len() - del;
        col = col.child(
            div()
                .w_full()
                .text_size(px(fs_sec()))
                .text_color(body(t))
                .child(format!("已勾 {del} 个删除、{res} 个恢复")),
        );
    } else {
        // 一个都不勾时也要说清"那就把勾选框勾上"——否则用户会以为这框坏了
        col = col.child(crate::widgets::empty_state("还没有勾选任何一项", "", t).flex_grow(0.));
    }
    col
}

/// 底栏：取消 + 确认。**确认键用 danger**
///
/// ⚠️ **它没有"没勾就置灰"**——官方 `Dialog::footer` 收的是
/// `impl IntoElement`（**构建好的元素**），不是每帧重调的闭包，
/// 按钮的 `disabled` 在建框那一刻就定死了。硬要那个效果只有两条路：
/// 把按钮挪进 `content`（破坏 Dialog 的惯例），或者点下去静默没反应。
/// **两条都比"点了告诉你没什么可做"更糟**——后者是明说的。
/// 所以：确认键一直可点，空清单点下去由 `run_purge` 记一句
/// 「没有需要清理的内容」；勾了几个则由 `content` 里那行实时计数告诉用户。
fn purge_footer(this: &Entity<Shell>, gv: ButtonCustomVariant) -> Div {
    let go = this.clone();
    footer(
        None,
        [
            dlg_cancel(
                "purge-cancel",
                "取消",
                this,
                |shell, _| shell.purge_dlg = None,
                gv,
            ),
            Button::new("purge-go")
                .label("按勾选清理")
                .danger()
                .with_size(Size::Small)
                .on_click(move |_, window, cx| {
                    go.update(cx, |shell, cx| shell.run_purge(cx));
                    // 不等结果就关：这一页没有"停在原地看输出"的必要，
                    // 进度与结论都在日志区
                    window.close_dialog(cx);
                }),
        ],
    )
}
