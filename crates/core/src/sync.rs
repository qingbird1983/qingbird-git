//! 三键操作与提交推送：检测云端更新 / 一键拉取（--ff-only）/ 一键推送 / 提交并推送。
//! 自 src-tauri 平移：`AppHandle`/`Channel` 去除，进度改走回调闭包，设置读写走 core store。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

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

/// 每仓库一个忙槽。
///
/// **为什么把锁从进程级降下来**（路线表 32-8）：原来那颗 `OP_LOCK: Mutex<()>`
/// 让「整个应用同一时刻只放一个忙操作」成了所有上层设计的**隐含前提**——
/// 批量只能串行（`gui/src/ops.rs` 的注释明写这就是理由）、后台自动同步与用户操作
/// 只能二选一、而"两个不同仓库并发写各自的 `.git`"本来毫无冲突却被一并挡住。
/// 仓库之间不共享 `.git`，**串行不是必需，是被锁的形状逼出来的**。
///
/// ⚠️ **全局并发上限这一轮故意没有**：锁下沉之后并发并不会自己变多（界面还是一次
/// 点一键、后台仍一仓一仓跑），先加一个没人消费的限额只是多一处会漂的数。
/// 它的消费者是 32-9 批量并发——**那一批必须连着把上限一起落**，
/// 否则一次批量就是同时派生 17 个 git。已登记进 Q28。
static SLOTS: OnceLock<Mutex<HashMap<String, Arc<Slot>>>> = OnceLock::new();

/// 一个仓库的忙位。**取消标记不在这里**：那一份只有子进程轮询那一层需要读
/// （`git::run_git_inner`），放在 `git::CANCELLED`。两处共用 [`git::repo_key`]
/// 这**一个**键的出处——各写一份归一，就会出现"锁拿到了、取消却打不到"。
struct Slot {
    busy: AtomicBool,
}

fn slots() -> &'static Mutex<HashMap<String, Arc<Slot>>> {
    SLOTS.get_or_init(Default::default)
}

/// 取（或建）这个仓库的槽。
///
/// 表只增不减：一条 40 字节上下，而它的上界就是用户添加过的仓库数（本机 17 个）。
/// 移除仓库后留着那条**不影响正确性**——槽空着，重建同名仓库拿回同一个键。
fn slot_for(repo_path: &str) -> Arc<Slot> {
    slots()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(git::repo_key(repo_path))
        .or_insert_with(|| Arc::new(Slot { busy: AtomicBool::new(false) }))
        .clone()
}

/// **同时在跑的忙操作上限**（路线表 Q28，2026-10-10 定 4 路）。
///
/// 为什么是 4 而不是"越多越好"：批量并发真正的瓶颈不是 CPU，也不是磁盘，
/// 而是**同时开十几个 git 进程去连同一个远端**——github 一类远端对同账号 / 同 IP
/// 的并发请求会限流，路数越高越容易撞 429 / 502，而撞上的表现是"个别仓库拉取失败"，
/// 用户只会以为程序不稳。17 个仓库在 4 路上已经从"分钟级"掉到十几秒，
/// 再往上收益递减、失败率上升。⇒ **要改只改这一颗数**，别在调用处各写一遍。
pub const MAX_CONCURRENT_OPS: usize = 4;

/// 在途路数。用 `isize` 而不是 `usize`：还票写错（多还一次）时 `usize` 会**下溢成一个
/// 巨大值** ⇒ 上限从此形同虚设而且看不出来；`isize` 会跑到负数，被下面那条
/// `debug_assert` 当场抓住（release 下它是空操作，所以真正的防线是
/// "票只在 `OpGuard` 的 `Drop` 里还一次"这个结构）
static IN_FLIGHT: AtomicIsize = AtomicIsize::new(0);

/// 取一票额度。拿不到返回 `false`——**不排队**，排队是调用方的事
fn acquire_credit() -> bool {
    let mut cur = IN_FLIGHT.load(Ordering::SeqCst);
    loop {
        if cur >= MAX_CONCURRENT_OPS as isize {
            return false;
        }
        match IN_FLIGHT.compare_exchange_weak(
            cur,
            cur + 1,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => return true,
            Err(actual) => cur = actual,
        }
    }
}

fn release_credit() {
    let after = IN_FLIGHT.fetch_sub(1, Ordering::SeqCst) - 1;
    debug_assert!(after >= 0, "全局并发额度被多还了一次，现在 {after}");
}

