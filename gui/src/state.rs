//! 界面状态与纯逻辑：仓库视图、操作类别、对话框状态、分档口径。
//!
//! `bucket_of` 是全站唯一的仓库分档口径——侧栏徽标、总览、⌘K 状态点
//! 都从这儿取。别在别处另写一套判断。
//! （原来这里还列着"同步环"：那颗环 2026-10-05 已随用户口径「占地方还难看」下线，
//! 见 `panels.rs::sync_card` 那句「没有圆环」。**别再去找那个文件**——
//! `gui/src/sync_ring.rs` 从来没有存在过，环以前是画在 `sync_card` 里的）

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::component::command::CommandState;
use gpui_kit::component::input::InputState;
use gpui_kit::component::list::ListState;
use gpui_kit::component::resizable::ResizableState;
use gpui_kit::component::table::TableState;
use gpui_kit::component::text::TextViewState;

use crate::tokens::UiScale;
use gpui_kit::component::Theme;
use gpui_kit::{AnyWindowHandle, Entity, FocusHandle, Hsla, ScrollHandle, SharedString, Task};
use qingbird_core::history::{CommitDetail, CommitPatch, CommitSummary};
use qingbird_core::repos::{self, ActivityDay, ChangeEntry, LangStat};
// ⚠️ **取别名而不是直接 `scan_cache`**：本 crate 自己也有一份 `scan_cache`
// （界面侧那条接线，`gui/src/scan_cache.rs`），两个同名搁在同一个文件里
// 会让人读错——而读错的那一处编译器不会报，它只会挑中另一个
use qingbird_core::scan_cache as core_cache;
use qingbird_core::run_cmd::{self, Slot};
use qingbird_core::space;
use qingbird_core::sync;
use qingbird_core::stash::StashEntry;
use qingbird_core::store::{self, CloseAction, RepoEntry};
use qingbird_core::types::{ForkInfo, RepoStatus, UpdateChanges};
use std::collections::{HashMap, HashSet};

use crate::history_tab::CommitDelegate;
use crate::palette::PaletteAction;
use crate::tokens::*;

/// 一个仓库在界面上要显示的东西
pub(crate) struct RepoView {
    pub(crate) status: RepoStatus,
    /// 这一份 `status` **是真读出来的，还是空的**。
    ///
    /// ⚠️ 加这一格是因为 `RepoStatus::default()` 与"读过、结论是这个文件夹
    /// 不是 Git 仓库"**在结构体里长得一模一样**（两条都是 `is_repo: false`、
    /// `remote_url: ""`、ahead/behind 全 0）。于是界面上"我还没问过 git"这句
    /// 真话没有任何一格可以说，只能说成那两句假话之一。
    /// 与 D73（进度信息断在字段声明里）、D78（归属断在参数里）同族，
    /// 也正是 `SpaceState::scanned_at` / `ProfileState::scanned_at` 早就解决过
    /// 的那件事——**这两路有时间戳，状态这一路一直没有**。
    ///
    /// 写的地方只有三处：扫描回填、缓存灌入（都置 `true`）、`blank()`（`false`）。
    /// 读的地方是 `Shell::status_read`——同步卡主句与头卡「云端」那一格共用它，
    /// **不许在别处重新判一遍**（判两遍迟早漂成"卡里说待扫描、头卡说没配远端"）。
    pub(crate) has_status: bool,
    /// 未提交文件数（仓库头的「未提交 N 文件」）
    pub(crate) changed: u32,
    /// 未提交的**条目**（提交对话框的变更摘要、检视抽屉的「变更」页签）。
    ///
    /// 与 `changed` 同源：`changed` 就是这个数组的长度，状态扫描也只跑
    /// 一条 `git status --porcelain`（`changed_files` 跑的是同一条命令，
    /// 换过去等于白赚一份文件名列表，不多派生任何子进程）
    pub(crate) changes: Vec<ChangeEntry>,
    /// 自动识别出来的四条命令：仓库没存命令时拿来预填
    pub(crate) detected: run_cmd::CmdDetect,
}

impl RepoView {
    pub(crate) fn blank() -> Self {
        Self {
            status: RepoStatus::default(),
            // 一份都没读过。这一格是 `false` 才让"待扫描"那句有得可说
            has_status: false,
            changed: 0,
            changes: Vec::new(),
            detected: run_cmd::CmdDetect::default(),
        }
    }
}

/// 「这一份是**真问过 git** 的读数吗」——**库里那一版**的那个事实。
///
/// 与 `Shell::status_read(path)` 是同一个事实的两个出处（一个问内存视图、
/// 一个问库里的行），32-10 起界面上不再有任何地方拿 `RepoCache` 的**内容**
/// 去反推它：`is_repo`、有没有提交、命令空不空——都不回答"这一份是谁写的"
/// （31.4 补十五给 `RepoStatus` 补 `status_read` 那格是同一条纪律）
pub(crate) fn cache_git_was_asked(c: &core_cache::RepoCache) -> bool {
    c.status_read
}

/// 这一份自动识别出来的快捷命令**是不是真认出来的**：四条全空 = 没认出来。
///
/// 它挡的是"把空的一份写进库、盖掉库里那份好的"——空 detected 的两种来源：
/// `RepoView::blank()`（视图还没建起来）与启动时 `detect_cmds` 真没认出任何东西
pub(crate) fn recognised_commands_are_present(d: &run_cmd::CmdDetect) -> bool {
    !(d.test_cmd.is_empty()
        && d.build_cmd.is_empty()
        && d.dev_cmd.is_empty()
        && d.install_cmd.is_empty())
}

/// 缓存里那一份 `detected`（自动识别出来的快捷命令）**能不能灌进界面**。
///
/// ⛔ 这条判据以前写在 `apply_view_cache` 里，是 `c.last_commit_ts > 0`——那颗字段
/// 与"有没有识别到命令"没有半点因果关系。它**恰好**挡住了真正要挡的那种行
/// （只写体积那一路建出来的行 `last_commit_ts` 也是 0，那份 `detected` 是空的，
/// 灌进去会把 `Shell::new` 刚现算出来的好的一份盖掉），于是它的"对"是巧合，
/// 而它的"错"是必然：**一颗刚 `git init`、还没有提交的仓库，扫描那一路照样
/// 把识别好的命令写进了库，却永远读不回来**（32-10 砍 `last_commit_ts` 时撞见）。
///
/// ⚠️ **这一格只有下面那两个调用点，而它们在 `Shell` 的方法里，一条单测都写不了**
/// （Windows 没有窗口级单测，路线表 D40）。所以下面那两条真值表钉住的是
/// **这颗函数本身**，"函数真的被挂在了那一格上"由**帧**负责——本轮的落法是
/// 造一颗**有 package.json 但没有一次提交**的仓库，重启后快捷命令仍然预填
pub(crate) fn cache_detected_is_trustworthy(c: &core_cache::RepoCache) -> bool {
    cache_git_was_asked(c) && recognised_commands_are_present(&c.detected)
}

/// 三键的顺序：**拉取 · 推送 · 检测**。
///
/// 这是一个**跨三处**的不变量：详情页同步卡从左到右是这个序（设计 6.1 把
/// 三键定成"按位置记忆"）、`shell.rs::bind_keys` 绑的 `1` `2` `3` 是这个序、
/// 侧栏与状态条的文案也按它。**错位了用户按 `2` 却跑成检测，而界面上
/// 没有任何东西会告诉他**。
///
/// `bind_keys` 那边读不了这个常量（`KeyBinding::new` 要的是具体 Action
/// 类型，没法从数组里造），所以那处与同步卡都留了指向本行的注释——
/// 改键位要三处一起改，`shell.rs` 的单测 `the_three_number_keys_match_the_sync_card_order` 钉住这个序
pub(crate) const SYNC_SLOTS: [Op; 3] = [Op::Pull, Op::Push, Op::Check];

/// 三键各自的中文说法。**与 `SYNC_SLOTS` 同序**
pub(crate) const SYNC_SLOT_LABELS: [&str; 3] = ["拉取", "推送", "检测"];

/// 输出块里正在跑的操作类别
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Op {
    Cmd(Slot),
    Pull,
    Push,
    Check,
    /// 备份并变基到云端；bool = 工作区脏时用户同意先暂存
    Rebase(bool),
    /// 硬回退到变基备份分支（界面已二次确认）
    UndoRebase,
    /// 提交并推送（含可能夹在中间的 `--ff-only` 拉取）
    Commit,
}

impl Op {
    /// 全部变体。**只有一条用处**：那条"每个落库的键都被归过类"的测试要遍历它。
    ///
    /// 为什么用一份清单而不是让编译器逼出来：新增一种操作时 `key()` 那个 match
    /// **加一条臂就编译过了**，而"它算不算和云端交换过"没人想过 ⇒
    /// 「久未同步」那片默默漏掉它。判错的两个方向都不报错，所以只能这样钉
    ///
    /// `#[cfg(test)]` 而不是 `#[allow(dead_code)]`：它**真的只活在测试里**。
    /// 留一条 allow 等于允许生产代码里也有人来读它、并误以为那是一份可用的清单
    #[cfg(test)]
    pub(crate) const ALL: [Op; 7] = [
        Op::Cmd(Slot::Test),
        Op::Pull,
        Op::Push,
        Op::Check,
        Op::Rebase(true),
        Op::UndoRebase,
        Op::Commit,
    ];
    /// 落进 `op_history` 表的**稳定键**（英文小写）。
    /// **`None` = 这一类不进操作历史**。
    ///
    /// ⚠️ **它会写进磁盘、跟着数据库活很久**，所以别拿中文文案当键——
    /// 改一次文案就等于换一批记录。也不要用会变的字符串拼装
    /// （`format!("{}-{}", …)` 那种），拼错一个字符就是新的一类。
    ///
    /// **返回 `Option` 而不是让调用点自己判**：不记是这一类操作**自身的性质**，
    /// 写在类型里，谁漏了都绕不过去。
    pub(crate) fn key(self) -> Option<&'static str> {
        match self {
            Op::Pull => Some("pull"),
            Op::Push => Some("push"),
            Op::Check => Some("check"),
            Op::Rebase(_) => Some("rebase"),
            Op::UndoRebase => Some("undo_rebase"),
            Op::Commit => Some("commit"),
            // 命令槽位**不记**，两条理由：
            // ① `op_history` 回答的是「这个仓库上次被**更新**过什么」，
            //    跑一次 `cargo test` 不改变仓库的更新状态；
            // ② 槽位键是 `pinned:{i}`，**下标来自 config 里按钮的顺序**——
            //    用户在 ⌘K 里重排一次按钮，历史里那条 `pinned:2` 就对不上
            //    它当时指的是哪条命令了。同步操作没有这个问题。
            Op::Cmd(_) => None,
        }
    }

    /// 从落库的键反查回中文说法。**认不出来的给一个说人话的兜底**
    /// ——库里存着上个版本写的键时，宁可显示"某项操作"也不要空字符串
    pub(crate) fn label_of_key(key: &str) -> &'static str {
        match key {
            "pull" => "拉取",
            "push" => "推送",
            "check" => "检测更新",
            "rebase" => "变基",
            "undo_rebase" => "撤销变基",
            "commit" => "提交并推送",
            _ => "某项操作",
        }
    }

    /// 这个操作在界面上叫什么。
    ///
    /// **与 `label_of_key` 是同一件事的两处写法**（一个从枚举出发、一个从
    /// 落库的键出发，没法合成一个函数）。单测
    /// `every_op_key_round_trips_through_its_label` 钉住两者一致——
    /// **两处各写一遍的东西迟早有一处会漂**
    ///
    /// ⚠️ **它与 `SYNC_SLOT_LABELS` 刻意不完全一样**：同步卡那颗键要短
    /// （「检测」两个字），这里要能嵌进一句话（「检测更新 · 3 分钟前」）。
    /// 别为了"统一"去改任何一边。
    pub(crate) fn label(self) -> &'static str {
        match self {
            Op::Pull => "拉取",
            Op::Push => "推送",
            Op::Check => "检测更新",
            Op::Rebase(_) => "变基",
            Op::UndoRebase => "撤销变基",
            Op::Commit => "提交并推送",
            Op::Cmd(s) => s.default_label(),
        }
    }
}

/// 一个仓库的日志。
///
/// **按仓库路径存**（D7），不是单份全局：切仓库不该把上一个仓库正在跑的输出
/// 冲掉，操作跑完回来时也不该把结果记到"当前选中"名下——用户在这期间可能
/// 已经切到别的仓库去了。
///
/// `Clone` 是给「上次批量结果」那个对话框用的：它的 `content` 闭包里只有
/// `&mut App`，拿不到 `&mut Context<Shell>`，只能读一份快照出来渲染
#[derive(Clone, Default)]
pub(crate) struct LogState {
    pub(crate) running: bool,
    /// 正在跑哪个操作（None = 没在跑）
    pub(crate) op: Option<Op>,
    pub(crate) lines: Vec<String>,
    /// 命令起不来 / 非零退出时的一句话。
    /// 与 `verdict` 分开：这是"没跑起来"，那是"跑完了但没成功"，语气不一样
    pub(crate) error: Option<String>,
    /// 上次跑完的结论 `(成功?, 一句话)`
    pub(crate) verdict: Option<(bool, String)>,
    /// 走代理用的地址；None = 直连。值得单列：代理是"连上了但绕了一圈"，
    /// 混在成功里会让人以为一切正常，排查网络问题时也就没有线索了
    pub(crate) used_proxy: Option<String>,
    /// 当前阶段文案（拉取 / 检测会报阶段名）。
    ///
    /// **这一栏已经带着网络路径**：`net::route_of` 把路径拼在阶段后面
    /// （`接收对象（经代理 127.0.0.1:7897）`），所以它一并回答"走的哪条路"——
    /// 路径切换提示只闪现一瞬间，光看那一句记不住
    pub(crate) stage: Option<String>,
    /// 传输量与网速（`已传输 1.21 MiB · 56.00 KiB/s`）。
    /// **空串 = 本行没有速度信息**：`done.`、纯计数行、以及我们自己发的
    /// 阶段提示都是空的。它与 `percent` 是两个独立来源——百分比定进度条，
    /// 这一栏报"传了多少、现在多快"
    pub(crate) detail: String,
    /// 确定态百分比 0–100。**None = 不定态**：跑命令给不出可信百分比，
    /// 造一个假的只会让人以为快好了，然后越等越焦
    pub(crate) percent: Option<f32>,
    /// 上次操作的秒级时间戳，0 = 本会话没跑过
    pub(crate) ts: u64,
    /// **这次拉回来什么**（提交数 / 增删行 / 文件数 + 提交清单）。
    ///
    /// ⚠️ core 早就算好了（`sync::pull_changes`），2026-10-08 之前它在这一层
    /// **一个读者都没有**：`ops.rs` 收尾只读 `ok` / `message` / `used_proxy` /
    /// `fork` / `backup_branch` 五个字段，`changes` 整条丢掉。
    /// 与 D73（进度信息断在中间层的字段声明里）同族——**编译器不报、单测不红、
    /// 抓帧也只是"少了一行"**。加这一格就是把那条断线接上。
    ///
    /// **只有成功的拉取才是 `Some`**：Already up to date 时 core 返回 `None`
    /// （`commits` 与 `diff_stat` 都空），界面那一行跟着整条不出现。
    /// 会话级、不落盘——措辞是「这次带回」，重启后再说"这次"就是谎话；
    /// 跨会话那句归 `op_history`（表头那三条档在读它）
    pub(crate) pulled: Option<UpdateChanges>,
    /// `pulled` 那一行摊开了没有（「看清单」那颗键的开关）。
    ///
    /// 存在这里而不是 `Shell` 上：日志按仓库存，开关也得跟着仓库走，
    /// 否则在 A 仓展开、切到 B 仓会看到 B 的清单是开着的。
    /// 新操作重建 `LogState` 时随 `..default()` 自动关掉，不用手写重置
    pub(crate) pull_open: bool,
}

/// 没日志时读的兜底。**放在模块级而不是 `impl` 里的 `static`**：
/// `impl` 块内不允许 `static` 项，而且模块级常量能直接取地址，
/// 界面各处 `unwrap_or(&LOG_EMPTY)` 不会每帧造一个临时值。
///
/// 为什么需要它：`log()` 读的是「当前选中仓库的那份」，而选中可能为
/// `None`、也可能这个仓库还没跑过任何操作。没有这个兜底的话，十几处
/// 读取点都要先判有没有再取值，漏一处就是一次 panic
pub(crate) static LOG_EMPTY: LogState = LogState {
    running: false,
    op: None,
    lines: Vec::new(),
    error: None,
    verdict: None,
    used_proxy: None,
    stage: None,
    detail: String::new(),
    percent: None,
    ts: 0,
    pulled: None,
    pull_open: false,
};

