//! 视觉档位与图标包装：字号 / 圆角 / 间距 / 图标尺寸四组常量，以及 theme.json 覆盖不到的补充灰阶。
//!
//! 禁写裸数字的理由与各档用途见 docs/UI-DESIGN.md 第 4.2 节。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::component::{Icon, Sizable, Size, Theme};
use gpui_kit::{px, rgb, Hsla};

// ---------- 规范板色板中 theme.json 没有的补充灰阶 ----------
// 设计规范：正文 #37352F / 三级 #787871 / 四级 #A4A099 / 占位 #BBB8B1，
// 以及选中暖灰面 #EDEBE7、条轨 #EDEAE5、可更新暖橙底 #FDEFE4。
// theme.json 的 token 覆盖不了这几级，集中在这里定义，禁止散落魔法数字。

/// 正文 / 行内主要文字（比 foreground #1A1A1A 柔一档）
pub(crate) fn body(_t: &Theme) -> Hsla {
    rgb(0x37352F).into()
}
/// 三级：元信息、说明文字
pub(crate) fn tier3(_t: &Theme) -> Hsla {
    rgb(0x787871).into()
}
/// 四级：弱提示、上次结果
pub(crate) fn tier4(_t: &Theme) -> Hsla {
    rgb(0xA4A099).into()
}
/// 构成条 / 进度条的轨道
pub(crate) fn track(_t: &Theme) -> Hsla {
    rgb(0xEDEAE5).into()
}
/// 「可更新」徽章 / 清理条的暖橙底
pub(crate) fn orange_bg(_t: &Theme) -> Hsla {
    rgb(0xFDEFE4).into()
}
/// 清理条上的深橙文字
pub(crate) fn orange_text(_t: &Theme) -> Hsla {
    rgb(0x9A4500).into()
}
/// 「运行中 / 未提交」徽章的蓝灰底
pub(crate) fn blue_bg(_t: &Theme) -> Hsla {
    rgb(0xEDF1F5).into()
}
/// 「失败」徽章的浅红底。跟 orange_bg / blue_bg 是同一套 wash 逻辑，
/// 只是色相对齐 theme.json 的 `danger.background` #C2373B——
/// 文字色直接用 `t.colors.danger`，底和字必须是同一个红的两档
pub(crate) fn red_bg(_t: &Theme) -> Hsla {
    rgb(0xFBEBEC).into()
}
/// 次按钮描边（比 hairline 深一档）
pub(crate) fn edge(_t: &Theme) -> Hsla {
    rgb(0xC8C4BE).into()
}

// ---------- 选中面：全站同类高亮的唯一出处 ----------

/// 选中遮罩的不透明度。**0.08 不是随手取的**，它由设计规范板的
/// 「选中面」#EDEBE7 反推：白卡（#FFFFFF）上叠一层前景色（#1A1A1A），
/// 要落到 237/255 就得 `a ≈ (255−237) / (255−26) ≈ 0.079`。
/// G/B 两通道因为规范板偏暖会差 2–6/255，肉眼不可分。
///
/// `tests::mask_lands_on_the_design_boards_selection_face` 把这条钉住。
pub(crate) const SELECTED_MASK_ALPHA: f32 = 0.08;

