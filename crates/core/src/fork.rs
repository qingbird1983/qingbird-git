//! 分叉恢复向导：把"本地与云端历史分叉"从一个死胡同变成一组有出口的动作。
//!
//! 默认拉取路径仍然是 `git pull --ff-only` 不会变；变基只可能是用户在分叉卡片上的
//! 显式动作，而且动手前强制先把"变基前"的位置固定在备份分支上、冲突自动中止复原
//! （见 `rebase_core`）。
//!
//! 变基序列是本功能唯一改写本地历史的地方，因此写成"注入执行器"的形式：
//! 生产代码传 `run_local` / `run_net_op` 的包装闭包，单测传直接调 git 的闭包，
//! 在临时目录的真实仓库上跑真实 git 验证。

use std::path::{Path, PathBuf};

use crate::git;
use crate::net::{self, NetMode};
use crate::repos::{self, valid_repo};
use crate::sync::{self, NET_TIMEOUT};
use crate::types::{combined, ForkInfo, OpResult, ProgressEvent, RepoStatus};

/// 本地命令执行器
pub type LocalRun<'a> = &'a dyn Fn(&[&str]) -> git::GitOutput;
/// 网络命令执行器，返回 (输出, 本次所用代理)
pub type RemoteRun<'a> = &'a dyn Fn(&[&str]) -> (git::GitOutput, Option<String>);

/// 变基序列的结果
pub struct RebaseOutcome {
    pub ok: bool,
    pub message: String,
    /// 全过程 git 输出（界面日志区展示）
    pub log: String,
    pub used_proxy: Option<String>,
    /// 只有变基真的跑起来了才为 Some——界面据此显示"撤销变基"
    pub backup_branch: Option<String>,
}

/// 变基备份分支。单一名、每次覆盖到"变基前"的位置：不用时间戳是为了不让 refs 越堆越多，
/// 固定名字也方便用户在自己的 git 工具里一眼找到
pub const BACKUP_BRANCH: &str = "backup/qingbird-git";
// （这里原来挂着 `pub const STASH_MSG`——变基前自动暂存的那句说明。32-2 起
// 它和拉取侧那句一起收进 `crate::stash`：**"这条是不是青鸟藏的"只能有一份判据**，
// 而两条路各写一句措辞正是它长不出来的原因。**不留转发层**）
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

