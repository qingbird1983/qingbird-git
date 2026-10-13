//! 总览页：页头 + 四张状态卡 + 活跃度柱状图 + 仓库汇总表。
//!
//! **只做统计，不做批量。** 旧版本这里有复选框列、底部批量条、两个「全部拉取/推送」
//! 按钮——三种入口干同一件事，用户得先想清楚"这次要批量还是要单点"。
//! 阶段 6 全部删掉，批量改由 ⌘K 承载（阶段 11）：命令面板本来就是"挑几行做一件事"的地方，
//! 在表格里另造一套勾选语法只是让同一个动作有两种心智模型。
//!
//! 唯一一处自研的整形，是 `fill_days`：**补齐 14 天窗口**——
//! 后端只给有提交的日子，空白天要画成 0 高度的柱子而不是直接不画。
//! **窗口右端必须是「本地的今天」**（`local_day_key`）：core 那边的
//! 日期串是 `git log --date=format-local` 算出来的本地日期，
//! 两边口径必须一致，否则**凌晨 0–8 点今天的提交会落在窗口外**——
//! 那一段曾经正是这么错的，见 `local_day_key` 的注释。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::chart::AreaChart;
use gpui_kit::component::table::{Column, DataTable, TableDelegate, TableState};
// `.when(..)` 来自 gpui 的 `FluentBuilder`（`util.rs:21`），不在 Button 的任何一个
// trait 上——少了这一行的报错是"no method named when"，看着像官方没有这个能力
use gpui_kit::prelude::FluentBuilder as _;
// 这三个 `as _` 各管一颗键上的一段：`Selectable` 给 `.selected(..)`（`button.rs:538`）、
// `Disableable` 给 `.disabled(..)`，而 `.when(..)` 在 gpui 的 `FluentBuilder`（`util.rs:21`）。
// ⚠️ 官方 `Button` 的 builder 方法**散在四个 trait 上**（还有一个 `ButtonVariants`），
// 少导入哪一个，报错都写成"no method named …"——看着像官方没这个能力，
// 其实只是这个文件没把那条 trait 拿进来。抄样板时**连导入一起抄**
use gpui_kit::component::{ActiveTheme, Disableable as _, Selectable as _, Sizable, Size, Theme};
use gpui_kit::{
    div, px, AnyElement, App, Context, DefiniteLength, Div, Entity, FontWeight, IntoElement,
    ParentElement, Styled, Window,
};
use chrono::{DateTime, Duration, Local};
use qingbird_core::repos::ActivityDay;

use crate::sidebar::BadgeKind;
use crate::state::*;
use qingbird_core::store;
use crate::tokens::*;
use crate::widgets::*;

/// 波形图的窗口天数。设计文档第 3.2 节定的 14 天
pub(crate) const ACTIVITY_DAYS: u32 = 14;

// ================= 表格列宽 =================

/// 七列的基准宽（仓库名 / 归属 / 分支 / 状态 / 落后 / 待推送 / 未提交）。
/// `ov_column_widths` 的输入，也是窗口不够宽时各列的下限参照
pub(crate) const OV_COL_BASE: [f32; 7] = [220., 84., 120., 104., 64., 72., 72.];
/// 各列硬下限。低于这个值表头就断字了，比留白更难看
pub(crate) const OV_COL_MIN: [f32; 7] = [140., 68., 80., 88., 56., 60., 60.];
/// 放大倍数上限。
///
/// 「跟随窗口等比缩放」照字面做会出事：基准七列合计 736px，
/// 而宽屏的内容区能有 3300px 多，等比就是 **4.6 倍**——
/// 「归属」那列会变成 385px 宽的一片空白，比不改还难看。
/// 所以除「仓库名 / 分支」外的列最多长到 `1.6 ×` 基准，
/// 再宽的余量全给那两列——它们本来就是会随窗口变长的文本列
pub(crate) const OV_COL_SCALE_MAX: f32 = 1.6;

/// 表格外面那一圈固定开销（从窗口宽里扣掉的那一条）：**逐项列出来，不要写凑出来的数**。
///
/// 2026-10-08 这一条错过一次：给表格卡补上 `card()` 的 `p(GAP_XL)` 之后，
/// 卡内左右各吃掉 16，而这一项还是旧的 83 ⇒ 列宽合计比可用宽多 32 ⇒
/// **整张卡顶到窗口右缘**（用户定的"右 9"实测变 0）。
/// 旧的 `2. * GAP_LG` 是"页面左右内边距各 12"，那一格现在归壳层（左 0 + 右 `GAP`）。
///
/// **宁可多扣也不要少扣**（D56）：少扣 = 合计大于可用宽度 = 冒出横向滚动条，
/// 而横向滚动条会把内容区再压窄——那正是自激的起点。多扣只是右边空十几像素，
/// 是一次定死的静态状态，不会自己动
pub(crate) const OV_TABLE_CHROME: f32 =
    NAV_W + 1. + GAP + 2. * GAP_XL + 2. + 16.;

/// 把可用宽度摊到七列上，合计正好等于 `avail`。
///
/// 三步，顺序不能换：
/// 1. 先按 `avail / 基准合计` **等比**缩放——窗口在常见区间里这就是纯等比，
///    七列的相对关系完全不变；
/// 2. 缩放倍数夹到 `OV_COL_SCALE_MAX`，同时把每列抬到 `OV_COL_MIN`
///    （窗口窄到极致时等比会把表头压断字）；
/// 3. 还剩的余量按「仓库名 : 分支」的基准比例补给这两列，
///    保证**七列合计恰好铺满可用宽度**。
///
/// 第 3 步是必须的：合计比可用宽度少一点，卡片右边就空一条；
/// 多一点则冒出横向滚动条，表格会左右晃。
///
/// **窄到各列下限开始顶住时铺不满**，这时停在下限上、让表格自己出横向滚动条
/// ——下限是为了表头不断字，优先级更高。合计只会**偏宽不会偏窄**：
/// 偏宽有滚动条告诉用户"还有"，偏窄则是右边一条说不清缘由的空。
/// 窗口下限 1080 时可用宽度约 1009px，走不到这一档，但纯函数得把
/// 那一档的行为钉住，不能留一个"到时候再说"的分支。
///
/// `avail` 传的是**窗口宽扣掉外面那一圈**（`OV_TABLE_CHROME`）之后的值。
///
/// ⚠️ **它必须是窗口宽的纯函数**（D56）：一旦让"表格当前的实际宽度"参与进来，
/// 就成了正反馈环——列宽一大出横向滚动条、内容区变窄、下一次量到更小的值，
/// 界面于是持续往右拉伸。纯函数 + 开环输入，窗口不动列宽就不动。
pub(crate) fn ov_column_widths(avail: f32) -> [f32; 7] {
    let base_total: f32 = OV_COL_BASE.iter().sum();
    if !(avail > 0.0) || base_total <= 0.0 {
        return OV_COL_BASE;
    }
    let scale = (avail / base_total).min(OV_COL_SCALE_MAX);
    let mut w = [0f32; 7];
    for i in 0..7 {
        w[i] = (OV_COL_BASE[i] * scale).max(OV_COL_MIN[i]);
    }
    let slack = avail - w.iter().sum::<f32>();
    if slack > 0.0 {
        let (name_base, branch_base) = (OV_COL_BASE[0], OV_COL_BASE[2]);
        let flex_total = name_base + branch_base;
        w[0] += slack * name_base / flex_total;
        w[2] += slack * branch_base / flex_total;
    }
    w
}

