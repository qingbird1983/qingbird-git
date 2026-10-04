//! 青鸟 Git · GPUI Kit 版入口。
//!
//! 主题整份来自 gui/theme.json（设计规范板的灰白 token）——改配色只动这一个文件。

mod actions;
mod app_assets;
mod changes;
mod detail;
mod dialogs;
mod history_tab;
mod inspector;
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
mod shell;
mod sidebar;
mod space_tab;
mod state;
mod tokens;
mod tray;
mod widgets;

use gpui_kit::component::{Theme, ThemeSet};
use gpui_kit::*;
use std::rc::Rc;

fn main() {
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

            // 主题：把 theme.json 里那套灰白 token 应用到全局。
            // 刻意用 include_str! 内嵌——配色跟代码一起进版本，不依赖磁盘上有没有这个文件。
            let set: ThemeSet =
                serde_json::from_str(include_str!("../theme.json")).expect("解析 gui/theme.json");
            let cfg = Rc::new(
                set.themes
                    .into_iter()
                    .next()
                    .expect("gui/theme.json 里至少要有一套主题"),
            );
            Theme::update(cx, |t| t.apply_config(&cfg));

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
