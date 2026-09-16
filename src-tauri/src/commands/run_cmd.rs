//! 一键运行命令：测试 / 构建 / 开发 / 安装依赖，后台运行并流式回传输出。

use crate::store;
use serde::Serialize;
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

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

/// 把四条命令写进仓库条目：逐字段覆盖、互不影响，空串表示清除
///（清除后界面会用自动识别结果预填，不落盘）
fn apply_repo_cmds(
    e: &mut store::RepoEntry,
    test_cmd: &str,
    build_cmd: &str,
    dev_cmd: &str,
    install_cmd: &str,
) {
    e.test_cmd = test_cmd.trim().to_string();
    e.build_cmd = build_cmd.trim().to_string();
    e.dev_cmd = dev_cmd.trim().to_string();
    e.install_cmd = install_cmd.trim().to_string();
}

/// 保存某仓库的一键运行命令（测试 / 构建 / 开发 / 安装依赖），一行一个字段，
/// 逐字段覆盖，互不影响；保存后点按钮直接在终端运行
#[tauri::command]
pub fn set_repo_cmds(
    app: tauri::AppHandle,
    path: String,
    test_cmd: String,
    build_cmd: String,
    dev_cmd: String,
    install_cmd: String,
) -> Vec<store::RepoEntry> {
    let mut cfg = store::load(&app);
    if let Some(e) = cfg.repos.iter_mut().find(|r| r.path == path) {
        apply_repo_cmds(e, &test_cmd, &build_cmd, &dev_cmd, &install_cmd);
    }
    let _ = store::save(&app, &cfg);
    cfg.repos
}

/// 操作区最多钉几个自定义按钮：再多就把工具栏挤成两行还认不出谁是谁
pub(super) const MAX_PINNED_CMDS: usize = 3;
/// 自动兜底的按钮文案最长几个字（中文按字符算，不按字节）
const PINNED_LABEL_CHARS: usize = 12;

/// 命令 → 按钮文案：取前两个词（"pnpm run lint" → "pnpm run"），够短也够认得出来
fn derive_pinned_label(cmd: &str) -> String {
    let joined = cmd.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
    joined.chars().take(PINNED_LABEL_CHARS).collect()
}

/// 校验并整理要钉到操作区的命令。
/// 上限是硬约束（超了直接报错，不静默截断——用户以为钉上了却没出现更糟）；
/// 空命令的行直接丢掉（多半是编辑器里没填完的空行），文案留空则用命令兜底
fn sanitize_pinned(cmds: Vec<store::PinnedCmd>) -> Result<Vec<store::PinnedCmd>, String> {
    if cmds.len() > MAX_PINNED_CMDS {
        return Err(format!("操作区最多只能钉 {MAX_PINNED_CMDS} 个按钮"));
    }
    let mut out = Vec::new();
    for c in cmds {
        let cmd = c.cmd.trim().to_string();
        if cmd.is_empty() {
            continue;
        }
        let label = match c.label.trim() {
            "" => derive_pinned_label(&cmd),
            l => l.to_string(),
        };
        out.push(store::PinnedCmd { label, cmd });
    }
    Ok(out)
}

/// 保存某仓库钉在操作区的自定义命令按钮（最多 MAX_PINNED_CMDS 个）
#[tauri::command]
pub fn set_pinned_cmds(
    app: tauri::AppHandle,
    path: String,
    cmds: Vec<store::PinnedCmd>,
) -> Result<Vec<store::RepoEntry>, String> {
    let pinned = sanitize_pinned(cmds)?;
    let mut cfg = store::load(&app);
    if let Some(e) = cfg.repos.iter_mut().find(|r| r.path == path) {
        e.pinned_cmds = pinned;
    }
    store::save(&app, &cfg)?;
    Ok(cfg.repos)
}

/// 设置某仓库的定时自动同步。两种模式：
/// - 间隔模式：daily_time 为空，mins > 0（分钟）
/// - 每天定时模式：daily_time = "HH:MM"（本地时区），mins 置 0
/// mins 与 daily_time 均无效（0 + 空）即为关闭。
/// 开启 / 切换模式 / 改参数时把 last_auto_check 归零，让调度器立即跑首轮。
#[tauri::command]
pub fn set_auto_sync(
    app: tauri::AppHandle,
    path: String,
    mins: u32,
    auto_pull: bool,
    daily_time: String,
) -> Vec<store::RepoEntry> {
    // 每天定时模式只在时间合法时生效（"HH:MM"，0-23:0-59）
    let daily = store::parse_hhmm(daily_time.trim())
        .map(|(h, m)| format!("{h:02}:{m:02}"))
        .unwrap_or_default();
    let eff_mins = if daily.is_empty() { mins } else { 0 };
    let mut cfg = store::load(&app);
    if let Some(e) = cfg.repos.iter_mut().find(|r| r.path == path) {
        if e.auto_sync_mins != eff_mins || e.auto_pull != auto_pull || e.daily_time != daily {
            e.auto_sync_mins = eff_mins;
            e.daily_time = daily;
            e.auto_pull = auto_pull;
            // 重新开启（或改间隔 / 时间点）时立即到期；关闭时清掉旧结果摘要
            e.last_auto_check = if eff_mins > 0 || !e.daily_time.is_empty() {
                0
            } else {
                e.last_auto_check
            };
            if eff_mins == 0 && e.daily_time.is_empty() {
                e.last_auto_result = String::new();
            }
        }
    }
    let _ = store::save(&app, &cfg);
    cfg.repos
}

