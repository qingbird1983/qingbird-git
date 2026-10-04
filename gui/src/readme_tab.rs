//! 「说明」页签：把仓库根目录的 README 整篇按 markdown 渲染。
//!
//! **两类仓库同款**（设计文档 3.3；2026-10-05 起版面也同款了）。
//!
//! 整篇渲染走官方 `component::text::TextView`：它自己解析 markdown、
//! 保留标题层级与列表缩进，还能选中复制。**自己拿 `div` 拼标题和段落不是
//! 选项**——README 里的表格、代码块、引用块会立刻露馅。
//!
//! ## 表头（2026-10-05）
//!
//! 顶部那排（文件名 / 中英切换 / 在文件管理器中查看）**做成卡内的一条表头**，
//! 与日志段那张表头同一套做法（`log::log_header`）：左标题右操作、中间撑开。
//! 它原来浮在卡片**外面**，于是那一屏有两个"头"，看着像漏了半截。
//!
//! ## 中英切换
//!
//! 双语仓库的两份文档在**根目录里是两个文件**（`README.md` + `README_zh.md`
//! / `README_en.md`）。core 的 `readme_alt(路径, 当前文件)` 给出"另一份是哪个"，
//! 只有两份都认出来时表头上才出现那两颗键——**只有一份就不出现**，
//! 摆一颗切不过去的键比不摆更糟。
//!
//! ## 链接能点
//!
//! README 自带的跳转链接必须真的能跳，分类只有四种，各走各的路
//! （`classify_link`）：

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::empty::{
    Empty, EmptyContent, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, AnyElement, Context, Div, IntoElement, ParentElement, SharedString,
    Styled, Window,
};
use qingbird_core::repos;

use crate::state::*;
use crate::tokens::*;
use crate::widgets::{ghost_variant, skeleton_bar, vdivider, CardHeader};

/// 排版区左右留白。README 的表格常常比正文宽，两边各留一档才不顶到边
const README_PAD: f32 = 20.0;

/// 一条 README 里的链接要点去哪
#[derive(Debug, PartialEq, Eq)]
enum ReadmeLink {
    /// `#english` —— 文档内同名标题
    Anchor(String),
    /// `./README_en.md` —— 仓库根目录的**另一份说明文档**，切过去。
    /// 双语 README 的中英链接多半是这种，它就是中英切换本身
    SwitchDoc(String),
    /// 相对路径的别的文件（`docs/guide.md`）—— 用系统默认程序打开
    OpenFile(String),
    /// `http(s)://` / `mailto:` —— 交给系统
    OpenUrl(String),
    /// 认不出来（空、纯锚点、协议不认识）—— 不动
    Ignore,
}

/// 把 href 分到四类里
///
/// **判"是不是说明文档"用 core 的 `is_readme_name`**，不在这里重写一遍
/// `starts_with("readme")`——那会把 `README_api.md` 也当成能切换的文档，
/// 而 core 明确不认它
fn classify_link(href: &str) -> ReadmeLink {
    let h = href.trim();
    if let Some(anchor) = h.strip_prefix('#') {
        return match anchor.is_empty() {
            true => ReadmeLink::Ignore,
            false => ReadmeLink::Anchor(anchor.to_string()),
        };
    }
    if h.starts_with("http://") || h.starts_with("https://") || h.starts_with("mailto:") {
        return ReadmeLink::OpenUrl(h.to_string());
    }
    if h.starts_with('#') || h.is_empty() {
        return ReadmeLink::Ignore;
    }
    // 剩下都是相对路径。先去掉锚点与查询串，再去掉 `./` 前缀
    let bare = h.split(['#', '?']).next().unwrap_or("").trim();
    let rel = bare.strip_prefix("./").unwrap_or(bare).to_string();
    if rel.is_empty() {
        return ReadmeLink::Ignore;
    }
    // ⚠️ **只有"没有目录部分"的才算切文档**。带目录的（`sub/README.md`）
    // 是子目录里另一份说明，点它不该把我们换成根目录那份——那等于点一下
    // 悄悄给了另一份文档，比没反应更难查（flowix 的 README 里就有这么一条）
    if !rel.contains('/') && repos::is_readme_name(&rel) {
        ReadmeLink::SwitchDoc(rel)
    } else {
        // 带目录的整个相对路径都留着：`Path::join` 拼上仓库根目录就能开，
        // 连 `../docs/x.md` 这种也一样（Windows 打开时会自己解 `..`）
        ReadmeLink::OpenFile(rel)
    }
}

