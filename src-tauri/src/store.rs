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
