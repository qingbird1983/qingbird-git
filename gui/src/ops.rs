//! 后台操作编排：跑命令、三键同步、扫磁盘、扫档案、总览批量。
//!
//! 每个耗时操作都是「先记状态 → 起后台线程 → 回来更新 UI」，界面永远先画上一次结果。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::component::input::InputState;
use gpui_kit::component::WindowExt;
use gpui_kit::{AppContext, Context, SharedString, Window};
use qingbird_core::fork;
use qingbird_core::git;
use qingbird_core::repos::{self};
use qingbird_core::run_cmd::{self, Slot};
use qingbird_core::scan_cache;
use qingbird_core::space;
use qingbird_core::store;
use qingbird_core::sync;
use qingbird_core::types::ProgressEvent;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::state::*;
use crate::widgets::auto_sync_text;

/// 后台线程往共享缓冲里放的两种东西：输出一行，或者报一次进度。
///
/// 分成两种是因为进度**不上屏**：它只更新状态行与进度条。
/// 混成字符串塞进 `lines` 的话，"拉取"和"检测更新"两类操作在输出流里
/// 会长得一模一样，而它们的阶段差别正是用户想看的
#[derive(Clone)]
enum LogEvent {
    Line(String),
    Stage { stage: String, percent: Option<f32> },
}

impl Shell {
    // ---------- 跑命令 ----------

    pub(crate) fn run_slot(&mut self, slot: Slot, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        let cmd = self.effective_cmd(&path, slot);
        if cmd.is_empty() {
            let lg = self.log_of(&path);
            lg.error = Some(format!(
                "识别不出「{}」的命令，这条先跳过",
                slot.default_label()
            ));
            lg.ts = store::now_secs();
            cx.notify();
            return;
        }

        *self.log_of(&path) = LogState {
            running: true,
            op: Some(Op::Cmd(slot)),
            ts: store::now_secs(),
            ..LogState::default()
        };
        cx.notify();

        // 命令在后台线程跑，行进共享缓冲；UI 线程定时把新行搬进状态
        let buf: Arc<Mutex<Vec<LogEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));

        let sink_buf = Arc::clone(&buf);
        let sink_done = Arc::clone(&done);
        let run_path = path.clone();
        let runner = cx.background_executor().spawn(async move {
            let on_line: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line: &str| {
                if let Ok(mut g) = sink_buf.lock() {
                    g.push(LogEvent::Line(line.to_string()));
                }
            });
            let r = run_cmd::run_repo_cmd(&run_path, &cmd, slot, on_line);
            sink_done.store(true, Ordering::SeqCst);
            r
        });

        let ui_buf = Arc::clone(&buf);
        let ui_done = Arc::clone(&done);
        let ui_path = path.clone();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(120))
                .await;
            let batch = {
                let mut g = ui_buf.lock().unwrap_or_else(|e| e.into_inner());
                std::mem::take(&mut *g)
            };
            let finished = ui_done.load(Ordering::SeqCst);
            this.update(cx, |this, cx| {
                let lg = this.log_of(&ui_path);
                for ev in batch {
                    match ev {
                        LogEvent::Line(l) => lg.lines.push(l),
                        LogEvent::Stage { stage, percent } => {
                            lg.stage = Some(stage);
                            lg.percent = percent;
                        }
                    }
                }
                if finished {
                    lg.running = false;
                }
                cx.notify();
            })
            .ok();
            if finished {
                break;
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            let result = runner.await;
            let fin_path = path.clone();
            this.update(cx, |this, cx| {
                let lg = this.log_of(&fin_path);
                match result {
                    Ok(_) => {
                        lg.error = None;
                        lg.verdict = Some((true, format!("{} 已完成", slot.default_label())));
                    }
                    Err(e) => lg.error = Some(e),
                }
                lg.running = false;
                lg.percent = None;
                // 命令跑完仓库状态可能变了（生成了构建产物、改了文件），顺手重扫
                this.repos = repos::list_repos();
                this.refresh_status(cx);
            })
            .ok();
        })
        .detach();
    }
}

