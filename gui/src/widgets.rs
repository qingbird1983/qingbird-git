//! 跨屏复用的小部件：分组标题、空态主区、统计卡、状态药丸、加载骨架、竖分隔线。
//!
//! 这里只放纯展示、无状态的部件；有状态的一律回各页自己的模块。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::ButtonCustomVariant;
use gpui_kit::component::empty::{Empty, EmptyContent, EmptyHeader, EmptyTitle};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::{ActiveTheme, Sizable, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, AnyElement, App, Div, FontWeight, Hsla, IntoElement, ParentElement, Styled, Window,
};
use qingbird_core::store::RepoEntry;

use crate::shell::OpenPalette;
use crate::tokens::*;

/// 全局幽灵按钮的**悬停 / 按下底色**：一层淡灰遮罩。
///
/// ## 为什么不直接用官方 `.ghost()`
///
/// 基座把 `ButtonVariant::Ghost` 的 hover 底色**写死成 `accent`**，而且浅色模式下
/// **不乘透明度**（`button.rs:1156-1163`：深色模式才 `opacity(0.5)`）。
/// 本项目 `accent.background = #5B7A99`，于是白底上悬停一颗图标键，
/// 出来的是一块深蓝灰**实底 + 白字**——2026-10-05 抓帧确认（「编辑」那颗）。
///
/// 其它变体没这个问题：`Default` 读 `button_hover`、`Secondary` 读
/// `button_secondary_hover`，两者都落到已配好的 `secondary.hover.background`。
/// **Ghost 是唯一不读 token、直接取 `accent` 的一支**，所以改 `theme.json` 改不到它。
///
/// 调用方 `.bg()` 也压不掉：官方在每个状态层末尾都会重放调用方样式，
/// 同一份底色于是**常态层也生效**，按钮会永远带一块灰底（`AGENTS.md` 已记）。
/// `.hover(..)` 更别想——`div.rs:843` 的 `debug_assert!` 让 debug 构建直接 panic。
///
/// ## 这里为什么敢用 `ButtonCustomVariant`
///
/// 官方把「换变体 = 重新设计那颗按钮」记在 `AGENTS.md` 里，指的是焦点环与
/// 描边逻辑会跟着变。这里变的**只有两层底色**：常态仍然透明、无描边，
/// 与现在的 ghost 一模一样；`foreground` 显式给成主题前景色，免得悬停那一刻
/// 图标被换成 `accent_foreground`（白字）——那正是原来那块实底难看的另一半。
///
/// ## 取值不新增颜色
///
/// 直接复用 `secondary_hover` / `secondary_active`——侧栏行悬停、表格行悬停
/// 现在用的就是这两个灰（`#F6F5F4` / `#EFEEEC`）。按钮与列表用同一档悬停灰，
/// 鼠标横扫一屏时底色是连续的；另起一个色号只会多一处需要同步的档位。
pub(crate) fn ghost_variant(cx: &App) -> ButtonCustomVariant {
    let t = cx.theme();
    // ⚠️ 取的是 `Theme` 上的**已解析**字段（`Hsla`），不是 `t.tokens.*`
    // （那是 `ThemeToken` 枚举，得先 `resolve` 才能当颜色用，E0308）
    ButtonCustomVariant::new(cx)
        // 常态：透明底，与换之前的 ghost 观感完全一致
        .color(t.transparent)
        .foreground(t.foreground)
        .hover(t.secondary_hover)
        .active(t.secondary_active)
        .shadow(false)
}

