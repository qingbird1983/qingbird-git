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

/// 剥掉 scheme，取裸 `host:port`。
fn bare_addr(candidate: &str) -> &str {
    candidate
        .rsplit_once("://")
        .map(|(_, s)| s)
        .unwrap_or(candidate)
}

/// 只做 TCP 可达检查（**不做 CONNECT 实测**）。
///
/// 与 `probe` 的区别是刻意的：赛跑**起跑前不该先验车**——赛跑本身就是在验证
/// 代理能不能用。起跑前先做一次完整 `probe()`（CONNECT github.com:443，
/// 建连 5s + 读 8s）等于把本该并行的事先串行做完一遍，代理软件没在跑时那
/// 十几秒纯粹白等，而那恰恰是最需要它早点让位的场景。端口不通的那一路
/// 几毫秒就退赛，赢的那一路完全不受影响。
pub fn tcp_reachable(candidate: &str) -> bool {
    let Some((host, port)) = split_addr(bare_addr(candidate)) else {
        return false;
    };
    tcp_connect(&host, port, Duration::from_secs(2)).is_some()
}

/// 赛跑要用的那个候选代理：**手填 → 缓存 → 系统代理**。None = 没有可赛跑的对手。
///
/// 顺序与 `gather_candidates` 的前三位一致（manual, last_good, system），
/// 理由是**信号强度**：手填是用户亲自敲进去的，缓存是上次验证过的，
/// 系统代理是"开着代理软件就必然有"——越靠前越可能是他真正想要的。
///
/// ## 为什么要加「系统代理」这一档
///
/// 原来只在 `last_good_proxy` 有值时才赛跑，于是**冷启动必然先直连**：
/// 用户清过一次数据（或换机器、换代理软件），缓存是空的 ⇒ 第一次网络操作
/// 老老实实走串行直连 ⇒ 被墙时那 20 秒「低速让位」机制抓不到
/// （TCP 建连就通、卡在 TLS/HTTP 响应那一步，**传输循环压根没进**）⇒
/// 白等满 `NET_TIMEOUT` 300 秒才想起还有代理。
///
/// 而系统代理是**开着代理软件就必然存在**的那一档（`ProxyEnable=1`），
/// 拿它当赛跑对手，冷启动第一次就变成"直连 6 秒 / 代理 11 秒，谁先成谁上"，
/// **不用等 5 分钟**。赛跑成功后会把这个地址写回 `last_good_proxy`
/// （`net.rs`），下一次连 `tcp_reachable` 都不用调。
///
/// ## 前两档不探活，第三档要探
///
/// `manual_proxy` 与 `last_good_proxy` 都不探：手填是用户明确指定的
/// （探不通就说明他填错了，那该报错而不是悄悄换一个），
/// 缓存的定义就是"上次验证过能用"。赛跑对这两者都宽容——不灵就输了。
/// 只有系统代理要探：它**不是用户指定的**，是"碰巧开着"，
/// 拿一个没在运行的地址去赛跑等于白起一个 git 进程。
pub fn race_candidate(settings: &Settings, last_good: &Option<String>) -> Option<String> {
    let manual = settings.manual_proxy.trim();
    if !manual.is_empty() {
        return Some(manual.to_string());
    }
    if let Some(lg) = last_good {
        let t = lg.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    system_proxy().filter(|s| tcp_reachable(s))
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
    let bare = bare_addr(candidate);
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

    /// 赛跑对手**优先手填与缓存，且这两档都不探活**。
    ///
    /// "不探活"是这两档的设计要点，判据要钉住：起跑前做一次完整 `probe()`
    /// 等于把本该并行的事先串行做完一遍，代理软件没在跑时那十几秒纯白等。
    /// 所以给了个**故意填错的**地址也得照样返回它——赛跑对它宽容，输了就是输了。
    /// 而手填那档更不该探：用户亲自敲的，探不通说明他填错了，
    /// **那该报错而不是悄悄换一个代理**。
    #[test]
    fn race_candidate_prefers_manual_then_cache_without_probing() {
        let none = Settings::default();
        // 手填里可能带 scheme（`http://…`），也可能是裸 `host:port`——都原样带出去，
        // `to_git_proxy_url` 才负责补 scheme
        let manual = Settings { manual_proxy: "socks5://127.0.0.1:1080".into(), ..Default::default() };
        assert_eq!(
            race_candidate(&manual, &Some("127.0.0.1:1".into())),
            Some("socks5://127.0.0.1:1080".to_string()),
            "手填优先于缓存，且不探活"
        );
        // 纯空白的缓存不算"有缓存"——那会让它赢下这一档，然后拿空串起进程
        assert_eq!(
            race_candidate(&none, &Some("   ".into())),
            race_candidate(&none, &None),
            "空白缓存与无缓存必须同档"
        );
        assert_eq!(
            race_candidate(&none, &Some("127.0.0.1:1".into())),
            Some("127.0.0.1:1".to_string()),
            "缓存那一档不探活：它按定义就是验证过能用的（:1 上没有东西在听）"
        );
    }

    /// 前两档都没有时只看系统代理；它不是用户指定的，**所以要探活**——
    /// 拿一个没在运行的地址去赛跑等于白起一个 git 进程。
    /// 断言的是**契约**（要么是那个系统代理、要么 None），不是具体值：
    /// 这条在装了代理软件的机器与没装的机器上都得绿
    #[test]
    fn race_candidate_falls_back_to_a_live_system_proxy_or_nothing() {
        let none = Settings::default();
        if let Some(sp) = system_proxy() {
            assert_eq!(race_candidate(&none, &None), Some(sp).filter(|s| tcp_reachable(s)));
        } else {
            assert_eq!(race_candidate(&none, &None), None);
        }
    }

    #[test]
    fn tcp_reachable_rejects_garbage_fast() {
        // 解析不出端口的**必须立刻返回 false**，不能去 connect 一个 :0
        assert!(!tcp_reachable("不是地址"));
        assert!(!tcp_reachable("127.0.0.1:不是端口"));
        // scheme 要剥掉再拆
        assert!(split_addr(bare_addr("http://127.0.0.1:7890")).is_some());
        assert_eq!(bare_addr("socks5://1.2.3.4:1080"), "1.2.3.4:1080");
        assert_eq!(bare_addr("1.2.3.4:1080"), "1.2.3.4:1080");
    }

    #[test]
    fn split_addr_works() {
        assert_eq!(split_addr("127.0.0.1:7890"), Some(("127.0.0.1".into(), 7890)));
        assert_eq!(split_addr("localhost:noport"), None);
    }
}
