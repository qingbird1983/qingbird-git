//! 日志区：表头 / 状态行 / 进度条 / 输出流 / 结论行。
//!
//! **2026-10-05 起它是概览页的第一段，也是那一屏占最大、最重要的一块。**
//! 走过的三步：检视抽屉第一页签 → 概览页下半 → 提到最上的独立一段。
//! 每一步都是因为右栏与中栏撞了车（同一份数据两个入口），
//! 而不是它自己变好或变坏了。
//!
//! **表头挂五颗键，是这一段存在的理由。** 复制 / 清空 / 终端 / 开发运行 /
//! 编译构建 全在这一行上：它们是「从刚才那次操作接着往下走」的五种走法。
//! 拆散到别处，每一种都要用户自己先找到它、再点一次
//!
//! **浅色输出流，不要深色代码块。** 早期版本拿墨色底当终端用，但这一屏
//! 的读者是在看 git 的结论，不是在读代码——深色块在整屏灰白里是一块黑洞，
//! 而且中文在里面掉字重。等宽字体只需要在这一小段里用。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, AnyElement, ClipboardItem, Context, Div, FontWeight, InteractiveElement, IntoElement,
    ParentElement, ScrollHandle, StatefulInteractiveElement, Styled,
};
use qingbird_core::run_cmd::Slot;
use qingbird_core::types::UpdateChanges;

use crate::os;
use crate::keys::{OpenTerminal, StopOp};
use crate::state::*;
use crate::widgets::{
    card, ghost_variant, meta_text, progress_readout, progress_text, sunken_box, vdivider,
    CardHeader,
};
use crate::tokens::*;

/// 表头上**常显**的两条命令键。**这一对就是全部的常驻命令入口**：
/// 「常用命令」段 2026-10-05 撤掉了（同一对入口摆两遍），测试 / 安装依赖 /
/// 钉住的命令与它们的编辑入口都在 ⌘K（`palette.rs` 的 `RunSlot` 与 `EditCmds`）
pub(crate) const QUICK_SLOTS: [(Slot, &str); 2] =
    [(Slot::Dev, "开发运行"), (Slot::Build, "编译构建")];

// ================= 表头 =================

impl Shell {
    /// 日志区本体。**概览页第一段**（`detail_body` 的 Overview 分支）。
    ///
    /// 两类仓库（我的 / 第三方）**走的是同一个组件、同一套状态机**：
    /// 变的只是"跑什么会往里写"，而那是 `ops.rs` 的事。
    ///
    /// **高度由外层管**：这里只给 `flex_1 + min_h_0`，`detail.rs` 负责
    /// 让它吃掉剩余高度（`LOG_MIN_H` 是它的下限）
    /// 日志卡。**`progress_here` 由调用方给**：操作进行中那根进度条平时坐在
    /// 同步卡的语言技术栈那一格上（`panels::sync_card` 走 `slot_cover` 盖上去），
    /// 只有用户把「语言技术栈」这一段关掉时，那一格不存在、进度才退回这里来画。
    /// ⇒ 判据与"这一屏显示哪几段"同处一地（`detail.rs` 那一处 `on(..)`），
    /// 不在这里自己读一遍 store——两处各读迟早漂成"两个地方同时画进度"或"都没有"
    pub(crate) fn log_panel(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
        progress_here: bool,
    ) -> Div {
        let Some(path) = self.selected.clone() else {
            return card(t)
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .child(meta_text("先选一个仓库", t));
        };
        let lg = self.logs.get(&path).unwrap_or(&LOG_EMPTY);

        // 阶段 21-1：新行追加时把视口带到底。判据在**渲染之前**算——
        // `scroll_to_bottom` 只是置一个标记（`div.rs:4386` 写的是
        // `state.scroll_to_bottom = true`），由基座在下一帧布局时消费，
        // 所以它在哪一帧调都生效，但**要在这一帧 notify 之前**立起来，
        // 否则观感上就是"慢一帧才滚"。
        let stick = self.log_follow_bottom();

        // ⚠️ **"真的什么都没有"时那句状态话整条不给**（原型 `.log-card` 里
        // 那条 `bar` 只在有输出时长）。它第三档回落去问落盘历史，问不到就写
        // 「还没有操作记录」——而这句话**空态块的第一行已经在说了**，
        // 于是同一张卡左上角一句、正中间一句，两句一模一样的话隔着 300px 对视。
        // 修法是让"没信息"这件事只由空态那一处说，不是一句都不说。
        // 2026-10-08 那句话从"独占一行"上移进表头（见 `log_header`），**这条判据一字没改**
        let past = self.op_log_line(path.as_str());
        let has_record = lg.running || lg.ts > 0 || !past.is_empty();
        let state_line = has_record.then(|| {
            log_state_line(lg.running, lg.op.map(|op| op.label()), lg.ts, past.as_str())
        });

        let panel = card(t)
            .flex_1()
            .min_h_0()
            .child(self.log_header(path.as_str(), lg, state_line.as_deref(), t, cx))
            // 进度条——**平时不在这儿**（2026-10-08 用户：它把日志卡占掉一大块，
            // 要挪去同步卡里语言技术栈那一格）。这一行只剩"语言段被关掉"那一种退路：
            // 那一格没了，进度总得有个地方说（D73：末端看不见就是缺陷）
            .when(lg.running && progress_here, |col| {
                col.child(progress_bar(t, "log-progress", lg))
            })
            .child(output_stream(
                "log-stream",
                &self.log_scroll,
                t,
                lg,
                // 空态照原型：一枚 wash 色块 + 主句 + 一句更淡的说明。
                // **主句说"没有"，说明句说"怎么才会有"**——两句是一套，
                // 拆开摆就变成上面那个对视的样子
                Some("还没有操作记录"),
                "点同步三键或命令行的按钮，输出会实时回显在这里",
            ))
            // **这次拉回来什么**（乙档默认一句，点开列前 3 条）。
            // 坐在输出流与结论行之间：结论那句「拉取成功」回答"成没成"，
            // 这一行回答"成了什么"——顺序就是提问的顺序
            .when_some(
                self.pull_block(path.as_str(), lg, t, cx),
                |col, blk| col.child(blk),
            )
            .child(self.log_verdict_row(t, lg));

        if stick {
            self.log_scroll.scroll_to_bottom();
        }
        panel
    }

