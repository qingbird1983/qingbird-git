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
            eprintln!("托盘菜单项加入失败：{e}");
        }
    }

    // 图标建不出来（尺寸不合法之类）不该让整个托盘没掉——
    // 没图标的托盘仍然能用，只是不好看，比什么都没有强
    let icon = tray_icon::Icon::from_rgba(app_icon_rgba(), APP_ICON, APP_ICON).ok();
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
            // 这条只能记日志——此刻界面还没开，没有任何地方能报给用户
            eprintln!("托盘图标创建失败：{e}");
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
        eprintln!("重开主窗口失败：{e}");
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

/// 托盘图标尺寸。Windows 托盘在 100% 缩放下是 16px、150% 是 24px，
/// 32px 够两边缩放（系统自己缩），再大只是白占内存
const APP_ICON: u32 = 32;

/// 品牌主色，取 `gui/theme.json` 的 `accent.background`——
/// 与界面上"待推送 / 未提交"那档蓝是同一个色，托盘与主区一眼认得出是一家人
const BRAND: [u8; 3] = [0x5B, 0x7A, 0x99];

/// 托盘图标：主题色圆底 + 白色 git 分支。
///
/// **为什么是分支图而不是鸟。** 应用叫"青鸟"，但 `assets/icon-source.png`
/// 里**根本没有鸟**——logo 是一个带 git 分支的立体箱子。托盘图标要跟
/// 窗口/任务栏那一套对得上，所以取的是品牌里真正有辨识度的那部分：分支图。
/// （先按"青鸟"画了飞鸟，白白调了三轮参数，方向一开始就错了。）
///
/// **为什么带圆底。** 托盘图标浮在任务栏上，而任务栏有深浅两套。
/// 不带底的纯色字形在深色任务栏上会直接消失（试过：墨色分支、墨色箱轮廓
/// 在 `#202020` 上一点不剩）。圆底自带底色，两种任务栏都成立，
/// 也就**不必读 `AppsUseLightTheme` 注册表切深浅两套图标**。
///
/// **为什么手绘位图而不是加载 `icons/32x32.png`。** 那张图在托盘的 16px 下
/// 是一坨青蓝色块，认不出是什么；而要解 PNG 就得为一张 16px 的图拖一个
/// 图像解码依赖进二进制。托盘图标就在 16–32px 上看，矢量简化版才读得出来。
///
/// **几何一律以圆心为原点**（下面所有常数都是相对 `n` 的比例，无 `CENTER +`）。
/// 这一点是踩过的坑：第一版按"原点在左上角"写，谷底掉到画布外被裁掉，
/// 两翼正好在圆底被切断，渲出来是"圆底被啃两口"。`icon_coverage` 下面
/// 的测试就是钉这件事的。
fn app_icon_rgba() -> Vec<u8> {
    let n = APP_ICON as f32;
    let center = n / 2.0;
    let radius = n / 2.0 - 0.5;
    let curve = branch_curve(n);
    let mut out = Vec::with_capacity((APP_ICON * APP_ICON * 4) as usize);
    for y in 0..APP_ICON {
        for x in 0..APP_ICON {
            let (fx, fy) = (x as f32 - center + 0.5, y as f32 - center + 0.5);
            // 圆边抗锯齿：托盘会缩到 16px，硬边会锯得很难看
            let a = (radius - (fx * fx + fy * fy).sqrt()).clamp(0.0, 1.0);
            let cov = icon_coverage(n, fx, fy, &curve);
            let px = [
                blend(BRAND[0], cov),
                blend(BRAND[1], cov),
                blend(BRAND[2], cov),
            ];
            out.extend_from_slice(&[px[0], px[1], px[2], (a * 255.0) as u8]);
        }
    }
    out
}

/// 把字形覆盖率当白色叠在品牌色上
fn blend(base: u8, cov: f32) -> u8 {
    (base as f32 + (255.0 - base as f32) * cov).round() as u8
}

/// 笔画半宽 / 节点半径，都按尺寸取比例
const STROKE: f32 = 0.048;
const NODE: f32 = 0.100;

/// 贝塞尔采样段数。24 段在 32px 上单段不足 1.5px，肉眼看不出折线
const CURVE_STEPS: usize = 24;

