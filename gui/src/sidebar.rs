//! 仓库侧栏：两组树（我的 / 第三方）+ 过滤 + 底部两个动作。
//!
//! 宽度与折叠不在这里管——外面是官方 `resizable_panel`（见 shell.rs 与路线表 D12）。
//! 这里只管"有哪些仓库、每一行长什么样、右键能干什么"。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::separator::Separator;
use gpui_kit::component::sidebar::{SidebarItem as _, SidebarMenuItem};
use gpui_kit::component::spinner::Spinner;
// `ActiveTheme` 是 `cx.theme()` 的来源：suffix 闭包拿到的是 `&mut App`
// 而不是 `&mut Context<Self>`，这个 trait 让两处都能取到主题
use gpui_kit::component::{ActiveTheme, Selectable as _, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, percentage, px, AnyElement, App, ClipboardItem, Context, Div, Entity, FontWeight, Hsla,
    InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled, Window,
};
use qingbird_core::net;
use qingbird_core::store::RepoEntry;
use qingbird_core::types::RepoStatus;

use crate::os;
use crate::state::*;
use crate::tokens::*;
use crate::widgets::*;

// ================= 侧栏骨架 =================

/// 侧栏那一列的外壳：**就是 `widgets::card` 那一张卡**，这里只覆盖内距与外边距。
///
/// 2026-10-08 之前它不是卡：`.bg(t.colors.background).border_r_1()` —— 一根与画布
/// 同灰的柱子，右边一条分隔线。那是阶段 21 那次"左栏改与画布同灰"留下的形态，
/// 而它当时的病是**旧画布 #FAFAF9 与白卡只差 5/255**，侧栏读成"一块分不清的白板"。
/// 画布换成冷灰蓝之后那条动机已经不存在（卡与底差 23/255，卡自己浮得起来），
/// 而原型 `.sidebar` 与 `.card` 本来就是同一套底（card 面 + `line-soft` 边 +
/// `r-card` 圆角 + `sh-2`）⇒ 归位成卡。
///
/// ⚠️ **外壳不重写一份**：底色 / 圆角 / 描边 / 阴影全部来自 `card(t)`，
/// 这里只覆盖 `p` / `gap` 并加 `mt` / `mb` / `mr`。内距与主区不同是**两种关系**
/// （这里放的是一列行，行自己带水平内距），不是同一种关系写了两个值。
///
/// `mt(GAP)` 与主区那一列的 `pt(GAP)`（`detail.rs`）**同源**：两张卡的顶边必须在
/// 同一条线上；`mr(GAP_TIGHT)` 就是"卡与卡之间"那一档（代码 4，帧上量 6，
/// 因为相邻两张卡各有一条 1px 边）。
fn side_shell(t: &Theme) -> Div {
    crate::widgets::card(t)
        .flex_none()
        // ⚠️ **不是 `h_full()`**：`h_full` 是"100% 父高"，加上 `mt`/`mb` 之后
        // 总高超出父容器，**下面那条 `mb(GAP)` 直接被吃掉**（实测卡底 791 正压在
        // 状态条 792 上，那 8px 缝没了）。`flex_1 + min_h_0` 才是"占满剩余、
        // 且让外边距从剩余里扣"的写法
        .flex_1()
        .min_h_0()
        .min_w_0()
        .gap(px(GAP_TIGHT))
        .p(px(GAP))
        .pt(px(GAP_LG))
        .pb(px(GAP))
        .mt(px(GAP))
        // ⚠️ **这里没有 `mb`，是有意的**：主区那一列的底部外边距是 **0**
        // （用户 2026-10-08 定的「下 28 = 主区 0 + 状态条 28」）。上一版留了 8，
        // 结果**侧栏卡底 782、日志卡底 790**——两列下边差 8px，他一眼看出来
        // "日志的卡片跟侧栏下边不平行"。两条诉求要同时成立，
        // 只能让侧栏也跟着贴到状态条上沿
        .mr(px(GAP_TIGHT))
}

