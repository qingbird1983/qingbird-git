//! 托盘图标与托盘菜单（路线表 Q11）。
//!
//! 旧 Tauri 版有两件这里没有的东西：托盘图标，以及"关掉窗口之后进程继续活着"。
//! 后者是后台自动同步（Q8）能成立的前提——没有它，定时器只在窗口开着时才跑，
//! 用户设了"每天 09:00"而晚上关了窗口，第二天早上根本不会触发。
//!
//! ## 关窗不退出靠的是什么
//!
//! **不靠拦截关闭。** gpui-pre 在 Windows 上根本没有这个钩子
//! （`TitleBar::on_close_window` 源码第一行就是 `if cfg!(target_os = "linux")`），
//! 而我们也不需要：旧 Tauri 版同样是**销毁窗口**（`window.destroy()`），
//! 它要的只是进程活着。
//!
//! 真正决定这件事的是 `QuitMode`（`gpui-pre/src/app.rs:2922-2926`）：
//! `Default` 在非 macOS 上**等于** `LastWindowClosed`（最后一个窗口关了就退出），
//! `Explicit` 才是"只在我调 `App::quit` 时退"。`main.rs` 里那一行
//! `with_quit_mode(QuitMode::Explicit)` 就是全部改动。
//!
//! ⚠️ **那行与本文件必须同一次改动上。** 只设 `QuitMode::Explicit` 而没有托盘，
//! 窗口一关应用就变成"进程活着但没有任何入口"的幽灵，比直接退出更糟。
//!
//! ## 事件怎么回到主线程
//!
//! 官方给的 `set_event_handler` 要求回调 `Send + Sync`，而 gpui 的主线程执行器
//! 带 `PhantomData<Rc<()>>`（`executor.rs:32`），**不是 `Send`**，送不进托盘线程。
//! 所以这里走另一条路：**不设 handler，用官方的 `receiver().try_recv()` 在主线程轮询**。
//! 100ms 一轮，延迟看不出来，而省掉了一整条跨线程桥。
//!
//! Windows 上托盘图标是一个隐藏 HWND，而 gpui 的消息泵是标准
//! `GetMessageW` + `DispatchMessageW`（`gpui-pre-windows/src/platform.rs:516-519`），
//! 会把那个窗口的消息一起派发掉——**托盘图标必须在主线程创建**，这也是本文件
//! 不另起线程的原因。

use gpui_kit::{App, AppContext as _, AsyncApp};
use muda::{Menu, MenuItem};
use std::cell::RefCell;
use std::time::Duration;
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};

/// 轮询周期。托盘事件没有完成通知（我们没用 handler），
/// 所以只能轮询。100ms 足够快，代价是每秒 10 次 `try_recv`
const POLL_MS: u64 = 100;

/// 托盘提示文字
const TOOLTIP: &str = "青鸟 Git — 本地仓库一键同步";

/// 托盘菜单的三个动作
const ID_SHOW: &str = "show";
const ID_SCAN: &str = "scan";
const ID_QUIT: &str = "quit";

thread_local! {
    /// `TrayIcon` 一 drop 托盘图标就没了，而它没有任何"活着的必要条件"可挂，
    /// 所以钉在**主线程**的线程局部里，进程活多久它活多久。
    ///
    /// 用线程局部而不是 `static`：`TrayIcon` 持有 Win32 资源，
    /// 而托盘本来就是在主线程建的，放线程局部最贴合它的生命周期。
    static TRAY: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
}

