use crate::{git, proxy, store};
use serde::Serialize;
use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::Manager;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const LOCAL_TIMEOUT: Duration = Duration::from_secs(10);
const NET_TIMEOUT: Duration = Duration::from_secs(300);
/// 检查云端更新只做 fetch + 对比，超时比完整拉取短得多
const CHECK_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Serialize, Clone, Default)]
pub struct RepoStatus {
    pub is_repo: bool,
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub has_upstream: bool,
    pub remote_url: String,
}

/// 拉取/克隆时推送给前端的进度事件
#[derive(Serialize, Clone)]
pub struct ProgressEvent {
    pub stage: String,
    pub detail: String,
    pub percent: Option<f32>,
}

/// 拉取成功后的更新内容：新拉下来的提交与文件变更摘要
#[derive(Serialize, Clone)]
pub struct UpdateChanges {
    pub commits: Vec<String>,
    pub diff_stat: String,
}

#[derive(Serialize, Clone)]
pub struct OpResult {
    pub ok: bool,
    /// 面向用户的一句话结论
    pub message: String,
    /// git 原始输出（日志区展示）
    pub output: String,
    /// 走代理成功时记录所用代理，None 表示直连
    pub used_proxy: Option<String>,
    pub status: Option<RepoStatus>,
    /// pull 时本地有未提交变更，提示用户可选择 stash
    pub need_stash: bool,
    /// 拉取成功时的更新内容（新提交 + 文件变更摘要）
    pub changes: Option<UpdateChanges>,
}

