//! 一键运行命令：测试 / 构建 / 开发 / 安装依赖，后台运行并流式回传输出。
//!
//! 平移自 `src-tauri/commands/run_cmd.rs`，去掉 Tauri 依赖：原来往
//! `tauri::ipc::Channel` 推行，现在改成通用回调，由 gui 侧自己接回 UI 线程。
//!
//! 同一时间只保留一个命令进程：新命令启动前先整树终止上一个，
//! 否则用户连点两个「测试」会同时跑起两套 node/cargo，日志还会串在一起。

use crate::git;
use crate::store::{self, CmdRunRecord, PinnedCmd, RepoEntry};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// 操作区最多钉几个自定义按钮：再多就把命令卡挤成两行还认不出谁是谁
pub const MAX_PINNED_CMDS: usize = 3;
/// 自动兜底的按钮文案最长几个字（中文按字符算，不按字节）
const PINNED_LABEL_CHARS: usize = 12;

/// 命令卡上的每一行是个槽位。上次运行结果按槽位记，
/// 不按命令文本记——用户改了命令不该把上次结果一起清掉。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Test,
    Build,
    Dev,
    Install,
    /// 第 i 个钉住的自定义按钮（从 0 起）
    Pinned(usize),
}

impl Slot {
    /// 存进 `RepoEntry::cmd_runs` 的键
    pub fn key(self) -> String {
        match self {
            Slot::Test => "test".into(),
            Slot::Build => "build".into(),
            Slot::Dev => "dev".into(),
            Slot::Install => "install".into(),
            Slot::Pinned(i) => format!("pinned:{i}"),
        }
    }

    /// 卡片上的固定文案
    pub fn default_label(self) -> &'static str {
        match self {
            Slot::Test => "测试",
            Slot::Build => "构建",
            Slot::Dev => "开发",
            Slot::Install => "安装依赖",
            Slot::Pinned(_) => "",
        }
    }

    /// 该槽位在仓库条目上对应的命令；钉住按钮取不到就返回空串
    pub fn cmd<'a>(self, e: &'a RepoEntry) -> &'a str {
        match self {
            Slot::Test => &e.test_cmd,
            Slot::Build => &e.build_cmd,
            Slot::Dev => &e.dev_cmd,
            Slot::Install => &e.install_cmd,
            Slot::Pinned(i) => e
                .pinned_cmds
                .get(i)
                .map(|c| c.cmd.as_str())
                .unwrap_or_default(),
        }
    }
}

// ---------- 全局：当前后台命令 ----------

/// 当前后台命令的 Job 句柄与代数编号。
/// 子进程本身留在运行线程里等待回收，全局只记 Job 句柄（Windows 无 Child::try_clone）。
struct CmdProc {
    job: isize,
    gen: u64,
}

static CMD_CHILD: Mutex<Option<CmdProc>> = Mutex::new(None);
static CMD_STOP: AtomicBool = AtomicBool::new(false);
static CMD_GEN: AtomicU64 = AtomicU64::new(0);

/// 手动终止当前后台命令（整树杀掉，不留孤儿 node/cargo 进程）
pub fn stop_repo_cmd() -> Result<(), String> {
    match CMD_CHILD.lock().unwrap().take() {
        Some(p) => {
            CMD_STOP.store(true, Ordering::SeqCst);
            git::terminate_job_tree(p.job);
            Ok(())
        }
        None => Err("当前没有正在运行的后台命令".into()),
    }
}

