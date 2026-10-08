//! 详情页内的各张卡片：分叉条 / 同步段 / 语言技术栈 / 磁盘摘要 / 活跃度图表。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::Disableable as _;
use gpui_kit::component::{Sizable, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, AnyElement, Context, Div, FontWeight, Hsla, IntoElement, ParentElement, SharedString,
    Styled,
};
use qingbird_core::types::RepoStatus;

use crate::detail::DetailTab;
use crate::dialogs::fork::fork_commit_block;
use crate::keys::{SyncCheck, SyncPull, SyncPush};
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
                        .text_size(px(fs_meta()))
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
                .p(px(GAP_XL))
                .rounded(px(r_xl(t)))
                .bg(red_bg(t))
                .border_1()
                // 这一圈边按 v4 §2 取 `line_soft`（卡/卡内盒一档），不取 `line`。
                // ⚠️ **原型 `.sync-alert` 根本没有四周一圈边**，只有
                // `border-left: 3px solid var(--warn)` + `--r-inner` 圆角 + `12/15` 内距
                // ——那是**改形态**（色条替代描边），不在这一轮擅动的范围，登记待他认
                .border_color(track(t))
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
                                .text_size(px(fs_body()))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.colors.foreground)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(fs_sec()))
                                .text_color(t.colors.danger)
                                .child(summary),
                        ),
                )
                .children(blocked.as_deref().map(|reason| {
                    div()
                        .text_size(px(fs_sec()))
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
    pub(crate) fn sync_card(
        &self,
        path: &str,
        t: &Theme,
        cx: &mut Context<Self>,
        lang_on: bool,
    ) -> Div {
        let view = self.view(path);
        let status = view.map(|v| v.status.clone()).unwrap_or_default();
        let changed = view.map(|v| v.changed).unwrap_or(0);
        let lg = self.logs.get(path).unwrap_or(&LOG_EMPTY);
        let busy = lg.running;
        let third = self.entry(path).map(|e| e.is_third_party).unwrap_or(false);
        let b = bucket_of(&status);
        // 状态句与告警**由同一个函数一起出**——判据只有一份（见 `sync_readout`）。
        // 这里只取主句；`alert` 为 `Some` 时主句降到中性灰，详情交给头卡那颗感叹号
        let (sentence, alert) = sync_readout(&status, b, lg);
        // 「上次同步」的值**跨会话**答：本会话没跑过就去那台仓库的历史里翻最后一次。
        // ⚠️ 读的是**这一屏的 `path`**，不是 `self.selected`——原来那一格拿的是
        // `selected`，两个不一致时（切了组没换选中、或反过来）会答成**另一台仓库**的记录。
        // 判据与 D78 同一条：**能被参数化的就别从"当前那个"里捞**
        let synced = match lg.ts {
            0 => self.op_log_when(path).unwrap_or_else(|| "还没有记录".into()),
            ts => rel_time(ts),
        };
        // 第三方那几段补充读数（方案 A，2026-10-08 用户点）：**并进状态句那一行**，
        // 不再独占一行四列。判据"只补主句没说的"写在 `third_bits` 里，由单测钉着
        let bits: Vec<(&'static str, String)> = if third {
            third_bits(&status, b, lg, &synced)
        } else {
            Vec::new()
        };

        // ⚠️ **「一屏只留一颗主按钮」——原型 `.sync-row` 上面那段注释就是这条**：
        // 「真正要用户做的是什么，primary 就给什么，其余一律降级」。
        // 原来这里是"三键里哪把该点给哪把"，而「提交并推送」是 ghost ⇒
        // 有未提交改动时，屏幕上最重的那颗键恰恰**不是**该做的事，
        // 而原型把 primary 给它。现在按同一条判据排优先级：
        //   有未提交改动 → 提交并推送；否则落后/分叉 → 拉取；否则待推送 → 推送；
        //   都不成立（已同步）→ 这一屏没有"该做的"，**一颗 primary 都不给**
        // （硬给一颗等于催用户点一个什么都不发生的东西——谎报干净那一族的镜像）
        let commit_hot = !third && changed > 0;
        // 落后与待推送是互斥的，分叉时两把都要（得先变基或拉取，推送也会被拒）
        let pull_hot = !commit_hot && (b == Bucket::Behind || b == Bucket::Forked);
        let push_hot = !commit_hot && !third && (b == Bucket::Ahead || b == Bucket::Forked);

        // tooltip 走官方 `tooltip_with_action`：它自己从已绑定的 keymap 里查
        // 出这颗键的 `Kbd` 画上去。**不要手写「（1）」进文案**——键位一改，
        // 手写的那份就成了假提示，而查出来的那份永远不会错
        //
        // ⚠️ **降级那两把一律 `.secondary().outline()`**：原型的 `.btn.ghost` 是
        // **白底 + 1px `line` 描边 + 13 字 + 14 图标**（探针量的：34 高 / padding 0 16 / gap 7）。
        // 只给 `.secondary()` 不够——theme 里 `secondary.background` 就是 #FFFFFF，
        // **白卡上的白底 = 看不见边框**，三键在帧上就是三个裸文字（他第⑧条
        // "跟原型相差太大"最先看到的就是这个）。官方 `Button::outline()`（`button.rs:328`）
        // 正是为这一档存在的：描边取色与 hover/active 都由基座自己管，
        // **不用换 `Custom`**（那是 Q12 里"重新设计那颗按钮"的量级，不在这一条）
        let key = |id: &'static str,
                   label: &'static str,
                   icon: AssetIcon,
                   hot: bool,
                   tip: String,
                   op: Op,
                   action: &dyn gpui_kit::Action,
                   lg: &LogState,
                   cx: &mut Context<Self>| {
            Button::new(SharedString::from(id))
                .label(label)
                .icon(icon)
                .tooltip_with_action(SharedString::from(tip), action, None)
                .when(hot, |btn| btn.primary())
                .when(!hot, |btn| btn.secondary().outline())
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
            // 三键各带一颗图标（原型 `.sync-row button > svg` 14×14：
            // 向下到线 = 拉取、向上出线 = 推送、放大镜 = 只看不动）。
            // **方向相反的两个字最容易看错**，而点错的代价是一次真的网络操作
            AssetIcon::ArrowDownToLine,
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
        // 第三方没有推送键 ⇒ **不画推送，也不给它留格子**。
        // 早先这里塞了一个 `PUSH_KEY_W`(76) 的透明 div，理由是"别让「检测」跳到
        // 第二个位置，而 1·2·3 是按位置对位的"——那条理由站不住：
        // **键位提示是 `tooltip_with_action` 从 keymap 查出来画在 tooltip 上的**，
        // 不靠"第几颗"对位；而留下的洞是**肉眼看得见的 84px 空白**（76 + gap 8），
        // 用户看到的只是"这两颗键中间怎么空了一块"（2026-10-08 实机报的那条）。
        // ⇒ 占位守的是一个看不见的约定，赔的是一屏看得见的空白
        if !third {
            keys = keys.child(key(
                "sync-push",
                SYNC_SLOT_LABELS[1],
                AssetIcon::ArrowUpFromLine,
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
            AssetIcon::Search,
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
            // 第一行 = 原型 `.sync-row`：**一行装完**（探针量的：34 高、gap 9、
            // 三键 81 宽 / padding 0 16 / 图标 14 / 字 13）。
            // 原来这里排的是**两行**——第一行整行只有一句状态话 + 一颗「定时同步」
            // 文字键，第二行才是三键。他第⑧条「跟原型相差太大」看到的正是这个：
            // 一屏最该先看到的操作被压到第二行，而第一行那句"与云端已同步"
            // 白占一行还什么都不是（原型那一屏**没有**这一行——
            // 它把待推送/未提交放进了头卡那四格）
            .child(
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(GAP))
                    .child(keys)
                    .child(div().flex_1())
                    // 顺序照原型：**先"看清单"（说的是"有什么"）→ 再"提交并推送"
                    // （说的是"该做什么"，primary 在它身上）→ 最后那颗图标键**。
                    // 原来"提交并推送"排在"看清单"前面，读起来是先做事再看有什么事
                    // 变更摘要兼**入口**：变更是中栏的一个页签了，
                    // 而概览页原来那张「未提交的改动」卡已经删掉。
                    // 这一行是概览页上唯一提到未提交改动的地方，
                    // 留一个可点的入口才不会让人以为要自己去页签栏找。
                    // **用官方 `Button` 而不是给文字挂 `on_click`**：
                    // 纯 div 的命中区只有那行字，悬停底色还得自己在 `Shell`
                    // 上存一个状态字段来回传
                    .when(changed > 0, |row| {
                        row.child(
                            Button::new("sync-changes-jump")
                                .label(format!("{changed} 个文件待提交 · 看清单"))
                                .custom(ghost_variant(cx))
                                .with_size(Size::Small)
                                .tooltip("打开「变更」页签，逐个看未提交的文件")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.open_detail_tab(DetailTab::Changes, cx)
                                })),
                        )
                    })
                    // 「提交并推送…」与「提交是改自己的历史」绑在一起：第三方不出现。
                    // 它那一格空出来的位置由**下面那行**的只读档案明细接手——
                    // 少一把键总得少一份信息量，换个地方给回去，而不是让这一行空着
                    .when(!third, |row| {
                        row.child(
                            Button::new("sync-commit")
                                .label("提交并推送")
                                // ⚠️ **图标在文字右边**（原型 `.btn.primary` 就是
                                // 文字 + 一颗向上箭头排在后面），而官方 `Button::icon`
                                // 只给"图标在左"那一个位置。要放右边得用 `icon_with_text`
                                // 那类内部件 ⇒ 这一条**先不落**，落之前要查清官方有没有
                                // 右侧图标的口；现在按"图标在左"给，形态上仍比裸文字强
                                .icon(AssetIcon::ArrowUp)
                                .when(commit_hot, |b| b.primary())
                                .when(!commit_hot, |b| b.secondary().outline())
                                .with_size(Size::Medium)
                                .disabled(busy)
                                .tooltip("先提交本地改动，再推到云端")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_commit_dlg(None, window, cx)
                                })),
                        )
                    })
                    // 「定时同步」从一颗文字键降成**图标键**（原型 `.btn.ghost.icon`
                    // 34×34、只有一颗时钟）。它是这一屏的**设置入口**不是动作，
                    // 与三键并排摆文字会抢读；tooltip 仍把"间隔 / 每天定时 /
                    // 检测后自动拉取"说全，图标不猜
                    .child(
                        Button::new("sync-timer")
                            .icon(AssetIcon::Clock)
                            // ⚠️ 与三键**同一种重量**（原型这颗就是 `.btn.ghost.icon`
                            // = 白底 + 1px 描边 34×34，不是透明那档）。
                            // 用 `.ghost()` 会把它降成透明——同一行里"带边的方框"
                            // 与"没边的方框"混着摆，就是他说的"大杂烩"那种形态
                            .secondary()
                            .outline()
                            .with_size(Size::Medium)
                            .tooltip("后台定时自动检测并按设置处理（间隔 / 每天定时 / 检测后自动拉取）")
                            .accessibility_label("定时同步设置")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_sync_dlg(window, cx)
                            })),
                    ),
            )
            // 第二行（**只在"有话说"时才有**）：状态句 + 第三方的补充读数。
            //
            // 判据是 `!bits.is_empty() || alert.is_some() || b != Bucket::Synced`。
            // 比原来多一项 `bits`：第三方即使已经同步，那句里还有「跟踪 origin/main」
            // 要说——档案行撤掉之后，**这一行就是它唯一的落点**，
            // 再让"同步了就不出现"把整行藏掉，等于把那份信息静默丢了（D73 那一族）。
            //
            // ⚠️ 主句原来挂 `.flex_1().truncate()`：那是"这一行只有主句"时的写法。
            // 现在后面要跟几段读数，`flex_1` 会把它们**整排推到最右边**，
            // 读起来成"右边那一堆跟这句话没关系"。⇒ 主句按内容宽排，
            // 整行 `flex_wrap()`：窗口窄到放不下时折第二行，而不是裁掉尾巴
            .when(!bits.is_empty() || alert.is_some() || b != Bucket::Synced, |col| {
                col.child(
                    h_flex()
                        .w_full()
                        .flex_shrink_0()
                        .gap(px(GAP_TIGHT))
                        .flex_wrap()
                        .items_center()
                        .child(
                            div()
                                .text_size(px(fs_sec()))
                                .font_weight(FontWeight::MEDIUM)
                                // ⚠️ 有告警时主句**降到中性灰**（`tier3`）：详情已经进
                                // 头卡那颗感叹号的气泡了，这里再穿前景色 + 占最主位的字，
                                // 就是"一条提示压过这一屏的操作"——用户不要的那个形态
                                .text_color(if alert.is_some() {
                                    tier3(t)
                                } else {
                                    body(t)
                                })
                                .child(sentence),
                        )
                        // 补充段：` · 标签 值`，标签为空的段（"没有落后"）只有值。
                        // **标签走 `tier4`、值走 `body`** —— 与这一屏其它"键 + 值"
                        // 的分工同一套（淡的是脚手架，墨的才是内容）
                        .children({
                            let mut segs: Vec<AnyElement> = Vec::new();
                            for (label, value) in bits.iter() {
                                segs.push(
                                    div()
                                        .text_size(px(fs_sec()))
                                        .text_color(tier4(t))
                                        .child("·")
                                        .into_any_element(),
                                );
                                if !label.is_empty() {
                                    segs.push(
                                        div()
                                            .text_size(px(fs_sec()))
                                            .text_color(tier4(t))
                                            .child(label.to_string())
                                            .into_any_element(),
                                    );
                                }
                                segs.push(
                                    div()
                                        .text_size(px(fs_sec()))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(body(t))
                                        .child(value.clone())
                                        .into_any_element(),
                                );
                            }
                            segs
                        }),
                )
            })
            // 只读声明（**只有第三方有**）：撤掉档案行之后这一句留下，
            // 它是"这一类仓库不给写"的规则说明，不是读数，没有别处说它
            .when(third, |col| {
                col.child(
                    div()
                        .w_full()
                        .text_size(px(fs_meta()))
                        .text_color(tier4(t))
                        .child("第三方仓库只读：拉取与检测之外不提供任何写操作。"),
                )
            })
            // **语言构成并进这张卡**（原型：`.sync-row` → `.card-split` →
            // `.mini-head` → `.lang-bar` → `.legend`，全在同一张 `.card` 里）。
            // 可见性由调用方把 `MOD_SEC_LANG` 的判定结果传进来，**不在这里读 store**：
            // "这一屏显示哪几段"的判据只有 `overview_body` 那一处，两处各读一次迟早漂
            .when(lang_on, |col| {
                col.child(crate::widgets::card_split(t))
                    .child(self.lang_section(t))
            })
    }
}