/// 主区空态：一个仓库都没选时占住中间。
/// 用官方 `Empty` 而不是手搓——它自带虚线框、图标位与三种文案层级
pub(crate) fn empty_main(t: &Theme, window: &Window) -> Div {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .p(px(GAP_LG))
        .child(
            Empty::new()
                // 外层已经有列边界了，再套一圈虚线框是两层框
                .border_0()
                .header(EmptyHeader::new().title(EmptyTitle::new().child("先选一个仓库")))
                .content(
                    EmptyContent::new().child(
                        v_flex()
                            .items_center()
                            .gap(px(GAP_TIGHT))
                            .child(
                                div()
                                    .text_size(px(FS_META))
                                    .text_color(tier4(t))
                                    .child("左边点一行就能开始"),
                            )
                            // ⌘K 唯一的提示位。D9 把标题栏那个全局搜索框删了之后，
                            // 它就没有任何可见入口了——命令面板只能靠键盘进，
                            // 而"只能靠键盘进"的东西得有个地方告诉用户它存在
                            .child(palette_hint(t, window)),
                    ),
                ),
        )
}

/// 空态里那行「找不到要的功能时按 `Ctrl K`」。
///
/// **键位从 keymap 查（官方 `Kbd::global_binding_for_action`），不写死字符串。**
/// 这一行是 ⌘K 唯一的可见入口，写死的键位在改键之后就是一句假提示——
/// 而它恰恰是全应用最不能被改错的一处提示
///
/// 查不到时**退成不带键位的纯文字**：宁可少给一个提示，也不给一个可能已经
/// 失效的键位
fn palette_hint(t: &Theme, window: &Window) -> AnyElement {
    let tail = div().text_size(px(FS_META)).text_color(tier4(t));
    match Kbd::global_binding_for_action(&OpenPalette, window) {
        Some(kbd) => h_flex()
            .items_center()
            .gap(px(GAP_TIGHT))
            .child(tail.child("找不到要的功能时按"))
            .child(kbd)
            .into_any_element(),
        None => tail.child("找不到要的功能时用命令面板").into_any_element(),
    }
}

/// 统计卡：大数字 + 标签，数字按语义着色。
///
/// **不加边框、不加阴影。** 这一屏已经有表格的横线当结构了，
/// 再给每张卡套一圈框会变成"框里套框"；四张卡靠背景色与 8px 间距分组就够。
///
/// ## 图标位（阶段 21-8）
///
/// 原来这张卡只有数字与文字，**没有图标**——所以总览页那一屏看起来
/// "图标大大小小各种风格"，其实那一处是**压根没有**。现在补上一枚
/// lucide 图标（16px），与左工具栏、导航轨是同一套线框、同一档尺寸。
///
/// ⚠️ **图标颜色跟着数字走**，不取中性灰：数字已经在用语义色表达状态
/// （已同步绿 / 落后橙 / 分叉红），图标再给一个中性色就成了两套信号，
/// 而"这颗图标的颜色和那个数字无关"是假的——它们说的是同一件事。
/// 数字为 0 时**两者一起降到 tier4**：一排彩色图标配一排 0 会显得吵，
/// 而"没东西"本来就不该有动作色。
pub(crate) fn stat_card(
    n: usize,
    label: &str,
    color: Hsla,
    icon: gpui_kit::assets::IconName,
    t: &Theme,
) -> Div {
    let tint = if n > 0 { color } else { tier4(t) };
    v_flex()
        .flex_1()
        .gap(px(GAP_TIGHT))
        .p(px(GAP_LG))
        .rounded(px(R_LG))
        .bg(t.colors.popover)
        .child(
            h_flex()
                .w_full()
                .gap(px(GAP_TIGHT))
                .items_center()
                // 图标在数字**左边**：先认形状、再读数——中文语境下数字不是
                // 段首，左边一个图标把这一行锚住，四张卡的视觉重量才对齐
                .child(icon_md(icon).text_color(tint))
                .child(
                    div()
                        .text_size(px(FS_NUM))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tint)
                        .child(n.to_string()),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(px(FS_SEC))
                        .text_color(tier3(t))
                        .child(label.to_string()),
                ),
        )
}

