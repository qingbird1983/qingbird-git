//! 软件内自更新（Q44，2026-10-13 拍板：**全自动版**）。
//!
//! 三步：检查（GitHub Releases API）→ 下载安装包到临时目录 → 界面拉起安装器
//! （拉起与退出在 gui 侧，这里只管前两步）。
//!
//! ## 为什么 HTTP 走系统 `curl.exe` 而不是引 HTTP 库
//!
//! 引 ureq/reqwest 这类库就要碰 TLS 栈：默认的 rustls→**ring 带 build script**，
//! 而依赖带 build script 会破坏进程树内的编译缓存复用（2026-09-30 实测坑，
//! AGENTS 有记录）。`curl.exe` 在 Windows 10 1803+ 系统自带（System32），
//! TLS 走 Schannel——**零新依赖**，TLS 与代理语义都交给一个久经考验的外部工具，
//! 与「git 封装=spawn git」是同一条架构路线。
//!
//! ## 代理回退与 `net::run_net_op` 同一套哲学
//!
//! 直连优先；失败且判定为**网络问题**（curl 退出码 6/7/28/35/52/55/56，见
//! `is_network_code`）才挨个试 `proxy::gather_candidates` 的候选，每个候选先
//! `proxy::probe` 探活（与 net.rs 一样探两次给 clash 建连留波动余量），成功后
//! 写回 `last_good_proxy`——下次连探活都省掉。代理 URL 复用
//! `proxy::to_git_proxy_url`（裸地址补 `http://`，socks 原样保留，curl 两种都认）。
//!
//! 下载进度**不在这一层解析 curl 的进度条**（stderr 的 `#` 行格式会漂）：
//! 安装包总大小在 API 响应里有（`asset_size`），调用方轮询目标文件大小即可，
//! 又简单又可测。

use crate::proxy;
use crate::store;
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

/// 本应用在 GitHub 上的仓库（releases 由此发）。
pub const OWNER_REPO: &str = "qingbird1983/qingbird-git";

/// 安装包资产名的公共前缀 / 后缀（发版物 `qingbird-git-Setup-<版本>.exe`）。
const ASSET_PREFIX: &str = "qingbird-git-Setup-";
const ASSET_SUFFIX: &str = ".exe";

/// 检查用超时：一次 API 查询，30 秒足够（含代理候选回退的每段）。
const CHECK_TIMEOUT: u64 = 30;
/// 下载用超时：9 MB 级安装包，慢代理 10 分钟封顶。
const DOWNLOAD_TIMEOUT: u64 = 600;

/// 一次 release 检查的结果里 GUI 需要的东西。
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// 发布标签，如 `"v0.3.1"`
    pub tag: String,
    /// 安装包资产名，如 `"qingbird-git-Setup-0.3.1.exe"`
    pub asset_name: String,
    /// 安装包下载地址（`browser_download_url`，会 302 到 objects.githubusercontent.com）
    pub asset_url: String,
    /// 安装包字节数；0 = API 没给（进度条退化为不定长）
    pub asset_size: u64,
}

/// 下载期间向外暴露的信号（GUI 轮询读，core 只写）。
#[derive(Default)]
pub struct DownloadSignals {
    /// 实际用来下载的代理地址（`http://host:port`）；None = 直连。
    /// **每次换路（直连↔某代理）都更新**——界面上"正在通过哪个口下载"靠它
    pub via: Mutex<Option<String>>,
}

/// `"v0.3.1"` / `"0.3.1"` / `"0.3.1-beta"` → `(0, 3, 1)`。
///
/// 只认三段数字：两段与四段都返回 None——**宁可不提示更新，也不要拿
/// 一个猜出来的版本号去比大小**（比错方向的代价是"永远提示更新"或
/// "永远不提示"，两者都比不提示更糟）。
pub fn version_triple(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    // 预发布/构建元数据尾巴不参与比较：0.3.1-beta 的"版本数"就是 0.3.1
    let s = s.split(['-', '+']).next()?;
    let mut it = s.split('.');
    let maj: u32 = it.next()?.parse().ok()?;
    let min: u32 = it.next()?.parse().ok()?;
    let pat: u32 = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((maj, min, pat))
}