    /// 该不该跟着新输出滚到底（阶段 21-1）。
    ///
    /// **判据是"视口是不是还贴着底"**，而不是"用户有没有滚过"——
    /// 见 `Shell::log_stick_to_bottom` 的注释：用户自己滚回底部后
    /// 跟随就该自动恢复，否则会留一个只有用户自己摸得出来的怪状态
    fn log_follow_bottom(&self) -> bool {
        should_follow_bottom(
            self.log_scroll.offset().y.to_f64() as f32,
            self.log_scroll.max_offset().y.to_f64() as f32,
        )
    }
}

impl Shell {
    /// 表头：左边「日志」+ 那句状态话，右边三颗键（复制 / 清空 / 终端），
    /// **正在跑时最左边再多一颗「停止」并拉一条分组竖线**。
    ///
    /// **阶段 31.4 删掉了右边那两颗「开发运行」「编译构建」**（用户：
    /// 「仓库名后面加了开发运行和编译构建两个按钮，下面日志右上角的同功能按钮
    /// 就可以取消了」）。它们与仓库头那一对是**同一功能的两个入口**——
    /// 同一动作一屏两个入口本来就是设计缺陷（v4 铁律 3），而且头卡那对
    /// 是常驻的、切页签不重画，日志这一对只在概览页签出现，**留着只会让人
    /// 以为这两个地方做的事不一样**。
    /// ⚠️ 那次分组用的 `vdivider` 一起删过：它当时分的是"看结果 │ 接着跑"，
    /// 右边那组没了、一条竖线挂在末尾就是句没头没尾的话。**现在这条线又有了，
    /// 但分的是另一组关系**（结束这次操作 │ 处理已经拿到的输出），别当是同一条
    ///
    /// `state_line` 由调用方算好送进来（`log_state_line` 三档 + "真的什么都没有
    /// 时整条不给"那条判据都在 `log_panel` 那一处），这里只管摆
    fn log_header(
        &self,
        path: &str,
        lg: &LogState,
        state_line: Option<&str>,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let text = lg.lines.join("\n");
        let has_text = !text.is_empty();
        // **那句状态话挂在标题后面**（2026-10-08 用户：「把 日志 下面那行
        // 检测更新.刚刚 的这个状态文字 上移到 日志 的后面显示，这样日志下面的
        // 详细 显示是不是可以像上又可以加大很多啊。有限空间都是被这些
        // 小状态提示词给占用了」）。它原来**独占一行**（就在表头下面，
        // 行高 `row_h(fs_sec())` ≈ 24px），那一行整行撤掉 ⇒ 输出流向上顶 24px。
        // ⚠️ 样式走 `CardHeader::note` 那一份（说明页 / 空间页 / 设置页…八处
        // 表头附注同源），**不在这里自己拼一个 div**——那又是"同一个属性两个出处"。
        // 代价照实说：附注只有 `tier3` 一档，比原来常态那句的 `tier4` 深一点，
        // 换来的是与全站表头附注同一个出处；原来 running 档那颗 `Spinner` **撤了**
        // （同帧有三处指示：仓库头那颗「停止」、侧栏行那颗、状态条那句，
        // 而「进行中…」四个字本身就在说这件事）
        let mut hdr = CardHeader::new("日志");
        if let Some(line) = state_line {
            hdr = hdr.note(line.to_string(), false);
        }
        if lg.running {
            hdr = hdr
                .right(
                    // ⛔ **不给它 `.loading()`**（D64）：loading 的键整颗失能，
                    // 而这颗键正是那个进行中操作的终止手段。"正在跑"不用它表达——
                    // 它只在 running 时出现，存在本身就是指示器
                    Button::new("log-stop")
                        .label("停止")
                        .custom(ghost_variant(cx))
                        .with_size(Size::XSmall)
                        .tooltip_with_action("停止当前操作", &StopOp, None)
                        .accessibility_label("停止当前操作")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.stop_current(window, cx)
                        }))
                        .into_any_element(),
                )
                .right(vdivider(t).into_any_element());
        }
        let hdr = hdr
            .right(
                Button::new("log-copy")
                    .label("复制")
                    .custom(ghost_variant(cx))
                    .disabled(!has_text)
                    .with_size(Size::XSmall)
                    .tooltip("复制全部输出")
                    .on_click(cx.listener(move |_, _, window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        window.push_notification("已复制全部输出", cx);
                    }))
                    .into_any_element(),
            )
            .right(
                Button::new("log-clear")
                    .label("清空")
                    .custom(ghost_variant(cx))
                    .disabled(!has_text)
                    .with_size(Size::XSmall)
                    .tooltip("清空输出（不影响正在跑的操作）")
                    .on_click(cx.listener(|this, _, _, cx| this.clear_log(cx)))
                    .into_any_element(),
            )
            .right(
                // 官方 `tooltip_with_action` 会自己去 keymap 里查 `Ctrl T`
                // 并画成 `Kbd` 角标。手写「（Ctrl T）」进文案的话，
                // 键位一改那句话就成了假提示
                Button::new("log-term")
                    .label("终端")
                    .custom(ghost_variant(cx))
                    .with_size(Size::XSmall)
                    .tooltip_with_action("在终端里 cd 到这个仓库", &OpenTerminal, None)
                    .accessibility_label("在终端中打开这个仓库")
                    .on_click({
                        let path = path.to_string();
                        cx.listener(move |_, _, window, cx| {
                            if let Err(e) = os::open_terminal(&path) {
                                window.push_notification(format!("打开终端失败：{e}"), cx);
                            }
                        })
                    })
                    .into_any_element(),
            );
        hdr.build(t)
    }
}