// ================= 表格委托 =================

/// 官方 `DataTable` 的数据源。
///
/// **不存数据，只存索引**：真实行在 `Shell::ov_rows`，委托带着一个
/// `Entity<Shell>` 回读。这样"数据变了"只需要改 `Shell` 那一份，
/// 不会出现委托里存一份、Shell 里存一份、两者差一帧的情况
///
/// 唯一自己存下的是**七列的当前宽度**——那是布局量，不在 `Shell` 里。
/// 存在委托里而不是每帧从 `Shell` 读，是因为 `Shell::render` 里同步刷新列宽
/// 时要去 `update` 这张表，而那一刻 `Shell` 自己的数据正被可变借用着，
/// 再 `shell.read(cx)` 就是一次借冲突（同一条约束见 D45）
pub(crate) struct OvDelegate {
    shell: Entity<Shell>,
    columns: Vec<Column>,
    widths: [f32; 7],
}

impl OvDelegate {
    pub(crate) fn new(shell: Entity<Shell>) -> Self {
        // 七列。
        //
        // **不要给数字列写 `text_right()`**：基座里 `Column::align` 只有
        // `table::Table` 那条路（`table.rs` 的 `TableCell`）会读，
        // `DataTable` 走的 `TableState` 全程不看这个字段——写了不报错、
        // 也不生效，是一句会骗人的代码（2026-10-03 查 `state.rs` 确认）。
        // 数字列要真右对齐得在 `cell()` 里自己摆 `justify_end`，
        // 而那样表头还是左对齐的，两边反而对不齐，所以这里统一左对齐 +
        // 默认内边距：表头与数字同一起跑线。
        //
        // 同样**不要** `p_0()`：它确实生效（`state.rs` 读了 `column.paddings`），
        // 但去掉内边距后数字会紧贴左边那列的状态 pill 尾巴，
        // 「未提交 49 —」读成一串。留默认内边距。
        //
        // 这里只给**基准宽**；真实宽度每次 `column()` 都按 `self.widths`
        // 覆盖（见 `ov_column_widths`）
        let columns = vec![
            Column::new("name", "仓库名")
                .width(OV_COL_BASE[0])
                .resizable(true)
                .min_width(OV_COL_MIN[0]),
            Column::new("group", "归属").width(OV_COL_BASE[1]),
            Column::new("branch", "分支")
                .width(OV_COL_BASE[2])
                .resizable(true)
                .min_width(OV_COL_MIN[2]),
            Column::new("status", "状态").width(OV_COL_BASE[3]),
            Column::new("behind", "落后").width(OV_COL_BASE[4]),
            Column::new("ahead", "待推送").width(OV_COL_BASE[5]),
            Column::new("changed", "未提交").width(OV_COL_BASE[6]),
        ];
        Self {
            shell,
            columns,
            widths: OV_COL_BASE,
        }
    }
}

impl TableDelegate for OvDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, cx: &App) -> usize {
        self.shell.read(cx).ov_rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        let mut c = self.columns[col_ix].clone();
        // 真实宽度在这里覆盖基准宽。`TableState` 只在 `prepare_col_groups`
        // 时问一次列宽，所以宽度变了要 `refresh()`（见 `sync_ov_col_widths`）
        c.width = px(self.widths[col_ix]);
        c
    }

    /// 单元格。主题从 `Context<TableState>` 里取——委托拿不到 `&Theme`，
    /// 而 `ActiveTheme` 对 `Context<T>` 有实现，`cx.theme()` 即可
    // 委托签名要求返回 `impl IntoElement`，这里收窄成 `AnyElement` 是安全的
    // （`render_td` 的实现体只有两种出口，都是 `AnyElement`），但要让编译器闭嘴
    // 得显式说一句，否则每次编译都飘一条 `refining_impl_trait` 告警
    #[allow(refining_impl_trait_internal)]
    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> AnyElement {
        let t = cx.theme().clone();
        let Some(row) = self.shell.read(cx).ov_rows.get(row_ix).cloned() else {
            return div().into_any_element();
        };
        cell(col_ix, &row, &t)
    }
}

/// 一格的内容。右对齐的数字列统一用等宽字体——数字在等宽里才能竖着比大小
fn cell(col_ix: usize, row: &OvRow, t: &Theme) -> AnyElement {
    let num = |s: String| {
        div()
            .font_family(t.mono_font_family.clone())
            .text_size(px(fs_sec()))
            .text_color(tier3(t))
            .child(s)
    };
    match col_ix {
        // 名称：状态字形 + 名字。字形与侧栏是同一份 `bucket_of`（D13）
        0 => h_flex()
            .gap(px(GAP_TIGHT))
            .items_center()
            .min_w_0()
            .child(
                icon_sm(crate::sidebar::bucket_icon(row.bucket))
                    .text_color(dot_color(row.bucket, t)),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(fs_body()))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.colors.foreground)
                    .child(row.name.clone()),
            )
            .into_any_element(),
        1 => div()
            .text_size(px(fs_sec()))
            .text_color(tier3(t))
            .child(if row.third { "第三方" } else { "我的" })
            .into_any_element(),
        2 => div()
            .font_family(t.mono_font_family.clone())
            .text_size(px(fs_sec()))
            .text_color(tier3(t))
            .child(if row.branch.is_empty() {
                "—".to_string()
            } else {
                row.branch.clone()
            })
            .into_any_element(),
        3 => {
            let (_, fg, bg) = crate::sidebar::badge_tone(row.bucket, row.changed, row.third, t);
            // ⚠️ **外面必须包一层 `h_flex`**：单元格里那个 `Div` 会被给到整列宽，
            // 于是这枚胶囊**铺满整列**、看着像一格输入框（阶段 31.4 抓帧查实）。
            // 修在调用侧，不给 `pill` 加宽度开关——它在侧栏行后缀那一格是好的，
            // 那里父容器本身就是按内容排的横排
            h_flex().child(pill(&row.status, fg, bg)).into_any_element()
        }
        4 => num(count_cell(row.behind)).into_any_element(),
        5 => num(count_cell(row.ahead)).into_any_element(),
        6 => num(count_cell(row.changed)).into_any_element(),
        _ => div().into_any_element(),
    }
}

/// 0 显示成 `—` 而不是 `0`。「落后 0」和「没落后」在用户眼里不是一回事：
/// 前者是"确认过没有"，后者是"还不知道"
fn count_cell(n: u32) -> String {
    if n == 0 {
        "—".to_string()
    } else {
        n.to_string()
    }
}

// ================= 状态卡 =================

