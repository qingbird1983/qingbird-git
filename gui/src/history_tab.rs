//! 「历史」页签：最近提交列表 → 点行进 `Sheet` 看这次提交改了哪些文件、
//! 点文件看 patch。
//!
//! **两类仓库同款**（设计文档 3.3）：我自己项目的提交历史和第三方的是同一份需求，
//! 砍掉第三方那一半只是让界面更薄，没有收益。
//!
//! 用官方 `component::list::{List, ListState, ListItem}` + `ListDelegate`：
//! 它自带虚拟滚动、键盘上下走位与选中态。数据只存一份在
//! `Shell::history`，委托只带一个回指 `Shell` 的 Entity——和总览表格同一个套路，
//! 免得委托里存第二份（两份必然有一帧对不上）。
//!
//! **只在打开这个页签时才扫**。`git log` 要派生子进程，
//! 塞进每轮都跑的状态扫描里等于让所有仓库在后台无条件付这份代价。
//!
//! ## 为什么 Sheet 的内容要挂一个 Render 桥
//!
//! 官方 `Sheet` 走的是 `ParentElement`：children 是**一次性建好的元素**，
//! 没有 `Dialog::content` 那种每帧重画的闭包。直接 `.child(元素)` 的后果是
//! patch 加载完也不会换——那层元素建好就定型了。所以这里建一个
//! `HistorySheetView` 实体塞进去，它 `observe` 着 `Shell`，
//! `Shell` 一变就重画。这不是绕路，是官方 API 逼出来的唯一做法。

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::empty::{Empty, EmptyContent, EmptyHeader, EmptyTitle};
use gpui_kit::component::list::{List, ListDelegate, ListItem, ListState};
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::{ActiveTheme, IndexPath, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, AnyElement, App, AppContext, Context, Entity, FontWeight, Hsla, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use qingbird_core::history::{self, CommitFile, CommitPatch};
use qingbird_core::store;

use crate::state::*;
use crate::tokens::*;
use crate::widgets::{ghost_variant, meta_text, sunken_box, skeleton_bar, CardHeader};

/// 一次列多少条。`history::commit_log` 自己还会夹到它自己的上限
const LOG_LIMIT: usize = 200;
/// Sheet 里那条 patch 最多渲染多少行
const PATCH_MAX_LINES: usize = 2000;
/// diff 行的行高
const PATCH_LINE_H: f32 = 18.0;
/// Sheet 宽度。提交详情要左文件右 diff 两栏，760px 是两栏都读得舒服的下限
const SHEET_W: f32 = 760.0;
/// Sheet 里左侧文件列的宽
const FILE_COL_W: f32 = 260.0;

impl Shell {
    /// 「历史」页签正文
    pub(crate) fn history_body(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(path) = self.selected.as_deref() else {
            return empty_history(t, "没有选中仓库", "");
        };
        let h = &self.history;
        // 这份数据不是当前仓库的：说明页签刚被点开、扫描还没回来。
        // 摆着上一个仓库的提交比摆空列表更糟——那看着就是这个仓库的历史
        if h.path != path {
            return skeleton_history();
        }
        if h.scanning && h.commits.is_empty() {
            return skeleton_history();
        }
        if h.commits.is_empty() {
            return match &h.error {
                Some(e) => empty_history(t, "读提交历史失败", e),
                None => empty_history(t, "这个仓库还没有任何提交", ""),
            };
        }

        v_flex()
            .flex_1()
            .min_h_0()
            // 页级留白归主区外层（`detail_page`），这一层不再套一圈 `p`
            .gap(px(GAP_TIGHT))
            .child(
                // 白卡 + 卡内表头，与日志 / 说明 / 空间同一套（`widgets::card` +
                // `CardHeader`）。原先这里是裸列表加一行浮在上方的小字，
                // 那一行看着像内容的一部分，而不是"这一段的表头"
                crate::widgets::card(t)
                    .flex_1()
                    .min_h_0()
                    .child(
                        // 表头左缘与下面的提交行同一档（36）——从前它停在卡的 16，
                        // 于是"最近 8 次提交"比它下面的第一行靠左 20，
                        // 正是用户第 1 条里"同一页两档"的那一处。推导见 `widgets::tab_head`
                        crate::widgets::tab_head(
                            CardHeader::new(format!("最近 {} 次提交", h.commits.len()))
                                .right(
                                    Button::new("history-rescan")
                                        .icon(icon_sm(AssetIcon::RefreshCw).text_color(tier4(t)))
                                        .custom(ghost_variant(cx))
                                        .with_size(Size::XSmall)
                                        .loading(h.scanning)
                                        .tooltip("重新读取提交历史")
                                        .accessibility_label("重新读取提交历史")
                                        .on_click(cx.listener(|this, _, _, cx| this.rescan_history(cx)))
                                        .into_any_element(),
                                ),
                            t,
                        ),
                    )
                    // 官方 List 自己管虚拟滚动与选中态。行高必须一致——
                    // 它的测量逻辑假设所有行一样高。
                    //
                    // **左右留白与滑块落位与说明页同一档**（用户 2026-10-09 第 1 条）。
                    // 左右内距挂在 `List` 自己身上：官方把 `padding` 转交给内层滚动 Div
                    // （`list.rs:778-780`「Take paddings … and clear them from style」），
                    // 而滑块那根条按**滚动盒自己边框盒的右缘**画 ⇒ 内距只推内容、不推条。
                    // ⚠️ **但"向右探出"这一格不能也挂在 `List` 上**：`List` 自己 `render`
                    // 时不认外面的 margin，探出直接为 0，条就比另外两页偏左 5px
                    // （2026-10-09 实测 1332..1337 vs 说明 1337..1342）。
                    // ⇒ 探出挂在**外面这颗裸 Div** 上，`List` 在里面铺满它。
                    // 三张表与系数都在 `tokens::SCROLL_BLEED`
                    .child(
                        scroll_cell(
                            div()
                                .flex_1()
                                .min_h_0()
                                .child(
                                    List::new(&self.history_table)
                                        .w_full()
                                        .h_full()
                                        // ⚠️ **补差而不是给满 20**：行本身是官方 `ListItem`，
                                        // 它自己带 `px(GAP_LG)`(12) 那一档内距 ⇒ 这一格只补
                                        // `20 − 12`，墨迹才正好落在"距卡白面 36"那一档。
                                        // 给满 20 的实测结果是 48（16 + 20 + 12 叠了三层），
                                        // 比说明页宽 12——**同一屏两个页签错开 12px**，
                                        // 与第 1 条要修的正是同一件事
                                        .pl(px(TAB_PAD - GAP_LG))
                                        .pr(px(TAB_PAD_RIGHT - GAP_LG)),
                                ),
                        ),
                    ),
            )
            .into_any_element()
    }
}

fn skeleton_history() -> AnyElement {
    v_flex()
        .flex_1()
        .min_h_0()
        // 骨架条之间的呼吸量与它替的那几张卡的卡间同档（4），
        // 换过来那一帧不会跳一下
        .gap(px(GAP_TIGHT))
        .child(Skeleton::new().w_full().h(px(28.0)).rounded(px(R_MD)))
        .child(Skeleton::new().w(px(360.0)).h(px(28.0)).rounded(px(R_MD)))
        .child(Skeleton::new().w_full().h(px(28.0)).rounded(px(R_MD)))
        .into_any_element()
}

fn empty_history(t: &Theme, title: &str, detail: &str) -> AnyElement {
    v_flex()
        .flex_1()
        .min_h_0()
        // 页级留白归主区外层，这一层不再套一圈 `p`
        .child(
            Empty::new()
                // 外层已经有列边界了，再画一圈虚线框是两层框
                .border_0()
                .header(EmptyHeader::new().title(EmptyTitle::new().child(title.to_string())))
                .content(
                    EmptyContent::new().child(
                        div()
                            .text_size(px(fs_meta()))
                            .text_color(if detail.is_empty() {
                                tier4(t)
                            } else {
                                t.colors.danger
                            })
                            .child(if detail.is_empty() {
                                "第一次提交之后这里就会有内容".to_string()
                            } else {
                                detail.to_string()
                            }),
                    ),
                ),
        )
        .into_any_element()
}

// ================= 委托 =================

/// 提交列表的委托。只带一个回指 `Shell` 的 Entity，条数与内容都读 `Shell::history`
pub(crate) struct CommitDelegate {
    me: Entity<Shell>,
    /// 官方 `List` 自己管选中态，它只把下标告诉委托；
    /// 委托要在这儿留一份，才能在 `confirm` 里知道用户点的是哪一行
    selected: Option<usize>,
}

impl CommitDelegate {
    pub(crate) fn new(me: Entity<Shell>) -> Self {
        Self { me, selected: None }
    }
}

impl ListDelegate for CommitDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, cx: &App) -> usize {
        self.me.read(cx).history.commits.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        // `Context<T>` 解引用就是 `App`，委托里读 Shell 不用绕回 Entity
        let shell = self.me.read(cx);
        let h = &shell.history;
        let c = h.commits.get(ix.row)?;
        // 正在看的那个高亮。`List` 自己也会按 selected_index 上选中态，
        // 这一层是"刚点开、还没拿到 detail"时也看得出来
        let open = h.open.as_ref().map(|d| d.hash == c.hash).unwrap_or(false);
        let t = cx.theme().clone();
        let when = rel_time(c.ts);
        let hash = c.hash.clone();
        let subject = c.subject.clone();