impl OpResult {
    fn err(message: String, output: String) -> Self {
        OpResult {
            ok: false,
            message,
            output,
            used_proxy: None,
            status: None,
            need_stash: false,
            changes: None,
        }
    }
}

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
struct RouteTag(std::sync::Arc<std::sync::Mutex<String>>);

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
/// stage 在直连/代理切换瞬间发出提示并同步路径标记
fn progress_hooks(
    on_progress: &Channel<ProgressEvent>,
) -> (
    RouteTag,
    impl Fn(&str) + '_,
    impl Fn(&str) + '_,
) {
    let tag = RouteTag::default();
    let t = tag.clone();
    let forward = move |line: &str| {
        if let Some(mut ev) = parse_progress(line) {
            let r = t.get();
            if !r.is_empty() {
                ev.stage = format!("{}（{}）", ev.stage, r);
            }
            let _ = on_progress.send(ev);
        }
    };
    let t2 = tag.clone();
    let stage = move |s: &str| {
        t2.set(&route_of(s));
        let _ = on_progress.send(ProgressEvent {
            stage: s.into(),
            detail: String::new(),
            percent: None,
        });
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

fn valid_repo(path: &str) -> bool {
    std::path::Path::new(path).join(".git").exists()
}

fn repo_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
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
fn build_net_auth(cfg: &store::AppConfig, remote_url: &str) -> (Vec<String>, Option<String>) {
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
fn remote_url(repo_path: &str) -> String {
    run_local(repo_path, &["remote", "get-url", "origin"])
        .stdout
        .trim()
        .to_string()
}

/// 提交时使用的 Git 身份（-c 项；设置里未填则交给全局配置）
fn identity_config(cfg: &store::AppConfig) -> Vec<String> {
    let mut c = Vec::new();
    if !cfg.settings.git_name.trim().is_empty() {
        c.push(format!("user.name={}", cfg.settings.git_name.trim()));
    }
    if !cfg.settings.git_email.trim().is_empty() {
        c.push(format!("user.email={}", cfg.settings.git_email.trim()));
    }
    c
}

/// 执行本地 git 命令（不涉及网络，不走代理回退）
fn run_local(repo_path: &str, args: &[&str]) -> git::GitOutput {
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
enum NetMode {
    Auto,
    DirectOnly,
    ProxyOnly,
}

impl NetMode {
    fn parse(s: &str) -> Self {
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
fn run_net_op(
    app: &tauri::AppHandle,
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
    let cfg = store::load(&app);
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
                    let mut cfg = store::load(&app);
                    cfg.last_good_proxy = Some(last_good);
                    let _ = store::save(&app, &cfg);
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
            let mut cfg = store::load(&app);
            cfg.last_good_proxy = Some(candidate);
            let _ = store::save(&app, &cfg);
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

fn read_status(repo_path: &str) -> RepoStatus {
    let mut st = RepoStatus {
        is_repo: valid_repo(repo_path),
        ..Default::default()
    };
    if !st.is_repo {
        return st;
    }
    st.branch = run_local(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .stdout
        .trim()
        .to_string();

    let upstream = run_local(
        repo_path,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    );
    st.has_upstream = upstream.success;
    if upstream.success {
        let counts = run_local(
            repo_path,
            &["rev-list", "--left-right", "--count", "HEAD...@{u}"],
        );
        if counts.success {
            let line = counts.stdout.trim().to_string();
            if let [ahead, behind] = line.split_whitespace().collect::<Vec<_>>()[..] {
                st.ahead = ahead.parse().unwrap_or(0);
                st.behind = behind.parse().unwrap_or(0);
            }
        }
    }
    st.dirty = !run_local(repo_path, &["status", "--porcelain"])
        .stdout
        .trim()
        .is_empty();
    st.remote_url = run_local(repo_path, &["remote", "get-url", "origin"])
        .stdout
        .trim()
        .to_string();
    st
}

fn touch_repo(app: &tauri::AppHandle, path: &str) {
    let mut cfg = store::load(&app);
    if let Some(entry) = cfg.repos.iter_mut().find(|r| r.path == path) {
        entry.last_used = store::now_secs();
    }
    let _ = store::save(&app, &cfg);
}

// ---------- 仓库管理 ----------

#[tauri::command]
pub fn list_repos(app: tauri::AppHandle) -> Vec<store::RepoEntry> {
    let cfg = store::load(&app);
    cfg.repos
}

#[tauri::command]
pub fn add_repo(app: tauri::AppHandle, path: String) -> Result<Vec<store::RepoEntry>, String> {
    let path = path.trim().to_string();
    if !valid_repo(&path) {
        return Err("所选文件夹不是 git 仓库（未找到 .git）".to_string());
    }
    let mut cfg = store::load(&app);
    if !cfg.repos.iter().any(|r| r.path == path) {
        cfg.repos.push(store::RepoEntry {
            name: repo_name(&path),
            alias: String::new(),
            path: path.clone(),
            added_at: store::now_secs(),
            last_used: store::now_secs(),
            test_cmd: String::new(),
            build_cmd: String::new(),
        });
    }
    store::save(&app, &cfg)?;
    Ok(cfg.repos)
}

#[tauri::command]
pub fn remove_repo(app: tauri::AppHandle, path: String) -> Vec<store::RepoEntry> {
    let mut cfg = store::load(&app);
    cfg.repos.retain(|r| r.path != path);
    let _ = store::save(&app, &cfg);
    cfg.repos
}

/// 设置本地自定义显示名（如中文名）；alias 传空串恢复原始仓库名
#[tauri::command]
pub fn rename_repo(app: tauri::AppHandle, path: String, alias: String) -> Vec<store::RepoEntry> {
    let mut cfg = store::load(&app);
    if let Some(e) = cfg.repos.iter_mut().find(|r| r.path == path) {
        e.alias = alias.trim().to_string();
    }
    let _ = store::save(&app, &cfg);
    cfg.repos
}

// ---------- 初始化已有文件夹 ----------

#[derive(Serialize, Clone)]
pub struct DirItem {
    pub name: String,
    pub is_dir: bool,
}

/// 列出文件夹顶层内容（供初始化时勾选排除项）
#[tauri::command]
pub fn list_dir_items(path: String) -> Vec<DirItem> {
    let mut items: Vec<DirItem> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&path) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name == ".git" {
                continue;
            }
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            items.push(DirItem { name, is_dir });
        }
    }
    items.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    items
}

/// 把非 git 文件夹初始化为仓库：git init → 写入排除项到 .gitignore → 初始提交 → 加入管理列表
#[tauri::command]
pub fn init_repo(
    app: tauri::AppHandle,
    path: String,
    excludes: Vec<String>,
) -> Result<store::RepoEntry, String> {
    let path = path.trim().to_string();
    let dir = std::path::Path::new(&path);
    if !dir.is_dir() {
        return Err("文件夹不存在".into());
    }
    if valid_repo(&path) {
        return Err("该文件夹已经是 git 仓库".into());
    }

    let init = git::run_git_auth(
        &path,
        &["init", "-b", "main"],
        None,
        LOCAL_TIMEOUT,
        None,
        &[],
        None,
    );
    if !init.success {
        let fallback = git::run_git_auth(&path, &["init"], None, LOCAL_TIMEOUT, None, &[], None);
        if !fallback.success {
            return Err(format!("git init 失败：{}", fallback.stderr.trim()));
        }
    }

    // 把勾选的排除项追加到 .gitignore
    if !excludes.is_empty() {
        let gi = dir.join(".gitignore");
        let mut content = String::new();
        if gi.exists() {
            content = std::fs::read_to_string(&gi).unwrap_or_default();
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
        }
        content.push_str("# 由青鸟 Git 添加的排除项\n");
        for e in &excludes {
            content.push_str(e.trim());
            content.push('\n');
        }
        std::fs::write(&gi, content).map_err(|e| format!("写入 .gitignore 失败：{e}"))?;
    }

    let cfg0 = store::load(&app);
    let identity = identity_config(&cfg0);
    let mut argv: Vec<&str> = identity.iter().map(|s| s.as_str()).collect();
    argv.extend(["add", "-A"]);
    run_local(&path, &argv);

    let mut cargv: Vec<&str> = identity.iter().map(|s| s.as_str()).collect();
    cargv.extend(["commit", "-m", "init: 初始提交"]);
    let commit = run_local(&path, &cargv);
    if !commit.success {
        return Err(format!(
            "仓库已初始化，但初始提交失败（可能未配置 Git 身份，可在设置里填写）：{}",
            commit.stderr.trim()
        ));
    }

    let name = repo_name(&path);
    let mut cfg = store::load(&app);
    let entry = store::RepoEntry {
        name,
        path: path.clone(),
        alias: String::new(),
        added_at: store::now_secs(),
        last_used: store::now_secs(),
        test_cmd: String::new(),
        build_cmd: String::new(),
    };
    if !cfg.repos.iter().any(|r| r.path == path) {
        cfg.repos.push(entry.clone());
        store::save(&app, &cfg)?;
    }
    Ok(entry)
}

// ---------- 状态与三键操作 ----------

#[tauri::command]
pub fn repo_status(path: String) -> RepoStatus {
    read_status(&path)
}

/// 请求取消当前正在进行的网络操作（克隆/检测更新/拉取/推送），
/// 运行中的 git 子进程会被立即终止
#[tauri::command]
pub fn cancel_busy_op() {
    git::request_cancel();
}

#[tauri::command(async)]
pub fn check_update(
    app: tauri::AppHandle,
    path: String,
    on_progress: Channel<ProgressEvent>,
) -> OpResult {
    if !valid_repo(&path) {
        return OpResult::err("该文件夹不是 git 仓库".into(), String::new());
    }
    let _ = on_progress.send(ProgressEvent {
        stage: "正在连接远端…".into(),
        detail: String::new(),
        percent: None,
    });
    let (_tag, forward, stage) = progress_hooks(&on_progress);
    let (fetch_out, used_proxy) = run_net_op(
        &app,
        &path,
        &["fetch", "--all", "--prune", "--progress"],
        Some(&forward),
        None,
        CHECK_TIMEOUT,
        true,
        Some(&stage),
        NetMode::Auto,
    );
    let status = read_status(&path);
    touch_repo(&app, &path);

    if !fetch_out.success {
        let mut msg = "检查更新失败：无法连接远端".to_string();
        if used_proxy.is_none() && git::is_network_error(&fetch_out) {
            msg.push_str("，且未找到可用代理");
        }
        return OpResult {
            ok: false,
            message: msg,
            output: combined(&fetch_out),
            used_proxy,
            status: Some(status),
            need_stash: false,
            changes: None,
        };
    }

    let message = if !status.has_upstream {
        "当前分支没有设置上游分支，无法对比云端更新".to_string()
    } else if status.behind > 0 && status.ahead > 0 {
        format!(
            "云端有 {0} 个新提交，同时本地有 {1} 个未推送提交",
            status.behind, status.ahead
        )
    } else if status.behind > 0 {
        format!("云端有 {} 个新提交，可以拉取", status.behind)
    } else if status.ahead > 0 {
        format!("已是最新，本地领先 {} 个提交待推送", status.ahead)
    } else {
        "已是最新，无需同步".to_string()
    };
    OpResult {
        ok: true,
        message,
        output: combined(&fetch_out),
        used_proxy,
        status: Some(status),
        need_stash: false,
        changes: None,
    }
}

#[tauri::command(async)]
pub fn pull_repo(
    app: tauri::AppHandle,
    path: String,
    stash: bool,
    on_progress: Channel<ProgressEvent>,
) -> OpResult {
    if !valid_repo(&path) {
        return OpResult::err("该文件夹不是 git 仓库".into(), String::new());
    }
    let (_tag, forward, forward_stage) = progress_hooks(&on_progress);
    let status_before = read_status(&path);
    if status_before.dirty && !stash {
        return OpResult {
            ok: false,
            message: "本地有未提交的变更，直接拉取可能冲突".to_string(),
            output: String::new(),
            used_proxy: None,
            status: Some(status_before),
            need_stash: true,
            changes: None,
        };
    }

    let old_head = run_local(&path, &["rev-parse", "HEAD"])
        .stdout
        .trim()
        .to_string();

    let mut log = String::new();
    let mut stashed = false;
    if stash && status_before.dirty {
        let s = run_local(
            &path,
            &["stash", "push", "-u", "-m", "qingbird-git auto stash"],
        );
        log.push_str(&combined(&s));
        if !s.success {
            return OpResult::err("暂存本地变更失败，已取消拉取".into(), log);
        }
        stashed = true;
    }

    let (pull_out, used_proxy) = run_net_op(
        &app,
        &path,
        &["pull", "--ff-only", "--progress"],
        Some(&forward),
        None,
        NET_TIMEOUT,
        true,
        Some(&forward_stage),
        NetMode::Auto,
    );
    log.push_str(&combined(&pull_out));
    let mut status = read_status(&path);
    touch_repo(&app, &path);

    if stashed {
        let pop = run_local(&path, &["stash", "pop"]);
        log.push_str(&combined(&pop));
        if !pop.success {
            return OpResult {
                ok: false,
                message: "拉取完成，但恢复暂存变更时冲突，请在终端执行 git stash pop 处理".into(),
                output: log,
                used_proxy: used_proxy.clone(),
                status: Some(status),
                need_stash: false,
                changes: None,
            };
        }
    }

    if !pull_out.success {
        let message = if pull_out.stderr.contains("no tracking information") {
            "当前分支没有上游分支，无法拉取".to_string()
        } else if pull_out.stderr.contains("Not possible to fast-forward")
            || pull_out.stderr.contains("divergent")
        {
            "本地与云端历史分叉，ff 拉取失败，请手动处理".to_string()
        } else if used_proxy.is_none() && git::is_network_error(&pull_out) {
            "拉取失败：无法连接远端，且未找到可用代理".to_string()
        } else {
            "拉取失败".to_string()
        };
        return OpResult {
            ok: false,
            message,
            output: log,
            used_proxy,
            status: Some(status),
            need_stash: false,
            changes: None,
        };
    }

    status = read_status(&path);
    let changes = pull_changes(&path, &old_head);
    let message = if used_proxy.is_some() {
        "拉取成功（已自动通过代理完成）".to_string()
    } else {
        "拉取成功".to_string()
    };
    OpResult {
        ok: true,
        message,
        output: log,
        used_proxy,
        status: Some(status),
        need_stash: false,
        changes,
    }
}

/// 对比拉取前的 HEAD，提取本次拉下来的提交列表与文件变更摘要
fn pull_changes(path: &str, old_head: &str) -> Option<UpdateChanges> {
    if old_head.is_empty() {
        return None;
    }
    let range = format!("{old_head}..HEAD");
    let mut changes = UpdateChanges {
        commits: Vec::new(),
        diff_stat: String::new(),
    };

    let log = run_local(path, &["log", "--oneline", "--no-decorate", &range]);
    if log.success {
        changes.commits = log
            .stdout
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
    }
    let stat = run_local(path, &["diff", "--stat", &range]);
    if stat.success {
        changes.diff_stat = stat.stdout.trim_end().to_string();
    }
    // 完全没有新内容（Already up to date）时不返回更新卡片
    if changes.commits.is_empty() && changes.diff_stat.is_empty() {
        return None;
    }
    Some(changes)
}

#[tauri::command(async)]
pub fn push_repo(app: tauri::AppHandle, path: String) -> OpResult {
    if !valid_repo(&path) {
        return OpResult::err("该文件夹不是 git 仓库".into(), String::new());
    }
    do_push(&app, &path)
}

/// 推送的核心流程（push_repo 与 commit_push 共用）
fn do_push(app: &tauri::AppHandle, path: &str) -> OpResult {
    let status = read_status(path);
    if status.ahead == 0 && status.has_upstream {
        return OpResult {
            ok: true,
            message: "没有待推送的提交".to_string(),
            output: String::new(),
            used_proxy: None,
            status: Some(status),
            need_stash: false,
            changes: None,
        };
    }
    if !status.has_upstream {
        let remotes = run_local(path, &["remote"]);
        if remotes.stdout.trim().is_empty() {
            return OpResult::err(
                "该仓库没有配置远端仓库，无法推送".into(),
                combined(&remotes),
            );
        }
    }

    let args: Vec<&str> = if status.has_upstream {
        vec!["push", "--progress"]
    } else {
        vec!["push", "--progress", "--set-upstream", "origin", "HEAD"]
    };
    let (push_out, used_proxy) = run_net_op(app, path, &args, None, None, NET_TIMEOUT, true, None, NetMode::Auto);
    let status = read_status(path);
    touch_repo(app, path);

    if !push_out.success {
        let message =
            if push_out.stderr.contains("fetch first") || push_out.stderr.contains("rejected") {
                "推送被拒绝：云端有新提交，请先拉取".to_string()
            } else if used_proxy.is_none() && git::is_network_error(&push_out) {
                "推送失败：无法连接远端，且未找到可用代理".to_string()
            } else if push_out.stderr.contains("could not read Username")
                || push_out.stderr.contains("Authentication failed")
            {
                "推送失败：认证未通过，可在设置里填写对应平台的访问令牌".to_string()
            } else {
                "推送失败".to_string()
            };
        return OpResult {
            ok: false,
            message,
            output: combined(&push_out),
            used_proxy,
            status: Some(status),
            need_stash: false,
            changes: None,
        };
    }

    let message = if used_proxy.is_some() {
        "推送成功（已自动通过代理完成）".to_string()
    } else {
        "推送成功".to_string()
    };
    OpResult {
        ok: true,
        message,
        output: combined(&push_out),
        used_proxy,
        status: Some(status),
        need_stash: false,
        changes: None,
    }
}

// ---------- 提交 ----------

#[derive(Serialize, Clone)]
pub struct FileChange {
    /// git status 代码：M 修改 / A 新增 / D 删除 / ?? 未跟踪 / R 重命名 等
    pub code: String,
    pub path: String,
}

#[tauri::command]
pub fn list_changes(path: String) -> Vec<FileChange> {
    if !valid_repo(&path) {
        return Vec::new();
    }
    run_local(&path, &["status", "--porcelain"])
        .stdout
        .lines()
        .filter(|l| l.len() >= 3)
        .map(|l| FileChange {
            code: l[..2].trim().to_string(),
            path: l[3..].trim().to_string(),
        })
        .collect()
}

#[derive(Serialize, Clone)]
pub struct CommitInfo {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub summary: String,
}

#[tauri::command]
pub fn recent_commits(path: String) -> Vec<CommitInfo> {
    if !valid_repo(&path) {
        return Vec::new();
    }
    run_local(
        &path,
        &[
            "log",
            "-n",
            "20",
            "--date=format-local:%Y-%m-%d %H:%M",
            "--pretty=format:%h%x09%an%x09%ad%x09%s",
        ],
    )
    .stdout
    .lines()
    .filter_map(|l| {
        let mut parts = l.splitn(4, '\t');
        Some(CommitInfo {
            hash: parts.next()?.to_string(),
            author: parts.next()?.to_string(),
            date: parts.next()?.to_string(),
            summary: parts.next().unwrap_or("").to_string(),
        })
    })
    .collect()
}

/// 项目语言构成（类似 GitHub 语言条）：按扩展名统计代码文件字节占比。
/// 用 `git ls-files -co --exclude-standard` 取文件列表，自动跳过 .gitignore 忽略的文件。
#[derive(Serialize, Clone)]
pub struct LangStat {
    pub language: String,
    pub percent: f32,
}

/// 扩展名 → 语言名（对齐 GitHub Linguist 的常见分类；配置/数据类文件不计入）
fn lang_of_ext(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "rs" => "Rust",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "rb" => "Ruby",
        "php" => "PHP",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "C++",
        "cs" => "C#",
        "m" | "mm" => "Objective-C",
        "scala" => "Scala",
        "dart" => "Dart",
        "lua" => "Lua",
        "html" | "htm" => "HTML",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "css" => "CSS",
        "scss" | "sass" => "SCSS",
        "less" => "Less",
        "sh" | "bash" | "zsh" => "Shell",
        "ps1" | "psm1" => "PowerShell",
        "sql" => "SQL",
        "zig" => "Zig",
        "sol" => "Solidity",
        _ => return None,
    })
}

#[tauri::command]
pub fn repo_languages(path: String) -> Vec<LangStat> {
    if !valid_repo(&path) {
        return Vec::new();
    }
    let ls = run_local(&path, &["ls-files", "-co", "--exclude-standard"]);
    if !ls.success {
        return Vec::new();
    }
    let mut totals: HashMap<&str, u64> = HashMap::new();
    let mut total: u64 = 0;
    for f in ls.stdout.lines() {
        let ext = std::path::Path::new(f)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        let Some(lang) = lang_of_ext(&ext) else {
            continue;
        };
        let len = std::fs::metadata(std::path::Path::new(&path).join(f))
            .map(|m| m.len())
            .unwrap_or(0);
        *totals.entry(lang).or_insert(0) += len;
        total += len;
    }
    if total == 0 {
        return Vec::new();
    }
    let mut stats: Vec<LangStat> = totals
        .into_iter()
        .map(|(lang, bytes)| LangStat {
            language: lang.to_string(),
            percent: (bytes as f32) / (total as f32) * 100.0,
        })
        .collect();
    stats.sort_by(|a, b| b.percent.total_cmp(&a.percent));
    // 只保留前 6 项，其余合并为"其他"
    if stats.len() > 6 {
        let rest: f32 = stats[6..].iter().map(|s| s.percent).sum();
        stats.truncate(6);
        stats.push(LangStat {
            language: "其他".into(),
            percent: rest,
        });
    }
    stats
}

/// 一键提交并推送：add -A → commit（自动带 Git 身份）→ push
#[tauri::command(async)]
pub fn commit_push(app: tauri::AppHandle, path: String, message: String) -> OpResult {
    if !valid_repo(&path) {
        return OpResult::err("该文件夹不是 git 仓库".into(), String::new());
    }
    let msg = message.trim();
    if msg.is_empty() {
        return OpResult::err("请先填写提交说明".into(), String::new());
    }
    let st = read_status(&path);
    if !st.dirty {
        return OpResult {
            ok: true,
            message: "没有可提交的变更，直接推送即可".to_string(),
            output: String::new(),
            used_proxy: None,
            status: Some(st),
            need_stash: false,
            changes: None,
        };
    }

    let cfg = store::load(&app);
    let add = run_local(&path, &["add", "-A"]);
    if !add.success {
        return OpResult::err("添加变更文件失败".into(), combined(&add));
    }

    let cargs = identity_config(&cfg);
    let mut argv: Vec<&str> = cargs.iter().map(|s| s.as_str()).collect();
    argv.extend(["commit", "-m", msg]);
    let commit = run_local(&path, &argv);
    if !commit.success {
        let message = if commit.stderr.contains("Please tell me who you are")
            || commit.stderr.contains("unable to auto-detect email")
        {
            "提交失败：还没有配置 Git 身份，请在「设置 → Git 身份」里填写姓名和邮箱".to_string()
        } else {
            "提交失败".to_string()
        };
        return OpResult::err(message, combined(&commit));
    }

    let mut result = do_push(&app, &path);
    result.output = format!("{}{}", combined(&commit), result.output);
    if result.ok {
        result.message = format!("已提交并推送：「{msg}」");
    }
    result
}

// ---------- 新建仓库 ----------

#[derive(Serialize, Clone)]
pub struct CreateOutcome {
    pub path: String,
    pub name: String,
    pub repos: Vec<store::RepoEntry>,
    /// 面向用户的结果说明（可能包含部分成功信息）
    pub message: String,
}

fn valid_repo_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
        && !name.starts_with('.')
}

/// 在 GitHub 上创建远端仓库，返回 full_name（owner/repo）
fn api_create_github(
    token: &str,
    name: &str,
    private: bool,
    description: &str,
) -> Result<String, String> {
    let mut body = ureq::json!({ "name": name, "private": private, "auto_init": false });
    if !description.is_empty() {
        body["description"] = ureq::json!(description);
    }
    match ureq::post("https://api.github.com/user/repos")
        .set("Authorization", &format!("Bearer {token}"))
        .set("User-Agent", "qingbird-git")
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(30))
        .send_json(body)
    {
        Ok(resp) => {
            let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
            v["full_name"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "GitHub 返回数据异常".into())
        }
        Err(ureq::Error::Status(422, _)) => {
            Err("GitHub 上已存在同名仓库（或名称不合法），换个名字试试".into())
        }
        Err(ureq::Error::Status(401, _)) => Err("GitHub 令牌无效或已过期，请在设置里检查".into()),
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            Err(format!(
                "GitHub 创建仓库失败（HTTP {code}）：{}",
                body.chars().take(200).collect::<String>()
            ))
        }
        Err(e) => Err(format!("无法连接 GitHub API：{e}")),
    }
}

