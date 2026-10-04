//! git 网络操作编排：直连→代理回退、认证注入、stderr 进度解析。
//! 自 src-tauri 平移：`Channel<ProgressEvent>` 改为回调闭包，设置读写走 core store。

use std::time::Duration;

use crate::proxy;
use crate::store::{self, AppConfig};
use crate::types::ProgressEvent;
use crate::git;

/// 本地 git 命令（status / log 等）超时
pub const LOCAL_TIMEOUT: Duration = Duration::from_secs(10);

/// 从 git 进度行的参数部分提取传输量与网速：
/// ":  6% (120/2000), 1.21 MiB | 56.00 KiB/s" → "已传输 1.21 MiB · 56.00 KiB/s"
/// 无速度信息（如 "done."、纯计数行）时返回空串
fn transfer_detail(rest: &str) -> String {
    let tail = match rest.split_once("), ") {
        Some((_, t)) => t,
        None => return String::new(),
    };
    if !tail.contains("/s") {
        return String::new();
    }
    format!("已传输 {}", tail.replace(" | ", " · "))
}

/// 当前网络路径标记（"直连" / "经代理 127.0.0.1:7897" 等），
/// 附加到每条进度行的阶段文本上，让用户随时看到当前走的是哪条路（切换提示只闪现一瞬间，不够）
#[derive(Clone, Default)]
pub struct RouteTag(std::sync::Arc<std::sync::Mutex<String>>);

