//! 仓库头：常驻不随页签切换。两类仓库共用同一个头。
//!
//! **为什么两类仓库必须共用一个头**：头承载的是"我在哪个仓库、它在哪、
//! 我能跳到哪去"这几件事，这几件事对"我的仓库"和"第三方仓库"没有区别。
//! 分成两个头，就会有两处各自漂移的地址格式、两份取色——而用户在两个页面
//! 之间来回切时，那种"看着一样、点下去不一样"的差异是最伤信任的。
//!
//! ## 地址区（2026-10-08 收成一行两段）
//!
//! 原来是「路径 + 最后提交」一行、一个只读 `Input` 胶囊框、页签上方一根分隔线；
//! 中间一版拆成**「本地地址 / 云端地址」两行**；现在是**一行两段**
//! （`本地 路径 · 云端 地址`，原型 `.l2` 那个形态，实测整行 18 高）。
//! 收成一行不是为了照抄原型，是**为日志让高**：两行加行间距白吃约 24px，
//! 而头卡下面每一段都在跟它抢（用户第⑤条自己把动机说了：
//! 「为什么要做到一行，是为了压缩首卡片的高度。把最大化空间留给日志页面」）。
//! 地址本体仍可点（本地 → 文件管理器，云端 → 浏览器），
//! 悬停才现一颗复制键，点完换成对号两秒——**按段悬停**，
//! 停在哪一段只露那一段的键。
//! 胶囊框与那根多余的分隔线一并删掉——它们与地址叠在一起，仓库头横线太多。
//!
//! 四个跳转动作与侧栏右键菜单共用 `os::` 那几个函数，动词与结果三处一致。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Icon, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, AnyElement, ClipboardItem, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement as _, Styled,
};
use qingbird_core::net;
use qingbird_core::run_cmd::Slot;
use qingbird_core::space;
use qingbird_core::types::RepoStatus;

use crate::os;
use crate::log::QUICK_SLOTS;
use crate::panels::sync_alert;
use crate::state::*;
use crate::tokens::*;
use crate::widgets::ghost_variant;
use crate::widgets::{pill, warn_dot};

/// 复制成功后那颗对号停留多久（秒）。
///
/// **两秒是"来得及看清"与"不会一直挂着"之间取的**：一秒太快，眼睛刚移过去
/// 它就没了；五秒太久，人已经走神，回来只会以为它卡在那儿不动。
const COPY_FEEDBACK_SECS: u64 = 2;

