use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 钉在操作区的自定义命令按钮
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PinnedCmd {
    /// 按钮文案（留空时后端用命令前两个词兜底）
    pub label: String,
    /// 要运行的命令，走与「跑测试」同一条后台管道
    pub cmd: String,
}

/// 一键运行的上次结果。
///
/// 按**槽位**记（`test` / `build` / `dev` / `install` / `pinned:N`），不按命令文本记：
/// 用户改了命令不该把上次结果一起清掉——那是他想对照的历史，不是过期数据。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CmdRunRecord {
    /// 上次运行结束的时间戳（秒）
    pub at: u64,
    /// 退出码为 0
    pub ok: bool,
    pub code: i32,
    /// 输出了多少行（界面上「已输出 214 行」那个数字）
    pub lines: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct RepoEntry {
    pub path: String,
    pub name: String,
    /// 本地自定义显示名（如中文名），不影响仓库名与磁盘文件；空串表示未设置
    #[serde(default)]
    pub alias: String,
    pub added_at: u64,
    pub last_used: u64,
    /// 一键运行：测试命令；空串表示未设置（首次点击时自动识别预填）
    #[serde(default)]
    pub test_cmd: String,
    /// 一键运行：构建命令；空串表示未设置
    #[serde(default)]
    pub build_cmd: String,
    /// 一键运行：开发调试命令；空串表示未设置（面板用自动识别结果预填，不落盘）
    #[serde(default)]
    pub dev_cmd: String,
    /// 一键运行：安装依赖命令；空串表示未设置（面板用自动识别结果预填，不落盘）
    #[serde(default)]
    pub install_cmd: String,
    /// 钉在操作区的自定义命令按钮（最多若干个，见 run_cmd::MAX_PINNED_CMDS）
    #[serde(default)]
    pub pinned_cmds: Vec<PinnedCmd>,
    /// 一键运行的上次结果，按槽位记（键见 run_cmd::Slot）
    #[serde(default)]
    pub cmd_runs: HashMap<String, CmdRunRecord>,
    /// 定时自动同步间隔（分钟），0 = 关闭或处于"每天定时"模式；只有用户明确开启的仓库才参与调度
    #[serde(default)]
    pub auto_sync_mins: u32,
    /// "每天定时"模式的执行时间点（本地时区 "HH:MM"）；非空表示每天定时模式，优先于间隔模式
    #[serde(default)]
    pub daily_time: String,
    /// 检测到云端更新后是否自动拉取（false = 只检测提醒，不动工作区）
    #[serde(default)]
    pub auto_pull: bool,
    /// 上次自动检查时间戳（秒），由后台调度器维护
    #[serde(default)]
    pub last_auto_check: u64,
    /// 上次自动同步结果摘要（"ok:…" / "behind:…" / "skip:…" / "err:…"）
    #[serde(default)]
    pub last_auto_result: String,
    /// 用户置顶：排在列表最前（其余按 last_used 自动靠前）
    #[serde(default)]
    pub pinned: bool,
    /// 第三方仓库（别人的项目：关注简介/技术栈/活跃度/透明拉取）；
    /// false = 我自己的仓库（关注快捷命令/磁盘清理）。侧栏分组与页面路由都看它
    #[serde(default)]
    pub is_third_party: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    /// 用户手动指定的代理，如 http://127.0.0.1:7890 或 socks5://127.0.0.1:1080；为空表示全自动探测
    #[serde(default)]
    pub manual_proxy: String,
    /// 界面主题：`dark` / `light`。
    ///
    /// 老 Tauri 版留过一个 `system` 分支，**GPUI 这一侧从来不认它**——
    /// 真实配置里存的是 `"light"`，而设置那颗开关是两态的。
    /// 空串与任何别的值都落浅色（= 保持今天的样子），见 `gui/src/theme.rs`。
    #[serde(default)]
    pub theme: String,
    /// Git 提交身份（提交时未在全局配置时使用）
    #[serde(default)]
    pub git_name: String,
    #[serde(default)]
    pub git_email: String,
    /// 各平台私人访问令牌（仅存本地配置文件，用于私有仓库认证与云端建仓 API）
    #[serde(default)]
    pub github_token: String,
    #[serde(default)]
    pub gitee_token: String,
    /// SSH 私钥路径（远端为 ssh 协议时对该次 git 操作注入 GIT_SSH_COMMAND）
    #[serde(default)]
    pub ssh_key_path: String,
    /// 后台自动同步结果是否发 Windows 系统通知。
    /// 默认开：能进调度器的仓库本身就是用户显式开了自动同步的，结果理应被告知
    /// （主窗口可见且聚焦时 notify 会自行静默，不会打扰正在看界面的人）。
    #[serde(default = "default_true")]
    pub notify_auto_sync: bool,
    /// 仓库侧栏宽度（px）。GPUI 界面用；范围 200–360 由界面夹住后再存。
    /// 与 Tauri 版共用同一份 config.json，所以这里必须给**专用**默认值：
    /// 裸 `#[serde(default)]` 取的是 `f32::default()`（0.0）而不是 `Settings::default()`，
    /// 旧配置读出来会是 0，界面得再兜一层才不至于把侧栏夹成最窄
    #[serde(default = "default_sidebar_w")]
    pub sidebar_w: f32,
    /// 仓库侧栏是否折叠。进总览页不算折叠（那屏没有侧栏），
    /// 只有用户自己收起或 `Ctrl B` 才改这个值
    #[serde(default)]
    pub sidebar_collapsed: bool,
    /// 右侧检视列宽（px）。与 `sidebar_w` 同理：专用 serde 默认值 + 范围由
    /// core 夹住，界面只管传。**默认给 320**（就是原先写死在 `tokens::INSPECTOR_W`
    /// 的那个数），只是把它从"界面常量"升级成"用户能拖、且存得住"的值
    #[serde(default = "default_inspector_w")]
    pub inspector_w: f32,
    /// 检视列是否展开。**只由用户自己收起 / `Ctrl I` 改**，
    /// 与 `sidebar_collapsed` 同一口径：切页不该顺手改它
    ///
    /// **默认 false**：2026-10-05 起右栏退出常显模式（用户口径："启动默认
    /// 不显示，只有点击展开按钮才展开"）。它现在正文是「开发中」占位，
    /// 一条常驻的 320px 空列只会让主区白白少三分之一
    #[serde(default)]
    pub inspector_open: bool,
    /// 「我的」这一类仓库**藏了哪些模块**（阶段 19）
    ///
    /// **存「藏了哪些」而不是「显示哪些」**：空列表 = 什么都不藏 = 今天的界面。
    /// 反过来存"显示哪些"的话，老配置读出来是空，界面上就变成什么都不显示
    /// ——改个存储方向就把老用户的界面清空了。`#[serde(default)]` 天然
    /// 给出"什么都不藏"，**老配置零迁移**
    #[serde(default)]
    pub hidden_modules_mine: Vec<String>,
    /// 「第三方」这一类仓库藏了哪些模块。**同样存「藏了哪些」**，理由同上
    #[serde(default)]
    pub hidden_modules_third: Vec<String>,

    /// 点窗口关闭键时怎么办（阶段 22）。
    ///
    /// **存字面量而不是 bool**：要表达的是**三态**——每次都问 / 总是关到托盘 /
    /// 总是完全退出。bool 装不下"问"这一档（"这次不记住"和"记住关到托盘"
    /// 是两回事），硬压成 bool 就得拆两个字段，而两个字段必然能读出
    /// 互相矛盾的组合。
    ///
    /// **默认 `ask`**：老配置读出来就是这一档，零迁移；而且这一档是
    /// **最安全的兜底方向**——见 [`close_action_of`] 的注释。
    #[serde(default = "default_close_action")]
    pub close_action: String,
}

