//! 详情页内的各张卡片：分叉条 / 同步段 / 语言技术栈 / 磁盘摘要 / 活跃度图表。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::{Sizable, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, Context, Div, FontWeight, Hsla, ParentElement, SharedString, Styled,
};
use qingbird_core::space;
use qingbird_core::types::RepoStatus;

use crate::detail::DetailTab;
use crate::dialogs::fork::fork_commit_block;
use crate::shell::{SyncCheck, SyncPull, SyncPush};
use crate::state::*;
use crate::tokens::*;
use crate::widgets::*;

impl Shell {
    /// 分叉恢复向导卡：本地与云端历史分叉（或刚变基完备份还在）时出现在头部下方，
    /// 把「ff 拉取失败」这种死胡同变成一组有出口的动作。
    /// 按钮逻辑与旧版 forkGuide 一致：仓库卡在用户自己的变基/合并里时只解释、
    /// 一个动作都不给；「撤销变基」会丢弃变基后的新提交，必须二次确认。
    pub(crate) fn fork_card(&self, t: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        let busy = self.log().running;
        let (title, summary, blocked) = if let Some(fk) = &self.fork {
            let mut parts = vec![
                format!("本地有 {} 个提交未推送", fk.ahead),
                format!("云端有 {} 个新提交", fk.behind),
            ];
            if fk.dirty {
                parts.push("工作区还有未提交变更".into());
            }
            (
                "本地与云端已分叉".to_string(),
                format!("{}。", parts.join("，")),
                fk.blocked_reason.clone(),
            )
        } else if let Some(b) = &self.fork_backup {
            (
                "上次变基的备份还在".to_string(),
                format!("本地提交已经重放到云端提交之上。觉得不对可以回到变基前（备份分支 {b}）。"),
                None,
            )
        } else {
            return None;
        };

        // 动作行：变基（脏工作区 = 带暂存版）+ 撤销（二次确认）+ 差异开关
        let mut actions = h_flex().flex_wrap().items_center().gap(px(GAP));
        if blocked.is_none() {
            if let Some(fk) = &self.fork {
                let (label, hint, stash) = if fk.dirty {
                    (
                        "暂存并变基到云端",
                        "先把改动暂存起来再变基，完成后自动恢复；本地提交会被重放到云端提交之上，动手前会建好备份分支",
                        true,
                    )
                } else {
                    (
                        "备份并变基到云端",
                        "把本地提交重放到云端提交之上；动手前会建好备份分支，冲突会自动中止并复原",
                        false,
                    )
                };
                actions = actions.child(
                    Button::new("fk-rebase")
                        .label(label)
                        .tooltip(SharedString::from(hint))
                        .primary()
                        .with_size(Size::Small)
                        .disabled(busy)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.run_sync(Op::Rebase(stash), cx)),
                        ),
                );
            }
            if let Some(_b) = &self.fork_backup {
                if self.fork_confirm {
                    actions = actions
                        .child(
                            Button::new("fk-undo-yes")
                                .label("确认撤销？")
                                .danger()
                                .with_size(Size::Small)
                                .disabled(busy)
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.run_sync(Op::UndoRebase, cx)),
                                ),
                        )
                        .child(
                            Button::new("fk-undo-no")
                                .label("取消")
                                .custom(ghost_variant(cx))
                                .with_size(Size::Small)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.fork_confirm = false;
                                    cx.notify();
                                })),
                        );
                } else {
                    actions = actions.child(
                        Button::new("fk-undo")
                            .label("撤销变基")
                            .tooltip(format!(
                                "回到变基前的状态（备份分支 {}），会丢弃变基之后新产生的提交",
                                self.fork_backup.clone().unwrap_or_default()
                            ))
                            .danger()
                            .with_size(Size::Small)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.fork_confirm = true;
                                cx.notify();
                            })),
                    );
                }
            }
            if self.fork.is_some() {
                actions = actions.child(
                    Button::new("fk-diff")
                        .label(if self.fork_diff {
                            "收起差异"
                        } else {
                            "只看差异"
                        })
                        .custom(ghost_variant(cx))
                        .with_size(Size::Small)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.fork_diff = !this.fork_diff;
                            cx.notify();
                        })),
                );
            }
        }

        // 差异区：两列提交清单 + 一句说明（旧版 fork-diff 原样平移）
        let diff = self.fork.as_ref().filter(|_| self.fork_diff).map(|fk| {
            v_flex()
                .w_full()
                .gap(px(GAP))
                .child(fork_commit_block(
                    true,
                    &fk.local_commits,
                    t,
                ))
                .child(fork_commit_block(
                    false,
                    &fk.remote_commits,
                    t,
                ))
                .child(
                    div()
                        .text_size(px(FS_META))
                        .text_color(tier4(t))
                        .child(format!(
                            "变基会把本地提交重放到 {} 之上，提交内容不变但历史会改写；动手前后端会先把\"变基前\"的位置固定在备份分支上",
                            if fk.upstream.is_empty() { "云端".into() } else { fk.upstream.clone() }
                        )),
                )
        });

        Some(
            v_flex()
                .w_full()
                .gap(px(GAP))
                .p(px(16.0))
                .rounded(px(R_XL))
                .bg(red_bg(t))
                .border_1()
                .border_color(t.colors.border)
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(GAP))
                        .child(
                            div()
                                .text_color(t.colors.danger)
                                .child(icon_md(AssetIcon::GitMerge)),
                        )
                        .child(
                            div()
                                .text_size(px(FS_BODY))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.colors.foreground)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(FS_SEC))
                                .text_color(t.colors.danger)
                                .child(summary),
                        ),
                )
                .children(blocked.as_deref().map(|reason| {
                    div()
                        .text_size(px(FS_SEC))
                        .text_color(body(t))
                        .child(reason.to_string())
                }))
                .when(blocked.is_none(), |card| card.child(actions))
                .children(diff),
        )
    }
}