/// 选中面。**全站同类高亮只认这一个函数**（D60）。
///
/// 「同类」= ghost 图标按钮的选中态：导航轨三颗切屏键、侧栏那颗筛选按钮。
/// 它们以前各自走官方 `Button::selected`，底色来自 `button.active.background`
/// —— 而 `gui/theme.json` **没有配这一项**，于是落到基座兜底
/// `input.mix_oklab(transparent, 0.7)`，即"输入框底色的 70%"，比规范板的
/// 选中面深一大截（2026-10-03 实机反馈："图标的高亮颜色太深"）。
///
/// 为什么是**遮罩**而不是再往 theme.json 里填一个实色：
/// ① 实色要挑一个"在白轨上好看"的值，换个底（深色主题、卡片底）就又不对了；
/// ② 遮罩是前景色按比例压一层，**在任何底上都成立**，天然"适应全局同类"。
/// ③ 官方 `Button` 会在每个状态层里**最后重放调用方的样式**
/// （`button.rs:733` 与 `:795` 的 `refine_style(&instance_style)`），
/// 所以 `.when(active, |this| this.bg(selected_mask(t)))` 盖得过 selected 底色，
/// 不必去改基座。
///
/// ⚠️ 用法只能是 `.when(active, ..)`：**无条件 `.bg()` 会让未选中的按钮也带上底**，
/// 因为同一份 `instance_style` 在常态层里也会被重放。
///
/// ⚠️ **同类里还剩一个没统一的：ghost 按钮的「悬停」面是实心 `accent` 蓝**
/// （`button.rs` `hovered()` 的 `Self::Ghost` 臂，浅色模式不降透明度）。
/// 2026-10-03 实机实测导航轨上确实渲成 #5B7A99——比这个选中面重得多，
/// 也就是"扫过一颗没选中的图标"比"我现在在哪一屏"更抢眼。
/// **没顺手改掉**：正解是换 `ButtonVariant::Custom`，而那会连带改掉三样东西
/// （焦点环 `Edge`→`Inside`、`button.rs:699` 那条只给 `Default`/`outline` 加边的
/// 描边逻辑、以及 `normal()` 那条臂），是**重新设计这颗按钮**而不是改个色。
/// 后门也不通：调用方再调一次 `.hover(..)` 会撞上 `div.rs:843` 的
/// `debug_assert!(hover_style already set)`，**debug 构建直接 panic**；
/// 而它在 release 下又是空操作、会"静默地看起来能用"（D40 吃过一次这种亏）。
/// 已开 **Q12** 等拍板。
pub(crate) fn selected_mask(t: &Theme) -> Hsla {
    t.colors.foreground.opacity(SELECTED_MASK_ALPHA)
}

/// 「落后」的橙色（Q7，2026-10-03 用户拍板）。
/// 主题 token 里没有单独的 warning 语义色（只有 `button_warning`），
/// theme.json 已把 `chart.3` 定成规范板的 #DD5B00，这里借用它。
///
/// ⚠️ **「已分叉」不用它**——分叉是红 `danger`（`state::dot_color` /
/// `sidebar::badge_tone` / `panels::fork_card` 三处同色）。早先这里写的是
/// "「落后 / 分叉」的橙色"，三处里只有徽标跟着改了，忘了分叉条，
/// 结果同一屏里"环是红的、条是橙的"，看着像两件事
pub(crate) fn orange(t: &Theme) -> Hsla {
    t.colors.chart_3
}

// ---------- 版面规范：字号 / 圆角 / 间距 / 按钮 / 图标 ----------
// 之前这些值是就地硬编码，结果同一个「卡片圆角」在不同屏上冒出
// 1/2/3/5/10 五种杂值，按钮高度有 6 种。这里定成档位，新增元素只能选档，
// 不许再写裸数字——视觉一致靠的是只有这几档可挑。

/// 元信息、弱提示（标签、说明）
pub(crate) const FS_META: f32 = 11.0;
/// 次要文字（表格行、副标题）
pub(crate) const FS_SEC: f32 = 12.0;
/// 正文
pub(crate) const FS_BODY: f32 = 13.0;
/// 卡片标题 / 弹窗标题
pub(crate) const FS_TITLE: f32 = 16.0;
/// 页面主标题
pub(crate) const FS_HEAD: f32 = 18.0;
/// 统计卡大数字
pub(crate) const FS_NUM: f32 = 20.0;

/// 小色块 / 进度条
pub(crate) const R_SM: f32 = 4.0;
/// chip、徽章、图例方块
pub(crate) const R_MD: f32 = 6.0;
/// 按钮、列表行、小面板
pub(crate) const R_LG: f32 = 8.0;
/// 大卡片、表格容器
pub(crate) const R_XL: f32 = 12.0;

/// 密集排列：同一串**同类元素**相邻两项之间的呼吸量。
///
/// 覆盖三种关系（`UI-DESIGN.md` 4.2 的间距语义表里补的这一行）：
/// 列表行之间、构成条/进度条的色块分段之间、同一个块里的多行元信息之间。
///
/// 之前这三类全是**散落的裸数字 2.0**（共 5 处）。它们不是"更小的
/// `GAP_TIGHT`"——`GAP_TIGHT` 4px 说的是"两个不同的东西之间的关系"
/// （图标与文字、标题与说明、柱间），这一档说的是"一串同类元素里的
/// 呼吸量"。两种意图混用一个数字，改档时就会连着改错一片
pub(crate) const GAP_DENSE: f32 = 2.0;
/// 紧密（标签与值、柱间）
pub(crate) const GAP_TIGHT: f32 = 4.0;
/// 常规（图标与文字、行内元素）
pub(crate) const GAP: f32 = 8.0;
/// 分组（卡片内区块）
pub(crate) const GAP_LG: f32 = 12.0;