impl Shell {
    /// 跑一个同步操作（检测更新 / 拉取 / 推送 / 变基）。
    /// 与 run_slot 同一套后台管道：后台线程跑 + 共享缓冲，UI 线程 120ms 轮询回显。
    pub(crate) fn run_sync(&mut self, op: Op, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        // 连点护栏：只看**这个仓库**在不在跑。别的仓库在跑是 core 的锁管的事，
        // 硬拦在这里只会让"两个仓库排队跑"这种正常用法直接没反应
        if self.log_of(&path).running {
            return;
        }
        *self.log_of(&path) = LogState {
            running: true,
            op: Some(op),
            ts: store::now_secs(),
            ..LogState::default()
        };
        cx.notify();

        let buf: Arc<Mutex<Vec<LogEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));

        let sink_buf = Arc::clone(&buf);
        let sink_done = Arc::clone(&done);
        let run_path = path.clone();
        let runner = cx.background_executor().spawn(async move {
            let on_progress: Arc<dyn Fn(&ProgressEvent) + Send + Sync> =
                Arc::new(move |ev: &ProgressEvent| {
                    if let Ok(mut g) = sink_buf.lock() {
                        // **优先上 git 原文**（用户要的是"像终端那样一行行显示"）。
                        // 原来这里把 stage + detail 拼成 `· 接收对象  已传输 1.2 MiB`
                        // 这样的**二次转述**——信息在翻译里被磨掉了一层。
                        // 现在：raw 非空就用原文；空（我们自己发的阶段提示）才退回转述。
                        if !ev.raw.is_empty() {
                            if ev.to_log {
                                g.push(LogEvent::Line(ev.raw.clone()));
                            }
                        } else {
                            let mut line = format!("· {}", ev.stage);
                            if !ev.detail.is_empty() {
                                line.push_str(&format!("  {}", ev.detail));
                            }
                            g.push(LogEvent::Line(line));
                        }
                        // 进度条只吃 stage/percent，跟日志是不是原文无关
                        if !ev.stage.is_empty() || ev.percent.is_some() {
                            g.push(LogEvent::Stage {
                                stage: ev.stage.clone(),
                                percent: ev.percent,
                            });
                        }
                    }
                });
            let r = match op {
                Op::Pull => sync::pull_repo(&run_path, true, Some(&*on_progress)),
                Op::Push => sync::push_repo(&run_path),
                Op::Check => sync::check_update(&run_path, Some(&*on_progress)),
                Op::Rebase(stash) => fork::rebase_fork(&run_path, stash, Some(&*on_progress)),
                Op::UndoRebase => fork::undo_rebase(&run_path),
                Op::Commit => unreachable!("提交并推送走 run_commit，不进 run_sync"),
                Op::Cmd(_) => unreachable!("run_sync 只接同步操作"),
            };
            sink_done.store(true, Ordering::SeqCst);
            r
        });

        let ui_buf = Arc::clone(&buf);
        let ui_done = Arc::clone(&done);
        let ui_path = path.clone();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(120))
                .await;
            let batch = {
                let mut g = ui_buf.lock().unwrap_or_else(|e| e.into_inner());
                std::mem::take(&mut *g)
            };
            let finished = ui_done.load(Ordering::SeqCst);
            this.update(cx, |this, cx| {
                let lg = this.log_of(&ui_path);
                for ev in batch {
                    match ev {
                        LogEvent::Line(l) => lg.lines.push(l),
                        LogEvent::Stage { stage, percent } => {
                            lg.stage = Some(stage);
                            lg.percent = percent;
                        }
                    }
                }
                if finished {
                    lg.running = false;
                }
                cx.notify();
            })
            .ok();
            if finished {
                break;
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            let result = runner.await;
            let fin_path = path.clone();
            this.update(cx, |this, cx| {
                {
                    let lg = this.log_of(&fin_path);
                    lg.lines.push(if result.ok {
                        format!("✓ {}", result.message)
                    } else {
                        format!("✗ {}", result.message)
                    });
                    lg.verdict = Some((result.ok, result.message.clone()));
                    lg.used_proxy = result.used_proxy.clone();
                    lg.running = false;
                    lg.percent = None;
                }
                // 变基改写了历史：接住后端算好的分叉信息与备份分支。
                // 备份分支只在变基成功那一刻提供"撤销"；撤销完成后不再提供
                match op {
                    Op::Rebase(_) => {
                        this.fork = result.fork;
                        this.fork_backup = result.backup_branch.filter(|_| result.ok);
                        this.fork_confirm = false;
                    }
                    Op::UndoRebase => {
                        this.fork = result.fork;
                        this.fork_backup = None;
                        this.fork_confirm = false;
                    }
                    _ => {}
                }
                // 拉取/推送/变基都会改 ahead/behind，重扫一遍刷新侧栏与徽章
                this.repos = repos::list_repos();
                this.refresh_status(cx);
            })
            .ok();
        })
        .detach();
    }
}

