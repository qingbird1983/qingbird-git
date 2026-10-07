//! 「空间」页签：`space::SpaceReport` 的完整呈现。
//!
//! 四段，从"有多大"一路走到"为什么会这么大"：总占用 → 最大的文件 →
//! 可清理的构建缓存 → 没被忽略的缓存目录。
//!
//! 仓库名后面只留一行摘要（`repo_head::space_badge` 的文案来自
//! `panels::space_summary_line`）。全量数据摆进那一行会把头撑到要换行，
//! 而头的职责是"我在哪个仓库、它在哪"——磁盘大小只是搭个便车。
//!
//! **清理与 .gitignore 都逐项给按钮，不给"一键全清"。** 删缓存虽然有白名单
//! （core 那边 `delete_cache_dir` 只放行固定几个目录名），但一次删掉三四个
//! 目录意味着用户要重新构建才能继续干活；而 .gitignore 是**写用户的仓库文件**，
//! 一次塞进去四条规则更没人记得住。逐项点，用户每一步都知道自己干了什么。

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Disableable as _, Sizable, Size, Theme};
use gpui_kit::{
    div, px, AnyElement, Context, Div, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, Role, SharedString, StatefulInteractiveElement, Styled,
};
use gpui_kit::prelude::FluentBuilder as _;
use qingbird_core::space;

use crate::state::*;
use crate::tokens::*;
use crate::widgets::{ghost_variant, meta_text, skeleton_bar, CardHeader};

/// 构成条里最多单列几段，其余并成「其他」
const SEG_KEEP: usize = 4;
/// 行高
const ROW_H: f32 = 30.0;

impl Shell {
    /// 空间页签正文。滚动容器在这里，页签本身不再套一层滚动
    pub(crate) fn space_body(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let st = &self.space;
        if st.scanning && st.report.is_none() {
            return v_flex()
                .flex_1()
                .min_h_0()
                .id("space-scroll")
                .overflow_y_scroll()
                .p(px(GAP_LG))
                .gap(px(GAP))
                .child(section(
                    "磁盘占用",
                    t,
                    vec![skeleton_bar(None, t), skeleton_bar(Some(px(280.0)), t)],
                ))
                .into_any_element();
        }
        if let Some(e) = &st.error {
            return v_flex()
                .flex_1()
                .min_h_0()
                .p(px(GAP_LG))
                .gap(px(GAP))
                .child(card_with(
                    CardHeader::new("磁盘占用").right(rescan_button(t, cx)),
                    t,
                    vec![error_line(e, t)],
                ))
                .into_any_element();
        }
        let Some(rep) = st.report.as_ref() else {
            return v_flex()
                .flex_1()
                .min_h_0()
                .p(px(GAP_LG))
                .gap(px(GAP))
                .child(card_with(
                    CardHeader::new("磁盘占用").right(rescan_button(t, cx)),
                    t,
                    vec![hint_line("这个仓库还没有扫描过磁盘占用。", t)],
                ))
                .into_any_element();
        };

        let cache_total: u64 = rep
            .dirs
            .iter()
            .filter(|d| d.is_cache && d.size > 0)
            .map(|d| d.size)
            .sum();
        let ignorable: Vec<&space::IgnoreSuggestion> = rep.ignorable.iter().collect();

        // 局部变量别叫 body：同文件里有个取色函数就叫 body(t)
        let mut list = v_flex()
            .flex_1()
            .min_h_0()
            .id("space-scroll")
            .overflow_y_scroll()
            .p(px(GAP_LG))
            .gap(px(GAP))
            // 1 · 总占用 + 一级目录构成。「重新扫描」挂在这一段的表头上——
            // 它扫的就是这一段，原先孤零零摆在整页最下面
            .child(card_with(space_header(st, t, cx), t, vec![
                    h_flex()
                        .w_full()
                        .items_baseline()
                        .gap(px(GAP))
                        .child(
                            div()
                                .text_size(px(fs_num()))
                                .font_family(t.mono_font_family.clone())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(t.colors.foreground)
                                .child(space::human_bytes(rep.total)),
                        )
                        .child(meta_text(
                            if cache_total > 0 {
                                // 清单里一律用 `human_bytes`：整个缓存不到 1 MB 时
                                // 「可清理 0 MB」既不真也不可读
                                format!("可清理 {}", space::human_bytes(cache_total))
                            } else {
                                "没有可清理的构建缓存".to_string()
                            },
                            t,
                        ))
                        .into_any_element(),
                    comp_bar(rep, t),
                    legend(rep, t),
                    hint_line(
                        "构成按一级子目录统计；版本库（.git）算在总量里，不在缓存里。",
                        t,
                    ),
                ],
            ));
        // 2 · 仓库内容（目录树）。放在构成条下面而不是页面最底——
        // 用户进这一页的主要诉求就是"里面有什么"，先答这个问题
        list = list.child(self.tree_section(t, cx));
        // 3 · 最大的文件
        if !rep.largest.is_empty() {
            list = list.child(section(
                &format!("最大的文件（{}）", rep.largest.len()),
                t,
                vec![v_flex()
                    .w_full()
                    .children(rep.largest.iter().take(10).map(|f| {
                        row(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(fs_sec()))
                                .font_family(t.mono_font_family.clone())
                                .text_color(body(t))
                                .truncate()
                                .child(f.name.clone()),
                            div()
                                .flex_shrink_0()
                                .text_size(px(fs_sec()))
                                .text_color(tier3(t))
                                .child(space::human_bytes(f.size)),
                        )
                    }))
                    .into_any_element()],
            ));
        }

