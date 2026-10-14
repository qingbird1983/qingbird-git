//! 跑一条外部命令并拿回它的输出。
//!
//! ⚠️ 与 core 里所有派生子进程的地方**同一条纪律：一律加 `CREATE_NO_WINDOW`**
//! （常量复用 `git::CREATE_NO_WINDOW`，别在这儿写第二份 `0x0800_0000`）。
//! 少了它，release 是 GUI 子系统，每扫一次 CLI 就会闪一下黑窗
//! （本机 2026-10-13 刚为 `git --version` 与 `curl` 各修过一次同款）。

use std::io::BufRead;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
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

/// 流式跑一条命令：**每读到一行就回调一次**，主线程只盯退出 / 超时 / 取消。
///
/// 与 [`run_program`] 的分工：那个是"等它跑完，拿全部输出"（列清单 / 查更新 /
/// 卸载清理一条龙）；这个是"跑的过程中就要知道它在干什么"——`cli::upgrade`
/// 的五阶段管道（分步计时 / 可取消 / 百分比）靠它供血：命令跑完才知道
/// 发生了什么的话，管道全程只能画"执行中"，那正是 v8 定稿时如实登记的
/// "事件没接上之前不画假进度"。
///
/// 三条实现上的口径：
/// - **`on_line` 在调用方线程上同步调**（主循环从 channel 里取），两条 reader
///   线程只负责把行搬进 channel ⇒ 不必 `Sync`（`FnMut` 就够），stdout /
///   stderr 按到达序交错（终端的本来样子，日志照实记，不重新排）
/// - **取消 / 超时杀进程树**（`taskkill /T`）：`cmd /C winget upgrade …` 里
///   真正干活的是 cmd 的孙子进程，光 `child.kill()` 只杀掉 cmd 自己——
///   屏幕上"已取消"、盘上还在装，那是取消键说谎
/// - **取消与退出撞车时以退出为准**：杀之前再 `try_wait` 一次，已经跑完的
///   命令不因为取消旗号晚到 20ms 就被报成"已取消"
///
/// 返回的 `CmdOutput`：`code == None` = 被杀了（取消或超时），`stdout` /
/// `stderr` 是杀之前收到的部分。调用方用自己的取消旗号区分是哪一种
pub fn run_program_live(
    program: &str,
    args: &[&str],
    timeout: Duration,
    cancel: &AtomicBool,
    mut on_line: impl FnMut(&str),
) -> Result<CmdOutput, String> {
    let mut cmd = Command::new("cmd");
    cmd.arg("/C").arg(program).args(args);
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(crate::git::CREATE_NO_WINDOW);

    let mut child = cmd.spawn().map_err(|e| format!("无法启动 {program}：{e}"))?;

    // 两条 reader 线程按行读，行进 channel（带来源标记：stderr 单独攒一份）
    let (tx, rx) = mpsc::channel::<(bool, String)>();
    let out_tx = tx.clone();
    let out_h = child.stdout.take().map(|r| pump_lines(r, out_tx, false));
    let err_h = child.stderr.take().map(|r| pump_lines(r, tx, true));

    let mut stdout = String::new();
    let mut stderr = String::new();
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                // 把已到达的行交出去（顺带攒进完整输出）。**先交行再判退出**：
                // 退出前最后一批输出不能丢
                while let Ok((is_err, line)) = rx.try_recv() {
                    on_line(&line);
                    if is_err {
                        stderr.push_str(&line);
                        stderr.push('\n');
                    } else {
                        stdout.push_str(&line);
                        stdout.push('\n');
                    }
                }
                if cancel.load(Ordering::SeqCst) {
                    // 取消与退出撞车：以退出为准（模块头第三条）
                    if let Ok(Some(s)) = child.try_wait() {
                        break Some(s);
                    }
                    kill_tree(&mut child);
                    break None;
                }
                if start.elapsed() > timeout {
                    kill_tree(&mut child);
                    break None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("等待 {program} 退出失败：{e}")),
        }
    };

    // reader 线程随管道关闭自然结束；join 之前把残留的行收干净
    // （杀进程之后管道里可能还有没读完的行——那些也是"发生了什么"的一部分）
    for h in out_h.into_iter().chain(err_h) {
        let _ = h.join();
    }
    while let Ok((is_err, line)) = rx.try_recv() {
        on_line(&line);
        if is_err {
            stderr.push_str(&line);
            stderr.push('\n');
        } else {
            stdout.push_str(&line);
            stdout.push('\n');
        }
    }

    Ok(CmdOutput {
        code: status.and_then(|s| s.code()),
        stdout,
        stderr,
    })
}

/// 杀进程树（取消 / 超时共用）。`cmd /C <program>` 结构下真正干活的进程是
/// cmd 的**后代**，只 `child.kill()` 会留下一个没人管的孤儿接着跑。
/// taskkill 自己起不来就当没杀成，至少还挨一下 `child.kill()`
fn kill_tree(child: &mut std::process::Child) {
    let pid = child.id();
    let _ = run_program(
        "taskkill",
        &["/PID", &pid.to_string(), "/T", "/F"],
        Duration::from_secs(5),
    );
    let _ = child.kill();
    let _ = child.wait();
}

/// 把一条流按行读出来，每行进 channel；流关（子进程退出 / 被杀）后线程结束。
///
/// 两处行界口径：**行尾的 `\r\n` / `\n` / `\r` 都剥掉**（`BufRead::lines()`
/// 同款——`\r` 留着的话等宽日志每行行尾多个不可见字符，行距会花）；
/// **行内残留的 `\r` 只送最后一段**：进度条用回车在同一行反复刷新，
/// 那一行在终端停下来时的样子就是最后一段，中间的刷新态是过程不是结果。
///
/// 按字节读、UTF-8 宽松解（与 [`read_all`] 同一条：一个非 UTF-8 字节不该
/// 让整条流变空串）
fn pump_lines<R: std::io::Read + Send + 'static>(
    r: R,
    tx: mpsc::Sender<(bool, String)>,
    is_err: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(r);
        let mut buf: Vec<u8> = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    while matches!(buf.last(), Some(b'\n' | b'\r')) {
                        buf.pop();
                    }
                    if buf.is_empty() {
                        continue;
                    }
                    let text = String::from_utf8_lossy(&buf).into_owned();
                    let shown = text.rsplit('\r').next().unwrap_or(&text);
                    if shown.is_empty() {
                        continue;
                    }
                    if tx.send((is_err, shown.to_string())).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
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
