//! ⌘K 命令面板（官方 `command::{Command, CommandState, CommandGroup, CommandItem}`）。
//!
//! 这一层在设计文档里是「低频层」的落点：不常用但必须存在的动作
//! （添加 / 克隆 / 设置 / 打开文件夹 / 开终端 / 重扫 / 切仓库 / 批量同步）
//! 统一挂在这儿。**主面板与导航轨都不许再为它们加图标按钮**——
//! 上一版界面最大的问题就是轨上一排图标分不清哪些是切屏、哪些是动作。
//!
//! ## 为什么不用官方的 `CommandItem::action`
//!
//! 官方那一路要一个 `Box<dyn Action>`，而 Action 带 payload 需要
//! `serde::Deserialize` + `schemars::JsonSchema`。这个 GUI crate 里的 Action
//! 全是 `#[action(namespace = qingbird_gui, no_json)]` 的 unit struct
//! （`ToggleSidebar` / `StopOp` …），为一堆"切到某个仓库"的动作把这整套
//! serde + schemars 链拖进来不划算。
//!
//! 改走官方的另一条路：**渲染时把"第几组第几项 = 什么动作"存进
//! `Shell::palette_actions`，回车时用 `on_confirm` 给的 `IndexPath` 取回来**。
//! 这也正是 `on_confirm` 存在的理由——它给的就是**原始下标**，
//! 过滤不会改变它（见官方 `Command::on_select` 的注释）。
//!
//! ## 分组下标是硬约定
//!
//! 官方的算法是 `section_ix = group_ix + usize::from(has_ungrouped_items)`：
//! 只要**混进任何一条 `.item()` 或 `.separator()`**，所有分组的下标会整体错开一格，
//! 回车就会执行隔壁那条。所以这里**只建 `CommandGroup`，一条散项都不加**。
//! `G_*` 常量与 `palette_actions` 的行号都靠这条约定对齐，改的时候一起改。

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::v_flex;
use gpui_kit::component::command::{Command, CommandGroup, CommandItem};
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::empty::{Empty, EmptyContent, EmptyHeader, EmptyTitle};
// `Sizable` 是 `Icon::with_size` 的来源；`Size` 与 `px` 用于写死 16px 那一档
use gpui_kit::component::{ActiveTheme, Disableable as _, Icon, Sizable as _, Size, Theme, WindowExt};
use gpui_kit::{div, px, App, Context, Focusable as _, ParentElement, Styled, Window};

use qingbird_core::run_cmd::{Slot, MAX_PINNED_CMDS};

use crate::os;
use crate::sidebar::bucket_icon;
use crate::state::*;
use crate::tokens::*;

// 分组下标。**只建 group、绝不混 `.item()` / `.separator()`**（见模块头）
//
// **「切到仓库」排在最后**，不是随手排的：它是唯一长度随仓库数增长的一组，
// 17 个仓库就能把面板的可视高度吃光。排第一时，打开 ⌘K 看到的
// 只有一屏仓库名，其余四组要滚过 17 行才够得着——而那四组才是低频层
// 真正要承载的固定命令（2026-10-03 实机首轮）。
// 仓库名靠搜索找（每个仓库的关键词里带名字 / 别名 / 路径 / 分支 / 归属），
// 不靠滚列表找，所以它放最后不吃亏。
const G_LOW: usize = 0;
const G_CUR: usize = 1;
const G_BATCH: usize = 2;
const G_VIEW: usize = 3;
/// CLI 工具那一组（阶段三：重新扫描 / 升级全部 / 清理共享缓存）。
/// **排在「切到仓库」之前**——它也是固定长度的一组
const G_CLI: usize = 4;
const G_REPO: usize = 5;

