//! 系统动作：打开文件夹、开终端、开浏览器。
//!
//! 这些**不是** git 操作，所以不进 core（core 只放业务逻辑，界面相关的副作用留在界面侧）。
//! 但也不能在菜单代码里就地 `Command::new` —— 三处调用点各写一遍 `explorer` 参数，
//! 迟早有一处漏掉转义。
//!
//! Windows 专用：本应用只发 Windows 包。别的平台要做的话在这里加分支，
//! 不要让调用方写 `#[cfg]`。

use std::process::Command;

// 本应用是 GUI 程序（release 走 `windows_subsystem = "windows"`，见 main.rs），
// 但**派生**子进程时 Windows 仍会按父进程的规则给它分配控制台：控制台类程序
// （`cmd` / `powershell`）每次 spawn 都会闪一个黑框，GUI 类的 `explorer`
// 也可能跟着闪一下。加 `CREATE_NO_WINDOW` 抑制，且**只影响子进程自己**——
// 它派生出去的目标程序（`start` 拉起的编辑器/浏览器）照常有自己的窗口。
//
// ⚠️ 与 `crates/core/src/git.rs` 是同一套写法与同一个常量，两处各写一份是因为
// core 与 gui 不互相依赖（`CommandExt` 是 std，不需要 `windows` crate）。
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// 在文件管理器里打开仓库目录
pub(crate) fn open_folder(path: &str) -> Result<(), String> {
    if !std::path::Path::new(path).is_dir() {
        return Err("文件夹不存在".into());
    }
    let mut cmd = Command::new("explorer");
    cmd.arg(path);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.spawn().map(|_| ()).map_err(|e| format!("无法打开文件夹：{e}"))
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
    let mut cmd = Command::new("cmd");
    cmd.args(["/c", "start", "", path]);
    // 只藏 `cmd` 自己；`start` 拉起来的那个程序有自己的窗口，不受影响
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.spawn().map(|_| ()).map_err(|e| format!("无法打开文件：{e}"))
}

/// 在终端里打开仓库目录
///
/// 优先 Windows Terminal（`wt`），失败回退到 PowerShell。两者都不在时给一句
/// 中文结论，别把 `program not found` 抛给用户看。
///
/// ⚠️ **这里是本文件唯一故意不加 `CREATE_NO_WINDOW` 的地方**：用户点的就是
/// 「开终端」，要的就是一个**看得见**的终端窗口——加上它这个功能等于作废。
/// 这条「全局抑制」的纪律有个例外，判据是「这颗操作要交付给用户的东西
/// 本身是不是一个窗口」。
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
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "start", ""]).arg(url);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.spawn().map(|_| ()).map_err(|e| format!("无法打开浏览器：{e}"))
}

// ================= 选文件夹（阶段 21-2） =================

/// 拉起系统文件夹选择器。**取消返回 `Ok(None)`**，不是 `Err`。
///
/// ## 为什么走 `powershell` 而不是自己绑 COM
///
/// 核实过 `gpui-kit 0.7` / `gpui-component 0.7` / `gpui-pre 0.3.7` 三处，
/// **都没有任何文件选择 API**（`file_picker` / `FilePicker` / `select_folder` /
/// `IFileDialog` 全部零命中）——是确认没有，不是没找到。剩下两条路：
///
/// ① **直接绑 `IFileOpenDialog`**：`windows` crate 就在依赖树里（托盘那一支带的），
///    看着最"正统"。**实写一遍发现代价被低估了**：`windows 0.62` 的绑定里
///    `IFileOpenDialog` **只有** `GetResults` / `GetSelectedItems`，
///    `Show` / `GetResult` / `SetOptions` 全在父接口 `IFileDialog` 上，
///    实例化还要自己备 CLSID/IID 与选项位运算——三个 import 直接
///    unresolved（`SIGDN_FILESYSDIRSYNTAX`、`HWND`、`S_FALSE`），
///    补齐还要再试几轮。**判据不是"能不能写出来"，是"这个对话框值不值为它啃一轮
///    COM 绑定"**——不值。
///
/// ② **`powershell` + `FolderBrowserDialog`**（采用）：系统自带、**零新增依赖**，
///    弹出来就是用户习惯的那个原生"浏览文件夹"树形选择器。
///    实测拉起正常、阻塞等待选择，`ShowDialog()` 的
///    `DialogResult::OK` / `Cancel` 两态原样带回来。
///
/// ⚠️ **代价**：多一个 powershell 子进程（约 200–400ms 启动）。
/// 这是**低频操作**（克隆仓库，一次几十秒起），换掉一整轮 COM 绑定值得。
/// 若哪天变成高频路径，该换成方案 ①，而不是去优化这个子进程。
///
/// ## 取消为什么不算错误
///
/// 用户按「取消」是**一个正常决定**，不是"出错了"。返回 `Err` 会让调用方
/// 弹一条红色报错，或者更糟——把对话框当成失败、连带把已经填好的地址清掉。
/// 三态（成功 / 取消 / 失败）里取消必须自己一档。
///
/// ⚠️ **必须在 UI 线程调**：`ShowDialog()` 自己起模态消息循环，会阻塞这一帧。
/// 调用点在 `Render::render` 兑现那一步（那里就是 UI 线程）；
/// **不要为了"别卡住界面"把它挪进 `background_executor`**——
/// 那样就没有任何线程是 STA，powershell 子进程自己会处理这件事，
/// 但**调用方**再想拿 `&mut Window` 回填输入框就已经晚了（见 clone.rs）。
pub(crate) fn pick_folder(initial: &str) -> Result<Option<String>, String> {
    // PowerShell 单引号串里唯一的转义是 `'` -> `''`，与 `open_terminal` 同一套。
    // 路径是本地仓库目录、不含用户输入，但仍然走转义而不是直接拼
    let init = if initial.is_empty() || !std::path::Path::new(initial).is_dir() {
        String::new()
    } else {
        let quoted = initial.replace('\'', "''");
        format!("$d.SelectedPath = '{quoted}'; ")
    };

    // 脚本自己判三态并只吐一个标记行。**不能用退出码区分**：取消与失败
    // 在 powershell 里都可能是 exit 0。用三个不同的标记字符串，外面按
    // **标记**分派——判据集中在读 stdout 的这一处，不散在两处
    let script = format!(
        "$ErrorActionPreference='Stop'; try {{ \
         Add-Type -AssemblyName System.Windows.Forms; \
         $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
         {init}\
         $r = $d.ShowDialog(); $p = $d.SelectedPath; $d.Dispose(); \
         if ($r -eq [System.Windows.Forms.DialogResult]::OK) {{ Write-Output ('QB_PICKED:' + $p) }} \
         else {{ Write-Output 'QB_CANCELED' }} \
         }} catch {{ Write-Output 'QB_FAILED'; exit 1 }}"
    );

    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-STA", "-Command", &script]);
    // 这条尤其要加：它会**阻塞 200–400ms** 等用户在模态对话框里选，
    // 不藏的话整个这段时间都挂着一个 powershell 黑框在界面上
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = cmd
        .output()
        .map_err(|e| format!("打不开系统选择器：{e}"))?;

    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("QB_"))
        .unwrap_or("");

    if let Some(p) = line.strip_prefix("QB_PICKED:") {
        if p.is_empty() {
            return Err("没拿到选中的目录".into());
        }
        Ok(Some(p.to_string()))
    } else if line == "QB_CANCELED" {
        // 取消：正常决定，见上面「取消为什么不算错误」
        Ok(None)
    } else {
        Err("系统选择器没正常返回".into())
    }
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
