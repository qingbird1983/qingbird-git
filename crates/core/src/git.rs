use std::collections::HashSet;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

// GUI 程序派生控制台子进程（git / curl）时 Windows 会弹出一个立即消失的终端窗口，
// 必须加 CREATE_NO_WINDOW 抑制；仅对后台子进程，open_in_terminal 的可见窗口不受影响
// ⚠️ **本 crate 的统一出处**（32-15）：repos.rs 的 `git --version`、update.rs 的
// `curl.exe` 都从这里引——之前那两处各漏了一颗 flag，就是「双击启动闪终端 /
// 检查更新闪终端」两个用户报障的根因。gui 侧 os.rs 在另一个 crate 里，另有一份
#[cfg(windows)]
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// 用户取消标记：**每个仓库各一份**。置位后那个仓库正在跑的 git 子进程在下个
/// 轮询周期（≤50ms）被整树终止，**别的仓库不受影响**。
///
/// ⚠️ 它以前是一颗进程级 `AtomicBool`，那时**够用**是因为 `sync::OP_LOCK` 也只放
/// 一个忙操作——两件事各自都对，错在它们共用同一个「全应用只有一个」的假设。
/// 路线表 32-8 把锁下沉到每仓库之后，全局标记就变成「点 A 的停止把 B 正在跑的
/// 拉取一起杀掉」，而且**它不报错**：两边都正常返回，只是 B 那句变成"已取消"，
/// 谁都不知道是谁干的。⇒ 撤销那颗假设必须连着撤这一颗，少撤一半就是这种形状。
static CANCELLED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn cancelled_set() -> &'static Mutex<HashSet<String>> {
    CANCELLED.get_or_init(Default::default)
}

/// 仓库路径的**归一键**：取消标记表（本文件）与忙锁表（`sync::SLOTS`）共用这一颗。
///
/// ⛔ **不许有第二份归一**。两表都以路径为键、又必须指向同一个仓库，各写一份的话
/// 最坏情况是"锁按一种写法分、取消按另一种写法打"⇒ **A 拿到的槽锁不住 B，
/// 而 B 的停止键按下去什么都不发生**——两边都不报错，只有帧上看得见。
///
/// 归一管的是三件事：反斜杠与正斜杠、结尾有没有斜杠、盘符大小写。同一仓库从列表、
/// 从缓存、从拖放各来一种写法是常态（界面上它们长得一样）。
///
/// `to_lowercase` 只在 Windows 语义下成立（那边路径大小写不敏感）。非 Windows 上
/// 大小写是路径的一部分，小写化会把两个真不同的目录并成一个键——本项目是 Windows
/// 桌面工具，**把这条边界写住，而不是假装它不存在**。
pub(crate) fn repo_key(repo_path: &str) -> String {
    let mut key = repo_path.trim().replace('\\', "/");
    while key.len() > 1 && key.ends_with('/') {
        key.pop();
    }
    #[cfg(windows)]
    {
        key = key.to_lowercase();
    }
    key
}

/// 锁中毒只发生在别的线程 panic 时，而这个集合里只放字符串、panic 不来自这里。
/// 取 `into_inner` 而不是丢掉：中毒之后**照样该认这个标记**——
/// "取消"宁可多认一次，也不许在用户按了停止之后什么都不发生。
fn with_cancelled<R>(f: impl FnOnce(&mut HashSet<String>) -> R) -> R {
    let mut set = cancelled_set().lock().unwrap_or_else(|e| e.into_inner());
    f(&mut set)
}

/// 请求取消**这一个仓库**正在进行的 git 操作。
///
/// 参数是仓库路径不是"当前那一个"：core 不认识界面，不知道用户选中的是谁，
/// 所以调用方必须把"要停的是哪一颗"说出来（D78：让"停错仓库"没有选项）。
pub fn request_cancel_for(repo_path: &str) {
    with_cancelled(|set| {
        set.insert(repo_key(repo_path));
    });
}

/// 清掉**这一个仓库**的取消标记。
///
/// ⛔ **调用时机只在「一次操作刚拿到它自己那把锁」那一刻**（`sync::begin_user_op` /
/// `try_begin_op` 内部），别学以前那样放在 `run_net_op` 开头。那个位置错过一次事故：
/// 它等于把"清除残留取消意图"外包给"这条命令恰好走网络层"，而 `do_pull` 在
/// `run_net_op` **之前**要跑 `read_status` + `rev-parse` + `stash push` 三条本地命令
/// ⇒ 用户按过一次停止之后，标记一直留着，**每一次新拉取都在 `stash push` 上被上一次
/// 的取消杀掉**，而那一步失败就 `return`、永远走不到会清标记的 `run_net_op`。
/// 症状是"这个仓库再也拉不动了，每次都显示拉取已取消"，而重启应用就好了——
/// 典型的只在看"有没有报错"时查不到（路线表 32-8 调研账②）。
/// 放在入口之后：拿到锁 ⇒ 同仓库没有别人在跑 ⇒ 清的只可能是上一次的残留。
pub fn clear_cancel_for(repo_path: &str) {
    with_cancelled(|set| {
        set.remove(&repo_key(repo_path));
    });
}