/// 面板里一条命令对应什么动作
#[derive(Clone)]
pub(crate) enum PaletteAction {
    /// 切到某个仓库（走 `reveal_repo`，与侧栏点行 / 总览点行同一条路）
    Repo(String),
    /// 「当前仓库」组里那几条。都用 `self.selected`，不带 payload
    Sync(Op),
    /// 跑当前仓库的某条命令。**2026-10-05 新增**：概览页只常显两把键
    /// （开发运行 / 编译构建），其余槽位失去了常驻的运行入口——
    /// 能配不能跑是残废的，⌘K 就是它们该在的地方
    RunSlot(Slot),
    /// 编辑四个槽位的命令 / 钉住的自定义命令。概览页那颗「编辑」键随
    /// 「常用命令」段一起撤了（同一对入口摆两遍），改入口落在这里——
    /// **能跑不能改同样是残废的**
    EditCmds,
    CommitPush,
    MoveGroup,
    Rename,
    EditRepo,
    RemoveRepo,
    /// 三条固定批量命令之一
    Batch(BatchKind),
    /// 导航轨那三个按钮
    Nav(Nav),
    AddRepo,
    CloneRepo,
    Settings,
    /// 打开当前选中仓库的文件夹
    OpenFolder,
    /// 在当前选中仓库开终端（`Ctrl T` 的同一个入口）
    OpenTerminal,
    Rescan,
    /// 看上次批量同步的逐仓库结果（`app_log` 唯一的读出口）
    BatchLog,
    ToggleSidebar,
    ToggleInspector,
    /// 重扫 CLI 工具（阶段一；CLI 屏左栏底部那颗键的键盘路径）
    ScanCli,
    /// 升级全部可更新的 CLI 工具（阶段三）。**逐颗升级**在详情卡那颗键上
    UpgradeAllCli,
    /// 清理共享缓存（阶段三；⌘K 是**整批**那条路，逐颗的在汇总条每行尾巴）
    CleanSharedCli,
}

