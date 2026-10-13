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
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable as _, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, AnyElement, App, AppContext, Context, Div, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Pixels, SharedString, StatefulInteractiveElement, Styled, Window,
};
use qingbird_core::proxy;
use qingbird_core::store::{self, CloseAction};
use std::collections::HashMap;

use crate::detail::DetailTab;
use crate::state::*;
use crate::tokens::*;
use crate::widgets::{
    CardHeader, FormRows, card, ghost_variant, info_dot, meta_text, pill, sunken_box,
};
use gpui_kit::FocusHandle;

use super::{dlg_cancel, footer, shell};

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
            ui_scale: UiScale::from_stored(&s.ui_scale),
            close_action: store::close_action_of(&s.close_action),
            probing: false,
            probe_note: None,
            mod_mine,
            mod_third,
            // 覆盖值直接从配置读，**不是从生效值反推**——反推的话
            // "没改过"与"改成了默认值"就分不开了，而这两者在前台要做的事一样、
            // 在"恢复默认"那一步要做的事不一样
            key_overrides: s.keybindings.clone(),
            recording: None,
            key_focus: cx.focus_handle(),
            key_cap_hover: None,
            key_yielded: Vec::new(),
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
        shell(
            window,
            cx,
            |_| "设置".into(),
            // ⚠️ **448 → 720**（阶段 20）。官方默认 448px（`dialog.rs:219`），
            // 那是给"确认一下"的弹窗的宽度。左导航 + 右内容这套版面
            // 挤在 448 里的话，右边能放的字不到 30 个一行，
            // 输入框与勾选框全得折行。**上限是视口减去两侧留白**，
            // 官方自己会夹（`dialog.rs:580`）
            // ⚠️ **720 → 880**（2026-10-08 用户："左边太大了，右边卡片太小了"）。
            // 病根不是左栏宽，是**总宽只够左栏把右栏挤到 ≈480**——
            // 而 480 正是官方 `setting::Settings` 转竖排的那条线
            // （`settings.rs:425`），右栏一直贴着下限过活。
            // 现在 880 − 左栏 150 − 缝 12 − 内边距 ≈ **700**，
            // 「显示哪些模块」两栏一行一项才铺得开
            Some(880.0),
            {
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
                            c.child(settings_body(
                                &cx.theme().clone(),
                                &st,
                                &this,
                                ghost_variant(cx),
                                h,
                            ))
                        }
                        None => c,
                    }
                }
            },
            move |cx| {
                settings_footer(&cx.theme().clone(), &this, ghost_variant(cx))
            },
        );
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
        crate::theme::apply(
            &cfg.settings.theme,
            UiScale::from_stored(&cfg.settings.ui_scale),
            cx,
        );
        cx.notify();
        true
    }

    /// 换界面字号档的**唯一实现**（落盘 + 两条字号路一起换）。
    ///
    /// 与 `toggle_theme_static` 同一个理由抽出来：**落盘 + 应用只有一份**。
    /// 这里还多一层——字号有**两条路**要一起换：`tokens::set_ui_scale`
    /// 管我们自己的 182 处文字，`theme::apply` 里那行 `font_size` 管官方件。
    /// 只换一条 ⇒ 一屏里两种字号混着（见 `theme.rs` 模块头那段）。
    ///
    /// 返回 `true` = 落盘成功
    pub(crate) fn set_ui_scale_static(
        scale: UiScale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut cfg = store::load();
        cfg.settings.ui_scale = scale.stored().to_string();
        if let Err(e) = store::save(&cfg) {
            window.push_notification(format!("保存失败：{e}"), cx);
            return false;
        }
        crate::theme::apply(&cfg.settings.theme, scale, cx);
        // **只在"用户自己拨了"这一刻记一行**——这才是可行动的信息
        // （用户报「字还是小」时，日志里能证明他拨到了哪一档）。
        // 启动时不记：那只是"当前是第几档"，用户自己能在设置里看见，
        // 而每次启动给 `app.log` 添一行垃圾是阶段 25 记过的代价
        crate::diag::note(&format!("界面字号切到 {}档：{}", scale.label(), scale.note()));
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
    /// 当前界面字号档（阶段 28）
    pub(crate) ui_scale: UiScale,
    /// 点关闭键时怎么办（阶段 22）
    pub(crate) close_action: CloseAction,
    pub(crate) probing: bool,
    pub(crate) probe_note: Option<String>,
    /// 勾上 = 显示的模块，两套各一份（阶段 19）
    pub(crate) mod_mine: Vec<String>,
    pub(crate) mod_third: Vec<String>,
    /// 快捷键覆盖（阶段 30）。展示层只读它，自己不判"生效的是哪一把"
    pub(crate) key_overrides: HashMap<String, String>,
    /// 正在录键的那一项
    pub(crate) recording: Option<String>,
    /// 录制器焦点（阶段 30）。渲染层要 `track_focus` 它
    pub(crate) key_focus: FocusHandle,
    /// 哪一颗键帽正被扫着（阶段 30）
    pub(crate) key_cap_hover: Option<String>,
    /// 本次打开期间"这把键被抢走了"的记录
    pub(crate) key_yielded: Vec<(String, String)>,
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
            ui_scale: d.ui_scale,
            close_action: d.close_action,
            probing: d.probing,
            probe_note: d.probe_note.clone(),
            mod_mine: d.mod_mine.clone(),
            mod_third: d.mod_third.clone(),
            key_overrides: d.key_overrides.clone(),
            recording: d.recording.clone(),
            key_cap_hover: d.key_cap_hover.clone(),
            key_focus: d.key_focus.clone(),
            key_yielded: d.key_yielded.clone(),
            error: d.error.clone(),
        })
    }
}

