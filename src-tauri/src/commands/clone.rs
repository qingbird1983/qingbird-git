//! 克隆远端仓库到本地。

use crate::{git, store};
use serde::Serialize;
use tauri::ipc::Channel;

use super::net::{progress_hooks, run_net_op, NetMode};
use super::types::ProgressEvent;
use super::{begin_user_op, NET_TIMEOUT};

#[derive(Serialize, Clone)]
pub struct CloneOutcome {
    pub path: String,
    pub name: String,
    pub repos: Vec<store::RepoEntry>,
}

/// 从 git URL 提取仓库名（兼容 https/git/ssh 及 user@host:path 形式，去掉 .git 后缀）
pub fn repo_name_from_url(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let tail = if let Some(pos) = url.rfind(':').filter(|_| url.contains('@')) {
        // ssh 短格式 user@host:owner/repo.git
        &url[pos + 1..]
    } else {
        url.rsplit_once('/')?.1
    };
    let tail = tail.rsplit('/').next().unwrap_or(tail);
    let name = tail.strip_suffix(".git").unwrap_or(tail);
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-. ".contains(c));
    valid.then(|| name.to_string())
}

#[tauri::command(async)]
pub fn clone_repo(
    app: tauri::AppHandle,
    url: String,
    parent_dir: String,
    net_mode: Option<String>,
    on_progress: Channel<ProgressEvent>,
) -> Result<CloneOutcome, String> {
    let Some(_guard) = begin_user_op() else {
        return Err("后台自动同步正在进行，已请求中断，请稍后重试".into());
    };
    let mode = NetMode::parse(net_mode.as_deref().unwrap_or("auto"));
    let url = url.trim().to_string();
    if !url.contains(':') && !url.contains('/') {
        return Err("仓库地址看起来不完整".into());
    }
    let name = repo_name_from_url(&url).ok_or_else(|| "无法从地址中解析出仓库名".to_string())?;
    let parent = std::path::Path::new(&parent_dir);
    if !parent.is_dir() {
        return Err("目标文件夹不存在".into());
    }
    let dest = parent.join(&name);
    if dest.exists() {
        return Err(format!("目标文件夹已存在：{}", dest.display()));
    }

    let (_tag, forward, stage) = progress_hooks(Some(&on_progress));
    let args = ["clone", "--progress", &url, &name];
    let (out, used_proxy) = run_net_op(
        &app,
        parent_dir.trim(),
        &args,
        Some(&forward),
        Some(&url),
        NET_TIMEOUT,
        false,
        Some(&stage),
        mode,
    );
    if !out.success {
        // 进入前已确认目标目录不存在，失败后的残留半成品目录是本次尝试产生的，清掉以便重试
        let _ = std::fs::remove_dir_all(&dest);
        let message = if out.stderr.contains(git::CANCELLED_MSG) {
            "克隆已取消".to_string()
        } else if out.stderr.contains("未找到可用代理") {
            "克隆失败：代理模式未找到可用代理，请确认代理软件已运行，或在设置中手动填写代理地址".to_string()
        } else if used_proxy.is_none() && git::is_network_error(&out) {
            "克隆失败：无法连接远端，且未找到可用代理".to_string()
        } else if out.stderr.contains("could not read Username")
            || out.stderr.contains("Authentication failed")
        {
            "克隆失败：这是私有仓库，请确认已在本机 Git 凭证管理器中登录".to_string()
        } else if out.stderr.contains("Repository not found") || out.stderr.contains("not found") {
            "克隆失败：仓库不存在或无权访问".to_string()
        } else {
            "克隆失败".to_string()
        };
        return Err(format!("{message}\n{}", out.stderr.trim()));
    }

    let dest_str = dest.to_string_lossy().to_string();
    let mut cfg = store::load(&app);
    if !cfg.repos.iter().any(|r| r.path == dest_str) {
        cfg.repos.push(store::RepoEntry {
            name: name.clone(),
            alias: String::new(),
            path: dest_str.clone(),
            added_at: store::now_secs(),
            last_used: store::now_secs(),
            test_cmd: String::new(),
            build_cmd: String::new(),
            dev_cmd: String::new(),
            install_cmd: String::new(),
            pinned_cmds: Vec::new(),
            auto_sync_mins: 0,
            daily_time: String::new(),
            auto_pull: false,
            last_auto_check: 0,
            last_auto_result: String::new(),
            pinned: false,
        });
    }
    store::save(&app, &cfg)?;
    Ok(CloneOutcome {
        path: dest_str,
        name,
        repos: cfg.repos,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_name_from_url_works() {
        assert_eq!(
            repo_name_from_url("https://github.com/user/hello-world.git"),
            Some("hello-world".into())
        );
        assert_eq!(
            repo_name_from_url("https://gitee.com/user/hello-world"),
            Some("hello-world".into())
        );
        assert_eq!(
            repo_name_from_url("git@github.com:user/repo.git"),
            Some("repo".into())
        );
        // 结尾带斜杠时去掉斜杠后取最后一段（是否为合法仓库交给 clone 结果判断）
        assert_eq!(
            repo_name_from_url("https://github.com/user/"),
            Some("user".into())
        );
    }
}