// ================= 状态话 =================

/// 表头那句状态话的三个档位。**抽成纯函数才单测得掉**（D48 那条规律：
/// 判据不抽出来就只能靠抓帧看，而抓帧只证明"某帧长那样"）。
///
/// ① 正在跑 → 报"进行中"（`op_label` 一定有值，`None` 只是兜底）
/// ② 本次会话跑过 → 报"做了什么 · 多久前"
/// ③ **本次会话没跑过** → 用调用方送进来的落盘历史 `past`（阶段 16 的 `op_history`）。
///    少了③，重启一次这里就写「还没有操作记录」——而昨天明明拉过。
///    **那一档不是"没有信息"，是"信息在库里没人去读"**
///
/// ⚠️ `past` 是**参数不是自查**：`Shell::op_log_line` 要 `&self`（读那张 SQLite 表），
/// 判据留在方法里就测不了——同 `diag::log_path_in` / `git::ssh_auth_hint` 那条纪律。
///
/// 空串 `past` 才轮到「还没有操作记录」，而**调用方在三档全空时根本不给这一句**
/// （`log_panel` 里那条 `has_record`）：空态块第一行说的就是同一句话，
/// 两处各说一遍是这张卡原来那个"两句隔着 300px 对视"的缺陷。
/// ⇒ 所以这一句在界面上**只可能出现在"库里也没有、但会话跑过"**那种不可能状态，
/// 留着它是兜底，不是给屏上看的
pub(crate) fn log_state_line(
    running: bool,
    op_label: Option<&str>,
    ts: u64,
    past: &str,
) -> String {
    if running {
        return match op_label {
            Some(label) => format!("{label} 进行中…"),
            None => "进行中…".to_string(),
        };
    }
    if ts == 0 {
        return if past.is_empty() {
            "还没有操作记录".to_string()
        } else {
            past.to_string()
        };
    }
    match op_label {
        Some(label) => format!("{label} · {}", rel_time(ts)),
        None => rel_time(ts),
    }
}

/// 进度读数。**委托给 `widgets::progress_readout`**，日志卡与克隆框共用一份。
///
/// 这里的 `lg.stage` 已经带着网络路径（`接收对象（经代理 127.0.0.1:7897）`），
/// `lg.detail` 是网速——两样都是 core 解析好送过来的，
/// **2026-10-05 之前它们在这一层全被丢掉**：函数只返回一根条，
/// `stage` 进了 `accessibility_label`（读屏专用，肉眼不可见）、
/// `percent` 进了 `bar.value()`。所以用户看到的是"光进度条"，
/// 而老版本同一行上还有路径标签、网速与百分比文字
///
/// 阶段还没来时给一句「准备中…」：常用命令那条路永远不发进度事件，
/// 空着的一行比一句实话更让人以为渲染坏了
///
/// ⛔ **`id` 由调用方给，但"文案怎么从 `LogState` 拼出来"只有这一处**。
/// 进度块现在有两个落点（同步卡语言段那一格 / 语言段被关掉时的日志卡），
/// 各自拼一遍文案的话，其中一条一定会漂——2026-10-08 就漂过一次：
/// 这里写 `unwrap_or("准备中…")`，同步卡那处写成 `unwrap_or_default()`，
/// 于是**同一个操作在两个落点上一个是空的**。挪落点可以，拼文案只认这一颗
pub(crate) fn progress_bar(t: &Theme, id: &'static str, lg: &LogState) -> AnyElement {
    progress_readout(
        t,
        id,
        &progress_text(lg.stage.as_deref().unwrap_or("准备中…"), &lg.detail),
        lg.percent,
    )
}