impl Shell {
    /// 四张状态卡。**不加边框不加阴影**，只靠背景色与间距分组——
    /// 这一屏已经有表格的横线当结构了，再给每张卡套一圈框会变成"框里套框"
    fn stat_cards(&self, t: &Theme) -> Div {
        let count = |f: &dyn Fn(&RepoView) -> bool| {
            self.repos
                .iter()
                .filter(|r| self.views.get(&r.path).is_some_and(f))
                .count()
        };
        h_flex()
            .w_full()
            .flex_shrink_0()
            // 卡间 4（帧上视觉 6）——与外层那条 gap 同一个数，**四张卡彼此、四张卡与
            // 下面两张卡，全屏幕只有这一档卡间**（2026-10-08 用户定"统一成 6px"）
            .gap(px(GAP_TIGHT))
            // 四张卡各配一枚 lucide 图标（阶段 21-8）：**它们说的是这张卡在讲什么**，
            // 而不是又一次重复右边那个数字。挑法是"状态"而不是"动作"——
            // 与同步三键那几个动词图标（Download/Upload/RefreshCw）不混，
            // 否则「已同步」那枚会和「拉取」那枚长得一样、看不出分别。
            .child(stat_card(
                count(&|v| bucket_of(&v.status) == Bucket::Synced),
                "已同步",
                t.colors.success,
                AssetIcon::CircleCheck,
                t,
            ))
            .child(stat_card(
                count(&|v| bucket_of(&v.status) == Bucket::Behind),
                "落后云端",
                orange(t),
                AssetIcon::CloudDownload,
                t,
            ))
            .child(stat_card(
                count(&|v| bucket_of(&v.status) == Bucket::Ahead),
                "有本地提交",
                t.colors.accent,
                AssetIcon::CloudUpload,
                t,
            ))
            .child(stat_card(
                count(&|v| bucket_of(&v.status) == Bucket::Forked),
                "已分叉",
                t.colors.danger,
                AssetIcon::GitFork,
                t,
            ))
    }
}

// ================= 活跃度波形图 =================

/// 波形图的一个点要的数据。**自带日期键**而不是只带计数——
/// 官方 `AreaChart` 的 x 轴值只能从 datum 本身取
#[derive(Clone)]
pub(crate) struct ActivityBar {
    pub(crate) date: String,
    pub(crate) count: u32,
}

/// 活跃度**图区**的高度。**不是拍的，是从改前的帧上量出来的**：
/// 这张卡原来写死 `.h(px(160.0))`，实测卡内那一格图区占 y=272..377 = **105**，
/// 而图真正画掉的只有 99（同一帧上网格线 4 条在 y=276/297/318/339、间距 21，
/// 轴基线 y=360，x 轴标签底 y=375）。
/// 卡高改由内容撑之后，这一档就是图区自己的高度，取整到 **100**。
/// ⚠️ 它是**图区**的高，不是**卡**的高——卡由「内边距 + 表头 + gap + 这一档」
/// 推出来，写死卡高才会出现"卡 160 而图只有 99、多出来的一截全成了空白"那种账
const OV_CHART_H: f32 = 100.0;

impl Shell {
    /// 近 14 天全仓库提交数。空数据给一句说明，不画一张空图——
    /// 空坐标系看起来像"程序坏了"
    /// 总览页的波形图。`panels.rs` 里还有一个同名的（单仓库的迷你柱状图），
    /// 两者数据源不同：那个读 `ProfileState`，这个读全仓库合并的结果
    fn ov_activity_chart(&self, t: &Theme) -> Div {
        let bars = fill_days(&self.ov_activity, ACTIVITY_DAYS);
        // 卡头右上那枚胶囊报的是**总数**——柱状图只给得出形状，给不出"这两周一共多少次"
        let total: u32 = bars.iter().map(|b| b.count).sum();

        let card = card_shell(
            v_flex()
                .w_full()
                .flex_shrink_0()
                // 卡高**不写死**：由「上下内边距 + 表头行 + gap + 图区 `OV_CHART_H`」
                // 推出来。原来这里钉着 `.h(px(160.0))`，比内容高出一截，
                // 多出来的那一段全变成图区上下的空白（2026-10-07 用户挑出来的那条）
                .p(px(GAP_LG))
                .gap(px(GAP_TIGHT)),
            t,
        );
        // 外壳那五样走 `card_shell`（与 `card()` / `stat_card()` 同一个出处）。
        // 原来这里手搓了第二份 `rounded + bg + border + shadow`，正是机器闸 R1
        // 那一类"同一属性两个出处"——描边色那次翻车就是这三份里两份 `track`、
        // 一份 `border`，一屏三种卡两种边重
        let card = card
            // 表头走全站唯一的 `CardHeader`（原来这里手搓了一份 h_flex：
            // 标题 + 附注，右侧什么都没有）。**卡头右上那枚胶囊是原型有的，
            // 我一直没落**——它报的是"这两周一共多少次提交"，
            // 而柱状图本身只看得出形状、看不出总数
            .child(
                CardHeader::new("提交活跃度")
                    .note(format!("近 {ACTIVITY_DAYS} 天 · 全部仓库合计"), false)
                    .right(
                        pill_with(
                            Some(AssetIcon::TrendingUp),
                            &format!("{} 次提交", total),
                            t.colors.accent,
                            wash(t),
                            None,
                        )
                        .into_any_element(),
                    )
                    .build(t),
            );

        if bars.is_empty() {
            return card.child(
                div()
                    // 空态也占**同一个图区高度**：不给高的话卡会塌成一条，
                    // 有数据与没数据就变成两张不同高的卡
                    .h(px(OV_CHART_H))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(fs_meta()))
                    .text_color(tier4(t))
                    .child("近两周没有提交记录"),
            );
        }

        let accent = t.colors.accent;
        // 波形底下那层不是同一种蓝：整块实心 accent 会把网格线吃掉，
        // 曲线反而读不出来。压到 12% 让它只当"底"
        let wash = accent.opacity(0.12);
        // 官方 `AreaChart` **没有实现 `Styled` / `ParentElement`**。好在外层的
        // `PlotElement::request_layout` 写死了 `Size::full()`，图表自己会填满父容器，
        // 所以这里只要给它一个撑开的 div 就够，不用也不能调 `.h_full()`
        //
        // 曲线默认就是 `Curve::Natural`（`plot/mod.rs:229-238`，d3 的
        // curveNatural），所以**不用显式 `.natural()`**——写出来也只是把
        // 默认值再说一遍。
        //
        // 图区给的是**明确高度** `OV_CHART_H` 而不是 `flex_1`：外层卡高现在由
        // 内容撑，`flex_1` 在没有固定高的父容器里会塌成 0（`PlotElement` 只会
        // 填满父容器，不会自己挣出高度）
        card.child(
            div().h(px(OV_CHART_H)).child(
                AreaChart::new(bars)
                    .id("ov-activity")
                    // 14 个 x 标签全写日期会挤成一团：只留 M/D。
                    // 完整日期在 tooltip 标题上，不靠轴标签传达
                    .x(|b| short_day(&b.date))
                    .y(|b| b.count as f64)
                    .stroke(accent)
                    .fill(wash)
                    .x_axis(true)
                    .y_axis(false)
                    .grid(true)
                    .interactive(true)
                    .tooltip_title(|b| b.date.clone().into())
                    .tooltip_value(|b, _, _| format!("{} 个提交", b.count).into()),
            ),
        )
    }
}

// ================= 页面 =================

