//! 资产源：在基座的 `AllAssets` 之外多认一张应用位图。
//!
//! **为什么不用 `Icon` 走 SVG 那条路。** 品牌图标是一张 PNG 位图
//! （老 Tauri 版 `src-tauri/icons/icon.ico` 里 6 个条目中最大的那张），
//! 不是矢量图。`gpui_kit::component::Icon` 只会 `svg().data(..)`，
//! 喂 PNG 进去是空白方块。
//!
//! **为什么不加 `include_bytes!` 之外的解码依赖。** `img()` + 内嵌资源这条路
//! 上，PNG 的解码是 gpui 自己做的（`elements/img.rs:669` 的
//! `image::guess_format` → `PngDecoder`，走的是 `image` crate，**已经
//! 在 gpui 的依赖树里**）。我们要做的只是把字节交给它：
//! `img("icons/app-icon.png")` → `Resource::Embedded` → `AssetSource::load`
//! → 拿到字节 → 解码 → 渲染。多拖一个图像解码依赖进来换一张图，不划算。
//!
//! **为什么图要拷进 `gui/assets/` 而不是 `include_bytes!` 直接指
//! `src-tauri/icons/`。** `src-tauri/` 当时是迁移源，早晚要整体退役（路线表 Q6）。
//! 让新 GUI 的构建去 `include_bytes!` 一个"准备删掉的目录"里的文件，
//! 等于把退役日期焊死在构建脚本上。这张图在这里只有一份真源
//! （源图 `assets/icon-source.png` 展开出的 128px 那张）。

use gpui_kit::assets::AllAssets;
use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

/// 资产路径。`img()` 拿到的字符串必须与 `load` 里比对的完全一致
pub(crate) const APP_ICON_ASSET: &str = "icons/app-icon.png";

/// 应用图标本体。老 Tauri 版 `icon.ico` 里那张 128×128 的 PNG
static APP_ICON_PNG: &[u8] = include_bytes!("../assets/app-icon.png");

/// 图标尺寸自检。这张图是 128×128，不是就别内嵌了——
/// 尺寸不对时 `img()` 会照样渲染，只是糊或者被裁，而那一眼看不出来
#[cfg(test)]
mod tests {
    use super::*;

    /// PNG 头 8 字节签名 + IHDR 里的宽高（大端 u32）
    const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    #[test]
    fn app_icon_is_a_128px_png() {
        assert!(APP_ICON_PNG.len() > 24, "字节太短，不像 PNG");
        assert_eq!(&APP_ICON_PNG[..8], &PNG_SIG, "不是 PNG");
        // IHDR：8 字节签名 + 4 长度 + 4 类型 = 偏移 16 起是宽高
        assert_eq!(&APP_ICON_PNG[12..16], b"IHDR", "第一个块不是 IHDR");
        let w = u32::from_be_bytes(APP_ICON_PNG[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(APP_ICON_PNG[20..24].try_into().unwrap());
        assert_eq!((w, h), (128, 128), "应用图标必须是 128×128");
    }
}

/// 只比 `AllAssets` 多一条路径的资产源。
///
/// 其余一律转交：图标枚举（`IconName`）里的每一个名字都必须照旧取得到，
/// 少转一条就是界面上一个空白方块，而 `cargo check` 一点都看不出来
/// （这正是 `AllAssets` 不能换回 `Assets` 的原因）。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == APP_ICON_ASSET {
            return Ok(Some(Cow::Borrowed(APP_ICON_PNG)));
        }
        AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        AllAssets.list(path)
    }
}