/// 变基 / 合并是不是正停在半路（只有这时才该 `--abort`）
fn rebase_in_progress(path: &str, local: LocalRun<'_>) -> bool {
    let Some(dir) = git_dir(path, local) else {
        return false;
    };
    dir.join("rebase-merge").exists() || dir.join("rebase-apply").exists()
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

// ---------- 变基恢复 ----------

/// 从 git 输出里挑一句最适合给用户看的
fn first_error_line(o: &git::GitOutput) -> Option<String> {
    let lines: Vec<String> = o
        .stderr
        .lines()
        .chain(o.stdout.lines())
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    lines
        .iter()
        .find(|l| ["fatal:", "error:", "CONFLICT"].into_iter().any(|k| l.contains(k)))
        .cloned()
        .or_else(|| lines.last().cloned())
}

/// 冲突中的文件清单（`git diff --name-only --diff-filter=U`）。
///
/// ⚠️ **必须在 `rebase --abort` 之前问**：abort 会把 index 复原、U 位全部清掉，
/// 之后再问就是一张空表——那等于把"用户看不到冲在哪几个文件"这句话**假装修好了**，
/// 而且不报错、不改任何功能（32-6 要挡的正是这个）。
/// ⚠️ 读不出来（不是仓库 / 命令失败）也给空表，由 `conflict_note` 那边说"没点到名"，
/// 不假装"没有冲突"。
/// ⚠️ **`-c core.quotePath=false` 必须带着，而且必须排在 `diff` 之前**（实测：不带时
/// 中文文件名输出成 `"\344\270\255\346\226\207 \346\226\207\344\273\266.txt"`，
/// 用户看到的是一串八进制码而不是文件名）。与 `repos.rs` 那条 status 命令同一条纪律
fn conflicted_files(local: LocalRun<'_>) -> Vec<String> {
    let o = local(&[
        "-c",
        "core.quotePath=false",
        "diff",
        "--name-only",
        "--diff-filter=U",
    ]);
    if !o.success {
        return Vec::new();
    }
    o.stdout.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()
}

/// 把冲突清单拼成一句话。**上限五颗**——告警那一格只有那么宽，
/// 而"还有多少颗"这个数字比第五个文件名更有用（用户据此决定去不去终端看）。
///
/// 空表必须**有自己那句话**：拼成「冲突的文件：」后面什么都没有，
/// 比不拼更坏（那是 AGENTS 里「控件禁用了但话还说错了」的同族）。
fn conflict_note(files: &[String]) -> String {
    const CAP: usize = 5;
    if files.is_empty() {
        return "这次没能读出具体是哪几个文件（可能是未跟踪文件挡了路），请在终端跑一次 git status".to_string();
    }
    let head = files.iter().take(CAP).cloned().collect::<Vec<_>>().join("、");
    if files.len() > CAP {
        format!("冲突的文件：{head} 等 {} 个", files.len())
    } else {
        format!("冲突的文件：{head}")
    }
}

/// 变基失败时的结论：中文说明比 git 原文更有用，所以只在拿不出结论时兜底用原文
fn rebase_failure_message(
    o: &git::GitOutput,
    used_proxy: Option<&str>,
    aborted: bool,
    conflicts: &[String],
) -> String {
    let base = if aborted {
        "变基遇到冲突，已自动中止并复原，本地历史没有任何改动".to_string()
    } else if used_proxy.is_none() && git::is_network_error(o) {
        "变基失败：无法连接远端，且未找到可用代理".to_string()
    } else {
        match first_error_line(o) {
            Some(d) => format!("变基失败：{d}"),
            None => "变基失败".to_string(),
        }
    };
    // 网络那一臂根本还没走到冲突 ⇒ 不硬拼清单（拼了就是一句假话）。
    // 而"自动中止"那一臂即使清单是空的也要说出来——用户有权知道我们没读到
    if aborted || !conflicts.is_empty() {
        format!("{base}。{}", conflict_note(conflicts))
    } else {
        base
    }
}

/// 备份并变基到云端。本功能唯一会改写历史的地方，所以：
/// 1. 动手前先把"变基前"的位置固定在备份分支上，任何一步失败都能一键退回来；
/// 2. 只有确实进入了变基才 `rebase --abort`，绝不误杀用户自己的变基；
/// 3. 失败时原样返回，历史与工作区尽量保持原状。
pub fn rebase_core(
    path: &str,
    before: &RepoStatus,
    stash: bool,
    local: LocalRun<'_>,
    remote: RemoteRun<'_>,
) -> RebaseOutcome {
    let mut log = String::new();
    let mut backup_branch = None;
    let mut used_proxy = None;
    let mut aborted = false;
    // 只有真的走到了"改历史"那一步，才值得给用户一个撤销按钮
    let mut attempted = false;

    let (ok, message) = 'seq: {
        if let Some(reason) = blocked_reason(path, local) {
            break 'seq (false, reason);
        }
        if !before.has_upstream {
            break 'seq (false, "当前分支没有上游分支，无法变基到云端".to_string());
        }

        // `-f` 覆盖同名分支：这是本工具自己的命名空间，不需要用户手动清理旧备份
        let backup = local(&["branch", "-f", BACKUP_BRANCH, "HEAD"]);
        log.push_str(&combined(&backup));
        if !backup.success {
            break 'seq (false, "创建备份分支失败，已取消（没有改动任何历史）".to_string());
        }
        backup_branch = Some(BACKUP_BRANCH.to_string());

        // 变基要求工作区干净。与 do_pull 的暂存流程保持一致：用户确认过才 stash，
        // 变基完成后 pop 回来；pop 冲突时如实报告，不替用户决定取舍
        let mut stashed = false;
        if before.dirty {
            if !stash {
                break 'seq (
                    false,
                    "工作区有未提交变更，变基前需要先暂存（不会丢掉，变基后自动恢复）".to_string(),
                );
            }
            let s = local(&["stash", "push", "-u", "-m", crate::stash::MSG_REBASE]);
            log.push_str(&combined(&s));
            if !s.success {
                break 'seq (false, "暂存本地变更失败，已取消变基（历史未改动）".to_string());
            }
            stashed = true;
        }

        attempted = true;
        let (pull, proxy) = remote(&["pull", "--rebase", "--progress"]);
        log.push_str(&combined(&pull));
        used_proxy = proxy;

        if !pull.success {
            // ⚠️ **顺序判据**：清单必须在 abort 之前读。把这两行换个次序，
            // 冲突文件就永远是空表，而界面上一句"没能读出是哪几个文件"照样说得通——
            // 没有任何东西会红（32-6 的整条价值就压在这个顺序上）
            let conflicts = conflicted_files(local);
            if rebase_in_progress(path, local) {
                let abort = local(&["rebase", "--abort"]);
                log.push_str(&combined(&abort));
                aborted = true;
            }
            if stashed {
                log.push_str(&combined(&local(&["stash", "pop"])));
            }
            break 'seq (
                false,
                rebase_failure_message(&pull, used_proxy.as_deref(), aborted, &conflicts),
            );
        }

        if stashed {
            let pop = local(&["stash", "pop"]);
            log.push_str(&combined(&pop));
            if !pop.success {
                break 'seq (
                    false,
                    "变基成功，但恢复暂存变更时冲突，请在终端执行 git stash pop 处理".to_string(),
                );
            }
        }
        (true, "已变基到云端：本地提交被重放到云端提交之上".to_string())
    };

    RebaseOutcome {
        ok,
        message,
        log,
        used_proxy,
        // 冲突已中止时 HEAD 本来就等于备份位置，再给"撤销"只会让人困惑
        backup_branch: if attempted && !aborted { backup_branch } else { None },
    }
}