// ================= 输出流 =================

/// 输出流是否该自动滚到底部（阶段 21-1）。
///
/// **纯函数**：要 `&Self` 与滚动句柄才能调的判据留在方法里就只能靠抓帧看，
/// 而抓帧证明不了"点了有反应"。抽出来才单测得掉（D48 那条规律）。
///
/// `offset.y` 是视口顶相对容器顶的位移，**越往下越负**（`div.rs:4393` 文档）；
/// `max_offset.y` 是能滚到的最深处的同一个量。两者的差就是"离底还有多远"。
///
/// 差在 **2px 以内**算贴底：那 2px 不是宽容度，是"内容不足一屏时 `max_offset`
/// 恒为 0，而 `offset` 也恰好是 0"——差为 0 判成贴底是对的；
/// 而滚动到最底时浮点/取整可能差 1–2px，卡死在那几像素上的话用户
/// 滚到底之后跟随就再也不恢复了。
pub(crate) fn should_follow_bottom(offset_y: f32, max_offset_y: f32) -> bool {
    (offset_y - max_offset_y).abs() <= 2.0
}

/// 输出流：浅色底 + 1px 边 + 等宽字体。
///
/// **那圈里框只在有内容时画**（2026-10-05）。它的作用是把等宽输出与白卡分开；
/// 空态时它只剩一个套在白卡里的空盒子，所以空态走另一条分支，只留居中提示语。
///
/// **错误行只给前缀标色，不整段染红。** 整段红会把整块输出变成一片警示墙，
/// 真正要看的报错那几行反而被淹掉；只标 `✗` 前缀，视线自然落到行首
///
/// `pub(crate)` 且收空态文案与元素 id：批量汇总（`app_log`）也是一份 `LogState`，
/// 也要用这一段渲染。两处各画一份的话，改配色 / 间距只改一处，
/// 另一个对话框会留在旧样式上。
///
/// **id 与滚动句柄都由调用方给**（阶段 21-1）：gpui 的滚动状态按 `ElementId`
/// 全局存，两个元素共用一个 id 会共用同一个滚动句柄——在批量结果对话框里
/// 滚一下，底下概览页的日志会跟着跳
pub(crate) fn output_stream(
    id: &'static str,
    scroll: &ScrollHandle,
    t: &Theme,
    lg: &LogState,
    // `empty_title`：空态主句（"还没有操作记录"那一句）。`None` = 只画说明句——
    // 批量结果对话框那份 `LogState` 一定有内容，它不需要这一档
    empty_title: Option<&str>,
    empty_hint: &str,
) -> impl IntoElement {
    // 等宽输出流盒 = `sunken_box`（v4 §3.1：卡内盒的圆角是 `R_LG` 那一档，
    // `R_MD` 只给键帽与图标小底；边是 `line_soft`，`line` 归主描边与分隔线）。
    // 原来这里是 `R_MD + muted + t.colors.border`，而 `dialogs/clone.rs` 与
    // `history_tab.rs` 的 patch 框各有第二份、第三份同款拷贝
    let mut area = sunken_box(
        v_flex()
            .w_full()
            .flex_1()
            // ⚠️ 这里原来还写着一行 `.min_h(px(120.0))`：它被紧跟其后的
            // `min_h_0()` **整个覆盖掉**（同一个属性后写的那次生效），
            // 也就是说"至少 120 高"从来没有成立过。删掉死的那行、保留现状行为；
            // 真要下限得连 `flex_1` 一起想，那是改布局不是改样式
            .min_h_0()
            .p(px(GAP))
            .gap(px(GAP_TIGHT))
            .font_family(t.mono_font_family.clone()),
        t,
    )
    .id(id)
    // ⚠️ **必须有 `track_scroll` 才滚得动**（阶段 21-1）：`overflow_y_scroll`
    // 只给这个元素一个滚动条，滚动状态本身存在 `ScrollHandle` 里，
    // 没有句柄就没有任何外部代码能读出"现在滚到哪了"、也没法命令它滚
    .overflow_y_scroll()
    .track_scroll(scroll);

    if lg.lines.is_empty() {
        // 空态**不画里框**：那圈 1px 边 + 浅灰底在有内容时干的是"把等宽输出与
        // 白卡分开"，空的时候它只剩一个套在白卡里的空盒子，白卡上凭空多出一层。
        // ⚠️ 也**不继承等宽字体**：`area` 上挂的是 `mono_font_family`，
        // 而 Windows 默认等宽是 Consolas、不含汉字，提示语一落进去就掉字重
        // （读起来像换了一款字）
        //
        // **两档文案是一套**：主句说"没有"，说明句说"怎么才会有"。
        // 主句 `fs_sec` + tier3、说明 `fs_meta` + tier4 ⇒ **差一档字号 + 差一档颜色**，
        // 这是 v4 那条"层级靠字号档差与颜色退档叠出来"的用法（gpui 没有字距）
        let mut empty = v_flex()
            .flex_1()
            .min_h(px(120.0))
            .min_h_0()
            .items_center()
            .justify_center()
            .gap(px(GAP_LG));
        if let Some(title) = empty_title {
            empty = empty
                // 那枚 wash 色块就是原型 `.log-empty .glyph`。尺寸**不新开一档**：
                // 原型给 46×46 / r15，而全站图标色块只有 `glyph()` 一处（42 / r13），
                // 为这一处再长一档就是把统一又拆回去
                .child(crate::widgets::glyph(t, AssetIcon::Terminal, tier3(t)))
                .child(
                    v_flex()
                        .items_center()
                        .gap(px(GAP_TIGHT))
                        .child(
                            div()
                                .text_size(px(fs_sec()))
                                .text_color(tier3(t))
                                .child(title.to_string()),
                        )
                        .child(
                            div()
                                .text_size(px(fs_meta()))
                                .text_color(tier4(t))
                                .child(empty_hint.to_string()),
                        ),
                );
        } else {
            empty = empty.child(
                div()
                    .text_size(px(fs_meta()))
                    .text_color(tier4(t))
                    .child(empty_hint.to_string()),
            );
        }
        return empty.into_any_element();
    }
    for line in &lg.lines {
        // 前缀单独占一列：只给**标记**上色，文字仍用正文色。
        // 整段染红会把整块输出变成一片警示墙，真正的报错那几行反而被淹掉；
        // 只标行首，视线自然落到那里
        let (mark, color) = match line.chars().next() {
            Some('✗') => (Some('✗'), t.colors.danger),
            Some('✓') => (Some('✓'), t.colors.success),
            _ => (None, tier3(t)),
        };
        let body = match mark {
            Some(_) => {
                line[line.char_indices().nth(1).map_or(line.len(), |(i, _)| i)..].trim_start()
            }
            None => line.as_str(),
        };
        area = area.child(
            h_flex()
                .w_full()
                .gap(px(GAP_TIGHT))
                .items_start()
                .child(
                    div()
                        .w(px(10.0))
                        .flex_shrink_0()
                        .text_size(px(fs_meta()))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color)
                        .child(mark.map(|c| c.to_string()).unwrap_or_default()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(fs_meta()))
                        .text_color(if mark.is_some() { tier3(t) } else { color })
                        .child(body.to_string()),
                ),
        );
    }
    area.into_any_element()
}