        Some(
            ListItem::new(format!("commit-{hash}"))
                .selected(open)
                .accessibility_label(format!("{hash} {subject}"))
                // 右侧时间。**用绝对日期不用相对**：三年前的提交写"5 年前"没有信息量
                .suffix({
                    let t = t.clone();
                    let when = when.clone();
                    move |_, _| {
                        div()
                            .text_size(px(fs_meta()))
                            .text_color(tier4(&t))
                            .child(when.clone())
                            .into_any_element()
                    }
                })
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap(px(GAP))
                        .child(
                            div()
                                .w(px(64.0))
                                .flex_shrink_0()
                                .text_size(px(fs_meta()))
                                .font_family(t.mono_font_family.clone())
                                .text_color(tier3(&t))
                                .child(hash),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(fs_body()))
                                .text_color(body(&t))
                                .truncate()
                                .child(subject),
                        ),
                ),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix.map(|i| i.row);
    }

    /// 点行 / 回车 = 打开这次提交的详情 Sheet。
    ///
    /// **走 `confirm` 而不是给 `ListItem` 挂 `on_click`**：`render_list_item`
    /// 已经把选中与确认的键盘/鼠标逻辑包在行外面一层了，委托再挂一份
    /// on_click 会让「选中」与「打开」两件事各走各的（点一下高亮要等一拍）
    fn confirm(
        &mut self,
        _secondary: bool,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let Some(row) = self.selected else {
            return;
        };
        let Some(hash) = self
            .me
            .read(cx)
            .history
            .commits
            .get(row)
            .map(|c| c.hash.clone())
        else {
            return;
        };
        self.me.update(cx, |shell, cx| shell.open_commit(hash, cx));
    }
}