impl Shell {
    /// 侧栏三段：头（标题 + 筛选）/ 身（一组树，可滚动）/ 脚（两个动作）。
    ///
    /// **只画当前 `side_focus` 那一组**（用户要求：分组已经由导航轨两颗键表达，
    /// 侧栏里再列一遍「我的 / 第三方」是同一件事说两遍）。导航轨答"我在看哪一组"，
    /// 侧栏答"这一组里有哪些仓库"，两层各管一件事。
    ///
    /// 行是**平铺**的，不用 `SidebarMenu` 包一层：那层只是个 `v_flex`，
    /// 但它会把行藏进子容器里，滚动容器的直接子元素就变成了那个菜单，
    /// `ScrollHandle::bounds_for_item` 换算不出行号，定位滚动就废了。
    pub(crate) fn sidebar(&self, t: &Theme, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let filter = self.filter_text(cx);
        let mut tree = v_flex()
            .flex_1()
            .min_h_0()
            .id("sidebar-tree")
            .track_scroll(&self.tree_scroll)
            .overflow_y_scroll()
            // ⚠️ **`.mx(px(-GAP))` 不是排版，是为了让裁剪盒变宽**：
            // 滚动容器的裁剪盒 = **它自己的边框盒，且两个轴都裁**
            // （`style.rs:639` `overflow_mask`——`overflow_y_scroll` 只把 y 设成 Scroll，
            // x 仍是默认的 Visible，落到 `(true,false)` 那一臂，取的是
            // `min.x..max.x` = 边框盒的整个横向）。
            // 竖条要住在卡的 padding 里（原型实测"距卡左外缘 +1"），
            // 而它挂在行里、行在树里 ⇒ **树不铺到那儿，竖条就被裁掉**：
            // 帧上 49..58 逐列打色全是卡的白面，一像素都不出，而同尺寸同纵向写法的
            // 一根条放在 59（边框盒内）就出。树往外扩 `GAP`(=壳的 `px`) 之后
            // 边框盒 = 49..402，气口进了裁剪盒，竖条才落得下去。
            //
            // ⚠️ `px(SIDE_GUTTER)`（10）**必须等于原来那两层 padding 之和**
            //（壳 8 + 树 2）：树扩出去了，内容要收回**原位**，否则分组标题、
            // 空态、每一行会一起往左跑 8px。负 `mx` 与这个 `px` 是一对，
            // **改一个必须同时改另一个**
            .mx(px(-GAP))
            .px(px(SIDE_GUTTER))
            .py(px(GAP_TIGHT))
            .gap(px(GAP_DENSE));

        // 索引与实际渲染顺序严格一一对应：定位滚动靠它把仓库路径换算成行号。
        // **行号从 1 起**——第 0 行是分组标题自己（`apply_reveal` 那边同一条约定）
        let rows: Vec<&RepoEntry> = self
            .repos
            .iter()
            .filter(|r| r.is_third_party == self.side_focus.is_third())
            .filter(|r| matches_filter(r, &self.status_of(&r.path), &filter))
            .collect();
        tree = tree.child(self.group_header(rows.len(), t, window, cx));
        let mut ix = 1usize;
        if self.group_open[self.side_focus.ix()] {
            for r in rows {
                tree = tree.child(self.repo_row(r, ix, t, cx));
                ix += 1;
            }
        }

        // **当前这一组**一个仓库都没有时不画树：树的容器是 flex_1，空着它会把空态
        // 挤到下面去，让人以为"下面还有东西没加载出来"。
        //
        // ⚠️ 判据是**当前组为空**，不是 `self.repos.is_empty()`：侧栏只画
        // `side_focus` 那一组，所以"第三方有 5 个仓库、我的仓库一个都没有"是
        // 一个真实会遇到的空态。用总数判会画出一棵空树加一个折叠箭头，
        // 界面上什么都不说——用户以为侧栏坏了。
        if self
            .repos
            .iter()
            .all(|r| r.is_third_party != self.side_focus.is_third())
        {
            // 另一组有没有东西决定空态给不给"换一组看看"
            let other_has_repos = self
                .repos
                .iter()
                .any(|r| r.is_third_party == self.side_focus.is_third());
            return side_shell(t)
                // ⚠️ **空态也画分组标题**（阶段 21-5 的连带）。
                // 侧栏头那一行删掉之后，「这一组叫什么」就没有别的落脚点了——
                // 空态只剩一句「我的仓库里还没有仓库」，**筛选键也跟着消失**
                // （它在标题的 `suffix` 上）。而"这一组是空的、但我可以筛选/添加"
                // 是一个真实可达的状态：仓库被移走之后就是它。
                .child(self.group_header(0, t, window, cx))
                .child(self.sidebar_header(t, cx))
                .child(no_repos(self.side_focus, other_has_repos, t))
                .child(Separator::horizontal().color(edge(t)))
                .child(self.sidebar_footer(t, cx));
        }

        side_shell(t)
            .child(self.sidebar_header(t, cx))
            .child(tree)
            .child(Separator::horizontal().color(edge(t)))
            .child(self.sidebar_footer(t, cx))
    }