/// 磁盘占用卡的状态（只跟随当前选中的仓库）

#[derive(Default)]
pub(crate) struct SpaceState {
    pub(crate) report: Option<space::SpaceReport>,
    pub(crate) scanning: bool,
    /// 一键清理进行中：按钮转圈、防连点
    pub(crate) cleaning: bool,
    /// 上次清理的结果（「已释放 1.6 GB」）
    pub(crate) note: Option<String>,
    pub(crate) error: Option<String>,
    /// 这一份占用是**什么时候**算出来的，秒级时间戳，0 = 还没算过。
    ///
    /// 阶段 16 起这一栏可能是**从库里读出来的旧数**（启动 / 切仓库时先灌
    /// 缓存，再后台刷新）。有它才能在表头上写「扫描于 3 小时前」——
    /// **没有它，一份昨天的数字和一份刚算出来的数字长得一模一样**，
    /// 用户无从知道该信哪个
    pub(crate) scanned_at: u64,

    // ---- 目录树（2026-10-05 阶段 13）----
    /// 树的根层。**由 `scan_repo_space` 的 `dirs` 直接喂进来**，不重新扫——
    /// 那一层的递归体积刚算过，再走一遍等于把整个仓库重数一次
    pub(crate) tree: Vec<TreeNode>,
    /// 正在扫的层：行尾画一个转圈
    pub(crate) tree_loading: HashSet<String>,
    /// 树扫描代数：切仓库 / 重建根层时作废旧结果。
    /// 这里**不存任务句柄**：树是"多点一下就多扫一层"，用代数守卫挡过期
    /// 结果比互相取消好——被取消的那一层下次点还会再要一次
    pub(crate) tree_seq: u64,
}

/// 目录树的一个节点
///
/// **为什么不用官方 `component::tree`**（2026-10-05 查证，理由写在这里以免
/// 下一个人再试一遍）：`TreeItem::is_folder()` 是 `!children.is_empty()`
/// （`gpui-base/src/tree.rs:132-134`），而**懒加载的文件夹在扫之前 children
/// 必然是空的** → 基座 `toggle_expand` 第一行 `if !entry.is_folder() { return }`
/// 就把那行挡掉，那个文件夹永远展不开；而且它没有"展开时去加载"的钩子。
/// 绕过去只能给每个文件夹塞一个假子项，那东西会在扫回来之前闪一下。
/// 代价是自己画行，少了基座给的键盘走位与虚拟滚动——这一页是**只读浏览**，
/// 行数由"用户展开了多少层"决定（几十到几百），`v_flex` 扛得住
pub(crate) struct TreeNode {
    pub entry: space::DirEntry,
    /// `None` = 这一层还没扫过；`Some(vec![])` = 扫过，确实是空的
    pub children: Option<Vec<TreeNode>>,
    pub expanded: bool,
}

impl TreeNode {
    pub(crate) fn new(entry: space::DirEntry) -> Self {
        Self {
            entry,
            children: None,
            expanded: false,
        }
    }

    /// 画不画展开箭头：**是文件夹且非空**。`child_count` 是 core 数出来的一层子项数
    pub(crate) fn foldable(&self) -> bool {
        self.entry.is_dir && self.entry.child_count > 0
    }
}

/// 档案页（Screen 1）的本地数据：语言统计 / 活跃度 / 最近提交 / README。
/// 全部是几条本地 git 命令 + 读一个文件，放后台线程扫，界面先画占位。
#[derive(Default)]
pub(crate) struct ProfileState {
    pub(crate) langs: Vec<LangStat>,
    /// 近 30 天有提交的日子（后端不补空白日）
    pub(crate) activity: Vec<ActivityDay>,
    /// README **原文**，给「说明」页签整篇渲染。
    ///
    /// 存 `SharedString` 而不是 `String`：官方 `TextView::markdown` 收的是
    /// `impl Into<SharedString>`，存 `String` 就得在 render 里克隆一份——
    /// 一份上限 12 万字的 README 每帧克隆一次，光这一处就能吃掉一帧的预算
    pub(crate) readme_text: Option<SharedString>,
    /// 实际读到的是哪个文件（`README.md` 之类）。空 = 没有 README
    pub(crate) readme_file: Option<String>,
    /// 「另一个语言」那一份说明文档。`None` = 只有一份，界面上那颗切换键不出现。
    ///
    /// 判据是 **`readme_file` 这一份**而不是"主文档"——用户切到英文之后
    /// 再点一次要切回来，靠 core 的 `readme_alt(路径, 当前文件)` 重算
    pub(crate) readme_alt: Option<qingbird_core::repos::ReadmeAlt>,
    /// 换文档失败时的一句话（`读不到仓库根目录下的 README_en.md`）。
    /// 空 = 没事
    ///
    /// **为什么落状态而不是弹气泡**：切文档走后台任务（`cx.spawn` 的闭包
    /// 必须 `'static`，抓不到 `&mut Window`），弹通知那条路在那儿是断的。
    /// 而"点了没反应"比报错更让人以为功能坏了，所以这句话要摆在表头上
    pub(crate) readme_error: Option<String>,
    /// 根目录下**每一份**说明文档的正文（文件名 → 正文，阶段 18）
    ///
    /// **有它，中英两颗键才是同步切换**：点一下就在内存里换掉，
    /// 不派生后台任务、不碰 `scanning`、表头那一句时间也不会闪。
    /// 没有它的话每次切换都要重读一遍盘，而磁盘上明明就摆着
    pub(crate) readme_docs: HashMap<String, SharedString>,
    /// 候选表（名字 + 语言 + 排序档），与 `readme_docs` 同一次扫出来
    ///
    /// 留着它是为了切换时**也能脱离磁盘算**「另一份是哪一份」——
    /// 判据问的是 core 那个 `alt_of`，界面不自己写第二份规则
    pub(crate) readme_index: Vec<qingbird_core::repos::ReadmePart>,
    /// 正在**读一份还没握在手里的**说明文档（README 正文里点相对链接
    /// 切到第三份文档才会走这条路，阶段 18）
    ///
    /// ⚠️ **它不能借用 `scanning`**：那个标志是**整份档案扫描**在用的，
    /// 仓库头的活跃度芯片与语言构成条都读它。借去用的话，点一次中英切换
    /// 会让那两个毫不相干的地方一起说「统计中…」，读完了再一起说不是——
    /// **一个标志两件事**，与 D35 那条是同一类
    pub(crate) readme_loading: bool,
    pub(crate) scanning: bool,
    /// 档案数据（语言 / 活跃度 / README）是**什么时候**读出来的。
    /// 与 [`SpaceState::scanned_at`] 同一个理由：从库里读出来的旧数据
    /// 必须能让用户看出它旧
    pub(crate) scanned_at: u64,
}

/// 「历史」页签的状态。
///
/// **不进 `RepoView`**：历史只在打开那个页签时才扫，一次 `git log` 要派生
/// 一个子进程；放进每轮都跑的状态扫描里等于让所有仓库在后台无条件付这份代价。
/// 按仓库路径记一份，切回来时如果路径对得上就直接显示上一次的结果
#[derive(Default)]
pub(crate) struct HistoryState {
    /// 这份数据是哪个仓库的
    pub(crate) path: String,
    pub(crate) commits: Vec<CommitSummary>,
    pub(crate) scanning: bool,
    /// 正在看的提交（None = Sheet 关着）
    pub(crate) open: Option<CommitDetail>,
    /// 正在看的某个文件的 patch。`Some(("", _))` = 刚点开提交还没点文件
    pub(crate) patch: Option<(String, CommitPatch)>,
    /// Sheet 里的加载中（文件列表 / patch 各自一次）
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    /// 扫描代数：过期结果不许覆盖新结果
    pub(crate) seq: u64,
    pub(crate) task: Option<Task<()>>,
}

/// CLI 工具屏的状态（阶段一：只读扫描展示）。
///
/// **一份全局**，不按仓库——CLI 工具是整台机器上的，与"当前选中哪个仓库"无关。
pub(crate) struct CliState {
    /// 扫到的全部 CLI 包（已按 provider + 名字排好，排序在 core 扫描那一步做）
    pub(crate) packages: Vec<qingbird_core::cli::CliPackage>,
    /// 当前选中的包 id（`"npm:pnpm"`）。None = 没选。
    /// ⚠️ **存 id 不存下标**：重扫一次清单就变，下标会错位，而 id 稳定
    pub(crate) selected: Option<String>,
    pub(crate) scanning: bool,
    /// 这一份是什么时候扫的（秒级，0 = 本会话还没扫过）
    pub(crate) scanned_at: u64,
    /// 某台 provider 失败的一句话（扫描协调器给的）。空 = 都成功。
    /// 它**不影响别的 provider 的结果**——一句话提示，不是"整屏没扫到"
    pub(crate) errors: Vec<String>,
    /// 每个 provider 分组展开没展开，下标同 `ProviderId::all()`
    /// （`[scoop, npm, pipx]`）。默认全展开
    pub(crate) group_open: [bool; 3],
    /// 工具图标：包 id → 已经提到的那张 PNG 的绝对路径。
    ///
    /// **查不到这一条** = 这个工具没有自带图标（或者还没轮到它）⇒ 画终字符块
    /// （设计 §10.1 三级降的第 ③ 级）。两种"没有"在界面上是同一句话，
    /// 所以不拆两个值——**"还没提完"与"它就没有"，用户看到的都是那个块**。
    ///
    /// 首屏灌的是磁盘缓存（`icon::cached_icons`，一次 `read_dir`），
    /// 后台那条提取再逐颗核对替换（`ops::scan_cli`）。
    pub(crate) icons: HashMap<String, std::path::PathBuf>,
    /// 共享缓存（provider 级，不属于任何一颗包）。设计 §9：它们只进右栏底部那条汇总。
    /// 体积同样是惰性的（`size` 为 `None` = 界面显「计算中…」）
    pub(crate) shared: Vec<qingbird_core::cli::PackageDir>,
}

impl Default for CliState {
    fn default() -> Self {
        Self {
            packages: Vec::new(),
            selected: None,
            scanning: false,
            scanned_at: 0,
            errors: Vec::new(),
            // 默认全展开：**不是** `[false; 3]`——那会让首次进屏看到一列
            // 折叠着的组标题，以为"扫到了但里面是空的"
            group_open: [true; 3],
            icons: HashMap::new(),
            shared: Vec::new(),
        }
    }
}

/// 总览表格的一行。**扁平化成值类型**而不是存 `(&RepoEntry, &RepoView)`：
/// 委托是 `'static` 的，拿不到 `Shell` 的借用，而引用会逼着整张表在
/// 每次 render 里重新构造一遍委托
#[derive(Clone)]
pub(crate) struct OvRow {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) third: bool,
    pub(crate) branch: String,
    pub(crate) bucket: Bucket,
    /// 状态文案（已同步 / 可更新 / 已分叉 / 待推送 / 未提交 / 待扫描）
    pub(crate) status: String,
    pub(crate) behind: u32,
    pub(crate) ahead: u32,
    pub(crate) changed: u32,
    /// 最后一次用这个仓库的秒级时间戳（`RepoEntry::last_used` 原样搬过来）。
    /// **表格排序要读它**——排序档「按最后使用」是原型卡头右上那颗键的默认档
    pub(crate) last_used: u64,
    /// 这一行属于「久未同步」四档里的哪一档。
    ///
    /// ⚠️ **存算好的档位而不是那个时间戳**：表格里没有"上次同步"这一列,
    /// 这一格唯一的读者是那颗筛选片与它上面的计数。存时间戳的话，
    /// "多久算久"这条规则会在过滤处再写一遍（两份迟早漂），
    /// 而界面哪天要加那一列，多存一个字段比少存一个便宜
    pub(crate) sync_age: core_cache::SyncAge,
}

/// 总览页「全部仓库」那张卡的行序（原型 `.card-head .btn` 那两档）。
///
/// 只有两档，且**默认是 `LastUsed`**：总览页是"我下次该动哪个仓库"那一屏，
/// 最近碰过的排前面才回答得上这个问题；按名称排是给"我知道要找哪个仓库"的场景，
/// 那场景其实该用 ⌘K。做成两档而不是列头点击排序，是因为七列里只有这两档有意义，
/// 其余五列（状态 / 落后 / 待推送 / 未提交）的值域只有几个数，排了也分不出什么
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OvSort {
    LastUsed,
    Name,
}

impl OvSort {
    /// 卡头那颗键上写的字 = **当前生效的那一档**（不是"点它会切到哪一档"）。
    /// 原型就是这么写的（`按最后使用`），而那颗键的图标是一条收窄的漏斗，
    /// 读起来是"现在按这个排"。写成下一档会让人以为点了没生效
    pub(crate) fn label(self) -> &'static str {
        match self {
            OvSort::LastUsed => "按最后使用",
            OvSort::Name => "按名称",
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            OvSort::LastUsed => OvSort::Name,
            OvSort::Name => OvSort::LastUsed,
        }
    }
}

/// 这一行要不要出现在表里。**收成一颗纯函数**的理由：那颗片上的计数与点进去的
/// 过滤读的是同一个判断——两处各写一份，就会出现"片上写 3、点开只有 2 行"，
/// 而那种不一致会被当成"数据没刷新"放过去（本项目为"同一属性两个出处"交过四次学费）
pub(crate) fn ov_row_shows(age: core_cache::SyncAge, stale_only: bool) -> bool {
    !stale_only || core_cache::sync_age_is_stale(age)
}

/// 那颗片**能不能用**：读得出来才能用。
///
/// 为什么把一句 `is_some()` 抽出来：它是这一整格唯一挡住"一屏假话"的闸——
/// 表读不出来时如果片还能按，点下去 `Unknown` 那一档就会把每颗仓库都说成
/// 三个月没同步。留在 `render` 里它就只能靠抓帧看，抽出来它才**能被变异打红**
/// （AGENTS：凡是靠 `&self` 才能调的显示判据，先抽成纯函数再单测）
pub(crate) fn sync_table_is_readable(times: &Option<HashMap<String, u64>>) -> bool {
    times.is_some()
}

/// 那颗片上写的字。
///
/// ⛔ **读不出来时不许写 0**。上一版是 `久未同步 {n}`，而 `n` 在 `None` 那一路
/// 天然是 0 ⇒ 帧上拍出来是"一颗灰掉的键写着「久未同步 0」"，读的人得到的是
/// **一句有值的结论**："一颗都没有"。这正是 `Unknown` 那一档存在的全部理由，
/// 而它漏在了末端这一格上（2026-10-11 取证时才看见——单测钉住了计数与筛选，
/// 没有一句钉住"读不出来时那颗键上写什么"，所以这条判据抽成函数并补了单测）。
///
/// 空值记号用 `–`，与同一屏表格里"没有这个数"那几列**同一个记号**
pub(crate) fn chip_label(readable: bool, n: usize) -> String {
    if readable {
        format!("久未同步 {n}")
    } else {
        "久未同步 –".to_string()
    }
}

/// 那颗片上的两个数：`(超过 90 天没交换的, 没有一次成功同步记录的)`。
///
/// ⚠️ **必须在过滤之前统计**：筛完之后 `Never` 那一档只剩… 不，它还在，
/// 但一旦有人以为"数应该跟着当前可见的行走"，第二个数就会随点击漂。
/// 固定在过滤前算，它就是一个与开关无关的事实。
/// ⚠️ `Unknown` 两档都不进——它不是"没同步"，是"这张表我读不出来"，
/// 那一档的落法是**把片禁用**，见 `overview_page` 里那一句
pub(crate) fn stale_counts(rows: &[OvRow]) -> (usize, usize) {
    let mut over = 0usize;
    let mut none = 0usize;
    for r in rows {
        match r.sync_age {
            core_cache::SyncAge::Stale => over += 1,
            core_cache::SyncAge::Never => none += 1,
            core_cache::SyncAge::Recent | core_cache::SyncAge::Unknown => {}
        }
    }
    (over, none)
}

/// 导航轨指向的页面
///
/// ⚠️ **仓库那一路仍然只有"仓库页"一个 `Page`**（D2）：第 2、3 个导航按钮的语义是
/// "驱动侧栏"而不是"切到第三种页面"，「当前看哪一组」是侧栏的焦点态 `side_focus`。
/// 给"第三方仓库"单开一个 `Page` 会让总览/仓库两屏的判据分裂成两套。
///
/// `CliTools` 是**另一件事**：CLI 屏没有"我的/第三方"这种分组焦点，
/// 也没有仓库详情那套页签，它就是第三种屏。放进 `Page` 是唯一正确的落点——
/// 塞进 `Nav` 而不进 `Page` 的话，"当前在哪一屏"就少了这一个值，
/// 而它恰恰会被 render / 侧栏挂载 / 窗口撑宽几处判据读到。
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Page {
    /// 仓库工作区（含两类仓库，按归属裁剪内容）
    Repos,
    /// 总览：只有统计，没有"当前仓库"这个对象
    Overview,
    /// CLI 工具屏（阶段一：只读扫描展示）
    CliTools,
}