// ---- 模块可见性（阶段 19）----
//
// 键名是**界面与配置之间唯一的约定**，所以放 core：界面那份勾选区、
// 那份判据、以及用户 config 里存的字面量，必须是同一批字符串。
// 拼错了的判据是「静默地不生效」，界面上看不出来。
//
// 前缀分两组（`tab.` 页签 / `sec.` 概览段），不是为了好看——是设置对话框
// 要按组分区画，而 `starts_with` 一比就够，不必另存一份分组表

/// 详情页页签
pub const MOD_TAB_OVERVIEW: &str = "tab.overview";
pub const MOD_TAB_README: &str = "tab.readme";
pub const MOD_TAB_HISTORY: &str = "tab.history";
pub const MOD_TAB_SPACE: &str = "tab.space";
pub const MOD_TAB_CHANGES: &str = "tab.changes";
/// 概览页的段
pub const MOD_SEC_FORK: &str = "sec.fork";
pub const MOD_SEC_SYNC: &str = "sec.sync";
pub const MOD_SEC_LOG: &str = "sec.log";
pub const MOD_SEC_LANG: &str = "sec.lang";

/// 第三方**天生不适用**的模块
///
/// ⚠️ **基线不能被开关放出来**，理由不是"不想给"：「空间」页里有一键清理
/// （真删 `node_modules` / `target` / `dist`），摆到一个只是关注着的别人
/// 项目上就是一个**越界的删除入口**；分叉条改的是本地历史，对第三方
/// 本来就没有意义。**基线编码的是安全边界，开关只负责在基线之上再藏**。
/// 设置里那两项是禁用 + 说明，不是可勾的
pub const THIRD_BASELINE_HIDDEN: &[&str] = &[MOD_TAB_SPACE, MOD_SEC_FORK];

