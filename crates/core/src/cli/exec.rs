//! 跑一条外部命令并拿回它的输出。
//!
//! ⚠️ 与 core 里所有派生子进程的地方**同一条纪律：一律加 `CREATE_NO_WINDOW`**
//! （常量复用 `git::CREATE_NO_WINDOW`，别在这儿写第二份 `0x0800_0000`）。
//! 少了它，release 是 GUI 子系统，每扫一次 CLI 就会闪一下黑窗
//! （本机 2026-10-13 刚为 `git --version` 与 `curl` 各修过一次同款）。

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// 一条命令跑完的样子。
pub struct CmdOutput {
    /// 退出码；被信号杀掉时为 `None`
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl CmdOutput {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

/// 跑 `<program> <args...>` 并等它结束，超时杀进程。
///
/// ⚠️ **经 `cmd /C` 起**：Windows 上 `npm` / `scoop` / `pipx` 都是 `.cmd`
/// 脚本，而 `CreateProcess` 只补 `.exe`，直接 `Command::new("npm")` 会找不到
/// （`run_cmd.rs` 起仓库命令同款走 `cmd /C`）。
///
/// 阻塞——**调用方放后台线程**。
pub fn run_program(program: &str, args: &[&str], timeout: Duration) -> Result<CmdOutput, String> {
    let mut cmd = Command::new("cmd");
    cmd.arg("/C").arg(program).args(args);
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(crate::git::CREATE_NO_WINDOW);

    let mut child = cmd.spawn().map_err(|e| format!("无法启动 {program}：{e}"))?;

    // 输出交给两条读取线程，主线程只盯超时——同一个管道既写又读会把它堵死
    // （子进程写满管道缓冲区就停在那儿，我们在 `wait` 上永远等不到）
    let out = child.stdout.take().map(read_all);
    let err = child.stderr.take().map(read_all);

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("等待 {program} 退出失败：{e}")),
        }
    };

    let join = |h: Option<std::thread::JoinHandle<Vec<u8>>>| {
        h.and_then(|h| h.join().ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    };
    let stdout = join(out);
    let stderr = join(err);

    let Some(status) = status else {
        return Err(format!("{program} 超时（{} 秒）", timeout.as_secs()));
    };
    Ok(CmdOutput {
        code: status.code(),
        stdout,
        stderr,
    })
}

/// 把整条流读到 `Vec<u8>`。**按字节读**，调用侧再 `from_utf8_lossy`——
/// `read_to_string` 遇到一个非 UTF-8 字节会整个失败、返回空串，
/// 那会把"输出里有个乱码"变成"这个 provider 什么都没扫到"
fn read_all<R: std::io::Read + Send + 'static>(mut r: R) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = r.read_to_end(&mut buf);
        buf
    })
}

/// 这个命令在不在 PATH 上（`where <program>` 退出码为 0）。
///
/// 用它而不是"直接跑一次看报不报错"：探测要**毫秒级、无副作用**，
/// 而 `detect()` 一跑起来就是每个 provider 一次。
pub fn command_exists(program: &str) -> bool {
    run_program("where", &[program], Duration::from_secs(5))
        .map(|o| o.ok())
        .unwrap_or(false)
}