// 按钮高度不再自定数值：官方命名档才是配套的（Medium = 32px 高 / 14px 字 /
// 16px 图标，Small = 24px 高 / 14px 字 / 14px 图标）。
// 之前用 Size::Size(px(30)) 这类自定义档，官方 Button 会把文字放大到
// 16px、图标放大到 size×0.75（22.5px）——整屏图标过大、字顶边的根因。

/// 图标档位：行内小图标
pub(crate) const ICON_SM: f32 = 14.0;
/// 图标档位：按钮内图标
pub(crate) const ICON: f32 = 16.0;

// ---------- 壳层版面：窗口与四个列区的宽度 ----------
// 对应 docs/UI-DESIGN.md 第 1 节的版面表。这些是"版心尺寸"不是"视觉档位"，
// 但同样要只有一处真源：轨道 56 / 侧栏 240 / 抽屉 320 三个数字散在四个文件里
// 迟早会有一处漏改，窗口下限算式也会跟着失真。

/// 标题栏固定高。官方 `TitleBar` 自带 34px 档，这里用样式覆盖
/// （`refine_style` 在官方 `.h(TITLE_BAR_HEIGHT)` 之后生效，覆盖得住）。
///
/// 早先按设计稿写的 48px，实机看着"太宽"——那 14px 多出来的不是呼吸，
/// 是把窗口上沿整整吃掉一条，3456px 宽的屏上尤其明显。40px 仍在
/// Windows 习惯的 32–40 区间里，点 X / 最小化也照样好按。
pub(crate) const TITLE_BAR_H: f32 = 40.0;
/// 状态条固定高
pub(crate) const STATUS_BAR_H: f32 = 28.0;
/// 导航轨列宽。
///
/// 56 收成 44：轨上只放 32px 的图标按钮，左右各留 6px 正好。
/// 再窄就压到按钮的点击区，再宽就只是"一列空地"。
pub(crate) const NAV_W: f32 = 44.0;
/// 导航轨左右内边距。`NAV_W − 2 × NAV_PAD` 必须正好等于按钮边长（32），
/// 否则轨要么留白、要么挤掉按钮——这一条由 `nav_rail()` 的注释盯着
pub(crate) const NAV_PAD: f32 = 6.0;
/// 设置对话框左侧分类导航那一列的宽（阶段 20）。
///
/// **190 是被两件事夹出来的，不是随手取的**：
/// ① 右栏要留住 >480px——官方 `setting::Settings` 在内容面板 ≤480 时会
///    自动转成竖排（`settings.rs:425` 的 `STACKED_LAYOUT_MAX_WIDTH`），
///    标签在上、控件在下，那不是我们要的排法；
/// ② 「身份技术栈」那类四字标签 + 12px 说明句要能一行放下不断字。
/// 再宽就只是"一列空地"，和 `NAV_W` 当初从 56 收成 44 是同一条理由
pub(crate) const SET_NAV_W: f32 = 190.0;
/// 导航轨顶部内边距。
///
/// **这个数不是随手取的，是一条对齐基线定死的**：
/// **第一个导航键的图标中心要落在页面页头标题（总览页的「总览」，FS_HEAD）
/// 那一行的文字中心线上。** 用户看的就是这条线——图标和标题不在一行，
/// 整屏左上角像缺了半个对齐关系。
///
/// 推导（`title_bar()` 的图标也按同一条线走，见 `TITLE_BAR_PAD_L`）：
///
/// ```text
///   图标中心 = 标题栏下沿 + NAV_PAD_TOP + 按钮高/2
///   标题中心 = 标题栏下沿 + 页面内边距(GAP_LG) + 标题行高/2
/// ```
///
/// 实测（3456×1408 抓帧，2026-10-03）：页头「总览」字带 `66..82`、中心 74；
/// 导航第一个图标字带 `73..87`、中心 80——**差 6px**。所以 20 改 14。
///
/// ⚠️ 这条基线只在**总览页**成立（仓库页页头是仓库名 + 只读地址条，
/// 高度与位置都不同）。它是视觉上的"起手式"，不是全局栅格。
pub(crate) const NAV_PAD_TOP: f32 = 14.0;
/// 导航轨底部内边距（设置齿轮离窗口下沿的距离，D59）。
///
/// 早先**没有这一档**：齿轮是 `v_flex` 里最后一个子项，撑开的 `div().flex_1()`
///  spacer 把它顶到轨底，于是 26px 的按钮直接坐在状态条上沿——实机看着
/// "贴死在那儿"（2026-10-03 用户反馈："下方的设置图标离底也太近了一点"）。
///
/// 取 `GAP_LG`（12）而不是更小的档，理由**和按钮之间的 `gap` 是同一条**：
/// 齿轮是独立的一组（打开设置），跟上面三颗切屏键不构成"一串同类行"，
/// 它与窗口下沿之间要的是"两个不同的东西"那种能分开的气口，所以往大档走。
/// 早先顶与底共用一个 `NAV_PAD` 时也是这么混的（见 `nav_rail()` 的注释）。
pub(crate) const NAV_PAD_BOTTOM: f32 = GAP_LG;
/// 检视抽屉宽度上下限。与 core 的 `INSPECTOR_W_RANGE` 同一份定义。
///
/// **默认宽度不在这儿再写一份**：右栏从"写死 320 的固定列"变成"用户能拖、
/// 且存得住"的列之后，宽度成了一份**会被用户改的状态**——界面常量与落盘值
/// 各留一份必然有一帧对不上。老配置没这个字段时由 core 的
/// `DEFAULT_INSPECTOR_W` 兜底，`Shell::new` 里夹一次范围，界面只管用
/// `Shell::inspector_w`。侧栏 `SIDEBAR_MIN_W/MAX_W` 是同一个做法
pub(crate) const INSPECTOR_MIN_W: f32 = qingbird_core::store::INSPECTOR_W_RANGE.0;
pub(crate) const INSPECTOR_MAX_W: f32 = qingbird_core::store::INSPECTOR_W_RANGE.1;
/// 侧栏宽度上下限。与 core 的 `SIDEBAR_W_RANGE` 同一份定义——
/// 界面夹一遍、落盘再夹一遍，值域只有一个出处。
/// 默认宽度不在这里另写一份：老配置没这个字段时由 core 的
/// `DEFAULT_SIDEBAR_W` 兜底，界面只管夹范围
pub(crate) const SIDEBAR_MIN_W: f32 = qingbird_core::store::SIDEBAR_W_RANGE.0;
pub(crate) const SIDEBAR_MAX_W: f32 = qingbird_core::store::SIDEBAR_W_RANGE.1;
/// 窗口下限宽度。1080 − 轨 44 − 侧栏 200 = 主区 836px，右栏展开时再减 320 仍有 516px
pub(crate) const WIN_MIN_W: f32 = 1080.0;

