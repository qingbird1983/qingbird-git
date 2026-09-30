//! 青鸟 Git · guise 版入口。
//!
//! 主题从 gui/theme.json（设计规范板 token）整份加载——改配色只动这一个文件。

mod shell;

use gpui::prelude::*;
use gpui::{px, size, App, Bounds, SharedString, TitlebarOptions, WindowBounds, WindowOptions};

fn main() {
    gpui::Application::new().run(|cx: &mut App| {
        // 主题必须先于任何窗口安装（guise 硬规矩）
        let theme =
            guise::Theme::from_json(include_str!("../theme.json")).expect("解析 gui/theme.json");
        theme.init(cx);

        let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("青鸟 Git")),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_window, cx| cx.new(|_cx| shell::Shell::new()),
        )
        .expect("打开主窗口");
        cx.activate(true);
    });
}