/// 去除 ANSI 转义序列（颜色 / 光标控制等），终端彩色输出转成纯文本
pub fn strip_ansi(s: &str) -> String {
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

// ---------- 保存命令 ----------

/// 把四条命令写进仓库条目：逐字段覆盖、互不影响，空串表示清除
/// （清除后界面会用自动识别结果预填，不落盘）
fn apply_repo_cmds(
    e: &mut RepoEntry,
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

/// 保存某仓库的一键运行命令，逐字段覆盖，互不影响
pub fn set_repo_cmds(
    path: &str,
    test_cmd: &str,
    build_cmd: &str,
    dev_cmd: &str,
    install_cmd: &str,
) -> Result<Vec<RepoEntry>, String> {
    update_repo(path, |e| {
        apply_repo_cmds(e, test_cmd, build_cmd, dev_cmd, install_cmd)
    })
}

/// 命令 → 按钮文案：取前两个词（"pnpm run lint" → "pnpm run"），够短也够认得出来
pub fn derive_pinned_label(cmd: &str) -> String {
    let joined = cmd.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
    joined.chars().take(PINNED_LABEL_CHARS).collect()
}

/// 校验并整理要钉住的命令。
/// 上限是硬约束（超了直接报错，不静默截断——用户以为钉上了却没出现更糟）；
/// 空命令的行直接丢掉（多半是编辑器里没填完的空行），文案留空则用命令兜底
fn sanitize_pinned(cmds: Vec<PinnedCmd>) -> Result<Vec<PinnedCmd>, String> {
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
        out.push(PinnedCmd { label, cmd });
    }
    Ok(out)
}

/// 保存某仓库钉在操作区的自定义命令按钮（最多 MAX_PINNED_CMDS 个）
pub fn set_pinned_cmds(path: &str, cmds: Vec<PinnedCmd>) -> Result<Vec<RepoEntry>, String> {
    let pinned = sanitize_pinned(cmds)?;
    update_repo(path, |e| e.pinned_cmds = pinned)
}

/// 改某仓库的某个字段并落盘；仓库不在列表里时返回 None（不改配置）
fn update_repo<F>(path: &str, f: F) -> Result<Vec<RepoEntry>, String>
where
    F: FnOnce(&mut RepoEntry),
{
    let mut cfg = store::load();
    if let Some(e) = cfg.repos.iter_mut().find(|r| r.path == path) {
        f(e);
    }
    store::save(&cfg)?;
    Ok(cfg.repos)
}

/// 记下某槽位这次运行的结果。返回落盘后的条目列表。
pub fn record_run(
    path: &str,
    slot: Slot,
    ok: bool,
    code: i32,
    lines: u32,
) -> Result<Vec<RepoEntry>, String> {
    let rec = CmdRunRecord {
        at: store::now_secs(),
        ok,
        code,
        lines,
    };
    update_repo(path, |e| {
        e.cmd_runs.insert(slot.key(), rec);
    })
}

// ---------- 自动识别命令 ----------

/// 按特征文件自动识别常用的测试 / 构建命令（只做预填，用户可修改后保存）
pub fn detect_cmds(path: &str) -> CmdDetect {
    let root = std::path::Path::new(path);
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
    let cargo = root.join("Cargo.toml").is_file();
    if cargo {
        if d.test_cmd.is_empty() {
            d.test_cmd = "cargo test".into();
        }
        if d.build_cmd.is_empty() {
            d.build_cmd = "cargo build --release".into();
        }
        if d.dev_cmd.is_empty() {
            d.dev_cmd = "cargo run".into();
        }
    }
    let go = root.join("go.mod").is_file();
    if go {
        if d.test_cmd.is_empty() {
            d.test_cmd = "go test ./...".into();
        }
        if d.build_cmd.is_empty() {
            d.build_cmd = "go build ./...".into();
        }
        if d.dev_cmd.is_empty() && root.join("main.go").is_file() {
            d.dev_cmd = "go run .".into();
        }
    }
    if d.test_cmd.is_empty()
        && (root.join("pyproject.toml").is_file()
            || root.join("requirements.txt").is_file()
            || root.join("setup.py").is_file())
    {
        d.test_cmd = "pytest".into();
    }
    if root.join("pom.xml").is_file() {
        if d.test_cmd.is_empty() {
            d.test_cmd = "mvn test".into();
        }
        if d.build_cmd.is_empty() {
            d.build_cmd = "mvn package".into();
        }
    }
    if root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file() {
        if d.test_cmd.is_empty() {
            d.test_cmd = "gradle test".into();
        }
        if d.build_cmd.is_empty() {
            d.build_cmd = "gradle build".into();
        }
    }
    if d.test_cmd.is_empty()
        && d.build_cmd.is_empty()
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

/// 自动识别出来的四条命令（界面拿来预填，用户可改后保存）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CmdDetect {
    pub test_cmd: String,
    pub build_cmd: String,
    /// 开发调试命令（如 pnpm run dev / cargo run）
    pub dev_cmd: String,
    /// 安装依赖命令（如 pnpm install）
    pub install_cmd: String,
}

// ---------- 运行 ----------

/// 在仓库目录下运行一条命令，不弹终端窗口，stdout/stderr 逐行回调出去。
///
/// 阻塞到命令结束——**必须**在后台线程里调用（gui 侧走 `cx.background_executor()`）。
/// 结束后把结果按槽位落盘，返回刚写入的那条记录。
///
/// `on_line` 要跨读取线程共享，所以是 `Arc` 而不是 `&dyn Fn`：
/// 回调在两个读取线程上被调用，**不是**同一个线程，里面要自己加锁。
pub fn run_repo_cmd(
    path: &str,
    cmd: &str,
    slot: Slot,
    on_line: Arc<dyn Fn(&str) + Send + Sync + 'static>,
) -> Result<CmdRunRecord, String> {
    if !std::path::Path::new(path).is_dir() {
        return Err("仓库文件夹不存在".into());
    }
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return Err("命令为空".into());
    }

    let gen = CMD_GEN.fetch_add(1, Ordering::SeqCst) + 1;

    // 新命令启动前先终止上一个后台命令（其运行线程会自行回收子进程）
    if let Some(p) = CMD_CHILD.lock().unwrap().take() {
        git::terminate_job_tree(p.job);
    }
    CMD_STOP.store(false, Ordering::SeqCst);

    let mut command = Command::new("cmd");
    command
        .args(["/C", cmd])
        .current_dir(path)
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
    let job = git::bind_kill_on_job_close(&child);
    #[cfg(not(windows))]
    let job: isize = 0;

    // 收到的行数由两个读取线程共同累加
    let lines = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let pump = |stream: Box<dyn std::io::Read + Send>| {
        let on_line = Arc::clone(&on_line);
        let lines = std::sync::Arc::clone(&lines);
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
                            lines.fetch_add(1, Ordering::SeqCst);
                            on_line(&cleaned);
                        }
                    }
                }
            }
        });
    };
    if let Some(out) = child.stdout.take() {
        pump(Box::new(out));
    }
    if let Some(err) = child.stderr.take() {
        pump(Box::new(err));
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
    let stopped = CMD_STOP.load(Ordering::SeqCst);
    let line_count = lines.load(Ordering::SeqCst);
    let rec = CmdRunRecord {
        at: store::now_secs(),
        ok: code == 0,
        code,
        lines: line_count,
    };
    // 落盘失败不该盖掉命令本身的失败：命令的结论优先，落盘错误只在命令成功时上报
    let saved = record_run(path, slot, rec.ok, rec.code, rec.lines);
    if stopped {
        return Err("命令已被手动终止".into());
    }
    if code != 0 {
        return Err(format!("命令执行失败，退出码 {code}"));
    }
    saved?;
    Ok(rec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_color_codes() {
        // cargo / vite 彩色输出的典型形式
        assert_eq!(
            strip_ansi("\x1b[2mVITE\x1b[22m \x1b[36mv6.4.3\x1b[39m"),
            "VITE v6.4.3"
        );
        assert_eq!(
            strip_ansi("\x1b[92m   Compiling\x1b[0m ring v0.17.14"),
            "   Compiling ring v0.17.14"
        );
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
    fn slot_keys_are_stable_and_distinct() {
        // 这些键要落进 config.json，界面靠它对上号——改了等于丢历史
        assert_eq!(Slot::Test.key(), "test");
        assert_eq!(Slot::Pinned(0).key(), "pinned:0");
        assert_eq!(Slot::Pinned(2).key(), "pinned:2");
        let keys = [
            Slot::Test,
            Slot::Build,
            Slot::Dev,
            Slot::Install,
            Slot::Pinned(0),
        ]
        .map(Slot::key);
        let mut uniq = keys.to_vec();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), keys.len(), "槽位键撞车了：{keys:?}");
    }

    #[test]
    fn repo_cmds_are_saved_independently() {
        // 回归：四个命令字段曾互相覆盖（保存「测试」把「构建」也改了）
        let mut e = RepoEntry {
            path: "C:/repo".into(),
            name: "repo".into(),
            test_cmd: "cargo test".into(),
            build_cmd: "cargo build --release".into(),
            dev_cmd: "cargo run".into(),
            install_cmd: "cargo fetch".into(),
            ..RepoEntry::default()
        };

        apply_repo_cmds(
            &mut e,
            "pnpm test",
            "cargo build --release",
            "cargo run",
            "cargo fetch",
        );
        assert_eq!(e.test_cmd, "pnpm test");
        assert_eq!(e.build_cmd, "cargo build --release");
        assert_eq!(e.dev_cmd, "cargo run");
        assert_eq!(e.install_cmd, "cargo fetch");

        // 只改构建：测试不被改回去
        apply_repo_cmds(&mut e, "pnpm test", "pnpm run build", "cargo run", "cargo fetch");
        assert_eq!(e.test_cmd, "pnpm test");
        assert_eq!(e.build_cmd, "pnpm run build");

        // 首尾空白落盘前被去掉；清空即回到「未设置」
        apply_repo_cmds(&mut e, "  cargo test  ", "", "", "");
        assert_eq!(e.test_cmd, "cargo test");
        assert!(e.build_cmd.is_empty() && e.dev_cmd.is_empty() && e.install_cmd.is_empty());
    }

    #[test]
    fn pinned_label_falls_back_to_the_command() {
        // 文案留空时不能让界面上出现一个没字可点的按钮
        assert_eq!(derive_pinned_label("pnpm run lint"), "pnpm run");
        assert_eq!(derive_pinned_label("cargo clippy"), "cargo clippy");
        assert_eq!(derive_pinned_label("  make  "), "make");
        // 单行长命令没有空格可切，就按字符截断，且不能把中文截成半个字
        assert_eq!(
            derive_pinned_label("一个非常非常长的中文命令名占位"),
            "一个非常非常长的中文命令"
        );
        assert!(derive_pinned_label("abcdefghijklmnop").chars().count() <= PINNED_LABEL_CHARS);
    }

    #[test]
    fn pinned_cmds_over_the_cap_are_refused_not_silently_dropped() {
        let make = |n: usize| -> Vec<PinnedCmd> {
            (0..n)
                .map(|i| PinnedCmd {
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
            PinnedCmd {
                label: "  检查  ".into(),
                cmd: "  pnpm lint  ".into(),
            },
            // 编辑器里没填完的空行：丢掉而不是报错为难用户
            PinnedCmd {
                label: "空的".into(),
                cmd: "   ".into(),
            },
            // 没写文案：用命令前两个词兜底
            PinnedCmd {
                label: String::new(),
                cmd: "cargo clippy --all".into(),
            },
        ])
        .unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].label, "检查");
        assert_eq!(got[0].cmd, "pnpm lint");
        assert_eq!(got[1].label, "cargo clippy");
        assert_eq!(got[1].cmd, "cargo clippy --all");
    }

    #[test]
    fn slot_reads_its_own_command_only() {
        let e = RepoEntry {
            test_cmd: "pnpm test".into(),
            build_cmd: "pnpm run build".into(),
            pinned_cmds: vec![PinnedCmd {
                label: "lint".into(),
                cmd: "pnpm lint".into(),
            }],
            ..RepoEntry::default()
        };
        assert_eq!(Slot::Test.cmd(&e), "pnpm test");
        assert_eq!(Slot::Build.cmd(&e), "pnpm run build");
        assert_eq!(Slot::Dev.cmd(&e), "");
        assert_eq!(Slot::Pinned(0).cmd(&e), "pnpm lint");
        // 没钉第二个按钮时不能越界 panic
        assert_eq!(Slot::Pinned(1).cmd(&e), "");
    }

    #[test]
    fn detect_reads_the_lockfile_to_pick_the_package_manager() {
        let dir = std::env::temp_dir().join(format!("qingbird-detect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"test":"vitest","build":"vite build","dev":"vite"}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();

        let d = detect_cmds(dir.to_str().unwrap());
        assert_eq!(d.test_cmd, "pnpm test");
        assert_eq!(d.build_cmd, "pnpm run build");
        assert_eq!(d.dev_cmd, "pnpm run dev");
        assert_eq!(d.install_cmd, "pnpm install");

        // 锁文件决定包管理器，且有优先级：pnpm 挪走才轮到 yarn，两个都没有才退回 npm
        std::fs::remove_file(dir.join("pnpm-lock.yaml")).unwrap();
        std::fs::write(dir.join("yarn.lock"), "").unwrap();
        assert_eq!(detect_cmds(dir.to_str().unwrap()).test_cmd, "yarn test");

        std::fs::remove_file(dir.join("yarn.lock")).unwrap();
        assert_eq!(detect_cmds(dir.to_str().unwrap()).test_cmd, "npm test");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
