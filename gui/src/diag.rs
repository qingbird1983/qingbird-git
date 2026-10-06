//! 运行期诊断：`%APPDATA%\com.qingbird.git\app.log`
//!
//! ## 为什么需要这个文件
//!
//! release 走 `#![windows_subsystem = "windows"]`（`main.rs` 文件头），编出来的是
//! **GUI 子系统**程序：Windows **不给它分配控制台，因此没有 stderr**。
//! 于是 debug 里好好的 `eprintln!`，装到用户机器上就**凭空消失**——
//! 托盘图标建失败、重开窗口失败、自动同步写回失败这几处，
//! 表现全是"完全静默、什么都不发生"。
//!
//! 这个模块是那条诊断输出**唯一的去处**。
//!
//! **判据**：凡是"出错了但界面上看不见"的地方，写这里，**别再写 `eprintln!`**。
//! debug 构建下 `note()` 仍然同时打一份到 stderr，抓帧 / 启动探针那条链路
//! （`scripts/steps.py`、`startup_probe.py`）照样能在控制台里看到。
//!
//! ## 为什么顺带装 panic hook
//!
//! GUI 子系统下 panic 消息同样没有去处：release 里 `debug_assert!` 是空操作、
//! 真 panic 则直接终止进程，用户看到的是"程序自己没了"。
//! 装上 hook 之后，至少这份日志里有一行带位置的记录。
//!
//! ⚠️ **hook 只写日志，不弹窗、不吞掉 panic**——它调回默认 hook，
//! 保留 abort / 退出码那套既有行为。别在这里加 `MessageBox`：
//! 那会让一个后台 panic 变成一个模态框卡住整条消息泵。
//!
//! ## 为什么不引日志库
//!
//! `tracing` / `fern` 之类会带来初始化顺序、单文件大小、滚动策略一整套问题。
//! 这里要的只是"几行文本落盘 + 别无限涨"，`std::fs` 就够，**零新依赖**。
//! 时间戳用 `chrono`——它早就是 `gui` 的直接依赖（`overview::fill_days` 在用）。

use std::io::Write;
use std::path::PathBuf;

/// 超过这个大小就把 `app.log` 改名成 `app.log.1`（**覆盖**上一份，不留多代），
/// 从空文件重写。留一代就够——诊断看的是"刚才那次"，不是考古。
const MAX_BYTES: u64 = 512 * 1024;

/// 从一个 `APPDATA` 值求出日志路径。**纯函数，不自己读环境变量**——
/// 这样单测能直接喂一个临时路径进来，完全不必动 `APPDATA`。
fn log_path_in(appdata: Option<&str>) -> Option<PathBuf> {
    // 与 `store::config_path()` / `scan_cache::db_path()` 同一处目录。
    // 这里**不**去调它们：那两个是私有函数，而"配置目录"这个概念
    // 三处各拼一遍 `com.qingbird.git` 已经够了（日志不该长在配置读写那条路上，
    // 免得配置写失败时把日志也一起拖下水）。
    let appdata = appdata?;
    Some(PathBuf::from(appdata).join("com.qingbird.git").join("app.log"))
}

/// **本文件里唯一读 `APPDATA` 的地方**是 `note_at` 的调用方 `note()`。
/// 下面 `note_at` / `note_to` / `log_path_in` 一律收显式参数，
/// 于是单测整条链路都在沙箱里跑，不需要、也不允许自己去 `set_var`。
///
/// ⚠️ 中间试过留一个 `log_path()` 当"纯函数 + 读环境的壳"的分界，
/// 结果它在非测试构建里**没有调用方**（`warning: function log_path is never used`）——
/// 因为 `note()` 已经直接用 `log_path_in(env…)` 了。
/// **留着它就是留一句会骗人的话**：读代码的人会以为那是那条真实路径。
/// 需要真实路径的地方只有测试里"真实日志没被碰"那条断言，让它自己读一次。