/// 同步卡上那句话。同一个分档在环与这句里说的是同一件事，
/// 别让两处各写一套文案
/// 同步卡上那句**状态话**，外加一条**该不该到头卡那颗感叹号里去说**的告警。
///
/// ⚠️ **一个函数同时出两样，是刻意的**：状态句与告警句必须来自**同一份判据**。
/// 拆成两个函数各 `if` 一遍，迟早会漂成"卡里说与云端已同步、头卡上挂着
/// 未配置上游的感叹号"——**界面自己打自己的脸**，而编译器与单测都不报（D78 那一族）。
///
/// 阶段 31.4 之前，`!is_repo` / `!has_upstream` 这两条是**直接当主句摆在同步卡第一行**的
/// （"没有配置上游分支，同步操作会失败"）。用户 2026-10-06 明确不要那种形态：
/// 一条提示不该占住这一屏最主位的字。⇒ 现在主句只**中性陈述**（降到 `tier3` 灰），
/// 详情进头卡仓库名后面那颗 `warn_dot` 的气泡里。
pub(crate) fn sync_readout(s: &RepoStatus, b: Bucket, lg: &LogState) -> (String, Option<String>) {
    if !s.is_repo {
        return (
            "未识别为 Git 仓库".into(),
            Some("这个文件夹里没有 .git，同步三键都不会有结果。".into()),
        );
    }
    if !s.has_upstream {
        return (
            "未配置上游分支".into(),
            Some(
                "当前分支没有上游，拉取会失败。可以先设置远端，\
                 或直接用「推送」把本地分支推上去建立 origin/分支名。"
                    .into(),
            ),
        );
    }
    let sentence = if lg.running {
        match lg.op {
            Some(Op::Pull) => "正在拉取云端更新…".into(),
            Some(Op::Push) => "正在推送本地提交…".into(),
            Some(Op::Check) => "正在检查云端更新…".into(),
            Some(Op::Commit) => "正在提交并推送…".into(),
            _ => "正在同步…".into(),
        }
    } else {
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
    };
    (sentence, None)
}