    /// 侧栏头：**筛选框**（默认收起，D8）。**不再有标题与计数**（阶段 21-5）。
    ///
    /// ## 为什么上面那行整个删掉
    ///
    /// 它原来写着「仓库」+ 数字，而**正下方一行**的分组标题又写着
    /// 「我的仓库 · 3」——同一个数量在这一组里说了两遍，且两遍都在上方。
    /// 用户要求取消「仓库 + 统计」这一行、把筛选键移到分组标题右边。
    ///
    /// ⚠️ **组标题那个数字留着**：它紧挨着列表、说的是"这一组下面有几行"，
    /// 删掉它以后筛选框打开时**没有任何地方写着筛选结果有几句**
    /// ——用户会以为是筛选失灵了。而上面那行是整个侧栏的总数，
    /// 与它下面的具体分组无关，是重复的那一份。
    ///
    /// 筛选框仍**另起一行**：塞进标题那一行会被固定行高横向挤扁，
    /// 而官方 `Input::small()` 本身 24px 高，挤不出能用的输入区。
    fn sidebar_header(&self, t: &Theme, _cx: &mut Context<Self>) -> Div {
        v_flex()
            .w_full()
            .flex_shrink_0()
            .when(self.filter_open, |head| {
                // cleanable：官方自带的小叉，别自己再画一个清空按钮
                head.child(
                    div().w_full().px(px(GAP)).pt(px(GAP_TIGHT)).child(
                        Input::new(&self.side_filter)
                            .prefix(icon_sm(AssetIcon::Search).text_color(tier4(t)))
                            .cleanable(true)
                            .small()
                            .w_full(),
                    ),
                )
            })
    }

    /// 侧栏脚：添加仓库 / 克隆仓库。两个都保留，不合并成一个带开关的对话框
    fn sidebar_footer(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .px(px(GAP_LG))
            .py(px(GAP_LG))
            .gap(px(GAP))
            .child(
                Button::new("side-add")
                    .flex_1()
                    .icon(icon_sm(AssetIcon::Plus).text_color(tier3(t)))
                    .label("添加仓库")
                    .with_size(Size::Small)
                    .tooltip("把本地已有目录加进来")
                    .on_click(cx.listener(|this, _, window, cx| this.open_add_dlg(window, cx))),
            )
            .child(
                Button::new("side-clone")
                    .flex_1()
                    .icon(icon_sm(AssetIcon::Download).text_color(tier3(t)))
                    .label("克隆仓库")
                    .with_size(Size::Small)
                    .tooltip("从远端地址克隆一个新仓库到本地")
                    .on_click(cx.listener(|this, _, window, cx| this.open_clone_dlg(window, cx))),
            )
    }
}

// ================= 分组标题 =================