/// 侧栏的仓库分组。导航轨第 2、3 个按钮与侧栏两个分组标题共用这套口径。
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Group {
    /// 我的仓库
    Mine,
    /// 第三方仓库
    Third,
}

impl Group {
    pub(crate) fn is_third(self) -> bool {
        matches!(self, Group::Third)
    }

    /// 在 `Shell::group_open` 里的下标
    pub(crate) fn ix(self) -> usize {
        match self {
            Group::Mine => 0,
            Group::Third => 1,
        }
    }

    /// 这一组在界面上叫什么、配哪个图标。
    ///
    /// **只有侧栏组标题一处用**（导航轨那些键的 `label` 是各自的字面量，
    /// 见 `shell.rs::nav_rail`）。侧栏只画 `side_focus` 那一组，所以这里
    /// 取的是"当前这一组"的名字——没有"某一组"这个入参。
    pub(crate) fn label_and_icon(self) -> (&'static str, gpui_kit::assets::IconName) {
        use gpui_kit::assets::IconName as AssetIcon;
        match self {
            Group::Mine => ("我的仓库", AssetIcon::GitBranch),
            Group::Third => ("第三方仓库", AssetIcon::Users),
        }
    }
}

/// 拖进来的仓库落哪一组；`None` = **这一刻不接受拖放**（D78）。
///
/// ## 为什么是 `side_focus` 而不是"选中的那个仓库"
///
/// 用户的话是"**在哪个类别拖就进当前分类**"——他指的是**眼睛看着的那一组**，
/// 也就是侧栏标题上写着的那一组（`side_focus`）。选中项在拖放这一刻完全可能
/// 是另一组的仓库：侧栏只画一组，跨组的选中项不会被切走（`reveal_repo`
/// 才会切），所以拿 `selected_is_third()` 去判，落库结果能差一整组。
///
/// ## 总览页为什么不收
///
/// 总览页**没有侧栏**，也就没有"当前这一组"可言——`side_focus` 在那一屏
/// 只是"上次看的那组"，用户看不见它。拿一个看不见的状态替他决定归属，
/// 等于凭空猜。所以这一屏直接不收：Windows 的复制光标是**平台按窗口**
/// 给的（`gpui-pre-windows/…/window.rs:1169` 直接回 `DROPEFFECT_COPY`），
/// 改不掉，那就至少别在界面上假装能放——落点提示不画，松手时说一句去哪放。
///
/// **纯函数**：这一页不碰任何状态，`state.rs` 的单测直接钉它（见文件末尾）。
pub(crate) fn drop_target(page: Page, side_focus: Group) -> Option<Group> {
    match page {
        Page::Repos => Some(side_focus),
        // 总览页没有侧栏，也就没有"当前这一组"可言（见函数注释）。
        // CLI 屏同理：它压根不是仓库列表，拖一个文件夹进来没有落点
        Page::Overview | Page::CliTools => None,
    }
}

/// 导航轨三个按钮点下去各自意味着什么。
/// 前两个按钮的差别不是"切到哪一屏"，而是"侧栏看哪一组"（D2）。
/// `CliTools` 是真正切屏（第三种 `Page`），与它们是两回事。
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Nav {
    Overview,
    Group(Group),
    /// CLI 工具屏（阶段一新增）
    CliTools,
}