/// 第三方那一屏要**并进状态句**的补充段：`(标签, 值)`，标签为空串就是不用标签的短语。
///
/// 2026-10-08 用户挑的方案 A。原来这四样是独占一行的四列 `DescriptionList`，
/// 量出来两处硬伤：① `label_width(64)` 装不住四个 13px 汉字（字面 52 + 组件自己的
/// 内边距），四格**全部折成两行**、每格高 31；② 那四块 label 底实测 #E5EAEE，
/// 比设计板的 `card-sunken`（#F4F7F8）深 15/255——theme.json 没配 `DescriptionList`
/// 读的那个键，落到基座兜底值了（D60 那一族的第二次）。⇒ 整块撤掉，并成一句话。
///
/// ⚠️ **判据是「只补主句没说的」**，不是"把四样都搬过来"：
/// 主句在 `Behind` / `Forked` 时已经报了落后数，这里就**不再说"没有落后"**；
/// 主句在 `Synced` 时自带「· 上次同步 X」，这里也**不再补**。
/// 两处各说一遍就是这一路一直在消的那个毛病，而**同一句话里说两遍**更糟——
/// 它一眼看去像"有两个上次同步"。所以这条判据由单测钉住，不靠注释。
///
/// ⚠️ **「当前分支」不进这一句**：头卡仓库名右边那颗 `main` 分支胶囊已经在说它
/// （`branch_badge`），这里再说一遍就是重复。**去掉的那一样由别处承担，不是丢信息**。
pub(crate) fn third_bits(
    s: &RepoStatus,
    b: Bucket,
    lg: &LogState,
    synced: &str,
) -> Vec<(&'static str, String)> {
    // 正在跑的时候主句是「正在拉取…」，后面跟一串静态档案读起来像"边跑边改档案"
    if lg.running {
        return Vec::new();
    }
    let mut out: Vec<(&'static str, String)> = Vec::new();
    // 只有 `Ahead` 这一档主句没提落后（`Behind` / `Forked` 都提了，`Synced` 不必提）
    if b == Bucket::Ahead {
        out.push(("", "没有落后".into()));
    }
    if s.has_upstream && !s.upstream.is_empty() {
        out.push(("跟踪", s.upstream.clone()));
    }
    // `Synced` 那一档主句自己带「· 上次同步 X」，这里再给就是同一句里说两遍
    if b != Bucket::Synced {
        out.push(("上次同步", synced.to_string()));
    }
    out
}

impl Shell {
    /// 分支徽章：Surface 底 + 分支图标
    pub(crate) fn branch_badge(&self, branch: &str, t: &Theme) -> Div {
        let text = if branch.is_empty() || branch == "HEAD" {
            "游离 HEAD".to_string()
        } else {
            branch.to_string()
        };
        // **走全站唯一那份胶囊实现**（`widgets::pill_out`）。这里原来自己手搓了
        // 第四种写法：`R_MD` 方角 + `muted` 实底 + `fs_sec` 字 + `icon_md` 图标——
        // 既不在 `pill_h()` 那一档也不在全圆角那一档，与它左边那枚「我的」并排时
        // 一眼就是两种东西。分支该读成"身份"而不是"状态" ⇒ 描边那一档
        crate::widgets::pill_out(Some(AssetIcon::GitBranch), &text, t)
    }
}

// （这里原来挂着 `impl Shell { fn space_summary_line(..) }`——仓库头那枚磁盘摘要
// 徽标的文案。阶段 31.4 补五把那几个读数收进头卡那四格 `head_metrics`，
// 徽标整个删掉，这句话也就没有读者了。**不留 `#[allow(dead_code)]`**：
// 留着的话下一轮会有人以为它还有用，跑去改它的措辞）

/// 活跃度那一句结论（头卡页签行右边那句，原型「近 30 天 11 天有提交 · 共 61 次」）。
///
/// **抽成纯函数有三个理由**，都指向同一个毛病：
/// ① 这句话原来在 `repo_head::activity_chip` 里就地拼，而那一枚徽标本轮被撤
///    （它的两样东西——柱子与这句话——搬到了页签行）⇒ 不留这一份，
///    下一轮会有人在两处各拼一遍措辞，一处改字另一处跟着漂；
/// ② 它现在**两类仓库都要显示**，写死在"第三方独有"的那段里就接不上；
/// ③ 句式是单测唯一能钉住的东西——抽出来之前它只能靠抓帧看，
///    而抓帧只能证明"渲染成那样了"（AGENTS「凡是要靠 `&self` 才能调的显示判据，
///    抽成纯函数再单测」那条的同款落法）。
///
/// ⚠️ **"没扫过"与"扫过、真的是零"是两件事**（本项目那条老教训的第四种触发）：
/// 还在扫且一个数都没有 ⇒ 说「统计中…」；扫完 0 次 ⇒ 说「近 30 天没有提交」。
/// 把"没数"写成"没有提交"就是谎报干净那一族的镜像——谎报"没干活"。
pub(crate) fn activity_note(days: usize, commits: u32, scanning: bool) -> String {
    if scanning && commits == 0 && days == 0 {
        "近 30 天统计中…".to_string()
    } else if commits == 0 {
        "近 30 天没有提交".to_string()
    } else {
        format!("近 30 天 {days} 天有提交 · 共 {commits} 次")
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
                            .text_size(px(fs_sec()))
                            .text_color(tier3(t))
                            .child(l.language.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(fs_meta()))
                            .text_color(tier4(t))
                            .child(format!("{pct}%")),
                    ),
            );
        }

        // ⚠️ **这一段没有卡外壳**：它现在是**同步卡里的一段**（原型 `.card` 内
        // `card-split` + `mini-head` + `lang-bar` + `legend`），不是第三张卡。
        // 原来它自己 `card(t)` 一份 ⇒ 一屏三张卡，而它只有两行内容，
        // 却要多付一次「卡间距 + 上下内边距 + 表头」的垂直开销——
        // 抓帧实锤的后果是日志卡 `flex_1` 把剩余高度全吃掉、这张被压到屏底只露一条
        v_flex()
            .w_full()
            .flex_shrink_0()
            .gap(px(GAP_TIGHT))
            .child(crate::widgets::mini_head(
                "语言技术栈",
                Some("按文件字节数"),
                t,
            ))
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