impl Shell {
    /// 分组标题。**不带子项** —— 子项的展开态一旦交给 `SidebarMenuItem` 的内部
    /// `use_keyed_state`，导航按钮与总览点行就没法从外面把某一组打开了。
    /// 所以标题只切换 `group_open`，行由我们自己按开合态铺开。
    ///
    /// 标题上的名字就是 `self.side_focus`（**只画当前那一组**），没有 `group`
    /// 参数可传——传进来就意味着"某一组"的语义还在这一层，
    /// 而那一层已经由导航轨管了。
    fn group_header(
        &self,
        n: usize,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let group = self.side_focus;
        let open = self.group_open[group.ix()];
        let (label, icon) = group.label_and_icon();
        // `suffix` 要 'static 闭包，捕不了 `&Theme`。颜色在外面取好（Hsla 是 Copy），
        // 只把值抓进去——这是这类槽位的通用做法
        let caret_color = tier4(t);
        let row = SidebarMenuItem::new(format!("{label} · {n}"))
            .icon(icon_sm(icon).text_color(tier3(t)))
            // 分组标题不承担选中语义：只给展开/收起，不给 .active()
            // （`SidebarMenuItem` 没有整行样式入口，只有 `label_style` 能改文字；
            //  行高那些是官方 28px 档，不在这里另开一档）
            .label_style(
                gpui_kit::StyleRefinement::default()
                    .text_size(px(fs_meta()))
                    .font_weight(FontWeight::MEDIUM),
            )
            .suffix({
                // 阶段 21-5：筛选键挂到**分组标题这一行**的右侧（原来在侧栏头，
                // 而侧栏头那一行已删）。`suffix` 只有一个槽位，所以两颗
                // （筛选键 + 折叠箭头）塞进同一个 `h_flex`，彼此 `GAP_TIGHT`。
                //
                // ⚠️⚠️ **渲染期绝不能 `ent.read(cx)`**（2026-10-05 实机闪退）。
                // 这个 suffix 闭包是在 `SidebarMenuItem::render` 里被调的，
                // 那时 `Shell` **正被 update 持着锁**；在里面再 `read` 同一个
                // Entity 就是重入，基座直接 panic：
                //   `cannot read qingbird_git::state::Shell while it is already
                //    being updated`（`entity_map.rs:156`）
                // 表现为**点导航轨第二颗按钮直接闪退**，且 stderr 只有一个
                // panic 消息、没有栈——很容易误判成"面板下标算错"。
                //
                // 所以这里**在闭包外**就把值取好捕获进去（那时 `&self` 在手边，
                // 读的是普通字段、不碰 Entity）。闭包是 `'static` 也没关系：
                // `bool` 是 Copy。
                let filter_open = self.filter_open;
                let group_open = open;
                let ent = cx.entity();
                move |_window: &mut Window, cx: &mut App| {
                    let t = cx.theme().clone();
                    // 颜色与选中面都要 `&Theme`，而 `cx.theme()` 在这个
                    // `&mut App` 上是拿得到的（`ActiveTheme`）——不需要读 Shell
                    h_flex()
                        .gap(px(GAP_TIGHT))
                        .items_center()
                        .child(
                            Button::new("side-filter-toggle")
                                .icon(icon_sm(AssetIcon::ListFilter).text_color(tier4(&t)))
                                .custom(ghost_variant(cx))
                                .selected(filter_open)
                                // 与导航轨三颗切屏键**同一个**选中面（D60）：同类
                                // —— 都是 ghost 图标按钮的选中态。官方 ghost 选中的底
                                // 走 `button.active.background`，theme.json 没配、落基座
                                // 兜底的深灰。理由见 tokens.rs。
                                .when(filter_open, |b| b.bg(wash(&t)))
                                .with_size(Size::Small)
                                .tooltip("筛选仓库")
                                .accessibility_label("筛选仓库")
                                // 点击走 `update`：那时**不在渲染期**，
                                // 拿得到锁（`on_click` 是独立的事件回调）
                                .on_click({
                                    let ent = ent.clone();
                                    move |_, _, cx| {
                                        ent.update(cx, |this, cx| {
                                            this.filter_open = !this.filter_open;
                                            cx.notify();
                                        });
                                    }
                                }),
                        )
                        .child(
                            icon_sm(AssetIcon::ChevronRight)
                                .text_color(caret_color)
                                .rotate(percentage(
                                    if group_open { 90.0 } else { 0.0 } / 360.0
                                )),
                        )
                }
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.group_open[group.ix()] = !this.group_open[group.ix()];
                cx.notify();
            }));
        row.render(format!("side-group-{}", group.ix()), window, cx)
            .into_any_element()
    }
}

// ================= 仓库行 =================

