//! 应用壳：标题栏 / 导航轨 / 仓库侧栏 / 主区 / 状态条。
//!
//! 结构严格对应设计稿（Ardot 731415807346283）：
//! - Screen 1「仓库工作区」：选中**第三方**仓库时显示——侧栏(264px) + 档案页
//! - Screen 3「我的仓库工作台」：选中**我的**仓库时显示——无侧栏，主区直接铺满
//! - Screen 2「总览批量同步」：导航轨第二项，待实现
//!
//! 数据全部来自 qingbird-core：真实仓库列表、真实 git 状态、真实命令。
//! 耗时操作（扫状态、跑命令、扫磁盘）一律进后台线程，界面先画上一次结果。

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::InteractiveElementExt as _;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, AnyElement, AppContext as _, Context, Div, Entity, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Task, Window, px, rgb,
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

    /// 双击仓库行：切换「我的 / 第三方」归属（临时入口，
    /// 等添加 / 编辑仓库弹窗落地后由那里接管）
    fn toggle_third_party(&mut self, path: String, cx: &mut Context<Self>) {
        let cur = self
            .entry(&path)
            .map(|e| e.is_third_party)
            .unwrap_or(false);
        if qingbird_core::store::set_third_party(&path, !cur).is_err() {
            return; // 落盘失败就当没点，下次打开再试
        }
        self.repos = repos::list_repos();
        // 归属变了主区换页：划成第三方要立刻补扫档案数据，划回来清掉
        self.scan_profile(cx);
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
            Page::Overview => overview_page(&t).into_any_element(),
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
            .gap(px(12.0))
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
                    .rounded(px(4.0))
                    .bg(t.colors.primary)
                    .items_center()
                    .justify_center()
                    .text_color(t.colors.popover)
                    .child(Icon::new(AssetIcon::Bird).small()),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(body(t))
                    .child("青鸟 Git"),
            )
            .child(div().flex_1())
            // 全局搜索：官方 Input（检索逻辑待接 Ctrl K）
            .child(
                Input::new(&self.global_search)
                    .prefix(Icon::new(IconName::Search).small())
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
            .gap(px(6.0))
            .child(self.nav_item(IconName::Folder, Page::Repos, t, cx))
            .child(self.nav_item(IconName::LayoutDashboard, Page::Overview, t, cx))
            .child(div().flex_1())
            .child(
                Button::new("nav-plus")
                    .icon(IconName::Plus)
                    .ghost()
                    .with_size(Size::Size(px(36.0))),
            )
            .child(
                Button::new("nav-scan")
                    .icon(IconName::RefreshCw)
                    .ghost()
                    .with_size(Size::Size(px(36.0)))
                    .loading(self.scanning),
            )
            .child(
                Button::new("nav-settings")
                    .icon(IconName::Settings)
                    .ghost()
                    .with_size(Size::Size(px(36.0))),
            )
    }

    /// 可点导航项：36x36 圆角 8，选中暖灰面 + 墨色图标
    fn nav_item(
        &self,
        icon: IconName,
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
            .rounded(px(8.0))
            .items_center()
            .justify_center()
            .when(active, |it| it.bg(warm(t)))
            .child(div().text_color(color).child(Icon::new(icon).small()))
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
            .gap(px(8.0))
            .child(
                h_flex()
                    .w_full()
                    .px(px(2.0))
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(tier4(t))
                            .child("仓库"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(tier4(t))
                            .child(format!("{}", self.repos.len())),
                    ),
            )
            .child(
                // 侧栏过滤：官方 Input
                Input::new(&self.side_filter)
                    .prefix(Icon::new(IconName::Search).small())
                    .xsmall()
                    .w_full(),
            );

        if self.repos.is_empty() {
            return side.child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(tier4(t))
                            .child("还没有仓库"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
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
            // 底部「添加仓库」入口（弹窗待接，先占位）
            h_flex()
                .id("add-repo")
                .w_full()
                .h(px(36.0))
                .px(px(10.0))
                .rounded(px(8.0))
                .items_center()
                .gap(px(8.0))
                .text_color(tier4(t))
                .child(Icon::new(IconName::Plus).small())
                .child(div().text_size(px(13.0)).child("添加仓库")),
        )
    }

    /// 侧栏仓库行：状态点 + 名称 + 状态徽章；单击选中，双击切换归属
    fn repo_row(&self, r: &RepoEntry, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let path = r.path.clone();
        let toggle_path = path.clone();
        let name = self.title(&path);
        let status = self
            .view(&path)
            .map(|v| v.status.clone())
            .unwrap_or_default();
        let b = bucket_of(&status);
        let selected = self.selected.as_deref() == Some(path.as_str());

        let mut row = h_flex()
            .id(SharedString::from(format!("repo-{path}")))
            .w_full()
            .h(px(38.0))
            .px(px(10.0))
            .rounded(px(8.0))
            .items_center()
            .gap(px(8.0))
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
                    .text_size(px(13.0))
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
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .child(text),
                    ),
            );
        }
        row.on_click(cx.listener(move |this, _, _, cx| this.select(path.clone(), cx)))
            .on_double_click(cx.listener(move |this, _, _, cx| {
                this.toggle_third_party(toggle_path.clone(), cx)
            }))
    }
}