        // 3 · 可清理的构建缓存（逐项）
        let caches: Vec<&space::DirEntry> = rep
            .dirs
            .iter()
            .filter(|d| d.is_cache && d.size > 0)
            .collect();
        if caches.is_empty() {
            list = list.child(section(
                "构建缓存",
                t,
                vec![hint_line("没有识别到常见的构建缓存目录。", t)],
            ));
        } else {
            // 「一键清理」挂这一段的表头：它清的就是这一段。**逐项那颗键留着**
            // ——用户很可能只想删 `dist` 而留着 `target`
            list = list.child(card_with(
                CardHeader::new(format!("构建缓存（{} 项）", caches.len())).right(
                    Button::new("clean-all")
                        .label("一键清理")
                        .icon(AssetIcon::Trash)
                        .danger()
                        .with_size(Size::XSmall)
                        .disabled(st.cleaning)
                        .tooltip("全部删掉。target 大的话，下一次构建要重新跑一遍")
                        .on_click(cx.listener(|this, _, window, cx| this.open_clean_dlg(window, cx)))
                        .into_any_element(),
                ),
                t,
                vec![v_flex()
                    .w_full()
                    .children(caches.iter().map(|d| {
                        let name = d.name.clone();
                        row(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap(px(GAP_TIGHT))
                                .child(
                                    div()
                                        .text_size(px(fs_sec()))
                                        .text_color(body(t))
                                        .child(name.clone()),
                                )
                                .child(div().text_size(px(fs_meta())).text_color(tier4(t)).child(
                                    format!(
                                        "清理后需要重新构建 · 约 {}",
                                        space::human_bytes(d.size)
                                    ),
                                )),
                            Button::new(SharedString::from(format!("clean-{name}")))
                                .label(if st.cleaning {
                                    "清理中…"
                                } else {
                                    "清理"
                                })
                                .secondary()
                                .with_size(Size::Small)
                                .disabled(st.cleaning)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.clean_cache(&name, cx)),
                                ),
                        )
                    }))
                    .into_any_element()],
            ));
        }
        // 4 · 建议加入 .gitignore
        list = list.child(if ignorable.is_empty() {
            section(
                ".gitignore 建议",
                t,
                vec![hint_line("缓存目录都已经被 git 忽略了。", t)],
            )
        } else {
            section(
                ".gitignore 建议",
                t,
                vec![v_flex()
                    .w_full()
                    .children(ignorable.iter().map(|i| {
                        let name = i.name.clone();
                        row(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap(px(GAP_TIGHT))
                                .child(
                                    div()
                                        .text_size(px(fs_sec()))
                                        .text_color(body(t))
                                        .child(name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(fs_meta()))
                                        .text_color(if i.tracked {
                                            orange_text(t)
                                        } else {
                                            tier4(t)
                                        })
                                        .child(if i.tracked {
                                            "目录里已有被跟踪的文件，写进 .gitignore 不会取消跟踪"
                                        } else {
                                            "构建产物会被 git 跟踪，白占仓库体积"
                                        }),
                                ),
                            Button::new(SharedString::from(format!("ignore-{name}")))
                                .label("加入 .gitignore")
                                .secondary()
                                .with_size(Size::Small)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.ignore_dir(&name, cx)),
                                ),
                        )
                    }))
                    .into_any_element()],
            )
        });

        // 「重新扫描」已经挪到「磁盘占用」那一段的表头上了
        list.into_any_element()
    }
}