/// `latest_tag` 是否比 `current` 新。任一侧解析不出 ⇒ false（不提示）。
pub fn is_newer(current: &str, latest_tag: &str) -> bool {
    match (version_triple(current), version_triple(latest_tag)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

/// 这颗资产是不是我们的安装包。**单出一颗纯函数**：解析与测试共用同一份判据，
/// 别在两处各写一遍 `starts_with`。
fn is_setup_asset(name: &str) -> bool {
    name.starts_with(ASSET_PREFIX) && name.ends_with(ASSET_SUFFIX)
}

/// 从 `/releases/latest` 的响应体里挑出安装包。
///
/// GitHub 的 `latest` 已经排除了 draft 与 prerelease，不用再滤一遍。
/// 资产可能有多个（zip / 校验文件…），**只认名字对得上安装包的那一颗**。
pub fn parse_release(json: &str) -> Option<Release> {
    let v: Value = serde_json::from_str(json).ok()?;
    let tag = v.get("tag_name")?.as_str()?.trim().to_string();
    if tag.is_empty() {
        return None;
    }
    for a in v.get("assets")?.as_array()? {
        let name = a.get("name")?.as_str()?;
        if !is_setup_asset(name) {
            continue;
        }
        return Some(Release {
            tag,
            asset_name: name.to_string(),
            asset_url: a.get("browser_download_url")?.as_str()?.to_string(),
            asset_size: a.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
        });
    }
    None
}

/// curl 的这些退出码 = "没连上 / 连上了传不动"，**换一条网络路重试有意义**。
/// HTTP 4xx/5xx（退出码 22）不在内——那说明请求本身到了对端，换十个代理答案一样。
pub fn is_network_code(exit: i32) -> bool {
    matches!(exit, 6 | 7 | 16 | 28 | 35 | 52 | 55 | 56)
}

struct CurlFail {
    exit: i32,
    stderr: String,
}

/// 跑一次 curl，收成功时的响应体。
fn curl_run(url: &str, proxy: Option<&str>, timeout: u64, out: &Path) -> Result<(), CurlFail> {
    let mut cmd = Command::new("curl.exe");
    cmd.arg("-f") // HTTP >= 400 视为失败（退出码 22）
        .arg("-sSL") // 静默 + 报错 + 跟随 302（下载地址会跳 CDN）
        .arg("--max-time")
        .arg(timeout.to_string())
        // 低速让位：持续 30 秒低于 10KB/s 即中止——与 net.rs 给 git 配的
        // http.lowSpeedLimit 同一条哲学，"龟速爬行"不该拖死整条链路
        .arg("--speed-limit")
        .arg("10240")
        .arg("--speed-time")
        .arg("30")
        .arg("-H")
        .arg("User-Agent: qingbird-git-updater")
        .arg("-o")
        .arg(out);
    if let Some(p) = proxy {
        cmd.arg("-x").arg(p);
    }
    cmd.arg(url);
    // ⚠️ 必须带 CREATE_NO_WINDOW（32-15）：本应用是 GUI 程序，curl.exe 是控制台
    // 子进程，漏了这颗 flag 就是用户报的「点检查更新时黑色终端窗口闪一下」
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(crate::git::CREATE_NO_WINDOW);
    }
    match cmd.output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => {
            // curl 的报错在 stderr（`Command::output` 已收好，直接读）
            let stderr = String::from_utf8_lossy(&o.stderr).trim().replace('\n', " ");
            let stderr = if stderr.chars().count() > 200 {
                format!("{}…", stderr.chars().take(200).collect::<String>())
            } else {
                stderr
            };
            Err(CurlFail {
                exit: o.status.code().unwrap_or(-1),
                stderr,
            })
        }
        Err(e) => Err(CurlFail {
            exit: -1,
            stderr: format!("启动 curl.exe 失败：{e}（Windows 10 1803+ 系统自带）"),
        }),
    }
}

