//! 系统动作：打开文件夹、开终端、开浏览器。
//!
//! 这些**不是** git 操作，所以不进 core（core 只放业务逻辑，界面相关的副作用留在界面侧）。
//! 但也不能在菜单代码里就地 `Command::new` —— 三处调用点各写一遍 `explorer` 参数，
//! 迟早有一处漏掉转义。
//!
//! Windows 专用：本应用只发 Windows 包。别的平台要做的话在这里加分支，
//! 不要让调用方写 `#[cfg]`。

use std::process::Command;

/// 在文件管理器里打开仓库目录
pub(crate) fn open_folder(path: &str) -> Result<(), String> {
    if !std::path::Path::new(path).is_dir() {
        return Err("文件夹不存在".into());
    }
    Command::new("explorer")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("无法打开文件夹：{e}"))
}

/// 用**系统默认程序**打开一个文件（README 里的相对链接走这里）
///
/// 与 `open_folder` 的区别是**不传 `explorer` 的目录参数**，而是把文件本身
/// 交给 shell 的文件关联（`cmd /c start "" <路径>`）：`.md` 落到用户装的
/// 编辑器上，`.pdf` 落到阅读器上。少那个空串的话，路径里带 `&` 会被 `cmd`
/// 当命令连接符截断（与 `open_url` 同一个坑）
pub(crate) fn open_path(path: &str) -> Result<(), String> {
    if !std::path::Path::new(path).is_file() {
        return Err("文件不存在".into());
    }
    Command::new("cmd")
        .args(["/c", "start", "", path])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("无法打开文件：{e}"))
}

/// 在终端里打开仓库目录
///
/// 优先 Windows Terminal（`wt`），失败回退到 PowerShell。两者都不在时给一句
/// 中文结论，别把 `program not found` 抛给用户看。
pub(crate) fn open_terminal(path: &str) -> Result<(), String> {
    if !std::path::Path::new(path).is_dir() {
        return Err("文件夹不存在".into());
    }
    // PowerShell 单引号串里唯一的转义是 ' -> ''。路径是本地仓库目录，
    // 不含用户输入，但仍然走转义而不是直接拼——将来有人传参数进来就会出事
    let quoted = path.replace('\'', "''");
    let script = format!("Set-Location -LiteralPath '{quoted}'");

    if Command::new("wt").arg("-d").arg(path).spawn().is_ok() {
        return Ok(());
    }
    Command::new("cmd")
        .args(["/C", "start", "powershell", "-NoExit", "-Command"])
        .arg(&script)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("无法打开终端：{e}"))
}

/// 用系统默认浏览器打开一个 http(s) 地址
///
/// 走 `cmd /C start` 而不是直接 `Command::new(url)`：后者在 Windows 上会
/// 把 URL 当成可执行文件去 CreateProcess。`start ""` 的第一个空串是窗口标题占位，
/// 少了它地址里带 `&` 就会被 `cmd` 当命令连接符截断。
pub(crate) fn open_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("不是可打开的网址".into());
    }
    Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("无法打开浏览器：{e}"))
}

// ================= 剪贴板 =================
//
// 本文件**不再**提供 `copy_text`（阶段 11 收尾复查时删掉）。原先这里 spawn 了
// 一个 `clip` 子进程，还挂着 `#[allow(dead_code)]`，注释说是"留给没有窗口上下文的
// 场合"——但那句话是**错的**：唯一那个调用点（命令卡的「复制命令」）闭包里
// `window` 与 `cx` 都在手边，和仓库头那颗「复制远程地址」、右键菜单的
// 「复制远程地址」完全一样。三个复制入口现在统一走官方
// `cx.write_to_clipboard(ClipboardItem::new_string(..))`：
// 少一个子进程，也少一份"哪个入口该用哪个"的分叉。
//
// **别再加回来。** 官方剪贴板在这三个地方都能用，"没有窗口上下文"这个前提
// 在本项目里不成立。