pub(crate) struct Shell {
    /// 窗口句柄：异步跑完（初始化仓库等）要回来关弹窗
    pub(crate) win: AnyWindowHandle,
    /// **壳层根节点自己的焦点句柄。**
    ///
    /// 快捷键监听器挂在根 `v_flex` 上，而 Action 派发走的是
    /// "焦点节点 → 逐级往上冒泡"这条路（`Window::focus_node_id_in_rendered_frame`
    /// + `dispatch_path`）。**一个焦点都没有时**，派发落到 dispatch tree 的
    /// **根节点**（`DispatchNodeId(0)`，是 gpui-kit 包的那层 `Root`），
    /// 路径就只剩它自己——壳层的监听器在更深一层，一次都收不到。
    /// 而"刚启动、用户还没点过任何东西"正是最需要按 `Ctrl K` 的时刻。
    ///
    /// 所以启动时把焦点落在根节点上：之后无论焦点在壳层里的哪个控件
    /// （冒泡会一路走到根），还是哪个都没聚焦（根节点自己就是派发终点），
    /// 监听器都收得到。对话框是根节点之外的兄弟，`Esc` 仍然先被它吃掉
    /// （见路线表 D33），那是对的。
    pub(crate) focus: FocusHandle,
    pub(crate) page: Page,
    /// 侧栏当前聚焦的分组。导航轨第 2、3 个按钮点它，第 4 阶段再补展开与定位。
    pub(crate) side_focus: Group,
    /// 侧栏是否折叠。**这是用户自己的选择**，进总览页不改它
    /// （总览页没有侧栏是"那屏没有侧栏"，不是"用户把侧栏折了"，
    ///  两者混在一起会让用户下次回仓库页发现侧栏不见了）。
    /// 阶段 3 起宽度与折叠态都落盘，见 core 的 `store::set_sidebar_layout`。
    pub(crate) sidebar_collapsed: bool,
    /// 侧栏当前宽度（px，已夹在 200–360）。拖动结束后由 `on_resize` 写回这里，
    /// 折叠再展开时恢复的就是它。
    pub(crate) sidebar_w: f32,
    /// **窗口为了放得下这两栏，向右撑出去的那一段**（0 = 从没撑过）。
    ///
    /// 只在收起一栏时按原样还回去：加了多少减多少，**用户自己拖过的窗口尺寸不碰**。
    /// 判据、三条守则与"为什么必须记住这一个数"都在 `shell.rs::ensure_shell_width`
    pub(crate) shell_grown_w: f32,
    /// 上一次已经发出去、**还没兑现**的那个窗口宽目标（客户区宽，逻辑 px；
    /// 撑宽与收窄都记在同一格里）。
    /// ⚠️ **必须有这一格**：`Window::resize` 在 Windows 侧是 `spawn` 出去的
    /// `SetWindowPos`（`gpui-pre-windows/window.rs:652-674`），发出去到真正生效
    /// 中间那一两帧 `window.bounds()` 读到的**还是旧宽** ⇒ 不记目标就会每帧重发一次、
    /// `shell_grown_w` 被重复累加，收起一栏时把窗口减得比原来还窄。
    /// ⚠️ **兑现了就要清**（`ensure_shell_width` 开头那两行）：不清的话"同一个目标
    /// 只发一次"那条守则会把**下一次**该发的挡掉——用户自己把窗口拖窄之后我们
    /// 该再撑回去，却被上一次的目标钉住。
    pub(crate) shell_grow_pending: f32,
    /// 侧栏宽度的**代次**。只在 `commit_sidebar_width`（拖拽松手）里自增，
    /// 并且**只用来给弹簧的 id 换个后缀**——见 `shell.rs` 里那段。
    ///
    /// ## 为什么需要它
    ///
    /// `spring()` 把状态挂在 `window.use_keyed_state(id, ..)` 上，静止在目标上时
    /// **直接返回、不推进**（`motion.rs:609`）。于是拖拽这一路是脱节的：
    /// 用户把面板从 220 拖到 300，**面板**到了 300，可弹簧的内部位置还停在 220。
    /// 松手时 `sidebar_w` 变成 300 ⇒ 目标变了 ⇒ 弹簧从 220 起步往 300 弹，
    /// 而 `resize_panel` 每帧把它按在弹簧值上——**面板被拽回 220 再弹过去**。
    ///
    /// 用户报的是「侧栏在拉大拉小时会弹簧一下很难受」，就是这个反向抽搐，
    /// 而且**每次拖拽必现**。它不是"偶尔抖一下"，是稳定复现的整段回抽。
    ///
    /// ## 修法为什么是换 id
    ///
    /// 弹簧的内部状态在 `use_keyed_state` 里，外部**没有 API 能直接写它的 position**。
    /// 换一个新 id 就是换一份全新状态，而 `use_keyed_state` 的初值是
    /// `position = target`（`motion.rs:586-592`）——正好是"就地静止在新宽度上"。
    ///
    /// ⚠️ **不会泄漏**：keyed state 挂在 window 上，当帧没被访问的会在
    /// `next_frame.clear()` 时丢掉（`window.rs:3347`）。换 id 的当帧旧状态
    /// 就不再被访问 ⇒ 同帧回收。
    ///
    /// **代价**：拖拽中途若 `sidebar_w` 再变（例如松手后又立刻拖），会再换一次 id
    /// 重新采纳——这正是想要的语义（每次非动画的宽度变更都重新采纳）。
    pub(crate) sidebar_gen: u32,
    /// 「正从资源管理器往窗口里拖东西」。只用来决定**画不画那张落点提示卡**。
    ///
    /// **不要拿它当「拖拽进行中」的权威状态**——权威是基座的 `cx.active_drag`，
    /// 那里才有真正的拖拽会话。这是个纯展示用的镜像。
    ///
    /// 三个写点分别对应基座三个事件（`drag_drop.rs` 模块头有那张表）：
    /// `on_drag_move`（进入窗口）置 true、`on_file_drop_exit`（拖出去）置 false、
    /// `on_drop`（松手）置 false。第三个最容易漏——漏了的话提示卡会**一直挂着**，
    /// 而它的 `.absolute().inset_0()` 又正好盖住整个界面。
    pub(crate) dragging: bool,
    /// 「侧栏 / 主区」这条分隔的运行时状态。宽度、拖拽命中区、把手外观都在这里，
    /// 不要自己再存一份宽度——两份宽度必然有一帧对不上
    pub(crate) split: Entity<ResizableState>,
    /// 检视抽屉是否展开（仅仓库页有）
    pub(crate) inspector_open: bool,
    /// 检视抽屉当前宽度（px，已夹在 core 的 `INSPECTOR_W_RANGE`：.0 下限 / .1 上限）。
    /// 拖完由 `on_resize` 写回，收起再展开恢复的就是它。
    ///
    /// **默认宽度不在这儿再写一份**：右栏从"写死 320 的固定列"变成"用户能拖、
    /// 且存得住"的列之后，宽度成了一份**会被用户改的状态**——界面常量与落盘值
    /// 各留一份必然有一帧对不上。老配置没这个字段时由 core 的 `DEFAULT_INSPECTOR_W`
    /// 兜底，`Shell::new` 里夹一次范围，界面只管用 `Shell::inspector_w`
    pub(crate) inspector_w: f32,
    /// 检视抽屉宽度的代次。**与 `sidebar_gen` 是同一个东西的两份**，
    /// 理由、换 id 为什么能重新锚定、为什么不会泄漏 keyed state，
    /// 全部见 `sidebar_gen` 的注释。
    ///
    /// ⚠️ **两栏各有一份，不是漏抄**：它们是两个独立的 `ResizableState`
    /// （`split` 与 `insp_split`），动画 id 必须各自独立，否则两个弹簧
    /// 会共用一份 keyed state、互相把对方拽走。
    ///
    /// 2026-10-05 实机：左栏修完用户立刻报「右栏还是会弹簧一下」——
    /// **同一个 bug 的第二份拷贝**。凡是"两处用了同一套写法"的地方，
    /// 改完一处要**回头把另一处也改了**，并写明为什么它们必须各留一份。
    pub(crate) insp_gen: u32,
    /// 「我的」这一类**藏了哪些模块**（阶段 19）
    ///
    /// 存字段而不是每帧 `store::load()`：读配置要开一次文件，
    /// 页签栏与概览四段每帧都要问"这个模块显不显示"。
    /// 与 `sidebar_collapsed` / `inspector_open` 同一口径——启动读一次，
    /// 设置里改了再写回来
    pub(crate) hidden_mine: Vec<String>,
    /// 「第三方」这一类藏了哪些模块。**两套不共用一份**：用户的话是
    /// "我的和第三方只是操作功能不一样"，可勾选也该分得开
    pub(crate) hidden_third: Vec<String>,
    /// 「主区 / 检视抽屉」这条分隔的运行时状态。**与 `split` 是两个独立分组**：
    /// 侧栏那组只有仓库页有、且总览页整组不挂，混进同一组会让面板下标
    /// 随页面变，下标与列的对应关系就不再固定
    pub(crate) insp_split: Entity<ResizableState>,
    /// 仓库头地址区刚复制过的是哪一条（`"local"` / `"remote"` / `None`）。
    ///
    /// **不为 0 的原因**：复制成功要有个看得见的反馈，而反馈不能是
    /// `push_notification` 气泡——那正是这一版要取消的东西（用户要求）。
    /// 按钮自己在复制后短暂换成对号，这一份状态就是那个"短暂"的计时依据
    pub(crate) copied: Option<&'static str>,
    /// 「复制」反馈要翻回去的时刻（秒级时间戳）。`copied` 是"现在是复制态"，
    /// 这一份是"什么时候变回去"——两件事分开存，才不用每帧自己算 elapsed
    pub(crate) copied_until: u64,
    /// 地址区当前被鼠标悬停的是哪一行（`"local"` / `"remote"` / `None`）。
    ///
    /// **为什么不能靠 gpui 的 `:hover`**：gpui 没有 CSS 那样的伪类，悬停只能靠
    /// `on_hover` 回调（`div.rs:675`）把状态写回来。而那个闭包是 `'static`、
    /// 捕不了 `&self`，所以显隐判定只能放在 `Shell` 上。
    /// 复制键用 `when(hovered, ..)` **真不挂进树**，不是 `opacity(0)`——
    /// 透明元素照样能点到，那会变成"看不见但能按"的一颗键
    pub(crate) addr_hover: Option<&'static str>,
    pub(crate) repos: Vec<RepoEntry>,
    pub(crate) views: HashMap<String, RepoView>,
    pub(crate) selected: Option<String>,
    /// 侧栏过滤框（官方 Input）。过滤规则见 `sidebar.rs::matches_filter`
    pub(crate) side_filter: Entity<InputState>,
    /// 过滤框是否展开（D8：默认收起，不常年占侧栏里 32px 高度）
    pub(crate) filter_open: bool,
    /// 两个分组各自的展开态：`[我的, 第三方]`。
    ///
    /// **不能**用 `SidebarMenuItem` 的子菜单开合：那玩意把开合态存在
    /// `window.use_keyed_state` 里，外部拿不到，而导航按钮与总览点行
    /// 都要从外面把某一组打开。所以分组标题不带子项，开合由这里管。
    pub(crate) group_open: [bool; 2],
    /// 仓库树的滚动句柄。定位某一行时用 `bounds_for_item` 把它滚进视野
    pub(crate) tree_scroll: ScrollHandle,
    /// 空间页**整页**那一列的滚动句柄（2026-10-09 第 1 条）。
    ///
    /// 为什么只有它需要单独一个句柄：这一页的滚动盒在**卡的外面**（一张页里
    /// 叠着五张段卡，滚的是整页），别的三页滚的是卡内那一格。
    /// 基座对"卡内格"有现成的落位办法（`tokens::scroll_cell` 把格子探出去），
    /// 这一页探不出去——探出去就顶到窗口右缘了。⇒ 这一页的滚动条要**自己摆一层
    /// 覆盖层**（`widgets` 里那一颗），而覆盖层要拿到滚动盒的句柄才知道
    /// 视口多大、滚到哪儿了
    pub(crate) space_scroll: ScrollHandle,
    // ---------- 「日志」区 ----------
    //
    /// 输出流的滚动句柄（阶段 21-1）。**它要解决的是"日志不自己往下走"**：
    //  `log.rs::output_stream` 早先只挂了 `.overflow_y_scroll()`，全项目
    //  没有任何一处调过滚动 API，于是新行追加时视口**纹丝不动**——
    //  git 的结论（最后那几行才是）永远看不见。
    //
    //  ⚠️ **必须由调用方给 id**，理由与它自己那份注释同源：gpui 的滚动状态按
    //  `ElementId` 全局存，概览页的日志与批量结果对话框那份共用一个 id 就会
    //  共用同一个滚动句柄——在对话框里滚一下，底下概览页的日志跟着跳。
    //  所以两个调用点各挂各的（`log-stream` / `app-log`）。
    pub(crate) log_scroll: ScrollHandle,
    /// ⚠️ **这里没有「用户滚过就停下」那个 bool**（阶段 21-1）。
    ///
    /// 早先加过一个 `log_stick_to_bottom`，实测它**只写不读**——判据其实是
    /// `log.rs::should_follow_bottom` 每帧拿 `offset` 与 `max_offset` 比出来的，
    /// 这个字段谁也没读。留着的后果是**两处判据、只有一处生效**，
    /// 下一个人照着字段名去改就会改到那个没人读的地方。
    ///
    /// 为什么判据是「贴不贴底」而不是「滚没滚过」：用户自己滚回底部后
    /// 跟随就该自动恢复，否则会留一个只有用户自己摸得出来的怪状态
    /// （"滚到底之后日志再也不往下走"）。
    /// 批量结果对话框里那份输出的滚动句柄（阶段 21-1）。
    ///
    /// **必须与 `log_scroll` 分开**：gpui 的滚动状态按 `ElementId` 存，
    /// 而批量汇总与概览页日志共用同一份 `LogState`——共用一个句柄的话，
    /// 在对话框里滚一下，底下概览页的日志视口会被一起拽走。
    pub(crate) batch_log_scroll: ScrollHandle,
    // ---------- 「说明」页签 ----------
    //
    /// 正文的 `TextViewState`。**自己建而不用 `TextView::markdown(id, text)`
    /// 那个便捷构造**：README 里的 `#锚点` 链接要跳到对应标题，
    /// 那条路是 `TextViewState::reveal_range(..)`，只在 state 上有，
    /// 便捷构造把 state 藏在元素里就够不着了。
    ///
    /// 滚动**不用**自己管：`.scrollable(true)` 的 TextView 内部是一个
    /// `gpui::list`，基座文档明写「`gpui::list` 自己会滚到那一行」
    pub(crate) readme_state: Entity<TextViewState>,
    /// 切文档那一次后台读文件的任务句柄。
    ///
    /// ⚠️ **`Task` 掉了任务就没了**（gpui 的 `Task` 是取消令牌，drop 即取消），
    /// 所以 `cx.spawn(..)` 的返回值必须存住。重开一次就先把它换成新的——
    /// 旧句柄在这里析构，顺带取消上一轮还没回来的那次读
    pub(crate) readme_task: Option<Task<()>>,
    /// 待定位的仓库路径：设上之后下一帧滚到那一行并清空。
    /// 一次性的，别留着当"当前关注行"用
    pub(crate) reveal: Option<String>,
    /// 克隆成功后的待定位路径。**与 `reveal` 分开**：`reveal` 是
    /// "滚到那一行"（`apply_reveal` 消费），这个是"完整走一遍
    /// `reveal_repo`"（切页 + 展开侧栏 + 选中 + 扫描）。
    /// 两者都只能在下一次 `render` 里兑现——后台任务里既没有
    /// `&mut Window`，也不能重入 `Shell::update`
    pub(crate) pending_reveal: Option<String>,
    /// 「浏览…」点过了：设上之后**下一帧**拉起系统目录选择器并清空。
    ///
    /// ## 为什么必须是标记而不是就地调
    ///
    /// `IFileOpenDialog::Show` 要一个活的 `&mut Window`（回填 `InputState` 用），
    /// 而对话框正文那个闭包只有 `&mut App`；在 `on_click` 里调
    /// `cx.update_window(..)` 会**静默失败**——那一刻窗口已经被
    /// `App::update_window_id` 从 `cx.windows` 里 take 走了，整个闭包被丢掉，
    /// 外面那个 `.ok()` 再把错误吞掉，三处都不出声，表现为"点着没反应"
    /// （AGENTS.md 记着这个坑，`repo_head.rs` 的复制按钮也栽过）。
    ///
    /// 与 `pending_reveal` 同一个套路：**置位 → `Render::render` 兑现**，
    /// 那一帧里 `&mut self` 与 `window` 都在手边。
    ///
    /// ⚠️ **标记里必须写清"是哪一个弹窗要选目录"**（2026-10-09）。原来它是 `bool`，
    /// 而"选完回填到哪一格"是两个弹窗各自的 `InputState`：`bool` 只能表达"有人要过一次"，
    /// 加第二个使用者时要么回填错格子、要么在兑现处再猜一遍当前弹窗——
    /// 正是 D78 那一类（**必填信息没进签名 ⇒ 每条调用路径都得自己记得补，其中一条一定漏**）。
    /// 现在做成 `Option<FolderPick>`：新加一个使用者**不写变体就编译不过**。
    pub(crate) pending_folder: Option<FolderPick>,
    /// 点关闭键时怎么办（阶段 22）。**从配置读一次缓存在这里**，
    /// 不是每次关窗去 `store::load()` 读一遍文件
    pub(crate) close_action: CloseAction,
    /// 关窗询问面板**待弹**：由 `on_window_should_close` 置位，
    /// 由 `Render::render` 下一帧兑现（理由与 `pending_folder` 同源：
    /// 那个回调跑在 `WM_CLOSE` 处理过程中，不能在那儿拉起模态浮层）
    pub(crate) pending_close_prompt: bool,
    /// 面板里「记住我的选择」那颗勾的当前状态。**每次开面板重置为 false**——
    /// 用户主动勾上才叫"记住"，上一次勾过不等于这一次也要记
    pub(crate) close_remember: bool,
    /// 「完全退出」待执行：由 `on_window_should_close` 的 `Exit` 档置位，
    /// 下一帧调 `App::quit()`。**不直接返回 `true` 让系统关窗**——
    /// 那样只是销毁窗口，而 `QuitMode::Explicit` 下进程会留着
    pub(crate) pending_app_quit: bool,
    pub(crate) scanning: bool,
    /// 上次状态扫描完成的秒级时间戳，0 = 本会话还没扫完过。
    /// 状态条展示"上次扫描 …"，点它重扫。
    pub(crate) last_scan_ts: u64,
    /// 状态扫描的任务句柄：存着才能在下一次扫描时丢掉上一次
    pub(crate) scan: Option<Task<()>>,
    /// 扫描代数：过期结果不许覆盖新结果
    pub(crate) seq: u64,
    /// 每个仓库一份的日志（D7）。键是仓库路径，**别再存第二份全局的**——
    /// 那样后台任务回来只能写到"当前选中"名下，切个仓库就记错地方了
    pub(crate) logs: HashMap<String, LogState>,
    /// 每个仓库**最近一条**操作历史（阶段 16 的 `op_history`）。
    ///
    /// 与 `logs` 的分工：`logs` 是**本次会话**的输出流，跑完就没了；
    /// 这一份是**落盘的**，回答「这个仓库上次被更新过什么、上次是什么时候」。
    /// 没有它的话重启一次，日志卡就写「还没有操作记录」——而昨天明明拉过。
    ///
    /// **只存一条**（`op_history` 表里是全量追加，界面只问"最近那条"）。
    /// 存更多就得决定摆在哪、怎么翻页，而那是另一个设计决定。
    /// 切仓库时按需读一条，**别在 render 里读库**——那是每帧 I/O。
    pub(crate) op_logs: HashMap<String, qingbird_core::scan_cache::OpRecord>,
    /// 详情页当前页签（概览 / 说明 / 历史 / 空间 / 变更）。
    /// 跟着仓库走还是全局？**全局**：切页签不是切仓库，用户的注意力
    /// 应该在"看同一个仓库的另一个角度"上；跟着仓库走的话每换一个仓库
    /// 都要重新点回「概览」，多仓库用户会疯
    pub(crate) detail_tab: usize,
    pub(crate) space: SpaceState,
    /// 磁盘扫描的代数：切仓库 / 重扫时作废旧结果
    pub(crate) space_seq: u64,
    pub(crate) space_task: Option<Task<()>>,
    pub(crate) profile: ProfileState,
    pub(crate) profile_seq: u64,
    pub(crate) profile_task: Option<Task<()>>,
    /// CLI 工具屏的数据（阶段一：只读扫描展示）。**一份全局**——
    /// CLI 工具是整台机器的，与"当前选中哪个仓库"无关
    pub(crate) cli: CliState,
    /// CLI 扫描的代数：过期结果不许覆盖新结果
    pub(crate) cli_seq: u64,
    pub(crate) cli_task: Option<Task<()>>,
    /// 量数据目录体积那条后台通道（与 `cli_task` **分开一个句柄**：扫描一结束
    /// 它就没了，而体积是扫完之后才一条一条量的）
    pub(crate) cli_size_task: Option<Task<()>>,
    /// CLI 屏左栏（工具列表）是否收起。
    ///
    /// ⚠️ **与 `sidebar_collapsed` 各存一份，不是漏抄**：两屏的左栏共用同一套面板
    /// 机制（`body-split` 的 0 号面板 + `self.sidebar_w` 宽度来源），但"折没折"
    /// 是两个独立的意图——设计 §10.1 明写「两者互不干扰」。
    /// **不落盘**：它是一屏的读法（与 `ov_sort` 同口径），不是偏好
    pub(crate) cli_collapsed: bool,
    /// 「历史」页签的数据。只存一份在这里，页签与检视抽屉都从这儿读
    pub(crate) history: HistoryState,
    /// 提交列表（官方 `List`）。委托只带一个回指 `Shell` 的 Entity，
    /// 条数与内容每帧从 `Shell::history` 取——和总览表格同一个套路
    pub(crate) history_table: Entity<ListState<CommitDelegate>>,
    /// 总览页勾选的仓库路径 —— **阶段 6 已删**。
    /// 批量同步改由 ⌘K 承载（阶段 11），复选框那一套连同它的选中状态一起消失
    pub(crate) ov_table: Entity<TableState<crate::overview::OvDelegate>>,
    /// 总览页的仓库行快照。`ov_table` 的委托读它，
    /// 免得委托里存第二份（两份必然有一帧对不上）
    pub(crate) ov_rows: Vec<OvRow>,
    /// 那张卡的行序（卡头右上那颗键切它）。**不跨会话记**：
    /// 它是一屏的读法而不是偏好，切错了再点一次就是，落盘反而多一份要同步的状态
    pub(crate) ov_sort: OvSort,
    /// 每颗仓库**最后一次成功与云端交换**的时间（键是归一后的路径）。
    ///
    /// ⚠️ `None` = 那张表**读不出来**，而**不是**"所有仓库都没有同步记录"。
    /// 这两件事在界面上必须是两个样子：混起来的后果不是少一个数，
    /// 而是那颗筛选片一点下去，整张表看起来"全都三个月没同步"
    /// （31.4 补十五给 `RepoStatus` 补的 `status_read` 是同一族）。
    /// ⇒ 读不出来时那颗片**禁用**并把原因说出来，不猜
    pub(crate) last_sync: Option<std::collections::HashMap<String, u64>>,
    /// 那颗「久未同步」片按下去没有。一屏的读法，不跨会话记（同 `ov_sort`）
    pub(crate) ov_stale_only: bool,
    /// 片上那两个数：`(超过 90 天没交换的, 没有一次成功同步记录的)`。
    /// 由 `sync_ov_rows` 在**过滤之前**算好存进来，render 只读
    pub(crate) ov_stale_counts: (usize, usize),
    /// 全仓库合并的近 14 天提交数（后端只给有提交的日子）
    pub(crate) ov_activity: Vec<ActivityDay>,
    /// 活跃度扫描的代数：过期结果不许覆盖新结果
    pub(crate) ov_seq: u64,
    pub(crate) ov_task: Option<Task<()>>,
    /// 「本会话还没为总览页扫过活跃度」。
    ///
    /// 扫描挂在 `Shell::go(Nav::Overview)` 上，但**冷启动就停在总览页**
    /// （`new()` 里 `page: Page::Overview`），没有那次点击就永远不扫——
    /// 画出来是一排 0 高度的柱子，读起来像"近两周没人提交"（2026-10-03 实机首轮）。
    /// 一次性标记由 `render` 消费，点导航轨回来那条路仍走 `go`，两条路不重复扫
    pub(crate) ov_need_scan: bool,
    /// 批量同步的进度与结论；None = 本会话还没跑过批量。
    /// 入口是 ⌘K 的三条固定命令（Q3）。汇总落在 `app_log` 而不是某一个仓库
    pub(crate) batch: Option<BatchRun>,
    /// 批量跑完那一句**通知**的一次性标记，由 `render` 消费。
    ///
    /// 为什么不走 `this.win` + `cx.update_window` 那条现成的路：并发之后
    /// "最后一个完成"的那颗**可能是同步结清的脏跳过**（它一秒都不跑，
    /// 不占并发位），那条路是在主线程的 `update` 闭包里走到的——正是
    /// 「窗口已被 `update_window_id` take 走，`cx.update_window` 把整个闭包
    /// 静默丢掉」那一档（AGENTS 记过两次，症状都是"点了完全没反应"）。
    /// 两条路共用这一个出口，就不用去判断"我是从哪条路收尾的"
    pub(crate) pending_batch_note: Option<String>,
    /// 应用级日志。**只有跨仓库的批次往这里写**——按仓库存的那份在 `logs`，
    /// 一条都不该混进这个槽
    pub(crate) app_log: LogState,
    /// 添加 / 编辑仓库弹窗；None = 弹窗未打开
    pub(crate) dlg: Option<RepoDialog>,
    /// 当前选中仓库的分叉信息（ahead>0 且 behind>0 时由状态扫描顺手算出）
    pub(crate) fork: Option<ForkInfo>,
    /// 刚做完变基的备份分支名：有这个值界面才提供「撤销变基」，切仓库即失效
    pub(crate) fork_backup: Option<String>,
    /// 分叉卡「只看差异」是否展开
    pub(crate) fork_diff: bool,
    /// 「撤销变基」处于二次确认状态（会丢弃变基后的新提交，必须确认）
    pub(crate) fork_confirm: bool,
    /// 提交并推送对话框；None = 未打开
    pub(crate) commit_dlg: Option<CommitDialog>,
    /// 「一键清理构建缓存」的确认对话框；None = 未打开。
    /// **必须先过这个框**——`target` 删掉意味着整个工程要重新编译几十分钟，
    /// 这种代价不给一次「你确定」是不能接受的
    pub(crate) clean_dlg: Option<CleanDialog>,
    /// 第三方「清掉多出来的文件」逐项勾选框；None = 未打开。
    /// **只给第三方**（17-4）：自己的仓库要做这件事有正经工具链，
    /// 在这里给一条「一键丢工作区」的路是帮倒忙
    pub(crate) purge_dlg: Option<PurgeDialog>,
    /// 「收起来的改动」清单（32-2 的那个出口）；None = 未打开。
    /// 入口是仓库名后面那颗橙色三角，而它出现的判据是 `RepoStatus::stash_count > 0`
    /// （判据在 `panels::sync_alert`，**不在这里再判一遍**）
    pub(crate) stash_dlg: Option<StashDialog>,
    /// 克隆对话框；None = 未打开。**与添加/编辑仓库那个 `dlg` 分开**：
    /// 那个是"把已经在磁盘上的目录登记进来"，克隆是"先从网络取回来"，
    /// 两者字段几乎不重叠，挤在一个结构体里只会让两处各自判有没有
    pub(crate) clone_dlg: Option<CloneDialog>,
    /// 命令编辑对话框；None = 未打开
    pub(crate) cmd_dlg: Option<CmdDialog>,
    /// 设置对话框；None = 未打开。导航轨那颗齿轮与 ⌘K「设置」共用这一个入口
    pub(crate) settings_dlg: Option<SettingsDialog>,
    /// 软件自更新这条链的状态（Q44）。**挂在 Shell 上不挂在设置弹窗上**：
    /// 下载是几十秒的长活，弹窗关了下载照走、重开进度还在
    pub(crate) upd: crate::self_update::UpdFlow,
    /// 「同步设置」页签正在编辑的档位与开关。**存草稿而不是直接改配置**：
    /// 这一页有三项，挨个点一下存一次会让"取消"变成假的——用户撤销不了
    pub(crate) sync_cfg: SyncCfg,
    /// 命令面板的官方 `CommandState`（查询词、焦点、高亮项都在它身上）。
    /// **整个应用只建这一个 Entity**：官方 `Command` 每次渲染都会把条目
    /// 重新装进状态，重建 Entity 等于每帧丢一次查询与选中位置
    pub(crate) palette: Entity<CommandState>,
    /// 面板里每一条对应的动作，**下标与 `CommandGroup` 一一对应**。
    ///
    /// 官方的 `CommandItem::action` 要一个 `Box<dyn Action>`，而本 crate 的
    /// Action 全是 `no_json` 的 unit struct（带 payload 的 Action 要
    /// `serde::Deserialize` + `schemars::JsonSchema`，这条链对这个 GUI 库不划算）。
    /// 于是不走 Action，改成"渲染时把清单存下来、回车时按下标取"——
    /// 这也是官方 `on_confirm` 给 `IndexPath` 的用意
    pub(crate) palette_actions: Vec<Vec<PaletteAction>>,
    /// 面板是否已打开。连按 `Ctrl K` 不该叠出两层对话框
    pub(crate) palette_open: bool,
}