impl Shell {
    /// 打开命令面板。
    ///
    /// 用官方 `Dialog` 承载而不是自己叠浮层：Esc、点遮罩关闭、焦点归位、
    /// ↑↓ 与回车的键位绑定都在官方 Dialog + `CommandState::init` 里，
    /// 自己拼一层就等于把这些都重做一遍。
    ///
    /// 焦点延到下一帧给查询框：`Dialog` 的 content 闭包在它自己那层渲染里，
    /// 此刻命令还没装进 `CommandState`，当场 focus 拿不到输入控件
    pub(crate) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            return;
        }
        self.palette_open = true;
        let state = self.palette.clone();
        let focus_state = self.palette.clone();
        // 上一次的查询词必须清掉：不然用户以为面板是空的，回车却直接切了仓库
        state.update(cx, |st, cx| st.set_query("", window, cx));

        let me = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let t = cx.theme().clone();
            dialog
                .title("命令面板")
                .on_close({
                    // Esc / 点遮罩走这条路。`on_confirm` 那条路自己也会置
                    // false（`close_dialog` 会不会回调 `on_close` 不作依赖）
                    let me = me.clone();
                    move |_, _, cx| {
                        _ = me.update(cx, |this, cx| {
                            this.palette_open = false;
                            cx.notify();
                        });
                    }
                })
                .content({
                    let me = me.clone();
                    let state = state.clone();
                    move |c: DialogContent, _window, cx: &mut App| {
                        // 每帧重建清单：仓库列表、分档、批量筛出的个数都会变，
                        // 而官方的 `Command` 本来就是"每次渲染把条目装进状态"
                        let groups = me.update(cx, |this, _| this.palette_model(&t));
                        let cmd = with_groups(
                            Command::new(&state)
                                .bordered(false)
                                .placeholder("搜命令、仓库名或路径")
                                .max_h(px(420.0))
                                .empty(|_, _, _| {
                                    v_flex()
                                        .w_full()
                                        .items_center()
                                        .gap(px(GAP))
                                        .py(px(GAP_LG * 2.0))
                                        .child(
                                            Empty::new()
                                                .border_0()
                                                .header(EmptyHeader::new().title(
                                                    EmptyTitle::new().child("没有匹配的命令"),
                                                ))
                                                .content(EmptyContent::new().child(
                                                    div().child("换个词试试，或先扫一遍仓库"),
                                                )),
                                        )
                                })
                                .on_confirm({
                                    let me = me.clone();
                                    move |ix, window, cx| {
                                        let Some(act) = me.read(cx).palette_action(ix) else {
                                            return;
                                        };
                                        // `Entity::update` 在这个版本直接给回值，
                                        // 拿不到 Result。执行失败由各分支自己
                                        // `push_notification` 报，不在这里吞
                                        me.update(cx, |this, cx| {
                                            this.palette_open = false;
                                            this.run_palette(act, window, cx);
                                        });
                                        // 先关自己再开下一个：官方 Root 的
                                        // `open_dialog` 专门处理了"刚关就开"的
                                        // 焦点链（`pending_focus_restore`），
                                        // 顺序反了会把焦点丢在已销毁的对话框上
                                        window.close_dialog(cx);
                                    }
                                }),
                            groups,
                        );
                        c.child(cmd)
                    }
                })
        });

        // `CommandState::focus` 要一个 `&mut App`，而 `read()` 的守卫还攥着
        // `&App`——两步放同一个语句里会撞借用。所以先把 `FocusHandle` 取出来
        // （它是拥有所有权的值，守卫在这一行结束时就放了），再单独 focus。
        // 走 `Focusable` 而不是 `focus()`：前者顺带尊重 `searchable`
        window.defer(cx, move |window, cx| {
            let fh = focus_state.read(cx).focus_handle(cx);
            fh.focus(window, cx);
        });
    }

    /// 按 `IndexPath` 取动作。官方给的 `section` 是**分组号**（只建 group 时
    /// 恒等于组下标），`row` 是**组内下标**——过滤不改变这两个值
    pub(crate) fn palette_action(
        &self,
        ix: gpui_kit::component::IndexPath,
    ) -> Option<PaletteAction> {
        self.palette_actions
            .get(ix.section)
            .and_then(|g| g.get(ix.row))
            .cloned()
    }

    /// 回车之后干什么。**每条路径都与主面板的按钮走同一个函数**，
    /// 不在这里另写一份拉取/推送逻辑
    fn run_palette(&mut self, act: PaletteAction, window: &mut Window, cx: &mut Context<Self>) {
        match act {
            PaletteAction::Repo(path) => self.reveal_repo(path, window, cx),
            // 与 `1·2·3` 共用同一个转发口（`hotkey_sync`）：⌘K 与快捷键
            // 不该是两份拉取逻辑
            PaletteAction::Sync(op) => self.hotkey_sync(op, window, cx),
            // 与概览页那两颗键、日志表头那两颗走同一个 `run_slot`，
            // 不在这里另写一份跑命令的管道
            PaletteAction::RunSlot(slot) => self.run_slot(slot, cx),
            PaletteAction::CommitPush => self.open_commit_dlg(None, window, cx),
            PaletteAction::MoveGroup => {
                if let Some(p) = self.selected.clone() {
                    let third = self.selected_is_third();
                    self.set_group(&p, !third, window, cx);
                }
            }
            PaletteAction::Rename => {
                if let Some(p) = self.selected.clone() {
                    self.open_rename_dlg(p, window, cx);
                }
            }
            PaletteAction::EditRepo => {
                if let Some(p) = self.selected.clone() {
                    self.open_edit_dlg(p, window, cx);
                }
            }
            PaletteAction::EditCmds => {
                if let Some(p) = self.selected.clone() {
                    self.open_cmds_dlg(p, window, cx, None);
                }
            }
            PaletteAction::RemoveRepo => {
                if let Some(p) = self.selected.clone() {
                    self.remove_repo_undoable(&p, window, cx);
                }
            }
            PaletteAction::Batch(kind) => self.start_batch(kind, window, cx),
            PaletteAction::Nav(nav) => self.go(nav, window, cx),
            PaletteAction::AddRepo => self.open_add_dlg(window, cx),
            PaletteAction::CloneRepo => self.open_clone_dlg(window, cx),
            PaletteAction::Settings => self.open_settings_dlg(window, cx),
            PaletteAction::OpenFolder => match self.selected.clone() {
                Some(p) => notify_os(os::open_folder(&p), window, cx),
                None => window.push_notification("先选一个仓库", cx),
            },
            PaletteAction::OpenTerminal => match self.selected.clone() {
                Some(p) => notify_os(os::open_terminal(&p), window, cx),
                None => window.push_notification("先选一个仓库", cx),
            },
            PaletteAction::Rescan => {
                if !self.scanning {
                    self.refresh_status(cx);
                }
            }
            PaletteAction::BatchLog => self.open_batch_log_dlg(window, cx),
            PaletteAction::ToggleSidebar => self.toggle_sidebar(window, cx),
            PaletteAction::ToggleInspector => self.toggle_inspector(cx),
            // 与 CLI 屏左栏底部那颗「重新扫描」走同一个入口
            PaletteAction::ScanCli => self.scan_cli(cx),
            // 与详情卡那颗「升级」走同一个编排（`upgrade_cli_pkg` 的单包版）
            PaletteAction::UpgradeAllCli => self.upgrade_all_cli(cx),
            // 与汇总条「全部清理…」走同一个框（`open_cli_clean_dlg` 的整批版）
            PaletteAction::CleanSharedCli => {
                let targets = self.cli.shared.clone();
                self.open_cli_clean_dlg(targets, window, cx);
            }
        }
    }

    // ---------- 清单 ----------

    /// 组出这一帧的面板内容，并**顺手把动作清单存进 `palette_actions`**。
    ///
    /// 返回值只给渲染用；回车时读的是 `self.palette_actions`。
    /// 两者在同一个函数里产出，所以"界面上第 N 行"与"回车执行第 N 个动作"
    /// 不可能对不上
    pub(crate) fn palette_model(&mut self, t: &Theme) -> Vec<CommandGroup> {
        let mut groups: Vec<CommandGroup> = Vec::new();
        let mut acts: Vec<Vec<PaletteAction>> = Vec::new();
        debug_assert_eq!(groups.len(), G_LOW);

        // ---- 低频动作 ----
        // 「打开文件夹」「在终端中打开」「折叠侧栏」这几项在**没有选中仓库**或
        // 不在仓库页时没有意义，置灰而不是隐藏：藏了会让用户搜不到，
        // 以为这个功能不存在
        let has_sel = self.selected.is_some();
        let in_repos = self.page == Page::Repos;
        // 「上次批量结果」是 `app_log` 唯一的读出口。跑完那条通知会消失，
        // 汇总里的解释也就没了，而没成的逐仓库原因只在这个对话框里。
        // 没有记录时**置灰而不是隐藏**：藏了用户会以为这个功能不存在
        let has_log = !self.app_log.lines.is_empty();
        let log_item = CommandItem::new()
            .label(if has_log {
                "上次批量结果".to_string()
            } else {
                "上次批量结果（还没有记录）".to_string()
            })
            .icon(icon_sm(AssetIcon::ListChecks).text_color(tier3(t)))
            .keywords(["批量", "结果", "汇总", "batch", "日志"])
            .disabled(!has_log);
        groups.push(CommandGroup::new().label("低频动作").items([
            plain(
                "添加仓库",
                AssetIcon::FolderPlus,
                &["添加", "add", "登记", "新建"],
            ),
            plain(
                "克隆仓库",
                AssetIcon::Download,
                &["克隆", "clone", "拉取远端"],
            ),
            plain(
                "设置",
                AssetIcon::Settings,
                &["设置", "settings", "偏好", "主题", "令牌"],
            ),
            guarded(
                "打开文件夹",
                AssetIcon::FolderOpen,
                &["文件夹", "folder", "资源管理器"],
                has_sel,
            ),
            guarded(
                "在终端中打开",
                AssetIcon::Terminal,
                &["终端", "terminal", "命令行", "cmd"],
                has_sel,
            ),
            plain(
                "重新扫描",
                AssetIcon::RefreshCw,
                &["重扫", "扫描", "刷新", "rescan", "refresh"],
            ),
            log_item,
            toggle_item(
                if self.sidebar_collapsed || self.page == Page::Overview {
                    "展开侧栏"
                } else {
                    "折叠侧栏"
                },
                AssetIcon::PanelLeft,
                &["侧栏", "sidebar"],
            ),
            guarded(
                if self.inspector_open {
                    "折叠检视抽屉"
                } else {
                    "展开检视抽屉"
                },
                AssetIcon::PanelRight,
                &["检视", "抽屉", "inspector", "日志", "变更"],
                in_repos && has_sel,
            ),
        ]));
        acts.push(vec![
            PaletteAction::AddRepo,
            PaletteAction::CloneRepo,
            PaletteAction::Settings,
            PaletteAction::OpenFolder,
            PaletteAction::OpenTerminal,
            PaletteAction::Rescan,
            PaletteAction::BatchLog,
            PaletteAction::ToggleSidebar,
            PaletteAction::ToggleInspector,
        ]);
        debug_assert_eq!(groups.len(), G_CUR);

        // ---- 当前仓库（右键菜单那五条的第二条键盘路径）----
        //
        // 设计 6.1 把这几条的主入口定在右键菜单上，11 节的收尾清单又要求
        // "右键菜单里的命令都有第二条键盘路径"。这两条放在一起只有一个解：
        // ⌘K 是 6.1 自己指定的镜像通道，把它们收在这里。
        // 「复制远程地址」「在云端打开」**不进这一组**——它们已经有仓库头
        // 按钮 + 右键菜单两条路，再加第三条就撞上 6.1 的"不得新增第三处"
        let cur = self.selected.clone();
        let third = self.selected_is_third();
        let changed = self
            .selected
            .as_deref()
            .and_then(|p| self.view(p))
            .map(|v| v.changed)
            .unwrap_or(0);
        let has = cur.is_some();
        let mut cur_items = vec![
            // 三键与详情页同步卡同序（拉取 · 推送 · 检测），设计 6.1 把
            // 它们的主入口定在同步卡上，这里是它指定的那一处镜像
            guarded("拉取", AssetIcon::Download, &["拉取", "pull", "同步"], has),
            // 第三方出现即为越界（设计 3.5），与同步卡那个占位格子同一条
            guarded(
                "推送",
                AssetIcon::Upload,
                &["推送", "push", "同步"],
                has && !third,
            ),
            guarded(
                "检测",
                AssetIcon::RefreshCw,
                &["检测", "check", "刷新"],
                has,
            ),
            guarded(
                "提交并推送…",
                AssetIcon::GitMerge,
                &["提交", "commit"],
                has && !third && changed > 0,
            ),
            guarded(
                if third {
                    "移回我的仓库"
                } else {
                    "移到第三方仓库"
                },
                AssetIcon::ArrowUpRight,
                &["归属", "移到第三方", "分组"],
                has,
            ),
            guarded(
                "重命名…",
                AssetIcon::Pencil,
                &["重命名", "改名", "rename"],
                has,
            ),
            guarded(
                "编辑…",
                AssetIcon::SquarePen,
                &["编辑", "修改", "edit"],
                has,
            ),
            guarded("移除", AssetIcon::Trash, &["移除", "删除", "remove"], has),
        ];
        let mut cur_acts: Vec<PaletteAction> = vec![
            PaletteAction::Sync(Op::Pull),
            PaletteAction::Sync(Op::Push),
            PaletteAction::Sync(Op::Check),
            PaletteAction::CommitPush,
            PaletteAction::MoveGroup,
            PaletteAction::Rename,
            PaletteAction::EditRepo,
            PaletteAction::RemoveRepo,
        ];
        // ---- 跑这一仓库的命令 ----
        //
        // 概览页只常显「开发运行 / 编译构建」两颗，其余入口全在这里。
        // **四个固定槽位全列，不只列没露面的那两颗**——命令面板是"找任何
        // 功能"的地方，用户搜「测试」时不该先要想"那两颗在哪个键上"。
        // 钉住的自定义命令接在后面，文案取用户自己起的那个
        //
        // 没认出命令的槽位**置灰而不是不列**：藏了用户会以为这个功能不存在
        if let Some(p) = cur.as_deref().and_then(|p| self.entry(p)) {
            for slot in [Slot::Dev, Slot::Build, Slot::Test, Slot::Install] {
                let cmd = self.effective_cmd(p.path.as_str(), slot);
                cur_items.push(
                    CommandItem::new()
                        .label(format!("跑 {}", slot.default_label()))
                        .icon(icon_sm(AssetIcon::Play).text_color(tier3(t)))
                        .keywords([
                            slot.default_label(),
                            cmd.as_str(),
                            "运行",
                            "run",
                            "命令",
                        ])
                        .disabled(cmd.is_empty()),
                );
                cur_acts.push(PaletteAction::RunSlot(slot));
            }
            for i in 0..p.pinned_cmds.len().min(MAX_PINNED_CMDS) {
                let pc = &p.pinned_cmds[i];
                cur_items.push(
                    CommandItem::new()
                        .label(format!("跑 {}", pc.label))
                        .icon(icon_sm(AssetIcon::Pin).text_color(tier3(t)))
                        .keywords([pc.label.as_str(), pc.cmd.as_str(), "自定义", "钉住", "run"]),
                );
                cur_acts.push(PaletteAction::RunSlot(Slot::Pinned(i)));
            }
            // 「编辑常用命令…」：概览页那颗「编辑」键跟着常用命令段一起撤了
            // （同一对入口摆两遍），但**能跑不能改是残废的**——四个槽位的命令
            // 与钉住的自定义命令，改入口就落在这里
            cur_items.push(
                CommandItem::new()
                    .label("编辑常用命令…")
                    .icon(icon_sm(AssetIcon::Settings).text_color(tier3(t)))
                    .keywords([
                        "常用命令",
                        "编辑",
                        "钉住",
                        "自定义",
                        "命令",
                        "槽位",
                        "edit",
                    ]),
            );
            cur_acts.push(PaletteAction::EditCmds);
        }
        debug_assert_eq!(cur_items.len(), cur_acts.len());
        groups.push(CommandGroup::new().label("当前仓库").items(cur_items));
        acts.push(cur_acts);
        debug_assert_eq!(groups.len(), G_BATCH);
        // ---- 批量同步（Q3：固定三条，按分档自动筛） ----
        let busy = self.batch.as_ref().is_some_and(|b| b.running);
        let mut items = Vec::new();
        let mut a_batch = Vec::new();
        for kind in [BatchKind::Pull, BatchKind::Push, BatchKind::Check] {
            let n = self.batch_targets(kind).len();
            items.push(
                CommandItem::new()
                    .label(batch_line(kind, n))
                    .icon(icon_sm(batch_icon(kind)).text_color(tier3(t)))
                    .keywords(kind.keywords().iter().copied())
                    .disabled(n == 0 || busy),
            );
            a_batch.push(PaletteAction::Batch(kind));
        }
        debug_assert_eq!(items.len(), a_batch.len(), "批量组：条目与动作必须逐条对应");
        groups.push(CommandGroup::new().label("批量同步").items(items));
        acts.push(a_batch);
        debug_assert_eq!(groups.len(), G_VIEW);

        // ---- 切换视图（导航轨四个按钮的键盘路径） ----
        groups.push(CommandGroup::new().label("切换视图").items([
            nav_item("总览", AssetIcon::LayoutDashboard),
            nav_item("我的仓库", AssetIcon::GitBranch),
            nav_item("第三方仓库", AssetIcon::Users),
            nav_item("CLI 工具", AssetIcon::Terminal),
        ]));
        acts.push(vec![
            PaletteAction::Nav(Nav::Overview),
            PaletteAction::Nav(Nav::Group(Group::Mine)),
            PaletteAction::Nav(Nav::Group(Group::Third)),
            PaletteAction::Nav(Nav::CliTools),
        ]);
        debug_assert_eq!(groups.len(), G_CLI);

        // ---- CLI 工具（阶段三：重新扫描 / 升级全部 / 清理共享缓存） ----
        // 「升级全部」的目标数 = **真有新版且 provider 给得出升级命令**的两重判据
        // （与 `ops::upgrade_all_cli` 同一个口径——两处各数一遍迟早对不上）。
        // cargo 不覆写 `outdated`（`latest` 恒 `None`），天然不在里面
        let up_n = self
            .cli
            .packages
            .iter()
            .filter(|p| {
                p.latest.is_some()
                    && qingbird_core::cli::provider_of(p.provider)
                        .and_then(|pr| pr.upgrade_step(p))
                        .is_some()
            })
            .count();
        let shared_n = self.cli.shared.len();
        let cli_busy = self.cli_busy_any();
        groups.push(CommandGroup::new().label("CLI 工具").items([
            plain(
                "重新扫描 CLI 工具",
                AssetIcon::Terminal,
                &["cli", "工具", "扫描", "刷新"],
            ),
            // **没东西可做时把理由写进标签**（同 `batch_line` 那条判据）：
            // 一条灰着的命令什么都不说，读起来就是"坏了"
            CommandItem::new()
                .label(cli_upgrade_all_line(up_n))
                .icon(cmd_icon(AssetIcon::ArrowUp))
                .keywords(["cli", "工具", "升级", "更新", "upgrade"])
                .disabled(up_n == 0 || cli_busy),
            CommandItem::new()
                .label(cli_clean_shared_line(shared_n))
                .icon(cmd_icon(AssetIcon::Trash))
                .keywords(["cli", "缓存", "清理", "共享", "clean"])
                .disabled(shared_n == 0 || cli_busy),
        ]));
        acts.push(vec![
            PaletteAction::ScanCli,
            PaletteAction::UpgradeAllCli,
            PaletteAction::CleanSharedCli,
        ]);
        debug_assert_eq!(groups.len(), G_REPO);

        // ---- 切到仓库（**排最后**，理由见 `G_REPO` 的注释）----
        let mut items = Vec::new();
        let mut a_repos = Vec::new();
        for r in &self.repos {
            let st = self.status_of(&r.path);
            let b = bucket_of(&st);
            let words: Vec<String> = vec![
                r.name.clone(),
                r.alias.clone(),
                r.path.clone(),
                st.branch.clone(),
                bucket_word(b).to_string(),
                // 归属也是一个可搜的维度：搜「第三方」能把那一组全列出来
                if r.is_third_party {
                    "第三方".into()
                } else {
                    "我的".into()
                },
            ];
            items.push(
                CommandItem::new()
                    .label(self.title(&r.path))
                    .icon(icon_sm(bucket_icon(b)).text_color(dot_color(b, t)))
                    // 打勾标出"我现在就在这个仓库"。这比在标签里加括号干净，
                    // 也不会让标签变成两倍长
                    .checked(self.selected.as_deref() == Some(r.path.as_str()))
                    .keywords(words),
            );
            a_repos.push(PaletteAction::Repo(r.path.clone()));
        }
        if items.is_empty() {
            items.push(CommandItem::new().label("还没有仓库").disabled(true));
            a_repos.push(PaletteAction::Rescan);
        }
        debug_assert_eq!(items.len(), a_repos.len());
        groups.push(CommandGroup::new().label("切到仓库").items(items));
        acts.push(a_repos);

        debug_assert_eq!(acts.len(), groups.len());
        self.palette_actions = acts;
        groups
    }
}

