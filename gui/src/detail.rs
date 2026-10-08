//! 仓库详情页：两类仓库共用的骨架（仓库头 + 页签）+ 各自的概览内容。
//!
//! **两类仓库进的是同一个 `detail_page`。** 头承载"我在哪个仓库、能跳到哪去"，
//! 这件事对"我的"和"第三方"没有区别；分成两个页面就必然出现两套会各自漂移的
//! 头部与取色。差异只体现在两处：页签序列（`DetailTab::for_third`）与
//! 概览页的内容（`workbench_body` / `profile_body`）。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::base::v_flex;
use gpui_kit::component::tab::{Tab, TabBar, TabVariant};
use gpui_kit::component::{Sizable, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{div, px, AnyElement, Context, IntoElement, ParentElement, Styled};
use qingbird_core::store;

use crate::state::*;
use crate::tokens::*;

// ================= 详情页共用骨架 =================

/// 详情页页签。**两类仓库共用一套，按可见性裁剪**。
///
/// 2026-10-05 调整过一次序列：
/// - 「同步设置」**撤掉**，内容并进概览页同步段右上角那颗「定时同步」弹窗
///   （它是设置不是动作，给它一整页签不如给一个入口）。
/// - 「变更」**从右栏搬进来**：右栏原本有变更/历史/空间三个页签，与这里的
///   历史/空间是同一份数据、同一套内容，两处并排就是两个入口看同一件事。
///   变更同理——它已经在概览页底部有过一张摘要卡（已删），现在升格成页签。
///
/// **第三方天生没有「空间」**（阶段 19）：那是**我自己磁盘上**的占用与清理，
/// 对一个我只是关注着的别人的项目没有意义，摆出来是点了会越界的入口
/// ——那页里有一键清理，会**真删** `node_modules` / `target` / `dist`。
/// 这条现在是 core 的归属基线 `THIRD_BASELINE_HIDDEN`，
/// 设置里的开关**放不出来**（它编码的是安全边界，不是偏好）。
/// 「变更」两边都留着——第三方仓库只要在本地有一份工作区，
/// 未提交改动就真实存在
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum DetailTab {
    Overview,
    Readme,
    History,
    Space,
    Changes,
}

impl DetailTab {
    /// 这一类仓库**全都适用**的页签序列（含第三方的「空间」）
    ///
    /// ⚠️ **这里不再按归属裁剪**（阶段 19）：原来这个函数对第三方少给一个
    /// 「空间」，那是**一张手写的裁剪表**——用户要"可勾选显示/隐藏"时，
    /// 它就该被配置取代。现在归属差异走 core 的 `THIRD_BASELINE_HIDDEN`
    /// （安全边界：第三方那个「空间」页里有**一键清理**，真删文件），
    /// 用户勾选再叠在上面，两条路合流到一个判据
    pub(crate) const ALL: &'static [DetailTab] = &[
        DetailTab::Overview,
        DetailTab::Readme,
        DetailTab::History,
        DetailTab::Space,
        DetailTab::Changes,
    ];

    /// 模块键（与 core `store::MOD_TAB_*` 一一对应）
    pub(crate) fn module_key(self) -> &'static str {
        match self {
            DetailTab::Overview => store::MOD_TAB_OVERVIEW,
            DetailTab::Readme => store::MOD_TAB_README,
            DetailTab::History => store::MOD_TAB_HISTORY,
            DetailTab::Space => store::MOD_TAB_SPACE,
            DetailTab::Changes => store::MOD_TAB_CHANGES,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            DetailTab::Overview => "概览",
            DetailTab::Readme => "说明",
            DetailTab::History => "历史",
            DetailTab::Space => "空间",
            DetailTab::Changes => "变更",
        }
    }
}