/// 「一键清理构建缓存」确认框的状态
///
/// 打开那一刻把要删的目录**逐项快照**下来（名字 + 各自的体积）。
/// 不存快照而在对话框里读 `report`：对话框开着的时候后台扫描可能正好回来
/// 一份新的 `report`，那几项就已经不是用户看到的那几项了
pub(crate) struct CleanDialog {
    pub(crate) path: String,
    /// `(目录名, 字节数)`，按体积从大到小——先告诉用户最贵的那项
    pub(crate) items: Vec<(String, u64)>,
}

/// 阶段 17：第三方仓库「多出来的文件」逐项勾选框的状态
///
/// **两类东西必须分开列**（`purge.rs` 的分流是这一层的前提）：
/// - `untracked` = 从没进过版本控制 → 勾选意味着**永久删除**
/// - `tracked` = 已跟踪但被改 / 被删 / 被 add 过 → 勾选意味着**恢复成云端版本**
///
/// 摆成一列是危险的：用户看到两个长得差不多的条目，不会去想"这个删了能撤回吗"，
/// 而**两者的可逆性完全不同**。
///
/// 清单是**打开那一刻的快照**：对话框开着的时候后台状态扫描可能正好回来一份新的，
/// 那几项就已经不是用户勾的时候看到的那些了。core 那层会再扫一次取交集兜底。
///
/// `Clone` 是对话框的 `content` builder 每帧现读时要用（`batch_log.rs` 同一套）
#[derive(Clone)]
pub(crate) struct PurgeDialog {
    pub(crate) path: String,
    pub(crate) untracked: Vec<String>,
    pub(crate) tracked: Vec<String>,
    /// 勾中的那些。⚠️ **默认空，一个都不许预勾**——这是不可逆操作，
    /// 替用户勾好等于替他决定。`init` 向导那套"常见构建产物默认勾上"是
    /// 反过来的理由：写 `.gitignore` 是可逆且无害的
    pub(crate) picked: HashSet<String>,
}

/// 「收起来的改动」清单（32-2）。
///
/// ## 为什么每行自带动作、不给 footer 那颗统一执行键
///
/// `dialogs/purge.rs` 那套是"勾选 + footer 执行"，而它的 footer **定不住
/// "一个都没勾"那一档**（官方 `Dialog::footer` 收的是构建好的元素，不是每帧重调的
/// 闭包，见那文件的注释）。这一屏动的是"取回/丢掉"，其中**丢掉不可逆**——
/// 出现"点了没反应"或"不知道点的是哪条"都是不可接受的。⇒ 动作与对象并在同一行，
/// 结构上就没有"选了却没处执行"和"执行了却不知道选的是哪条"这两种状态（D78 同形）
///
/// ## 二次确认存的是（序号 + 消息原文），不是"那一条"
///
/// `drop` 之后所有序号整体前移。core 那层动手前会**当场重读清单比对原文**
/// （`stash::resolve`），对不上就一条都不动 ⇒ 这里必须把原文一起带上，
/// **序号只是定位用，身份是消息**
#[derive(Clone)]
pub(crate) struct StashDialog {
    pub(crate) path: String,
    /// 打开那一刻、以及**每次动手之后**重读的清单
    pub(crate) entries: Vec<StashEntry>,
    /// 正在等二次确认的那一条：（序号，消息原文）。None = 没有
    pub(crate) confirm: Option<(u32, String)>,
    /// 有一步正在跑。**防连点**，也防"确认框还挂着、清单已经被换掉"
    pub(crate) busy: bool,
    /// 32-7：展开了"这一条里是哪几个文件"的那一条：（序号，消息原文，读出来的结果）。
    /// ⚠️ 与 `confirm` 一样带**原文**：`drop` 之后序号整体前移，只存序号的话
    /// 展开区会挂到别的条目身上，而它显示的是别人的文件名
    ///
    /// ⚠️ **两层 Option 各说一件事**：外层 = 这一条有没有被展开；内层 = 读到了
    /// （`Some`，**空表是"这一条真的没有文件变化"**）还是读不出（`None`）。
    /// 并成一层就会把"读不出"显示成"没有文件"——core 那颗 `files_at` 之所以返回
    /// `Option<Vec<_>>` 就是为了这一格不必在末端挑一句假话
    ///
    /// ⚠️ 只存**一条**（不是 `HashMap`）：点开第二条就把第一条收掉。每条都留一份
    /// 的话这一屏会被撑成一列文件列表，而"这一条是什么"反而看不见了
    pub(crate) files: Option<(u32, String, Option<Vec<qingbird_core::stash::StashFile>>)>,
}

/// 提交并推送对话框的状态
pub(crate) struct CommitDialog {
    pub(crate) path: String,
    /// 提交信息输入框。**打开时已预填上次提交标题**（`InputState::set_value`）
    pub(crate) message: Entity<InputState>,
    /// ☐ 提交后拉取
    pub(crate) pull_first: bool,
    /// 打开那一刻的变更摘要。存快照而不是每帧重扫：
    /// 对话框开着的时候 git 状态不会自己变，重扫只会让列表闪
    pub(crate) changes: Vec<ChangeEntry>,
    /// 正在提交（按钮转圈、防连点）
    pub(crate) running: bool,
    pub(crate) error: Option<String>,
}

/// 命令编辑对话框的状态。
///
/// 输入框用**一张标签表 + 一个平行数组**配对（`CMD_FIELDS`），
/// 不写十个具名字段——具名的话「读出来 → 传给 `set_repo_cmds` /
/// `set_pinned_cmds`」那段要写十遍，漏一处的后果是某个槽位的命令
/// 悄悄存到别的槽位上
pub(crate) struct CmdDialog {
    pub(crate) path: String,
    /// 长度与 `CMD_FIELDS` 一致
    pub(crate) inputs: Vec<Entity<InputState>>,
    pub(crate) error: Option<String>,
    /// 前四个槽位**已经存着的**命令（下标同 `SLOTS`）。
    /// 那一行右侧的状态词（「未保存 / 已自定义 / 自动识别：x」）要拿**输入框当前值**
    /// 与它比，所以这份必须在打开时留一份快照，不能事后从 `entry` 现读——
    /// 现读会读到刚刚保存完的新值，「未保存」那一档就永远不出现了
    pub(crate) saved: Vec<String>,
    /// 前四个槽位**自动识别到的**命令（下标同 `SLOTS`）。
    /// 「自动」那颗键回填的就是这里，状态词里「自动识别：x」那一档读的也是它。
    /// ⚠️ **不许在打开时把它写进 `saved`**：识别结果不落盘是这一屏的约定
    /// （副标题明写"不点保存不会写进配置"），混成一格就变成"打开即改动配置"
    pub(crate) detected: Vec<String>,
}

/// 命令编辑对话框的字段表。**下标即 `CmdDialog::inputs` 的下标**，
/// 前四个是固定槽位，后六个是三个钉住命令的「文案 / 命令」成对字段。
///
/// 做成表而不是具名字段，是为了"读出来 → 交给 `set_repo_cmds` /
/// `set_pinned_cmds`"那段只写一遍：七个槽位各写一次下标推导的话，
/// 少写一个就意味着某个槽位的命令被存进了别的槽位
pub(crate) const CMD_FIELDS: [&str; 10] = [
    "测试",
    "构建",
    "开发",
    "安装依赖",
    "钉住 1 文案",
    "钉住 1 命令",
    "钉住 2 文案",
    "钉住 2 命令",
    "钉住 3 文案",
    "钉住 3 命令",
];

/// 设置对话框左侧那几个分类（阶段 20）
///
/// **为什么是左导航而不是页签**：那一排设置按「这是什么事」分才分得开
/// （身份与令牌是同一件事、代理与通知都是「出了事怎么找我」、显示是一类），
/// 而页签只能横排在顶部，四个名字会把 700px 宽的弹窗顶部切得很碎。
/// 左边一列、右边一屏，是设置这类"少而整"的内容最省地的形态
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetCat {
    /// 提交身份 / 访问令牌 / SSH 私钥
    Identity,
    /// 代理 / 系统通知
    Network,
    /// 主题 / 显示哪些模块
    Display,
    /// 快捷键（阶段 30）
    Keys,
}

impl SetCat {
    pub(crate) const ALL: &'static [SetCat] = &[
        SetCat::Identity,
        SetCat::Network,
        SetCat::Display,
        SetCat::Keys,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            SetCat::Identity => "身份与访问",
            SetCat::Network => "网络与通知",
            SetCat::Display => "显示",
            SetCat::Keys => "快捷键",
        }
    }

    /// `ElementId` 用的 ASCII 键。**不拿 `label()` 去拼 id**——
    /// 界面文案一改（"网络与通知" → "网络"）id 就跟着变，
    /// gpui 按 id 存的那份悬停状态当场作废，表现为"扫过没反应"
    pub(crate) fn key(self) -> &'static str {
        match self {
            SetCat::Identity => "identity",
            SetCat::Network => "network",
            SetCat::Display => "display",
            SetCat::Keys => "keys",
        }
    }

    /// 左侧那颗键旁边的一句话。**每个分类都要有**——只有名字的话，
    /// 「显示」这两个字说不清里面是主题还是别的东西。
    ///
    /// ⚠️ 阶段 31.5 从"一句自然语言"改成"中间点分隔的三个词"：
    /// 190px 那一列里长句会折成三行（2026-10-07 基线帧查实），而 `SET_NAV_W`
    /// 的注释②早就写着"说明句要能一行放下不断字"——**目标一直是对的，
    /// 内容没跟上**。词与词之间用 `·` 不用 `、`：前者是"并列的三样"，
    /// 后者读起来是一句话的停顿
    pub(crate) fn hint(self) -> &'static str {
        match self {
            SetCat::Identity => "名字 · 令牌 · SSH",
            SetCat::Network => "代理 · 系统通知",
            SetCat::Display => "主题 · 字号 · 模块",
            SetCat::Keys => "改键 · 解绑 · 恢复",
        }
    }

    /// 分类导航那一颗图标（阶段 31.5）。
    ///
    /// **图标在这里，不在 `set_nav()` 里 `match`**——导航那一行只管画，
    /// "这一类是什么"是分类自己的事。与 `label()` / `key()` / `hint()` 同一族
    pub(crate) fn icon(self) -> gpui_kit::assets::IconName {
        use gpui_kit::assets::IconName as I;
        match self {
            SetCat::Identity => I::Contact,
            SetCat::Network => I::Globe,
            SetCat::Display => I::Monitor,
            SetCat::Keys => I::Keyboard,
        }
    }
}

/// 设置对话框的状态。
///
/// **六个输入框的 Entity 存在这里而不是每帧重建**——重建会让用户每敲一个字
/// 就丢一次焦点。系统通知那个开关不占 Entity：它即时落盘，不参与「保存」
pub(crate) struct SettingsDialog {
    /// 左侧当前停在哪个分类
    pub(crate) cat: SetCat,
    /// 左侧哪一颗正被鼠标扫着。gpui 没有 CSS 的 `:hover`，悬停面只能靠
    /// `on_hover` 记状态（与 `Shell.hovered` 同一口径）——**没有这一格，
    /// 扫过一整列导航键全程零反馈**，用户不知道该点哪一颗
    pub(crate) nav_hover: Option<SetCat>,
    pub(crate) git_name: Entity<InputState>,
    pub(crate) git_email: Entity<InputState>,
    pub(crate) github_token: Entity<InputState>,
    pub(crate) gitee_token: Entity<InputState>,
    pub(crate) ssh_key: Entity<InputState>,
    pub(crate) proxy: Entity<InputState>,
    pub(crate) notify: bool,
    /// 当前是不是深色（阶段 20）。**只认深 / 浅两态**，
    /// 存进配置的 `system` 在界面上落浅色，见 `theme` 模块头
    pub(crate) theme_dark: bool,
    /// 当前界面字号档（阶段 28）。**即时落盘即时生效**，与深色那颗同口径
    pub(crate) ui_scale: UiScale,
    /// 点关闭键时怎么办（阶段 22）。存判据后的枚举而不是原始串：
    /// 界面拿它做 `==` 比对，不必每次都过一遍 `close_action_of`。
    /// **同一个值在 `Shell::close_action` 上也有一份**——那份是
    /// `on_window_should_close` 回调真正读的（它在消息处理里，不能读盘）
    pub(crate) close_action: CloseAction,
    /// 「检测代理」在跑（按钮转圈、防连点）
    pub(crate) probing: bool,
    /// 探测结果的一句话
    pub(crate) probe_note: Option<String>,
    /// 「我的」这一类**勾上 = 显示**的模块（阶段 19）
    ///
    /// **存"勾上"而不是"藏了"**是为了与界面上的复选框一一对应——
    /// 用户拨的是"这个模块给我看"，复选框就该直接跟着他拨的方向走。
    /// 落盘时再翻成 `hidden_*`（core 那边存"藏了哪些"是为了老配置兼容，
    /// 那是**存储**的选择，不是**交互**的选择，两个方向不矛盾）
    pub(crate) mod_mine: Vec<String>,
    /// 「第三方」这一类勾上 = 显示的模块
    pub(crate) mod_third: Vec<String>,
    /// 快捷键覆盖（阶段 30）。**存的是覆盖值不是生效值**：
    /// 没记过的键不在表里，界面自己去读默认（`keys::KEYS`）。
    /// 这样"没改过"与"解除了"分得开——压成生效值的话两者会合成同一个
    pub(crate) key_overrides: HashMap<String, String>,
    /// 正在录键的那一项（`store::KEY_*`）。`None` = 没在录
    ///
    /// 录制期间会挂一层 `NoAction` 遮罩把当前生效的键全盖住，理由与
    /// 代价见 `keys::shadows`
    pub(crate) recording: Option<String>,
    /// 录制器的焦点。**必须是真的焦点而不是"当前页刚好被画出来"**——
    /// 元素级 `on_key_down` 只在焦点节点的派发路径上收得到按键
    pub(crate) key_focus: FocusHandle,
    /// 哪一颗键帽正被鼠标扫着（阶段 30）。gpui 没有 CSS 的 `:hover`，
    /// 悬停面只能靠 `on_hover` 记状态——不记的话扫过一整列键帽全程零反馈
    pub(crate) key_cap_hover: Option<String>,
    /// 本次打开期间发生过"这把键被抢走了"的记录（`(被抢的动作, 抢它的动作)`）。
    ///
    /// **只活在这一次打开里，不落盘**：它是"刚刚发生了什么"的说明，
    /// 而用户随后改这两行里的任何一行，它就该消失。落一份历史反而会
    /// 在用户已经解决完之后还挂着一句过期的话
    pub(crate) key_yielded: Vec<(String, String)>,
    pub(crate) error: Option<String>,
}

/// 「同步设置」页签的草稿。
///
/// 编辑期间改动只落在这份草稿上，按「保存」才写进配置——这一页三项会互相影响
/// （档位一变，间隔与时间点谁生效就变了），逐项存会留下半套配置
pub(crate) struct SyncCfg {
    /// 档位下标：`0` = 不定时，`1..=5` = 每 N 分钟，`6` = 每天定时
    pub(crate) mode: usize,
    /// 检测到云端更新后自动拉取
    pub(crate) auto_pull: bool,
    /// 「每天几点」那个输入框。**Entity 存在草稿里而不是每帧重建**——
    /// 重建会让用户每敲一个字就丢一次焦点
    pub(crate) daily_input: Entity<InputState>,
    /// 保存失败的一句话
    pub(crate) error: Option<String>,
}

/// 定时同步的间隔档位（分钟）。与 core 的 `AUTO_SYNC_MINS_RANGE` 同一条下限：
/// 界面能选的最小值必须正好是 core 夹出来的下限，不然用户选了 3 分钟、
/// 存下去被夹成 5，下次打开界面显示 5，他会以为是自己记错了
pub(crate) const SYNC_MINS_PRESETS: [u32; 5] = [5, 15, 30, 60, 180];

impl SyncCfg {
    /// 档位下标：`0` = 不定时，`1..=5` = 每 N 分钟，`6` = 每天定时。
    /// 固定成这 7 档而不是动态列表，是因为"当前选中第几档"要能从配置反推出来
    pub(crate) const OFF: usize = 0;
    pub(crate) const DAILY: usize = SYNC_MINS_PRESETS.len() + 1;

    /// 当前草稿翻译成 core 的档位。**存盘与显示走同一个函数**，
    /// 免得"界面上选的是每天 09:00、存下去成了间隔模式"
    pub(crate) fn to_auto_sync(&self, daily: &str) -> store::AutoSync {
        match self.mode {
            Self::DAILY => store::AutoSync::Daily(daily.to_string()),
            Self::OFF => store::AutoSync::Off,
            i => store::AutoSync::Every(SYNC_MINS_PRESETS[i - 1]),
        }
    }

    /// 每天定时那一档填的时间合不合法。合法才让保存按钮可点
    pub(crate) fn daily_ok(&self, daily: &str) -> bool {
        self.mode != Self::DAILY || store::parse_hhmm(daily).is_some()
    }
}