/// 在 Gitee 上创建远端仓库，返回 full_name（owner/repo）
fn api_create_gitee(
    token: &str,
    name: &str,
    private: bool,
    description: &str,
) -> Result<String, String> {
    let mut form: Vec<(&str, &str)> = vec![
        ("access_token", token),
        ("name", name),
        ("private", if private { "true" } else { "false" }),
        ("auto_init", "false"),
    ];
    if !description.is_empty() {
        form.push(("description", description));
    }
    match ureq::post("https://gitee.com/api/v5/user/repos")
        .set("User-Agent", "qingbird-git")
        .timeout(Duration::from_secs(30))
        .send_form(&form)
    {
        Ok(resp) => {
            let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
            v["full_name"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "Gitee 返回数据异常".into())
        }
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            let hint = if code == 401 || body.contains("access_token") {
                "Gitee 令牌无效或已过期，请在设置里检查"
            } else if body.contains("exist") || body.contains("taken") {
                "Gitee 上已存在同名仓库（或名称不合法），换个名字试试"
            } else {
                "Gitee 创建仓库失败"
            };
            Err(format!("{hint}（HTTP {code}）"))
        }
        Err(e) => Err(format!("无法连接 Gitee API：{e}")),
    }
}

/// 一步创建新仓库：本地 init + 初始提交，可选同时在 GitHub/Gitee 创建远端并推送
#[tauri::command(async)]
pub fn create_repo(
    app: tauri::AppHandle,
    name: String,
    parent_dir: String,
    platform: String,
    private: bool,
    description: String,
) -> Result<CreateOutcome, String> {
    let name = name.trim().to_string();
    if !valid_repo_name(&name) {
        return Err("仓库名只能包含字母、数字、点、横线和下划线".into());
    }
    let parent = std::path::Path::new(&parent_dir);
    if !parent.is_dir() {
        return Err("目标文件夹不存在".into());
    }
    let dest = parent.join(&name);
    if dest.exists() {
        return Err(format!("目标文件夹已存在：{}", dest.display()));
    }

    let cfg = store::load(&app);
    let desc = description.trim();

    // 1. 先在云端创建远端（失败则整体中止，不留下半成品）
    let mut origin = String::new();
    if platform != "none" {
        let remote_full_name = match platform.as_str() {
            "github" => {
                let token = cfg.settings.github_token.trim();
                if token.is_empty() {
                    return Err("尚未配置 GitHub 访问令牌，请在「设置 → 令牌认证」里填写".into());
                }
                api_create_github(token, &name, private, desc)?
            }
            "gitee" => {
                let token = cfg.settings.gitee_token.trim();
                if token.is_empty() {
                    return Err("尚未配置 Gitee 私人令牌，请在「设置 → 令牌认证」里填写".into());
                }
                api_create_gitee(token, &name, private, desc)?
            }
            _ => return Err("不支持的平台".into()),
        };
        let host = if platform == "github" {
            "https://github.com"
        } else {
            "https://gitee.com"
        };
        origin = format!("{host}/{remote_full_name}.git");
    }

    // 2. 本地初始化 + 初始提交
    let init = git::run_git_auth(
        parent_dir.trim(),
        &["init", "-b", "main", &name],
        None,
        LOCAL_TIMEOUT,
        None,
        &[],
        None,
    );
    if !init.success {
        // 兼容不支持 -b 的旧版 git
        let fallback = git::run_git_auth(
            parent_dir.trim(),
            &["init", &name],
            None,
            LOCAL_TIMEOUT,
            None,
            &[],
            None,
        );
        if !fallback.success {
            return Err(format!("git init 失败：{}", fallback.stderr.trim()));
        }
    }
    std::fs::write(
        dest.join("README.md"),
        format!(
            "# {name}\n\n{}\n",
            if desc.is_empty() { name.as_str() } else { desc }
        ),
    )
    .map_err(|e| format!("写入 README 失败：{e}"))?;

    let dest_str = dest.to_string_lossy().to_string();
    let cargs = identity_config(&cfg);
    let mut argv: Vec<&str> = cargs.iter().map(|s| s.as_str()).collect();
    argv.extend(["add", "-A"]);
    run_local(&dest_str, &argv);
    let mut cargv: Vec<&str> = cargs.iter().map(|s| s.as_str()).collect();
    cargv.extend(["commit", "-m", "init: 初始提交"]);
    let commit = run_local(&dest_str, &cargv);
    if !commit.success {
        return Err(format!(
            "仓库已创建，但初始提交失败（可能未配置 Git 身份，可在设置里填写）：{}",
            commit.stderr.trim()
        ));
    }

    // 3. 关联远端并推送
    let mut message = "本地仓库创建成功".to_string();
    if !origin.is_empty() {
        let add = run_local(&dest_str, &["remote", "add", "origin", &origin]);
        if !add.success {
            return Err(format!("仓库已创建，但关联远端失败：{}", add.stderr.trim()));
        }
        let push_args: Vec<&str> = vec!["push", "--progress", "--set-upstream", "origin", "main"];
        let (push_out, used_proxy) = run_net_op(
            &app,
            &dest_str,
            &push_args,
            None,
            Some(&origin),
            NET_TIMEOUT,
            true,
            None,
            NetMode::Auto,
        );
        if !push_out.success {
            message.push_str("，远端已创建，但首次推送失败：");
            message.push_str(push_out.stderr.trim());
        } else if used_proxy.is_some() {
            message.push_str("，远端已创建并完成首次推送（经代理）");
        } else {
            message.push_str("，远端已创建并完成首次推送");
        }
    }

    // 4. 加入管理列表
    let mut cfg = store::load(&app);
    if !cfg.repos.iter().any(|r| r.path == dest_str) {
        cfg.repos.push(store::RepoEntry {
            name: name.clone(),
            alias: String::new(),
            path: dest_str.clone(),
            added_at: store::now_secs(),
            last_used: store::now_secs(),
            test_cmd: String::new(),
            build_cmd: String::new(),
        });
    }
    store::save(&app, &cfg)?;
    Ok(CreateOutcome {
        path: dest_str,
        name,
        repos: cfg.repos,
        message,
    })
}

