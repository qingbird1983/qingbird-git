use crate::{git, proxy, store};
use serde::Serialize;
use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::Manager;

const LOCAL_TIMEOUT: Duration = Duration::from_secs(10);
const NET_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Serialize, Clone, Default)]
pub struct RepoStatus {
    pub is_repo: bool,
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub has_upstream: bool,
    pub remote_url: String,
}

/// 拉取/克隆时推送给前端的进度事件
#[derive(Serialize, Clone)]
pub struct ProgressEvent {
    pub stage: String,
    pub detail: String,
    pub percent: Option<f32>,
}

/// 拉取成功后的更新内容：新拉下来的提交与文件变更摘要
#[derive(Serialize, Clone)]
pub struct UpdateChanges {
    pub commits: Vec<String>,
    pub diff_stat: String,
}

#[derive(Serialize, Clone)]
pub struct OpResult {
    pub ok: bool,
    /// 面向用户的一句话结论
    pub message: String,
    /// git 原始输出（日志区展示）
    pub output: String,
    /// 走代理成功时记录所用代理，None 表示直连
    pub used_proxy: Option<String>,
    pub status: Option<RepoStatus>,
    /// pull 时本地有未提交变更，提示用户可选择 stash
    pub need_stash: bool,
    /// 拉取成功时的更新内容（新提交 + 文件变更摘要）
    pub changes: Option<UpdateChanges>,
}

impl OpResult {
    fn err(message: String, output: String) -> Self {
        OpResult { ok: false, message, output, used_proxy: None, status: None, need_stash: false, changes: None }
    }
}

/// 解析 git 的 stderr 进度行，转换成可展示的进度事件
pub fn parse_progress(line: &str) -> Option<ProgressEvent> {
    let line = line.trim();
    let pct = |s: &str| -> Option<f32> {
        let idx = s.find('%')?;
        let start = s[..idx].rfind(|c: char| !c.is_ascii_digit() && c != '.')? + 1;
        s[start..idx].parse().ok()
    };
    for (kw, stage) in [
        ("Receiving objects", "接收对象"),
        ("Resolving deltas", "解析增量"),
        ("Updating files", "检出文件"),
        ("Counting objects", "统计对象"),
        ("Compressing objects", "压缩对象"),
        ("Enumerating objects", "枚举对象"),
    ] {
        if let Some(rest) = line.strip_prefix(kw) {
            return Some(ProgressEvent {
                stage: stage.into(),
                detail: line.to_string(),
                percent: pct(rest),
            });
        }
    }
    if line.starts_with("Cloning into") {
        return Some(ProgressEvent { stage: "开始克隆".into(), detail: line.to_string(), percent: None });
    }
    if line.starts_with("From ") || line.starts_with("FETCH_HEAD") {
        return Some(ProgressEvent { stage: "获取远端引用".into(), detail: line.to_string(), percent: None });
    }
    if line.starts_with("Fast-forward") {
        return Some(ProgressEvent { stage: "合并更新".into(), detail: line.to_string(), percent: None });
    }
    None
}

fn valid_repo(path: &str) -> bool {
    std::path::Path::new(path).join(".git").exists()
}

fn repo_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(BASE64_CHARS[(n >> 18) as usize & 63] as char);
        out.push(BASE64_CHARS[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { BASE64_CHARS[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { BASE64_CHARS[n as usize & 63] as char } else { '=' });
    }
    out
}

/// 识别远端地址所属平台
fn host_of(url: &str) -> Option<&'static str> {
    let u = url.to_lowercase();
    if u.contains("github.com") {
        Some("github")
    } else if u.contains("gitee.com") {
        Some("gitee")
    } else {
        None
    }
}

fn is_ssh_url(url: &str) -> bool {
    url.starts_with("git@") || url.starts_with("ssh://")
}

/// 按远端平台生成本次网络操作的认证配置：
/// - https + 令牌：注入 http.extraheader 基本认证（GitHub 用 x-access-token，Gitee 用 oauth2）
/// - ssh + 私钥：注入 GIT_SSH_COMMAND 指定私钥
fn build_net_auth(cfg: &store::AppConfig, remote_url: &str) -> (Vec<String>, Option<String>) {
    let mut config = Vec::new();
    let mut ssh = None;
    if let Some(host) = host_of(remote_url) {
        let token = match host {
            "github" => cfg.settings.github_token.trim(),
            "gitee" => cfg.settings.gitee_token.trim(),
            _ => "",
        };
        if !token.is_empty() && !is_ssh_url(remote_url) {
            let user = if host == "github" { "x-access-token" } else { "oauth2" };
            let b64 = base64_encode(format!("{user}:{token}").as_bytes());
            config.push(format!("http.extraheader=AUTHORIZATION: basic {b64}"));
        }
    }
    let key = cfg.settings.ssh_key_path.trim();
    if !key.is_empty() && is_ssh_url(remote_url) {
        ssh = Some(format!(
            "ssh -i {key} -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new"
        ));
    }
    (config, ssh)
}