impl Shell {
    /// 当前停在哪个详情页页签。**页签序列是筛出来的**，所以下标要夹回范围再取——
    /// 从第三方切到我的仓库时，原来那个下标可能停在被藏掉的那一页上
    pub(crate) fn cur_detail_tab(&self) -> DetailTab {
        let tabs = self.visible_detail_tabs(self.selected_is_third());
        // ⚠️ `visible_detail_tabs` 保证至少一个（core 那个不变量），
        // 所以这里 `len() - 1` 不会下溢。少写那句不变量的话，
        // 用户把页签全勾掉就能让 debug 构建**每帧 panic 一次**
        tabs[self.detail_tab.min(tabs.len() - 1)]
    }

    /// 这一类仓库**藏了哪些**模块（归属基线 + 用户勾的，合成一份）
    pub(crate) fn hidden_for(&self, third: bool) -> Vec<&str> {
        let user: &[String] = if third {
            &self.hidden_third
        } else {
            &self.hidden_mine
        };
        let mut out: Vec<&str> = user.iter().map(String::as_str).collect();
        if third {
            out.extend(store::THIRD_BASELINE_HIDDEN.iter().copied());
        }
        out
    }

    /// 这个模块在这个归属下显不显示。**判据只有这一份**
    /// ——页签栏、概览四段、仓库头那颗磁盘摘要徽标都问它
    pub(crate) fn module_on(&self, third: bool, key: &str) -> bool {
        store::module_shown(&self.hidden_for(third), key)
    }

    /// 这一类仓库**该显示哪几个页签**
    ///
    /// **全藏了回退到全给**（core `visible_modules`）——页签栏是导航本身，
    /// 一个都不剩的话详情页就没有可去的地方了
    pub(crate) fn visible_detail_tabs(&self, third: bool) -> Vec<DetailTab> {
        let hidden = self.hidden_for(third);
        let keys: Vec<&str> = DetailTab::ALL.iter().map(|t| t.module_key()).collect();
        store::visible_modules(&keys, &hidden)
            .into_iter()
            .filter_map(|k| DetailTab::ALL.iter().find(|t| t.module_key() == k).copied())
            .collect()
    }

    /// 当前选中的是不是第三方仓库
    pub(crate) fn selected_is_third(&self) -> bool {
        self.selected
            .as_deref()
            .and_then(|p| self.entry(p))
            .map(|e| e.is_third_party)
            .unwrap_or(false)
    }
}

impl Shell {
    /// 切到某个详情页页签。**给按钮用**（磁盘摘要卡的「查看」等）。
    ///
    /// 按枚举切而不是按下标：页签序列是**筛出来的**，写死下标的话
    /// 在藏过几个页签的仓库上会切到一个不存在的位置
    pub(crate) fn open_detail_tab(&mut self, tab: DetailTab, cx: &mut Context<Self>) {
        let tabs = self.visible_detail_tabs(self.selected_is_third());
        if let Some(ix) = tabs.iter().position(|x| *x == tab) {
            self.detail_tab = ix;
            self.sync_detail_tab(cx);
            // 命中缓存时 `scan_history` 不会 notify（它没改任何东西），
            // 少了这一行页签下标改了却画不出来
            cx.notify();
        }
        // ⚠️ **不可见就什么都不做**：曾经这里是"找不到就静默返回"，
        // 而调用方（仓库头那颗磁盘摘要徽标）不知道自己点了没反应，
        // 画面上就变成"点了跳到别的地方"。现在入口本身会跟着页签一起隐，
        // 这里只是**最后一道**——入口藏了就没人点得到它
    }

    /// 页签切到「历史」时补一次扫描。`git log` 要派生子进程，
    /// 放在切页签这一刻而不是每轮刷新，是这个页签唯一能被点开的入口
    fn sync_detail_tab(&mut self, cx: &mut Context<Self>) {
        if self.cur_detail_tab() == DetailTab::History {
            self.scan_history(cx);
        }
    }
}