// ---------- 设置 ----------

#[derive(Serialize, Clone)]
pub struct AppSettings {
    pub manual_proxy: String,
    pub last_good_proxy: Option<String>,
    pub theme: String,
}

#[tauri::command]
pub fn get_settings(app: tauri::AppHandle) -> AppSettings {
    let cfg = store::load(&app);
    AppSettings {
        manual_proxy: cfg.settings.manual_proxy,
        last_good_proxy: cfg.last_good_proxy,
        theme: cfg.settings.theme,
    }
}

#[tauri::command]
pub fn set_theme(app: tauri::AppHandle, theme: String) -> Result<(), String> {
    let t = match theme.as_str() {
        "dark" => Some(tauri::Theme::Dark),
        "light" => Some(tauri::Theme::Light),
        _ => None,
    };
    let mut cfg = store::load(&app);
    cfg.settings.theme = theme;
    store::save(&app, &cfg)?;
    if let Some(win) = app.get_webview_window("main") {
        win.set_theme(t).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------- 凭证与身份 ----------

#[derive(Serialize, Clone)]
pub struct AppCredentials {
    pub git_name: String,
    pub git_email: String,
    pub github_token: String,
    pub gitee_token: String,
    pub ssh_key_path: String,
}

#[tauri::command]
pub fn get_credentials(app: tauri::AppHandle) -> AppCredentials {
    let s = store::load(&app).settings;
    AppCredentials {
        git_name: s.git_name,
        git_email: s.git_email,
        github_token: s.github_token,
        gitee_token: s.gitee_token,
        ssh_key_path: s.ssh_key_path,
    }
}

#[tauri::command]
pub fn set_credentials(
    app: tauri::AppHandle,
    git_name: String,
    git_email: String,
    github_token: String,
    gitee_token: String,
    ssh_key_path: String,
) -> Result<(), String> {
    let mut cfg = store::load(&app);
    cfg.settings.git_name = git_name.trim().to_string();
    cfg.settings.git_email = git_email.trim().to_string();
    cfg.settings.github_token = github_token.trim().to_string();
    cfg.settings.gitee_token = gitee_token.trim().to_string();
    cfg.settings.ssh_key_path = ssh_key_path.trim().to_string();
    store::save(&app, &cfg)
}

// ---------- 快捷打开 ----------

#[tauri::command]
pub fn open_in_folder(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).is_dir() {
        return Err("文件夹不存在".into());
    }
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_in_terminal(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).is_dir() {
        return Err("文件夹不存在".into());
    }
    // 优先 Windows Terminal，失败回退 PowerShell
    if Command::new("wt").arg("-d").arg(&path).spawn().is_ok() {
        return Ok(());
    }
    Command::new("cmd")
        .args(["/C", "start", "powershell", "-NoExit", "-Command"])
        .arg(format!(
            "Set-Location -LiteralPath '{}'",
            path.replace('\'', "''")
        ))
        .spawn()
        .map_err(|e| format!("无法打开终端：{e}"))?;
    Ok(())
}

/// 打开仓库根目录的 README 说明文档（调用系统默认 Markdown 关联程序）
#[tauri::command]
pub fn open_readme(path: String) -> Result<(), String> {
    let root = std::path::Path::new(&path);
    if !root.is_dir() {
        return Err("仓库文件夹不存在".into());
    }
    // 常见命名都识别：大小写变体、.markdown 与无扩展名
    let candidates = [
        "README.md",
        "Readme.md",
        "readme.md",
        "README.markdown",
        "Readme.markdown",
        "README",
        "Readme",
        "readme",
    ];
    let readme = candidates
        .iter()
        .map(|n| root.join(n))
        .find(|p| p.is_file())
        .ok_or_else(|| "仓库根目录未找到 README.md 说明文档".to_string())?;
    tauri_plugin_opener::open_path(&readme, None::<&str>).map_err(|e| format!("打开 README 失败：{e}"))
}

#[tauri::command]
pub fn set_settings(app: tauri::AppHandle, manual_proxy: String) -> Result<(), String> {
    let mut cfg = store::load(&app);
    cfg.settings.manual_proxy = manual_proxy.trim().to_string();
    store::save(&app, &cfg)
}

#[tauri::command]
pub fn probe_proxy(addr: String) -> Result<bool, String> {
    let url = proxy::to_git_proxy_url(addr.trim());
    Ok(proxy::probe(&url))
}

fn combined(out: &git::GitOutput) -> String {
    // 进度刷新行（\r 重复输出）在进度条里已展示，日志里去掉避免刷屏
    const PROGRESS_NOISE: &[&str] = &[
        "Receiving objects",
        "Resolving deltas",
        "Updating files",
        "Counting objects",
        "Compressing objects",
        "Enumerating objects",
        "remote: Enumerating",
    ];
    let clean = |text: &str| -> String {
        text.split('\n')
            .filter(|line| {
                let l = line.trim();
                !l.is_empty() && !PROGRESS_NOISE.iter().any(|k| l.starts_with(k))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut s = String::new();
    let stdout = clean(&out.stdout);
    let stderr = clean(&out.stderr);
    if !stdout.is_empty() {
        s.push_str(&stdout);
        s.push('\n');
    }
    if !stderr.is_empty() {
        s.push_str(&stderr);
        s.push('\n');
    }
    s
}

// ---------- 克隆仓库 ----------

#[derive(Serialize, Clone)]
pub struct CloneOutcome {
    pub path: String,
    pub name: String,
    pub repos: Vec<store::RepoEntry>,
}

/// 从 git URL 提取仓库名（兼容 https/git/ssh 及 user@host:path 形式，去掉 .git 后缀）
pub fn repo_name_from_url(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let tail = if let Some(pos) = url.rfind(':').filter(|_| url.contains('@')) {
        // ssh 短格式 user@host:owner/repo.git
        &url[pos + 1..]
    } else {
        url.rsplit_once('/')?.1
    };
    let tail = tail.rsplit('/').next().unwrap_or(tail);
    let name = tail.strip_suffix(".git").unwrap_or(tail);
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-. ".contains(c));
    valid.then(|| name.to_string())
}

#[tauri::command(async)]
pub fn clone_repo(
    app: tauri::AppHandle,
    url: String,
    parent_dir: String,
    net_mode: Option<String>,
    on_progress: Channel<ProgressEvent>,
) -> Result<CloneOutcome, String> {
    let mode = NetMode::parse(net_mode.as_deref().unwrap_or("auto"));
    let url = url.trim().to_string();
    if !url.contains(':') && !url.contains('/') {
        return Err("仓库地址看起来不完整".into());
    }
    let name = repo_name_from_url(&url).ok_or_else(|| "无法从地址中解析出仓库名".to_string())?;
    let parent = std::path::Path::new(&parent_dir);
    if !parent.is_dir() {
        return Err("目标文件夹不存在".into());
    }
    let dest = parent.join(&name);
    if dest.exists() {
        return Err(format!("目标文件夹已存在：{}", dest.display()));
    }

    let (_tag, forward, stage) = progress_hooks(&on_progress);
    let args = ["clone", "--progress", &url, &name];
    let (out, used_proxy) = run_net_op(
        &app,
        parent_dir.trim(),
        &args,
        Some(&forward),
        Some(&url),
        NET_TIMEOUT,
        false,
        Some(&stage),
        mode,
    );
    if !out.success {
        // 进入前已确认目标目录不存在，失败后的残留半成品目录是本次尝试产生的，清掉以便重试
        let _ = std::fs::remove_dir_all(&dest);
        let message = if out.stderr.contains(git::CANCELLED_MSG) {
            "克隆已取消".to_string()
        } else if out.stderr.contains("未找到可用代理") {
            "克隆失败：代理模式未找到可用代理，请确认代理软件已运行，或在设置中手动填写代理地址".to_string()
        } else if used_proxy.is_none() && git::is_network_error(&out) {
            "克隆失败：无法连接远端，且未找到可用代理".to_string()
        } else if out.stderr.contains("could not read Username")
            || out.stderr.contains("Authentication failed")
        {
            "克隆失败：这是私有仓库，请确认已在本机 Git 凭证管理器中登录".to_string()
        } else if out.stderr.contains("Repository not found") || out.stderr.contains("not found") {
            "克隆失败：仓库不存在或无权访问".to_string()
        } else {
            "克隆失败".to_string()
        };
        return Err(format!("{message}\n{}", out.stderr.trim()));
    }

    let dest_str = dest.to_string_lossy().to_string();
    let mut cfg = store::load(&app);
    if !cfg.repos.iter().any(|r| r.path == dest_str) {
        cfg.repos.push(store::RepoEntry {
            name: name.clone(),
            alias: String::new(),
            path: dest_str.clone(),
            added_at: store::now_secs(),
            last_used: store::now_secs(),
            test_cmd: String::new(),
            build_cmd: String::new(),
        });
    }
    store::save(&app, &cfg)?;
    Ok(CloneOutcome {
        path: dest_str,
        name,
        repos: cfg.repos,
    })
}

// ---------- 一键扫描 ----------

#[derive(Serialize, Clone)]
pub struct ScanItem {
    pub path: String,
    pub status: RepoStatus,
}

#[tauri::command(async)]
pub fn scan_all(app: tauri::AppHandle) -> Vec<ScanItem> {
    let cfg = store::load(&app);
    cfg.repos
        .iter()
        .map(|r| ScanItem {
            path: r.path.clone(),
            status: read_status(&r.path),
        })
        .collect()
}

// ---------- 一键运行：测试 / 构建 ----------

/// 当前后台命令的 Job 句柄与代数编号；同一时间只保留一个，新命令会终止旧的。
/// 子进程本身留在运行线程里等待回收，全局只记 Job 句柄（Windows 无 Child::try_clone）。
struct CmdProc {
    job: isize,
    gen: u64,
}

static CMD_CHILD: std::sync::Mutex<Option<CmdProc>> = std::sync::Mutex::new(None);
static CMD_STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CMD_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 去除 ANSI 转义序列（颜色 / 光标控制等），终端彩色输出转成纯文本
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            // CSI 序列：ESC [ 参数... 终止字节（0x40–0x7E）
            chars.next();
            while let Some(&n) = chars.peek() {
                chars.next();
                if ('\x40'..='\x7e').contains(&n) {
                    break;
                }
            }
        } else if chars.peek() == Some(&']') {
            // OSC 序列（如 cargo 的超链接 ESC ]8;;URL ESC \）：以 BEL 或 ESC \ 结束
            chars.next();
            loop {
                match chars.next() {
                    None | Some('\x07') => break,
                    Some('\x1b') => {
                        chars.next();
                        break;
                    }
                    _ => {}
                }
            }
        } else if chars.peek().is_some() {
            // 其他两字符转义，跳过一个字符
            chars.next();
        }
    }
    out
}

/// 保存某仓库的一键运行命令（测试 / 构建），保存后点按钮直接在终端运行
#[tauri::command]
pub fn set_repo_cmds(
    app: tauri::AppHandle,
    path: String,
    test_cmd: String,
    build_cmd: String,
) -> Vec<store::RepoEntry> {
    let mut cfg = store::load(&app);
    if let Some(e) = cfg.repos.iter_mut().find(|r| r.path == path) {
        e.test_cmd = test_cmd.trim().to_string();
        e.build_cmd = build_cmd.trim().to_string();
    }
    let _ = store::save(&app, &cfg);
    cfg.repos
}

#[derive(Serialize, Clone, Default)]
pub struct CmdDetect {
    pub test_cmd: String,
    pub build_cmd: String,
    /// 开发调试命令（如 pnpm run dev / cargo run），仅预填快捷命令面板
    pub dev_cmd: String,
    /// 安装依赖命令（如 pnpm install），仅预填快捷命令面板
    pub install_cmd: String,
}

/// 按特征文件自动识别常用的测试 / 构建命令（只做预填，用户可修改后保存）
#[tauri::command]
pub fn detect_cmds(path: String) -> CmdDetect {
    let root = std::path::Path::new(&path);
    let mut d = CmdDetect::default();

    if root.join("package.json").is_file() {
        // 依据锁文件选择包管理器
        let pm = if root.join("pnpm-lock.yaml").is_file() {
            "pnpm"
        } else if root.join("yarn.lock").is_file() {
            "yarn"
        } else if root.join("bun.lockb").is_file() || root.join("bun.lock").is_file() {
            "bun"
        } else {
            "npm"
        };
        if let Ok(s) = std::fs::read_to_string(root.join("package.json")) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                let scripts = v.get("scripts");
                if scripts.and_then(|s| s.get("test")).is_some() {
                    d.test_cmd = format!("{pm} test");
                }
                if scripts.and_then(|s| s.get("build")).is_some() {
                    d.build_cmd = format!("{pm} run build");
                }
                if scripts.and_then(|s| s.get("dev")).is_some() {
                    d.dev_cmd = format!("{pm} run dev");
                }
                d.install_cmd = format!("{pm} install");
            }
        }
    }
    if d.test_cmd.is_empty() && root.join("Cargo.toml").is_file() {
        d.test_cmd = "cargo test".into();
    }
    if d.build_cmd.is_empty() && root.join("Cargo.toml").is_file() {
        d.build_cmd = "cargo build --release".into();
    }
    if d.dev_cmd.is_empty() && root.join("Cargo.toml").is_file() {
        d.dev_cmd = "cargo run".into();
    }
    if d.test_cmd.is_empty() && root.join("go.mod").is_file() {
        d.test_cmd = "go test ./...".into();
    }
    if d.build_cmd.is_empty() && root.join("go.mod").is_file() {
        d.build_cmd = "go build ./...".into();
    }
    if d.dev_cmd.is_empty() && root.join("main.go").is_file() {
        d.dev_cmd = "go run .".into();
    }
    if d.test_cmd.is_empty()
        && (root.join("pyproject.toml").is_file()
            || root.join("requirements.txt").is_file()
            || root.join("setup.py").is_file())
    {
        d.test_cmd = "pytest".into();
    }
    if d.test_cmd.is_empty() && root.join("pom.xml").is_file() {
        d.test_cmd = "mvn test".into();
    }
    if d.build_cmd.is_empty() && root.join("pom.xml").is_file() {
        d.build_cmd = "mvn package".into();
    }
    if d.test_cmd.is_empty()
        && (root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file())
    {
        d.test_cmd = "gradle test".into();
    }
    if d.build_cmd.is_empty()
        && (root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file())
    {
        d.build_cmd = "gradle build".into();
    }
    if d.test_cmd.is_empty()
        && std::fs::read_dir(root)
            .map(|rd| {
                rd.flatten().any(|e| {
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    n.ends_with(".csproj") || n.ends_with(".sln")
                })
            })
            .unwrap_or(false)
    {
        d.test_cmd = "dotnet test".into();
        d.build_cmd = "dotnet build".into();
    }
    d
}