// ================= 这次拉回来什么 =================

/// 拉取结果那一行的两段（原型 `.pull .h` 的分工）：
/// 主句报"带回几个提交"（`fs_body` / 中粗 / 墨），尾部报"改了多大量"
/// （`fs_sec` / tier3）。**两句合起来是一句话，拆成两段是因为阅读重量不同**——
/// 扫一眼要的是"3 个提交"，想知道细目才去看后半句。
///
/// ⚠️ **这一句只许有 `pull_summary` 一个出处**。日志卡与同步卡那次进度文案漂
/// （一处 `unwrap_or("准备中…")`、一处 `unwrap_or_default()`，同一个操作在两个
/// 落点上一个是空的）就是这么来的：拼文案的活儿一旦被复制，两条一定有一条先漂。
///
/// 四档措辞各有理由，一条都不许合并：
/// ① 提交数与文件数都是 0 ⇒ `None`，**那一行整条不出现**
///    （Already up to date 时 core 的 `pull_changes` 本来就返回 `None`；
///    这一档是防以后有人把它改成返回空结构体，那会让卡底凭空一行）
/// ② 只有文件数、没有提交（`git log` 那一趟没成而 `diff` 成了）⇒
///    主句改说「这次带回 5 个文件」，**不写「0 个提交」**——
///    用假数字撑起来的一句实话，不如换个说法
/// ③ 有提交但增删都是 0（整批换的都是二进制文件）⇒ 尾部**不带 `+A −B`**，
///    只剩「5 个文件」。写「+0 −0」是假精确：git 对二进制根本不给行数
/// ④ 正常那档 ⇒「这次带回 3 个提交」+「+120 −30，5 个文件」
pub(crate) fn pull_summary(c: &UpdateChanges) -> Option<PullCopy> {
    let commits = c.commits.len();
    let totals = c.totals;
    if commits == 0 && totals.files == 0 {
        return None;
    }
    if commits == 0 {
        return Some(PullCopy {
            head: format!("这次带回 {} 个文件", totals.files),
            tail: String::new(),
        });
    }
    let mut tail = String::new();
    if totals.has_line_counts() {
        tail.push_str(&format!("+{} −{}", totals.insertions, totals.deletions));
    }
    if totals.files > 0 {
        if !tail.is_empty() {
            tail.push('，');
        }
        tail.push_str(&format!("{} 个文件", totals.files));
    }
    Some(PullCopy {
        head: format!("这次带回 {commits} 个提交"),
        tail,
    })
}

/// `pull_summary` 的返回值：两段文案，尾部可能是空串（那一半没数字可报）
pub(crate) struct PullCopy {
    pub(crate) head: String,
    pub(crate) tail: String,
}