/// 读取仓库 origin 远端地址（读不到则返回空串）
fn remote_url(repo_path: &str) -> String {
    run_local(repo_path, &["remote", "get-url", "origin"]).stdout.trim().to_string()
}

/// 提交时使用的 Git 身份（-c 项；设置里未填则交给全局配置）
fn identity_config(cfg: &store::AppConfig) -> Vec<String> {
    let mut c = Vec::new();
    if !cfg.settings.git_name.trim().is_empty() {
        c.push(format!("user.name={}", cfg.settings.git_name.trim()));
    }
    if !cfg.settings.git_email.trim().is_empty() {
        c.push(format!("user.email={}", cfg.settings.git_email.trim()));
    }
    c
}

/// 执行本地 git 命令（不涉及网络，不走代理回退）
fn run_local(repo_path: &str, args: &[&str]) -> git::GitOutput {
    git::run_git(repo_path, args, None, LOCAL_TIMEOUT)
}

/// 网络型 git 操作的统一入口：先直连，失败且判定为网络问题时自动探测代理重试。
/// 代理只注入到当次 git 子进程的环境变量，结束即失效，不改系统代理。
/// auth_url 为参与认证判定的远端地址；None 时取 repo 的 origin。
/// git_config / ssh 为按平台令牌认证配置（见 build_net_auth）。
fn run_net_op(
    app: &tauri::AppHandle,
    repo_path: &str,
    args: &[&str],
    on_line: Option<&dyn Fn(&str)>,
    auth_url: Option<&str>,
) -> (git::GitOutput, Option<String>) {
    let cfg = store::load(&app);
    let url = match auth_url {
        Some(u) => u.to_string(),
        None => remote_url(repo_path),
    };
    let (git_config, ssh) = build_net_auth(&cfg, &url);
    let direct = git::run_git_auth(repo_path, args, None, NET_TIMEOUT, on_line, &git_config, ssh.as_deref());
    if direct.success || !git::is_network_error(&direct) {
        return (direct, None);
    }

    let candidates = proxy::gather_candidates(&cfg.settings, &cfg.last_good_proxy);
    for candidate in candidates {
        if !proxy::probe(&candidate) {
            continue;
        }
        let url = proxy::to_git_proxy_url(&candidate);
        let out = git::run_git_auth(repo_path, args, Some(&url), NET_TIMEOUT, on_line, &git_config, ssh.as_deref());
        if out.success {
            let mut cfg = store::load(&app);
            cfg.last_good_proxy = Some(candidate);
            let _ = store::save(&app, &cfg);
            return (out, Some(url));
        }
    }
    (direct, None)
}

fn read_status(repo_path: &str) -> RepoStatus {
    let mut st = RepoStatus { is_repo: valid_repo(repo_path), ..Default::default() };
    if !st.is_repo {
        return st;
    }
    st.branch = run_local(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .stdout
        .trim()
        .to_string();

    let upstream = run_local(repo_path, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]);
    st.has_upstream = upstream.success;
    if upstream.success {
        let counts = run_local(repo_path, &["rev-list", "--left-right", "--count", "HEAD...@{u}"]);
        if counts.success {
            let line = counts.stdout.trim().to_string();
            if let [ahead, behind] = line.split_whitespace().collect::<Vec<_>>()[..] {
                st.ahead = ahead.parse().unwrap_or(0);
                st.behind = behind.parse().unwrap_or(0);
            }
        }
    }
    st.dirty = !run_local(repo_path, &["status", "--porcelain"]).stdout.trim().is_empty();
    st.remote_url = run_local(repo_path, &["remote", "get-url", "origin"])
        .stdout
        .trim()
        .to_string();
    st
}

fn touch_repo(app: &tauri::AppHandle, path: &str) {
    let mut cfg = store::load(&app);
    if let Some(entry) = cfg.repos.iter_mut().find(|r| r.path == path) {
        entry.last_used = store::now_secs();
    }
    let _ = store::save(&app, &cfg);
}