/// 在仓库目录下后台运行命令（测试 / 构建等），不弹终端窗口，
/// stdout/stderr 逐行通过 Channel 推到前端日志窗口；同一时间只保留一个命令进程
#[tauri::command(async)]
pub fn run_repo_cmd(
    path: String,
    cmd: String,
    on_line: tauri::ipc::Channel<String>,
) -> Result<(), String> {
    if !std::path::Path::new(&path).is_dir() {
        return Err("仓库文件夹不存在".into());
    }
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return Err("命令为空".into());
    }

    let gen = CMD_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;

    // 新命令启动前先终止上一个后台命令（其运行线程会自行回收子进程）
    if let Some(p) = CMD_CHILD.lock().unwrap().take() {
        crate::git::terminate_job_tree(p.job);
    }
    CMD_STOP.store(false, std::sync::atomic::Ordering::SeqCst);

    let mut command = Command::new("cmd");
    command
        .args(["/C", cmd])
        .current_dir(&path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW：完全隐藏控制台窗口
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|e| format!("无法启动命令：{e}"))?;

    // 整树绑定 Job：手动终止或应用退出时连同 node/cargo 等子进程一起杀掉
    #[cfg(windows)]
    let job = crate::git::bind_kill_on_job_close(&child);
    #[cfg(not(windows))]
    let job: isize = 0;

    // 两个读线程分别转发 stdout / stderr，逐行推给前端日志
    let pump = |stream: Box<dyn std::io::Read + Send>, ch: tauri::ipc::Channel<String>| {
        std::thread::spawn(move || {
            use std::io::BufRead;
            let mut reader = std::io::BufReader::new(stream);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        // 过滤 ANSI 颜色/控制序列（cargo、vite 等的彩色输出），再去掉行尾空白
                        let cleaned = strip_ansi(line.trim_end_matches(['\r', '\n']))
                            .trim_end()
                            .to_string();
                        if !cleaned.is_empty() {
                            let _ = ch.send(cleaned);
                        }
                    }
                }
            }
        });
    };
    if let Some(out) = child.stdout.take() {
        pump(Box::new(out), on_line.clone());
    }
    if let Some(err) = child.stderr.take() {
        pump(Box::new(err), on_line);
    }

    // 交给全局持有：stop_repo_cmd 可随时整树终止
    *CMD_CHILD.lock().unwrap() = Some(CmdProc { job, gen });

    let status = child.wait().map_err(|e| format!("等待命令退出失败：{e}"))?;
    // 只清理仍然属于自己的条目，避免误删后启动的新命令
    let mut g = CMD_CHILD.lock().unwrap();
    if g.as_ref().map(|p| p.gen) == Some(gen) {
        *g = None;
    }
    drop(g);
    let code = status.code().unwrap_or(-1);
    if CMD_STOP.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("命令已被手动终止".into());
    }
    if code == 0 {
        Ok(())
    } else {
        Err(format!("命令执行失败，退出码 {code}"))
    }
}