// ================= Sheet（提交详情 / patch） =================

impl Shell {
    /// 打开某个提交的详情 Sheet，并开始取这次提交的文件摘要
    pub(crate) fn open_commit(&mut self, hash: String, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        self.history.open = None;
        self.history.patch = None;
        // **不要在这里置 `loading`**：`load_commit_detail` 开头有一道
        // "已经在加载就直接返回"的守卫，自己先置真等于把那道守卫变成死结，
        // Sheet 会一直停在骨架条上
        self.history.error = None;
        cx.notify();
        let title = self
            .history
            .commits
            .iter()
            .find(|c| c.hash == hash)
            .map(|c| c.subject.clone())
            .unwrap_or_else(|| "提交详情".to_string());
        self.build_history_sheet(title, cx);
        self.load_commit_detail(&path, hash, cx);
    }

    /// Sheet 本体。内容挂一个 `HistorySheetView` 实体，跟着 `Shell` 重画
    fn build_history_sheet(&mut self, title: String, cx: &mut Context<Self>) {
        let this = cx.entity();
        let win = self.win.clone();
        // `cx.update_window` 的闭包只给 `&mut App`，建实体要用 `AppContext::new`
        cx.update_window(win, move |_view, window, cx| {
            let view = cx.new(|cx| HistorySheetView::new(this.clone(), cx));
            let on_close = this.clone();
            // `open_sheet` 收的是 `Fn`（不是 FnOnce），所以闭包里只能借用、
            // 不能把外面的 Entity 移走 —— 每个分支各自 clone 一份
            window.open_sheet(cx, move |sheet, _w, _cx| {
                sheet
                    .title(div().text_size(px(fs_title())).child(title.clone()))
                    .size(px(SHEET_W))
                    .on_close({
                        let on_close = on_close.clone();
                        move |_, _window, cx| {
                            on_close.update(cx, |shell, cx| shell.close_commit(cx));
                        }
                    })
                    .child(view.clone())
            });
        })
        .ok();
    }