/// 设置页**一段的外壳**：一张卡 + 卡头（标题 / 附注 / 即时生效 / ⓘ）+ 表单行。
///
/// ## 它治的是"排版混乱"的哪一半
///
/// 阶段 31.5 之前每段是「标题 + 两三行灰字 + 控件」**浮在白色 dialog 上**，
/// 六段挤在一栏里。真正的根因是**没有卡**——段与段之间只靠 12px 间距区分，
/// 读不出边界，所以看着像"一列还没排完的字"。补上卡之后，卡头下面那两三行
/// 说明又成了新负担：一屏四张卡、每张顶着一段小字，边界反被字压住。
/// ⇒ 说明收进 ⓘ（原型 `settings-v4.html` 就是这么画的，方向已验收）。
///
/// ⚠️ **收进气泡的代价要认：ⓘ 不点不看见**。所以只有"不懂也能安全操作"的
/// 说明才收（"留空即自动探测"这类）。**会改变行为的信息不收**——
/// "这条即时生效、不用点保存"留在卡头当一枚**可见**的胶囊，正是这个道理。
/// 判据：**这句话不看会不会导致用户做出他不想做的事**，会就不许进气泡
struct SetCard {
    title: String,
    /// 标题右边的一句附注（不是下面的说明段）——「显示哪些模块」那句
    /// "两类仓库各存一份"是这一档
    note: Option<String>,
    live: bool,
    info: Option<(&'static str, String)>,
}

impl SetCard {
    fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            note: None,
            live: false,
            info: None,
        }
    }

    /// 标题右边的一句附注。收 `impl Into<String>` 而不是 `&str`：
    /// 「代理」那一段挂的是**探测结果**（一个 `String`），为它再开一个
    /// `note_string()` 就是同一件事两个入口
    fn note(mut self, text: impl Into<String>) -> Self {
        self.note = Some(text.into());
        self
    }

    /// 卡头右上那枚「即时生效」。**只有真即时生效的段才许挂**——
    /// 挂错段等于骗用户"不用点保存"，而那一格的值不点保存就真的丢
    fn live(mut self) -> Self {
        self.live = true;
        self
    }

    /// 卡头右上的 ⓘ 气泡。`id` 单独给是因为它是 `ElementId`：
    /// **一帧里两颗同 id 的按钮在 debug 下直接 panic**（D61），
    /// 所以 id 必须逐段不同，不能图省事都写 `"set-info"`
    fn info(mut self, id: &'static str, text: &str) -> Self {
        self.info = Some((id, text.to_string()));
        self
    }

    /// 卡身内容。**收 `impl IntoElement` 而不是收 `FormRows`**：
    /// 「显示哪些模块」那一段是**两组并排的勾选盒**，组内一行一项——
    /// 它不是 `FormRows` 那种"左标签 + 右控件"的结构，为它另开一个
    /// `build_body()` 就是同一件事两个入口（D89 那一族）。
    /// 传 `FormRows::new().row(..).build(t)` 与传一个 `h_flex` 走同一条路
    fn build(self, t: &Theme, gv: ButtonCustomVariant, content: impl IntoElement) -> Div {
        let mut hdr = CardHeader::new(self.title);
        if let Some(n) = self.note {
            hdr = hdr.note(n, false);
        }
        if self.live {
            hdr = hdr.right(live_badge(t));
        }
        if let Some((id, text)) = self.info {
            hdr = hdr.right(info_dot(id, text, t, gv));
        }
        card(t).child(hdr.build(t)).child(content)
    }
}

