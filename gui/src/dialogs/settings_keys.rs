//! 设置 · 「快捷键」一页（阶段 30）。
//!
//! ## 为什么录制要用「遮罩」这一层
//!
//! `window.rs:5943-5957` 里 **Action 派发先于**元素的 `on_key_down`
//! （元素回调排在 `:5959` 的 `finish_dispatch_key_event`）。所以用户点一下键帽
//! 开始录、正好按了 `Ctrl+B`，**侧栏会先折一下**才把键记下来；按 `Esc` 更糟，
//! 会把设置弹窗关掉。`keys::shadows` 在进入录制态时把当前生效的键全盖成
//! `NoAction`，就没有"先发生了"这一说。
//!
//! ## 为什么改完键要重挂一遍全部绑定
//!
//! 基座 `Keymap` **没有公开的删除入口**，遮罩是只进不出的。所有"恢复"的动作
//! （录完键、清除、单项恢复、一键恢复）都靠**再追加一遍全部生效绑定**把它们
//! 盖回去——`keymap.rs:158` 明写后加的优先级更高。这也让"恢复默认"不需要
//! 任何一行"撤销上一次修改"的代码。
//!
//! ⚠️ **所有落盘都走 [`Shell::save_key_overrides`] 一处**。改完配置忘了重挂绑定，
//! 表现是"设置里写着新键、实际按的还是老的"，而**重启一次就好了**——
//! 这类缺陷抓帧完全正常、编译器不报、单测全绿。
//!
//! ## 回调签名
//!
//! `Entity::update` 的闭包只拿 `(&mut Shell, &mut Context<Shell>)`，**`&mut Window`
//! 必须从 `on_click` 那一层捕获进来**（`settings.rs` 的 `toggle_module` 同款）。
//! 别写成三参闭包——那是把 `Context` 当成 `update_window` 了。

use std::collections::HashMap;

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::{Disableable as _, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, Context, Div, Entity, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
};
use qingbird_core::store;

use crate::dialogs::settings::SettingsState;
use crate::keys::{self, KeyDef};
use crate::state::*;
use crate::tokens::*;
use crate::widgets::{card, CardHeader, info_dot};

impl Shell {
    /// 全部落盘 + 重挂绑定的那一处
    fn save_key_overrides(
        &mut self,
        next: HashMap<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut cfg = store::load();
        cfg.settings.keybindings = next.clone();
        if let Err(e) = store::save(&cfg) {
            window.push_notification(format!("保存快捷键失败：{e}"), cx);
            return;
        }
        // 写回对话框状态：这一页每帧都读它，不写回就是"存住了但屏幕上没变"
        if let Some(d) = self.settings_dlg.as_mut() {
            d.key_overrides = next.clone();
        }
        // ⚠️ **落盘之后必须重挂**。少这一行的话界面显示新键、实际旧键还在，
        // 而**重启一次就好了** —— 典型的"只在本次运行里错"型缺陷
        cx.bind_keys(keys::bindings(&next));
        window.push_notification("快捷键已更新", cx);
        cx.notify();
    }

    /// 只重挂绑定，不落盘。录制被取消 / 录入失败时用
    pub(crate) fn rebind_keys(&mut self, cx: &mut Context<Self>) {
        let m = self
            .settings_dlg
            .as_ref()
            .map(|d| d.key_overrides.clone())
            .unwrap_or_default();
        cx.bind_keys(keys::bindings(&m));
        cx.notify();
    }

    /// 点一下键帽：开始录这一项。再点一次 = 取消
    ///
    /// ⚠️ **遮罩要在这一帧就挂上**，不能等用户按下第一个键——那正是
    /// "按 Ctrl+B 先把侧栏折了"发生的地方
    pub(crate) fn start_record_key(
        &mut self,
        id: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(d) = self.settings_dlg.as_ref() else {
            return;
        };
        let already = d.recording.as_deref() == Some(id);
        let cur = d.key_overrides.clone();
        let focus = d.key_focus.clone();
        if already {
            if let Some(d) = self.settings_dlg.as_mut() {
                d.recording = None;
            }
            // 取消也得重挂：进录制时那层遮罩是只进不出的
            cx.bind_keys(keys::bindings(&cur));
            cx.notify();
            return;
        }
        let shadows = keys::shadows(&cur);
        if let Some(d) = self.settings_dlg.as_mut() {
            d.recording = Some(id.to_string());
        }
        cx.bind_keys(shadows);
        // 焦点必须真的落在这个元素上，否则元素级 `on_key_down` 收不到按键
        focus.focus(window, cx);
        cx.notify();
    }