impl Shell {
    /// 一行仓库：状态图标 / 名称 / 右侧徽标。
    /// 三列分别落在 `SidebarMenuItem` 的 `icon` / `label` / `suffix` 三个官方槽位上
    fn repo_row(
        &self,
        r: &RepoEntry,
        ix: usize,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let path = r.path.clone();
        let status = self.status_of(&path);
        let changed = self.view(&path).map(|v| v.changed).unwrap_or(0);
        let b = bucket_of(&status);
        let selected = self.selected.as_deref() == Some(path.as_str());
        let me = cx.entity();
        let third = r.is_third_party;
        let lg = self.logs.get(&path).unwrap_or(&LOG_EMPTY);
        // 徽标只占一个位置，优先级从高到低：正在跑 > 上次失败 > 走代理 > 分档。
        // 分档信息并没有因此丢失——左侧那个字形已经带着同一份 `bucket_of`
        // 的结果了（D13），而 240px 的行塞不下第二枚胶囊
        let badge = if lg.running {
            Badge::Spinner { color: tier3(t) }
        } else if matches!(lg.verdict, Some((false, _))) {
            Badge::Pill {
                text: "失败".into(),
                fg: t.colors.danger,
                bg: red_bg(t),
            }
        } else if lg.used_proxy.is_some() {
            Badge::Pill {
                text: "走代理".into(),
                fg: orange_text(t),
                bg: orange_bg(t),
            }
        } else {
            bucket_badge(&status, changed, third, t)
        };

        // ⚠️ **这一行是自绘的，不再是官方 `SidebarMenuItem`**（2026-10-08 用户令
        // 「42 有点太高了，改到 36」）。官方件把行高写死在 `menu.rs:323` 的 `h_7()`
        // = 28px，**没有 size 档，外面挂 `.h()` 也压不到它内部的元素** ⇒ 要 36 只能自己画。
        // 官方件原本给的四样在这里都补齐了，一样都没少：
        //   命中区 `on_click` / 悬停面 `.hover` / 右键菜单 `ContextMenuExt`
        //   / 元素 id（滚动定位靠它，`apply_reveal` 按 `side-repo-{ix}` 找行）
        //
        // ## 行是**两层**的，不是一层
        //
        // 外层 = 命中区，盒子铺到**卡的 padding 边**（帧实测 49..402）；
        // 内层 `.child(face)` = 淡底/悬停面，距卡内缘 10（帧 59..392）。
        // 分成两层只有一个理由：原型那根竖条（`.repo-item.on::before`）**住在卡的
        // padding 里**，实测位置是"距卡左外缘 +1"，而淡底在 +11——**竖条与淡底之间
        // 隔着 7px 白**。淡底要是行的底色，竖条就只能画在淡底上（+11），
        // 那就不是原型那个东西了。
        //
        // ⛔ **外层为什么用负 `mx` 而不是给竖条写负 `left`：gpui 的绝对定位把负 inset 丢了。**
        // 2026-10-08 一次构建里同排两根 3×20 的条做对照：`left(0)` 那根出（帧 x59..61
        // 实测到色），`left(-10)` 那根**一像素都没出**（x49..58 逐列打色全是卡的白面），
        // 而两者的纵向 `top_1_2 + mt(-10)` 完全一样 ⇒ 坏的只有那一个负 `left`。
        // 负 **margin** 是通的——`widgets::card_split` 就靠 `.mx(px(-GAP_XL))` 铺满整卡宽，
        // 那条早就在帧上验过。所以行的盒子往外扩（`.mx(px(-SIDE_GUTTER))`），
        // 淡底那一层再用 `mx(SIDE_GUTTER)` 收回**改之前一模一样的位置**，
        // 多出来的那 10px 气口就是竖条的落点（与 `card_split` 同一条 idiom，不是新发明）。
        //
        // ⚠️ **气口不能做成行的 `px`，必须做成淡底那层的 `mx`**（同一轮帧上实测翻出来的一条）：
        // 绝对定位子元素的包含块是父元素的 **padding 盒**，而 `px` 不减 padding 盒的左缘——
        // 第一版写成 `.mx(-10).px(10)`，竖条 `left_0` 落在 59（淡底那一格）而不是 49，
        // 又因为它在 `children` 里排在淡底**前面**，被淡底那层不透明底色整个盖住，
        // 帧上 49..58 依旧全白。改成"行没有水平内距 + 淡底自己带 `mx`"之后
        // 包含块的左缘 = 盒子本身 = 49，竖条与淡底也**不再重叠**，两处一起对上。
        //
        // ⚠️ 这条负 `mx` 能不能落地还取决于**树有没有把裁剪盒扩到那里**
        //（见 `sidebar()` 里那条 `.mx(px(-GAP))`）——**两者是一对，少一边竖条就整个消失**
        let name = self.title(&path);
        h_flex()
            .id(format!("side-repo-{ix}"))
            .h(px(SIDE_ROW_H))
            .flex_shrink_0()
            // ⚠️ **`items_stretch` 不是 `items_center`**：`h_flex` 默认在交叉轴上居中，
            // 那样淡底会缩成**内容高**（字 20 上下留白），选中那一抹淡底就不铺满 36 了。
            // 淡底要占满整行高 = 让这一行的交叉轴拉伸（AGENTS「h_flex 与 v_flex 不对称」）
            .items_stretch()
            .mx(px(-SIDE_GUTTER))
            .relative()
            // 选中那根左侧 3px 品牌竖条（原型 `.repo-item.on::before`）
            .when(selected, |r| {
                r.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_1_2()
                        .mt(px(-SIDE_BAR_H / 2.0))
                        .w(px(SIDE_BAR_W))
                        .h(px(SIDE_BAR_H))
                        .rounded_r(px(R_SM))
                        .bg(t.colors.accent),
                )
            })
            .child(
                h_flex()
                    .id(format!("side-face-{ix}"))
                    .flex_1()
                    .min_w_0()
                    // ⚠️ **淡底靠自己的 `mx` 收回气口**，不是靠行的 `px`——
                    // 写成行的 `px` 会把竖条的包含块一起往右推（见上面那段实测）
                    .mx(px(SIDE_GUTTER))
                    .items_center()
                    .gap(px(GAP))
                    .px(px(SIDE_GUTTER))
                    .rounded(px(r_lg(t)))
                    .text_color(tier3(t))
                    .when(selected, |s| s.bg(wash(t)))
                    .hover(|s| s.bg(if selected { wash(t) } else { track(t) }))
                    .child(icon_sm(bucket_icon(b)).text_color(dot_color(b, t)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(fs_sec()))
                            // 选中那行名字升一档墨色并加粗（原型 `.repo-item.on .nm{ink;700}`）：
                            // 淡底色差只有 12/255，**只靠底色说不清"是哪一行被选了"**
                            .text_color(if selected { body(t) } else { tier3(t) })
                            .font_weight(if selected {
                                FontWeight::BOLD
                            } else {
                                FontWeight::MEDIUM
                            })
                            .child(name),
                    )
                    .child(match badge {
                        Badge::None => div().into_any_element(),
                        Badge::Pill { text, fg, bg } => pill(&text, fg, bg).into_any_element(),
                        Badge::Spinner { color } => Spinner::new()
                            .color(color)
                            .with_size(Size::XSmall)
                            .into_any_element(),
                    }),
            )
            .on_click({
                let me = me.clone();
                let path = path.clone();
                move |_, window, cx| {
                    me.update(cx, |this, cx| {
                        this.side_focus = if third { Group::Third } else { Group::Mine };
                        this.reveal_repo(path.clone(), window, cx);
                    });
                }
            })
            .context_menu({
                let me = me.clone();
                move |menu, _, cx| repo_context_menu(menu, &path, &me, cx)
            })
            .into_any_element()
    }
}