impl Shell {
    /// 详情页骨架：仓库头（常驻）/ 远程地址 / 页签 / 内容区。
    ///
    /// **两类仓库进的是同一个函数**，只有 `for_third` 那一段不同。
    /// 页签内容按 `Shell::detail_tab` 分发，而它是**全局的**——
    /// 切页签不是切仓库，多仓库用户不该每换一个仓库都要点回「概览」
    pub(crate) fn detail_page(
        &self,
        path: &str,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let third = self.entry(path).map(|e| e.is_third_party).unwrap_or(false);
        let tabs = self.visible_detail_tabs(third);
        // 切仓库后原来那个下标可能越界（比如从第三方切到我的，
        // 下标还停在被藏掉的那一页上）——夹回范围内
        let ix = self.detail_tab.min(tabs.len() - 1);
        let cur = tabs[ix];

        v_flex()
            .flex_1()
            .min_h_0()
            .h_full()
            .bg(t.colors.background)
            // **四边缩进（2026-10-08 用户定，同参照 = 窗口客户区边）**：
            // 上 52 = 标题栏 40 + 主区 `pt` 8 + 行盒误差 4；左 41 = 轨 40 + 边框 1 + 主区 **0**；
            // 右 9 = 卡边框 1 + **壳层那一格 `pr`（写在 `shell.rs`，不在这里）**；
            // 下 28 = 主区 **0** + 状态条 28。
            // ⚠️ 这一页的 `pt` 是 8，而**总览页那一格是 0**——不是双写：总览页顶上压着
            // 页头那一行（「总览」+ 计数句并排），那 38px 就是缓冲；这一页第一格直接是头卡，
            // 给 0 会让卡的上边框坐在标题栏下边线上、看着顶死。**两页顶部构件不同，
            // 才允许这两个值不同**；哪天这一页也加了页头标题行，这一格就要跟着归零
            .pt(px(GAP))
            // ⚠️ **卡间距只由这一条 `gap` 给一次**（v4 §3.2、D90）。
            // 阶段 28 踩过的那次：给滚动容器又加了一层 `padding-top`，叠在这个
            // gap 上 ⇒ 头卡下 20、卡间 10，用户报"间距太大又不一致"，而根因
            // 不是那个数字。**新加一层容器前先数这个间距被谁写了几次。**
            //
            // 2026-10-08 用户定「卡片与卡片之间统一 6px」⇒ 代码 **4**，帧上量 6
            // （相邻两卡各一条 1px 边框）。口径推导记在 `tokens::GAP_TIGHT`
            .gap(px(GAP_TIGHT))
            // **仓库头从"浮在画布上的一堆文字"改成一张卡**（用户：「右边上面
            // 仓库名那块感觉空了一大块，感觉也要加上卡片底」）。
            // 页签一并**收进这张卡的底部**（v4 §7「页签：下划线式，收进头卡底部」）——
            // 它是这张卡的下级导航，浮在卡外面就变成"一屏两个头"。
            // ⚠️ 这里替换掉的是阶段 21-7 那条"头部与页签之间用 `GAP` 收紧"的
            // 处理：那句话说的是"页签是仓库头的下级"，**判断没错，位置错了**——
            // 收紧到 8px 仍然救不了"页签浮在卡外"，收进卡里才是同一个意思的成立形态。
            .child(
                crate::widgets::card(t)
                    .gap(px(GAP_TIGHT))
                    // ⚠️ **底部内边距归 0**（原型 `.repo-head{padding:15px 18px 0}`）。
                    // 这张卡的最后一格是页签行，而页签行用 `card_split` 的负边距
                    // **撑到了卡的左右边**——它下面再留 16px 白，读起来就是
                    // "页签浮在卡里的一块岛上"，而原型那条下划线是压在卡底那条边上的
                    // （`.tab{margin-bottom:-1px}` 靠的就是"底下没有 padding"这件事）。
                    // 原型左右给 18、上给 15，与 `card()` 的 16 差 2/1 px——
                    // **不为这两像素再开一处 padding 出处**（v4 §3.2），只把"底部为 0"
                    // 这一条结构差异落下来
                    .pb_0()
                    .child(self.repo_head(path, t, cx))
                    .child(
                        // 页签间距走**官方的 `Size` 档**，不自己加 `gap()`：
                        // `TabBar` 的 gap 按 `(variant, size)` 写死（`tab_bar.rs:394-399`），
                        // 公开 API 没有单独调它的口。`Underline` 档 Small→Medium 的实际变化：
                        // gap 12→16、页签高 22→26、**字号不变**（两档都 `text_sm()`，
                        // `tab.rs:803-807` 的 `_` 臂）。也就是"要的就是间距变大"。
                        crate::widgets::card_split(t).child(
                            TabBar::new("detail-tabs")
                                .w_full()
                                .flex_shrink_0()
                                .with_variant(TabVariant::Underline)
                                .with_size(Size::Medium)
                                .selected_index(ix)
                                .on_click(cx.listener(|this, ix, _, cx| {
                                    this.detail_tab = *ix;
                                    this.sync_detail_tab(cx);
                                    cx.notify();
                                }))
                                .children(tabs.iter().map(|tb| Tab::new().label(tb.label()))),
                        ),
                    ),
            )
            .child(
                // 内容区自己就是一张卡（各页签内部用 `widgets::card`），
                // 所以这里**不再套一层 `mt`**——那一档 4px 叠在父 gap 上，
                // 就是"头卡下 20、卡间 10"那个症状的同一个形状
                v_flex().flex_1().min_h_0().child(self.detail_body(path, cur, t, cx)),
            )
    }