impl Shell {
    /// 同步段（执行层）：**一行状态句 + 一行三键**。概览页三段里的第一段，
    /// 排在日志之上——三键是这一屏的发起动作，先看见它再看见输出
    ///
    /// **没有圆环。** 2026-10-05 删（用户口径："占地方还难看"）。环那 90px 高
    /// 说的是 `sync_sentence()` 这行字已经说全的事——"云端有 3 个新提交，可以拉取"
    /// 与环上那个 62% 分档弧说的是同一件事。省下的高度全给了日志。
    /// 分档本身没丢：`bucket_of` 还在给侧栏徽标、总览表格与三键的 primary 态取用
    ///
    /// **三键的顺序是 拉取 · 推送 · 检测，不许重排。** 阶段 11 的 `1·2·3`
    /// 按**屏幕位置**对位（用户是先看见按钮再按位置的键），顺序一改，
    /// 快捷键与眼睛看到的东西就对不上了。`检测` 在最右是因为它
    /// 是三键里唯一不改动本地的一把，放末尾不会让人误以为它也在写。
    ///
    /// **定时同步是右上角一颗按钮，不是这一段里的一行设置。** 它是"设置"不是
    /// "动作"，而这一段要给日志让高度；点开一个弹窗去配，入口贴着同步最合理。
    /// 落盘走 `save_sync_settings`，与原来那个「同步设置」页签是同一条路
    ///
    /// **按归属裁剪**：第三方不是我的项目，推送会改别人的仓库，
    /// 所以那把键在它身上不出现（设计文档 3.5 的硬边界）。位置仍然对齐——
    /// 第三方那把空出来的格子补一个不可点的占位，让「拉取」永远在第一个
    pub(crate) fn sync_card(&self, path: &str, t: &Theme, cx: &mut Context<Self>) -> Div {
        let view = self.view(path);
        let status = view.map(|v| v.status.clone()).unwrap_or_default();
        let changed = view.map(|v| v.changed).unwrap_or(0);
        let lg = self.logs.get(path).unwrap_or(&LOG_EMPTY);
        let busy = lg.running;
        let third = self.entry(path).map(|e| e.is_third_party).unwrap_or(false);
        let b = bucket_of(&status);

        // 「有该做的」的那把键给 primary。落后与待推送是互斥的，
        // 分叉时两把都要（得先变基或拉取，推送也会被拒）
        let pull_hot = b == Bucket::Behind || b == Bucket::Forked;
        let push_hot = !third && (b == Bucket::Ahead || b == Bucket::Forked);

        // tooltip 走官方 `tooltip_with_action`：它自己从已绑定的 keymap 里查
        // 出这颗键的 `Kbd` 画上去。**不要手写「（1）」进文案**——键位一改，
        // 手写的那份就成了假提示，而查出来的那份永远不会错
        let key = |id: &'static str,
                   label: &'static str,
                   hot: bool,
                   tip: String,
                   op: Op,
                   action: &dyn gpui_kit::Action,
                   lg: &LogState,
                   cx: &mut Context<Self>| {
            Button::new(SharedString::from(id))
                .label(label)
                .tooltip_with_action(SharedString::from(tip), action, None)
                .when(hot, |btn| btn.primary())
                .when(!hot, |btn| btn.secondary())
                .loading(busy && lg.op == Some(op))
                .with_size(Size::Medium)
                .disabled(busy)
                .on_click(cx.listener(move |this, _, _, cx| this.run_sync(op, cx)))
        };

        let mut keys = h_flex().gap(px(GAP));
        keys = keys.child(key(
            "sync-pull",
            // 文案与 `Op` 都取自 `state.rs::SYNC_SLOTS` / `SYNC_SLOT_LABELS`，
            // 与 `shell.rs::bind_keys` 绑的 `1` `2` `3` 同序——**改一处必须改三处**
            SYNC_SLOT_LABELS[0],
            pull_hot,
            if status.behind > 0 {
                format!("云端有 {} 个新提交，点这里接上来", status.behind)
            } else {
                "拉取云端更新（--ff-only，不会改写本地历史）".into()
            },
            SYNC_SLOTS[0],
            &SyncPull,
            lg,
            cx,
        ));
        if third {
            // 第三方没有推送：留一个同尺寸的空格子，别让「检测」跳到第二个位置
            keys = keys.child(div().w(px(PUSH_KEY_W)).h(px(1.0)).flex_shrink_0());
        } else {
            keys = keys.child(key(
                "sync-push",
                SYNC_SLOT_LABELS[1],
                push_hot,
                if status.ahead > 0 {
                    format!("本地有 {} 个提交没推上去", status.ahead)
                } else {
                    "把本地提交推到云端".into()
                },
                SYNC_SLOTS[1],
                &SyncPush,
                lg,
                cx,
            ));
        }
        keys = keys.child(key(
            "sync-check",
            SYNC_SLOT_LABELS[2],
            false,
            "只看云端有没有更新，不动本地".into(),
            SYNC_SLOTS[2],
            &SyncCheck,
            lg,
            cx,
        ));

        crate::widgets::card(t)
            .flex_shrink_0()
            .gap(px(GAP_TIGHT))
            // 第一行：状态句 + 定时同步入口 +（右栏收着时的）展开入口
            .child(
                h_flex()
                    .w_full()
                    .h(px(24.0))
                    .flex_shrink_0()
                    .gap(px(GAP_TIGHT))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(FS_BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.colors.foreground)
                            .truncate()
                            .child(sync_sentence(&status, b, lg)),
                    )
                    .child(
                        Button::new("sync-timer")
                            .label("定时同步")
                            .custom(ghost_variant(cx))
                            .with_size(Size::XSmall)
                            .tooltip("后台定时自动检测并按设置处理（间隔 / 每天定时 / 检测后自动拉取）")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_sync_dlg(window, cx)
                            })),
                    ),
            )
            // 第二行：三键 + 提交 + 变更入口
            .child(
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(GAP))
                    .child(keys)
                    .child(div().flex_1())
                    // 「提交并推送…」与「提交是改自己的历史」绑在一起：
                    // 第三方不出现。它那一格空出来的位置由**下面第三行**的
                    // 只读档案明细接手——少一把键总得少一份信息量，
                    // 换个地方给回去，而不是让这一行空着
                    .when(!third, |row| {
                        row.child(
                            Button::new("sync-commit")
                                .label("提交并推送…")
                                .custom(ghost_variant(cx))
                                .with_size(Size::Medium)
                                .disabled(busy)
                                .tooltip("先提交本地改动，再推到云端")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_commit_dlg(None, window, cx)
                                })),
                        )
                    })
                    // 变更摘要兼**入口**：变更是中栏的一个页签了，
                    // 而概览页原来那张「未提交的改动」卡已经删掉。
                    // 这一行是概览页上唯一提到未提交改动的地方，
                    // 留一个可点的入口才不会让人以为要自己去页签栏找。
                    // **用官方 `Button` 而不是给文字挂 `on_click`**：
                    // 纯 div 的命中区只有那行字，悬停底色还得自己在 `Shell`
                    // 上存一个状态字段来回传
                    .when(
                        changed > 0,
                        |row| {
                            row.child(
                                Button::new("sync-changes-jump")
                                    .label(format!("{changed} 个文件待提交 · 看清单"))
                                    .custom(ghost_variant(cx))
                                    .with_size(Size::XSmall)
                                    .tooltip("打开「变更」页签，逐个看未提交的文件")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.open_detail_tab(DetailTab::Changes, cx)
                                    })),
                            )
                        },
                    )
                    .when(
                        changed == 0 && !third,
                        |row| {
                            row.child(meta_text("工作区干净，没有可提交的内容", t))
                        },
                    ),
            )
            // 第三行（**只有第三方有**）：那一行只读档案明细。
            //
            // ⚠️ **必须自己占一行，不能塞进上面那行按钮里**：它是四列
            // `DescriptionList`，和三键 + 变更入口抢同一行的宽度时，
            // 右端两颗（定时同步 / 看清单）会被**顶出画面**——
            // 2026-10-05 抓帧实锤，那一行右侧整块是裁掉的。
            // 少一把推送键省下的空间不够它用，宽度只能靠"独占一行"来给
            .when(third, |col| {
                col.child(self.third_archive_row(&status, lg, t))
            })
    }
}


