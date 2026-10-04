//! 仓库头：常驻不随页签切换。两类仓库共用同一个头。
//!
//! **为什么两类仓库必须共用一个头**：头承载的是"我在哪个仓库、它在哪、
//! 我能跳到哪去"这几件事，这几件事对"我的仓库"和"第三方仓库"没有区别。
//! 分成两个头，就会有两处各自漂移的地址格式、两份取色——而用户在两个页面
//! 之间来回切时，那种"看着一样、点下去不一样"的差异是最伤信任的。
//!
//! ## 地址区（2026-10-04 改版）
//!
//! 原来是「路径 + 最后提交」一行、一个只读 `Input` 胶囊框、页签上方一根分隔线。
//! 现在收成**「本地地址 / 云端地址」两行**（用户要求）：
//! 地址本体可点（本地 → 文件管理器，云端 → 浏览器），
//! 悬停才现一颗复制键，点完换成对号两秒。
//! 胶囊框与那根分隔线一并删掉——它们与两行地址叠在一起，仓库头横线太多。
//!
//! 四个跳转动作与侧栏右键菜单共用 `os::` 那几个函数，动词与结果三处一致。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable, Size, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, AnyElement, ClipboardItem, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement, StatefulInteractiveElement as _, Styled,
};
use qingbird_core::net;
use qingbird_core::types::RepoStatus;