    /// 页签内容。五个页签都有实现，这里不留"还没接"的兜底分支——
    /// 留着的话新增页签会被静默接进一个空态，而空态看上去和"真的没内容"一样
    ///
    /// **「概览」是唯一被拆成上下两段的页签**：上半是按内容高排的卡片堆，
    /// 下半是日志区。用户口径是"日志是主要展示区，下半段基本就是用来展示日志的"，
    /// 所以日志给更大的下限高度，卡片堆宁可自己滚。
    /// 其余四个页签各自已经能填满并自滚（说明是长文、历史是官方 List、
    /// 空间自带 `*_scroll`、变更是文件清单）
    fn detail_body(
        &self,
        path: &str,
        tab: DetailTab,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        match tab {
            DetailTab::Overview => {
                // 两个 body 各自返回 `impl IntoElement`，不透明类型不同，
                // 这里必须先收成 `AnyElement` 才能放进同一个分支
                let body: AnyElement = if self.selected_is_third() {
                    self.profile_body(path, t, cx).into_any_element()
                } else {
                    self.workbench_body(path, t, cx).into_any_element()
                };
                v_flex().flex_1().min_h_0().child(body).into_any_element()
            }
            DetailTab::Readme => v_flex()
                .flex_1()
                .min_h_0()
                .child(self.readme_body(path, t, cx))
                .into_any_element(),
            DetailTab::History => v_flex()
                .flex_1()
                .min_h_0()
                .child(self.history_body(t, cx))
                .into_any_element(),
            DetailTab::Space => v_flex()
                .flex_1()
                .min_h_0()
                .child(self.space_body(t, cx))
                .into_any_element(),
            DetailTab::Changes => v_flex()
                .flex_1()
                .min_h_0()
                .child(self.changes_panel(t, cx))
                .into_any_element(),
        }
    }
}

// ================= 我的仓库：概览页内容 =================

/// 概览页日志段的下限高度。
///
/// **它是三段里唯一吃剩余高度的一段**（`log_panel` 内部 `flex_1`），
/// 上面同步段与下面语言段按内容高固定、压不动。所以窗口越高，日志越大；
/// 写死 5:3 之类的比例反而会在窗口变小时先把日志压死——
/// 那一段是"操作完立刻要看"的东西，它被压到 6 行就等于没有
const LOG_MIN_H: f32 = 200.0;