// ---------- 仓库管理 ----------

#[tauri::command]
pub fn list_repos(app: tauri::AppHandle) -> Vec<store::RepoEntry> {
    let cfg = store::load(&app);
    cfg.repos
}

#[tauri::command]
pub fn add_repo(app: tauri::AppHandle, path: String) -> Result<Vec<store::RepoEntry>, String> {
    let path = path.trim().to_string();
    if !valid_repo(&path) {
        return Err("所选文件夹不是 git 仓库（未找到 .git）".to_string());
    }
    let mut cfg = store::load(&app);
    if !cfg.repos.iter().any(|r| r.path == path) {
        cfg.repos.push(store::RepoEntry {
            name: repo_name(&path),
            alias: String::new(),
            path: path.clone(),
            added_at: store::now_secs(),
            last_used: store::now_secs(),
        });
    }
    store::save(&app, &cfg)?;
    Ok(cfg.repos)
}

#[tauri::command]
pub fn remove_repo(app: tauri::AppHandle, path: String) -> Vec<store::RepoEntry> {
    let mut cfg = store::load(&app);
    cfg.repos.retain(|r| r.path != path);
    let _ = store::save(&app, &cfg);
    cfg.repos
}

/// 设置本地自定义显示名（如中文名）；alias 传空串恢复原始仓库名
#[tauri::command]
pub fn rename_repo(app: tauri::AppHandle, path: String, alias: String) -> Vec<store::RepoEntry> {
    let mut cfg = store::load(&app);
    if let Some(e) = cfg.repos.iter_mut().find(|r| r.path == path) {
        e.alias = alias.trim().to_string();
    }
    let _ = store::save(&app, &cfg);
    cfg.repos
}

// ---------- 初始化已有文件夹 ----------

#[derive(Serialize, Clone)]
pub struct DirItem {
    pub name: String,
    pub is_dir: bool,
}

/// 列出文件夹顶层内容（供初始化时勾选排除项）
#[tauri::command]
pub fn list_dir_items(path: String) -> Vec<DirItem> {
    let mut items: Vec<DirItem> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&path) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name == ".git" {
                continue;
            }
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            items.push(DirItem { name, is_dir });
        }
    }
    items.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    items
}

/// 把非 git 文件夹初始化为仓库：git init → 写入排除项到 .gitignore → 初始提交 → 加入管理列表
#[tauri::command]
pub fn init_repo(
    app: tauri::AppHandle,
    path: String,
    excludes: Vec<String>,
) -> Result<store::RepoEntry, String> {
    let path = path.trim().to_string();
    let dir = std::path::Path::new(&path);
    if !dir.is_dir() {
        return Err("文件夹不存在".into());
    }
    if valid_repo(&path) {
        return Err("该文件夹已经是 git 仓库".into());
    }

    let init = git::run_git_auth(&path, &["init", "-b", "main"], None, LOCAL_TIMEOUT, None, &[], None);
    if !init.success {
        let fallback = git::run_git_auth(&path, &["init"], None, LOCAL_TIMEOUT, None, &[], None);
        if !fallback.success {
            return Err(format!("git init 失败：{}", fallback.stderr.trim()));
        }
    }

    // 把勾选的排除项追加到 .gitignore
    if !excludes.is_empty() {
        let gi = dir.join(".gitignore");
        let mut content = String::new();
        if gi.exists() {
            content = std::fs::read_to_string(&gi).unwrap_or_default();
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
        }
        content.push_str("# 由青鸟 Git 添加的排除项\n");
        for e in &excludes {
            content.push_str(e.trim());
            content.push('\n');
        }
        std::fs::write(&gi, content).map_err(|e| format!("写入 .gitignore 失败：{e}"))?;
    }

    let cfg0 = store::load(&app);
    let identity = identity_config(&cfg0);
    let mut argv: Vec<&str> = identity.iter().map(|s| s.as_str()).collect();
    argv.extend(["add", "-A"]);
    run_local(&path, &argv);

    let mut cargv: Vec<&str> = identity.iter().map(|s| s.as_str()).collect();
    cargv.extend(["commit", "-m", "init: 初始提交"]);
    let commit = run_local(&path, &cargv);
    if !commit.success {
        return Err(format!(
            "仓库已初始化，但初始提交失败（可能未配置 Git 身份，可在设置里填写）：{}",
            commit.stderr.trim()
        ));
    }

    let name = repo_name(&path);
    let mut cfg = store::load(&app);
    let entry = store::RepoEntry {
        name,
        path: path.clone(),
        alias: String::new(),
        added_at: store::now_secs(),
        last_used: store::now_secs(),
    };
    if !cfg.repos.iter().any(|r| r.path == path) {
        cfg.repos.push(entry.clone());
        store::save(&app, &cfg)?;
    }
    Ok(entry)
}