/// 主窗口的窗口选项。
///
/// **托盘重开窗口与首次开窗共用这一份**：`main.rs` 首次开窗、托盘"显示主窗口"
/// 在窗口已被关掉之后重开，两处给不一样的尺寸会让用户看到窗口自己变形。
pub(crate) fn window_options(cx: &App) -> gpui_kit::WindowOptions {
    gpui_kit::WindowOptions {
        window_bounds: Some(gpui_kit::WindowBounds::centered(
            gpui_kit::size(gpui_kit::px(1280.), gpui_kit::px(820.)),
            cx,
        )),
        // 1080×640 是设计规范第 1 节算出来的下限：轨 56 + 侧栏 200 之后
        // 主区还有 824px，抽屉展开时再减 320 也剩 504px。
        window_min_size: Some(gpui_kit::size(
            gpui_kit::px(crate::tokens::WIN_MIN_W),
            gpui_kit::px(crate::tokens::WIN_MIN_H),
        )),
        // 官方 TitleBar 自绘窗口控件并接管拖拽区，所以窗口选项必须以它为底。
        // 保留系统装饰的话会同时出现两套关闭按钮（`WindowControls` 自己的注释
        // 里就写了这件事），而且拖不动窗口。
        ..gpui_kit::component::TitleBar::window_options()
    }
}

/// 建托盘并起那条轮询任务。**必须在主线程调用**（托盘的隐藏 HWND 归主线程消息泵管）
pub(crate) fn start(cx: &mut App) {
    let menu = Menu::new();
    // 「显示主窗口」必须给显式 id：不给的话 muda 自动生成一个，
    // 菜单这一路就永远匹配不上它，点了没反应——是个死菜单项
    let show = MenuItem::with_id(
        ID_SHOW,
        "显示主窗口",
        true,
        None::<muda::accelerator::Accelerator>,
    );
    let scan = MenuItem::with_id(
        ID_SCAN,
        "一键扫描全部仓库",
        true,
        None::<muda::accelerator::Accelerator>,
    );
    let quit = MenuItem::with_id(
        ID_QUIT,
        "退出",
        true,
        None::<muda::accelerator::Accelerator>,
    );
    for item in [&show, &scan, &quit] {
        if let Err(e) = menu.append(item) {
            // 托盘菜单建不起来不算致命：图标还在，只是不带菜单
            crate::diag::note(&format!("托盘菜单项加入失败：{e}"));
        }
    }

    // 图标建不出来（尺寸不合法之类）不该让整个托盘没掉——
    // 没图标的托盘仍然能用，只是不好看，比什么都没有强
    let icon = Some(tray_icon());
    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(TOOLTIP)
        // 左键是"显示/唤起"，不是弹菜单——菜单留给右键
        .with_menu_on_left_click(false);
    if let Some(icon) = icon {
        builder = builder.with_icon(icon);
    }
    let built = builder.build();

    match built {
        Ok(tray) => TRAY.with(|t| *t.borrow_mut() = Some(tray)),
        Err(e) => {
            // 建不出托盘就不能设 QuitMode::Explicit，否则就成了没有入口的幽灵。
            // 这条只能记日志——此刻界面还没开，没有任何地方能报给用户。
            // `diag::note` 落到 `%APPDATA%\com.qingbird.git\app.log`：
            // release 是 GUI 子系统，**没有 stderr**，`eprintln!` 会凭空消失
            crate::diag::note(&format!("托盘图标创建失败：{e}"));
            return;
        }
    }

    cx.spawn(async move |cx: &mut AsyncApp| loop {
        cx.background_executor()
            .timer(Duration::from_millis(POLL_MS))
            .await;
        drain(cx);
    })
    .detach();
}

/// 排空两路事件队列
fn drain(cx: &mut AsyncApp) {
    // 菜单事件要排在图标事件前面：先看有没有"退出"，
    // 免得同一轮里又开窗又退出
    while let Ok(ev) = muda::MenuEvent::receiver().try_recv() {
        let id = ev.id();
        if id == ID_QUIT {
            // `AsyncApp` 上没有 `quit`，退到 `App` 上去调
            cx.update(|cx: &mut App| cx.quit());
            return;
        }
        if id == ID_SCAN {
            rescan(cx);
        }
        if id == ID_SHOW {
            show_or_open(cx);
        }
    }
    while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
        // 左键单击：窗口在就唤起，不在就重开。与旧 Tauri 版同一条规则
        if let TrayIconEvent::Click {
            button: tray_icon::MouseButton::Left,
            button_state: tray_icon::MouseButtonState::Up,
            ..
        } = ev
        {
            show_or_open(cx);
        }
    }
}