// ================= 右键菜单 =================

/// 两类仓库各一套菜单。动词、启用态、结果必须与仓库头、⌘K 三处一致。
///
/// 全部走 `Entity<Shell>` 回写，不用 `cx.listener`：`context_menu` 的闭包里
/// 拿不到 `Context<Self>`，而 `&mut Window` 也不能被 `'static` 闭包捕获。
fn repo_context_menu(menu: PopupMenu, path: &str, me: &Entity<Shell>, cx: &mut App) -> PopupMenu {
    let path = path.to_string();
    let third = me
        .read(cx)
        .entry(&path)
        .map(|e| e.is_third_party)
        .unwrap_or(false);
    let remote = net::remote_url(&path);
    let has_remote = !remote.trim().is_empty();
    let web_url = net::browse_url_from_remote(&remote);

    let mut menu = menu
        .item(PopupMenuItem::new("打开").on_click({
            let me = me.clone();
            let path = path.clone();
            move |_, window, cx| {
                me.update(cx, |this, cx| this.reveal_repo(path.clone(), window, cx));
            }
        }))
        // 「提交并推送…」只出现在我的仓库，第三方出现即为越界。
        // 阶段 4 时这个按钮是置灰的空壳（那时提交对话框还没做），
        // 阶段 8 做完对话框之后忘了接回来——设计 3.6 把它记成"已接"，
        // 实际入口是断的。这里补上，并把对话框开在**右键的那一行**上
        .when(!third, |menu| {
            menu.item(
                PopupMenuItem::new("提交并推送…")
                    .icon(icon_sm(AssetIcon::GitMerge))
                    .on_click({
                        let me = me.clone();
                        let path = path.clone();
                        move |_, window, cx| {
                            me.update(cx, |this, cx| {
                                this.open_commit_dlg(Some(path.clone()), window, cx)
                            });
                        }
                    }),
            )
        })
        .item(PopupMenuItem::separator())
        .item(
            PopupMenuItem::new(if third {
                "移回我的仓库"
            } else {
                "移到第三方仓库"
            })
            .icon(icon_sm(AssetIcon::ArrowUpRight))
            .on_click({
                let me = me.clone();
                let path = path.clone();
                move |_, window, cx| {
                    me.update(cx, |this, cx| this.set_group(&path, !third, window, cx));
                }
            }),
        )
        .item(
            PopupMenuItem::new("打开文件夹")
                .icon(icon_sm(AssetIcon::FolderOpen))
                .on_click({
                    let path = path.clone();
                    move |_, window, cx| notify_err(os::open_folder(&path), window, cx)
                }),
        )
        .item(
            PopupMenuItem::new("在终端中打开")
                .icon(icon_sm(AssetIcon::Terminal))
                .on_click({
                    let path = path.clone();
                    move |_, window, cx| notify_err(os::open_terminal(&path), window, cx)
                }),
        )
        .item(
            PopupMenuItem::new("复制远程地址")
                .icon(icon_sm(AssetIcon::Copy))
                .disabled(!has_remote)
                .on_click(move |_, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(remote.clone()));
                    window.push_notification("已复制远程地址", cx);
                }),
        );

    // 「在云端打开」只对第三方有意义：我的仓库我已经在本地干活了
    if third {
        menu = menu.item(
            PopupMenuItem::new("在云端打开")
                .icon(icon_sm(AssetIcon::ExternalLink))
                .disabled(web_url.is_none())
                .on_click(
                    move |_, window, cx| match web_url.as_deref().map(os::open_url) {
                        Some(Ok(())) => {}
                        Some(Err(e)) => window.push_notification(e, cx),
                        None => window.push_notification("该仓库没有可识别的远端地址", cx),
                    },
                ),
        );
    }

    menu.item(PopupMenuItem::separator())
        .item(
            PopupMenuItem::new("重命名…")
                .icon(icon_sm(AssetIcon::Pencil))
                .on_click({
                    let me = me.clone();
                    let path = path.clone();
                    move |_, window, cx| {
                        me.update(cx, |this, cx| {
                            this.open_rename_dlg(path.clone(), window, cx)
                        })
                    }
                }),
        )
        .item(
            PopupMenuItem::new("编辑…")
                .icon(icon_sm(AssetIcon::SquarePen))
                .on_click({
                    let me = me.clone();
                    let path = path.clone();
                    move |_, window, cx| {
                        me.update(cx, |this, cx| this.open_edit_dlg(path.clone(), window, cx))
                    }
                }),
        )
        .item(PopupMenuItem::separator())
        .item(
            PopupMenuItem::new("移除")
                .icon(icon_sm(AssetIcon::Trash))
                .on_click({
                    let me = me.clone();
                    let path = path.clone();
                    move |_, window, cx| {
                        me.update(cx, |this, cx| this.remove_repo_undoable(&path, window, cx))
                    }
                }),
        )
}