// ---------- 状态与三键操作 ----------

#[tauri::command]
pub fn repo_status(path: String) -> RepoStatus {
    read_status(&path)
}

#[tauri::command]
pub fn check_update(app: tauri::AppHandle, path: String) -> OpResult {
    if !valid_repo(&path) {
        return OpResult::err("该文件夹不是 git 仓库".into(), String::new());
    }
    let (fetch_out, used_proxy) = run_net_op(&app, &path, &["fetch", "--all", "--prune"], None, None);
    let status = read_status(&path);
    touch_repo(&app, &path);

    if !fetch_out.success {
        let mut msg = "检查更新失败：无法连接远端".to_string();
        if used_proxy.is_none() && git::is_network_error(&fetch_out) {
            msg.push_str("，且未找到可用代理");
        }
        return OpResult {
            ok: false,
            message: msg,
            output: combined(&fetch_out),
            used_proxy,
            status: Some(status),
            need_stash: false,
            changes: None,
        };
    }

    let message = if !status.has_upstream {
        "当前分支没有设置上游分支，无法对比云端更新".to_string()
    } else if status.behind > 0 && status.ahead > 0 {
        format!("云端有 {0} 个新提交，同时本地有 {1} 个未推送提交", status.behind, status.ahead)
    } else if status.behind > 0 {
        format!("云端有 {} 个新提交，可以拉取", status.behind)
    } else if status.ahead > 0 {
        format!("已是最新，本地领先 {} 个提交待推送", status.ahead)
    } else {
        "已是最新，无需同步".to_string()
    };
    OpResult { ok: true, message, output: combined(&fetch_out), used_proxy, status: Some(status), need_stash: false, changes: None }
}

#[tauri::command]
pub fn pull_repo(
    app: tauri::AppHandle,
    path: String,
    stash: bool,
    on_progress: Channel<ProgressEvent>,
) -> OpResult {
    if !valid_repo(&path) {
        return OpResult::err("该文件夹不是 git 仓库".into(), String::new());
    }
    let forward = |line: &str| {
        if let Some(ev) = parse_progress(line) {
            let _ = on_progress.send(ev);
        }
    };
    let status_before = read_status(&path);
    if status_before.dirty && !stash {
        return OpResult {
            ok: false,
            message: "本地有未提交的变更，直接拉取可能冲突".to_string(),
            output: String::new(),
            used_proxy: None,
            status: Some(status_before),
            need_stash: true,
            changes: None,
        };
    }

    let old_head = run_local(&path, &["rev-parse", "HEAD"]).stdout.trim().to_string();

    let mut log = String::new();
    let mut stashed = false;
    if stash && status_before.dirty {
        let s = run_local(&path, &["stash", "push", "-u", "-m", "qingbird-git auto stash"]);
        log.push_str(&combined(&s));
        if !s.success {
            return OpResult::err("暂存本地变更失败，已取消拉取".into(), log);
        }
        stashed = true;
    }

    let (pull_out, used_proxy) = run_net_op(&app, &path, &["pull", "--ff-only", "--progress"], Some(&forward), None);
    log.push_str(&combined(&pull_out));
    let mut status = read_status(&path);
    touch_repo(&app, &path);

    if stashed {
        let pop = run_local(&path, &["stash", "pop"]);
        log.push_str(&combined(&pop));
        if !pop.success {
            return OpResult {
                ok: false,
                message: "拉取完成，但恢复暂存变更时冲突，请在终端执行 git stash pop 处理".into(),
                output: log,
                used_proxy: used_proxy.clone(),
                status: Some(status),
                need_stash: false,
                changes: None,
            };
        }
    }

    if !pull_out.success {
        let message = if pull_out.stderr.contains("no tracking information") {
            "当前分支没有上游分支，无法拉取".to_string()
        } else if pull_out.stderr.contains("Not possible to fast-forward") || pull_out.stderr.contains("divergent") {
            "本地与云端历史分叉，ff 拉取失败，请手动处理".to_string()
        } else if used_proxy.is_none() && git::is_network_error(&pull_out) {
            "拉取失败：无法连接远端，且未找到可用代理".to_string()
        } else {
            "拉取失败".to_string()
        };
        return OpResult { ok: false, message, output: log, used_proxy, status: Some(status), need_stash: false, changes: None };
    }

    status = read_status(&path);
    let changes = pull_changes(&path, &old_head);
    let message = if used_proxy.is_some() {
        "拉取成功（已自动通过代理完成）".to_string()
    } else {
        "拉取成功".to_string()
    };
    OpResult { ok: true, message, output: log, used_proxy, status: Some(status), need_stash: false, changes }
}