/// 批量那一行的标签。**没东西可跑时把这件事写进标签**——置灰的按钮配一句
/// 「没有可跑的仓库」，用户看到的是"现在没有落后的仓库"而不是"点了没反应"。
/// 抽成自由函数是为了能单测：这段文案挂在 `Shell` 上时得一整个 Shell 才调得了
fn batch_line(kind: BatchKind, n: usize) -> String {
    if n == 0 {
        format!("{}（没有可跑的仓库）", kind.verb())
    } else {
        format!("{} · {n} 个仓库", kind.verb())
    }
}

// ---------- 组装小件 ----------

/// 把分组折进 `Command`。**顺序就是 `palette_actions` 的下标顺序**，
/// 两边由同一次 `palette_model` 产出，改一处必须改另一处
fn with_groups(cmd: Command, groups: Vec<CommandGroup>) -> Command {
    groups.into_iter().fold(cmd, |cmd, g| cmd.group(g))
}

/// 「升级全部」那颗的标签。**没东西可升时把这件事写进标签**——
/// 置灰的命令配一句「没有可升级的工具」，用户看到的是"现在都最新"
/// 而不是"点了没反应"（同 `batch_line` 那条判据）。
/// 抽成自由函数是为了能单测（挂在 `Shell` 上时得一整个 Shell 才调得了）
fn cli_upgrade_all_line(n: usize) -> String {
    if n == 0 {
        "升级全部（没有可升级的工具）".to_string()
    } else {
        format!("升级全部 · {n} 个工具")
    }
}