/// 从上节点向右再向下弯的那条分支（`git-branch` 的标志性弧线）。
/// 直着画会读成"Γ"角标：水平出发再急转，左上角糊成一团
fn branch_curve(n: f32) -> [(f32, f32); CURVE_STEPS + 1] {
    let p0 = (-n * 0.19, -n * 0.21);
    let ctrl = (n * 0.21, -n * 0.21);
    let p2 = (n * 0.21, -n * 0.01);
    let mut out = [(0.0f32, 0.0f32); CURVE_STEPS + 1];
    for (i, slot) in out.iter_mut().enumerate() {
        let t = i as f32 / CURVE_STEPS as f32;
        let u = 1.0 - t;
        *slot = (
            u * u * p0.0 + 2.0 * u * t * ctrl.0 + t * t * p2.0,
            u * u * p0.1 + 2.0 * u * t * ctrl.1 + t * t * p2.1,
        );
    }
    out
}

/// 点到线段的距离
fn dist_to_seg(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= f32::EPSILON {
        0.0
    } else {
        (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    let (qx, qy) = (a.0 + t * dx, a.1 + t * dy);
    ((px - qx) * (px - qx) + (py - qy) * (py - qy)).sqrt()
}

/// 白色分支图的覆盖率 0..1。抗锯齿靠"半宽外扩半像素"
fn icon_coverage(n: f32, x: f32, y: f32, curve: &[(f32, f32)]) -> f32 {
    let top = (-n * 0.19, -n * 0.21);
    let bottom = (-n * 0.19, n * 0.22);
    let right = (n * 0.21, -n * 0.01);
    let mut d = dist_to_seg(x, y, top, bottom);
    for w in curve.windows(2) {
        d = d.min(dist_to_seg(x, y, w[0], w[1]));
    }
    let mut cov = (n * STROKE - d + 0.5).clamp(0.0, 1.0);
    for p in [top, bottom, right] {
        let dist = ((x - p.0) * (x - p.0) + (y - p.1) * (y - p.1)).sqrt();
        cov = cov.max((n * NODE - dist + 0.5).clamp(0.0, 1.0));
    }
    cov
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 钉住"几何必须落在画布内且大致居中"——第一版就是坐标原点用错，
    /// 字形掉到画布外被裁成"圆底被啃两口"，而 `cargo check` 一点都看不出来。
    ///
    /// 判据用**包围盒**而不是重心：竖直主干是根长条，全压在左边，
    /// 重心天生偏左约 2px（实测 13.7 vs 圆心 15.5）——那是形状本身，
    /// 不是缺陷。包围盒才对应"有没有掉出画布、有没有偏到边上"。
    #[test]
    fn icon_glyph_lands_inside_the_disc() {
        let px = app_icon_rgba();
        assert_eq!(px.len(), (APP_ICON * APP_ICON * 4) as usize);

        let (mut min_x, mut min_y) = (APP_ICON, APP_ICON);
        let (mut max_x, mut max_y) = (0u32, 0u32);
        let mut lit = 0u32;
        for y in 0..APP_ICON {
            for x in 0..APP_ICON {
                let i = ((y * APP_ICON + x) * 4) as usize;
                // 字形处已被 blend 成白色；只看亮度高于品牌色的像素
                if px[i] > BRAND[0] + 40 {
                    lit += 1;
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        // 32×32 的圆里，一个分支图大约占一到两成
        assert!(lit > 120, "字形像素只有 {lit} 个，八成是掉出画布了");
        assert!(lit < 420, "字形像素 {lit} 个，多半是糊成一片了");

        // 包围盒要四边都在圆内，且左右上下大致对称
        assert!(
            min_x >= 2 && max_x <= APP_ICON - 3,
            "包围盒 x {min_x}..{max_x} 出界"
        );
        assert!(
            min_y >= 2 && max_y <= APP_ICON - 3,
            "包围盒 y {min_y}..{max_y} 出界"
        );
        let span = (APP_ICON - 1) as f32;
        assert!(
            ((min_x + max_x) as f32 - span).abs() <= 1.0,
            "字形左右不对称"
        );
        assert!(
            ((min_y + max_y) as f32 - span).abs() <= 1.0,
            "字形上下不对称"
        );
    }

    /// 圆底必须在图内，且四角完全透明——否则托盘上会出现一个方块
    #[test]
    fn icon_corners_are_transparent() {
        let px = app_icon_rgba();
        let n = APP_ICON;
        for &(x, y) in &[(0, 0), (n - 1, 0), (0, n - 1), (n - 1, n - 1)] {
            let a = px[((y * n + x) * 4 + 3) as usize];
            assert_eq!(a, 0, "({x},{y}) 不透明");
        }
        // 圆心必须完全不透明，否则底色没铺满
        let c = ((n / 2) * n + n / 2) * 4 + 3;
        assert!(px[c as usize] >= 250, "圆心 alpha={}", px[c as usize]);
    }
}