/// 这个仓库此刻是不是被要求取消
fn is_cancelled_for(repo_path: &str) -> bool {
    with_cancelled(|set| set.contains(&repo_key(repo_path)))
}

/// 测试用的"别和取消测试同时派生 git"读写锁。
///
/// ⚠️ **路线表 32-8 之后它挡的还是同一件事，但成因窄了一圈**：取消标记已经从
/// 进程级改成按仓库，所以**两个各用自己那份 `$TEMP` 仓库的测试从此不再连坐**。
/// 它仍然必须留着，因为剩下的那一半没变：**同一份仓库路径被多条用例共用**时
/// （`sync.rs` 那组拉取用例都在同一个 fixture 上），取消测试一置位，隔壁正在
/// 跑的 git 子进程照样被整树终止，表现为"没有退出码、stderr 为空"的诡异失败——
/// 看起来像 git 挂了，其实是被隔壁测试连坐。所以约定：取消测试持写锁独占，
/// 其余会派生 git 的测试持读锁。
///
/// 判据：**「标记按谁分」决定「谁会被连坐」，不决定「有没有连坐」**。
/// 把标记拆细就顺手删掉这把锁，等于把"共用路径的那几条"暴露在红一下好一下里。
#[cfg(test)]
pub(crate) static GIT_TEST_LOCK: std::sync::RwLock<()> = std::sync::RwLock::new(());

pub const CANCELLED_MSG: &str = "操作已被用户取消";

#[derive(Debug, Clone)]
pub struct GitOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    #[allow(dead_code)]
    pub code: i32,
    pub timed_out: bool,
}

/// 网络类错误关键字（小写匹配）。git 所有 HTTP 错误都以 "unable to access" 开头，
/// 因此只匹配后续具体原因，避免把认证失败等误判为网络问题。
const NETWORK_ERRORS: &[&str] = &[
    "could not resolve host",
    "failed to connect",
    "connection timed out",
    "connection refused",
    "connection was reset",
    "network is unreachable",
    "timed out",
    "rpc failed",
    "early eof",
    "openssl ssl_read",
    "the remote end hung up",
    "the requested url returned error: 5",
    // 低速停滞中止（curl 28）在不同传输/错误路径下表面形式不同，两种都要覆盖
    "operation too slow",
    "unexpected disconnect",
];

pub fn is_network_error(out: &GitOutput) -> bool {
    if out.timed_out {
        return true;
    }
    let hay = format!("{} {}", out.stderr, out.stdout).to_lowercase();
    NETWORK_ERRORS.iter().any(|k| hay.contains(k))
}

/// SSH 公钥被拒时的人话结论；**None = 不是这类问题**。
///
/// **纯函数**（`pubkey_path` 由调用方给）：单测不许去读 `HOME` 或扫磁盘，
/// 路径是参数不是函数自己找的——同 `diag` 那条「把被依赖的值收成参数」。
///
/// ## 为什么值得单独判一类
///
/// `Permission denied (publickey)` 是网络操作里**唯一一种「连上了但进不去」**：
/// TCP 通了、ssh 握手也通了，是服务端不认你这把钥匙。而
/// - `is_network_error` 对它返回 **false** ⇒ 应用**不去试代理**——**这是对的**，
///   换十个代理结果一样；
/// - 但界面给的那句「无法连接远端」是**假的**（连上了），「未找到可用代理」
///   更假（代理跟这事没关系）。
///
/// 顺带说清楚一件容易白查的事：**SSH 远端压根不需要代理**。实测同一台机器上
/// `ssh -T git@github.com` 无代理 4.1 秒就认证成功——22 端口不被墙。
/// 被代理挡住的只有 HTTPS 远端。所以"给 SSH 加无感代理"是在解决一个不存在的问题。
///
/// `pubkey_path` 只在用户真有钥匙时才有意义，所以调用方给的是**实际存在的那个**
/// `.pub` 路径；一个都没找到时传空串，这一句就退成"去生成一把"。
pub fn ssh_auth_hint(stderr: &str, pubkey_path: &str) -> Option<String> {
    let hay = stderr.to_lowercase();
    // 只认 `publickey` 这一个词：HTTPS 侧的认证失败措辞完全不同
    // （`could not read Username` / `Authentication failed`），不沾边
    if !hay.contains("publickey") {
        return None;
    }
    let tail = if pubkey_path.is_empty() {
        "本机还没找到 SSH 密钥，先生成一对再把公钥贴上去".to_string()
    } else {
        format!("把 {pubkey_path} 的内容贴到该平台的「设置 → SSH 公钥」")
    };
    Some(format!(
        "SSH 公钥没被接受——这不是网络问题，代理帮不上忙。{tail}"
    ))
}

