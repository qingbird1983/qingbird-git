use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

// GUI 程序派生控制台子进程（git）时 Windows 会弹出一个立即消失的终端窗口，
// 必须加 CREATE_NO_WINDOW 抑制；仅对后台子进程，open_in_terminal 的可见窗口不受影响
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// 用户取消标记：置位后所有运行中的 git 子进程在下个轮询周期（≤50ms）被终止。
/// 同一时刻 UI 只允许一个忙操作，全局标记足够。
static CANCELLED: AtomicBool = AtomicBool::new(false);

/// 请求取消当前正在进行的 git 操作
pub fn request_cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}

/// 新操作开始前清除取消标记
pub fn clear_cancel() {
    CANCELLED.store(false, Ordering::SeqCst);
}

fn is_cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

/// 测试用的"别和取消测试同时派生 git"读写锁。
///
/// `CANCELLED` 是进程级全局标记（生产环境同一时刻只允许一个忙操作，这样最简单），
/// 但 `cargo test` 默认并行：取消测试一置位，别的测试正在跑的 git 子进程会被
/// 整树终止，表现为"没有退出码、stderr 为空"的诡异失败——看起来像 git 挂了，
/// 其实是被隔壁测试连坐。所以约定：取消测试持写锁独占，其余会派生 git 的测试持读锁。
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
        repo_path, args, proxy_url, timeout, on_line, git_config, ssh_command, None,
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
    run_git_inner(repo_path, args, None, timeout, None, &[], None, Some(stdin_data))
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
                if is_cancelled() {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn output(stderr: &str, timed_out: bool) -> GitOutput {
        GitOutput { success: false, stdout: String::new(), stderr: stderr.into(), code: -1, timed_out }
    }

    #[test]
    fn cancel_kills_running_git() {
        // 取消是整个进程的全局状态，跑这段时必须独占，否则会杀掉别的测试的 git
        let _exclusive = GIT_TEST_LOCK
            .write()
            .unwrap_or_else(|e| e.into_inner());
        clear_cancel();
        // 黑洞地址：TCP 连接会长时间挂起，给取消留出触发窗口
        let t = std::thread::spawn(|| {
            run_git_auth(
                ".",
                &["ls-remote", "https://10.255.255.1/x.git"],
                None,
                Duration::from_secs(30),
                None,
                &[],
                None,
            )
        });
        std::thread::sleep(Duration::from_millis(400));
        request_cancel();
        let out = t.join().unwrap();
        clear_cancel();
        assert!(!out.success);
        assert!(!out.timed_out);
        assert!(out.stderr.contains(CANCELLED_MSG), "stderr = {}", out.stderr);
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
}