/// 窗口下限**高度**。**720 而不是 640**：概览页底下两段（同步 / 语言技术栈）
/// 按内容高固定、压不动，日志吃剩下的（`flex_1` + `min_h(LOG_MIN_H)`）。
/// 640 高时日志只剩两百多像素，扣掉表头与状态行不到十行输出，
/// 而"日志是这一屏占最大、最重要的一块"就成了一句空话。
///
/// 「常用命令」段撤掉之后这里多出约 90px（段高 82 + 一个 `GAP`），
/// 但**仍不足以把下限降回 640**——真要降，先在 640 高抓一帧看日志还剩几行，
/// 别凭算式拍。这条数值的来历只有这一句，别把它抄到别处当"三段仪表盘"的依据
pub(crate) const WIN_MIN_H: f32 = 720.0;

/// 应用图标边长（标题栏与窗口左上角那一枚）。
///
/// 用的是老 Tauri 版那张 `icon.ico` 里的 128px 位图（`gui/assets/app-icon.png`），
/// 不是 `assets` 里那只 `Bird`——品牌真正在用的是「带 git 分支的立体箱子」，
/// 照名字脑补一只鸟是从第一秒就错的方向（D39）。
pub(crate) const APP_ICON: f32 = 22.0;

/// 标题栏左边距。**由轨宽反推**，不是另填一个数：
/// 标题栏那枚图标要与导航轨那一列的图标**中心线对齐**，而轨的中心线
/// 就在窗口内容区的 `NAV_W / 2` 处。所以左边距取 `NAV_W/2 − APP_ICON/2`
/// 就恒等成立，改了 `NAV_W` 或 `APP_ICON` 也不会错位。
///
/// （官方 `TitleBar` 自带 `pl(12)`，这里用样式覆盖——`refine_style`
/// 排在官方 `.pl(..)` 之后，覆盖得住。2026-10-03 实机实测：图标中心
/// 29.5 vs 轨中心 35.5，差 6px，肉眼一眼能看出来"没对齐"。）
pub(crate) const TITLE_BAR_PAD_L: f32 = NAV_W / 2.0 - APP_ICON / 2.0;