impl Shell {
    /// 仓库头。**常驻在页签之上**，切页签不重画它——
    /// 用户在"说明"和"历史"之间来回切时，头要是跟着跳，就得每次重新找自己在哪
    pub(crate) fn repo_head(
        &self,
        path: &str,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view = self.view(path);
        let status = view.map(|v| v.status.clone()).unwrap_or_default();
        let third = self.entry(path).map(|e| e.is_third_party).unwrap_or(false);
        // 复制按钮刚点过的那一条：短暂换对号（用户要求：复制反馈不再用气泡）
        let copied = self.copied;
        // 名字后面那颗感叹号的告警句。**与同步卡那句状态话曾经出自同一个纯函数**
        // （`panels::sync_alert`，原名 `sync_readout`）——那一行 2026-10-08 被用户
        // 整行取消之后只剩这一处读者。⚠️ **要往同步卡加回状态话，先回去和它合成一个函数**
        // （两处各 `if` 一遍迟早漂成"卡里说已同步、名字后面挂着未配置上游的感叹号"）
        let alert = sync_alert(&status, self.status_read(path));

        v_flex()
            .w_full()
            .flex_shrink_0()
            .gap(px(GAP_TIGHT))
            // 名称 + 归属徽标 + 分支 + 磁盘摘要 + 右侧那颗「展开检视」
            .child(
                h_flex()
                    .w_full()
                    .gap(px(GAP))
                    .items_center()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(fs_head()))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.colors.foreground)
                            .child(self.title(path)),
                    )
                    // ⚠️ **告警不摆整宽横幅，改成名字后面这颗感叹号 + 悬停气泡**
                    // （2026-10-06 用户：「把这个警告不要，如有警告在上面仓库名后面
                    // 出现个感叹号鼠标触发气泡提示就行了」）。位置紧贴名字、
                    // 在归属徽标**之前**——它说的是"这个名字背后有点问题"，
                    // 插在归属后面会读成"归属有问题"
                    .when_some(alert.clone(), |row, text| {
                        row.child(warn_dot("repo-warn", text, t, cx))
                    })
                    // 归属徽标常驻：这是"这个仓库我该不该改"的第一个判断依据，
                    // 藏在别处就得多点一次才知道
                    .child(pill(
                        if third { "第三方" } else { "我的" },
                        if third { orange_text(t) } else { tier3(t) },
                        if third { orange_bg(t) } else { track(t) },
                    ))
                    // ⚠️ **停止键挂在仓库头，不挂在日志段**（2026-10-05 用户反馈：
                    // 「构建卡死没有终止按钮，以前是有的」）。
                    //
                    // **原先它挂在日志段的状态行上**（`log.rs::log_status_row`），
                    // 而日志段同时受两件事约束：① 只在**概览页签**里；
                    // ② 受设置里「显示哪些模块 → 日志」那个开关控制。
                    // 于是用户切到「说明 / 历史 / 空间」——或者干脆把日志段关掉——
                    // 那颗键就**整个消失**，而构建还在跑。
                    // **一个"救命键"挂在可关掉的模块里，本身就是设计错。**
                    //
                    // 仓库头是**常驻**的（切页签不重画），才是"这个仓库正在跑东西"
                    // 该出现的地方。日志段里那颗保留——两处同一个入口，
                    // 用户在哪一屏都能停。
                    .when(self.log().running, |row| {
                        row.child(self.stop_button(cx))
                    })
                    .when(!status.branch.is_empty(), |row| {
                        row.child(self.branch_badge(&status.branch, t))
                    })
                    // ⚠️ **第三方那枚「近 30 天」活跃度徽标已撤**（2026-10-08 第⑦条）。
                    // 它身上那两样东西各有更好的落点：柱子与那句结论一起搬进
                    // **头卡页签行的右边**（原型 `.tabs-row` 的 `.sp` 之后就是那句），
                    // 而页签行是两类仓库共用的一块——留在名字行就只有第三方看得见，
                    // 同一屏两处说同一件活跃度，还各说一种措辞（那是 D73/D78 那一族）
                    // ⚠️ **磁盘摘要不再以一句话挂在名字后面**（阶段 31.4 补五）。
                    // 原来这里是 `space_badge(..)`——一颗写着「占用 1.2 MB · 没有可清理的
                    // 构建缓存」的幽灵键。原型把这类读数收进下面那四格 `metrics`，
                    // 并在注释里明写「它们此前分散在磁盘卡、侧栏徽章、同步卡三处」。
                    // **接四格必须同时撤这一处**：留着就是同一屏两个地方说同一件事，
                    // 而两处的刷新时机不同，早晚给出两个数（D73 那一族的反面）
                    .child(div().flex_1())
                    .child(self.head_metrics(&status, view.map(|v| v.changed).unwrap_or(0), t))
                    // 常用命令组（2026-10-05 用户反馈：「常用命令没有入口了」）
                    .child(self.quick_cmds(t, cx))
                    // 右侧只留「展开检视」——原来那四颗（打开文件夹 / 终端 /
                    // 云端 / 复制）里三颗已经进了下面的地址区（用户要求），
                    // 剩下的「终端」并进了日志工具行
                    .when(!self.inspector_open, |row| {
                        row.child(self.inspector_reopen(t, cx))
                    }),
            )
            // 地址区：**一行**（本地 · 云端），压缩头卡高度把空间让给日志
            .child(self.addr_line(path, &status, copied, t, cx))
    }

    /// 头卡右侧那**四格读数**（原型 `.metrics`：磁盘占用 / 可清理 / 待推送 / 未提交）。
    ///
    /// **它们是"收拢"进来的，不是新加的**——这四个数此前分散在三处：磁盘那句挂在
    /// 名字后面（`space_badge`，本轮撤掉）、可清理在空间页、待推送与未提交在侧栏徽章
    /// 与同步卡。原型那句注释说的就是这件事。⇒ **接它必须同时撤旧位**，
    /// 否则同一屏两处说同一件事，而两处刷新时机不同、迟早给出两个数。
    ///
    /// ⚠️ **「没扫过」与「扫过、确实是 0」是两件事**（阶段 16 那条老教训的第三种触发）：
    /// 磁盘两格吃的是后台空间扫描的结果，没扫过时 `report` 是 `None` ⇒ 给 **`—`**；
    /// 扫过而缓存为 0 ⇒ 给 **`0`**。把"没数"写成"0"就是谎报干净。
    /// 待推送与未提交吃的是 `read_status`，选中这个仓库就会扫，所以 0 是真的 0。
    fn head_metrics(&self, status: &RepoStatus, changed: u32, t: &Theme) -> Div {
        let rep = self.space.report.as_ref();
        // 算式在 core 的 `SpaceReport::cache_bytes`，**不在这里 filter 一遍**
        let cache: u64 = rep.map(|r| r.cache_bytes()).unwrap_or(0);
        // `None` = 还没扫过。单位一律跟着数值走（原型 `118.0` + 小号 `GB`）
        let disk = rep.map(|r| {
            let (n, u) = space::human_parts(r.total);
            (n, Some(u.to_string()))
        });
        let clean = rep.map(|_| {
            if cache > 0 {
                let (n, u) = space::human_parts(cache);
                (n, Some(u.to_string()))
            } else {
                ("0".to_string(), None)
            }
        });

        h_flex()
            .flex_shrink_0()
            .items_stretch()
            // ⚠️ **第一格 `sep=false`**：原型 `.m + .m{border-left}` 说的是
            // "格与格之间"才有线，第一格左边那条是悬空的（探针实测四格三条线）
            .child(crate::widgets::metric_cell(
                disk.as_ref().map(|d| d.0.clone()).unwrap_or_else(|| "—".into()),
                disk.as_ref().and_then(|d| d.1.clone()),
                "磁盘占用",
                false,
                false,
                t,
            ))
            .child(crate::widgets::metric_cell(
                clean.as_ref().map(|d| d.0.clone()).unwrap_or_else(|| "—".into()),
                clean.as_ref().and_then(|d| d.1.clone()),
                "可清理",
                // 只有"真扫出来有东西可清"才升到 warn 色——没扫过时它是 `—`，
                // 给个橙色等于催用户去清一个还不知道多大的东西
                cache > 0,
                true,
                t,
            ))
            .child(crate::widgets::metric_cell(
                format!("{}", status.ahead),
                None,
                "待推送",
                false,
                true,
                t,
            ))
            .child(crate::widgets::metric_cell(
                format!("{changed}"),
                None,
                "未提交",
                false,
                true,
                t,
            ))
    }

    /// 仓库头右侧的常用命令组：**「常用命令」+ 一颗展开键**。
    ///
    /// ## 为什么这里要有，而不是只留 ⌘K
    ///
    /// 阶段 21-5 把概览页的「常用命令」段撤了（同一对入口摆两遍），
    /// 改入口落在 ⌘K。**但 ⌘K 只能靠键盘进**——项目文档明写「命令面板只能靠
    /// 键盘进」是个已知取舍，而用户反馈回来的原话是「**现在没有入口了**」。
    /// 那条取舍对「低频动作」成立，对**构建/测试**不成立：
    /// 它是日常要用的，而它唯一的入口是一个用户不一定记得快捷键的面板。
    ///
    /// ## 为什么是「一颗键 + 菜单」而不是平铺四个
    ///
    /// 四个槽位（开发运行 / 编译构建 / 测试 / 安装依赖）全平铺在仓库头
    /// 会把标题行撑成一排按钮。**平铺两颗最常用的、其余收进菜单**——
    /// 与项目一贯的「高频常显、低频进菜单」是同一条。
    ///
    /// ## 编辑入口跟着一起给
    ///
    /// 用户明确要「手动设置开发和构建的命令」。**能跑不能改同样是残废的**
    /// ——而且更隐蔽：命令配错了用户只会看到"跑了个错的命令"，
    /// 不会想到要去改。所以编辑键与运行键**并排**，不能藏在更深的地方。
    fn quick_cmds(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(path) = self.selected.clone() else {
            return div().into_any_element();
        };
        let running = self.log().running;
        // 跑着的时候与日志段那两颗同口径禁用：同仓库已有活在跑，
        // 再起一个会撞掉 core 那个进程级全局锁
        let mut group = h_flex().gap(px(GAP_TIGHT)).items_center();

        for (slot, label) in QUICK_SLOTS {
            let cmd = self.effective_cmd(&path, slot);
            let busy = running && self.log().op == Some(Op::Cmd(slot));
            group = group.child(
                Button::new(SharedString::from(format!("head-cmd-{}", slot.key())))
                    .label(label)
                    // ⚠️ **两颗键各带一颗图标**（原型 `.head-acts button > svg` 13×13，
                    // 2026-10-08 用户第④条点名"这两个原型设计是有带图标的，实际未做"）。
                    // 图标不是装饰：这两颗并排时**只差两个字**，扫一眼分不清哪个是跑
                    // 哪个是构建，而跑错一个是白等一遍。尺寸不给——官方 Button 会按
                    // 按钮档自己定（`button.rs:756` 无条件 `with_size(icon_size)`），
                    // 这里传 `icon_sm` 的 14 也会被换成档内值，给了反而是句谎话
                    .icon(slot_icon(slot))
                    .custom(ghost_variant(cx))
                    .with_size(Size::XSmall)
                    .loading(busy)
                    .disabled(running)
                    .tooltip(if cmd.is_empty() {
                        format!(
                            "这个仓库还没认出{}，点右边的「常用命令…」去设",
                            slot.default_label()
                        )
                    } else {
                        cmd
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.run_slot(slot, cx))),
            );
        }

        group
            // 「常用命令…」：编辑四个槽位 + 跑钉住的自定义命令。
            // **它就是 ⌘K 里那两条入口的可见版**（`RunSlot` / `EditCmds`），
            // 走的是同两个动作，不另写一套逻辑
            .child(
                Button::new("head-cmds-more")
                    .icon(icon_sm(AssetIcon::SlidersHorizontal).text_color(tier3(t)))
                    .custom(ghost_variant(cx))
                    .with_size(Size::XSmall)
                    .tooltip("常用命令…（跑钉住的命令 / 编辑四个槽位的命令）")
                    .accessibility_label("常用命令")
                    // 闭包要 `'static`，所以先把路径 clone 一份再捕获——
                    // 直接捕获 `path` 会借用本函数的局部变量（E0373）
                    .on_click({
                        let p = path.clone();
                        cx.listener(move |this, _, window, cx| {
                            this.open_cmds_dlg(p.clone(), window, cx, None);
                        })
                    }),
            )
            .into_any_element()
    }

    /// 仓库头右侧那颗常驻「停止」键（只在有活跑着时出现）。
    ///
    /// ## 为什么用危险色
    ///
    /// 「停止」不是普通动作：它会**打断**一个正在进行的操作。
    /// 与旁边那些"看 / 复制 / 打开"的键同规格同底色的话，
    /// 误触一次就是白跑一遍构建。危险色 + tooltip 说明"停止当前操作"，
    /// 让它在视觉上就比别的键重一档。
    ///
    /// ## ⛔ 为什么**绝对不能**带 `loading`（2026-10-05 实机抓出来的）
    ///
    /// 第一版给这颗键写了 `.loading(lg.running)`，想让它顺带当"正在跑"的指示器。
    /// **结果这颗键永远点不动**——而抓帧看着完全正常：按钮在、红色对的、
    /// hover 也变，`cargo check` 零错，单测全绿。
    ///
    /// 根因在基座，源码 `gpui-component-0.7.0/src/button/button.rs:1679` 写死了
    /// 而且**有单测锁着这条约束**：
    /// > A loading button must be as inert as a disabled one. `interactive` is what
    /// > gates the hover and active styling, the `cursor_pointer` of link buttons
    /// > and the `mouse_down` handler.
    /// ```ignore
    /// assert!(!Button::new("test").loading(true).interactive());
    /// ```
    ///
    /// 于是 `loading` 与「只要 running 就出现」这两个条件叠在一起 = **这颗键
    /// 出现的那一帧起就是死的**。而它恰恰是构建卡死时唯一的救命键。
    ///
    /// **语义上它俩就是反的**：`loading` 说的是"这个操作正在自己进行中，
    /// 别再点"；「停止」说的恰恰是"打断现在进行中的这个"。
    /// 同理 `.disabled(running)` 也不能加——那更直白地自杀。
    ///
    /// **"正在跑"这件事已经不用它表达了**：这颗键**只在 running 时出现**，
    /// 它的存在本身就是指示器（旁边还常驻着状态栏的「运行中 · 仓库名」）。
    /// 要额外的视觉重量就加个静态图标，别动 `loading`。
    fn stop_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let what = match self.log().op {
            Some(op) => op.label().to_string(),
            None => "当前操作".to_string(),
        };
        Button::new("head-stop")
            .label("停止")
            // ⚠️ 用 `danger` 档而不是 ghost：见上面「为什么用危险色」。
            // 官方 Button 有 `danger` 变体（`ButtonVariants::danger`），不用自研
            .danger()
            .with_size(Size::Small)
            // ⚠️ **这里刻意没有 `.loading(..)`**：不是漏了，是加了键就废了。
            // 根因与证据见上面那段注释。改这颗键前先把它读完。
            .tooltip(format!("停止{what}"))
            .accessibility_label(format!("停止{what}"))
            .on_click(cx.listener(|this, _, window, cx| this.stop_current(window, cx)))
            .into_any_element()
    }

    /// 地址区：**一行**（原型 `.l2{display:flex;gap:7px}`，实测整行 18 高）。
    ///
    /// 2026-10-08 用户第⑤条原话：「本地加网址 小圆点 云端加网址 在一行，真实的
    /// 排版 文字 都没去做适配，**为什么要做到一行，是为了压缩首卡片的高度。
    /// 把最大化空间留给日志页面**」——他把动机一起说了，所以这不是"照抄原型"，
    /// 是**为日志让高**：原来两行 + 行间 gap 白吃 ~24px，而头卡下面每一段都要跟它抢。
    ///
    /// 一行装两段的排法：**本地那段 `flex_1 + min_w_0`**（磁盘路径是最长的那种，
    /// 让它先截），云端那段只 `min_w_0`（能缩，但不抢余量）。
    /// 复制键仍按段悬停（`Shell::addr_hover` 的两个键不变），
    /// 停在哪一段只露那一段的键——一行两段共用一个 hover 标志会两颗一起冒出来
    pub(crate) fn addr_line(
        &self,
        path: &str,
        status: &RepoStatus,
        copied: Option<&'static str>,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 「云端」这一格答的是**这台仓库记没记一个云端地址**，
        // **不是**「那个地址能不能翻成浏览器打开的网页」。
        //
        // 原来这里写的是 `browse_url_from_remote(&status.remote_url)
        // .filter(|_| status.has_upstream)` —— 拿"翻不翻得出网页"当"有没有远端"的答案，
        // 于是 `file://` 这类本地远端、自定义 ssh 别名的远端会被说成
        // 「该仓库还没有配置远端」，而**同一屏下面那句状态话又说「跟踪 origin/main」**，
        // 两处直接矛盾（2026-10-08 抓帧登记，本轮修）。
        // 他真实那 17 台都是 GitHub / Gitee，翻得出来 ⇒ 平时看不见这句错话。
        //
        // 现在拆成三问，各答各的，**一个都不许替另一个回答**：
        //   问过没有 ← `status_read`（没问过就承认不知道，别替它答"没配"）
        //   有没有   ← `remote_url` 非空
        //   点得开吗 ← `browse_url_from_remote` 得得出网址，**只决定可点性**
        //
        // ⚠️ 那个 `.filter(has_upstream)` 一并去掉：`browse_url_from_remote` 只做
        // 字符串换算、给出的又是**仓库主页**（`net.rs:284`，不碰分支），
        // 当前分支有没有上游都不会让它变成空白页。"上游"那件事由同步卡主句与
        // 名字后面那颗感叹号说，不该由这一格兼职——它兼职的代价就是上面那句矛盾。
        let web = net::browse_url_from_remote(&status.remote_url);
        // 显示用的文案**先算出来再移动**：`web` 下面要整个交给 `AddrKind`，
        // 而 `as_deref()` 借的是它——同一个值既借又移，编译器会拦（E0505）
        //
        // ⚠️ **显示的那一句 = 点它要去的那一句 = 复制键给的那一句**，三者必须同一个值
        // （`AddrKind::Cloud(Some(u))` 里那个 `u` 同时是跳转目标和复制内容）。
        // 所以翻得出网页时这里显示的是**主页地址**而不是 `remote_url` 原文：
        // 显示 `git@github.com:x/y.git`、点进去却是 `https://github.com/x/y`，
        // 用户复制到的和他刚点开的就是两个东西（他这一路一直在消的"两处各说一遍"）。
        // 而翻不出网页时没有"要去的那一句"可对齐，就把**原地址本身**写出来——
        // 它点不了，可它是真的
        let cloud_text = if !self.status_read(path) {
            // 与总览「状态」列、⌘K、同步卡主句**同一个词**，不另立第三种说法
            bucket_word(Bucket::Unknown).to_string()
        } else if let Some(url) = web.clone() {
            url
        } else if status.remote_url.trim().is_empty() {
            "该仓库还没有配置远端".into()
        } else {
            status.remote_url.clone()
        };
        h_flex()
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap(px(GAP_TIGHT))
            .child(self.addr_seg(
                "本地",
                path,
                AddrKind::Local(path.to_string()),
                copied == Some("local"),
                t,
                cx,
            ))
            // 两段之间那个小圆点：它是**正文里的一个字符**，不是一颗图标，
            // 所以不算"文字冒充图标"（令牌闸那条 R5 管的是拿 ✓/✗ 当状态图标）
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(fs_meta()))
                    .text_color(tier4(t))
                    .child("·"),
            )
            .child(self.addr_seg(
                "云端",
                &cloud_text,
                AddrKind::Cloud(web),
                copied == Some("remote"),
                t,
                cx,
            ))
            .into_any_element()
    }

    /// 一行里的**一段**地址：标签 / 可点地址 / 悬停才现的复制键
    ///
    /// ⚠️ 悬停状态是**记在 `Shell` 上的**，不是 gpui 的 `:hover`——gpui 没有
    /// CSS 那样的伪类，悬停只能靠 `on_hover` 回调把状态写回来（`div.rs:675`）。
    /// 而 `on_hover` 的闭包是 `'static`，捕不了 `&self`，
    /// 所以复制键的显隐由 `Shell::addr_hover` 决定（`["local"|"remote"|None]`）。
    ///
    /// `grows` = 这一段要不要吃掉行里的余量。**只有本地那段给 `true`**：
    /// 磁盘路径是这一屏最长的那种文本，让它先占位、先截断；
    /// 云端那段能缩（`min_w_0`）但不抢余量，否则两段各一半，
    /// 「该仓库还没有配置远端」这种短句右边会空出一大片
    fn addr_seg(
        &self,
        label: &'static str,
        text: &str,
        kind: AddrKind,
        just_copied: bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = kind.key();
        // ⚠️ 判据是「**这一段**被悬停」，不是「有任意一段被悬停」——
        // 只判 `is_some()` 的话指针停在哪一段、**两段的复制键一起冒出来**
        // （2026-10-05 抓帧）。两个键就是 `local` / `remote`，直接比同一个键
        let hovered = self.addr_hover == Some(key);
        // 可点性**只由 `AddrKind` 判这一次**（文件末尾那条注释就是这个意思）。
        // ⚠️ 这里原来还挂着第二条判据 `text.starts_with("该仓库")`——**拿文案字符串
        // 当状态**：改一个字就静默失效。而「云端」这一格现在有三种值
        // （待扫描 / 能打开的主页 / 有地址但翻不出网页），前缀判据连第三种都判不出来。
        // 逐一核过：`clickable` 为真时 `text` 必然是真地址（`Cloud(Some(u))` 的 `u`
        // 就是显示的那一句），所以 `!disabled` 恒成立 ⇒ **查过不生效就删**，
        // 不留一句会骗人的话
        let clickable =
            matches!(kind, AddrKind::Cloud(Some(_))) || matches!(kind, AddrKind::Local(_));

        h_flex()
            .id(if key == "local" {
                "addr-local"
            } else {
                "addr-remote"
            })
            // ⚠️ **既不 `w_full` 也不 `flex_1`**：`w_full` 两段各占 100% 父宽、
            // 第二段被挤出卡外；`flex_1` 会把本地那段撑满余量，于是
            // 「· 云端 …」被推到**行最右端**，中间一大片空（改后第一版抓帧就是这样，
            // 与原型"三段紧挨着排"完全相反）。原型 `.l2{display:flex;gap:7px}`
            // 就是按内容左排。截断靠 `min_w_0` + 默认的 shrink，不靠撑满
            .min_w_0()
            .gap(px(GAP))
            .items_center()
            // ⚠️ `.id(..)` 之后这行是 `Stateful<Div>`，`on_hover` / `on_click`
            // 才有——裸 `Div` 上没有这两个方法（E0599）
            .on_hover(cx.listener({
                move |this, over, _, cx| {
                    this.addr_hover = if *over { Some(key) } else { None };
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(fs_meta()))
                    .text_color(tier4(t))
                    .child(label),
            )
            // 地址与复制键装进**同一个内层行**，`flex_1` 挂在内层行上。
            //
            // ⚠️ 之前复制键是这一行的**直接子项**，而地址文本自己 `flex_1()`——
            // 文本把余量全吃掉，键被顶到**行最右端**，离地址文字 600px
            // （2026-10-05 抓帧）。要求是"地址后跟一颗"，所以要让键跟着文本走：
            // 文本按内容宽排（只保留 `min_w_0` 供长路径截断），键紧跟其后。
            // 文本过长被截断时键会贴到行尾——那是唯一说得过去的位置。
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(GAP_TIGHT))
                    .items_center()
                    .child(
                        div()
                            // ⚠️ 这个 `id` 是 `on_click` 的前置条件：`on_click` 只在
                            // `Stateful<Div>` 上有（`InteractiveElement`），
                            // 裸 `Div` 上是 E0599。id 本身也顺带给了这一行稳定标识。
                            .id(if key == "local" {
                                "addr-text-local"
                            } else {
                                "addr-text-remote"
                            })
                            .min_w_0()
                            .truncate()
                            .font_family(t.mono_font_family.clone())
                            .text_size(px(fs_meta()))
                            // 可点 = 三级灰，不可点 = 四级灰。**颜色本身就是"能不能点"的提示**，
                            // 不必再画一个禁用态出来
                            .text_color(if clickable {
                                tier3(t)
                            } else {
                                tier4(t)
                            })
                            // ⚠️ **那颗"下沉小块"只给点得开的那一段**：原型
                            // `.l2 code{background:var(--card-sunken);
                            // border:1px solid var(--line-soft);padding:1px 7px;
                            // border-radius:7px}` 包的是路径，而"该仓库还没有配置远端"
                            // 是一个**裸的 faint `<span>`**（原型 HTML 里就写着内联色）。
                            // 给点不了的那一段也画上底+边，等于用容器说"这里有个能点的东西"
                            // 而它什么都不能点——那是假 affordance。
                            // 「有地址但翻不出网页」现在也走这一臂（不给底），
                            // 它和占位句在"点了没反应"上确实同类，区别由文本自己说
                            //
                            // `--card-sunken` 与 theme 的 `muted.background` 是同一个值
                            // （亮 #F4F7F8 / 暗 #101E22 两边都精确对上）⇒ **读 token 不写色值**
                            .when(clickable, |addr| {
                                addr.bg(t.colors.muted)
                                    .border_1()
                                    .border_color(track(t))
                                    .rounded(px(R_MD))
                                    .px(px(GAP))
                            })
                            .when(clickable, |addr| {
                                // 只把**跳转目标**搬进闭包，不搬 `kind` 本身：
                                // `kind` 下面还要给复制键用（E0382 use of moved value）
                                let target = kind.jump_target();
                                // 闭包里是 `&target` 而不是移动它：监听器是 `Fn`
                                // （一次点击可能被派发多次），搬走就编不过（E0507）
                                addr.cursor_pointer().on_click(cx.listener(
                                    move |_, _, window, cx| {
                                        let Some((url, is_local)) = target.as_ref() else {
                                            return;
                                        };
                                        let r = if *is_local {
                                            os::open_folder(url)
                                        } else {
                                            os::open_url(url)
                                        };
                                        if let Err(e) = r {
                                            window.push_notification(
                                                if *is_local {
                                                    format!("打开文件夹失败：{e}")
                                                } else {
                                                    format!("打开浏览器失败：{e}")
                                                },
                                                cx,
                                            );
                                        }
                                    },
                                ))
                            })
                            .child(text.to_string()),
                    )
                    // 复制键：**平时不占位**。`when` 而不是给个 `opacity(0)`——
                    // 透明元素照样能点到，那会变成"看不见但能按"的一颗键
                    .when(hovered, |inner| {
                        inner.child(self.addr_copy_button(kind, just_copied, t, cx))
                    }),
            )
            .into_any_element()
    }

    /// 一颗复制键。**点完短暂换对号**（`just_copied`），不弹气泡通知。
    ///
    /// 用官方 `Button` 的 ghost + XSmall 档（外框 20px、14px 字 / 14px 图标）：
    /// 这是"悬停才出现"的那种小键，尺寸跟着 `Size::XSmall` 走就够，
    /// 没必要自己拼一个 `div().on_click(..)`——那样会同时丢掉悬停面、
    /// 焦点环与 tooltip，而这三样正是"看不见的一颗键"能被找到的原因。
    fn addr_copy_button(
        &self,
        kind: AddrKind,
        just_copied: bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (id, label) = match kind.key() {
            "local" => ("addr-copy-local", "复制本地地址"),
            _ => ("addr-copy-remote", "复制云端地址"),
        };
        // 复制的是什么：本地复制磁盘路径，云端复制**能打开的主页地址**。
        // 复制 origin 的 `git@github.com:x/y.git` 对用户没用——粘到浏览器里打不开。
        // 所以与 `jump_target` 共用同一份判据，没有远端时那颗键就是禁用的
        let payload = kind.jump_target().map(|(p, _)| p);
        let enabled = payload.is_some();
        Button::new(id)
            .icon(
                icon_sm(if just_copied {
                    AssetIcon::Check
                } else {
                    AssetIcon::Copy
                })
                .text_color(if just_copied {
                    t.colors.success
                } else {
                    tier3(t)
                }),
            )
            .custom(ghost_variant(cx))
            .with_size(Size::XSmall)
            .tooltip(label)
            .accessibility_label(label)
            .disabled(!enabled)
            .on_click(cx.listener(move |this, _, _, cx| {
                let Some(text) = payload.clone() else {
                    return;
                };
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                this.mark_copied(kind.key(), cx);
            }))
            .into_any_element()
    }

    /// 记下"刚复制过哪一条"，并在两秒后翻回去
    ///
    /// **不用 `push_notification` 做反馈**（用户明确要求取消那个气泡）：
    /// 复制是个一秒内完成的动作，为它弹一个通知会盖住地址本身。
    /// 按钮换对号两秒是"就地"反馈，不打断视线。
    fn mark_copied(&mut self, key: &'static str, cx: &mut Context<Self>) {
        self.copied = Some(key);
        self.copied_until = qingbird_core::store::now_secs() + COPY_FEEDBACK_SECS;
        self.arm_copy_expiry(cx);
        cx.notify();
    }

    /// 排一个短任务，到点后清掉 `copied` 并重画。
    ///
    /// ## 为什么用"排一个定时任务"而不是在 `render` 里轮询
    ///
    /// 轮询要每帧 notify 才能等到两秒后，白烧两秒 CPU；而这里**只在 `copied`
    /// 还挂着的那几帧里**排这一个任务，它醒来做完就结束。
    ///
    /// ## 任务句柄刻意**不存**进 `Shell` 的字段
    ///
    /// 连点两次复制会排出两个任务，而它们做的都是"把 `copied` 清掉"，幂等。
    /// 为一个幂等动作留一个必被 `dead_code` 盯上的句柄字段不划算。
    ///
    /// 窗口一关任务随 `Context` 一起死，`copied` 随 `Shell` 一起没了——
    /// 不会留下"下次开窗就带着一个过期对号"的状态
    pub(crate) fn arm_copy_expiry(&self, cx: &mut Context<Self>) {
        let key = self.copied;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(COPY_FEEDBACK_SECS))
                .await;
            // 窗口可能在两秒内被关掉，`update` 拿不回实体就是它不在了。
            // 那是正常路径（不是错误），所以 `.ok()` 显式吞掉——
            // 留着 `Result` 未处理会变成一条每次关窗都打出来的警告
            this.update(cx, |this, cx| {
                // 只清自己那一条：两秒内又复制了另一条地址的话，
                // 不该把新那条的对号一起抹掉
                if this.copied == key {
                    this.copied = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

/*
/// 2026-10-04 从这个文件里撤掉的东西（留这段说明，是为了别按 AGENTS.md
/// 里旧的描述把它们加回来）：
///
/// - `Shell::addr`（`state.rs`）：一个只读 `Input` 胶囊框装云端地址。
///   它的值只被 `refresh_addr` 写进来、**界面上没有任何渲染读它**，
///   是 D35 记的那一类「只被写者自己读的状态槽」。留着会让下一个人
///   以为"地址条还在"而不敢动这段。
/// - 页签上方那根 `Separator`：两行地址自己已经把头部和页签分开了，
///   再来一根横线只是让仓库头的横线多一条。
/// - 右上角那四颗动作键：打开文件夹 / 终端 / 云端 / 复制。前三颗分别
///   并进了地址行（可点地址 + 复制键），「终端」并进了日志工具行。
///   四颗并排是旧布局的遗留，键的总数一屏里已经偏多。
*/

/// 两颗快捷命令键各自那颗图标（原型 `.head-acts button > svg`）。
///
/// **映射写在 GUI 不写进 core**：`Slot` 是 `qingbird_core::run_cmd` 的类型，
/// 它不认识 gpui 的图标枚举，为它加一个返回图标名的方法等于让 core 认识界面。
///
/// 为什么值得给图标而不是靠字：这两颗并排时标签只差两个字（开发运行 / 编译构建），
/// 扫一眼分不出来，而点错一颗是白等一遍构建。
fn slot_icon(slot: Slot) -> Icon {
    icon_sm(match slot {
        Slot::Dev => AssetIcon::Play,
        Slot::Build => AssetIcon::Hammer,
        Slot::Test => AssetIcon::FlaskConical,
        Slot::Install => AssetIcon::PackagePlus,
        Slot::Pinned(_) => AssetIcon::Play,
    })
}

/// 一行地址的种类。
///
/// `Cloud(None)` = **这一格点不开**，不是"这台仓库没有云端"。
/// 它有两种来路：压根没配远端（文本是占位句），以及**有远端但翻不出网页**
/// （`file://`、自定义 ssh 别名那类，文本就是那条真地址）。
/// `addr_seg` 用它把那一段画成四级灰、不给底、不给手型光标，复制键也跟着禁用。
/// ⚠️ 别拿它当"没有远端"用——2026-10-08 台账第一条就是这个混法
/// （拿"能不能翻成网页"回答"有没有配置"，于是同一屏下面那句还能说「跟踪 origin/main」）。
/// "有没有得跳"的分界只在这里判一次，别在别处重新判一遍。
#[derive(Clone, Debug)]
enum AddrKind {
    /// 本地磁盘路径。恒可点（打开文件管理器）
    Local(String),
    /// 云端**主页**地址。`None` = 这一格点不开（≠ 没有远端）
    Cloud(Option<String>),
}

impl AddrKind {
    /// 稳定标识：作 `Shell::addr_hover` / `Shell::copied` 的键，
    /// 也作元素 id 的一部分。**两处必须一致**——一个是悬停态的键、
    /// 一个是复制反馈的键，写岔了会出现"悬停亮的是本地、复制反馈的是云端"
    fn key(&self) -> &'static str {
        match self {
            AddrKind::Local(_) => "local",
            AddrKind::Cloud(_) => "remote",
        }
    }

    /// 点地址要去哪：`Some((地址, 是不是本地))`，`None` = 没有可跳的远端。
    ///
    /// **单独抽出来是因为"能不能点"只该判一次**：`addr_row` 用它决定
    /// 要不要挂 `on_click` 与手型光标，点击闭包里再拿同一个值去执行，
    /// 两处判同一个布尔值迟早会漂（挂了手型却什么都没发生的最坏情况）。
    /// 另有一层好处：点击闭包只搬这一个 `String`，
    /// 不用把整个 `AddrKind` 搬进去（`kind` 下面还要给复制键用）
    fn jump_target(&self) -> Option<(String, bool)> {
        match self {
            AddrKind::Local(p) => Some((p.clone(), true)),
            AddrKind::Cloud(Some(u)) => Some((u.clone(), false)),
            AddrKind::Cloud(None) => None,
        }
    }
}