impl Shell {
    /// 当前选中仓库的日志（只读）。没有记录时回落到 `LogState::EMPTY`，
    /// 这样界面各处不必先判有没有再取
    pub(crate) fn log(&self) -> &LogState {
        match self.selected.as_deref() {
            Some(p) => self.logs.get(p).unwrap_or(&LOG_EMPTY),
            None => &LOG_EMPTY,
        }
    }

    /// 某个仓库的日志（可写），没有就建一条空的。
    ///
    /// **写入一律走这个，别用 `log()`**。后台线程回来时用户可能已经切到别的
    /// 仓库去了，`log()` 会把结果写进别人的日志里
    pub(crate) fn log_of(&mut self, path: &str) -> &mut LogState {
        self.logs.entry(path.to_string()).or_default()
    }

    /// 某分组里的第一个仓库路径。导航轨第 2、3 个按钮用它落选中项。
    pub(crate) fn first_in_group(&self, group: Group) -> Option<String> {
        self.repos
            .iter()
            .find(|r| r.is_third_party == group.is_third())
            .map(|r| r.path.clone())
    }

    /// 当前选中的仓库是不是属于这一组。分组内点行不改变焦点，反之亦然。
    pub(crate) fn selected_in_group(&self, group: Group) -> bool {
        match self.selected.as_deref().and_then(|p| self.entry(p)) {
            Some(e) => e.is_third_party == group.is_third(),
            None => false,
        }
    }

    /// **现在往窗口里拖一个文件夹，它会落进哪一组**（`None` = 这一屏不收）。
    ///
    /// 判据见 [`drop_target`]。三条闸门（落点提示 / 松手 / `on_paths_dropped`
    /// 自己的早退）都调它——**别在别处重写一遍 `self.side_focus`，
    /// 改一处忘了另一处就是"总览页也能拖"这种不一致**。
    pub(crate) fn drop_target(&self) -> Option<Group> {
        drop_target(self.page, self.side_focus)
    }
}

/// 「浏览…」这颗按钮是**哪一个弹窗**按下的。回填的是两份不同的 `InputState`，
/// 所以这个信息必须随标记一起走（见 `Shell::pending_folder` 的注释）。
/// `Copy` 是给按钮的 `on_click` 闭包用的——那个闭包是 `Fn`，不能吞掉捕获值
#[derive(Clone, Copy)]
pub(crate) enum FolderPick {
    /// 克隆弹窗的「克隆到」目标目录
    Clone,
    /// 添加仓库弹窗的「路径」
    AddRepo,
}

/// 添加 / 编辑仓库弹窗的状态。
/// 输入框的 Entity 在打开弹窗时重建（初值靠 placeholder 表达，不预填——
/// 预填要走 `InputState::set_value`，那需要 window，弹窗入口处没有）。
pub(crate) struct RepoDialog {
    /// None = 添加模式；Some(path) = 编辑该仓库
    pub(crate) editing: Option<String>,
    pub(crate) path_input: Entity<InputState>,
    pub(crate) name_input: Entity<InputState>,
    /// 归属：第三方仓库（false = 我的）
    pub(crate) third: bool,
    pub(crate) error: Option<String>,
    /// 非空 = 初始化向导：填的路径还不是 git 仓库，转这里引导 git init
    pub(crate) init: Option<InitState>,
}

/// 克隆对话框的状态。
///
/// **输入框的 Entity 存在这里而不是每帧重建**——重建会让用户每敲一个字
/// 就丢一次焦点。
pub(crate) struct CloneDialog {
    /// 远端地址
    pub(crate) url: Entity<InputState>,
    /// 目标**父目录**（仓库会建在它下面）
    pub(crate) parent: Entity<InputState>,
    /// 正在克隆（按钮转圈、防连点）
    pub(crate) running: bool,
    /// 进度四档。⚠️ **单拆成 `CloneProgress` 而不是摊在这一层**：`CloneDialog`
    /// 带着两颗 `Entity<InputState>`，脱离 gpui 上下文**根本构造不出来** ⇒
    /// 「克隆进行中就该拿到阶段/网速/百分比」这条判据一条单测都写不了，
    /// 而它恰恰是 2026-10-10 那条缺陷（全程只显示"准备中…"）的落点。
    /// 拆成纯数据之后搬运规则才进得了单测（AGENTS：能抽纯函数就抽）
    pub(crate) progress: CloneProgress,
    /// 失败的一句中文结论 + git 原始 stderr
    pub(crate) error: Option<String>,
    /// **本次克隆那个 git 进程实际的 `-C` 地址**（目标父目录）。
    ///
    /// 取消标记按仓库带地址之后（路线表 32-8），「取消克隆」必须打在这一个串上。
    /// ⚠️ 它是**起跑那一刻定死的**，不是取消时再去读输入框：克隆要几分钟，
    /// 期间那个框里的字可能被改、被粘贴、被用户清了重写，照着"此刻框里是什么"
    /// 去取消 = 按下去什么都不发生。
    /// 判据同那条老账：**「用户看不见的状态」要在界面上说出来**反过来的那一半——
    /// **「操作自己的输入要在操作起跑时固化，不能在结束时重新问一遍」**
    pub(crate) cloning_into: String,
}

/// 克隆进行时要上屏的四档。
///
/// ⚠️ **与 `LogState` 的 `stage` / `detail` / `percent` 是同一套语义**——
/// 那边是拉取/一键命令的日志卡，这边是克隆弹窗。两条链路都必须**在操作还在跑
/// 的时候就逐批拿到值**，不是等命令返回（D73 的同族：断点在 GUI 的搬运层，
/// 不在 core）。三档的更新判据两处一样，见 `dialogs::clone` 的 `pump` 注释。
#[derive(Default, Clone)]
pub(crate) struct CloneProgress {
    /// 当前阶段词（"接收对象" / "正在取消…" …）。
    /// **路径标签已经拼在里面**（`接收对象（经代理 127.0.0.1:7897）`），同 `LogState::stage`
    pub(crate) stage: String,
    /// 传输量与网速（`已传输 1.21 MiB · 56.00 KiB/s`）；空串 = 这行没有速度信息
    pub(crate) detail: String,
    /// 确定态百分比 0–100。**None = 不定态**，界面走官方的不定态条
    pub(crate) percent: Option<f32>,
    /// 实时输出（只留文字变了的那些行）
    pub(crate) output: Vec<String>,
}

/// 初始化向导的状态：顶层目录清单 + 勾选的排除项
#[derive(Clone)]
pub(crate) struct InitState {
    pub(crate) path: String,
    pub(crate) items: Vec<repos::DirItem>,
    pub(crate) excludes: std::collections::HashSet<String>,
    pub(crate) working: bool,
}

/// 初始化时默认勾选的排除项：构建产物 / 依赖 / IDE 目录。
/// 与旧版同一份清单——用户大概率不想把这些提交进去。
pub(crate) const INIT_EXCLUDE_HINTS: &[&str] = &[
    "node_modules",
    "dist",
    "build",
    "out",
    "target",
    ".idea",
    ".vs",
    "vendor",
    "__pycache__",
    ".gradle",
    "bin",
    "obj",
    "coverage",
    ".next",
    ".nuxt",
    "venv",
];

/// 批量同步的三条固定命令（Q3，2026-10-03 用户拍板）。
///
/// 形态是**固定三条 + 按分档自动筛**，不是"在列表里勾选再跑"：
/// 判断依据（谁落后、谁待推送）在状态扫描里已经算好了，界面上再摆一套
/// 复选框等于把同一件事做两遍，而复选框那一套在阶段 6 已经从总览页删掉了。
/// 仓库是零散的，一口气列出来让人挑反而更容易挑错
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BatchKind {
    Pull,
    Push,
    Check,
}

impl BatchKind {
    /// 「批量拉取」这种句式，用在状态条与汇总里
    pub(crate) fn label(self) -> &'static str {
        match self {
            BatchKind::Pull => "批量拉取",
            BatchKind::Push => "批量推送",
            BatchKind::Check => "批量检测",
        }
    }

    /// 「拉取全部」这种句式，用在 ⌘K 那一行上
    pub(crate) fn verb(self) -> &'static str {
        match self {
            BatchKind::Pull => "拉取全部",
            BatchKind::Push => "推送全部",
            BatchKind::Check => "检测全部",
        }
    }

    /// 这个仓库要不要进这一批。**纯读已扫到的状态，不新增任何 git 调用**
    pub(crate) fn wants(self, third: bool, s: &RepoStatus) -> bool {
        match self {
            // behind > 0 才值得拉。分叉的仓库 behind 也 > 0，而 `--ff-only`
            // 拉不下来——它会失败并在日志里说清该走变基向导，这比在这里
            // 悄悄把它滤掉更有用（用户至少知道自己那个仓库有问题）
            BatchKind::Pull => s.behind > 0,
            // 推送会改**别人**的仓库：第三方一律不进这条队列。
            // 这条是设计 3.5 的硬边界，批量层不能因为"方便"就绕过
            BatchKind::Push => !third && s.ahead > 0,
            // 检测不改任何东西，全部都跑
            BatchKind::Check => true,
        }
    }

    /// 这一批在搜索框里能命中哪些词
    pub(crate) fn keywords(self) -> &'static [&'static str] {
        match self {
            BatchKind::Pull => &["拉取", "pull", "落后", "behind", "更新", "同步"],
            BatchKind::Push => &["推送", "push", "待推送", "ahead", "上传", "同步"],
            BatchKind::Check => &["检测", "check", "刷新", "扫描", "状态"],
        }
    }
}

/// 批量跑完（或中途）的一句话。**抽成自由函数**是为了能单测——它挂在
/// `Shell` 上时要一整个 Shell 才能调，而这段文案的规则恰恰最容易漂：
/// 「跳过 0 个」这种废话一旦漏进去就再也看不出来了
///
/// 规则：**跳过 / 失败为 0 时那一段整个不出现**（旧 React 版就钉过这条，
/// `pnpm test:web` 里的「不写『跳过 0 个』这种废话」）。顺序固定在成功之后。
pub(crate) fn batch_summary(
    kind: BatchKind,
    done: usize,
    total: usize,
    ok: usize,
    skipped: usize,
    failed: usize,
    running: &[String],
) -> String {
    if done < total {
        // ⚠️ 32-9 起必须把**正在跑哪几颗**说出来。以前那句只有「批量拉取 · 3/17」，
        // 串行时"3/17"顺带就回答了"现在在第几颗"；并发之后完成顺序是乱的，
        // 数字只回答"完了几个"、**不回答"现在在跑谁"**——那是 D73 的同一种形状：
        // 信息在换写法的那一步被悄悄丢掉，界面看着还挺对。
        // 状态条那一格只给短句（完整解释在 `app_log` 与跑完的通知里），
        // 所以三颗以上收成「等 N 颗」，不把名字一路铺下去把路径挤出可视区
        let head = format!("{} · {done}/{total}", kind.label());
        return match running.len() {
            0 => head,
            1 => format!("{head} · 正在跑 {}", running[0]),
            n if n <= 2 => format!("{head} · 正在跑 {}", running.join("、")),
            n => format!("{head} · 正在跑 {} 等 {n} 颗", running[0]),
        };
    }
    let mut s = format!("{} · {ok} 成功", kind.label());
    if skipped > 0 {
        s.push_str(&format!(" · {skipped} 跳过"));
    }
    if failed > 0 {
        s.push_str(&format!(" · {failed} 失败"));
    }
    s
}

/// 批量调度那一问的**纯函数**版：还能不能再发一颗。
///
/// ⚠️ 为什么不直接写在 `Shell::pump_batch` 里：`Shell` 带着 gpui 上下文，
/// **构造不出来** ⇒ "上限有没有生效"这条判据一条单测都写不了，
/// 而它恰恰是这一批最容易被改坏的一处（把 `<` 改成 `<=`、把常量抄成 100，
/// check 零错、界面看着也还在跑）。抽出来之后变异能红在它身上
/// （AGENTS：凡是要靠 `&self` 才能调的判据，抽成纯函数再单测）
pub(crate) fn batch_can_launch_more(running: bool, in_flight: usize, queued: usize) -> bool {
    running && in_flight < sync::MAX_CONCURRENT_OPS && queued > 0
}

/// 批量同步的进度与结论。入口在 ⌘K（Q3），**界面上的复选框与批量条在阶段 6 已删**。
///
/// 汇总写进 `Shell::app_log`（全局槽）而不是某个仓库的日志：这一批横跨多个
/// 仓库，记到"当前选中"名下会让人以为只动了一个
///
/// ⚠️ 32-9 起它是**一个自己数着并发位的调度器**，不再是一条按顺序走的队列：
/// `queue` 是还没发的，`running_now` 是正在发的（最多 `sync::MAX_CONCURRENT_OPS` 颗）。
/// 调度为什么放在界面这一层而不是"17 个任务一起丢给 core 排队"——
/// gpui 那个后台执行器是**共享线程池**，全堵在额度等待里会把扫描、克隆进度这些
/// 别的活儿饿死；而按固定批数分批又有"一批里一颗挂死、整批等它"的队头阻塞。
/// 自己数着"完成一颗补一颗"两样都躲开
pub(crate) struct BatchRun {
    pub(crate) kind: BatchKind,
    /// 这一批全部的目标，顺序即**入队**顺序（执行顺序并发之后就不固定了）
    pub(crate) paths: Vec<String>,
    /// 还没发出去的
    pub(crate) queue: Vec<String>,
    /// 正在跑的这几颗（路径）。状态条那句"正在跑 甲、乙"读它，
    /// **不是从 `done` 反推的**——并发之后"第几个"这个说法已经不成立了
    pub(crate) running_now: Vec<String>,
    pub(crate) done: usize,
    pub(crate) ok: usize,
    /// 拉取时脏工作区跳过的个数（推送与检测没有这条路，恒 0）
    pub(crate) skipped: usize,
    pub(crate) failed: usize,
    pub(crate) running: bool,
    /// 跑完（或失败）的一句话结论，状态条展示
    pub(crate) note: Option<String>,
}

/// 档案页迷你柱图的天数窗口。设计稿标「近 30 天活跃度」；
/// 与旧检视抽屉的 14 天不是一回事，各用各的参数。
pub(crate) const PROFILE_ACTIVITY_DAYS: u32 = 30;

/// 仓库在侧栏的状态口径：分叉 > 落后 > 领先 > 脏 > 已同步。
/// 与后端 `ForkInfo` 一致（ahead>0 && behind>0 就是分叉），不另立标准。
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Bucket {
    Synced,
    Dirty,
    Ahead,
    Behind,
    Forked,
    /// **领先/落后这一档读不出来**（32-12：git < 2.32 时那条命令不给 `# branch.ab`）。
    ///
    /// ⚠️ 为什么单开一档而不是复用 `Unknown`：`Unknown` 那句话是"待扫描"，
    /// 而这一档**扫过了、只是那一格没数**——把"不知道"说成"还没扫"，
    /// 与把它说成"已同步"是同一种错，只是方向相反（31.4 补十五那条纪律）。
    /// ⇒ 开一档新的，让编译器把"每一处遇到它要怎么说"全部列出来（D78）
    AbUnknown,
    Unknown,
}

pub(crate) fn bucket_of(s: &RepoStatus) -> Bucket {
    if !s.is_repo {
        return Bucket::Unknown;
    }
    if s.ahead > 0 && s.behind > 0 {
        Bucket::Forked
    } else if s.behind > 0 {
        Bucket::Behind
    } else if s.ahead > 0 {
        Bucket::Ahead
    } else if s.dirty {
        // 脏是**同一条命令真读到的**，那一格不因为另一格读不出而作废
        Bucket::Dirty
    } else if s.ab_unknown {
        Bucket::AbUnknown
    } else {
        Bucket::Synced
    }
}