/// 窗口控件（最小化 / 最大化 / 关闭）单键的边长。
///
/// 官方 `ControlIcon` 是**私有 enum**（`title_bar.rs:111`），导不出来复用，
/// 它那套宽度写死成 `TITLE_BAR_HEIGHT`（= 34px，`title_bar.rs:15`/`:211`）。
/// 自研标题栏时这一档只能自己定，**但对齐官方那个值**（34）：
/// Windows 的窗口控件命中区是系统约定的固定宽度，跟着官方的走，
/// 换成一个"我觉得合适"的数只会让这块和系统其余窗口不一致。
///
/// ⚠️ 这三个键**只声明 `window_control_area`，不自己处理点击**（见
/// `shell.rs::window_controls`）：命中与行为都走系统那侧的 `HTMINBUTTON` /
/// `HTMAXBUTTON` / `HTCLOSE`。自己 `on_click` 调 `remove_window()` 看着一样，
/// 行为不一样——那条路绕过 `WM_CLOSE`，托盘唤起与 `QuitMode::Explicit`
/// 那一套就接不上了
pub(crate) const TITLE_BAR_CTRL_W: f32 = 34.0;

/// 行内图标（统一走 lucide 一套，只有品牌 logo 用 assets 的 Bird）。
/// 尺寸显式写死而不是 `.small()`：两套图标集混着用时，默认尺寸并不一致，
/// 显式档位才能保证行与行对齐。
pub(crate) fn icon_sm(name: impl Into<Icon>) -> Icon {
    Icon::new(name).with_size(Size::Size(px(ICON_SM)))
}

/// 按钮内图标
pub(crate) fn icon_md(name: impl Into<Icon>) -> Icon {
    Icon::new(name).with_size(Size::Size(px(ICON)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 导航轨的底是白卡（`sidebar.background` = #FFFFFF），设计规范板的选中面是
    /// `sidebar.accent.background` = #EDEBE7。**遮罩叠上去必须落在这附近**——
    /// 差太远就说明 `SELECTED_MASK_ALPHA` 跟设计规范板脱钩了，
    /// 也说明"导航轨的选中面"和"侧栏仓库行的选中面"已经不是同一个东西。
    ///
    /// 这里的字面量是**规范板**的值，不是代码里任何一处的值：主题真源在
    /// `gui/theme.json`，本测试钉的是"遮罩与规范板对得上"这条**关系**。
    #[test]
    fn mask_lands_on_the_design_boards_selection_face() {
        let ink: Hsla = rgb(0x1A1A1A).into(); // theme.json: foreground
        let card: Hsla = rgb(0xFFFFFF).into(); // theme.json: sidebar.background
        let board: Hsla = rgb(0xEDEBE7).into(); // 设计规范板: 选中面
        let got = card
            .to_rgb()
            .blend(ink.opacity(SELECTED_MASK_ALPHA).to_rgb());
        let want = board.to_rgb();
        let gap = |a: f32, b: f32| ((a - b) * 255.0).abs();
        let worst = gap(got.r, want.r)
            .max(gap(got.g, want.g))
            .max(gap(got.b, want.b));
        assert!(
            worst <= 8.0,
            "遮罩叠白卡得 {got:?}，规范板选中面是 {want:?}，差 {worst:.1}/255——\
             这两个数必须看着一样，否则导航轨的选中面和侧栏仓库行的选中面已经分家了"
        );
    }

    /// 导航轨的**三处**内边距各由不同的理由钉死：横向被"装得下按钮"钉死、
    /// 纵向顶部是对齐基线、底部是"独立一组的气口"（D58 / D59）。
    ///
    /// 这条测试守的是**同一个错犯过两次**：早先顶与底共用 `NAV_PAD`（6），
    /// 看着"logo 和三颗图标糊成一片"；修完顶部又忘了底部，齿轮直接坐在状态条上。
    /// 三个数一旦被图省事并成一个，这里立刻红。
    #[test]
    fn the_three_rail_paddings_never_collapse_into_one() {
        let all = [NAV_PAD, NAV_PAD_TOP, NAV_PAD_BOTTOM];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(
                    all[i], all[j],
                    "导航轨的三处内边距不能共用同一个数，现在都是 {}：{all:?}",
                    all[i]
                );
            }
        }
    }
}