impl Shell {
    /// 把 `repos` + `views` 压成表格要的扁平行。
    ///
    /// 每一帧都重算：数据量就是仓库数（几十），压一遍是微秒级，
    /// 而"只在变化时更新"要自己判断什么算变化——判断错了就是表格少一行，
    /// 而少一行是那种用户不会报、只会自己绕开的 bug
    pub(crate) fn sync_ov_rows(&mut self) {
        // ⚠️ **整批用同一个 `now`**：逐颗取时钟的话，一帧里跨过一次秒界就会让
        // 90 天边界那一档在同一张表里给出两个答案（片上的数与点进去的行对不上）
        let now = store::now_secs();
        let mut rows: Vec<OvRow> = self
            .repos
            .iter()
            .map(|e| {
                let v = self.views.get(&e.path).unwrap_or(&EMPTY_VIEW);
                let b = bucket_of(&v.status);
                let status = ov_status_text(b, v.changed, e.is_third_party);
                OvRow {
                    path: e.path.clone(),
                    name: qingbird_core::scheduler::display_name(e),
                    third: e.is_third_party,
                    branch: v.status.branch.clone(),
                    bucket: b,
                    status,
                    behind: v.status.behind,
                    ahead: v.status.ahead,
                    changed: v.changed,
                    last_used: e.last_used,
                    sync_age: qingbird_core::scan_cache::sync_age(
                        &self.last_sync,
                        &e.path,
                        now,
                        qingbird_core::scan_cache::STALE_DAYS,
                    ),
                }
            })
            .collect();
        // 片上那两个数**必须在过滤之前算**（`stale_counts`），而且存进 `Shell`
        // 让 render 只读不算：留在 render 里现算就得再拿一份行集合，
        // 而那份与过滤后的那份迟早会漂
        self.ov_stale_counts = stale_counts(&rows);
        // 「久未同步」那片按下去就只剩那一档（判据在 `ov_row_shows`，
        // 与片上那两个数同一颗函数——两处各写一份会出现"片上写 3、点开两行"）
        let stale_only = self.ov_stale_only;
        rows.retain(|r| ov_row_shows(r.sync_age, stale_only));
        // 行序由卡头那颗键定（两档，见 `OvSort`）。**名称那一档不区分大小写**——
        // 仓库名里拉丁与中文混着，直接 `cmp` 会把大写全排到小写前面，看着像没排
        match self.ov_sort {
            OvSort::LastUsed => rows.sort_by(|a, b| b.last_used.cmp(&a.last_used)),
            OvSort::Name => {
                rows.sort_by(|a, b| {
                    a.name.to_lowercase().cmp(&b.name.to_lowercase())
                })
            }
        }
        self.ov_rows = rows;
    }

    /// 让总览表格的七列宽度跟着窗口走。
    ///
    /// **量的是窗口宽度**（减去外面那一圈固定开销），**不是**表格自己的
    /// 内容区宽度——后者是被列宽决定的，拿来当输入就会自激，推导见下面注释。
    ///
    /// **为什么宽度存进委托而不是 `Shell`。** 这一段跑在 `Shell::render` 里面，
    /// 此刻 `Shell` 的数据正被可变借用着；改宽度要 `update` 这张表，
    /// 而 `TableState::refresh` 内部会回调 `delegate.column()`。
    /// 委托要是那时去 `shell.read(cx)` 读行数据，就是一次借冲突。
    /// 行数据仍然每次 `render_td` 读（那时 `Shell::render` 早就返回了，安全）。
    ///
    /// `cx.notify()` 只在宽度真的变了时调：`refresh` 自己不通知，不补这一下
    /// 新宽度要等到下一次别的东西触发重绘才生效，拖窗口时会明显滞后
    pub(crate) fn sync_ov_col_widths(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // ⚠️ **必须量窗口宽度，不能量表格自己的内容区宽度**（D56，2026-10-03 实机）。
        // 表格的可用宽度是被**列宽**决定的，拿它当输入就是一个正反馈环：
        //   列宽合计一大 → 出横向滚动条 → 内容区变窄 → 量到的 avail 变小
        //   → 列宽变小 → 滚动条消失 → avail 变大 → 列宽又大 → ……
        // 而鼠标在窗口里动一下就会触发一次重绘，于是界面**持续往右拉伸**、
        // 拉到窗口外面去。单测抓不到它（纯函数本身是对的），只有真跑起来才看得见。
        //
        // 开环就没有这个问题：列宽是窗口宽的纯函数，窗口不动它就不动。
        // 代价是外面那一圈固定开销（`OV_TABLE_CHROME`）要自己算准，
        // 算多了右边空一条、算少了冒出横向滚动条——但那是**一次定死**的静态状态，
        // 不会自己来回动。
        let avail = window.viewport_size().width - px(OV_TABLE_CHROME);
        let want = ov_column_widths(avail.as_f32());
        let cur = self.ov_table.read(cx).delegate().widths;
        if (0..7).all(|i| (cur[i] - want[i]).abs() < 0.5) {
            return;
        }
        self.ov_table.update(cx, |t, cx| {
            t.delegate_mut().widths = want;
            // 列宽是 `TableState` 缓存的（`col_groups`），只在 prepare/refresh
            // 时问一次委托；不 refresh 的话改了等于没改
            t.refresh(cx);
            cx.notify();
        });
    }

