//! 添加 / 编辑仓库对话框。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::component::button::ButtonCustomVariant;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
// `Focusable` 只用来调 `focus_handle`，方法本身用不上，所以按 `as _` 引
use gpui_kit::{
    div, px, AppContext, Context, Div, Entity, Focusable as _, ParentElement, Styled, Window,
};
use qingbird_core::repos::{self};
use qingbird_core::run_cmd::{self};

use super::init::{init_body, init_footer};
use crate::state::*;
use crate::widgets::ghost_variant;
use crate::tokens::*;

/// 仓库弹窗的正文：路径 / 显示名 / 归属 + 错误提示。
/// 每帧重画（Dialog 的 content builder 会被反复调用），所以只接引用。
pub(crate) fn dlg_body(
    t: &Theme,
    editing: &Option<String>,
    third: bool,
    error: &Option<String>,
    path_input: &Entity<InputState>,
    name_input: &Entity<InputState>,
    this: &Entity<Shell>,
    gv: ButtonCustomVariant,
) -> Div {
    let field = |label: &str, input: &Entity<InputState>| -> Div {
        v_flex()
            .w_full()
            .gap(px(GAP_TIGHT))
            .child(
                div()
                    .text_size(px(fs_meta()))
                    .text_color(tier4(t))
                    .child(label.to_string()),
            )
            .child(Input::new(input).w_full())
    };
    let path_field = match editing {
        // 编辑时路径只读：换路径等于换仓库，不该在这个框里发生
        Some(p) => v_flex()
            .w_full()
            .gap(px(GAP_TIGHT))
            .child(
                div()
                    .text_size(px(fs_meta()))
                    .text_color(tier4(t))
                    .child("路径"),
            )
            .child(
                div()
                    .w_full()
                    .text_size(px(fs_sec()))
                    .font_family(t.mono_font_family.clone())
                    .text_color(tier3(t))
                    .child(p.clone()),
            ),
        None => v_flex()
            .w_full()
            .gap(px(GAP_TIGHT))
            .child(
                div()
                    .text_size(px(fs_meta()))
                    .text_color(tier4(t))
                    .child("路径"),
            )
            // ⚠️ **「浏览…」这颗是补接回来的，不是新设计**（用户 2026-10-09：
            // 「添加路径 原是可以点击进入本地资源管理器选择的 现在没有了 是没接线了吗」）。
            // 查实：旧 Tauri 版真有——`src/store.ts` 的 `addRepoDialog` 调
            // `open({ directory: true, title: "选择 git 仓库文件夹" })`；GPUI 重写时
            // **从来没写过这颗键**（`git log --all -S "浏览" -- gui/src/dialogs/repo.rs` 零命中）。
            // 能力本身一直在线：`os::pick_folder`（带 `CREATE_NO_WINDOW`）此前只有
            // 克隆框在用 ⇒ 缺的只是这一格接线，不是缺功能。
            // 输入框仍然留着：手打整条路径是唯一没有父目录可挑的场合
            .child(
                h_flex()
                    .w_full()
                    .gap(px(GAP_TIGHT))
                    .items_center()
                    .child(Input::new(path_input).w_full().flex_1())
                    .child(super::clone::folder_pick_button(
                        this,
                        gv,
                        FolderPick::AddRepo,
                    )),
            ),
    };

    v_flex()
        .w_full()
        .gap(px(GAP_LG))
        .child(path_field)
        .child(field("显示名", name_input))
        .child(
            v_flex()
                .w_full()
                .gap(px(GAP_TIGHT))
                .child(
                    div()
                        .text_size(px(fs_meta()))
                        .text_color(tier4(t))
                        .child("归属"),
                )
                .child(
                    h_flex()
                        .gap(px(GAP))
                        .child(
                            Button::new("dlg-mine")
                                .label("我的仓库")
                                .toggled(!third)
                                .on_click({
                                    let this = this.clone();
                                    move |_, _, cx| {
                                        this.update(cx, |this, cx| {
                                            if let Some(d) = this.dlg.as_mut() {
                                                d.third = false;
                                            }
                                            cx.notify();
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("dlg-third")
                                .label("第三方仓库")
                                .toggled(third)
                                .on_click({
                                    let this = this.clone();
                                    move |_, _, cx| {
                                        this.update(cx, |this, cx| {
                                            if let Some(d) = this.dlg.as_mut() {
                                                d.third = true;
                                            }
                                            cx.notify();
                                        });
                                    }
                                }),
                        ),
                ),
        )
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

/// 初始化向导正文：说明 + 路径 + 排除项勾选列表。

/// 弹窗底栏：编辑模式左侧「移除仓库」，右侧取消 / 保存。
pub(crate) fn dlg_footer(
    this: &Entity<Shell>,
    editing: &Option<String>,
    path_input: &Entity<InputState>,
    name_input: &Entity<InputState>,
    gv: ButtonCustomVariant,
) -> Div {
    let mut footer = h_flex().w_full().justify_between().items_center();
    match editing {
        Some(p) => {
            footer = footer.child(
                Button::new("dlg-remove")
                    .label("移除仓库")
                    .outline()
                    .on_click({
                        let this = this.clone();
                        let p = p.clone();
                        move |_, window, cx| {
                            this.update(cx, |this, cx| this.remove_repo_at(p.clone(), window, cx));
                            window.close_dialog(cx);
                        }
                    }),
            );
        }
        None => footer = footer.child(div()),
    }
    footer.child(
        h_flex()
            .gap(px(GAP))
            .child(Button::new("dlg-cancel").label("取消").custom(gv).on_click({
                let this = this.clone();
                move |_, window, cx| {
                    this.update(cx, |this, cx| {
                        this.dlg = None;
                        cx.notify();
                    });
                    window.close_dialog(cx);
                }
            }))
            .child(
                Button::new("dlg-save")
                    .label(if editing.is_some() {
                        "保存"
                    } else {
                        "添加"
                    })
                    .primary()
                    .on_click({
                        let this = this.clone();
                        let editing = editing.clone();
                        let path_input = path_input.clone();
                        let name_input = name_input.clone();
                        move |_, window, cx| {
                            // 值要在 App 上读成 String 再交给 Shell 落盘
                            let raw_path = path_input.read(cx).value().to_string();
                            let raw_name = name_input.read(cx).value().to_string();
                            let ok = this.update(cx, |this, cx| {
                                this.commit_dlg(editing.clone(), raw_path, raw_name, cx)
                            });
                            if ok {
                                window.close_dialog(cx);
                            }
                        }
                    }),
            ),
    )
}

impl Shell {
    // ---------- 添加 / 编辑仓库弹窗 ----------

    /// 打开「添加仓库」。没有原生目录选择对话框可用（gpui-kit 不带，
    /// 也不为它引新依赖），走「粘贴路径 + 校验」：资源管理器里复制文件地址即可。
    ///
    /// **归属默认取当前侧栏那一组**（D78）：用户的规则是"在哪个类别加就进当前分类"。
    /// 早先这里写死 `false`，于是站在「第三方仓库」页点「添加仓库」，
    /// 弹窗里默认勾的还是「我的仓库」——得再手点一下才对。
    /// 弹窗里那两颗键仍然留着，用户随时能改回来。
    pub(crate) fn open_add_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dlg = Some(RepoDialog {
            editing: None,
            path_input: cx.new(|cx| {
                InputState::new(window, cx).placeholder("粘贴仓库文件夹路径，如 F:/AIwork/xxx")
            }),
            name_input: cx.new(|cx| InputState::new(window, cx).placeholder("留空用目录名")),
            third: self.side_focus.is_third(),
            error: None,
            init: None,
        });
        self.build_dlg(window, cx);
    }
}

impl Shell {
    /// 打开「仓库设置」：改显示名与归属，或从列表移除（不动磁盘文件）。
    pub(crate) fn open_edit_dlg(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_repo_dlg(path, window, cx, false);
    }

    /// 「重命名…」：同一个对话框，但把光标直接放进「显示名」。
    ///
    /// 为什么不单开一个只有一行的对话框：多一个弹窗就多一套标题/按钮/校验，
    /// 而这两个入口要改的字段本来就重叠。区别只落在"焦点落在哪"，
    /// 这正是用户点「重命名…」时的意图。
    pub(crate) fn open_rename_dlg(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_repo_dlg(path, window, cx, true);
    }
}

impl Shell {
    fn open_repo_dlg(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
        focus_name: bool,
    ) {
        let third = self.entry(&path).map(|e| e.is_third_party).unwrap_or(false);
        let alias = self
            .entry(&path)
            .map(|e| e.alias.clone())
            .unwrap_or_default();
        // 当前别名进 placeholder：留空 = 不改（编辑模式不预填，避免误清空）
        let name_hint = if alias.trim().is_empty() {
            "留空用目录名".to_string()
        } else {
            format!("当前：{alias} · 留空即不改")
        };
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder(name_hint));
        self.dlg = Some(RepoDialog {
            editing: Some(path.clone()),
            path_input: cx.new(|cx| InputState::new(window, cx).placeholder(path.clone())),
            name_input: name_input.clone(),
            third,
            error: None,
            init: None,
        });
        if focus_name {
            // `InputState::focus` 不是公开 API，公开的只有 focus_handle。
            // 在弹窗挂上之后立刻聚焦——open_dialog 是同步建的，不用等下一帧
            let fh = name_input.read(cx).focus_handle(cx);
            window.focus(&fh, cx);
        }
        self.build_dlg(window, cx);
    }
}

impl Shell {
    /// 弹窗本体：官方 Dialog（覆盖层、聚焦、关闭都由框架管）。
    /// Shell 只负责把自己的状态与输入框挂进去。
    pub(crate) fn build_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        let Some(dlg) = self.dlg.as_ref() else {
            return;
        };
        let editing = dlg.editing.clone();
        let path_input = dlg.path_input.clone();
        let name_input = dlg.name_input.clone();

        window.open_dialog(cx, move |dialog, _window, cx| {
            let t = cx.theme().clone();
            let state = this.read(cx).dlg.as_ref();
            let third = state.map(|d| d.third).unwrap_or(false);
            let error: Option<String> = state.and_then(|d| d.error.clone());
            let init: Option<InitState> = state.and_then(|d| d.init.clone());

            dialog
                .title(if init.is_some() {
                    "初始化为 Git 仓库"
                } else if editing.is_some() {
                    "仓库设置"
                } else {
                    "添加仓库"
                })
                // content / footer 的 builder 会被反复调用，每次重画一份
                .content({
                    let t = t.clone();
                    let editing = editing.clone();
                    let path_input = path_input.clone();
                    let name_input = name_input.clone();
                    let error = error.clone();
                    let init = init.clone();
                    let this = this.clone();
                    // 幽灵按钮的悬停底色要从活的 `App` 上取，而 content 的 builder
                    // 是**没有 `cx` 的自由闭包** ⇒ 在这里算好按值带进去
                    // （`ButtonCustomVariant` 是 `Copy`）。与下面 `.footer(..)` 同一做法
                    let gv = ghost_variant(cx);
                    move |c, _, _| match &init {
                        Some(i) => c.child(init_body(&t, i, &error, &this)),
                        None => c.child(dlg_body(
                            &t,
                            &editing,
                            third,
                            &error,
                            &path_input,
                            &name_input,
                            &this,
                            gv,
                        )),
                    }
                })
                .footer({
                    // `gv` 在这里算一次传下去：footer 的 builder 是**没有 `cx`
                    // 的自由函数**（只收 `&Entity` + 数据），而幽灵按钮的悬停
                    // 底色必须从活的 `App` 上取（`ghost_variant`）。
                    // `ButtonCustomVariant` 是 `Copy`，按值传最省事。
                    let gv = ghost_variant(cx);
                    match &init {
                        Some(i) => init_footer(&t, i, &this, gv),
                        None => dlg_footer(&this, &editing, &path_input, &name_input, gv),
                    }
                })
        });
    }
}

impl Shell {
    /// 「浏览…」选完目录，回填到添加仓库框的「路径」格。
    ///
    /// 与 `clone.rs::pick_clone_folder` **同一套两拍**：按钮的 `on_click` 里
    /// 既没有活的 `&mut Window`（那一刻窗口已被 take 走，调了会静默失败），
    /// 回填用的 `set_value` 又必须要 window ⇒ 这里只置 `pending_folder`，
    /// 由 `Render::render` 下一帧兑现。
    ///
    /// **取消不动原值**：`pick_folder` 把取消给成 `Ok(None)` 而不是 `Err`——
    /// 用户按取消是一个正常决定，输入框里已经填好的路径原封不动
    pub(crate) fn pick_repo_path_folder(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picked) = self
            .dlg
            .as_ref()
            .map(|d| d.path_input.read(cx).value().to_string())
        else {
            // 弹窗已经关了（比如置位与兑现之间被别的东西关掉）——什么都不做
            return;
        };
        match crate::os::pick_folder(&picked) {
            Ok(Some(dir)) => {
                if let Some(d) = self.dlg.as_ref() {
                    let input = d.path_input.clone();
                    input.update(cx, |st, cx| st.set_value(dir, window, cx));
                }
                cx.notify();
            }
            Ok(None) => {}
            Err(e) => window.push_notification(e, cx),
        }
    }

    /// 保存弹窗：添加时校验路径与 .git，编辑时改名与改归属。
    /// 返回 true = 成功（调用方据此关弹窗），false = 留在原地看错误。
    pub(crate) fn commit_dlg(
        &mut self,
        editing: Option<String>,
        raw_path: String,
        raw_name: String,
        cx: &mut Context<Self>,
    ) -> bool {
        let third = self.dlg.as_ref().map(|d| d.third).unwrap_or(false);
        let name = raw_name.trim().to_string();

        match editing {
            None => {
                let path = repos::normalize_path_input(&raw_path);
                if path.is_empty() {
                    self.set_dlg_error("请先填写仓库路径", cx);
                    return false;
                }
                if self.repos.iter().any(|r| r.path == path) {
                    self.set_dlg_error("这个仓库已经在列表里了", cx);
                    return false;
                }
                // 不是 git 仓库不是用户的错：转初始化向导，别甩一句报错（旧版同行为）
                if !repos::valid_repo(&path) {
                    if !std::path::Path::new(&path).is_dir() {
                        self.set_dlg_error("文件夹不存在，请检查路径", cx);
                        return false;
                    }
                    self.enter_init(path, cx);
                    return false;
                }
                let list = match repos::add_repo(&path, third) {
                    Ok(l) => l,
                    Err(e) => {
                        self.set_dlg_error(&e, cx);
                        return false;
                    }
                };
                // 名字与归属都只在添加成功之后落盘，别留半成品。
                // ⚠️ **归属随 `add_repo` 一次写进去了**（D78），不再补第二句
                if !name.is_empty() {
                    let _ = repos::rename_repo(&path, &name);
                }
                self.repos = repos::list_repos();
                let _ = list;
                // 新仓库的命令要重新识别（启动时只识别了列表里已有的）
                self.views
                    .entry(path.clone())
                    .or_insert_with(RepoView::blank)
                    .detected = run_cmd::detect_cmds(&path);
                self.selected = Some(path.clone());
                self.logs.remove(&path);
                self.dlg = None;
                self.refresh_status(cx);
                self.scan_space(cx);
                self.scan_profile(cx);
                cx.notify();
                true
            }
            Some(path) => {
                // 留空 = 不改（placeholder 就是这么写的，别把别名清掉）
                if !name.is_empty() && repos::rename_repo(&path, &name).is_err() {
                    self.set_dlg_error("显示名保存失败（配置写不进去）", cx);
                    return false;
                }
                if qingbird_core::store::set_third_party(&path, third).is_err() {
                    self.set_dlg_error("归属保存失败（配置写不进去）", cx);
                    return false;
                }
                self.repos = repos::list_repos();
                self.dlg = None;
                // 归属变了主区换页：划成第三方要立刻补扫档案数据
                self.scan_profile(cx);
                cx.notify();
                true
            }
        }
    }
}

impl Shell {
    /// 写错误提示并通知重画：弹窗是浮层，不 notify 的话错误文案不会刷出来
    pub(crate) fn set_dlg_error(&mut self, msg: &str, cx: &mut Context<Self>) {
        if let Some(d) = self.dlg.as_mut() {
            d.error = Some(msg.to_string());
        }
        cx.notify();
    }
}