/// 展开态列几条。**3 就是原型 `.pull ul` 那个数**：再多这张卡就变成第二块内容，
/// 而"完整清单"本来就在「历史」页签里——这一行只负责"一眼知道动了什么"。
///
/// ⚠️ 甲档（摊开三行 ≈120px）与乙档（一句 ≈33px）**共用这同一颗函数与同一个盒子**，
/// 差别只是"列几条"这个开关（用户 2026-10-08 拍的是乙档为默认态）。
/// 分成两套写法定会长成两套东西
pub(crate) const PULL_LIST_MAX: usize = 3;

/// 截断尾巴。**只有真被截时才说"这里列前 N 条"**——
/// 3 条以内也挂这句就成了又一句谎话（①那一档的判据同源：宁可少说不假说）
pub(crate) fn pull_list_note(total: usize, shown: usize) -> Option<String> {
    (total > shown).then(|| format!("完整清单在「历史」页签 · 共 {total} 条，这里列前 {shown} 条"))
}

/// `git log --oneline` 的一行拆成（短 hash，标题）。
///
/// hash 走等宽、标题走正文字体，两列各管各的截断（标题省略号、hash 不缩）。
/// 拆不出空格或后半截是空的（畸形行）⇒ **整行当标题、hash 留空**，
/// 宁可少一列也不把内容吃掉半行
pub(crate) fn oneline_split(line: &str) -> (&str, &str) {
    match line.split_once(' ') {
        Some((hash, subject)) if !hash.is_empty() && !subject.is_empty() => (hash, subject),
        _ => ("", line),
    }
}

impl Shell {
    /// 「这次带回…」那一行，以及点开之后的清单。
    ///
    /// **位置**：输出流与结论行之间（原型 `.pull` 就坐在 `.verdict` 上面）。
    /// **容器**：收起与展开共用同一个 `sunken_box`（`card-sunken` 底 + `line-soft`
    /// 边），这样点一下**文字不会横向跳**——收起时不套盒、展开才套，
    /// 摘要行的左缘会内缩一个 padding，那是"点一下抖一下"那一族。
    ///
    /// 三件事一起说清：
    /// ① 它是**"这次"的**：跟着 `LogState` 走、不落盘，新一次操作重建时自动没有。
    ///    跨会话那句归表头（`op_history` 的三档在 `log_state_line`）
    /// ② 默认收起（乙档）：一句 + 一颗「看清单」；点开才列前 `PULL_LIST_MAX` 条
    /// ③ 开关存在**这个仓库的 `LogState::pull_open`**，不存在 `Shell` 上：
    ///    否则在 A 仓展开、切到 B 仓会看到 B 的清单也开着
    fn pull_block(
        &self,
        path: &str,
        lg: &LogState,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let c = lg.pulled.as_ref()?;
        let copy = pull_summary(c)?;
        let open = lg.pull_open;
        let shown = c.commits.len().min(PULL_LIST_MAX);

        let mut block = v_flex()
            .w_full()
            .min_w_0()
            .gap(px(GAP_DENSE))
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap(px(GAP_TIGHT))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(fs_body()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.colors.foreground)
                            .child(copy.head),
                    )
                    .when(!copy.tail.is_empty(), |row| {
                        // 尾部必须能缩能截：主区被检视抽屉挤窄时这一句最长
                        // （`+120 −30，5 个文件` 在 320 宽里会顶到键），
                        // 而 `h_flex` 里没 `min_w_0` 的东西不肯缩——
                        // 症状是「看清单」那颗键**不见了**（`CardHeader::note` 那次同款）
                        row.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(fs_sec()))
                                .text_color(tier3(t))
                                .child(copy.tail.clone()),
                        )
                    })
                    .when(copy.tail.is_empty(), |row| row.child(div().flex_1()))
                    .child(
                        Button::new("log-pull-list")
                            .label(if open { "收起" } else { "看清单" })
                            .custom(ghost_variant(cx))
                            .with_size(Size::XSmall)
                            .tooltip(if open {
                                "收起这批提交"
                            } else {
                                "看这次带回的提交清单"
                            })
                            .on_click(cx.listener({
                                let key = path.to_string();
                                move |this, _, _, cx| {
                                    if let Some(l) = this.logs.get_mut(&key) {
                                        l.pull_open = !l.pull_open;
                                    }
                                    cx.notify();
                                }
                            }))
                            .into_any_element(),
                    ),
            );

        if open {
            for line in c.commits.iter().take(PULL_LIST_MAX) {
                let (hash, subject) = oneline_split(line);
                block = block.child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .items_center()
                        .gap(px(GAP_TIGHT))
                        .when(!hash.is_empty(), |row| {
                            row.child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(fs_sec()))
                                    .font_family(t.mono_font_family.clone())
                                    .text_color(tier4(t))
                                    .child(hash.to_string()),
                            )
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(fs_sec()))
                                .text_color(tier3(t))
                                .child(subject.to_string()),
                        ),
                );
            }
            if let Some(note) = pull_list_note(c.commits.len(), shown) {
                block = block.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .truncate()
                        .text_size(px(fs_sec()))
                        .text_color(tier4(t))
                        .child(note),
                );
            }
        }

        // 竖向 `GAP_TIGHT`(4) / 横向 `GAP`(8)：实测 4 + 24 行高 + 4 ≈ 33px，
        // 就是原型乙档那个数。**横向比竖向宽**是因为这一行右边挂着那颗键，
        // 文字与键之间要有呼吸量
        Some(sunken_box(block.py(px(GAP_TIGHT)).px(px(GAP)), t))
    }
}