// ================= 目录树 =================

/// 树里一行的行高。**与上面 `ROW_H`(30) 分开是有理由的**：那两条要放
/// 「名字 + 一句解释」两行文字，这一条只有名字
const TREE_ROW_H: f32 = 26.0;
/// 每深一层缩进多少
const TREE_INDENT: f32 = 16.0;
/// 树自身可视区的高度上限（超过就树里滚，页面不跟着长）
const TREE_MAX_H: f32 = 420.0;

impl Shell {
    /// 根层重建：直接吃 `scan_repo_space` 的 `dirs`——
    /// **那一层的递归体积刚算过，再走一遍等于把整个仓库重数一次**
    pub(crate) fn tree_reset(&mut self, cx: &mut Context<Self>) {
        self.space.tree_seq += 1;
        self.space.tree_loading.clear();
        let mut roots: Vec<space::DirEntry> = self
            .space
            .report
            .as_ref()
            .map(|r| r.dirs.clone())
            .unwrap_or_default();
        // `report.dirs` 是"谁最占地方"那个榜（按体积降序），子层才走 `list_dir`
        // 的名称序。**不排的话同一棵树里两套顺序**，根层大小子层名字，看着像坏了
        space::sort_entries(&mut roots);
        self.space.tree = roots.into_iter().map(TreeNode::new).collect();
        cx.notify();
    }

    /// 收起全部（表头上那颗键）。**只翻标记，不清已扫过的层**——
    /// 再展开时是免费的，而重新扫一遍要几秒
    pub(crate) fn tree_collapse_all(&mut self, cx: &mut Context<Self>) {
        collapse_all(&mut self.space.tree);
        cx.notify();
    }

    /// 点一行文件夹：翻展开标记；没扫过的那一层起一个后台任务
    fn tree_toggle(&mut self, rel: &str, cx: &mut Context<Self>) {
        let Some(node) = find_node_mut(&mut self.space.tree, rel) else {
            return;
        };
        // 文件与空文件夹点了没反应——**不给一个点不动的箭头**，
        // 箭头只在 `foldable()` 时才画
        if !node.foldable() {
            return;
        }
        node.expanded = !node.expanded;
        if node.expanded && node.children.is_none() {
            self.tree_load(rel, cx);
        }
        cx.notify();
    }