// ================= 小块 =================

/// 过滤：按显示名 / 仓库名 / 分支做不区分大小写的包含匹配。
/// 空串 = 不过滤（最常见的情况，不必走一遍字符串处理）
///
/// `pub(crate)`：`actions.rs::apply_reveal` 要用同一条规则算行号，
/// 那边看不到"这一行被过滤掉了"，就会滚到一个不存在的行上
pub(crate) fn matches_filter(r: &RepoEntry, status: &RepoStatus, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    format!("{} {} {}", r.name, r.alias, status.branch)
        .to_lowercase()
        .contains(q)
}

/// 每一档的图标。**形状也带信息**，不只靠颜色区分——五档全靠色相的话，
/// 色觉障碍下"落后"与"已分叉"会长得一模一样
pub(crate) fn bucket_icon(b: Bucket) -> AssetIcon {
    match b {
        Bucket::Synced => AssetIcon::Check,
        Bucket::Dirty => AssetIcon::CircleDot,
        Bucket::Ahead => AssetIcon::ArrowUp,
        Bucket::Behind => AssetIcon::ArrowDown,
        Bucket::Forked => AssetIcon::GitMerge,
        Bucket::Unknown => AssetIcon::Circle,
    }
}

/// 仓库行右侧那一格。**三种形态而不是五种**——
/// 240px 的行里只塞得下一枚胶囊，形状不同时语义才分得开：
/// 胶囊 = 静态结论，转圈 = 正在进行，空 = 这一档不需要动手
#[derive(Clone)]
enum Badge {
    None,
    Pill { text: String, fg: Hsla, bg: Hsla },
    Spinner { color: Hsla },
}

/// 空闲时的分档徽标。**只放"现在需要动手"的数字**：
/// 干净的列表比满屏胶囊好读——胶囊一多，每一颗都不显眼了
fn bucket_badge(status: &RepoStatus, changed: u32, third: bool, t: &Theme) -> Badge {
    let (kind, fg, bg) = badge_tone(bucket_of(status), changed, third, t);
    match kind {
        BadgeKind::None => Badge::None,
        BadgeKind::Behind => Badge::Pill {
            text: if third {
                "可更新".into()
            } else {
                "落后".into()
            },
            fg,
            bg,
        },
        BadgeKind::Forked => Badge::Pill {
            text: "已分叉".into(),
            fg,
            bg,
        },
        BadgeKind::Ahead => Badge::Pill {
            text: format!("待推送 {}", status.ahead),
            fg,
            bg,
        },
        BadgeKind::Dirty => Badge::Pill {
            text: format!("未提交 {changed}"),
            fg,
            bg,
        },
    }
}