/// 追加一行诊断。**永不 panic、永不阻塞**：写不进去就什么也不做。
///
/// 判据是"记下来有没有用"，不是"写成功没有"——一个只用于诊断的通道
/// 绝不能自己把程序带崩。`APPDATA` 不存在、目录建不了、文件被占用，
/// 三种情况都直接返回。
pub(crate) fn note(msg: &str) {
    // debug 仍然打一份到 stderr：抓帧那条验证链路是 agent shell 起的进程，
    // 它读得到 stderr，而读不到用户的 %APPDATA%。
    #[cfg(debug_assertions)]
    eprintln!("{msg}");

    note_at(std::env::var("APPDATA").ok().as_deref(), msg);
}

/// `appdata` 是 `None` ⇒ **立刻返回，什么也不做**。
///
/// 为什么把 `appdata` 也收成参数（而不是只在 `log_path` 那一层拆）：
/// 单测要能整条链路都在沙箱里跑到底。**第一版那条用例调的是 `note()`**，
/// 于是它读**真实 `APPDATA`**，每跑一次 `cargo test` 就往用户的
/// `%APPDATA%\com.qingbird.git\app.log` 追加一行"这条不该崩"
/// ——测试把自己的垃圾写到了沙箱外面，和主进程那次隔离失误同一类。
/// ⇒ `note()` 只是"读环境 + 转发"的一层壳，可测的东西一律在 `note_at`。
fn note_at(appdata: Option<&str>, msg: &str) {
    let Some(path) = log_path_in(appdata) else {
        return;
    };
    note_to(&path, msg);
}