/// 同步卡上那句话。同一个分档在环与这句里说的是同一件事，
/// 别让两处各写一套文案
fn sync_sentence(s: &RepoStatus, b: Bucket, lg: &LogState) -> String {
    if !s.is_repo {
        return "这个文件夹不是 Git 仓库".into();
    }
    if !s.has_upstream {
        return "没有配置上游分支，同步操作会失败".into();
    }
    if lg.running {
        return match lg.op {
            Some(Op::Pull) => "正在拉取云端更新…".into(),
            Some(Op::Push) => "正在推送本地提交…".into(),
            Some(Op::Check) => "正在检查云端更新…".into(),
            Some(Op::Commit) => "正在提交并推送…".into(),
            _ => "正在同步…".into(),
        };
    }
    match b {
        Bucket::Behind => format!("云端有 {} 个新提交，可以拉取", s.behind),
        Bucket::Ahead => format!("本地有 {} 个提交待推送", s.ahead),
        Bucket::Forked => format!(
            "本地 {} 个未推送 · 云端 {} 个新提交，需要变基",
            s.ahead, s.behind
        ),
        Bucket::Dirty => "改动都在本地，提交后即可推送".to_string(),
        _ => {
            if lg.ts == 0 {
                "与云端已同步".to_string()
            } else {
                format!("与云端已同步 · 上次同步 {}", rel_time(lg.ts))
            }
        }
    }
}