fn friendly(exit: i32, stderr: &str) -> String {
    let head = match exit {
        -1 => "无法启动下载工具".to_string(),
        6 => "无法解析 GitHub 域名（DNS 不通）".to_string(),
        7 => "无法连接 GitHub（直连不通）".to_string(),
        28 => "连接超时".to_string(),
        35 | 52 | 55 | 56 => "连接在传输途中断开".to_string(),
        22 => format!("GitHub 返回了错误（HTTP 层面被拒绝）"),
        _ => format!("curl 退出码 {exit}"),
    };
    if stderr.is_empty() {
        head
    } else {
        format!("{head}：{stderr}")
    }
}

/// 检查最新 release。**直连优先，网络型失败才走代理候选**；
/// 代理成功照 `net.rs` 的惯例写回 `last_good_proxy`。
pub fn fetch_latest() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{OWNER_REPO}/releases/latest");
    let body_path = std::env::temp_dir().join("qingbird-git-release.json");
    let _ = std::fs::remove_file(&body_path);
    fetch_via_fallback(&url, &body_path, CHECK_TIMEOUT, &DownloadSignals::default())?;
    let body = std::fs::read_to_string(&body_path)
        .map_err(|e| format!("读不到检查结果：{e}"))?;
    let _ = std::fs::remove_file(&body_path);
    parse_release(&body)
        .ok_or_else(|| "GitHub 响应里没有安装包资产（可能这版没有附安装包）".to_string())
}

/// 「直连 → 代理候选」的公共骨架，检查与下载共用。
///
/// 返回成功时实际用的代理（None = 直连），失败时是**最后一条路**的报错。
/// 候选顺序、探活两次、写回 `last_good_proxy` 三件事都只在这一份里。
fn fetch_via_fallback(
    url: &str,
    dest: &Path,
    timeout: u64,
    signals: &DownloadSignals,
) -> Result<Option<String>, String> {
    *signals.via.lock().unwrap_or_else(|e| e.into_inner()) = None;
    if let Err(f) = curl_run(url, None, timeout, dest) {
        // 非网络型失败（4xx 等）换代理没有意义，直接报
        if !is_network_code(f.exit) {
            return Err(friendly(f.exit, &f.stderr));
        }
    } else {
        return Ok(None);
    }

    let cfg = store::load();
    for candidate in proxy::gather_candidates(&cfg.settings, &cfg.last_good_proxy) {
        // 与 net.rs 同款：探测偶尔卡在超时红线上，失败再给一次机会
        if !proxy::probe(&candidate) && !proxy::probe(&candidate) {
            continue;
        }
        let purl = proxy::to_git_proxy_url(&candidate);
        *signals.via.lock().unwrap_or_else(|e| e.into_inner()) = Some(purl.clone());
        // 换路重下：curl 默认从头写，直连留下的半个文件不碍事
        if curl_run(url, Some(&purl), timeout, dest).is_ok() {
            let mut cfg = store::load();
            cfg.last_good_proxy = Some(candidate);
            let _ = store::save(&cfg);
            return Ok(Some(purl));
        }
    }
    Err("直连与所有候选代理都连不上 GitHub，请检查网络或稍后再试".to_string())
}