/// 圆角胶囊徽章：蓝灰「未提交 / 待推送 / 运行中」与暖橙「可更新」
///
/// **高度 24 而不是早先的 20（阶段 21-3）**：20 高装 11px 的字，
/// 上下各只剩 4.5px，气口被压到"贴着字"——用户反馈的「太扁」就是这个。
/// **24 是官方 `Size::Small` 按钮的高度**，取它而不是另填一个数：
/// 胶囊与并排的小按钮在同一行里时，行高本来就该由最高的那件决定，
/// 胶囊跟着那颗按钮走，两类东西在一条基线上才对得齐。
/// 字仍是 `FS_META`（11px），只改外框不动字——把字也放大就不是"胶囊加高"，
/// 是把徽章升格成第二级文字，行内会开始互相打架。
///
/// **不接 `Theme`**：它一个主题字段都不用，接了只会逼调用方在 `'static`
/// 闭包里想办法弄到主题（侧栏的 `suffix` 闭包就拿不到）。
/// 颜色由调用方算好传进来，那边有主题。
pub(crate) fn pill(text: &str, fg: Hsla, bg: Hsla) -> Div {
    div()
        .h(px(PILL_H))
        .px(px(8.0))
        .rounded_full()
        .bg(bg)
        .items_center()
        .justify_center()
        // 21-3b：**不给任何描边，也不给焦点环**。用户说的「选中后有高亮边框」
        // 来自 `Input` 获焦时的官方描边，不是胶囊自己画的——胶囊是纯展示件，
        // 不可点、不接受输入，给它留一个焦点环只会让"它能被选中"变成一句假话。
        // 颜色变化（换 `bg` 表达状态）仍然是唯一的选中反馈。
        .child(
            div()
                .text_size(px(FS_META))
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .child(text.to_string()),
        )
}

pub(crate) fn auto_sync_text(e: &RepoEntry) -> String {
    if !e.daily_time.is_empty() {
        format!("每天 {}", e.daily_time)
    } else if e.auto_sync_mins > 0 {
        format!("每 {} 分钟", e.auto_sync_mins)
    } else {
        "未开启".into()
    }
}

/// 骨架占位条：用官方 Skeleton——它自带 2 秒循环的呼吸动画，
/// 静态的灰条看不出"正在加载"，只会像渲染坏了。
pub(crate) fn skeleton_bar(w: Option<gpui_kit::Pixels>, _t: &Theme) -> AnyElement {
    let mut bar = Skeleton::new().h(px(10.0)).rounded(px(R_MD));
    bar = match w {
        Some(w) => bar.w(w).flex_shrink_0(),
        None => bar.w_full(),
    };
    bar.into_any_element()
}

/// 状态条与日志区共用的一档小字。分成一个函数是因为状态条里的文字
/// 全部是 12px 三级灰，四个地方各写一遍迟早有一处写成 13px
pub(crate) fn meta_text(text: impl Into<String>, t: &Theme) -> Div {
    div()
        .text_size(px(FS_SEC))
        .text_color(tier3(t))
        .child(text.into())
}

/// 每一段的**共同外壳**：白卡 + 1px 边 + 12px 圆角 + 8px 内边距。
///
/// 2026-10-05 收口。原先有两份：`log::section`（内边距 8）与 `space_tab::section`
/// （内边距 **16**），历史 / 变更两个页签则根本没有卡——于是五个页签摆在
/// 一屏里，内边距有两个值、有的有壳有的没壳。早先每张卡单独手搓还长出
/// 9 种圆角、6 种按钮高度（`AGENTS.md` 视觉规范一节记着那次扫描）。
///
/// **不给高度**：日志要 `flex_1` 吃剩余，其余各段要 `flex_shrink_0` 按内容高，
/// 压不动。这个差别由调用方加
pub(crate) fn card(t: &Theme) -> Div {
    v_flex()
        .w_full()
        .gap(px(GAP))
        .p(px(GAP))
        .rounded(px(R_XL))
        .bg(t.colors.popover)
        .border_1()
        .border_color(t.colors.border)
}