/// 「即时生效」那一枚。用 `pill` 而不是自己画：它与仓库状态徽标是**同一副形状**
/// （小字 + wash 底 + 全圆），只是语义不同（一个是数据的态，一个是配置的生效方式）。
/// 底色走 `tokens::OK_WASH` = v4 §2 的 `ok-wash`
fn live_badge(t: &Theme) -> AnyElement {
    pill("即时生效", t.colors.success, wash_bg(OK_WASH, t)).into_any_element()
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

/// 正文那块的定高。**0.55 / 上限 480 → 0.72 / 上限 600**（2026-10-08 用户："右边卡片太小了"）。
///
/// 三页内容量差很多（网络页 250 / 身份页 410 / **显示页在「模块」改成一行一项之后 557**），
/// 而 480 那一档是"模块两列"时代量的——列数一改成 1，显示页就装不下了，
/// 最后两项掉到滚动线以下。**卡片太小这件事有两半：宽（弹窗 720→880、左栏 190→140）
/// 和高（就是这一档）**，只改宽等于把内容挤得更长。
///
/// 上限 600 的天花板仍是官方那道 `max_h`（`dialog.rs:582` 的 `view_size − margin × 2`）：
/// 1080 高的窗口给 ≈760，弹窗 = 标题 50 + 正文 + 底部按钮 60 + 内边距 ⇒ 正文 600 放得下。
/// 小窗口那一头由 `×0.72` 自己跟上（600 高的窗口 → 403），**下限 300 不动**：
/// 低于 300 连身份页那五格输入都放不下（实测 410）
///
/// **定高而不是让它随内容长**：跟着内容走的话每切一次分类弹窗就长高或缩一截，
/// 焦点跟着跳
fn pane_height(window: &Window) -> Pixels {
    (window.viewport_size().height * 0.72).clamp(px(300.0), px(600.0))
}

/// 左侧分类导航。**不是官方 `setting::Settings`**——查过了，三个理由：
///
/// ① 它自带**搜索框**（`settings.rs:151-152`，没有开关能去掉），而这里
///    只有三个分类；② 它是 `h_resizable` + `resizable_panel`（`:416-422`），
///    弹窗里给一条可拖拽的分隔条只是噪音；③ 侧栏自带 `tokens.sidebar` 底色
///    与右边框（`sidebar/mod.rs:422`、`:426`），与本应用的白卡版面不是一套。
///
/// 选中面复用 `tokens::wash`——与导航轨三颗、侧栏筛选**同一个来源**，
/// 全站"我现在在哪"就这一个面
fn set_nav(t: &Theme, st: &SettingsState, this: &Entity<Shell>) -> impl IntoElement {
    v_flex()
        .w(px(SET_NAV_W))
        .flex_shrink_0()
        .gap(px(GAP_DENSE))
        .children(SetCat::ALL.iter().map(|c| {
            let active = st.cat == *c;
            let hovered = st.nav_hover == Some(*c);
            let me = this.clone();
            v_flex()
                .id(SharedString::from(format!("set-nav-{}", c.key())))
                .w_full()
                .gap(px(GAP_DENSE))
                .pl(px(GAP))
                .pr(px(GAP_TIGHT))
                .py(px(GAP))
                .rounded(px(R_MD))
                .cursor_pointer()
                // 选中面优先：官方那颗的悬停是实心 accent 蓝，比选中面重得多，
                // 两层叠在一起会看成"这块被按住了"
                .when(active, |d| d.bg(wash(t)))
                .when(!active && hovered, |d| d.bg(hover_face(t)))
                // ⚠️ **左缘那条竖条常挂，只换颜色**（非选中给 `transparent_black`）。
                // 写成 `.when(active, |d| d.border_l_2())` 的话，选中态一出现
                // 整列文字就往右挪 2px——切一次分类全列跳一下。
                // **凡是"条件出现的描边"都要问这一句：它占不占位**
                .border_l_2()
                .border_color(if active {
                    t.colors.accent
                } else {
                    gpui_kit::Hsla::transparent_black()
                })
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(GAP_TIGHT))
                        .child(
                            icon_box(c.icon())
                                .text_color(if active { t.colors.foreground } else { tier3(t) }),
                        )
                        .child(
                            div()
                                .text_size(px(fs_body()))
                                .text_color(if active { t.colors.foreground } else { body(t) })
                                .when(active, |d| d.font_weight(FontWeight::MEDIUM))
                                .child(c.label()),
                        ),
                )
                .child(
                    div()
                        .text_size(px(fs_meta()))
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
        // 版本号钉在左栏**底部**（Q38）。占位这块把那一格推到底：
        // 基座没有 `mt_auto`（gpui-pre 0.3.7 全量搜过），只能用 `flex_1` 的空块顶开。
        // 上面那个占位块**什么都不画**，所以不会引入新的颜色或间距档
        .child(div().flex_1())
        .child(app_version_line(t))
}

/// 界面唯一一处读版本号的地方（Q38 ⇒ 32-13 那份单源的**第一个读者**）。
///
/// 为什么落在设置页左栏底部，而不是别的三格：
/// ① 它必须**常驻**——出问题时要当场报得出自己跑的是哪一版。放在任何一个分类的
///    内容区里，等于要求用户"先想到去那一页找"；左栏四页共用，切哪一类都在。
/// ② footer 那一行左边已经被「哪几格即时生效」那句占了（`settings_footer`），
///    两句语义无关的话并排会互相稀释；
/// ③ 检视抽屉右列是「AI 助理面板 · 开发中」，**硬约束不许塞东西**（路线表 Q 段那条）。
///
/// ⚠️ **`env!("CARGO_PKG_VERSION")` 全仓库只许出现在这一颗函数里**。
/// 32-13 之前版本号手写三处、"只改一两处不会有任何报错"；收成单源之后，
/// 这里是显示侧。第二次出现就是第二个会漂的出处。
/// 单测 `the_version_on_screen_is_the_one_in_the_workspace_toml` 把它钉成运行期断言
fn app_version_line(t: &Theme) -> Div {
    v_flex()
        .gap(px(GAP_DENSE))
        .child(
            div()
                .text_size(px(fs_meta()))
                .text_color(tier4(t))
                .child("青鸟 Git"),
        )
        .child(
            div()
                .text_size(px(fs_meta()))
                .text_color(tier4(t))
                // 那句话本身抽成纯函数：不开窗口也能断言它等于根表那个数
                .child(version_text()),
        )
}

/// 「版本 0.1.0」那一句。**纯函数**，不碰主题也不碰 `Shell`
pub(crate) fn version_text() -> String {
    format!("版本 {}", env!("CARGO_PKG_VERSION"))
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
            SetCat::Identity => identity_page(t, st, gv),
            SetCat::Network => network_page(t, st, this, gv),
            SetCat::Display => display_page(t, st, this, gv),
            SetCat::Keys => crate::dialogs::settings_keys::keys_page(t, st, this, gv),
        })
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

fn identity_page(t: &Theme, st: &SettingsState, gv: ButtonCustomVariant) -> Div {
    v_flex()
        .w_full()
        .gap(px(GAP_LG))
        .child(
            SetCard::new("提交身份")
                .info(
                    "set-info-identity",
                    "仓库里没有全局 user.name / user.email 时用它。\
                     只存在本机配置文件里，不会提交进任何仓库。",
                )
                .build(
                    t,
                    gv,
                    FormRows::new()
                        .row("名字", Input::new(&st.git_name).w_full().small())
                        .row("邮箱", Input::new(&st.git_email).w_full().small())
                        .build(t),
                ),
        )
        .child(
            SetCard::new("访问令牌")
                .info(
                    "set-info-token",
                    "私有仓库认证与云端建仓用。认证通过当次 git 子进程注入\
                     （-c http.extraheader 或 GIT_SSH_COMMAND），\
                     不落盘到仓库里，只存本机配置文件。",
                )
                .build(
                    t,
                    gv,
                    // 令牌用官方 `mask_toggle`：默认打码 + 一颗眼睛切换。
                    // 令牌明文摊在屏幕上等于把它显示给旁边的人，也会被截图带走
                    FormRows::new()
                        .row(
                            "GitHub 令牌",
                            Input::new(&st.github_token).w_full().small().mask_toggle(),
                        )
                        .row(
                            "Gitee 令牌",
                            Input::new(&st.gitee_token).w_full().small().mask_toggle(),
                        )
                        .build(t),
                ),
        )
        .child(
            SetCard::new("SSH 私钥")
                .info(
                    "set-info-ssh",
                    "远端是 ssh:// 或 git@ 形式时，对该次 git 操作设置 GIT_SSH_COMMAND。\
                     这里填的是私钥路径，程序不读内容、不复制、不上传。",
                )
                .build(
                    t,
                    gv,
                    FormRows::new()
                        .row("私钥路径", Input::new(&st.ssh_key).w_full().small())
                        .build(t),
                ),
        )
}

fn network_page(
    t: &Theme,
    st: &SettingsState,
    this: &Entity<Shell>,
    gv: ButtonCustomVariant,
) -> Div {
    let probe_me = this.clone();
    let notify_me = this.clone();
    // 卡头右边那一句 = 上一次的探测结果。**它是"这一段现在的状态"**，正是
    // `CardHeader::note` 那一格的用途；单独占一行会变成一句没头没尾的孤句
    let mut proxy_card = SetCard::new("代理").info(
        "set-info-proxy",
        "留空即自动探测：系统代理 → 上次成功 → 常见端口（7897 / 7890 / 10808 等）。\
         探到的代理只对当次 git 子进程生效——不改系统代理、不影响别的程序。\
         手填那一格如果探不通，程序会照你填的报错，不会悄悄换一个。",
    );
    if let Some(n) = st.probe_note.clone() {
        proxy_card = proxy_card.note(n);
    }
    v_flex()
        .w_full()
        .gap(px(GAP_LG))
        .child(proxy_card.build(
            t,
            gv,
            FormRows::new().row(
                "手动地址",
                // **输入框与那颗键同一行**（原型就是这么画的）。分成两行的话，
                // 「探测」那一行只剩一颗 ghost 键，读起来像另一条独立的设置项，
                // 而它其实是"把上面那一格填的值试一次"
                //
                // ⚠️ **这一组必须自己 `w_full()`**：`FormRows` 的控件列是
                // `justify_end` + 按内容宽，这里不给 `w_full` 的话整组缩成
                // 内容宽，里面那颗 `flex_1` 的输入框就塌成 0 宽——
                // 2026-10-07 抓帧实锤：那一格只剩一个椭圆，placeholder 全看不见
                h_flex()
                    .w_full()
                    .items_center()
                    .gap(px(GAP))
                    .child(Input::new(&st.proxy).flex_1().min_w_0().small())
                    .child(
                        // ⚠️ **显式 `.with_size(Size::Small)`**（阶段 20）。不写就走
                        // `Size::default()` = `Medium`（32 高 / **14 字**，
                        // `sizing.rs:10-11`），而这一行上下都是 24px 的输入框，
                        // 14px 的字顶在中间特别扎眼
                        Button::new("set-probe")
                            .label("检测代理")
                            .custom(gv)
                            .with_size(Size::Small)
                            .disabled(st.probing)
                            .on_click(move |_, window, cx| {
                                probe_me.update(cx, |this, cx| {
                                    this.probe_proxy(window, cx)
                                });
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
                                        .text_size(px(fs_meta()))
                                        .text_color(tier4(t))
                                        .child("正在试"),
                                ),
                        )
                    }),
            )
            .build(t),
        ))
        .child(
            SetCard::new("系统通知")
                .live()
                .info(
                    "set-info-notify",
                    "后台自动同步的结果弹一条系统通知。这个开关即时生效，\
                     不受下面的「保存」约束。",
                )
                .build(
                    t,
                    gv,
                    FormRows::new().row(
                        "自动同步",
                        // 可见的名字在标签那一列（「自动同步」），这颗只留
                        // `accessibility_label`——**去掉它是无障碍回归**：
                        // 读屏原本能念出"自动同步结果发系统通知"，
                        // 靠 `justify_end` 把它推到行末之后，屏幕上不再需要第二个名字
                        Switch::new("set-notify")
                            .checked(st.notify)
                            .accessibility_label("自动同步结果发系统通知")
                            .on_click(move |on, window, cx| {
                                notify_me.update(cx, |this, cx| {
                                    this.toggle_notify(*on, window, cx)
                                });
                            }),
                    )
                    .build(t),
                ),
        )
}

fn display_page(t: &Theme, st: &SettingsState, this: &Entity<Shell>, gv: ButtonCustomVariant) -> Div {
    let theme_me = this.clone();
    v_flex()
        .w_full()
        .gap(px(GAP_LG))
        .child(
            SetCard::new("外观")
                .live()
                .info(
                    "set-info-appearance",
                    "两套配色都写在 gui/theme.json 里，改配色只动那一个文件。\
                     字号三档是整屏一起换（含标题栏、卡片、表格与官方按钮、输入框）。\
                     关窗那一档：选「每次都问」时，面板里那刻「记住我的选择」打勾就会把\
                     这次的选择记下来，之后直接照做、不再问；不打勾就每次都问。\
                     这三条都即时生效，不用点下面的「保存」。",
                )
                .build(
                    t,
                    gv,
                    FormRows::new()
                        .row(
                            "深色主题",
                            // 可见的名字在标签那一列，所以这颗不再自带文字；
                            // 但 `accessibility_label` 必须给——读屏只念得着 label
                            Switch::new("set-theme")
                                .checked(st.theme_dark)
                                .accessibility_label("深色主题")
                                .on_click(move |on, window, cx| {
                                    theme_me.update(cx, |this, cx| {
                                        this.toggle_theme(*on, window, cx)
                                    });
                                }),
                        )
                        .row("界面字号", ui_scale_picker(st, this))
                        .row("关闭窗口时", close_action_picker(st, this))
                        .build(t),
                ),
        )
        .child(
            SetCard::new("显示哪些模块")
                .note("两类仓库各存一份")
                .live()
                .info(
                    "set-info-modules",
                    "前五项是详情页的页签，后四项是概览页里的四段。勾上就显示，不勾就不画；\
                     两类仓库各存一份，互不影响。至少要留一个，否则那一屏会空掉。\
                     虚线那两项固定不显示：一键清理会真删磁盘上的构建产物，\
                     分叉条改的是本地历史——它们是安全边界，不是偏好。",
                )
                .build(
                    t,
                    gv,
                    // **两组并排，不是一上一下**：这两组是**同一件事的两份**
                    // （同一批模块、两种仓库），并排才读得出"它们在对照"；
                    // 上下排会读成两个独立段落。
                    // ⚠️ 组**内部**是一行一项（`MOD_COLS`，2026-10-08 用户定）——
                    // "并排"说的是两组之间这一层，别拿它去压组内的列数
                    v_flex()
                        .w_full()
                        .gap(px(GAP_LG))
                        .child(
                            h_flex()
                                .w_full()
                                // ⚠️ D41：`h_flex` 默认 `items_center`。两组行数不同
                                // （第三方天生少两项），居中会让它们**顶边错开**，
                                // 两栏的"概览"就不在同一行上。并排的东西一律 start
                                .items_start()
                                .gap(px(GAP_LG))
                                .child(mod_grid(t, st, false, this))
                                .child(mod_grid(t, st, true, this)),
                        )
                        // 那句"为什么有两项是灰的"从第三方盒子里挪到卡底：
                        // 它解释的是**两栏共同**的那条规则，挂在其中一栏下面
                        // 会让人以为只适用于那一栏
                        .child(meta_text(
                            "虚线那两项固定不显示：一键清理会真删磁盘上的构建产物，\
                             分叉条改的是本地历史。",
                            t,
                        )),
                ),
        )
}

/// 界面字号那三档（阶段 28，阶段 31.5 换成官方 `RadioGroup`）。
///
/// ## 为什么是 `RadioGroup` 而不是三颗 `Checkbox`
///
/// 三颗复选框做单选**本来就允许全不勾、也允许全勾**，于是每次都要在外面补一段
/// "点已勾上的那颗 = 保持"的兜底（旧版注释里那段话就是在解释这个兜底）。
/// `RadioGroup` 的勾选态**完全由 `selected_index` 算出来**（`radio.rs:403`
/// 的 `checked = selected_ix == Some(ix)`），点谁回调给谁的下标、不翻转 ⇒
/// **"一颗都没选"这个状态在这条路上压根构造不出来**，兜底那段代码一起消失。
///
/// ## 为什么不用 `ToggleGroup::segmented()`（外观更像原型）
///
/// 查过：`segmented()` **只改外观**（去掉 gap、把相邻边框接成一条），
/// 语义上它还是"多项各自可勾"——`toggle.rs:389` 那句 `next[ix] = !next[ix]`
/// 就是逐项翻转，**照样能勾出零选和多选**。那正是 D88 要消灭的东西。
/// ⇒ 外观与原型有意不等（第四处，登记在 D88 与 v4 §11）
fn ui_scale_picker(st: &SettingsState, this: &Entity<Shell>) -> impl IntoElement {
    let cur = st.ui_scale;
    let ix = UiScale::ALL.iter().position(|s| *s == cur);
    let me = this.clone();
    h_flex()
        // **不要 `w_full`**：这一组要靠右贴到行末（`FormRows` 的控件列已经
        // `justify_end`）。写 `w_full` 会把整列占满，单选组就又回到紧贴标签列、
        // 右边一大片空——正是 2026-10-07 抓帧抓出来的那个毛病
        .items_center()
        .gap(px(GAP_LG))
        .child(
            RadioGroup::horizontal("set-scale")
                .selected_index(ix)
                .children(UiScale::ALL.map(|s| {
                    Radio::new(SharedString::from(s.stored()))
                        .label(s.label())
                        .tooltip(s.note())
                }))
                .on_click(move |ix, window, cx| {
                    let Some(s) = UiScale::ALL.get(*ix).copied() else {
                        return;
                    };
                    me.update(cx, |_this, cx| Shell::set_ui_scale_static(s, window, cx));
                }),
        )
    // ⚠️ **这一行不再挂"正文 18px · 弱提示 14px"那一句**（阶段 31.5 实测）。
    // 原来挂在单选组右边，而**大档下这一组本身就更宽**（字 18px），
    // 两者加起来超过控件列 ⇒ RadioGroup 直接折行，「大」掉到第二行、
    // 右边空一片——那正是这一轮要治的"没排完"。
    // 每档的实际像素值现在只在**悬停 tooltip** 里（`s.note()`），
    // 判据：它是"选了会变成多大"，**不看也能安全操作** ⇒ 属于可收进气泡那一类
}

/// 关窗那三档（阶段 22，阶段 31.5 换成官方 `RadioGroup`，理由见
/// `ui_scale_picker` 的函数头——**两处是同一个判据，改一处要回头改另一处**）
fn close_action_picker(st: &SettingsState, this: &Entity<Shell>) -> impl IntoElement {
    const OPTS: [(CloseAction, &str, &str, &str); 3] = [
        (
            CloseAction::Ask,
            store::CLOSE_ASK,
            "每次都问",
            "弹出面板让我选，这是默认档",
        ),
        (
            CloseAction::Tray,
            store::CLOSE_TRAY,
            "收到托盘",
            "程序留在后台，定时同步照常跑",
        ),
        (
            CloseAction::Exit,
            store::CLOSE_EXIT,
            "直接退出",
            "后台定时同步也一起停",
        ),
    ];
    let ix = OPTS.iter().position(|(k, ..)| st.close_action == *k);
    let me = this.clone();
    RadioGroup::horizontal("set-close")
        .selected_index(ix)
        .children(OPTS.map(|(_, raw, label, tip)| {
            // ⚠️ `RadioGroup` 会把每一项的 id 覆写成下标（`radio.rs:405`），
            // 所以这里传的 `raw` 只是给人看的。**但两组的 group id 必须不同**
            // （`set-scale` / `set-close`）：子项 id 是"有 id 的祖先链"算出来的，
            // group id 撞了就是同帧撞车（D61）
            Radio::new(SharedString::from(raw))
                .label(label)
                .tooltip(tip)
        }))
        .on_click(move |ix, _window, cx| {
            let Some((_, raw, _, _)) = OPTS.get(*ix) else {
                return;
            };
            let raw = raw.to_string();
            me.update(cx, |this, cx| this.set_close_action(&raw, cx));
        })
}

/// 勾选区一行放几颗。**一行一项**（2026-10-08 用户："把显示哪些模块按照一行一行的排版"）。
///
/// 这一档**从来不是审美，是宽度的函数**：三列 → 两列（阶段 31.5，因为并排两盒之后
/// 每栏只剩 ~200px，"语言技术栈"放不下）→ 一列（弹窗 720 → 880、左栏 190 → 150，
/// 每栏现在 ~340px，一行一项还有余量）。
/// ⇒ **以后动这一档要先动的是宽度**：`SET_NAV_W` 与弹窗 `.w()` 在那头，
/// 只把这里改回 2 的话，两列并排会重新挤成折行
const MOD_COLS: usize = 1;

/// 这一页能勾的那些模块：`(配置里的键, 界面上的名字)`。
///
/// 抽成函数是因为**计数与勾选框读的是同一份表**：两份的话"7/9"那个分母
/// 和实际画出来的格子数会漂，而漂了没有任何东西会报错
fn mod_items() -> Vec<(&'static str, &'static str)> {
    let mut items: Vec<(&'static str, &'static str)> = DetailTab::ALL
        .iter()
        .map(|t2| (t2.module_key(), t2.label()))
        .collect();
    items.extend([
        (store::MOD_SEC_FORK, "分叉条"),
        (store::MOD_SEC_SYNC, "同步"),
        (store::MOD_SEC_LOG, "日志"),
        (store::MOD_SEC_LANG, "语言技术栈"),
    ]);
    items
}

/// 一组模块的 `(已勾几项, 适用几项)`。**纯函数**，单测只碰它不碰窗口。
///
/// 分母走 `store::module_applicable` 而不是 `items.len()`：第三方天生没有
/// 「空间」与分叉条，那两项**永远勾不上**。写成 `4/9` 会让人一直去找
/// 那两个不存在的东西——**分母算错比不显示更糟**
fn mod_counts(third: bool, items: &[(&'static str, &'static str)], shown: &[String]) -> (usize, usize) {
    let applicable = items.iter().filter(|(k, _)| store::module_applicable(third, k));
    let total = applicable.clone().count();
    let on = applicable.filter(|(k, _)| shown.iter().any(|x| x == *k)).count();
    (on, total)
}

/// 一组模块勾选框（「我的」或「第三方」），**一个盒子 + 两列**（阶段 31.5）
///
/// ## 为什么套盒子
///
/// 阶段 20 它是"组标题 + 三列勾选框"直接浮在卡上。两栏并排之后，两组之间
/// 要有"这是同一件事的两份"的边界感，靠标题那一行小字撑不住 ⇒ 各套一个
/// `muted` 底 + `line-soft` 描边的盒（v4 §2 的 `card-sunken`：
/// **它是"卡在卡里"，比卡低一层，不是另一张卡**——用 `card()` 会读成两张平级卡）
///
/// ## 计数那一格的分母不含基线锁死项
///
/// 写成 `7/9` 会让人以为"还有 2 个没开"，而那 2 个**永远勾不上**
/// （第三方天生没有「空间」与分叉条）。分母只数**适用的**项 ⇒ 第三方那栏是 `4/7`。
/// **分母算错比不显示更糟**：它会让人一直去找那两个不存在的东西
///
/// **禁用 + 不勾 + 写明原因**（第三方那两项）是基线项唯一正确的画法：那两项
/// 不是"用户现在不想看"，是"这一类仓库上它不成立"。
/// ⚠️ 这里**不能勾上**：这一页的复选框语义是"勾上 = 显示"，而它们其实没有
/// 显示——勾上等于对用户说谎说"空间页给你看着呢"，而底下那件事照旧不发生。
/// 画成可勾的更糟：用户勾完界面变了、页面还是那个页面，那是骗人
fn mod_grid(t: &Theme, st: &SettingsState, third: bool, this: &Entity<Shell>) -> Div {
    let shown = if third { &st.mod_third } else { &st.mod_mine };
    let items = mod_items();
    let applicable = |k: &str| store::module_applicable(third, k);
    let (on, total) = mod_counts(third, &items, shown);

    // 模块开关那一组是**只读的一栏**（里面没有可点的行，开关在栏的每一行里）
    // ⇒ 走 `sunken_box`：`card-sunken` 底 + `line_soft` 边 + `r_lg`，
    // 与提交弹窗那份变更清单同一个出处
    sunken_box(
        v_flex()
            .flex_1()
            // 少了 `min_w_0`，栏里任意一行字变长就把整栏顶宽，另一栏被挤到可视区外
            // （与 Q17 那个挤压是同一族：**并排的列一定要 `flex_1 + min_w_0` 成对写**）
            .min_w_0()
            .gap(px(GAP_TIGHT))
            .p(px(GAP)),
        t,
    )
    .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(fs_sec()))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(body(t))
                        .child(if third { "第三方仓库" } else { "我的仓库" }),
                )
                .child(
                    div()
                        .text_size(px(fs_meta()))
                        .text_color(tier4(t))
                        .child(format!("{on}/{total}")),
                ),
        )
        .children(items.chunks(MOD_COLS).map(|chunk| {
            h_flex().w_full().gap(px(GAP)).children(chunk.iter().map(|(key, label)| {
                let (key, label) = (*key, *label);
                // 内层不再叫 `on`：外层那一格 `on` 是"已勾几项"的计数，
                // 同名遮蔽之后这一行读起来像在用计数判勾选
                let checked = shown.iter().any(|x| x == key);
                let me = this.clone();
                // 复选框 = "勾上就显示"，与 `toggle_module` 正好对上
                // （那边收的是"新值"，官方 `Checkbox` 给的是翻转后的值）
                Checkbox::new(id_for(key, third))
                    .checked(checked)
                    .disabled(!applicable(key))
                    .label(label)
                    .on_click(move |checked, window, cx| {
                        me.update(cx, |this, cx| {
                            this.toggle_module(third, key, *checked, window, cx)
                        });
                    })
            }))
        }))
}

