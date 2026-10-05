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
use gpui_kit::component::chart::AreaChart;
use gpui_kit::component::empty::{Empty, EmptyContent, EmptyHeader, EmptyTitle};
use gpui_kit::component::table::{Column, DataTable, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme, Sizable, Size, Theme};
use gpui_kit::{
    div, px, AnyElement, App, Context, Div, Entity, FontWeight, IntoElement, ParentElement, Styled,
    Window,
};
use chrono::{DateTime, Duration, Local};
use qingbird_core::repos::ActivityDay;

use crate::sidebar::BadgeKind;
use crate::state::*;
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

/// 表格外面那一圈固定开销（从窗口宽里扣掉的那一条）：导航轨 + 分隔线 +
/// 页面左右内边距 + 卡片左右描边 + 纵向滚动条。
///
/// **宁可多扣也不要少扣**（D56）：少扣 = 合计大于可用宽度 = 冒出横向滚动条，
/// 而横向滚动条会把内容区再压窄——那正是自激的起点。多扣只是右边空十几像素，
/// 是一次定死的静态状态，不会自己动
pub(crate) const OV_TABLE_CHROME: f32 = NAV_W + 1. + 2. * GAP_LG + 2. + 16.;

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
            .text_size(px(FS_SEC))
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
                    .text_size(px(FS_BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.colors.foreground)
                    .child(row.name.clone()),
            )
            .into_any_element(),
        1 => div()
            .text_size(px(FS_SEC))
            .text_color(tier3(t))
            .child(if row.third { "第三方" } else { "我的" })
            .into_any_element(),
        2 => div()
            .font_family(t.mono_font_family.clone())
            .text_size(px(FS_SEC))
            .text_color(tier3(t))
            .child(if row.branch.is_empty() {
                "—".to_string()
            } else {
                row.branch.clone()
            })
            .into_any_element(),
        3 => {
            let (_, fg, bg) = crate::sidebar::badge_tone(row.bucket, row.changed, row.third, t);
            pill(&row.status, fg, bg).into_any_element()
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
            .gap(px(GAP))
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

impl Shell {
    /// 近 14 天全仓库提交数。空数据给一句说明，不画一张空图——
    /// 空坐标系看起来像"程序坏了"
    /// 总览页的波形图。`panels.rs` 里还有一个同名的（单仓库的迷你柱状图），
    /// 两者数据源不同：那个读 `ProfileState`，这个读全仓库合并的结果
    fn ov_activity_chart(&self, t: &Theme) -> Div {
        let bars = fill_days(&self.ov_activity, ACTIVITY_DAYS);

        let card = v_flex()
            .w_full()
            .flex_shrink_0()
            .h(px(160.0))
            .p(px(GAP_LG))
            .gap(px(GAP_TIGHT))
            .rounded(px(R_LG))
            .bg(t.colors.popover)
            .border_1()
            .border_color(t.colors.border)
            .child(
                h_flex()
                    .items_baseline()
                    .gap(px(GAP))
                    .child(
                        div()
                            .text_size(px(FS_BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.colors.foreground)
                            .child("提交活跃度"),
                    )
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .text_color(tier4(t))
                            .child(format!("近 {ACTIVITY_DAYS} 天 · 全部仓库合计")),
                    ),
            );

        if bars.is_empty() {
            return card.child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(FS_META))
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
        card.child(
            div().flex_1().min_h_0().child(
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
        let rows: Vec<OvRow> = self
            .repos
            .iter()
            .map(|e| {
                let v = self.views.get(&e.path).unwrap_or(&EMPTY_VIEW);
                let b = bucket_of(&v.status);
                let status = ov_status_text(b, v.changed, e.is_third_party);
                OvRow {
                    path: e.path.clone(),
                    name: if e.alias.trim().is_empty() {
                        e.name.clone()
                    } else {
                        e.alias.trim().to_string()
                    },
                    third: e.is_third_party,
                    branch: v.status.branch.clone(),
                    bucket: b,
                    status,
                    behind: v.status.behind,
                    ahead: v.status.ahead,
                    changed: v.changed,
                }
            })
            .collect();
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
    pub(crate) fn overview_page(&self, t: &Theme, _cx: &mut Context<Self>) -> impl IntoElement {
        let mine = self.repos.iter().filter(|r| !r.is_third_party).count();
        let third = self.repos.len() - mine;

        let head = v_flex()
            .w_full()
            .flex_shrink_0()
            .gap(px(GAP_TIGHT))
            .child(
                div()
                    .text_size(px(FS_HEAD))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.colors.foreground)
                    .child("总览"),
            )
            .child(
                div()
                    .text_size(px(FS_SEC))
                    .text_color(tier3(t))
                    .child(format!(
                        "{} 个仓库 · 我的 {} · 第三方 {}",
                        self.repos.len(),
                        mine,
                        third
                    )),
            );

        let table = if self.repos.is_empty() {
            v_flex()
                .w_full()
                .flex_1()
                .min_h_0()
                .rounded(px(R_LG))
                .bg(t.colors.popover)
                .border_1()
                .border_color(t.colors.border)
                .child(
                    Empty::new()
                        .border_0()
                        .header(EmptyHeader::new().title(EmptyTitle::new().child("还没有仓库")))
                        .content(
                            EmptyContent::new().child(
                                div()
                                    .text_size(px(FS_META))
                                    .text_color(tier4(t))
                                    .child("回到「我的仓库」加一个，总览才有东西可看"),
                            ),
                        ),
                )
        } else {
            // 官方 `DataTable` 同样没有 `Styled`，撑高交给外层
            div()
                .w_full()
                .flex_1()
                .min_h_0()
                .rounded(px(R_LG))
                .overflow_hidden()
                .border_1()
                .border_color(t.colors.border)
                .child(
                    // 官方 `DataTable` 没有 `accessibility_label`（那是 `table::Table`
                    // 才有的），裸 `div` 也没有——所以这块不硬凑一个可访问名，
                    // 上面的页头「N 个仓库 · 我的 X · 第三方 Y」已经把表格在讲什么说清了
                    div().size_full().child(
                        DataTable::new(&self.ov_table)
                            .with_size(Size::Small)
                            .bordered(false)
                            .stripe(true),
                    ),
                )
        };

        v_flex()
            .flex_1()
            .min_h_0()
            .h_full()
            .bg(t.colors.background)
            .p(px(GAP_LG))
            .gap(px(GAP_LG))
            .child(head)
            .child(self.stat_cards(t))
            .child(self.ov_activity_chart(t))
            .child(table)
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
    // 没扫过：措辞用 `bucket_word`——那句是**全站唯一一份**（⌘K 搜索同源）
    if b == Bucket::Unknown {
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
