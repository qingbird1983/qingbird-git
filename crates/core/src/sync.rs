//! 三键操作与提交推送：检测云端更新 / 一键拉取（--ff-only）/ 一键推送 / 提交并推送。
//! 自 src-tauri 平移：`AppHandle`/`Channel` 去除，进度改走回调闭包，设置读写走 core store。

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use crate::fork;
use crate::history::{parse_numstat, CommitFile};
use crate::net::{
    identity_config, progress_hooks, run_local, run_local_essential, run_net_op, NetMode,
};
use crate::repos::{read_status, touch_repo, valid_repo};
use crate::store;
use crate::types::{combined, DiffTotals, ForkInfo, OpResult, ProgressEvent, UpdateChanges};
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

/// 三处失败出口（检测 / 拉取 / 推送）都要判"SSH 公钥被拒"，
/// 所以取公钥路径也收成一处。**只在这条分支上被调用**——
/// 一次网络操作失败才可能需要它，平时不扫磁盘。
fn pubkey_path() -> String {
    git::existing_pubkey().unwrap_or_default()
}

/// 检查云端更新（用户触发）。on_progress 为 None 时进度静默丢弃。
pub fn check_update(path: &str, on_progress: Option<&dyn Fn(&ProgressEvent)>) -> OpResult {    let Some(_guard) = begin_user_op() else {
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
            // **公钥被拒要顶掉整句**：它是唯一一种「连上了但进不去」，
            // 说"无法连接远端"是假的（连上了），追加"未找到可用代理"更假
            message: git::ssh_auth_hint_or(&fetch_out.stderr, &pubkey_path(), &msg),
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
            // 取消落在 `stash push` 这一档时说"已取消"，不说"暂存失败"：
            // 那一步是被按停的，不是出了故障（同 `pull_verdict` 里①那一档一个道理）。
            // 这里 `stashed` 还是 false ⇒ 没有文件压在 stash 里，回滚是干净的
            let msg = if s.stderr.contains(git::CANCELLED_MSG) {
                "拉取已取消".to_string()
            } else {
                "暂存本地变更失败，已取消拉取".to_string()
            };
            return OpResult::err(msg, log);
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
    touch_repo(path);

    // ⛔ **收尾的 `stash pop` 排在所有结论之前，而且必达**。两件事一起才成立：
    // ① 顺序：拉取成功、失败、被取消——**三种结局都得把用户那些没提交的文件还回去**。
    //    原来 pop 的失败判断排在"拉取成败"之前，于是取消那一次界面说的是
    //    「拉取完成，但恢复暂存变更时冲突」——**一句里两个假话**：既没完成，也不是冲突
    // ② 必达：原来走 `run_local`，而它每 50ms 查一次进程级取消标记 ⇒
    //    用户点「停止」时标记早就置位，这条 pop 在第一个周期就被整树杀掉 ⇒
    //    **7 个未跟踪文件留在 stash 里**（2026-10-08 实测坐实，路线表 31.4 补二十一）
    let mut pop_ok: Option<bool> = None;
    if stashed {
        let pop = run_local_essential(path, &["stash", "pop"]);
        log.push_str(&combined(&pop));
        pop_ok = Some(pop.success);
    }

    let status = read_status(path);
    // 分叉用真实状态判定（ahead/behind），不去猜 stderr 里的措辞：
    // git 版本的提示语会变，但"两边各有对方没有的提交"这个事实不会
    let fork = if pull_out.success {
        None
    } else {
        fork::fork_info(path, &status, &|a| run_local(path, a))
    };
    let (ok, message) = pull_verdict(
        &pull_out,
        fork.as_ref(),
        used_proxy.as_deref(),
        pop_ok,
        git::ssh_auth_hint(&pull_out.stderr, &pubkey_path()).as_deref(),
    );
    let changes = if ok { pull_changes(path, &old_head) } else { None };
    OpResult {
        ok,
        message,
        output: log,
        used_proxy,
        status: Some(status),
        need_stash: false,
        changes,
        fork: if ok { None } else { fork },
        backup_branch: None,
    }
}

/// 拉取收尾那句话的**唯一出处**：`(成功?, 面向用户的一句话)`。
///
/// 四档判据的**顺序就是这条链上最容易做反的东西**，所以逐档钉在单测里：
/// ① **被用户取消排第一**。取消时不报"拉取失败"——那会让人以为仓库出了事，
///    而它只是被自己按停了；更不许报"拉取完成"（原来那一版就是这么说的）
/// ② 拉取本身失败：四档原因（无上游 / 公钥 / 分叉 / 网络）。
///    公钥那档排在分叉**前面**：公钥失败时压根没拉到东西，
///    **谈合流是把一个认证问题说成了历史问题**（这条顺序原来就写着，原样搬）
/// ③ 拉取成功但 stash 没恢复：**这算失败**——用户文件还压在 stash 里，
///    而"拉取成功"四个字会把这件事完全盖掉
/// ④ 全好
///
/// ⚠️ `ssh_hint` 与 `log_state_line` 的 `past` 是同一条手法：**要读配置的那个
/// 判据留在调用方，纯函数只收算好的值**——`pubkey_path()` 会去读
/// `%APPDATA%` 下那份真配置，留在函数里就没法单测（也测不稳）
fn pull_verdict(
    pull_out: &git::GitOutput,
    fork: Option<&ForkInfo>,
    used_proxy: Option<&str>,
    pop_ok: Option<bool>,
    ssh_hint: Option<&str>,
) -> (bool, String) {
    if pull_out.stderr.contains(git::CANCELLED_MSG) {
        return (false, "拉取已取消".to_string());
    }
    if !pull_out.success {
        let message = if pull_out.stderr.contains("no tracking information") {
            "当前分支没有上游分支，无法拉取".to_string()
        } else if let Some(h) = ssh_hint {
            h.to_string()
        } else if fork.is_some() {
            "本地与云端历史分叉，ff 拉取失败——可以用下面的方式合流".to_string()
        } else if used_proxy.is_none() && git::is_network_error(pull_out) {
            "拉取失败：无法连接远端，且未找到可用代理".to_string()
        } else {
            "拉取失败".to_string()
        };
        return (false, message);
    }
    if pop_ok == Some(false) {
        return (
            false,
            "拉取完成，但恢复暂存变更时冲突，请在终端执行 git stash pop 处理".to_string(),
        );
    }
    let message = if used_proxy.is_some() {
        "拉取成功（已自动通过代理完成）".to_string()
    } else {
        "拉取成功".to_string()
    };
    (true, message)
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
        totals: DiffTotals::default(),
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
    // 数字另走一趟 `--numstat`。⚠️ **不拿上面那句 `--stat` 的汇总行当数据源**
    // （「4 files changed, 5 insertions(+), 2 deletions(-)」）：那是给人看的措辞，
    // 措辞会随 git 版本变，而"制表符前两列是数字"这个结构不会。与下面分叉判据
    // 用 ahead/behind 而不猜 stderr 文本是同一条纪律。
    // 解析复用 `history::parse_numstat`（提交详情页早就在用），这里只加总
    let numstat = run_local(path, &["diff", "--numstat", &range]);
    if numstat.success {
        changes.totals = totals_of(&parse_numstat(&numstat.stdout));
    }
    // 完全没有新内容（Already up to date）时不返回更新卡片
    if changes.commits.is_empty() && changes.diff_stat.is_empty() {
        return None;
    }
    Some(changes)
}

/// 把 `--numstat` 的逐文件条目汇总成三个数。
///
/// ⚠️ **解析规则不在这里，在 `history::parse_numstat`**——那一颗早就存在，
/// 已经把 `-\t-`（二进制）算成 `None`、把 rename 的「旧 => 新」与路径里的空格
/// 留在第三列。这里**只做加总**。同一条链上再写一份解析，就是本项目数过四次
/// 的那类缺陷（「同一属性两个出处」）：以后调二进制口径会改一处漏一处。
///
/// 两条加总规则各有理由：
/// ① **二进制条目按 0 行计，但照算一个文件**——换了一批图片也是"带回了 5 个文件"，
///    报 0 个是假话；而它确实没有行数可言，所以写「+0 −0」那种假精确由
///    `has_line_counts` 挡在界面那一侧
/// ② 空 range ⇒ 空条目 ⇒ 三个数全 0，界面那一行整条不出现
fn totals_of(files: &[CommitFile]) -> DiffTotals {
    DiffTotals {
        files: files.len(),
        insertions: files.iter().filter_map(|f| f.additions).sum::<u32>() as usize,
        deletions: files.iter().filter_map(|f| f.deletions).sum::<u32>() as usize,
    }
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
            } else if let Some(h) = git::ssh_auth_hint(&push_out.stderr, &pubkey_path()) {
                h
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

    /// 拉取带回的那三个数：走 `parse_numstat` → `totals_of` **整条链**。
    ///
    /// 输入是本机造夹具**真跑出来的** `git diff --numstat` 输出
    /// （二进制那行确实是 `-\t-`，rename 确实是 `0\t0\t旧 => 新`），不是照着想的样子编的。
    /// 之所以测两层串联而不是只测加总：**解析在 `history`、加总在这里**，
    /// 中间那个「二进制 = `None`」的约定由两层共用——任何一层改了它，
    /// 单独测另一层都看不出来（同 D73：断点在两层的接缝上，不在任何一层里）
    #[test]
    fn numstat_lines_become_files_and_line_counts() {
        let t = totals_of(&parse_numstat(
            "1\t1\tbin.png\n2\t0\tf1.txt\n-\t-\treal.bin\n0\t0\twill_rename.txt => renamed.txt\n",
        ));
        assert_eq!((t.files, t.insertions, t.deletions), (4, 3, 1));
        assert!(t.has_line_counts());

        // 空 range（Already up to date）：三个数全 0，界面那一行整条不出现
        let z = totals_of(&parse_numstat(""));
        assert_eq!((z.files, z.insertions, z.deletions), (0, 0, 0));
        assert!(!z.has_line_counts());

        // 只有二进制：换了文件但 git 不给行数 ⇒ 文件数有、`has_line_counts` 假
        // （界面据此退成不带 `+A −B` 的那句，而不是写「+0 −0」那种假精确）
        let b = totals_of(&parse_numstat("-\t-\tlogo.png\n"));
        assert_eq!((b.files, b.insertions, b.deletions), (1, 0, 0));
        assert!(!b.has_line_counts());

        // 没有制表符的坏行不算文件：宁可少报，也不报个假的
        assert_eq!(totals_of(&parse_numstat("garbage\n")).files, 0);
    }

    /// 造一份 git 输出。`pull_verdict` 只看 `success` 与 `stderr` 两样
    fn out(success: bool, stderr: &str) -> git::GitOutput {
        git::GitOutput {
            success,
            stdout: String::new(),
            stderr: stderr.to_string(),
            code: if success { 0 } else { 1 },
            timed_out: false,
        }
    }

    fn fork() -> ForkInfo {
        ForkInfo {
            ahead: 1,
            behind: 2,
            dirty: false,
            branch: "main".into(),
            upstream: "origin/main".into(),
            local_commits: vec!["a1 本地".into()],
            remote_commits: vec!["b2 云端".into()],
            blocked_reason: None,
        }
    }

    /// 拉取收尾那句结论的真值表。**每一档都对应一次真实的说错话**，
    /// 而**顺序**尤其要钉：`pull_verdict` 里那四档判据谁先谁后，
    /// 决定用户看到的是"被自己按停了"还是"拉取完成，但…"。
    ///
    /// 抽成纯函数才测得到这一条——`do_pull` 里那一段要真跑 git、
    /// 还要真造一个"中途取消"的时序，那种测试写完也只能靠人点
    #[test]
    fn the_pull_verdict_puts_the_cancel_first() {
        // ① **取消排第一**。最坏的情形是说成"完成"：2026-10-08 实测就是这句——
        //    `stash pop` 被同一个取消标记带倒，而旧代码先判 pop、后判拉取 ⇒
        //    界面报「拉取完成，但恢复暂存变更时冲突」，**一句里两个假话**
        let cancelled = out(false, git::CANCELLED_MSG);
        let (ok, msg) = pull_verdict(&cancelled, None, None, Some(false), None);
        assert!(!ok);
        assert_eq!(msg, "拉取已取消");
        assert!(!msg.contains("完成"), "取消那一次不许出现「完成」：{msg}");
        assert!(!msg.contains("冲突"), "取消那一次也不是 stash 冲突：{msg}");

        // ② 公钥提示排在分叉**前面**：两样都给时用前者。
        //    分叉那档给的是"可以用下面的方式合流"，而公钥失败压根没拉到东西
        let both = out(false, "Permission denied (publickey)");
        let (ok, msg) = pull_verdict(&both, Some(&fork()), None, None, Some("请先添加公钥"));
        assert!(!ok);
        assert_eq!(msg, "请先添加公钥");

        // ③ 拉取成功但 stash 没恢复：**这算失败**，用户文件还压在 stash 里
        let (ok, msg) = pull_verdict(&out(true, ""), None, None, Some(false), None);
        assert!(!ok);
        assert!(msg.contains("git stash pop"), "{msg}");

        // ④ 全好：没 stash 过（`None`）与恢复成功（`Some(true)`）是同一档
        for pop in [None, Some(true)] {
            let (ok, msg) = pull_verdict(&out(true, ""), None, None, pop, None);
            assert!(ok);
            assert_eq!(msg, "拉取成功");
        }
        // 走代理成功要说出来：它是"连上了但绕了一圈"，混在成功里会让人以为一切正常
        let (ok, msg) = pull_verdict(&out(true, ""), None, Some("127.0.0.1:7897"), None, None);
        assert!(ok);
        assert!(msg.contains("代理"), "{msg}");

        // ② 的另外三档，各钉一条
        let (ok, msg) = pull_verdict(
            &out(false, "fatal: no tracking information for the current branch"),
            None,
            None,
            None,
            None,
        );
        assert!(!ok);
        assert!(msg.contains("上游分支"), "{msg}");
        let (ok, msg) = pull_verdict(
            &out(false, "fatal: Not possible to fast-forward"),
            Some(&fork()),
            None,
            None,
            None,
        );
        assert!(!ok);
        assert!(msg.contains("分叉"), "{msg}");
        let net = "fatal: unable to access: Failed to connect to github.com port 443";
        let (ok, msg) = pull_verdict(&out(false, net), None, None, None, None);
        assert!(!ok);
        assert!(msg.contains("未找到可用代理"), "{msg}");
        // **已经走过代理却还是失败 ⇒ 不能再报"未找到可用代理"**（那句是假话）。
        // 这一档钉的是 `used_proxy.is_none()` 那个条件——去掉它就永远说没找到代理
        let (ok, msg) = pull_verdict(&out(false, net), None, Some("127.0.0.1:7897"), None, None);
        assert!(!ok);
        assert_eq!(msg, "拉取失败");
    }

    /// 测试里派生 git 子进程要走的本地跑法（与 `git::run_git` 不同：
    /// 这里要的是"断言之前先把夹具搭好"，不需要超时与取消那套）
    fn local(dir: &std::path::Path, args: &[&str]) -> String {
        let o = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .expect("git 起不来");
        assert!(
            o.status.success(),
            "git {args:?} 失败：{}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).to_string()
    }

    /// **中途取消，用户那些没提交的文件必须回到工作区**（2026-10-08 那条真缺陷）。
    ///
    /// 这条测的是**接线**：机制那一层由 `git::an_essential_command_ignores_the_cancel_flag`
    /// 钉着，但**机制对、调用点写回 `run_local` 一样会丢文件**——而丢的是用户的东西。
    /// 原缺陷的实测症状就是这一句：拉取被按停 ⇒ `stash pop` 被同一个标记带倒 ⇒
    /// 未跟踪文件留在 stash 里，界面还报「拉取完成，但恢复暂存变更时冲突」
    ///
    /// 时序：远端指向**本机一个连得上、但 accept 之后一个字节都不回**的地址
    /// （`git::hanging_url()`，理由写在那儿——黑洞地址 `10.255.255.1` 靠的是
    /// "本机网络栈会把这条连接挂着"，2026-10-09 它 0 毫秒就拒绝，前提就没了）⇒
    /// 后台跑 `do_pull`，1500ms 后按停。断言三件事，**顺序按重要性排**：
    /// ① 那个未跟踪文件与那处修改**都还在工作区里**；② stash 是空的；
    /// ③ 结论是「拉取已取消」。
    ///
    /// ⚠️ 整段持 `GIT_TEST_LOCK` **写锁**独占——它会动那个进程级取消标记，
    /// 放着不管会连坐别的测试正在跑的 git（表现为"没有退出码、stderr 为空"）
    #[test]
    fn a_cancelled_pull_still_returns_the_stashed_files() {
        let _exclusive = crate::git::GIT_TEST_LOCK
            .write()
            .unwrap_or_else(|e| e.into_inner());
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("qbg-cancel-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        local(&root, &["init", "-q", "--initial-branch=main", "repo"]);
        let repo = root.join("repo");
        // ⚠️ **身份必须配进这个仓库的 config，不能只在 commit 时用 `-c` 传**：
        // `git stash push` 自己也要建一个 commit，没身份就整个失败 ⇒
        // 那句「暂存本地变更失败，已取消拉取」会把这条测试引到别处去（第一次跑就是这么红的）
        local(&repo, &["config", "user.email", "t@t"]);
        local(&repo, &["config", "user.name", "t"]);
        std::fs::write(repo.join("tracked.txt"), "原始\n").unwrap();
        local(&repo, &["add", "tracked.txt"]);
        local(&repo, &["commit", "-q", "-m", "base"]);
        // 远端 = 本机那个"不回话"的监听。listener 交给 `hold_the_connection` 握着，
        // **不能就地 drop**（一 drop 端口没人听，连接立刻被拒 ⇒ 前提没了）
        let (remote_url, listener) = crate::git::hanging_url();
        crate::git::hold_the_connection(listener);
        local(&repo, &["remote", "add", "origin", &remote_url]);
        local(&repo, &["config", "branch.main.remote", "origin"]);
        local(&repo, &["config", "branch.main.merge", "refs/heads/main"]);
        // 用户没提交的两样东西：`stash push -u` 会把它们**一起**收走，
        // 所以收尾那一步没跑的话，两样都会从工作区消失
        std::fs::write(repo.join("tracked.txt"), "改了没提交\n").unwrap();
        std::fs::write(repo.join("untracked.txt"), "别丢\n").unwrap();

        crate::git::clear_cancel();
        let path = repo.to_string_lossy().to_string();
        let t = std::thread::spawn(move || do_pull(&path, true, None));
        // ⚠️ **取消要落在网络那一步，不能落在 `stash push` 期间**：
        // 前面那几道本地 git（`read_status` + `rev-parse`）加起来就要几百毫秒，
        // 400ms 会正好撞在 push 上——第一次跑就是这么红的，那一次报的是
        // 「暂存本地变更失败」（跟"收尾没跑"是两件事，验不到这条链的重点）。
        // 1.5 秒时 push 早已完成、pull 还挂在那条不回话的连接上（`NET_TIMEOUT` 是 300 秒）
        std::thread::sleep(Duration::from_millis(1500));
        crate::git::request_cancel();
        let r = t.join().expect("do_pull  panicked");
        crate::git::clear_cancel();

        let got = std::fs::read_to_string(repo.join("untracked.txt"))
            .unwrap_or_default()
            .trim_end()
            .to_string();
        assert_eq!(
            got, "别丢",
            "取消之后那个未跟踪文件没回到工作区（它多半还压在 stash 里）；结论那句是「{}」",
            r.message
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("tracked.txt"))
                .unwrap()
                .trim_end(),
            "改了没提交",
            "取消之后那处修改没回来；结论那句是「{}」",
            r.message
        );
        assert!(
            local(&repo, &["stash", "list"]).trim().is_empty(),
            "stash 里还留着东西 ⇒ 收尾那一步没跑完：{}",
            local(&repo, &["stash", "list"])
        );
        assert!(!r.ok);
        assert_eq!(r.message, "拉取已取消", "取消那一次界面说的是：{}", r.message);
        let _ = std::fs::remove_dir_all(&root);
    }
}