/// 手动终止当前后台命令（整树杀掉，不留孤儿终端进程）
#[tauri::command]
pub fn stop_repo_cmd() -> Result<(), String> {
    let p = CMD_CHILD.lock().unwrap().take();
    match p {
        Some(p) => {
            CMD_STOP.store(true, std::sync::atomic::Ordering::SeqCst);
            crate::git::terminate_job_tree(p.job);
            Ok(())
        }
        None => Err("当前没有正在运行的后台命令".into()),
    }
}

// ---------- 磁盘空间扫描与缓存清理 ----------

/// 可清理的构建缓存目录名白名单（均为可再生的构建产物 / 依赖缓存）
const CACHE_DIRS: &[&str] = &[
    "target",
    "node_modules",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    "coverage",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".venv",
    "venv",
    ".gradle",
    "Debug",
    "Release",
    ".dart_tool",
];

#[derive(Serialize, Clone)]
pub struct DirSize {
    pub name: String,
    pub path: String,
    pub size: u64,
    /// 属于白名单内的可清理缓存目录
    pub is_cache: bool,
}

#[derive(Serialize, Clone)]
pub struct SpaceReport {
    /// 仓库文件夹总占用（字节）
    pub total: u64,
    /// 一级子目录 / 文件按大小排序
    pub dirs: Vec<DirSize>,
}