/// 「推送」键的占位宽度。第三方那一格要留住它，
/// 否则「检测」会跑到第二个位置，而 `1·2·3` 是按位置对位的
const PUSH_KEY_W: f32 = 76.0;

impl Shell {
    /// 分支徽章：Surface 底 + 分支图标
    pub(crate) fn branch_badge(&self, branch: &str, t: &Theme) -> Div {
        let text = if branch.is_empty() || branch == "HEAD" {
            "游离 HEAD".to_string()
        } else {
            branch.to_string()
        };
        h_flex()
            .h(px(22.0))
            .px(px(8.0))
            .rounded(px(R_MD))
            .bg(t.colors.muted)
            .items_center()
            .gap(px(GAP_TIGHT))
            .text_color(tier3(t))
            .child(icon_md(AssetIcon::GitBranch))
            .child(div().text_size(px(FS_SEC)).child(text))
    }
}

impl Shell {
    /// 磁盘摘要**一句话**（仓库头那枚徽标用）。
    ///
    /// 原来是概览页底排一张「磁盘占用」卡：一行结论 + 「查看」+ 重新扫描。
    /// 用户要求把统计**搬到仓库名后面**，所以卡整个撤了，只留这句话。
    ///
    /// **判据一个字没动**，只是从"卡的正文"变成"徽标的文案"——
    /// 扫不扫 / 扫出什么 / 有没有可清的，全在这一个函数里判。
    /// 完整数据（总占用 / Top 10 大文件 / 逐项清理 / `.gitignore` 建议 /
    /// 重新扫描 / 上一轮清理的结论）都在「空间」页签里，徽标点过去就是它。
    pub(crate) fn space_summary_line(&self) -> String {
        let rep = self.space.report.as_ref();
        let cache = rep
            .map(|r| r.dirs.iter().filter(|d| d.is_cache).map(|d| d.size).sum())
            .unwrap_or(0u64);

        // ⚠️ 判据是 `scanning && 还没有数`，**不是光看 `scanning`**（阶段 16）。
        // 后台刷新时库里那份已经在屏幕上了，这时候报"扫描中…"等于
        // 刚闪出来的数字立刻缩回去——缓存做的那点秒开全白费。
        if self.space.scanning && rep.is_none() {
            "扫描中…".to_string()
        } else if let Some(e) = &self.space.error {
            e.clone()
        } else if let Some(r) = rep {
            // 用 `human_bytes` 不用 `human_size`：清完缓存的小仓库会落到几十 KB，
            // `human_size` 一律折成 MB，那会儿标题上写的是「占用 0 MB」——
            // 一个还有源码的仓库显示成 0，读起来像"空仓库"
            if cache > 0 {
                format!(
                    "占用 {} · 可清理 {}",
                    space::human_bytes(r.total),
                    space::human_bytes(cache)
                )
            } else {
                format!("占用 {} · 没有可清理的构建缓存", space::human_bytes(r.total))
            }
        } else {
            "尚未扫描".to_string()
        }
    }
}