/// 「清理共享缓存」那颗的标签。同上：没有共享缓存时说出为什么
fn cli_clean_shared_line(n: usize) -> String {
    if n == 0 {
        "清理共享缓存（没有共享缓存）".to_string()
    } else {
        format!("清理共享缓存 · {n} 个目录")
    }
}

/// ⌘K 三种命令项的图标。**统一走本项目的 `ICON` 档（16px）**。
///
/// ## 这里为什么不能调 `icon_sm`
///
/// ⌘K 那一屏的图标与行内小图标**不同档**：行内是 14px（`icon_sm`），
/// 命令面板的行更高、字更大，14px 的图标在里面显得缩了一圈。
/// 这里跟左工具栏、统计卡一样取 **16px**，全站三类位置各归一档。
///
/// ## 为什么还要显式写，尽管写了也可能被丢弃
///
/// ⚠️ **基座在渲染时会无条件 `.size_4()` 覆盖调用方的尺寸**
/// （`command/state.rs:821`：`icon.size_4().text_color(..)`），
/// 传进去的任何尺寸**都会被丢掉**——与 `Button::icon()` 同一个毛病
/// （AGENTS.md 已记）。而 `size_4()` 落在 tailwind 语义里正是 16px，
/// 恰好与 `ICON` 同值，**所以最终观感是对的**。
///
/// 显式写出来是为了让"这行要 16px"成为**代码里的一个事实**而不是
/// 基座实现细节的巧合：哪天基座改了 `size_4()` 的取值，
/// 这里会跟着变，而注释告诉下一个人该去核对哪一处。
/// ⚠️ **别改成 `icon_sm`**——那是 14px，会让 ⌘K 的图标比别处小一圈。
fn cmd_icon(icon: AssetIcon) -> Icon {
    Icon::new(icon).with_size(Size::Size(px(ICON)))
}

