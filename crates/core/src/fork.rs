//! 分叉检测：本地与云端各有对方没有的提交时的结构化信息。
//! 变基恢复向导（rebase_fork / undo_rebase）暂未平移——等 GUI 分叉卡落地时再迁。

use std::path::{Path, PathBuf};

use crate::git;
use crate::types::{ForkInfo, RepoStatus};

/// 本地命令执行器
pub type LocalRun<'a> = &'a dyn Fn(&[&str]) -> git::GitOutput;

/// 变基备份分支。单一名、每次覆盖到"变基前"的位置：不用时间戳是为了不让 refs 越堆越多，
/// 固定名字也方便用户在自己的 git 工具里一眼找到
pub const BACKUP_BRANCH: &str = "backup/qingbird-git";
/// 自动暂存的说明，用户 `git stash list` 时能认出是谁存的
pub const STASH_MSG: &str = "qingbird-git: 变基前自动暂存";
/// 提交列表最多列几条：分叉几百个提交时不该把整页撑爆
const MAX_LISTED: usize = 20;

/// `git rev-parse --git-dir` 的绝对化结果（worktree、自定义 GIT_DIR 也能命中）
fn git_dir(path: &str, local: LocalRun<'_>) -> Option<PathBuf> {
    let out = local(&["rev-parse", "--git-dir"]);
    if !out.success {
        return None;
    }
    let raw = out.stdout.trim();
    if raw.is_empty() {
        return None;
    }
    let p = PathBuf::from(raw);
    Some(if p.is_absolute() {
        p
    } else {
        Path::new(path).join(p)
    })
}

/// 变基/合并中途的说明；None 表示可以放心动。
/// 向导一律不碰用户自己的半成品状态——那种情况下替他动手只会把事情弄得更糟
fn blocked_reason(path: &str, local: LocalRun<'_>) -> Option<String> {
    let dir = git_dir(path, local)?;
    for (marker, what, how) in [
        ("rebase-merge", "变基", "git rebase --abort"),
        ("rebase-apply", "变基", "git rebase --abort"),
        ("MERGE_HEAD", "合并", "git merge --abort"),
    ] {
        if dir.join(marker).exists() {
            return Some(format!(
                "仓库正处于一次未完成的{what}中。请先在终端里把它完成或中止（{how}）再回来处理分叉——向导不会替你动这一步"
            ));
        }
    }
    None
}

/// 取一段提交列表（`--oneline`，最多 `MAX_LISTED` 条）
fn commit_list(range: &str, local: LocalRun<'_>) -> Vec<String> {
    let limit = MAX_LISTED.to_string();
    let out = local(&["log", "--oneline", "--no-decorate", "-n", &limit, range]);
    if !out.success {
        return Vec::new();
    }
    out.stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// 组装分叉信息；不是分叉（`ahead` / `behind` 有一边为 0）就返回 None
pub fn fork_info(path: &str, st: &RepoStatus, local: LocalRun<'_>) -> Option<ForkInfo> {
    if !st.has_upstream || st.ahead == 0 || st.behind == 0 {
        return None;
    }
    let upstream = local(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
        .stdout
        .trim()
        .to_string();
    Some(ForkInfo {
        ahead: st.ahead,
        behind: st.behind,
        dirty: st.dirty,
        branch: st.branch.clone(),
        upstream,
        local_commits: commit_list("@{u}..HEAD", local),
        remote_commits: commit_list("HEAD..@{u}", local),
        blocked_reason: blocked_reason(path, local),
    })
}
