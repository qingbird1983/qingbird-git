//! 应用壳：标题栏 / 导航轨 / 仓库侧栏 / 主区 / 状态条。
//!
//! 结构严格对应设计稿（Ardot 731415807346283）：
//! - Screen 1「仓库工作区」：选中**第三方**仓库时显示——侧栏(264px) + 档案页
//! - Screen 3「我的仓库工作台」：选中**我的**仓库时显示——无侧栏，主区直接铺满
//! - Screen 2「总览批量同步」：导航轨第二项，待实现
//!
//! 数据全部来自 qingbird-core：真实仓库列表、真实 git 状态、真实命令。
//! 耗时操作（扫状态、跑命令、扫磁盘）一律进后台线程，界面先画上一次结果。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::InteractiveElementExt as _;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, Size, Theme, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, ease_in_out, relative, AnyElement, AnyWindowHandle, Animation, AnimationExt as _,
    AppContext as _, Context, Div, Entity, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement, Render, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled, Task, Window, px, rgb,
};
use qingbird_core::repos::{self, ActivityDay, LangStat, ReadmePreview};
use qingbird_core::run_cmd::{self, Slot, MAX_PINNED_CMDS};
use qingbird_core::space;
use qingbird_core::store::RepoEntry;
use qingbird_core::sync;
use qingbird_core::types::RepoStatus;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---------- 规范板色板中 theme.json 没有的补充灰阶 ----------
// 设计规范：正文 #37352F / 三级 #787871 / 四级 #A4A099 / 占位 #BBB8B1，
// 以及选中暖灰面 #EDEBE7、条轨 #EDEAE5、可更新暖橙底 #FDEFE4。
// theme.json 的 token 覆盖不了这几级，集中在这里定义，禁止散落魔法数字。

/// 正文 / 行内主要文字（比 foreground #1A1A1A 柔一档）
fn body(_t: &Theme) -> Hsla {
    rgb(0x37352F).into()
}
/// 三级：元信息、说明文字
fn tier3(_t: &Theme) -> Hsla {
    rgb(0x787871).into()
}
/// 四级：弱提示、上次结果
fn tier4(_t: &Theme) -> Hsla {
    rgb(0xA4A099).into()
}
/// 选中项暖灰面（侧栏行 / 导航图标底）
fn warm(_t: &Theme) -> Hsla {
    rgb(0xEDEBE7).into()
}
/// 构成条 / 进度条的轨道
fn track(_t: &Theme) -> Hsla {
    rgb(0xEDEAE5).into()
}
/// 「可更新」徽章 / 清理条的暖橙底
fn orange_bg(_t: &Theme) -> Hsla {
    rgb(0xFDEFE4).into()
}
/// 清理条上的深橙文字
fn orange_text(_t: &Theme) -> Hsla {
    rgb(0x9A4500).into()
}
/// 「运行中 / 未提交」徽章的蓝灰底
fn blue_bg(_t: &Theme) -> Hsla {
    rgb(0xEDF1F5).into()
}
/// 次按钮描边（比 hairline 深一档）
fn edge(_t: &Theme) -> Hsla {
    rgb(0xC8C4BE).into()
}

/// 「落后 / 分叉」的橙色。
/// 主题 token 里没有单独的 warning 语义色（只有 `button_warning`），
/// theme.json 已把 `chart.3` 定成规范板的 #DD5B00，这里借用它。
fn orange(t: &Theme) -> Hsla {
    t.colors.chart_3
}

// ---------- 版面规范：字号 / 圆角 / 间距 / 按钮 / 图标 ----------
// 之前这些值是就地硬编码，结果同一个「卡片圆角」在不同屏上冒出
// 1/2/3/5/10 五种杂值，按钮高度有 6 种。这里定成档位，新增元素只能选档，
// 不许再写裸数字——视觉一致靠的是只有这几档可挑。

/// 元信息、弱提示（标签、说明）
const FS_META: f32 = 11.0;
/// 次要文字（表格行、副标题）
const FS_SEC: f32 = 12.0;
/// 正文
const FS_BODY: f32 = 13.0;
/// 卡片标题 / 弹窗标题
const FS_TITLE: f32 = 16.0;
/// 页面主标题
const FS_HEAD: f32 = 18.0;
/// 统计卡大数字
const FS_NUM: f32 = 20.0;

/// 小色块 / 进度条
const R_SM: f32 = 4.0;
/// chip、徽章、图例方块
const R_MD: f32 = 6.0;
/// 按钮、列表行、小面板
const R_LG: f32 = 8.0;
/// 大卡片、表格容器
const R_XL: f32 = 12.0;

/// 紧密（标签与值、柱间）
const GAP_TIGHT: f32 = 4.0;
/// 常规（图标与文字、行内元素）
const GAP: f32 = 8.0;
/// 分组（卡片内区块）
const GAP_LG: f32 = 12.0;

/// 行内小按钮
const BTN_SM: f32 = 26.0;
/// 常规按钮
const BTN: f32 = 30.0;
/// 主操作按钮（拉取 / 保存）
const BTN_LG: f32 = 34.0;

/// 图标档位：行内小图标
const ICON_SM: f32 = 14.0;
/// 图标档位：按钮内图标
const ICON: f32 = 16.0;

/// 悬停状态的 key：仓库行用路径，「添加仓库」这条没有路径，用固定 key
const ADD_HOVER_KEY: &str = "__add_repo__";

/// 行内图标（统一走 lucide 一套，只有品牌 logo 用 assets 的 Bird）。
/// 尺寸显式写死而不是 `.small()`：两套图标集混着用时，默认尺寸并不一致，
/// 显式档位才能保证行与行对齐。
fn icon_sm(name: impl Into<Icon>) -> Icon {
    Icon::new(name).with_size(Size::Size(px(ICON_SM)))
}

/// 按钮内图标
fn icon_md(name: impl Into<Icon>) -> Icon {
    Icon::new(name).with_size(Size::Size(px(ICON)))
}

/// 一个仓库在界面上要显示的东西
struct RepoView {
    status: RepoStatus,
    /// 未提交文件数（仓库头的「未提交 N 文件」）
    changed: u32,
    /// 自动识别出来的四条命令：仓库没存命令时拿来预填
    detected: run_cmd::CmdDetect,
}

impl RepoView {
    fn blank() -> Self {
        Self {
            status: RepoStatus::default(),
            changed: 0,
            detected: run_cmd::CmdDetect::default(),
        }
    }
}

/// 输出块里正在跑的操作类别
#[derive(Clone, Copy, PartialEq)]
enum Op {
    Cmd(Slot),
    Pull,
    Push,
    Check,
}

/// 深色输出块的状态
#[derive(Default)]
struct OutState {
    running: bool,
    /// 正在跑哪个操作（None = 没在跑）
    op: Option<Op>,
    lines: Vec<String>,
    /// 命令起不来 / 非零退出时的一句话
    error: Option<String>,
}

/// 磁盘占用卡的状态（只跟随当前选中的仓库）
#[derive(Default)]
struct SpaceState {
    report: Option<space::SpaceReport>,
    scanning: bool,
    /// 一键清理进行中：按钮转圈、防连点
    cleaning: bool,
    /// 上次清理的结果（「已释放 1.6 GB」）
    note: Option<String>,
    error: Option<String>,
}

/// 档案页（Screen 1）的本地数据：语言统计 / 活跃度 / 最近提交 / README。
/// 全部是几条本地 git 命令 + 读一个文件，放后台线程扫，界面先画占位。
#[derive(Default)]
struct ProfileState {
    langs: Vec<LangStat>,
    /// 近 30 天有提交的日子（后端不补空白日）
    activity: Vec<ActivityDay>,
    /// 最近一次提交的秒级时间戳，0 = 从没提交过
    last_commit_ts: u64,
    readme: Option<ReadmePreview>,
    scanning: bool,
}

/// 导航轨指向的页面
#[derive(Clone, Copy, PartialEq, Debug)]
enum Page {
    /// Screen 1 / Screen 3：仓库工作区
    Repos,
    /// Screen 2：总览批量同步（待实现）
    Overview,
}

pub struct Shell {
    /// 窗口句柄：异步跑完（初始化仓库等）要回来关弹窗
    win: AnyWindowHandle,
    page: Page,
    repos: Vec<RepoEntry>,
    views: HashMap<String, RepoView>,
    selected: Option<String>,
    /// 标题栏全局搜索（官方 Input，检索逻辑待接）
    global_search: Entity<InputState>,
    /// 侧栏过滤框（官方 Input，过滤逻辑待接）
    side_filter: Entity<InputState>,
    scanning: bool,
    /// 状态扫描的任务句柄：存着才能在下一次扫描时丢掉上一次
    scan: Option<Task<()>>,
    /// 扫描代数：过期结果不许覆盖新结果
    seq: u64,
    out: OutState,
    space: SpaceState,
    /// 磁盘扫描的代数：切仓库 / 重扫时作废旧结果
    space_seq: u64,
    space_task: Option<Task<()>>,
    profile: ProfileState,
    profile_seq: u64,
    profile_task: Option<Task<()>>,
    /// 总览页勾选的仓库路径（Screen 2 表格行首复选框）
    ov_checked: std::collections::HashSet<String>,
    /// 总览页批量同步的进度与结论；None = 本会话还没跑过批量
    batch: Option<BatchRun>,
    /// 添加 / 编辑仓库弹窗；None = 弹窗未打开
    dlg: Option<RepoDialog>,
    /// 鼠标悬停在哪一行（侧栏仓库 / 总览表格行）。
    /// gpui 没有 CSS 那样的 `:hover` 样式，悬停反馈只能靠事件记状态再重画。
    hovered: Option<String>,
}

/// 添加 / 编辑仓库弹窗的状态。
/// 输入框的 Entity 在打开弹窗时重建（初值靠 placeholder 表达，不预填——
/// 预填要走 `InputState::set_value`，那需要 window，弹窗入口处没有）。
struct RepoDialog {
    /// None = 添加模式；Some(path) = 编辑该仓库
    editing: Option<String>,
    path_input: Entity<InputState>,
    name_input: Entity<InputState>,
    /// 归属：第三方仓库（false = 我的）
    third: bool,
    error: Option<String>,
    /// 非空 = 初始化向导：填的路径还不是 git 仓库，转这里引导 git init
    init: Option<InitState>,
}

/// 初始化向导的状态：顶层目录清单 + 勾选的排除项
#[derive(Clone)]
struct InitState {
    path: String,
    items: Vec<repos::DirItem>,
    excludes: std::collections::HashSet<String>,
    working: bool,
}