/// 用户 `~/.ssh` 下第一个真实存在的公钥文件（完整路径）。
///
/// **只报存在的那个**：报一个不存在的路径比不报更糟，用户会去 `notepad` 那个
/// 路径、发现没有、然后以为程序在骗人。常见文件名按 ed25519 → rsa → ecdsa 排。
/// 一个都没有时返回 `None`，由 `ssh_auth_hint` 退成"先生成一对"。
pub fn existing_pubkey() -> Option<String> {
    let home = std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())?;
    let dir = std::path::Path::new(&home).join(".ssh");
    for name in ["id_ed25519.pub", "id_rsa.pub", "id_ecdsa.pub"] {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p.display().to_string());
        }
    }
    None
}

/// 加上公钥那句，返回 `None` 的场合原样返回。
///
/// 存在是因为**三处失败出口都要判同一件事**（检测 / 拉取 / 推送），
/// 而漏一处的后果不是编译错误、不是测试红，是**用户在其中两个操作上
/// 看到一句假话**。
pub fn ssh_auth_hint_or(stderr: &str, pubkey_path: &str, base: &str) -> String {
    ssh_auth_hint(stderr, pubkey_path).unwrap_or_else(|| base.to_string())
}

/// 执行一条 git 命令。proxy_url 为 Some 时仅对本次子进程注入代理环境变量，
/// 命令结束即失效，不影响系统与其他程序。
pub fn run_git(repo_path: &str, args: &[&str], proxy_url: Option<&str>, timeout: Duration) -> GitOutput {
    run_git_stream(repo_path, args, proxy_url, timeout, None)
}

/// 同 run_git，但 stderr 每输出一行就回调一次 on_line（git 的进度信息走 stderr）。
/// 回调在本线程内同步执行，不会乱序。
pub fn run_git_stream(
    repo_path: &str,
    args: &[&str],
    proxy_url: Option<&str>,
    timeout: Duration,
    on_line: Option<&dyn Fn(&str)>,
) -> GitOutput {
    run_git_auth(repo_path, args, proxy_url, timeout, on_line, &[], None)
}

/// 同 run_git_stream，另外支持本次操作专属的 `-c key=val` 配置项与环境变量
/// （用于令牌认证头、指定 SSH 私钥），命令结束即失效。
pub fn run_git_auth(
    repo_path: &str,
    args: &[&str],
    proxy_url: Option<&str>,
    timeout: Duration,
    on_line: Option<&dyn Fn(&str)>,
    git_config: &[String],
    ssh_command: Option<&str>,
) -> GitOutput {
    run_git_inner(
        repo_path, args, proxy_url, timeout, on_line, git_config, ssh_command, None, true,
    )
}

/// **收尾必达**那一类：用户已经取消、甚至正在取消，这一步也**必须跑完**。
///
/// 目前只有一个用户：`do_pull` 把本地变更 `git stash push -u` 之后，
/// 无论拉取成功、失败还是被取消，**都得把 `stash pop` 跑掉**——
/// 那是用户没提交的文件，留在 stash 里就是"东西不见了"。
///
/// ⚠️ 走普通 `run_local` 会被**那个仓库的取消标记**带倒：轮询每 50ms 查一次
/// `is_cancelled_for(repo_path)`（`git.rs` 里那个 `cancelled` 分支），而用户点「停止」时
/// 标记早就置位了 ⇒ 这条 pop 在第一个周期就被整树杀掉 ⇒
/// **文件留在 stash 里，界面还说"拉取完成"**
/// （2026-10-08 实测坐实，路线表 31.4 补二十一）。
///
/// 它**不是**"能忽略取消的通用跑法"：网络操作一律不许走这里，
/// 那会让人按了停止却还在传。判据是「这一步不做完，用户的东西就回不来」。
/// 超时由调用方给（`net::run_local_essential` 传 `LOCAL_TIMEOUT`），
/// 这里不自己写一个 10 秒——同一个档不许有第二个出处
pub fn run_essential(repo_path: &str, args: &[&str], timeout: Duration) -> GitOutput {
    run_git_inner(
        repo_path, args, None, timeout, None, &[], None, None, false,
    )
}