impl Shell {
    /// 我的工作台的**内容区**——概览页那三段，从上到下：
    ///
    /// ```text
    ///  分叉条      （只在本地与云端分叉 / 变基备份还在时出现）
    ///  同步        定时同步入口 + 三键 + 提交
    ///  日志        flex_1，吃剩余高度
    ///  语言技术栈  GitHub 那种色带 + 图例
    /// ```
    ///
    /// **同步在日志之上**：三键是这一屏的发起动作，先看见它再看见输出，
    /// 「按了 → 看结果」是一条向下的动线；反过来（先日志后按钮）要求用户
    /// 越过输出流去找操作入口。
    ///
    /// **分叉条排最上而不是跟着同步段**：它是"现在这么点会出事"的告警，
    /// 夹在别处会被输出流挤下去，而它恰恰是唯一一条不点掉就会让后面
    /// 操作失败的话
    ///
    /// **没有「常用命令」段**：「开发运行 / 编译构建」已经长在日志表头上，
    /// 再摆一排是同一对入口摆两遍。腾出来的高度全给日志。
    /// 测试 / 安装依赖 / 钉住的命令与**它们的编辑入口**都在 ⌘K
    /// （`palette.rs` 的 `RunSlot` 与 `EditCmds`）——留一个能配不能改的
    /// 残废配置才是真问题
    ///
    /// **「未提交改动」卡没有单独一段**：变更是中栏的一个页签了，
    /// 入口长在同步段那行「N 个文件待提交 · 看清单」上。
    /// 少一个入口比多一张只说同一句话的段好
    pub(crate) fn workbench_body(
        &self,
        path: &str,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.overview_body(path, t, cx, false)
    }
}


// ================= 概览页正文：两类仓库共用（阶段 19） =================

impl Shell {
    /// 概览页正文。**「我的」与「第三方」进的是同一个函数**（阶段 19）
    ///
    /// 以前是两个函数各写一遍版面，而它们**逐行相同、只差一个分叉条**——
    /// 两份迟早漂。现在分叉条是四个模块之一（`sec.fork`），
    /// 而它在第三方的**归属基线**里就是藏着的，于是"第三方没有分叉条"
    /// 这件事由基线表达，而不是由两份版面各自记住。
    ///
    /// `third` 只用来**问判据**与裁剪 `sync_card` 内部那把推送键，
    /// 版面本身两套一模一样——这正是用户那条设计原则的落法。
    pub(crate) fn overview_body(
        &self,
        path: &str,
        t: &Theme,
        cx: &mut Context<Self>,
        third: bool,
    ) -> impl IntoElement {
        // 四段里可见的那些。**顺序照旧**（分叉条 → 同步 → 日志 → 语言），
        // 藏掉某段不会让别的段顶上它的位置——`flex_1` 落在日志段上，
        // 藏掉语言技术栈时日志会把多出来的高度吃掉，这是对的
        let on = |k: &str| self.module_on(third, k);
        let any = on(store::MOD_SEC_FORK)
            || on(store::MOD_SEC_SYNC)
            || on(store::MOD_SEC_LOG)
            || on(store::MOD_SEC_LANG);

        v_flex()
            .flex_1()
            .min_h_0()
            // ⚠️ **卡间只认 `GAP_LG`，而且只由这一条 gap 给**（用户第③条：
            // 「仓库名卡片和语言技术栈卡片之间的间距要和下面日志的间距一样大」）。
            // 病根不在数字上：外层（`detail_body` 那层）用 `GAP_LG`、这一层用 `GAP`，
            // 于是**头卡→同步卡 12、同步卡→日志卡 8**，同一屏里两种"卡间"。
            // 现在两层都是 `GAP_LG`，并且这里**不再叠任何 `mt` / `padding-top`**——
            // 阶段 28 那次"给滚动容器又加一层 padding"叠在父 gap 上，
            // 出来的就是用户报的那种"看着就是不齐"而根因不是数值（D90）。
            .gap(px(GAP_LG))
            .when(on(store::MOD_SEC_FORK), |b| b.children(self.fork_card(t, cx)))
            .when(on(store::MOD_SEC_SYNC), |b| {
                b.child(self.sync_card(path, t, cx, on(store::MOD_SEC_LANG)))
            })
            .when(on(store::MOD_SEC_LOG), |b| {
                b.child(self.log_panel(t, cx).min_h(px(LOG_MIN_H)))
            })
            // ⚠️ **语言段正常情况下不是卡，是同步卡里的一段**（原型就这么长）。
            // 这里只处理一种退路：用户把「同步」这一段关掉了，那么它的宿主没了，
            // 语言段不能跟着凭空消失 ⇒ 自己套一张卡。
            // 两条路共用同一个 `lang_section(..)`，卡外壳也只认 `widgets::card` 一处
            .when(on(store::MOD_SEC_LANG) && !on(store::MOD_SEC_SYNC), |b| {
                b.child(crate::widgets::card(t).child(self.lang_section(t)))
            })
            // **全藏了要说清楚为什么空**，不能留一页白。
            // 这不是防御性代码：勾选区就在设置里摆着，全勾掉是用户做得到的
            .when(!any, |b| {
                b.child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .text_size(px(fs_body()))
                                .text_color(tier4(t))
                                .child("这一屏的模块都被关掉了 · 到「设置 → 显示哪些模块」里打开"),
                        ),
                )
            })
    }
}