/// 初始化时默认勾选的排除项：构建产物 / 依赖 / IDE 目录。
/// 与旧版同一份清单——用户大概率不想把这些提交进去。
const INIT_EXCLUDE_HINTS: &[&str] = &[
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

/// 总览页的一键批量。与旧版约定一致：前端串行循环 core 的 pull/push，
/// 不新增后端批量命令——代理回退、脏工作区判定都已在那两条路径里。
struct BatchRun {
    pull: bool,
    /// 队列里的仓库路径，顺序即执行顺序
    paths: Vec<String>,
    done: usize,
    ok: usize,
    /// 拉取时脏工作区跳过的个数（推送没有这条路，恒 0）
    skipped: usize,
    failed: usize,
    running: bool,
    /// 跑完（或失败）的一句话结论，总览页状态条展示
    note: Option<String>,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let repos = repos::list_repos();
        // 自动识别只读文件、不派生 git 子进程，放启动路径上不卡界面
        let mut views = HashMap::new();
        for r in &repos {
            views.insert(
                r.path.clone(),
                RepoView {
                    detected: run_cmd::detect_cmds(&r.path),
                    ..RepoView::blank()
                },
            );
        }
        let selected = repos.first().map(|r| r.path.clone());
        let global_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索仓库 · Ctrl K"));
        let side_filter = cx.new(|cx| InputState::new(window, cx).placeholder("筛选名称或分支"));
        let mut this = Self {
            win: window.window_handle(),
            page: Page::Repos,
            repos,
            views,
            selected,
            global_search,
            side_filter,
            scanning: false,
            scan: None,
            seq: 0,
            out: OutState::default(),
            space: SpaceState::default(),
            space_seq: 0,
            space_task: None,
            profile: ProfileState::default(),
            profile_seq: 0,
            profile_task: None,
            ov_checked: std::collections::HashSet::new(),
            batch: None,
            dlg: None,
            hovered: None,
        };
        this.refresh_status(cx);
        this.scan_space(cx);
        this.scan_profile(cx);
        this
    }

    fn entry(&self, path: &str) -> Option<&RepoEntry> {
        self.repos.iter().find(|r| r.path == path)
    }

    fn view(&self, path: &str) -> Option<&RepoView> {
        self.views.get(path)
    }

    /// 当前仓库的显示名：有自定义名用自定义名
    fn title(&self, path: &str) -> String {
        self.entry(path)
            .map(|r| {
                if r.alias.trim().is_empty() {
                    r.name.clone()
                } else {
                    r.alias.trim().to_string()
                }
            })
            .unwrap_or_default()
    }

    /// 某槽位实际要跑的命令：仓库里存了就用存的，没存就用自动识别的
    /// （与旧版一致：识别结果只预填，不落盘）
    fn effective_cmd(&self, path: &str, slot: Slot) -> String {
        if let Some(e) = self.entry(path) {
            let saved = slot.cmd(e).trim();
            if !saved.is_empty() {
                return saved.to_string();
            }
        }
        let Some(v) = self.view(path) else {
            return String::new();
        };
        let d = &v.detected;
        match slot {
            Slot::Test => d.test_cmd.clone(),
            Slot::Build => d.build_cmd.clone(),
            Slot::Dev => d.dev_cmd.clone(),
            Slot::Install => d.install_cmd.clone(),
            Slot::Pinned(_) => String::new(),
        }
    }

    // ---------- 状态扫描 ----------

    /// 重扫所有仓库的 git 状态。每个仓库要派生 4~5 条 git 命令，串起来好几秒，
    /// 所以整个扫描丢进后台线程池，仓库之间并发，界面不等着。
    fn refresh_status(&mut self, cx: &mut Context<Self>) {
        self.seq += 1;
        let seq = self.seq;
        // 丢掉上一次的句柄（= 取消），免得两轮结果打架
        self.scan = None;
        self.scanning = true;
        cx.notify();

        let paths: Vec<String> = self.repos.iter().map(|r| r.path.clone()).collect();
        if paths.is_empty() {
            self.scanning = false;
            return;
        }
        self.scan = Some(cx.spawn(async move |this, cx| {
            let scan = cx
                .background_executor()
                .spawn(async move {
                    std::thread::scope(|s| {
                        let handles: Vec<_> = paths
                            .iter()
                            .map(|p| {
                                s.spawn(move || {
                                    (p.clone(), repos::read_status(p), repos::changed_files(p))
                                })
                            })
                            .collect();
                        handles
                            .into_iter()
                            .map(|h| {
                                h.join().unwrap_or_else(|_| {
                                    (String::new(), RepoStatus::default(), 0)
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .await;
            this.update(cx, |this, cx| {
                if this.seq != seq {
                    return; // 已经有更新的一轮在跑了，这批过期
                }
                this.scanning = false;
                for (path, status, changed) in scan {
                    let v = this.views.entry(path).or_insert_with(RepoView::blank);
                    v.status = status;
                    v.changed = changed;
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn select(&mut self, path: String, cx: &mut Context<Self>) {
        if self.selected.as_deref() == Some(path.as_str()) {
            return;
        }
        self.selected = Some(path.clone());
        self.out = OutState::default();
        repos::touch_repo(&path);
        self.scan_space(cx);
        self.scan_profile(cx);
        cx.notify();
    }

    // ---------- 添加 / 编辑仓库弹窗 ----------

    /// 打开「添加仓库」。没有原生目录选择对话框可用（gpui-kit 不带，
    /// 也不为它引新依赖），走「粘贴路径 + 校验」：资源管理器里复制文件地址即可。
    fn open_add_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dlg = Some(RepoDialog {
            editing: None,
            path_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("粘贴仓库文件夹路径，如 F:/AIwork/xxx")),
            name_input: cx.new(|cx| InputState::new(window, cx).placeholder("留空用目录名")),
            third: false,
            error: None,
            init: None,
        });
        self.build_dlg(window, cx);
    }

    /// 打开「仓库设置」：改显示名与归属，或从列表移除（不动磁盘文件）。
    fn open_edit_dlg(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let third = self
            .entry(&path)
            .map(|e| e.is_third_party)
            .unwrap_or(false);
        let alias = self.entry(&path).map(|e| e.alias.clone()).unwrap_or_default();
        // 当前别名进 placeholder：留空 = 不改（编辑模式不预填，避免误清空）
        let name_hint = if alias.trim().is_empty() {
            "留空用目录名".to_string()
        } else {
            format!("当前：{alias} · 留空即不改")
        };
        self.dlg = Some(RepoDialog {
            editing: Some(path.clone()),
            path_input: cx.new(|cx| InputState::new(window, cx).placeholder(path.clone())),
            name_input: cx.new(|cx| InputState::new(window, cx).placeholder(name_hint)),
            third,
            error: None,
            init: None,
        });
        self.build_dlg(window, cx);
    }

    /// 弹窗本体：官方 Dialog（覆盖层、聚焦、关闭都由框架管）。
    /// Shell 只负责把自己的状态与输入框挂进去。
    fn build_dlg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        let Some(dlg) = self.dlg.as_ref() else {
            return;
        };
        let editing = dlg.editing.clone();
        let path_input = dlg.path_input.clone();
        let name_input = dlg.name_input.clone();

        window.open_dialog(cx, move |dialog, _window, cx| {
            let t = cx.theme().clone();
            let state = this.read(cx).dlg.as_ref();
            let third = state.map(|d| d.third).unwrap_or(false);
            let error: Option<String> = state.and_then(|d| d.error.clone());
            let init: Option<InitState> = state.and_then(|d| d.init.clone());

            dialog
                .title(if init.is_some() {
                    "初始化为 Git 仓库"
                } else if editing.is_some() {
                    "仓库设置"
                } else {
                    "添加仓库"
                })
                // content / footer 的 builder 会被反复调用，每次重画一份
                .content({
                    let t = t.clone();
                    let editing = editing.clone();
                    let path_input = path_input.clone();
                    let name_input = name_input.clone();
                    let error = error.clone();
                    let init = init.clone();
                    let this = this.clone();
                    move |c, _, _| match &init {
                        Some(i) => c.child(init_body(&t, i, &error, &this)),
                        None => c.child(dlg_body(
                            &t,
                            &editing,
                            third,
                            &error,
                            &path_input,
                            &name_input,
                            &this,
                        )),
                    }
                })
                .footer(match &init {
                    Some(i) => init_footer(&t, i, &this),
                    None => dlg_footer(&this, &editing, &path_input, &name_input),
                })
        });
    }

    /// 保存弹窗：添加时校验路径与 .git，编辑时改名与改归属。
    /// 返回 true = 成功（调用方据此关弹窗），false = 留在原地看错误。
    fn commit_dlg(
        &mut self,
        editing: Option<String>,
        raw_path: String,
        raw_name: String,
        cx: &mut Context<Self>,
    ) -> bool {
        let third = self.dlg.as_ref().map(|d| d.third).unwrap_or(false);
        let name = raw_name.trim().to_string();

        match editing {
            None => {
                let path = repos::normalize_path_input(&raw_path);
                if path.is_empty() {
                    self.set_dlg_error("请先填写仓库路径", cx);
                    return false;
                }
                if self.repos.iter().any(|r| r.path == path) {
                    self.set_dlg_error("这个仓库已经在列表里了", cx);
                    return false;
                }
                // 不是 git 仓库不是用户的错：转初始化向导，别甩一句报错（旧版同行为）
                if !repos::valid_repo(&path) {
                    if !std::path::Path::new(&path).is_dir() {
                        self.set_dlg_error("文件夹不存在，请检查路径", cx);
                        return false;
                    }
                    self.enter_init(path, cx);
                    return false;
                }
                let list = match repos::add_repo(&path) {
                    Ok(l) => l,
                    Err(e) => {
                        self.set_dlg_error(&e, cx);
                        return false;
                    }
                };
                // 名字与归属都只在添加成功之后落盘，别留半成品
                if !name.is_empty() {
                    let _ = repos::rename_repo(&path, &name);
                }
                let _ = qingbird_core::store::set_third_party(&path, third);
                self.repos = repos::list_repos();
                let _ = list;
                // 新仓库的命令要重新识别（启动时只识别了列表里已有的）
                self.views
                    .entry(path.clone())
                    .or_insert_with(RepoView::blank)
                    .detected = run_cmd::detect_cmds(&path);
                self.selected = Some(path.clone());
                self.out = OutState::default();
                self.dlg = None;
                self.refresh_status(cx);
                self.scan_space(cx);
                self.scan_profile(cx);
                cx.notify();
                true
            }
            Some(path) => {
                // 留空 = 不改（placeholder 就是这么写的，别把别名清掉）
                if !name.is_empty() && repos::rename_repo(&path, &name).is_err() {
                    self.set_dlg_error("显示名保存失败（配置写不进去）", cx);
                    return false;
                }
                if qingbird_core::store::set_third_party(&path, third).is_err() {
                    self.set_dlg_error("归属保存失败（配置写不进去）", cx);
                    return false;
                }
                self.repos = repos::list_repos();
                self.dlg = None;
                // 归属变了主区换页：划成第三方要立刻补扫档案数据
                self.scan_profile(cx);
                cx.notify();
                true
            }
        }
    }

    /// 弹窗切到初始化向导：读出顶层目录，预勾常见排除项
    fn enter_init(&mut self, path: String, cx: &mut Context<Self>) {
        let items = repos::list_dir_items(&path);
        let mut excludes = std::collections::HashSet::new();
        for it in &items {
            if it.is_dir && INIT_EXCLUDE_HINTS.contains(&it.name.as_str()) {
                excludes.insert(it.name.clone());
            }
        }
        if let Some(d) = self.dlg.as_mut() {
            d.init = Some(InitState {
                path,
                items,
                excludes,
                working: false,
            });
            d.error = None;
        }
        cx.notify();
    }

    /// 跑初始化：后台线程（git init + add + commit 要好几秒），
    /// 成功则入列表并选中，失败把原因留在弹窗里。
    fn run_init(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.dlg.as_ref() else {
            return;
        };
        let Some(init) = d.init.as_ref() else {
            return;
        };
        if init.working {
            return;
        }
        let path = init.path.clone();
        let excludes: Vec<String> = init.excludes.iter().cloned().collect();
        if let Some(d) = self.dlg.as_mut() {
            if let Some(i) = d.init.as_mut() {
                i.working = true;
            }
            d.error = None;
        }
        cx.notify();

        cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { repos::init_repo(&path, &excludes) })
                .await;
            let win = this.update(cx, |this, _| this.win).ok();
            this.update(cx, |this, cx| {
                match r {
                    Ok(entry) => {
                        this.repos = repos::list_repos();
                        this.views
                            .entry(entry.path.clone())
                            .or_insert_with(RepoView::blank)
                            .detected = run_cmd::detect_cmds(&entry.path);
                        this.selected = Some(entry.path.clone());
                        this.out = OutState::default();
                        this.dlg = None;
                        this.refresh_status(cx);
                        this.scan_space(cx);
                        this.scan_profile(cx);
                        // 成了就把弹窗关掉；失败留在原地让用户看到原因
                        if let Some(win) = win {
                            let _ = cx.update_window(win, |_, window, cx| {
                                window.close_dialog(cx);
                            });
                        }
                    }
                    Err(e) => {
                        if let Some(d) = this.dlg.as_mut() {
                            d.error = Some(e);
                            if let Some(i) = d.init.as_mut() {
                                i.working = false;
                            }
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 写错误提示并通知重画：弹窗是浮层，不 notify 的话错误文案不会刷出来
    fn set_dlg_error(&mut self, msg: &str, cx: &mut Context<Self>) {
        if let Some(d) = self.dlg.as_mut() {
            d.error = Some(msg.to_string());
        }
        cx.notify();
    }

    /// 从列表移除仓库（只删记录，不动磁盘文件）
    fn remove_repo_at(&mut self, path: String, cx: &mut Context<Self>) {
        let _ = repos::remove_repo(&path);
        self.repos = repos::list_repos();
        self.views.remove(&path);
        if self.selected.as_deref() == Some(path.as_str()) {
            self.selected = self.repos.first().map(|r| r.path.clone());
            self.out = OutState::default();
            self.scan_space(cx);
            self.scan_profile(cx);
        }
        self.dlg = None;
        cx.notify();
    }

    // ---------- 跑命令 ----------

    fn run_slot(&mut self, slot: Slot, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        let cmd = self.effective_cmd(&path, slot);
        if cmd.is_empty() {
            self.out.error = Some(format!(
                "识别不出「{}」的命令，这条先跳过",
                slot.default_label()
            ));
            cx.notify();
            return;
        }

        self.out = OutState {
            running: true,
            op: Some(Op::Cmd(slot)),
            lines: Vec::new(),
            error: None,
        };
        cx.notify();

        // 命令在后台线程跑，行进共享缓冲；UI 线程定时把新行搬进状态
        let buf: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));

        let sink_buf = Arc::clone(&buf);
        let sink_done = Arc::clone(&done);
        let run_path = path.clone();
        let runner = cx.background_executor().spawn(async move {
            let on_line: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line: &str| {
                if let Ok(mut g) = sink_buf.lock() {
                    g.push(line.to_string());
                }
            });
            let r = run_cmd::run_repo_cmd(&run_path, &cmd, slot, on_line);
            sink_done.store(true, Ordering::SeqCst);
            r
        });

        let ui_buf = Arc::clone(&buf);
        let ui_done = Arc::clone(&done);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                let batch = {
                    let mut g = ui_buf.lock().unwrap_or_else(|e| e.into_inner());
                    std::mem::take(&mut *g)
                };
                let finished = ui_done.load(Ordering::SeqCst);
                this.update(cx, |this, cx| {
                    this.out.lines.extend(batch);
                    if finished {
                        this.out.running = false;
                    }
                    cx.notify();
                })
                .ok();
                if finished {
                    break;
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            let result = runner.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(_) => this.out.error = None,
                    Err(e) => this.out.error = Some(e),
                }
                this.out.running = false;
                // 命令跑完仓库状态可能变了（生成了构建产物、改了文件），顺手重扫
                this.repos = repos::list_repos();
                this.refresh_status(cx);
            })
            .ok();
        })
        .detach();
    }

    /// 跑一个同步操作（检测更新 / 拉取 / 推送）。
    /// 与 run_slot 同一套后台管道：后台线程跑 + 共享缓冲，UI 线程 120ms 轮询回显。
    fn run_sync(&mut self, op: Op, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        // core 的操作锁同一时刻只放一个网络操作，UI 先挡掉连点
        if self.out.running {
            return;
        }
        self.out = OutState {
            running: true,
            op: Some(op),
            lines: Vec::new(),
            error: None,
        };
        cx.notify();

        let buf: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));

        let sink_buf = Arc::clone(&buf);
        let sink_done = Arc::clone(&done);
        let run_path = path.clone();
        let runner = cx.background_executor().spawn(async move {
            let on_progress: Arc<dyn Fn(&qingbird_core::types::ProgressEvent) + Send + Sync> =
                Arc::new(move |ev: &qingbird_core::types::ProgressEvent| {
                    if let Ok(mut g) = sink_buf.lock() {
                        let mut line = format!("· {}", ev.stage);
                        if !ev.detail.is_empty() {
                            line.push_str(&format!("  {}", ev.detail));
                        }
                        g.push(line);
                    }
                });
            let r = match op {
                Op::Pull => sync::pull_repo(&run_path, true, Some(&*on_progress)),
                Op::Push => sync::push_repo(&run_path),
                Op::Check => sync::check_update(&run_path, Some(&*on_progress)),
                Op::Cmd(_) => unreachable!("run_sync 只接同步操作"),
            };
            sink_done.store(true, Ordering::SeqCst);
            r
        });

        let ui_buf = Arc::clone(&buf);
        let ui_done = Arc::clone(&done);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                let batch = {
                    let mut g = ui_buf.lock().unwrap_or_else(|e| e.into_inner());
                    std::mem::take(&mut *g)
                };
                let finished = ui_done.load(Ordering::SeqCst);
                this.update(cx, |this, cx| {
                    this.out.lines.extend(batch);
                    if finished {
                        this.out.running = false;
                    }
                    cx.notify();
                })
                .ok();
                if finished {
                    break;
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            let result = runner.await;
            this.update(cx, |this, cx| {
                this.out.lines.push(if result.ok {
                    format!("✓ {}", result.message)
                } else {
                    format!("✗ {}", result.message)
                });
                this.out.running = false;
                // 拉取/推送会改 ahead/behind，重扫一遍刷新侧栏与徽章
                this.repos = repos::list_repos();
                this.refresh_status(cx);
            })
            .ok();
        })
        .detach();
    }

    // ---------- 磁盘占用 ----------

    /// 扫当前仓库的磁盘占用。大仓库可能好几秒，放后台，界面先画「扫描中」。
    fn scan_space(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        self.space_seq += 1;
        let seq = self.space_seq;
        self.space_task = None;
        // 重扫不打掉「已释放 X」的提示：清理完会立刻重扫，note 得活着
        let note = self.space.note.clone();
        self.space = SpaceState {
            scanning: true,
            note,
            ..SpaceState::default()
        };
        cx.notify();

        self.space_task = Some(cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { space::scan_repo_space(&path) })
                .await;
            this.update(cx, |this, cx| {
                if this.space_seq != seq {
                    return; // 切了仓库 / 已有新一轮，这批过期
                }
                this.space.scanning = false;
                match r {
                    Ok(rep) => this.space.report = Some(rep),
                    Err(e) => this.space.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// 一键清理：把报告里的可清理缓存目录逐个删掉（白名单由 core 把关）。
    fn clean_all(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        if self.space.cleaning {
            return;
        }
        let names: Vec<String> = self
            .space
            .report
            .as_ref()
            .map(|r| {
                r.dirs
                    .iter()
                    .filter(|d| d.is_cache && d.size > 0)
                    .map(|d| d.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        if names.is_empty() {
            return;
        }
        self.space.cleaning = true;
        self.space.note = None;
        self.space.error = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let freed = cx
                .background_executor()
                .spawn(async move {
                    let mut total = 0u64;
                    for n in &names {
                        match space::delete_cache_dir(&path, n) {
                            Ok(s) => total += s,
                            Err(e) => return Err(format!("清理「{n}」时中断：{e}")),
                        }
                    }
                    Ok(total)
                })
                .await;
            this.update(cx, |this, cx| {
                this.space.cleaning = false;
                match freed {
                    Ok(bytes) => {
                        this.space.note = Some(format!("已释放 {}", space::human_size(bytes)));
                        // 清完重扫，构成条与图例立刻对上
                        this.scan_space(cx);
                    }
                    Err(e) => this.space.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ---------- 档案页数据（Screen 1） ----------

    /// 扫当前仓库的档案数据：语言统计 / 近 30 天活跃度 / 最近提交 / README。
    /// 合起来十几条本地 git 命令，一个后台任务跑完一次性上屏。
    /// 只有第三方仓库显示档案页，「我的」仓库不扫这些。
    fn scan_profile(&mut self, cx: &mut Context<Self>) {
        let need = self
            .selected
            .as_deref()
            .and_then(|p| self.entry(p))
            .map(|e| e.is_third_party)
            .unwrap_or(false);
        self.profile_seq += 1;
        self.profile_task = None;
        if !need {
            self.profile = ProfileState::default();
            return;
        }
        let Some(path) = self.selected.clone() else {
            return;
        };
        let seq = self.profile_seq;
        self.profile = ProfileState {
            scanning: true,
            ..ProfileState::default()
        };
        cx.notify();

        self.profile_task = Some(cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move {
                    (
                        repos::repo_languages(&path),
                        repos::commit_activity(&path, PROFILE_ACTIVITY_DAYS),
                        repos::last_commit_ts(&path),
                        repos::read_readme(&path),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                if this.profile_seq != seq {
                    return; // 切了仓库 / 已有新一轮，这批过期
                }
                let (langs, activity, ts, readme) = r;
                this.profile = ProfileState {
                    langs,
                    activity,
                    last_commit_ts: ts,
                    readme,
                    scanning: false,
                };
                cx.notify();
            })
            .ok();
        }));
    }

    // ---------- 总览批量同步（Screen 2） ----------

    /// 串行跑一批同步操作。拉取遇脏工作区跳过（不自动 stash——那是替用户
    /// 做决定），与旧版总览批量的约定一致；推送没有跳过这条路。
    /// 全程一个后台任务，每完成一项就刷一次进度。
    fn run_batch(&mut self, pull: bool, paths: Vec<String>, cx: &mut Context<Self>) {
        if self.batch.as_ref().is_some_and(|b| b.running) {
            return; // 上一批还在跑
        }
        if paths.is_empty() {
            return;
        }
        self.batch = Some(BatchRun {
            pull,
            paths: paths.clone(),
            done: 0,
            ok: 0,
            skipped: 0,
            failed: 0,
            running: true,
            note: None,
        });
        cx.notify();

        cx.spawn(async move |this, cx| {
            for path in paths {
                // 跑之前先看脏不脏：拉取对脏工作区让路
                let skip = this
                    .update(cx, |this, _| {
                        pull
                            && this
                                .views
                                .get(&path)
                                .map(|v| v.status.dirty)
                                .unwrap_or(false)
                    })
                    .unwrap_or(false);
                // None = 跳过，Some(true) 成功，Some(false) 失败
                let outcome: Option<bool> = if skip {
                    None
                } else {
                    Some(
                        cx.background_executor()
                            .spawn(async move {
                                if pull {
                                    sync::pull_repo(&path, true, None).ok
                                } else {
                                    sync::push_repo(&path).ok
                                }
                            })
                            .await,
                    )
                };
                this.update(cx, |this, cx| {
                    let Some(b) = this.batch.as_mut() else {
                        return;
                    };
                    b.done += 1;
                    match outcome {
                        None => b.skipped += 1,
                        Some(true) => b.ok += 1,
                        Some(false) => b.failed += 1,
                    }
                    cx.notify();
                })
                .ok();
            }
            this.update(cx, |this, cx| {
                if let Some(b) = this.batch.as_mut() {
                    b.running = false;
                    let name = if b.pull { "批量拉取" } else { "批量推送" };
                    let mut parts = vec![format!("{} 个成功", b.ok)];
                    if b.skipped > 0 {
                        parts.push(format!("{} 个跳过（工作区有改动，需切到该仓库确认后再拉）", b.skipped));
                    }
                    if b.failed > 0 {
                        parts.push(format!("{} 个失败，原因见各自仓库的日志", b.failed));
                    }
                    b.note = Some(format!("{name}完成：{}", parts.join(" · ")));
                    // 拉取/推送改了 ahead/behind，重扫刷新侧栏与表格
                    this.refresh_status(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// 档案页迷你柱图的天数窗口。设计稿标「近 30 天活跃度」；
/// 与旧检视抽屉的 14 天不是一回事，各用各的参数。
const PROFILE_ACTIVITY_DAYS: u32 = 30;

/// 仓库在侧栏的状态口径：分叉 > 落后 > 领先 > 脏 > 已同步。
/// 与后端 `ForkInfo` 一致（ahead>0 && behind>0 就是分叉），不另立标准。
#[derive(Clone, Copy, PartialEq)]
enum Bucket {
    Synced,
    Dirty,
    Ahead,
    Behind,
    Forked,
    Unknown,
}

fn bucket_of(s: &RepoStatus) -> Bucket {
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

fn dot_color(b: Bucket, t: &Theme) -> Hsla {
    match b {
        Bucket::Synced => t.colors.success,
        // 设计稿：未提交用灰蓝强调点，不是灰
        Bucket::Dirty => t.colors.accent,
        Bucket::Ahead => t.colors.accent,
        Bucket::Behind | Bucket::Forked => orange(t),
        Bucket::Unknown => t.colors.border,
    }
}

/// 时间戳 → 「2 小时前」这种说法。0 = 从没跑过。
fn rel_time(ts: u64) -> String {
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

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();

        // 主区：按页面与仓库归属路由
        let main: AnyElement = match self.page {
            Page::Overview => self.overview_page(&t, cx).into_any_element(),
            Page::Repos => match self.selected.clone() {
                Some(path) => {
                    let third = self
                        .entry(&path)
                        .map(|e| e.is_third_party)
                        .unwrap_or(false);
                    if third {
                        self.profile_page(&path, &t, cx).into_any_element()
                    } else {
                        self.workbench_page(&path, &t, cx).into_any_element()
                    }
                }
                None => empty_main(&t),
            },
        };

        // 仓库页侧栏常驻（用户拍板）：选中的是「我的」主区显示工作台，
        // 「第三方」显示档案页；Screen 3 未画侧栏视为画稿聚焦省略
        let show_sidebar = self.page == Page::Repos;

        let body = h_flex()
            .flex_1()
            .min_h_0()
            .child(self.nav_rail(&t, cx))
            .child(vline(&t));
        let body = if show_sidebar {
            body.child(self.sidebar(&t, cx))
                .child(vline(&t))
                .child(main)
        } else {
            body.child(main)
        };

        v_flex()
            .size_full()
            .bg(t.colors.background)
            .text_color(t.colors.foreground)
            .child(self.title_bar(&t))
            .child(body)
    }
}

// ================= 标题栏 =================

impl Shell {
    /// 48px 白底顶栏：Logo + 应用名 + 全局搜索（窗口控制交给系统装饰）
    fn title_bar(&self, t: &Theme) -> Div {
        h_flex()
            .w_full()
            .h(px(48.0))
            .flex_shrink_0()
            .px(px(16.0))
            .gap(px(GAP_LG))
            .bg(t.colors.popover)
            .border_b_1()
            .border_color(t.colors.border)
            .items_center()
            // Logo：墨色圆角方块 + 白鸟
            .child(
                div()
                    .w(px(20.0))
                    .h(px(20.0))
                    .flex_shrink_0()
                    .rounded(px(R_SM))
                    .bg(t.colors.primary)
                    .items_center()
                    .justify_center()
                    .text_color(t.colors.popover)
                    .child(icon_sm(AssetIcon::Bird)),
            )
            .child(
                div()
                    .text_size(px(FS_BODY))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(body(t))
                    .child("青鸟 Git"),
            )
            .child(div().flex_1())
            // 全局搜索：官方 Input（检索逻辑待接 Ctrl K）
            .child(
                Input::new(&self.global_search)
                    .prefix(icon_sm(AssetIcon::Search))
                    .xsmall()
                    .w(px(260.0)),
            )
            .child(div().flex_1())
    }
}

// ================= 导航轨 =================

impl Shell {
    fn nav_rail(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        v_flex()
            .w(px(56.0))
            .h_full()
            .flex_shrink_0()
            .bg(t.colors.popover)
            .items_center()
            .p(px(10.0))
            .gap(px(GAP_TIGHT))
            .child(self.nav_item(AssetIcon::Folder, Page::Repos, t, cx))
            .child(self.nav_item(AssetIcon::LayoutDashboard, Page::Overview, t, cx))
            .child(div().flex_1())
            .child(
                Button::new("nav-plus")
                    .icon(AssetIcon::Plus)
                    .ghost()
                    .with_size(Size::Size(px(BTN_LG))),
            )
            .child(
                Button::new("nav-scan")
                    .icon(AssetIcon::RefreshCw)
                    .ghost()
                    .with_size(Size::Size(px(BTN_LG)))
                    .loading(self.scanning),
            )
            .child(
                Button::new("nav-settings")
                    .icon(AssetIcon::Settings)
                    .ghost()
                    .with_size(Size::Size(px(BTN_LG))),
            )
    }

    /// 可点导航项：36x36 圆角 8，选中暖灰面 + 墨色图标
    fn nav_item(
        &self,
        icon: AssetIcon,
        page: Page,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.page == page;
        let color = if active {
            t.colors.foreground
        } else {
            tier3(t)
        };
        h_flex()
            .id(SharedString::from(format!("nav-{:?}", page)))
            .w(px(36.0))
            .h(px(36.0))
            .flex_shrink_0()
            .rounded(px(R_LG))
            .items_center()
            .justify_center()
            .when(active, |it| it.bg(warm(t)))
            .child(div().text_color(color).child(icon_md(icon)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.page != page {
                    this.page = page;
                    cx.notify();
                }
            }))
    }
}

// ================= 仓库侧栏（Screen 1） =================

impl Shell {
    /// 264px 白底侧栏：标题 / 搜索 / 我的与第三方分组 / 底部添加仓库
    fn sidebar(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let side = v_flex()
            .w(px(264.0))
            .h_full()
            .flex_shrink_0()
            .bg(t.colors.popover)
            .p(px(12.0))
            .gap(px(GAP))
            .child(
                h_flex()
                    .w_full()
                    .px(px(2.0))
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(FS_SEC))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(tier4(t))
                            .child("仓库"),
                    )
                    .child(
                        div()
                            .text_size(px(FS_SEC))
                            .text_color(tier4(t))
                            .child(format!("{}", self.repos.len())),
                    ),
            )
            .child(
                // 侧栏过滤：官方 Input
                Input::new(&self.side_filter)
                    .prefix(icon_sm(AssetIcon::Search))
                    .xsmall()
                    .w_full(),
            );

        if self.repos.is_empty() {
            return side.child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap(px(GAP_TIGHT))
                    .child(
                        div()
                            .text_size(px(FS_SEC))
                            .text_color(tier4(t))
                            .child("还没有仓库"),
                    )
                    .child(
                        div()
                            .text_size(px(FS_SEC))
                            .text_color(tier4(t))
                            .child("用下面的「添加仓库」加一个"),
                    ),
            );
        }

        let mine: Vec<&RepoEntry> = self
            .repos
            .iter()
            .filter(|r| !r.is_third_party)
            .collect();
        let third: Vec<&RepoEntry> = self.repos.iter().filter(|r| r.is_third_party).collect();

        let mut list = v_flex()
            .id("sidebar-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap(px(2.0))
            .child(
                group_header(&format!("我的仓库 · {}", mine.len()), t),
            );
        for r in &mine {
            list = list.child(self.repo_row(r, t, cx));
        }
        list = list.child(group_header(&format!("第三方仓库 · {}", third.len()), t));
        for r in &third {
            list = list.child(self.repo_row(r, t, cx));
        }

        side.child(list).child(
            h_flex()
                .id("add-repo")
                .w_full()
                .h(px(36.0))
                .px(px(10.0))
                .rounded(px(R_LG))
                .items_center()
                .gap(px(GAP))
                .text_color(tier4(t))
                .when(self.hovered.as_deref() == Some(ADD_HOVER_KEY), |r| {
                    r.bg(t.colors.list_hover)
                })
                .child(icon_sm(AssetIcon::Plus))
                .child(div().text_size(px(FS_BODY)).child("添加仓库"))
                .on_click(cx.listener(|this, _, window, cx| this.open_add_dlg(window, cx)))
                .on_hover({
                    let this = cx.entity();
                    move |hovered, _, cx| {
                        this.update(cx, |this, cx| {
                            if *hovered {
                                this.hovered = Some(ADD_HOVER_KEY.to_string());
                            } else if this.hovered.as_deref() == Some(ADD_HOVER_KEY) {
                                this.hovered = None;
                            }
                            cx.notify();
                        });
                    }
                }),
        )
    }

    /// 侧栏仓库行：状态点 + 名称 + 状态徽章；单击选中，双击开设置
    fn repo_row(&self, r: &RepoEntry, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let path = r.path.clone();
        let toggle_path = path.clone();
        let hov_path = path.clone();
        let name = self.title(&path);
        let status = self
            .view(&path)
            .map(|v| v.status.clone())
            .unwrap_or_default();
        let b = bucket_of(&status);
        let selected = self.selected.as_deref() == Some(path.as_str());
        let hovered = self.hovered.as_deref() == Some(path.as_str());

        let mut row = h_flex()
            .id(SharedString::from(format!("repo-{path}")))
            .w_full()
            .h(px(38.0))
            .px(px(10.0))
            .rounded(px(R_LG))
            .items_center()
            .gap(px(GAP))
            // 悬停给一层极淡的底色：整行可点，鼠标放上去必须看得出能点
            .when(hovered && !selected, |r| r.bg(t.colors.list_hover))
            .child(
                div()
                    .w(px(7.0))
                    .h(px(7.0))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(dot_color(b, t)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(FS_BODY))
                    .when(selected, |d| d.font_weight(FontWeight::MEDIUM))
                    .text_color(if selected {
                        t.colors.foreground
                    } else {
                        body(t)
                    })
                    .child(name),
            )
            .child(div().flex_1());
        if selected {
            row = row.bg(warm(t));
        }
        // 状态徽章：可更新 / 已分叉（暖橙），待推送（蓝灰）
        let badge: Option<(String, Hsla, Hsla)> = match b {
            Bucket::Behind => Some(("可更新".into(), orange(t), orange_bg(t))),
            Bucket::Forked => Some(("已分叉".into(), orange(t), orange_bg(t))),
            Bucket::Ahead => Some((
                format!("待推送 {}", status.ahead),
                t.colors.accent,
                blue_bg(t),
            )),
            _ => None,
        };
        if let Some((text, fg, bg)) = badge {
            let text: String = text.into();
            row = row.child(
                h_flex()
                    .h(px(18.0))
                    .px(px(6.0))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(bg)
                    .items_center()
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .child(text),
                    ),
            );
        }
        row.on_click(cx.listener(move |this, _, _, cx| this.select(path.clone(), cx)))
            // 双击开「仓库设置」：改名 / 改归属 / 移除
            .on_double_click(cx.listener(move |this, _, window, cx| {
                this.open_edit_dlg(toggle_path.clone(), window, cx)
            }))
            .on_hover({
                let this = cx.entity();
                move |hovered, _, cx| {
                    this.update(cx, |this, cx| {
                        if *hovered {
                            this.hovered = Some(hov_path.clone());
                        } else if this.hovered.as_deref() == Some(hov_path.as_str()) {
                            this.hovered = None;
                        }
                        cx.notify();
                    });
                }
            })
    }
}

fn group_header(text: &str, t: &Theme) -> Div {
    h_flex().w_full().px(px(2.0)).pt(px(4.0)).child(
        div()
            .text_size(px(FS_META))
            .font_weight(FontWeight::MEDIUM)
            .text_color(tier4(t))
            .child(text.to_string()),
    )
}

// ================= 主区路由 =================

fn empty_main(t: &Theme) -> AnyElement {
    v_flex()
        .flex_1()
        .min_h_0()
        .h_full()
        .items_center()
        .justify_center()
        .bg(t.colors.background)
        .child(
            div()
                .text_size(px(FS_BODY))
                .text_color(tier3(t))
                .child("选一个仓库"),
        )
        .into_any_element()
}

// ================= Screen 2：总览批量同步 =================

impl Shell {
    /// 总览页：标题统计 + 四张状态卡 + 分组表格 + 底部批量条。
    /// 数据全部来自已有 views 状态缓存（侧栏扫描的那份），不另扫一遍。
    fn overview_page(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        // 行数据：归属分组 + 桶判定（口径与侧栏、ForkInfo 一致）
        let rows: Vec<(&RepoEntry, &RepoView)> = self
            .repos
            .iter()
            .map(|r| (r, self.views.get(&r.path).unwrap_or(&EMPTY_VIEW)))
            .collect();
        let mine: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, (e, _))| !e.is_third_party)
            .map(|(i, _)| i)
            .collect();
        let third: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, (e, _))| e.is_third_party)
            .map(|(i, _)| i)
            .collect();

        // 四张统计卡：最新 / 落后云端 / 已分叉 / 未提交（未提交是独立维度，
        // 与其余三档可重叠——设计稿如此，别强行互斥）
        let count = |f: &dyn Fn(&RepoView) -> bool| rows.iter().filter(|(_, v)| f(v)).count();
        let synced_n = count(&|v| bucket_of(&v.status) == Bucket::Synced);
        let behind_n = count(&|v| bucket_of(&v.status) == Bucket::Behind);
        let fork_n = count(&|v| bucket_of(&v.status) == Bucket::Forked);
        let dirty_n = count(&|v| v.status.dirty);

        // 批量队列：只收同类动作的行（分叉是「查看」，不收——
        // 备份变基会改写历史，只能进那个仓库的向导里点）
        let todo_paths = |pull: bool| -> Vec<String> {
            rows.iter()
                .filter(|(_, v)| {
                    let b = bucket_of(&v.status);
                    if pull {
                        b == Bucket::Behind
                    } else {
                        b == Bucket::Ahead
                    }
                })
                .map(|(e, _)| e.path.clone())
                .collect()
        };
        let pull_paths = todo_paths(true);
        let push_paths = todo_paths(false);
        let batch_running = self.batch.as_ref().map(|b| b.running).unwrap_or(false);

        let head = h_flex()
            .w_full()
            .justify_between()
            .items_start()
            .child(
                v_flex()
                    .gap(px(GAP_TIGHT))
                    .child(
                        div()
                            .text_size(px(FS_HEAD))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.colors.foreground)
                            .child("总览"),
                    )
                    .child(
                        div()
                            .text_size(px(FS_SEC))
                            .text_color(tier3(t))
                            .child(format!(
                                "{} 个仓库 · 我的 {} · 第三方 {}",
                                rows.len(),
                                mine.len(),
                                third.len()
                            )),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(GAP))
                    .child(
                        Button::new("ov-pull-all")
                            .label(if pull_paths.is_empty() {
                                "全部拉取".to_string()
                            } else {
                                format!("全部拉取 {}", pull_paths.len())
                            })
                            .outline()
                            .disabled(pull_paths.is_empty() || batch_running)
                            .with_size(Size::Size(px(BTN_LG)))
                            .on_click(cx.listener({
                                let paths = pull_paths.clone();
                                move |this, _, _, cx| this.run_batch(true, paths.clone(), cx)
                            })),
                    )
                    .child(
                        Button::new("ov-push-all")
                            .label(if push_paths.is_empty() {
                                "全部推送".to_string()
                            } else {
                                format!("全部推送 {}", push_paths.len())
                            })
                            .primary()
                            .disabled(push_paths.is_empty() || batch_running)
                            .with_size(Size::Size(px(BTN_LG)))
                            .on_click(cx.listener({
                                let paths = push_paths.clone();
                                move |this, _, _, cx| this.run_batch(false, paths.clone(), cx)
                            })),
                    ),
            );

        let cards = h_flex()
            .w_full()
            .gap(px(GAP))
            .child(stat_card(synced_n, "最新", t.colors.success, t))
            .child(stat_card(behind_n, "落后云端", orange(t), t))
            .child(stat_card(fork_n, "已分叉", orange(t), t))
            .child(stat_card(dirty_n, "未提交", t.colors.accent, t));

        // 表格头
        let col_branch = px(90.0);
        let col_status = px(96.0);
        let col_counts = px(90.0);
        let col_synced = px(90.0);
        let col_action = px(120.0);
        let head_row = |name: &str| -> Div {
            h_flex()
                .w_full()
                .items_center()
                .px(px(12.0))
                .h(px(32.0))
                .gap(px(GAP))
                .child(div().w(px(24.0))) // 复选框占位
                .child(
                    div()
                        .flex_1()
                        .text_size(px(FS_META))
                        .text_color(tier4(t))
                        .child(name.to_string()),
                )
                .child(div().w(col_branch).text_size(px(FS_META)).text_color(tier4(t)).child("分支"))
                .child(div().w(col_status).text_size(px(FS_META)).text_color(tier4(t)).child("状态"))
                .child(div().w(col_counts).text_size(px(FS_META)).text_color(tier4(t)).child("领先 · 落后"))
                .child(div().w(col_synced).text_size(px(FS_META)).text_color(tier4(t)).child("上次同步"))
                .child(div().w(col_action).text_size(px(FS_META)).text_color(tier4(t)).child("操作"))
        };

        // 返回 Stateful<Div>：行要能响应悬停（on_hover 只在 Stateful 上）
        let row =
            |(i, (e, v)): (usize, (&RepoEntry, &RepoView)), cx: &mut Context<Self>| -> Stateful<Div> {
            let path = e.path.clone();
            let hov_path = path.clone();
            let b = bucket_of(&v.status);
            let name = if e.alias.trim().is_empty() {
                e.name.clone()
            } else {
                e.alias.trim().to_string()
            };
            let checked = self.ov_checked.contains(&path);
            let pill_view = row_pill(v, e.is_third_party, t);

            // 行尾动作：拉取 / 推送 / 查看按桶给；跑批量中禁用
            let action: Option<Button> = match b {
                Bucket::Behind => Some(
                    Button::new(SharedString::from(format!("ov-act-pull-{i}")))
                        .label("拉取")
                        .outline()
                        .disabled(batch_running)
                        .with_size(Size::Size(px(BTN_SM)))
                        .on_click(cx.listener({
                            let path = path.clone();
                            move |this, _, _, cx| {
                                // 切到该仓库再跑：日志落进它的输出块（与旧版 pullFor 同语义）
                                this.select(path.clone(), cx);
                                this.run_sync(Op::Pull, cx);
                            }
                        })),
                ),
                Bucket::Ahead => Some(
                    Button::new(SharedString::from(format!("ov-act-push-{i}")))
                        .label("推送")
                        .outline()
                        .disabled(batch_running)
                        .with_size(Size::Size(px(BTN_SM)))
                        .on_click(cx.listener({
                            let path = path.clone();
                            move |this, _, _, cx| {
                                this.select(path.clone(), cx);
                                this.run_sync(Op::Push, cx);
                            }
                        })),
                ),
                Bucket::Forked | Bucket::Unknown => Some(
                    Button::new(SharedString::from(format!("ov-act-view-{i}")))
                        .label("查看")
                        .ghost()
                        .with_size(Size::Size(px(BTN_SM)))
                        .on_click(cx.listener({
                            let path = path.clone();
                            move |this, _, _, cx| {
                                this.select(path.clone(), cx);
                                this.page = Page::Repos;
                                cx.notify();
                            }
                        })),
                ),
                _ => None,
            };

            h_flex()
                .w_full()
                .items_center()
                .px(px(12.0))
                .h(px(36.0))
                .gap(px(GAP))
                .child(
                    div().w(px(24.0)).flex_shrink_0().child(
                        Checkbox::new(SharedString::from(format!("ov-cb-{i}")))
                            .checked(checked)
                            .on_click({
                                let path = path.clone();
                                let this = cx.entity();
                                move |checked, _, app| {
                                    this.update(app, |this, cx| {
                                        if *checked {
                                            this.ov_checked.insert(path.clone());
                                        } else {
                                            this.ov_checked.remove(&path);
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(FS_BODY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.colors.foreground)
                        .child(name),
                )
                .child(
                    div()
                        .w(col_branch)
                        .text_size(px(FS_SEC))
                        .font_family(t.mono_font_family.clone())
                        .text_color(tier3(t))
                        .child(if v.status.branch.is_empty() {
                            "—".to_string()
                        } else {
                            v.status.branch.clone()
                        }),
                )
                .child(div().w(col_status).child(pill_view))
                .child(
                    div()
                        .w(col_counts)
                        .text_size(px(FS_SEC))
                        .font_family(t.mono_font_family.clone())
                        .text_color(if v.status.ahead > 0 || v.status.behind > 0 {
                            body(t)
                        } else {
                            tier4(t)
                        })
                        .child(format!("{} · {}", v.status.ahead, v.status.behind)),
                )
                .child(
                    div()
                        .w(col_synced)
                        .text_size(px(FS_SEC))
                        .text_color(tier3(t))
                        .child({
                            let ts = e.last_auto_check;
                            if ts == 0 { "—".to_string() } else { rel_time(ts) }
                        }),
                )
                .child(
                    div().w(col_action).child(match action {
                        Some(btn) => btn.into_any_element(),
                        None => div().into_any_element(),
                    }),
                )
                .id(SharedString::from(format!("ov-row-{i}")))
                .when(self.hovered.as_deref() == Some(path.as_str()), |r| {
                    r.bg(t.colors.list_hover)
                })
                .on_hover({
                    let this = cx.entity();
                    move |hovered, _, cx| {
                        this.update(cx, |this, cx| {
                            if *hovered {
                                this.hovered = Some(hov_path.clone());
                            } else if this.hovered.as_deref() == Some(hov_path.as_str()) {
                                this.hovered = None;
                            }
                            cx.notify();
                        });
                    }
                })
        };

        // 分组表格：组头 + 行；空组不画
        let mut table = v_flex()
            .w_full()
            .flex_1()
            .min_h_0()
            .id("ov-table")
            .overflow_y_scroll()
            .rounded(px(R_LG))
            .bg(t.colors.popover)
            .border_1()
            .border_color(t.colors.border)
            .child(head_row("仓库名"));
        if !mine.is_empty() {
            table = table.child(group_header("我的仓库 · 重点关注", t));
            for (i, idx) in mine.iter().enumerate() {
                table = table
                    .child(row((*idx, rows[*idx]), cx).when(i % 2 == 1, |r| r.bg(t.colors.list_even)));
            }
        }
        if !third.is_empty() {
            table = table.child(group_header("第三方仓库 · 跟踪更新", t));
            for (i, idx) in third.iter().enumerate() {
                table = table
                    .child(row((*idx, rows[*idx]), cx).when(i % 2 == 1, |r| r.bg(t.colors.list_even)));
            }
        }
        if rows.is_empty() {
            table = table.child(
                div()
                    .p(px(32.0))
                    .text_size(px(FS_BODY))
                    .text_color(tier3(t))
                    .child("还没有仓库。回到工作台添加一个吧。"),
            );
        }

        // 底部批量条：勾选行按各自动作收束成一批；混选时逐个按行内动作跑
        let checked_paths: Vec<String> = rows
            .iter()
            .filter(|(e, _)| self.ov_checked.contains(&e.path))
            .map(|(e, _)| e.path.clone())
            .collect();
        let checked_pull: Vec<String> = checked_paths
            .iter()
            .filter(|p| {
                rows.iter().any(|(e, v)| {
                    &e.path == *p && bucket_of(&v.status) == Bucket::Behind
                })
            })
            .cloned()
            .collect();
        let checked_push: Vec<String> = checked_paths
            .iter()
            .filter(|p| {
                rows.iter().any(|(e, v)| {
                    &e.path == *p && bucket_of(&v.status) == Bucket::Ahead
                })
            })
            .cloned()
            .collect();
        let mut bottom_bar = h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .child(
                div().text_size(px(FS_SEC)).text_color(tier3(t)).child(
                    match &self.batch {
                        Some(b) if b.running => format!(
                            "批量{}中 {}/{}…",
                            if b.pull { "拉取" } else { "推送" },
                            b.done,
                            b.paths.len()
                        ),
                        Some(b) if b.note.is_some() => b.note.clone().unwrap(),
                        _ => format!("勾选仓库后可批量同步；当前 {} 个待拉取、{} 个待推送", pull_paths.len(), push_paths.len()),
                    },
                ),
            );
        if !checked_paths.is_empty() {
            bottom_bar = bottom_bar.child(
                h_flex()
                    .gap(px(GAP))
                    .when(!checked_pull.is_empty(), |bar| {
                        bar.child(
                            Button::new("ov-batch-checked-pull")
                                .label(format!("拉取所选 ({})", checked_pull.len()))
                                .primary()
                                .disabled(batch_running)
                                .with_size(Size::Size(px(BTN)))
                                .on_click(cx.listener({
                                    let paths = checked_pull.clone();
                                    move |this, _, _, cx| this.run_batch(true, paths.clone(), cx)
                                })),
                        )
                    })
                    .when(!checked_push.is_empty(), |bar| {
                        bar.child(
                            Button::new("ov-batch-checked-push")
                                .label(format!("推送所选 ({})", checked_push.len()))
                                .primary()
                                .disabled(batch_running)
                                .with_size(Size::Size(px(BTN)))
                                .on_click(cx.listener({
                                    let paths = checked_push.clone();
                                    move |this, _, _, cx| this.run_batch(false, paths.clone(), cx)
                                })),
                        )
                    }),
            );
        }

        v_flex()
            .flex_1()
            .min_h_0()
            .h_full()
            .bg(t.colors.background)
            .p(px(24.0))
            .gap(px(GAP_LG))
            .child(head)
            .child(cards)
            .child(table)
            .child(bottom_bar)
    }
}

/// 空视图：仓库还没有被扫到时（首帧、扫描中）给总览行兜底
static EMPTY_VIEW: RepoView = RepoView {
    status: RepoStatus {
        is_repo: false,
        branch: String::new(),
        ahead: 0,
        behind: 0,
        dirty: false,
        has_upstream: false,
        remote_url: String::new(),
    },
    changed: 0,
    detected: run_cmd::CmdDetect {
        test_cmd: String::new(),
        build_cmd: String::new(),
        dev_cmd: String::new(),
        install_cmd: String::new(),
    },
};

/// 仓库弹窗的正文：路径 / 显示名 / 归属 + 错误提示。
/// 每帧重画（Dialog 的 content builder 会被反复调用），所以只接引用。
fn dlg_body(
    t: &Theme,
    editing: &Option<String>,
    third: bool,
    error: &Option<String>,
    path_input: &Entity<InputState>,
    name_input: &Entity<InputState>,
    this: &Entity<Shell>,
) -> Div {
    let field = |label: &str, input: &Entity<InputState>| -> Div {
        v_flex()
            .w_full()
            .gap(px(GAP_TIGHT))
            .child(
                div()
                    .text_size(px(FS_META))
                    .text_color(tier4(t))
                    .child(label.to_string()),
            )
            .child(Input::new(input).w_full())
    };
    let path_field = match editing {
        // 编辑时路径只读：换路径等于换仓库，不该在这个框里发生
        Some(p) => v_flex()
            .w_full()
            .gap(px(GAP_TIGHT))
            .child(
                div()
                    .text_size(px(FS_META))
                    .text_color(tier4(t))
                    .child("路径"),
            )
            .child(
                div()
                    .w_full()
                    .text_size(px(FS_SEC))
                    .font_family(t.mono_font_family.clone())
                    .text_color(tier3(t))
                    .child(p.clone()),
            ),
        None => field("路径", path_input),
    };

    v_flex()
        .w_full()
        .gap(px(GAP_LG))
        .child(path_field)
        .child(field("显示名", name_input))
        .child(
            v_flex()
                .w_full()
                .gap(px(GAP_TIGHT))
                .child(
                    div()
                        .text_size(px(FS_META))
                        .text_color(tier4(t))
                        .child("归属"),
                )
                .child(
                    h_flex()
                        .gap(px(GAP))
                        .child(
                            Button::new("dlg-mine")
                                .label("我的仓库")
                                .toggled(!third)
                                .on_click({
                                    let this = this.clone();
                                    move |_, _, cx| {
                                        this.update(cx, |this, cx| {
                                            if let Some(d) = this.dlg.as_mut() {
                                                d.third = false;
                                            }
                                            cx.notify();
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("dlg-third")
                                .label("第三方仓库")
                                .toggled(third)
                                .on_click({
                                    let this = this.clone();
                                    move |_, _, cx| {
                                        this.update(cx, |this, cx| {
                                            if let Some(d) = this.dlg.as_mut() {
                                                d.third = true;
                                            }
                                            cx.notify();
                                        });
                                    }
                                }),
                        ),
                ),
        )
        .when_some(error.clone(), |b, e| {
            b.child(
                div()
                    .w_full()
                    .text_size(px(FS_SEC))
                    .text_color(t.colors.danger)
                    .child(e),
            )
        })
}

/// 初始化向导正文：说明 + 路径 + 排除项勾选列表。
/// 常见构建产物 / 依赖目录默认勾上（进 `enter_init` 时预勾），用户可改。
fn init_body(
    t: &Theme,
    init: &InitState,
    error: &Option<String>,
    this: &Entity<Shell>,
) -> Div {
    let dir_name = std::path::Path::new(&init.path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| init.path.clone());

    let mut list = v_flex()
        .id("init-excludes")
        .w_full()
        .max_h(px(180.0))
        .overflow_y_scroll()
        .gap(px(2.0))
        .p(px(8.0))
        .rounded(px(R_LG))
        .bg(t.colors.background)
        .border_1()
        .border_color(t.colors.border);
    if init.items.is_empty() {
        list = list.child(
            div()
                .text_size(px(FS_SEC))
                .text_color(tier4(t))
                .child("（空文件夹）"),
        );
    }
    for (ix, it) in init.items.iter().enumerate() {
        let name = it.name.clone();
        let checked = init.excludes.contains(&name);
        list = list.child(
            h_flex()
                .w_full()
                .h(px(26.0))
                .px(px(6.0))
                .items_center()
                .gap(px(GAP))
                .child(
                    Checkbox::new(SharedString::from(format!("init-cb-{ix}")))
                        .checked(checked)
                        .on_click({
                            let this = this.clone();
                            let name = name.clone();
                            move |checked, _, cx| {
                                this.update(cx, |this, cx| {
                                    if let Some(i) = this.dlg.as_mut().and_then(|d| d.init.as_mut())
                                    {
                                        if *checked {
                                            i.excludes.insert(name.clone());
                                        } else {
                                            i.excludes.remove(&name);
                                        }
                                    }
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(
                    div()
                        .text_size(px(FS_SEC))
                        .text_color(body(t))
                        .child(name.clone()),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(px(FS_META))
                        .text_color(tier4(t))
                        .child(if it.is_dir { "文件夹" } else { "文件" }.to_string()),
                ),
        );
    }

    v_flex()
        .w_full()
        .gap(px(GAP_LG))
        .child(
            div()
                .w_full()
                .text_size(px(FS_BODY))
                .line_height(px(20.0))
                .text_color(tier3(t))
                .child(format!(
                    "「{dir_name}」还不是 git 仓库。初始化后自动生成首次提交；勾选不想纳入版本管理的项，会写入 .gitignore："
                )),
        )
        .child(
            div()
                .w_full()
                .text_size(px(FS_SEC))
                .font_family(t.mono_font_family.clone())
                .text_color(tier4(t))
                .child(init.path.clone()),
        )
        .child(list)
        .when_some(error.clone(), |b, e| {
            b.child(
                div()
                    .w_full()
                    .text_size(px(FS_SEC))
                    .text_color(t.colors.danger)
                    .child(e),
            )
        })
}

/// 初始化向导底栏：取消 / 初始化（跑起来时按钮转圈）
fn init_footer(t: &Theme, init: &InitState, this: &Entity<Shell>) -> Div {
    let _ = t;
    h_flex()
        .w_full()
        .justify_end()
        .items_center()
        .gap(px(GAP))
        .child(
            Button::new("init-cancel")
                .label("取消")
                .ghost()
                .disabled(init.working)
                .on_click({
                    let this = this.clone();
                    move |_, window, cx| {
                        this.update(cx, |this, cx| {
                            this.dlg = None;
                            cx.notify();
                        });
                        window.close_dialog(cx);
                    }
                }),
        )
        .child(
            Button::new("init-go")
                .label("初始化")
                .primary()
                .loading(init.working)
                .on_click({
                    let this = this.clone();
                    move |_, _, cx| {
                        this.update(cx, |this, cx| this.run_init(cx));
                    }
                }),
        )
}

/// 弹窗底栏：编辑模式左侧「移除仓库」，右侧取消 / 保存。
fn dlg_footer(
    this: &Entity<Shell>,
    editing: &Option<String>,
    path_input: &Entity<InputState>,
    name_input: &Entity<InputState>,
) -> Div {
    let mut footer = h_flex().w_full().justify_between().items_center();
    match editing {
        Some(p) => {
            footer = footer.child(
                Button::new("dlg-remove")
                    .label("移除仓库")
                    .outline()
                    .on_click({
                        let this = this.clone();
                        let p = p.clone();
                        move |_, window, cx| {
                            this.update(cx, |this, cx| this.remove_repo_at(p.clone(), cx));
                            window.close_dialog(cx);
                        }
                    }),
            );
        }
        None => footer = footer.child(div()),
    }
    footer.child(
        h_flex()
            .gap(px(GAP))
            .child(
                Button::new("dlg-cancel")
                    .label("取消")
                    .ghost()
                    .on_click({
                        let this = this.clone();
                        move |_, window, cx| {
                            this.update(cx, |this, cx| {
                                this.dlg = None;
                                cx.notify();
                            });
                            window.close_dialog(cx);
                        }
                    }),
            )
            .child(
                Button::new("dlg-save")
                    .label(if editing.is_some() { "保存" } else { "添加" })
                    .primary()
                    .on_click({
                        let this = this.clone();
                        let editing = editing.clone();
                        let path_input = path_input.clone();
                        let name_input = name_input.clone();
                        move |_, window, cx| {
                            // 值要在 App 上读成 String 再交给 Shell 落盘
                            let raw_path = path_input.read(cx).value().to_string();
                            let raw_name = name_input.read(cx).value().to_string();
                            let ok = this.update(cx, |this, cx| {
                                this.commit_dlg(editing.clone(), raw_path, raw_name, cx)
                            });
                            if ok {
                                window.close_dialog(cx);
                            }
                        }
                    }),
            ),
    )
}

/// 统计卡：大数字 + 标签，数字按语义着色
fn stat_card(n: usize, label: &str, color: Hsla, t: &Theme) -> Div {
    v_flex()
        .flex_1()
        .gap(px(2.0))
        .p(px(14.0))
        .rounded(px(R_LG))
        .bg(t.colors.popover)
        .border_1()
        .border_color(t.colors.border)
        .child(
            div()
                .text_size(px(FS_NUM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if n > 0 { color } else { tier4(t) }),
        )
        .child(div().text_size(px(FS_SEC)).text_color(tier3(t)).child(label.to_string()))
}

/// 总览行状态 pill：分叉 > 落后/可更新 > 未提交 > 待推送 > 最新。
/// 「落后 / 可更新」按归属分文案（我的：落后；第三方：可更新），语义同侧栏徽标。
fn row_pill(v: &RepoView, third: bool, t: &Theme) -> Div {
    let s = &v.status;
    let b = bucket_of(s);
    let (text, fg, bg) = if b == Bucket::Forked {
        ("已分叉", orange(t), orange_bg(t))
    } else if b == Bucket::Behind {
        (if third { "可更新" } else { "落后" }, orange(t), orange_bg(t))
    } else if s.dirty {
        ("未提交", t.colors.accent, blue_bg(t))
    } else if b == Bucket::Ahead {
        ("待推送", t.colors.accent, blue_bg(t))
    } else if b == Bucket::Unknown {
        ("待扫描", tier3(t), track(t))
    } else {
        ("最新", t.colors.success, track(t))
    };
    pill(text, fg, bg, t)
}

// ================= Screen 3：我的仓库工作台 =================

impl Shell {
    /// 我的工作台：头 / 常用命令卡 / 底排（磁盘 + Git）/ 状态条（侧栏常驻在壳层）
    fn workbench_page(&self, path: &str, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.view(path);
        let status = view.map(|v| v.status.clone()).unwrap_or_default();
        let changed = view.map(|v| v.changed).unwrap_or(0);
        let b = bucket_of(&status);

        let title_row = h_flex()
            .items_center()
            .gap(px(GAP))
            .child(
                div()
                    .text_size(px(FS_HEAD))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.colors.foreground)
                    .child(self.title(path)),
            )
            .child(self.branch_badge(&status.branch, t))
            .when(changed > 0, |row| {
                row.child(pill(
                    &format!("未提交 {changed} 文件"),
                    t.colors.accent,
                    blue_bg(t),
                    t,
                ))
            })
            .when(
                b == Bucket::Forked || b == Bucket::Behind,
                |row| {
                    row.child(pill(
                        if b == Bucket::Forked { "已分叉" } else { "可更新" },
                        orange(t),
                        orange_bg(t),
                        t,
                    ))
                },
            )
            .when(b == Bucket::Ahead, |row| {
                row.child(pill(
                    &format!("待推送 {}", status.ahead),
                    t.colors.accent,
                    blue_bg(t),
                    t,
                ))
            });

        let head = h_flex()
            .w_full()
            .justify_between()
            .items_start()
            .gap(px(GAP_LG))
            .child(
                v_flex()
                    .gap(px(GAP_TIGHT))
                    .child(title_row)
                    .child(
                        div()
                            .text_size(px(FS_SEC))
                            .text_color(tier3(t))
                            .child(format!("我的仓库 · 本地路径 {path}")),
                    ),
            )
            .child(self.head_actions(t, cx));

        // 常用命令卡
        let slots: Vec<Slot> = {
            let mut v = vec![Slot::Test, Slot::Dev, Slot::Build, Slot::Install];
            if let Some(e) = self.entry(path) {
                for i in 0..e.pinned_cmds.len().min(MAX_PINNED_CMDS) {
                    v.push(Slot::Pinned(i));
                }
            }
            v
        };
        let rows: Vec<AnyElement> = slots
            .into_iter()
            .map(|slot| {
                let label = match slot {
                    Slot::Pinned(i) => self
                        .entry(path)
                        .and_then(|e| e.pinned_cmds.get(i))
                        .map(|c| {
                            if c.label.trim().is_empty() {
                                run_cmd::derive_pinned_label(&c.cmd)
                            } else {
                                c.label.trim().to_string()
                            }
                        })
                        .unwrap_or_else(|| format!("命令 {}", i + 1)),
                    _ => slot.default_label().to_string(),
                };
                self.cmd_row(path, slot, label, t, cx).into_any_element()
            })
            .collect();

        let cmd_card = v_flex()
            .w_full()
            // 设计稿：命令卡与底排分摊主区剩余高度（fill_container），不是按内容收缩
            .flex_1()
            .min_h_0()
            .gap(px(GAP_TIGHT))
            .p(px(16.0))
            .rounded(px(R_XL))
            .bg(t.colors.popover)
            .border_1()
            .border_color(t.colors.border)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap(px(GAP))
                    .child(
                        div()
                            .text_size(px(FS_BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.colors.foreground)
                            .child("常用命令"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(FS_META))
                            .text_color(tier4(t))
                            .child("一键运行 · 输出实时回显 · 可钉住自定义命令（上限 3）"),
                    )
                    .child(
                        Button::new("cmd-edit")
                            .label("编辑")
                            .ghost()
                            .with_size(Size::Size(px(BTN_SM))),
                    ),
            )
            .children(rows)
            .child(self.output_block(t));
        // 底排：磁盘占用卡 + Git 附带卡
        let bottom = h_flex()
            .w_full()
            .flex_1()
            .min_h_0()
            .gap(px(GAP_LG))
            .child(self.space_card(t, cx))
            .child(self.git_card(path, &status, changed, t));

        v_flex()
            .flex_1()
            .min_h_0()
            .h_full()
            .bg(t.colors.background)
            .p(px(24.0))
            .gap(px(GAP_LG))
            .child(head)
            .child(cmd_card)
            .child(bottom)
            .child(self.status_bar(t, path))
    }

    /// 头部右侧：拉取 / 推送（弱）+ 分组线 + 四个图标动作
    fn head_actions(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let pulling = self.out.running && self.out.op == Some(Op::Pull);
        let pushing = self.out.running && self.out.op == Some(Op::Push);
        h_flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(GAP))
            .child(
                Button::new("wb-pull")
                    .label("拉取")
                    .icon(AssetIcon::ArrowDown)
                    .outline()
                    .loading(pulling)
                    .with_size(Size::Size(px(BTN)))
                    .on_click(cx.listener(|this, _, _, cx| this.run_sync(Op::Pull, cx))),
            )
            .child(
                Button::new("wb-push")
                    .label("推送")
                    .icon(AssetIcon::ArrowUp)
                    .outline()
                    .loading(pushing)
                    .with_size(Size::Size(px(BTN)))
                    .on_click(cx.listener(|this, _, _, cx| this.run_sync(Op::Push, cx))),
            )
            .child(div().w(px(1.0)).h(px(20.0)).bg(t.colors.border))
            .child(
                Button::new("act-folder")
                    .icon(AssetIcon::FolderOpen)
                    .ghost()
                    .with_size(Size::Size(px(BTN))),
            )
            .child(
                Button::new("act-cloud")
                    .icon(AssetIcon::Globe)
                    .ghost()
                    .with_size(Size::Size(px(BTN))),
            )
            .child(
                Button::new("act-doc")
                    .icon(AssetIcon::BookOpen)
                    .ghost()
                    .with_size(Size::Size(px(BTN))),
            )
            .child(
                Button::new("act-timer")
                    .icon(AssetIcon::Clock)
                    .ghost()
                    .with_size(Size::Size(px(BTN))),
            )
    }

    /// 分支徽章：Surface 底 + 分支图标
    fn branch_badge(&self, branch: &str, t: &Theme) -> Div {
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

    /// 命令卡里的一行：圆形播放钮 + 槽位名 + 命令文本 + 上次结果
    fn cmd_row(
        &self,
        path: &str,
        slot: Slot,
        label: String,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let cmd = self.effective_cmd(path, slot);
        let rec = self
            .entry(path)
            .and_then(|e| e.cmd_runs.get(&slot.key()))
            .cloned();
        let running = self.out.running && self.out.op == Some(Op::Cmd(slot));

        let mut row = h_flex()
            .id(SharedString::from(format!("cmd-row-{}", slot.key())))
            .w_full()
            .h(px(40.0))
            .px(px(10.0))
            .rounded(px(R_MD))
            .items_center()
            .gap(px(GAP_LG))
            .child(
                // 22px 墨色圆 + 白色播放三角；运行中换成加载图标
                div()
                    .w(px(22.0))
                    .h(px(22.0))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(t.colors.primary)
                    .items_center()
                    .justify_center()
                    .text_color(t.colors.popover)
                    .child(if running {
                        icon_sm(AssetIcon::LoaderCircle)
                    } else {
                        icon_sm(AssetIcon::Play)
                    }),
            )
            .child(
                div()
                    .w(px(76.0))
                    .flex_shrink_0()
                    .text_size(px(FS_BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(body(t))
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(FS_SEC))
                    .font_family(t.mono_font_family.clone())
                    .text_color(tier3(t))
                    .child(if cmd.is_empty() {
                        "未识别到命令".to_string()
                    } else {
                        cmd
                    }),
            );
        if running {
            row = row.bg(t.colors.muted).child(pill(
                &format!("运行中 · 已输出 {} 行", self.out.lines.len()),
                t.colors.accent,
                blue_bg(t),
                t,
            ));
        } else if let Some(r) = &rec {
            let when = rel_time(r.at);
            row = row.child(
                div()
                    .flex_shrink_0()
                    .text_size(px(FS_META))
                    .text_color(if r.ok { tier4(t) } else { t.colors.danger })
                    .child(format!(
                        "{}{}{}",
                        if r.ok { "成功" } else { "失败" },
                        if when.is_empty() { "" } else { " · " },
                        when
                    )),
            );
        }
        row.on_click(cx.listener(move |this, _, _, cx| this.run_slot(slot, cx)))
    }

    /// 深色输出块：墨底终端，跟白卡片形成对比
    fn output_block(&self, t: &Theme) -> impl IntoElement {
        let ink = t.colors.primary;
        let text = t.colors.background;
        let dim = tier4(t);

        let mut block = v_flex()
            .id("out-block")
            .w_full()
            // 卡片撑高后，输出块吃掉命令行以下的空间；内容多时块内滚动
            .flex_1()
            .min_h(px(96.0))
            .overflow_y_scroll()
            .p(px(10.0))
            .gap(px(GAP_TIGHT))
            .rounded(px(R_LG))
            .bg(ink)
            .font_family(t.mono_font_family.clone())
            .text_size(px(FS_META))
            .text_color(text);

        if self.out.lines.is_empty() {
            block = block.child(
                div()
                    .text_color(dim)
                    .child("点任一行的圆形按钮，输出会实时回显在这里"),
            );
        } else {
            let lines: Vec<Div> = self
                .out
                .lines
                .iter()
                .map(|l| div().text_color(text).child(l.clone()))
                .collect();
            block = block.children(lines);
        }
        if let Some(e) = &self.out.error {
            block = block.child(div().text_color(t.colors.danger).child(e.clone()));
        }
        block
    }

    /// 磁盘占用卡：头 / 构成条 / 图例 / 清理条
    fn space_card(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let rep = self.space.report.clone();

        let total_text = if self.space.scanning {
            "扫描中…".to_string()
        } else if let Some(e) = &self.space.error {
            e.clone()
        } else {
            match &rep {
                Some(r) => format!("共 {}", space::human_size(r.total)),
                None => "尚未扫描".to_string(),
            }
        };

        // 构成段：按大小取前 3 个一级条目，其余并成「其他」（设计稿三行图例）
        let palette = [
            t.colors.accent,
            orange(t),
            edge(t),
            t.colors.chart_2,
            t.colors.chart_5,
            tier4(t),
        ];
        let mut segs: Vec<(Hsla, u64, String, String)> = Vec::new();
        if let Some(r) = &rep {
            for (i, d) in r.dirs.iter().take(3).enumerate() {
                if d.size == 0 {
                    continue;
                }
                let remark = if d.is_cache {
                    " · 构建缓存"
                } else if d.name == ".git" {
                    " · 版本库"
                } else {
                    ""
                };
                segs.push((
                    palette[i],
                    d.size,
                    d.name.clone(),
                    format!("{} — {}{}", d.name, space::human_size(d.size), remark),
                ));
            }
            let rest: u64 = r.dirs.iter().skip(3).map(|d| d.size).sum();
            if rest > 0 {
                segs.push((
                    track(t),
                    rest,
                    "其他".into(),
                    format!("其他 {} 项 — {}", r.dirs.len().saturating_sub(3), space::human_size(rest)),
                ));
            }
        }

        let cache_total = rep
            .as_ref()
            .map(|r| r.dirs.iter().filter(|d| d.is_cache).map(|d| d.size).sum())
            .unwrap_or(0u64);
        let cache_names = rep
            .as_ref()
            .map(|r| {
                r.dirs
                    .iter()
                    .filter(|d| d.is_cache && d.size > 0)
                    .map(|d| d.name.clone())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_default();

        let mut card = v_flex()
            .flex_1()
            .min_h_0()
            .gap(px(GAP))
            .p(px(16.0))
            .rounded(px(R_XL))
            .bg(t.colors.popover)
            .border_1()
            .border_color(t.colors.border)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap(px(GAP))
                    .child(
                        div()
                            .text_size(px(FS_BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.colors.foreground)
                            .child("磁盘占用"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(FS_SEC))
                            .text_color(tier3(t))
                            .child(total_text),
                    )
                    .child(
                        Button::new("space-rescan")
                            .label(if self.space.scanning { "扫描中…" } else { "重新扫描" })
                            .ghost()
                            .with_size(Size::Size(px(BTN_SM)))
                            .loading(self.space.scanning)
                            .on_click(cx.listener(|this, _, _, cx| this.scan_space(cx))),
                    ),
            );

        // 构成条 + 图例
        if !segs.is_empty() {
            let mut bar = h_flex()
                .w_full()
                .h(px(8.0))
                .rounded(px(R_SM))
                .overflow_hidden()
                .gap(px(2.0))
                .bg(track(t));
            for (color, size, _, _) in &segs {
                bar = bar.child(div().h_full().rounded(px(R_SM)).bg(*color).flex_grow(*size as f32));
            }
            card = card.child(bar);

            let mut legend = v_flex().w_full().gap(px(GAP_TIGHT));
            for (color, _, _, text) in &segs {
                legend = legend.child(
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
                                .bg(*color),
                        )
                        .child(
                            div()
                                .text_size(px(FS_SEC))
                                .text_color(body(t))
                                .child(text.clone()),
                        ),
                );
            }
            card = card.child(legend);
        } else {
            card = card.child(if self.space.scanning {
                v_flex()
                    .w_full()
                    .gap(px(GAP_TIGHT))
                    .child(skeleton_bar(None, t))
                    .child(skeleton_bar(Some(px(160.0)), t))
                    .into_any_element()
            } else {
                div()
                    .text_size(px(FS_SEC))
                    .text_color(tier3(t))
                    .child("扫描完成后在这里展示目录构成".to_string())
                    .into_any_element()
            });
        }

        // 清理条：暖橙底 + 深橙文字 + 墨色一键清理
        if cache_total > 0 {
            card = card.child(
                h_flex()
                    .w_full()
                    .p(px(10.0))
                    .rounded(px(R_LG))
                    .bg(orange_bg(t))
                    .items_center()
                    .gap(px(GAP))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(FS_SEC))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(orange_text(t))
                            .child(format!(
                                "构建缓存 {}/ 可清理约 {}",
                                cache_names,
                                space::human_size(cache_total)
                            )),
                    )
                    .child(
                        Button::new("clean-all")
                            .label(if self.space.cleaning { "清理中…" } else { "一键清理" })
                            .primary()
                            .with_size(Size::Size(px(BTN)))
                            .loading(self.space.cleaning)
                            .on_click(cx.listener(|this, _, _, cx| this.clean_all(cx))),
                    ),
            );
        } else if rep.is_some() && !self.space.scanning {
            card = card.child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap(px(GAP_TIGHT))
                    .text_color(t.colors.success)
                    .child(icon_sm(AssetIcon::CircleCheck))
                    .child(
                        div()
                            .text_size(px(FS_SEC))
                            .text_color(tier3(t))
                            .child("没有可清理的构建缓存"),
                    ),
            );
        }
        card
    }

    /// Git 附带卡（设计稿固定 300 宽）：同步概况 / 未提交 / 弱按钮 / 查看详情
    fn git_card(&self, path: &str, s: &RepoStatus, changed: u32, t: &Theme) -> Div {
        let b = bucket_of(s);
        let desc = match b {
            Bucket::Synced => "与云端基本一致",
            Bucket::Dirty => "与云端基本一致",
            Bucket::Ahead => "领先云端，可推送",
            Bucket::Behind => "云端有更新",
            Bucket::Forked => "本地与云端分叉",
            Bucket::Unknown => "不是 git 仓库",
        };
        v_flex()
            .w(px(300.0))
            .flex_shrink_0()
            .gap(px(GAP))
            .p(px(16.0))
            .rounded(px(R_XL))
            .bg(t.colors.popover)
            .border_1()
            .border_color(t.colors.border)
            .child(
                div()
                    .text_size(px(FS_SEC))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(tier3(t))
                    .child("Git · 附带功能"),
            )
            .child(
                h_flex()
                    .items_baseline()
                    .gap(px(GAP))
                    .child(
                        div()
                            .text_size(px(FS_TITLE))
                            .font_family(t.mono_font_family.clone())
                            .font_weight(FontWeight::BOLD)
                            .text_color(t.colors.foreground)
                            .child(format!("↑{} ↓{}", s.ahead, s.behind)),
                    )
                    .child(div().text_size(px(FS_META)).text_color(tier4(t)).child(desc)),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap(px(GAP))
                    .child(
                        div()
                            .w(px(7.0))
                            .h(px(7.0))
                            .rounded_full()
                            .bg(if changed > 0 {
                                t.colors.accent
                            } else {
                                t.colors.success
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(FS_SEC))
                            .text_color(body(t))
                            .child(if changed > 0 {
                                format!("{changed} 个文件待提交")
                            } else {
                                "工作区干净".to_string()
                            }),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap(px(GAP))
                    .child(
                        Button::new("git-pull")
                            .label("拉取")
                            .secondary()
                            .with_size(Size::Size(px(BTN))),
                    )
                    .child(
                        Button::new("git-push")
                            .label("推送")
                            .secondary()
                            .with_size(Size::Size(px(BTN))),
                    ),
            )
            .child(div().w_full().h(px(1.0)).bg(track(t)))
            .child(
                div()
                    .text_size(px(FS_SEC))
                    .text_color(t.colors.accent)
                    .child("查看提交历史与文件变更"),
            )
            .child(
                div()
                    .text_size(px(FS_META))
                    .text_color(tier4(t))
                    .child(format!(
                        "上次运行 {} · 定时 {}",
                        self.entry(path)
                            .and_then(|e| e.cmd_runs.values().max_by_key(|r| r.at))
                            .map(|r| rel_time(r.at))
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "—".into()),
                        self.entry(path)
                            .map(auto_sync_text)
                            .unwrap_or_else(|| "未开启".into())
                    )),
            )
    }

    /// 主区底部状态条：左侧运行状态点，右侧定时同步
    fn status_bar(&self, t: &Theme, path: &str) -> Div {
        let running = self.out.running;
        h_flex()
            .w_full()
            .h(px(26.0))
            .flex_shrink_0()
            .items_center()
            .gap(px(GAP_LG))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(GAP_TIGHT))
                    .child(
                        div()
                            .w(px(6.0))
                            .h(px(6.0))
                            .rounded_full()
                            .bg(if running { orange(t) } else { t.colors.success }),
                    )
                    .child(
                        div()
                            .text_size(px(FS_SEC))
                            .text_color(tier3(t))
                            .child(if running {
                                "命令运行中".to_string()
                            } else {
                                format!("就绪 · {} 个仓库", self.repos.len())
                            }),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(FS_SEC))
                    .text_color(tier4(t))
                    .child(format!(
                        "定时同步：{}",
                        self.entry(path)
                            .map(auto_sync_text)
                            .unwrap_or_else(|| "未开启".into())
                    )),
            )
    }
}

/// 圆角胶囊徽章：蓝灰「未提交 / 待推送 / 运行中」与暖橙「可更新」
fn pill(text: &str, fg: Hsla, bg: Hsla, _t: &Theme) -> Div {
    div()
        .h(px(20.0))
        .px(px(8.0))
        .rounded_full()
        .bg(bg)
        .items_center()
        .justify_center()
        .child(
            div()
                .text_size(px(FS_META))
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .child(text.to_string()),
        )
}

fn auto_sync_text(e: &RepoEntry) -> String {
    if !e.daily_time.is_empty() {
        format!("每天 {}", e.daily_time)
    } else if e.auto_sync_mins > 0 {
        format!("每 {} 分钟", e.auto_sync_mins)
    } else {
        "未开启".into()
    }
}

// ================= Screen 1：第三方仓库档案页 =================

impl Shell {
    /// 侧栏 + 主区：头 / 信息页签 / 内容卡（README + 同步面板）/ 状态条
    fn profile_page(&self, path: &str, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.view(path);
        let status = view.map(|v| v.status.clone()).unwrap_or_default();
        let changed = view.map(|v| v.changed).unwrap_or(0);
        let b = bucket_of(&status);

        let title_row = h_flex()
            .items_center()
            .gap(px(GAP))
            .child(
                div()
                    .text_size(px(FS_HEAD))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.colors.foreground)
                    .child(self.title(path)),
            )
            .child(self.branch_badge(&status.branch, t))
            .when(
                b == Bucket::Forked || b == Bucket::Behind,
                |row| {
                    row.child(pill(
                        if b == Bucket::Forked { "已分叉" } else { "可更新" },
                        orange(t),
                        orange_bg(t),
                        t,
                    ))
                },
            )
            .when(changed > 0, |row| {
                row.child(pill(
                    &format!("未提交 {changed} 文件"),
                    t.colors.accent,
                    blue_bg(t),
                    t,
                ))
            });

        let head = h_flex()
            .w_full()
            .justify_between()
            .items_start()
            .gap(px(GAP_LG))
            .child(
                v_flex()
                    .gap(px(GAP_TIGHT))
                    .child(title_row)
                    .child(
                        div()
                            .text_size(px(FS_BODY))
                            .text_color(tier3(t))
                            .child(format!("本地路径 {path}")),
                    ),
            )
            .child(
                // 右侧动作：四个图标 + 分组线 + 检测更新（次）+ 拉取更新（主）
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(GAP))
                    .child(
                        Button::new("pf-folder")
                            .icon(AssetIcon::FolderOpen)
                            .ghost()
                            .with_size(Size::Size(px(BTN))),
                    )
                    .child(
                        Button::new("pf-cloud")
                            .icon(AssetIcon::Globe)
                            .ghost()
                            .with_size(Size::Size(px(BTN))),
                    )
                    .child(
                        Button::new("pf-doc")
                            .icon(AssetIcon::BookOpen)
                            .ghost()
                            .with_size(Size::Size(px(BTN))),
                    )
                    .child(
                        Button::new("pf-timer")
                            .icon(AssetIcon::Clock)
                            .ghost()
                            .with_size(Size::Size(px(BTN))),
                    )
                    .child(div().w(px(1.0)).h(px(20.0)).bg(t.colors.border))
                    .child(
                        Button::new("pf-check-update")
                            .label("检测更新")
                            .outline()
                            .loading(self.out.running && self.out.op == Some(Op::Check))
                            .with_size(Size::Size(px(BTN_LG)))
                            .on_click(cx.listener(|this, _, _, cx| this.run_sync(Op::Check, cx))),
                    )
                    .child(
                        Button::new("pf-pull-update")
                            .label("拉取更新")
                            .primary()
                            .loading(self.out.running && self.out.op == Some(Op::Pull))
                            .with_size(Size::Size(px(BTN_LG)))
                            .on_click(cx.listener(|this, _, _, cx| this.run_sync(Op::Pull, cx))),
                    ),
            );

        // 信息页签：标签 + 值。技术栈 / 最近提交 / 本地来自本地扫描；
        // Stars 要远端 API（GitHub / Gitee），等克隆 / 新建仓库接入时一起做。
        let chip = |label: &str, value: &str| -> Div {
            h_flex()
                .h(px(30.0))
                .px(px(10.0))
                .rounded(px(R_MD))
                .bg(t.colors.popover)
                .border_1()
                .border_color(t.colors.border)
                .items_center()
                .gap(px(GAP_TIGHT))
                .child(
                    div()
                        .text_size(px(FS_META))
                        .text_color(tier4(t))
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(px(FS_SEC))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(body(t))
                        .child(value.to_string()),
                )
        };
        // 技术栈：语言占比前二，跟设计稿「TypeScript 63% · Vue 32%」一个格式
        let stack_text = if self.profile.scanning {
            "…".to_string()
        } else {
            match self.profile.langs.len() {
                0 => "—".to_string(),
                1 => format!("{} {:.0}%", self.profile.langs[0].language, self.profile.langs[0].percent),
                _ => self.profile.langs[..2]
                    .iter()
                    .map(|l| format!("{} {:.0}%", l.language, l.percent))
                    .collect::<Vec<_>>()
                    .join(" · "),
            }
        };
        let last_commit_text = if self.profile.last_commit_ts == 0 {
            "—".to_string()
        } else {
            rel_time(self.profile.last_commit_ts)
        };
        let local_size_text = match &self.space.report {
            Some(r) => space::human_size(r.total),
            None => "—".to_string(),
        };
        let chips = h_flex()
            .w_full()
            .gap(px(GAP))
            .child(chip("技术栈", &stack_text))
            .child(chip("Stars", "—"))
            .child(chip("最近提交", &last_commit_text))
            .child(chip("本地", &local_size_text))
            .child(
                h_flex()
                    .h(px(30.0))
                    .px(px(10.0))
                    .gap(px(GAP))
                    .rounded(px(R_MD))
                    .bg(t.colors.popover)
                    .border_1()
                    .border_color(t.colors.border)
                    .items_center()
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .text_color(tier4(t))
                            .child("近 30 天活跃度"),
                    )
                    .child(self.activity_chart(t)),
            );

        // 内容卡：README 预览 + 分隔 + 同步面板。
        // README 数据来自本地仓库根目录（标题 + 首段简介，core 提炼好）；
        // 没扫完 / 没有 README 时给骨架条占位。
        let name = self.title(path);
        let readme = self.profile.readme.clone();
        let readme_title = readme
            .as_ref()
            .map(|r| r.title.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| name.clone());
        let content_card = h_flex()
            .w_full()
            .flex_1()
            .min_h_0()
            .rounded(px(R_XL))
            .bg(t.colors.popover)
            .border_1()
            .border_color(t.colors.border)
            .overflow_hidden()
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .id("readme-pane")
                    .overflow_y_scroll()
                    .p(px(20.0))
                    .gap(px(GAP_LG))
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .font_family(t.mono_font_family.clone())
                            .text_color(tier4(t))
                            .child("README"),
                    )
                    .child(
                        div()
                            .text_size(px(FS_TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.colors.foreground)
                            .child(readme_title),
                    )
                    .children(match &readme {
                        Some(r) if !r.intro.is_empty() => vec![div()
                            .w_full()
                            .text_size(px(FS_BODY))
                            .line_height(px(22.0))
                            .text_color(tier3(t))
                            .child(r.intro.clone())
                            .into_any_element()],
                        Some(_) => vec![div()
                            .w_full()
                            .text_size(px(FS_BODY))
                            .line_height(px(22.0))
                            .text_color(tier3(t))
                            .child("这份仓库的 README 没有可展示的简介段落。")
                            .into_any_element()],
                        None => {
                            if self.profile.scanning {
                                vec![skeleton_bar(None, t), skeleton_bar(Some(px(420.0)), t)]
                            } else {
                                vec![div()
                                    .w_full()
                                    .text_size(px(FS_BODY))
                                    .line_height(px(22.0))
                                    .text_color(tier3(t))
                                    .child("仓库根目录没有找到 README 文件。")
                                    .into_any_element()]
                            }
                        }
                    })
                    .child(
                        div()
                            .pt(px(4.0))
                            .text_size(px(FS_META))
                            .text_color(tier4(t))
                            .child("语言构成"),
                    )
                    .child(self.lang_bar(t)),
            )
            .child(div().w(px(1.0)).h_full().flex_shrink_0().bg(t.colors.border))
            .child(self.sync_panel(t, cx));

        v_flex()
            .flex_1()
            .min_h_0()
            .h_full()
            .bg(t.colors.background)
            .p(px(24.0))
            .gap(px(GAP_LG))
            .child(head)
            .child(chips)
            .child(content_card)
            .child(self.profile_status_bar(t))
    }

    /// 活跃度迷你柱图：最近 8 个有提交的日子的提交数。
    /// 没提交的日子不补 0 柱——柱子数量本身就说明「这 30 天里几天在干活」；
    /// 扫描中 / 空仓库画一根矮的轨道色柱，别让 chip 里空一块。
    fn activity_chart(&self, t: &Theme) -> Div {
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
            .gap(px(2.0))
            .children((0..BARS).map(|i| {
                let h = match counts.get(i) {
                    Some(&c) => 6.0 + (c as f32 / max) * 10.0,
                    None => 6.0,
                };
                div()
                    .flex_1()
                    .h(px(h))
                    .rounded(px(R_SM))
                    .bg(if i < n { t.colors.accent } else { track(t) })
            }))
    }

    /// 语言构成条：按字节占比分段，取前 6 色，循环取规范板的 chart 色板。
    fn lang_bar(&self, t: &Theme) -> Div {
        let palette = [
            t.colors.chart_1,
            t.colors.chart_2,
            orange(t),
            t.colors.chart_4,
            t.colors.chart_5,
            edge(t),
        ];
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
            .gap(px(2.0))
            .bg(track(t));
        for (i, l) in langs.iter().enumerate() {
            bar = bar.child(
                div()
                    .h_full()
                    .rounded(px(R_SM))
                    .bg(palette[i % palette.len()])
                    .flex_grow(l.percent.max(0.5)),
            );
        }
        bar
    }

    /// 拉取同步面板（380 宽）：实时进度、日志与最终状态（透明拉取的核心展示区）
    fn sync_panel(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let busy = self.out.running;
        let running_op = self.out.op;
        // 日志区：无内容给占位提示；有内容逐行回显，末行结果按成败着色
        let mut log_area = v_flex()
            .id("pull-log")
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(10.0))
            .gap(px(GAP_TIGHT))
            .rounded(px(R_LG))
            .bg(t.colors.popover)
            .border_1()
            .border_color(t.colors.border);
        if self.out.lines.is_empty() {
            log_area = log_area.child(
                div()
                    .text_size(px(FS_SEC))
                    .font_family(t.mono_font_family.clone())
                    .text_color(tier4(t))
                    .child("透明拉取：进度、日志与最终状态会显示在这里"),
            );
        } else {
            let last = self.out.lines.len().saturating_sub(1);
            for (i, line) in self.out.lines.iter().enumerate() {
                let color = if i == last {
                    if line.starts_with('✓') {
                        t.colors.success
                    } else if line.starts_with('✗') {
                        t.colors.danger
                    } else {
                        tier3(t)
                    }
                } else {
                    tier3(t)
                };
                log_area = log_area.child(
                    div()
                        .text_size(px(FS_META))
                        .font_family(t.mono_font_family.clone())
                        .text_color(color)
                        .child(line.clone()),
                );
            }
        }

        v_flex()
            .w(px(380.0))
            .h_full()
            .flex_shrink_0()
            .bg(t.colors.background)
            .p(px(16.0))
            .gap(px(GAP_LG))
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(FS_BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.colors.foreground)
                            .child("同步"),
                    )
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .text_color(if busy { orange_text(t) } else { tier4(t) })
                            .child(if busy {
                                "同步中…".to_string()
                            } else if self.out.lines.is_empty() {
                                "未执行".to_string()
                            } else {
                                "已完成".to_string()
                            }),
                    ),
            )
            .child(
                div()
                    .text_size(px(FS_SEC))
                    .text_color(tier3(t))
                    .child(match (busy, running_op) {
                        (true, Some(Op::Pull)) => "正在拉取云端更新…".to_string(),
                        (true, Some(Op::Push)) => "正在推送本地提交…".to_string(),
                        (true, Some(Op::Check)) => "正在检查云端更新…".to_string(),
                        (true, _) => "正在同步…".to_string(),
                        (false, _) => "点击下方按钮开始同步".to_string(),
                    }),
            )
            .child({
                let bar = div()
                    .w_full()
                    .h(px(6.0))
                    .rounded(px(R_SM))
                    .bg(track(t))
                    .overflow_hidden();
                if busy {
                    // git 的进度给不出可信百分比，跑来回扫动的 indeterminate 条：
                    // 固定不动的一段会让人以为程序卡住了
                    bar.child(
                        div()
                            .h_full()
                            .rounded(px(R_SM))
                            .bg(t.colors.accent)
                            .with_animation(
                                "sync-bar",
                                Animation::new(Duration::from_millis(1400))
                                    .repeat()
                                    .with_easing(ease_in_out),
                                |this, delta| this.w(relative(0.15 + delta * 0.7)),
                            ),
                    )
                } else {
                    bar.child(div().w(px(0.0)).h_full())
                }
            })
            .child(log_area)
            .child(
                h_flex()
                    .items_center()
                    .gap(px(GAP))
                    .child(
                        Button::new("sync-pull")
                            .label("拉取")
                            .secondary()
                            .loading(busy && running_op == Some(Op::Pull))
                            .with_size(Size::Size(px(BTN)))
                            .on_click(cx.listener(|this, _, _, cx| this.run_sync(Op::Pull, cx))),
                    )
                    .child(
                        Button::new("sync-push")
                            .label("推送")
                            .secondary()
                            .loading(busy && running_op == Some(Op::Push))
                            .with_size(Size::Size(px(BTN)))
                            .on_click(cx.listener(|this, _, _, cx| this.run_sync(Op::Push, cx))),
                    )
                    .child(
                        Button::new("sync-check")
                            .label("检测更新")
                            .ghost()
                            .loading(busy && running_op == Some(Op::Check))
                            .with_size(Size::Size(px(BTN)))
                            .on_click(cx.listener(|this, _, _, cx| this.run_sync(Op::Check, cx))),
                    ),
            )
    }

    fn profile_status_bar(&self, t: &Theme) -> Div {
        h_flex()
            .w_full()
            .h(px(26.0))
            .flex_shrink_0()
            .items_center()
            .gap(px(GAP_LG))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(GAP_TIGHT))
                    .child(
                        div()
                            .w(px(6.0))
                            .h(px(6.0))
                            .rounded_full()
                            .bg(if self.scanning {
                                orange(t)
                            } else {
                                t.colors.success
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(FS_SEC))
                            .text_color(tier3(t))
                            .child(if self.scanning {
                                "扫描中".to_string()
                            } else {
                                "就绪".to_string()
                            }),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(FS_SEC))
                    .text_color(tier4(t))
                    .child("上次操作：—"),
            )
    }
}

/// README 骨架条（数据接入前的占位）
/// 骨架占位条：用官方 Skeleton——它自带 2 秒循环的呼吸动画，
/// 静态的灰条看不出"正在加载"，只会像渲染坏了。
fn skeleton_bar(w: Option<gpui_kit::Pixels>, _t: &Theme) -> AnyElement {
    let mut bar = Skeleton::new().h(px(10.0)).rounded(px(R_MD));
    bar = match w {
        Some(w) => bar.w(w).flex_shrink_0(),
        None => bar.w_full(),
    };
    bar.into_any_element()
}

fn vline(t: &Theme) -> Div {
    div().w(px(1.0)).h_full().flex_shrink_0().bg(t.colors.border)
}