/// 超长路径加 \\?\ 前缀，避免深层 node_modules / target 触发 Windows MAX_PATH 限制
fn verbatim_path(p: &std::path::Path) -> std::path::PathBuf {
    let s = p.as_os_str().to_string_lossy();
    if s.len() > 240 && !s.starts_with("\\\\?\\") {
        if let Some(rest) = s.strip_prefix("\\\\") {
            return std::path::PathBuf::from(format!("\\\\?\\UNC\\{rest}"));
        }
        std::path::PathBuf::from(format!("\\\\?\\{s}"))
    } else {
        p.to_path_buf()
    }
}

/// 递归统计目录大小；符号链接 / junction 不跟进，避免循环与重复统计
fn dir_size(p: &std::path::Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    let mut total = 0u64;
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            total += dir_size(&e.path());
        } else if ft.is_file() {
            total += e.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

/// 扫描仓库一级内容的磁盘占用（大仓库可能耗时数秒，放后台线程执行）
#[tauri::command]
pub async fn scan_repo_space(path: String) -> Result<SpaceReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = std::path::Path::new(&path);
        if !root.is_dir() {
            return Err("文件夹不存在".into());
        }
        let root_v = verbatim_path(root);
        let mut dirs: Vec<DirSize> = Vec::new();
        let mut total = 0u64;
        if let Ok(rd) = std::fs::read_dir(&root_v) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let Ok(ft) = e.file_type() else { continue };
                let size = if ft.is_dir() {
                    dir_size(&e.path())
                } else if ft.is_file() {
                    e.metadata().map(|m| m.len()).unwrap_or(0)
                } else {
                    0
                };
                total += size;
                dirs.push(DirSize {
                    path: root.join(&name).to_string_lossy().to_string(),
                    is_cache: ft.is_dir() && CACHE_DIRS.contains(&name.as_str()),
                    name,
                    size,
                });
            }
        }
        dirs.sort_by(|a, b| {
            b.size
                .cmp(&a.size)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(SpaceReport { total, dirs })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 删除仓库下的构建缓存目录。只允许白名单内的一级子目录，避免误删任意内容。
#[tauri::command]
pub async fn delete_cache_dir(path: String, name: String) -> Result<u64, String> {
    if !CACHE_DIRS.contains(&name.as_str()) {
        return Err(format!("“{name}”不在可清理目录白名单中"));
    }
    let root = std::path::Path::new(&path);
    if !root.is_dir() {
        return Err("仓库文件夹不存在".into());
    }
    let dir_v = verbatim_path(&root.join(&name));
    let meta = std::fs::metadata(&dir_v).map_err(|_| "目录不存在".to_string())?;
    if !meta.is_dir() {
        return Err("目标不是文件夹".into());
    }
    let size = dir_size(&dir_v);
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::remove_dir_all(&dir_v).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                "删除失败：目录被占用（可能有编译 / 开发进程正在使用），请先关闭相关程序"
                    .to_string()
            } else {
                format!("删除失败：{e}")
            }
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_color_codes() {
        // cargo / vite 彩色输出的典型形式
        assert_eq!(strip_ansi("\x1b[2mVITE\x1b[22m \x1b[36mv6.4.3\x1b[39m"), "VITE v6.4.3");
        assert_eq!(strip_ansi("\x1b[92m   Compiling\x1b[0m ring v0.17.14"), "   Compiling ring v0.17.14");
        assert_eq!(strip_ansi("\x1b[1m\x1b[96m    Building\x1b[0m [====>] 502/510"), "    Building [====>] 502/510");
        // OSC 8 超链接（cargo 的 Finished 链接）
        assert_eq!(
            strip_ansi("\x1b]8;;https://doc.rust-lang.org/x\x1b\\`dev`\x1b]8;;\x1b\\ profile"),
            "`dev` profile"
        );
        // 纯文本原样保留
        assert_eq!(strip_ansi("plain text 中文"), "plain text 中文");
        // 悬空的 ESC 不至于 panic
        assert_eq!(strip_ansi("tail \x1b"), "tail ");
    }

    #[test]
    fn repo_name_from_url_works() {
        assert_eq!(
            repo_name_from_url("https://github.com/user/hello-world.git"),
            Some("hello-world".into())
        );
        assert_eq!(
            repo_name_from_url("https://gitee.com/user/hello-world"),
            Some("hello-world".into())
        );
        assert_eq!(
            repo_name_from_url("git@github.com:user/repo.git"),
            Some("repo".into())
        );
        // 结尾带斜杠时去掉斜杠后取最后一段（是否为合法仓库交给 clone 结果判断）
        assert_eq!(
            repo_name_from_url("https://github.com/user/"),
            Some("user".into())
        );
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

    #[test]
    fn repo_name_validation() {
        assert!(valid_repo_name("hello-world"));
        assert!(valid_repo_name("my_repo.1"));
        assert!(!valid_repo_name(""));
        assert!(!valid_repo_name("a b"));
        assert!(!valid_repo_name(".hidden"));
        assert!(!valid_repo_name("中文"));
    }
}