/// 置顶 / 取消置顶仓库；置顶的仓库在列表最前，其余仍按最近使用排序
#[tauri::command]
pub fn pin_repo(app: tauri::AppHandle, path: String, pinned: bool) -> Vec<store::RepoEntry> {
    let mut cfg = store::load(&app);
    if let Some(e) = cfg.repos.iter_mut().find(|r| r.path == path) {
        e.pinned = pinned;
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
    fn repo_cmds_are_saved_independently() {
        // 回归：四个命令字段曾互相覆盖（保存「测试」把「构建」也改了）。
        // 这里逐字段写入，任何一个字段都不该被另一个字段的值顶掉。
        let mut e = store::RepoEntry {
            path: "C:/repo".into(),
            name: "repo".into(),
            alias: String::new(),
            added_at: 0,
            last_used: 0,
            test_cmd: "cargo test".into(),
            build_cmd: "cargo build --release".into(),
            dev_cmd: "cargo run".into(),
            install_cmd: "cargo fetch".into(),
            pinned_cmds: Vec::new(),
            auto_sync_mins: 0,
            daily_time: String::new(),
            auto_pull: false,
            last_auto_check: 0,
            last_auto_result: String::new(),
            pinned: false,
        };

        // 只改测试：其余三个字段保持原值（回归：曾出现保存「测试」把「构建」也改掉）
        apply_repo_cmds(&mut e, "pnpm test", "cargo build --release", "cargo run", "cargo fetch");
        assert_eq!(e.test_cmd, "pnpm test");
        assert_eq!(e.build_cmd, "cargo build --release");
        assert_eq!(e.dev_cmd, "cargo run");
        assert_eq!(e.install_cmd, "cargo fetch");

        // 只改构建：测试不被改回去
        apply_repo_cmds(&mut e, "pnpm test", "pnpm run build", "cargo run", "cargo fetch");
        assert_eq!(e.test_cmd, "pnpm test");
        assert_eq!(e.build_cmd, "pnpm run build");

        // 只改开发 / 安装依赖（用户自定义覆盖自动识别结果）
        apply_repo_cmds(&mut e, "pnpm test", "pnpm run build", "pnpm run dev", "pnpm install");
        assert_eq!(e.test_cmd, "pnpm test");
        assert_eq!(e.build_cmd, "pnpm run build");
        assert_eq!(e.dev_cmd, "pnpm run dev");
        assert_eq!(e.install_cmd, "pnpm install");

        // 首尾空白落盘前被去掉
        apply_repo_cmds(&mut e, "  cargo test  ", "", "", "");
        assert_eq!(e.test_cmd, "cargo test");
        // 清空即回到"未设置"（界面下次打开会用自动识别结果预填）
        assert!(e.build_cmd.is_empty() && e.dev_cmd.is_empty() && e.install_cmd.is_empty());
    }

    #[test]
    fn pinned_label_falls_back_to_the_command() {
        // 文案留空时不能让界面上出现一个没字可点的按钮
        assert_eq!(derive_pinned_label("pnpm run lint"), "pnpm run");
        assert_eq!(derive_pinned_label("cargo clippy"), "cargo clippy");
        assert_eq!(derive_pinned_label("  make  "), "make");
        // 单行长命令没有空格可切，就按字符截断，且不能把中文截成半个字
        assert_eq!(derive_pinned_label("一个非常非常长的中文命令名占位"), "一个非常非常长的中文命令");
        assert!(derive_pinned_label("abcdefghijklmnop").chars().count() <= PINNED_LABEL_CHARS);
    }

    #[test]
    fn pinned_cmds_over_the_cap_are_refused_not_silently_dropped() {
        let make = |n: usize| -> Vec<store::PinnedCmd> {
            (0..n)
                .map(|i| store::PinnedCmd {
                    label: format!("b{i}"),
                    cmd: format!("cmd{i}"),
                })
                .collect()
        };
        assert!(sanitize_pinned(make(MAX_PINNED_CMDS)).is_ok());
        let err = sanitize_pinned(make(MAX_PINNED_CMDS + 1)).unwrap_err();
        assert!(err.contains("最多"), "{err}");
    }

    #[test]
    fn pinned_cmds_drop_blank_rows_and_trim() {
        let got = sanitize_pinned(vec![
            store::PinnedCmd { label: "  检查  ".into(), cmd: "  pnpm lint  ".into() },
            // 编辑器里没填完的空行：丢掉而不是报错为难用户
            store::PinnedCmd { label: "空的".into(), cmd: "   ".into() },
            // 没写文案：用命令前两个词兜底
            store::PinnedCmd { label: String::new(), cmd: "cargo clippy --all".into() },
        ])
        .unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].label, "检查");
        assert_eq!(got[0].cmd, "pnpm lint");
        assert_eq!(got[1].label, "cargo clippy");
        assert_eq!(got[1].cmd, "cargo clippy --all");
    }
}
