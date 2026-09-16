//! 应用设置与 Git 凭证（仅存本地配置文件）。

use crate::store;
use serde::Serialize;
use tauri::Manager;

#[derive(Serialize, Clone)]
pub struct AppSettings {
    pub manual_proxy: String,
    pub last_good_proxy: Option<String>,
    pub theme: String,
}

#[tauri::command]
pub fn get_settings(app: tauri::AppHandle) -> AppSettings {
    let cfg = store::load(&app);
    AppSettings {
        manual_proxy: cfg.settings.manual_proxy,
        last_good_proxy: cfg.last_good_proxy,
        theme: cfg.settings.theme,
    }
}

#[tauri::command]
pub fn set_theme(app: tauri::AppHandle, theme: String) -> Result<(), String> {
    let t = match theme.as_str() {
        "dark" => Some(tauri::Theme::Dark),
        "light" => Some(tauri::Theme::Light),
        _ => None,
    };
    let mut cfg = store::load(&app);
    cfg.settings.theme = theme;
    store::save(&app, &cfg)?;
    if let Some(win) = app.get_webview_window("main") {
        win.set_theme(t).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------- 凭证与身份 ----------

#[derive(Serialize, Clone)]
pub struct AppCredentials {
    pub git_name: String,
    pub git_email: String,
    pub github_token: String,
    pub gitee_token: String,
    pub ssh_key_path: String,
}

#[tauri::command]
pub fn get_credentials(app: tauri::AppHandle) -> AppCredentials {
    let s = store::load(&app).settings;
    AppCredentials {
        git_name: s.git_name,
        git_email: s.git_email,
        github_token: s.github_token,
        gitee_token: s.gitee_token,
        ssh_key_path: s.ssh_key_path,
    }
}

#[tauri::command]
pub fn set_credentials(
    app: tauri::AppHandle,
    git_name: String,
    git_email: String,
    github_token: String,
    gitee_token: String,
    ssh_key_path: String,
) -> Result<(), String> {
    let mut cfg = store::load(&app);
    cfg.settings.git_name = git_name.trim().to_string();
    cfg.settings.git_email = git_email.trim().to_string();
    cfg.settings.github_token = github_token.trim().to_string();
    cfg.settings.gitee_token = gitee_token.trim().to_string();
    cfg.settings.ssh_key_path = ssh_key_path.trim().to_string();
    store::save(&app, &cfg)
}


#[tauri::command]
pub fn set_settings(app: tauri::AppHandle, manual_proxy: String) -> Result<(), String> {
    let mut cfg = store::load(&app);
    cfg.settings.manual_proxy = manual_proxy.trim().to_string();
    store::save(&app, &cfg)
}
