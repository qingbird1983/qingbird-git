// **必须是内层属性，且必须在任何 item（含 `//!` 文档注释）之前**——
// 放错了编译器报 "`#![...]` attributes cannot be applied here"。
//
// **为什么需要**：`windows_subsystem` 决定 PE 头里的 `Subsystem` 字段。
// 不写这一行，Rust 默认编出 **console（3）** 子系统程序，Windows 会在它启动时
// **分配一个控制台**——从资源管理器双击、或从开始菜单点，就是一个黑终端窗口
// 跟着界面一起活到关窗口。安装版装完启动必带黑框，主因就是这里（阶段 25）。
//
// **为什么用 `cfg_attr(not(debug_assertions))` 而不是无条件写死**：
// release 是用户装的那份，要 GUI 子系统（无黑框）；**debug 要保留控制台**，
// 因为它才是我自己的验收产物——启动即崩、panic 栈、`eprintln!` 诊断
// 全靠那个控制台（`scripts/steps.py` 抓帧、`uia_probe.py`、启动探针都拉它）。
// 无条件写死的话，debug 里 panic 消息会**直接丢失**，"闪退"就变成一件查不了的事。
// 这也是 Tauri 等 GUI 框架的通行写法。
//
// ⚠️ 连带后果记在这：release 走 GUI 子系统后，`tray.rs` / `scheduler.rs` 里
// 那几处 `eprintln!` 在**用户手上那份安装版里不再有任何去处**。
// debug 构建仍然看得见。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! 青鸟 Git · GPUI Kit 版入口。
//!
//! 主题整份来自 gui/theme.json（设计规范板的灰白 token）——改配色只动这一个文件。

mod actions;
mod app_assets;
mod changes;
mod cli_tab;
mod detail;
mod diag;
mod dialogs;
mod drag_drop;
mod history_tab;
mod inspector;
mod keys;
mod log;
mod ops;
mod os;
mod overview;
mod palette;
mod panels;
mod readme_tab;
mod repo_head;
mod scan_cache;
mod scheduler;
mod self_update;
mod shell;
mod sidebar;
mod space_tab;
mod state;
mod theme;
mod tokens;
mod tray;
mod widgets;

use gpui_kit::*;

fn main() {
    // **必须第一行**：GUI 子系统没有 stderr，panic 消息默认无处可去。
    // 装上之后每一处 panic 都会落到 `%APPDATA%\com.qingbird.git\app.log`，
    // 见 `diag` 模块头。晚于任何可能 panic 的代码装就等于没装
    diag::install_panic_hook();

    // **必须用带 `AllAssets` 的 `AppAssets` 而不是裸 `Assets`。**
    // `Assets` 只内嵌基座挑的 104 个默认图标，而这一屏要用到的
    // `git-branch` / `git-merge` / `users` 都不在那 104 个里——用 `Assets`
    // 时它们能编译通过（`IconName` 枚举是全量的），运行时却取不到图，
    // 界面上就是一个空白方块。`AppAssets` = `AllAssets` 再加一张应用位图，
    // 见 `app_assets` 模块头
    gpui_kit::application()
        .with_assets(crate::app_assets::AppAssets)
        // **关窗不退出（Q11）。** 默认档 `QuitMode::Default` 在非 macOS 上
        // **等于** `LastWindowClosed`——最后一个窗口关掉进程就结束（`gpui-pre`
        // `app.rs:2922-2926`）。改成 `Explicit`：点 X 仍然销毁窗口
        // （Windows 上没有拦截关闭的钩子，也不需要拦截），但进程留着，
        // 托盘还能把它叫回来。
        //
        // ⚠️ 这一行与 `tray::start` 必须是同一次改动：只设它而没有托盘，
        // 窗口一关应用就成了没有任何入口的幽灵，比直接退出更糟
        .with_quit_mode(gpui_kit::QuitMode::Explicit)
        .run(|cx: &mut App| {
            // Windows 上没打包身份的应用发不出系统通知，官方要求尽早设。
            // 装成 NSIS 包后要保证开始菜单快捷方式用同一个 AUMID。
            cx.set_app_identity("com.qingbird.git", "青鸟 Git");

            // 必须在碰任何组件之前
            gpui_kit::init(cx);

            // 主题：整份来自 gui/theme.json（内嵌，配色跟代码一起进版本）。
            // **按配置里的 `settings.theme` 取那一套**，不再是"永远拿第一套"
            // ——theme.json 的 `themes` 本来就是数组，阶段 20 加了第二套
            // `qingbird-dark` 之后，`.next()` 会让深色永远选不中。
            // 认哪几个值、以及为什么 `system` 落浅色，见 `theme` 模块头
            crate::theme::apply_saved(cx);

            // 托盘必须在**主线程**建：Windows 上托盘图标是一个隐藏 HWND，
            // 靠 gpui 那套标准消息泵（`GetMessageW` + `DispatchMessageW`）派发它的消息
            crate::tray::start(cx);

            // 后台定时自动同步（Q8）。**必须在开窗之前起**，
            // 而且它不绑窗口：窗口关掉之后它照跑，这才是"后台"。
            // 与托盘 + QuitMode::Explicit 是一套，见 gui/src/scheduler.rs 模块头
            crate::scheduler::start(cx);

            gpui_kit::open_window(crate::tray::window_options(cx), cx, |window, cx| {
                cx.new(|cx| state::Shell::new(window, cx))
            })
            .expect("打开主窗口");
        });
}
