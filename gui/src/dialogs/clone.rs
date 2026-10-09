//! 「克隆仓库…」对话框：地址 + 目标父目录 + 进度 + 实时输出。
//!
//! 三件事按顺序摆在一屏里：**远端地址**（要什么）→ **目标父目录**（放哪里）
//! → **进度与输出**（正在做什么）。这是一个多步的低频操作，不该挤进
//! 侧栏脚那两颗按钮里。
//!
//! 执行走 core 的 `clone::clone_repo`——和拉取/推送同一条 `net::run_net_op`
//! 链路，代理回退、凭证注入、传输停滞保护都在里面（GUI 不自己拼 git 子进程，D5）。
//!
//! **失败时对话框不关**：地址还留在输入框里，用户改完就能重试；
//! 关掉的话他得把刚才那串地址重新敲一遍。

use gpui_kit::component::button::ButtonCustomVariant;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, AnyElement, AnyWindowHandle, App, AppContext, Context, Div, Entity,
    Focusable as _, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window,
};
use qingbird_core::clone;
use qingbird_core::git;
use qingbird_core::net::NetMode;
use qingbird_core::repos;
use qingbird_core::types::ProgressEvent;

use crate::state::*;
use crate::widgets::{ghost_variant, progress_readout, progress_text, sunken_box};
use crate::tokens::*;

/// 实时输出最多留多少行。git 的进度行按 \r 反复刷新同一句，
/// 一次大克隆能刷出上千行，全留着对话框会被撑爆；只留尾巴——
/// 用户要看的"刚才发生了什么"就在那儿
const OUT_MAX_LINES: usize = 200;

impl Shell {
    /// 打开「克隆仓库…」对话框。
    ///
    /// 目标父目录默认取当前选中仓库的父目录：绝大多数用户的仓库都住在
    /// 同一个目录下，从那儿克隆最省事；一个仓库都没有时退回家目录
    pub(crate) fn open_clone_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {        let parent = self
            .selected
            .as_deref()
            .and_then(|p| std::path::Path::new(p).parent())
            .map(|p| p.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("USERPROFILE").ok())
            .unwrap_or_default();
        let url = cx.new(|cx| {
            InputState::new(window, cx).placeholder("仓库地址，如 https://github.com/user/repo.git")
        });
        let parent_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("克隆到哪个文件夹下"));
        // 预填父目录要一个活的 `Window`，而 `Entity::update` 的闭包只给
        // `Context`。所以每一轮都从外层**重新借**一次 window——
        // 直接 `move` 进闭包会在第一轮就把 `window` 移走
        let w: &mut Window = window;
        parent_input.update(cx, move |st, cx| st.set_value(parent, w, cx));

        // 焦点直接落在地址上——这是这个对话框里唯一必须先填的字段
        let fh = url.read(cx).focus_handle(cx);
        window.focus(&fh, cx);

        self.clone_dlg = Some(CloneDialog {
            url,
            parent: parent_input,
            running: false,
            stage: String::new(),
            detail: String::new(),
            percent: None,
            output: Vec::new(),
            error: None,
        });
        self.build_clone_dlg(window, cx);
    }

    /// 兑现「浏览…」：拉起系统目录选择器，选完回填「克隆到」那一格。
    ///
    /// **由 `Render::render` 调**（那里 `&mut self` 与 `window` 都齐），
    /// 所以这里能直接拿 `&mut Shell` 与 `&mut Window`——
    /// 按钮的 `on_click` 闭包里两样都缺（见 `folder_pick_button` 的注释）。
    ///
    /// **取消不动原值**：`pick_folder` 把取消给成 `Ok(None)` 而不是 `Err`，
    /// 用户按取消是一个正常决定；这里直接 return，输入框里已经填好的
    /// 路径原封不动。
    pub(crate) fn pick_clone_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.clone_dlg.as_ref() else {
            return;
        };
        // 跑着的时候别让人改目标目录：这一轮的路径已经交给 git 了
        if d.running {
            window.push_notification("正在克隆，先停掉这一轮再改目录", cx);
            return;
        }
        let current = d.parent.read(cx).value().to_string();
        match crate::os::pick_folder(&current) {
            Ok(Some(picked)) => {
                let parent = self.clone_dlg.as_ref().map(|d| d.parent.clone());
                if let Some(parent) = parent {
                    parent.update(cx, |st, cx| st.set_value(picked, window, cx));
                }
                cx.notify();
            }
            // 取消：什么都不做，也不报错
            Ok(None) => {}
            Err(e) => window.push_notification(e, cx),
        }
    }
}