/// 真正的写入。**路径由调用方给**——这是本模块能写出**并行安全**单测的原因，
/// 见 `tests` 里那条「为什么这里不 set_var」的注释。
fn note_to(path: &std::path::Path, msg: &str) {
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    rotate_if_big(path);

    let line = format!("[{}] {msg}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// 太大了就轮转。**先按大小判再动手**——`metadata()` 失败（文件刚被删、
/// 目录刚没了）就当没这回事，不去猜。
fn rotate_if_big(path: &std::path::Path) {
    let too_big = std::fs::metadata(path)
        .map(|m| m.len() > MAX_BYTES)
        .unwrap_or(false);
    if !too_big {
        return;
    }
    // `app.log.1` 用 rename 覆盖：Windows 上 rename 会覆盖同名目标，
    // 所以**不需要**先删——真失败了也只是不轮转，不影响追加。
    let _ = std::fs::rename(path, path.with_extension("log.1"));
}

/// 装 panic hook。**必须在任何可能 panic 的代码之前调**（`main()` 最开头）。
///
/// 只加日志、不改行为：默认 hook 照常调回去，退出码与 abort 语义不变。
pub(crate) fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // `info` 的 Display 在 panic 展开时**可能**二次 panic（用户装的
        // Display 实现里 panic），所以连它一起兜住。
        let where_ = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "位置未知".into());
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            info.payload()
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "非字符串的 panic payload".into())
        }))
        .unwrap_or_else(|_| "读 panic 消息时又炸了".into());
        note(&format!("PANIC at {where_}: {payload}"));
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 判据不是"文件在不在"，而是**行内容真的落到位**：
    /// 时间戳 + 原文都要在里面。这条同时挡住"只建了空文件"和"格式不对"。
    ///
    /// ## 为什么这里 `note_to(&临时路径, ..)` 而不是 `set_var("APPDATA", ..)`
    ///
    /// **2026-10-05 实机踩过**：第一版这条用例是
    /// `set_var("APPDATA", 临时目录)` + 调 `note()`，**第一次跑是绿的**。
    /// 改右栏弹簧重跑时它红了——而红的不是它自己，是**与别的用例互踩**：
    /// `APPDATA` 是**进程级全局**，cargo 默认多线程跑同一个二进制里的用例，
    /// `scan_cache` 的用例也在 set/还原它（它自己带了一把 `APPDATA_LOCK`，
    /// 注释里明写"改环境变量是进程级的，并行跑会'偶发红、重跑又好了'"）。
    /// 我没上锁 ⇒ 两边各自把对方的值换掉。
    ///
    /// **根因不是"忘了加锁"，是"测试要去改一个全局"**。
    /// ⇒ 修法是**把依赖变成参数**（`log_path_in` / `note_to`），
    /// 让用例根本不需要碰环境变量——**并行安全由构造保证，不靠一把运行时锁**。
    /// 判据：任何单测如果需要 `set_var` 才能跑，先问"能不能把那个值
    /// 从参数进来"。
    #[test]
    fn note_appends_a_timestamped_line() {
        let dir = std::env::temp_dir().join("qb-diag-test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = log_path_in(Some(dir.to_str().unwrap())).expect("给了路径就有");
        note_to(&path, "第一行");
        note_to(&path, "第二行");

        let text = std::fs::read_to_string(&path).expect("刚写的读不到");
        assert_eq!(text.lines().count(), 2, "两行就两行：{text:?}");
        assert!(text.contains("第一行") && text.contains("第二行"));
        // 时间戳前缀：`[YYYY-MM-DD HH:MM:SS] `
        assert!(
            text.starts_with('[') && text.contains("] 第一行"),
            "每行都要带本地时间戳前缀：{text:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `APPDATA` 缺失 ⇒ **没有路径**，立刻返回、什么都不做、**也不崩**。
    /// 诊断通道自己把程序带崩是本末倒置。
    ///
    /// ⚠️ 判据是"**真实 APPDATA 里没多出任何东西**"：这正是第一版那条用例
    /// 漏掉的——它调 `note()`，于是每跑一次 `cargo test` 就往用户真实的
    /// `app.log` 追加一行。现在只调 `note_at(None, ..)`，整条链路都在沙箱里。
    #[test]
    fn a_missing_appdata_writes_nothing_and_does_not_panic() {
        assert!(log_path_in(None).is_none());
        // 判据是「**前后指纹不变**」而不是「文件不存在」——真实跑过一次之后
        // 它本来就该存在，拿"不存在"当判据只会在第一次跑的时候成立
        let before = fingerprint(&real_log_path());
        note_at(None, "这条不该写出去，也不该崩");
        let after = fingerprint(&real_log_path());
        assert_eq!(
            before, after,
            "appdata 为 None 时不该碰真实日志：before={before:?} after={after:?}"
        );
    }

    /// 判据是"**给了路径就真的写到那儿**"，且写到的是**临时目录**那份。
    #[test]
    fn note_at_writes_into_the_given_directory() {
        let dir = std::env::temp_dir().join("qb-diag-noteat");
        let _ = std::fs::remove_dir_all(&dir);
        let appdata = dir.to_str().unwrap();
        note_at(Some(appdata), "写进沙箱");
        let text = std::fs::read_to_string(
            log_path_in(Some(appdata)).expect("给了 appdata 就有路径"),
        )
        .expect("刚写的读不到");
        assert!(text.contains("写进沙箱"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 文件指纹：`(存在?, 长度)`。**只读**，不创建。
    fn fingerprint(path: &std::path::Path) -> Option<u64> {
        std::fs::metadata(path).ok().map(|m| m.len())
    }

    /// 真实日志的路径，**只用于断言它没被碰**——不写它。
    fn real_log_path() -> std::path::PathBuf {
        log_path_in(std::env::var("APPDATA").ok().as_deref())
            .unwrap_or_else(|| std::path::PathBuf::from("__no_appdata__"))
    }
}

#[cfg(test)]
mod rotation_tests {
    use super::*;

    /// 判据是**体积真的换了一代**且旧内容不在新文件里——
    /// 只看"文件变小了"会把"追加失败"也误判成轮转成功。
    #[test]
    fn an_oversized_log_is_replaced_not_appended_to() {
        let dir = std::env::temp_dir().join("qb-diag-rotate");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.log");
        // 造一个已经超过上限的旧日志
        std::fs::write(&path, vec![b'x'; MAX_BYTES as usize + 16]).unwrap();

        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        // 直接调轮转再追加，模拟 note() 的顺序
        drop(f);
        rotate_if_big(&path);
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        f.write_all(b"fresh\n").unwrap();
        drop(f);

        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "fresh\n", "旧内容必须已经轮转走");
        assert!(
            path.with_extension("log.1").exists(),
            "上一代要留成 app.log.1"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
