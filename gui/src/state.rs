//! 界面状态与纯逻辑：仓库视图、操作类别、对话框状态、分档口径。
//!
//! `bucket_of` 是全站唯一的仓库分档口径——侧栏徽标、总览、同步环、
//! ⌘K 状态点都从这儿取。别在别处另写一套判断。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::component::command::CommandState;
use gpui_kit::component::input::InputState;
use gpui_kit::component::list::ListState;
use gpui_kit::component::resizable::ResizableState;
use gpui_kit::component::table::TableState;
use gpui_kit::component::text::TextViewState;
use gpui_kit::component::Theme;
use gpui_kit::{AnyWindowHandle, Entity, FocusHandle, Hsla, ScrollHandle, SharedString, Task};
use qingbird_core::history::{CommitDetail, CommitPatch, CommitSummary};
use qingbird_core::repos::{self, ActivityDay, ChangeEntry, LangStat};
use qingbird_core::run_cmd::{self, Slot};
use qingbird_core::space;
use qingbird_core::store::{self, CloseAction, RepoEntry};
use qingbird_core::types::{ForkInfo, RepoStatus};
use std::collections::{HashMap, HashSet};

use crate::history_tab::CommitDelegate;
use crate::palette::PaletteAction;
use crate::tokens::*;

/// 一个仓库在界面上要显示的东西
pub(crate) struct RepoView {
    pub(crate) status: RepoStatus,
    /// 未提交文件数（仓库头的「未提交 N 文件」）
    pub(crate) changed: u32,
    /// 未提交的**条目**（提交对话框的变更摘要、检视抽屉的「变更」页签）。
    ///
    /// 与 `changed` 同源：`changed` 就是这个数组的长度，状态扫描也只跑
    /// 一条 `git status --porcelain`（`changed_files` 跑的是同一条命令，
    /// 换过去等于白赚一份文件名列表，不多派生任何子进程）
    pub(crate) changes: Vec<ChangeEntry>,
    /// 最后一次提交的秒级时间戳，0 = 从没提交过（仓库头那一行）
    pub(crate) last_commit_ts: u64,
    /// 自动识别出来的四条命令：仓库没存命令时拿来预填
    pub(crate) detected: run_cmd::CmdDetect,
}

impl RepoView {
    pub(crate) fn blank() -> Self {
        Self {
            status: RepoStatus::default(),
            changed: 0,
            changes: Vec::new(),
            last_commit_ts: 0,
            detected: run_cmd::CmdDetect::default(),
        }
    }
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
    /// 当前阶段文案（拉取 / 检测会报阶段名）
    pub(crate) stage: Option<String>,
    /// 确定态百分比 0–100。**None = 不定态**：跑命令给不出可信百分比，
    /// 造一个假的只会让人以为快好了，然后越等越焦
    pub(crate) percent: Option<f32>,
    /// 上次操作的秒级时间戳，0 = 本会话没跑过
    pub(crate) ts: u64,
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
    percent: None,
    ts: 0,
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
}