impl RouteTag {
    fn set(&self, s: &str) {
        if let Ok(mut g) = self.0.lock() {
            *g = s.to_string();
        }
    }
    fn get(&self) -> String {
        self.0.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

/// 从阶段提示文本推断当前路径标记
fn route_of(msg: &str) -> String {
    if let Some(rest) = msg.strip_prefix("直连失败，正在通过代理 ") {
        let addr = rest.trim_end_matches(" 重试…");
        return format!("经代理 {addr}");
    }
    if msg.contains("代理并行") {
        return "并行".into();
    }
    "直连".into()
}

/// 生成配对的进度回调：forward 转发 git stderr 进度行（阶段带路径标记），
/// stage 在直连/代理切换瞬间发出提示并同步路径标记。
/// on_progress 传 None 时回调静默空转（后台自动同步没有前端通道）。
pub fn progress_hooks(
    on_progress: Option<&dyn Fn(&ProgressEvent)>,
) -> (RouteTag, impl Fn(&str) + '_, impl Fn(&str) + '_) {
    let tag = RouteTag::default();
    let t = tag.clone();
    let forward = move |line: &str| {
        if let Some(on_progress) = on_progress {
            if let Some(mut ev) = parse_progress(line) {
                let r = t.get();
                if !r.is_empty() {
                    ev.stage = format!("{}（{}）", ev.stage, r);
                }
                on_progress(&ev);
            }
        }
    };
    let t2 = tag.clone();
    let stage = move |s: &str| {
        t2.set(&route_of(s));
        if let Some(on_progress) = on_progress {
            on_progress(&ProgressEvent {
                stage: s.into(),
                detail: String::new(),
                percent: None,
            });
        }
    };
    (tag, forward, stage)
}

/// 解析 git 的 stderr 进度行，转换成可展示的进度事件
pub fn parse_progress(line: &str) -> Option<ProgressEvent> {
    let line = line.trim();
    let pct = |s: &str| -> Option<f32> {
        let idx = s.find('%')?;
        let start = s[..idx].rfind(|c: char| !c.is_ascii_digit() && c != '.')? + 1;
        s[start..idx].parse().ok()
    };
    for (kw, stage) in [
        ("Receiving objects", "接收对象"),
        ("Resolving deltas", "解析增量"),
        ("Updating files", "检出文件"),
        ("Counting objects", "统计对象"),
        ("Compressing objects", "压缩对象"),
        ("Enumerating objects", "枚举对象"),
    ] {
        if let Some(rest) = line.strip_prefix(kw) {
            return Some(ProgressEvent {
                stage: stage.into(),
                detail: transfer_detail(rest),
                percent: pct(rest),
            });
        }
    }
    if line.starts_with("Cloning into") {
        return Some(ProgressEvent {
            stage: "开始克隆".into(),
            detail: line.to_string(),
            percent: None,
        });
    }
    if line.starts_with("From ") || line.starts_with("FETCH_HEAD") {
        return Some(ProgressEvent {
            stage: "获取远端引用".into(),
            detail: line.to_string(),
            percent: None,
        });
    }
    if line.starts_with("Fast-forward") {
        return Some(ProgressEvent {
            stage: "合并更新".into(),
            detail: line.to_string(),
            percent: None,
        });
    }
    None
}

const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(BASE64_CHARS[(n >> 18) as usize & 63] as char);
        out.push(BASE64_CHARS[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            BASE64_CHARS[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64_CHARS[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// 识别远端地址所属平台
fn host_of(url: &str) -> Option<&'static str> {
    let u = url.to_lowercase();
    if u.contains("github.com") {
        Some("github")
    } else if u.contains("gitee.com") {
        Some("gitee")
    } else {
        None
    }
}

fn is_ssh_url(url: &str) -> bool {
    url.starts_with("git@") || url.starts_with("ssh://")
}

/// 按远端平台生成本次网络操作的认证配置：
/// - https + 令牌：注入 http.extraheader 基本认证（GitHub 用 x-access-token，Gitee 用 oauth2）
/// - ssh + 私钥：注入 GIT_SSH_COMMAND 指定私钥
fn build_net_auth(cfg: &AppConfig, remote_url: &str) -> (Vec<String>, Option<String>) {
    let mut config = Vec::new();
    let mut ssh = None;
    if let Some(host) = host_of(remote_url) {
        let token = match host {
            "github" => cfg.settings.github_token.trim(),
            "gitee" => cfg.settings.gitee_token.trim(),
            _ => "",
        };
        if !token.is_empty() && !is_ssh_url(remote_url) {
            let user = if host == "github" {
                "x-access-token"
            } else {
                "oauth2"
            };
            let b64 = base64_encode(format!("{user}:{token}").as_bytes());
            config.push(format!("http.extraheader=AUTHORIZATION: basic {b64}"));
        }
    }
    let key = cfg.settings.ssh_key_path.trim();
    if !key.is_empty() && is_ssh_url(remote_url) {
        ssh = Some(format!(
            "ssh -i {key} -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new"
        ));
    }
    (config, ssh)
}

/// 读取仓库 origin 远端地址（读不到则返回空串）
pub fn remote_url(repo_path: &str) -> String {
    run_local(repo_path, &["remote", "get-url", "origin"])
        .stdout
        .trim()
        .to_string()
}

/// 把 origin 远端地址转成浏览器能打开的仓库主页地址：
/// `git@host:path` 与 `ssh://` 形式转 https，http(s) 形式去掉 `.git` 后缀；
/// 认不出来的形式返回 None（让调用方给"没有可识别的远端地址"，不要瞎拼一个地址）
///
/// 这段原先只在 Tauri 版的 shell.rs 里，两套界面各写一份的话迟早漂移
/// （比如 Gitee 改域名格式，只有其中一边知道）。纯解析，不碰系统，放 core。
pub fn browse_url_from_remote(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    if url.is_empty() {
        return None;
    }
    let strip_git = |s: &str| s.strip_suffix(".git").unwrap_or(s).to_string();
    if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        return Some(format!("https://{host}/{}", strip_git(path)));
    }
    if let Some(rest) = url.strip_prefix("ssh://") {
        let rest = rest.split_once('@').map(|(_, r)| r).unwrap_or(rest);
        let (host, path) = rest.split_once('/')?;
        return Some(format!("https://{host}/{}", strip_git(path)));
    }
    if url.starts_with("https://") || url.starts_with("http://") {
        return Some(strip_git(url));
    }
    None
}

/// 提交时使用的 Git 身份：**交错好的 `-c` 项**，可直接接在子命令前面
/// （`git -c user.name=.. -c user.email=.. commit`）。设置里未填则返回空，
/// 交给全局 / 仓库配置。
///
/// 交错是在这里做好的：调用方直接 `argv.extend(...)` 就行，
/// 不必各自记得补 `-c`——漏了就变成 `git user.name=x commit` 这种非法命令。
pub fn identity_config(cfg: &AppConfig) -> Vec<String> {
    let mut c = Vec::new();
    if !cfg.settings.git_name.trim().is_empty() {
        c.push("-c".to_string());
        c.push(format!("user.name={}", cfg.settings.git_name.trim()));
    }
    if !cfg.settings.git_email.trim().is_empty() {
        c.push("-c".to_string());
        c.push(format!("user.email={}", cfg.settings.git_email.trim()));
    }
    c
}

/// 执行本地 git 命令（不涉及网络，不走代理回退）
pub fn run_local(repo_path: &str, args: &[&str]) -> git::GitOutput {
    git::run_git(repo_path, args, None, LOCAL_TIMEOUT)
}

/// 直连与已知可用代理并行赛跑：谁先成功用谁。
/// 解决直连被墙时长时间挂起、要等满超时才轮到代理重试的问题。
fn race_direct_vs_proxy(
    repo_path: &str,
    args: &[&str],
    git_config: &[String],
    ssh: Option<&str>,
    proxy_addr: &str,
    on_line: Option<&dyn Fn(&str)>,
    timeout: Duration,
) -> (git::GitOutput, Option<String>) {
    let (line_tx, line_rx) = std::sync::mpsc::channel::<String>();
    let spawn_attempt = |proxy: Option<String>, line_tx: std::sync::mpsc::Sender<String>| {
        let repo = repo_path.to_string();
        let owned_args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let owned_cfg = git_config.to_vec();
        let owned_ssh = ssh.map(|s| s.to_string());
        let git_proxy = proxy.as_deref().map(proxy::to_git_proxy_url);
        std::thread::spawn(move || {
            let args: Vec<&str> = owned_args.iter().map(String::as_str).collect();
            git::run_git_auth(
                &repo,
                &args,
                git_proxy.as_deref(),
                timeout,
                Some(&|line: &str| {
                    let _ = line_tx.send(line.to_string());
                }),
                &owned_cfg,
                owned_ssh.as_deref(),
            )
        })
    };
    let mut t_direct = Some(spawn_attempt(None, line_tx.clone()));
    let mut t_proxy = Some(spawn_attempt(Some(proxy_addr.to_string()), line_tx));
    let fail_out = || git::GitOutput {
        success: false,
        stdout: String::new(),
        stderr: "git 进程异常退出".into(),
        code: -1,
        timed_out: false,
    };

    let mut direct_out: Option<git::GitOutput> = None;
    let mut proxy_out: Option<git::GitOutput> = None;
    loop {
        while let Ok(line) = line_rx.try_recv() {
            if let Some(cb) = on_line {
                cb(&line);
            }
        }
        if direct_out.is_none() && t_direct.as_ref().is_some_and(|h| h.is_finished()) {
            direct_out = Some(
                t_direct
                    .take()
                    .unwrap()
                    .join()
                    .unwrap_or_else(|_| fail_out()),
            );
            if direct_out.as_ref().unwrap().success {
                return (direct_out.unwrap(), None);
            }
        }
        if proxy_out.is_none() && t_proxy.as_ref().is_some_and(|h| h.is_finished()) {
            proxy_out = Some(
                t_proxy
                    .take()
                    .unwrap()
                    .join()
                    .unwrap_or_else(|_| fail_out()),
            );
            if proxy_out.as_ref().unwrap().success {
                return (
                    proxy_out.unwrap(),
                    Some(proxy::to_git_proxy_url(proxy_addr)),
                );
            }
        }
        if direct_out.is_some() && proxy_out.is_some() {
            // 两路都失败：返回直连结果，交给外层继续走候选代理串行回退
            return (direct_out.unwrap(), None);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 网络路线：Auto=直连优先失败转代理（默认）；DirectOnly=只直连；ProxyOnly=只走代理
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NetMode {
    Auto,
    DirectOnly,
    ProxyOnly,
}

impl NetMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "direct" => NetMode::DirectOnly,
            "proxy" => NetMode::ProxyOnly,
            _ => NetMode::Auto,
        }
    }
}

/// 网络型 git 操作的统一入口：直连优先，失败且判定为网络问题时自动探测代理重试。
/// 已有验证可用的代理（last_good_proxy）时，直连与该代理并行赛跑取先成功者。
/// 代理只注入到当次 git 子进程的环境变量，结束即失效，不改系统代理。
/// auth_url 为参与认证判定的远端地址；None 时取 repo 的 origin。
/// git_config / ssh 为按平台令牌认证配置（见 build_net_auth）。
/// allow_race 为 false 时不做并行赛跑（clone 会同时写同一目标目录，必须串行）。
/// on_stage 在"直连 / 某代理"尝试切换时回调，供前端展示当前走的网络路径。
#[allow(clippy::too_many_arguments)]
pub fn run_net_op(
    repo_path: &str,
    args: &[&str],
    on_line: Option<&dyn Fn(&str)>,
    auth_url: Option<&str>,
    timeout: Duration,
    allow_race: bool,
    on_stage: Option<&dyn Fn(&str)>,
    mode: NetMode,
) -> (git::GitOutput, Option<String>) {
    git::clear_cancel();
    // 强制 IPv4：IPv6 被禁用/不可用的机器上，git 会先尝试 AAAA 连接白白浪费时间
    let mut owned_args: Vec<&str> = args.to_vec();
    owned_args.insert(1, "--ipv4");
    let args = owned_args.as_slice();
    let cfg = store::load();
    let url = match auth_url {
        Some(u) => u.to_string(),
        None => remote_url(repo_path),
    };
    let (mut git_config, ssh) = build_net_auth(&cfg, &url);
    // 强制 HTTP/1.1：git 默认的 HTTP/2 走本地代理（clash 等 CONNECT 隧道）时
    // 存在著名的龟速问题——浏览器正常、克隆只有十几 KiB/s，1.1 可直接解决
    git_config.push("http.version=HTTP/1.1".into());
    // 传输停滞保护（http.lowSpeedLimit/Time：持续低于限速达到时限即中止本次尝试）
    // - 直连首试用激进阈值：GitHub 直连被限速时常见十几 KiB/s 的"龟速爬行"，
    //   按 1KB/s 阈值永远不会判失败，代理就永远没机会接管；20 秒低于 30KB/s 即让位
    // - 代理尝试与直连兜底用保守阈值：45 秒低于 1KB/s 才算真卡死，避免误杀慢节点
    let mut direct_cfg = git_config.clone();
    direct_cfg.push("http.lowSpeedLimit=30720".into());
    direct_cfg.push("http.lowSpeedTime=20".into());
    git_config.push("http.lowSpeedLimit=1000".into());
    git_config.push("http.lowSpeedTime=45".into());

    if allow_race && mode == NetMode::Auto {
        if let Some(last_good) = cfg.last_good_proxy.clone() {
            if let Some(cb) = on_stage {
                cb("直连与已缓存代理并行尝试中…");
            }
            let (out, proxy_url) = race_direct_vs_proxy(
                repo_path,
                args,
                &git_config,
                ssh.as_deref(),
                &last_good,
                on_line,
                timeout,
            );
            if out.success {
                if proxy_url.is_some() {
                    let mut cfg = store::load();
                    cfg.last_good_proxy = Some(last_good);
                    let _ = store::save(&cfg);
                }
                return (out, proxy_url);
            }
        }
    }

    let mut direct: Option<git::GitOutput> = None;
    if mode != NetMode::ProxyOnly {
        if let Some(cb) = on_stage {
            cb("正在尝试直连…");
        }
        // 用户强制直连时用保守停滞阈值，尊重其选择（龟速也由它跑，静默挂死仍会中止）
        let direct_cfg = if mode == NetMode::DirectOnly {
            git_config.clone()
        } else {
            direct_cfg
        };
        let out = git::run_git_auth(
            repo_path,
            args,
            None,
            timeout,
            on_line,
            &direct_cfg,
            ssh.as_deref(),
        );
        if out.success || !git::is_network_error(&out) {
            return (out, None);
        }
        direct = Some(out);
    }

    let candidates = if mode == NetMode::DirectOnly {
        Vec::new()
    } else {
        proxy::gather_candidates(&cfg.settings, &cfg.last_good_proxy)
    };
    for candidate in candidates {
        // 仅当赛跑真的跑过（allow_race 且 last_good 存在）才跳过重复候选；
        // clone 等禁用赛跑的操作若也跳过，last_good 代理就永远没机会被尝试
        if allow_race && Some(&candidate) == cfg.last_good_proxy.as_ref() {
            continue;
        }
        // 探测偶尔卡在超时红线上（clash 建连 3~5 秒波动），失败再给一次机会
        if !proxy::probe(&candidate) && !proxy::probe(&candidate) {
            continue;
        }
        if let Some(cb) = on_stage {
            cb(&format!("直连失败，正在通过代理 {} 重试…", candidate));
        }
        let url = proxy::to_git_proxy_url(&candidate);
        let out = git::run_git_auth(
            repo_path,
            args,
            Some(&url),
            timeout,
            on_line,
            &git_config,
            ssh.as_deref(),
        );
        if out.success {
            let mut cfg = store::load();
            cfg.last_good_proxy = Some(candidate);
            let _ = store::save(&cfg);
            return (out, Some(url));
        }
        // 用户已取消：立即返回，不再尝试后续候选与兜底
        if out.stderr.contains(git::CANCELLED_MSG) {
            return (out, None);
        }
    }

    // 用户已取消且直连结果也是被取消的：直接返回
    if let Some(d) = &direct {
        if d.stderr.contains(git::CANCELLED_MSG) {
            return (d.clone(), None);
        }
    }

    // 强制代理却没有任何可用候选：直接报错，不回退直连
    if mode == NetMode::ProxyOnly {
        return (
            git::GitOutput {
                success: false,
                stdout: String::new(),
                stderr: "未找到可用代理，请确认代理软件已运行，或在设置中手动填写代理地址".into(),
                code: -1,
                timed_out: false,
            },
            None,
        );
    }

    // 所有候选代理都不可用（仅 Auto）：用保守阈值再兜底直连一次——
    // 直连可能只是龟速（不满足激进阈值）而非不可达，没代理时仍要能慢慢完成
    if mode == NetMode::Auto {
        if let Some(cb) = on_stage {
            cb("未找到可用代理，改用直连继续（速度可能较慢）…");
        }
        let retry = git::run_git_auth(
            repo_path,
            args,
            None,
            timeout,
            on_line,
            &git_config,
            ssh.as_deref(),
        );
        if retry.success {
            return (retry, None);
        }
    }
    // 走到这里说明 mode 为 Auto/DirectOnly，直连结果一定存在
    (direct.expect("direct result"), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_lines_are_parsed() {
        let ev = parse_progress("Receiving objects:  45% (123/273), 1.2 MiB | 2.3 MiB/s").unwrap();
        assert_eq!(ev.stage, "接收对象");
        assert_eq!(ev.percent, Some(45.0));
        assert_eq!(ev.detail, "已传输 1.2 MiB · 2.3 MiB/s");
        let ev = parse_progress("Resolving deltas: 100% (10/10), done.").unwrap();
        assert_eq!(ev.percent, Some(100.0));
        assert_eq!(ev.detail, ""); // done. 无速度信息
        assert!(parse_progress("fatal: unable to access").is_none());
    }

    #[test]
    fn base64_works() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn host_detection_works() {
        assert_eq!(host_of("https://github.com/u/r.git"), Some("github"));
        assert_eq!(host_of("git@gitee.com:u/r.git"), Some("gitee"));
        assert_eq!(host_of("https://example.com/u/r.git"), None);
        assert!(is_ssh_url("git@github.com:u/r.git"));
        assert!(!is_ssh_url("https://github.com/u/r.git"));
    }

    #[test]
    fn identity_args_carry_their_own_dash_c() {
        // 调用方直接把返回值拼进 argv：漏掉 -c 就变成「git user.name=x commit」这种非法命令
        let mut cfg = store::AppConfig::default();
        assert!(identity_config(&cfg).is_empty(), "没配身份就不干预 git");

        cfg.settings.git_name = " 木言 ".into();
        cfg.settings.git_email = "a@b.c".into();
        assert_eq!(
            identity_config(&cfg),
            vec!["-c", "user.name=木言", "-c", "user.email=a@b.c"]
        );
    }

    #[test]
    fn net_auth_uses_platform_tokens() {
        let mut cfg = store::AppConfig::default();
        cfg.settings.github_token = "gh-tok".into();
        cfg.settings.gitee_token = "ge-tok".into();
        cfg.settings.ssh_key_path = "C:\\keys\\id_ed25519".into();

        let (c, ssh) = build_net_auth(&cfg, "https://github.com/u/r.git");
        assert_eq!(c.len(), 1);
        assert!(c[0].starts_with("http.extraheader=AUTHORIZATION: basic "));
        // x-access-token:gh-tok 的 base64
        assert!(c[0].ends_with(&base64_encode(b"x-access-token:gh-tok")));
        assert!(ssh.is_none()); // https 远端不注入 ssh

        let (c, _) = build_net_auth(&cfg, "https://gitee.com/u/r.git");
        assert!(c[0].ends_with(&base64_encode(b"oauth2:ge-tok")));

        let (_, ssh) = build_net_auth(&cfg, "git@github.com:u/r.git");
        assert!(ssh.unwrap().contains("C:\\keys\\id_ed25519"));

        // 无令牌、无 ssh 远端时不注入任何东西
        let empty = store::AppConfig::default();
        let (c, ssh) = build_net_auth(&empty, "https://example.com/u/r.git");
        assert!(c.is_empty() && ssh.is_none());
    }
}
#[cfg(test)]
mod browse_tests {
    use super::browse_url_from_remote;

    #[test]
    fn scp_style_ssh_becomes_https() {
        assert_eq!(
            browse_url_from_remote("git@github.com:user/repo.git"),
            Some("https://github.com/user/repo".into())
        );
        assert_eq!(
            browse_url_from_remote("git@gitee.com:group/sub/proj.git"),
            Some("https://gitee.com/group/sub/proj".into())
        );
    }

    #[test]
    fn ssh_url_becomes_https_and_drops_the_user_part() {
        assert_eq!(
            browse_url_from_remote("ssh://git@github.com/user/repo.git"),
            Some("https://github.com/user/repo".into())
        );
    }

    #[test]
    fn http_urls_only_lose_the_git_suffix() {
        assert_eq!(
            browse_url_from_remote("https://github.com/user/repo.git"),
            Some("https://github.com/user/repo".into())
        );
        // 已经是主页地址就原样返回，不要再剥一次
        assert_eq!(
            browse_url_from_remote("http://gitee.com/a/b/"),
            Some("http://gitee.com/a/b".into())
        );
    }

    #[test]
    fn unrecognised_forms_return_none_instead_of_a_guessed_url() {
        // 拼一个假的地址比不给更糟：用户点开是 404，还以为仓库没了
        assert_eq!(browse_url_from_remote(""), None);
        assert_eq!(browse_url_from_remote("   "), None);
        assert_eq!(browse_url_from_remote("D:/some/local/path"), None);
        assert_eq!(browse_url_from_remote("github.com/user/repo"), None);
    }
}