    /// 扫全仓库的近 14 天提交数。**只在进总览页时扫**，切走不扫——
    /// 每个仓库一条 `git log`，仓库多了不便宜，而用户多半只是路过看一眼
    pub(crate) fn scan_overview_activity(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<String> = self.repos.iter().map(|r| r.path.clone()).collect();
        self.ov_seq += 1;
        let seq = self.ov_seq;
        self.ov_task = None;
        if paths.is_empty() {
            self.ov_activity.clear();
            return;
        }
        self.ov_task = Some(cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(
                    async move { qingbird_core::repos::commit_activity_all(&paths, ACTIVITY_DAYS) },
                )
                .await;
            this.update(cx, |this, cx| {
                if this.ov_seq != seq {
                    return; // 已有新一轮，这批过期
                }
                this.ov_activity = r;
                cx.notify();
            })
            .ok();
        }));
    }

    /// 总览页。数据全部来自已有状态缓存（状态扫描那份 + 后台算的活跃度），不另扫一遍
    pub(crate) fn overview_page(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        // 表格那张卡**原来没有卡头**——原型有（`.card-head`：标题「全部仓库」+ 右上那颗排序键），
        // 而我一直在拿页头那句「N 个仓库 · …」当它的说明。2026-10-08 照原型补回来。
        // ⚠️ 空态与有数据**共用同一个卡头**，只差右上那颗键：没有行可排的时候
        // 摆一颗排序键就是一颗点了没反应的假键
        let head = CardHeader::new("全部仓库");
        let (body, head) = if self.repos.is_empty() {
            (
                list_box(v_flex().w_full().flex_1().min_h_0(), t)
                    // 与有数据那一格同一个 `list_box`（原来这里自己写了一份
                    // `rounded + bg(popover) + border(track)`，而 `popover.background`
                    // 在两条主题里都等于卡面白 ⇒ 那个 `.bg()` 是个空操作，
                    // 却让人以为"空态盒比表格深一档"）
                    .child(
                        crate::widgets::empty_state(
                            "还没有仓库",
                            "回到「我的仓库」加一个，总览才有东西可看",
                            t,
                        )
                        .into_any_element(),
                    )
                    .into_any_element(),
                head,
            )
        } else {
            // 「久未同步」那片（32-11）。它是**这张表的一个读法**，所以跟排序那颗键
            // 同挂卡头右上、不另起一行浮在外面；选中面沿用侧栏那颗筛选键那一套
            // （`ghost` + `selected` + `wash`，D60 同一个），不自立第三种
            let (over, none) = self.ov_stale_counts;
            // ⛔ **读不出来时这一档不可用，而不是"零颗"**：那时整张表对"上次同步"
            // 没有任何知识，一片下去会把每一颗都说成三个月没同步——那不是少一个数，
            // 是一屏假话（`SyncAge::Unknown` 那一档就是为了这一格存在的）
            let unreadable = !sync_table_is_readable(&self.last_sync);
            let chip = Button::new("ov-filter-stale")
                // ⛔ 读不出来时这颗键上**不写 0**（判据与理由在 `chip_label`）
                .label(chip_label(!unreadable, over + none))
                .custom(ghost_variant(cx))
                .with_size(Size::Small)
                .selected(self.ov_stale_only && !unreadable)
                .when(self.ov_stale_only && !unreadable, |b| b.bg(wash(t)))
                .disabled(unreadable)
                .tooltip(if unreadable {
                    "同步记录读不出来，这一档暂时用不了".to_string()
                } else {
                    format!(
                        "只看这 {} 颗：{over} 颗超过 90 天没和云端交换过 · \
                         {none} 颗从没有过成功的同步记录",
                        over + none
                    )
                })
                .accessibility_label("只看久未同步的仓库")
                .on_click(cx.listener(|this, _, _, cx| {
                    // 禁用态官方已经不吃点击，这里再挡一次是防"哪天改成不禁用"
                    // 时把读不出来那一档当成"零颗"筛（同 32-8 那颗停止键的写法）
                    if !sync_table_is_readable(&this.last_sync) {
                        return;
                    }
                    this.ov_stale_only = !this.ov_stale_only;
                    this.sync_ov_rows();
                    cx.notify();
                }))
                .into_any_element();
            let sort_btn = Button::new("ov-sort")
                .icon(icon_sm(AssetIcon::ListFilter).text_color(tier3(t)))
                .label(self.ov_sort.label())
                .custom(ghost_variant(cx))
                .with_size(Size::Small)
                .tooltip("切换排序：按最后使用 / 按名称")
                .accessibility_label("仓库排序")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.ov_sort = this.ov_sort.next();
                    this.sync_ov_rows();
                    cx.notify();
                }))
                .into_any_element();
            // 官方 `DataTable` 同样没有 `Styled`，撑高交给外层
            (
                // ⚠️ **被那片筛空了这一格要自己说话**。`repos` 非空而 `ov_rows` 空
                // 只有这一个原因（筛没了），而官方 `DataTable` 在这种时候给一句
                // "No data"——英文、且读起来像表格坏了。
                // **新增一档状态，先问"没数据那一档显示什么"**（AGENTS 那条）
                if self.ov_rows.is_empty() {
                    list_box(v_flex().w_full().flex_1().min_h_0(), t)
                        .child(
                            crate::widgets::empty_state(
                                "没有久未同步的仓库",
                                "再点一次上面那颗「久未同步」，回到全部仓库",
                                t,
                            )
                            .into_any_element(),
                        )
                        .into_any_element()
                } else {
                list_box(div().w_full().flex_1().min_h_0(), t)
                    // `overflow_hidden` 不进 `list_box`：它是这一格**特有**的
                    // （`DataTable` 自己画方角的行底与表头，不裁就顶出圆角），
                    // 而空态那一格没有要裁的东西
                    .overflow_hidden()
                    .child(
                        // 官方 `DataTable` 没有 `accessibility_label`（那是 `table::Table`
                        // 才有的），裸 `div` 也没有——所以这块不硬凑一个可访问名，
                        // 卡头那句「全部仓库」已经把这张表在讲什么说清了
                        div().size_full().child(
                            DataTable::new(&self.ov_table)
                                // **行高从 `Small`(30) 提到 `Large`(40)**（2026-10-08 用户令：
                                // 状态那一列的胶囊"比表格行距还大"）。
                                // 官方档：XSmall 26 / Small 30 / Medium 32 / Large 40
                                // （`gpui-component/src/sizing.rs:57`），**最高就是 40**——
                                // 原型 `tbody td { height: 50px }` 那 50 只能靠
                                // `Size::Size(px(50.))` 拿到，而那条自定义 px 档
                                // 会联动把单元格里文字与图标一起放大（`AGENTS.md` 为按钮记过这笔账），
                                // 所以宁可用 40 这一档，不越那条禁令
                                .with_size(Size::Large)
                                .bordered(false)
                                .stripe(true),
                        ),
                    )
                    .into_any_element()
                },
                head.right(chip).right(sort_btn),
            )
        };
        let table = card(t).flex_1().min_h_0().child(head.build(t)).child(body);

        v_flex()
            .flex_1()
            .min_h_0()
            .h_full()
            .bg(t.colors.background)
            // **四边缩进按 2026-10-08 用户定的那四条落**（同参照 = 窗口客户区边）：
            // 左 41 = 导航轨 40 + 轨边框 1 + 主区 **0**
            // 右 9  = 卡自身边框 1 + 壳层那一格 `pr`（**写在 `shell.rs`，不在这里**）
            // 上 94 / 下 28 里，主区自己那两格都是 **0**（下 28 全是状态条，不是留白）
            // ⇒ 这一层**一条 padding 都不写**。别把它补回 `p(..)`"看着才像有边距"——
            // 同灰之后左边距留着会让第一张卡与轨之间凭空多一道 8px 的缝，
            // 而那条缝在参考图里根本不存在
            // **卡片与卡片之间统一 6**（用户 2026-10-08 定）。代码写 4，帧上量 6：
            // 相邻两张卡各有一条 1px 边框（`4+1+1=6`），口径与推导记在 `tokens::GAP_TIGHT`
            .gap(px(GAP_TIGHT))
            .child(self.ov_head(t))
            .child(self.stat_cards(t))
            .child(self.ov_activity_chart(t))
            .child(table)
    }

    /// 总览页页头。**照原型 `.page-head` 的结构：两行**。
    ///
    /// 2026-10-08 曾为凑用户算的"上缩进 94"把它压成单行（94 那笔账只有按单行才加得平）。
    /// 他这一轮的原话是「你最起码总览页先完善到位」，而"到位"的基准是原型
    /// ⇒ **形态回到两行，缩进让位**。
    /// ⚠️ **同一天晚些时候两行与 94 同时成立了**：把两行的行盒从 `LH_ROW` 收到
    /// `LH_HEAD` 之后，第一张统计卡的顶边实测落在帧上 **y=94**（改前两行是 104）。
    /// 他那笔"上缩进 94"的参照是**窗口顶**（含 36 高的标题栏），不是客户区上沿——
    /// 按客户区量是 58，**别拿那个数去对账**。
    /// ⇒ **不要再为了"上缩进"砍这一行**：那笔账的真正出处是行盒，不是行数
    fn ov_head(&self, t: &Theme) -> Div {
        let mine = self.repos.iter().filter(|r| !r.is_third_party).count();
        let third = self.repos.len() - mine;

        v_flex()
            .w_full()
            .flex_shrink_0()
            // **页头右移一个圆角半径**（2026-10-08 用户令：「总览与副标题要向右移动到
            // 下面卡片的圆角尺寸让出来，假定圆角半径是 10，那就向右移动 10」——
            // 10 是他举的例子，真源是 `r_xl(t)`，当前浅色档是 20）。
            // 判据是**派生量**不是像素：圆角改成 14 那天，这一格自己跟上，
            // 不会留下"标题又伸进圆角切掉的那块空里"的账（同 `TITLE_BAR_PAD_L` 那条）
            .pl(px(r_xl(t)))
            // **对齐基线**：第一个导航键的图标中心要与这一行的文字中心同高
            // （`tokens::NAV_PAD_TOP` 那条，他对 6px 的偏差就报过一次）。
            // 行盒从 1.7 收到 `LH_HEAD` 之后标题中心跟着上移了 4（实测 49 对图标 53），
            // 而轨上那 3px 已经是图标能贴顶的极限 ⇒ 让**页头**补这一格，
            // 不是让图标往上钻。改 `LH_HEAD` / `title_bar_h` 任一处都要回来重量这两个中心
            .pt(px(GAP_TIGHT))
            .mb(px(GAP - GAP_TIGHT))
            // **两行的行盒一起收到 `LH_HEAD`**（推导与实测账见那个常量）。
            // 设在**这一列**上而不是逐行设：gpui 的 `.line_height()` 原文是
            // "of this element **and its children**"，副标题那一行里
            // `ov_num` / `ov_txt` / `ov_sep` 六段各自带字号，逐段去挂就是
            // 同一个属性六个出处——它们只要跟着这一列走就够了
            .line_height(DefiniteLength::Fraction(LH_HEAD))
            // 原型 `.page-head .sub { margin-top: 6px }`——我们最近的档是 `GAP_TIGHT`(4)，
            // **不为 2px 开一档**（v4 §3.2），所以这一格取 4 不取 6
            .gap(px(GAP_TIGHT))
            .child(
                div()
                    .text_size(px(fs_head()))
                    .font_weight(FontWeight::EXTRA_BOLD)
                    .text_color(t.colors.foreground)
                    .child("总览"),
            )
            // 副标题那一句里**数字是加粗的、且取正文字色**（原型 `.sub b`）：
            // 整句都灰 = 三个数与前后缀一样重，扫一眼拿不到"几个仓库"。
            // 拆成一串小段横排，就是原型那个 `display:flex; gap:8px` 的结构
            .child(
                h_flex()
                    .items_center()
                    .gap(px(GAP))
                    .child(ov_num(self.repos.len().to_string(), t))
                    .child(ov_txt("个仓库", t))
                    .child(ov_sep(t))
                    .child(ov_txt("我的", t))
                    .child(ov_num(mine.to_string(), t))
                    .child(ov_sep(t))
                    .child(ov_txt("第三方", t))
                    .child(ov_num(third.to_string(), t)),
            )
    }
}

