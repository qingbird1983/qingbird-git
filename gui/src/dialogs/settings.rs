//! 「设置」对话框：提交身份 / 访问令牌 / SSH 私钥 / 代理 / 系统通知。
//!
//! **一个 `Dialog` 而不是新开一屏**（Q4，2026-10-03 用户拍板）：`Page` 只有
//! 总览与仓库两态是设计 2.1 的硬约定，而设置属低频层——低频层不该占掉
//! 一整屏导航位。导航轨那颗齿轮本来就是个没有 `on_click` 的空壳，
//! 接上一个对话框正好。
//!
//! ## 三条落盘约定
//!
//! 1. **文本字段走「保存」**：六格一起写进 `store::Settings`。逐项存会让
//!    "取消"变成假的——用户撤销不了刚才敲错的那个令牌。
//! 2. **系统通知开关即时生效**：拨了开关又点取消，状态与界面必须一致。
//!    这一点与旧版 `set_notify_auto_sync` 的注释是同一个理由。
//! 3. **保存时重新 `load` 再改那六个字段**，而不是拿打开时那份快照整体覆盖：
//!    对话框开着的时候侧栏宽度、归属这些也可能被改过，整体覆盖会把它们抹掉。
//!
//! ## 版面：左分类导航 + 右内容（阶段 20 的 19-7）
//!
//! 三类：**身份与访问**（提交身份 / 令牌 / SSH）、**网络与通知**（代理 / 系统通知）、
//! **显示**（主题 / 显示哪些模块）。正文**定高**且右栏自带滚动条——
//! 平铺那一版是被外层 `max_h` 裁死的，根因见 `settings_body` 的函数头。
//!
//! ## 主题：深浅两套（阶段 20）
//!
//! 「显示」页那颗开关换的是**全局** `Theme`，因此**开着的这个弹窗会跟着一起
//! 变色**——那是必须的：`Theme::update` 之后没有别的路让浮层重画，
//! "外壳变了、弹窗还是老的"看着就像坏了。即时落盘，不挂「保存」。
//! 配色两套都在 `gui/theme.json` 里；`theme.json` 覆盖不到的那几级灰阶
//! 在 `tokens.rs`，两套值都在那儿。

use gpui_kit::component::button::ButtonCustomVariant;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable as _, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, AppContext, Context, Div, Entity, FontWeight, InteractiveElement, IntoElement,
    ParentElement, Pixels, SharedString, StatefulInteractiveElement, Styled, Window,
};
use qingbird_core::proxy;
use qingbird_core::store::{self, CloseAction};

use crate::detail::DetailTab;
use crate::state::*;
use crate::widgets::ghost_variant;
use crate::tokens::*;

/// 代理探测一次最多试几个候选。挨个试完 7897/7890/10808 加系统代理
/// 要好几十秒，而用户点「检测」只想知道"现在有没有代理可用"
const PROBE_MAX: usize = 4;