fn nav_item(label: &'static str, icon: AssetIcon) -> CommandItem {
    CommandItem::new().label(label).icon(cmd_icon(icon))
}

fn plain(label: &'static str, icon: AssetIcon, words: &[&str]) -> CommandItem {
    CommandItem::new()
        .label(label)
        .icon(cmd_icon(icon))
        .keywords(words.iter().copied())
}

fn guarded(label: &'static str, icon: AssetIcon, words: &[&str], on: bool) -> CommandItem {
    CommandItem::new()
        .label(label)
        .icon(cmd_icon(icon))
        .keywords(words.iter().copied())
        .disabled(!on)
}

fn toggle_item(label: &'static str, icon: AssetIcon, words: &[&str]) -> CommandItem {
    plain(label, icon, words)
}

fn batch_icon(kind: BatchKind) -> AssetIcon {
    match kind {
        BatchKind::Pull => AssetIcon::Download,
        BatchKind::Push => AssetIcon::Upload,
        BatchKind::Check => AssetIcon::RefreshCw,
    }
}

fn notify_os(r: Result<(), String>, window: &mut Window, cx: &mut App) {
    if let Err(e) = r {
        window.push_notification(e, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 没有可跑的仓库时**把这件事写进标签**。这是用户唯一能看到的解释——
    /// 一颗灰着的键什么都不说，读起来就是"坏了"
    #[test]
    fn an_empty_batch_says_why_it_is_disabled() {
        assert_eq!(
            batch_line(BatchKind::Pull, 0),
            "拉取全部（没有可跑的仓库）"
        );
        assert_eq!(
            batch_line(BatchKind::Push, 0),
            "推送全部（没有可跑的仓库）"
        );
    }

    /// 有东西可跑时把个数写进去：点之前就知道会动几个仓库
    #[test]
    fn a_non_empty_batch_carries_its_count() {
        assert_eq!(batch_line(BatchKind::Pull, 3), "拉取全部 · 3 个仓库");
        assert_eq!(batch_line(BatchKind::Check, 1), "检测全部 · 1 个仓库");
    }

    /// 三档各有一颗键，顺序固定（用户是按位置记的）
    #[test]
    fn the_three_batch_kinds_keep_a_fixed_order() {
        let ks = [BatchKind::Pull, BatchKind::Push, BatchKind::Check];
        let lines: Vec<String> = ks.iter().map(|k| batch_line(*k, 1)).collect();
        assert_eq!(
            lines,
            vec!["拉取全部 · 1 个仓库", "推送全部 · 1 个仓库", "检测全部 · 1 个仓库"]
        );
    }

    /// CLI 那两颗：**没东西可做时把理由写进标签**（同批量那条判据）。
    /// 一条灰着的命令什么都不说，读起来就是"坏了"
    #[test]
    fn cli_palette_lines_say_why_they_are_disabled() {
        assert_eq!(cli_upgrade_all_line(0), "升级全部（没有可升级的工具）");
        assert_eq!(cli_upgrade_all_line(3), "升级全部 · 3 个工具");
        assert_eq!(cli_clean_shared_line(0), "清理共享缓存（没有共享缓存）");
        assert_eq!(cli_clean_shared_line(2), "清理共享缓存 · 2 个目录");
    }
}