    /// 录到了。`raw` 是 `Keystroke::unparse` 给的规范串
    pub(crate) fn apply_recorded_key(
        &mut self,
        raw: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(canon) = keys::canonical(raw) else {
            // `KeyBinding::new` 解析不了就 panic，用户输入必须在进它之前被筛掉
            if let Some(d) = self.settings_dlg.as_mut() {
                d.recording = None;
                d.error = Some("这个键没法用，请换一个".into());
            }
            self.rebind_keys(cx);
            return;
        };
        let Some(d) = self.settings_dlg.as_ref() else {
            return;
        };
        let Some(target) = d.recording.clone() else {
            return;
        };
        // 挡住自己：同一项按它自己那把键不是冲突
        let stolen = keys::owner_of(&canon, &d.key_overrides, &target).map(|d| d.id);
        let mut next = d.key_overrides.clone();
        next.insert(target.clone(), canon.clone());
        // ⚠️ **抢键是用户拍板的「自动替换」**：原主人就地解除，
        // 而不是悄悄换一把别的键。原主人那一行会显示「未绑定」，
        // 用户随时能自己再录一把
        if let Some(from) = stolen {
            next.insert(from.to_string(), String::new());
        }
        if let Some(d) = self.settings_dlg.as_mut() {
            d.recording = None;
            // 这一项自己改过了，上一轮的"已让给"对它不再成立
            d.key_yielded.retain(|(from, _)| *from != target);
            if let Some(from) = stolen {
                d.key_yielded.push((from.to_string(), target.clone()));
            }
        }
        if let Some(from) = stolen {
            if let Some(def) = keys::def_of(from) {
                window.push_notification(
                    format!(
                        "「{}」改用 {} · 原主人「{}」变成未绑定",
                        display_of(&target),
                        display_key(&canon),
                        def.title
                    ),
                    cx,
                );
            }
        }
        self.save_key_overrides(next, window, cx);
    }

    /// 「清除」：解除这一项的绑定，只能靠鼠标点
    pub(crate) fn clear_key(
        &mut self,
        id: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(d) = self.settings_dlg.as_ref() else {
            return;
        };
        let mut next = d.key_overrides.clone();
        next.insert(id.to_string(), String::new());
        if let Some(d) = self.settings_dlg.as_mut() {
            d.key_yielded.retain(|(from, _)| *from != id);
        }
        self.save_key_overrides(next, window, cx);
    }

    /// 单项恢复：删掉覆盖项，回到默认键
    pub(crate) fn reset_key(
        &mut self,
        id: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(d) = self.settings_dlg.as_ref() else {
            return;
        };
        let mut next = d.key_overrides.clone();
        next.remove(id);
        if let Some(d) = self.settings_dlg.as_mut() {
            d.key_yielded.retain(|(from, to)| *from != id && *to != id);
        }
        self.save_key_overrides(next, window, cx);
    }

    /// 顶部那颗「恢复默认快捷键」：清空整张覆盖表
    pub(crate) fn reset_all_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = self.settings_dlg.as_mut() {
            d.recording = None;
            d.key_yielded.clear();
        }
        // 空 map = 全部走默认（core 的 `Settings::default()` 语义，
        // 不是"全部没有键"）
        self.save_key_overrides(HashMap::new(), window, cx);
    }
}