// ================= 自研的两处整形 =================

/// 本地时区的 `YYYY-MM-DD`。
///
/// **不能用 `toISOString`**：那是 UTC 日期。本地在 UTC 以东时（比如 UTC+8 的
/// 晚上 8 点之后），`toISOString` 给的是"明天"，整根柱子会挪到隔壁去，
/// 而且"今天"这个概念在用户脑子里永远是本地的今天
/// 本地时区的 `YYYY-MM-DD`，往前数 `days_ago` 天。
///
/// ⚠️ **这里原来是一套手写的 epoch → civil 换算**（`civil_from_days`，
/// Howard Hinnant 的算法，算法本身没错），错的是**喂给它的偏移量**：
/// 偏移为 0 时 `epoch.div_euclid(86_400)` 给的是 **UTC 日期**，
/// 而 core 那边 `git log --date=format-local:%Y-%m-%d` 给的是**本地日期**。
/// 两边对不上，而窗口右端（"今天"）走的正是偏移 0 那条路。
///
/// **实测后果**（2026-10-05 04:34 +08:00，合成仓库当天提交 3 次）：
/// x 轴标签从 9/21 到 **10/04**（本该到 10/05），整张图一根柱子都没有。
/// 也就是**本地时间 0:00–8:00 之间，今天的提交在总览图上永远看不见**。
///
/// 换成 `chrono` 是因为**它早就在依赖树里**（`gpui-base` 自己就依赖它，
/// 加这条边不新增编译单元），而"本机现在的 UTC 偏移是多少"这件事
/// 自己手搓要么引 FFI、要么猜——**夏令时一换就悄悄错**。
pub(crate) fn local_day_key(now: DateTime<Local>, days_ago: i64) -> String {
    (now.date_naive() - Duration::days(days_ago))
        .format("%Y-%m-%d")
        .to_string()
}
/// 把后端给的「有提交的日子」补齐成连续 14 个点，按日期升序（旧 → 新）。
///
/// 图表的 x 轴从左到右是时间轴，**空的点会让后面的点往前挤**——
/// 少一天两根就靠拢了，看起来像"提交变频繁了"。所以宁可画一个 0，
/// 也不能让那一天消失
pub(crate) fn fill_days(src: &[ActivityDay], days: u32) -> Vec<ActivityBar> {
    fill_days_from(src, days, Local::now())
}

/// 主体。**`now` 显式传进来**——窗口右端是"本地的今天"，
/// 那由本机时区决定，从外面钉不住，要能注入才测得了
pub(crate) fn fill_days_from(
    src: &[ActivityDay],
    days: u32,
    now: DateTime<Local>,
) -> Vec<ActivityBar> {
    let mut out: Vec<ActivityBar> = Vec::with_capacity(days as usize);
    // 从 (今天 - days + 1) 到今天，共 days 天
    for i in 0..days as i64 {
        let key = local_day_key(now, days as i64 - 1 - i);
        out.push(ActivityBar {
            count: src
                .iter()
                .find(|d| d.date == key)
                .map(|d| d.count)
                .unwrap_or(0),
            date: key,
        });
    }
    out
}

/// `YYYY-MM-DD` → `M/D`。x 轴标签 14 个全写会挤成一团
///
/// **必须跳掉年**：早先写的是取前两段，得到的是 `2026/09`——14 个标签上
/// 全印着同一个 "2026/09"，一屏重复 14 次，一个日期都读不出来
/// （2026-10-03 实机首轮）。`local_day_key` 产出的是三段 `年-月-日`，
/// 段数不同就意味着"按位置取"这种写法本身不可靠，这里按下标取并容错。
fn short_day(date: &str) -> String {
    let mut it = date.split('-');
    let _year = it.next();
    let m = it.next().unwrap_or("");
    let d = it.next().unwrap_or("");
    format!("{}/{}", m.trim_start_matches('0'), d)
}

// ================= 「状态」列那句话 =================

/// 总览表格「状态」列写什么。**纯函数**，所以能单测。
///
/// ⚠️ 抽出来是被一次实机抓帧逼的：阶段 16 起启动只刷选中的那一个仓库，
/// 于是**启动那一刻其余每一行都是没扫过的**，而这一列原来会把
/// `Bucket::Unknown` 落进「已同步」——那不是"没消息"，是**谎报干净**，
/// 而且恰好是用户最不愿意听错的一句。
///
/// 判据原先藏在 `sync_ov_rows` 里、和取色缠着，编译器与单测都拦不住
/// （`cargo check` 零错、`bucket_of` 的单测全绿，因为它判得**没错**，
/// 错的是显示那一侧没接住 `Unknown`）。
pub(crate) fn ov_status_text(b: Bucket, changed: u32, third: bool) -> String {
    // 没扫过 / 同步读不出：措辞用 `bucket_word`——那句是**全站唯一一份**（⌘K 搜索同源）。
    // ⚠️ 两档都要在这里挡住，**不能让 `AbUnknown` 掉进下面那个 match**：
    // 它经 `badge_kind_of` 落进 `BadgeKind::None` ⇒ 会拼出"已同步"，
    // 而那正是这一整条链上最不能说的一句（git 太老、根本没算出领先落后）
    if b == Bucket::Unknown || b == Bucket::AbUnknown {
        return bucket_word(b).to_string();
    }
    // 未提交与分档可重叠：分档先给出"要做什么"，未提交作为补充说明
    match crate::sidebar::badge_kind_of(b, changed) {
        BadgeKind::None => "已同步".into(),
        BadgeKind::Behind => match (changed > 0, third) {
            (true, _) => format!("{} · 未提交 {changed}", if third { "可更新" } else { "落后" }),
            (false, true) => "可更新".into(),
            (false, false) => "落后".into(),
        },
        BadgeKind::Ahead => "待推送".into(),
        BadgeKind::Forked => "已分叉".into(),
        BadgeKind::Dirty => format!("未提交 {changed}"),
    }
}