    /// 读一层。**体积是递归算的**（一个文件夹占多少是这页的本题），
    /// 所以大仓库点开 `target/` 要走一遍树——放后台线程，界面上只多一行转圈
    fn tree_load(&mut self, rel: &str, cx: &mut Context<Self>) {
        let Some(repo) = self.selected.clone() else {
            return;
        };
        self.space.tree_loading.insert(rel.to_string());
        // ⚠️ **先自增再取 seq**（与另外三处后台扫描同一套写法，D62）。
        // 反了的话守卫永远对不上 → 闭包静默早退 → 转圈永远不消失
        self.space.tree_seq += 1;
        let seq = self.space.tree_seq;
        let rel_task = rel.to_string();
        let rel_back = rel.to_string();
        cx.notify();
        // **不存句柄、detach 掉**：这一层是"多点一下就多扫一层"，
        // 用代数守卫挡过期结果比互相取消好——取消掉的那些层下次点还会再要一次
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { space::list_dir(&repo, &rel_task) })
                .await;
            this.update(cx, |this, cx| {
                // 转圈先收掉，**再**判过期：过期的结果也要把那一行的转圈带走，
                // 不然用户会盯着一个永远转下去的图标
                this.space.tree_loading.remove(&rel_back);
                if this.space.tree_seq != seq {
                    return;
                }
                let Ok(entries) = r else {
                    if let Some(n) = find_node_mut(&mut this.space.tree, &rel_back) {
                        // 标成"扫过且是空的"：再点不会无限重试
                        n.children = Some(Vec::new());
                    }
                    cx.notify();
                    return;
                };
                if let Some(n) = find_node_mut(&mut this.space.tree, &rel_back) {
                    n.children = Some(entries.into_iter().map(TreeNode::new).collect());
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// 按 id 在树里找节点（可变借用）
fn find_node_mut<'a>(nodes: &'a mut [TreeNode], rel: &str) -> Option<&'a mut TreeNode> {
    for n in nodes.iter_mut() {
        if n.entry.rel == rel {
            return Some(n);
        }
        if let Some(kids) = n.children.as_mut() {
            if let Some(f) = find_node_mut(kids, rel) {
                return Some(f);
            }
        }
    }
    None
}

fn collapse_all(nodes: &mut [TreeNode]) {
    for n in nodes.iter_mut() {
        n.expanded = false;
        if let Some(kids) = n.children.as_mut() {
            collapse_all(kids);
        }
    }
}

/// 展开态拍平成一串可见行。**行数由"用户展开了多少层"决定**，
/// 不是整个仓库的条目数——所以 `v_flex` 直接画就行，不必上虚拟滚动
fn flatten<'a>(nodes: &'a [TreeNode], depth: usize, out: &mut Vec<(&'a TreeNode, usize)>) {
    for n in nodes {
        out.push((n, depth));
        if n.expanded {
            if let Some(kids) = &n.children {
                flatten(kids, depth + 1, out);
            }
        }
    }
}

impl Shell {
    /// 「仓库内容」那张卡：表头（条数 + 收起全部）+ 树
    fn tree_section(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let mut rows: Vec<(&TreeNode, usize)> = Vec::new();
        flatten(&self.space.tree, 0, &mut rows);
        // 「有没有展开过」按**可见行数**判，不按根层的 `expanded` 标记判：
        // 折起一个父目录不会清掉子目录的标记，只看根层会在"深层还开着、
        // 根层全折了"的时候把「收起全部」禁掉，那正是最需要它的时候
        let any_expanded = rows.len() > self.space.tree.len();

        let body = if self.space.tree.is_empty() {
            hint_line("这个文件夹是空的。", t).into_any_element()
        } else {
            v_flex()
                .w_full()
                // **树自己也要能滚，而且必须给上限**。这一页的可展开量取决于
                // 仓库有多大——实测用户手上 Locowrk 98 GB / qingniao 28 GB，
                // 展开几层就上千行。不封顶的话下面那几段（构建缓存 / .gitignore
                // 建议）会被顶到屏幕外，而它们才是这一页唯一能动手的地方
                .max_h(px(TREE_MAX_H))
                .id("space-tree-scroll")
                .overflow_y_scroll()
                .gap(px(GAP_DENSE))
                .children(rows.iter().map(|(n, d)| self.tree_row(n, *d, t, cx)))
                .into_any_element()
        };

        // 条数取**屏幕上真正有的行数**，不是根层数——根层数只是"第一层有几个"
        card_with(
            CardHeader::new(format!("仓库内容（{} 项）", rows.len()))
                .note("只读", false)
                .right(
                    Button::new("space-collapse-all")
                        .label("收起全部")
                        .custom(ghost_variant(cx))
                        .with_size(Size::XSmall)
                        .disabled(!any_expanded)
                        .tooltip("把所有展开的目录折回去")
                        .on_click(cx.listener(|this, _, _, cx| this.tree_collapse_all(cx)))
                        .into_any_element(),
                ),
            t,
            vec![body, hint_line("点文件夹展开下一层；这一页是只读的，删东西走下面那段。", t)],
        )
    }

    /// 树的一行：缩进 + 图标 + 名字 + 体积（+ 可清理标记）
    fn tree_row(
        &self,
        node: &TreeNode,
        depth: usize,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let e = &node.entry;
        let foldable = node.foldable();
        let loading = self.space.tree_loading.contains(&e.rel);
        let icon = if !e.is_dir {
            AssetIcon::File
        } else if node.expanded {
            AssetIcon::FolderOpen
        } else {
            AssetIcon::Folder
        };
        // 图标位是固定的 14px，折叠与否只换字形——**不靠缩放表示状态**，
        // 也不给空文件夹画一个点不动的箭头
        let ink = if e.is_cache { orange(t) } else { tier4(t) };

        let mut row = h_flex()
            .w_full()
            .h(px(TREE_ROW_H))
            .flex_shrink_0()
            .px(px(GAP_TIGHT))
            .rounded(px(R_SM))
            .items_center()
            .gap(px(GAP_TIGHT))
            .id(SharedString::from(format!("space-node-{}", e.rel)))
            .child(div().w(px(depth as f32 * TREE_INDENT)).flex_shrink_0())
            .child(
                div()
                    .w(px(14.0))
                    .flex_shrink_0()
                    .text_color(ink)
                    .child(icon_sm(icon)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(fs_sec()))
                    .font_family(t.mono_font_family.clone())
                    .text_color(body(t))
                    .child(e.name.clone()),
            )
            .child(meta_text(space::human_bytes(e.size), t))
            .when(e.is_cache, |r| {
                r.child(crate::widgets::pill("可清理", orange_text(t), orange_bg(t)))
            });
        if foldable {
            // 有下级的才给箭头；`.role` 让读屏软件把它当树节点念出来
            row = row.child(
                div()
                    .w(px(14.0))
                    .flex_shrink_0()
                    .text_color(tier4(t))
                    .child(icon_sm(if node.expanded {
                        AssetIcon::ChevronDown
                    } else {
                        AssetIcon::ChevronRight
                    })),
            );
        } else {
            row = row.child(div().w(px(14.0)).flex_shrink_0());
        }
        if loading {
            row = row.child(Spinner::new().color(tier3(t)).with_size(Size::XSmall));
        }

        if !foldable {
            // 文件与空文件夹不是可点的东西：不给 `on_click`，
            // 免得整行画出一个能按的鼠标手型却什么都不发生
            return div().w_full().child(row).into_any_element();
        }
        let rel = e.rel.clone();
        // 挂 `TreeItem` 角色：读屏软件才念得出"这是个可展开的目录"。
        // 官方 `tree` 挂了 `TreeItem` + `aria_level` + `aria_expanded`（`gpui-base/
        // src/tree.rs:448-491`），自研的这一份至少把角色补上——
        // **别顺手补 `aria_expanded`**：那一族的 `.aria_*` 与焦点节点强相关，
        // 这里没有对应的 `FocusHandle`，写了是给一个不存在的状态贴标签
        row.role(Role::TreeItem)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.tree_toggle(&rel, cx)))
            .into_any_element()
    }
}