/// 对比拉取前的 HEAD，提取本次拉下来的提交列表与文件变更摘要
fn pull_changes(path: &str, old_head: &str) -> Option<UpdateChanges> {
    if old_head.is_empty() {
        return None;
    }
    let range = format!("{old_head}..HEAD");
    let mut changes = UpdateChanges { commits: Vec::new(), diff_stat: String::new() };

    let log = run_local(path, &["log", "--oneline", "--no-decorate", &range]);
    if log.success {
        changes.commits = log
            .stdout
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
    }
    let stat = run_local(path, &["diff", "--stat", &range]);
    if stat.success {
        changes.diff_stat = stat.stdout.trim_end().to_string();
    }
    // 完全没有新内容（Already up to date）时不返回更新卡片
    if changes.commits.is_empty() && changes.diff_stat.is_empty() {
        return None;
    }
    Some(changes)
}

#[tauri::command]
pub fn push_repo(app: tauri::AppHandle, path: String) -> OpResult {
    if !valid_repo(&path) {
        return OpResult::err("该文件夹不是 git 仓库".into(), String::new());
    }
    do_push(&app, &path)
}

/// 推送的核心流程（push_repo 与 commit_push 共用）
fn do_push(app: &tauri::AppHandle, path: &str) -> OpResult {
    let status = read_status(path);
    if status.ahead == 0 && status.has_upstream {
        return OpResult {
            ok: true,
            message: "没有待推送的提交".to_string(),
            output: String::new(),
            used_proxy: None,
            status: Some(status),
            need_stash: false,
            changes: None,
        };
    }
    if !status.has_upstream {
        let remotes = run_local(path, &["remote"]);
        if remotes.stdout.trim().is_empty() {
            return OpResult::err("该仓库没有配置远端仓库，无法推送".into(), combined(&remotes));
        }
    }

    let args: Vec<&str> = if status.has_upstream {
        vec!["push"]
    } else {
        vec!["push", "--set-upstream", "origin", "HEAD"]
    };
    let (push_out, used_proxy) = run_net_op(app, path, &args, None, None);
    let status = read_status(path);
    touch_repo(app, path);

    if !push_out.success {
        let message = if push_out.stderr.contains("fetch first") || push_out.stderr.contains("rejected") {
            "推送被拒绝：云端有新提交，请先拉取".to_string()
        } else if used_proxy.is_none() && git::is_network_error(&push_out) {
            "推送失败：无法连接远端，且未找到可用代理".to_string()
        } else if push_out.stderr.contains("could not read Username") || push_out.stderr.contains("Authentication failed") {
            "推送失败：认证未通过，可在设置里填写对应平台的访问令牌".to_string()
        } else {
            "推送失败".to_string()
        };
        return OpResult { ok: false, message, output: combined(&push_out), used_proxy, status: Some(status), need_stash: false, changes: None };
    }

    let message = if used_proxy.is_some() {
        "推送成功（已自动通过代理完成）".to_string()
    } else {
        "推送成功".to_string()
    };
    OpResult { ok: true, message, output: combined(&push_out), used_proxy, status: Some(status), need_stash: false, changes: None }
}

// ---------- 提交 ----------

#[derive(Serialize, Clone)]
pub struct FileChange {
    /// git status 代码：M 修改 / A 新增 / D 删除 / ?? 未跟踪 / R 重命名 等
    pub code: String,
    pub path: String,
}

#[tauri::command]
pub fn list_changes(path: String) -> Vec<FileChange> {
    if !valid_repo(&path) {
        return Vec::new();
    }
    run_local(&path, &["status", "--porcelain"])
        .stdout
        .lines()
        .filter(|l| l.len() >= 3)
        .map(|l| FileChange {
            code: l[..2].trim().to_string(),
            path: l[3..].trim().to_string(),
        })
        .collect()
}