    /// 关闭 Sheet：清掉"正在看"的状态，否则详情页那一行会一直高亮着
    pub(crate) fn close_commit(&mut self, cx: &mut Context<Self>) {
        self.history.open = None;
        self.history.patch = None;
        self.history.loading = false;
        self.history.error = None;
        cx.notify();
    }

    /// 取某个提交的文件摘要。**同一时刻只开一个**，重复点不会堆出多份后台任务
    fn load_commit_detail(&mut self, path: &str, hash: String, cx: &mut Context<Self>) {
        if self.history.loading {
            return;
        }
        self.history.loading = true;
        self.history.error = None;
        cx.notify();
        let (p, h) = (path.to_string(), hash);
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { history::commit_detail(&p, &h) })
                .await;
            this.update(cx, |this, cx| {
                this.history.loading = false;
                match r {
                    Ok(d) => this.history.open = Some(d),
                    Err(e) => this.history.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 取某个文件在这个提交里的 patch
    fn load_commit_patch(&mut self, path: &str, file: String, cx: &mut Context<Self>) {
        let Some(hash) = self.history.open.as_ref().map(|d| d.hash.clone()) else {
            return;
        };
        if self.history.loading {
            return;
        }
        self.history.loading = true;
        self.history.error = None;
        // 先摆一个空的：左侧那一行立刻高亮，右侧给骨架条，
        // 而不是让用户看着"点了没反应"
        self.history.patch = Some((
            file.clone(),
            CommitPatch {
                path: file.clone(),
                text: String::new(),
                truncated: false,
            },
        ));
        cx.notify();
        let (p, h, f) = (path.to_string(), hash, file);
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { history::commit_patch(&p, &h, &f) })
                .await;
            this.update(cx, |this, cx| {
                this.history.loading = false;
                match r {
                    Ok(patch) => this.history.patch = Some((patch.path.clone(), patch)),
                    Err(e) => this.history.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 扫当前仓库的提交历史。**只在打开「历史」页签时调**
    pub(crate) fn scan_history(&mut self, cx: &mut Context<Self>) {
        self.run_history_scan(false, cx)
    }

    /// 「重新读取」那把键：无视缓存强制再扫一遍
    fn rescan_history(&mut self, cx: &mut Context<Self>) {
        self.run_history_scan(true, cx)
    }

    fn run_history_scan(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        // 同一仓库已经扫过就直接用缓存：反复切页签不该反复起 git 进程
        if !force && self.history.path == path && !self.history.commits.is_empty() {
            return;
        }
        self.history.seq += 1;
        let seq = self.history.seq;
        self.history.task = None;
        self.history.path = path.clone();
        self.history.scanning = true;
        self.history.error = None;
        self.history.open = None;
        self.history.patch = None;
        cx.notify();

        self.history.task = Some(cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { history::commit_log(&path, LOG_LIMIT) })
                .await;
            this.update(cx, |this, cx| {
                if this.history.seq != seq {
                    return; // 切了仓库 / 已有新一轮，这批过期
                }
                this.history.scanning = false;
                this.history.commits = r;
                cx.notify();
            })
            .ok();
        }));
    }
}

// ================= Sheet 内容（Render 桥） =================

/// Sheet 的正文。**建一次、每帧重画**。
///
/// `Shell` 一变（后台任务回来、用户点文件）就 `notify` 自己，
/// 靠 `cx.observe` 挂在 `Shell` 上——Sheet 是 Root 托管的独立视图，
/// `Shell::notify` 不会自动传到它身上，这一行是这条链路唯一的一环
struct HistorySheetView {
    me: Entity<Shell>,
}

impl HistorySheetView {
    fn new(me: Entity<Shell>, cx: &mut Context<Self>) -> Self {
        cx.observe(&me, |_, _, cx| cx.notify()).detach();
        Self { me }
    }
}

impl Render for HistorySheetView {
    // 官方 `Render::render` 声明的是 `impl IntoElement`，这里收窄成 `AnyElement`。
    // `detail_body` 那一列的五条分支返回的确实是不同的具名类型，
    // 不收窄就没法把 match 的结果当成一个元素返回
    #[allow(refining_impl_trait)]
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        // `this` 是本地克隆，借 `read` 守卫看数据，不去 clone 整份 HistoryState
        // （它里面挂着一个 `Task`，克隆不了，也不该克隆）
        let this = self.me.clone();
        let shell = this.read(cx);
        let path = shell.selected.clone().unwrap_or_default();
        sheet_body(&t, &shell.history, &path, &this, window)
    }
}

fn sheet_body(
    t: &Theme,
    st: &HistoryState,
    path: &str,
    me: &Entity<Shell>,
    _window: &mut Window,
) -> AnyElement {
    let Some(detail) = st.open.as_ref() else {
        // 正在取文件摘要：给骨架，别给"没有内容"——那看着像坏了
        return v_flex()
            .w_full()
            .flex_1()
            .min_h_0()
            .gap(px(GAP_LG))
            .child(skeleton_bar(Some(px(300.0)), t))
            .child(skeleton_bar(None, t))
            .into_any_element();
    };

    v_flex()
        .w_full()
        .flex_1()
        .min_h_0()
        .gap(px(GAP_LG))
        .child(
            v_flex()
                .w_full()
                .flex_shrink_0()
                .gap(px(GAP_TIGHT))
                .child(
                    div()
                        .text_size(px(fs_body()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.colors.foreground)
                        .child(detail.subject.clone()),
                )
                .child(meta_text(
                    format!(
                        "{} · {} · 改了 {} 个文件",
                        detail.hash,
                        if detail.ts == 0 {
                            "时间未知".to_string()
                        } else {
                            rel_time(detail.ts)
                        },
                        detail.files.len()
                    ),
                    t,
                ))
                .when(detail.truncated, |row| {
                    row.child(
                        div()
                            .text_size(px(fs_meta()))
                            .text_color(orange_text(t))
                            .child("这个提交改的文件太多，列表只显示前一部分。"),
                    )
                }),
        )
        .child(
            h_flex()
                .w_full()
                .flex_1()
                .min_h_0()
                .gap(px(GAP))
                .child(file_col(t, st, detail, path, me))
                .child(
                    div()
                        .w(px(1.0))
                        .h_full()
                        .flex_shrink_0()
                        .bg(edge(t)),
                )
                .child(patch_pane(t, st)),
        )
        .into_any_element()
}

/// 左侧那列文件
fn file_col(
    t: &Theme,
    st: &HistoryState,
    detail: &history::CommitDetail,
    path: &str,
    me: &Entity<Shell>,
) -> AnyElement {
    v_flex()
        .w(px(FILE_COL_W))
        .flex_shrink_0()
        .h_full()
        .id("commit-files")
        .overflow_y_scroll()
        .gap(px(GAP_TIGHT))
        .children(detail.files.iter().map(|f| {
            let cur = st
                .patch
                .as_ref()
                .map(|(p, _)| p == &f.path)
                .unwrap_or(false);
            let me2 = me.clone();
            let p2 = path.to_string();
            let fp = f.path.clone();
            div()
                .id(SharedString::from(format!("cf-{}", f.path)))
                .w_full()
                .min_h(px(28.0))
                .px(px(GAP))
                .py(px(GAP_TIGHT))
                .rounded(px(R_MD))
                .flex_shrink_0()
                .items_center()
                .gap(px(GAP_TIGHT))
                .cursor_pointer()
                .when(cur, |row| row.bg(t.colors.muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(fs_sec()))
                        .font_family(t.mono_font_family.clone())
                        .text_color(if cur { t.colors.foreground } else { body(t) })
                        .truncate()
                        .child(f.path.clone()),
                )
                .child(file_stat(f, t))
                .on_click(move |_, _, cx| {
                    me2.update(cx, |shell, cx| shell.load_commit_patch(&p2, fp.clone(), cx));
                })
        }))
        .into_any_element()
}

/// 一个文件的增删统计。**二进制给「二进制」而不是「±0」**——
/// git 的 `--numstat` 对二进制给的是 `-`，界面显示 0 会让人以为这文件没被改
fn file_stat(f: &CommitFile, t: &Theme) -> AnyElement {
    match (f.additions, f.deletions) {
        (Some(a), Some(d)) => h_flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(GAP_TIGHT))
            .child(
                div()
                    .text_size(px(fs_meta()))
                    .font_family(t.mono_font_family.clone())
                    .text_color(t.colors.success)
                    .child(format!("+{a}")),
            )
            .child(
                div()
                    .text_size(px(fs_meta()))
                    .font_family(t.mono_font_family.clone())
                    .text_color(t.colors.danger)
                    .child(format!("-{d}")),
            )
            .into_any_element(),
        // 二进制文件：git 的 `--numstat` 给的是 `-`，界面显示 ±0
        // 会让人以为这文件根本没被改
        _ => div()
            .flex_shrink_0()
            .text_size(px(fs_meta()))
            .text_color(tier4(t))
            .child("二进制")
            .into_any_element(),
    }
}

/// 右侧那块：还没有选文件 / 正在加载 / 选好了显示 patch
fn patch_pane(t: &Theme, st: &HistoryState) -> AnyElement {
    let Some((_, patch)) = st.patch.as_ref() else {
        return v_flex()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_size(px(fs_sec()))
                    .text_color(tier4(t))
                    .child("左边点一个文件，看这次提交对它改了什么"),
            )
            .into_any_element();
    };
    if patch.text.trim().is_empty() {
        return v_flex()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_size(px(fs_sec()))
                    .text_color(tier4(t))
                    .child(if st.loading {
                        "正在读取…"
                    } else {
                        "这个文件在这次提交里没有文本差异（可能是改名或权限变化）"
                    }),
            )
            .into_any_element();
    }