/// 复选框的 `ElementId`。**必须带上归属**——同一帧里两组会同时出现，
/// 而 gpui 按 `ElementId` 全局存状态，同名会共用一份勾选状态，
/// debug 构建还会直接 panic（`Duplicate a11y node id`，D61）
fn id_for(key: &str, third: bool) -> String {
    format!("set-mod-{}", if third { "t" } else { "m" })[..]
        .to_string()
        + &key.replace('.', "-")
}

fn settings_footer(t: &Theme, this: &Entity<Shell>, gv: ButtonCustomVariant) -> Div {
    let save = this.clone();
    // 左下角那一句是**这一页最重要的一条规则**：哪几格即时生效、哪几格要点保存。
    // 它原来不在——于是用户拨了主题开关又点「取消」，就会怀疑那次拨动没算数。
    // ⚠️ 这句必须**跟着实际的即时项改**：将来若把某项挂回「保存」，
    // 这句就成了当面的假话（与 D78 那条"通知里那句恒为真"同族）
    let left = h_flex()
        .items_center()
        .gap(px(GAP_TIGHT))
        .min_w_0()
        .child(icon_sm(gpui_kit::assets::IconName::Info).text_color(tier4(t)))
        .child(meta_text(
            "外观、通知与模块勾选是即时的，「保存」只管上面那几格文字",
            t,
        ))
        .into_any_element();
    footer(
        Some(left),
        [
            dlg_cancel(
                "set-cancel",
                "取消",
                this,
                |shell, _| shell.settings_dlg = None,
                gv,
            ),
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
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 界面那一格显示的版本，必须等于仓库根 `Cargo.toml` 的 `[workspace.package].version`。
    ///
    /// 32-13 把版本号收成单源，但"单源"这件事以前**只靠人记得住**（登记里写的是
    /// 「发版时只改这一行」）。这条把它钉成运行期断言：
    /// 变异验证把 `gui/Cargo.toml` 的 `version.workspace = true` 改成写死一个别的数
    /// ⇒ 精确红在这一条，而那正是"包里的版本与界面读出来的不是同一个数"的形态
    #[test]
    fn the_version_on_screen_is_the_one_in_the_workspace_toml() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml");
        let txt = std::fs::read_to_string(&root).expect("读不到仓库根的 Cargo.toml");
        let ws = txt
            .lines()
            .skip_while(|l| l.trim() != "[workspace.package]")
            .find_map(|l| {
                let l = l.trim();
                l.strip_prefix("version").and_then(|r| {
                    r.trim_start()
                        .strip_prefix('=')
                        .map(|v| v.trim().trim_matches('"').to_string())
                })
            })
            .expect("根 Cargo.toml 的 [workspace.package] 里没有 version 那一行");
        assert_eq!(version_text(), format!("版本 {ws}"));
        // 顺带挡住最坏那一档：`env!` 没读到、整格退成占位值
        assert_ne!(ws, "0.0.0", "版本号退成了占位值 ⇒ 这条线根本没接上");
    }

    /// 计数那一格的**分母**必须只数"勾得上"的项。
    ///
    /// 第三方天生没有「空间」与分叉条（core 的基线锁死），全勾时如果显示
    /// `7/9`，用户就会一直去找那两个不存在的东西——**分母算错比不显示更糟**。
    /// 这条断言挑的是最容易写错的那一面：**勾满**
    #[test]
    fn a_fully_ticked_third_party_group_reads_n_out_of_n_not_n_out_of_nine() {
        let items = mod_items();
        let all: Vec<String> = items
            .iter()
            .filter(|(k, _)| store::module_applicable(true, k))
            .map(|(k, _)| k.to_string())
            .collect();
        let (on, total) = mod_counts(true, &items, &all);
        assert_eq!(on, total, "勾满了却不等于分母，说明分母数进了勾不上的项");
        assert!(
            total < items.len(),
            "第三方那一栏的分母没扣掉基线锁死的两项：{total} / {}",
            items.len()
        );
    }

    /// 「我的」那一栏九项全适用 ⇒ 分母就是全表长度。
    /// 这条与上一条**成对**：只留一条的话，把 `module_applicable` 写反了
    /// 也照样有一条能过
    #[test]
    fn the_mine_group_counts_every_module_because_none_is_locked() {
        let items = mod_items();
        let none: Vec<String> = Vec::new();
        let (on, total) = mod_counts(false, &items, &none);
        assert_eq!((on, total), (0, items.len()));
    }

    /// 三档字号写进配置的字符串必须互不相同。
    ///
    /// 它们同时是 `Radio` 的 id 与落盘值：重复了不会编译错、不会 panic，
    /// 只会"选了大档、重启回来变中档"——`from_stored` 认到第一个匹配就返回
    #[test]
    fn the_three_font_scales_store_three_different_strings() {
        let mut keys: Vec<&str> = UiScale::ALL.iter().map(|s| s.stored()).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), UiScale::ALL.len(), "字号档的 stored() 有重复");
    }

    /// 左侧分类导航的说明句必须一行放得下——`SET_NAV_W` 的注释②早就写了这条
    /// 要求，而阶段 31.5 之前的 `hint()` 是一句 18 字长句，实测折成三行。
    /// **目标写在了注释里、内容没跟上**，就是这种没人管的漂移。
    ///
    /// ⚠️ **预算从列宽算出来，不抄字数**（D78 那条判据：让"忘"没有选项）。
    /// 早先这里写死 `<= 14`，而那个 14 是"190px 那一列"推出来的——2026-10-08
    /// 列宽收到 140 之后，14 这条断言就变成**绿着放不下**：断言还在、
    /// 尺子已经不是那一列了（第一版改成 `<= 11` 时它立刻把「名字 · 令牌 · SSH」
    /// 判红了，而那句在帧上实测只有 96px，是**误报**）。
    ///
    /// ⇒ 尺子按**字符类型加权**算宽：汉字按 `fs_meta` 中档 12px，
    /// 拉丁 / 空格 / 「·」按半宽 6px（帧上那句 13 个字符量出来 96px，
    /// 这个口径算出 114px，**偏保守但不误报**）。
    /// 预算 = 列宽 − 缩进 14（`pl 8` + `pr 4` + `border_l 2`，见 `set_nav`）。
    /// ⚠️ **大字号档（`fs_meta` 14px）下预算只剩 90**，这一列在大档会折行——
    /// 已知取舍：切到大档时整屏都在变紧，不为这一列单独开一档宽度
    #[test]
    fn each_category_hint_fits_on_one_line_of_the_nav_column() {
        const NAV_INDENT: f32 = 14.0;
        const CJK_PX: f32 = 12.0;
        let hint_px = |s: &str| -> f32 {
            s.chars()
                .map(|c| {
                    if ('一'..='鿿').contains(&c) {
                        CJK_PX
                    } else {
                        CJK_PX / 2.0
                    }
                })
                .sum()
        };
        let budget = crate::tokens::SET_NAV_W - NAV_INDENT;
        for c in SetCat::ALL {
            let w = hint_px(c.hint());
            assert!(
                w <= budget,
                "{} 的说明句估宽 {w:.0}px，导航那一列只有 {:.0}px：{}",
                c.label(),
                budget,
                c.hint()
            );
        }
    }
}