/// 这个模块在**这个归属**下是否适用（= 不在基线里）
pub fn module_applicable(third: bool, key: &str) -> bool {
    !third || !THIRD_BASELINE_HIDDEN.contains(&key)
}

/// 某个归属下**藏了哪些**。基线 + 用户勾的，合成一份
pub fn hidden_modules<'a>(settings: &'a Settings, third: bool) -> Vec<&'a str> {
    let user: &[String] = if third {
        &settings.hidden_modules_third
    } else {
        &settings.hidden_modules_mine
    };
    let mut out: Vec<&str> = user.iter().map(String::as_str).collect();
    if third {
        out.extend(THIRD_BASELINE_HIDDEN.iter().copied());
    }
    out
}

/// 这个模块显不显示。**纯函数**——判据只有这一份，界面各处都问它
///
/// 泛型收 `AsRef<str>` 是因为有**两种形状**要查：配置里那份是
/// `Vec<String>`（用户写的字面量），合并归属基线之后那份是 `Vec<&str>`。
/// 写成两个函数就是两条会各自漂的判据
pub fn module_shown<S: AsRef<str>>(hidden: &[S], key: &str) -> bool {
    !hidden.iter().any(|k| k.as_ref() == key)
}

/// 在给定候选序列里挑出该显示的，**但至少留一个**
///
/// ⚠️ **全藏了就回退到基线（也就是全显示）**，不给一页空白：
/// 空白页看着像坏了，而不是像"我刚把这儿都关了"。同理，一个页签都不剩时
/// 页签栏就没法当导航用了
pub fn visible_modules<'a>(all: &[&'a str], hidden: &[&str]) -> Vec<&'a str> {
    let vis: Vec<&str> = all
        .iter()
        .copied()
        .filter(|k| module_shown(hidden, k))
        .collect();
    if vis.is_empty() {
        all.to_vec()
    } else {
        vis
    }
}

fn default_true() -> bool {
    true
}

/// 侧栏宽度的 serde 缺省值。见 `Settings::sidebar_w` 的注释：
/// 走独立函数而不是 `Settings::default()`，否则反序列化路径与新建路径会给出不同的宽度

// ---- 关窗行为（阶段 22）----

/// 点关闭键时怎么办。**存字面量而不是 bool**：理由见 `Settings::close_action`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseAction {
    /// 每次都问（默认）
    Ask,
    /// 直接关到托盘：窗口销毁、进程留着，托盘图标能把它叫回来
    Tray,
    /// 直接完全退出：进程结束，后台自动同步也跟着停
    Exit,
}

/// 配置里存的那三个字面量。**键名是界面与配置之间唯一的约定**，所以放 core
pub const CLOSE_ASK: &str = "ask";
pub const CLOSE_TRAY: &str = "tray";
pub const CLOSE_EXIT: &str = "exit";

/// 配置里那个字符串 -> 判据。
///
/// **认不出的值一律落 `Ask`**，与 `module_shown` 同一取向：
/// **兜底到需要用户在场的那一档**。反过来兜底（认不出 = 直接执行）的话，
/// 一次手改配置或旧版本留下的非法值，就能让用户"点了 X 软件就没了"，
/// 而他什么也没选过——那是不可撤销的那种错。
pub fn close_action_of(raw: &str) -> CloseAction {
    match raw.trim() {
        CLOSE_TRAY => CloseAction::Tray,
        CLOSE_EXIT => CloseAction::Exit,
        _ => CloseAction::Ask,
    }
}

/// serde 缺省值。走独立函数而不是 `Settings::default()`，
/// 理由与 `default_sidebar_w` 一样：反序列化路径与新建路径必须给出同一个值
fn default_close_action() -> String {
    CLOSE_ASK.to_string()
}

fn default_sidebar_w() -> f32 {
    DEFAULT_SIDEBAR_W
}

/// 检视列宽的 serde 缺省值，理由与 `default_sidebar_w` 相同
fn default_inspector_w() -> f32 {
    DEFAULT_INSPECTOR_W
}