    let lines: Vec<&str> = patch.text.lines().collect();
    let shown = lines.len().min(PATCH_MAX_LINES);
    v_flex()
        .flex_1()
        .min_h_0()
        .gap(px(GAP_TIGHT))
        .child(
            h_flex()
                .w_full()
                .flex_shrink_0()
                .items_center()
                .gap(px(GAP))
                .child(
                    div()
                        .text_size(px(fs_sec()))
                        .font_family(t.mono_font_family.clone())
                        .text_color(t.colors.foreground)
                        .child(patch.path.clone()),
                )
                .child(div().flex_1())
                .when(patch.truncated, |row| {
                    row.child(
                        div()
                            .text_size(px(fs_meta()))
                            .text_color(orange_text(t))
                            .child("diff 文本太长，已截断"),
                    )
                })
                .when(lines.len() > shown, |row| {
                    row.child(
                        div()
                            .text_size(px(fs_meta()))
                            .text_color(orange_text(t))
                            .child(format!("只显示前 {shown} 行")),
                    )
                }),
        )
        // diff 是**浅底等宽**，不是深色代码块。整屏灰白调里插一块深色会像贴了张截图。
        // 官方 `TextView` 在这里不合适：它按 markdown 解析，diff 里的
        // `#` / `*` / `---` 会被吃掉，还原不成原文
        .child(
            sunken_box(
                v_flex().flex_1().min_h_0().py(px(GAP_TIGHT)),
                t,
            )
            // 与日志输出流同一个 `sunken_box`（原来这里是第二份
            // `R_MD + muted + t.colors.border` 拷贝）
            .id("commit-patch")
            .overflow_y_scroll()
            .children(
                    lines[..shown]
                        .iter()
                        .enumerate()
                        .map(|(i, l)| diff_line(i + 1, l, t)),
                ),
        )
        .into_any_element()
}

