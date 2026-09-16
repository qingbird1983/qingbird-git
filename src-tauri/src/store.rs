use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;

#[derive(Serialize, Deserialize, Clone, Debug)]
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
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Settings {
    /// 用户手动指定的代理，如 http://127.0.0.1:7890 或 socks5://127.0.0.1:1080；为空表示全自动探测
    #[serde(default)]
    pub manual_proxy: String,
    /// 界面主题：dark / light / system
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
}

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

fn config_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("config.json"))
}

pub fn load(app: &tauri::AppHandle) -> AppConfig {
    config_path(app)
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(app: &tauri::AppHandle, cfg: &AppConfig) -> Result<(), String> {
    let path = config_path(app).ok_or("无法确定配置目录")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}
