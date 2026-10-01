//! 青鸟 Git · GPUI Kit 版入口。
//!
//! 主题整份来自 gui/theme.json（设计规范板的灰白 token）——改配色只动这一个文件。

mod shell;

use gpui_kit::assets::Assets;
use gpui_kit::component::{Theme, ThemeSet};
use gpui_kit::*;
use std::rc::Rc;

fn main() {
    gpui_kit::application().with_assets(Assets).run(|cx: &mut App| {
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

        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1280.), px(820.)), cx)),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| shell::Shell::new(window, cx)),
        )
        .expect("打开主窗口");
    });
}