// ================= 窗口右端的单测 =================

#[cfg(test)]
mod window_tests {
    use super::{fill_days, fill_days_from, local_day_key, ACTIVITY_DAYS};
    use chrono::{DateTime, Local, Utc};
    use qingbird_core::repos::ActivityDay;

    fn local_today() -> String {
        Local::now().date_naive().format("%Y-%m-%d").to_string()
    }

    /// **回归钉住：今天的提交必须落在窗口最右端那一格。**
    ///
    /// 原来的 `local_date_key(0)` 是 `epoch.div_euclid(86_400)`，给的是
    /// **UTC 日期**；core 那边的日期串来自 `git log --date=format-local`，
    /// 是**本地日期**。UTC+8 的凌晨两者差一天，于是今天的提交整根柱子
    /// 落在窗口外（图看着像"近两周没人提交"）。
    /// 实测 2026-10-05 04:34 +08:00，当天 3 次提交，x 轴止于 10/04，零柱子。
    #[test]
    fn a_commit_today_lands_on_the_last_column() {
        let today = ActivityDay { date: local_today(), count: 3 };
        let bars = fill_days(&[today], ACTIVITY_DAYS);
        assert_eq!(bars.len(), ACTIVITY_DAYS as usize);
        assert_eq!(bars.last().unwrap().date, local_today(), "右端是本地的今天");
        assert_eq!(
            bars.last().unwrap().count,
            3,
            "今天的提交必须被数进去，而不是静默掉出窗口"
        );
    }

    /// 窗口右端跟 `Local::now()` 对齐，**不是** UTC 的今天。
    /// 两者在非零偏移的机器上每天有 24 − 2×|offset| 小时不等，
    /// 旧实现每 16 小时就会在这条上红一次（UTC+8）
    #[test]
    fn the_window_ends_on_the_local_today() {
        let bars = fill_days(&[], ACTIVITY_DAYS);
        assert_eq!(bars.last().unwrap().date, local_today());
    }

    /// 注入一个固定时刻：**本地日期必须取本地的那个**，
    /// 哪怕同一瞬间 UTC 还在前一天。造不出这种时刻的机器（UTC）上
    /// 这条会平凡通过——但它至少把"取本地"这件事写成了断言，
    /// 而不是一个没人看的注释
    #[test]
    fn the_local_civil_date_is_used_even_when_utc_is_a_day_behind() {
        // 一个随便的时刻；本机时区非零时它与 UTC 的日期就有机会不同
        let instant: DateTime<Utc> = DateTime::from_timestamp(1_760_000_000, 0).unwrap();
        let local = instant.with_timezone(&Local);
        assert_eq!(
            local_day_key(local, 0),
            local.date_naive().format("%Y-%m-%d").to_string()
        );
    }

    /// 14 根柱子必须是**连续的 14 个日历日**，中间一天都不能少。
    /// x 轴是时间轴，少一天后面的就往前挤（看着像"提交变频繁了"）
    #[test]
    fn the_window_is_consecutive_calendar_days() {
        let now = Local::now();
        let bars = fill_days_from(&[], 14, now);
        for (i, b) in bars.iter().enumerate() {
            let want = local_day_key(now, 13 - i as i64);
            assert_eq!(b.date, want, "第 {i} 根柱子");
        }
    }
}

// ================= 「状态」列的单测 =================

#[cfg(test)]
mod status_tests {
    use super::ov_status_text;
    use crate::sidebar::badge_kind_of;
    use crate::state::Bucket;

    /// **「没扫过」不许写成「已同步」。**
    ///
    /// 阶段 16 起启动只刷选中的那一个仓库，于是启动那一刻其余每一行都是
    /// 没扫过的。这条错在实机抓帧里才看见：`cargo check` 零错、
    /// `bucket_of` 的单测全绿（它判得**没错**，错的是显示那侧没接住
    /// `Unknown`）。所以判据必须被单独钉住。
    #[test]
    fn an_unscanned_repo_never_reads_as_synced() {
        assert_eq!(ov_status_text(Bucket::Unknown, 0, false), "待扫描");
        assert_eq!(ov_status_text(Bucket::Unknown, 0, true), "待扫描");
        // `changed` 也不该把它变成别的结论：没扫过就是没扫过，
        // 那个数是从哪来的都不知道
        assert_eq!(ov_status_text(Bucket::Unknown, 3, false), "待扫描");
    }

    /// 32-12 新档：**问过 git 了、但它那个版本读不出领先/落后**。
    /// 这一档最容易被兜底臂吃掉（`badge_kind_of` 有 `_` 臂、编译器不提醒），
    /// 而吃掉的后果恰恰是这一整条链上最不能说的那句"已同步"
    #[test]
    fn a_repo_whose_ahead_behind_cannot_be_read_is_not_called_synced() {
        assert_eq!(ov_status_text(Bucket::AbUnknown, 0, false), "同步未知");
        assert_eq!(
            ov_status_text(Bucket::AbUnknown, 0, true),
            "同步未知",
            "第三方也不许说成「可更新」——那是同一个谎换个说法"
        );
        assert_ne!(
            ov_status_text(Bucket::AbUnknown, 0, false),
            ov_status_text(Bucket::Synced, 0, false),
            "「同步未知」与「已同步」必须是两句话"
        );
        assert_ne!(
            ov_status_text(Bucket::AbUnknown, 0, false),
            ov_status_text(Bucket::Unknown, 0, false),
            "「问过了、答不上来」与「还没问」也必须是两句话"
        );
    }

    /// 扫过且干净的才轮到「已同步」。**别把上一条改回去**
    #[test]
    fn a_scanned_clean_repo_says_synced() {
        assert_eq!(ov_status_text(Bucket::Synced, 0, false), "已同步");
    }

    /// 归属只改「落后」那句的措辞，不改它「落后」这件事
    #[test]
    fn behind_is_worded_by_ownership() {
        assert_eq!(ov_status_text(Bucket::Behind, 0, false), "落后");
        assert_eq!(ov_status_text(Bucket::Behind, 0, true), "可更新");
        assert_eq!(ov_status_text(Bucket::Behind, 2, false), "落后 · 未提交 2");
        assert_eq!(ov_status_text(Bucket::Behind, 2, true), "可更新 · 未提交 2");
    }