#[derive(Serialize, Clone)]
pub struct CommitInfo {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub summary: String,
}

#[tauri::command]
pub fn recent_commits(path: String) -> Vec<CommitInfo> {
    if !valid_repo(&path) {
        return Vec::new();
    }
    run_local(
        &path,
        &[
            "log",
            "-n",
            "20",
            "--date=format-local:%Y-%m-%d %H:%M",
            "--pretty=format:%h%x09%an%x09%ad%x09%s",
        ],
    )
    .stdout
    .lines()
    .filter_map(|l| {
        let mut parts = l.splitn(4, '\t');
        Some(CommitInfo {
            hash: parts.next()?.to_string(),
            author: parts.next()?.to_string(),
            date: parts.next()?.to_string(),
            summary: parts.next().unwrap_or("").to_string(),
        })
    })
    .collect()
}

/// 项目语言构成（类似 GitHub 语言条）：按扩展名统计代码文件字节占比。
/// 用 `git ls-files -co --exclude-standard` 取文件列表，自动跳过 .gitignore 忽略的文件。
#[derive(Serialize, Clone)]
pub struct LangStat {
    pub language: String,
    pub percent: f32,
}

/// 扩展名 → 语言名（对齐 GitHub Linguist 的常见分类；配置/数据类文件不计入）
fn lang_of_ext(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "rs" => "Rust",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "rb" => "Ruby",
        "php" => "PHP",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "C++",
        "cs" => "C#",
        "m" | "mm" => "Objective-C",
        "scala" => "Scala",
        "dart" => "Dart",
        "lua" => "Lua",
        "html" | "htm" => "HTML",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "css" => "CSS",
        "scss" | "sass" => "SCSS",
        "less" => "Less",
        "sh" | "bash" | "zsh" => "Shell",
        "ps1" | "psm1" => "PowerShell",
        "sql" => "SQL",
        "zig" => "Zig",
        "sol" => "Solidity",
        _ => return None,
    })
}

#[tauri::command]
pub fn repo_languages(path: String) -> Vec<LangStat> {
    if !valid_repo(&path) {
        return Vec::new();
    }
    let ls = run_local(&path, &["ls-files", "-co", "--exclude-standard"]);
    if !ls.success {
        return Vec::new();
    }
    let mut totals: HashMap<&str, u64> = HashMap::new();
    let mut total: u64 = 0;
    for f in ls.stdout.lines() {
        let ext = std::path::Path::new(f)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        let Some(lang) = lang_of_ext(&ext) else { continue };
        let len = std::fs::metadata(std::path::Path::new(&path).join(f))
            .map(|m| m.len())
            .unwrap_or(0);
        *totals.entry(lang).or_insert(0) += len;
        total += len;
    }
    if total == 0 {
        return Vec::new();
    }
    let mut stats: Vec<LangStat> = totals
        .into_iter()
        .map(|(lang, bytes)| LangStat {
            language: lang.to_string(),
            percent: (bytes as f32) / (total as f32) * 100.0,
        })
        .collect();
    stats.sort_by(|a, b| b.percent.total_cmp(&a.percent));
    // 只保留前 6 项，其余合并为"其他"
    if stats.len() > 6 {
        let rest: f32 = stats[6..].iter().map(|s| s.percent).sum();
        stats.truncate(6);
        stats.push(LangStat { language: "其他".into(), percent: rest });
    }
    stats
}

/// 一键提交并推送：add -A → commit（自动带 Git 身份）→ push
#[tauri::command]
pub fn commit_push(app: tauri::AppHandle, path: String, message: String) -> OpResult {
    if !valid_repo(&path) {
        return OpResult::err("该文件夹不是 git 仓库".into(), String::new());
    }
    let msg = message.trim();
    if msg.is_empty() {
        return OpResult::err("请先填写提交说明".into(), String::new());
    }
    let st = read_status(&path);
    if !st.dirty {
        return OpResult {
            ok: true,
            message: "没有可提交的变更，直接推送即可".to_string(),
            output: String::new(),
            used_proxy: None,
            status: Some(st),
            need_stash: false,
            changes: None,
        };
    }

    let cfg = store::load(&app);
    let add = run_local(&path, &["add", "-A"]);
    if !add.success {
        return OpResult::err("添加变更文件失败".into(), combined(&add));
    }

    let cargs = identity_config(&cfg);
    let mut argv: Vec<&str> = cargs.iter().map(|s| s.as_str()).collect();
    argv.extend(["commit", "-m", msg]);
    let commit = run_local(&path, &argv);
    if !commit.success {
        let message = if commit.stderr.contains("Please tell me who you are")
            || commit.stderr.contains("unable to auto-detect email")
        {
            "提交失败：还没有配置 Git 身份，请在「设置 → Git 身份」里填写姓名和邮箱".to_string()
        } else {
            "提交失败".to_string()
        };
        return OpResult::err(message, combined(&commit));
    }

    let mut result = do_push(&app, &path);
    result.output = format!("{}{}", combined(&commit), result.output);
    if result.ok {
        result.message = format!("已提交并推送：「{msg}」");
    }
    result
}

