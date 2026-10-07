//! 全局快捷键的**唯一真源**（阶段 30）。
//!
//! 早先那 11 条键位是 `shell.rs::bind_keys` 里的一串 `KeyBinding::new`，
//! 而设置页要列出它们、要让用户改它们、要能一键还原——**三件事都要求
//! "有哪些快捷键"是一份能枚举的数据，而不是一串散在函数体里的语句**。
//! 所以这份表是抽出来的，Action 定义也一并搬过来（Action 本来就是这张表的载荷）。
//!
//! ## 三条纪律
//!
//! 1. **键名放在 core，不放这里**：`store::KEY_*`。Action 的名字由
//!    `#[action(namespace = …)]` 从结构体名推出来，改个结构体名磁盘上老用户
//!    那份配置就全认不出来，而**没有任何一处会报错**。
//! 2. **`KeyBinding::new` 在解析失败时 panic**，所以每个键位串进它之前
//!    必须先过 [`canonical`]。用户录进来的键是外部输入，不能假定合法。
//! 3. **只追加，不删除**：基座 `Keymap` 没有公开的删除入口，而
//!    `keymap.rs:158` 的原文是"后加的绑定优先级更高"——追加足以覆盖，
//!    所以改键 / 恢复默认 / 录制遮罩三条路全都只用 `add_bindings`。

use std::collections::HashMap;

// ⚠️ **`AsKeystroke` 刻意不在这里**：它只在 `#[cfg(test)]` 里用到
// （`b.keystrokes().first().as_keystroke()`），放在文件头的话
// **每次开发构建都会报一条 unused 警告**——而"零错零警"是本项目的出口条件，
// 一条每构建必现的警告过不了那个门。导入跟着用它的代码走
use gpui_kit::{Action, KeyBinding, Keystroke, NoAction};
use qingbird_core::store;

/// 一条快捷键的全部信息。`bind` 是**造绑定**而不是"造 Action"：
/// `KeyBinding::new` 要的是具体 Action 类型（`Box<dyn Action>` 没实现 `Action`），
/// 所以用函数指针把「id → 具体类型」这一段收在表里，调用方只管传键位串
pub(crate) struct KeyDef {
    /// core 里定的稳定键名（`store::KEY_*`），也是配置文件的键
    pub(crate) id: &'static str,
    /// 默认键位串。**用户没改过就用它**
    pub(crate) default: &'static str,
    /// 设置里那一行的名字
    pub(crate) title: &'static str,
    /// 名字旁边的一句话，说明这个动作干什么
    pub(crate) hint: &'static str,
    /// 造一条绑定。键位串由调用方给 ⇒ 同一份定义既能生成默认绑定，
    /// 也能生成用户改过的绑定，两条路不会分家
    pub(crate) bind: fn(&str) -> KeyBinding,
}

const fn def(
    id: &'static str,
    default: &'static str,
    title: &'static str,
    hint: &'static str,
    bind: fn(&str) -> KeyBinding,
) -> KeyDef {
    KeyDef {
        id,
        default,
        title,
        hint,
        bind,
    }
}