/// 分档徽标落在哪一档。**纯逻辑**：不碰主题，所以能单测。
///
/// 单独抽出来的理由不是"为了分层"，是**「状态」列那句话曾经把
/// 「没扫过」写成「已同步」**（阶段 16）——判据藏在 `badge_tone` 里、
/// 和取色缠在一起，就没法单独钉住。
pub(crate) fn badge_kind_of(b: Bucket, changed: u32) -> BadgeKind {
    match b {
        Bucket::Behind => BadgeKind::Behind,
        Bucket::Forked => BadgeKind::Forked,
        Bucket::Ahead => BadgeKind::Ahead,
        // 没扫过不是"没消息"，它落进 `None` 只是因为「没有要动手的事」，
        // 显示那一侧另有分支（`ov_status_text`）
        Bucket::Unknown => BadgeKind::None,
        // 未提交与上面几档可重叠，是独立维度，所以放在后面兜底
        _ if changed > 0 => BadgeKind::Dirty,
        _ => BadgeKind::None,
    }
}

/// 分档徽标落在哪一档。**全站唯一一处**写这个判定。
///
/// 文案由调用方拼而不是这里给：侧栏那一格只有 200px，要带数字（"待推送 3"），
/// 总览表格的「状态」列旁边就有独立的数字列，再带一遍就是重复信息。
/// 但"哪一档、什么颜色"必须只有一份——以前两边各写一遍映射，
/// 结果总览写「落后 N」侧栏写「落后」，同一仓库两屏两种说法，
/// 一眼就让人怀疑其中一屏的数据不对
pub(crate) fn badge_tone(
    b: Bucket,
    changed: u32,
    third: bool,
    t: &Theme,
) -> (BadgeKind, Hsla, Hsla) {
    let tone = |kind: BadgeKind, fg: Hsla, bg: Hsla| (kind, fg, bg);
    let kind = badge_kind_of(b, changed);
    match kind {
        // 「可更新」与「落后」同色，只是文案按归属分——第三方不是我的项目，
        // 说它"落后"像在挑刺
        BadgeKind::Behind => {
            let _ = third;
            tone(kind, orange(t), orange_bg(t))
        }
        // 分叉是红（Q7）：徽标、状态点、分叉条**三处必须同一色**，
        // 同一屏里出现"这里红、那里橙"会让人以为那是两件事。
        // （原来这一条列的是四处，第四处是同步环——那颗环 2026-10-05 已下线）
        BadgeKind::Forked => tone(kind, t.colors.danger, red_bg(t)),
        BadgeKind::Ahead => tone(kind, t.colors.accent, blue_bg(t)),
        BadgeKind::Dirty => tone(kind, t.colors.accent, blue_bg(t)),
        // `None` 有两条来路，显示必须分开：
        // **没扫过** → 中性灰。它进不了「需要动手」那一列，不该穿行动色；
        // 同一行里状态点是灰的（`dot_color` 的 Unknown 档 = border），
        // 胶囊若是蓝的，一行两种语气。形状仍是 `None`，
        // 侧栏那一格因此什么都不画，与「徽标只放要动手的数字」一致
        BadgeKind::None if b == Bucket::Unknown => tone(kind, tier3(t), track(t)),
        BadgeKind::None => tone(kind, t.colors.accent, blue_bg(t)),
    }
}

/// 分档徽标的档位。单独一个类型而不是直接给字符串：
/// 逼调用方自己拼文案，文案密度才可能按版面调整
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum BadgeKind {
    None,
    Behind,
    Forked,
    Ahead,
    Dirty,
}

/// **当前这一组**没有仓库时的占位。
///
/// 收 `group` 是因为侧栏只画 `side_focus` 那一组：空态必须说清是**哪一组**空，
/// 否则停在「第三方仓库」而它恰好是空的时，一句"还没有仓库"会让人以为自己
/// 刚加的仓库不见了。`other_has_repos` 决定要不要给"换一组看看"那句——
/// 两边都空时说这句是空话，指点一个空地方不如不说
fn no_repos(group: Group, other_has_repos: bool, t: &Theme) -> Div {
    let (label, _) = group.label_and_icon();
    v_flex()
        .flex_1()
        .min_h_0()
        .items_center()
        .justify_center()
        .px(px(GAP_LG))
        .gap(px(GAP_TIGHT))
        .child(
            div()
                .text_size(px(fs_sec()))
                .text_color(tier3(t))
                .child(format!("{label}里还没有仓库")),
        )
        .child(
            div()
                .text_size(px(fs_meta()))
                .text_color(tier4(t))
                .child("用下面的「添加仓库」加一个"),
        )
        .when(other_has_repos, |col| {
            col.child(
                div()
                    .text_size(px(fs_meta()))
                    .text_color(tier4(t))
                    .child("也可以从左边导航轨切到另一组"),
            )
        })
}

/// 失败提示统一走通知，不弹模态
fn notify_err(r: Result<(), String>, window: &mut Window, cx: &mut App) {
    if let Err(e) = r {
        window.push_notification(e, cx);
    }
}