/// 分档取色。**全站唯一一份口径**（Q7，2026-10-03 用户拍板），
/// 侧栏状态点 / 侧栏徽标 / ⌘K 状态点 / 总览统计卡都从这里取
/// （原来还多一项"同步环"，那颗环 2026-10-05 已下线）。
///
/// | 档位 | 取色 | 理由 |
/// |---|---|---|
/// | 已同步 | 绿 `success` | 干净是常态也安全，与旧 React 版一致，不回归 |
/// | 有未提交 | 蓝 `accent` | 需要动手但不紧急 |
/// | 待推送 | 蓝 `accent` | 与上一档同语义，不另开色 |
/// | 落后 | 橙 `chart_3` | 拉一下就好 |
/// | 已分叉 | 红 `danger` | 唯一"不能一键解决"的档，值得最重的色 |
/// | 待扫描 | `line` 档（`edge(t)`） | **没数据的那档不给行动色**：蓝/橙/红都在说"有件事要做"，而这里什么都不知道 |
///
/// ⚠️ 「已同步」刻意**不占强调色**：它是占比最高的一档，而它恰恰是
/// 不需要动手的一档，给它蓝色会让绝大多数干净的仓库在侧栏和总览里一片蓝。
/// ⚠️ 「已分叉」从旧 React 版的紫 `#7c3aed` 改成红：theme.json 只有
/// chart.1–4 四个语义色（蓝 / 绿 / 橙 / 红），没有紫色档；为分叉单开
/// 第五个色相跟整套"灰白调 + 四个语义色"打架。
pub(crate) fn dot_color(b: Bucket, t: &Theme) -> Hsla {
    match b {
        Bucket::Synced => t.colors.success,
        Bucket::Dirty => t.colors.accent,
        Bucket::Ahead => t.colors.accent,
        Bucket::Behind => orange(t),
        Bucket::Forked => t.colors.danger,
        // 「待扫描」与「同步未知」都借 `line` 那一档灰。这里原来直写
        // `t.colors.border`——与 `edge(t)` 是**同一个值的两个名字**
        // （2026-10-08 台账第三条），归到唯一入口是为了"将来调这一档不会漏改这里"
        Bucket::Unknown | Bucket::AbUnknown => edge(t),
    }
}

/// 分档的一个词。**它这句措辞的唯一出处有两个消费者**：⌘K 搜索（按
/// 「落后」「分叉」这类词找仓库）与总览表格「状态」列里「没扫过」那一行。
/// 界面上其余那句（带数字、带归属口吻的）由各处按版面自己拼——
/// 侧栏 200px 要带数字，总览表格旁边就有独立数字列。
/// 放在这里是让 ⌘K 能按「落后」「分叉」这类词搜到仓库：
/// 官方的 `CommandItem` 只做子串包含匹配，用户打「分叉」时
/// 标签里没有这个词就一条都搜不出来
pub(crate) fn bucket_word(b: Bucket) -> &'static str {
    match b {
        Bucket::Synced => "已同步",
        Bucket::Dirty => "未提交",
        Bucket::Ahead => "待推送",
        Bucket::Behind => "落后",
        Bucket::Forked => "已分叉",
        // ⚠️ 这两句必须是**两句话**（与 32-11 那颗片上「零颗」与「不知道」同一条纪律）：
        // 「待扫描」= 还没问过 git；「同步未知」= 问过 git，但它那个版本读不出
        // 领先/落后（< 2.32）。合成一句就等于把"我问了、答不上来"说成"我还没问"
        Bucket::Unknown => "待扫描",
        Bucket::AbUnknown => "同步未知",
    }
}

/// 时间戳 → 「2 小时前」这种说法。0 = 从没跑过。
pub(crate) fn rel_time(ts: u64) -> String {
    if ts == 0 {
        return String::new();
    }
    let now = qingbird_core::store::now_secs();
    let d = now.saturating_sub(ts);
    match d {
        0..=59 => "刚刚".into(),
        60..=3_599 => format!("{} 分钟前", d / 60),
        3_600..=86_399 => format!("{} 小时前", d / 3_600),
        86_400..=172_799 => "昨天".into(),
        _ => format!("{} 天前", d / 86_400),
    }
}

/// 「这份数据是什么时候的」那句话。**空串 = 不该显示**（没扫过 / 不知道），
/// 调用方拿到空串就别往 `CardHeader::note` 里塞——那个槽会照画一个空
/// `div` 出来，白占一个 `GAP_TIGHT` 的间距。
///
/// 抽成纯函数是为了能单测：`scanning` 与 `scanned_at` 两条分支正是
/// 阶段 16 最要紧的判据（后台刷新时缓存已经在屏幕上了，这时**不能**
/// 报"扫描中"把内容顶掉），走 `CardHeader` 就没法测。
pub(crate) fn scanned_note(scanning: bool, scanned_at: u64) -> String {
    if scanning {
        "刷新中…".to_string()
    } else if scanned_at > 0 {
        rel_time(scanned_at)
    } else {
        String::new()
    }
}

/// 「说明」页签表头那一句（阶段 18）
///
/// **返回的就是最终要上屏的那一句**（`读取于 刚刚`），不是一个还要调用方
/// 拼前缀的半成品——「正在读」那一档根本没有"读取于"这个前缀，
/// 拼出来是「读取于 读取中…」。抽成纯函数还有第二个理由：这道闪烁
/// 就是这一句在闪，而**闪没闪只有它能钉**（抓帧只证明某帧长什么样，
/// 证明不了它没有中间态）。
pub(crate) fn readme_note(loading: bool, scanning: bool, scanned_at: u64) -> String {
    if loading {
        // 只在**真要读一份手里没有的文档**时出现（README 正文里点相对链接
        // 切到第三份）。中英两颗键走内存同步切换，**永远不该走到这一档**
        return "读取中…".to_string();
    }
    let n = scanned_note(scanning, scanned_at);
    if n.is_empty() {
        String::new()
    } else {
        format!("读取于 {n}")
    }
}

