//! 仓库管理：增删改查、状态读取、初始化、变更列表、语言统计、一键扫描。

use crate::{git, store};
use serde::Serialize;
use std::collections::HashMap;

use super::net::{identity_config, run_local};
use super::types::RepoStatus;
use super::LOCAL_TIMEOUT;

pub(crate) fn valid_repo(path: &str) -> bool {
    std::path::Path::new(path).join(".git").exists()
}

pub(super) fn repo_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

pub(super) fn read_status(repo_path: &str) -> RepoStatus {
    let mut st = RepoStatus {
        is_repo: valid_repo(repo_path),
        ..Default::default()
    };
    if !st.is_repo {
        return st;
    }
    st.branch = run_local(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .stdout
        .trim()
        .to_string();

    let upstream = run_local(
        repo_path,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    );
    st.has_upstream = upstream.success;
    if upstream.success {
        let counts = run_local(
            repo_path,
            &["rev-list", "--left-right", "--count", "HEAD...@{u}"],
        );
        if counts.success {
            let line = counts.stdout.trim().to_string();
            if let [ahead, behind] = line.split_whitespace().collect::<Vec<_>>()[..] {
                st.ahead = ahead.parse().unwrap_or(0);
                st.behind = behind.parse().unwrap_or(0);
            }
        }
    }
    st.dirty = !run_local(repo_path, &["status", "--porcelain"])
        .stdout
        .trim()
        .is_empty();
    st.remote_url = run_local(repo_path, &["remote", "get-url", "origin"])
        .stdout
        .trim()
        .to_string();
    st
}

pub(super) fn touch_repo(app: &tauri::AppHandle, path: &str) {
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

    let init = git::run_git_auth(
        &path,
        &["init", "-b", "main"],
        None,
        LOCAL_TIMEOUT,
        None,
        &[],
        None,
    );
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
    };
    if !cfg.repos.iter().any(|r| r.path == path) {
        cfg.repos.push(entry.clone());
        store::save(&app, &cfg)?;
    }
    Ok(entry)
}

// ---------- 仓库状态 / 变更 / 提交历史 / 语言 / 扫描 ----------

#[tauri::command]
pub fn repo_status(path: String) -> RepoStatus {
    read_status(&path)
}

/// 请求取消当前正在进行的网络操作（克隆/检测更新/拉取/推送），
/// 运行中的 git 子进程会被立即终止
#[tauri::command]
pub fn cancel_busy_op() {
    git::request_cancel();
}

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
        let Some(lang) = lang_of_ext(&ext) else {
            continue;
        };
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
        stats.push(LangStat {
            language: "其他".into(),
            percent: rest,
        });
    }
    stats
}

#[derive(Serialize, Clone)]
pub struct ScanItem {
    pub path: String,
    pub status: RepoStatus,
}

#[tauri::command(async)]
pub fn scan_all(app: tauri::AppHandle) -> Vec<ScanItem> {
    let cfg = store::load(&app);
    cfg.repos
        .iter()
        .map(|r| ScanItem {
            path: r.path.clone(),
            status: read_status(&r.path),
        })
        .collect()
}