// ---------- 新建仓库 ----------

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
fn api_create_github(token: &str, name: &str, private: bool, description: &str) -> Result<String, String> {
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
            Err(format!("GitHub 创建仓库失败（HTTP {code}）：{}", body.chars().take(200).collect::<String>()))
        }
        Err(e) => Err(format!("无法连接 GitHub API：{e}")),
    }
}

/// 在 Gitee 上创建远端仓库，返回 full_name（owner/repo）
fn api_create_gitee(token: &str, name: &str, private: bool, description: &str) -> Result<String, String> {
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
#[tauri::command]
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
        let host = if platform == "github" { "https://github.com" } else { "https://gitee.com" };
        origin = format!("{host}/{remote_full_name}.git");
    }

    // 2. 本地初始化 + 初始提交
    let init = git::run_git_auth(parent_dir.trim(), &["init", "-b", "main", &name], None, LOCAL_TIMEOUT, None, &[], None);
    if !init.success {
        // 兼容不支持 -b 的旧版 git
        let fallback = git::run_git_auth(parent_dir.trim(), &["init", &name], None, LOCAL_TIMEOUT, None, &[], None);
        if !fallback.success {
            return Err(format!("git init 失败：{}", fallback.stderr.trim()));
        }
    }
    std::fs::write(
        dest.join("README.md"),
        format!("# {name}\n\n{}\n", if desc.is_empty() { name.as_str() } else { desc }),
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
        let push_args: Vec<&str> = vec!["push", "--set-upstream", "origin", "main"];
        let (push_out, used_proxy) = run_net_op(&app, &dest_str, &push_args, None, Some(&origin));
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
        });
    }
    store::save(&app, &cfg)?;
    Ok(CreateOutcome { path: dest_str, name, repos: cfg.repos, message })
}

// ---------- 设置 ----------

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

// ---------- 快捷打开 ----------

#[tauri::command]
pub fn open_in_folder(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).is_dir() {
        return Err("文件夹不存在".into());
    }
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_in_terminal(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).is_dir() {
        return Err("文件夹不存在".into());
    }
    // 优先 Windows Terminal，失败回退 PowerShell
    if Command::new("wt").arg("-d").arg(&path).spawn().is_ok() {
        return Ok(());
    }
    Command::new("cmd")
        .args(["/C", "start", "powershell", "-NoExit", "-Command"])
        .arg(format!("Set-Location -LiteralPath '{}'", path.replace('\'', "''")))
        .spawn()
        .map_err(|e| format!("无法打开终端：{e}"))?;
    Ok(())
}

#[tauri::command]
pub fn set_settings(app: tauri::AppHandle, manual_proxy: String) -> Result<(), String> {
    let mut cfg = store::load(&app);
    cfg.settings.manual_proxy = manual_proxy.trim().to_string();
    store::save(&app, &cfg)
}

#[tauri::command]
pub fn probe_proxy(addr: String) -> Result<bool, String> {
    let url = proxy::to_git_proxy_url(addr.trim());
    Ok(proxy::probe(&url))
}

