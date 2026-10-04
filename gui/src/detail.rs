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
use gpui_kit::{px, AnyElement, Context, IntoElement, ParentElement, Styled};

use crate::state::*;
use crate::tokens::*;

// ================= 详情页共用骨架 =================

/// 详情页页签。**两类仓库共用一套，按归属裁剪**。
///
/// 2026-10-05 调整过一次序列：
/// - 「同步设置」**撤掉**，内容并进概览页同步段右上角那颗「定时同步」弹窗
///   （它是设置不是动作，给它一整页签不如给一个入口）。
/// - 「变更」**从右栏搬进来**：右栏原本有变更/历史/空间三个页签，与这里的
///   历史/空间是同一份数据、同一套内容，两处并排就是两个入口看同一件事。
///   变更同理——它已经在概览页底部有过一张摘要卡（已删），现在升格成页签。
///
/// 第三方裁掉的是「空间」：那是**我自己磁盘上**的占用与清理，
/// 对一个我只是关注着的别人的项目没有意义，摆出来是点了会越界的入口。
/// 「变更」留着——第三方仓库只要在本地有一份工作区，未提交改动就真实存在。
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum DetailTab {
    Overview,
    Readme,
    History,
    Space,
    Changes,
}

impl DetailTab {
    /// 两类仓库的完整页签序列。第三方裁掉「空间」
    pub(crate) fn for_third(third: bool) -> &'static [DetailTab] {
        if third {
            &[
                DetailTab::Overview,
                DetailTab::Readme,
                DetailTab::History,
                DetailTab::Changes,
            ]
        } else {
            &[
                DetailTab::Overview,
                DetailTab::Readme,
                DetailTab::History,
                DetailTab::Space,
                DetailTab::Changes,
            ]
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
    /// 当前停在哪个详情页页签。**页签序列按归属裁剪**，所以下标要夹回范围再取——
    /// 从第三方切到我的仓库时，原来那个下标可能停在被裁掉的「同步设置」上
    pub(crate) fn cur_detail_tab(&self) -> DetailTab {
        let tabs = DetailTab::for_third(self.selected_is_third());
        tabs[self.detail_tab.min(tabs.len() - 1)]
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
    /// 切到某个详情页页签。**给按钮用**（磁盘摘要卡的「查看」、抽屉「历史」的入口）。
    ///
    /// 按枚举切而不是按下标：页签序列按归属裁剪，写死下标的话在第三方
    /// 仓库上会切到一个不存在的位置
    pub(crate) fn open_detail_tab(&mut self, tab: DetailTab, cx: &mut Context<Self>) {
        let tabs = DetailTab::for_third(self.selected_is_third());
        if let Some(ix) = tabs.iter().position(|x| *x == tab) {
            self.detail_tab = ix;
            self.sync_detail_tab(cx);
            // 命中缓存时 `scan_history` 不会 notify（它没改任何东西），
            // 少了这一行页签下标改了却画不出来
            cx.notify();
        }
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
        let tabs = DetailTab::for_third(third);
        // 切仓库后原来那个下标可能越界（比如从第三方切到我的，
        // 下标还停在被裁掉的「同步设置」上）——夹回范围内
        let ix = self.detail_tab.min(tabs.len() - 1);
        let cur = tabs[ix];

        v_flex()
            .flex_1()
            .min_h_0()
            .h_full()
            .bg(t.colors.background)
            .p(px(GAP_LG))
            .gap(px(GAP_LG))
            .child(self.repo_head(path, t, cx))
            .child(
                // 页签间距（用户要求放大）走**官方的 `Size` 档**，不自己加
                // `gap()`：`TabBar` 的 `gap` 是按 `(variant, size)` 写死的
                // （`tab_bar.rs:394-399`），公开 API 没有单独调它的口。
                // `Underline` 档下 Small→Medium 的实际变化：
                //   gap 12→16、页签高 22→26、**字号不变**（两档都 `text_sm()`，
                //   `tab.rs:803-807` 的 `_` 臂），`inner_paddings` 恒为 0
                //   （`tab.rs:80-82`，Underline 不吃水平内边距）。
                // 也就是"要的就是间距变大"，而唯一会跟着变的多出来 4px 高
                // 正好把页签从贴字变成有呼吸的一行。
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
            )
            .child(self.detail_body(path, cur, t, cx))
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
        v_flex()
            .flex_1()
            .min_h_0()
            .gap(px(GAP))
            .children(self.fork_card(t, cx))
            .child(self.sync_card(path, t, cx))
            .child(self.log_panel(t, cx).min_h(px(LOG_MIN_H)))
            .child(self.lang_section(t))
    }
}


// ================= Screen 1：第三方仓库档案页 =================

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
        v_flex()
            .flex_1()
            .min_h_0()
            .gap(px(GAP))
            .child(self.sync_card(path, t, cx))
            .child(self.log_panel(t, cx).min_h(px(LOG_MIN_H)))
            .child(self.lang_section(t))
    }
}