// （这里原来挂着 `impl Shell { fn third_archive_row(..) }`——第三方那一行四列
// `DescriptionList` 的只读档案。2026-10-08 用户挑的方案 A 把那四样并进状态句，
// 整块连同它的 `label_width(64)` 折行与基座兜底灰底一起删除，**不留转发层**。
// 唯一的例外是那句「第三方仓库只读」，它还在，见 `sync_card` 里的 `.when(third, ..)`）

#[cfg(test)]
mod tests {
    use super::{activity_note, third_bits, Bucket, LogState, RepoStatus};

    /// 造一份状态。`upstream` 传空串就是"没有上游"
    fn st(ahead: u32, behind: u32, upstream: &str) -> RepoStatus {
        RepoStatus {
            is_repo: true,
            branch: "main".into(),
            ahead,
            behind,
            dirty: false,
            has_upstream: !upstream.is_empty(),
            upstream: upstream.into(),
            remote_url: String::new(),
        }
    }

    /// 把段拼成人读的一行，断言时直接看句子本身而不是看元组
    fn line(bits: Vec<(&'static str, String)>) -> String {
        bits.iter()
            .map(|(k, v)| {
                if k.is_empty() {
                    v.clone()
                } else {
                    format!("{k} {v}")
                }
            })
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// 方案 A 的判据：**只补主句没说的**。
    ///
    /// 这一条是整块改动的全部风险所在——档案行并进状态句之后，
    /// 同一句话里出现两遍「上次同步」或「落后」，读起来就像有两个时钟、
    /// 两个落后数，而**它一眼看去完全正常**：`cargo check` 零错、抓帧也只是
    /// "这句话长了一点"。⇒ 逐档钉死，一档一个断言。
    #[test]
    fn third_bits_fills_only_what_the_main_sentence_does_not_say() {
        // `Ahead`：主句是「本地有 N 个提交待推送」，没提落后、也没提上次同步 ⇒ 三样都补
        assert_eq!(
            line(third_bits(&st(33, 0, "origin/main"), Bucket::Ahead, &LogState::default(), "还没有记录")),
            "没有落后 · 跟踪 origin/main · 上次同步 还没有记录"
        );
        // `Synced`：主句自带「· 上次同步 X」⇒ 这里**绝不**再给一次，只剩跟踪
        assert_eq!(
            line(third_bits(&st(0, 0, "origin/main"), Bucket::Synced, &LogState::default(), "3 天前")),
            "跟踪 origin/main"
        );
        // `Behind`：主句已经报了落后数 ⇒ "没有落后"是**谎话**，不能给
        assert_eq!(
            line(third_bits(&st(0, 4, "origin/main"), Bucket::Behind, &LogState::default(), "昨天")),
            "跟踪 origin/main · 上次同步 昨天"
        );
        // `Forked`：同上，两个数主句都说了
        assert_eq!(
            line(third_bits(&st(2, 4, "origin/main"), Bucket::Forked, &LogState::default(), "5 小时前")),
            "跟踪 origin/main · 上次同步 5 小时前"
        );
        // 没有上游：主句走「未配置上游分支」那一档 ⇒ 不给"跟踪"（值本来就没有）
        assert_eq!(
            line(third_bits(&st(1, 0, ""), Bucket::Ahead, &LogState::default(), "还没有记录")),
            "没有落后 · 上次同步 还没有记录"
        );
    }

    /// **正在跑的时候一段都不给。**
    /// 主句那时是「正在拉取云端更新…」，后面跟一串静态档案会读成
    /// "边跑边把结果写进档案"，而那几个数其实还是上一次的了
    #[test]
    fn third_bits_says_nothing_while_an_op_is_running() {
        let lg = LogState {
            running: true,
            ..Default::default()
        };
        assert!(third_bits(&st(33, 0, "origin/main"), Bucket::Ahead, &lg, "3 天前").is_empty());
    }

    /// 头卡页签行右边那句活跃度结论。
    ///
    /// 钉三件事，每件都对应本项目为它交过学费的地方：
    /// ① **句式**——「近 30 天 N 天有提交 · 共 M 次」是原型那句，
    ///    不钉住就会被下一次"顺手改个措辞"漂走（AGENTS：改了文案要回头搜
    ///    认旧措辞的判据，同理句式进了单测才算有主）；
    /// ② **"还没扫"不说成"没有提交"**——那是"没扫过不许写成已同步"那一族的镜像；
    /// ③ **有数据就不许还说统计中**——阶段 16 抓出来的老账（库里那份已上屏时
    ///    界面仍报"统计中…"）。
    #[test]
    fn activity_note_says_the_window_and_the_two_counts() {
        assert_eq!(activity_note(11, 61, false), "近 30 天 11 天有提交 · 共 61 次");
    }

    #[test]
    fn activity_note_does_not_claim_no_commits_while_still_scanning() {
        // 一个数都还没回来：说"统计中"，不能说"没有提交"（谎报"这项目没干活"）
        assert_eq!(activity_note(0, 0, true), "近 30 天统计中…");
    }

    #[test]
    fn activity_note_reports_a_quiet_repo_only_after_the_scan_lands() {
        assert_eq!(activity_note(0, 0, false), "近 30 天没有提交");
        // 扫完了、有提交：哪怕还在扫下一轮，也不许退回"统计中"
        assert_eq!(activity_note(3, 7, true), "近 30 天 3 天有提交 · 共 7 次");
    }
}