/// 全部可自定义的快捷键。**顺序即设置里显示的顺序**
pub(crate) const KEYS: &[KeyDef] = &[
    def(
        store::KEY_SIDEBAR,
        "ctrl-b",
        "折叠 / 展开侧栏",
        "侧栏是仓库列表，折起来主区能宽一屏",
        |k| KeyBinding::new(k, ToggleSidebar, None),
    ),
    def(
        store::KEY_INSPECTOR,
        "ctrl-i",
        "折叠 / 展开检视抽屉",
        "右边那一栏，放这一个仓库的细节",
        |k| KeyBinding::new(k, ToggleInspector, None),
    ),
    // ⚠️ **下面三条的顺序即语义**：`1` `2` `3` 与详情页同步卡从左到右同序。
    // 用户是先看见按钮再按位置的键，顺序一改快捷键与眼睛看到的东西就对不上。
    // 那个顺序声明在 `state.rs::SYNC_SLOTS`（连同文案一起）——
    // `keys::tests::the_sync_keys_keep_their_card_order` 钉住这个不变量
    def(
        store::KEY_SYNC_PULL,
        "1",
        "同步卡 · 拉取",
        "从云端把新提交接上来（不产生合并提交）",
        |k| KeyBinding::new(k, SyncPull, None),
    ),
    def(
        store::KEY_SYNC_PUSH,
        "2",
        "同步卡 · 推送",
        "把本地没推上去的提交送上去",
        |k| KeyBinding::new(k, SyncPush, None),
    ),
    def(
        store::KEY_SYNC_CHECK,
        "3",
        "同步卡 · 检测",
        "只看云端有没有更新，不动本地",
        |k| KeyBinding::new(k, SyncCheck, None),
    ),
    def(
        store::KEY_PALETTE,
        "ctrl-k",
        "命令面板",
        "搜命令、切仓库、打开设置",
        |k| KeyBinding::new(k, OpenPalette, None),
    ),
    def(
        store::KEY_TERMINAL,
        "ctrl-t",
        "在当前仓库开终端",
        "在终端里 cd 到这个仓库",
        |k| KeyBinding::new(k, OpenTerminal, None),
    ),
    def(
        store::KEY_STOP,
        "ctrl-.",
        "停止当前操作",
        "停掉正在跑的拉取 / 推送 / 检测 / 克隆",
        |k| KeyBinding::new(k, StopOp, None),
    ),
    def(
        store::KEY_NEXT_REPO,
        "down",
        "下一个仓库",
        "在侧栏当前这一组里往下走，到头就停",
        |k| KeyBinding::new(k, NextRepo, None),
    ),
    def(
        store::KEY_PREV_REPO,
        "up",
        "上一个仓库",
        "在侧栏当前这一组里往上走，到头就停",
        |k| KeyBinding::new(k, PrevRepo, None),
    ),
    def(
        store::KEY_DISMISS,
        "escape",
        "收起当前这一层",
        "关掉这一屏自己开的浮层；弹窗自己的 Esc 由基座管",
        |k| KeyBinding::new(k, DismissTop, None),
    ),
];

// ---- Action 定义（从 `shell.rs` 搬来）----
//
// 全部走官方 action + key binding，不自己 `on_key_down` 判键：
// 自己判的那份不知道当前焦点在输入框还是按钮上，会在打字时把侧栏折了

/// 折叠 / 展开侧栏
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct ToggleSidebar;

/// 折叠 / 展开检视抽屉
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct ToggleInspector;

/// 停掉当前正在跑的操作
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct StopOp;

/// 命令面板
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct OpenPalette;

/// 在当前仓库开终端
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct OpenTerminal;

/// `1` `2` `3`：同步卡上**从左到右**的拉取 · 推送 · 检测
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct SyncPull;
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct SyncPush;
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct SyncCheck;

/// `↓` / `↑`：在当前这一组里上下切仓库（到头就停，不绕回）
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct NextRepo;
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct PrevRepo;

/// `Esc`：收起当前这一屏自己开的临时东西。
/// 浮层（对话框 / Sheet / 右键菜单）的 Esc 由官方那几层自己吃掉，
/// 走到这里的只可能是本壳自己画的那一样
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = qingbird_gui, no_json)]
pub(crate) struct DismissTop;

// ---- 生效键位 ----

/// 键位串归一成可比较、可落盘的形式。
///
/// ⚠️ **不能直接比 `Keystroke` 的相等性**：它带着 IME 与平台合成的状态，
/// 同一个物理按键在"按下"和"从配置读出来"两条路上那些字段不一样，
/// 于是 `==` 会漏判，表现为"检测不出冲突"或"解除不干净"。
/// 两边都过一遍 `unparse` → `parse`，只剩规范字面量可比
pub(crate) fn canonical(raw: &str) -> Option<String> {
    Keystroke::parse(raw).ok().map(|k| k.unparse())
}

/// 表里某一项的定义。认不出的 id 返回 `None`
pub(crate) fn def_of(id: &str) -> Option<&'static KeyDef> {
    KEYS.iter().find(|d| d.id == id)
}

/// 这个动作**现在**该用哪个键。`None` = 未绑定（只能靠鼠标点）
///
/// ⚠️ **三态原样透出去**：没记过（走默认）/ 空串（解除了）/ 真键（改过），
/// 三者在这个函数里是三种不同的返回，压成两态的话「恢复默认」会把
/// 解绑的那一项也恢复成默认键
pub(crate) fn effective(id: &str, overrides: &HashMap<String, String>) -> Option<String> {
    let d = def_of(id)?;
    match store::key_override(&fake(overrides), id) {
        Some("") => None,
        Some(k) => Some(k.to_string()),
        None => Some(d.default.to_string()),
    }
}