/// 备份并变基到云端（用户在分叉卡片上的显式动作）
pub fn rebase_fork(
    path: &str,
    stash: bool,
    on_progress: Option<&dyn Fn(&ProgressEvent)>,
) -> OpResult {
    if !valid_repo(path) {
        return OpResult::err("该文件夹不是 git 仓库".into(), String::new());
    }
    let Some(_guard) = sync::begin_user_op(path) else {
        return OpResult::err(sync::BUSY_MSG.into(), String::new());
    };
    // 动手前的状态：是否脏、有没有上游，都在 rebase_core 之前定下来
    let before = repos::read_status(path);
    let (_tag, forward, stage) = net::progress_hooks(on_progress);
    let local = |args: &[&str]| net::run_local(path, args);
    let remote = |args: &[&str]| {
        net::run_net_op(
            path,
            args,
            Some(&forward),
            None,
            NET_TIMEOUT,
            Some(&stage),
            NetMode::Auto,
        )
    };

    let outcome = rebase_core(path, &before, stash, &local, &remote);
    let status = repos::read_status(path);
    repos::touch_repo(path);
    let fork = fork_info(path, &status, &local);

    OpResult {
        ok: outcome.ok,
        message: outcome.message,
        output: outcome.log,
        used_proxy: outcome.used_proxy,
        status: Some(status),
        // 卡片按 ForkInfo 自己决定给哪几个按钮，这里不额外指示
        need_stash: false,
        changes: None,
        fork,
        backup_branch: outcome.backup_branch,
    }
}

/// 撤销上次变基：把当前分支硬回退到备份分支的位置。
/// 界面必须先二次确认——这会丢弃变基之后新产生的提交
pub fn undo_rebase(path: &str) -> OpResult {
    if !valid_repo(path) {
        return OpResult::err("该文件夹不是 git 仓库".into(), String::new());
    }
    let Some(_guard) = sync::begin_user_op(path) else {
        return OpResult::err(sync::BUSY_MSG.into(), String::new());
    };
    let local = |args: &[&str]| net::run_local(path, args);
    let refname = format!("refs/heads/{BACKUP_BRANCH}");
    if !local(&["show-ref", "--verify", "--quiet", &refname]).success {
        return OpResult::err("找不到变基备份分支，无法撤销".into(), String::new());
    }

    // 撤下来的是哪些提交：给用户一个数字，比"确定要撤销吗"有信息量
    let discarded = local(&["rev-list", "--count", &format!("{BACKUP_BRANCH}..HEAD")])
        .stdout
        .trim()
        .to_string();
    let reset = local(&["reset", "--hard", BACKUP_BRANCH]);
    let status = repos::read_status(path);
    repos::touch_repo(path);
    let fork = fork_info(path, &status, &local);

    if !reset.success {
        let mut r = OpResult::err("撤销失败".into(), combined(&reset));
        r.status = Some(status);
        r.fork = fork;
        return r;
    }
    OpResult {
        ok: true,
        message: format!(
            "已回到变基前的状态（撤下 {discarded} 个重放提交），备份分支 {BACKUP_BRANCH} 仍保留"
        ),
        output: combined(&reset),
        used_proxy: None,
        status: Some(status),
        need_stash: false,
        changes: None,
        fork,
        // 撤销完成后不再提供撤销，避免"撤销的撤销"这种绕圈操作
        backup_branch: None,
    }
}

#[cfg(test)]
mod fork_tests {
    use super::*;