/// 同 `run_git_auth`，另外把 `stdin_data` 喂给 git 的标准输入。
///
/// **为什么需要它**：只有 `git restore --pathspec-from-file=-` 这种
/// "路径从 stdin 读" 的命令用得上。路径走参数会撞上 Windows 命令行
/// 32k 上限（一个改了几百个文件的仓库按参数传会被系统直接截断），
/// 走 stdin 再配 `--pathspec-file-nul` 则路径里有空格、换行、引号
/// 都不用转义。
pub fn run_git_stdin(
    repo_path: &str,
    args: &[&str],
    stdin_data: &[u8],
    timeout: Duration,
) -> GitOutput {
    run_git_inner(repo_path, args, None, timeout, None, &[], None, Some(stdin_data), true)
}

#[allow(clippy::too_many_arguments)]
fn run_git_inner(
    repo_path: &str,
    args: &[&str],
    proxy_url: Option<&str>,
    timeout: Duration,
    on_line: Option<&dyn Fn(&str)>,
    git_config: &[String],
    ssh_command: Option<&str>,
    stdin_data: Option<&[u8]>,
    // false = 这一步**不受那个仓库的取消标记影响**（只有 `run_essential` 传）。
    // ⚠️ 它是参数不是"调用方自己记得判"：新加一条收尾必达的命令时，
    // 少传一个参数**编译不过**（D78 那条：让"忘"没有选项）
    cancellable: bool,
) -> GitOutput {
    let fail = |stderr: String| GitOutput {
        success: false,
        stdout: String::new(),
        stderr,
        code: -1,
        timed_out: false,
    };

    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo_path);
    for c in git_config {
        cmd.arg("-c").arg(c);
    }
    cmd.args(args);
    cmd.env("GIT_TERMINAL_PROMPT", "0"); // 禁止交互式终端提示，防止卡死
    cmd.env("GCM_INTERACTIVE", "never");
    // 清除继承的全局代理环境变量，保证直连语义可控
    for var in [
        "HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy", "ALL_PROXY", "all_proxy",
    ] {
        cmd.env_remove(var);
    }
    if let Some(p) = proxy_url {
        cmd.env("HTTPS_PROXY", p);
        cmd.env("HTTP_PROXY", p);
    }
    if let Some(ssh) = ssh_command {
        cmd.env("GIT_SSH_COMMAND", ssh);
    }
    // stdin 默认接空——不给 git 任何输入它就不会等人敲回车。
    // 需要喂数据的（`restore --pathspec-from-file=-`）走 run_git_stdin
    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return fail(format!("无法启动 git 进程: {e}")),
    };
    // 挂到随主进程退出的 Job 上：
    // 1) 取消/超时/强杀时能整树终止——git 在 Windows 上是多层进程
    //    （git.exe → git remote-https / git index-pack），只 kill 顶层会留下
    //    继续传数据、占管道和目录的孤儿，正是"取消没反应、文件夹删不掉"的根源
    // 2) 主进程被强杀时 Job 句柄消失，系统回收整棵树
    #[cfg(windows)]
    let job_handle = bind_kill_on_job_close(&child);
    #[cfg(not(windows))]
    let job_handle: isize = 0;

    // 喂 stdin。**必须另起一个线程写**：管道缓冲区满了之后写侧会阻塞，
    // 而 git 要等读完 stdin 才开始吐 stdout——同一个线程里先写后读就是死锁。
    // 路径多了（几百个文件）时数据量能到几十 KB，正好够塞满缓冲区
    if let Some(data) = stdin_data {
        if let Some(mut pipe) = child.stdin.take() {
            let owned = data.to_vec();
            std::thread::spawn(move || {
                use std::io::Write as _;
                // 写失败（git 提前退出）不算错：下面的退出码会说明原因
                let _ = pipe.write_all(&owned);
                let _ = pipe.flush();
            });
        }
    }

    let mut stdout_pipe = child.stdout.take().expect("stdout piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr piped");
    let t_out = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = Read::read_to_string(&mut stdout_pipe, &mut s);
        s
    });
    // stderr 线程逐行转发到 mpsc，由等待循环同步消费并回调。
    // git 的进度信息用 \r 在同一行刷新，因此 \r 和 \n 都按行分隔。
    let (line_tx, line_rx) = std::sync::mpsc::channel::<String>();
    let t_err = std::thread::spawn(move || {
        let mut s = String::new();
        let mut buf = [0u8; 4096];
        let mut pending: Vec<u8> = Vec::new();
        loop {
            match stderr_pipe.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    s.push_str(&String::from_utf8_lossy(&buf[..n]));
                    pending.extend_from_slice(&buf[..n]);
                    while let Some(pos) = pending.iter().position(|&b| b == b'\n' || b == b'\r') {
                        let line: Vec<u8> = pending.drain(..=pos).collect();
                        let text = String::from_utf8_lossy(&line[..line.len() - 1]).to_string();
                        if !text.trim().is_empty() {
                            let _ = line_tx.send(text);
                        }
                    }
                }
                Err(_) => break,
            }
        }
        if !pending.is_empty() {
            let _ = line_tx.send(String::from_utf8_lossy(&pending).to_string());
        }
        s
    });

    let start = Instant::now();
    let mut cancelled = false;
    let mut stalled = false;
    // 静默停滞保护：连接阶段没有任何传输数据，http.lowSpeed 保护不生效
    // （典型场景：直连握手被墙丢包，永远卡在"启动阶段"）。
    // 开了 --progress 的网络操作正常每秒都有输出，30 秒一行都没有即判定挂死
    const STALL_SILENCE: Duration = Duration::from_secs(30);
    let mut last_line = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                let mut got_line = false;
                while let Ok(line) = line_rx.try_recv() {
                    got_line = true;
                    if let Some(cb) = on_line {
                        cb(&line);
                    }
                }
                if got_line {
                    last_line = Instant::now();
                }
                if cancellable && is_cancelled_for(repo_path) {
                    terminate_job_tree(job_handle);
                    let _ = child.kill();
                    let _ = child.wait();
                    cancelled = true;
                    break None;
                }
                if last_line.elapsed() > STALL_SILENCE {
                    terminate_job_tree(job_handle);
                    let _ = child.kill();
                    let _ = child.wait();
                    stalled = true;
                    break None;
                }
                if start.elapsed() > timeout {
                    terminate_job_tree(job_handle);
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break None,
        }
    };
    // 收尾：把剩余行全部消费掉
    if let Some(cb) = on_line {
        while let Ok(line) = line_rx.try_recv() {
            cb(&line);
        }
    }

    let out = t_out.join().unwrap_or_default();
    let err = t_err.join().unwrap_or_default();

    match status {
        Some(st) => GitOutput {
            success: st.success(),
            stdout: out,
            stderr: err,
            code: st.code().unwrap_or(-1),
            timed_out: false,
        },
        None => {
            // 取消与停滞/超时都走这里；取消不算网络错误，避免取消后又触发代理回退
            if cancelled {
                return GitOutput {
                    success: false,
                    stdout: out,
                    stderr: CANCELLED_MSG.into(),
                    code: -1,
                    timed_out: false,
                };
            }
            let mut stderr = err;
            if stalled {
                stderr.push_str("\n连接或传输超过 30 秒无任何响应，已中止本次尝试");
            } else {
                stderr.push_str("\n操作超时，已终止");
            }
            GitOutput { success: false, stdout: out, stderr, code: -1, timed_out: true }
        }
    }
}

