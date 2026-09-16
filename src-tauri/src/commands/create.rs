//! 云端新建仓库（GitHub / Gitee API）。

use crate::{git, store};
use serde::Serialize;
use std::time::Duration;

use super::net::{identity_config, run_local, run_net_op, NetMode};
use super::{LOCAL_TIMEOUT, NET_TIMEOUT};

#[derive(Serialize, Clone)]
pub struct CreateOutcome {
    pub path: String,
    pub name: String,
    pub repos: Vec<store::RepoEntry>,
    /// 面向用户的结果说明（可能包含部分成功信息）
    pub message: String,
}

fn valid_repo_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
        && !name.starts_with('.')
}

/// 在 GitHub 上创建远端仓库，返回 full_name（owner/repo）
fn api_create_github(
    token: &str,
    name: &str,
    private: bool,
    description: &str,
) -> Result<String, String> {
    let mut body = ureq::json!({ "name": name, "private": private, "auto_init": false });
    if !description.is_empty() {
        body["description"] = ureq::json!(description);
    }
    match ureq::post("https://api.github.com/user/repos")
        .set("Authorization", &format!("Bearer {token}"))
        .set("User-Agent", "qingbird-git")
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(30))
        .send_json(body)
    {
        Ok(resp) => {
            let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
            v["full_name"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "GitHub 返回数据异常".into())
        }
        Err(ureq::Error::Status(422, _)) => {
            Err("GitHub 上已存在同名仓库（或名称不合法），换个名字试试".into())
        }
        Err(ureq::Error::Status(401, _)) => Err("GitHub 令牌无效或已过期，请在设置里检查".into()),
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            Err(format!(
                "GitHub 创建仓库失败（HTTP {code}）：{}",
                body.chars().take(200).collect::<String>()
            ))
        }
        Err(e) => Err(format!("无法连接 GitHub API：{e}")),
    }
}

/// 在 Gitee 上创建远端仓库，返回 full_name（owner/repo）
fn api_create_gitee(
    token: &str,
    name: &str,
    private: bool,
    description: &str,
) -> Result<String, String> {
    let mut form: Vec<(&str, &str)> = vec![
        ("access_token", token),
        ("name", name),
        ("private", if private { "true" } else { "false" }),
        ("auto_init", "false"),
    ];
    if !description.is_empty() {
        form.push(("description", description));
    }
    match ureq::post("https://gitee.com/api/v5/user/repos")
        .set("User-Agent", "qingbird-git")
        .timeout(Duration::from_secs(30))
        .send_form(&form)
    {
        Ok(resp) => {
            let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
            v["full_name"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "Gitee 返回数据异常".into())
        }
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            let hint = if code == 401 || body.contains("access_token") {
                "Gitee 令牌无效或已过期，请在设置里检查"
            } else if body.contains("exist") || body.contains("taken") {
                "Gitee 上已存在同名仓库（或名称不合法），换个名字试试"
            } else {
                "Gitee 创建仓库失败"
            };
            Err(format!("{hint}（HTTP {code}）"))
        }
        Err(e) => Err(format!("无法连接 Gitee API：{e}")),
    }
}

