//! 跨屏复用的小部件：分组标题、空态主区、统计卡、状态药丸、加载骨架、竖分隔线。
//!
//! 这里只放纯展示、无状态的部件；有状态的一律回各页自己的模块。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
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

use crate::keys::OpenPalette;
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
                                    .text_size(px(fs_meta()))
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
    let tail = div().text_size(px(fs_meta())).text_color(tier4(t));
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
/// **图标底块：全站唯一出处**（v4 §6 登记表第 6 行）。
///
/// 这是 v4 那个核心手法的落点：**图标的视觉重量靠底块面积，不靠笔画**。
/// 想调笔画是调不了的——1830 个 lucide 里 1825 个把 `stroke-width="2"` 硬编码在
/// SVG 里，`paint_svg` 只吃一个 `Hsla`，`Icon` 不暴露任何 stroke API（v4 §5.2、§9）。
/// 所以"图标看着太轻"的正解是给它一块 `wash` 底，不是去找一个调粗的开关。
///
/// ⚠️ **一档，不许多**：40 / 圆角 12 / 内图标 20。原型里 42 / 46 混着画过，
/// 落地统一 40（v4 §11）。多开一档的代价不是现在不好看，是下一处会开出第四档。
///
/// `tint` 默认给 `brand`。统计卡那种"图标色就是数字色"的变体传状态色是有意的
/// （见 `stat_card` 的注释），**但底块一律 `wash`，不跟状态色**——
/// 四种底色配四个语义会把"这块底是装饰"说成"这块底是状态"，而状态已经由图标自己表达了。
pub(crate) fn glyph(t: &Theme, name: gpui_kit::assets::IconName, tint: Hsla) -> Div {
    div()
        .w(px(GLYPH))
        .h(px(GLYPH))
        .flex_shrink_0()
        .rounded(px(GLYPH_R))
        .bg(wash(t))
        .flex()
        .items_center()
        .justify_center()
        .shadow(sh_1(t))
        .child(icon_box(name).text_color(tint))
}

