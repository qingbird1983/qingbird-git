use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return fail(format!("无法启动 git 进程: {e}")),
    };

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
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if let Some(cb) = on_line {
                    while let Ok(line) = line_rx.try_recv() {
                        cb(&line);
                    }
                }
                if start.elapsed() > timeout {
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
            let mut stderr = err;
            stderr.push_str("\n操作超时，已终止");
            GitOutput { success: false, stdout: out, stderr, code: -1, timed_out: true }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(stderr: &str, timed_out: bool) -> GitOutput {
        GitOutput { success: false, stdout: String::new(), stderr: stderr.into(), code: -1, timed_out }
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
