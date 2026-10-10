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
    // **只认「并行」两个字，不认「代理并行」**。2026-10-06 之前赛跑提示写的是
    // 「直连与**已缓存代理**并行尝试中…」，赛跑对手扩到系统代理后文案变成
    // 「直连与**代理 127.0.0.1:7897** 并行尝试中…」——代理与并行被地址隔开，
    // 那个连续子串不成立了，而后半句又确实只有并行这一种形态
    if msg.contains("并行") {
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
                // **stage 为空就别拼路径标记**——那类行是"只有原文"的分支，
                // 拼出来会变成 `（直连）` 这种无意义文本
                if !r.is_empty() && !ev.stage.is_empty() {
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
                // 路径切换提示是**我们自己说的**，不是 git 说的，
                // 所以 raw 留空——否则日志里会出现两行重复的东西
                raw: String::new(),
                to_log: true,
            });
        }
    };
    (tag, forward, stage)
}

/// 解析 git 的 stderr 进度行，转换成可展示的进度事件
///
/// **认不出的行也要返回**（`stage` / `detail` 留空、只带 `raw` 原文）——
/// 这条是 2026-10-05 修的：原来返回 `None`，而调用方是
/// `if let Some(ev) = parse_progress(line)`，于是**没进白名单的行被静默丢掉**。
/// 用户看到的现象是「拉取过程一点看不到，只有一句拉取成功」。
///
/// 丢掉的是最有信息量的那批：git 输出**远端侧**进度时前面带 ` remote: `
/// （`remote: Compressing objects: 100% …`），而白名单是 `strip_prefix`
/// **从行首**匹配的，前缀一挡就全不中。
pub fn parse_progress(line: &str) -> Option<ProgressEvent> {
    let line = line.trim();
    // 远端侧的进度行会带这个前缀，先剥掉再匹配。**顺序不能反**：
    // 先匹配的话 `remote: Compressing objects` 永远认不出来。
    let body = line.strip_prefix("remote:").map(str::trim_start).unwrap_or(line);
    let raw = line.to_string();
    // 「只有原文」的分支：解析不出来的行照常上屏，stage/detail 留空。
    // **收参数而不是 capture `raw`**——capture 会把它 move 走，
    // 后面每个 return 分支再用就编译不过
    let plain = |raw: &str, to_log: bool| {
        Some(ProgressEvent {
            stage: String::new(),
            detail: String::new(),
            percent: None,
            raw: raw.to_string(),
            to_log,
        })
    };

    let pct = |s: &str| -> Option<f32> {
        let idx = s.find('%')?;
        let start = s[..idx].rfind(|c: char| !c.is_ascii_digit() && c != '.')? + 1;
        s[start..idx].parse().ok()
    };
    for (kw, stage) in [
        ("Updating files", "检出文件"),
        ("Checking out files", "检出文件"),
        ("Unpacking objects", "解包对象"),
        ("Counting objects", "统计对象"),
        ("Compressing objects", "压缩对象"),
        ("Enumerating objects", "枚举对象"),
        ("Receiving objects", "接收对象"),
        ("Resolving deltas", "解析增量"),
        ("Writing objects", "写对象"),
    ] {
        if let Some(rest) = body.strip_prefix(kw) {
            let percent = pct(rest);
            // **到 100% 才进日志**：`Receiving objects: 45% …` 每秒刷十几行，
            // 全塞进日志会把真正的记录冲掉。完成那一条是"这一步做完了"，
            // 才是用户要看的东西。judge 放这里——core 才懂 git 输出的形状
            let done = percent.map(|p| p >= 100.0).unwrap_or(true);
            return Some(ProgressEvent {
                stage: stage.into(),
                detail: transfer_detail(rest),
                percent,
                raw,
                to_log: done,
            });
        }
    }
    if body.starts_with("Cloning into") {
        return Some(ProgressEvent {
            stage: "开始克隆".into(),
            detail: body.to_string(),
            percent: None,
            raw,
            to_log: true,
        });
    }
    if body.starts_with("From ") || body.starts_with("FETCH_HEAD") {
        return Some(ProgressEvent {
            stage: "获取远端引用".into(),
            detail: body.to_string(),
            percent: None,
            raw,
            to_log: true,
        });
    }
    if body.starts_with("Fast-forward") {
        return Some(ProgressEvent {
            stage: "合并更新".into(),
            detail: body.to_string(),
            percent: None,
            raw,
            to_log: true,
        });
    }
    // git 的报错与提示行（`warning:` / `fatal:` / `error:` / `Auto-merging` …）
    // 一律原文上屏。**只滤纯空行**——宁可多一行也不要静默吞掉信息。
    if body.is_empty() {
        None
    } else {
        plain(&raw, true)
    }
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

/// **收尾必达**那一类本地命令：用户已经按了停止也照样跑完。
///
/// 与 `run_local` 的唯一区别就是不受进程级取消标记影响，
/// 判据与限制都写在 `git::run_essential` 那条注释里
/// （一句话：**只给"这一步不做完，用户的东西就回不来"的命令用**，
/// 网络操作与一切可以被打断的活儿照旧走 `run_local`）
pub fn run_local_essential(repo_path: &str, args: &[&str]) -> git::GitOutput {
    git::run_essential(repo_path, args, LOCAL_TIMEOUT)
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
    // 消息带**路标签**（2026-10-05 修）。原来两路的行走同一个裸 channel，
    // 接收时不区分来源，于是用户看到同一段 `remote: Enumerating objects…`
    // **原样出现两遍**却没有任何交代——那是直连与代理在赛跑，两边都真跑了。
    // 修好后每路第一次出现时插一条 `—— 直连 ——` / `—— 经代理 x ——` 分隔行，
    // 原文一字不动（**不要给 raw 加前缀**：那是 git 的输出，不是我们的话）
    let (line_tx, line_rx) = std::sync::mpsc::channel::<(String, String)>();
    let spawn_attempt = |label: &str, proxy: Option<String>, line_tx: std::sync::mpsc::Sender<(String, String)>| {
        let repo = repo_path.to_string();
        let owned_args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let owned_cfg = git_config.to_vec();
        let owned_ssh = ssh.map(|s| s.to_string());
        let git_proxy = proxy.as_deref().map(proxy::to_git_proxy_url);
        let label = label.to_string();
        std::thread::spawn(move || {
            let args: Vec<&str> = owned_args.iter().map(String::as_str).collect();
            git::run_git_auth(
                &repo,
                &args,
                git_proxy.as_deref(),
                timeout,
                Some(&|line: &str| {
                    let _ = line_tx.send((label.clone(), line.to_string()));
                }),
                &owned_cfg,
                owned_ssh.as_deref(),
            )
        })
    };
    let mut t_direct = Some(spawn_attempt("直连", None, line_tx.clone()));
    // 没有可用代理地址时标签别留个空括号（合成环境/没配代理时会出现
    // `—— 经代理 ——` 这种没头没尾的一行）
    let proxy_label = if proxy_addr.trim().is_empty() {
        "代理路".to_string()
    } else {
        format!("经代理 {}", proxy_addr.trim())
    };
    let mut t_proxy = Some(spawn_attempt(
        &proxy_label,
        Some(proxy_addr.to_string()),
        line_tx,
    ));
    let fail_out = || git::GitOutput {
        success: false,
        stdout: String::new(),
        stderr: "git 进程异常退出".into(),
        code: -1,
        timed_out: false,
    };

    let mut direct_out: Option<git::GitOutput> = None;
    let mut proxy_out: Option<git::GitOutput> = None;
    // 已经打过分隔行的路。最多两路，Vec 就够
    let mut announced: Vec<String> = Vec::new();
    loop {
        while let Ok((label, line)) = line_rx.try_recv() {
            if !announced.iter().any(|a| a == &label) {
                announced.push(label.clone());
                if let Some(cb) = on_line {
                    cb(&format!("—— {label} ——"));
                }
            }
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

/// 这条 git 子命令会不会往**本地仓库**写东西（refs / FETCH_HEAD / objects / 工作区）。
///
/// 判据不放调用方而放这里，是因为「会不会写」由**命令自己**决定，不由调用方决定。
/// 以前它是 `run_net_op` 的一个 `bool` 参数，四个调用点里三个传 `true`
/// （`sync.rs` 的检测/拉取/推送、`fork.rs` 的变基拉取），只有克隆传 `false`——
/// 而克隆那条理由（「并行的两条路会同时写同一个目录」）**对 pull 一字不改地成立**。
/// ⇒ D78 的又一处：**必填信息没进签名 ⇒ 每条路径都得自己记得，其中一条一定漏**。
///
/// **实测（2026-10-10，`file://` 远端 + 沙箱配置把 `last_good_proxy` 写成死端口，
/// 两路都会起跑）**：赛跑时同一秒里有两条 `git pull --ff-only` 在写同一个 `.git`，撞出来的是
/// `error: cannot lock ref 'refs/remotes/origin/main': is at X but expected Y`
/// 紧跟 `fatal: Cannot fast-forward to multiple branches.`，而第三次串行兜底会成功
/// ⇒ 症状正是「每次拉取日志里先撞两行墙再说成功」（路线表 31.4 补二十⑥ 登记的那条）。
///
/// 命名：答的是**事实**（会不会写），不是推论（该不该赛跑）。⚠️ **这颗公开函数别改名成
/// `may_race`** ——`edge()` 那条教训（注释把用途写死，第二个用途来了就另起一个名字，
/// 同一个值于是长出两个名字）说的就是这件事。
/// 函数里那个局部 `may_race` 是它的**推论**，推论名挂在局部没问题。
pub fn writes_the_repo(args: &[&str]) -> bool {
    // 第一个不是选项的位置参数才是子命令；`-c k=v` 的值是**下一个参数**，要一起跳过
    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        if a == "-c" {
            i += 2;
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        break;
    }
    let sub = args.get(i).copied().unwrap_or("");
    // **安全侧在"算会写"这一边**：只有明确列出的只读命令才允许并行，
    // 认不出来的（含空参数）一律不赛跑。宁可少跑一次赛跑，不可多撞一次墙——
    // 前者最坏是慢几秒，后者是往用户日志里塞两行 fatal
    !matches!(sub, "ls-remote")
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
/// **只有不写本地仓库的命令（`writes_the_repo`）才并行赛跑**——会写的命令
/// （fetch / pull / push / clone）**一律串行**：赛跑是同一次调用里并发起两条
/// 一模一样的 git 进程，让它们同时写同一个 `.git` 会撞 ref 锁（实测撞出
/// `fatal: Cannot fast-forward to multiple branches.`，详见那颗函数的注释）。
/// 代理只注入到当次 git 子进程的环境变量，结束即失效，不改系统代理。
/// auth_url 为参与认证判定的远端地址；None 时取 repo 的 origin。
/// git_config / ssh 为按平台令牌认证配置（见 build_net_auth）。
/// on_stage 在"直连 / 某代理"尝试切换时回调，供前端展示当前走的网络路径。
#[allow(clippy::too_many_arguments)]
pub fn run_net_op(
    repo_path: &str,
    args: &[&str],
    on_line: Option<&dyn Fn(&str)>,
    auth_url: Option<&str>,
    timeout: Duration,
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

    // 赛跑对手：缓存优先，其次系统代理（`proxy::race_candidate` 里有"为什么要
    // 加系统代理这一档"的完整理由——冷启动缓存为空时，串行直连会被卡满
    // NET_TIMEOUT 300 秒，因为低速让位机制在 TLS/HTTP 响应那一步还没进传输循环）
    // 赛跑与否由**命令自己**决定（见 `writes_the_repo`），不给调用方一个可以传错的布尔
    let may_race = !writes_the_repo(args);
    let mut raced: Option<String> = None;
    if may_race && mode == NetMode::Auto {
        if let Some(race_with) = proxy::race_candidate(&cfg.settings, &cfg.last_good_proxy) {
            if let Some(cb) = on_stage {
                // **文案里带上地址**：这个赛跑对手可能是缓存、也可能是系统代理，
                // 说"已缓存代理"在后者那一刻是句假话，而用户正要靠这一句判断
                // 代理软件有没有生效
                cb(&format!("直连与代理 {race_with} 并行尝试中…"));
            }
            let (out, proxy_url) = race_direct_vs_proxy(
                repo_path,
                args,
                &git_config,
                ssh.as_deref(),
                &race_with,
                on_line,
                timeout,
            );
            if out.success {
                if proxy_url.is_some() {
                    let mut cfg = store::load();
                    cfg.last_good_proxy = Some(race_with.clone());
                    let _ = store::save(&cfg);
                }
                return (out, proxy_url);
            }
            raced = Some(race_with);
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
        // 只跳过**真的赛跑过**的那个对手。
        // 判据原来是 `allow_race`——那是"允许赛跑"，比"跑过"宽一档：`mode != Auto` 时
        // 赛跑压根不起跑（下面那个 `if` 进不去），而它仍然是 `true`，于是
        // `last_good_proxy` 那一档会被当成"已经试过"而**永远跳过**。
        // ⚠️ **但这一档当前不可达**：界面只传 `NetMode::Auto`（`dialogs/clone.rs:215` 那一条），
        // `NetMode::parse` 认的 "direct"/"proxy" 两个值**零调用方** ⇒
        // **这条不是"修掉了一个用户碰到的 bug"，是把判据换成不需要再补条件的形状**
        // （路线表 D74 的前置教训：现象与缺陷要分清，否则下一个人会以为这里曾经坏过）。
        // **两个都要跳**：赛跑对手可能是缓存、也可能是系统代理，
        // 少跳一个就是同一个地址连试两遍
        if raced.is_some()
            && (Some(&candidate) == cfg.last_good_proxy.as_ref()
                || Some(&candidate) == raced.as_ref())
        {
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

    /// 32-1 的判据：**会不会写本地仓库**决定能不能并行赛跑。
    /// 钉的不是"赛跑快不快"，是"两条一模一样的 git 会不会同时写同一个 `.git`"——
    /// 实测那样做会撞出 `cannot lock ref` + `fatal: Cannot fast-forward to
    /// multiple branches.`（`fetch`/`pull`/`push`/`clone` 全都会撞上）。
    #[test]
    fn only_an_op_that_lands_nothing_locally_may_run_in_parallel() {
        // 会写的一律不许并行（四个真实调用点的参数形状，逐条钉住）
        for args in [
            vec!["fetch", "--all", "--prune", "--progress"],
            vec!["pull", "--ff-only", "--progress"],
            vec!["pull", "--rebase", "--progress"],
            vec!["push", "--progress"],
            vec!["push", "--progress", "--set-upstream", "origin", "HEAD"],
            vec!["clone", "--progress", "https://github.com/u/r.git", "r"],
        ] {
            assert!(writes_the_repo(&args), "{args:?} 会写本地仓库，判成了只读");
        }
        // 只落远端答复、不碰本地的才可以并行
        for args in [vec!["ls-remote"], vec!["ls-remote", "--heads", "origin"]] {
            assert!(!writes_the_repo(&args), "{args:?} 不落地，却被判成会写");
        }
    }

    /// **安全侧要落在哪一边**：认不出来的子命令一律算"会写"（即不赛跑）。
    /// 反过来判的话，将来新增一条没列进表里的写命令就会**默认拿到赛跑**——
    /// 那正是 32-1 要消灭的那种"每条路径都得自己记得"。
    #[test]
    fn an_unrecognised_subcommand_is_treated_as_a_write() {
        assert!(writes_the_repo(&[]), "空参数应当算会写");
        assert!(writes_the_repo(&["--version"]), "认不出的子命令应当算会写");
        assert!(writes_the_repo(&["rebase", "--continue"]), "没列进表里的写操作应当算会写");
    }

    /// 子命令是"**第一个不是选项的位置参数**"，不是"永远是 args[0]"：
    /// `-c k=v` 那对要一起跳过（它的值是下一个参数），`run_net_op` 自己插的 `--ipv4`
    /// 在 index 1，也靠这一条判据才不会被当成子命令。
    #[test]
    fn leading_global_options_do_not_hide_the_subcommand() {
        assert!(writes_the_repo(&["-c", "http.proxy=", "pull", "--ff-only"]));
        assert!(!writes_the_repo(&["-c", "http.proxy=", "ls-remote"]));
        assert!(writes_the_repo(&["--ipv4", "fetch", "--all"]));
    }

    /// 签名形状本身也钉住：**「要不要赛跑」不许再回到参数表里**。
    /// 这条断言不判运行结果，它判"这个函数能不能被那样调用"——
    /// 谁再加回一个 `bool`，这里就编译不过（D78：让「忘」没有选项，靠编译器不靠记性）。
    #[test]
    fn the_race_switch_is_not_a_caller_supplied_boolean() {
        let _entry: fn(
            &str,
            &[&str],
            Option<&dyn Fn(&str)>,
            Option<&str>,
            std::time::Duration,
            Option<&dyn Fn(&str)>,
            NetMode,
        ) -> (git::GitOutput, Option<String>) = run_net_op;
    }

    #[test]
    fn progress_lines_are_parsed() {
        let ev = parse_progress("Receiving objects:  45% (123/273), 1.2 MiB | 2.3 MiB/s").unwrap();
        assert_eq!(ev.stage, "接收对象");
        assert_eq!(ev.percent, Some(45.0));
        assert_eq!(ev.detail, "已传输 1.2 MiB · 2.3 MiB/s");
        let ev = parse_progress("Resolving deltas: 100% (10/10), done.").unwrap();
        assert_eq!(ev.percent, Some(100.0));
        assert_eq!(ev.detail, ""); // done. 无速度信息
    }

    /// **2026-10-05 的退化就在这里**：`remote: ` 前缀一挡，白名单
    /// 从行首匹配全部落空 → `if let Some` 把它静默丢掉 → 用户看到
    /// 「过程一点看不到」。这四行是真实 git 输出的原样。
    #[test]
    fn remote_prefixed_progress_is_not_dropped() {
        for line in [
            " remote: Compressing objects: 100% (1234/1234), done.",
            " remote: Counting objects: 100% (5000/5000), done.",
            " Unpacking objects: 100% (3000/3000), done.",
            " Checking out files:  50% (500/1000)",
        ] {
            let ev = parse_progress(line)
                .unwrap_or_else(|| panic!("这行被丢了：{line}"));
            assert!(!ev.raw.is_empty(), "raw 必须带回原文：{line}");
        }
        // 剥掉前缀后要能认出阶段
        assert_eq!(
            parse_progress(" remote: Compressing objects: 100% (1/1), done.")
                .unwrap()
                .stage,
            "压缩对象"
        );
        assert_eq!(
            parse_progress(" Unpacking objects: 100% (3/3), done.")
                .unwrap()
                .stage,
            "解包对象"
        );
        // **原文要去掉前导空格**（git 输出带一个前导空格）
        assert_eq!(
            parse_progress(" remote: Counting objects: 100% (5/5), done.")
                .unwrap()
                .raw,
            "remote: Counting objects: 100% (5/5), done."
        );
    }

    /// 路径标签（`（直连）` / `（经代理 127.0.0.1:7897）`）与网速是
    /// 进度行上"现在走哪条路、多快"的**唯一来源**。
    ///
    /// 2026-10-05 用户报「进度条上没有直连/代理字样，也没有网速」——
    /// core 这一半一直是好的（`route_of` 一直在拼、`transfer_detail`
    /// 一直在抽），**坏的是 GUI 侧把它丢了**。这条测试把 core 这一半钉住：
    /// 走完 `on_stage` → `forward` 这条真实链路，断言两样都到了
    /// `ProgressEvent` 上。以后谁动 `route_of` 或 `parse_progress`，
    /// 少了哪一样都会在这里红，而不是等到用户发现界面上少了一行字
    #[test]
    fn route_tag_and_speed_reach_progress_events() {
        let evs: std::cell::RefCell<Vec<ProgressEvent>> = std::cell::RefCell::new(Vec::new());
        {
            let sink = |ev: &ProgressEvent| evs.borrow_mut().push(ev.clone());
            let (_tag, forward, stage) = progress_hooks(Some(&sink));

            // `run_net_op` 每次切路径前都会先回调一次 `on_stage`（net.rs:524 / :563 / :612）
            stage("正在尝试直连…");
            forward("Receiving objects:  45% (123/273), 1.2 MiB | 2.3 MiB/s");
            stage("直连失败，正在通过代理 127.0.0.1:7897 重试…");
            forward("Receiving objects:  60% (160/273), 1.6 MiB | 2.9 MiB/s");
            // 赛跑那一条的标签是「并行」，不能退化成「直连」
            stage("直连与代理 127.0.0.1:7897 并行尝试中…");
            forward("Receiving objects:  70% (190/273), 1.9 MiB | 3.1 MiB/s");
        }
        // ⚠️ 那个块不能省：`sink` 借着 `evs`，而下面要把它 move 走
        let v = evs.into_inner();
        // 事件顺序：stage, forward, stage, forward, stage, forward
        assert_eq!(v[1].stage, "接收对象（直连）", "直连时路径标签要跟在阶段后面");
        assert_eq!(v[1].detail, "已传输 1.2 MiB · 2.3 MiB/s", "网速要送到 detail");
        assert_eq!(v[3].stage, "接收对象（经代理 127.0.0.1:7897）");
        assert_eq!(v[3].detail, "已传输 1.6 MiB · 2.9 MiB/s");
        assert_eq!(v[5].stage, "接收对象（并行）", "赛跑那一条的标签是「并行」");

        // 切路径那几条是**我们自己说的**，raw 必须空着——
        // 否则日志里会多出一行和输出流重复的东西
        assert!(v[0].raw.is_empty() && v[2].raw.is_empty() && v[4].raw.is_empty());
    }

    /// 赛跑提示的文案 2026-10-06 变了（「直连与**已缓存代理**并行尝试中…」→
    /// 「直连与**代理 127.0.0.1:7897** 并行尝试中…」，因为赛跑对手扩到了系统代理）。
    /// **`route_of` 原先认的是「代理并行」这个连续子串**，新文案里代理与并行
    /// 被地址隔开 ⇒ 那一帧的路径标签会退化成「直连」，**而那恰恰是赛跑阶段**，
    /// 用户看到的路径标签全程说"直连"。判据改成只认「并行」两个字
    #[test]
    fn race_stage_is_labelled_parallel_not_direct() {
        // 新文案
        assert_eq!(route_of("直连与代理 127.0.0.1:7897 并行尝试中…"), "并行");
        // 旧文案仍要认（配置里可能还有历史文案对应的路径）
        assert_eq!(route_of("直连与已缓存代理并行尝试中…"), "并行");
        // 两种常见提示不受影响
        assert_eq!(route_of("正在尝试直连…"), "直连");
        assert_eq!(
            route_of("直连失败，正在通过代理 127.0.0.1:7897 重试…"),
            "经代理 127.0.0.1:7897"
        );
    }

    /// 认不出的原文行**不该把上一次的路径标签与网速冲掉**。
    ///
    /// 这条是 GUI 侧 `LogEvent::Stage` 那个守卫（`ops.rs`：空 stage / 空 detail
    /// 都不覆盖）的理由——`remote: Total 42 (delta 0)…` 这类行里既没有阶段
    /// 也没有速度，覆盖下去会让「经代理 xxx」在进度刷新时一闪一闪
    #[test]
    fn unrecognized_lines_carry_no_stage_or_detail() {
        // ⚠️ 别拿 `Enumerating objects` 试——**它在白名单里**（`net.rs` 的关键词表），
        // 会解析出「枚举对象」。认不出的是这种汇总行
        let ev = parse_progress(" remote: Total 42 (delta 0), reused 0 (delta 0), pack-reused 0")
            .unwrap();
        assert_eq!(ev.stage, "", "认不出的行不该带阶段");
        assert_eq!(ev.detail, "", "认不出的行不该带网速");
        assert!(!ev.raw.is_empty(), "但原文一定要带回来");
    }

    /// 进度刷新行不进日志（每秒十几条会冲掉真正的记录），
    /// **到 100% 那一条要进**——它是"这一步做完了"。
    #[test]
    fn only_finished_progress_lines_go_to_the_log() {
        let mid = parse_progress("Receiving objects:  45% (123/273), 1.2 MiB").unwrap();
        assert!(!mid.to_log, "45% 这种刷新行不该进日志");
        let done = parse_progress("Receiving objects: 100% (273/273), 6.00 MiB | 2.3 MiB/s, done.").unwrap();
        assert!(done.to_log, "完成那一条要进日志");
        // 没有百分比的行（From / Fast-forward）都是一次性事件
        assert!(parse_progress("From github.com:x/y").unwrap().to_log);
        assert!(parse_progress("Fast-forward").unwrap().to_log);
    }

    /// 认不出的行**不再返回 None**，只带原文上屏。
    /// 原断言是 `fatal: …` 应为 `None`——那正是被静默吞掉的那一类。
    #[test]
    fn unrecognised_lines_carry_their_raw_text_instead_of_vanishing() {
        let ev = parse_progress("fatal: unable to access 'https://…'").unwrap();
        assert_eq!(ev.raw, "fatal: unable to access 'https://…'");
        assert!(ev.stage.is_empty(), "认不出阶段就别编一个");
        assert!(ev.detail.is_empty());
        assert!(ev.to_log, "报错行一定要上屏");
        // 只有纯空行才真的没有内容
        assert!(parse_progress("   ").is_none());
    }

    /// 真实 `git clone --progress` 的输出形状（2026-10-05 实测，共 104 行，
    /// 从本地 bare 仓库 `file://` 浅克隆抓的 stderr）。
    ///
    /// 钉它是因为**整段输出里近 55 行带 ` remote: ` 前缀**，
    /// 而旧白名单是 `strip_prefix` 从行首匹配的——那 55 行一条都认不出来，
    /// 全被 `if let Some` 静默丢掉。行尾那些尾随空格也是真的
    /// （git 用它们覆盖同一行），所以顺带钉住 trim。
    #[test]
    fn real_clone_output_shape_is_fully_recognised() {
        let real = [
            "Cloning into 'C:\\tmp\\x'...",
            "remote: Enumerating objects: 47, done.        ",
            "remote: Counting objects:   2% (1/47)        ",
            "remote: Counting objects: 100% (47/47)        ",
            "remote: Counting objects: 100% (47/47), done.        ",
            "remote: Compressing objects:  25% (1/4)        ",
            "remote: Compressing objects: 100% (4/4), done.        ",
            "remote: Total 47 (delta 0), reused 0 (delta 0), pack-reused 0 (from 0)        ",
            "Receiving objects:   2% (1/47)",
            "Receiving objects: 100% (47/47), done.",
        ];
        for line in real {
            let ev = parse_progress(line)
                .unwrap_or_else(|| panic!("这行被丢了：{line:?}"));
            assert_eq!(ev.raw, line.trim(), "raw 要带原文且去掉尾随空格");
        }
        // **里程碑必须进日志**：完成那几条 + 无百分比的统计行
        for line in [
            "Cloning into 'C:\\tmp\\x'...",
            "remote: Enumerating objects: 47, done.",
            "remote: Counting objects: 100% (47/47), done.",
            "remote: Compressing objects: 100% (4/4), done.",
            "remote: Total 47 (delta 0), reused 0 (delta 0), pack-reused 0 (from 0)",
            "Receiving objects: 100% (47/47), done.",
        ] {
            assert!(
                parse_progress(line).unwrap().to_log,
                "这一步做完了，必须上日志：{line}"
            );
        }
        // **中途的刷新行不进日志**（否则 104 行会把真正的记录冲掉）
        for line in [
            "remote: Counting objects:   2% (1/47)",
            "remote: Compressing objects:  25% (1/4)",
            "Receiving objects:   2% (1/47)",
        ] {
            assert!(
                !parse_progress(line).unwrap().to_log,
                "刷新行不该进日志：{line}"
            );
        }
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