/// 空视图：仓库还没有被扫到时（首帧、扫描中）给总览行兜底
pub(crate) static EMPTY_VIEW: RepoView = RepoView {
    status: RepoStatus {
        is_repo: false,
        branch: String::new(),
        ahead: 0,
        behind: 0,
        dirty: false,
        has_upstream: false,
        upstream: String::new(),
        remote_url: String::new(),
        stash_count: 0,
        ab_unknown: false,
    },
    changed: 0,
    // 「没读过」——总览那一行兜底用的就是这一格，所以它显示「待扫描」而不是
    // 「已同步」或「未识别为 Git 仓库」。`has_status` 的注释记着为什么这三档分得开
    has_status: false,
    changes: Vec::new(),
    detected: run_cmd::CmdDetect {
        test_cmd: String::new(),
        build_cmd: String::new(),
        dev_cmd: String::new(),
        install_cmd: String::new(),
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- D78：拖放落哪一组 ----------

    /// **这才是那个 bug 的钉子**：在「第三方仓库」页拖进来的文件夹必须进第三方。
    /// 修之前这条路的归属是 `add_repo` 的默认值（`false` = 我的），
    /// 所以两组都会错——用户报的现象是"进了我的仓库"。
    #[test]
    fn a_drop_lands_in_the_group_the_sidebar_is_showing() {
        assert_eq!(drop_target(Page::Repos, Group::Mine), Some(Group::Mine));
        assert_eq!(
            drop_target(Page::Repos, Group::Third),
            Some(Group::Third)
        );
    }

    /// 总览页没有侧栏、也就没有"当前这一组"，所以**不收**（用户拍板）。
    /// 两种分组都要是 `None`——判据是"有没有侧栏"，不是"上次看的是哪组"。
    #[test]
    fn the_overview_page_takes_no_drops_at_all() {
        assert_eq!(drop_target(Page::Overview, Group::Mine), None);
        assert_eq!(drop_target(Page::Overview, Group::Third), None);
    }

    /// 真值表**整张**钉死：3 页 × 2 组，六格全列出来。
    ///
    /// 为什么不用上面两条代替：`Page` 加第三种页面时，这个 `match` 会编译不过
    /// （没有 `_` 分支），那时**必须回来重新想一遍**这一页收不收——而不是让它
    /// 悄悄继承某一支的默认值。两格变六格时，靠"记得改测试"是防不住的
    /// （2026-10-13 加 CLI 屏这条就是靠它当场提醒的）。
    #[test]
    fn the_whole_truth_table_is_pinned() {
        for page in [Page::Repos, Page::Overview, Page::CliTools] {
            for g in [Group::Mine, Group::Third] {
                let got = drop_target(page, g);
                let want = match page {
                    Page::Repos => Some(g),
                    Page::Overview | Page::CliTools => None,
                };
                assert_eq!(got, want, "{page:?} + {g:?}");
            }
        }
    }

    /// 判据是**眼睛看着的那一组**，不是"选中的那个仓库"。
    ///
    /// 这条钉不住实现（`drop_target` 的签名里压根没有 `selected` 这个入参，
    /// 编译期就排除了拿选中项去判的可能），但它把**为什么没有那个入参**
    /// 写在了单测里：下一个人若觉得"选中项更准"想加回来，得先读到这里。
    #[test]
    fn the_group_comes_from_the_sidebar_not_from_the_selection() {
        // 侧栏停在第三方：无论界面上还选着谁（跨组的选中项不会被切走），
        // 新拖进来的都归第三方
        assert_eq!(drop_target(Page::Repos, Group::Third), Some(Group::Third));
    }

    fn st(is_repo: bool, ahead: u32, behind: u32, dirty: bool) -> RepoStatus {
        RepoStatus {
            is_repo,
            branch: "main".into(),
            ahead,
            behind,
            dirty,
            has_upstream: true,
            upstream: String::new(),
            remote_url: String::new(),
            // **测试夹具一律 0**：这些用例判的是同步卡与感叹号那几档，
            // 暂存那一档要它非零的用例在 `panels` 里单独造（写在这里会让
            // 所有既有断言一起变红，而红的原因不是缺陷）
            stash_count: 0,
            ab_unknown: false,
        }
    }

    /// 分档优先级是**一条链**，顺序错了会静默归错档：
    /// `ahead>0 && behind>0` 必须先判成「已分叉」，不能拆成待推 + 落后两件事
    /// （分叉 `--ff-only` 拉不下来，是唯一不能一键解决的那一档）
    #[test]
    fn bucket_priority_puts_fork_first_then_dirty_last() {
        assert_eq!(bucket_of(&st(false, 0, 0, false)), Bucket::Unknown, "没扫过 / 不是仓库");
        assert_eq!(bucket_of(&st(true, 0, 0, true)), Bucket::Dirty, "有未提交");
        assert_eq!(bucket_of(&st(true, 0, 0, false)), Bucket::Synced);
        assert_eq!(bucket_of(&st(true, 2, 0, false)), Bucket::Ahead);
        assert_eq!(bucket_of(&st(true, 0, 3, false)), Bucket::Behind);
        assert_eq!(
            bucket_of(&st(true, 2, 3, true)),
            Bucket::Forked,
            "分叉优先于落后 / 待推 / 未提交——它是唯一不能一键解决的那档"
        );
    }

    /// 每一档都要有中文说法：侧栏徽标、⌘K 状态点、统计卡都从这里取，
    /// 空串会让某几处画出一个没有文字的徽标
    #[test]
    fn every_bucket_has_a_chinese_word() {
        for b in [
            Bucket::Synced,
            Bucket::Dirty,
            Bucket::Ahead,
            Bucket::Behind,
            Bucket::Forked,
            Bucket::Unknown,
        ] {
            assert!(!bucket_word(b).is_empty(), "{:?} 缺标签", b);
        }
    }

    /// Q3 拍板的筛选口径（2026-10-03）。**第三方一律不进推送队列**——
    /// 推送会改别人的仓库，这是设计 3.5 的硬边界
    #[test]
    fn batch_filter_follows_the_three_tiers_q3_decided() {
        let clean = st(true, 0, 0, false);
        let ahead = st(true, 2, 0, false);
        let behind = st(true, 0, 3, false);
        // 落后：拉
        assert!(BatchKind::Pull.wants(false, &behind));
        // 待推：推
        assert!(BatchKind::Push.wants(false, &ahead));
        // 干净的：两条都不进——按钮上会写「没有可跑的仓库」而不是点了没反应
        assert!(!BatchKind::Pull.wants(false, &clean));
        assert!(!BatchKind::Push.wants(false, &clean));
        // 检测：全部都进，它不改任何东西
        assert!(BatchKind::Check.wants(false, &clean));
        assert!(BatchKind::Check.wants(true, &clean));
        // **第三方不进推送**
        assert!(!BatchKind::Push.wants(true, &ahead), "第三方推送是越界");
        // 第三方该拉还是要拉
        assert!(BatchKind::Pull.wants(true, &behind));
    }

    /// 每一档的两种句式都不能为空：⌘K 用 `verb`（「拉取全部」），
    /// 状态条与对话框用 `label`（「批量拉取」）
    #[test]
    fn batch_kinds_have_both_sentences() {
        for k in [BatchKind::Pull, BatchKind::Push, BatchKind::Check] {
            assert!(!k.label().is_empty() && !k.verb().is_empty());
            assert!(!k.keywords().is_empty());
        }
    }

    /// 「跳过 0 个」这种废话一旦漏进去就再也看不出来——所以钉住
    #[test]
    fn batch_summary_omits_the_zero_parts() {
        assert_eq!(
            batch_summary(BatchKind::Pull, 3, 3, 3, 0, 0, &[]),
            "批量拉取 · 3 成功"
        );
        assert_eq!(
            batch_summary(BatchKind::Push, 2, 2, 1, 1, 0, &[]),
            "批量推送 · 1 成功 · 1 跳过",
            "顺序固定在成功之后"
        );
        assert_eq!(
            batch_summary(BatchKind::Check, 4, 4, 1, 0, 3, &[]),
            "批量检测 · 1 成功 · 3 失败"
        );
    }

    /// 跑一半停下来要说清跑到哪、还剩几个没动；
    /// **停在最后一个上算跑完了**，不报「0 个没动」
    #[test]
    fn batch_summary_distinguishes_stopped_from_finished() {
        assert_eq!(
            batch_summary(BatchKind::Pull, 2, 5, 2, 0, 1, &[]),
            "批量拉取 · 2/5",
            "还在跑 / 被停：报进度"
        );
        assert_eq!(
            batch_summary(BatchKind::Push, 2, 2, 2, 0, 0, &[]),
            "批量推送 · 2 成功",
            "done == total 就是跑完了，哪怕 running 是 false"
        );
    }

    /// 并发之后**光有 `2/5` 已经回答不了"现在在跑谁"**——串行时那个数字
    /// 顺带就是答案（第 3 颗在跑），并发之后完成顺序是乱的，
    /// 从"完了几个"推不出"现在在跑哪几个"。
    /// 这一族是 D73 的同型：**信息不是被截断的，是被一种新写法悄悄顶掉的**
    #[test]
    fn batch_progress_names_what_is_running_now() {
        let a = "甲".to_string();
        let b = "乙".to_string();
        let c = "丙".to_string();
        let d = "丁".to_string();
        assert_eq!(
            batch_summary(BatchKind::Pull, 2, 6, 2, 0, 0, &[a.clone()]),
            "批量拉取 · 2/6 · 正在跑 甲"
        );
        assert_eq!(
            batch_summary(BatchKind::Pull, 2, 6, 2, 0, 0, &[a.clone(), b.clone()]),
            "批量拉取 · 2/6 · 正在跑 甲、乙",
            "两颗都点名"
        );
        // 三颗以上收成"等 N 颗"：状态条那一格只给短句，
        // 把四个名字一路铺下去会把中间那格撑走、路径被挤出可视区
        assert_eq!(
            batch_summary(BatchKind::Pull, 2, 6, 2, 0, 0, &[a.clone(), b.clone(), c.clone()]),
            "批量拉取 · 2/6 · 正在跑 甲 等 3 颗"
        );
        assert_eq!(
            batch_summary(
                BatchKind::Push,
                0,
                6,
                0,
                0,
                0,
                &[a.clone(), b.clone(), c.clone(), d.clone()]
            ),
            "批量推送 · 0/6 · 正在跑 甲 等 4 颗",
            "一颗都没完时也要说得出在跑谁"
        );
    }

    /// 并发上限这一问的四种组合逐一钉住。
    ///
    /// 把它抽成纯函数只为这一条：`Shell` 带着 gpui 上下文**构造不出来**，
    /// 不抽出来"上限有没有生效"就只能靠抓帧看——而抓帧恰恰看不出
    /// "本该发第 5 颗却没发"这种事（画面上一切正常）。
    /// 最后那条区间断言是防"把上限写成 `usize::MAX`"：那种改法上面几条**全绿**，
    /// 而"限流"这件事已经整条没了
    #[test]
    fn the_batch_gate_opens_only_within_the_limit() {
        let max = sync::MAX_CONCURRENT_OPS;
        assert!(batch_can_launch_more(true, 0, 1), "有空位又有排队就该发");
        assert!(batch_can_launch_more(true, max - 1, 9), "只差一个空位也要发");
        assert!(
            !batch_can_launch_more(true, max, 9),
            "满了还许发 ⇒ 上限形同虚设，批量会一次开十几个 git 进程去撞远端限流"
        );
        assert!(
            !batch_can_launch_more(true, 0, 0),
            "队列空了不许发（那样会 panic 在 `first()` 上）"
        );
        assert!(!batch_can_launch_more(false, 0, 5), "这一批已收尾，不许再发");
        assert!(
            (2..=8).contains(&max),
            "并发上限取 {max} 路：再高撞远端限流、再低批量就没意义，\
             这个区间是 Q28 定下来的口径"
        );
    }

    /// 阶段 16 那句话的两条分支。**「没扫过」必须是空串而不是"尚未扫描"**——
    /// 真没扫过时表头另有一条提示（"这个仓库还没有扫描过磁盘占用"），
    /// 这里再写一遍就成了同一屏两句话。
    #[test]
    fn scanned_note_says_nothing_when_nothing_was_ever_scanned() {
        assert_eq!(scanned_note(false, 0), "");
        // 正在扫但从没有过数据：这一刻屏幕上只有骨架屏，
        // 说"刷新中"是在骗人
        assert_eq!(scanned_note(true, 0), "刷新中…");
    }

    /// 正在扫**且屏上已经有数**时，报的是"刷新中"而不是把时间抹掉。
    /// 两者都要有内容，但后者会让人以为这是一份刚算出来的数
    #[test]
    fn scanned_note_prefers_the_refreshing_wording_while_scanning() {
        let now = qingbird_core::store::now_secs();
        assert_eq!(scanned_note(true, now), "刷新中…");
        assert_eq!(scanned_note(false, now), "刚刚");
    }

    /// ⚠️ **这条是阶段 18 那道闪烁的判据**：中英两颗键走的是**内存同步切换**，
    /// 三个标志一个都不许动 ⇒ 这句话必须与切换前**逐字相同**。
    /// 曾经它第一行就 `scanning = true`，于是表头先闪一下「读取中…」再换回来
    /// （实机抓帧量到：点完第一帧只变了 280 像素，包围盒 (585,209)-(631,221)，
    /// 正好就是这一句）
    #[test]
    fn readme_note_never_flashes_while_switching_in_memory() {
        let now = qingbird_core::store::now_secs();
        // 切换前后：同一个 `scanning`、同一个 `scanned_at`、没有 loading
        let before = readme_note(false, false, now);
        let after = readme_note(false, false, now);
        assert_eq!(before, after, "同步切换不许改动这一句");
        assert_eq!(before, "读取于 刚刚");
    }

    /// 三个标志各管一件事，**优先级不能串**：
    /// `loading` 是"正在读一份手里没有的文档"，`scanning` 是"整份档案在刷新"。
    /// 串了的话点一次中英切换会让仓库头的活跃度芯片一起说「统计中…」
    #[test]
    fn readme_note_keeps_the_three_flags_apart() {
        let now = qingbird_core::store::now_secs();
        // 正在读 → 说的是"读取中"，不能套上"读取于"这个前缀
        assert_eq!(readme_note(true, false, now), "读取中…");
        assert_eq!(readme_note(true, true, now), "读取中…");
        // 档案在刷新 → 说的是"刷新中"，与正在读那份文档不是一回事
        assert_eq!(readme_note(false, true, now), "读取于 刷新中…");
        // 什么都没扫过 → 不摆这句话（表头那个槽会照画一个空 div 出来）
        assert_eq!(readme_note(false, false, 0), "");
    }

    /// 落库的键 ↔ 界面的说法，**两个方向都要对**。
    ///
    /// ⚠️ 这条守的是一件具体的事：`label_of_key` 是 `key` 的逆函数，
    /// 两处各写一遍。往后加一个新 `Op` 变体时，忘了在 `label_of_key`
    /// 里补一条**编译期不报、单测也不报**——库里明明写着 `checkout`，
    /// 界面上却显示「某项操作」，而那句话永远没人会去查。
    /// **往 `key()` 里加一条、往 `label_of_key` 忘一条，这条单测立刻红。**
    #[test]
    fn every_op_key_round_trips_through_its_label() {
        // 覆盖每一个会落库的变体。`Rebase(false)` / `Rebase(true)`
        // 键相同（是同一个操作，区别只在要不要先暂存），列一个就够
        for op in [
            Op::Pull,
            Op::Push,
            Op::Check,
            Op::Rebase(false),
            Op::UndoRebase,
            Op::Commit,
        ] {
            let key = op.key().expect("这几个都要落 op_history");
            assert_eq!(
                Op::label_of_key(key),
                op.label(),
                "键 `{key}` 反查回来的说法与 `label()` 不是同一句"
            );
        }
    }

    /// 落库的键**不许重名**，也不许是空串。
    ///
    /// 重名那条更阴：两个**不同的**操作共用一个键，界面上会把 A 显示成 B，
    /// 而所有单测都还是绿的（它们各自都没写错）。
    #[test]
    fn op_keys_are_unique_and_never_empty() {
        let mut seen = std::collections::HashSet::new();
        for op in [
            Op::Pull,
            Op::Push,
            Op::Check,
            Op::Rebase(false),
            Op::UndoRebase,
            Op::Commit,
        ] {
            let key = op.key().expect("这几个都要落 op_history");
            assert!(!key.is_empty(), "{op:?} 的落库键是空串");
            assert!(seen.insert(key), "键 `{key}` 被两个操作共用了：{op:?}");
        }
    }

    /// ⚠️ **同一次操作的两个变体必须共用一个键**——这是上面那条唯一允许的重名。
    ///
    /// `Rebase(bool)` 里的 bool 是"工作区脏时用户同意先暂存"，**不是两种操作**：
    /// 不暂存的变基与先暂存的变基，在"这个仓库上次被更新过什么"这个问题上
    /// 是同一件事。给它们两个键的话，界面上会出现两条长得一样、
    /// 只有时间不同的记录，读的人会以为是两次变基。
    ///
    /// 写死这条是防下一个人"顺手修好它"——看到两个变体一个键，多数人会当 bug
    #[test]
    fn the_two_rebase_variants_share_one_key_on_purpose() {
        assert_eq!(Op::Rebase(false).key(), Op::Rebase(true).key());
        assert_ne!(
            Op::Rebase(false).key(),
            Op::UndoRebase.key(),
            "变基与撤销变基是两件事，键必须分开"
        );
    }

    /// 每个落库的操作键都必须被 core 那一档**明确归类**：要么在"算同步"清单里，
    /// 要么在"不算"清单里，两边都不在就是没人想过。
    ///
    /// 编译器在这里逼不出来——新增一种操作时 `key()` 那个 match 加一条臂就过了，
    /// 而"它算不算和云端交换过"没人答 ⇒ 「久未同步」那片默默漏掉它。
    /// 判错的两个方向都不报错：多算让那条问句永远空，少算把正常的仓库说成三个月没动
    /// （D78 的同型：**让"忘"没有选项**，只是这一档的闸只能落在测试上）
    #[test]
    fn every_op_key_is_classified_as_sync_or_not() {
        use qingbird_core::scan_cache as sc;
        for op in Op::ALL {
            // `Cmd` 压根不落库（`key()` 是 `None`），不在这条的范围内
            let Some(key) = op.key() else { continue };
            assert!(
                sc::SYNC_OP_KEYS.contains(&key) || sc::NON_SYNC_OP_KEYS.contains(&key),
                "操作键「{key}」既不在「算同步」也不在「不算」的清单里 ⇒ \
                 「哪几个三个月没同步」对它是个盲区"
            );
            assert_eq!(
                sc::is_sync_op(key),
                sc::SYNC_OP_KEYS.contains(&key),
                "{key}：两份清单与那颗谓词说的不是同一件事"
            );
        }
        // 两份清单不许交叉：一个键既算又不算，取到哪一份就取决于谁先读
        for k in sc::SYNC_OP_KEYS {
            assert!(!sc::NON_SYNC_OP_KEYS.contains(&k), "「{k}」同时被归进两边");
        }
    }

    /// 命令槽位**不落 op_history**，而认不出来的键有个说人话的兜底。
    ///
    /// 兜底这条是给**上一个版本写下的键**留的：那时没有这一类，
    /// 数据库里躺着别的键。宁可显示"某项操作"，也不要一行空白——
    /// 空白会被读成"没跑过操作"，那是假话
    #[test]
    fn cmd_slots_stay_out_of_op_history_and_unknown_keys_still_say_something() {
        assert_eq!(
            Op::Cmd(Slot::Test).key(),
            None,
            "命令槽位的下标来自 config 里按钮的顺序，进不了落库键"
        );
        assert_eq!(Op::label_of_key("pinned:0"), "某项操作");
        assert_eq!(Op::label_of_key(""), "某项操作");
    }

    /// 只填判据用得着的字段。`OvRow` 十个字段全写一遍会让这条测试变成
    /// 对字段清单的抄写——新增一格时它编译不过（这是好事），但每次都要补十行
    /// 会让下一个人选择跳过这条测试
    fn row_with(age: core_cache::SyncAge) -> OvRow {
        OvRow {
            path: String::new(),
            name: String::new(),
            third: false,
            branch: String::new(),
            bucket: Bucket::Synced,
            status: String::new(),
            behind: 0,
            ahead: 0,
            changed: 0,
            last_used: 0,
            sync_age: age,
        }
    }

    /// 那颗片的**开关 × 四档**真值表，八组逐一钉住。
    ///
    /// 只钉"筛出来那两组"是不够的：`&&` 写成 `||`、或者"片没按下时也走筛选臂"，
    /// 都只会让某一格错。⚠️ `Unknown` 在开关按下时**也不该出现**——
    /// 那时那颗片本身是禁用的（读不出来），这条把那条界面判据的另外一半钉住了
    #[test]
    fn the_chip_shows_everything_when_off_and_only_two_tiers_when_on() {
        use core_cache::SyncAge as A;
        for age in [A::Unknown, A::Never, A::Recent, A::Stale] {
            assert!(
                ov_row_shows(age, false),
                "{age:?}：片没按下时任何一档都不该被藏起来"
            );
        }
        assert!(ov_row_shows(A::Stale, true));
        assert!(ov_row_shows(A::Never, true));
        assert!(!ov_row_shows(A::Recent, true));
        assert!(
            !ov_row_shows(A::Unknown, true),
            "读不出来那一档被当成「久未同步」筛出来 ⇒ 整张表都说成三个月没动"
        );
    }

    /// 片上那两个数与点进去的行数必须对得上——**这条就是那个"对不上"的闸**
    #[test]
    fn the_numbers_on_the_chip_match_the_rows_it_filters_to() {
        use core_cache::SyncAge as A;
        let rows = vec![
            row_with(A::Stale),
            row_with(A::Stale),
            row_with(A::Never),
            row_with(A::Recent),
            row_with(A::Recent),
            row_with(A::Unknown),
        ];
        let (over, none) = stale_counts(&rows);
        assert_eq!((over, none), (2, 1), "Unknown 两档都不进：它不是「没同步」，是「不知道」");
        // 片上的数是两个之和，可见行数也必须等于那个和
        let shown = rows.iter().filter(|r| ov_row_shows(r.sync_age, true)).count();
        assert_eq!(
            shown,
            over + none,
            "片上写 {} 而点进去 {shown} 行 ⇒ 计数与筛选两处各写了一份规则",
            over + none
        );
    }

    /// 表读不出来时那颗片必须**不可用**，而不是"可用但显示 0"。
    /// 后者是这一格里最坏的那种错：数字看着正常，点下去整张表变成"全都三个月没同步"
    #[test]
    fn an_unreadable_table_disables_the_chip_instead_of_showing_zero() {
        assert!(
            !sync_table_is_readable(&None),
            "读不出来被当成可用 ⇒ 那一档会开始给结论"
        );
        let mut m = HashMap::new();
        m.insert("f:/a".to_string(), 1u64);
        assert!(sync_table_is_readable(&Some(m)));
        // ⚠️ **一张读得出来的空表是可用的**：那时"0 颗久未同步"是真话。
        // 把它和读不出来混成一档，才是 core 那边 `Unknown` 存在的理由的反面
        assert!(sync_table_is_readable(&Some(HashMap::new())));
    }

    /// 那颗键上到底写什么。**"零颗"与"不知道"必须是两句话**。
    ///
    /// 上一版的错就藏在这里看不见的地方：禁用态已经做对了，可 label 仍然写
    /// 「久未同步 0」——灰掉的键配一个 0，读的人得到的是"一颗都没有"这句假话。
    /// 这条判据当时只存在于 render 的字符串拼接里，所以单测一条都抓不到它
    #[test]
    fn zero_and_unknown_are_two_different_sentences() {
        assert_eq!(chip_label(true, 0), "久未同步 0", "读得出来、确实一颗都没有");
        assert_eq!(chip_label(true, 3), "久未同步 3");
        assert_eq!(chip_label(false, 0), "久未同步 –", "读不出来时那个记号不是数字");
        assert_ne!(
            chip_label(false, 0),
            chip_label(true, 0),
            "「不知道」与「零颗」写成同一句 ⇒ 那一档的保护整个失效"
        );
    }

    /// 32-10：那一格闸门的真值表。**两行各挡一个方向**，因为旧判据
    /// （`last_commit_ts > 0`）的错法有两种，而且它恰好把第一种挡住了
    #[test]
    fn a_space_only_row_does_not_clobber_the_recognised_commands() {
        // 只写体积那一路建的行：`status` 全空、`detected` 也是空的，
        // 但库里可能带着一份昨天识别好的命令 —— 这种行不许灌进界面，
        // 否则 `Shell::new` 刚现算出来的好的一份会被空的一份盖掉
        let c = core_cache::RepoCache {
            status_read: false,
            status: RepoStatus {
                is_repo: true,
                ..RepoStatus::default()
            },
            detected: run_cmd::CmdDetect {
                test_cmd: "pnpm test".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(
            !cache_detected_is_trustworthy(&c),
            "没读过 git 的那一行，缓存里的命令不是可信的一份"
        );
    }

    #[test]
    fn a_repo_with_no_commits_still_gets_its_commands_back() {
        // 刚 `git init`、一次提交都没有的仓库：旧判据在这种行上恒假 ⇒
        // 识别好的命令永远进不了缓存。这一行必须是真
        let c = core_cache::RepoCache {
            status_read: true,
            // ⚠️ 故意留 `RepoStatus::default()`（`is_repo = false`）与 `changed = 0`：
            // 谁把判据换成"从内容反推"（认 `is_repo`、认有没有提交、认 detected
            // 非空），这一条就当场红——那些内容都不回答"这一份是谁写的"
            changed: 0,
            detected: run_cmd::CmdDetect {
                build_cmd: "cargo build".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(
            cache_detected_is_trustworthy(&c),
            "扫描那一路写下的行，识别好的命令必须读得回来——有没有提交不算条件"
        );
    }

    /// 32-12：`ab_unknown` 落在哪一档。**三档都要钉**，因为它们各自的错法不同：
    /// 读不出 ⇒ 不许说"已同步"；脏是真读到的 ⇒ 不许被那一档带下去；
    /// 没有上游 ⇒ 那是"没有这一档"，不是"读不出来"
    #[test]
    fn an_unreadable_ahead_behind_is_neither_synced_nor_unscanned() {
        let mut s = st(true, 0, 0, false);
        s.ab_unknown = true;
        assert_eq!(bucket_of(&s), Bucket::AbUnknown, "0/0 不许冒充已同步");
        s.dirty = true;
        assert_eq!(bucket_of(&s), Bucket::Dirty, "脏是同一条命令真读到的，不作废");

        let mut t = st(true, 0, 0, false);
        t.has_upstream = false;
        assert_eq!(bucket_of(&t), Bucket::Synced, "没有上游 ⇒ 那一档本来就不存在");

        // 数字读得出来时，新那一档绝不能自己冒出来
        let u = st(true, 2, 0, false);
        assert_eq!(bucket_of(&u), Bucket::Ahead);
    }

    /// 空的一份 detected（四条全空）**既不落库也不灌进界面**：它在库里存在的
    /// 唯一作用就是把 `Shell::new` 现算出来的那份好的盖掉
    #[test]
    fn four_blank_commands_are_not_a_value() {
        let blank = run_cmd::CmdDetect::default();
        assert!(
            !recognised_commands_are_present(&blank),
            "四条全空 = 没认出来，不是「认出来四条空的」"
        );
        // 只认出一条也算认出来了——这条钉住那个 `&&` 串（改成 `||` 当场红）
        for d in [
            run_cmd::CmdDetect {
                test_cmd: "cargo test".into(),
                ..Default::default()
            },
            run_cmd::CmdDetect {
                build_cmd: "cargo build".into(),
                ..Default::default()
            },
            run_cmd::CmdDetect {
                dev_cmd: "cargo run".into(),
                ..Default::default()
            },
            run_cmd::CmdDetect {
                install_cmd: "cargo fetch".into(),
                ..Default::default()
            },
        ] {
            assert!(recognised_commands_are_present(&d), "四条里任意一条有值都算认出来");
        }
        // 读过的行 + 空命令 ⇒ 不可信（不然是旧判据 `last_commit_ts > 0` 也放行的一种）
        let c = core_cache::RepoCache {
            status_read: true,
            detected: blank,
            ..Default::default()
        };
        assert!(!cache_detected_is_trustworthy(&c), "空的一份不该盖掉现算的那份");
    }
}