/// 卡内表头：左标题（+ 可选图标位与附注），右操作组，中间一个 `flex_1` 顶开。
///
/// 日志表头与说明表头本来就是同一副形状（各写一遍：24px 行高、`FS_BODY` /
/// `MEDIUM` / 前景色、右侧 XSmall 幽灵键），历史 / 变更 / 空间三个页签原先
/// 压根没有表头行——那一行 meta 小字 + 一颗键浮在内容上方，看着像内容的一部分。
/// 收成一个构造器之后，"这一段的表头"在五个页签里是同一个东西。
///
/// 右侧操作之间要用 `vdivider` 分组时**由调用方插**：日志是
/// 「复制 / 清空 │ 终端 / 开发运行 / 编译构建」两组，说明是
/// 「中文 / English │ 在文件管理器中查看」。分组关系各页不同，
/// 塞进这个函数只会多出一堆布尔开关
pub(crate) struct CardHeader {
    title: String,
    leading: Option<AnyElement>,
    /// 标题右边的一句附注。`danger` 为真时用危险色——切换文档失败那类
    /// "你要的东西没拿到"的话不是一句说明
    note: Option<(String, bool)>,
    right: Vec<AnyElement>,
}

impl CardHeader {
    pub(crate) fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            leading: None,
            note: None,
            right: Vec::new(),
        }
    }

    /// 标题左边的图标位（说明页那张 📄）
    pub(crate) fn leading(mut self, e: AnyElement) -> Self {
        self.leading = Some(e);
        self
    }

    pub(crate) fn note(mut self, text: impl Into<String>, danger: bool) -> Self {
        self.note = Some((text.into(), danger));
        self
    }

    /// 往右侧操作组里加一颗键。**可多次调**，顺序即从左到右
    pub(crate) fn right(mut self, e: AnyElement) -> Self {
        self.right.push(e);
        self
    }

    pub(crate) fn build(self, t: &Theme) -> Div {
        let mut row = h_flex()
            .w_full()
            .h(px(24.0))
            .flex_shrink_0()
            .gap(px(GAP_TIGHT))
            .items_center()
            .when_some(self.leading, |r, e| r.child(e))
            .child(
                div()
                    .text_size(px(FS_BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.colors.foreground)
                    .child(self.title),
            )
            .when_some(self.note, |r, (text, danger)| {
                r.child(
                    div()
                        .text_size(px(FS_SEC))
                        .text_color(if danger { t.colors.danger } else { tier3(t) })
                        .child(text),
                )
            })
            .child(div().flex_1());
        for e in self.right {
            row = row.child(e);
        }
        row
    }
}

/// 表头里那条 1px 竖细线：把右侧操作分成两组
pub(crate) fn vdivider(t: &Theme) -> Div {
    div().w(px(1.0)).h(px(16.0)).flex_shrink_0().bg(track(t))
}

/// 进度行左侧那行字：`阶段（路径） · 已传输 1.21 MiB · 56.00 KiB/s`
///
/// **抽成纯函数是为了单测得掉**（D48 那条规律：判据不抽出来就只能靠抓帧看，
/// 而抓帧证明不了"三样都在"）。判据就是这三段：阶段、网速、` · ` 连接。
/// 这三样在 Tauri 版是同一行里的三段（`BusyPop.tsx`），
/// 重写成 GPUI 时日志卡那段**整条漏了，只剩一根光条**。
///
/// **缺哪段就不补连接符**——core 的路径标签是拼在阶段尾巴上的，
/// 阶段空的时候单拎网速出来是对的，补个 ` · ` 会变成没有主语的碎片
pub(crate) fn progress_text(stage: &str, detail: &str) -> String {
    match (stage.trim().is_empty(), detail.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => stage.trim().to_string(),
        (true, false) => detail.trim().to_string(),
        (false, false) => format!("{} · {}", stage.trim(), detail.trim()),
    }
}