// ================= 结论行 =================

impl Shell {
    /// 结论行：成功 ✓ / 失败 ✗ + 走代理标记。
    /// 「走代理」单列是因为它是"连上了但绕了一圈"——混在成功里会让人以为一切正常
    fn log_verdict_row(&self, t: &Theme, lg: &LogState) -> Div {
        let mut row = v_flex().w_full().flex_shrink_0().gap(px(GAP_TIGHT));

        if let Some(e) = &lg.error {
            row = row.child(
                h_flex()
                    .w_full()
                    .gap(px(GAP_TIGHT))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(fs_sec()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.colors.danger)
                            .child("✗"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(fs_sec()))
                            .text_color(t.colors.danger)
                            .child(e.clone()),
                    ),
            );
        }

        if let Some((ok, msg)) = &lg.verdict {
            let color = if *ok {
                t.colors.success
            } else {
                t.colors.danger
            };
            row = row.child(
                h_flex()
                    .w_full()
                    .gap(px(GAP_TIGHT))
                    .child(
                        div()
                            .flex_shrink_0()
                            .child(crate::widgets::status_mark(*ok, color)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(fs_sec()))
                            .text_color(tier3(t))
                            .child(msg.clone()),
                    )
                    .when_some(lg.used_proxy.clone(), |r, proxy| {
                        // 地址直接写在胶囊上：这一列只有 320px，
                        // 塞不下"一个图标 + 一句 tooltip 之外的信息"，
                        // 而代理地址恰恰是排查网络时最需要的那串字符
                        r.child(crate::widgets::pill(
                            &format!("代理 {proxy}"),
                            orange_text(t),
                            orange_bg(t),
                        ))
                    }),
            );
        }

        row
    }
}

#[cfg(test)]
mod tests {
    use super::{
        log_state_line, oneline_split, pull_list_note, pull_summary, should_follow_bottom,
    };

    /// 表头那句状态话的三档。**2026-10-08 它从"独占一行"上移到标题后面，
    /// 判据一个字没改**——这句断言钉的就是"搬位置不许改判据"。
    ///
    /// 三档各钉一条，外加一条最容易做反的：**本会话跑过（`ts > 0`）时不许去读库里那条**
    /// （读的话，重启前刚拉成功的仓库会被库里那条更早的失败记录顶掉）
    #[test]
    fn the_three_tiers_of_the_header_state_line() {
        // ① 正在跑：报"进行中"，操作名带上
        assert_eq!(log_state_line(true, Some("拉取"), 0, ""), "拉取 进行中…");
        // ① 兜底：操作名没记上也要有话说，不能空
        assert_eq!(log_state_line(true, None, 0, ""), "进行中…");
        // ② 本会话跑过：「做了什么 · 多久前」。`now - 630` = 十分钟档，
        //    留 30 秒余量是给跑测试时的秒边界，不钉死"10"这个数
        let now = qingbird_core::store::now_secs();
        assert!(log_state_line(false, Some("推送"), now - 630, "库里那条")
            .starts_with("推送 · "));
        // ③ 本会话没跑过：**照搬落盘那一条**（少了这一档，重启一次昨天拉过的
        //    事就整个看不见——D73 那一族："信息在库里没人去读"）
        assert_eq!(log_state_line(false, None, 0, "上次拉取 成功 · 3 天前"),
                   "上次拉取 成功 · 3 天前");
        // ② 优先于 ③：`ts > 0` 时那条落盘历史**不许冒出来**
        assert!(!log_state_line(false, Some("检测"), now - 630, "上次拉取 失败")
            .contains("上次拉取"));
        // ④ 三档全空才轮到这句，而调用方在这种情形**根本不给这一句**（`has_record`），
        //    因为空态块第一行说的就是同一句话
        assert_eq!(log_state_line(false, None, 0, ""), "还没有操作记录");
    }

    /// 贴底判据守着的是 21-1 最容易做坏的那个方向：**把「用户正在回看」
    /// 误判成「在底部」**，于是每来一行输出就把视口拽走。
    ///
    /// 四个边界各有一次：一个都不守的话，"滚到底之后跟随不恢复"
    /// 这种只有用户自己摸得出来的状态就漏出去了。
    #[test]
    fn follows_only_while_the_viewport_is_at_the_bottom() {
        // 内容不足一屏：两者都是 0，必须判成"贴底"跟随，
        // 否则刚拉取、输出还没满一屏时视口就不动了（看起来像没生效）
        assert!(should_follow_bottom(0.0, 0.0));
        // 正常贴底：offset 与 max_offset 同值
        assert!(should_follow_bottom(-320.0, -320.0));
        // 用户手动往上滚了 200px：**不许跟随**
        assert!(!should_follow_bottom(-120.0, -320.0));
        // 用户滚回底部附近（差 2px，取整误差）：**要恢复跟随**，
        // 否则"回到底部之后日志再也不往下走"——那是这个功能本身失效
        assert!(should_follow_bottom(-318.0, -320.0));
        // 差 3px 就算离开底部：2px 是取整误差的容忍，不是任意宽容度
        assert!(!should_follow_bottom(-317.0, -320.0));
    }