/// GitHub 风格锚点解成"该去比的标题文字"
///
/// `## Getting Started` 的锚点是 `#getting-started`，而渲染后的标题行是
/// `Getting Started`——两边差的是**小写 + 连字符当空格**。README 里的中文锚点
/// 基本都是百分号编码的（`#%E4%B8%AD%E6%96%87`），所以先解 `%XX`
fn decode_anchor(anchor: &str) -> String {
    let raw = anchor.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            if let Ok(v) = u8::from_str_radix(&anchor[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out)
        .to_lowercase()
        .replace(['-', '_', ' '], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 在渲染后的纯文本里找标题那一行，返回它在该文本里的字节范围
///
/// `reveal_range` 的 range 落在 **`rendered_text()`** 上（基座 `state.rs:650`
/// 取的就是它），而那是**去掉 markdown 标记之后的纯文本**——`## English`
/// 在里面就是 `English` 一行。所以**不用自己解析 markdown**，按行找即可
///
/// ## 取**最后一个**匹配，不是第一个
///
/// 目录项与标题在渲染后的纯文本里**无从区分**（`* [API](#api)` 与 `## API`
/// 都是光秃秃一行 `API`），但**位置上有规律：README 的目录几乎总在被跳的
/// 那一节前面**。取第一个匹配，落点几乎总是目录里那一条——而它就在用户
/// 手指底下，"点了没反应"；取最后一个匹配，落点才是标题本身。
///
/// 这正是双语 README 顶上的 `[English](#english)` 该有的行为：GitHub 上
/// 点它跳到文末那节 `## English`，这里也跳到同一处。
///
/// 代价：若同一段标题文字在后面又单独成行出现过（正文里复述、页脚再来一份
/// 目录），会落到后出现的那处。README 里这种写法少见，认这个交换
fn anchor_range(rendered: &str, anchor: &str) -> Option<std::ops::Range<usize>> {
    let want = decode_anchor(anchor);
    if want.is_empty() {
        return None;
    }
    let mut off = 0usize;
    let mut hit = None;
    for line in rendered.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim();
        if decode_anchor(trimmed) == want {
            let start = off + (body.len() - body.trim_start().len());
            hit = Some(start..(start + trimmed.len()));
        }
        off += line.len();
    }
    hit
}

impl Shell {
    /// 「说明」页签正文
    pub(crate) fn readme_body(
        &self,
        path: &str,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 还没扫完**且真的没有正文**：骨架条。README 可能上千行，
        // 扫之前先摆着，别让页签先闪一下空态再被内容顶掉。
        //
        // ⚠️ 判据多一个 `readme_text.is_none()`（阶段 16）：后台刷新时
        // 库里那份已经在屏幕上了，这时候塌成骨架屏等于把刚显示出来的
        // 正文收回去——切换页签那一瞬间看着像"内容没了"
        if self.profile.scanning && self.profile.readme_text.is_none() {
            return v_flex()
                .flex_1()
                .min_h_0()
                .p(px(GAP_LG))
                .gap(px(GAP))
                .child(skeleton_bar(Some(px(320.0)), t))
                .child(skeleton_bar(None, t))
                .child(skeleton_bar(None, t))
                .child(skeleton_bar(Some(px(420.0)), t))
                .into_any_element();
        }
        if self
            .profile
            .readme_text
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            return self.readme_empty(path, t, cx);
        }

        // 白卡 + 表头 + 内滚。与日志段同一套外壳与表头（`widgets::card` /
        // `CardHeader`）。官方 TextView 自己就能滚（`scrollable(true)`
        // 内部是一个 `gpui::list`），所以**外层不要再套 `overflow_y_scroll`**——
        // 套了之后滚动条变成两层，而锚点跳转也就不再由基座自己管了
        let card = crate::widgets::card(t)
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .gap(px(GAP_TIGHT))
            .child(self.readme_header(path, t, cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p(px(README_PAD))
                    // 用 `TextView::new(&state)` 而不是 `TextView::markdown(..)`：
                    // 后者自己建一个藏起来的 state，而锚点跳转要在同一个 state
                    // 上调 `reveal_range`
                    .child(
                        TextView::new(&self.readme_state)
                            .w_full()
                            .h_full()
                            .selectable(true)
                            .scrollable(true)
                            .on_link_click({
                                // ⚠️ **不能写 `cx.listener(..)`**：
                                // 官方这个钩子的签名是裸
                                // `Fn(&SharedString, &ClickEvent, &mut Window, &mut App)`，
                                // 不是带 `this` 的事件（`cx.listener` 那套要 5 个参数）。
                                // 所以自己抓一份 `Entity<Shell>` 进去
                                let shell = cx.entity();
                                let path = path.to_string();
                                move |href, _ev, window, cx| {
                                    shell.update(cx, |this, cx| {
                                        this.follow_readme_link(&path, href, window, cx)
                                    });
                                }
                            }),
                    ),
            );

        v_flex()
            .flex_1()
            .min_h_0()
            .p(px(GAP_LG))
            .child(card)
            .into_any_element()
    }

    /// 表头：左「README.md」+ 文档图标，右边「中文 / English」与「在文件管理器中查看」
    ///
    /// **中英两颗键只在真的有两份时才出现**（`profile.readme_alt` 为 `Some`）。
    /// 摆一颗切不过去的键比不摆更糟——用户会以为功能坏了。
    /// 当前那一份给一层浅底（`.when(..)`，D60 验过的写法）而不是去改 ghost
    /// 的选中态，那会踩到 Q12
    fn readme_header(&self, path: &str, t: &Theme, cx: &mut Context<Self>) -> Div {
        let file = self
            .profile
            .readme_file
            .clone()
            .unwrap_or_else(|| "README".to_string());
        let alt = self.profile.readme_alt.clone();
        // 当前这一份是哪种语言。`readme_alt` 存在时它的 `zh` 说的是"**另一份**"，
        // 取反就是当前这份；不存在时两颗键根本不画，这里是 None
        let cur_zh = alt.as_ref().map(|a| !a.zh);

        let mut hdr = CardHeader::new(file)
            .leading(div().text_color(tier4(t)).child(icon_sm(AssetIcon::FileText)).into_any_element());
        // 切文档失败时把话摆在这儿。**用 danger 色**：这句话是"你要的东西
        // 没拿到"，不是一句说明
        if let Some(msg) = self.profile.readme_error.clone() {
            hdr = hdr.note(msg, true);
        } else {
            // 阶段 16：这段正文可能是**从库里读出来的旧文**（先灌缓存、
            // 再后台重读一遍文件）。不标时间，昨天那份文档和刚读出来的
            // 那份**长得一模一样**——用户按着旧文档写代码，绕一大圈。
            // 阶段 18：`readme_note` 把「正在读某一份文档」与「整份档案在刷新」
            // 分成两个标志——中英切换走内存同步切换，一个都不该动，
            // 而这道闪烁恰恰就是这一句在闪
            let note = readme_note(
                self.profile.readme_loading,
                self.profile.scanning,
                self.profile.scanned_at,
            );
            if !note.is_empty() {
                hdr = hdr.note(note, false);
            }
        }
        // 中英两颗键只在真的有两份时才出现。摆一颗切不过去的键比不摆更糟——
        // 用户会以为功能坏了
        if alt.is_some() {
            hdr = hdr
                .right(
                    h_flex()
                        .flex_shrink_0()
                        .gap(px(GAP_DENSE))
                        .child(self.lang_key("readme-lang-zh", "中文", true, cur_zh, t, cx))
                        .child(self.lang_key("readme-lang-en", "English", false, cur_zh, t, cx))
                        .into_any_element(),
                )
                .right(vdivider(t).into_any_element());
        }
        hdr.right(
            Button::new("readme-reveal")
                .label("在文件管理器中查看")
                .custom(ghost_variant(cx))
                .with_size(Size::XSmall)
                .tooltip("用文件管理器打开仓库目录")
                .on_click({
                    let path = path.to_string();
                    cx.listener(move |_, _, window, cx| {
                        if let Err(e) = crate::os::open_folder(&path) {
                            window.push_notification(e, cx);
                        }
                    })
                })
                .into_any_element(),
        )
        .build(t)
    }

    /// 「中文」或「English」一颗。当前那一份给浅底，点另一份就换文档
    fn lang_key(
        &self,
        id: &'static str,
        label: &'static str,
        zh: bool,
        cur_zh: Option<bool>,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_cur = cur_zh == Some(zh);
        Button::new(id)
            .label(label)
            .custom(ghost_variant(cx))
            .with_size(Size::XSmall)
            .when(is_cur, |b| b.bg(selected_mask(t)))
            .tooltip(if is_cur {
                "正在显示这一份"
            } else {
                "切到这一份"
            })
            .on_click({
                let path = self.selected.clone();
                cx.listener(move |this, _, _, cx| {
                    let Some(p) = path.clone() else { return };
                    this.switch_readme_doc(&p, cx)
                })
            })
            .into_any_element()
    }

    /// 换到"另一语言"那一份（表头上那两颗键）
    ///
    /// ⚠️ **阶段 18：目标是"点了就换完"，中间不许有任何一帧在闪**。
    /// 目标那份正文早在 `readme_docs` 里（`scan_profile` 一次读齐、
    /// 并落进 SQLite），所以这里是**纯内存同步切换**：不派生后台任务、
    /// 不动 `scanning`、不动 `readme_loading`，表头那一句时间纹丝不动。
    ///
    /// 以前这条路径走 `load_readme_doc`，第一行就 `scanning = true` →
    /// 表头先闪一下「读取中…」，再等后台读完才换。实机抓帧量到那一下：
    /// 点完的第一帧只变了 **280 像素，包围盒 (585,209)-(631,221)**，
    /// 正好就是那一句——文档本身是同一帧换完的，闪的只有它
    fn switch_readme_doc(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(alt) = self.profile.readme_alt.clone() else {
            return;
        };
        // 手上真有这一份 → 同步换（绝大多数情况：表是同一次扫出来的）
        if let Some(text) = self.profile.readme_docs.get(&alt.file).cloned() {
            self.show_readme_doc(alt.file, text, cx);
            return;
        }
        // 手上没有（老库、或者用户中途把那份删了）→ 老老实实回后台读。
        // 宁可慢一拍，也不能拿一份错的正文上屏
        self.load_readme_doc(path, alt.file, cx);
    }

    /// 把一份**已经在手上**的正文换上屏。同步、纯内存、一帧内完成。
    ///
    /// 「另一份是哪一份」问的是 core 的 `alt_of` + 缓存里那张候选表，
    /// **与扫描时用的是同一个函数**——界面不自己再写一遍挑哪一份的规则
    fn show_readme_doc(&mut self, file: String, text: SharedString, cx: &mut Context<Self>) {
        let alt = repos::alt_of(&self.profile.readme_index, &file);
        self.readme_state
            .update(cx, |st, cx| st.set_text(&text, cx));
        self.profile.readme_text = Some(text);
        self.profile.readme_file = Some(file);
        self.profile.readme_alt = alt;
        self.profile.readme_error = None;
        cx.notify();
    }

    /// 读一份指定的说明文档并上屏。**只在手上没有那一份时才会走到这里**——
    /// README 正文里点相对链接切到第三份文档，或者老库里没存过那一份。
    ///
    /// **读文件走后台线程**——一份 README 十万字，堵在 UI 线程上那一帧会卡住。
    ///
    /// ⚠️ **不接 `&mut Window`**：`cx.spawn` 的闭包必须 `'static`，抓一个
    /// `&mut Window` 进去是编不过的（E0521）。所以读失败**不发气泡**，
    /// 而是把话写进 `profile.readme_error`、由表头显示——「点了没反应」
    /// 比报错更让人以为功能坏了
    fn load_readme_doc(&mut self, path: &str, name: String, cx: &mut Context<Self>) {
        let repo = path.to_string();
        let want = name.clone();
        // ⚠️ **借的是 `readme_loading`，不是 `scanning`**（阶段 18）。
        // `scanning` 是**整份档案扫描**的标志，仓库头的活跃度芯片与语言构成条
        // 都读它——借去用的话，点一次中英切换会让那两个毫不相干的地方一起说
        // 「统计中…」，读完再一起说不是：**一个标志两件事**
        self.profile.readme_loading = true;
        self.profile.readme_error = None;
        cx.notify();
        // ⚠️ **先自增再取 seq**，与 `scan_space` / `scan_profile` / `scan_overview_activity`
        // 同一套写法。写成「先取 seq 再自增」的话，守卫拿的是加之前的值，
        // 回来一比永远对不上 → 闭包静默早退 → 标志永远落不回 false，
        // 画面就永久停在骨架屏（2026-10-05 实机点出来的：点「中文」没反应，
        // 表头也跟着空掉——那条早退把表头一起吞了，因为它在同一个 early return 之后）
        self.profile_seq += 1;
        let seq = self.profile_seq;
        self.readme_task = Some(cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move {
                    let text = repos::read_readme_named(&repo, &want);
                    // 切过去之后"另一份"要重算：它现在指的是**切之前那份**
                    let alt = repos::readme_alt(&repo, &want);
                    (text, alt)
                })
                .await;
            this.update(cx, |this, cx| {
                // ⚠️ **过期结果也要先把标志落回去**，与 `scan_space` 的
                // `tree_loading` 同一个道理（AGENTS.md D 那条）：`profile_seq`
                // 也会被 `scan_profile` 顶掉，而那次并不管 `readme_loading`——
                // 早退之前不清，表头就会**永远**停在「读取中…」。
                // 宁可新来那一轮在途时少说一句，也不留一个收不回来的标志
                this.profile.readme_loading = false;
                if this.profile_seq != seq {
                    return;
                }
                let (text, alt) = r;
                match text {
                    Some(text) => {
                        // 顺手把它补进内存那份：这一份读过了，下次切回来就是同步的
                        this.profile
                            .readme_docs
                            .insert(name.clone(), SharedString::from(text.clone()));
                        this.readme_state
                            .update(cx, |st, cx| st.set_text(&text, cx));
                        this.profile.readme_text = Some(SharedString::from(text));
                        this.profile.readme_file = Some(name);
                        this.profile.readme_alt = alt;
                        this.profile.readme_error = None;
                    }
                    None => {
                        this.profile.readme_error =
                            Some(format!("读不到仓库根目录下的 {name}"));
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// README 里点了一条链接
    fn follow_readme_link(
        &mut self,
        path: &str,
        href: &SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match classify_link(href.as_ref()) {
            ReadmeLink::Anchor(a) => {
                // `reveal_range` 的 range 落在 `rendered_text()` 上。
                // **只在当前这份文档里跳**：不切文档 + 锚点的组合不做——
                // 切文档要走 `set_text`，而新文本要等后台解析落地才进
                // `rendered_text`，那一刻发出去的 range 是对着旧文本的。
                //
                // 基座那边是**最小滚动**（刚够把那一行带进视野），所以标题
                // 落在一侧边缘而不是顶端——不是顶对齐，但那一节确实进屏了。
                // 想顶对齐就得自己去驱 list 的 `ScrollHandle`，而 `TextView`
                // 不把它露出来，那属于拿内部实现当接口用，不划算
                let rendered = self
                    .readme_state
                    .read_with(cx, |s, _| s.rendered_text().as_str().to_string());
                let Some(range) = anchor_range(&rendered, &a) else {
                    window.push_notification(format!("文档里没有「{a}」这一节"), cx);
                    return;
                };
                self.readme_state
                    .update(cx, |s, cx| _ = s.reveal_range(range, cx));
            }
            ReadmeLink::SwitchDoc(name) => self.load_readme_doc(path, name, cx),
            ReadmeLink::OpenFile(name) => {
                // 拼仓库根目录就能开。`../docs/x.md` 这类也一样——`Path::join`
                // 保留 `..`，Windows 打开时自己会解
                let full = std::path::Path::new(path).join(&name);
                if let Err(e) = crate::os::open_path(full.to_str().unwrap_or_default()) {
                    window.push_notification(e, cx);
                }
            }
            ReadmeLink::OpenUrl(u) => {
                if let Err(e) = crate::os::open_url(&u) {
                    window.push_notification(e, cx);
                }
            }
            ReadmeLink::Ignore => {}
        }
    }

    /// 没有 README 时的**可操作空态**：说清没有，再给一条能往下走的路。
    ///
    /// 官方 `Empty` 给了标题与说明两档文案；下面挂一个真按钮，
    /// 因为"这个文件夹是空的"这句话本身帮不上任何忙
    fn readme_empty(&self, path: &str, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .flex_1()
            .min_h_0()
            .p(px(GAP_LG))
            .child(
                Empty::new()
                    // 外层已经有列边界了，再画一圈虚线框是两层框
                    .border_0()
                    .header(
                        EmptyHeader::new()
                            // 官方 `EmptyMedia` 的 Icon 档自带 32px 圆角底 +
                            // 1rem 图标位，图标就用 16px 那一档（`icon_md`），
                            // 正好等于它给的基准字号
                            .media(
                                EmptyMedia::new()
                                    .with_variant(EmptyMediaVariant::Icon)
                                    .child(icon_md(AssetIcon::BookOpen)),
                            )
                            .title(EmptyTitle::new().child("这个仓库没有说明文档")),
                    )
                    .content(
                        EmptyContent::new()
                            .child(
                                div().text_size(px(FS_META)).text_color(tier4(t)).child(
                                    "仓库根目录里没找到 README.md / README.txt / README.rst。",
                                ),
                            )
                            .child(
                                Button::new("readme-open-folder")
                                    .label("在文件管理器中查看")
                                    .secondary()
                                    .with_size(Size::Small)
                                    .on_click({
                                        let path = path.to_string();
                                        cx.listener(move |_, _, window, cx| {
                                            if let Err(e) = crate::os::open_folder(&path) {
                                                window.push_notification(e, cx);
                                            }
                                        })
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 四类链接各走各的路，判错一类就是"点了没反应"或"弹了个没打开的窗口"
    #[test]
    fn links_are_classified_into_four_destinations() {
        assert_eq!(
            classify_link("#english"),
            ReadmeLink::Anchor("english".into())
        );
        assert_eq!(
            classify_link("#%E4%B8%AD%E6%96%87"),
            ReadmeLink::Anchor("%E4%B8%AD%E6%96%87".into())
        );
        // 双语 README 的中英链接：切文档，不是开文件
        assert_eq!(
            classify_link("./README_en.md"),
            ReadmeLink::SwitchDoc("README_en.md".into())
        );
        assert_eq!(
            classify_link("README_zh-CN.md#top"),
            ReadmeLink::SwitchDoc("README_zh-CN.md".into())
        );
        // 其它相对路径交给系统，**整个路径都留着**（`join` 拼仓库根目录就够）
        assert_eq!(
            classify_link("docs/guide.md"),
            ReadmeLink::OpenFile("docs/guide.md".into())
        );
        assert_eq!(
            classify_link("./docs/guide.md#install"),
            ReadmeLink::OpenFile("docs/guide.md".into())
        );
        // ⚠️ **子目录里的 README 不是"切文档"**：它压根不在仓库根目录，
        // 切成根目录那份等于点一下悄悄给了另一份文档（flowix 的 README
        // 里就有 `[dsh-flowix-memory](dsh-flowix-memory/README.md)` 这么一条）
        for h in [
            "dsh-flowix-memory/README.md",
            "sub/README_en.md",
            "../docs/x.md",
        ] {
            assert_eq!(
                classify_link(h),
                ReadmeLink::OpenFile(h.trim_start_matches("./").into()),
                "{h:?} 该交给系统去开"
            );
        }
        assert_eq!(
            classify_link("https://github.com/a/b"),
            ReadmeLink::OpenUrl("https://github.com/a/b".into())
        );
        // 认不出来的与空的不动
        for h in ["", "#", "   "] {
            assert_eq!(classify_link(h), ReadmeLink::Ignore, "{h:?} 该被忽略");
        }
    }

    /// 锚点要在**渲染后的纯文本**里按行找：`## English` 渲染出来是 `English`
    /// 一行，范围要正好罩住那一行（首尾空白不算进去）
    ///
    /// ⚠️ **范围是字节偏移，不是字符下标**——`reveal_range` 要的就是字节。
    /// 而中文一个字 3 字节，所以「中文\n」占 7 不是 3：下面每个期望值都要
    /// 按字节算，写成字符数会全错（第一版就是这么写错的）
    #[test]
    fn anchor_range_finds_the_heading_line() {
        let rendered = "中文\nEnglish\nsee below\n";
        // "中文" 6 字节 + "\n" 1 = 第 0 行结束于 7
        assert_eq!(anchor_range(rendered, "english"), Some(7..14));
        // 大小写、连字符、空格：GitHub 的 slug 规则
        assert_eq!(
            anchor_range(rendered, "Getting-Started"),
            None,
            "这一行不在文档里，不该瞎猜"
        );
        let rendered2 = "目录\nGetting Started\n正文\n";
        assert_eq!(anchor_range(rendered2, "getting-started"), Some(7..22));
        // 百分号编码的中文锚点
        assert_eq!(anchor_range(rendered2, "%E4%B8%AD%E6%96%87"), None);
        let rendered3 = "目录\n中文\n";
        assert_eq!(anchor_range(rendered3, "%E4%B8%AD%E6%96%87"), Some(7..13));
    }

    /// 目录项与标题渲染出来是同一样东西，**位置**才是唯一的线索：
    /// README 的目录总在被跳的那一节前面，所以取**最后一个**匹配。
    /// 取第一个的话落点就是用户手指底下那行目录项——"点了没反应"
    #[test]
    fn anchor_range_prefers_the_heading_over_the_contents_entry() {
        // "Contents\n" 9 / "API\n" 4 / "正文\n" 7（正文 6 字节）/ "更多\n" 7 / "API\n" 4
        let toc = "Contents\nAPI\n正文\n更多\nAPI\n";
        assert_eq!(anchor_range(toc, "api"), Some(27..30), "该跳到后面那个标题");
        // 文档里根本没有这一节就别瞎猜
        assert_eq!(anchor_range(toc, "extensions"), None);
        // 只有一处匹配时（没写目录）行为不变
        let plain = "Feature highlights\ncompliant\n";
        assert_eq!(anchor_range(plain, "feature-highlights"), Some(0..18));
    }
}