/// 一行 diff。**只给前缀上色**：整行染色会让加长的一行变成一整条色块，
/// 反而看不清改了哪个字
fn diff_line(no: usize, text: &str, t: &Theme) -> AnyElement {
    let (ink, bg) = if text.starts_with('+') {
        (t.colors.success, t.colors.success.opacity(0.08))
    } else if text.starts_with('-') {
        (t.colors.danger, t.colors.danger.opacity(0.08))
    } else if text.starts_with("@@") {
        (t.colors.accent, Hsla::transparent_black())
    } else if text.starts_with("diff ") || text.starts_with("index ") {
        (tier4(t), Hsla::transparent_black())
    } else {
        (tier3(t), Hsla::transparent_black())
    };
    h_flex()
        .w_full()
        .min_h(px(PATCH_LINE_H))
        .items_start()
        .bg(bg)
        .child(
            div()
                .w(px(44.0))
                .flex_shrink_0()
                .pr(px(GAP_TIGHT))
                .text_right()
                .text_size(px(fs_meta()))
                .font_family(t.mono_font_family.clone())
                .text_color(tier4(t))
                .child(no.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .text_size(px(fs_meta()))
                .font_family(t.mono_font_family.clone())
                .text_color(ink)
                .child(text.to_string()),
        )
        .into_any_element()
}

/// 让 `store` 在本文件有一处真实引用（时间戳的相对化走 core 的 `now_secs`）
const _: fn() -> u64 = store::now_secs;
