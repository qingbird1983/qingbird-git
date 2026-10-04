//! 三键操作与提交推送：检测云端更新 / 一键拉取（--ff-only）/ 一键推送 / 提交并推送。
//! 自 src-tauri 平移：`AppHandle`/`Channel` 去除，进度改走回调闭包，设置读写走 core store。

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use crate::fork;
use crate::net::{identity_config, progress_hooks, run_local, run_net_op, NetMode};
use crate::repos::{read_status, touch_repo, valid_repo};
use crate::store;
use crate::types::{combined, OpResult, ProgressEvent, UpdateChanges};
use crate::git;

/// 网络 git 操作超时
pub const NET_TIMEOUT: Duration = Duration::from_secs(300);
/// 检查云端更新只做 fetch + 对比，超时比完整拉取短得多
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(90);

/// 全局网络操作互斥锁：用户操作与后台自动同步互斥。
/// 自动同步只 try_lock（拿不到就跳过本轮），用户操作优先并可打断后台同步。
static OP_LOCK: Mutex<()> = Mutex::new(());

/// 后台自动同步尝试进入操作区：用户正在操作时返回 None（跳过本轮）
pub fn try_begin_op() -> Option<MutexGuard<'static, ()>> {
    OP_LOCK.try_lock().ok()
}

/// 用户网络操作进入操作区：若被后台自动同步占用，先请求取消其 git 子进程
/// 再等锁（最多约 10 秒），等不到说明有异常占用，返回 None
pub fn begin_user_op() -> Option<MutexGuard<'static, ()>> {
    if let Ok(g) = OP_LOCK.try_lock() {
        return Some(g);
    }
    git::request_cancel();
    for _ in 0..50 {
        if let Ok(g) = OP_LOCK.try_lock() {
            return Some(g);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

/// 检查云端更新（用户触发）。on_progress 为 None 时进度静默丢弃。
pub fn check_update(path: &str, on_progress: Option<&dyn Fn(&ProgressEvent)>) -> OpResult {
    let Some(_guard) = begin_user_op() else {
        return OpResult::err("后台自动同步正在进行，已请求中断，请稍后重试".into(), String::new());
    };
    do_check(path, on_progress)
}

/// 检查云端更新的核心流程（用户操作与后台自动同步共用）
pub fn do_check(path: &str, on_progress: Option<&dyn Fn(&ProgressEvent)>) -> OpResult {
    if !valid_repo(path) {
        return OpResult::err("该文件夹不是 git 仓库".into(), String::new());
    }
    if let Some(cb) = on_progress {
        cb(&ProgressEvent {
            stage: "正在连接远端…".into(),
            detail: String::new(),
            percent: None,
            // 这是我们自己说的提示，不是 git 输出，所以 raw 留空
            raw: String::new(),
            to_log: true,
        });
    }
    let (_tag, forward, stage) = progress_hooks(on_progress);
    let (fetch_out, used_proxy) = run_net_op(
        path,
        &["fetch", "--all", "--prune", "--progress"],
        Some(&forward),
        None,
        CHECK_TIMEOUT,
        true,
        Some(&stage),
        NetMode::Auto,
    );
    let status = read_status(path);
    touch_repo(path);

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
            fork: None,
            backup_branch: None,
        };
    }

    let message = if !status.has_upstream {
        "当前分支没有设置上游分支，无法对比云端更新".to_string()
    } else if status.behind > 0 && status.ahead > 0 {
        format!(
            "云端有 {0} 个新提交，同时本地有 {1} 个未推送提交",
            status.behind, status.ahead
        )
    } else if status.behind > 0 {
        format!("云端有 {} 个新提交，可以拉取", status.behind)
    } else if status.ahead > 0 {
        format!("已是最新，本地领先 {} 个提交待推送", status.ahead)
    } else {
        "已是最新，无需同步".to_string()
    };
    // 检测阶段就发现分叉也要给出口：多数用户是从"检测更新"这里撞上分叉的
    let fork = fork::fork_info(path, &status, &|a| run_local(path, a));
    OpResult {
        ok: true,
        message,
        output: combined(&fetch_out),
        used_proxy,
        status: Some(status),
        need_stash: false,
        changes: None,
        fork,
        backup_branch: None,
    }
}

/// 一键拉取（用户触发）
pub fn pull_repo(
    path: &str,
    stash: bool,
    on_progress: Option<&dyn Fn(&ProgressEvent)>,
) -> OpResult {
    let Some(_guard) = begin_user_op() else {
        return OpResult::err("后台自动同步正在进行，已请求中断，请稍后重试".into(), String::new());
    };
    do_pull(path, stash, on_progress)
}

