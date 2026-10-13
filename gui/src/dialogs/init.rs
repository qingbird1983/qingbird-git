//! 初始化向导：选目录 + 勾排除项 + git init。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::component::button::ButtonCustomVariant;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::{Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, AppContext, Context, Div, Entity, InteractiveElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled,
};
use qingbird_core::repos::{self};
use qingbird_core::run_cmd::{self};

use super::{dlg_cancel, footer};
use crate::state::*;
use crate::tokens::*;
use crate::widgets::list_box;

/// 常见构建产物 / 依赖目录默认勾上（进 `enter_init` 时预勾），用户可改。
pub(crate) fn init_body(
    t: &Theme,
    init: &InitState,
    error: &Option<String>,
    this: &Entity<Shell>,
) -> Div {
    let dir_name = std::path::Path::new(&init.path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| init.path.clone());

    // 装可勾选行的盒子 → `list_box`（`r_lg` + 1px `track` 边，**不给底**）。
    // 原来这里是 `.bg(t.colors.background)` + `.border_color(t.colors.border)`：
    // 前者是**画布**档（浅 #E8EDF0），于是白卡里挖出一块比桌面还灰的洞；
    // 后者比卡边重一档。同一屏 `dialogs/purge.rs` 是这两样的第二份拷贝
    let mut list = list_box(
        v_flex()
            .w_full()
            .max_h(px(180.0))
            .gap(px(GAP_DENSE))
            .p(px(GAP)),
        t,
    )
    .id("init-excludes")
    .overflow_y_scroll();
    if init.items.is_empty() {
        list = list.child(
            div()
                .text_size(px(fs_sec()))
                .text_color(tier4(t))
                .child("（空文件夹）"),
        );
    }
    for (ix, it) in init.items.iter().enumerate() {
        let name = it.name.clone();
        let checked = init.excludes.contains(&name);
        list = list.child(
            h_flex()
                .w_full()
                .h(px(26.0))
                .px(px(GAP))
                .items_center()
                .gap(px(GAP))
                .child(
                    Checkbox::new(SharedString::from(format!("init-cb-{ix}")))
                        .checked(checked)
                        .on_click({
                            let this = this.clone();
                            let name = name.clone();
                            move |checked, _, cx| {
                                this.update(cx, |this, cx| {
                                    if let Some(i) = this.dlg.as_mut().and_then(|d| d.init.as_mut())
                                    {
                                        if *checked {
                                            i.excludes.insert(name.clone());
                                        } else {
                                            i.excludes.remove(&name);
                                        }
                                    }
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(
                    div()
                        .text_size(px(fs_sec()))
                        .text_color(body(t))
                        .child(name.clone()),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(px(fs_meta()))
                        .text_color(tier4(t))
                        .child(if it.is_dir { "文件夹" } else { "文件" }.to_string()),
                ),
        );
    }

    v_flex()
        .w_full()
        .gap(px(GAP_LG))
        .child(
            div()
                .w_full()
                .text_size(px(fs_body()))
                .line_height(px(20.0))
                .text_color(tier3(t))
                .child(format!(
                    "「{dir_name}」还不是 git 仓库。初始化后自动生成首次提交；勾选不想纳入版本管理的项，会写入 .gitignore："
                )),
        )
        .child(
            div()
                .w_full()
                .text_size(px(fs_sec()))
                .font_family(t.mono_font_family.clone())
                .text_color(tier4(t))
                .child(init.path.clone()),
        )
        .child(list)
        .when_some(error.clone(), |b, e| {
            b.child(
                div()
                    .w_full()
                    .text_size(px(fs_sec()))
                    .text_color(t.colors.danger)
                    .child(e),
            )
        })
}

/// 初始化向导底栏：取消 / 初始化（跑起来时按钮转圈）
pub(crate) fn init_footer(init: &InitState, this: &Entity<Shell>, gv: ButtonCustomVariant) -> Div {
    footer(
        None,
        [
            dlg_cancel(
                "init-cancel",
                "取消",
                this,
                |shell, cx| {
                    shell.dlg = None;
                    cx.notify();
                },
                gv,
            )
            .disabled(init.working),
            Button::new("init-go")
                .label("初始化")
                .primary()
                .loading(init.working)
                .on_click({
                    let this = this.clone();
                    move |_, _, cx| {
                        this.update(cx, |this, cx| this.run_init(cx));
                    }
                }),
        ],
    )
}

impl Shell {
    /// 弹窗切到初始化向导：读出顶层目录，预勾常见排除项
    pub(crate) fn enter_init(&mut self, path: String, cx: &mut Context<Self>) {
        let items = repos::list_dir_items(&path);
        let mut excludes = std::collections::HashSet::new();
        for it in &items {
            if it.is_dir && INIT_EXCLUDE_HINTS.contains(&it.name.as_str()) {
                excludes.insert(it.name.clone());
            }
        }
        if let Some(d) = self.dlg.as_mut() {
            d.init = Some(InitState {
                path,
                items,
                excludes,
                working: false,
            });
            d.error = None;
        }
        cx.notify();
    }
}

impl Shell {
    /// 跑初始化：后台线程（git init + add + commit 要好几秒），
    /// 成功则入列表并选中，失败把原因留在弹窗里。
    pub(crate) fn run_init(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.dlg.as_ref() else {
            return;
        };
        let Some(init) = d.init.as_ref() else {
            return;
        };
        if init.working {
            return;
        }
        let path = init.path.clone();
        let excludes: Vec<String> = init.excludes.iter().cloned().collect();
        if let Some(d) = self.dlg.as_mut() {
            if let Some(i) = d.init.as_mut() {
                i.working = true;
            }
            d.error = None;
        }
        cx.notify();

        cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { repos::init_repo(&path, &excludes, true) })
                .await;
            let win = this.update(cx, |this, _| this.win).ok();
            this.update(cx, |this, cx| {
                match r {
                    Ok(entry) => {
                        this.repos = repos::list_repos();
                        this.views
                            .entry(entry.path.clone())
                            .or_insert_with(RepoView::blank)
                            .detected = run_cmd::detect_cmds(&entry.path);
                        this.selected = Some(entry.path.clone());
                        this.log_of(&entry.path).ts = qingbird_core::store::now_secs();
                        this.dlg = None;
                        this.refresh_status(cx);
                        this.scan_space(cx);
                        this.scan_profile(cx);
                        // 成了就把弹窗关掉；失败留在原地让用户看到原因
                        if let Some(win) = win {
                            let _ = cx.update_window(win, |_, window, cx| {
                                window.close_dialog(cx);
                            });
                        }
                    }
                    Err(e) => {
                        if let Some(d) = this.dlg.as_mut() {
                            d.error = Some(e);
                            if let Some(i) = d.init.as_mut() {
                                i.working = false;
                            }
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