/// 进度读数：上面一行「阶段（路径）· 网速」+ 右对齐百分比，下面一根条。
///
/// ⚠️ **别和 `dialogs/clone.rs` 里那个同名形状的 `progress_block` 搞混**：
/// 那一个是克隆框的**进度区**（读数 + 实时输出流），这一个只有读数与条，
/// 日志卡要的也正是这个。两个都留着，名字分开——它们是包含关系，不是重复
///
/// **全应用只有这一个出处**（日志卡与克隆框共用）。原先日志卡那份只画条：
/// `stage` 进了 `accessibility_label`（读屏专用，**肉眼看不见**）、
/// `percent` 进了 `bar.value()`（一个不可见的数），而克隆框那份有文字——
/// 同一份 `ProgressEvent`、同一个 core，两处渲染却不一样。这正是
/// `AGENTS.md` 记的「两处用了同一套写法」的形状，收到一个函数里就不可能再漂。
///
/// **有没有 `percent` 就是两种形态**：有走确定态，没有走官方自带的不定态扫光。
/// 跑常用命令给不出可信百分比，造一个假的只会让人以为快好了，然后越等越焦
///
/// `id` 由调用方给（`log-progress` / `clone-progress`）：gpui 的滚动与动画状态
/// 按 `ElementId` 全局存，**一帧里出现两次同一个 id 就是 debug 构建 panic**
/// （`Duplicate a11y node id`），两处必须各给各的
pub(crate) fn progress_readout(
    t: &Theme,
    id: &'static str,
    text: &str,
    percent: Option<f32>,
) -> AnyElement {
    let bar = Progress::new(id)
        .w_full()
        .with_size(Size::XSmall)
        .color(t.colors.accent)
        .loading(percent.is_none())
        .accessibility_label(if text.is_empty() {
            "操作进行中".to_string()
        } else {
            text.to_string()
        });
    let bar = match percent {
        Some(pct) => bar.value(pct).into_any_element(),
        None => bar.into_any_element(),
    };
    v_flex()
        .w_full()
        .flex_shrink_0()
        .gap(px(GAP_TIGHT))
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap(px(GAP))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(FS_SEC))
                        .text_color(tier3(t))
                        .child(text.to_string()),
                )
                // 百分比右对齐 + 等宽，跟左侧那行字分开重量；
                // 没有 percent 就不画——空着的百分比位比没有更让人以为卡在 0%
                .when_some(percent, |row, p| {
                    row.child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(FS_SEC))
                            .font_family(t.mono_font_family.clone())
                            .text_color(tier3(t))
                            .child(format!("{p:.0}%")),
                    )
                }),
        )
        .child(bar)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::progress_text;

    /// 三段都在：阶段（带路径标签）+ 网速。这是 Tauri 版 `BusyPop` 那一行的形状，
    /// 也是 2026-10-05 丢的那三样
    #[test]
    fn stage_and_speed_are_joined() {
        assert_eq!(
            progress_text("接收对象（经代理 127.0.0.1:7897）", "已传输 1.2 MiB · 2.3 MiB/s"),
            "接收对象（经代理 127.0.0.1:7897） · 已传输 1.2 MiB · 2.3 MiB/s"
        );
    }

    /// 只有阶段（我们自己发的路径切换提示、`done.` 之后那类）
    #[test]
    fn stage_alone_has_no_dangling_separator() {
        assert_eq!(progress_text("正在尝试直连…", ""), "正在尝试直连…");
    }

    /// 只有网速（理论上不该发生，但别拼出「 · 已传输…」这种没有主语的碎片）
    #[test]
    fn speed_alone_does_not_lead_with_a_separator() {
        assert_eq!(progress_text("", "已传输 1.2 MiB"), "已传输 1.2 MiB");
    }

    /// 两样都空 = 这一行什么都不画。**判据是空串**，
    /// 不是那行文字里有没有 ` · `——调用方靠这个决定左半边给不给占位
    #[test]
    fn both_empty_is_empty() {
        assert_eq!(progress_text("", ""), "");
        // 空白也算空：core 那几处是 `trim` 过的，别让一个空格顶出「 · 」
        assert_eq!(progress_text("   ", "  "), "");
    }
}