/// 总览页顶部那四张统计卡。
///
/// ## 图标位（阶段 21-8）
///
/// 原来这张卡只有数字与文字，**没有图标**——所以总览页那一屏看起来
/// "图标大大小小各种风格"，其实那一处是**压根没有**。
///
/// ## 阶段 31.3：改回"有边、有薄阴影"
///
/// 这里原先明写「**不加边框、不加阴影**，这一屏已经有表格的横线当结构了，
/// 再套一圈框会变成框里套框」——那是**旧画布**（#FAFAF9，与白卡只差 5/255）
/// 下才成立的判断：那时候加边框等于给一屏再加一层灰线，越加越糊。
/// 现在画布是冷灰蓝、卡与底差 23/255，**卡自己就浮得起来**，
/// 那条判断的前提没了。⇒ 与 `card()` 同形态（边 `line-soft` + `SH_2`），
/// 只是它比正文区的卡矮一档：`px(GAP_XL) / py(GAP_LG)`。
///
/// ## 阶段 31.4：一行只有两部分（图标块 + 一列），不再把标签推到最右
///
/// 原来这一行是 `图标 · 数字 · flex_1 空档 · 标签`——标签那一路被
/// `div().flex_1()` 顶到了卡的最右边，于是**每张卡中间横着一道空洞**
/// （2026-10-07 抓帧实测：数字墨迹右缘 x=149、标签墨迹左缘 x=297，
/// 中间空 **147px**，占掉这张卡白区宽度（294）的一半），而这道空洞还顺手把卡高撑了出来。
/// 原型 `.stat` 的结构是**两件事**：左边 `glyph` 底块，右边一列
/// （数字在上、标签在下，都左对齐）。⇒ 现在照这个排，
/// **卡高由内容撑**（这一行的决定者是 40 高的图标块与那一列里较高的那个），
/// 不写死、也不靠 `flex_1` 撑。
///  **别再往这一行塞 `flex_1` / `justify_end` 推标签**：外层那颗 `.flex_1()`
/// 是**卡片自己**在四等分行里占宽用的，跟标签无关，别混为一谈。
///
/// ⚠️ **图标颜色跟着数字走**，不取中性灰：数字已经在用语义色表达状态
/// （已同步绿 / 落后橙 / 分叉红），图标再给一个中性色就成了两套信号，
/// 而"这颗图标的颜色和那个数字无关"是假的——它们说的是同一件事。
/// 数字为 0 时**两者一起降到 tier4**：一排彩色图标配一排 0 会显得吵，
/// 而"没东西"本来就不该有动作色。⚠️ 但**底块始终 `wash`**，不跟着染色（见 `glyph`）。
pub(crate) fn stat_card(
    n: usize,
    label: &str,
    color: Hsla,
    icon: gpui_kit::assets::IconName,
    t: &Theme,
) -> Div {
    let tint = if n > 0 { color } else { tier4(t) };
    h_flex()
        .flex_1()
        .items_center()
        .gap(px(GAP_LG))
        .px(px(GAP_XL))
        .py(px(GAP_LG))
        .rounded(px(r_xl(t)))
        .bg(card_face(t))
        .border_1()
        .border_color(track(t))
        .shadow(sh_2(t))
        // 图标在数字**左边**：先认形状、再读数——中文语境下数字不是
        // 段首，左边一个图标把这一行锚住，四张卡的视觉重量才对齐
        .child(glyph(t, icon, tint))
        .child(
            v_flex()
                .gap(px(GAP_DENSE))
                .child(
                    div()
                        // 两行的行高都按 `num_row_h` 给，不落到 `row_h` 的 1.9 倍：
                        // 竖着叠两行时那 0.7 个字高的余量要翻倍，卡会被撑到 98 高
                        // （实测与推导见 `tokens::num_row_h`）
                        .h(px(num_row_h(fs_num())))
                        .text_size(px(fs_num()))
                        .font_weight(FontWeight::BOLD)
                        .text_color(tint)
                        .child(n.to_string()),
                )
                .child(
                    div()
                        .h(px(num_row_h(fs_meta())))
                        .text_size(px(fs_meta()))
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
/// 字仍是 `fs_meta()`（11px），只改外框不动字——把字也放大就不是"胶囊加高"，
/// 是把徽章升格成第二级文字，行内会开始互相打架。
///
/// **不接 `Theme`**：它一个主题字段都不用，接了只会逼调用方在 `'static`
/// 闭包里想办法弄到主题（侧栏的 `suffix` 闭包就拿不到）。
/// 颜色由调用方算好传进来，那边有主题。
pub(crate) fn pill(text: &str, fg: Hsla, bg: Hsla) -> Div {
    pill_with(None, text, fg, bg)
}

/// 带一枚前置图标的胶囊。**与 `pill` 共用同一份实现**——
/// 高度、圆角、字号、水平内边距只有一个出处，不会出现"带图标的那种胶囊矮一档"
pub(crate) fn pill_icon(
    icon: gpui_kit::assets::IconName,
    text: &str,
    fg: Hsla,
    bg: Hsla,
) -> Div {
    pill_with(Some(icon), text, fg, bg)
}

fn pill_with(
    icon: Option<gpui_kit::assets::IconName>,
    text: &str,
    fg: Hsla,
    bg: Hsla,
) -> Div {
    // ⚠️ **必须是 `h_flex`，不能是裸 `div()`**。原来这里写的是 `div()`——
    // 裸 Div 是**列方向**，那么 `.items_center()` 管的就是**水平**居中，
    // 垂直方向压根没人管 ⇒ 26 高的胶囊里那 12 的字偏上（2026-10-08 用户挑的
    // 「状态胶囊文字与中心对齐」，实测上隙 6 / 下隙 8）。
    // 换成 h_flex 后 `items_center` 才真的落在垂直轴上，`justify_center` 管水平
    h_flex()
        .h(px(pill_h()))
        .px(px(GAP))
        .rounded_full()
        .bg(bg)
        .items_center()
        .justify_center()
        .gap(px(GAP_DENSE))
        // 21-3b：**不给任何描边，也不给焦点环**。用户说的「选中后有高亮边框」
        // 来自 `Input` 获焦时的官方描边，不是胶囊自己画的——胶囊是纯展示件，
        // 不可点、不接受输入，给它留一个焦点环只会让"它能被选中"变成一句假话。
        // 颜色变化（换 `bg` 表达状态）仍然是唯一的选中反馈。
        .when_some(icon, |p, i| p.child(icon_sm(i).text_color(fg)))
        .child(
            div()
                .text_size(px(fs_meta()))
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .child(text.to_string()),
        )
}

/// 结论对 / 错那枚标记。**全站唯一出处**（v4 §5.7「禁止文字冒充图标」）。
///
/// 以前这里两处各写一个 `"✓"` / `"✗"` 字符串（`log.rs` 与 `dialogs/batch_log.rs`）。
/// 拿字符当图标有两层坏：① 它跟着**文字**的字体与字重走，两处的字号档一不同、
/// 字形就不同，同一屏里"对号"会长得不一样；② 读屏软件会把它**念成**
/// "check mark"，而图标是有 `accessibility_label` 的。
/// 机器闸 R5 现在盯着这一类（`scripts/check_design_tokens.py`）。
///
/// 颜色由调用方给：日志那一行的对 / 错与它后面那句结论**同色**，
/// 在这里写死 success/danger 反而会跟调用处分家。
pub(crate) fn status_mark(ok: bool, color: Hsla) -> AnyElement {
    let name = if ok {
        gpui_kit::assets::IconName::Check
    } else {
        gpui_kit::assets::IconName::X
    };
    icon_sm(name).text_color(color).into_any_element()
}

/// 卡内分隔 / 分段行：**撑到卡边那一条 `line-soft`**（v4 §6 登记表第 3 行）。
///
/// 以前"卡里来一条横线"各页自己拼 `border_t_1` + 一对负 margin，
/// 于是线的颜色有 `border` 有 `track`、左右缩进有三种——同一件事画成五种样子。
///
/// ⚠️ **负 margin 只能是 `-GAP_XL`**，因为它抵消的就是 `card()` 那一圈内边距。
/// 这两个数**必须一起改**：把卡内边距调成 20 而这里还写 -16，线就两头缩进，
/// 而"线没对齐卡边"这种事抓帧看着只是"有点怪"，谁都不会去查一个常量。
///
/// 用法有两种，都走这一个出处：① 单纯断一段（不给 child）；
/// ② 承载一行分段控件（仓库头下面那排页签就是这种——原型
/// `.tabs-row { margin: 13px -18px 0; padding: 0 18px; border-top: 1px solid var(--line-soft) }`
/// 说的正是"页签是这张卡的一部分，不是浮在卡外面的一行"）。
pub(crate) fn card_split(t: &Theme) -> Div {
    div()
        .w_full()
        .flex_shrink_0()
        .mx(px(-GAP_XL))
        .px(px(GAP_XL))
        .mt(px(GAP_LG))
        .border_t_1()
        .border_color(track(t))
}

/// 行内告警点：**一颗 `warn` 色的告警图标 + 官方 tooltip 气泡**（v4 §7「告警」行）。
///
/// ⛔ 它替代的是"整块 wash 横贯全宽"那种告警条。一条提示的优先级**不该抬到
/// 这一屏的操作之上**——用户每次进这个仓库先要吃一条横幅，而它说的往往是他
/// 早就知道的事（2026-10-06 用户原话：「把这个警告不要，如有警告在上面仓库名
/// 后面出现个感叹号鼠标触发气泡提示就行了」）。
///
/// ⚠️ **与 v4 §7 那条契约有意不符，写在 §9 能力表里**：契约原本要"18px 圆 +
/// `warn-wash` 底块"。做不到的原因是**官方 `tooltip` 只挂在 `Button` / `Toggle`
/// 上**（`button.rs:399`、`toggle.rs:72`），裸 `Div` 没有这个入口
/// （`attachment.rs:305` 那个是附件条自己的）。要底块就得自绘一颗假按钮 +
/// 自己管浮层——**那正是 v4 §6 禁止的"手拼会漂的假提示"**。
/// ⇒ 取"官方 tooltip + 无底块"，舍"底块 + 自绘浮层"。
/// 副作用：ghost 档带一点悬停面，看着略微像可点；它不给 `on_click`，点了什么都不发生。
pub(crate) fn warn_dot(
    id: &'static str,
    text: impl Into<gpui_kit::SharedString>,
    t: &Theme,
    cx: &gpui_kit::App,
) -> AnyElement {
    Button::new(id)
        .icon(icon_sm(gpui_kit::assets::IconName::TriangleAlert).text_color(orange(t)))
        .custom(ghost_variant(cx))
        .with_size(Size::Small)
        .tooltip(text)
        .accessibility_label("这个仓库有一条需要注意的提示")
        .into_any_element()
}

/// 卡头右上的 ⓘ 那颗：一句**不该常驻在界面上**的说明收进气泡里。
///
/// 与 `warn_dot` 是同一副形状（ghost 图标键 + `.tooltip()`），差在颜色与语义：
/// 那颗是"有件事你得知道"（橙），这颗是"想知道可以问"（三级灰）。
///
/// ⚠️ **为什么是按钮而不是一枚图标**：`.tooltip()` 只存在于 `Button`
/// （`button.rs:399`）与 `Toggle`（`toggle.rs:72`）上，**裸 `Div` 没有这条路**。
/// 要气泡就必须是按钮——`warn_dot` 那里已经为同一件事偏离过一次 v4 契约
/// （契约画的是 wash 圆底），这里是同一个妥协的第二处，**不是新画的形态**。
///
/// ⚠️ **气泡靠边会不会自己翻边：未实测**（Q14）。官方 `tooltip()` 理论上选边，
/// 而"理论上"在本项目吃过好几次亏。设置页这几颗都在卡头（偏上），风险最低；
/// 真机验一次要挑**最靠底部**的那颗
///
/// ⚠️ **气泡正文是纯文本**：`tooltip()` 收的是 `SharedString`，**不走 markdown**，
/// 所以 `**加粗**` 与 `` `代码` `` 会**连星号和反引号一起画出来**。
/// 强调靠措辞（"只有"、"不会"），不靠符号；路径与命令名直接写裸文本
///
/// ⚠️ **签名与 `warn_dot` 不一样：那颗收 `cx`，这颗收 `gv`**。不是漏改——
/// 这颗注定的调用点是设置页那几个**纯渲染函数**（`identity_page(t, st)` 这一类），
/// 它们整条链上都没有 `&mut App`；而为了拿一颗 `gv` 把 `cx` 一路穿六层参数，
/// 就是把"页函数"变成"要窗口的函数"。**别去统一它们**
pub(crate) fn info_dot(
    id: &'static str,
    text: impl Into<gpui_kit::SharedString>,
    t: &Theme,
    gv: ButtonCustomVariant,
) -> AnyElement {
    Button::new(id)
        .icon(icon_sm(gpui_kit::assets::IconName::Info).text_color(tier4(t)))
        .custom(gv)
        .with_size(Size::XSmall)
        .tooltip(text)
        .accessibility_label("这条的详细说明")
        .into_any_element()
}

/// 设置页表单行的**唯一出处**：标签定宽在左、控件吃掉剩下的宽度，
/// 行与行之间一条 `line-soft`。
///
/// ## 它治的是什么
///
/// 阶段 31.5 之前设置页有两种对齐混在一页里：`field()` 是"标签在上、
/// 输入框在下、框拉满整宽"，而勾选区是"一列长句竖排"。同一个对话框里
/// 两种排法，用户读不出"哪几条是一组"——那就是用户说的"排版混乱"。
/// **两种对齐混在一页里，比哪一种都不好看**。
///
/// ## 为什么是个收集器而不是一个函数
///
/// 分隔线要"除第一行之外"，那就得知道自己是第几行。让每个调用方自己数
/// （`if ix > 0`）等于把这条规则抄 N 遍，抄错的那一处就是"第一段没有分隔线、
/// 第二段有"。收成 `FormRows` 之后**行数由容器知道，不由调用方记得**
/// ——与 D78「让『忘』没有选项」是同一条手法。
///
/// ## 标签列宽
///
/// 读 `tokens::FORM_LABEL_W`（112，被"最长那条标签不断字"夹出来的数）。
/// **别在调用方再写一个 `w(px(..))`**：那是 v4 §6 登记表要挡的那一类
pub(crate) struct FormRows {
    rows: Vec<(String, AnyElement)>,
}

impl FormRows {
    pub(crate) fn new() -> Self {
        Self { rows: Vec::new() }
    }

    /// 一行 = 一个标签 + 一个控件。控件给什么都能装（`Input`、`Switch`、
    /// `RadioGroup`、一颗按钮，或调用方自己组的 `h_flex`）
    pub(crate) fn row(mut self, label: impl Into<String>, control: impl IntoElement) -> Self {
        self.rows.push((label.into(), control.into_any_element()));
        self
    }

    pub(crate) fn build(self, t: &Theme) -> Div {
        let last = self.rows.len().saturating_sub(1);
        v_flex()
            .w_full()
            .children(self.rows.into_iter().enumerate().map(|(ix, (label, control))| {
                h_flex()
                    .w_full()
                    // ⚠️ D41：`h_flex` 默认 `items_center`。这里要的正是居中
                    // （标签与 24px 输入框、与开关同高），但**换控件时先想清楚**：
                    // 放一颗多行说明进去，标签会跟着跑到中间去
                    .items_center()
                    .gap(px(GAP_LG))
                    .py(px(GAP))
                    // 第一行不画上沿：卡头与它之间已经有 `card` 的 12px 呼吸量，
                    // 再来一条线就成了"卡头下面一道横杠"
                    .when(ix > 0, |r| r.border_t_1().border_color(track(t)))
                    .when(ix == last, |r| r.pb_0())
                    .child(
                        div()
                            .w(px(FORM_LABEL_W))
                            .flex_shrink_0()
                            .text_size(px(fs_body()))
                            .text_color(body(t))
                            .child(label),
                    )
                    // 控件那一列必须 `flex_1 + min_w_0`：少了 `min_w_0`，
                    // 里面一句长地址就把整列顶宽，标签列跟着被挤窄折行。
                    //
                    // ⚠️ `justify_end` 是这一列**唯一**的对齐口径：开关、单选组、
                    // 按钮这类**按内容宽**的控件必须贴到行末——不然它们会紧贴在
                    // 标签列后面、右边空一大片，一整页读下来就是"没排完"
                    // （2026-10-07 抓帧实锤：深色主题那颗开关）。
                    // **别在调用方再写一次 `justify_end` 或 `ml_auto`。**
                    //
                    // ⚠️ 反过来也成立：**想占满整列的控件必须自己写 `w_full()`**
                    // （`Input::new(..).w_full()`，或一组 `h_flex().w_full()`）。
                    // 这一列是按内容宽靠右的，不给 `w_full` 的话里面任何
                    // `flex_1` 都塌成 0 宽——同一天抓帧的另一张图里，
                    // 「手动地址」那一格只剩一个椭圆，placeholder 全看不见
                    //
                    // ⚠️ **别把这一列写成 `div()`**：gpui 的裸 `Div` 是列方向，
                    // `justify_end` 会把子项往**下**推，看着像"完全没生效"。
                    // 要横向推到行末必须是 `h_flex`
                    .child(h_flex().flex_1().min_w_0().justify_end().child(control))
            }))
    }
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
        .text_size(px(fs_sec()))
        .text_color(tier3(t))
        .child(text.into())
}

/// 每一段的**共同外壳**：白卡 + 1px 边 + `R_XL` 圆角 + **16px 内边距** + 薄阴影。
///
/// 2026-10-05 收口。原先有两份：`log::section`（内边距 8）与 `space_tab::section`
/// （内边距 **16**），历史 / 变更两个页签则根本没有卡——于是五个页签摆在
/// 一屏里，内边距有两个值、有的有壳有的没壳。早先每张卡单独手搓还长出
/// 9 种圆角、6 种按钮高度（`AGENTS.md` 视觉规范一节记着那次扫描）。
///
/// 2026-10-07 阶段 31 换成 v4 形态：内边距 `GAP` → **`GAP_XL`(16)**、
/// 块间距 `GAP` → **`GAP_LG`(12)**、底 `popover` → **`card_face`**（语义对了：
/// 它是"卡"不是"弹层"，两档主题里恰好同值，所以以前没露出来）、
/// 并加上 **`sh_2`** 那层薄阴影（模糊 6px / 4% 墨，v4 §3.4——旧的 24px 厚阴影
/// 是"卡片太厚"那句反馈的根因）。
///
/// ⚠️ **卡间距只允许由父容器的 `gap` 给一次**。阶段 28 踩过：给滚动容器又加了
/// 一层 `padding-top`，叠在父 gap 上 ⇒ 头卡下 20、卡间 10，看着"就是不齐"而
/// 根因不是数值（v4 §3.2、D90）。
///
/// **不给高度**：日志要 `flex_1` 吃剩余，其余各段要 `flex_shrink_0` 按内容高，
/// 压不动。这个差别由调用方加
pub(crate) fn card(t: &Theme) -> Div {
    v_flex()
        .w_full()
        .gap(px(GAP_LG))
        .p(px(GAP_XL))
        .rounded(px(r_xl(t)))
        .bg(card_face(t))
        .border_1()
        // ⚠️ **卡描边只用 `line_soft`**（v4 §2 那张表把它的用途写成"卡内分隔、卡描边"，
        // §8.2 深色那节再钉一遍"卡与画布的分界由 `line_soft` 承担"）。
        // 这里原来写的是 `t.colors.border`（= `line` #D7E0E4，深一档），而
        // `stat_card` 与总览活跃度卡走的是 `track(t)`（= `line_soft` #E8EEF0）
        // ⇒ **同一屏三种卡、两种描边色**（帧上实测 215,224,228 对 225,231,233，差 10/255，
        // 肉眼就是"有的卡边重有的轻"）。`line` 归"主描边／分隔线"那一格，不是卡
        .border_color(track(t))
        .shadow(sh_2(t))
}

/// 卡内表头：左标题（+ 可选图标位与附注），右操作组，中间一个 `flex_1` 顶开。
///
/// 日志表头与说明表头本来就是同一副形状（各写一遍：24px 行高、`fs_body()` /
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
            // **原来这里挂着 `.w_full()`，删了**——不是因为它把卡撑破了
            // （2026-10-08 我先这么归因过一次，实测删掉右缩进仍是 0，真凶是
            // `OV_TABLE_CHROME` 没扣掉卡内那 32px，见 `overview.rs` 那条注释），
            // 而是因为它**本来就是多余的**：父是 `v_flex`（列方向），
            // 交叉轴默认就是 stretch，再写 100% 只是给同一件事第二个出处
            .flex_shrink_0()
            // 阶段 28：跟着标题那一档 `fs_body()` 走。原来写死 24，
            // 而这一行装的是 `fs_body()` 标题 + `fs_sec()` 附注 + 右侧
            // XSmall 键——**一屏里每一张卡都有它**，切字是一屏都切
            .h(px(row_h(fs_body())))
            .flex_shrink_0()
            .gap(px(GAP_TIGHT))
            .items_center()
            .when_some(self.leading, |r, e| r.child(e))
            .child(
                div()
                    .text_size(px(fs_body()))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.colors.foreground)
                    .child(self.title),
            )
            .when_some(self.note, |r, (text, danger)| {
                r.child(
                    div()
                        .text_size(px(fs_sec()))
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
                        .text_size(px(fs_sec()))
                        .text_color(tier3(t))
                        .child(text.to_string()),
                )
                // 百分比右对齐 + 等宽，跟左侧那行字分开重量；
                // 没有 percent 就不画——空着的百分比位比没有更让人以为卡在 0%
                .when_some(percent, |row, p| {
                    row.child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(fs_sec()))
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