/// 键帽。**用官方 `Kbd` 而不是自己拼一个胶囊**——全站（按钮 tooltip、
/// 命令面板）画的键位都是它，形状与配色不一致比画错更要命。
///
/// 常态**不铺底色**：让 `Kbd` 自己那层画（`kbd.rs:242-243` 用的是
/// `tokens.muted` 底 + `muted_foreground` 字）。自己再铺一层只会让这里
/// 与 tooltip 里的那颗**看着不是同一个东西**，而那正是本页面要避免的
fn key_cap(t: &Theme, raw: Option<&str>, recording: bool, hovered: bool) -> Div {
    let base = div()
        .min_w(px(72.0))
        .h(px(28.0))
        .px(px(GAP_TIGHT))
        .rounded(px(R_MD))
        .flex()
        .items_center()
        .justify_center()
        .when(recording || hovered, |d| {
            d.bg(wash(t)).border_1().border_color(if recording {
                t.colors.accent
            } else {
                edge(t)
            })
        });
    if recording {
        // 录制态占住原来那格：键帽尺寸不变，用户才看得出"还是这一格"
        return base
            .text_size(px(fs_meta()))
            .text_color(tier3(t))
            .child("按下新的键…");
    }
    match raw.and_then(|k| keys::canonical(k)).and_then(|k| parse(&k)) {
        Some(ks) => base.child(Kbd::new(ks).appearance(false)),
        None => base
            .text_size(px(fs_meta()))
            .text_color(tier4(t))
            .child("未绑定"),
    }
}

/// 一行的名字 + 说明
fn row_label(d: &KeyDef, t: &Theme) -> Div {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap(px(GAP_DENSE))
        .child(
            div()
                .text_size(px(fs_body()))
                .text_color(t.colors.foreground)
                .child(d.title),
        )
        .child(
            div()
                .text_size(px(fs_meta()))
                .text_color(tier4(t))
                .child(d.hint),
        )
}

/// 「快捷键」那一页
pub(crate) fn keys_page(
    t: &Theme,
    st: &SettingsState,
    this: &Entity<Shell>,
    gv: ButtonCustomVariant,
) -> Div {
    let over = &st.key_overrides;
    // 一项都没改的时候那颗按钮灰着：用户点了没反应还看不出原因
    let customized = !over.is_empty();

    // 卡头右侧那一句 = **这一段现在的状态**（改了几项），与「代理」那一段挂
    // 探测结果是同一个用法；那颗「恢复默认」跟着它排在最右
    let mut hdr = CardHeader::new("快捷键").note(
        if customized {
            format!("已改 {} 项", over.len())
        } else {
            "全部是默认键".to_string()
        },
        false,
    );
    hdr = hdr.right(info_dot(
        "set-info-keys",
        "点右边的键帽改键；「清除」让这一项只能靠鼠标点。改完立刻生效，不用重启。",
        t,
        gv,
    ));
    hdr = hdr.right(
        Button::new("keys-reset-all")
            .label("恢复默认快捷键")
            .custom(gv)
            .with_size(Size::Small)
            .disabled(!customized)
            .on_click({
                let me = this.clone();
                move |_, window, cx| {
                    me.update(cx, |this, cx| this.reset_all_keys(window, cx));
                }
            })
            .into_any_element(),
    );

    // ⚠️ 录制器本体就是这一摞行的父容器：`on_key_down` 只在焦点节点的
    // 派发路径上收得到按键，所以必须 `track_focus`。不 track 的话整页点得动、
    // 就是录不到键，**而抓帧完全正常**
    let recording = st.recording.clone();
    let rows = keys::KEYS
        .iter()
        .map(|d| key_row(t, st, this, d, gv))
        .collect::<Vec<_>>();

    card(t).child(hdr.build(t)).child(
        v_flex()
            .w_full()
            .gap(px(GAP_TIGHT))
            .track_focus(&st.key_focus)
            .on_key_down({
                let me = this.clone();
                move |e, window, cx| {
                    if recording.is_none() {
                        return;
                    }
                    let raw = e.keystroke.unparse();
                    // 回车 / Esc 是这一页的"别录了"，它们不属于任何一个动作。
                    // ⚠️ **必须在录进去之前拦**：录上去的话用户会发现
                    // 回车被绑给了某个动作，而他自己想的是"回车结束录制"
                    if raw == "enter" || raw == "escape" {
                        me.update(cx, |this, cx| {
                            if let Some(d) = this.settings_dlg.as_mut() {
                                d.recording = None;
                            }
                            this.rebind_keys(cx);
                        });
                        return;
                    }
                    me.update(cx, |this, cx| this.apply_recorded_key(&raw, window, cx));
                }
            })
            .children(rows),
    )
}