fn group_header(text: &str, t: &Theme) -> Div {
    h_flex().w_full().px(px(2.0)).pt(px(4.0)).child(
        div()
            .text_size(px(11.0))
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
                .text_size(px(13.0))
                .text_color(tier3(t))
                .child("选一个仓库"),
        )
        .into_any_element()
}

fn overview_page(t: &Theme) -> Div {
    v_flex()
        .flex_1()
        .min_h_0()
        .h_full()
        .items_center()
        .justify_center()
        .gap(px(4.0))
        .bg(t.colors.background)
        .child(
            div()
                .text_size(px(16.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.colors.foreground)
                .child("总览 · 批量同步"),
        )
        .child(
            div()
                .text_size(px(13.0))
                .text_color(tier3(t))
                .child("按 Screen 2 设计稿实现：仓库表格分组 + 批量拉取 / 推送 + 透明日志"),
        )
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
            .gap(px(10.0))
            .child(
                div()
                    .text_size(px(18.0))
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
            .gap(px(12.0))
            .child(
                v_flex()
                    .gap(px(5.0))
                    .child(title_row)
                    .child(
                        div()
                            .text_size(px(12.0))
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
            .gap(px(6.0))
            .p(px(16.0))
            .rounded(px(12.0))
            .bg(t.colors.popover)
            .border_1()
            .border_color(t.colors.border)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.colors.foreground)
                            .child("常用命令"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.0))
                            .text_color(tier4(t))
                            .child("一键运行 · 输出实时回显 · 可钉住自定义命令（上限 3）"),
                    )
                    .child(
                        Button::new("cmd-edit")
                            .label("编辑")
                            .ghost()
                            .with_size(Size::Size(px(26.0))),
                    ),
            )
            .children(rows)
            .child(self.output_block(t));
        // 底排：磁盘占用卡 + Git 附带卡
        let bottom = h_flex()
            .w_full()
            .flex_1()
            .min_h_0()
            .gap(px(12.0))
            .child(self.space_card(t, cx))
            .child(self.git_card(path, &status, changed, t));

        v_flex()
            .flex_1()
            .min_h_0()
            .h_full()
            .bg(t.colors.background)
            .p(px(24.0))
            .gap(px(14.0))
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
            .gap(px(8.0))
            .child(
                Button::new("wb-pull")
                    .label("拉取")
                    .icon(IconName::ArrowDown)
                    .outline()
                    .loading(pulling)
                    .with_size(Size::Size(px(30.0)))
                    .on_click(cx.listener(|this, _, _, cx| this.run_sync(Op::Pull, cx))),
            )
            .child(
                Button::new("wb-push")
                    .label("推送")
                    .icon(IconName::ArrowUp)
                    .outline()
                    .loading(pushing)
                    .with_size(Size::Size(px(30.0)))
                    .on_click(cx.listener(|this, _, _, cx| this.run_sync(Op::Push, cx))),
            )
            .child(div().w(px(1.0)).h(px(20.0)).bg(t.colors.border))
            .child(
                Button::new("act-folder")
                    .icon(IconName::FolderOpen)
                    .ghost()
                    .with_size(Size::Size(px(30.0))),
            )
            .child(
                Button::new("act-cloud")
                    .icon(IconName::Globe)
                    .ghost()
                    .with_size(Size::Size(px(30.0))),
            )
            .child(
                Button::new("act-doc")
                    .icon(IconName::BookOpen)
                    .ghost()
                    .with_size(Size::Size(px(30.0))),
            )
            .child(
                Button::new("act-timer")
                    .icon(AssetIcon::Clock)
                    .ghost()
                    .with_size(Size::Size(px(30.0))),
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
            .rounded(px(6.0))
            .bg(t.colors.muted)
            .items_center()
            .gap(px(4.0))
            .text_color(tier3(t))
            .child(Icon::new(AssetIcon::GitBranch).small())
            .child(div().text_size(px(12.0)).child(text))
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
            .rounded(px(6.0))
            .items_center()
            .gap(px(12.0))
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
                        Icon::new(IconName::LoaderCircle).small()
                    } else {
                        Icon::new(IconName::Play).small()
                    }),
            )
            .child(
                div()
                    .w(px(76.0))
                    .flex_shrink_0()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(body(t))
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.0))
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
                    .text_size(px(11.0))
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
            .gap(px(4.0))
            .rounded(px(8.0))
            .bg(ink)
            .font_family(t.mono_font_family.clone())
            .text_size(px(11.0))
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
            .gap(px(10.0))
            .p(px(16.0))
            .rounded(px(12.0))
            .bg(t.colors.popover)
            .border_1()
            .border_color(t.colors.border)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.colors.foreground)
                            .child("磁盘占用"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(tier3(t))
                            .child(total_text),
                    )
                    .child(
                        Button::new("space-rescan")
                            .label(if self.space.scanning { "扫描中…" } else { "重新扫描" })
                            .ghost()
                            .with_size(Size::Size(px(26.0)))
                            .loading(self.space.scanning)
                            .on_click(cx.listener(|this, _, _, cx| this.scan_space(cx))),
                    ),
            );

        // 构成条 + 图例
        if !segs.is_empty() {
            let mut bar = h_flex()
                .w_full()
                .h(px(8.0))
                .rounded(px(4.0))
                .overflow_hidden()
                .gap(px(2.0))
                .bg(track(t));
            for (color, size, _, _) in &segs {
                bar = bar.child(div().h_full().rounded(px(4.0)).bg(*color).flex_grow(*size as f32));
            }
            card = card.child(bar);

            let mut legend = v_flex().w_full().gap(px(4.0));
            for (color, _, _, text) in &segs {
                legend = legend.child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .w(px(8.0))
                                .h(px(8.0))
                                .flex_shrink_0()
                                .rounded(px(2.0))
                                .bg(*color),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(body(t))
                                .child(text.clone()),
                        ),
                );
            }
            card = card.child(legend);
        } else {
            card = card.child(
                div()
                    .text_size(px(12.0))
                    .text_color(tier3(t))
                    .child(if self.space.scanning {
                        "正在统计各目录占用…".to_string()
                    } else {
                        "扫描完成后在这里展示目录构成".to_string()
                    }),
            );
        }

        // 清理条：暖橙底 + 深橙文字 + 墨色一键清理
        if cache_total > 0 {
            card = card.child(
                h_flex()
                    .w_full()
                    .p(px(10.0))
                    .rounded(px(8.0))
                    .bg(orange_bg(t))
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.0))
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
                            .with_size(Size::Size(px(28.0)))
                            .loading(self.space.cleaning)
                            .on_click(cx.listener(|this, _, _, cx| this.clean_all(cx))),
                    ),
            );
        } else if rep.is_some() && !self.space.scanning {
            card = card.child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap(px(6.0))
                    .text_color(t.colors.success)
                    .child(Icon::new(IconName::CircleCheck).small())
                    .child(
                        div()
                            .text_size(px(12.0))
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
            .gap(px(10.0))
            .p(px(16.0))
            .rounded(px(12.0))
            .bg(t.colors.popover)
            .border_1()
            .border_color(t.colors.border)
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(tier3(t))
                    .child("Git · 附带功能"),
            )
            .child(
                h_flex()
                    .items_baseline()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(16.0))
                            .font_family(t.mono_font_family.clone())
                            .font_weight(FontWeight::BOLD)
                            .text_color(t.colors.foreground)
                            .child(format!("↑{} ↓{}", s.ahead, s.behind)),
                    )
                    .child(div().text_size(px(11.0)).text_color(tier4(t)).child(desc)),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap(px(8.0))
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
                            .text_size(px(12.0))
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
                    .gap(px(8.0))
                    .child(
                        Button::new("git-pull")
                            .label("拉取")
                            .secondary()
                            .with_size(Size::Size(px(30.0))),
                    )
                    .child(
                        Button::new("git-push")
                            .label("推送")
                            .secondary()
                            .with_size(Size::Size(px(30.0))),
                    ),
            )
            .child(div().w_full().h(px(1.0)).bg(track(t)))
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(t.colors.accent)
                    .child("查看提交历史与文件变更"),
            )
            .child(
                div()
                    .text_size(px(11.0))
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
            .gap(px(12.0))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .w(px(6.0))
                            .h(px(6.0))
                            .rounded_full()
                            .bg(if running { orange(t) } else { t.colors.success }),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
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
                    .text_size(px(12.0))
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
                .text_size(px(11.0))
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
            .gap(px(10.0))
            .child(
                div()
                    .text_size(px(18.0))
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
            .gap(px(12.0))
            .child(
                v_flex()
                    .gap(px(5.0))
                    .child(title_row)
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(tier3(t))
                            .child(format!("本地路径 {path}")),
                    ),
            )
            .child(
                // 右侧动作：四个图标 + 分组线 + 检测更新（次）+ 拉取更新（主）
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        Button::new("pf-folder")
                            .icon(IconName::FolderOpen)
                            .ghost()
                            .with_size(Size::Size(px(30.0))),
                    )
                    .child(
                        Button::new("pf-cloud")
                            .icon(IconName::Globe)
                            .ghost()
                            .with_size(Size::Size(px(30.0))),
                    )
                    .child(
                        Button::new("pf-doc")
                            .icon(IconName::BookOpen)
                            .ghost()
                            .with_size(Size::Size(px(30.0))),
                    )
                    .child(
                        Button::new("pf-timer")
                            .icon(AssetIcon::Clock)
                            .ghost()
                            .with_size(Size::Size(px(30.0))),
                    )
                    .child(div().w(px(1.0)).h(px(20.0)).bg(t.colors.border))
                    .child(
                        Button::new("pf-check-update")
                            .label("检测更新")
                            .outline()
                            .loading(self.out.running && self.out.op == Some(Op::Check))
                            .with_size(Size::Size(px(34.0)))
                            .on_click(cx.listener(|this, _, _, cx| this.run_sync(Op::Check, cx))),
                    )
                    .child(
                        Button::new("pf-pull-update")
                            .label("拉取更新")
                            .primary()
                            .loading(self.out.running && self.out.op == Some(Op::Pull))
                            .with_size(Size::Size(px(34.0)))
                            .on_click(cx.listener(|this, _, _, cx| this.run_sync(Op::Pull, cx))),
                    ),
            );

        // 信息页签：标签 + 值。技术栈 / 最近提交 / 本地来自本地扫描；
        // Stars 要远端 API（GitHub / Gitee），等克隆 / 新建仓库接入时一起做。
        let chip = |label: &str, value: &str| -> Div {
            h_flex()
                .h(px(30.0))
                .px(px(10.0))
                .rounded(px(6.0))
                .bg(t.colors.popover)
                .border_1()
                .border_color(t.colors.border)
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(tier4(t))
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(px(12.0))
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
            .gap(px(8.0))
            .child(chip("技术栈", &stack_text))
            .child(chip("Stars", "—"))
            .child(chip("最近提交", &last_commit_text))
            .child(chip("本地", &local_size_text))
            .child(
                h_flex()
                    .h(px(30.0))
                    .px(px(10.0))
                    .gap(px(8.0))
                    .rounded(px(6.0))
                    .bg(t.colors.popover)
                    .border_1()
                    .border_color(t.colors.border)
                    .items_center()
                    .child(
                        div()
                            .text_size(px(11.0))
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
            .rounded(px(12.0))
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
                    .gap(px(12.0))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .font_family(t.mono_font_family.clone())
                            .text_color(tier4(t))
                            .child("README"),
                    )
                    .child(
                        div()
                            .text_size(px(16.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.colors.foreground)
                            .child(readme_title),
                    )
                    .children(match &readme {
                        Some(r) if !r.intro.is_empty() => vec![div()
                            .w_full()
                            .text_size(px(13.0))
                            .line_height(px(22.0))
                            .text_color(tier3(t))
                            .child(r.intro.clone())],
                        Some(_) => vec![div()
                            .w_full()
                            .text_size(px(13.0))
                            .line_height(px(22.0))
                            .text_color(tier3(t))
                            .child("这份仓库的 README 没有可展示的简介段落。")],
                        None => {
                            if self.profile.scanning {
                                vec![skeleton_bar(None, t), skeleton_bar(Some(px(420.0)), t)]
                            } else {
                                vec![div()
                                    .w_full()
                                    .text_size(px(13.0))
                                    .line_height(px(22.0))
                                    .text_color(tier3(t))
                                    .child("仓库根目录没有找到 README 文件。")]
                            }
                        }
                    })
                    .child(
                        div()
                            .pt(px(4.0))
                            .text_size(px(11.0))
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
            .gap(px(14.0))
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
                    .rounded(px(1.0))
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
                .rounded(px(4.0))
                .overflow_hidden()
                .bg(track(t))
                .child(div().h_full().rounded(px(4.0)).bg(track(t)));
        }
        let mut bar = h_flex()
            .w_full()
            .h(px(8.0))
            .rounded(px(4.0))
            .overflow_hidden()
            .gap(px(2.0))
            .bg(track(t));
        for (i, l) in langs.iter().enumerate() {
            bar = bar.child(
                div()
                    .h_full()
                    .rounded(px(4.0))
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
            .gap(px(4.0))
            .rounded(px(8.0))
            .bg(t.colors.popover)
            .border_1()
            .border_color(t.colors.border);
        if self.out.lines.is_empty() {
            log_area = log_area.child(
                div()
                    .text_size(px(12.0))
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
                        .text_size(px(11.0))
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
            .gap(px(12.0))
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.colors.foreground)
                            .child("同步"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
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
                    .text_size(px(12.0))
                    .text_color(tier3(t))
                    .child(match (busy, running_op) {
                        (true, Some(Op::Pull)) => "正在拉取云端更新…".to_string(),
                        (true, Some(Op::Push)) => "正在推送本地提交…".to_string(),
                        (true, Some(Op::Check)) => "正在检查云端更新…".to_string(),
                        (true, _) => "正在同步…".to_string(),
                        (false, _) => "点击下方按钮开始同步".to_string(),
                    }),
            )
            .child(
                div()
                    .w_full()
                    .h(px(6.0))
                    .rounded(px(3.0))
                    .bg(track(t))
                    .child(
                        div()
                            .w(if busy { px(60.0) } else { px(0.0) })
                            .h_full()
                            .rounded(px(3.0))
                            .bg(t.colors.accent),
                    ),
            )
            .child(log_area)
            .child(
                h_flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        Button::new("sync-pull")
                            .label("拉取")
                            .secondary()
                            .loading(busy && running_op == Some(Op::Pull))
                            .with_size(Size::Size(px(30.0)))
                            .on_click(cx.listener(|this, _, _, cx| this.run_sync(Op::Pull, cx))),
                    )
                    .child(
                        Button::new("sync-push")
                            .label("推送")
                            .secondary()
                            .loading(busy && running_op == Some(Op::Push))
                            .with_size(Size::Size(px(30.0)))
                            .on_click(cx.listener(|this, _, _, cx| this.run_sync(Op::Push, cx))),
                    )
                    .child(
                        Button::new("sync-check")
                            .label("检测更新")
                            .ghost()
                            .loading(busy && running_op == Some(Op::Check))
                            .with_size(Size::Size(px(30.0)))
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
            .gap(px(12.0))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(6.0))
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
                            .text_size(px(12.0))
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
                    .text_size(px(12.0))
                    .text_color(tier4(t))
                    .child("上次操作：—"),
            )
    }
}

/// README 骨架条（数据接入前的占位）
fn skeleton_bar(w: Option<gpui_kit::Pixels>, t: &Theme) -> Div {
    let mut bar = div()
        .w_full()
        .h(px(10.0))
        .rounded(px(5.0))
        .bg(track(t));
    if let Some(w) = w {
        bar = bar.w(w).flex_shrink_0();
    }
    bar
}

fn vline(t: &Theme) -> Div {
    div().w(px(1.0)).h_full().flex_shrink_0().bg(t.colors.border)
}