impl Shell {
    /// 活跃度迷你柱图：最近 8 个有提交的日子的提交数。
    /// 没提交的日子不补 0 柱——柱子数量本身就说明「这 30 天里几天在干活」；
    /// 扫描中 / 空仓库画一根矮的轨道色柱，别让 chip 里空一块。
    pub(crate) fn activity_chart(&self, t: &Theme) -> Div {
        const BARS: usize = 8;
        let counts: Vec<u32> = self
            .profile
            .activity
            .iter()
            // 后端按时间倒序（新的在前），迷你图也习惯新的在右：反转一下
            .rev()
            .take(BARS)
            .map(|d| d.count)
            .collect();
        let max = counts.iter().copied().max().unwrap_or(0).max(1) as f32;
        let n = counts.len();
        h_flex()
            .w(px(64.0))
            .h(px(16.0))
            .items_end()
            .gap(px(GAP_TIGHT))
            .children((0..BARS).map(|i| {
                let h = match counts.get(i) {
                    Some(&c) => 6.0 + (c as f32 / max) * 10.0,
                    None => 6.0,
                };
                div().flex_1().h(px(h)).rounded(px(R_SM)).bg(if i < n {
                    t.colors.accent
                } else {
                    track(t)
                })
            }))
    }
}

impl Shell {
    /// 语言构成条：按字节占比分段。**每种语言用它自己的 GitHub 色**
    /// （core 的 `repos::lang_color`），认不出来的才回落到规范板色板循环
    pub(crate) fn lang_bar(&self, t: &Theme) -> Div {
        let langs = &self.profile.langs;
        if langs.is_empty() {
            // 扫描中 / 空仓库：一根轨道色条，占位即可
            return h_flex()
                .w_full()
                .h(px(8.0))
                .rounded(px(R_SM))
                .overflow_hidden()
                .bg(track(t))
                .child(div().h_full().rounded(px(R_SM)).bg(track(t)));
        }
        let mut bar = h_flex()
            .w_full()
            .h(px(8.0))
            .rounded(px(R_SM))
            .overflow_hidden()
            .gap(px(GAP_DENSE))
            .bg(track(t));
        for (i, l) in langs.iter().enumerate() {
            bar = bar.child(
                div()
                    .h_full()
                    .rounded(px(R_SM))
                    .bg(lang_ink(&l.language, i, t))
                    .flex_grow(l.percent.max(0.5)),
            );
        }
        bar
    }

