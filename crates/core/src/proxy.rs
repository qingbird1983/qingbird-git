use crate::store::Settings;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// 常见代理端口：clash-verge(7897) / clash(7890,7891) / v2rayN(10808,10809) / 常见杂项
const COMMON_PORTS: &[u16] = &[7897, 7890, 7891, 10808, 10809, 2080, 8889, 1080, 8118];

fn tcp_connect(host: &str, port: u16, timeout: Duration) -> Option<TcpStream> {
    let addr = (host, port).to_socket_addrs().ok()?.next()?;
    TcpStream::connect_timeout(&addr, timeout).ok()
}

/// 通过 HTTP CONNECT 隧道连 github.com:443 验证代理真正可用。
/// 超时给足余量：clash 规则模式下建立节点连接可能要 3~5 秒，
/// 探测卡在红线上会导致"明明有代理却判定不可用"
fn probe_http_proxy(bare_addr: &str) -> bool {
    let Some((host, port)) = split_addr(bare_addr) else {
        return false;
    };
    let Some(mut stream) = tcp_connect(&host, port, Duration::from_secs(5)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(8)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let req = "CONNECT github.com:443 HTTP/1.1\r\nHost: github.com:443\r\n\r\n";
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 128];
    match stream.read(&mut buf) {
        Ok(n) if n > 0 => {
            let head = &buf[..n];
            head.starts_with(b"HTTP/1.1 200") || head.starts_with(b"HTTP/1.0 200")
        }
        _ => false,
    }
}

fn split_addr(addr: &str) -> Option<(String, u16)> {
    let (host, port) = addr.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    Some((host.trim_matches(['[', ']']).to_string(), port))
}

/// 读取 Windows 系统代理（IE/WinInet 设置），clash 等开启"系统代理"后会写到这里
#[cfg(windows)]
fn system_proxy() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .ok()?;
    let enable: u32 = key.get_value("ProxyEnable").ok()?;
    if enable == 0 {
        return None;
    }
    let server: String = key.get_value("ProxyServer").ok()?;
    let candidate = if server.contains('=') {
        // 分代理格式："http=...;https=...;ftp=..."
        server.split(';').find_map(|kv| {
            let (scheme, value) = kv.split_once('=')?;
            (scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
                .then(|| value.to_string())
        })?
    } else if server.contains("://") {
        server.rsplit_once("://").map(|(_, s)| s).unwrap_or("").to_string()
    } else {
        server
    };
    let candidate = candidate.trim().to_string();
    (!candidate.is_empty()).then_some(candidate)
}

#[cfg(not(windows))]
fn system_proxy() -> Option<String> {
    None
}

/// 生成按优先级排列的候选代理列表（bare 地址，如 "127.0.0.1:7890"，manual 可带 scheme）
pub fn gather_candidates(settings: &Settings, last_good: &Option<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let push = |c: Option<String>, out: &mut Vec<String>| {        if let Some(c) = c {
            let c = c.trim().to_string();
            if !c.is_empty() && !out.contains(&c) {
                out.push(c);
            }
        }
    };
    let manual = settings.manual_proxy.trim().to_string();
    if !manual.is_empty() {
        push(Some(manual), &mut out);
    }
    push(last_good.clone(), &mut out);
    push(system_proxy(), &mut out);
    for port in COMMON_PORTS {
        push(Some(format!("127.0.0.1:{port}")), &mut out);
    }
    out
}

pub fn is_socks(candidate: &str) -> bool {
    candidate.starts_with("socks5") || candidate.starts_with("socks4")
}

/// 转成 git 可用的代理 URL；手动值保留原 scheme，扫描出的默认按 http 代理处理
pub fn to_git_proxy_url(candidate: &str) -> String {
    if candidate.contains("://") {
        candidate.to_string()
    } else {
        format!("http://{candidate}")
    }
}

/// 验证候选代理是否可用；socks 只做 TCP 可达检查，http 做 CONNECT 实测
pub fn probe(candidate: &str) -> bool {
    let bare = candidate
        .rsplit_once("://")
        .map(|(_, s)| s)
        .unwrap_or(candidate);
    if is_socks(candidate) {
        let Some((host, port)) = split_addr(bare) else {
            return false;
        };
        tcp_connect(&host, port, Duration::from_secs(5)).is_some()
    } else {
        probe_http_proxy(bare)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Settings;

    #[test]
    fn candidates_order_manual_first_and_dedup() {
        let settings = Settings { manual_proxy: "http://127.0.0.1:7890".into(), ..Default::default() };
        let cands = gather_candidates(&settings, &Some("http://127.0.0.1:7890".into()));
        assert_eq!(cands[0], "http://127.0.0.1:7890");
        // 去重：manual 与 last_good 相同只出现一次
        assert_eq!(cands.iter().filter(|c| c.as_str() == "http://127.0.0.1:7890").count(), 1);
        // 常见端口在列表里
        assert!(cands.iter().any(|c| c.ends_with(":7897")));
    }

    #[test]
    fn git_proxy_url_keeps_scheme() {
        assert_eq!(to_git_proxy_url("127.0.0.1:7890"), "http://127.0.0.1:7890");
        assert_eq!(to_git_proxy_url("socks5://127.0.0.1:1080"), "socks5://127.0.0.1:1080");
    }

    #[test]
    fn split_addr_works() {
        assert_eq!(split_addr("127.0.0.1:7890"), Some(("127.0.0.1".into(), 7890)));
        assert_eq!(split_addr("localhost:noport"), None);
    }
}