use crate::detail::DetailTab;
use crate::os;
use crate::state::*;
use crate::tokens::*;
use crate::widgets::ghost_variant;
use crate::widgets::{meta_text, pill};

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
                            .text_size(px(FS_HEAD))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.colors.foreground)
                            .child(self.title(path)),
                    )
                    // 归属徽标常驻：这是"这个仓库我该不该改"的第一个判断依据，
                    // 藏在别处就得多点一次才知道
                    .child(pill(
                        if third { "第三方" } else { "我的" },
                        if third { orange_text(t) } else { tier3(t) },
                        if third { orange_bg(t) } else { track(t) },
                    ))
                    .when(!status.branch.is_empty(), |row| {
                        row.child(self.branch_badge(&status.branch, t))
                    })
                    // 磁盘摘要跟在名字这一行的元信息簇末尾（用户要求搬到仓库名后面）。
                    // 位置选在分支徽标**之后**而不是紧贴名字：名字 + 归属 + 分支
                    // 说的是"这是谁"，磁盘说的是"它多大"，插进去会把那一簇拆开
                    .child(self.space_badge(t, cx))
                    // 活跃度**只给第三方**：概览页三段版面是两边共用的，
                    // 而"这个项目最近还活着吗"是评估一个别人项目时最该先看的一句，
                    // 塞进共用版面就得为它单开一段。它属于**概要位**——
                    // 与磁盘摘要同一类问题（这东西怎么样），所以挂在同一行
                    .when(third, |row| row.child(self.activity_chip(t)))
                    .child(div().flex_1())
                    // 右侧只留「展开检视」——原来那四颗（打开文件夹 / 终端 /
                    // 云端 / 复制）里三颗已经进了下面的地址区（用户要求），
                    // 剩下的「终端」并进了日志工具行
                    .when(!self.inspector_open, |row| {
                        row.child(self.inspector_reopen(t, cx))
                    }),
            )
            // 地址区：本地 + 云端两行，各自可点跳转、各带一颗悬停才现的复制键
            .child(self.addr_rows(path, &status, copied, t, cx))
    }

    /// 第三方仓库名后面那枚「近 30 天」活跃度徽标（迷你柱图 + 一句结论）。
    ///
    /// **只读、不可点**——所以不用官方 `Button`（它自带命中区与悬停面，
    /// 给一个不响应点击的东西挂上那是骗人）。但**不能光摆一根柱子**：
    /// 用户看不懂"8 根柱里有 3 根是满的"是在说什么，所以配一句结论文字。
    ///
    /// 数据来自 `panels::activity_chart`（读 `ProfileState.activity`）。
    /// 它是**第三方独有**的：`scan_profile` 只给第三方跑 `commit_activity`
    /// （扫 30 天 log），所以这一枚也只在第三方那颗「第三方」徽标之后出现。
    fn activity_chip(&self, t: &Theme) -> Div {
        let days = self
            .profile
            .activity
            .iter()
            .filter(|d| d.count > 0)
            .count();
        let commits: u32 = self.profile.activity.iter().map(|d| d.count).sum();
        let note = if self.profile.scanning && self.profile.activity.is_empty() {
            // ⚠️ 与 `panels::space_summary_line` 同一处修正（阶段 16）：
            // **有数据就别再报"统计中"**。库里那份已经上屏时还说统计中，
            // 图表上明明画着柱子了
            "统计中…".to_string()
        } else if commits == 0 {
            "近 30 天没有提交".to_string()
        } else {
            format!("近 30 天 {days} 天 · {commits} 次提交")
        };
        h_flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(GAP_TIGHT))
            .child(self.activity_chart(t))
            .child(meta_text(note, t))
    }

    /// 仓库名后面那枚磁盘摘要徽标（用户要求把磁盘统计从底排那张卡搬上来）。
    ///
    /// **为什么用官方 `Button` 而不是现成的 `pill()`**：`pill()` 是个纯 `Div`，
    /// 既没有命中区也没有悬停面，而这枚要能点（跳「空间」页签）。`Button` 的
    /// ghost + XSmall 档自带命中区、悬停面与焦点环，只是观感比实底胶囊轻——
    /// 而这正是"概要位"该有的重量：提醒有这回事，不是宣称这是一个状态。
    ///
    /// 文案与判据全在 `Shell::space_summary_line`（`panels.rs`）里，
    /// 这里只管摆位与跳转，不把"扫没扫 / 有没有可清的"重写一遍。
    fn space_badge(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let line = self.space_summary_line();
        Button::new("head-space")
            .icon(icon_sm(AssetIcon::HardDrive).text_color(tier4(t)))
            .label(line.clone())
            .custom(ghost_variant(cx))
            .with_size(Size::XSmall)
            .tooltip("查看磁盘占用详情")
            .accessibility_label(line)
            .on_click(cx.listener(|this, _, _, cx| this.open_detail_tab(DetailTab::Space, cx)))
            .into_any_element()
    }

    /// 地址区两行：**本地地址 / 云端地址**（用户要求）。
    ///
    /// 每行的结构都一样：一个固定宽的标签 + 一段可点的地址 + 一颗复制键。
    /// 两行共用 `addr_row` 那个函数——差别只在"点地址做什么"和"复制什么"，
    /// 写法分开的话两处样式会各自漂移（这正是要消掉的那类重复）。
    ///
    /// **地址本体可点**：本地 → 在文件管理器打开；云端 → 浏览器打开仓库主页。
    /// 这两件事原先是右上角四颗图标键里的两颗，现在直接做进地址里——
    /// 地址本身就是那个目的地，指一遍比再点一颗不相干的图标短。
    ///
    /// **复制键平时藏着，鼠标移上这一行才出现**（用户要求）。不用气泡通知做反馈，
    /// 点完按钮自己换成对号再翻回去（`Shell::copied` / `copied_until`）。
    fn addr_rows(
        &self,
        path: &str,
        status: &RepoStatus,
        copied: Option<&'static str>,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 「云端主页」的门槛是**真的能打开一个网页**，不是"有远端就显示"：
        // 一个没有 upstream 的仓库点开是空白页，比不显示更糟
        let cloud = net::browse_url_from_remote(&status.remote_url).filter(|_| status.has_upstream);
        // 显示用的文案**先算出来再移动**：`cloud` 下面要整个交给 `AddrKind`，
        // 而 `as_deref()` 借的是它——同一个值既借又移，编译器会拦（E0505）
        let cloud_text = cloud
            .clone()
            .unwrap_or_else(|| "该仓库还没有配置远端".into());
        v_flex()
            .w_full()
            .flex_shrink_0()
            .gap(px(GAP_DENSE))
            .child(self.addr_row(
                "本地地址",
                path,
                AddrKind::Local(path.to_string()),
                copied == Some("local"),
                t,
                cx,
            ))
            .child(self.addr_row(
                "云端地址",
                &cloud_text,
                AddrKind::Cloud(cloud),
                copied == Some("remote"),
                t,
                cx,
            ))
            .into_any_element()
    }

    /// 一行地址：标签 / 可点地址 / 悬停才现的复制键
    ///
    /// ⚠️ 悬停状态是**记在 `Shell` 上的**，不是 gpui 的 `:hover`——gpui 没有
    /// CSS 那样的伪类，悬停只能靠 `on_hover` 回调把状态写回来（`div.rs:675`）。
    /// 而 `on_hover` 的闭包是 `'static`，捕不了 `&self`，
    /// 所以复制键的显隐由 `Shell::addr_hover` 决定（`["local"|"remote"|None]`）。
    fn addr_row(
        &self,
        label: &'static str,
        text: &str,
        kind: AddrKind,
        just_copied: bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = kind.key();
        // ⚠️ 判据是「**这一行**被悬停」，不是「有任意一行被悬停」——
        // 只判 `is_some()` 的话指针停在哪一行、**两行的复制键一起冒出来**
        // （2026-10-05 抓帧）。两个键就是 `local` / `remote`，直接比同一个键
        let hovered = self.addr_hover == Some(key);
        let disabled = text.starts_with("该仓库");
        // 可点性是**状态**：没有远端时那一行给的是占位句，不是地址
        let clickable =
            matches!(kind, AddrKind::Cloud(Some(_))) || matches!(kind, AddrKind::Local(_));

        h_flex()
            .id(if key == "local" {
                "addr-local"
            } else {
                "addr-remote"
            })
            .w_full()
            .gap(px(GAP_TIGHT))
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
                    .w(px(56.0))
                    .text_size(px(FS_META))
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
                            .text_size(px(FS_META))
                            // 可点 = 三级灰，不可点 = 四级灰。**颜色本身就是"能不能点"的提示**，
                            // 不必再画一个禁用态出来
                            .text_color(if clickable && !disabled {
                                tier3(t)
                            } else {
                                tier4(t)
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

/// 一行地址的种类。
///
/// `Cloud(None)`（有仓库但没有能打开的远端）**不是可点状态**：
/// `addr_row` 用它把那一行画成占位句 + 四级灰，不给一个点了没反应的手型光标。
/// 这个"有没有得跳"的分界只在这里判一次，别在别处重新判一遍。
#[derive(Clone, Debug)]
enum AddrKind {
    /// 本地磁盘路径。恒可点（打开文件管理器）
    Local(String),
    /// 云端仓库主页地址。`None` = 没有能打开的远端
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
