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

// ---------- 提交活动（工作区柱状图） ----------

/// 柱状图覆盖的天数。固定值：这个图只回答「最近这一阵子的节奏」，
/// 做成参数反而让前端多一套分支处理。
const ACTIVITY_DAYS: u32 = 14;

/// 某一天的提交数
#[derive(Serialize, Clone, Debug)]
pub struct ActivityDay {
    /// YYYY-MM-DD，按**本地时区**（与 recent_commits 的日期一致）
    pub date: String,
    pub count: u32,
}

/// 近 ACTIVITY_DAYS 天的提交活动。
///
/// **只返回有提交的日子**，没有提交的日子不在这里——补齐 14 根柱子要算日期，
/// 那在前端更省事（后端不必引入日期库），日期口径两边一致（都取本地时区）。
/// 全是空的仓库返回空数组。
#[tauri::command]
pub fn commit_activity(path: String) -> Vec<ActivityDay> {
    if !valid_repo(&path) {
        return Vec::new();
    }
    // --all：柱状图看的是「我在这个仓库干了多少活」，提交落在别的分支上也得算数。
    // --since 走 git 自己的日期计算，不用我们处理时区与夏令时。
    let out = run_local(
        &path,
        &[
            "log",
            "--all",
            &format!("--since={ACTIVITY_DAYS}.days.ago"),
            "--date=format-local:%Y-%m-%d",
            "--pretty=format:%ad",
        ],
    );
    if !out.success {
        return Vec::new();
    }
    bucket_days(&out.stdout)
}

/// 把 `git log` 的逐行日期分桶成「日期 → 计数」，按日期倒序（新的在前）。
/// git log 默认按时间倒序输出，这里保持同样顺序，省一次排序。
fn bucket_days(log_out: &str) -> Vec<ActivityDay> {
    let mut days: Vec<ActivityDay> = Vec::new();
    for line in log_out.lines() {
        let date = line.trim();
        // --date 固定输出 YYYY-MM-DD；形状不对的行（理论上不该有）直接跳过，
        // 免得脏数据混进图表
        let b = date.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            continue;
        }
        match days.iter_mut().find(|d| d.date == date) {
            Some(d) => d.count += 1,
            None => days.push(ActivityDay {
                date: date.to_string(),
                count: 1,
            }),
        }
    }
    days
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

#[cfg(test)]
mod activity_tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn days_are_bucketed_and_kept_in_git_order() {
        // git log 默认按时间倒序，这里要保持同样顺序
        let days = bucket_days("2026-09-28\n2026-09-28\n2026-09-27\n2026-09-28\n2026-09-25\n");
        let got: Vec<(&str, u32)> = days.iter().map(|d| (d.date.as_str(), d.count)).collect();
        assert_eq!(got, vec![("2026-09-28", 3), ("2026-09-27", 1), ("2026-09-25", 1)]);
    }

    #[test]
    fn blank_or_malformed_lines_are_skipped() {
        // 空仓库时 git log 什么都不输出；夹杂的噪声行不该变成一根 0 高度的柱子
        assert!(bucket_days("").is_empty());
        assert!(bucket_days("\n\n").is_empty());
        let days = bucket_days("2026-09-28\nnot-a-date\n2026-9-8\n2026-09-28\n");
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].count, 2, "只数格式正确的日期行");
    }

    /// 端到端跑真 git：这条路径验的是命令参数本身（--since / %ad 格式），
    /// 纯函数测不出来。
    #[test]
    fn activity_of_a_real_repo_counts_todays_commits() {
        // 会派生 git 子进程：持读锁，避免与持写锁的取消测试互相杀掉对方的 git
        let _guard = crate::git::GIT_TEST_LOCK
            .read()
            .unwrap_or_else(|e| e.into_inner());
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root: PathBuf = std::env::temp_dir()
            .join(format!("qbg-activity-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        fn sh(dir: &Path, args: &[&str]) -> String {
            let out = crate::git::run_git(
                &dir.to_string_lossy(),
                args,
                None,
                std::time::Duration::from_secs(20),
            );
            assert!(out.success, "git {args:?} 失败：{}", out.stderr.trim());
            out.stdout.trim().to_string()
        }

        sh(&root, &["init", "-q", "--initial-branch=main"]);
        sh(&root, &["config", "user.email", "t@example.com"]);
        sh(&root, &["config", "user.name", "t"]);
        sh(&root, &["config", "commit.gpgsign", "false"]);
        sh(&root, &["config", "core.autocrlf", "false"]);

        // 没有提交的仓库：空数组，前端自己补 14 根 0 高度的柱子
        assert!(commit_activity(root.to_string_lossy().to_string()).is_empty());

        // 连续三次提交必然落在同一天：断言计数等于 3，不去猜是哪一天
        for i in 0..3 {
            std::fs::write(root.join("f.txt"), format!("{i}\n")).unwrap();
            sh(&root, &["add", "-A"]);
            sh(&root, &["commit", "-q", "-m", &format!("c{i}")]);
        }
        let days = commit_activity(root.to_string_lossy().to_string());
        assert_eq!(days.len(), 1, "同一天的提交合并成一个桶：{days:?}");
        assert_eq!(days[0].count, 3);
        assert_eq!(days[0].date.len(), 10, "YYYY-MM-DD，实际 {:?}", days[0].date);

        // 非仓库目录：安静返回空，不报错
        let not_repo = root.join("nope");
        std::fs::create_dir_all(&not_repo).unwrap();
        assert!(commit_activity(not_repo.to_string_lossy().to_string()).is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }
}