impl Shell {
    /// 停掉当前正在跑的操作（工具行那个「停止」按钮与 `Ctrl .` 共用这一个入口）。
    ///
    /// core 那两个入口都是**进程级全局**的——同一时刻只放一个忙操作——
    /// 所以这里不按仓库分：只要有活就停，活是谁的不影响结果。
    pub(crate) fn stop_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let lg = self.log();
        if !lg.running {
            window.push_notification("当前没有正在运行的操作", cx);
            return;
        }
        let is_cmd = matches!(lg.op, Some(Op::Cmd(_)));
        let label = match lg.op {
            Some(Op::Cmd(s)) => s.default_label().to_string(),
            Some(Op::Pull) => "拉取".to_string(),
            Some(Op::Push) => "推送".to_string(),
            Some(Op::Check) => "检测更新".to_string(),
            Some(Op::Rebase(_)) => "变基".to_string(),
            Some(Op::UndoRebase) => "撤销变基".to_string(),
            Some(Op::Commit) => "提交并推送".to_string(),
            None => "操作".to_string(),
        };
        let r = if is_cmd {
            run_cmd::stop_repo_cmd()
        } else {
            git::request_cancel();
            Ok(())
        };
        match r {
            Ok(()) => {
                // 停在**它自己的**日志里，别写到当前选中的仓库名下
                if let Some(path) = self.selected.clone() {
                    self.log_of(&path)
                        .lines
                        .push(format!("· 已请求停止{label}"));
                }
                cx.notify();
            }
            Err(e) => window.push_notification(format!("停止失败：{e}"), cx),
        }
    }

    /// 清空当前仓库的日志。只清内容不动"正在跑"——
    /// 跑到一半清屏是合法操作，进程还在，写进 `lines` 的新行照常出现
    pub(crate) fn clear_log(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.selected.clone() {
            let lg = self.log_of(&path);
            lg.lines.clear();
            lg.error = None;
        }
        cx.notify();
    }
}

impl Shell {
    // ---------- 磁盘占用 ----------

    /// 扫当前仓库的磁盘占用。大仓库可能好几秒，放后台，界面先画上一次的结果。
    ///
    /// ⚠️ **先把库里那一份灌进来，再亮 `scanning`**（阶段 16）。
    /// 反过来写就是"先把旧数扔掉、显示扫描中"——缓存一秒都没起作用，
    /// 正好和做它的目的一样。空间页那条判据
    /// `scanning && report.is_none()` 也正是这个前提：报告还在就不出骨架屏。
    pub(crate) fn scan_space(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        self.space_seq += 1;
        let seq = self.space_seq;
        self.space_task = None;
        match scan_cache::load_cache(&path) {
            Some(c) => self.apply_selected_cache(&c, cx),
            // 没有缓存：整份清空走骨架屏，与从没扫过一模一样
            None => self.space = SpaceState::default(),
        }
        self.space.scanning = true;
        self.space.error = None;
        cx.notify();

        // 后台线程要用一份自己的 `path`，回来还要按路径落库
        let repo = path.clone();
        self.space_task = Some(cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { space::scan_repo_space(&repo) })
                .await;
            this.update(cx, |this, cx| {
                if this.space_seq != seq {
                    return; // 切了仓库 / 已有新一轮，这批过期
                }
                this.space.scanning = false;
                match r {
                    Ok(rep) => {
                        this.space.report = Some(rep);
                        this.space.scanned_at = store::now_secs();
                        this.space.error = None;
                        // 根层直接吃刚扫出来的 `dirs`（那一层的递归体积刚算过），
                        // 并借 `tree_reset` 里的 `tree_seq` 自增作废在途的子层扫描
                        this.tree_reset(cx);
                        // **扫完落库**：下次启动直接吃这一份，不必再数一遍
                        // （98 GB 的仓库这是唯一能接受的开销）
                        this.persist_space_cache(&path);
                    }
                    Err(e) => this.space.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        }));
    }
}

impl Shell {
    // ---------- 档案页数据（Screen 1） ----------