/// 手写而非 derive：`notify_auto_sync` 默认 true，derive 出来的默认值会是 false，
/// 导致"配置文件不存在/字段缺失"时开关状态与新建配置不一致
impl Default for Settings {
    fn default() -> Self {
        Self {
            manual_proxy: String::new(),
            theme: String::new(),
            git_name: String::new(),
            git_email: String::new(),
            github_token: String::new(),
            gitee_token: String::new(),
            ssh_key_path: String::new(),
            notify_auto_sync: true,
            sidebar_w: DEFAULT_SIDEBAR_W,
            sidebar_collapsed: false,
            inspector_w: DEFAULT_INSPECTOR_W,
            inspector_open: false,
            hidden_modules_mine: Vec::new(),
            hidden_modules_third: Vec::new(),
            close_action: default_close_action(),
        }
    }
}

/// 侧栏默认宽。旧配置里没有 `sidebar_w`，读出来是 0.0，
/// 界面看到非正值会回落到这个值，所以默认值必须写在 core 而不是只写在界面常量里
pub const DEFAULT_SIDEBAR_W: f32 = 240.0;
/// 侧栏宽度下限 / 上限（界面与本函数共用，存进去的永远是夹过的值）
pub const SIDEBAR_W_RANGE: (f32, f32) = (200.0, 360.0);

/// 检视列默认宽。原来这个数写死在界面 `tokens::INSPECTOR_W`，
/// 有了落盘字段之后它就是"没有存过值时用哪个"，仍写在 core
pub const DEFAULT_INSPECTOR_W: f32 = 320.0;
/// 检视列宽下限 / 上限。
///
/// 上限 560 的理由是主区还要剩得下内容：窗口下限 1080 − 轨 44 − 侧栏 200
/// = 836px 主区，抽屉 560 就只剩 276px，那已经是"没法看东西"了。
/// 下限 240 装得下最窄的一行变更文件名
pub const INSPECTOR_W_RANGE: (f32, f32) = (240.0, 560.0);

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub repos: Vec<RepoEntry>,
    #[serde(default)]
    pub settings: Settings,
    /// 最近一次代理回退成功的地址，下次优先尝试
    #[serde(default)]
    pub last_good_proxy: Option<String>,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 切换仓库的「我的 / 第三方」归属并落盘。
/// 旧配置文件没有这个字段：serde default 会补成 false（我的），不会丢数据。
pub fn set_third_party(path: &str, val: bool) -> Result<(), String> {
    let mut cfg = load();
    let Some(e) = cfg.repos.iter_mut().find(|e| e.path == path) else {
        return Err("仓库不存在".into());
    };
    e.is_third_party = val;
    save(&cfg)
}

/// 侧栏宽度夹到合法范围。0 与负数（老配置没有这个字段）回落到默认宽度，
/// 而不是夹到下限——把"没设过"当成"用户想要最窄"会让老用户第一次开新版就看到一条缝
pub fn clamp_sidebar_w(w: f32) -> f32 {
    if !w.is_finite() || w <= 0.0 {
        return DEFAULT_SIDEBAR_W;
    }
    w.clamp(SIDEBAR_W_RANGE.0, SIDEBAR_W_RANGE.1)
}

/// 落盘侧栏版式（宽度 + 折叠态）。
/// 宽度在这里夹一次再存，而不是存界面里的原始值：配置文件会被别的工具读，
/// 存进去一个 900px 的侧栏只会把那个工具的版面算歪
pub fn set_sidebar_layout(w: f32, collapsed: bool) -> Result<(), String> {
    let mut cfg = load();
    cfg.settings.sidebar_w = clamp_sidebar_w(w);
    cfg.settings.sidebar_collapsed = collapsed;
    save(&cfg)
}

/// 检视列宽度夹到合法范围。**与 `clamp_sidebar_w` 同一套判据**：
/// 非有限值与 0 回落默认（老配置没这个字段），其余夹进 `INSPECTOR_W_RANGE`
pub fn clamp_inspector_w(w: f32) -> f32 {
    if !w.is_finite() || w <= 0.0 {
        return DEFAULT_INSPECTOR_W;
    }
    w.clamp(INSPECTOR_W_RANGE.0, INSPECTOR_W_RANGE.1)
}

/// 落盘检视列版式（宽度 + 展开态）。与 `set_sidebar_layout` 一对
pub fn set_inspector_layout(w: f32, open: bool) -> Result<(), String> {
    let mut cfg = load();
    cfg.settings.inspector_w = clamp_inspector_w(w);
    cfg.settings.inspector_open = open;
    save(&cfg)
}