/// 拉取的核心流程（用户操作与后台自动同步共用），永远 --ff-only
pub fn do_pull(
    path: &str,
    stash: bool,
    on_progress: Option<&dyn Fn(&ProgressEvent)>,
) -> OpResult {
    if !valid_repo(path) {
        return OpResult::err("该文件夹不是 git 仓库".into(), String::new());
    }
    let (_tag, forward, forward_stage) = progress_hooks(on_progress);
    let status_before = read_status(path);
    if status_before.dirty && !stash {
        return OpResult {
            ok: false,
            message: "本地有未提交的变更，直接拉取可能冲突".to_string(),
            output: String::new(),
            used_proxy: None,
            status: Some(status_before),
            need_stash: true,
            changes: None,
            fork: None,
            backup_branch: None,
        };
    }

    let old_head = run_local(path, &["rev-parse", "HEAD"])
        .stdout
        .trim()
        .to_string();

    let mut log = String::new();
    let mut stashed = false;
    if stash && status_before.dirty {
        let s = run_local(
            path,
            &["stash", "push", "-u", "-m", "qingbird-git auto stash"],
        );
        log.push_str(&combined(&s));
        if !s.success {
            return OpResult::err("暂存本地变更失败，已取消拉取".into(), log);
        }
        stashed = true;
    }

    let (pull_out, used_proxy) = run_net_op(
        path,
        &["pull", "--ff-only", "--progress"],
        Some(&forward),
        None,
        NET_TIMEOUT,
        true,
        Some(&forward_stage),
        NetMode::Auto,
    );
    log.push_str(&combined(&pull_out));
    let mut status = read_status(path);
    touch_repo(path);

    if stashed {
        let pop = run_local(path, &["stash", "pop"]);
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
                fork: None,
                backup_branch: None,
            };
        }
    }

    if !pull_out.success {
        // 分叉用真实状态判定（ahead/behind），不去猜 stderr 里的措辞：
        // git 版本的提示语会变，但"两边各有对方没有的提交"这个事实不会
        let fork = fork::fork_info(path, &status, &|a| run_local(path, a));
        let message = if pull_out.stderr.contains("no tracking information") {
            "当前分支没有上游分支，无法拉取".to_string()
        } else if fork.is_some() {
            "本地与云端历史分叉，ff 拉取失败——可以用下面的方式合流".to_string()
        } else if used_proxy.is_none() && git::is_network_error(&pull_out) {
            "拉取失败：无法连接远端，且未找到可用代理".to_string()
        } else {
            "拉取失败".to_string()
        };
        return OpResult {
            ok: false,
            message,
            output: log,
            used_proxy,
            status: Some(status),
            need_stash: false,
            changes: None,
            fork,
            backup_branch: None,
        };
    }

    status = read_status(path);
    let changes = pull_changes(path, &old_head);
    let message = if used_proxy.is_some() {
        "拉取成功（已自动通过代理完成）".to_string()
    } else {
        "拉取成功".to_string()
    };
    OpResult {
        ok: true,
        message,
        output: log,
        used_proxy,
        status: Some(status),
        need_stash: false,
        changes,
        fork: None,
        backup_branch: None,
    }
}