/// 一步创建新仓库：本地 init + 初始提交，可选同时在 GitHub/Gitee 创建远端并推送
#[tauri::command(async)]
pub fn create_repo(
    app: tauri::AppHandle,
    name: String,
    parent_dir: String,
    platform: String,
    private: bool,
    description: String,
) -> Result<CreateOutcome, String> {
    let name = name.trim().to_string();
    if !valid_repo_name(&name) {
        return Err("仓库名只能包含字母、数字、点、横线和下划线".into());
    }
    let parent = std::path::Path::new(&parent_dir);
    if !parent.is_dir() {
        return Err("目标文件夹不存在".into());
    }
    let dest = parent.join(&name);
    if dest.exists() {
        return Err(format!("目标文件夹已存在：{}", dest.display()));
    }

    let cfg = store::load(&app);
    let desc = description.trim();

    // 1. 先在云端创建远端（失败则整体中止，不留下半成品）
    let mut origin = String::new();
    if platform != "none" {
        let remote_full_name = match platform.as_str() {
            "github" => {
                let token = cfg.settings.github_token.trim();
                if token.is_empty() {
                    return Err("尚未配置 GitHub 访问令牌，请在「设置 → 令牌认证」里填写".into());
                }
                api_create_github(token, &name, private, desc)?
            }
            "gitee" => {
                let token = cfg.settings.gitee_token.trim();
                if token.is_empty() {
                    return Err("尚未配置 Gitee 私人令牌，请在「设置 → 令牌认证」里填写".into());
                }
                api_create_gitee(token, &name, private, desc)?
            }
            _ => return Err("不支持的平台".into()),
        };
        let host = if platform == "github" {
            "https://github.com"
        } else {
            "https://gitee.com"
        };
        origin = format!("{host}/{remote_full_name}.git");
    }

    // 2. 本地初始化 + 初始提交
    let init = git::run_git_auth(
        parent_dir.trim(),
        &["init", "-b", "main", &name],
        None,
        LOCAL_TIMEOUT,
        None,
        &[],
        None,
    );
    if !init.success {
        // 兼容不支持 -b 的旧版 git
        let fallback = git::run_git_auth(
            parent_dir.trim(),
            &["init", &name],
            None,
            LOCAL_TIMEOUT,
            None,
            &[],
            None,
        );
        if !fallback.success {
            return Err(format!("git init 失败：{}", fallback.stderr.trim()));
        }
    }
    std::fs::write(
        dest.join("README.md"),
        format!(
            "# {name}\n\n{}\n",
            if desc.is_empty() { name.as_str() } else { desc }
        ),
    )
    .map_err(|e| format!("写入 README 失败：{e}"))?;

    let dest_str = dest.to_string_lossy().to_string();
    let cargs = identity_config(&cfg);
    let mut argv: Vec<&str> = cargs.iter().map(|s| s.as_str()).collect();
    argv.extend(["add", "-A"]);
    run_local(&dest_str, &argv);
    let mut cargv: Vec<&str> = cargs.iter().map(|s| s.as_str()).collect();
    cargv.extend(["commit", "-m", "init: 初始提交"]);
    let commit = run_local(&dest_str, &cargv);
    if !commit.success {
        return Err(format!(
            "仓库已创建，但初始提交失败（可能未配置 Git 身份，可在设置里填写）：{}",
            commit.stderr.trim()
        ));
    }

    // 3. 关联远端并推送
    let mut message = "本地仓库创建成功".to_string();
    if !origin.is_empty() {
        let add = run_local(&dest_str, &["remote", "add", "origin", &origin]);
        if !add.success {
            return Err(format!("仓库已创建，但关联远端失败：{}", add.stderr.trim()));
        }
        let push_args: Vec<&str> = vec!["push", "--progress", "--set-upstream", "origin", "main"];
        let (push_out, used_proxy) = run_net_op(
            &app,
            &dest_str,
            &push_args,
            None,
            Some(&origin),
            NET_TIMEOUT,
            true,
            None,
            NetMode::Auto,
        );
        if !push_out.success {
            message.push_str("，远端已创建，但首次推送失败：");
            message.push_str(push_out.stderr.trim());
        } else if used_proxy.is_some() {
            message.push_str("，远端已创建并完成首次推送（经代理）");
        } else {
            message.push_str("，远端已创建并完成首次推送");
        }
    }

    // 4. 加入管理列表
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
            auto_sync_mins: 0,
            daily_time: String::new(),
            auto_pull: false,
            last_auto_check: 0,
            last_auto_result: String::new(),
            pinned: false,
        });
    }
    store::save(&app, &cfg)?;
    Ok(CreateOutcome {
        path: dest_str,
        name,
        repos: cfg.repos,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_name_validation() {
        assert!(valid_repo_name("hello-world"));
        assert!(valid_repo_name("my_repo.1"));
        assert!(!valid_repo_name(""));
        assert!(!valid_repo_name("a b"));
        assert!(!valid_repo_name(".hidden"));
        assert!(!valid_repo_name("中文"));
    }
}