/// 一行快捷键
fn key_row(
    t: &Theme,
    st: &SettingsState,
    this: &Entity<Shell>,
    d: &'static KeyDef,
    gv: ButtonCustomVariant,
) -> Div {
    let over = &st.key_overrides;
    let recording = st.recording.as_deref() == Some(d.id);
    let cur = keys::effective(d.id, over);
    let changed = over.contains_key(d.id);
    // 单项「恢复」只在真的偏离默认时才有意义（本来就是默认键、或者
    // 已经解绑了，都给它一颗键是让人白点一次）
    let can_reset = match over.get(d.id).map(|s| s.as_str()) {
        None => false,
        // 已解绑：单项「恢复」对它没意义，真要回去得走上面的键帽重录
        Some("") => false,
        Some(k) => keys::canonical(k).as_deref() != keys::canonical(d.default).as_deref(),
    };

    let cap = key_cap(
        t,
        cur.as_deref(),
        recording,
        st.key_cap_hover.as_deref() == Some(d.id),
    )
    .id(SharedString::from(format!("key-cap-{}", d.id)))
    .cursor_pointer()
    .on_click({
        let me = this.clone();
        move |_, window, cx| {
            me.update(cx, |this, cx| this.start_record_key(d.id, window, cx));
        }
    })
    .on_hover({
        let me = this.clone();
        move |h, _, cx| {
            let (id, on) = (d.id, *h);
            me.update(cx, |this, cx| {
                if let Some(dl) = this.settings_dlg.as_mut() {
                    dl.key_cap_hover = if on { Some(id.to_string()) } else { None };
                }
                cx.notify();
            });
        }
    });

    let clear = Button::new(SharedString::from(format!("key-clear-{}", d.id)))
        .label("清除")
        .custom(gv)
        .with_size(Size::Small)
        // 已经是未绑定的就别再给一颗按了没反应的键
        .disabled(cur.is_none())
        .on_click({
            let me = this.clone();
            move |_, window, cx| {
                me.update(cx, |this, cx| this.clear_key(d.id, window, cx));
            }
        });

    let reset = Button::new(SharedString::from(format!("key-reset-{}", d.id)))
        .label("恢复")
        .custom(gv)
        .with_size(Size::Small)
        .disabled(!can_reset)
        .on_click({
            let me = this.clone();
            move |_, window, cx| {
                me.update(cx, |this, cx| this.reset_key(d.id, window, cx));
            }
        });

    // 「已让给 XXX」只在本次打开期间出现，理由见 `SettingsDialog::key_yielded`
    let yielded_to = st
        .key_yielded
        .iter()
        .find(|(from, _)| from == d.id)
        .and_then(|(_, to)| keys::def_of(to))
        .map(|victim| victim.title);

    let badge = |text: String, color: gpui_kit::Hsla| {
        div()
            .text_size(px(fs_meta()))
            .text_color(color)
            .whitespace_nowrap()
            .child(text)
    };

    h_flex()
        .w_full()
        .gap(px(GAP))
        .items_center()
        .py(px(GAP_TIGHT))
        .border_b_1()
        .border_color(track(t))
        .child(row_label(d, t))
        .child(match yielded_to {
            Some(name) => badge(format!("已让给「{name}」"), t.colors.danger),
            None => div().w(px(0.0)),
        })
        .child(cap)
        .child(clear)
        .child(reset)
        .child(if changed {
            badge("已改".to_string(), tier4(t))
        } else {
            div().w(px(0.0))
        })
}

/// ⚠️ 这里**不再有**一个 `section_line()`：阶段 31.5 之前这一页顶上是
/// "标题 + 说明"两行浮在 dialog 底上，而其余三页已经换成卡 + 卡头 ⇒
/// 四页两种版面。现在这一页也走 `widgets::card` + `CardHeader`，
/// 那份手搓的段标题删掉了，**不留转发层**（AGENTS：留一层只会让人以为它还能用）

fn parse(raw: &str) -> Option<gpui_kit::Keystroke> {
    gpui_kit::Keystroke::parse(raw).ok()
}

/// 动作名
fn display_of(id: &str) -> String {
    keys::def_of(id)
        .map(|d| d.title.to_string())
        .unwrap_or_else(|| id.into())
}

/// 键位的人话（"Ctrl+R"）
fn display_key(raw: &str) -> String {
    match keys::canonical(raw).and_then(|k| parse(&k)) {
        Some(ks) => Kbd::format(&ks),
        None => raw.to_string(),
    }
}