/// 定时自动同步的间隔档位。界面只在这三档里选，**不给自由输入**：
/// 间隔能填出 `0`（等于关）或 3 分钟（每 20 秒派生一次 git，用户会以为程序坏了）。
/// 范围下限 5 分钟也是同一个道理——比这更密的轮询对 git 没有意义，只有负担
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AutoSync {
    /// 不定时
    Off,
    /// 每 N 分钟
    Every(u32),
    /// 每天某个时刻（`"HH:MM"`，本地时区）
    Daily(String),
}

/// 定时同步间隔的合法值域（分钟）
pub const AUTO_SYNC_MINS_RANGE: (u32, u32) = (5, 24 * 60);

/// 把界面选的那一档规范化成要落盘的两个字段 `(auto_sync_mins, daily_time)`。
///
/// **纯函数**：这层规范化值得有单测，因为它挡的是"配置被写坏"——
/// 一旦 `daily_time` 里存进 `9点` 这种值，后台调度器就再也匹配不上时间点，
/// 而界面自己看不出哪里错了。
///
/// 规范化规则：
/// * `Off` → `(0, "")`
/// * `Every(n)` → 夹进 [`AUTO_SYNC_MINS_RANGE`]；夹到下限而不是 0，
///   因为"要 3 分钟"和"不要定时"是两件事，不能互相顶替
/// * `Daily(t)` → 合法时间点给规范化后的 `"HH:MM"`，非法当关闭处理。
///   界面在时间非法时会禁用保存按钮，所以这条回落只防外部写入
pub fn normalize_auto_sync(m: &AutoSync) -> (u32, String) {
    match m {
        AutoSync::Off => (0, String::new()),
        AutoSync::Every(n) => (
            (*n).clamp(AUTO_SYNC_MINS_RANGE.0, AUTO_SYNC_MINS_RANGE.1),
            String::new(),
        ),
        AutoSync::Daily(t) => match parse_hhmm(t) {
            Some((h, m)) => (0, format!("{h:02}:{m:02}")),
            None => (0, String::new()),
        },
    }
}

/// 落盘某仓库的定时同步设置。
/// `auto_pull` 只在定时档位下才有意义，但它总是落盘——用户关掉定时之后
/// 回头再开时，检测到更新就自动拉取这个选择应该还在
pub fn set_auto_sync(path: &str, mode: &AutoSync, auto_pull: bool) -> Result<(), String> {
    let (mins, daily) = normalize_auto_sync(mode);
    let mut cfg = load();
    let Some(e) = cfg.repos.iter_mut().find(|e| e.path == path) else {
        return Err("仓库不存在".into());
    };
    e.auto_sync_mins = mins;
    e.daily_time = daily;
    e.auto_pull = auto_pull;
    // 档位变了就得重新计时：拿上一轮的时间戳去比新的间隔，
    // 会在改完设置的瞬间就先跑一轮
    e.last_auto_check = 0;
    save(&cfg)
}

/// 解析 "HH:MM" 时间字符串，合法返回 (小时, 分钟)，非法返回 None
pub fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.split_once(':')?;
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = m.trim().parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some((h, m))
}

fn config_path() -> Option<PathBuf> {
    // 与 src-tauri 版保持同一配置文件：%APPDATA%/com.qingbird.git/config.json
    let appdata = std::env::var("APPDATA").ok()?;
    Some(PathBuf::from(appdata)
        .join("com.qingbird.git")
        .join("config.json"))
}