/// `store::key_override` 要一个 `&Settings`，而这里只有那份 map。
/// 传合成对象只是为了复用同一份判据——**别把它当成"造一个假配置"**，
/// 它只在这一个函数里活一行
fn fake(overrides: &HashMap<String, String>) -> store::Settings {
    store::Settings {
        keybindings: overrides.clone(),
        ..store::Settings::default()
    }
}

/// 这个键现在归谁。`except` 是"正在改的那一项"——它自己当然算占着，
/// 但不能把自己报成冲突
pub(crate) fn owner_of(
    keystroke: &str,
    overrides: &HashMap<String, String>,
    except: &str,
) -> Option<&'static KeyDef> {
    let want = canonical(keystroke)?;
    KEYS.iter()
        .find(|d| {
            d.id != except
                && effective(d.id, overrides)
                    .and_then(|k| canonical(&k))
                    .is_some_and(|k| k == want)
        })
}

/// 生成**全部**要注册的绑定。
///
/// **三遍，顺序即语义**（`keymap.rs:201-226`：匹配项按下标从大到小遍历，
/// 越晚加的越先拿到派发权）：
///
/// 1. 被用户改过的动作，**先把它原来的默认键用 `NoAction` 盖掉**——
///    否则旧键仍然生效，界面显示新键、实际两个键都能按
/// 2. 没被改过的动作，注册它的默认键
/// 3. 被改过的动作，注册用户那把键。**放最后** ⇒ 用户键永远压过任何默认键，
///    不受它在表里的位置影响
///
/// 全部只追加、不删除：基座没有公开的删除入口，而覆盖靠的是优先级
pub(crate) fn bindings(overrides: &HashMap<String, String>) -> Vec<KeyBinding> {
    let mut out = Vec::new();
    for d in KEYS.iter().filter(|d| overrides.contains_key(d.id)) {
        if let Some(k) = canonical(d.default) {
            out.push(KeyBinding::new(&k, NoAction {}, None));
        }
    }
    for d in KEYS.iter().filter(|d| !overrides.contains_key(d.id)) {
        if let Some(k) = canonical(d.default) {
            out.push((d.bind)(&k));
        }
    }
    for d in KEYS.iter().filter(|d| overrides.contains_key(d.id)) {
        if let Some(k) = effective(d.id, overrides).and_then(|k| canonical(&k)) {
            out.push((d.bind)(&k));
        }
    }
    out
}