/// 撞同一颗仓库的槽时最多等多久。原来写成"50 次 × 200 毫秒"两个魔法数，
/// 收成一颗数才好读也好测
const SLOT_WAIT: Duration = Duration::from_secs(10);
const SLOT_POLL: Duration = Duration::from_millis(200);
/// 等额度时的轮询间隔。比等槽密：额度是"前面那路跑完就有"，通常几秒内变化
const CREDIT_POLL: Duration = Duration::from_millis(50);

/// 一次忙操作的凭证：持有期间这个仓库不许有第二个忙操作，并且占一份全局额度。
/// `Drop` 把**两样一起还**。
///
/// 类型是自己写的而不是 `MutexGuard`：**守卫要能在函数间移动、且活到 `'static`，
/// 而 `Mutex<HashMap<..>>` 的守卫借用那张表，会把表的锁一路带进整条调用链**
/// （连 git 子进程跑那几十秒都攥着表锁，别的仓库连"查一下自己忙不忙"都做不了）。
pub struct OpGuard {
    slot: Option<Arc<Slot>>,
    /// 这一票额度。**漏还一次，整个应用就永久少一路并发**，
    /// 症状是"越用越慢、最后又变回一次只能做一件事"，而没有任何一处会报错。
    /// ⇒ 它只在 `Drop` 里还、且 `Drop` 之外没有任何手工还票的出口
    credit: bool,
}

impl Drop for OpGuard {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            slot.busy.store(false, Ordering::SeqCst);
        }
        if self.credit {
            release_credit();
        }
    }
}

/// 抢这颗仓库的槽。抢到返回它的 `Arc`，抢不到返回 `None`——**不碰额度**，
/// 还不还由调用方决定（`begin_user_op` 要还回去重试，`try_begin_op` 也还回去）
fn take_slot(repo_path: &str) -> Option<Arc<Slot>> {
    let slot = slot_for(repo_path);
    slot.busy
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .ok()
        .map(|_| slot)
}

/// 后台自动同步尝试进入**那个仓库**的操作区：额度满了、或那颗仓库正被用户操作，
/// 都返回 `None`（跳过这一轮，**不排队也不等待**——那是用户操作那条路的特权）
pub fn try_begin_op(repo_path: &str) -> Option<OpGuard> {
    if !acquire_credit() {
        return None;
    }
    let Some(slot) = take_slot(repo_path) else {
        release_credit();
        return None;
    };
    // ⛔ **清除只能放在这里，不能放回 `run_net_op` 开头**。理由写在
    // `git::clear_cancel_for` 那条注释里（一句话：那个位置把"清残留取消意图"
    // 外包给了"这条命令恰好走网络层"，而拉取在网络层之前要跑三条本地命令 ⇒
    // 用户按过一次停止之后，这个仓库每次都死在第一步）。
    // 拿到槽 ⇒ 同仓库没有别人在跑 ⇒ 此刻存在的标记只可能是上一次留下的。
    git::clear_cancel_for(repo_path);
    Some(OpGuard { slot: Some(slot), credit: true })
}

/// 用户对**某个仓库**发起操作。**两段等待语义不同，顺序也不能反**：
///
/// ① **额度**（全应用最多 [`MAX_CONCURRENT_OPS`] 路）——这是**排队**：
///   批量里第 5 颗等前面跑完是设计上的行为，等到有额度为止；
///   上限取 `NET_TIMEOUT`（等不到一条完整网络操作的时间，就算真被占死了）。
/// ② **这颗仓库的槽**——这是"它正被别的操作占着"：先请求取消**它那一个**
///   正在跑的 git 子进程，再等 [`SLOT_WAIT`]，等不到就报忙（不能无限等，
///   一次异常占用会把这条仓库钉死）。
///
/// ⛔ **必须先额度后槽**。反过来写会"占着额度排队等槽"：4 个额度全被这种等待占住，
/// 而它们各自等的那颗仓库正跑在第五个操作上、第五个又拿不到额度
/// ⇒ **自己造出来的死锁**，而且它不报任何错，只是界面从此不动。
/// 所以抢不到槽一定要**把额度还回去**再重试。
pub fn begin_user_op(repo_path: &str) -> Option<OpGuard> {
    let credit_until = Instant::now() + NET_TIMEOUT;
    let slot_deadline = Instant::now() + SLOT_WAIT;
    let mut asked = false;
    loop {
        if acquire_credit() {
            if let Some(slot) = take_slot(repo_path) {
                git::clear_cancel_for(repo_path);
                return Some(OpGuard { slot: Some(slot), credit: true });
            }
            release_credit();
            if !asked {
                // **只打一次**取消。`request_cancel_for` 是"请它停下"，
                // 每 200ms 重发一次不会更快，反而让这个意图一直挂着
                git::request_cancel_for(repo_path);
                asked = true;
            }
            if Instant::now() >= slot_deadline {
                return None;
            }
            std::thread::sleep(SLOT_POLL);
        } else {
            if Instant::now() >= credit_until {
                return None;
            }
            std::thread::sleep(CREDIT_POLL);
        }
    }
}

