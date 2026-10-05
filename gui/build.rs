//! 构建期：把**一份**应用图标派生成三处要用的形态。
//!
//! ## 为什么要有这个文件
//!
//! 同一个图标要出现在三个地方，而三处要的东西**不是同一种东西**：
//!
//! | 位置 | 要什么 | 怎么来 |
//! |---|---|---|
//! | 窗口标题栏左上角 | PNG，走 `img()` + `AppAssets` | 直接 `include_bytes!` 原始 PNG |
//! | 系统托盘 | **原始 RGBA 字节** | `tray_icon::Icon::from_rgba` 不解任何图片格式 |
//! | 桌面 / 任务栏 / Alt-Tab | exe 里的 **`.ico` 资源** | `embed-resource` 在链接期塞进 PE |
//!
//! 三处**必须来自同一张源图**。三份各自画/各自存，就是"标题栏一个样、托盘另一个样、
//! 桌面又是第三个样"——那正是 2026-10-03 之前的状况（D39）。
//!
//! ## 为什么在**构建期**解 PNG，而不在运行期拖一个图像解码依赖
//!
//! 运行期解 PNG 要给 `qingbird-gui` 加 `image`（约 1 MB 体积 + 一次解码），
//! 而**它只是为了把一张固定的图变成固定的字节**。放到构建期做，运行期就只剩
//! `include_bytes!`——**主依赖表一个图像库都不加**。
//!
//! 而 `image` 与 `embed-resource` **本来就在 `Cargo.lock` 里**（`image` 是
//! `gpui-pre` 的传递依赖，`embed-resource` 是其链上带进来的），所以它们进
//! `[build-dependencies]` 不会带来任何额外的编译成本。
//!
//! ## 尺寸
//!
//! - 托盘给 **32×32**：`tray_icon` 建的位图由系统按任务栏密度缩
//!   （100% 是 16px、150% 是 24px），32 够两边缩，再大只是白占内存
//! - `.ico` 给 **16/24/32/48/64/128/256** 七档：资源管理器小格 16、大格 32、
//!   资源管理器"大图标"视图 48/64、安装器 128、任务栏高 DPI 256。
//!   **缺档时 Windows 会拿最近的那档硬放大**，糊成一团
//!
//! ## 换图标时只改 `assets/app-icon.png`
//!
//! 这个文件是唯一的真源。改完重新 `cargo build` 即可，**不要**再去手改
//! 托盘那段绘制代码——那套已经删了（见 `tray.rs` 的模块头）。

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::FilterType;
use image::{ExtendedColorType, RgbaImage};
use std::path::{Path, PathBuf};

/// 源图。**唯一真源**——标题栏那枚、托盘那枚、桌面那枚全从它派生
const SRC_PNG: &[u8] = include_bytes!("assets/app-icon.png");

/// 源图必须是 128×128（`app_assets.rs` 里那条单测也钉着这个尺寸）
const SRC_EDGE: u32 = 128;

/// 托盘位图边长。`tray_icon` 不解码，系统按任务栏密度缩，32 够 16 与 24 两档
const TRAY_EDGE: u32 = 32;

/// `.ico` 里的七档。**缺档 = Windows 拿最近那档硬放大**
const ICO_EDGES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon.png");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR 未设置"));

    let src = image::load_from_memory(SRC_PNG).expect("解不开 assets/app-icon.png");
    let rgba = src.to_rgba8();
    assert_eq!(
        (rgba.width(), rgba.height()),
        (SRC_EDGE, SRC_EDGE),
        "源图标必须是 {SRC_EDGE}×{SRC_EDGE}"
    );

    // ① 托盘要的原始 RGBA。`Lanczos3` 是缩小时的默认选择：
    // 这张图有大量细线（分支节点、"Git" 字样），双线性会把它们糊掉
    let tray = resize(&rgba, TRAY_EDGE);
    write(&out.join("app-tray.rgba"), tray.as_raw());

    // ② exe 资源要的多尺寸 .ico。条目内部存 PNG（Vista+ 全支持）
    let mut ico: Vec<u8> = Vec::new();
    let frames: Vec<IcoFrame<'_>> = ICO_EDGES
        .iter()
        .map(|&edge| {
            let im = resize(&rgba, edge);
            IcoFrame::as_png(im.as_raw(), edge, edge, ExtendedColorType::Rgba8)
                .unwrap_or_else(|e| panic!("编不了 {edge}×{edge} 的 ico 条目：{e}"))
        })
        .collect();
    IcoEncoder::new(&mut ico)
        .encode_images(&frames)
        .expect("编不了 .ico");
    let ico_path = out.join("app.ico");
    write(&ico_path, &ico);

    // ③ 嵌进 exe —— 桌面图标、任务栏、Alt-Tab 全部从这里来
    //
    // ⚠️ `embed_resource::compile()` 吃的是 **`.rc` 脚本**，不是 `.ico`。
    // 它把参数原样交给 `rc.exe` 编译（`windows_msvc.rs:36`），rc.exe 见到二进制
    // 就当"没有资源语句"处理，**不报错、只产出一个零条目的空 `.res`**——
    // 构建全程绿灯，exe 里却一个图标都没有，取出来的是 Windows 通用蓝白图标。
    // 所以必须先落一份真的 rc 脚本，参数名才不会骗人。
    let rc_path = out.join("app.rc");
    write(&rc_path, rc_script(&ico_path).as_bytes());
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .expect("图标没能嵌进 exe");
}

/// 拼出 rc 脚本正文。
///
/// - 资源 ID 用**序号 1**：桌面/任务栏取的是 exe 里的第一个 `RT_GROUP_ICON`，
///   ID 叫什么无所谓，序号最省事
/// - 路径**一律换成正斜杠**：`\` 在 rc 的预处理里是续行符，转义一次就够踩坑，
///   正斜杠 `rc.exe` / `windres` / `llvm-rc` 三家都吃
fn rc_script(ico: &Path) -> String {
    let p = ico.to_string_lossy().replace('\\', "/");
    format!("1 ICON DISCARDABLE \"{p}\"\n")
}

fn resize(src: &RgbaImage, edge: u32) -> RgbaImage {
    if src.width() == edge && src.height() == edge {
        return src.clone();
    }
    image::imageops::resize(src, edge, edge, FilterType::Lanczos3)
}

fn write(p: &Path, bytes: &[u8]) {
    if let Err(e) = std::fs::write(p, bytes) {
        panic!("写不到 {}：{e}", p.display());
    }
}