    /// 扫当前仓库的档案数据：语言统计 / 近 30 天活跃度 / README 原文。
    /// 一个后台任务跑完一次性上屏。
    ///
    /// **README 与语言构成两类仓库都跑，只有活跃度只给第三方。**
    /// README 在两类仓库的「说明」页签里都有，读一个文件又不贵；
    /// 语言构成 2026-10-05 起是概览页的**语言技术栈段**，那也是两类都有
    /// （`repo_languages` 要遍历 `git ls-files` 的每个文件，但不派生网络请求，
    /// 一次 ls-files + 一轮 metadata 足够，第三方那屏也正是靠它出那张条）。
    /// `commit_activity` 要扫 30 天 log，那条只留给第三方的「仓库档案」卡
    pub(crate) fn scan_profile(&mut self, cx: &mut Context<Self>) {
        self.profile_seq += 1;
        self.profile_task = None;
        let Some(path) = self.selected.clone() else {
            self.profile = ProfileState::default();
            return;
        };
        let third = self.selected_is_third();
        let seq = self.profile_seq;
        // ⚠️ 与 `scan_space` 同一个道理：**先把库里的那份灌进来**。
        // 否则 `ProfileState { scanning: true, ..Default }` 一执行，
        // 缓存里的语言条 / README 就被扔了——而「说明」页签那条判据
        // （`profile.scanning` 就出骨架屏）本来是留给"真没内容"的
        match scan_cache::load_cache(&path) {
            Some(c) => self.apply_selected_cache(&c, cx),
            None => self.profile = ProfileState::default(),
        }
        self.profile.scanning = true;
        self.profile.readme_error = None;
        cx.notify();

        // 后台线程要一份自己的 `path`（`path` 得留着回来落库）
        let repo = path.clone();
        self.profile_task = Some(cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move {
                    let activity = if third {
                        repos::commit_activity(&repo, PROFILE_ACTIVITY_DAYS)
                    } else {
                        Vec::new()
                    };
                    (
                        repos::repo_languages(&repo),
                        activity,
                        // 原文与文件名各调一次 `find_readme`（同一个只读文件探查），
                        // 换来界面上能说清"渲染的是 README.md 还是 README.rst"
                        repos::read_readme_text(&repo),
                        repos::readme_file_name(&repo),
                        // 「另一个语言」那一份。**判据是刚读到的这个文件名**——
                        // 用户切到英文之后再点一次要切回来，靠它重算。
                        // 跟 `readme_file` 一起在这儿问：`repo` 是被移进这个
                        // 后台任务的，回到 update 闭包里就拿不到了
                        repos::readme_file_name(&repo)
                            .and_then(|f| repos::readme_alt(&repo, &f)),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                if this.profile_seq != seq {
                    return; // 切了仓库 / 已有新一轮，这批过期
                }
                let (langs, activity, readme_text, readme_file, readme_alt) = r;
                // 扫完这一次性转成 SharedString，之后每帧只是克隆一个引用计数
                let readme_text = readme_text.map(SharedString::from);
                // 正文状态也换掉：用户切页签回来看到的是新仓库的文档，
                // 不是上一个仓库留在那儿的那份
                this.readme_state.update(cx, |st, cx| {
                    st.set_text(readme_text.as_deref().unwrap_or_default(), cx)
                });
                this.profile = ProfileState {
                    langs,
                    activity,
                    readme_text,
                    readme_file,
                    readme_alt,
                    readme_error: None,
                    scanning: false,
                    scanned_at: store::now_secs(),
                };
                // 档案数据落库，下次启动不用再遍历一遍 `git ls-files`
                this.persist_profile_cache(&path);
                cx.notify();
            })
            .ok();
        }));
    }
}

impl Shell {
    // ---------- 批量同步（⌘K 的三条固定命令，Q3） ----------

    /// 这一批要跑哪些仓库。
    ///
    /// **按状态扫描已经算好的分档筛，不新增任何 git 调用**：
    /// 「谁落后、谁待推送」这几条数据本来就在 `RepoView::status` 里，
    /// 界面再探一遍就是白派生一遍 git 子进程。
    ///
    /// 推送这一条会把第三方整个滤掉——推送会改别人的仓库，
    /// 那是设计 3.5 的硬边界，批量层不能因为"方便"绕过它
    pub(crate) fn batch_targets(&self, kind: BatchKind) -> Vec<String> {
        self.repos
            .iter()
            .filter(|r| kind.wants(r.is_third_party, &self.status_of(&r.path)))
            .map(|r| r.path.clone())
            .collect()
    }

    /// 从命令面板起一批。**回车即跑，不弹确认框**：
    /// 队列已经按分档筛过（没有落后的仓库根本不会进这一批），
    /// 再问一次"确定要拉取这 3 个仓库吗"只是把系统已经知道的事问一遍
    pub(crate) fn start_batch(
        &mut self,
        kind: BatchKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.batch.as_ref().is_some_and(|b| b.running) {
            window.push_notification("上一批还在跑，等它跑完", cx);
            return;
        }
        let paths = self.batch_targets(kind);
        if paths.is_empty() {
            window.push_notification(
                match kind {
                    BatchKind::Pull => "现在没有落后的仓库",
                    BatchKind::Push => "现在没有待推送的仓库",
                    BatchKind::Check => "还没有任何仓库",
                }
                .to_string(),
                cx,
            );
            return;
        }
        self.run_batch(kind, paths, cx);
    }