/// 一段「卡 + 卡内表头 + 内容」。**与日志 / 说明同一套**（`widgets::card` +
/// `CardHeader`）——原先这一页自己手搓了一份外壳（内边距 **16**，其余各页是
/// 8），五个页签摆在一屏里内边距有两个值，一眼就不齐
fn card_with(hdr: CardHeader, t: &Theme, body: Vec<AnyElement>) -> Div {
    let mut card = crate::widgets::card(t).flex_shrink_0().child(hdr.build(t));
    for e in body {
        card = card.child(e);
    }
    card
}

/// 只要标题、不带右侧操作的段
fn section(title: &str, t: &Theme, body: Vec<AnyElement>) -> Div {
    card_with(CardHeader::new(title), t, body)
}

/// 一行：左边说明 + 右边控件。右边没有内容时传 `div()`
fn row<L: IntoElement, R: IntoElement>(left: L, right: R) -> Div {
    h_flex()
        .w_full()
        .h(px(ROW_H))
        .flex_shrink_0()
        .items_center()
        .gap(px(GAP))
        .child(left)
        .child(div().flex_1())
        .child(right)
}

fn hint_line(text: &str, t: &Theme) -> AnyElement {
    div()
        .w_full()
        .text_size(px(fs_meta()))
        .text_color(tier4(t))
        .child(text.to_string())
        .into_any_element()
}