    /// 档位判定与显示那句话必须**同一份**：状态列写着「未提交 3」，
    /// 侧栏徽标却是空胶囊的话，同一仓库两屏两种说法
    #[test]
    fn the_wording_and_the_badge_agree_on_every_bucket() {
        for b in [
            Bucket::Synced,
            Bucket::Dirty,
            Bucket::Ahead,
            Bucket::Behind,
            Bucket::Forked,
            Bucket::Unknown,
        ] {
            let kind = badge_kind_of(b, 0);
            let text = ov_status_text(b, 0, false);
            assert!(!text.is_empty(), "{:?} 空话", b);
            match kind {
                crate::sidebar::BadgeKind::None => {
                    assert!(
                        text == "已同步" || text == "待扫描",
                        "{:?} 无动作却写了 {text}",
                        b
                    )
                }
                _ => assert_ne!(text, "已同步", "{:?} 有事要办却说已同步", b),
            }
        }
    }
}

// ================= 页头副标题那三样 =================
// 原型 `.page-head .sub` 是 `display:flex; gap:8px` 的一串小段，
// 里面 `<b>` 包的是数字。整句一个颜色扫不出数，所以**不拼成一个 String** 而是排元素

/// 副标题里的一枚**数字**（原型 `.sub b`：加粗 + 正文字色）。
/// 数字比它前后的说明文字重一档，"几个仓库"才是一眼里拿得到的
fn ov_num(s: String, t: &Theme) -> Div {
    div()
        .text_size(px(fs_sec()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.colors.foreground)
        .child(s)
}

/// 副标题里的一段**说明文字**（比数字淡一档，不跟它抢）
fn ov_txt(s: &str, t: &Theme) -> Div {
    div()
        .text_size(px(fs_sec()))
        .text_color(tier3(t))
        .child(s.to_string())
}

/// 中间点分隔符。原型 `.sub .sep` 用的是 `--line`（发丝线那一档），
/// 但**照搬到我们这套配色上就完全看不见**（2026-10-08 抓帧：那两个点一个像素都没认出来）——
/// 我们的 `track` 与画布只差 1/255，本来就是为了"卡间发丝线"调的，不是给文字用的。
/// ⇒ 取 `tier4`（最淡的一档**文字**色）：它仍不参与信息竞争，但眼睛能看见
fn ov_sep(t: &Theme) -> Div {
    div()
        .text_size(px(fs_sec()))
        .text_color(tier4(t))
        .child("·")
}

// ================= 列宽的单测 =================

#[cfg(test)]
mod col_width_tests {
    use super::{ov_column_widths, OV_COL_BASE, OV_COL_MIN, OV_COL_SCALE_MAX, OV_TABLE_CHROME};
    use crate::tokens::WIN_MIN_W;

    /// 列宽合计**永远不小于可用宽度**：能铺满就正好铺满，铺不满就略微超出
    /// （表格自己出横向滚动条）。
    ///
    /// 反过来才是灾难：合计比可用宽度少，卡片右边就空一条；而且空出来的
    /// 那条**不会**让下一帧变好——它只会让"看上去差一点点"这件事一直存在。
    /// 「差一点多」出横向滚动条是可以接受的代价：滚动条让用户知道还有内容。
    #[test]
    fn columns_never_come_up_short() {
        let base_total: f32 = OV_COL_BASE.iter().sum();
        for avail in [
            100f32, 400., 552., 600., 736., 900., 1009., 1400., 2000., 3369.,
        ] {
            let w = ov_column_widths(avail);
            let sum: f32 = w.iter().sum();
            assert!(
                sum >= avail - 0.5,
                "avail={avail} 合计={sum}，比可用宽度还窄，右边会空一条"
            );
            // 宽屏余量全部摊给两列，合计回到 avail；窄到下限顶住时最多也就
            // 停在基准合计（736）以内——不会没完没了地长
            assert!(
                sum <= avail.max(base_total) + 0.5,
                "avail={avail} 合计={sum}，超过了 {}",
                avail.max(base_total)
            );
        }
    }

    /// 没有列被下限顶住时（七列都在纯等比那一档），合计必须**正好**等于可用宽度
    #[test]
    fn unobstructed_widths_are_exact() {
        for avail in [736f32, 900., 1009., 1150.] {
            let w = ov_column_widths(avail);
            let sum: f32 = w.iter().sum();
            assert!(
                (sum - avail).abs() < 0.5,
                "avail={avail} 合计={sum}，差了 {:.2}px",
                sum - avail
            );
        }
    }

    /// 窗口在常见区间里要保持基准的相对比例（= 纯等比，不被上限截断）
    #[test]
    fn common_widths_stay_proportional() {
        let avail = 1009.0;
        let scale = avail / 736.0;
        assert!(scale <= OV_COL_SCALE_MAX, "前提：这一档不该撞上限");
        let w = ov_column_widths(avail);
        for (i, &base) in OV_COL_BASE.iter().enumerate() {
            let want = base * scale;
            assert!(
                (w[i] - want).abs() < 0.5,
                "第 {i} 列：想要 {want}，实得 {}",
                w[i]
            );
        }
    }

    /// 宽屏上「归属 / 状态 / 落后 / 待推送 / 未提交」不能被拉成大片空白，
    /// 余量必须落在「仓库名 / 分支」这两列上
    #[test]
    fn wide_windows_push_the_slack_into_the_text_columns() {
        let w = ov_column_widths(3369.0);
        for i in [1usize, 3, 4, 5, 6] {
            assert!(
                w[i] <= OV_COL_BASE[i] * OV_COL_SCALE_MAX + 0.5,
                "第 {i} 列 {} 超出了 {OV_COL_SCALE_MAX} 倍上限",
                w[i]
            );
        }
        let widest = w
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i);
        assert_eq!(widest, Some(0), "最宽的应该是「仓库名」");
    }

    /// 任何可用宽度下都不许低于下限（表头不能断字）
    #[test]
    fn no_column_ever_drops_below_its_floor() {
        for avail in [100f32, 200., 300., 400., 736., 1500.] {
            let w = ov_column_widths(avail);
            for (i, &min) in OV_COL_MIN.iter().enumerate() {
                assert!(
                    w[i] >= min - 0.01,
                    "avail={avail} 第 {i} 列 {} 低于下限 {min}",
                    w[i]
                );
            }
        }
    }

    /// 可用宽度还量不出来时退回基准宽，不要给一堆 0
    #[test]
    fn unknown_width_falls_back_to_base() {
        assert_eq!(ov_column_widths(0.0), OV_COL_BASE);
        assert_eq!(ov_column_widths(-1.0), OV_COL_BASE);
    }

    /// 窗口在 `[WIN_MIN_W, 4000]` 全区间里，**合计都不得超过 `avail`**。
    ///
    /// 这条是 D56 那次自激的护栏：合计一旦超过可用宽度就出横向滚动条，
    /// 滚动条又把内容区压窄，下一帧算出来更小——界面持续往右拉伸。
    /// 开环 + 不溢出，两者缺一不可
    #[test]
    fn total_never_exceeds_the_given_width() {
        let mut w = WIN_MIN_W;
        while w <= 4000.0 {
            let avail = w - OV_TABLE_CHROME;
            let sum: f32 = ov_column_widths(avail).iter().sum();
            assert!(
                sum <= avail + 0.5,
                "窗口 {w}px：合计 {sum} > 可用 {avail}，会出横向滚动条"
            );
            w += 37.0;
        }
    }
}