/// 下载安装包到 `dest`。阻塞式（调用方放后台线程）；
/// 进度由调用方轮询 `dest` 的文件大小（总大小用 `Release::asset_size`）。
pub fn download_installer(rel: &Release, dest: &Path, signals: &DownloadSignals) -> Result<(), String> {
    fetch_via_fallback(&rel.asset_url, dest, DOWNLOAD_TIMEOUT, signals)?;
    // 空文件不算成功：curl 某些半路死法会留下 0 字节的壳
    let len = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    if len == 0 {
        return Err("下载结果是空的，请稍后再试".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_triple_reads_v_prefixed_and_plain_tags() {
        assert_eq!(version_triple("v0.3.1"), Some((0, 3, 1)));
        assert_eq!(version_triple("0.3.1"), Some((0, 3, 1)));
        assert_eq!(version_triple("  v0.10.2 "), Some((0, 10, 2)));
        // 预发布尾巴剥掉再比：0.3.1-beta 的版本数是 0.3.1
        assert_eq!(version_triple("v0.3.1-beta.1"), Some((0, 3, 1)));
    }

    #[test]
    fn version_triple_refuses_what_it_cannot_count() {
        assert_eq!(version_triple("v0.3"), None);
        assert_eq!(version_triple("v0.3.1.2"), None);
        assert_eq!(version_triple(""), None);
        assert_eq!(version_triple("版本"), None);
        assert_eq!(version_triple("vX.Y.Z"), None);
    }

    /// 比**大小**不比字典序：`0.3.10` 比 `0.3.9` 新——按字符串比它反而在前。
    /// 这一档写错，用户要么永远收不到更新提示、要么永远被提示。
    #[test]
    fn is_newer_compares_numerically_not_lexicographically() {
        assert!(is_newer("0.3.9", "v0.3.10"));
        assert!(is_newer("0.3.1", "v0.4.0"));
        assert!(is_newer("0.3.1", "v1.0.0"));
    }

    #[test]
    fn same_or_older_is_not_newer() {
        assert!(!is_newer("0.3.1", "v0.3.1"));
        assert!(!is_newer("0.4.0", "v0.3.1"));
        // 解析不出的一侧 ⇒ 不提示，不要拿猜的比
        assert!(!is_newer("0.3.1", "vNext"));
        assert!(!is_newer("dev", "v0.4.0"));
    }

    const FIXTURE: &str = r#"{
        "tag_name": "v0.4.0",
        "name": "v0.4.0",
        "assets": [
            { "name": "qingbird-git-0.4.0.zip", "size": 100, "browser_download_url": "https://x/z.zip" },
            { "name": "qingbird-git-Setup-0.4.0.exe", "size": 9773050, "browser_download_url": "https://x/setup.exe" }
        ]
    }"#;

    #[test]
    fn parse_release_picks_the_setup_exe_not_the_zip() {
        let r = parse_release(FIXTURE).expect("fixture 里明明有安装包");
        assert_eq!(r.tag, "v0.4.0");
        assert_eq!(r.asset_name, "qingbird-git-Setup-0.4.0.exe");
        assert_eq!(r.asset_url, "https://x/setup.exe");
        assert_eq!(r.asset_size, 9773050);
    }

    #[test]
    fn parse_release_refuses_shapes_without_a_setup_asset() {
        assert_eq!(parse_release("不是 JSON"), None);
        assert_eq!(parse_release(r#"{"tag_name":"v1"}"#), None, "没有 assets");
        assert_eq!(
            parse_release(r#"{"tag_name":"v1","assets":[{"name":"x.zip","size":1,"browser_download_url":"https://x"}]}"#),
            None,
            "有 assets 但没有安装包 ⇒ None，让调用方报『没有安装包资产』"
        );
    }

    #[test]
    fn is_setup_asset_matches_prefix_and_suffix_exactly() {
        assert!(is_setup_asset("qingbird-git-Setup-0.3.1.exe"));
        assert!(!is_setup_asset("qingbird-git-0.3.1.zip"));
        assert!(!is_setup_asset("qingbird-git-Setup-0.3.1.exe.sha256"));
        assert!(!is_setup_asset("Setup-0.3.1.exe"));
    }

    #[test]
    fn network_codes_cover_the_connectivity_family_only() {
        for c in [6, 7, 16, 28, 35, 52, 55, 56] {
            assert!(is_network_code(c), "{c} 该算网络问题");
        }
        // 22 = HTTP >= 400：请求到了对端才有的码，换代理没有意义
        assert!(!is_network_code(22));
        assert!(!is_network_code(0));
    }
}