fn error_line(text: &str, t: &Theme) -> AnyElement {
    div()
        .w_full()
        .text_size(px(fs_sec()))
        .text_color(t.colors.danger)
        .child(text.to_string())
        .into_any_element()
}

fn rescan_button(t: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    Button::new("space-tab-rescan")
        .icon(icon_sm(AssetIcon::RefreshCw).text_color(tier4(t)))
        .custom(ghost_variant(cx))
        .with_size(Size::Small)
        .tooltip("重新扫描磁盘占用")
        .accessibility_label("重新扫描磁盘占用")
        .on_click(cx.listener(|this, _, _, cx| this.scan_space(cx)))
        .into_any_element()
}

/// 「磁盘占用」那张卡的表头：标题 + **多旧** + 重新扫描。
///
/// 阶段 16 起这一段可能是**从库里读出来的旧数**（先灌缓存、再后台重扫）。
/// 不写清楚它多旧，昨天那份数字和刚算出来的那份**长得一模一样**——
/// 用户只会当成"这就是现在的占用"，而实际上 `target/` 可能已经长了一倍。
/// **缓存不标时间就等于假装新鲜**，这句 note 就是那个"假装"的解药。
fn space_header(st: &SpaceState, t: &Theme, cx: &mut Context<Shell>) -> CardHeader {
    let mut h = CardHeader::new("磁盘占用");
    let note = scanned_note(st.scanning, st.scanned_at);
    if !note.is_empty() {
        h = h.note(format!("扫描于 {note}"), false);
    }
    h.right(rescan_button(t, cx))
}

/// 一级目录构成条。空段不画：0 字节的目录占一段宽度只会让图例看着撒谎
fn comp_bar(rep: &space::SpaceReport, t: &Theme) -> AnyElement {
    let palette = [
        t.colors.accent,
        orange(t),
        t.colors.chart_2,
        t.colors.chart_5,
        edge(t),
    ];
    let segs: Vec<(Hsla, u64, String)> = rep
        .dirs
        .iter()
        .filter(|d| d.size > 0)
        .take(SEG_KEEP)
        .enumerate()
        .map(|(i, d)| {
            (
                palette[i % palette.len()],
                d.size,
                if d.is_cache {
                    format!("{} · 构建缓存", d.name)
                } else if d.name == ".git" {
                    ".git · 版本库".to_string()
                } else {
                    d.name.clone()
                },
            )
        })
        .collect();
    let rest: u64 = rep.dirs.iter().skip(SEG_KEEP).map(|d| d.size).sum();
    let mut bar = h_flex()
        .w_full()
        .h(px(8.0))
        .rounded(px(R_SM))
        .overflow_hidden()
        .gap(px(GAP_DENSE))
        .bg(track(t));
    for (color, size, _) in &segs {
        bar = bar.child(
            div()
                .h_full()
                .rounded(px(R_SM))
                .bg(*color)
                .flex_grow(*size as f32),
        );
    }
    if rest > 0 {
        bar = bar.child(
            div()
                .h_full()
                .rounded(px(R_SM))
                .bg(track(t))
                .flex_grow(rest as f32),
        );
    }
    bar.into_any_element()
}