    /// 临时目录里造一个真实的"已分叉"仓库：bare 远端 + 本地提交 + 云端提交。
    /// 执行器直接调 git——这些用例验证的是 git 的真实行为，不是我们对它的假设
    struct Fixture {
        root: PathBuf,
        bare: PathBuf,
        work: PathBuf,
        /// 持到测试结束：全部用例都在派生 git，不能和取消测试（全局 CANCELLED）并行
        _git_lock: std::sync::RwLockReadGuard<'static, ()>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn sh(dir: &Path, args: &[&str]) -> git::GitOutput {
        git::run_git(
            &dir.to_string_lossy(),
            args,
            None,
            std::time::Duration::from_secs(30),
        )
    }

    /// 跑一条 git 命令并要求成功。失败时把 git 自己的话带进 panic——
    /// 藏起 stderr 就只能靠猜
    fn ok(dir: &Path, args: &[&str]) -> git::GitOutput {
        let out = sh(dir, args);
        assert!(
            out.success,
            "git {args:?} 在 {} 失败（code={}, timed_out={}）：\n{}",
            dir.display(),
            out.code,
            out.timed_out,
            out.stderr.trim()
        );
        out
    }

    fn out_of(dir: &Path, args: &[&str]) -> String {
        ok(dir, args).stdout.trim().to_string()
    }

    /// 克隆时就必须把换行处理定型：检出若按跑测试机器的全局 `core.autocrlf`
    /// （Windows 上常见 true）来，随后的 `git add -A` 会把 CRLF 固化进 blob，
    /// 于是同一文件在不同提交里一个 LF 一个 CRLF，按字节比内容时全对不上，
    /// `git stash pop` 也会因此报内容冲突
    fn clone(root: &Path, bare: &Path, dest: &Path) {
        ok(
            root,
            &[
                "-c",
                "core.autocrlf=false",
                "-c",
                "core.eol=lf",
                "clone",
                "-q",
                &bare.to_string_lossy(),
                &dest.to_string_lossy(),
            ],
        );
    }

    /// 不让测试结果取决于跑测试那台机器的全局 git 配置
    fn configure(dir: &Path) {
        ok(dir, &["config", "user.email", "t@example.com"]);
        ok(dir, &["config", "user.name", "t"]);
        ok(dir, &["config", "core.autocrlf", "false"]);
        ok(dir, &["config", "core.eol", "lf"]);
        ok(dir, &["config", "commit.gpgsign", "false"]);
    }

    fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) {
        std::fs::write(dir.join(name), body).unwrap();
        ok(dir, &["add", "-A"]);
        ok(dir, &["commit", "-q", "-m", msg]);
    }

    fn fixture() -> Fixture {
        let git_lock = crate::git::GIT_TEST_LOCK
            .read()
            .unwrap_or_else(|e| e.into_inner());
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("qbg-fork-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bare = root.join("remote.git");
        let work = root.join("work");
        std::fs::create_dir_all(&root).unwrap();

        ok(
            &root,
            &[
                "init",
                "--bare",
                "-q",
                "--initial-branch=main",
                &bare.to_string_lossy(),
            ],
        );
        clone(&root, &bare, &work);
        configure(&work);

        let f = Fixture {
            root,
            bare,
            work,
            _git_lock: git_lock,
        };
        commit_file(&f.work, "base.txt", "base\n", "base");
        ok(&f.work, &["push", "-q", "-u", "origin", "main"]);
        f
    }

    /// 云端（另一个克隆）提交并推上去，再让本地 fetch 到
    fn push_from_clone(f: &Fixture, dir: &Path, name: &str, body: &str, msg: &str) {
        if !dir.exists() {
            clone(&f.root, &f.bare, dir);
            configure(dir);
        }
        commit_file(dir, name, body, msg);
        ok(dir, &["push", "-q", "origin", "main"]);
        ok(&f.work, &["fetch", "-q", "origin"]);
    }

    /// 让本地与云端各前进一步，形成分叉
    fn diverge(f: &Fixture) {
        commit_file(&f.work, "local.txt", "local\n", "local work");
        push_from_clone(f, &f.root.join("other"), "remote.txt", "remote\n", "remote work");
    }

    fn status_of(dir: &Path) -> RepoStatus {
        // 借生产代码的读取逻辑，保证测的就是真实判定
        repos::read_status(&dir.to_string_lossy())
    }

    #[test]
    fn diverged_repo_is_reported_as_fork() {
        let f = fixture();
        diverge(&f);
        let st = status_of(&f.work);
        assert_eq!((st.ahead, st.behind), (1, 1), "本地 1 个、云端 1 个");

        let path = f.work.to_string_lossy().to_string();
        let info = fork_info(&path, &st, &|a| sh(&f.work, a)).expect("应判定为分叉");
        assert_eq!(info.upstream, "origin/main");
        assert!(info.local_commits[0].contains("local work"));
        assert!(info.remote_commits[0].contains("remote work"));
        assert!(info.blocked_reason.is_none(), "干净仓库不该被拦");
    }

    #[test]
    fn fast_forwardable_repo_is_not_a_fork() {
        let f = fixture();
        // 只有云端前进，本地没提交 → behind 但 ahead=0，属于正常"可拉取"
        push_from_clone(&f, &f.root.join("other"), "remote.txt", "remote\n", "remote work");

        let st = status_of(&f.work);
        let path = f.work.to_string_lossy().to_string();
        assert!(fork_info(&path, &st, &|a| sh(&f.work, a)).is_none());
    }

    #[test]
    fn rebase_replays_local_commit_on_top_of_remote_and_keeps_backup() {
        let f = fixture();
        diverge(&f);
        let before = status_of(&f.work);
        let head_before = out_of(&f.work, &["rev-parse", "HEAD"]);
        let path = f.work.to_string_lossy().to_string();

        let out = rebase_core(&path, &before, false, &|a| sh(&f.work, a), &|a| {
            (sh(&f.work, a), None)
        });
        assert!(out.ok, "变基应成功：{} / {}", out.message, out.log);
        assert_eq!(out.backup_branch.as_deref(), Some(BACKUP_BRANCH));

        // 备份分支停在"变基前"，这是撤销的唯一依据
        let backup = out_of(&f.work, &["rev-parse", BACKUP_BRANCH]);
        assert_eq!(backup, head_before, "备份分支必须指向变基前的 HEAD");

        let st = status_of(&f.work);
        assert_eq!((st.ahead, st.behind), (1, 0), "变基后只应领先 1 个、不再落后");
        assert!(fork_info(&path, &st, &|a| sh(&f.work, a)).is_none(), "分叉卡片应消失");
        let log = sh(&f.work, &["log", "--oneline", "-3"]).stdout;
        assert!(log.contains("local work") && log.contains("remote work"));
        let order = sh(&f.work, &["log", "--oneline"]).stdout;
        let local_pos = order.find("local work").unwrap();
        let remote_pos = order.find("remote work").unwrap();
        assert!(local_pos < remote_pos, "本地提交应被重放到云端提交之上：{order}");
    }

    #[test]
    fn rebase_stashes_and_restores_dirty_worktree() {
        let f = fixture();
        diverge(&f);
        // 未跟踪 + 已跟踪修改各一份，验证 -u 也覆盖到了
        std::fs::write(f.work.join("wip.txt"), "wip\n").unwrap();
        std::fs::write(f.work.join("base.txt"), "base changed\n").unwrap();
        let before = status_of(&f.work);
        assert!(before.dirty);
        let path = f.work.to_string_lossy().to_string();

        let out = rebase_core(&path, &before, true, &|a| sh(&f.work, a), &|a| {
            (sh(&f.work, a), None)
        });
        assert!(out.ok, "脏工作区 + 暂存应能变基成功：{} / {}", out.message, out.log);
        assert!(f.work.join("wip.txt").exists(), "未跟踪文件应被恢复");
        assert_eq!(
            std::fs::read_to_string(f.work.join("base.txt")).unwrap(),
            "base changed\n",
            "已跟踪修改应被恢复"
        );
        assert!(status_of(&f.work).dirty, "改动回来之后工作区仍是脏的");
    }

    #[test]
    fn dirty_worktree_without_stash_consent_is_refused_untouched() {
        let f = fixture();
        diverge(&f);
        std::fs::write(f.work.join("base.txt"), "base changed\n").unwrap();
        let before = status_of(&f.work);
        let head_before = out_of(&f.work, &["rev-parse", "HEAD"]);
        let path = f.work.to_string_lossy().to_string();

        let out = rebase_core(&path, &before, false, &|a| sh(&f.work, a), &|a| {
            (sh(&f.work, a), None)
        });
        assert!(!out.ok);
        assert!(out.message.contains("暂存"), "要说清楚为什么没动：{}", out.message);
        assert!(out.backup_branch.is_none(), "没真的改历史就不该给撤销按钮");
        assert_eq!(sh(&f.work, &["rev-parse", "HEAD"]).stdout.trim(), head_before);
    }

    #[test]
    fn conflicting_rebase_is_aborted_and_history_restored() {
        let f = fixture();
        // 两边都改同一个文件的同一行 → 必然冲突
        push_from_clone(&f, &f.root.join("other"), "base.txt", "remote version\n", "remote edit");
        commit_file(&f.work, "base.txt", "local version\n", "local edit");

        let before = status_of(&f.work);
        assert!(before.ahead > 0 && before.behind > 0, "先确认真的分叉了");
        let path = f.work.to_string_lossy().to_string();
        let head_before = out_of(&f.work, &["rev-parse", "HEAD"]);

        let out = rebase_core(&path, &before, false, &|a| sh(&f.work, a), &|a| {
            (sh(&f.work, a), None)
        });
        assert!(!out.ok);
        assert!(
            out.message.contains("已自动中止并复原"),
            "要说清楚冲突被自动收拾了：{} / {}",
            out.message,
            out.log
        );
        // 32-6 的整条价值压在这里：点到名是哪几个文件。
        // ⚠️ 这条断言同时钉住"清单在 abort **之前**读"——把那两行换个次序，
        // abort 会把 index 里的 U 位全清掉，句子就退成"没能读出是哪几个文件"，
        // 而那句话听起来仍然像"我们查过了"
        assert!(
            out.message.contains("冲突的文件") && out.message.contains("base.txt"),
            "冲突要说出文件名，不能只说\"有冲突\"：{}",
            out.message
        );
        assert!(
            out.backup_branch.is_none(),
            "已中止复原就不该再给撤销按钮（HEAD 本来就等于备份位置）"
        );
        assert_eq!(
            sh(&f.work, &["rev-parse", "HEAD"]).stdout.trim(),
            head_before,
            "历史必须一动不动"
        );
        assert!(
            !f.work.join(".git/rebase-merge").exists() && !f.work.join(".git/rebase-apply").exists(),
            "不能留下未完成的变基状态"
        );
        assert_eq!(
            std::fs::read_to_string(f.work.join("base.txt")).unwrap(),
            "local version\n",
            "工作区应回到变基前的样子"
        );
    }

    #[test]
    fn a_conflict_in_a_chinese_file_is_named_in_plain_words_not_escapes() {
        // 真 git 实测（32-6）：不带 `-c core.quotePath=false` 时它输出
        // `"\344\270\255\346\226\207 \346\226\207\344\273\266.txt"`——用户看到一串八进制码。
        // ⚠️ **这一路只有真 git 能证**：上面那条 `the_conflict_query_…` 的单测断的是
        // 命令数组的形态，它绿着而这里红，说明"参数写对了但 git 不认"；反过来
        // 参数删掉它才红。⇒ 与「格式串/解析类判据必须有一组真跑被解析方」同一条纪律
        let f = fixture();
        let name = "中文 文件.txt";
        push_from_clone(&f, &f.root.join("other"), name, "云端那一版\n", "remote cn");
        commit_file(&f.work, name, "本地那一版\n", "local cn");

        let before = status_of(&f.work);
        assert!(before.ahead > 0 && before.behind > 0, "先确认真的分叉了");
        let path = f.work.to_string_lossy().to_string();

        let out = rebase_core(&path, &before, false, &|a| sh(&f.work, a), &|a| {
            (sh(&f.work, a), None)
        });
        assert!(!out.ok);
        assert!(
            out.message.contains(name),
            "文件名要原样可读，不能是转义码：{}",
            out.message
        );
        assert!(
            !out.message.contains("\\34"),
            "出现八进制转义就是 quotePath 没关掉：{}",
            out.message
        );
    }

    #[test]
    fn unfinished_rebase_blocks_the_wizard() {
        let f = fixture();
        diverge(&f);
        // 造一个"变基中途"的现场
        std::fs::create_dir_all(f.work.join(".git/rebase-merge")).unwrap();
        let st = status_of(&f.work);
        let path = f.work.to_string_lossy().to_string();

        let info = fork_info(&path, &st, &|a| sh(&f.work, a)).expect("仍是分叉");
        let reason = info.blocked_reason.expect("应被拦下");
        assert!(reason.contains("变基"), "要说清楚卡在哪：{reason}");

        // 而且真的什么都不做
        let head_before = out_of(&f.work, &["rev-parse", "HEAD"]);
        let out = rebase_core(&path, &st, false, &|a| sh(&f.work, a), &|a| {
            (sh(&f.work, a), None)
        });
        assert!(!out.ok);
        assert!(out.backup_branch.is_none());
        assert_eq!(sh(&f.work, &["rev-parse", "HEAD"]).stdout.trim(), head_before);
    }

    #[test]
    fn dirty_worktree_with_conflicting_rebase_gets_its_changes_back() {
        // 最坏的一条路：脏工作区 + 变基冲突。要先暂存、冲突要中止复原、
        // 暂存的改动还得原样回到工作区——三件事都得成立
        let f = fixture();
        push_from_clone(&f, &f.root.join("other"), "base.txt", "remote version\n", "remote edit");
        commit_file(&f.work, "base.txt", "local version\n", "local edit");
        // 已提交之外再留一份未提交改动，验证它经历 stash/abort/pop 之后不丢
        std::fs::write(f.work.join("base.txt"), "uncommitted edit\n").unwrap();

        let before = status_of(&f.work);
        assert!(before.dirty && before.ahead > 0 && before.behind > 0);
        let path = f.work.to_string_lossy().to_string();
        let head_before = out_of(&f.work, &["rev-parse", "HEAD"]);

        let out = rebase_core(&path, &before, true, &|a| sh(&f.work, a), &|a| {
            (sh(&f.work, a), None)
        });
        assert!(!out.ok);
        assert!(
            out.message.contains("已自动中止并复原"),
            "冲突必须被自动收拾干净：{} / {}",
            out.message,
            out.log
        );
        assert_eq!(out_of(&f.work, &["rev-parse", "HEAD"]), head_before, "历史不动");
        assert_eq!(
            out_of(&f.work, &["stash", "list"]),
            "",
            "暂存的改动应当已经回到工作区，stash 里不该还留着"
        );
        assert_eq!(
            std::fs::read_to_string(f.work.join("base.txt")).unwrap(),
            "uncommitted edit\n",
            "未提交的改动要原样回来"
        );
    }

    #[test]
    fn undo_rebase_brings_the_branch_back() {
        let f = fixture();
        diverge(&f);
        let before = status_of(&f.work);
        let head_before = out_of(&f.work, &["rev-parse", "HEAD"]);
        let path = f.work.to_string_lossy().to_string();

        let out = rebase_core(&path, &before, false, &|a| sh(&f.work, a), &|a| {
            (sh(&f.work, a), None)
        });
        assert!(out.ok);
        assert_ne!(
            sh(&f.work, &["rev-parse", "HEAD"]).stdout.trim(),
            head_before,
            "变基应当改写了 HEAD"
        );

        // 撤销动作本身（与 undo_rebase 里同一条命令）
        assert!(sh(&f.work, &["reset", "--hard", BACKUP_BRANCH]).success);
        assert_eq!(
            sh(&f.work, &["rev-parse", "HEAD"]).stdout.trim(),
            head_before,
            "撤销后应回到变基前的位置"
        );
        // 撤销之后原始提交一条不少（都在备份分支上，没有真的丢东西）
        let saved = sh(&f.work, &["log", "--oneline", "--all"]).stdout;
        assert!(saved.contains("local work") && saved.contains("remote work"));
    }

    #[test]
    fn stash_message_is_recognizable() {
        // 用户 `git stash list` 时要能认出是谁存的。**判据不在这条测试里自己写**
        // （原来这里断的是 `STASH_MSG.starts_with("qingbird-git:")`——一个和
        // `stash::is_auto` 并行的第二判据，措辞一改它就漂），改成直接问那颗谓词
        assert!(crate::stash::is_auto(crate::stash::MSG_REBASE));
    }

    fn v(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn conflict_note_has_its_own_sentence_for_the_empty_list() {
        // 空表那句话是**独立的一句**，不是"冲突的文件："后面空着——
        // 那是 AGENTS 里「控件 disable 了但话还说错了」的同族：
        // 一个看起来像有值的空，比明说"没读到"更会骗人
        let none = conflict_note(&v(&[]));
        assert!(none.contains("没能读出"), "{none}");
        assert!(!none.contains("冲突的文件："), "空表不许披上点名那句话的壳：{none}");

        assert_eq!(conflict_note(&v(&["a.txt"])), "冲突的文件：a.txt");
        // 恰好到上限：不许多出"等 N 个"
        let five = conflict_note(&v(&["a", "b", "c", "d", "e"]));
        assert_eq!(five, "冲突的文件：a、b、c、d、e");
        assert!(!five.contains("等"), "五颗以内不该出现计数：{five}");

        // 超上限：列五颗 + 报出总数（用户据此决定去不去终端看）
        let seven = conflict_note(&v(&["a", "b", "c", "d", "e", "f", "g"]));
        assert!(seven.contains("冲突的文件：a、b、c、d、e 等 7 个"), "{seven}");
        assert!(!seven.contains("f"), "上限就是上限：{seven}");
    }

    #[test]
    fn the_conflict_query_turns_quoting_off_before_the_subcommand() {
        // 真 git 实测（32-6）：不带 `-c core.quotePath=false` 时中文文件名输出成
        // "\344\270\255\346\226\207"，用户看到的是一串八进制码而不是文件名。
        // 而这条纪律**只有顺序对了才生效**，所以这里断的是命令数组本身
        use std::cell::RefCell;
        let seen: RefCell<Vec<Vec<String>>> = RefCell::new(Vec::new());
        let out = git::GitOutput {
            success: true,
            stdout: "  src/a.rs \n\nsrc/b.rs\n".to_string(),
            stderr: String::new(),
            code: 0,
            timed_out: false,
        };
        let got = conflicted_files(&|a: &[&str]| {
            seen.borrow_mut().push(a.iter().map(|s| s.to_string()).collect());
            out.clone()
        });
        let args = &seen.borrow()[0];
        assert_eq!(args[0], "-c", "全局开关必须排在子命令之前：{args:?}");
        assert_eq!(args[1], "core.quotePath=false");
        assert_eq!(args[2], "diff");
        assert!(args.contains(&"--diff-filter=U".to_string()), "{args:?}");
        // 逐行 trim、空行丢掉（git 的输出末尾总带一个换行）
        assert_eq!(got, v(&["src/a.rs", "src/b.rs"]));

        // 命令失败 ⇒ 空表，由 `conflict_note` 去说"没点到名"，这里不编内容
        let bad = git::GitOutput {
            success: false,
            stdout: "not a git repository".into(),
            stderr: String::new(),
            code: 128,
            timed_out: false,
        };
        assert!(conflicted_files(&|_: &[&str]| bad.clone()).is_empty());
    }

    #[test]
    fn failure_message_lists_files_only_where_a_conflict_actually_happened() {
        let o = git::GitOutput {
            success: false,
            stdout: String::new(),
            stderr: "fatal: Unable to create 'x/.git/index.lock': File exists.".into(),
            code: 1,
            timed_out: false,
        };
        let files = v(&["base.txt"]);

        // ① 走到了"自动中止"那一臂：即使一颗都没读到也要说出来（用户有权知道没读到）
        let aborted_empty = rebase_failure_message(&o, None, true, &[]);
        assert!(aborted_empty.contains("已自动中止并复原"), "{aborted_empty}");
        assert!(aborted_empty.contains("没能读出"), "{aborted_empty}");

        // ② 网络那一臂根本还没走到冲突 ⇒ 不硬拼清单，拼了就是句假话
        let net = git::GitOutput {
            stderr: "fatal: unable to access 'https://github.com/': Could not resolve host".into(),
            ..o.clone()
        };
        let msg = rebase_failure_message(&net, None, false, &[]);
        assert!(msg.contains("无法连接远端"), "{msg}");
        // ⚠️ 只断 `!contains("冲突")` **测不出"无条件拼清单"那一档变异**（32-6 变异 M8
        // 实测一条都没红）：空表那句话是"这次没能读出具体是哪几个文件"，里面
        // 既没有"冲突"也没有文件名——光看关键字会以为拼上去也看不出破绽。
        // ⇒ 判据落在"网络臂这句话一个字都不许多"上，清单那两句话的特征串逐个点名
        assert!(!msg.contains("没能读出"), "网络失败不许带清单那半句：{msg}");
        assert!(!msg.contains("冲突的文件"), "同上：{msg}");

        // ③ 别的失败（锁文件之类）也没清单 ⇒ 就一句结论，不画蛇添足
        let plain = rebase_failure_message(&o, None, false, &[]);
        assert!(!plain.contains("冲突的文件"), "{plain}");

        // ④ 有清单就拼上，而且不管它是不是 aborted 臂
        let both = rebase_failure_message(&o, None, true, &files);
        assert!(both.contains("已自动中止并复原") && both.contains("base.txt"), "{both}");
        let mid = rebase_failure_message(&o, None, false, &files);
        assert!(mid.contains("base.txt"), "非中止但读到了清单也要说：{mid}");
    }
}