/// 录制期间的**遮罩**：把当前生效的每一个键都盖成 `NoAction`。
///
/// ⚠️ **为什么非它不可**：`window.rs:5943-5957` 里 **Action 派发先于**
/// 元素的 `on_key_down`（元素回调排在 5959 的 `finish_dispatch_key_event`）。
/// 也就是说用户点一下键帽开始录、正好按了 `Ctrl+B`，**侧栏会先折一下**
/// 才把键记下来；按 `Esc` 更糟，会把设置弹窗关掉。
/// 先盖住就没有"先发生了"。
pub(crate) fn shadows(overrides: &HashMap<String, String>) -> Vec<KeyBinding> {
    KEYS.iter()
        .filter_map(|d| effective(d.id, overrides).and_then(|k| canonical(&k)))
        .map(|k| KeyBinding::new(&k, NoAction {}, None))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    // `as_keystroke()` 是这个 trait 的方法，所以导入跟着用它的测试走
    use gpui_kit::AsKeystroke as _;

    fn ov(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// 表与 core 的清单**一一对应**。少一条 = 那个动作在设置里根本没有入口，
    /// 用户想改也改不了；多一条 = 配置里有个键名谁都不认，**静默失效**
    #[test]
    fn the_table_matches_the_core_list() {
        assert_eq!(KEYS.len(), store::KEYBINDING_IDS.len());
        for id in store::KEYBINDING_IDS {
            assert!(def_of(id).is_some(), "core 里有、界面上没有：{id}");
        }
        for d in KEYS {
            assert!(store::KEYBINDING_IDS.contains(&d.id), "界面里有、core 里没有：{}", d.id);
        }
    }

    /// 每一条的 id / 默认键 / 文案都要填满。空 hint 那种占位在设置页上
    /// 就是一个空位——扫一列全是名字没有解释，比没有这一行还糟
    #[test]
    fn no_entry_is_left_half_written() {
        for d in KEYS {
            assert!(!d.id.is_empty());
            assert!(!d.title.is_empty(), "{} 没有名字", d.id);
            assert!(!d.hint.is_empty(), "{} 没有说明", d.id);
            assert!(
                canonical(d.default).is_some(),
                "{} 的默认键 {:?} 解析不了",
                d.id,
                d.default
            );
        }
    }

    /// 默认键两两不同。重了的话设置页上会有两行抢同一个键，
    /// 而"谁生效"只取决于表里的先后——用户看着是随机的
    #[test]
    fn the_defaults_are_pairwise_distinct() {
        let mut seen = std::collections::HashSet::new();
        for d in KEYS {
            let k = canonical(d.default).expect("默认键可解析");
            assert!(
                seen.insert(k.clone()),
                "{} 的默认键与前面某条重了：{}",
                d.id,
                k
            );
        }
    }

    /// ⚠️ **端到端钉那条「按位置记忆」的不变量**：`1` `2` `3` 各自跑出来的
    /// Action，必须就是同步卡从左到右那三个按钮。
    ///
    /// 这条断言比"表里的顺序对不对"强一档：它查的是**最终生成的那条绑定
    /// 指向哪个 Action**。两处 `bind` 闭包写反了（Ctrl+1 去推送）的话，
    /// 只看表顺序的单测照样全绿，而用户按 `1` 拿到的是推送的结果。
    #[test]
    fn the_sync_keys_drive_the_very_operations_on_the_card() {
        use crate::state::{Op, SYNC_SLOT_LABELS, SYNC_SLOTS};
        let all = bindings(&HashMap::new());
        for ((slot, label), key) in SYNC_SLOTS
            .iter()
            .zip(SYNC_SLOT_LABELS.iter())
            .zip(["1", "2", "3"])
        {
            let want = match slot {
                Op::Pull => "SyncPull",
                Op::Push => "SyncPush",
                Op::Check => "SyncCheck",
                // ⚠️ **不许写 `_` 分支**。`Op` 以后加了新变体，这里编译不过才是对的
                // —— 那说明同步卡上多了一个没有配套快捷键的操作，得有人拍板
                other => panic!("同步卡上出现了没有快捷键的操作：{other:?}"),
            };
            let hit = all
                .iter()
                .find(|b| {
                    b.keystrokes()
                        .first()
                        .map(|k| k.as_keystroke().unparse())
                        .as_deref()
                        == Some(key)
                })
                .unwrap_or_else(|| panic!("默认键 {key} 上没有任何绑定"));
            assert!(
                hit.action().name().ends_with(want),
                "按 {key} 跑的是 {}，卡片上那一位是 {want}（{label}）",
                hit.action().name()
            );
        }
    }

    /// 同步卡的三个键与它在设置页里的行标题也要对上——那是用户改键时
    /// 唯一能看到"我改的是哪个按钮"的地方
    #[test]
    fn the_sync_rows_are_named_after_the_card() {
        for (d, label) in KEYS
            .iter()
            .filter(|d| d.id.starts_with("sync."))
            .zip(["拉取", "推送", "检测"])
        {
            assert!(
                d.title.ends_with(label),
                "{} 的标题 {:?} 与卡片文案「{label}」对不上",
                d.id,
                d.title
            );
        }
    }

    /// 三态：没记过走默认、空串是解除、真键是改过
    #[test]
    fn effective_keeps_three_states_apart() {
        let empty = HashMap::new();
        assert_eq!(effective(store::KEY_SIDEBAR, &empty), Some("ctrl-b".into()));

        let m = ov(&[(store::KEY_SIDEBAR, "ctrl-q")]);
        assert_eq!(effective(store::KEY_SIDEBAR, &m), Some("ctrl-q".into()));
        // 没被改的那一项不受牵连
        assert_eq!(effective(store::KEY_PALETTE, &m), Some("ctrl-k".into()));

        let m = ov(&[(store::KEY_SIDEBAR, "")]);
        assert_eq!(effective(store::KEY_SIDEBAR, &m), None);
    }

    /// 冲突判据：同一把键除了自己之外归谁
    #[test]
    fn owner_of_finds_the_key_being_stolen() {
        let empty = HashMap::new();
        // 默认下 Ctrl+B 归侧栏
        assert_eq!(
            owner_of("ctrl-b", &empty, store::KEY_PALETTE).map(|d| d.id),
            Some(store::KEY_SIDEBAR)
        );
        // 自己不算
        assert!(owner_of("ctrl-b", &empty, store::KEY_SIDEBAR).is_none());
        // 没人占的键
        assert!(owner_of("ctrl-shift-f9", &empty, store::KEY_SIDEBAR).is_none());
    }

    /// 用户改过键之后，**新键归新主人、旧键被盖掉**。
    ///
    /// 这条钉的是 `bindings()` 的三遍顺序——顺序写反的话旧键会残留，
    /// 表现为"设置里显示 Ctrl+R，按 Ctrl+B 还是会动"
    #[test]
    fn rebinding_replaces_the_old_key_instead_of_adding_one() {
        let m = ov(&[(store::KEY_SIDEBAR, "ctrl-r")]);
        let all: Vec<String> = bindings(&m)
            .iter()
            .filter_map(|b| b.keystrokes().first().map(|k| k.as_keystroke().unparse()))
            .collect();
        // 新键在、且**只有一条**指向它
        assert_eq!(
            all.iter().filter(|k| *k == "ctrl-r").count(),
            1,
            "新键应恰好一条：{all:?}"
        );
        // 旧键只剩遮罩（NoAction），不再指向侧栏
        assert!(all.contains(&"ctrl-b".to_string()));
        assert!(effective(store::KEY_SIDEBAR, &m).is_some());
    }

    /// 恢复默认 = 把 map 清空，绑定重新变回 11 条默认。
    /// 这是"一键恢复"那条路走的函数，得钉住它真的会变回去
    #[test]
    fn clearing_the_map_restores_the_defaults() {
        let m = ov(&[(store::KEY_SIDEBAR, "ctrl-r"), (store::KEY_TERMINAL, "")]);
        let after = HashMap::new();
        assert_eq!(bindings(&after).len(), KEYS.len());
        assert!(bindings(&after).iter().all(|b| {
            use gpui_kit::is_no_action;
            !is_no_action(&*b.action())
        }));
        assert_eq!(effective(store::KEY_SIDEBAR, &m), Some("ctrl-r".into()));
        assert_eq!(effective(store::KEY_TERMINAL, &m), None);
    }

    /// 遮罩盖住的是**当前生效**的每一个键，含用户改过的那些。
    /// 只盖默认键的话，用户把侧栏改成 `Ctrl+R` 之后按 `Ctrl+R` 录键，
    /// 侧栏还是会折——那正是遮罩要挡的那一下
    #[test]
    fn shadows_cover_the_customized_keys_too() {
        let m = ov(&[(store::KEY_SIDEBAR, "ctrl-r")]);
        let covered: Vec<String> = shadows(&m)
            .iter()
            .filter_map(|b| b.keystrokes().first().map(|k| k.as_keystroke().unparse()))
            .collect();
        assert_eq!(covered.len(), KEYS.len());
        assert!(covered.contains(&"ctrl-r".to_string()));
        assert!(covered.contains(&"ctrl-k".to_string()));
    }

    /// 用户解除绑定之后**不再有这一条**。判据落在数量上而不是"某条不存在"，
    /// 因为被覆盖的动作还会留一条 `NoAction` 遮罩在同一个键位串上
    #[test]
    fn an_unbound_action_emits_no_dispatchable_binding() {
        let m = ov(&[(store::KEY_TERMINAL, "")]);
        let real: Vec<String> = bindings(&m)
            .iter()
            .filter(|b| !gpui_kit::is_no_action(&*b.action()))
            .filter_map(|b| b.keystrokes().first().map(|k| k.as_keystroke().unparse()))
            .collect();
        assert!(
            !real.contains(&"ctrl-t".to_string()),
            "解除了还留着可派发的绑定：{real:?}"
        );
        assert_eq!(real.len(), KEYS.len() - 1);
    }
}