/// 对比拉取前的 HEAD，提取本次拉下来的提交列表与文件变更摘要
fn pull_changes(path: &str, old_head: &str) -> Option<UpdateChanges> {
    if old_head.is_empty() {
        return None;
    }
    let range = format!("{old_head}..HEAD");
    let mut changes = UpdateChanges {
        commits: Vec::new(),
        diff_stat: String::new(),
    };

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

/// 一键推送（用户触发）
pub fn push_repo(path: &str) -> OpResult {
    if !valid_repo(path) {
        return OpResult::err("该文件夹不是 git 仓库".into(), String::new());
    }
    let Some(_guard) = begin_user_op() else {
        return OpResult::err("后台自动同步正在进行，已请求中断，请稍后重试".into(), String::new());
    };
    do_push(path)
}

/// 推送的核心流程（push_repo 与 commit_push 共用）
fn do_push(path: &str) -> OpResult {
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
            fork: None,
            backup_branch: None,
        };
    }
    if !status.has_upstream {
        let remotes = run_local(path, &["remote"]);
        if remotes.stdout.trim().is_empty() {
            return OpResult::err(
                "该仓库没有配置远端仓库，无法推送".into(),
                combined(&remotes),
            );
        }
    }

    let args: Vec<&str> = if status.has_upstream {
        vec!["push", "--progress"]
    } else {
        vec!["push", "--progress", "--set-upstream", "origin", "HEAD"]
    };
    let (push_out, used_proxy) = run_net_op(path, &args, None, None, NET_TIMEOUT, true, None, NetMode::Auto);
    let status = read_status(path);
    touch_repo(path);

    if !push_out.success {
        // 推送被拒多半是对面又前进了：这时同样要给分叉出口，而不是丢一句"请先拉取"
        let message =
            if push_out.stderr.contains("fetch first") || push_out.stderr.contains("rejected") {
                "推送被拒绝：云端有新提交，请先拉取".to_string()
            } else if used_proxy.is_none() && git::is_network_error(&push_out) {
                "推送失败：无法连接远端，且未找到可用代理".to_string()
            } else if push_out.stderr.contains("could not read Username")
                || push_out.stderr.contains("Authentication failed")
            {
                "推送失败：认证未通过，可在设置里填写对应平台的访问令牌".to_string()
            } else {
                "推送失败".to_string()
            };
        let fork = fork::fork_info(path, &status, &|a| run_local(path, a));
        return OpResult {
            ok: false,
            message,
            output: combined(&push_out),
            used_proxy,
            status: Some(status),
            need_stash: false,
            changes: None,
            fork,
            backup_branch: None,
        };
    }

    let message = if used_proxy.is_some() {
        "推送成功（已自动通过代理完成）".to_string()
    } else {
        "推送成功".to_string()
    };
    OpResult {
        ok: true,
        message,
        output: combined(&push_out),
        used_proxy,
        status: Some(status),
        need_stash: false,
        changes: None,
        fork: None,
        backup_branch: None,
    }
}

/// 一键提交并推送：add -A → commit（自动带 Git 身份）→ push
pub fn commit_push(path: &str, message: &str) -> OpResult {
    commit_push_with(path, message, false)
}

/// 同 [`commit_push`]，但 `pull_first` 为真时在 commit 之后、push 之前插一次
/// `--ff-only` 拉取。
///
/// **这个选项解决的是一条死胡同**：本地落后云端时，`push` 一定被拒
/// （non-fast-forward），用户得自己发现"要先拉"、切到仓库、手动拉一次、
/// 再回来点推送。把它并进一次操作里，那条死路就没有了。
///
/// 拉取**必须带 `--ff-only`**：这里只解决"接上远端的提交"，
/// 一旦需要改写本地历史，那是变基向导的活，得由用户显式点进去。
pub fn commit_push_with(path: &str, message: &str, pull_first: bool) -> OpResult {
    if !valid_repo(path) {
        return OpResult::err("该文件夹不是 git 仓库".into(), String::new());
    }
    let Some(_guard) = begin_user_op() else {
        return OpResult::err("后台自动同步正在进行，已请求中断，请稍后重试".into(), String::new());
    };
    let msg = message.trim();
    if msg.is_empty() {
        return OpResult::err("请先填写提交说明".into(), String::new());
    }
    let st = read_status(path);
    if !st.dirty {
        return OpResult {
            ok: true,
            message: "没有可提交的变更，直接推送即可".to_string(),
            output: String::new(),
            used_proxy: None,
            status: Some(st),
            need_stash: false,
            changes: None,
            fork: None,
            backup_branch: None,
        };
    }

    let cfg = store::load();
    let add = run_local(path, &["add", "-A"]);
    if !add.success {
        return OpResult::err("添加变更文件失败".into(), combined(&add));
    }

    let cargs = identity_config(&cfg);
    let mut argv: Vec<&str> = cargs.iter().map(|s| s.as_str()).collect();
    argv.extend(["commit", "-m", msg]);
    let commit = run_local(path, &argv);
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

    let mut result = if pull_first {
        // 借用 do_push 之前先放掉自己这把锁：do_push 不自己加锁，
        // 而 do_pull 内部会走 run_net_op（那是另一把网络锁）
        drop(_guard);
        let pulled = do_pull(path, true, None);
        if !pulled.ok {
            // 拉取失败就别硬推了：推上去一定被拒，不如把拉取的真实原因交出去
            return pulled;
        }
        do_push(path)
    } else {
        do_push(path)
    };
    result.output = format!("{}{}", combined(&commit), result.output);
    if result.ok {
        result.message = format!("已提交并推送：「{msg}」");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn op_lock_is_reentrant_user_side() {
        // begin_user_op 在没有并发占用时必然成功
        let g = begin_user_op();
        assert!(g.is_some());
        // 持有期间 try_begin_op（后台视角）拿不到
        assert!(try_begin_op().is_none());
        drop(g);
        assert!(try_begin_op().is_some());
    }
}