/// 唤起已有窗口，或在已被关掉的情况下重开一个
fn show_or_open(cx: &mut AsyncApp) {
    let existing = cx.update(|cx: &mut App| cx.windows());
    if let Some(w) = existing.first().copied() {
        // `update_window` 失败说明窗口刚好在这一瞬间被关了，
        // 退回"重开"而不是什么都不做
        if cx
            .update_window(w, |_, window, _| {
                // **只 `activate_window()`**。`Window` 上没有 `is_minimized`
                // 也没有 `unminimize_window`（只有 `minimize_window` /
                // `zoom_window` / `is_maximized`），而 `zoom_window` 是"最大化
                // ↔ 还原"的意思，对一个普通窗口调它会把窗口放大——那是错的。
                // Windows 上 `activate_window` 走 `SetForegroundWindow`，
                // 最小化的窗口通常会被一并恢复；万一没恢复，用户再点一次
                // 任务栏即可，代价远小于把窗口莫名其妙最大化。
                window.activate_window();
            })
            .is_ok()
        {
            return;
        }
    }
    open_main_window(cx);
}

/// 新开一个主窗口
fn open_main_window(cx: &mut AsyncApp) {
    let opts = cx.update(|cx: &mut App| window_options(cx));
    if let Err(e) = cx.open_window(opts, |window, cx| {
        cx.new(|cx| crate::state::Shell::new(window, cx))
    }) {
        crate::diag::note(&format!("重开主窗口失败：{e}"));
    }
}

/// 托盘「一键扫描全部仓库」：**有窗口就刷它，没有窗口就先开一个**。
///
/// 扫描本身走 `Shell::refresh_status`——那是窗口实体的方法，托盘这边
/// 不自己派生 git 子进程（那会绕过 `Shell` 里那套 `seq` 代数守卫，
/// 两个扫描会打架）。
///
/// ⚠️ 早先这里写的是"有窗口就什么都不做"，理由是"用户正在看，别突然刷一层"。
/// **那是错的，而且方向正好反了**：托盘点"扫描"绝大多数时候窗口是开着的，
/// 那一版等于让这个菜单项在常见场景下**完全没反应**。旧 Tauri 版是
/// `show_main` + 推一个 `scan-all` 事件，窗口在不在都做事。
/// 刷新是用户主动点的，界面变化是他要的，不存在"突然"——
/// 托盘不点的用户不会因为托盘被点了一下而受影响
fn rescan(cx: &mut AsyncApp) {
    let existing = cx.update(|cx: &mut App| cx.windows());
    match existing.first().copied() {
        Some(w) => {
            let _ = cx.update_window(w, |_, window, cx| {
                if let Some(Some(shell)) = window.root::<crate::state::Shell>() {
                    shell.update(cx, |shell, cx| shell.refresh_status(cx));
                }
            });
        }
        // 窗口都没有，用户点了"扫描"就是想看结果——先把窗口叫出来。
        // `Shell::new` 本身就会做一轮初始扫描，所以这里不用再补一次
        None => open_main_window(cx),
    }
}

/// 托盘位图的边长。`tray_icon` 不解码图片、系统按任务栏密度缩放，
/// 32 够 100%（16px）与 150%（24px）两档，再大只是白占内存。
const TRAY_EDGE: u32 = 32;

/// 托盘图标的像素。**由 `build.rs` 从 `assets/app-icon.png` 缩到 32×32 生成**，
/// 与窗口标题栏左上角那一枚、桌面图标**同一张源图**。
static TRAY_ICON_RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app-tray.rgba"));