pub fn load() -> AppConfig {
    config_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &AppConfig) -> Result<(), String> {
    let path = config_path().ok_or("无法确定配置目录")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 这里只测纯逻辑。`set_sidebar_layout` 会写 %APPDATA% 下真实的 config.json，
    // 拿它做单测等于每次跑测试都改掉用户自己的配置——那种测试不许写。

    /// 阶段 19 的一条硬约定：**空列表 = 什么都不藏 = 今天的界面**。
    /// 反过来存"显示哪些"的话，老配置读出来是空 ⇒ 界面全空
    #[test]
    fn an_empty_hidden_list_hides_nothing() {
        let s = Settings::default();
        assert!(s.hidden_modules_mine.is_empty());
        assert!(s.hidden_modules_third.is_empty());
        for k in [
            MOD_TAB_OVERVIEW,
            MOD_TAB_README,
            MOD_TAB_HISTORY,
            MOD_TAB_SPACE,
            MOD_TAB_CHANGES,
            MOD_SEC_FORK,
            MOD_SEC_SYNC,
            MOD_SEC_LOG,
            MOD_SEC_LANG,
        ] {
            assert!(module_shown(&hidden_modules(&s, false), k), "我的：{k}");
        }
    }

    /// 第三方天生少两项，且**开关放不出来**。这不是"不想给"：「空间」页里
    /// 有一键清理（真删文件），摆到别人项目上就是越界的入口
    #[test]
    fn the_third_party_baseline_cannot_be_switched_back_on() {
        let s = Settings::default();
        let third = hidden_modules(&s, true);
        assert!(!module_shown(&third, MOD_TAB_SPACE), "空间对第三方不适用");
        assert!(!module_shown(&third, MOD_SEC_FORK), "分叉条对第三方不适用");
        assert!(!module_applicable(true, MOD_TAB_SPACE));
        assert!(!module_applicable(true, MOD_SEC_FORK));
        // 「我的」那边两项都适用
        assert!(module_applicable(false, MOD_TAB_SPACE));
        assert!(module_applicable(false, MOD_SEC_FORK));

        // ⚠️ **用户把它写进"没藏"的反义里也没用**：基线是并进去的，
        // 不看用户那份。少写这一条的话，一个手改过的 config 就能把
        // 一键清理挂到别人项目上
        let sneaky = Settings {
            hidden_modules_third: vec![MOD_TAB_HISTORY.to_string()],
            ..Settings::default()
        };
        let t2 = hidden_modules(&sneaky, true);
        assert!(!module_shown(&t2, MOD_TAB_SPACE));
        assert!(!module_shown(&t2, MOD_SEC_FORK));
        // 而用户自己勾的那一项确实生效了
        assert!(!module_shown(&t2, MOD_TAB_HISTORY));
    }

    /// 两套互不串：藏「我的」的历史页签，第三方那边不受影响
    #[test]
    fn the_two_scopes_do_not_leak_into_each_other() {
        let s = Settings {
            hidden_modules_mine: vec![MOD_TAB_HISTORY.to_string()],
            ..Settings::default()
        };
        assert!(!module_shown(&hidden_modules(&s, false), MOD_TAB_HISTORY));
        assert!(module_shown(&hidden_modules(&s, true), MOD_TAB_HISTORY));
    }

    /// **全藏了要回退到全显示，不能给一页空白**。空白页看着像坏了，
    /// 而不是像"我刚把这儿都关了"
    #[test]
    fn hiding_everything_falls_back_instead_of_going_blank() {
        let all = [MOD_TAB_OVERVIEW, MOD_TAB_HISTORY, MOD_TAB_CHANGES];
        let vis = visible_modules(&all, &all);
        assert_eq!(vis, all, "一个都不剩时回退到基线");
        // 留一个就按留的来，不要多给
        let vis = visible_modules(&all, &[MOD_TAB_HISTORY]);
        assert_eq!(vis, vec![MOD_TAB_OVERVIEW, MOD_TAB_CHANGES]);
    }

    #[test]
    fn clamp_keeps_width_inside_the_design_range() {
        assert_eq!(clamp_sidebar_w(240.0), 240.0);
        assert_eq!(clamp_sidebar_w(200.0), 200.0);
        assert_eq!(clamp_sidebar_w(360.0), 360.0);
        assert_eq!(clamp_sidebar_w(120.0), 200.0);
        assert_eq!(clamp_sidebar_w(900.0), 360.0);
    }

    #[test]
    fn clamp_treats_a_missing_width_as_default_not_as_narrowest() {
        // 旧配置没有这个字段，serde 给 0.0。夹到下限 200 会让老用户
        // 第一次打开新版就看到一条比平时窄一半的侧栏
        assert_eq!(clamp_sidebar_w(0.0), DEFAULT_SIDEBAR_W);
        assert_eq!(clamp_sidebar_w(-50.0), DEFAULT_SIDEBAR_W);
        assert_eq!(clamp_sidebar_w(f32::NAN), DEFAULT_SIDEBAR_W);
        assert_eq!(clamp_sidebar_w(f32::INFINITY), DEFAULT_SIDEBAR_W);
    }

    #[test]
    fn default_settings_carry_a_usable_sidebar_layout() {
        let s = Settings::default();
        assert_eq!(s.sidebar_w, DEFAULT_SIDEBAR_W);
        assert!(!s.sidebar_collapsed);
        // notify_auto_sync 默认 true 是手写 Default 的原因，别在加字段时改回 derive
        assert!(s.notify_auto_sync);
    }

    #[test]
    fn old_config_without_sidebar_fields_still_loads() {
        // 这份 JSON 是加侧栏字段之前写出来的形状
        let old = r#"{
            "repos": [],
            "settings": { "theme": "light", "notify_auto_sync": false },
            "last_good_proxy": null
        }"#;
        let cfg: AppConfig = serde_json::from_str(old).expect("旧配置应能解析");
        assert_eq!(cfg.settings.theme, "light");
        assert!(!cfg.settings.notify_auto_sync);
        assert_eq!(cfg.settings.sidebar_w, DEFAULT_SIDEBAR_W);
        assert!(!cfg.settings.sidebar_collapsed);
    }

    #[test]
    fn sidebar_layout_round_trips_through_json() {
        let mut cfg = AppConfig::default();
        cfg.settings.sidebar_w = 288.0;
        cfg.settings.sidebar_collapsed = true;
        let json = serde_json::to_string(&cfg).expect("序列化");
        let back: AppConfig = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(back.settings.sidebar_w, 288.0);
        assert!(back.settings.sidebar_collapsed);
    }

    /// 检视列的夹取与侧栏**必须同一套判据**：非有限值与 0 回落默认，
    /// 其余夹进范围。两边行为不一致时，界面读一个值、落盘另一个值，
    /// 表现是"拖完松手宽度又跳回去"
    #[test]
    fn inspector_clamp_matches_the_sidebar_contract() {
        assert_eq!(clamp_inspector_w(320.0), 320.0);
        assert_eq!(clamp_inspector_w(240.0), INSPECTOR_W_RANGE.0);
        assert_eq!(clamp_inspector_w(560.0), INSPECTOR_W_RANGE.1);
        assert_eq!(clamp_inspector_w(120.0), INSPECTOR_W_RANGE.0);
        assert_eq!(clamp_inspector_w(900.0), INSPECTOR_W_RANGE.1);
        assert_eq!(clamp_inspector_w(0.0), DEFAULT_INSPECTOR_W);
        assert_eq!(clamp_inspector_w(-50.0), DEFAULT_INSPECTOR_W);
        assert_eq!(clamp_inspector_w(f32::NAN), DEFAULT_INSPECTOR_W);
        assert_eq!(clamp_inspector_w(f32::INFINITY), DEFAULT_INSPECTOR_W);
    }

    /// 检视列默认**收起**、但宽度可用。2026-10-05 改的：右栏正文此时是
    /// 「开发中」占位，一条常驻的 320px 空列只会让主区白白少三分之一
    /// （用户口径："右栏退出常显模式，启动默认不显示"）。
    /// 展开入口一直都在——同步段右上角那颗 `inspector_reopen`
    #[test]
    fn inspector_defaults_to_collapsed_with_a_usable_width() {
        let s = Settings::default();
        assert_eq!(s.inspector_w, DEFAULT_INSPECTOR_W);
        assert!(!s.inspector_open);
    }

    /// 旧配置（加检视字段之前写出来的）必须仍能解析，且补上的是默认值。
    ///
    /// ⚠️ `inspector_open` 走**裸 `#[serde(default)]`**（= false），
    /// 与 `sidebar_w` 那种"用专用函数补成默认值"的做法相反——这里要的就是
    /// 「没这个字段的老配置 = 右栏收起」。用 `default_true` 的话，老用户
    /// 装上新版仍会被弹出一条 320px 空列，而那一列此刻什么也没有
    #[test]
    fn old_config_without_inspector_fields_still_loads() {
        let old = r#"{
            "repos": [],
            "settings": { "theme": "light", "sidebar_w": 288.0 },
            "last_good_proxy": null
        }"#;
        let cfg: AppConfig = serde_json::from_str(old).expect("旧配置应能解析");
        assert_eq!(cfg.settings.sidebar_w, 288.0);
        assert_eq!(cfg.settings.inspector_w, DEFAULT_INSPECTOR_W);
        assert!(!cfg.settings.inspector_open);
    }

    #[test]
    fn inspector_layout_round_trips_through_json() {
        let mut cfg = AppConfig::default();
        cfg.settings.inspector_w = 420.0;
        cfg.settings.inspector_open = false;
        let json = serde_json::to_string(&cfg).expect("序列化");
        let back: AppConfig = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(back.settings.inspector_w, 420.0);
        assert!(!back.settings.inspector_open);
    }

    /// 上限 560 是有约束的：窗口下限 1080 − 轨 44 − 侧栏下限 200 = 836px 主区，
    /// 再减去上限 560 只剩 276px。这个测试把那条算式钉住，
    /// 免得以后有人把上限调到 700 还以为"更灵活"
    #[test]
    fn inspector_max_still_leaves_the_main_area_usable() {
        let main = WIN_MIN_W_CONST - NAV_W_CONST - SIDEBAR_W_RANGE.0;
        assert!(
            main - INSPECTOR_W_RANGE.1 >= 240.0,
            "抽屉拉到上限后主区只剩 {:.0}px，低于 240 就没法看内容了（主区上限算式被改坏了）",
            main - INSPECTOR_W_RANGE.1
        );
    }

    /// 版面常数：与界面 `tokens.rs` 同源，这里只留算式用得到的两个
    const WIN_MIN_W_CONST: f32 = 1080.0;
    const NAV_W_CONST: f32 = 44.0;

    #[test]
    fn off_clears_both_timer_fields() {
        // 两个字段必须一起清：只清一个的话，调度器会拿着上次的
        // daily_time 继续每天跑，用户明明已经关了
        assert_eq!(normalize_auto_sync(&AutoSync::Off), (0, String::new()));
    }

    #[test]
    fn interval_clamps_instead_of_falling_back_to_off() {
        // 3 分钟：夹到下限，而不是当成"不要定时"。
        // 两者在界面上是两个不同的档位，混掉就是用户设了却没生效
        assert_eq!(normalize_auto_sync(&AutoSync::Every(3)).0, 5);
        assert_eq!(normalize_auto_sync(&AutoSync::Every(0)).0, 5);
        assert_eq!(normalize_auto_sync(&AutoSync::Every(30)), (30, String::new()));
        // 上限：一天跑 1440 次没有意义
        assert_eq!(normalize_auto_sync(&AutoSync::Every(9999)).0, 1440);
    }

    #[test]
    fn daily_time_is_normalized_to_zero_padded_hh_mm() {
        // 存进去 "9:5" 的话，后台调度要自己补零才能匹配
        assert_eq!(
            normalize_auto_sync(&AutoSync::Daily("9:5".into())).1,
            "09:05"
        );
        assert_eq!(
            normalize_auto_sync(&AutoSync::Daily(" 22 : 30 ".into())).1,
            "22:30"
        );
    }

    #[test]
    fn an_unparsable_daily_time_turns_the_timer_off() {
        // 非法时间当关闭处理：留一个匹配不上的 daily_time 在配置里，
        // 调度器每分钟都比一次、永远不匹配，用户看不出哪里出了问题
        assert_eq!(
            normalize_auto_sync(&AutoSync::Daily("9点".into())),
            (0, String::new())
        );
        assert_eq!(
            normalize_auto_sync(&AutoSync::Daily("24:00".into())),
            (0, String::new())
        );
        assert_eq!(
            normalize_auto_sync(&AutoSync::Daily(String::new())),
            (0, String::new())
        );
    }

    #[test]
    fn daily_mode_never_leaves_a_stale_interval_behind() {
        // 从"每 15 分钟"切到"每天 09:00"：auto_sync_mins 必须让位，
        // 否则调度器两个条件都满足，行为取决于实现细节
        let (mins, daily) = normalize_auto_sync(&AutoSync::Daily("09:00".into()));
        assert_eq!(mins, 0);
        assert_eq!(daily, "09:00");
    }


    #[test]
    fn close_action_recognizes_the_three_literals() {
        assert_eq!(close_action_of(CLOSE_ASK), CloseAction::Ask);
        assert_eq!(close_action_of(CLOSE_TRAY), CloseAction::Tray);
        assert_eq!(close_action_of(CLOSE_EXIT), CloseAction::Exit);
    }

    #[test]
    fn an_unrecognized_close_action_falls_back_to_asking() {
        // 空串是**老配置**读出来的样子（字段刚加，旧 config.json 里没有这一项），
        // 它必须落 Ask 而不是 Exit——见 close_action_of 的注释
        assert_eq!(close_action_of(""), CloseAction::Ask);
        assert_eq!(close_action_of("TRay"), CloseAction::Ask);
        assert_eq!(close_action_of("quit"), CloseAction::Ask);
        // 收尾空白容错：这份字段是人手改的 config.json
        assert_eq!(close_action_of("tray "), CloseAction::Tray);
    }

    #[test]
    fn the_default_is_asking_not_exiting() {
        // 这一条是**安全断言**：默认值改错方向的话，老用户升级后
        // "点一下 X 软件就没了"，而他从没选过
        assert_eq!(close_action_of(&default_close_action()), CloseAction::Ask);
        assert_eq!(close_action_of(""), CloseAction::Ask);
    }
}