impl Shell {
    /// 打开设置。六格全部预填当前值（`InputState::set_value`），
    /// 空的那几格用 placeholder 说清该填什么
    pub(crate) fn open_settings_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = qingbird_core::store::load().settings;
        // 闭包里要改 `window`（`set_value` 要一个活的 `&mut Window`），
        // 所以绑定得是 `mut`
        let mut mk = |ph: &'static str, v: String, cx: &mut Context<Self>| {
            let e = cx.new(|cx| InputState::new(window, cx).placeholder(ph));
            if !v.is_empty() {
                let w: &mut Window = window;
                e.update(cx, move |st, cx| st.set_value(v, w, cx));
            }
            e
        };
        let git_name = mk("你的名字", s.git_name.clone(), cx);
        let git_email = mk("you@example.com", s.git_email.clone(), cx);
        let github_token = mk("ghp_…（只存在本机配置里）", s.github_token.clone(), cx);
        let gitee_token = mk("Gitee 私人令牌", s.gitee_token.clone(), cx);
        let ssh_key = mk(
            "C:\\Users\\你\\.ssh\\id_ed25519",
            s.ssh_key_path.clone(),
            cx,
        );
        let proxy_input = mk("留空即自动探测", s.manual_proxy.clone(), cx);

        // 模块勾选区预填（阶段 19）。**存的是"勾上 = 显示"**，
        // 而 core 存的是"藏了哪些"——打开时翻一次，保存时翻回去。
        // 归属基线（第三方天生没有「空间」与分叉条）在这两层之间**不参与**：
        // 它不是用户的偏好，是安全边界，界面上表现为**禁用 + 勾上**
        let mut mod_mine = Vec::new();
        for t in DetailTab::ALL {
            if store::module_applicable(false, t.module_key())
                && store::module_shown(&s.hidden_modules_mine, t.module_key())
            {
                mod_mine.push(t.module_key().to_string());
            }
        }
        for k in [store::MOD_SEC_FORK, store::MOD_SEC_SYNC, store::MOD_SEC_LOG, store::MOD_SEC_LANG] {
            if store::module_shown(&s.hidden_modules_mine, k) {
                mod_mine.push(k.to_string());
            }
        }
        let mut mod_third = Vec::new();
        for t in DetailTab::ALL {
            if store::module_applicable(true, t.module_key())
                && store::module_shown(&s.hidden_modules_third, t.module_key())
            {
                mod_third.push(t.module_key().to_string());
            }
        }
        for k in [store::MOD_SEC_FORK, store::MOD_SEC_SYNC, store::MOD_SEC_LOG, store::MOD_SEC_LANG] {
            if store::module_applicable(true, k) && store::module_shown(&s.hidden_modules_third, k) {
                mod_third.push(k.to_string());
            }
        }

        self.settings_dlg = Some(SettingsDialog {
            cat: SetCat::Identity,
            nav_hover: None,
            git_name,
            git_email,
            github_token,
            gitee_token,
            ssh_key,
            proxy: proxy_input,
            notify: s.notify_auto_sync,
            theme_dark: s.theme == crate::theme::DARK,
            close_action: store::close_action_of(&s.close_action),
            probing: false,
            probe_note: None,
            mod_mine,
            mod_third,
            error: None,
        });
        self.build_settings_dlg(window, cx);
    }

    /// 勾掉 / 勾上一个模块。**即时落盘 + 即时生效**——与系统通知同口径
    ///
    /// ⚠️ **不挂在「保存」上**：这个开关改的是**现在这一屏摆着什么**，
    /// 用户拨完就期待立刻看见变化。让他再去点一次保存、而中间这一屏
    /// 还按旧的摆着，等于告诉他"你刚才拨的没生效"
    pub(crate) fn toggle_module(
        &mut self,
        third: bool,
        key: &'static str,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 基线里的项**碰都不碰**：它是安全边界（第三方的「空间」页里有
        // 一键清理，会真删文件），不是一个可以打开的偏好。
        // 界面把那几颗置灰，这里是最后一道
        if !store::module_applicable(third, key) {
            return;
        }
        let Some(d) = self.settings_dlg.as_mut() else { return };
        let list = if third {
            &mut d.mod_third
        } else {
            &mut d.mod_mine
        };
        let k = key.to_string();
        // **按传进来的新值设，不自己翻转**。官方 `Checkbox::on_click` 给的
        // 就是"点完之后该是什么"，照它设；自己翻转的话，界面与状态
        // 一旦对不上（比如别处改了同一项）就会越拨越歪，而且那种错
        // 看上去还像"界面抖了一下"
        if on {
            if !list.contains(&k) {
                list.push(k);
            }
        } else {
            list.retain(|x| *x != k);
        }

        // 翻成"藏了哪些"落盘
        let all: Vec<&str> = {
            let mut v: Vec<&str> = DetailTab::ALL.iter().map(|t| t.module_key()).collect();
            v.extend([
                store::MOD_SEC_FORK,
                store::MOD_SEC_SYNC,
                store::MOD_SEC_LOG,
                store::MOD_SEC_LANG,
            ]);
            v
        };
        let shown = if third { &d.mod_third } else { &d.mod_mine };
        let hidden: Vec<String> = all
            .iter()
            // ⚠️ **基线项不写进配置**。它们由 core 的 `THIRD_BASELINE_HIDDEN`
            // 单独管着，而 `mod_*` 里永远不会出现它们（`open_settings_dlg`
            // 预填时就按 `module_applicable` 过滤过了），所以这一层不滤的话
            // 每次拨任何一颗都会把基线顺手抄进用户的"藏了哪些"——
            // 存进去不难看，**难看不难修**：将来若放开某一项基线，
            // 这条陈年记录会继续把它按在隐藏里，而用户从没做过那个选择
            .filter(|k| store::module_applicable(third, k))
            .filter(|k| !shown.iter().any(|x| x == *k))
            .map(|k| k.to_string())
            .collect();

        let mut cfg = store::load();
        if third {
            cfg.settings.hidden_modules_third = hidden.clone();
        } else {
            cfg.settings.hidden_modules_mine = hidden.clone();
        }
        if let Err(e) = store::save(&cfg) {
            window.push_notification(format!("保存失败：{e}"), cx);
            return;
        }
        // 写回 `Shell`：页签栏与概览四段每帧都问它，不写回就是"存住了但没生效"
        if third {
            self.hidden_third = hidden;
        } else {
            self.hidden_mine = hidden;
        }
        // 当前停在的那一页可能被刚藏掉了 → 夹回可见序列，
        // 否则内容区会停在一个页签栏上**根本没有**的那一页
        let ix = self.detail_tab.min(self.visible_detail_tabs(self.selected_is_third()).len() - 1);
        self.detail_tab = ix;
        cx.notify();
    }

    /// 左侧换分类。**不落盘**：停在哪一屏是这次打开的临时状态，
    /// 存进配置的话下次打开会莫名其妙直接落在「显示」上
    pub(crate) fn set_settings_cat(&mut self, cat: SetCat, cx: &mut Context<Self>) {
        let Some(d) = self.settings_dlg.as_mut() else {
            return;
        };
        if d.cat == cat {
            return;
        }
        d.cat = cat;
        // 换页时把悬停也清掉：指针还停在原来那颗的位置上，
        // 底色却跟着新页走，看上去像"这一颗被选中了"
        d.nav_hover = None;
        cx.notify();
    }

    fn build_settings_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let t = cx.theme().clone();
            dialog
                .title("设置")
                // ⚠️ **448 → 720**（阶段 20）。官方默认 448px（`dialog.rs:219`），
                // 那是给"确认一下"的弹窗的宽度。左导航 + 右内容这套版面
                // 挤在 448 里的话，右边能放的字不到 30 个一行，
                // 输入框与勾选框全得折行。**上限是视口减去两侧留白**，
                // 官方自己会夹（`dialog.rs:580`）
                .w(px(720.0))
                .content({
                    let t = t.clone();
                    let this = this.clone();
                    move |c: DialogContent, w: &mut Window, cx: &mut App| {
                        // 对话框被 Esc 关掉时 `settings_dlg` 已经清空，
                        // 这一帧就什么都不画——不能拿一份编出来的空快照顶上，
                        // 那会让输入框在一帧之内全部消失
                        match this.read(cx).settings_state() {
                            Some(st) => {
                                // 正文高度**这一帧**才算：它跟着窗口大小走，
                                // 写死一个数就会在小窗口上又回到裁切
                                let h = pane_height(w);
                                c.child(settings_body(&t, &st, &this, ghost_variant(cx), h))
                            }
                            None => c,
                        }
                    }
                })
                .footer(settings_footer(&this, ghost_variant(cx)))
        });
    }

    /// 保存六格。**重新 load 配置**再改这六个字段——见模块头的第 3 条
    pub(crate) fn save_settings(
        &mut self,
        values: [String; 6],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut cfg = store::load();
        cfg.settings.git_name = values[0].trim().to_string();
        cfg.settings.git_email = values[1].trim().to_string();
        cfg.settings.github_token = values[2].trim().to_string();
        cfg.settings.gitee_token = values[3].trim().to_string();
        cfg.settings.ssh_key_path = values[4].trim().to_string();
        cfg.settings.manual_proxy = values[5].trim().to_string();
        match store::save(&cfg) {
            Ok(()) => {
                self.settings_dlg = None;
                window.push_notification("设置已保存", cx);
                true
            }
            Err(e) => {
                if let Some(d) = self.settings_dlg.as_mut() {
                    d.error = Some(format!("保存失败：{e}"));
                }
                cx.notify();
                false
            }
        }
    }

    /// 系统通知开关。**即时落盘**，不挂在「保存」上——
    /// 拨了开关又点取消，界面与实际行为不一致是最伤信任的一种不一致
    fn toggle_notify(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        let mut cfg = store::load();
        cfg.settings.notify_auto_sync = on;
        if let Err(e) = store::save(&cfg) {
            window.push_notification(format!("保存失败：{e}"), cx);
            return;
        }
        if let Some(d) = self.settings_dlg.as_mut() {
            d.notify = on;
        }
        // 打开时当场弹一条：用户不必等下一次自动同步才知道通知能不能出来
        if on {
            window.push_notification("自动同步结果会发系统通知", cx);
        }
        cx.notify();
    }

    /// 深色主题开关。**即时落盘 + 即时换整套配色**，与系统通知同口径。
    ///
    /// 换的是**全局** `Theme`，所以开着这个对话框时它自己也一起变色——
    /// 那是必须的，`Theme::update` 之后没有别的路能让浮层跟着重画，
    /// 而"外壳变了、弹窗还是老的"看着就像坏了
    pub(crate) fn toggle_theme(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        // 设置对话框里的开关：除了换配色，还要把**它自己**那一行翻回去
        // （对话框可能是这次改完还开着的，见上面那段注释）
        if Self::toggle_theme_static(on, window, cx) {
            if let Some(d) = self.settings_dlg.as_mut() {
                d.theme_dark = on;
            }
        }
    }

    /// 换主题的**唯一实现**（落盘 + 换配色）。
    ///
    /// ## 为什么要抽成不依赖 `&mut Shell` 的静态版
    ///
    /// 标题栏那颗（阶段 21-4）在 `cx.listener` 里被调，签名只能拿到
    /// `(&mut Shell, &mut Window, &mut Context<Self>)`——**那本来是够的**，
    /// 但 `theme_toggle` 自己要读 `cx.theme()` 决定摆哪个图标，
    /// 于是调用点在 `Shell` 上会形成"为了读主题而借 Shell"，绕开更麻烦。
    ///
    /// 更要紧的是**别写两份**：设置里那颗与标题栏那颗必须落同一个配置字段、
    /// 走同一条 `theme::apply`。两份实现改了一处就分家，而分家的后果是
    /// "标题栏切了、设置里那颗还显示旧状态"——只有用户把两边都点一遍才发现。
    ///
    /// 返回 `true` = 落盘成功（失败时已通知，且**没有**换配色）
    pub(crate) fn toggle_theme_static(
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut cfg = store::load();
        cfg.settings.theme = if on {
            crate::theme::DARK
        } else {
            crate::theme::LIGHT
        }
        .to_string();
        if let Err(e) = store::save(&cfg) {
            window.push_notification(format!("保存失败：{e}"), cx);
            return false;
        }
        crate::theme::apply(&cfg.settings.theme, cx);
        cx.notify();
        true
    }
    /// 「检测代理」。
    ///
    /// 填了地址就只试那一个；留空就按 core 的候选顺序（手动 → 上次成功 →
    /// 系统代理 → 常见端口）试前几个。**只报结果不改配置**——
    /// 自动探测本来就是 git 网络操作失败时的兜底，手动填一个"试过了能用"
    /// 的地址进来是用户自己的决定，不该由一次检测替他做
    fn probe_proxy(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.settings_dlg.as_ref() else {
            return;
        };
        if d.probing {
            return;
        }
        let typed = d.proxy.read(cx).value().trim().to_string();
        let cfg = store::load();
        // socks 只做 TCP 可达、http 做 CONNECT 实测，每个都要等一个来回，
        // 放后台线程，别让界面在按下按钮后僵住
        let cands: Vec<String> = if typed.is_empty() {
            proxy::gather_candidates(&cfg.settings, &cfg.last_good_proxy)
                .into_iter()
                .take(PROBE_MAX)
                .collect()
        } else {
            vec![typed]
        };
        if cands.is_empty() {
            if let Some(d) = self.settings_dlg.as_mut() {
                d.probe_note = Some("没找到候选代理：系统代理未开启，常见端口也没人应答".into());
            }
            cx.notify();
            return;
        }
        if let Some(d) = self.settings_dlg.as_mut() {
            d.probing = true;
            d.probe_note = Some(format!("正在试 {} 个候选…", cands.len()));
        }
        cx.notify();

        cx.spawn(async move |this, cx| {
            let hit = cx
                .background_executor()
                .spawn(async move { cands.iter().find(|c| proxy::probe(c)).cloned() })
                .await;
            this.update(cx, |this, cx| {
                if let Some(d) = this.settings_dlg.as_mut() {
                    d.probing = false;
                    d.probe_note = Some(match &hit {
                        Some(c) => format!("可用：{c}（填进上面那一格可固定用它）"),
                        None => "这几个候选都没应答。直连能通的话本来也不走代理".into(),
                    });
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// 对话框的只读快照。`content` 闭包里只有 `&mut App`，展示函数收不可变的一份
pub(crate) struct SettingsState {
    /// 左侧当前停在哪个分类
    pub(crate) cat: SetCat,
    /// 左侧哪一颗正被扫着（悬停面）
    pub(crate) nav_hover: Option<SetCat>,
    pub(crate) git_name: Entity<InputState>,
    pub(crate) git_email: Entity<InputState>,
    pub(crate) github_token: Entity<InputState>,
    pub(crate) gitee_token: Entity<InputState>,
    pub(crate) ssh_key: Entity<InputState>,
    pub(crate) proxy: Entity<InputState>,
    pub(crate) notify: bool,
    /// 当前是不是深色（阶段 20）
    pub(crate) theme_dark: bool,
    /// 点关闭键时怎么办（阶段 22）
    pub(crate) close_action: CloseAction,
    pub(crate) probing: bool,
    pub(crate) probe_note: Option<String>,
    /// 勾上 = 显示的模块，两套各一份（阶段 19）
    pub(crate) mod_mine: Vec<String>,
    pub(crate) mod_third: Vec<String>,
    pub(crate) error: Option<String>,
}

impl Shell {
    fn settings_state(&self) -> Option<SettingsState> {
        let d = self.settings_dlg.as_ref()?;
        Some(SettingsState {
            cat: d.cat,
            nav_hover: d.nav_hover,
            git_name: d.git_name.clone(),
            git_email: d.git_email.clone(),
            github_token: d.github_token.clone(),
            gitee_token: d.gitee_token.clone(),
            ssh_key: d.ssh_key.clone(),
            proxy: d.proxy.clone(),
            notify: d.notify,
            theme_dark: d.theme_dark,
            close_action: d.close_action,
            probing: d.probing,
            probe_note: d.probe_note.clone(),
            mod_mine: d.mod_mine.clone(),
            mod_third: d.mod_third.clone(),
            error: d.error.clone(),
        })
    }
}

/// 一段的标题 + 说明。**标题是二级（12px / `body`）不是三级**（阶段 21-8a）。
///
/// 早先它用 `FS_META`（11px）+ `tier3`——那一档的语义是"元信息、
/// 弱提示"，而**段标题是这一段的门面**：它下面挂着这一段全部内容，
/// 却是全对话框里最淡的一行。用户在找「深色主题」那个开关时先扫的就是它。
///
/// 这条属于 21-8a 说的那一类：**大量该当主文字的内容被当成元信息用了**。
/// 升到二级之后「标题 / 说明」的大小差也回到 12 / 11，
/// 而不是原来的 11 / 11（同一档里分主次，只能靠颜色硬分）
fn section(t: &Theme, title: &str, note: &str) -> Div {
    v_flex()
        .w_full()
        .gap(px(GAP_TIGHT))
        .child(
            div()
                .text_size(px(FS_SEC))
                .font_weight(FontWeight::MEDIUM)
                .text_color(body(t))
                .child(title.to_string()),
        )
        .child(
            div()
                .text_size(px(FS_META))
                .text_color(tier4(t))
                .child(note.to_string()),
        )
}

fn field(label: &str, input: &Entity<InputState>, t: &Theme) -> Div {
    v_flex()
        .w_full()
        .gap(px(GAP_TIGHT))
        .child(
            div()
                .text_size(px(FS_META))
                .text_color(tier3(t))
                .child(label.to_string()),
        )
        .child(Input::new(input).w_full().small())
}

fn secret(label: &str, input: &Entity<InputState>, t: &Theme) -> Div {
    v_flex()
        .w_full()
        .gap(px(GAP_TIGHT))
        .child(
            div()
                .text_size(px(FS_META))
                .text_color(tier3(t))
                .child(label.to_string()),
        )
        // 令牌用官方 `mask_toggle`：默认打码 + 一颗眼睛切换。
        // 令牌明文摊在屏幕上等于把它显示给在旁边的人，也会被截图带走
        .child(Input::new(input).w_full().small().mask_toggle())
}

/// 设置对话框正文。**左侧分类导航 + 右侧内容**（阶段 20）
///
/// ## 为什么分栏，而不是继续往下加段
///
/// 平铺那一版有个**先前就有**的缺陷：官方 `Dialog` 确实给正文包了一层
/// `overflow_y_scrollbar()`（`dialog.rs:716-725`），但那是 **`children()`
/// 那条路**——我们走 `content()`，正文落在 `DialogContent` 里，那一层的
/// 高度是**无界的自动高度**，外层只有一句 `max_h`（`dialog.rs:682`）
/// 加 `overflow_hidden`（`:686`）。于是内容一高就被**裁掉**，而滚轮无事可做
/// （框没溢出，容器认为自己是完整的）。加「显示哪些模块」之前就已经这样了：
/// 系统通知那颗开关都只露一半。
///
/// 分栏顺手解决三件事：① 每一页都装得下，裁切自己就没了；
/// ② 设置按「这是什么事」才分得开（身份与令牌是同一件事）；③ 720px 宽
/// 弹窗里平铺六段，输入框一行放不下两个。
///
/// ## 定高是"能滚"的前提
///
/// `.h(...)` 给这一行一个有界高度，右栏 `flex_1 + min_h_0` 才真的被约束住、
/// 内容超出才会在**自己这一层**里滚。上一版给正文加 `.h_full()` 之所以没用：
/// 父级高度无界，`h_full` 量出来就等于内容高度，永远没有"超出"这回事
fn settings_body(
    t: &Theme,
    st: &SettingsState,
    this: &Entity<Shell>,
    gv: ButtonCustomVariant,
    body_h: Pixels,
) -> impl IntoElement {
    h_flex()
        .id("set-body")
        .w_full()
        .h(body_h)
        // ⚠️ D41：`h_flex` 装的是 `items_center()`，两个列容器会被**垂直居中**——
        // 左导航三颗、右内容一屏，高度差一大就整块错开了
        .items_stretch()
        .gap(px(GAP_LG))
        .child(set_nav(t, st, this))
        .child(set_pane(t, st, this, gv))
}

/// 正文那块的定高。夹在 300..480：低于 300 装不下「身份与访问」那一页的
/// 五格输入（实测 410），高于 480 弹窗会顶到官方那道 `max_h`
/// （`dialog.rs:582` 的 `view_size − margin × 2`），于是又多一层我们管不到的裁切
///
/// **定高而不是让它随内容长**：三页内容量差两倍（网络页 250 / 身份页 410），
/// 跟着内容走的话每切一次分类弹窗就长高或缩一截，焦点跟着跳
fn pane_height(window: &Window) -> Pixels {
    (window.viewport_size().height * 0.55).clamp(px(300.0), px(480.0))
}

/// 左侧分类导航。**不是官方 `setting::Settings`**——查过了，三个理由：
///
/// ① 它自带**搜索框**（`settings.rs:151-152`，没有开关能去掉），而这里
///    只有三个分类；② 它是 `h_resizable` + `resizable_panel`（`:416-422`），
///    弹窗里给一条可拖拽的分隔条只是噪音；③ 侧栏自带 `tokens.sidebar` 底色
///    与右边框（`sidebar/mod.rs:422`、`:426`），与本应用的白卡版面不是一套。
///
/// 选中面复用 `tokens::selected_mask`——与导航轨三颗、侧栏筛选**同一个来源**，
/// 全站"我现在在哪"就这一个面
fn set_nav(t: &Theme, st: &SettingsState, this: &Entity<Shell>) -> impl IntoElement {
    v_flex()
        .w(px(SET_NAV_W))
        .flex_shrink_0()
        .gap(px(GAP_DENSE))
        .border_r_1()
        .border_color(track(t))
        .children(SetCat::ALL.iter().map(|c| {
            let active = st.cat == *c;
            let hovered = st.nav_hover == Some(*c);
            let me = this.clone();
            v_flex()
                .id(SharedString::from(format!("set-nav-{}", c.key())))
                .w_full()
                .gap(px(GAP_DENSE))
                .px(px(GAP_TIGHT))
                .py(px(GAP_TIGHT))
                .rounded(px(R_MD))
                .cursor_pointer()
                .when(active, |d| d.bg(selected_mask(t)))
                // 选中面优先：官方那颗的悬停是实心 accent 蓝，比选中面重得多，
                // 两层叠在一起会看成"这块被按住了"
                .when(!active && hovered, |d| d.bg(t.secondary_hover))
                .child(
                    div()
                        .text_size(px(FS_BODY))
                        .text_color(if active { t.colors.foreground } else { tier3(t) })
                        .when(active, |d| d.font_weight(FontWeight::MEDIUM))
                        .child(c.label()),
                )
                .child(
                    div()
                        .text_size(px(FS_META))
                        .text_color(tier4(t))
                        .child(c.hint()),
                )
                .on_click({
                    let me = me.clone();
                    move |_, _, cx| {
                        me.update(cx, |this, cx| this.set_settings_cat(*c, cx));
                    }
                })
                // ⚠️ 悬停只能靠 `on_hover` 记状态：gpui 没有 CSS 那种 `:hover`。
                // 不记的话扫过一整列导航键全程零反馈，用户不知道该点哪一颗
                .on_hover({
                    let me = me.clone();
                    move |h, _, cx| {
                        let (cat, on) = (*c, *h);
                        me.update(cx, |this, cx| {
                            if let Some(d) = this.settings_dlg.as_mut() {
                                d.nav_hover = if on { Some(cat) } else { None };
                            }
                            cx.notify();
                        });
                    }
                })
        }))
}

/// 右侧内容。**只有当前那一页**
fn set_pane(
    t: &Theme,
    st: &SettingsState,
    this: &Entity<Shell>,
    gv: ButtonCustomVariant,
) -> impl IntoElement {
    v_flex()
        .flex_1()
        .min_w_0()
        .min_h_0()
        // ⚠️ 这三行是"内容够得着"的全部原因，见 `settings_body` 的函数头。
        // `overflow_y_scrollbar()`（不是 `overflow_y_scroll`）自带滚动条，
        // 用户看得见"下面还有"，而不是滚不动也看不出为什么
        .id("set-pane")
        .overflow_y_scrollbar()
        .gap(px(GAP_LG))
        .child(match st.cat {
            SetCat::Identity => identity_page(t, st),
            SetCat::Network => network_page(t, st, this, gv),
            SetCat::Display => display_page(t, st, this),
        })
        .when_some(st.error.clone(), |b, e| {
            b.child(
                div()
                    .w_full()
                    .text_size(px(FS_SEC))
                    .text_color(t.colors.danger)
                    .child(e),
            )
        })
}

fn identity_page(t: &Theme, st: &SettingsState) -> Div {
    v_flex()
        .w_full()
        .gap(px(GAP_LG))
        .child(section(
            t,
            "提交身份",
            "仓库里没有全局 user.name / user.email 时用它。只存在本机配置文件。",
        ))
        .child(field("名字", &st.git_name, t))
        .child(field("邮箱", &st.git_email, t))
        .child(section(
            t,
            "访问令牌",
            "私有仓库认证与云端建仓用。认证通过当次 git 子进程注入，不写进任何仓库。",
        ))
        .child(secret("GitHub 令牌", &st.github_token, t))
        .child(secret("Gitee 令牌", &st.gitee_token, t))
        .child(section(
            t,
            "SSH 私钥",
            "远端是 ssh:// 或 git@ 形式时，对该次 git 操作设置 GIT_SSH_COMMAND。",
        ))
        .child(field("私钥路径", &st.ssh_key, t))
}

fn network_page(
    t: &Theme,
    st: &SettingsState,
    this: &Entity<Shell>,
    gv: ButtonCustomVariant,
) -> Div {
    let probe_me = this.clone();
    let notify_me = this.clone();
    v_flex()
        .w_full()
        .gap(px(GAP_LG))
        .child(section(
            t,
            "代理",
            "留空即自动探测：系统代理 → 上次成功 → 常见端口，只对当次 git 子进程生效。",
        ))
        .child(field("手动地址", &st.proxy, t))
        .child(
            h_flex()
                .items_center()
                .gap(px(GAP))
                .child(
                    // ⚠️ **显式 `.with_size(Size::Small)`**（阶段 20）。不写就走
                    // `Size::default()` = `Medium`（32 高 / **14 字**，
                    // `sizing.rs:10-11`），而这一页上下都是 12px 元信息与
                    // 24px 输入框，14px 的字顶在中间特别扎眼
                    Button::new("set-probe")
                        .label("检测代理")
                        .custom(gv)
                        .with_size(Size::Small)
                        .disabled(st.probing)
                        .on_click(move |_, window, cx| {
                            probe_me.update(cx, |this, cx| this.probe_proxy(window, cx));
                        }),
                )
                .when(st.probing, |row| {
                    row.child(
                        h_flex()
                            .items_center()
                            .gap(px(GAP_TIGHT))
                            .child(
                                Spinner::new()
                                    .with_size(Size::XSmall)
                                    .color(tier4(t)),
                            )
                            .child(
                                div()
                                    .text_size(px(FS_META))
                                    .text_color(tier4(t))
                                    .child("正在试"),
                            ),
                    )
                }),
        )
        .when_some(st.probe_note.clone(), |b, n| {
            b.child(
                div()
                    .w_full()
                    .text_size(px(FS_META))
                    .text_color(tier3(t))
                    .child(n),
            )
        })
        .child(section(
            t,
            "系统通知",
            "后台自动同步的结果弹一条系统通知。这个开关即时生效，不受下面的「保存」约束。",
        ))
        .child(
            Switch::new("set-notify")
                .checked(st.notify)
                .label("自动同步结果发系统通知")
                .on_click(move |on, window, cx| {
                    notify_me.update(cx, |this, cx| this.toggle_notify(*on, window, cx));
                }),
        )
}

fn display_page(t: &Theme, st: &SettingsState, this: &Entity<Shell>) -> Div {
    let theme_me = this.clone();
    v_flex()
        .w_full()
        .gap(px(GAP_LG))
        .child(section(
            t,
            "主题",
            "深浅两套配色都写死在 gui/theme.json 里（改配色只动那一个文件，\
             两套都在里面）。这个开关即时生效，不受下面的「保存」约束。",
        ))
        .child(
            Switch::new("set-theme")
                .checked(st.theme_dark)
                .label("深色主题")
                .on_click(move |on, window, cx| {
                    theme_me.update(cx, |this, cx| this.toggle_theme(*on, window, cx));
                }),
        )
        .child(section(
            t,
            "关闭窗口时",
            "点右上角 × 之后做什么。三选一，勾上那颗就是当前档。\n\
             选「每次都问」时，面板里那刻「记住我的选择」打勾就会把这次的选择记下来，\n\
             之后直接照做、不再问；不打勾就每次都问。",
        ))
        .child(close_action_picker(st, this))
        .child(section(
            t,
            "显示哪些模块",
            "前五项是详情页的页签，后四项是概览页里的四段。勾上就显示，不勾就不画；\
             两类仓库各一份，互不影响。至少要留一个，否则那一屏会空掉。\
             这个开关即时生效，不用点下面的「保存」。",
        ))
        .child(mod_grid(t, st, false, this))
        .child(mod_grid(t, st, true, this))
}

/// 关窗那三档的勾选区（阶段 22）。**三颗复选框做单选**——勾上那颗就是当前档。
///
/// ## 为什么用勾选框而不是分段键
///
/// 用户拍板的口径是「打勾选择」。而这一页其余的可选项（主题、模块）用的都是
/// 勾选框，**同一页里混两种选择形态会让"这一条还能不能改"变得看不出来**。
///
/// ## 点已勾上的那颗 = 取消还是保持
///
/// **保持**。三颗都是真选项、没有"不选"这一档；点当前档就什么都不发生。
/// 官方 `Checkbox` 给的是**翻转后**的值，照着它写成"取消勾"会造出
/// "一颗都不勾"的非法状态，而那个状态在 `close_action_of` 里落回
/// `Ask`——**界面上看着没选、实际等于选了 Ask**，是那种最难查的骗。
fn close_action_picker(st: &SettingsState, this: &Entity<Shell>) -> impl IntoElement {
    v_flex()
        .w_full()
        .gap(px(GAP_TIGHT))
        .children([
            (
                CloseAction::Ask,
                store::CLOSE_ASK,
                "每次都问：弹出面板让我选（默认）",
            ),
            (
                CloseAction::Tray,
                store::CLOSE_TRAY,
                "直接关闭到托盘：程序留在后台，定时同步照常",
            ),
            (
                CloseAction::Exit,
                store::CLOSE_EXIT,
                "直接完全退出：后台同步也一起停",
            ),
        ]
        .map(|(kind, raw, label)| {
            let me = this.clone();
            // ⚠️ id 必须带 raw：同一帧三颗都在，gpui 按 `ElementId` 全局存状态，
            // 同名会共用一份勾选状态（D61）
            Checkbox::new(SharedString::from(format!("set-close-{raw}")))
                .checked(st.close_action == kind)
                .label(label)
                .on_click(move |_checked, _window, cx| {
                    me.update(cx, |this, cx| this.set_close_action(raw, cx));
                })
        }))
}

/// 勾选区一行放几颗
const MOD_COLS: usize = 3;

/// 一组模块勾选框（「我的」或「第三方」），**三列网格**（阶段 20）
///
/// 十八颗（两套 × 九项）竖着排要 500px 出头，右栏装不下——平铺那一版就是
/// 这样被裁到点不到的。三列排完是六行。
///
/// **「在哪」不再逐项重复**：三列每列只有 ~150px，那一列塞不下"详情页页签"
/// 这种五个字，挤进去就得缩字或者断行。改成段标题说一次"前五项 / 后四项"、
/// 组标题说一次归属。
///
/// **禁用 + 不勾 + 写明原因**（第三方那两项）是基线项唯一正确的画法：那两项
/// 不是"用户现在不想看"，是"这一类仓库上它不成立"。
/// ⚠️ 这里**不能勾上**：这一页的复选框语义是"勾上 = 显示"，而它们其实没有
/// 显示——勾上等于对用户说谎说"空间页给你看着呢"，而底下那件事照旧不发生。
/// 画成可勾的更糟：用户勾完界面变了、页面还是那个页面，那是骗人
fn mod_grid(t: &Theme, st: &SettingsState, third: bool, this: &Entity<Shell>) -> Div {
    let shown = if third { &st.mod_third } else { &st.mod_mine };
    // 「我的」五个页签 + 概览四段；第三方少两项（基线）
    let mut items: Vec<(&'static str, &'static str)> =
        DetailTab::ALL.iter().map(|t2| (t2.module_key(), t2.label())).collect();
    items.extend([
        (store::MOD_SEC_FORK, "分叉条"),
        (store::MOD_SEC_SYNC, "同步"),
        (store::MOD_SEC_LOG, "日志"),
        (store::MOD_SEC_LANG, "语言技术栈"),
    ]);

    v_flex()
        .w_full()
        .gap(px(GAP_TIGHT))
        .child(
            div()
                .text_size(px(FS_META))
                .font_weight(FontWeight::MEDIUM)
                .text_color(tier3(t))
                .child(if third { "第三方仓库" } else { "我的仓库" }),
        )
        .children(items.chunks(MOD_COLS).map(|chunk| {
            h_flex().w_full().gap(px(GAP)).children(chunk.iter().map(|(key, label)| {
                let (key, label) = (*key, *label);
                let applicable = store::module_applicable(third, key);
                let on = shown.iter().any(|x| x == key);
                let me = this.clone();
                // 复选框 = "勾上就显示"，与 `toggle_module` 正好对上
                // （那边收的是"新值"，官方 `Checkbox` 给的是翻转后的值）
                Checkbox::new(id_for(key, third))
                    .checked(on)
                    .disabled(!applicable)
                    .label(label)
                    .on_click(move |checked, window, cx| {
                        me.update(cx, |this, cx| {
                            this.toggle_module(third, key, *checked, window, cx)
                        });
                    })
            }))
        }))
        .when(third, |d| {
            d.child(
                div()
                    .text_size(px(FS_META))
                    .text_color(tier4(t))
                    .child(
                        "「空间」与「分叉」对第三方仓库不成立：空间页里有一键清理\
                         （会真删磁盘上的构建产物），分叉条改的是本地历史。这两项固定不显示。",
                    ),
            )
        })
}

/// 复选框的 `ElementId`。**必须带上归属**——同一帧里两组会同时出现，
/// 而 gpui 按 `ElementId` 全局存状态，同名会共用一份勾选状态，
/// debug 构建还会直接 panic（`Duplicate a11y node id`，D61）
fn id_for(key: &str, third: bool) -> String {
    format!("set-mod-{}", if third { "t" } else { "m" })[..]
        .to_string()
        + &key.replace('.', "-")
}

fn settings_footer(this: &Entity<Shell>, gv: ButtonCustomVariant) -> Div {
    let cancel = this.clone();
    let save = this.clone();
    h_flex()
        .w_full()
        .justify_end()
        .gap(px(GAP))
        .child(
            Button::new("set-cancel")
                .label("取消")
                .custom(gv)
                .on_click(move |_, window, cx| {
                    cancel.update(cx, |shell, _| shell.settings_dlg = None);
                    window.close_dialog(cx);
                }),
        )
        .child(
            Button::new("set-save")
                .label("保存")
                .primary()
                .on_click(move |_, window, cx| {
                    let vals: [String; 6] = save
                        .read(cx)
                        .settings_dlg
                        .as_ref()
                        .map(|d| {
                            [
                                d.git_name.read(cx).value().to_string(),
                                d.git_email.read(cx).value().to_string(),
                                d.github_token.read(cx).value().to_string(),
                                d.gitee_token.read(cx).value().to_string(),
                                d.ssh_key.read(cx).value().to_string(),
                                d.proxy.read(cx).value().to_string(),
                            ]
                        })
                        .unwrap_or_default();
                    if save.update(cx, |shell, cx| shell.save_settings(vals, window, cx)) {
                        window.close_dialog(cx);
                    }
                }),
        )
}