    use qingbird_core::types::{DiffTotals, UpdateChanges};

    /// 造一份 `pulled`：提交数、文件数、增删行数各给一个，其余字段界面不读
    fn changes(commits: usize, files: usize, ins: usize, del: usize) -> UpdateChanges {
        UpdateChanges {
            commits: (0..commits)
                .map(|i| format!("abc{i:02} 标题 {i}"))
                .collect(),
            diff_stat: String::new(),
            totals: DiffTotals {
                files,
                insertions: ins,
                deletions: del,
            },
        }
    }

    /// 拉取结果那一行的四档措辞。**每档各钉一条**，因为难写对的正是那三档
    /// "没有数字可报"的：一退化成"有数就拼上去"，界面就会一本正经地报出
    /// git 根本没给过的数（二进制文件的行数、没跑成的 `git log` 的提交数）。
    ///
    /// 主句与尾部分两段断言（`PullCopy`）：那是原型 `.pull .h` 与 `.h em` 的分工，
    /// 合成一条字符串就测不出"尾部该不该出现"这一档
    #[test]
    fn the_pull_summary_line_never_reports_a_number_git_did_not_give() {
        // ④ 正常那档：主句报提交数，尾部报量
        let s = pull_summary(&changes(3, 5, 120, 30)).unwrap();
        assert_eq!(s.head, "这次带回 3 个提交");
        assert_eq!(s.tail, "+120 −30，5 个文件");

        // ③ 整批换的都是二进制：`git diff` 对二进制给的是 `-\t-`、core 存成 `None`
        //    ⇒ 尾部**只剩文件数**，一个 `+` 都不许出现
        let b = pull_summary(&changes(2, 4, 0, 0)).unwrap();
        assert_eq!(b.head, "这次带回 2 个提交");
        assert_eq!(b.tail, "4 个文件");
        assert!(!b.tail.contains('+'));

        // 只有提交、连一个文件都数不出来（`--numstat` 那趟没成）⇒ 尾部空串，
        // **调用方那一档整半句不画**（`pull_block` 里那条 `when`）
        let n = pull_summary(&changes(1, 0, 0, 0)).unwrap();
        assert_eq!(n.head, "这次带回 1 个提交");
        assert_eq!(n.tail, "");

        // ① 什么都没带回来（Already up to date）⇒ `None`，那一行整条不出现
        assert!(pull_summary(&changes(0, 0, 0, 0)).is_none());

        // ② 只数得出文件、`git log` 那一趟没成 ⇒ 主句改口说文件，**不写「0 个提交」**
        let f = pull_summary(&changes(0, 7, 10, 2)).unwrap();
        assert_eq!(f.head, "这次带回 7 个文件");
        assert_eq!(f.tail, "");
    }

    /// 截断尾巴那句只在**真被截**时出现：3 条以内也说"这里列前 3 条"是谎话，
    /// 而那句谎话就挂在用户能一眼看见的地方
    #[test]
    fn the_truncation_note_only_appears_when_there_is_something_to_truncate() {
        assert_eq!(pull_list_note(2, 2), None);
        assert_eq!(pull_list_note(3, 3), None);
        assert_eq!(
            pull_list_note(9, 3).unwrap(),
            "完整清单在「历史」页签 · 共 9 条，这里列前 3 条"
        );
    }

    /// `git log --oneline` 那行的拆分。两列各管各的截断（hash 不缩、标题省略号），
    /// 所以判据是"切在**第一个**空格"——标题里还有空格时不许切成两段
    #[test]
    fn an_oneline_commit_splits_at_the_first_space_only() {
        assert_eq!(
            oneline_split("9f2c1ab 修代理赛跑"),
            ("9f2c1ab", "修代理赛跑")
        );
        assert_eq!(
            oneline_split("4d0b7e3 空间页：目录树根层改按名称排"),
            ("4d0b7e3", "空间页：目录树根层改按名称排")
        );
        // 标题**自己带空格**是常态（中文提交标题里空格很常见）：
        // 从右边切会把 hash 切成标题的最后一截，所以判据是"只切第一个"
        assert_eq!(
            oneline_split("c81a55f README：中英 两份 都进缓存"),
            ("c81a55f", "README：中英 两份 都进缓存")
        );
        // 畸形行（没有空格 / 标题是空的）⇒ **整行当标题**，宁可少一列
        // 也不把用户的东西吃掉半行
        assert_eq!(oneline_split("没有空格的一行"), ("", "没有空格的一行"));
        assert_eq!(oneline_split("abc123"), ("", "abc123"));
    }
}