/// 建托盘图标。
///
/// **为什么不再手绘**（2026-10-05 定）：以前这里是**程序化画**的
/// "品牌色圆底 + 白色 git 分支"——理由是"托盘只有 16px，那张位图缩下来是一坨"。
/// 代价是**托盘图标与标题栏那颗长得不一样**（一个是墨底箱子，一个是青蓝圆底分支），
/// 而它们本该是同一个软件的同一张脸。
///
/// 现在改成**一份源图派三份**（`build.rs`）：标题栏读原始 PNG、托盘读缩到 32 的
/// RGBA、桌面读多尺寸 `.ico` 资源。三处永远一致，换图只改 `assets/app-icon.png`。
///
/// ⚠️ 源图带**深色底**。托盘浮在浅色任务栏上时它是一块深色小方块——
/// 这是"三处同一个图标"必然带来的观感，不是画错了。**真要按任务栏深浅切两套，
/// 得先有第二张浅底图**，而不是在托盘上另画一个。
fn tray_icon() -> tray_icon::Icon {
    tray_icon::Icon::from_rgba(TRAY_ICON_RGBA.to_vec(), TRAY_EDGE, TRAY_EDGE)
        .expect("托盘图标字节数与 32×32 对不上（build.rs 没跑？）")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 钉住"build.rs 真的产出了 32×32×4 字节"。
    /// 少了这条，`from_rgba` 会在**运行期启动那一瞬**才报字节数不对——
    /// 症状是"程序起来但托盘空着"，而那时日志已经被窗口占住了
    #[test]
    fn tray_rgba_is_a_32x32_buffer() {
        assert_eq!(
            TRAY_ICON_RGBA.len(),
            (TRAY_EDGE * TRAY_EDGE * 4) as usize,
            "托盘字节数不对，build.rs 那一步多半没跑"
        );
    }

    /// 钉住"这张图是**满幅不透明**的"——而这条是**一条反直觉的事实**，
    /// 2026-10-05 第一次写这条断言时按"圆角方块 = 四角透明"写的，当场红了。
    ///
    /// `assets/app-icon.png` 的四角 alpha 是 **252**（不是 0）：那个圆角是
    /// **画在图里的**——白底 + 圆角深墨方块，整张满幅不透明。
    /// 后果有三件，都得记着：
    /// ① 托盘上是**一块 16px 的深色方块**（不是圆底），这与"三处统一"的
    ///    要求一致，见 `tray_icon` 那段注释
    /// ② **深色主题下标题栏那枚会露出白色四角**（缩到 22px 后白边只剩一两像素，
    ///    浅色主题上看不出来）
    /// ③ 想要"透明圆角"得**先有一张真正带 alpha 的源图**，
    ///    不能在托盘这头做遮罩——那会让三处又不一致了
    #[test]
    fn tray_icon_is_opaque_edge_to_edge() {
        let n = TRAY_EDGE as usize;
        let at = |x: usize, y: usize| TRAY_ICON_RGBA[((y * n + x) * 4 + 3) as usize];
        for &(x, y) in &[(0, 0), (n - 1, 0), (0, n - 1), (n - 1, n - 1)] {
            assert!(at(x, y) >= 200, "({x},{y}) alpha={}，源图不该有透明角", at(x, y));
        }
        assert!(at(n / 2, n / 2) >= 250, "圆心该是实底");
    }

    /// 钉住"它不是一张全透明的空图"。
    ///
    /// 这是 D39 那条教训的正向版本：当年手绘的托盘图标渲出来是个对话气泡，
    /// 而判据（长度对、alpha 对、不 panic）**全部通过**。
    /// 唯一能分开"画对了"与"画歪了"的，是**把产物本身看一眼**——
    /// 而这里能落成一条机械判据的是"确实有内容、且深浅两色都在"。
    #[test]
    fn tray_icon_is_not_blank() {
        let px = TRAY_ICON_RGBA;
        let mut opaque = 0usize;
        let mut min = u8::MAX;
        let mut max = 0u8;
        for c in px.chunks_exact(4).filter(|c| c[3] > 200) {
            opaque += 1;
            min = min.min(c[0]);
            max = max.max(c[0]);
        }
        // 32×32 的圆角方块大约铺掉 90% 以上；给足余量防 Lanczos 的边角
        assert!(opaque > (TRAY_EDGE as usize) * (TRAY_EDGE as usize) * 3 / 4,
            "几乎全透明 —— 托盘上会是空的（不透明像素 {opaque}）");
        // 源图有深色底 + 青色箱体，红通道上两者差得开
        assert!(max - min > 60, "只有一个色阶，多半是一块纯色（红通道 {min}..{max}）");
    }
}