/// 导航轨指向的页面
/// **只有两态**（D2）：第 2、3 个导航按钮的语义是"驱动侧栏"而不是"切到第三种页面"，
/// 「当前看哪一组」是侧栏的焦点态 `side_focus`，不往 `Page` 上加变体。
/// 给"第三方仓库"开第三个 `Page` 会让总览/仓库两屏的判据分裂成两套。
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Page {
    /// 仓库工作区（含两类仓库，按归属裁剪内容）
    Repos,
    /// 总览：只有统计，没有"当前仓库"这个对象
    Overview,
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

/// 导航轨三个按钮点下去各自意味着什么。
/// 前两个按钮的差别不是"切到哪一屏"，而是"侧栏看哪一组"（D2）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Nav {
    Overview,
    Group(Group),
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
    /// 检视抽屉当前宽度（px，已夹在 `INSPECTOR_MIN_W..INSPECTOR_MAX_W`）。
    /// 拖完由 `on_resize` 写回，收起再展开恢复的就是它
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
    pub(crate) pending_folder: bool,
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
    /// 克隆对话框；None = 未打开。**与添加/编辑仓库那个 `dlg` 分开**：
    /// 那个是"把已经在磁盘上的目录登记进来"，克隆是"先从网络取回来"，
    /// 两者字段几乎不重叠，挤在一个结构体里只会让两处各自判有没有
    pub(crate) clone_dlg: Option<CloneDialog>,
    /// 命令编辑对话框；None = 未打开
    pub(crate) cmd_dlg: Option<CmdDialog>,
    /// 设置对话框；None = 未打开。导航轨那颗齿轮与 ⌘K「设置」共用这一个入口
    pub(crate) settings_dlg: Option<SettingsDialog>,
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

/// 提交并推送对话框的状态
pub(crate) struct CommitDialog {    pub(crate) path: String,
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
}

impl SetCat {
    pub(crate) const ALL: &'static [SetCat] = &[SetCat::Identity, SetCat::Network, SetCat::Display];

    pub(crate) fn label(self) -> &'static str {
        match self {
            SetCat::Identity => "身份与访问",
            SetCat::Network => "网络与通知",
            SetCat::Display => "显示",
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
        }
    }

    /// 左侧那颗键旁边的一句话。**每个分类都要有**——只有名字的话，
    /// 「显示」这两个字说不清里面是主题还是别的东西
    pub(crate) fn hint(self) -> &'static str {
        match self {
            SetCat::Identity => "提交用的名字与邮箱、云端令牌、SSH 私钥",
            SetCat::Network => "git 走不走代理、自动同步要不要弹通知",
            SetCat::Display => "浅色还是深色、界面上摆哪几样东西",
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
    /// 当前阶段词（"接收对象" / "正在取消…" …）
    pub(crate) stage: String,
    /// 确定态百分比 0–100。**None = 不定态**，界面走官方的不定态条
    pub(crate) percent: Option<f32>,
    /// 实时输出（只留文字变了的那些行）
    pub(crate) output: Vec<String>,
    /// 失败的一句中文结论 + git 原始 stderr
    pub(crate) error: Option<String>,
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
pub(crate) fn batch_summary(kind: BatchKind, done: usize, total: usize, ok: usize, skipped: usize, failed: usize) -> String {
    if done < total {
        return format!(
            "{} · {done}/{total}",
            kind.label()
        );
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

/// 批量同步的进度与结论。入口在 ⌘K（Q3），**界面上的复选框与批量条在阶段 6 已删**。
///
/// 汇总写进 `Shell::app_log`（全局槽）而不是某个仓库的日志：这一批横跨多个
/// 仓库，记到"当前选中"名下会让人以为只动了一个
pub(crate) struct BatchRun {
    pub(crate) kind: BatchKind,
    /// 队列里的仓库路径，顺序即执行顺序
    pub(crate) paths: Vec<String>,
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
        Bucket::Dirty
    } else {
        Bucket::Synced
    }
}

/// 分档取色。**全站唯一一份口径**（Q7，2026-10-03 用户拍板），
/// 侧栏状态点 / 侧栏徽标 / 同步环 / ⌘K 状态点 / 总览统计卡都从这里取。
///
/// | 档位 | 取色 | 理由 |
/// |---|---|---|
/// | 已同步 | 绿 `success` | 干净是常态也安全，与旧 React 版一致，不回归 |
/// | 有未提交 | 蓝 `accent` | 需要动手但不紧急 |
/// | 待推送 | 蓝 `accent` | 与上一档同语义，不另开色 |
/// | 落后 | 橙 `chart_3` | 拉一下就好 |
/// | 已分叉 | 红 `danger` | 唯一"不能一键解决"的档，值得最重的色 |
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
        Bucket::Unknown => t.colors.border,
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
        Bucket::Unknown => "待扫描",
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
    last_commit_ts: 0,
    status: RepoStatus {
        is_repo: false,
        branch: String::new(),
        ahead: 0,
        behind: 0,
        dirty: false,
        has_upstream: false,
        upstream: String::new(),
        remote_url: String::new(),
    },
    changed: 0,
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
            batch_summary(BatchKind::Pull, 3, 3, 3, 0, 0),
            "批量拉取 · 3 成功"
        );
        assert_eq!(
            batch_summary(BatchKind::Push, 2, 2, 1, 1, 0),
            "批量推送 · 1 成功 · 1 跳过",
            "顺序固定在成功之后"
        );
        assert_eq!(
            batch_summary(BatchKind::Check, 4, 4, 1, 0, 3),
            "批量检测 · 1 成功 · 3 失败"
        );
    }

    /// 跑一半停下来要说清跑到哪、还剩几个没动；
    /// **停在最后一个上算跑完了**，不报「0 个没动」
    #[test]
    fn batch_summary_distinguishes_stopped_from_finished() {
        assert_eq!(
            batch_summary(BatchKind::Pull, 2, 5, 2, 0, 1),
            "批量拉取 · 2/5",
            "还在跑 / 被停：报进度"
        );
        assert_eq!(
            batch_summary(BatchKind::Push, 2, 2, 2, 0, 0),
            "批量推送 · 2 成功",
            "done == total 就是跑完了，哪怕 running 是 false"
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
}