    /// 串行跑一批同步操作。
    ///
    /// **串行而不是并发**：core 那边的 `OP_LOCK` 是进程级的，同一时刻只放
    /// 一个忙操作，并发发过去等于后面每一个都立刻失败。串行还顺带给了
    /// 用户一个"跑到第几个了"的确定进度。
    ///
    /// 拉取遇脏工作区**跳过并记日志、继续下一个**（不自动 stash——那是替用户
    /// 做决定），与旧版总览批量的约定一致。
    ///
    /// **不新增后端批量 command**：代理回退、分叉识别、脏工作区判定都已经
    /// 在 `sync::pull_repo` / `push_repo` / `check_update` 那两条路径里，
    /// 再写一份 `pull_many` 只会让两边行为漂移。
    pub(crate) fn run_batch(
        &mut self,
        kind: BatchKind,
        paths: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if self.batch.as_ref().is_some_and(|b| b.running) {
            return; // 上一批还在跑
        }
        if paths.is_empty() {
            return;
        }
        self.batch = Some(BatchRun {
            kind,
            paths: paths.clone(),
            done: 0,
            ok: 0,
            skipped: 0,
            failed: 0,
            running: true,
            note: None,
        });
        // 批次是跨仓库的，汇总不写进任何仓库的日志
        self.app_log.lines.clear();
        self.app_log.error = None;
        self.app_log.verdict = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            for path in paths {
                // 跑之前先看脏不脏：拉取对脏工作区让路。
                // 只在拉取时判定——检测不改工作区，推送推的是提交不是文件
                let skip = kind == BatchKind::Pull
                    && this
                        .update(cx, |this, _| this.status_of(&path).dirty)
                        .unwrap_or(false);
                // None = 跳过，Some((成功?, 一句话)) = 跑过了
                let outcome: Option<(bool, String)> = if skip {
                    None
                } else {
                    let k = kind;
                    let p = path.clone();
                    this.update(cx, |this, cx| {
                        // 让侧栏那一行转起来：批量也是"点下去就有反馈"的操作，
                        // 不标出来用户会以为卡住了
                        let lg = this.log_of(&p);
                        lg.running = true;
                        lg.op = Some(match k {
                            BatchKind::Pull => Op::Pull,
                            BatchKind::Push => Op::Push,
                            BatchKind::Check => Op::Check,
                        });
                        lg.ts = store::now_secs();
                        cx.notify();
                    })
                    .ok();
                    Some(
                        cx.background_executor()
                            .spawn(async move {
                                let r = match k {
                                    BatchKind::Pull => sync::pull_repo(&p, true, None),
                                    BatchKind::Push => sync::push_repo(&p),
                                    BatchKind::Check => sync::check_update(&p, None),
                                };
                                (r.ok, r.message)
                            })
                            .await,
                    )
                };
                let p = path.clone();
                this.update(cx, |this, cx| {
                    // 先算全局那一行：`log_of` 会把 `this` 整个可变借走，
                    // 拿了它再去碰 `app_log` 是第二个可变借用
                    let app_line = match &outcome {
                        None => Some(format!("· {}：工作区有改动，已跳过", this.title(&p))),
                        Some((true, _)) => None,
                        Some((false, msg)) => Some(format!("✗ {}：{msg}", this.title(&p))),
                    };
                    if let Some(line) = app_line {
                        this.app_log.lines.push(line);
                    }
                    // 每一条都记进**那个仓库自己的**日志：失败原因在那里，
                    // 汇总只报数字
                    let lg = this.log_of(&p);
                    match &outcome {
                        None => lg
                            .lines
                            .push("· 工作区有改动，已跳过（切到该仓库确认后再拉）".into()),
                        Some((true, msg)) => lg.lines.push(format!("✓ {msg}")),
                        Some((false, msg)) => {
                            lg.lines.push(format!("✗ {msg}"));
                            lg.error = Some(msg.clone());
                        }
                    }
                    lg.running = false;
                    let Some(b) = this.batch.as_mut() else {
                        return;
                    };
                    b.done += 1;
                    match &outcome {
                        None => b.skipped += 1,
                        Some((true, _)) => b.ok += 1,
                        Some((false, _)) => b.failed += 1,
                    }
                    cx.notify();
                })
                .ok();
            }
            let win = this.update(cx, |this, _| this.win).ok();
            let note = this
                .update(cx, |this, cx| {
                    let Some(b) = this.batch.as_mut() else {
                        return None;
                    };
                    b.running = false;
                    let mut parts = vec![format!("{} 个成功", b.ok)];
                    if b.skipped > 0 {
                        parts.push(format!(
                            "{} 个跳过（工作区有改动，需切到该仓库确认后再拉）",
                            b.skipped
                        ));
                    }
                    if b.failed > 0 {
                        parts.push(format!("{} 个失败，原因见各自仓库的日志", b.failed));
                    }
                    let note = format!("{}完成：{}", b.kind.label(), parts.join(" · "));
                    b.note = Some(note.clone());
                    // 汇总落全局槽。这一批横跨多个仓库，记到"当前选中"名下
                    // 会让人以为只动了一个仓库
                    this.app_log.lines.push(note.clone());
                    this.app_log.verdict = Some((b.failed == 0, note.clone()));
                    this.app_log.ts = store::now_secs();
                    // 拉取/推送改了 ahead/behind，重扫刷新侧栏与表格
                    this.refresh_status(cx);
                    cx.notify();
                    Some(note)
                })
                .ok()
                .flatten();
            // 通知要一个活的 `&mut Window`，而后台任务里只有 `&mut AsyncApp`。
            // 用记着的窗口句柄换回来——和总览表格点行那条路同一个套路
            if let (Some(note), Some(win)) = (note, win) {
                cx.update_window(win, |_, window, cx| {
                    window.push_notification(note, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    /// 状态条里那一格批量进度（跑批时覆盖「运行中 · X」）。
    ///
    /// **只给短句**：完整那句汇总带"切到该仓库确认后再拉"这样的解释，
    /// 塞进状态条会把中间那一格撑走、路径被挤出可视区。
    /// 完整结论在 `app_log` 与跑完时的通知里
    pub(crate) fn batch_note(&self) -> Option<String> {
        self.batch.as_ref().map(|b| {
            batch_summary(b.kind, b.done, b.paths.len(), b.ok, b.skipped, b.failed)
        })
    }
}

impl Shell {
    // ---------- 提交并推送 ----------

    /// 提交并推送。走 core 的 `sync::commit_push_with`——和拉取/推送同一条
    /// `run_net_op` 链路，代理回退与凭证注入都在里面。
    ///
    /// 返回 true = 成功（调用方据此关对话框）。失败时返回 false，
    /// 错误已经写进日志与对话框，用户填的提交信息还在原地
    pub(crate) fn run_commit(
        &mut self,
        path: String,
        message: String,
        cx: &mut Context<Self>,
    ) -> bool {
        if message.trim().is_empty() {
            if let Some(d) = self.commit_dlg.as_mut() {
                d.error = Some("先写一句这次改了什么".into());
            }
            cx.notify();
            return false;
        }
        if self.log_of(&path).running {
            if let Some(d) = self.commit_dlg.as_mut() {
                d.error = Some("这个仓库还有操作在跑，等它跑完再提交".into());
            }
            cx.notify();
            return false;
        }
        let pull_first = self
            .commit_dlg
            .as_ref()
            .map(|d| d.pull_first)
            .unwrap_or(true);

        *self.log_of(&path) = LogState {
            running: true,
            op: Some(Op::Commit),
            ts: store::now_secs(),
            ..LogState::default()
        };
        if let Some(d) = self.commit_dlg.as_mut() {
            d.running = true;
            d.error = None;
        }
        cx.notify();

        let run_path = path.clone();
        let msg = message.clone();
        cx.spawn(async move |this, cx| {
            // 提交本身是本地的（不走网络），拉取与推送才走 run_net_op。
            // core 那边已经按顺序串好了，界面只等一个结果
            let r = cx
                .background_executor()
                .spawn(async move { sync::commit_push_with(&run_path, &msg, pull_first) })
                .await;
            this.update(cx, |this, cx| {
                let lg = this.log_of(&path);
                lg.lines.push(if r.ok {
                    format!("✓ {}", r.message)
                } else {
                    format!("✗ {}", r.message)
                });
                lg.verdict = Some((r.ok, r.message.clone()));
                lg.used_proxy = r.used_proxy.clone();
                lg.running = false;
                lg.percent = None;
                if let Some(d) = this.commit_dlg.as_mut() {
                    d.running = false;
                    if !r.ok {
                        d.error = Some(r.message.clone());
                    }
                }
                if r.ok {
                    this.commit_dlg = None;
                }
                // 提交与推送都改 ahead/behind，变更清单也变了，一并重扫
                this.repos = repos::list_repos();
                this.refresh_status(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        true
    }
}

impl Shell {
    // ---------- 命令编辑 ----------

    /// 保存命令编辑对话框里的十个字段。
    /// 返回 true = 成功（调用方据此关对话框），false = 留在原地看错误
    pub(crate) fn save_cmds(
        &mut self,
        path: String,
        values: Vec<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if values.len() != CMD_FIELDS.len() {
            return false;
        }
        let (slots, pinned) = values.split_at(4);
        match run_cmd::set_repo_cmds(
            &path,
            slots[0].trim(),
            slots[1].trim(),
            slots[2].trim(),
            slots[3].trim(),
        ) {
            Ok(list) => self.repos = list,
            Err(e) => return self.set_cmds_error(&format!("保存失败：{e}"), cx),
        }
        // 三个钉住命令按「文案 / 命令」成对读回来
        let cmds: Vec<store::PinnedCmd> = (0..run_cmd::MAX_PINNED_CMDS)
            .map(|i| store::PinnedCmd {
                label: pinned[i * 2].trim().to_string(),
                cmd: pinned[i * 2 + 1].trim().to_string(),
            })
            .collect();
        match run_cmd::set_pinned_cmds(&path, cmds) {
            Ok(list) => self.repos = list,
            Err(e) => return self.set_cmds_error(&e, cx),
        }
        self.cmd_dlg = None;
        cx.notify();
        true
    }

    fn set_cmds_error(&mut self, msg: &str, cx: &mut Context<Self>) -> bool {
        if let Some(d) = self.cmd_dlg.as_mut() {
            d.error = Some(msg.to_string());
        }
        cx.notify();
        false
    }
}

impl Shell {
    // ---------- 空间页签的两个逐项动作 ----------

    /// 清理某一个缓存目录。清完立刻重扫，构成条与图例才跟上
    pub(crate) fn clean_cache(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        if self.space.cleaning {
            return;
        }
        self.space.cleaning = true;
        self.space.error = None;
        cx.notify();
        let name = name.to_string();
        // 下面那个后台 spawn 会把 `path` / `name` move 走，而 `update` 闭包里
        // 还要用它们拼日志文案——**先各留一份**
        let for_task = (path.clone(), name.clone());
        cx.spawn(async move |this, cx| {
            let (p, n) = for_task;
            let r = cx
                .background_executor()
                .spawn(async move { space::delete_cache_dir(&p, &n) })
                .await;
            this.update(cx, |this, cx| {
                this.space.cleaning = false;
                let (ok, note) = match r {
                    Ok(bytes) => (true, format!("清理 {name}：已释放 {}", space::human_bytes(bytes))),
                    Err(e) => {
                        this.space.error = Some(e.clone());
                        (false, format!("清理 {name} 失败：{e}"))
                    }
                };
                this.space.note = Some(note.clone());
                // 落进**这个仓库自己的**日志槽（`log_of`）。只写 `space.note`
                // 的话结论只活在这一页的状态条上，切到概览页就永远看不到了——
                // 而概览页的日志卡才是用户默认会去看的地方
                let err = this.space.error.clone();
                let lg = this.log_of(&path);
                lg.lines.push(note.clone());
                lg.verdict = Some((ok, note));
                lg.error = err;
                lg.ts = store::now_secs();
                this.scan_space(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 一键清掉全部可清理缓存。**确认框在 `dialogs::clean`**，这里只管执行
    ///
    /// 与逐项 `clean_cache` 的差别只有两点：走 core 的 `delete_cache_dirs`
    /// （逐项独立，一项被占用不连坐），以及结论用 `CleanOutcome::summary`
    /// 一句话说完——成功与失败要同时看得见，只报成功等于骗人
    pub(crate) fn clean_cache_all(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.space.cleaning {
            return;
        }
        let Some(rep) = self.space.report.as_ref() else {
            return;
        };
        let names: Vec<String> = rep
            .dirs
            .iter()
            .filter(|d| d.is_cache && d.size > 0)
            .map(|d| d.name.clone())
            .collect();
        if names.is_empty() {
            return;
        }
        self.space.cleaning = true;
        self.space.error = None;
        cx.notify();
        let path = path.to_string();
        let log_key = path.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { space::delete_cache_dirs(&path, &names) })
                .await;
            this.update(cx, |this, cx| {
                this.space.cleaning = false;
                this.clean_dlg = None;
                let out = r;
                let note = out.summary();
                this.space.note = Some(note.clone());
                // 落进**这个仓库自己的**日志槽（`log_of`），不是 `app_log`——
                // 后者是给"横跨多个仓库的批量汇总"用的，概览页的日志卡读不到它
                let all_ok = out.failed.is_empty();
                let lg = this.log_of(&log_key);
                lg.lines.push(note.clone());
                // **没删成的逐条写进去**：只把成功的写上等于骗人。`target`
                // 被编译进程占用而删不掉时，用户看到的应该是"没删成 + 原因"
                for (n, why) in &out.failed {
                    lg.lines.push(format!("{n} 没删成：{why}"));
                }
                lg.verdict = Some((all_ok, note));
                lg.ts = store::now_secs();
                this.scan_space(cx);
            })
            .ok();
        })
        .detach();
    }

    /// 把一个目录加进 `.gitignore`。**只加这一个**——
    /// 一次写四条规则用户过后不会记得自己改过什么，
    /// 出了问题（某个目录确实该被跟踪）也难撤回
    pub(crate) fn ignore_dir(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        let name = name.to_string();
        let shown = name.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move { space::add_gitignore_entries(&path, &[name]) })
                .await;
            this.update(cx, |this, cx| {
                match r {
                    Ok(written) => {
                        let msg = if written.is_empty() {
                            format!("{shown} 已经在 .gitignore 里了")
                        } else {
                            format!("已写入 .gitignore：{}", written.join("、"))
                        };
                        this.space.note = Some(msg);
                    }
                    Err(e) => this.space.error = Some(e),
                }
                // 加了忽略规则之后未跟踪文件数会变，顺手重扫
                this.refresh_status(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Shell {
    // ---------- 同步设置 ----------

    /// 切仓库时重建这份草稿。**必须重建**：页签下标是全局的，
    /// 用户在 A 仓库改了一半没保存、切到 B 再切回来，不该看到 A 的半成品
    pub(crate) fn load_sync_cfg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let e = self
            .selected
            .as_deref()
            .and_then(|p| self.entry(p))
            .cloned();
        self.sync_cfg = sync_cfg_from(e.as_ref(), window, cx);
    }

    /// 保存同步设置。规范化在 core 那边做，界面不自己夹一遍。
    ///
    /// **返回 `true` = 存成了**，调用方据此决定要不要关弹窗。
    /// 存不成时错误写在 `sync_cfg.error` 上留在窗里——关掉窗用户就看不到
    /// 「时间格式不对」这句话了，而那正是他存不进去的原因
    pub(crate) fn save_sync_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(path) = self.selected.clone() else {
            return false;
        };
        let daily = self.sync_cfg.daily_input.read(cx).value().to_string();
        if !self.sync_cfg.daily_ok(&daily) {
            self.sync_cfg.error = Some("时间格式要对：24 小时制的 HH:MM".into());
            cx.notify();
            return false;
        }
        let mode = self.sync_cfg.to_auto_sync(&daily);
        let auto_pull = self.sync_cfg.auto_pull;
        match store::set_auto_sync(&path, &mode, auto_pull) {
            Ok(()) => {
                self.repos = repos::list_repos();
                self.sync_cfg.error = None;
                // 用刚落盘的值重建草稿：core 可能夹了值（比如时间格式），
                // 不重建的话界面上还摆着用户填的原样
                self.load_sync_cfg(window, cx);
                let desc = match self.entry(&path) {
                    Some(e) => auto_sync_text(e),
                    None => "已保存".to_string(),
                };
                window.push_notification(format!("定时同步：{desc}"), cx);
                true
            }
            Err(e) => {
                self.sync_cfg.error = Some(format!("保存失败：{e}"));
                cx.notify();
                false
            }
        }
    }
}

/// 从落盘配置建一份同步设置草稿。
///
/// **放在这里而不是 `state.rs`**：它要建一个 `Entity<InputState>`，
/// 而 `Entity::new` 要一个 `Context<Shell>`——`state.rs` 里的
/// `impl SyncCfg` 拿到的 `Context<Self>` 是 `Context<SyncCfg>`，建不出来
pub(crate) fn sync_cfg_from(
    e: Option<&qingbird_core::store::RepoEntry>,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> SyncCfg {
    let mut daily = String::from("09:00");
    let mut mode = SyncCfg::OFF;
    let mut auto_pull = false;
    if let Some(e) = e {
        auto_pull = e.auto_pull;
        if !e.daily_time.is_empty() {
            mode = SyncCfg::DAILY;
            daily = e.daily_time.clone();
        } else if e.auto_sync_mins > 0 {
            mode = SYNC_MINS_PRESETS
                .iter()
                .position(|m| *m == e.auto_sync_mins)
                .map(|i| i + 1)
                .unwrap_or(SyncCfg::OFF);
        }
    }
    let daily_input = cx.new(|cx| InputState::new(window, cx).placeholder("09:00"));
    if daily != "09:00" {
        let w: &mut Window = window;
        daily_input.update(cx, move |st, cx| st.set_value(daily, w, cx));
    }
    SyncCfg {
        mode,
        auto_pull,
        daily_input,
        error: None,
    }
}
