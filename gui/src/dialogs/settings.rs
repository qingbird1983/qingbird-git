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
//! ## 主题这一项为什么不给开关
//!
//! 这一版界面只有**一套**配色，写死在 `gui/theme.json` 里（改配色只动那一个
//! 文件是设计上的硬约定）。给一个"深色"开关却拿不出配套的深色 token，
//! 只会切到基座自带的默认深色——那既不是本应用的配色，也与设计 4.1
//! "不要深色面板"冲突。所以这里只把当前主题显示出来，写明出处；
//! `Settings.theme` 字段保留给旧界面，两套共用同一份 config.json。

use gpui_kit::component::button::ButtonCustomVariant;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable as _, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, AppContext, Context, Div, Entity, FontWeight, ParentElement, Styled, Window,
};
use qingbird_core::proxy;
use qingbird_core::store;

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

        self.settings_dlg = Some(SettingsDialog {
            git_name,
            git_email,
            github_token,
            gitee_token,
            ssh_key,
            proxy: proxy_input,
            notify: s.notify_auto_sync,
            probing: false,
            probe_note: None,
            error: None,
        });
        self.build_settings_dlg(window, cx);
    }

    fn build_settings_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let t = cx.theme().clone();
            dialog
                .title("设置")
                .content({
                    let t = t.clone();
                    let this = this.clone();
                    move |c: DialogContent, _w, cx: &mut App| {
                        // 对话框被 Esc 关掉时 `settings_dlg` 已经清空，
                        // 这一帧就什么都不画——不能拿一份编出来的空快照顶上，
                        // 那会让输入框在一帧之内全部消失
                        match this.read(cx).settings_state() {
                            Some(st) => c.child(settings_body(&t, &st, &this, ghost_variant(cx))),
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
    pub(crate) git_name: Entity<InputState>,
    pub(crate) git_email: Entity<InputState>,
    pub(crate) github_token: Entity<InputState>,
    pub(crate) gitee_token: Entity<InputState>,
    pub(crate) ssh_key: Entity<InputState>,
    pub(crate) proxy: Entity<InputState>,
    pub(crate) notify: bool,
    pub(crate) probing: bool,
    pub(crate) probe_note: Option<String>,
    pub(crate) error: Option<String>,
}

impl Shell {
    fn settings_state(&self) -> Option<SettingsState> {
        let d = self.settings_dlg.as_ref()?;
        Some(SettingsState {
            git_name: d.git_name.clone(),
            git_email: d.git_email.clone(),
            github_token: d.github_token.clone(),
            gitee_token: d.gitee_token.clone(),
            ssh_key: d.ssh_key.clone(),
            proxy: d.proxy.clone(),
            notify: d.notify,
            probing: d.probing,
            probe_note: d.probe_note.clone(),
            error: d.error.clone(),
        })
    }
}

fn section(t: &Theme, title: &str, note: &str) -> Div {
    v_flex()
        .w_full()
        .gap(px(GAP_TIGHT))
        .child(
            div()
                .text_size(px(FS_META))
                .font_weight(FontWeight::MEDIUM)
                .text_color(tier3(t))
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

fn settings_body(t: &Theme, st: &SettingsState, this: &Entity<Shell>, gv: ButtonCustomVariant) -> Div {
    let notify_me = this.clone();
    let probe_me = this.clone();

    v_flex()
        .w_full()
        .gap(px(GAP_LG))
        // 主题：只读一行。理由见模块头
        .child(div().w_full().child(
            // `DescriptionList` 没有 `Styled`，要 `w_full` 得外套一层
            div().w_full().child(
                DescriptionList::new()
                    .item("主题", "浅色 · 灰白", 1)
                    .item("配色出处", "gui/theme.json", 1),
            ),
        ))
        .child(
            v_flex()
                .w_full()
                .gap(px(GAP))
                .child(section(
                    t,
                    "提交身份",
                    "仓库里没有全局 user.name / user.email 时用它。只存在本机配置文件。",
                ))
                .child(field("名字", &st.git_name, t))
                .child(field("邮箱", &st.git_email, t)),
        )
        .child(
            v_flex()
                .w_full()
                .gap(px(GAP))
                .child(section(
                    t,
                    "访问令牌",
                    "私有仓库认证与云端建仓用。认证通过当次 git 子进程注入，不写进任何仓库。",
                ))
                .child(secret("GitHub 令牌", &st.github_token, t))
                .child(secret("Gitee 令牌", &st.gitee_token, t)),
        )
        .child(
            v_flex()
                .w_full()
                .gap(px(GAP))
                .child(section(
                    t,
                    "SSH 私钥",
                    "远端是 ssh:// 或 git@ 形式时，对该次 git 操作设置 GIT_SSH_COMMAND。",
                ))
                .child(field("私钥路径", &st.ssh_key, t)),
        )
        .child(
            v_flex()
                .w_full()
                .gap(px(GAP))
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
                            Button::new("set-probe")
                                .label("检测代理")
                                .custom(gv)
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
                }),
        )
        .child(
            v_flex()
                .w_full()
                .gap(px(GAP_TIGHT))
                .child(section(
                    t,
                    "系统通知",
                    "后台自动同步的结果弹一条系统通知。**这个开关即时生效**，不受下面的「保存」约束。",
                ))
                .child(
                    Switch::new("set-notify")
                        .checked(st.notify)
                        .label("自动同步结果发系统通知")
                        .on_click(move |on, window, cx| {
                            notify_me.update(cx, |this, cx| this.toggle_notify(*on, window, cx));
                        }),
                ),
        )
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
