//! 仓库管理：状态读取、增删改查。平移自 `src-tauri/commands/repos.rs`，去 Tauri 依赖。
//!
//! 只搬界面真正要用的那一部分：Screen 3（我的仓库工作台）需要仓库列表与实时状态，
//! 其余（初始化向导、一键扫描、语言统计、提交历史）随对应屏一起迁，不提前铺。

use crate::git;
use crate::store::{self, AppConfig, RepoEntry};
use crate::types::RepoStatus;
use std::time::Duration;

/// 本地 git 命令（status / log 等）超时。与 src-tauri 侧同值：
/// 这些命令不联网，10 秒还不出结果基本是仓库卡死或路径不对。
pub const LOCAL_TIMEOUT: Duration = Duration::from_secs(10);

/// 执行本地 git 命令（不涉及网络，不走代理回退）
fn run_local(repo_path: &str, args: &[&str]) -> git::GitOutput {
    git::run_git(repo_path, args, None, LOCAL_TIMEOUT)
}

/// 是不是一个 git 仓库（只看 .git 存在，不递归探测——
/// 非仓库的文件夹也允许添加进来，界面上要能显示「不是仓库」而不是直接拒绝）
pub fn valid_repo(path: &str) -> bool {
    std::path::Path::new(path).join(".git").exists()
}

/// 从路径尾部取仓库名；取不到就整条路径当名字
pub fn repo_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// 读取仓库状态：分支 / 领先落后 / 脏工作区 / 有无上游 / 远端地址。
///
/// 每项各跑一条 git 命令，仓库多了会明显慢——所以调用方应该把它放后台线程，
/// 不要在渲染路径上同步等（界面靠上一次结果先画着，拿到新的再刷）。
pub fn read_status(repo_path: &str) -> RepoStatus {
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

/// 未提交的文件数量（仓库头的「未提交 N 文件」那个数字）。
/// 不是仓库时返回 0。
pub fn changed_files(repo_path: &str) -> u32 {
    if !valid_repo(repo_path) {
        return 0;
    }
    run_local(repo_path, &["status", "--porcelain"])
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count() as u32
}

// ---------- 仓库增删改查 ----------

pub fn list_repos() -> Vec<RepoEntry> {
    store::load().repos
}

/// 添加仓库；已在列表里则原样返回，不重复添加
pub fn add_repo(path: &str) -> Result<Vec<RepoEntry>, String> {
    let path = path.trim().to_string();
    if !valid_repo(&path) {
        return Err("所选文件夹不是 git 仓库（未找到 .git）".to_string());
    }
    let mut cfg = store::load();
    if !cfg.repos.iter().any(|r| r.path == path) {
        cfg.repos.push(RepoEntry {
            name: repo_name(&path),
            path: path.clone(),
            added_at: store::now_secs(),
            ..RepoEntry::default()
        });
    }
    store::save(&cfg)?;
    Ok(cfg.repos)
}

/// 移除仓库（只删记录，不动磁盘上的文件）
pub fn remove_repo(path: &str) -> Result<Vec<RepoEntry>, String> {
    let mut cfg = store::load();
    cfg.repos.retain(|r| r.path != path);
    store::save(&cfg)?;
    Ok(cfg.repos)
}

/// 设置本地自定义显示名；alias 传空串恢复原始仓库名
pub fn rename_repo(path: &str, alias: &str) -> Result<Vec<RepoEntry>, String> {
    with_repo(path, |e| e.alias = alias.trim().to_string())
}

/// 记一次「最近用过」，置顶的排在最前，其余按此排序
pub fn touch_repo(path: &str) {
    let _ = with_repo(path, |e| e.last_used = store::now_secs());
}

/// 改某仓库的某个字段并落盘。仓库不在列表里时返回 None（不改配置）。
fn with_repo<F>(path: &str, f: F) -> Result<Vec<RepoEntry>, String>
where
    F: FnOnce(&mut RepoEntry),
{
    let mut cfg: AppConfig = store::load();
    if let Some(e) = cfg.repos.iter_mut().find(|r| r.path == path) {
        f(e);
    }
    store::save(&cfg)?;
    Ok(cfg.repos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::RepoEntry;

    /// 造一个带全部字段的条目：字段一多，写字面量漏字段就会编译不过——
    /// 这正是加新字段时想要的提醒，而不是靠 `..Default::default()` 把漏掉的静默吃掉
    fn full_entry() -> RepoEntry {
        RepoEntry {
            path: "C:/repo".into(),
            name: "repo".into(),
            alias: String::new(),
            added_at: 0,
            last_used: 0,
            test_cmd: String::new(),
            build_cmd: String::new(),
            dev_cmd: String::new(),
            install_cmd: String::new(),
            pinned_cmds: Vec::new(),
            cmd_runs: Default::default(),
            auto_sync_mins: 0,
            daily_time: String::new(),
            auto_pull: false,
            last_auto_check: 0,
            last_auto_result: String::new(),
            pinned: false,
            is_third_party: false,
        }
    }

    #[test]
    fn repo_name_takes_the_last_path_segment() {
        assert_eq!(repo_name("D:/AIwork/qingbird-git"), "qingbird-git");
        // 尾部带分隔符时不能给出空名字
        assert_eq!(repo_name("D:/AIwork/qingbird-git/"), "qingbird-git");
    }

    #[test]
    fn status_of_a_non_repo_is_not_a_repo() {
        let st = read_status("D:/AIwork/这个路径不存在-xyz");
        assert!(!st.is_repo);
        assert!(st.branch.is_empty());
        assert_eq!((st.ahead, st.behind), (0, 0));
        assert!(!st.dirty);
    }

    #[test]
    fn changed_files_of_a_non_repo_is_zero() {
        assert_eq!(changed_files("D:/AIwork/这个路径不存在-xyz"), 0);
    }

    #[test]
    fn new_entry_defaults_do_not_bite() {
        // 加字段后老配置仍要能读出来：全空值 = 未设置，不是崩溃
        let e = full_entry();
        assert!(e.cmd_runs.is_empty());
        assert!(e.pinned_cmds.is_empty());
    }
}