impl Shell {
    /// 对话框本体（官方 Dialog）。`content` 的闭包每帧重画，所以进度与
    /// 输出每帧从 `Entity<Shell>` 读；`footer` 是**一次性元素**
    /// （官方签名就是 `impl IntoElement`），所以那里的按钮只能回写 Entity，
    /// 跑起来时的状态由 content 里的进度区承担
    fn build_clone_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let t = cx.theme().clone();
            dialog
                .title("克隆仓库")
                .content({
                    let t = t.clone();
                    let this = this.clone();
                    move |c: DialogContent, _w, cx: &mut App| {
                        let st = this.read(cx).clone_state();
                        c.child(clone_body(&this, ghost_variant(cx), &t, &st))
                    }
                })
                .footer(clone_footer(&this, ghost_variant(cx)))
        });
    }
}

impl Shell {
    /// 跑克隆。**后台任务**——一次大仓库冷克隆是分钟级的，
    /// 放 UI 线程上界面会整块冻住
    pub(crate) fn run_clone(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.clone_dlg.as_ref() else {
            return;
        };
        if d.running {
            return;
        }
        let url = d.url.read(cx).value().to_string();
        let parent = d.parent.read(cx).value().to_string();
        // 归属在**这里**定死：core 不认识界面，不知道用户当时在看哪一组。
        // 取当前侧栏那一组（D78）——与拖放、添加弹窗同一个口径。
        let third = self.side_focus.is_third();
        if url.trim().is_empty() {
            self.set_clone_error("请填写仓库地址".into(), cx);
            return;
        }
        if parent.trim().is_empty() {
            self.set_clone_error("请填写目标文件夹".into(), cx);
            return;
        }
        // 先解析一次仓库名：解析不出来就没必要起 git 子进程，
        // 而且「无法从地址中解析出仓库名」比 git 的英文报错有用得多
        if repos::repo_name_from_url(&url).is_none() {
            self.set_clone_error(
                "无法从地址中解析出仓库名（地址应形如 host/owner/repo）".into(),
                cx,
            );
            return;
        }

        if let Some(d) = self.clone_dlg.as_mut() {
            d.running = true;
            d.stage = "准备中…".into();
            d.detail.clear();
            d.percent = None;
            d.output.clear();
            d.error = None;
        }
        cx.notify();

        // 进度回调跑在后台线程上。**不要在后台线程里 `Entity::update`**：
        // core 不认识 gpui，界面状态也不该跨线程被改。这里用一个
        // `Arc<Mutex<..>>` 把事件攒起来，由主线程一次性倒进状态
        let sink = std::sync::Arc::new(std::sync::Mutex::new(ProgressSink::default()));
        let sink2 = sink.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move {
                    let on_progress = |ev: &ProgressEvent| {
                        if let Ok(mut s) = sink2.lock() {
                            s.push(ev);
                        }
                    };
                    clone::clone_repo(&url, &parent, NetMode::Auto, third, Some(&on_progress))
                })
                .await;
            let win = this.update(cx, |this, _| this.win).ok();
            let sink3 = sink.clone();
            this.update(cx, |this, cx| {
                if let Ok(s) = sink3.lock() {
                    if let Some(d) = this.clone_dlg.as_mut() {
                        s.drain_into(d);
                    }
                }
                match r {
                    Ok(out) => this.finish_clone(out, win, cx),
                    Err(e) => {
                        if let Some(d) = this.clone_dlg.as_mut() {
                            d.running = false;
                            d.error = Some(e);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 成功：关对话框、通知一句、刷新列表，并**把定位留给下一帧**。
    ///
    /// ## 为什么定位要延后一帧
    ///
    /// 这个函数是从 `this.update(cx, |this, cx| …)` 里面调的，
    /// 此刻 `Shell` 已经被借用了。在里面再 `downcast::<Shell>()` 拿回同一个
    /// Entity 去做 `reveal_repo` 会因为拿不到锁而**静默失败**（`update` 返回
    /// `Err`），用户看到的就是"克隆成功了但侧栏没动"。
    /// 而 `reveal_repo` 又必须同时拿 `&mut Shell` 和 `&mut Window`——
    /// 后台任务里两者都凑不齐（core 不认识 gpui，也没有 `&mut Window`）。
    ///
    /// 所以这里只置一个一次性标记，由 `Render::render` 在下一帧兑现：
    /// 那一帧里 `&mut self` 与 `window` 都在手边。跟 `apply_reveal` 同一个套路。
    fn finish_clone(
        &mut self,
        out: clone::CloneOutcome,
        win: Option<AnyWindowHandle>,
        cx: &mut Context<Self>,
    ) {
        self.repos = out.repos;
        self.views
            .entry(out.path.clone())
            .or_insert_with(RepoView::blank);
        // 这行以前**恒为 false**：`add_repo` 造的是默认记录，通知里的组别
        // 于是永远说「我的仓库」。现在归属随 `clone_repo(.., third, ..)` 一起落盘（D78），
        // 回读才有意义
        let third = self
            .entry(&out.path)
            .map(|e| e.is_third_party)
            .unwrap_or(false);
        self.clone_dlg = None;
        self.pending_reveal = Some(out.path.clone());

        if let Some(w) = win {
            let name = out.name;
            let group = if third {
                "第三方仓库"
            } else {
                "我的仓库"
            };
            // 这里只用 `Window` 自己的方法（关窗 + 通知），不碰 Shell——
            // `update_window` 给的闭包里那个 `&mut Shell` 是拿不到的
            let _ = cx.update_window(w, move |_view, window, cx| {
                window.close_dialog(cx);
                window.push_notification(format!("已克隆「{name}」，已加入「{group}」"), cx);
            });
        }
    }

    /// 取消走 core 的全局取消标记。**不在这里清目标目录**——
    /// 那是 `clone_repo` 失败分支的事，它知道那个目录是本次尝试产生的
    pub(crate) fn cancel_clone(&mut self, cx: &mut Context<Self>) {
        let running = self.clone_dlg.as_ref().map(|d| d.running).unwrap_or(false);
        if !running {
            self.clone_dlg = None;
            cx.notify();
            return;
        }
        git::request_cancel();
        if let Some(d) = self.clone_dlg.as_mut() {
            d.stage = "正在取消…".into();
        }
        cx.notify();
    }

    fn set_clone_error(&mut self, e: String, cx: &mut Context<Self>) {
        if let Some(d) = self.clone_dlg.as_mut() {
            d.error = Some(e);
        }
        cx.notify();
    }
}

// ================= 展示 =================

/// 对话框正文要用的字段快照。
///
/// 带 `Entity` 而不是字符串：`Dialog::content` 的闭包里要挂 `Input`，
/// 而那个闭包只有 `&mut App`，拿不到 `&mut Context<Shell>`——
/// 所以渲染与读取在这里**分开**：渲染要 Entity，读取（校验、提交）
/// 在 `run_clone` 里有真正的 `Context` 可用
pub(crate) struct CloneView {
    /// 对话框关着时是 None——那时没有输入框可挂，body 也不该画那两行
    url: Option<Entity<InputState>>,
    parent: Option<Entity<InputState>>,
    running: bool,
    stage: String,
    detail: String,
    percent: Option<f32>,
    output: Vec<String>,
    error: Option<String>,
}

impl Shell {
    pub(crate) fn clone_state(&self) -> CloneView {
        match self.clone_dlg.as_ref() {
            Some(d) => CloneView {
                url: Some(d.url.clone()),
                parent: Some(d.parent.clone()),
                running: d.running,
                stage: d.stage.clone(),
                detail: d.detail.clone(),
                percent: d.percent,
                output: d.output.clone(),
                error: d.error.clone(),
            },
            None => CloneView {
                url: None,
                parent: None,
                running: false,
                stage: String::new(),
                detail: String::new(),
                percent: None,
                output: Vec::new(),
                error: None,
            },
        }
    }
}

fn clone_body(this: &Entity<Shell>, gv: ButtonCustomVariant, t: &Theme, st: &CloneView) -> Div {
    v_flex()
        .w_full()
        .gap(px(GAP_LG))
        .when_some(st.url.clone(), |b, e| {
            b.child(field(t, "仓库地址", Input::new(&e).small().w_full()))
        })
        .when_some(st.parent.clone(), |b, e| {
            // 阶段 21-2：**输入框 + 一颗「浏览…」**。原先只有一个裸输入框，
            // 要用户手打一整条 Windows 路径；旧 Tauri 版有这个入口，重写时掉了。
            // 输入框仍然留着——「浏览」是选目录，而整条路径是唯一没有父目录的场合
            b.child(field(
                t,
                "克隆到",
                h_flex()
                    .w_full()
                    .gap(px(GAP_TIGHT))
                    .items_center()
                    .child(Input::new(&e).small().flex_1())
                    .child(folder_pick_button(this, gv, FolderPick::Clone)),
            ))
        })
        .child(hint(t))
        .when(
            st.running || !st.output.is_empty() || st.error.is_some(),
            |b| b.child(progress_block(t, st)),
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

/// 「浏览…」那颗键。
///
/// ## 为什么不能在这里直接拉起对话框
///
/// `Dialog::content` 的闭包只有 `&mut App`，拿不到 `&mut Context<Shell>`，
/// 也拿不到活着的 `&mut Window`（`InputState::set_value` 要后者）。
/// 而**在窗口自己的事件回调里调 `cx.update_window(..)` 会静默失败**——
/// 那一刻窗口已经被 `App::update_window_id` 从 `cx.windows` 里 take 走了，
/// 闭包整个被丢掉，表现为「点着没反应」（AGENTS.md 与 `repo_head.rs` 记着同一个坑）。
///
/// 所以走**一次性标记**（`Shell::pending_folder`）：这里只置位，
/// 由 `Render::render` 下一帧兑现——那一帧里 `&mut self` 与 `window` 都在手边。
/// 与 `pending_reveal` / `pending_head_copy` 同一个套路。
///
/// ⚠️ `target` 是"这一颗键属于哪个弹窗"。添加仓库那个框也要选目录，
/// 而回填的是两份不同的 `InputState` ⇒ 这个信息**做成参数**，
/// 不留给兑现处去猜当前开着的是哪个框（D78）
pub(crate) fn folder_pick_button(
    this: &Entity<Shell>,
    gv: ButtonCustomVariant,
    target: FolderPick,
) -> AnyElement {
    Button::new("clone-browse")
        .label("浏览…")
        .custom(gv)
        .with_size(Size::Small)
        .tooltip("在资源管理器里挑一个目录")
        .accessibility_label("选择目标文件夹")
        .on_click({
            let this = this.clone();
            move |_, _, cx| {
                // 不在这里 notify：标记会在 render 那一帧兑现并自行 notify
                this.update(cx, |this, _| {
                    this.pending_folder = Some(target);
                });
            }
        })
        .into_any_element()
}

fn field(t: &Theme, label: &str, input: impl IntoElement) -> Div {
    v_flex()
        .w_full()
        .gap(px(GAP_TIGHT))
        .child(
            div()
                .text_size(px(fs_meta()))
                .text_color(tier4(t))
                .child(label.to_string()),
        )
        .child(input)
}

fn hint(t: &Theme) -> Div {
    div()
        .w_full()
        .text_size(px(fs_meta()))
        .text_color(tier4(t))
        .child("支持 https / ssh / git@host:owner/repo 三种写法；私有仓库请先在本机 Git 凭证管理器里登录。")
}

/// 进度区：阶段词 + 百分比 + 实时输出。
/// **百分比不做假**：core 只在 git 真的报了百分比时才给，没有就走
/// 官方的 `Progress::loading(true)` 不定态条——一个一直在跳的假进度
/// 比没有进度更让人焦
fn progress_block(t: &Theme, st: &CloneView) -> Div {
    // 第三份等宽输出盒（另两份在 `log.rs` 与 `history_tab.rs`）⇒ 同一颗 `sunken_box`
    let mut log = sunken_box(
        v_flex()
            .w_full()
            .max_h(px(140.0))
            .gap(px(GAP_TIGHT))
            .p(px(GAP))
            .font_family(t.mono_font_family.clone()),
        t,
    )
    .id("clone-out")
    .overflow_y_scroll();

    if st.output.is_empty() {
        log = log.child(
            div()
                .text_size(px(fs_meta()))
                .text_color(tier4(t))
                .child(if st.running {
                    "等待远端响应…"
                } else {
                    "（没有输出）"
                }),
        );
    } else {
        for line in &st.output {
            // 只给**前缀**标色，不整段染红——理由同 `log.rs::output_stream`：
            // 整段红会把输出变成一片警示墙，真正要看的报错那几行反而被淹掉
            let (mark, color) = match line.chars().next() {
                Some('✗') => (Some('✗'), t.colors.danger),
                Some('✓') => (Some('✓'), t.colors.success),
                _ => (None, tier3(t)),
            };
            let rest = match mark {
                Some(_) => {
                    line[line.char_indices().nth(1).map_or(line.len(), |(i, _)| i)..].trim_start()
                }
                None => line.as_str(),
            };
            log = log.child(
                h_flex()
                    .w_full()
                    .gap(px(GAP_TIGHT))
                    .items_start()
                    .child(
                        div()
                            .w(px(10.0))
                            .flex_shrink_0()
                            .text_size(px(fs_meta()))
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .text_color(color)
                            .child(mark.map(|c| c.to_string()).unwrap_or_default()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(fs_meta()))
                            .text_color(color)
                            .child(rest.to_string()),
                    ),
            );
        }
    }

    // 跑的时候是「阶段（路径） · 网速」，跑完是「…（已结束）」。
    //
    // **委托给共用的 `widgets::progress_readout`**：原先这里是手搓的第二份，
    // 而日志卡那份只画了一根条——同一份 `ProgressEvent`、同一个 core，
    // 两处渲染却不一样（2026-10-05 收到一个函数里）。
    // 顺带统一了一处配色：原先"正在跑"用 `body(t)`、"已结束"用 `tier4(t)`，
    // 现在两处进度读数都用 `tier3(t)`，跑完那句话不再单独暗一档
    let text = if st.running {
        progress_text(&st.stage, &st.detail)
    } else if st.stage.is_empty() {
        "已结束".to_string()
    } else {
        format!("{}（已结束）", st.stage)
    };

    v_flex()
        .w_full()
        .gap(px(GAP))
        .child(progress_readout(t, "clone-progress", &text, st.percent))
        .child(log)
}

fn clone_footer(this: &Entity<Shell>, gv: ButtonCustomVariant) -> Div {
    h_flex()
        .w_full()
        .justify_end()
        .items_center()
        .gap(px(GAP))
        .child(
            Button::new("clone-cancel")
                .label("取消")
                .custom(gv)
                .with_size(Size::Small)
                .on_click({
                    let this = this.clone();
                    move |_, window, cx| {
                        let running = this
                            .read(cx)
                            .clone_dlg
                            .as_ref()
                            .map(|d| d.running)
                            .unwrap_or(false);
                        this.update(cx, |this, cx| this.cancel_clone(cx));
                        // 没在跑就说明是真关闭；跑着的时候 core 那边
                        // 会把 git 停掉并回一个「克隆已取消」，那时才关窗
                        if !running {
                            window.close_dialog(cx);
                        }
                    }
                }),
        )
        .child(
            Button::new("clone-go")
                .label("开始克隆")
                .primary()
                .with_size(Size::Small)
                .on_click({
                    let this = this.clone();
                    move |_, _, cx| {
                        this.update(cx, |this, cx| this.run_clone(cx));
                    }
                }),
        )
}

// ================= 后台进度汇聚 =================

/// 后台线程把 `ProgressEvent` 攒在这里，主线程一次性取走。
#[derive(Default)]
struct ProgressSink {
    /// `(阶段, 网速, 展示文本)` 成对存。**不要靠 `split_once(" · ")` 从展示
    /// 文本里反解阶段**——展示文本换成 git 原文之后那个分隔符就不存在了，
    /// 阶段会整个退化成整行原文（2026-10-05 改输出为原文时踩到）。
    ///
    /// 网速单独一栏：它要上进度条旁边那行可见文字（`已传输 1.21 MiB · 56.00 KiB/s`），
    /// 而展示文本那一列是 git 原文，从那里拆不出速度
    events: Vec<(String, String, String)>,
    last: String,
    percent: Option<f32>,
}

impl ProgressSink {
    fn push(&mut self, ev: &ProgressEvent) {
        // git 的进度行按 \r 反复刷新同一句（"接收对象 42%" → "43%" → …），
        // 全攒下来能把输出区刷爆。只在**文字真的变了**时记一条
        //
        // **优先用 git 原文**（用户要的是"像终端那样"），raw 为空
        // ——也就是我们自己发的阶段提示——才退回中文转述。
        let text = if !ev.raw.is_empty() {
            ev.raw.clone()
        } else if ev.detail.is_empty() {
            ev.stage.clone()
        } else {
            format!("{} · {}", ev.stage, ev.detail)
        };
        if text != self.last {
            self.last = text.clone();
            self.events.push((ev.stage.clone(), ev.detail.clone(), text));
        }
        if let Some(p) = ev.percent {
            self.percent = Some(p);
        }
    }

    fn drain_into(&self, d: &mut CloneDialog) {
        for (stage, detail, text) in &self.events {
            // 阶段词取最后一条非空的：它才是"现在在干什么"。
            // 空 stage = 认不出的原文行，不该把上一次的阶段冲掉
            if !stage.is_empty() {
                d.stage = stage.clone();
            }
            // 网速同理：没有速度的行（`remote: Enumerating…`）不该把上一条清掉
            if !detail.is_empty() {
                d.detail = detail.clone();
            }
            d.output.push(text.clone());
        }
        if d.output.len() > OUT_MAX_LINES {
            let drop = d.output.len() - OUT_MAX_LINES;
            d.output.drain(..drop);
        }
        if let Some(p) = self.percent {
            d.percent = Some(p);
        }
    }
}