fn combined(out: &git::GitOutput) -> String {
    // 进度刷新行（\r 重复输出）在进度条里已展示，日志里去掉避免刷屏
    const PROGRESS_NOISE: &[&str] = &[
        "Receiving objects", "Resolving deltas", "Updating files", "Counting objects",
        "Compressing objects", "Enumerating objects", "remote: Enumerating",
    ];
    let clean = |text: &str| -> String {
        text.split('\n')
            .filter(|line| {
                let l = line.trim();
                !l.is_empty() && !PROGRESS_NOISE.iter().any(|k| l.starts_with(k))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut s = String::new();
    let stdout = clean(&out.stdout);
    let stderr = clean(&out.stderr);
    if !stdout.is_empty() {
        s.push_str(&stdout);
        s.push('\n');
    }
    if !stderr.is_empty() {
        s.push_str(&stderr);
        s.push('\n');
    }
    s
}

// ---------- 克隆仓库 ----------

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

#[tauri::command]
pub fn clone_repo(
    app: tauri::AppHandle,
    url: String,
    parent_dir: String,
    on_progress: Channel<ProgressEvent>,
) -> Result<CloneOutcome, String> {
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

    let forward = |line: &str| {
        if let Some(ev) = parse_progress(line) {
            let _ = on_progress.send(ev);
        }
    };
    let args = ["clone", "--progress", &url, &name];
    let (out, used_proxy) = run_net_op(&app, parent_dir.trim(), &args, Some(&forward), Some(&url));
    if !out.success {
        let message = if used_proxy.is_none() && git::is_network_error(&out) {
            "克隆失败：无法连接远端，且未找到可用代理".to_string()
        } else if out.stderr.contains("could not read Username") || out.stderr.contains("Authentication failed") {
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
        });
    }
    store::save(&app, &cfg)?;
    Ok(CloneOutcome { path: dest_str, name, repos: cfg.repos })
}

// ---------- 一键扫描 ----------

#[derive(Serialize, Clone)]
pub struct ScanItem {
    pub path: String,
    pub status: RepoStatus,
}

#[tauri::command]
pub fn scan_all(app: tauri::AppHandle) -> Vec<ScanItem> {
    let cfg = store::load(&app);
    cfg.repos
        .iter()
        .map(|r| ScanItem { path: r.path.clone(), status: read_status(&r.path) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_name_from_url_works() {
        assert_eq!(repo_name_from_url("https://github.com/user/hello-world.git"), Some("hello-world".into()));
        assert_eq!(repo_name_from_url("https://gitee.com/user/hello-world"), Some("hello-world".into()));
        assert_eq!(repo_name_from_url("git@github.com:user/repo.git"), Some("repo".into()));
        // 结尾带斜杠时去掉斜杠后取最后一段（是否为合法仓库交给 clone 结果判断）
        assert_eq!(repo_name_from_url("https://github.com/user/"), Some("user".into()));
    }

    #[test]
    fn progress_lines_are_parsed() {
        let ev = parse_progress("Receiving objects:  45% (123/273), 1.2 MiB | 2.3 MiB/s").unwrap();
        assert_eq!(ev.stage, "接收对象");
        assert_eq!(ev.percent, Some(45.0));
        let ev = parse_progress("Resolving deltas: 100% (10/10), done.").unwrap();
        assert_eq!(ev.percent, Some(100.0));
        assert!(parse_progress("fatal: unable to access").is_none());
    }

    #[test]
    fn base64_works() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn host_detection_works() {
        assert_eq!(host_of("https://github.com/u/r.git"), Some("github"));
        assert_eq!(host_of("git@gitee.com:u/r.git"), Some("gitee"));
        assert_eq!(host_of("https://example.com/u/r.git"), None);
        assert!(is_ssh_url("git@github.com:u/r.git"));
        assert!(!is_ssh_url("https://github.com/u/r.git"));
    }

    #[test]
    fn net_auth_uses_platform_tokens() {
        let mut cfg = store::AppConfig::default();
        cfg.settings.github_token = "gh-tok".into();
        cfg.settings.gitee_token = "ge-tok".into();
        cfg.settings.ssh_key_path = "C:\\keys\\id_ed25519".into();

        let (c, ssh) = build_net_auth(&cfg, "https://github.com/u/r.git");
        assert_eq!(c.len(), 1);
        assert!(c[0].starts_with("http.extraheader=AUTHORIZATION: basic "));
        // x-access-token:gh-tok 的 base64
        assert!(c[0].ends_with(&base64_encode(b"x-access-token:gh-tok")));
        assert!(ssh.is_none()); // https 远端不注入 ssh

        let (c, _) = build_net_auth(&cfg, "https://gitee.com/u/r.git");
        assert!(c[0].ends_with(&base64_encode(b"oauth2:ge-tok")));

        let (_, ssh) = build_net_auth(&cfg, "git@github.com:u/r.git");
        assert!(ssh.unwrap().contains("C:\\keys\\id_ed25519"));

        // 无令牌、无 ssh 远端时不注入任何东西
        let empty = store::AppConfig::default();
        let (c, ssh) = build_net_auth(&empty, "https://example.com/u/r.git");
        assert!(c.is_empty() && ssh.is_none());
    }

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