    /// 语言技术栈段：色带 + **下面一行图例（色点 · 名称 · 百分比）**。
    /// 概览页三段里的最后一段
    ///
    /// **图例这一行是它区别于「一条彩色进度条」的地方。** 只有色带的话
    /// 用户看到的是"这个仓库由几块颜色组成"，看不出哪块是什么；
    /// 加上名称与百分比，它才是"这个仓库主要用什么写的"
    pub(crate) fn lang_section(&self, t: &Theme) -> Div {
        let langs = &self.profile.langs;
        let mut legend = h_flex().w_full().flex_wrap().gap(px(GAP)).items_center();
        for (i, l) in langs.iter().enumerate() {
            let pct = (l.percent * 10.0).round() / 10.0;
            legend = legend.child(
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(GAP_TIGHT))
                    .child(div().w(px(8.0)).h(px(8.0)).rounded_full().bg(lang_ink(&l.language, i, t)))
                    .child(
                        div()
                            .text_size(px(FS_SEC))
                            .text_color(tier3(t))
                            .child(l.language.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .text_color(tier4(t))
                            .child(format!("{pct}%")),
                    ),
            );
        }

        crate::widgets::card(t)
            .flex_shrink_0()
            .gap(px(GAP_TIGHT))
            .child(
                div()
                    .text_size(px(FS_BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.colors.foreground)
                    .child("语言技术栈"),
            )
            .child(self.lang_bar(t))
            .when(langs.is_empty(), |col| {
                // 扫描中就说扫描中。摆一根空条加一个 0%，用户会以为这仓库没有代码
                col.child(meta_text(
                    if self.profile.scanning {
                        "统计中…"
                    } else {
                        "没统计出代码文件（仓库可能是空的，或全是配置 / 数据文件）"
                    },
                    t,
                ))
            })
            .when(!langs.is_empty(), |col| col.child(legend))
    }
}

/// 第 `ix` 种语言的墨色。**优先 core 的 GitHub 语言色**——TypeScript 就该是
/// 那个蓝，Rust 就该是那个浅棕；而规范板色板循环给的是"第几段用第几号颜色"，
/// 同一个仓库换个语言排序颜色就全变了，两屏对同一份数据也画得不一样
fn lang_ink(lang: &str, ix: usize, t: &Theme) -> Hsla {
    qingbird_core::repos::lang_color(lang)
        .map(|hex| Hsla::from(gpui_kit::rgba(hex)))
        .unwrap_or_else(|| match ix {
            0 => t.colors.chart_1,
            1 => t.colors.chart_2,
            2 => orange(t),
            3 => t.colors.chart_4,
            4 => t.colors.chart_5,
            _ => edge(t),
        })
}

impl Shell {
    /// 第三方同步卡**第三行**那一行只读档案明细（官方 `DescriptionList`）。
    ///
    /// **五项都是只读，不给任何一把键。** 第三方不是我的项目，这一行回答的是
    /// 「我看到的这份副本处在什么位置」，不是「下一步该做什么」——该做什么是上面
    /// 拉取 / 检测两把键的事。摆一排只读格子的另一个理由是替代被裁掉的那把
    /// 「提交并推送…」：少一把键总得少一份信息量，换个地方给回去
    ///
    /// **独占一行**而不是挤进第二行的按钮里——见调用处的注释（四列
    /// `DescriptionList` 会把右端两颗顶出画面）
    fn third_archive_row(&self, status: &RepoStatus, lg: &LogState, t: &Theme) -> Div {
        let branch = if status.branch.is_empty() || status.branch == "HEAD" {
            "游离 HEAD".to_string()
        } else {
            status.branch.clone()
        };
        let behind = match status.behind {
            0 => "没有落后".to_string(),
            n => format!("落后 {n} 个提交"),
        };
        // 「上次同步」这一格：**标签问的是跨会话，值就得答跨会话。**
        // 原来只有本会话的 `lg.ts` 可用，所以退成「本会话还没有同步过」——
        // 标签写着"上次"、值却说"本会话"，**一个格子自己跟自己打架**。
        // 阶段 16 落了 `op_history` 之后这格答得出来了
        let synced = if lg.ts != 0 {
            rel_time(lg.ts)
        } else {
            match self.selected.as_deref().and_then(|p| self.op_log_when(p)) {
                Some(when) => when,
                None => "还没有同步记录".to_string(),
            }
        };
        v_flex()
            .w_full()
            .child(
                DescriptionList::new()
                    .columns(4)
                    .label_width(px(64.0))
                    .with_size(Size::Small)
                    .item("当前分支", branch, 1)
                    .item(
                        "跟踪分支",
                        if status.upstream.is_empty() {
                            "没有上游".to_string()
                        } else {
                            status.upstream.clone()
                        },
                        1,
                    )
                    .item("是否落后", behind, 1)
                    .item("上次同步", synced, 1),
            )
            .child(
                div()
                    .pt(px(GAP_TIGHT))
                    .text_size(px(FS_META))
                    .text_color(tier4(t))
                    .child("第三方仓库只读：拉取与检测之外不提供任何写操作。"),
            )
    }
}
