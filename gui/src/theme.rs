//! 主题的**选取与应用**（阶段 20：深色）。
//!
//! `gui/theme.json` 里的 `themes` **本来就是数组**——多套配色是这格式自带的
//! 机制，之前只有一项 `qingbird-light` 而已。启动那段代码写的是
//! `.themes.into_iter().next()`，也就是"永远拿第一套"，加第二套进去也不会
//! 被读到。这里只做一件事：**按名字取那一套**。
//!
//! ## 为什么单独一个模块而不是在 main.rs 里写完
//!
//! 因为"取哪一套"这件事**不止启动时要做一次**：设置里那颗开关要能在运行期
//! 换。两个地方各写一遍 `.into_iter().find(..)`，改配色名的时候必有一处漏。
//!
//! ## 配色内嵌，不读磁盘
//!
//! 刻意用 `include_str!`——配色跟代码一起进版本，不依赖 exe 旁边有没有那个
//! json 文件（用户装的是单文件 exe）。
//!
//! ## 认哪几个值
//!
//! `store::Settings.theme` 是 `dark` / `light` / `system` 三档（老 Tauri 版留下
//! 的字段）。**这一版只认前两档**，`system` 与空串一律落浅色。理由：
//!
//! * GPUI 这一侧从来没写过这个字段，真实配置里存的是 `"light"`——
//!   `system` 只是老代码的注释里写着的一个从没被用过的分支；
//! * 设置页那颗开关是**两态**的，三态要另配一个分段控件，
//!   而那属于新的一轮设计，不该顺手塞进这个阶段；
//! * 空串落浅色 = 保持今天的样子（`.next()` 拿到的就是第一套 `qingbird-light`）。

use std::rc::Rc;

use gpui_kit::App;
use gpui_kit::component::{Theme, ThemeConfig, ThemeSet};
use gpui_kit::px;
use qingbird_core::store;

use crate::tokens::UiScale;

/// `theme.json` 里浅色那套的名字
pub(crate) const LIGHT: &str = "qingbird-light";
/// `theme.json` 里深色那套的名字
pub(crate) const DARK: &str = "qingbird-dark";

/// 整份 theme.json。**每次调用重新解析**——`ThemeSet` 是一次性的
/// （取走了就把 `themes` move 走），运行期要反复取配置，就得反复解析。
/// 一份 8 KB 的 json 解析一次是微秒级，换来的是"没有全局可变状态"
fn set() -> ThemeSet {
    serde_json::from_str(include_str!("../theme.json")).expect("解析 gui/theme.json")
}

/// 按名字取一套。**取不到就落浅色**，不 panic
///
/// ⚠️ 名字对不上时**不能退到 `.next()`**：那正是旧代码的毛病——
/// "我以为拿到的是这一套"，而实际拿到的是数组里的第一套。
/// 配色改名要在这里显式改一处，而不是让所有取不到的场景静默拿浅色
pub(crate) fn config(name: &str) -> Rc<ThemeConfig> {
    let set = set();
    let fallback = || {
        Rc::new(
            set.themes
                .iter()
                .find(|c| c.name.as_ref() == LIGHT)
                .cloned()
                .expect("gui/theme.json 里必须有 qingbird-light"),
        )
    };
    match name {
        DARK => set
            .themes
            .iter()
            .find(|c| c.name.as_ref() == DARK)
            .cloned()
            .map(Rc::new)
            .unwrap_or_else(fallback),
        _ => fallback(),
    }
}

/// 应用某一套。**运行期也走这里**（设置里那颗开关）
///
/// **字号跟着界面字号档走**：官方件（Button / Input / Table）的文字读的是
/// `Theme.font_size`，而我们自己的 182 处文字读的是 `tokens::fs_*()`——
/// **两条路**。只改其中一条 ⇒ 官方件与自绘文字差一档，一屏里两种字号
/// 混着，比"整体偏小"更难看。所以 `font_size` 在这里覆写，
/// 取值就是那一档的**正文**字号（`UiScale::base_font`）。
///
/// 写法照基座故事应用那份（`story/src/title_bar.rs:125`）：
/// `Theme::update` 之后直接改 `theme.font_size`。
pub(crate) fn apply(name: &str, scale: UiScale, cx: &mut App) {
    let cfg = config(name);
    let base = px(scale.base_font());
    Theme::update(cx, |t| {
        t.apply_config(&cfg);
        t.font_size = base;
    });
}

/// 启动时用：读配置里 `settings.theme` 与 `settings.ui_scale`。
///
/// **读不到 / 是 `system` / 别的值都落浅色**，见模块头；
/// 字号档同理落中档（`UiScale::from_stored` 认不出一律给中档）
pub(crate) fn apply_saved(cx: &mut App) {
    let s = store::load().settings;
    let scale = UiScale::from_stored(&s.ui_scale);
    // 运行期载体（`tokens::fs_*()` 读它）也要设——`apply` 只管官方件那一条路
    crate::tokens::set_ui_scale(scale);
    apply(if s.theme == DARK { DARK } else { LIGHT }, scale, cx);
}