/// 抢不到槽时给的那句话。**别说成"后台自动同步正在进行"**——那是进程级锁时代的
/// 唯一可能来源；槽按仓库分之后，占着它的也可能是这个仓库自己的上一次操作。
/// 话说不准的后果不是报错，是用户去等一个根本没有在跑的后台任务。
pub(crate) const BUSY_MSG: &str = "这个仓库还有另一个操作没结束（也许是后台自动同步），\
                                   已请求中断它，请稍后重试";

/// 三处失败出口（检测 / 拉取 / 推送）都要判"SSH 公钥被拒"，
/// 所以取公钥路径也收成一处。**只在这条分支上被调用**——
/// 一次网络操作失败才可能需要它，平时不扫磁盘。
fn pubkey_path() -> String {
    git::existing_pubkey().unwrap_or_default()
}

/// 检查云端更新（用户触发）。on_progress 为 None 时进度静默丢弃。
pub fn check_update(path: &str, on_progress: Option<&dyn Fn(&ProgressEvent)>) -> OpResult {
    let Some(_guard) = begin_user_op(path) else {
        return OpResult::err(BUSY_MSG.into(), String::new());
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
    let Some(_guard) = begin_user_op(path) else {
        return OpResult::err(BUSY_MSG.into(), String::new());
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
        // ⛔ **`stash push` 与收尾的 `stash pop` 是一对，essential 化必须一起做**。
        // 补二十一（2026-10-08）把 pop 改成"用户按了停止也照样跑完"，
        // 而 push 一直还是可取消的 `run_local` ⇒ 取消落在 push 期间时，git 已经
        // 打了 "Saved working directory…"、把文件收进了 stash，却在**清理工作区
        // 那一步被 kill**：`stashed` 还是 false ⇒ 收尾那句 pop 根本不跑，
        // 而 `pop_ok` 也是 None ⇒ 连"文件还压在暂存里"那一句都不说
        // ⇒ 用户的东西**凭空从工作区消失，界面一个字都不提**。
        // （2026-10-10 由一条偶发红抓出来：全量并行六轮红三轮，聚焦单跑十轮全绿——
        // 红不红只取决于有没有别的使用者在抢 CPU，而抢到的那一次正好把停止
        // 按进 push 的收尾里。见路线表 32-9 落地账。）
        //
        // 判据不是新发明的：`net.rs::run_local_essential` 那条写的就是
        // **"只给这一步不做完、用户的东西就回不来的命令用"**——push 半途被杀
        // 留下的状态连 git 自己都说不清，它比 pop 更符合这一条
        let s = run_local_essential(
            path,
            &["stash", "push", "-u", "-m", crate::stash::MSG_PULL],
        );
        log.push_str(&combined(&s));
        if !s.success {
            // essential 命令**不会**返回 `CANCELLED_MSG`（机制那条单测钉着），
            // 所以这一档只剩一种解释：暂存本身出了故障。
            // 这里 `stashed` 还是 false ⇒ 没有文件压在 stash 里，回滚是干净的
            return OpResult::err("暂存本地变更失败，已取消拉取".to_string(), log);
        }
        stashed = true;
    }

    let (pull_out, used_proxy) = run_net_op(
        path,
        &["pull", "--ff-only", "--progress"],
        Some(&forward),
        None,
        NET_TIMEOUT,
        Some(&forward_stage),
        NetMode::Auto,
    );
    log.push_str(&combined(&pull_out));
    touch_repo(path);

    // ⛔ **收尾的 `stash pop` 排在所有结论之前，而且必达**。两件事一起才成立：
    // ① 顺序：拉取成功、失败、被取消——**三种结局都得把用户那些没提交的文件还回去**。
    //    原来 pop 的失败判断排在"拉取成败"之前，于是取消那一次界面说的是
    //    「拉取完成，但恢复暂存变更时冲突」——**一句里两个假话**：既没完成，也不是冲突
    // ② 必达：原来走 `run_local`，而它每 50ms 查一次那个仓库的取消标记 ⇒
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

/// 「替你收起来的改动没放回去」那一句追加话。**三档共用一份**（取消 / 拉取失败 /
/// 拉取完成但 pop 冲突），所以它是常量而不是某一句的一部分——
/// 写在句子里的那个版本只会出现在第三档，前两档"忘了说"正是 32-2 查实的那条静默档。
///
/// ⚠️ **它指的路必须真的存在**：那句"点仓库名字后面那颗橙色三角"说的是
/// `gui/src/widgets.rs::warn_dot` 挂的头卡告警点，而它出现的判据是
/// `RepoStatus::stash_count > 0`（`repos::read_status` 里读 `stash::count`）。
/// ⇒ 这三处是**同一件事的三个环节**，改任何一处要回头对另外两处
const PARKED_TAIL: &str = " 另外：替你先收起来的改动没能放回去，还压在暂存里——\
                            点仓库名字后面那颗橙色三角，打开「收起来的改动」清单取回。";

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
/// ⚠️ ①②③ 三档**都要拼 [`PARKED_TAIL`]**（`pop_ok == Some(false)` 时）。
/// 原来只有③拼，于是"拉取失败 × pop 也失败"这一档只说"拉取失败"——
/// **那一档里用户丢的是文件，不是面子**
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
    // ⚠️ **三档结论各算自己的那半句，最后统一拼 `PARKED_TAIL`**。
    // 以前是"取消"与"失败"两档**先 return**、只有第三档提 stash ⇒
    // 「拉取失败或被取消」×「pop 又没成功」这个组合下，界面只说"拉取失败"，
    // **一个字都不提那些文件还压在暂存里**（32-2 查实的那条静默档：
    // 补二十一修的是"pop 必达"，这一档是必达了但没成功，文件照样不在工作区）。
    // 拼在最后的结构让"忘了提"没有选项
    let head = if pull_out.stderr.contains(git::CANCELLED_MSG) {
        "拉取已取消".to_string()
    } else if !pull_out.success {
        if pull_out.stderr.contains("no tracking information") {
            "当前分支没有上游分支，无法拉取".to_string()
        } else if let Some(h) = ssh_hint {
            h.to_string()
        } else if fork.is_some() {
            "本地与云端历史分叉，ff 拉取失败——可以用下面的方式合流".to_string()
        } else if used_proxy.is_none() && git::is_network_error(pull_out) {
            "拉取失败：无法连接远端，且未找到可用代理".to_string()
        } else {
            "拉取失败".to_string()
        }
    } else if pop_ok == Some(false) {
        "拉取完成，但恢复暂存变更时冲突".to_string()
    } else if used_proxy.is_some() {
        return (true, "拉取成功（已自动通过代理完成）".to_string());
    } else {
        return (true, "拉取成功".to_string());
    };
    // ⚠️ **只有"真的收起来过、又没放回去"才拼那一句**。`None` = 压根没 stash 过，
    // `Some(true)` = 已经放回去了 ⇒ 这两种情况下拼上就是**凭空定罪**
    // （对着一个从没暂存过的仓库说"你的改动还压在暂存里"）。
    // 判据收成一处，三档共用
    let tail = matches!(pop_ok, Some(false)).then_some(PARKED_TAIL).unwrap_or("");
    (false, head + tail)
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
    let Some(_guard) = begin_user_op(path) else {
        return OpResult::err(BUSY_MSG.into(), String::new());
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
    let (push_out, used_proxy) = run_net_op(path, &args, None, None, NET_TIMEOUT, None, NetMode::Auto);
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
    let Some(_guard) = begin_user_op(path) else {
        return OpResult::err(BUSY_MSG.into(), String::new());
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

    /// 额度是**全局的一扇门**：`MAX_CONCURRENT_OPS` 路同时持有没问题，
    /// 多一路就进不来，而**还一票就得放得进一票**。
    ///
    /// 这条测的是 32-9 新加的那道闸门。用 `try_begin_op` 而不是 `begin_user_op`：
    /// 前者撞满立刻返回 `None`，后者会排队等（等到 `NET_TIMEOUT` 才放弃）——
    /// **拿"会等的那条路"去测"满不满"，测出来的永远是"它还在等"**。
    ///
    /// ⚠️ 整段持 `GIT_TEST_LOCK` **写锁**独占，理由与取消测试不同：
    /// 额度是**进程级计数**，而 `stash.rs` 那几条用例走 `apply_one` ⇒
    /// `begin_user_op` ⇒ 各占一票。不独占的话这里会假红——
    /// 而红的不是上限，是隔壁占掉的票（"前提不能外包给环境"的同族）
    #[test]
    fn the_credit_gate_holds_exactly_max_concurrent_ops() {
        let _exclusive = crate::git::GIT_TEST_LOCK
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let base = "qbgcredit://";
        let mut held = Vec::new();
        // ⚠️ 循环上界**故意不写 `0..MAX_CONCURRENT_OPS`**：写成它的话，
        // 把上限变异成 `usize::MAX` 就变成要跑 1.8e19 次——那不是"红在预期的那条"，
        // 是测试自己挂住（本轮第一次跑变异就撞在这上面，四条里三条报"退出码 101、
        // 失败 0 条"，看着像判据坏，其实是脚本分不清"编译失败 / 挂住 / 真红"）
        const PROBES: usize = 8;
        for i in 0..PROBES {
            match try_begin_op(&format!("{base}{i}")) {
                Some(guard) => held.push(guard),
                None => break,
            }
        }
        assert_eq!(
            held.len(),
            MAX_CONCURRENT_OPS,
            "一次能拿到的票数不等于上限（拿到 {} 票）⇒ 要么限流整条没了，要么闸门漏了",
            held.len()
        );
        assert!(
            try_begin_op(&format!("{base}overflow")).is_none(),
            "额度用满之后还放得进第 {} 路 ⇒ 上限形同虚设，\
             批量会一次开十几个 git 进程去撞远端限流",
            MAX_CONCURRENT_OPS + 1
        );
        // 还一票必须放得进一票。**漏还的表现不是报错，是"越用越慢、
        // 最后又变回一次只能做一件事"**——所以这一半比上面那一半更要紧
        held.pop();
        assert!(
            try_begin_op(&format!("{base}overflow")).is_some(),
            "还了一票之后第 {} 路仍进不来 ⇒ 额度漏还了",
            MAX_CONCURRENT_OPS + 1
        );
        drop(held);
        // 全部放开之后，一整轮上限都得进得来（防"计数没归零"）
        let mut again = Vec::new();
        for i in 0..PROBES {
            match try_begin_op(&format!("{base}again{i}")) {
                Some(guard) => again.push(guard),
                None => break,
            }
        }
        assert_eq!(
            again.len(),
            MAX_CONCURRENT_OPS,
            "释放之后上限没能完整恢复（只拿到 {} 票）",
            again.len()
        );
    }

    /// 上限那几路是**真的同时在场**，不是拿到票之后仍然排队。
    ///
    /// 上面那条测"闸门发几张票"，这条测"票发出去之后有没有人一起跑"——
    /// 两件事可以各自都对：假如槽仍然串在一把进程级锁上，
    /// `try_begin_op` 照样能一次把 4 票发满（它发完就返回），
    /// 于是**上限那条全绿而批量还是逐颗跑**。这是 D73 的第三型：
    /// 结构对、末端没并行
    ///
    /// ⚠️ 判据取**同时在场的最大人数**，不取总耗时：耗时要靠"这机器跑多快"，
    /// 又是一条外包给环境的前提（AGENTS 明写的那条）。这里的形状是有界的：
    /// 每人在自己那一段里停 250ms 就走，谁也不会被卡住 ⇒
    /// **串行会得到 1、并行会得到上路数**，两种结局都能当场判定，不会挂住用例。
    /// 路数取 `min(上限, 8)`：写成上限本身，把上限变异成 40 就会起 40 个线程、
    /// 各自在 `begin_user_op` 里等满 10 秒——那还是"测试自己变成前提"
    #[test]
    fn the_credit_actually_runs_that_many_at_once() {
        // 只有这条用例用得到它，导在函数里 ⇒ 生产构建不会多一条 unused import
        use std::sync::atomic::AtomicUsize;
        // 与另外两条占用类用例同理：额度是进程级计数，必须独占
        let _exclusive = crate::git::GIT_TEST_LOCK
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let roads = MAX_CONCURRENT_OPS.min(8);
        let peak = Arc::new(AtomicUsize::new(0));
        let inside = Arc::new(AtomicUsize::new(0));
        let mut joins = Vec::new();
        for i in 0..roads {
            let (peak, inside) = (Arc::clone(&peak), Arc::clone(&inside));
            // 每颗仓库用自己那一个假路径（不碰磁盘）：共用一个键就变成在测互斥
            let path = format!("qbgconcurrent://{i}");
            joins.push(std::thread::spawn(move || {
                // 拿不到也继续往下走：那样 peak 会小于路数、由断言报出来，
                // 而不是在这里 panic 掉其余几路（ panic 会把"为什么"糊掉）
                if let Some(_guard) = begin_user_op(&path) {
                    let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(250));
                    inside.fetch_sub(1, Ordering::SeqCst);
                }
            }));
        }
        for j in joins {
            j.join().ok();
        }
        assert_eq!(
            peak.load(Ordering::SeqCst),
            roads,
            "同时在场最多只有 {} 路（上路数 {}）⇒ 上限发了票却没人一起跑，\
             批量仍然是逐颗跑：界面会显示「1/17」很久",
            peak.load(Ordering::SeqCst),
            roads
        );
    }

    /// 忙槽是**按仓库**的：同一颗仓库仍然互斥，别的仓库不再被牵连。
    ///
    /// 这条测试钉的是 32-8 的全部内容。它的反面就是旧形状：一颗进程级
    /// `Mutex<()>` 让"整个应用同时只有一个忙操作"变成所有上层设计的隐含前提，
    /// 于是批量只能串行、后台自动同步与用户操作只能二选一。
    ///
    /// ⚠️ 每条用例各用自己那一串假路径（不碰磁盘，也不需要真是仓库）：
    /// `cargo test` 并行跑，两条用例共用一个路径就会互相抢槽，
    /// 表现成"红一下好一下"的 flaky——那正是本项目最恨的东西
    #[test]
    fn the_busy_slot_is_per_repo() {
        // 额度是进程级计数 ⇒ 与上面那条同理，要独占
        let _exclusive = crate::git::GIT_TEST_LOCK
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let mine = "qbgslot://per-repo-mutex";
        let other = "qbgslot://per-repo-other";
        let guard = begin_user_op(mine).expect("无占用时用户入口必然拿到槽");
        // 持有期间后台视角（`try_begin_op`）拿不到同一颗仓库
        assert!(try_begin_op(mine).is_none());
        // 另一颗仓库照样进得来——**这条在旧的全局锁下必红**
        assert!(try_begin_op(other).is_some(), "另一个仓库被无辜挡住了");
        drop(guard);
        assert!(try_begin_op(mine).is_some(), "释放之后同一颗仓库进不来");
    }

    /// 同一颗仓库的三种写法必须算**同一把槽**。
    ///
    /// 这条不是洁癖：界面上同一仓库从列表、从缓存、从拖放各来一种写法是常态。
    /// 归一不彻底的症状**不是"报错"**，是两种更坏的：
    /// ① 同一仓库长出两个槽 ⇒ **真的并发写同一个 `.git`**（32-1 那条撞墙的老账）；
    /// ② 锁按一种写法分、取消按另一种写法打 ⇒ **停止键按下去什么都不发生**。
    #[cfg(windows)]
    #[test]
    fn one_repo_written_three_ways_occupies_one_slot() {
        // 同样要独占：这一条也占额度（`begin_user_op`）
        let _exclusive = crate::git::GIT_TEST_LOCK
            .write()
            .unwrap_or_else(|e| e.into_inner());
        let guard = begin_user_op(r"Qbgslot:/mixed-form").expect("first");
        for alt in [
            r"qbgslot:\mixed-form",
            r"QBGSLOT:/MIXED-FORM/",
            r" qbgslot:/mixed-form ",
        ] {
            assert!(
                try_begin_op(alt).is_none(),
                "写法「{alt}」被当成了另一颗仓库，于是同一仓库会有两个槽"
            );
        }
        drop(guard);
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
        assert!(msg.starts_with("拉取已取消"), "{msg}");
        assert!(!msg.contains("完成"), "取消那一次不许出现「完成」：{msg}");
        assert!(!msg.contains("冲突"), "取消那一次也不是 stash 冲突：{msg}");
        // ⚠️ **这一条断言改的就是 32-2 查实的那条静默档**。旧写法是
        // `assert_eq!(msg, "拉取已取消")`——它把"取消时只字不提 stash"钉成了
        // 期望行为，而那一档里用户丢的是文件。**旧断言不是被改坏的，是被改对的**
        assert!(msg.contains("还压在暂存里"), "取消 × pop 失败要说清文件在哪儿：{msg}");

        // ② 公钥提示排在分叉**前面**：两样都给时用前者。
        //    分叉那档给的是"可以用下面的方式合流"，而公钥失败压根没拉到东西
        let both = out(false, "Permission denied (publickey)");
        let (ok, msg) = pull_verdict(&both, Some(&fork()), None, None, Some("请先添加公钥"));
        assert!(!ok);
        assert_eq!(msg, "请先添加公钥");

        // ③ 拉取成功但 stash 没恢复：**这算失败**，用户文件还压在 stash 里
        let (ok, msg) = pull_verdict(&out(true, ""), None, None, Some(false), None);
        assert!(!ok);
        assert!(msg.starts_with("拉取完成，但恢复暂存变更时冲突"), "{msg}");
        // 原来这一档说的是「请在终端执行 git stash pop 处理」——**那是没有清单时
        // 给的话**。32-2 把清单做出来了，还让人去终端等于入口白开
        assert!(msg.contains("收起来的改动"), "要指到界面上的清单：{msg}");

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

    /// 「文件还压在暂存里」那一句**在三档失败上都要出现，在两档没压上时都不许出现**。
    ///
    /// 这是 32-2 查实的那条静默档的回归钉子：旧代码里取消与失败两档**先 return**，
    /// 于是"拉取失败 × pop 也失败"这一组合只说"拉取失败"，用户以为只是没拉到东西，
    /// 而他那些没提交的文件已经不在工作区里了。
    ///
    /// ⚠️ 反向那两条同样要钉：`None`（压根没暂存过）与 `Some(true)`（放回去了）
    /// 时若也拼这句，就是**对着一个没暂存过的仓库说"你的改动还压在暂存里"**——
    /// 凭空定罪和只字不提是同一个缺陷的两个方向
    #[test]
    fn the_parked_files_are_named_on_every_failing_arm() {
        let cancelled = || out(false, git::CANCELLED_MSG);
        let pull_failed = || out(false, "fatal: Not possible to fast-forward");
        let pull_ok = || out(true, "");

        for pull in [cancelled(), pull_failed(), pull_ok()] {
            let (_, msg) = pull_verdict(&pull, None, None, Some(false), None);
            assert!(msg.contains("还压在暂存里"), "{msg}");
            assert!(msg.contains("收起来的改动"), "要指到清单，不让人去终端：{msg}");
        }
        for pop in [None, Some(true)] {
            for pull in [cancelled(), pull_failed(), pull_ok()] {
                let (_, msg) = pull_verdict(&pull, None, None, pop, None);
                assert!(!msg.contains("暂存"), "{msg} 里不该提暂存（{pop:?}）");
            }
        }
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

    /// 断言消息里只放**尾部**那么长：偶发那条红要能自己报出成因，
    /// 而整份日志里前头全是 pull 的输出，pop 那句在最后一行
    fn tail_of(s: &str) -> String {
        let chars: Vec<char> = s.chars().collect();
        let take = chars.len().min(400);
        chars[chars.len() - take..].iter().collect()
    }

    /// 轮询用的"只看一眼"：读不到就返回 `None`，**绝不当成红**。
    ///
    /// `local` 那条在 git 非零退出时 panic，而这里读的正是一个**正在被写**的仓库
    /// （`stash push` 期间 refs 与 index 都在动），偶尔失败一次属于正常时序。
    /// 判据是"等到它成功为止"，所以失败必须是一次普通的循环而不是整条用例炸掉
    fn peek(dir: &std::path::Path, args: &[&str]) -> Option<String> {
        let o = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .ok()?;
        if !o.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&o.stdout).to_string())
    }

    /// **停止键落在拉取链上哪一点都不许丢东西**（2026-10-08 与 2026-10-10 两条真缺陷）。
    ///
    /// 这条测的是**接线**：机制那一层由 `git::an_essential_command_ignores_the_cancel_flag`
    /// 钉着，但**机制对、调用点写回 `run_local` 一样会丢文件**——而丢的是用户的东西。
    ///
    /// 三档取消时刻 = 这条链上三种"按停"的位置：
    /// ① 立刻按（`stash push` 之前）② 150ms 后按（多半落在 push 里面）
    /// ③ **观测到「stash 里已经有那一条」之后再按**（push 正在收尾）——
    /// 第三档就是 10-10 那条偶发红指出的位置：`stash push` 那时还是可取消的
    /// `run_local`，被 kill 在"文件已经收进 stash、工作区还没清理完"中间 ⇒
    /// `stashed` 留 false ⇒ 收尾那句 pop 根本不跑 ⇒ 文件既不在工作区、
    /// 界面也一个字不提"还压在暂存里"
    ///
    /// ⚠️ **第三档靠观测，不靠"睡到某个毫秒数"**。原来只睡 1500ms，赌的是
    /// "那一刻 push 早完了"——而这条前提是 **CPU 给的**：同一份代码在全量并行的
    /// `cargo test` 里六轮红三轮，单独聚焦跑十轮全绿，红与不红只取决于当时有多少
    /// 别的使用者在抢时间片。**测试的前提不能外包给环境**（AGENTS 明写），
    /// 所以这里改成"看到它真的发生了才动手"
    ///
    /// 远端 = 本机一个 accept 之后**一个字节都不回**的监听（`git::hanging_url()`）
    /// ⇒ pull 一定还挂着，三档都由这条链自己造出来，不猜外部网络会怎么反应
    ///
    /// ⚠️ 32-8 起这条**只持读锁**：三档各用自己那一份 `qbg-cancel-{pid}-{n}-{档}`
    /// 路径，而取消标记按仓库带地址 ⇒ 隔壁测试读的是另一个键，杀不到。
    /// **这正是这次下沉买到的东西**，所以把它写成锁的档位，让降级本身成为一条
    /// 可回归的判据（`git.rs` 那两条仍要写锁，因为它们共用 `.`）
    #[test]
    fn a_cancelled_pull_still_returns_the_stashed_files() {
        let _shared = crate::git::GIT_TEST_LOCK
            .read()
            .unwrap_or_else(|e| e.into_inner());
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // 那个不回话的监听整条测试共用一个就够。listener 交给 `hold_the_connection`
        // 握着，**不能就地 drop**（一 drop 端口没人听，连接立刻被拒 ⇒ 前提没了）
        let (remote_url, listener) = crate::git::hanging_url();
        crate::git::hold_the_connection(listener);

        for stage in 0..3 {
            let root = std::env::temp_dir().join(format!("qbg-cancel-{}-{n}-{stage}", std::process::id()));
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
            local(&repo, &["remote", "add", "origin", &remote_url]);
            local(&repo, &["config", "branch.main.remote", "origin"]);
            local(&repo, &["config", "branch.main.merge", "refs/heads/main"]);
            // 用户没提交的两样东西：`stash push -u` 会把它们**一起**收走，
            // 所以收尾那一步没跑的话，两样都会从工作区消失
            std::fs::write(repo.join("tracked.txt"), "改了没提交\n").unwrap();
            std::fs::write(repo.join("untracked.txt"), "别丢\n").unwrap();

            // 取消标记按仓库带地址（32-8）⇒ 这里先定下 path，再拿它清/打
            let path = repo.to_string_lossy().to_string();
            crate::git::clear_cancel_for(&path);
            let t = {
                let path = path.clone();
                std::thread::spawn(move || do_pull(&path, true, None))
            };
            match stage {
                // ① 什么都不等：停止键按在链的最前面
                0 => {}
                // ② 给前面那几道本地 git 一点时间，多半正好按进 push
                1 => std::thread::sleep(Duration::from_millis(150)),
                // ③ **等它真的把条目写进 stash** 再按——这一档才是缺陷所在的那个位置
                _ => {
                    let deadline = std::time::Instant::now() + Duration::from_secs(20);
                    loop {
                        let parked =
                            peek(&repo, &["stash", "list"]).is_some_and(|s| !s.trim().is_empty());
                        if parked {
                            break;
                        }
                        assert!(
                            std::time::Instant::now() < deadline,
                            "第 ③ 档 20 秒都没等到「stash 里出现那一条」⇒ 这条测试要的前提\
                             没造出来，取消落在哪一段是不可知的。红在这里报，\
                             别让下一次侥幸绿蒙过去"
                        );
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
            }
            crate::git::request_cancel_for(&path);
            let r = t.join().expect("do_pull panicked");
            // 生产里"把残留的取消意图清掉"由 `begin_user_op` 负责（它刚拿到这颗仓库的槽）；
            // 这条测试直接调 `do_pull`，不经过入口，所以自己清 —— **别把它当冗余删掉**：
            // 标记留在 `path` 上，下一个用同一份夹具的用例就会莫名其妙地被取消
            crate::git::clear_cancel_for(&path);

            let got = std::fs::read_to_string(repo.join("untracked.txt"))
                .unwrap_or_default()
                .trim_end()
                .to_string();
            assert_eq!(
                got, "别丢",
                "第 {stage} 档取消之后那个未跟踪文件没回到工作区（它多半还压在 stash 里）；\
                 结论那句是「{}」\n收尾日志（push / pull / pop 各说了什么）：{}",
                r.message,
                tail_of(&r.output),
            );
            assert_eq!(
                std::fs::read_to_string(repo.join("tracked.txt"))
                    .unwrap()
                    .trim_end(),
                "改了没提交",
                "第 {stage} 档取消之后那处修改没回来；结论那句是「{}」",
                r.message
            );
            assert!(
                local(&repo, &["stash", "list"]).trim().is_empty(),
                "第 {stage} 档 stash 里还留着东西 ⇒ 收尾那一步没跑完：{}",
                local(&repo, &["stash", "list"])
            );
            assert!(!r.ok, "第 {stage} 档");
            assert_eq!(
                r.message,
                "拉取已取消",
                "第 {stage} 档取消那一次界面说的是：{}",
                r.message
            );
            let _ = std::fs::remove_dir_all(&root);
        }
    }
}