/// 构成条的图例。**只列前 SEG_KEEP 段 + 其他**，和条上的段一一对应
fn legend(rep: &space::SpaceReport, t: &Theme) -> AnyElement {
    let palette = [
        t.colors.accent,
        orange(t),
        t.colors.chart_2,
        t.colors.chart_5,
        edge(t),
    ];
    let mut items: Vec<(Hsla, String)> = rep
        .dirs
        .iter()
        .filter(|d| d.size > 0)
        .take(SEG_KEEP)
        .enumerate()
        .map(|(i, d)| {
            let remark = if d.is_cache {
                " · 构建缓存"
            } else if d.name == ".git" {
                " · 版本库"
            } else {
                ""
            };
            (
                palette[i % palette.len()],
                format!("{} — {}{}", d.name, space::human_bytes(d.size), remark),
            )
        })
        .collect();
    let rest: u64 = rep.dirs.iter().skip(SEG_KEEP).map(|d| d.size).sum();
    if rest > 0 {
        items.push((
            track(t),
            format!(
                "其他 {} 项 — {}",
                rep.dirs
                    .iter()
                    .skip(SEG_KEEP)
                    .filter(|d| d.size > 0)
                    .count(),
                space::human_bytes(rest)
            ),
        ));
    }
    v_flex()
        .w_full()
        .gap(px(GAP_TIGHT))
        .children(items.into_iter().map(|(color, text)| {
            h_flex()
                .w_full()
                .items_center()
                .gap(px(GAP))
                .child(
                    div()
                        .w(px(8.0))
                        .h(px(8.0))
                        .flex_shrink_0()
                        .rounded(px(R_SM))
                        .bg(color),
                )
                .child(meta_text(text, t))
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(rel: &str, is_dir: bool, child_count: u32) -> space::DirEntry {
        space::DirEntry {
            rel: rel.to_string(),
            name: rel.rsplit('/').next().unwrap_or(rel).to_string(),
            path: format!("C:/r/{rel}"),
            is_dir,
            size: 1,
            child_count,
            is_cache: false,
        }
    }

    /// a / a-b / a-b-c / a-b-c-d
    fn deep_tree() -> Vec<TreeNode> {
        let mut c = TreeNode::new(dir("a/b/c", true, 1));
        c.children = Some(vec![TreeNode::new(dir("a/b/c/d", true, 0))]);
        let mut b = TreeNode::new(dir("a/b", true, 1));
        b.children = Some(vec![c]);
        let mut a = TreeNode::new(dir("a", true, 1));
        a.children = Some(vec![b]);
        vec![a, TreeNode::new(dir("top.txt", false, 0))]
    }

    #[test]
    fn flatten_only_walks_into_expanded_nodes() {
        let mut t = deep_tree();
        let mut out = Vec::new();
        flatten(&t, 0, &mut out);
        let names: Vec<(&str, usize)> = out.iter().map(|(n, d)| (n.entry.rel.as_str(), *d)).collect();
        assert_eq!(names, vec![("a", 0), ("top.txt", 0)], "没展开就只看得到自己");

        find_node_mut(&mut t, "a").unwrap().expanded = true;
        let mut out = Vec::new();
        flatten(&t, 0, &mut out);
        let names: Vec<(&str, usize)> = out.iter().map(|(n, d)| (n.entry.rel.as_str(), *d)).collect();
        assert_eq!(
            names,
            vec![("a", 0), ("a/b", 1), ("top.txt", 0)],
            "只下钻一层：b 没展开"
        );
    }

    #[test]
    fn flatten_gives_every_level_its_own_indent() {
        let mut t = deep_tree();
        for rel in ["a", "a/b", "a/b/c"] {
            find_node_mut(&mut t, rel).unwrap().expanded = true;
        }
        let mut out = Vec::new();
        flatten(&t, 0, &mut out);
        let names: Vec<(&str, usize)> = out.iter().map(|(n, d)| (n.entry.rel.as_str(), *d)).collect();
        assert_eq!(
            names,
            vec![
                ("a", 0),
                ("a/b", 1),
                ("a/b/c", 2),
                ("a/b/c/d", 3),
                ("top.txt", 0)
            ]
        );
    }

    /// 收起父目录**不会**清掉子目录的展开标记（`tree_collapse_all` 才清），
    /// 所以 `find_node_mut` 必须还能摸到折起来的那一层，再点一下是免费复原
    #[test]
    fn find_node_mut_reaches_through_folded_parents() {
        let mut t = deep_tree();
        assert!(find_node_mut(&mut t, "a/b/c/d").is_some());
        find_node_mut(&mut t, "a/b/c").unwrap().expanded = true;
        assert!(find_node_mut(&mut t, "a/b/c/d").is_some());
        collapse_all(&mut t);
        assert!(find_node_mut(&mut t, "a/b/c/d").is_some(), "收起只翻标记，不清层");
    }

    #[test]
    fn foldable_needs_to_be_a_non_empty_dir() {
        assert!(!TreeNode::new(dir("f.rs", false, 0)).foldable(), "文件");
        assert!(!TreeNode::new(dir("empty", true, 0)).foldable(), "空目录");
        assert!(TreeNode::new(dir("src", true, 3)).foldable());
    }
}