/// 把子进程挂到一个带 KILL_ON_JOB_CLOSE 的 Job 上，返回 Job 句柄（isize 表示）。
/// 句柄刻意不关闭：关闭最后一份句柄会立即触发 kill-on-job-close。
/// 让句柄随主进程存活，主进程退出（含任务管理器强杀）时系统回收整棵进程树。
#[cfg(windows)]
pub(crate) fn bind_kill_on_job_close(child: &std::process::Child) -> isize {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return 0;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if AssignProcessToJobObject(job, child.as_raw_handle()) == 0 {
            return 0;
        }
        job as isize
    }
}

/// 终止 Job 里的整棵进程树（含 git remote-https / index-pack 等孙进程）。
#[cfg(windows)]
pub(crate) fn terminate_job_tree(job: isize) {
    if job != 0 {
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(
                job as _,
                1,
            );
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn terminate_job_tree(_job: isize) {}

/// 造一个「连得上、但永远不回话」的本机地址，用来把 git 子进程**挂着**。
///
/// 这类测试的前提是"命令还在跑"。早先用黑洞地址 `10.255.255.1` 造这个前提，
/// 于是它其实同时测了"本机网络栈会不会把这条连接挂着"：
/// 2026-10-09 本机对那个地址 **0 毫秒就拒绝连接**
/// （`Failed to connect to 10.255.255.1:443 after 0 ms`），git 在 400ms 的
/// 取消窗口**之前**就走完 ⇒ 断言"`stderr` 含已取消"必红，
/// 而**红的不是取消，是前提没了**——这种红隔天会变成噪音，噪音会被关掉。
///
/// 现在自己监听一个本机端口，`accept` 之后**一个字节都不回也不关**，
/// git 的 http 客户端就一直等（curl 默认不设读超时）。
/// 前提由测试自己保证：不碰外网、不碰代理与防火墙、离线也能跑。
///
/// ⚠️ 返回的那个 `TcpListener` **必须交出去**（一般交给 `hold_the_connection`）：
/// 它一被 drop，端口就没人听着了，连接变成"立刻拒绝"，前提又没了
#[cfg(test)]
pub(crate) fn hanging_url() -> (String, std::net::TcpListener) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("本机监听失败：取消类测试没有夹具了");
    let addr = listener.local_addr().expect("拿不到本机监听地址");
    (format!("http://{addr}/x.git"), listener)
}

/// 握着这条连接不放，直到测试进程自己结束（不需要 join，进程退出即回收）
#[cfg(test)]
pub(crate) fn hold_the_connection(listener: std::net::TcpListener) {
    std::thread::spawn(move || {
        let _silent = listener.accept().map(|(stream, _)| stream).ok();
        std::thread::sleep(std::time::Duration::from_secs(30));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(stderr: &str, timed_out: bool) -> GitOutput {
        GitOutput { success: false, stdout: String::new(), stderr: stderr.into(), code: -1, timed_out }
    }

    #[test]
    fn cancel_kills_running_git() {
        // 标记从 32-8 起按仓库分，而这两条用例都打在 `.` 这个地址上
        // （`ls-remote` 不需要真仓库，只要有个目录给 `-C` 就行）⇒ 共用同一个键的
        // 就是它们俩，仍按老约定持写锁独占。别的用例各用自己那份 `$TEMP` 绝对路径，
        // **从此不再被这里连坐**——那才是这次下沉真正买到的东西
        let _exclusive = GIT_TEST_LOCK
            .write()
            .unwrap_or_else(|e| e.into_inner());
        clear_cancel_for(".");
        let (url, listener) = hanging_url();
        hold_the_connection(listener);
        let t = std::thread::spawn(move || {
            run_git_auth(
                ".",
                &["ls-remote", url.as_str()],
                None,
                Duration::from_secs(30),
                None,
                &[],
                None,
            )
        });
        std::thread::sleep(Duration::from_millis(400));
        request_cancel_for(".");
        let out = t.join().unwrap();
        clear_cancel_for(".");
        assert!(!out.success);
        assert!(!out.timed_out);
        assert!(out.stderr.contains(CANCELLED_MSG), "stderr = {}", out.stderr);
    }

    /// **收尾必达那一条命令，在取消标记已经置位时照样跑**（2026-10-08 修的那条）。
    ///
    /// 判据**不能是"它跑成功了"**：那要真造一个 stash 才谈得上恢复，
    /// 而短命令在第一个 50ms 轮询之前就返回了，**变异了也不会红**——
    /// 那种测试只是给"没测东西"盖一个测试的章。
    /// 取的是**两条退出分支互斥**这一点：轮询里先查取消、后查超时，
    /// 所以给一个 1ms 超时 + 一条必然挂着的命令，就能把"走的哪条分支"读出来：
    ///   · 正确（essential 不看取消）⇒ `timed_out = true`
    ///   · 错（照旧看取消）⇒ `stderr` 是 `CANCELLED_MSG`、`timed_out = false`
    ///
    /// "必然挂着"那条命令由 `hanging_url()` 给（本机监听、accept 之后不回话），
    /// 不再是黑洞地址——理由写在 `hanging_url()` 那儿。两趟都在 50ms 量级收手。
    /// 整段持写锁独占——它会动 `.` 这个地址上的取消标记，与上面那条共用同一个键
    #[test]
    fn an_essential_command_ignores_the_cancel_flag() {
        let _exclusive = GIT_TEST_LOCK
            .write()
            .unwrap_or_else(|e| e.into_inner());
        request_cancel_for("."); // 用户已经按了停止

        // 夹具同一个：`essential` 那半条要的是"1ms 超时到的时候命令还在跑"，
        // 用黑洞地址时这条靠的是网络栈的行为，本机一改（2026-10-09 就是 0ms 拒绝）
        // 断言就变成测运气。本机握着连接才是自己说了算
        let (url, listener) = hanging_url();
        hold_the_connection(listener);
        let black_hole = ["ls-remote", url.as_str()];
        let essential = run_essential(".", &black_hole, Duration::from_millis(1));
        // 对照那半条**先跑**：它依赖标记还置着。两趟都跑完再清、再断言，
        // 免得断言 panic 把标记留在置位状态去连坐别的测试
        let normal = run_git(".", &black_hole, None, Duration::from_secs(5));
        clear_cancel_for(".");

        assert!(
            essential.timed_out,
            "收尾必达的命令被取消标记带倒了（走的是取消分支不是超时分支）：stderr = {}",
            essential.stderr
        );
        assert!(
            !essential.stderr.contains(CANCELLED_MSG),
            "essential 不该走取消那条分支：stderr = {}",
            essential.stderr
        );
        assert!(
            !normal.success && normal.stderr.contains(CANCELLED_MSG),
            "普通命令在标记置位时**没**被取消 ⇒ 上面那半条测的其实是『取消整个坏了』：{}",
            normal.stderr
        );
    }

    /// 取消打在 A 上，**B 那一条必须活到它自己的超时**（路线表 32-8 的全部意义）。
    ///
    /// 判据取**两条退出分支互斥**，手法照上面 essential 那一条：轮询里先查取消、
    /// 后查超时，所以给 B 一个 1.5 秒超时 + 一条必然挂着的命令，就能读出它走的哪条分支：
    ///   · 正确（每个仓库各记一份）⇒ B `timed_out = true`
    ///   · 错（退回一颗全局标记）⇒ B 在 400ms 就被整树杀掉，`stderr` 是"已取消"
    /// **不能只断言"B 跑成功了"**：短命令在第一个 50ms 轮询之前就返回了，
    /// 那种测试变异了也不会红（AGENTS：只判"有没有报错"的自查拦不住任何东西）。
    ///
    /// 两个地址各要一个自己的挂起夹具——端口不同。共用一个监听的话，
    /// A 与 B 连的是同一条连接，"互不影响"这件事根本没被分开
    #[test]
    fn cancelling_one_repo_leaves_the_other_one_running() {
        // 读锁就够：这条只动它自己那两个键。以前它必须跟着写锁独占，
        // 因为一颗进程级标记会把隔壁测试的 git 一起杀掉——**那正是这次拆掉的东西**
        let _shared = GIT_TEST_LOCK
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join("qbg-cancel-two-repos");
        std::fs::create_dir_all(&root).expect("临时目录建不出来，这条没有夹具");
        // `-C` 只要一个**存在**的目录：`ls-remote` 问的是远端，不需要本地是仓库
        let a = root.join("a");
        let b = root.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let a = a.to_string_lossy().to_string();
        let b = b.to_string_lossy().to_string();

        let (url_a, l_a) = hanging_url();
        hold_the_connection(l_a);
        let (url_b, l_b) = hanging_url();
        hold_the_connection(l_b);

        let ta = {
            let (path, url) = (a.clone(), url_a.clone());
            std::thread::spawn(move || {
                run_git(&path, &["ls-remote", url.as_str()], None, Duration::from_secs(30))
            })
        };
        let tb = {
            let (path, url) = (b.clone(), url_b.clone());
            std::thread::spawn(move || {
                run_git(&path, &["ls-remote", url.as_str()], None, Duration::from_millis(1500))
            })
        };

        std::thread::sleep(Duration::from_millis(400));
        request_cancel_for(&a);
        let oa = ta.join().expect("A 那条线程该回来");
        let ob = tb.join().expect("B 那条线程该回来");
        clear_cancel_for(&a);
        clear_cancel_for(&b);
        let _ = std::fs::remove_dir_all(&root);

        assert!(
            oa.stderr.contains(CANCELLED_MSG),
            "A 该被取消，实际 stderr = {}",
            oa.stderr
        );
        assert!(
            ob.timed_out,
            "B 被 A 的取消带倒了：它本该走自己那条超时分支。stderr = {}",
            ob.stderr
        );
        assert!(
            !ob.stderr.contains(CANCELLED_MSG),
            "B 读到了 A 的取消标记 ⇒ 标记还是全局的一颗：{}",
            ob.stderr
        );
    }

    /// 归一键：同一颗仓库的三种写法必须并成一个串。
    ///
    /// 这条单测的是**键**，不是行为——行为由上面那条与 `sync` 那两条负责。
    /// 分开是因为症状不同：键没归一不会出现"测试红"，出现的是
    /// 「停止按下去没反应」和「同一仓库两个槽、真的并发写同一个 `.git`」
    #[test]
    fn one_path_three_spellings_is_one_key() {
        let base = repo_key("D:\\Work\\Repo");
        assert_eq!(base, repo_key("D:/Work/Repo"));
        assert_eq!(base, repo_key("  D:/Work/Repo/  "));
        // 只有 Windows 才把大小写当同一颗仓库，所以这一档跟着 cfg 走：
        // 在会区分大小写的那个平台上把它并起来是**错的**，不许悄悄钉进同一条断言
        #[cfg(windows)]
        assert_eq!(base, repo_key("d:\\work\\repo"));
        // 盘根是个例外：`D:\` 与 `D:` 归一之后必须仍是同一个键，
        // 而 `D:` 不能被削成空串（那是"所有仓库共用一个键"的那种事故）
        assert_eq!(repo_key("D:\\"), repo_key("D:"));
        assert!(!repo_key("D:\\").is_empty());
    }

    #[test]
    fn network_errors_are_classified() {
        assert!(is_network_error(&output(
            "fatal: unable to access 'https://github.com/x/y/': Could not resolve host: github.com",
            false,
        )));
        assert!(is_network_error(&output(
            "fatal: unable to access 'https://github.com/x/y/': Failed to connect to github.com port 443 after 21068 ms: Timed out",
            false,
        )));
        assert!(is_network_error(&output("fatal: OpenSSL SSL_read: Connection was reset", false)));
        assert!(is_network_error(&output("error: RPC failed; curl 56 Recv failure", false)));
        assert!(is_network_error(&output(
            "fatal: unable to access 'https://github.com/x/y/': Operation too slow. Less than 30720 bytes/sec transferred the last 20 seconds",
            false,
        )));
        assert!(is_network_error(&output("", true)));
    }

    #[test]
    fn non_network_errors_are_not_retried() {
        // 认证失败 / 分叉等不应触发代理回退
        assert!(!is_network_error(&output(
            "fatal: Authentication failed for 'https://github.com/x/y/'",
            false,
        )));
        assert!(!is_network_error(&output("error: Your local changes to the following files", false)));
        assert!(!is_network_error(&output("", false)));
    }

    /// 2026-10-06：gitee 那个仓库一直是 `Permission denied (publickey)`，
    /// 而界面给的是「拉取失败」/「无法连接远端」——**两句都是假的**
    /// （连上了，代理也压根不相关）。本条钉住"认得出 + 说人话 + 指出公钥在哪"
    #[test]
    fn publickey_failure_gets_a_human_hint_with_the_key_path() {
        // 真实 git 输出原样（gitee 实测）
        let stderr = "git@gitee.com: Permission denied (publickey).\r\n\
                       fatal: Could not read from remote repository.\r\n";
        let key = r"C:\Users\42527\.ssh\id_ed25519.pub";
        let h = ssh_auth_hint(stderr, key).expect("这行必须认得出");
        assert!(h.contains("不是网络问题"), "要点破代理帮不上忙：{h}");
        assert!(h.contains("代理帮不上忙"), "{h}");
        assert!(h.contains(key), "必须指出公钥文件的真实路径：{h}");
    }

    /// 三个都要避开的：**空串占位**（用户一把钥匙都没有时说路径是骗人）、
    /// **HTTPS 侧的认证失败**（措辞完全不同，不该被 SSH 那条判据吃掉）、
    /// **普通网络错误**。
    #[test]
    fn publickey_hint_fires_only_on_publickey_and_falls_back_cleanly() {
        let key = r"C:\Users\42527\.ssh\id_ed25519.pub";
        // 一把钥匙都没有 ⇒ 退成「先生成一把」，而不是报一个不存在的路径
        let h = ssh_auth_hint("Permission denied (publickey).", "").unwrap();
        assert!(h.contains("先生成一对"), "{h}");
        assert!(!h.contains(".ssh"), "没有钥匙时不该出现任何 .ssh 路径：{h}");

        // HTTPS 认证失败：措辞不同，**不归这条判据管**（它有自己的出口，
        // 见 sync.rs 里 "认证未通过，可在设置里填写访问令牌" 那一档）
        assert_eq!(
            ssh_auth_hint("fatal: Authentication failed for 'https://github.com/x/y/'", key),
            None
        );
        assert_eq!(ssh_auth_hint("fatal: could not read Username for 'https://…'", key), None);
        // 普通网络错误同理
        assert_eq!(ssh_auth_hint("fatal: unable to access 'https://…': Could not resolve host", key), None);
        // 大小写不敏感（OpenSSH 的措辞在不同版本间有出入）
        assert!(ssh_auth_hint("permission denied (publickey)", key).is_some());
        // `_or` 版本：不是这类问题时原样返回 base，**三个失败出口共用**
        assert_eq!(
            ssh_auth_hint_or("fatal: unrelated", key, "拉取失败"),
            "拉取失败"
        );
        assert!(ssh_auth_hint_or("Permission denied (publickey)", key, "拉取失败").contains("公钥"));
    }
}