impl Shell {
    /// 第三方仓库的**内容区**。头与页签由 `detail_page` 给。
    ///
    /// **版面与「我的」完全一样**（分叉条除外，它只对改本地历史有意义）：
    /// 同步 → 日志 → 语言技术栈，日志同样吃剩余高度。用户口径是"第三方
    /// 概览用相同布局，只是不用提交推送的按钮"——而**提交与推送的裁剪不
    /// 在这一层**：`sync_card` 内部本来就按 `third` 分支不画那把推送键、
    /// 留一个同尺寸空格子、也不画「提交并推送…」。**版面因此可以整段复用，
    /// 不需要为第三方写第二套**（两套版面迟早会漂）
    ///
    /// 原先这里还有一张「仓库档案」卡（近 30 天活跃度 + 语言构成 +
    /// 「看完整说明」），拆掉的理由：
    ///
    /// * **语言构成**并进 `lang_section`——同一条数据、同一个 `lang_bar`，
    ///   而且档案卡那条只有色带没有图例，合并后反而更全
    /// * **「看完整说明」**删掉——「说明」页签就摆在下面那一行，
    ///   而「我的」那一屏从来没有第三处 README 入口（D22 的结论）
    /// * **近 30 天活跃度**挪进仓库头（`repo_head::activity_chip`）——
    ///   那是第三方独有的数据，共用的三段里没有它的对应位置，而"这个项目
    ///   最近还活着吗"恰恰属于**概要位**该答的问题（同磁盘摘要徽标）
    ///
    /// README 不在这里：它已经占了「说明」页签整页，再在概览摊一份预览，
    /// 同一份 markdown 就有两个渲染入口，哪天改样式只改到一半
    pub(crate) fn profile_body(
        &self,
        path: &str,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // ⚠️ **不再是一份手写版面**（阶段 19）。它原来与「我的」那份**逐行相同、
        // 只少一个 `fork_card`**——两份函数各写一遍，改一处忘一处就要漂。
        // 现在两者都走 `overview_body`，分叉条是**四个模块之一**
        // （`sec.fork` 在第三方的基线里就是藏着的，于是这里什么都不用写）
        self.overview_body(path, t, cx, true)
    }
}
