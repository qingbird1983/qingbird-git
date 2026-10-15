//! 后台操作编排：跑命令、三键同步、扫磁盘、扫档案、总览批量。
//!
//! 每个耗时操作都是「先记状态 → 起后台线程 → 回来更新 UI」，界面永远先画上一次结果。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::component::input::InputState;
use gpui_kit::component::WindowExt;
use gpui_kit::{AppContext, Context, SharedString, Task, Window};
use qingbird_core::fork;
use qingbird_core::git;
use qingbird_core::purge;
use qingbird_core::repos::{self};
use qingbird_core::run_cmd::{self, Slot};
use qingbird_core::scan_cache;
use qingbird_core::space;
use qingbird_core::stash;
use qingbird_core::store;
use qingbird_core::sync;
use qingbird_core::types::{OpResult, ProgressEvent};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::state::*;
use crate::widgets::auto_sync_text;

/// 进度搬运的轮询间隔：后台线程把 git 的事件攒在共享缓冲里，主线程每隔这一档
/// 取走一批、更新状态、`notify` 重绘。
///
/// ⚠️ **这一档只许有一个出处**（2026-10-10）：原先只有 `run_slot` 与拉取那两条
/// 循环各写一个字面 120，而**克隆弹窗压根没有这条循环**——事件全在后台缓冲里
/// 攒着，直到 `clone_repo` 整个返回才搬一次（那一刻对话框已经要关了）。
/// 用户看到的是"进度条一直在滑，文字永远停在准备中…"，百分比 / 网速 /
/// 直连还是代理**一个都上不了屏**。现在三条链路共用这一颗常量。
///
/// 120ms 是"看着连续、又不刷爆重绘"的一档：git 的进度行一秒刷十几条，
/// 每 120ms 搬一批 ≈ 一秒重绘八次。
pub(crate) const PROGRESS_POLL: Duration = Duration::from_millis(120);

/// 后台线程往共享缓冲里放的两种东西：输出一行，或者报一次进度。
///
/// 分成两种是因为进度**不上屏**：它只更新状态行与进度条。
/// 混成字符串塞进 `lines` 的话，"拉取"和"检测更新"两类操作在输出流里
/// 会长得一模一样，而它们的阶段差别正是用户想看的
#[derive(Clone)]
enum LogEvent {
    Line(String),
    /// `detail` 是传输量与网速（`已传输 1.21 MiB · 56.00 KiB/s`）。
    /// ⚠️ **它必须在这一层就有位置**——2026-10-05 之前这个变体只有
    /// `stage` 与 `percent`，于是 core 已经解析好的网速在这里被整条丢掉，
    /// 界面只剩一根光条。**core 一行都不用改，丢的是 GUI 这一侧**：
    /// `ProgressEvent` 三个字段一直都在（`stage` / `detail` / `percent`）
    Stage {
        stage: String,
        detail: String,
        percent: Option<f32>,
    },
    /// 阶段化升级的进度快照（阶段四 P0）。**只有 CLI 升级那条路产它**
    /// （`cli::upgrade::run_upgrade` 的 `on_progress`），落进
    /// `Shell::cli_upgrade` 而不是某个 `LogState`——五阶段管道要的是
    /// 五格状态，`LogState::stage` 那一格装不下（同一个属性两个出处
    /// 的项目学费，这里用"放进它自己的字段"避开）。
    ///
    /// **带 id 是为了防迟到快照**：每趟升级有自己的缓冲与轮询，收尾清掉
    /// `cli_upgrade` 之后用户又开了下一趟，上一趟轮询的最后一批可能才到——
    /// 对不上 id 就丢，不然新现场的管道上会闪一秒旧结论
    CliUpgrade {
        id: String,
        progress: qingbird_core::cli::UpgradeProgress,
    },
    /// 卸载执行的步骤快照（设计 §3.11.6，阶段四）。与 [`LogEvent::CliUpgrade`]
    /// 同一条理由进 `Shell::cli_uninstall`（步骤管道要的是 N 行状态，
    /// `LogState::stage` 那一格装不下），也同样**带 id 防迟到快照**
    CliUninstall {
        id: String,
        progress: qingbird_core::cli::execute::PlanProgress,
    },
}

/// UI 线程那条轮询：每 `PROGRESS_POLL` 把共享缓冲里的新行搬进日志状态，
/// 后台置 `done` 之后搬完最后一批就退。
///
/// **它是"什么时候能看到进度"的唯一出路**：轮询这一轮与收尾那一次都走这一颗
/// ——2026-10-10 那条缺陷（进度条滑、文字停在「准备中…」、百分比与网速一个都
/// 上不了屏）就是只有第二条路径。`run_slot` 与 `run_sync` 两条链共用它：
/// 那两段原来是**逐字相同的两份**（审查台账 §1 第三条，pwsh 逐行 `-ceq`
/// 实测 identical=True），判据（空 stage 不清、空 detail 不清）因此也只有这一份。
///
/// `sink` 决定搬进哪个日志槽（阶段三起 CLI 屏也有动作流，见 [`LogSink`]）——
/// **不按屏另起一套**：两套轮询迟早漂，而漂了的症状是"某一屏永远不动"
fn drain_log_until_done(
    cx: &mut Context<Shell>,
    buf: &Arc<Mutex<Vec<LogEvent>>>,
    done: &Arc<AtomicBool>,
    sink: LogSink,
) {
    let ui_buf = Arc::clone(buf);
    let ui_done = Arc::clone(done);
    cx.spawn(async move |this, cx| loop {
        cx.background_executor().timer(PROGRESS_POLL).await;
        let batch = {
            let mut g = ui_buf.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::take(&mut *g)
        };
        let finished = ui_done.load(Ordering::SeqCst);
        this.update(cx, |this, cx| {
            // 进度快照**不进日志槽**（它们进 `Shell::cli_upgrade` /
            // `cli_uninstall`）——先择出来，日志槽那一借一还在手里，
            // 两处都要写就得撞车（E0499）
            let mut ups: Vec<(String, qingbird_core::cli::UpgradeProgress)> = Vec::new();
            let mut unins: Vec<(String, qingbird_core::cli::execute::PlanProgress)> = Vec::new();
            {
                let lg = this.log_sink(&sink);
                for ev in batch {
                    match ev {
                        LogEvent::Line(l) => lg.lines.push(l),
                        LogEvent::Stage {
                            stage,
                            detail,
                            percent,
                        } => {
                            // 空 stage = core 没认出这一行（只有 git 原文），
                            // 不该把上一次认出来的阶段冲掉。`clone.rs` 的
                            // `drain_into` 早就是这么判的（那里有注释），
                            // 同一条链上两处判据得一样
                            if !stage.is_empty() {
                                lg.stage = Some(stage);
                            }
                            // 网速同理：`done.` / 纯计数行没有速度信息，
                            // 让它们把上一条的网速清掉 ⇒ 数字在闪
                            if !detail.is_empty() {
                                lg.detail = detail;
                            }
                            lg.percent = percent;
                        }
                        LogEvent::CliUpgrade { id, progress } => ups.push((id, progress)),
                        LogEvent::CliUninstall { id, progress } => unins.push((id, progress)),
                    }
                }
                if finished {
                    lg.running = false;
                }
            }
            for (id, progress) in ups {
                // 迟到的旧快照对不上 id 就丢（变体注释那条理由）
                if let Some(u) = this.cli_upgrade.as_mut() {
                    if u.pkg_id == id {
                        u.progress = progress;
                    }
                }
            }
            for (id, progress) in unins {
                if let Some(u) = this.cli_uninstall.as_mut() {
                    if u.pkg_id == id {
                        u.progress = progress;
                    }
                }
            }
            cx.notify();
        })
        .ok();
        if finished {
            break;
        }
    })
    .detach();
}

/// 后台任务收尾的那一程：等 `runner` 的结果，在 UI 线程上**先写结论**
/// （`verdict` 由调用方给），再走每条链都一样的收尾。
///
/// 两种操作的结论长相不同（`run_slot` 只有成败一句；`run_sync` 还有 ✓/✗ 行、
/// `used_proxy`、拉回了什么、`op_history` 与分叉态），但**结论之后要做的事**
/// 一模一样：停 `running`、清 `percent` 与 `detail`、重扫仓库、刷新状态。
/// 那一截以前在两处各抄一遍（台账 §1 第三条），而它是"收尾"的唯一住处——
/// 以后结束时要多做什么，改这里一处就够
fn settle_after<R: 'static>(
    cx: &mut Context<Shell>,
    runner: Task<R>,
    path: String,
    verdict: impl FnOnce(&mut Shell, &mut Context<Shell>, R) + 'static,
) {
    cx.spawn(async move |this, cx| {
        let result = runner.await;
        this.update(cx, |this, cx| {
            verdict(this, cx, result);
            let lg = this.log_of(&path);
            lg.running = false;
            lg.percent = None;
            // 网速跟着 percent 一起收：留着就是一句已经过时的谎话
            lg.detail.clear();
            // 跑完仓库状态可能变了（拉取/推送/变基改 ahead/behind、命令生成
            // 构建产物或改文件），重扫一遍刷新侧栏与徽章
            this.repos = repos::list_repos();
            this.refresh_status(cx);
        })
        .ok();
    })
    .detach();
}

impl Shell {
    // ---------- 跑命令 ----------

    pub(crate) fn run_slot(&mut self, slot: Slot, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        self.run_slot_at(&path, slot, cx);
    }

    /// `run_slot` 的那条链路，但**目标仓库由调用方给**。
    ///
    /// 为什么要多开这一个口（2026-10-09）：命令编辑弹窗的「启动」不能走
    /// `run_slot`——它跑的是**弹窗自己那条仓库**，而弹窗开着的时候选中项是可以被
    /// 切走的（侧栏点另一条不会关弹窗）。拿 `self.selected` 当目标就会变成
    /// 「保存了 A 的命令、起跑的是 B」。与 D78 同一条：**该由调用方给的东西
    /// 不要在函数里猜**
    pub(crate) fn run_slot_at(&mut self, path: &str, slot: Slot, cx: &mut Context<Self>) {
        let path = path.to_string();
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

        drain_log_until_done(cx, &buf, &done, LogSink::Repo(path.clone()));
        let fin_path = path.clone();
        settle_after(cx, runner, fin_path.clone(), move |this, _cx, result| {
            let lg = this.log_of(&fin_path);
            match result {
                Ok(_) => {
                    lg.error = None;
                    lg.verdict = Some((true, format!("{} 已完成", slot.default_label())));
                }
                Err(e) => lg.error = Some(e),
            }
        });
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
                        // 进度条只吃 stage/detail/percent，跟日志是不是原文无关
                        if !ev.stage.is_empty() || ev.percent.is_some() {
                            g.push(LogEvent::Stage {
                                stage: ev.stage.clone(),
                                detail: ev.detail.clone(),
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

        drain_log_until_done(cx, &buf, &done, LogSink::Repo(path.clone()));
        let fin_path = path.clone();
        settle_after(cx, runner, fin_path.clone(), move |this, _cx, result: OpResult| {
            {
                let lg = this.log_of(&fin_path);
                lg.lines.push(if result.ok {
                    format!("✓ {}", result.message)
                } else {
                    format!("✗ {}", result.message)
                });
                lg.verdict = Some((result.ok, result.message.clone()));
                lg.used_proxy = result.used_proxy.clone();
                // **这次拉回来什么**（提交数 / 增删行 / 文件数）。
                // core 在 `sync::pull_changes` 里算好、填进 `OpResult.changes`，
                // 而这一层以前**从来没读过那个字段**（D73 那一族：断点在字段声明
                // 与收尾这几行里，不在逻辑里——`cargo check` 零错、core 单测全绿、
                // 界面只是答不出"改了啥"）。
                // 只在成功时留：失败那几条路径上 core 填的就是 `None`，
                // 这里再判一次是防以后有人给失败分支补上 changes
                lg.pulled = if result.ok { result.changes.clone() } else { None };
                lg.pull_open = false;
            }
            // **落进 `op_history`（阶段 16）**：这一句是**跨会话**的。
            // 没有它的话重启一次，日志卡就写「还没有操作记录」——
            // 而昨天明明拉取过。用户问「这仓库上次更新了什么」，
            // 答得出来的地方只有这里
            this.record_op(&fin_path, op, result.ok, &result.message);
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
        });
    }
}

impl Shell {
    /// 停掉**眼前这一颗仓库**正在跑的操作（工具行那个「停止」按钮与 `Ctrl .` 共用这一个入口）。
    ///
    /// ⚠️ 这句从 32-8 起才成立。以前这里调的是进程级 `git::request_cancel()`，
    /// 注释写「core 那两个入口都是全局的——同一时刻只放一个忙操作——所以这里
    /// 不按仓库分：只要有活就停，活是谁的不影响结果」。**那句"不影响结果"只在全局锁
    /// 之下才对**：锁下沉到每仓库之后照抄，就是点 A 的停止把 B 正在跑的拉取一起杀掉，
    /// 而两边都正常返回、界面上看不出是谁干的。
    /// 现在取消标记按仓库带地址，而 `log()` 取的就是选中仓库的日志 ⇒ 两者天然对齐。
    ///
    /// 一键命令（`Op::Cmd`）那一支仍然交给 `run_cmd::stop_repo_cmd()`：它自己那套
    /// 「全应用只放一个后台命令」是它的语义，不是被 `OP_LOCK` 逼出来的，本轮不动。
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
        } else if let Some(path) = self.selected.as_deref() {
            git::request_cancel_for(path);
            Ok(())
        } else {
            // 到不了这里（`running` 为真就必然有选中仓库），但"不知道该停哪一颗"
            // 不许变成"那就全停"——那正是这次要拆掉的那件事
            Err("没选中仓库，不知道该停哪一颗".to_string())
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
    /// **README 与语言构成两类仓库都跑，活跃度 2026-10-08 起也两类都跑。**
    /// README 在两类仓库的「说明」页签里都有，读一个文件又不贵；
    /// 语言构成 2026-10-05 起是概览页的**语言技术栈段**，那也是两类都有
    /// （`repo_languages` 要遍历 `git ls-files` 的每个文件，但不派生网络请求，
    /// 一次 ls-files + 一轮 metadata 足够，第三方那屏也正是靠它出那张条）。
    ///
    /// ⚠️ 活跃度原来**只给第三方**，理由是"那条只留给第三方的「仓库档案」卡"。
    /// 这个动机今天不成立了：用户第⑦条点名要把原型那句
    /// 「近 30 天 11 天有提交 · 共 61 次」放到**头卡页签行右边**，而头卡是
    /// **两类仓库共用的一块**（本文件顶上就写着"两类仓库共用同一个头"）。
    /// 只给第三方 ⇒ 「我的仓库」那一屏那句话永远是空的。
    /// 代价是一条本地 `git log --since`（不派网络请求，与 `ls-files` 同量级），
    /// 而且它已经进了 `scan_cache`，第二次打开这个仓库直接读库
    pub(crate) fn scan_profile(&mut self, cx: &mut Context<Self>) {
        self.profile_seq += 1;
        self.profile_task = None;
        let Some(path) = self.selected.clone() else {
            self.profile = ProfileState::default();
            return;
        };
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
                    // 活跃度**两类仓库都扫**（2026-10-08 第⑦条：那句统计要落到
                    // 两类共用的头卡页签行右边）。原来 `if third` 那一臂留着，
                    // 「我的仓库」那一屏就永远只能显示空的
                    let activity = repos::commit_activity(&repo, PROFILE_ACTIVITY_DAYS);
                    (
                        repos::repo_languages(&repo),
                        activity,
                        // 说明文档这一组（阶段 18）：**候选表与每一份的正文
                        // 一次扫齐**。以前是"读一份主文档 + 问一次另一份是哪个
                        // 文件"，于是切换时还得再读一次盘——那道闪烁就是这么来的。
                        // 现在界面握着全部正文与整张表，切语言在内存里就能换完。
                        // 顺手也把文件名说准了：界面上要能讲清渲染的是
                        // README.md 还是 README.rst
                        {
                            let index = repos::readme_index(&repo);
                            let docs = repos::readme_docs(&repo);
                            // 候选表排最前的就是默认那一份
                            let file = index.first().map(|c| c.name.clone());
                            // 「另一个语言」那一份。**判据是刚认下的这个文件名**
                            let alt = file.as_deref().and_then(|f| repos::alt_of(&index, f));
                            let text = file
                                .as_deref()
                                .and_then(|f| docs.get(f))
                                .cloned()
                                .or_else(|| repos::read_readme_text(&repo));
                            (index, docs, file, text, alt)
                        },
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                if this.profile_seq != seq {
                    return; // 切了仓库 / 已有新一轮，这批过期
                }
                let (langs, activity, (index, docs, readme_file, readme_text, readme_alt)) = r;
                // 扫完这一次性转成 SharedString，之后每帧只是克隆一个引用计数
                let readme_docs: HashMap<String, SharedString> = docs
                    .into_iter()
                    .map(|(k, v)| (k, SharedString::from(v)))
                    .collect();
                let readme_text = readme_text.map(SharedString::from);
                // ⚠️ **用户正在看哪一份是他自己的选择，后台扫描不该把它顶掉**。
                // 扫完直接 new 一个 `ProfileState` 的话，用户刚点过去的那份英文
                // 会被这一轮的结果按回中文——又是一次自己跟自己打架的闪动
                let cur_file = this.profile.readme_file.clone();
                let (file, text) = match cur_file.as_deref().and_then(|f| readme_docs.get(f)) {
                    Some(kept) => (cur_file, Some(kept.clone())),
                    None => (readme_file, readme_text),
                };
                // 正文状态也换掉：用户切页签回来看到的是新仓库的文档，
                // 不是上一个仓库留在那儿的那份
                this.readme_state.update(cx, |st, cx| {
                    st.set_text(text.as_deref().unwrap_or_default(), cx)
                });
                this.profile = ProfileState {
                    langs,
                    activity,
                    readme_text: text,
                    readme_file: file,
                    readme_alt,
                    readme_error: None,
                    scanning: false,
                    scanned_at: store::now_secs(),
                    // 候选表与全部正文无条件跟着走：切换要用的就是这两样
                    readme_index: index,
                    readme_docs,
                    readme_loading: false,
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

    /// 并发跑一批同步操作：**最多 `sync::MAX_CONCURRENT_OPS` 路，完成一颗补一颗**。
    ///
    /// ⚠️ 这里原来是 `for path in paths { …spawn().await }`——**一句 `await`
    /// 就把 core 那把每仓库锁给绕回串行上了**。路线表 32-8 那句
    /// "串行不是必需，是被锁的形状逼出来的"只说对一半：**锁是一半，
    /// GUI 自己 await 是另一半**。只拆 core 那一半，批量照样一颗一颗跑，
    /// 而且没有任何一处会报错——这是本轮最容易"只改一半就收手"的地方。
    ///
    /// 为什么调度放在这一层、而不是"17 个任务一起发给 core 让它排队"：
    /// gpui 那个后台执行器是**共享线程池**，全堵在额度等待里会把扫描、
    /// 克隆进度这些别的活儿饿死。自己数着在跑几颗，池子里最多占 4 个，
    /// 同时也没有"一批里一颗挂死、整批等它"的队头阻塞。
    ///
    /// 拉取遇脏工作区**跳过并记日志、继续下一个**（不自动 stash——那是替用户
    /// 做决定），与旧版总览批量的约定一致。
    ///
    /// **不新增后端批量 command**：代理回退、分叉识别、脏工作区判定都已经
    /// 在 `sync::pull_repo` / `push_repo` / `check_update` 那三条路径里，
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
            queue: paths,
            running_now: Vec::new(),
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
        self.pump_batch(cx);
    }

    /// 把队列里能发的发出去，发到并发上限为止。
    ///
    /// 写成"一次取一颗"的循环，是为了让 `self.batch` 的可变借用每取一颗就结束一次：
    /// 下面 `status_of` / `title` / `log_of` 都要再借一次 `self`，
    /// 拿着长借用去调它们是**编译不过**的那种"看着一样"的写法
    fn pump_batch(&mut self, cx: &mut Context<Self>) {
        loop {
            // 取下一颗：并发位满了、队列空了、或这一批已经收尾 ⇒ 停。
            // "该不该发"那一问是纯函数 `batch_can_launch_more`（理由写在那颗函数上：
            // `Shell` 构造不出来，不抽出来"上限有没有生效"就一条用例都写不了）
            let path = {
                let Some(b) = self.batch.as_mut() else { break };
                if !crate::state::batch_can_launch_more(b.running, b.running_now.len(), b.queue.len()) {
                    break;
                }
                let Some(path) = b.queue.first().cloned() else { break };
                b.queue.remove(0);
                path
            };
            self.start_batch_item(path, cx);
        }
    }

    /// 发一颗出去（或就地结清一颗不用跑的）
    fn start_batch_item(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(kind) = self.batch.as_ref().map(|b| b.kind) else {
            return;
        };
        // 拉取对脏工作区让路。**只在拉取时判定**——检测不改工作区，
        // 推送推的是提交不是文件。
        // ⚠️ 它一秒都不跑，所以**不占并发位**：占了就等于把这一批的上限压低一颗
        if kind == BatchKind::Pull && self.status_of(&path).dirty {
            self.finish_batch_item(&path, None, cx);
            return;
        }
        if let Some(b) = self.batch.as_mut() {
            b.running_now.push(path.clone());
        }
        // 让侧栏那一行转起来：批量也是"点下去就有反馈"的操作，
        // 不标出来用户会以为卡住了
        {
            let lg = self.log_of(&path);
            lg.running = true;
            lg.op = Some(match kind {
                BatchKind::Pull => Op::Pull,
                BatchKind::Push => Op::Push,
                BatchKind::Check => Op::Check,
            });
            lg.ts = store::now_secs();
        }
        cx.notify();
        let p = path.clone();
        cx.spawn(async move |this, cx| {
            // 内层那个 `async move` 会把 `p` 搬走，而回报结果时还要用它 ⇒
            // 两条路各留一份，别指望编译器替你想到
            let outcome = {
                let p = p.clone();
                cx.background_executor()
                    .spawn(async move {
                        let r = match kind {
                            BatchKind::Pull => sync::pull_repo(&p, true, None),
                            BatchKind::Push => sync::push_repo(&p),
                            BatchKind::Check => sync::check_update(&p, None),
                        };
                        (r.ok, r.message)
                    })
            }
            .await;
            this.update(cx, |this, cx| this.finish_batch_item(&p, Some(outcome), cx))
                .ok();
        })
        .detach();
    }

    /// 一颗有了结果（跑完了，或被跳过）：写日志、结计数、腾出并发位，
    /// 然后要么补一颗、要么收尾。`None` = 跳过（工作区有改动，一个字节都没动）
    fn finish_batch_item(
        &mut self,
        path: &str,
        outcome: Option<(bool, String)>,
        cx: &mut Context<Self>,
    ) {
        let Some(kind) = self.batch.as_ref().map(|b| b.kind) else {
            return;
        };
        // 先算全局那一行：`log_of` 会把 `self` 整个可变借走，
        // 拿了它再去碰 `app_log` 是第二个可变借用
        let app_line = match &outcome {
            None => Some(format!("· {}：工作区有改动，已跳过", self.title(path))),
            Some((true, _)) => None,
            Some((false, msg)) => Some(format!("✗ {}：{msg}", self.title(path))),
        };
        if let Some(line) = app_line {
            self.app_log.lines.push(line);
        }
        {
            // 每一条都记进**那个仓库自己的**日志：失败原因在那里，汇总只报数字
            let lg = self.log_of(path);
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
        }
        // 批量也落进 `op_history`（阶段 16），**逐仓库各记一条**——
        // 这一批改了五个仓库，切到任何一个都得看得到它自己那条，
        // 而不是"上一个仓库的时间"串到这边来。
        // ⚠️ **跳过的不记**：工作区有改动时一个字节都没动，
        // 记一条"跳过"进历史等于把"上次成功同步"的时间抹掉，而那次成功仍然是真的
        if let Some((ok, msg)) = &outcome {
            let op = match kind {
                BatchKind::Pull => Op::Pull,
                BatchKind::Push => Op::Push,
                BatchKind::Check => Op::Check,
            };
            self.record_op(path, op, *ok, msg);
        }
        let finished = {
            let Some(b) = self.batch.as_mut() else {
                cx.notify();
                return;
            };
            b.running_now.retain(|p| p != path);
            b.done += 1;
            match &outcome {
                None => b.skipped += 1,
                Some((true, _)) => b.ok += 1,
                Some((false, _)) => b.failed += 1,
            }
            b.done >= b.paths.len()
        };
        if finished {
            self.finish_batch(cx);
        } else {
            // 腾出一个位子就补一颗。收尾与补位互斥：整批完了就不该再发
            self.pump_batch(cx);
        }
    }

    /// 整批跑完：算那句结论、落全局槽、重扫刷新，通知交 `render` 兑现
    fn finish_batch(&mut self, cx: &mut Context<Self>) {
        let (note, all_ok) = {
            let Some(b) = self.batch.as_mut() else {
                return;
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
            (note, b.failed == 0)
        };
        // 汇总落全局槽。这一批横跨多个仓库，记到"当前选中"名下会让人以为只动了一个
        self.app_log.lines.push(note.clone());
        self.app_log.verdict = Some((all_ok, note.clone()));
        self.app_log.ts = store::now_secs();
        // ⚠️ 走一次性标记，不走这里直接 `cx.update_window`：
        // 收尾这一句可能在主线程的**同步结清**里走到（最后一颗是脏跳过），
        // 那条路上窗口已被 take 走，`update_window` 会把闭包静默丢掉
        self.pending_batch_note = Some(note);
        // 拉取/推送改了 ahead/behind，重扫刷新侧栏与表格
        self.refresh_status(cx);
        cx.notify();
    }

    /// 状态条里那一格批量进度（跑批时覆盖「运行中 · X」）。
    ///
    /// **只给短句**：完整那句汇总带"切到该仓库确认后再拉"这样的解释，
    /// 塞进状态条会把中间那一格撑走、路径被挤出可视区。
    /// 完整结论在 `app_log` 与跑完时的通知里
    pub(crate) fn batch_note(&self) -> Option<String> {
        self.batch.as_ref().map(|b| {
            // 传的是**显示名**不是路径：状态条那一格就这么宽。
            // `running_now` 是并发之后新增的那一格——光有 `done/total`
            // 只回答"完了几个"，不回答"现在在跑谁"，而后者在并发之后
            // 再也不能从前者推出来
            let running: Vec<String> = b.running_now.iter().map(|p| self.title(p)).collect();
            batch_summary(
                b.kind,
                b.done,
                b.paths.len(),
                b.ok,
                b.skipped,
                b.failed,
                &running,
            )
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
                lg.detail.clear();
                if let Some(d) = this.commit_dlg.as_mut() {
                    d.running = false;
                    if !r.ok {
                        d.error = Some(r.message.clone());
                    }
                }
                if r.ok {
                    this.commit_dlg = None;
                }
                // 落进 `op_history`（阶段 16）。与 `run_sync` 同一处收尾
                this.record_op(&path, Op::Commit, r.ok, &r.message);
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

    /// 执行阶段 17 的勾选清理。**勾选框在 `dialogs::purge`**，这里只管执行
    ///
    /// 结论落进**这个仓库自己的**日志槽（`log_of`）——只写 `space.note` 那种
    /// 活在一页上的地方，用户切到概览页就看不到了，而概览页才是默认会去的
    /// 那屏。**逐条记成功与失败**：删了 3 个有 1 个被占用，只报"清完了"是骗人
    pub(crate) fn run_purge(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.purge_dlg.take() else {
            return;
        };
        // 17-4 的闸门在**执行这一层**也留一道：入口键不画只是"通常到不了"，
        // 而这一层动的是不可逆的东西
        if !self.selected_is_third() {
            return;
        }
        let untracked: Vec<String> = d
            .untracked
            .iter()
            .filter(|p| d.picked.contains(*p))
            .cloned()
            .collect();
        let tracked: Vec<String> = d
            .tracked
            .iter()
            .filter(|p| d.picked.contains(*p))
            .cloned()
            .collect();
        if untracked.is_empty() && tracked.is_empty() {
            let lg = self.log_of(&d.path);
            lg.lines.push("· 清理：没有勾选任何一项，什么都没动".into());
            lg.ts = store::now_secs();
            cx.notify();
            return;
        }
        let path = d.path.clone();
        cx.spawn(async move |this, cx| {
            let p = path.clone();
            let r = cx
                .background_executor()
                .spawn(async move { purge::purge_changes(&p, &untracked, &tracked) })
                .await;
            this.update(cx, |this, cx| {
                let lg = this.log_of(&path);
                let note = r.summary();
                for p in &r.deleted {
                    lg.lines.push(format!("✗ 已删除 {p}（未跟踪，无法撤销）"));
                }
                for p in &r.restored {
                    lg.lines.push(format!("✓ 已恢复 {p}（改回云端版本）"));
                }
                for p in &r.skipped {
                    lg.lines.push(format!("· 跳过 {p}：动手时它已经不是那个状态了"));
                }
                for (p, why) in &r.failed {
                    lg.lines.push(format!("✗ {p}：{why}"));
                }
                lg.verdict = Some((r.failed.is_empty(), note.clone()));
                lg.error = if r.failed.is_empty() {
                    None
                } else {
                    Some(format!("{} 项没成功", r.failed.len()))
                };
                lg.ts = store::now_secs();
                // 清理完工作区变了，侧栏徽标 / 总览「状态」列都靠它
                this.refresh_status(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 点「丢掉」：**先要一次确认**，这一步只把哪一条标成"等确认"。
    ///
    /// 存的是（序号 + 消息原文）两样，不是"那一条的下标"——见 `state::StashDialog`
    pub(crate) fn ask_stash_drop(&mut self, index: u32, expect: &str, cx: &mut Context<Self>) {
        let Some(d) = self.stash_dlg.as_mut() else {
            return;
        };
        d.confirm = Some((index, expect.to_string()));
        cx.notify();
    }

    /// 反悔。确认框收掉，清单一个字都不动
    pub(crate) fn cancel_stash_drop(&mut self, cx: &mut Context<Self>) {
        if let Some(d) = self.stash_dlg.as_mut() {
            d.confirm = None;
        }
        cx.notify();
    }

    /// 取回与丢掉**共用这一条**：两者只差"调哪个 core 函数"与那句动词。
    ///
    /// ⚠️ `destructive` 那一档**不可逆**——调用方（stash 对话框）必须已经让用户
    /// 点过「确认丢掉」才会传 true（32-2）
    ///
    /// ⚠️ 为什么不写成两份：这两份的其余每一行（防连点、后台跑、结论落哪个日志槽、
    /// 跑完重读清单、刷新状态）都必须**逐字相同**，其中"跑完重读清单"那一行漏掉
    /// 一份，另一份就会拿着旧序号继续让用户点——而 `drop` 之后序号整体前移。
    ///
    /// ⚠️ **为什么整段在后台线程**：`stash::apply_one` 要先拿操作锁，撞锁时它会
    /// 等最多十秒（`begin_user_op`）。在 UI 线程上等那十秒就是把整个界面冻住
    pub(crate) fn stash_op(&mut self, index: u32, expect: &str, destructive: bool, cx: &mut Context<Self>) {
        let Some(d) = self.stash_dlg.as_mut() else {
            return;
        };
        if d.busy {
            // 正在跑就不接第二下。**不给按钮加 `.loading(..)`**：那颗键自己就是
            // 这一步的执行者，而 D64 那条纪律是"终止手段不许用 loading"——
            // 这里同理，用 `disabled(busy)` 表达"这一下还没跑完"
            return;
        }
        d.busy = true;
        d.confirm = None;
        let path = d.path.clone();
        let expect = expect.to_string();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let p = path.clone();
            let r = cx
                .background_executor()
                .spawn(async move {
                    if destructive {
                        stash::drop_one(&p, index, &expect)
                    } else {
                        stash::apply_one(&p, index, &expect)
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                {
                    // 结论落进**这个仓库自己的**日志槽（`log_of`）——只写在对话框上，
                    // 关掉那一屏就什么痕迹都没有了。
                    // ⚠️ 这一行**不再自己写动词**：`r.message` 自己就是
                    // 「已取回…」/「已丢掉…」，前面再拼一个"取回"就成了
                    // 「取回收起来的改动：已取回…」——帧上实测第一版就是这样，
                    // 一句话说两遍（阶段 16 那句「拉取成功 · 拉取成功」的同款）
                    let lg = this.log_of(&path);
                    lg.lines.push(format!("· 收起来的改动：{}", r.message));
                    lg.verdict = Some((r.ok, r.message.clone()));
                    lg.ts = store::now_secs();
                }
                // **重读清单**：丢掉之后序号全变了，旧的那份一张都不能留
                if let Some(d) = this.stash_dlg.as_mut() {
                    d.busy = false;
                    d.entries = stash::list(&path);
                }
                // 工作区被改过了：感叹号上那个数、侧栏徽标、总览「状态」列都要跟上。
                // 只刷这一个仓库（`refresh_one`），不必整表重扫
                this.refresh_one(path.clone(), cx);
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

impl Shell {
    /// 扫一遍本机的 CLI 工具（阶段一：只读）。
    ///
    /// 与 `scan_space` / `scan_profile` 同一套路数：**先把库里的那一份灌上屏，
    /// 再起后台任务扫真的**。入口有三处——导航轨进 CLI 屏（`go`）、
    /// CLI 屏左栏底部「重新扫描」、⌘K「重新扫描 CLI 工具」，都走这一颗。
    ///
    /// 代数守卫（`cli_seq`）与另外几处扫描一字不差：**先自增再取号**，
    /// 写反了守卫永远对不上，闭包静默早退、`scanning` 永远落不回去。
    pub(crate) fn scan_cli(&mut self, cx: &mut Context<Self>) {
        self.cli_seq += 1;
        let seq = self.cli_seq;
        self.cli_task = None;

        // 先上屏缓存。重扫时它会被后台结果替换，但**首屏不空、也不闪**
        if let Some(cached) = qingbird_core::cli::load_cached() {
            self.cli.packages = cached.packages;
            // 共享缓存与**每颗工具的数据目录**（连上次算好的体积）也一起上屏
            self.cli.shared = cached.shared;
            self.cli.scanned_at = cached.scanned_at;
        }
        // 图标也先上屏缓存（**与上面那份包清单同一套语言**：先渲染、后台再刷）。
        // 它只是一次 `read_dir`，不碰 Windows API、不 stat exe ⇒ 首屏不因此变慢。
        // 取不到目录（环境里没有 APPDATA）就是空表：图标全部走终字符块
        let icon_dir = qingbird_core::store::icons_dir();
        self.cli.icons = icon_dir
            .as_deref()
            .map(|d| qingbird_core::cli::icon::cached_icons(&self.cli.packages, d))
            .unwrap_or_default();
        self.cli.scanning = true;
        self.cli.errors.clear();
        self.cli_keep_selection();
        cx.notify();
        // 缓存里那份体积先摆上；共享缓存那一批**重新量**（见 `spawn_dir_sizes`）
        self.spawn_dir_sizes(cx, true);

        self.cli_task = Some(cx.spawn(async move |this, cx| {
            let report = cx
                .background_executor()
                .spawn(async move { qingbird_core::cli::scan() })
                .await;
            let mut fresh = report.packages.clone();
            let mut shared = report.shared.clone();
            let scanned_at = report.scanned_at;
            let landed = this
                .update(cx, |this, cx| {
                    if this.cli_seq != seq {
                        return false; // 已有新一轮，这批过期
                    }
                    // 体积接力：路径与类别都对上就把上一份算好的接回来
                    // （不然每重扫一次，屏上就齐刷刷闪一遍「计算中…」）。
                    // 后台那一趟再逐条重算——**接力不等于不重算**
                    qingbird_core::cli::dirs::carry_sizes(&mut fresh, &this.cli.packages);
                    qingbird_core::cli::dirs::carry_dir_sizes(&mut shared, &this.cli.shared);
                    this.cli.scanning = false;
                    this.cli.errors = report.errors.clone();
                    this.cli.scanned_at = scanned_at;
                    this.cli.packages = fresh;
                    this.cli.shared = shared;
                    this.cli_keep_selection();
                    // 落库失败不报：缓存是加速器不是依赖（core 那条约定）
                    qingbird_core::cli::save_cached(
                        &this.cli.packages,
                        &this.cli.shared,
                        scanned_at,
                    );
                    // 查更新**单独派发**（设计 §10.1「先渲染、后台再刷」）：
                    // 它要走网络（`npm outdated`），压进这一趟就是"进屏先等几秒"。
                    // 合并按 id 对，期间又重扫了也对不坏（见 `check_cli_updates`）
                    this.check_cli_updates(cx);
                    // 发行说明同一条纪律（阶段四 #4）：`winget show` 要走网络，
                    // 且只有 winget 给得出（npm 无可靠 changelog ⇒ 不查不画）
                    this.spawn_release_notes(cx);
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !landed {
                return;
            }

            // 数据目录的体积：扫完之后那一条通道（共享缓存那一批每次都重量）
            this.update(cx, |this, cx| this.spawn_dir_sizes(cx, false))
                .ok();

            // 图标提取：与扫描**同批派发**（设计 §10.1），**一颗一颗来、提完一颗
            // 就 notify 一次** —— 行内先画终字符块，好了换图，绝不阻塞首屏。
            // ⚠️ 这里与 `dialogs/clone.rs` 那条进度搬运是同一条纪律（D73 第三型）：
            // 后台跑着而**没有人在搬**，界面上就是"永远停在准备中"。
            let Some(dir) = icon_dir else {
                return;
            };
            for pkg in report.packages.iter() {
                let id = pkg.id.clone();
                // 提起图来的那条路要拿走一个**独立的** `CliPackage`（后台任务里用）
                let pkg = pkg.clone();
                // 每一轮都克隆一次：`dir` 要搬进那条后台任务里，
                // 而它下一轮还得用（搬走的是克隆，外面这颗一直是它的出处）
                let dir = dir.clone();
                let got = cx
                    .background_executor()
                    .spawn(async move { qingbird_core::cli::icon::ensure(&pkg, &dir) })
                    .await;
                let still = this
                    .update(cx, |this, cx| {
                        if this.cli_seq != seq {
                            return false;
                        }
                        // 提到了就挂上；**没提到要把旧的那条摘掉**——
                        // exe 换了版又变成没有图标时，留着旧图就是一句假话
                        match got {
                            Some(path) => {
                                this.cli.icons.insert(id.clone(), path);
                            }
                            None => {
                                this.cli.icons.remove(&id);
                            }
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !still {
                    return;
                }
            }
        }));
    }

    /// 把**还没算过体积**的数据目录在后台一条一条量出来（算完一条 notify 一次）。
    ///
    /// 两个口子由 `all_shared` 分开：
    /// * **共享缓存**那一批 —— 每次进屏都重量。它是整屏的头号数字（本机 npm 缓存
    ///   18.8 GB），装一个包它就变了；拿缓存里那份顶到底，界面上就是一句假话。
    /// * **每颗工具**那一批 —— 只量 `size` 还是 `None` 的（设计 §4 那个
    ///   `Option<u64>` 说的"惰性"）：第一次量完就随缓存接力，之后进屏不再走盘。
    ///
    /// 守卫与扫描共用一颗 `cli_seq`：重扫一次这一趟就作废（不写回、也不再往下跑）。
    ///
    /// ⚠️ 句柄**必须存进 `cli_size_task`**：gpui 的 `Task` 是 **drop 即取消**，
    /// 扔了等于这条通道当场没了（症状是那几条永远停在「计算中…」）
    pub(crate) fn spawn_dir_sizes(&mut self, cx: &mut Context<Self>, all_shared: bool) {
        let seq = self.cli_seq;
        // 要量的清单**现在**取定（路径 + 属于哪颗包）：后台只认这两个数回来写，
        // 期间列表变了（重扫）由 `seq` 挡住
        let mut targets: Vec<(Option<String>, String)> = Vec::new();
        for d in self.cli.shared.iter() {
            if all_shared || d.size.is_none() {
                targets.push((None, d.path.clone()));
            }
        }
        let sel = self.cli.selected.clone();
        for p in self
            .cli
            .packages
            .iter()
            .filter(|p| Some(&p.id) == sel.as_ref())
        {
            for d in p.dirs.iter().filter(|d| d.size.is_none()) {
                targets.push((Some(p.id.clone()), d.path.clone()));
            }
            // 程序本体（§3.11.8 #5）：④ 卡构成条的"程序"段，量体同样
            // 惰性——install_path 在、还没量过才派。**只给选中包**：
            // 量本体与量目录一样是走盘活，清单里几十颗全量一遍就是
            // "进屏先等几秒"（§3.11.4 那条纪律的反面）
            if let (Some(path), None) = (&p.install_path, p.program_size) {
                targets.push((Some(p.id.clone()), path.clone()));
            }
        }
        if targets.is_empty() {
            return;
        }

        // 每个目标一条后台任务（**不是串行**）。实测本机：npm 缓存 9.7 万文件、
        // cargo registry 12.4 万、pnpm store 8.9 万，串行量一遍 ~16 秒；而
        // `background_executor` 是多线程池 ⇒ `spawn` 一调用就把活丢给各自的线程，
        // 并发已经发生，后面逐个 `await` 只是**逐个收结果**（不是逐个干活），
        // wall time 落到**最慢那一条**（~9 秒），快的那些早把数摆上屏了。
        // ⚠️ 这里曾经写成"一个 async 块里 for 一圈 await"（真串行），
        // 界面 16 秒不出一行数字，看着像"没在搬"（D73 那一族的反面）
        let mut tasks: Vec<(Task<u64>, Option<String>, String)> = Vec::new();
        for (pkg, path) in targets {
            let probe = std::path::PathBuf::from(&path);
            let task = cx
                .background_executor()
                .spawn(async move { qingbird_core::cli::dirs::size_of(&probe) });
            tasks.push((task, pkg, path));
        }

        self.cli_size_task = Some(cx.spawn(async move |this, cx| {
            for (task, pkg, path) in tasks {
                let size = task.await;
                let written = this
                    .update(cx, |this, cx| {
                        if this.cli_seq != seq {
                            return false;
                        }
                        let hit = match &pkg {
                            None => this
                                .cli
                                .shared
                                .iter_mut()
                                .find(|d| d.path == path)
                                .map(|d| d.size = Some(size)),
                            Some(id) => this
                                .cli
                                .packages
                                .iter_mut()
                                .find(|p| &p.id == id)
                                .and_then(|p| {
                                    // 程序本体那条目标对准 `program_size`，
                                    // 目录条目对准 `dirs`——**先判本体**：
                                    // install_path 与某个数据目录路径相同
                                    // 的场合（罕见但允许），写错格子比
                                    // 少一格更难查
                                    if p.install_path.as_deref() == Some(path.as_str()) {
                                        p.program_size = Some(size);
                                        return Some(());
                                    }
                                    p.dirs
                                        .iter_mut()
                                        .find(|d| d.path == path)
                                        .map(|d| d.size = Some(size))
                                }),
                        };
                        // `hit` 现在是"对上了哪个格子"的信号（本体 / 目录 /
                        // 共享缓存各自写各自的），不是格子本身——写格子
                        // 已经在 match 里干完了
                        if hit.is_some() {
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !written {
                    return;
                }
            }
            // 体积量完**再落一次库**（第一次落库时体积还没算出来）
            this.update(cx, |this, _cx| {
                if this.cli_seq != seq {
                    return;
                }
                qingbird_core::cli::save_cached(
                    &this.cli.packages,
                    &this.cli.shared,
                    this.cli.scanned_at,
                );
            })
            .ok();
        }));
    }

    /// 清单换了之后，把选中项收到一个**还存在于清单里**的 id 上。
    /// 两条：选中的那个还在 ⇒ 一个字段都不动（重扫不该把用户的选择抖掉）；
    /// 不在（或压根没选）⇒ 落到第一个。**别让右栏空着**——"扫到了却点不开"
    /// 比"没扫到"更难懂。
    pub(crate) fn cli_keep_selection(&mut self) {
        let still_there = self
            .cli
            .selected
            .as_deref()
            .is_some_and(|id| self.cli.packages.iter().any(|p| p.id == id));
        if !still_there {
            self.cli.selected = self.cli.packages.first().map(|p| p.id.clone());
        }
    }

    // ---------- CLI 工具管理（阶段三） ----------

    /// 此刻有没有一个 CLI 操作在跑（卸载 / 升级 / 清理都算）。
    ///
    /// **设计 §11 串行**：同一时刻只放一个。界面侧据此禁掉所有入口键
    /// （详情卡那两颗、汇总条的「清理」、⌘K 那两条）——不是只禁正在忙的那一颗：
    /// 两颗一起跑会在同一个 `npm` 上撞车（一个卸一个升），而用户点的是两颗键
    pub(crate) fn cli_busy_any(&self) -> bool {
        !self.cli.busy.is_empty() || !self.cli.shared_busy.is_empty()
    }

    /// 查一遍哪些包可更新（阶段三）。**由 `scan_cli` 落地时顺手派发**——
    /// 设计 §10.1「先渲染、后台再刷」的同一套语言：查更新要走网络
    /// （`npm outdated`），压进扫描那一趟就是"进屏先等几秒"。
    ///
    /// 合并是 core 的纯函数（按 id 对）：期间用户又重扫了一次也对不坏——
    /// 对不上的条目原样不动，**不按名字猜一个写上**（那会多一个假徽标）
    pub(crate) fn check_cli_updates(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let ups = cx
                .background_executor()
                .spawn(async move { qingbird_core::cli::check_updates() })
                .await;
            this.update(cx, |this, cx| {
                qingbird_core::cli::merge_updates(&mut this.cli.packages, &ups);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 抓可更新包的发行说明（阶段四 #4，§3.11.1 就绪态"变更三统计 +
    /// 发布说明一行"）。**只有 winget 给得出**（`winget show` 带"发行说明"
    /// 段；npm 没有可靠的 per-version changelog ⇒ 不查、界面也不画）。
    ///
    /// - 与 [`check_cli_updates`] 同一条纪律：要走网络，扫描落地后**单独派发**
    /// - 只查 **winget 且有 `latest` 且还没抓过** 的包——没新版就没有
    ///   "变更"可言，抓了也没地方显示
    /// - 每颗一条后台任务（与 `spawn_dir_sizes` 同一套并发写法），收一条
    ///   写一条；抓失败（`Err`）就留着 `None`——界面上那行不出现，比
    ///   编一段"没有变更说明"出来诚实
    fn spawn_release_notes(&mut self, cx: &mut Context<Self>) {
        let targets: Vec<String> = self
            .cli
            .packages
            .iter()
            .filter(|p| {
                p.provider == qingbird_core::cli::ProviderId::Winget
                    && p.latest.is_some()
                    && p.release_notes.is_none()
            })
            .map(|p| p.id.clone())
            .collect();
        if targets.is_empty() {
            return;
        }
        let mut tasks: Vec<(Task<qingbird_core::cli::ReleaseNotes>, String)> = Vec::new();
        for id in targets {
            let task = cx.background_executor().spawn({
                let id = id.clone();
                async move {
                    // Err（show 没跑成）与 None（包没有说明段）在界面上是同一句话：
                    // 那行不出现
                    qingbird_core::cli::release_notes(&id)
                        .ok()
                        .flatten()
                        .unwrap_or_default()
                }
            });
            tasks.push((task, id));
        }
        cx.spawn(async move |this, cx| {
            for (task, id) in tasks {
                let notes = task.await;
                this.update(cx, |this, cx| {
                    // **按 id 对**，不按位置：网络那几秒里后台可能重扫，
                    // 清单会换序（`spawn_dir_sizes` 同一条课）
                    if let Some(p) = this.cli.packages.iter_mut().find(|p| p.id == id) {
                        // 默认值（全空）不写——抓回来的"没有"就留 None，
                        // 界面那一行不出现
                        if !notes.summary.is_empty() {
                            p.release_notes = Some(notes);
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 跑一串步骤（**plain 路**：⌘K 批量升级 / 共享缓存清理确认框调它）。
    ///
    /// 三步：① 计划预览**先写进 `cli_log`**（执行前用户必须看得见将要发生什么）
    /// ② 后台 `run_steps`，每步一行经共享缓冲搬进日志（与 `run_slot` 同一套管道）
    /// ③ 收尾：清忙、写结论、收框 + 一句通知（render 兑现）、刷新数据。
    ///
    /// 与另两条阶段化链路（[`spawn_cli_upgrade`] / [`spawn_cli_uninstall_run`]）
    /// 的分工见各自文档；**还给 plain 路的就这两种**——卸载 2026-10-15 起
    /// 有了步骤管道（设计 §3.11.6），不再走这儿
    fn spawn_cli_run(
        &mut self,
        cx: &mut Context<Self>,
        summary: String,
        steps: Vec<qingbird_core::cli::Step>,
        stop_on_command_failure: bool,
        after: CliAfter,
    ) {
        if steps.is_empty() {
            // 结构上到不了（调用方都先判过 `steps.is_empty()`）——到了就当
            // 什么都没发生，不开一个空执行
            let lg = &mut self.cli_log;
            lg.lines.push(format!("· {summary}：没有可执行的步骤"));
            lg.running = false;
            cx.notify();
            return;
        }

        // ① 预览。每步一行等宽命令——**这是用户点确认之前看到的同一份**
        // （对话框里那段预览），执行后再原样进日志，两边不会对不上
        let lg = &mut self.cli_log;
        lg.lines.push(format!("· {summary}"));
        for s in &steps {
            lg.lines.push(format!("  {}", s.preview()));
        }
        lg.running = true;
        lg.error = None;
        lg.verdict = None;
        lg.ts = store::now_secs();
        cx.notify();

        // ② 后台跑。每步结论**同时**进两处：`cli_log`（屏上看得见）与
        // `diag::note`（release 无 stderr，app.log 是显式捕获通道，设计 §11）
        let buf: Arc<Mutex<Vec<LogEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));
        let sink_buf = Arc::clone(&buf);
        let sink_done = Arc::clone(&done);
        let runner = cx.background_executor().spawn(async move {
            let on_step = move |o: &qingbird_core::cli::execute::StepOutcome| {
                crate::diag::note(&o.line());
                if let Ok(mut g) = sink_buf.lock() {
                    g.push(LogEvent::Line(o.line()));
                }
            };
            let ok = qingbird_core::cli::execute::run_steps(&steps, stop_on_command_failure, on_step);
            sink_done.store(true, Ordering::SeqCst);
            ok
        });
        drain_log_until_done(cx, &buf, &done, LogSink::Cli);

        // ③ 收尾
        cx.spawn(async move |this, cx| {
            let ok = runner.await;
            this.update(cx, |this, cx| {
                // 同一时刻只放一个 ⇒ 两个忙集一起清（不会误伤别的操作）
                this.cli.busy.clear();
                this.cli.shared_busy.clear();
                let verdict = if ok {
                    format!("{summary}：完成")
                } else {
                    format!("{summary}：有步骤没成功，详见上面")
                };
                let lg = &mut this.cli_log;
                lg.running = false;
                lg.verdict = Some((ok, verdict.clone()));
                lg.ts = store::now_secs();
                // 收框 + 一句结论。通知必须弹：用户可能已经切到别的屏，
                // 而日志区在 CLI 屏里——不弹一句他就此不知道结果
                if this.cli_audit.take().is_some()
                    || this.cli_clean_dlg.take().is_some()
                {
                    this.pending_cli_dlg_close = true;
                }
                this.pending_cli_note = Some(verdict);
                match after {
                    // 包清单变了（卸掉一颗 / 升了一颗）：整屏重扫
                    // （`scan_cli` 落地时会顺手再查一遍更新）
                    CliAfter::Rescan => this.scan_cli(cx),
                    // 只清了共享缓存：目录还在、内容没了 ⇒ 把那一批体积重量
                    CliAfter::RemeasureShared => this.spawn_dir_sizes(cx, true),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 跑一次**阶段化升级**（阶段四 P0）：② 卡 run 态的五阶段管道 +
    /// 进度带 + 「取消升级」都靠它供血。
    ///
    /// 与 [`spawn_cli_run`]（plain 路：⌘K 批量升级 / 共享缓存清理还在用）
    /// 的分工：那条每**步**回调一次（一步一结论），这条每**行**、每次
    /// **阶段切换**都回调——升级的五阶段管道（下载 → 校验 → 替换 …）
    /// 是阶段内的事件，plain 路那根管道看不见。卸载 2026-10-15 起走
    /// [`spawn_cli_uninstall_run`]（步骤管道，同一族机制）
    ///
    /// 三步与 plain 路同构：① 预览行进日志 + 起 run 态现场（初始进度
    /// 五格全「等待」+ 取消旗号）→ ② 后台 `cli::run_upgrade`，行进共享
    /// 缓冲、阶段快照走 `LogEvent::CliUpgrade` → ③ 收尾：清忙、清 run
    /// 态、写结论、一句通知、重扫（与 plain 路逐条同一套）。
    ///
    /// ⚠️ **批量升级（⌘K「升级全部」）不走这条**：管道是"一颗包的
    /// 五阶段"，一批包的阶段化是另一件事（roadmap 阶段四后续项），
    /// 在那之前批量保持 plain 路——给不出管道的那颗不画管道
    fn spawn_cli_upgrade(
        &mut self,
        cx: &mut Context<Self>,
        summary: String,
        pkg: &qingbird_core::cli::CliPackage,
        step: &qingbird_core::cli::Step,
    ) {
        let id = pkg.id.clone();
        // ① 预览：升级命令行先进日志——执行前用户必须看得见将要发生什么
        // （与 plain 路同一判据；对话框里看过的那份这里原样再放一遍）
        let lg = &mut self.cli_log;
        lg.lines.push(format!("· {summary}"));
        lg.lines.push(format!("  {}", step.preview()));
        lg.running = true;
        lg.error = None;
        lg.verdict = None;
        lg.ts = store::now_secs();
        cx.notify();

        // ② run 态现场。取消旗号在这里建一份、后台拿一份克隆——两边看得
        // 是同一个字节（core 侧 `CancelToken`）
        let cancel = qingbird_core::cli::CancelToken::new();
        self.cli_upgrade = Some(CliUpgradeRun {
            pkg_id: id.clone(),
            progress: qingbird_core::cli::UpgradeProgress::new(),
            cancel: cancel.clone(),
        });
        self.cli.busy.insert(id.clone());

        // ③ 后台跑。行与快照进同一颗缓冲（与 plain 路同一套轮询管道）
        let buf: Arc<Mutex<Vec<LogEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));
        let sink_buf = Arc::clone(&buf);
        let sink_done = Arc::clone(&done);
        let run_pkg = pkg.clone();
        let run_step = step.clone();
        let run_id = id.clone();
        let runner = cx.background_executor().spawn(async move {
            let line_buf = Arc::clone(&sink_buf);
            let prog_buf = Arc::clone(&sink_buf);
            let out = qingbird_core::cli::run_upgrade(
                &run_pkg,
                &run_step,
                &cancel,
                move |line| {
                    // 每行进两处：屏上看得见（`cli_log`）与 app.log
                    // （release 无 stderr，显式捕获通道，设计 §11）
                    crate::diag::note(line);
                    if let Ok(mut g) = line_buf.lock() {
                        g.push(LogEvent::Line(line.to_string()));
                    }
                },
                move |p| {
                    if let Ok(mut g) = prog_buf.lock() {
                        g.push(LogEvent::CliUpgrade {
                            id: run_id.clone(),
                            progress: p.clone(),
                        });
                    }
                },
            );
            // 置完成旗号：轮询那条循环搬完最后一批就退（plain 路同一颗）
            sink_done.store(true, Ordering::SeqCst);
            out
        });
        drain_log_until_done(cx, &buf, &done, LogSink::Cli);

        // ④ 收尾。与 plain 路同一套动作，只多两件：清 run 态现场、
        // 结论按 取消 / 完成 / 没成功 三种终局分别说
        let name = pkg.name.clone();
        cx.spawn(async move |this, cx| {
            let out = runner.await;
            this.update(cx, |this, cx| {
                // 同一时刻只放一个 ⇒ 两个忙集一起清（不会误伤别的操作）
                this.cli.busy.clear();
                this.cli.shared_busy.clear();
                // run 态现场收掉：② 卡回 ready 态。**最终那份进度不进
                // 界面**——结论在 ③ 终端日志卡与那一条通知里（一页开着
                // 却确认不了的执行态，比一句通知难懂）
                this.cli_upgrade = None;
                let verdict = if out.cancelled {
                    format!("升级 {name}：已取消")
                } else if out.ok {
                    format!("升级 {name}：完成")
                } else {
                    match &out.error {
                        Some(e) => format!("升级 {name}：{e}"),
                        None => format!("升级 {name}：有步骤没成功，详见上面"),
                    }
                };
                let lg = &mut this.cli_log;
                lg.running = false;
                lg.verdict = Some((out.ok, verdict.clone()));
                lg.ts = store::now_secs();
                // 通知必须弹：用户可能已经切到别的屏，而日志区在 CLI
                // 屏里——不弹一句他就此不知道结果（plain 路同一条）
                this.pending_cli_note = Some(verdict);
                // 包清单变了（升了一颗）：整屏重扫（`scan_cli` 落地时
                // 会顺手再查一遍更新）
                this.scan_cli(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 点「取消升级」（② 卡 run 态那颗键）。**只置旗号**：杀进程、
    /// 收尾全在 core 的 runner 与上面的收尾帧里做，这里不碰 busy 也
    /// 不清 `cli_upgrade`——清了管道就没了，而用户要看见「已取消」
    /// 落在哪一段上
    pub(crate) fn cancel_cli_upgrade(&mut self, cx: &mut Context<Self>) {
        if let Some(u) = self.cli_upgrade.as_ref() {
            u.cancel.cancel();
        }
        cx.notify();
    }

    /// 点「取消卸载」（② 卡卸载执行态那颗键）。**只置旗号**：杀进程、
    /// 收尾全在 core 的 runner 与收尾帧里做（同 [`cancel_cli_upgrade`]）——
    /// 用户要看见「已取消」落在哪一步上
    pub(crate) fn cancel_cli_uninstall(&mut self, cx: &mut Context<Self>) {
        if let Some(u) = self.cli_uninstall.as_ref() {
            u.cancel.cancel();
        }
        cx.notify();
    }

    /// 跑一次**卸载计划**（② 卡卸载执行态，设计 §3.11.6「七步管道 +
    /// 取消」）：步骤管道与「取消卸载」都靠它供血。
    ///
    /// 与 [`spawn_cli_run`]（plain 路：⌘K 批量升级 / 共享缓存清理还在用）
    /// 的分工：那条每**步**回调一次结论；这条每**行**、每次**步骤状态
    /// 切换**都回调（core `execute::run_plan_live`）——步骤管道要的是
    /// "这一步正在进行"，plain 路那根管道看不见步内的事。
    ///
    /// 三步与 plain 路同构：① 预览行进日志 + 起执行态现场（初始快照
    /// 全「等待」+ 取消旗号）→ ② 后台 `run_plan_live`（行进共享缓冲、
    /// 步骤快照走 `LogEvent::CliUninstall`）→ ③ 收尾：清忙、清执行态、
    /// 写结论、一句通知、重扫。
    ///
    /// ⚠️ **结论按三种终局分别说**（取消 / 完成 / 有步骤没成功），清理
    /// 真释放了字节才带那句——`out.freed` 是 core 加出来的合计，不是
    /// 审计页那个"预计"
    fn spawn_cli_uninstall_run(
        &mut self,
        cx: &mut Context<Self>,
        pkg: &qingbird_core::cli::CliPackage,
        plan: &qingbird_core::cli::OpPlan,
    ) {
        let id = pkg.id.clone();
        // ① 预览：每步一行等宽命令——**这是用户点确认之前看到的同一份**
        // （审计页里那段预览），执行后再原样进日志，两边不会对不上
        let lg = &mut self.cli_log;
        lg.lines.push(format!("· {}", plan.summary));
        for s in &plan.steps {
            lg.lines.push(format!("  {}", s.preview()));
        }
        lg.running = true;
        lg.error = None;
        lg.verdict = None;
        lg.ts = store::now_secs();
        cx.notify();

        // ② 执行态现场。取消旗号建一份、后台拿一份克隆——两边看得是同一个字节
        let cancel = qingbird_core::cli::CancelToken::new();
        self.cli_uninstall = Some(CliUninstallRun {
            pkg_id: id.clone(),
            progress: qingbird_core::cli::execute::PlanProgress::new(&plan.steps),
            cancel: cancel.clone(),
        });
        self.cli.busy.insert(id.clone());

        // ③ 后台跑。行与快照进同一颗缓冲（与 plain 路 / 升级同一条轮询管道）
        let buf: Arc<Mutex<Vec<LogEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));
        let sink_buf = Arc::clone(&buf);
        let sink_done = Arc::clone(&done);
        let run_plan = plan.clone();
        let run_id = id.clone();
        let runner = cx.background_executor().spawn(async move {
            let line_buf = Arc::clone(&sink_buf);
            let prog_buf = Arc::clone(&sink_buf);
            let out = qingbird_core::cli::execute::run_plan_live(
                &run_plan,
                &cancel,
                move |line| {
                    // 每行进两处：屏上看得见（`cli_log`）与 app.log
                    // （release 无 stderr，显式捕获通道，设计 §11）
                    crate::diag::note(line);
                    if let Ok(mut g) = line_buf.lock() {
                        g.push(LogEvent::Line(line.to_string()));
                    }
                },
                move |p| {
                    if let Ok(mut g) = prog_buf.lock() {
                        g.push(LogEvent::CliUninstall {
                            id: run_id.clone(),
                            progress: p.clone(),
                        });
                    }
                },
            );
            // 置完成旗号：轮询那条循环搬完最后一批就退（plain 路同一颗）
            sink_done.store(true, Ordering::SeqCst);
            out
        });
        drain_log_until_done(cx, &buf, &done, LogSink::Cli);

        // ④ 收尾。与 plain 路同一套动作，只多两件：清执行态现场、
        // 结论按三种终局分别说
        let summary = plan.summary.clone();
        cx.spawn(async move |this, cx| {
            let out = runner.await;
            this.update(cx, |this, cx| {
                // 同一时刻只放一个 ⇒ 两个忙集一起清（不会误伤别的操作）
                this.cli.busy.clear();
                this.cli.shared_busy.clear();
                // 执行态收掉：② 卡回 ready 态。**最终那份快照不进界面**——
                // 结论在 ③ 终端日志卡与那一条通知里（与升级执行态同一条）
                this.cli_uninstall = None;
                let mut verdict = if out.cancelled {
                    format!("{summary}：已取消")
                } else if out.ok {
                    format!("{summary}：完成")
                } else {
                    format!("{summary}：有步骤没成功，详见上面")
                };
                if out.freed > 0 {
                    verdict.push_str(&format!(
                        "，清理释放 {}",
                        qingbird_core::space::human_bytes(out.freed)
                    ));
                }
                let lg = &mut this.cli_log;
                lg.running = false;
                lg.verdict = Some((out.ok, verdict.clone()));
                lg.ts = store::now_secs();
                // 通知必须弹：用户可能已经切到别的屏，而日志区在 CLI
                // 屏里——不弹一句他就此不知道结果（plain 路同一条）
                this.pending_cli_note = Some(verdict);
                // 包清单变了（卸掉一颗）：整屏重扫（`scan_cli` 落地时会
                // 顺手再查一遍更新）
                this.scan_cli(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 卸载（审计页那颗「卸载」）。**执行前用当下清单按 id 重建计划**：
    /// 审计页开着的时候后台可能重扫，快照里的版本 / 目录可能已经变了
    /// （`CliUninstallAudit::pkg` 字段注释）
    ///
    /// 阶段四起确认即**交棒给卸载执行态**（设计 §3.11.6）：审计页收掉，
    /// ② 卡换成步骤管道（`cli_uninstall`），跑完收尾即清
    pub(crate) fn run_cli_uninstall(&mut self, cx: &mut Context<Self>) {
        // 整份克隆出来再动手：下面要**置空** `cli_audit`，而 `d` 还借在
        // 它身上——克隆一份快照，借用当场结束
        let Some(d) = self.cli_audit.clone() else {
            return;
        };
        // 串行锁（替代审计页那颗"执行中…"）：忙时入口全禁，压根到不了
        // 这儿；两颗监听排在一起时第二次读到的已在忙，再挡一次
        if self.cli_busy_any() {
            return;
        }
        let id = d.pkg.id.clone();
        // 包没了（重扫之后被卸掉 / 改名）：收审计态说清楚，**不开空执行**
        let Some(pkg) = self.cli.packages.iter().find(|p| p.id == id).cloned() else {
            self.cli_audit = None;
            self.pending_cli_note = Some(format!(
                "{} 已经不在清单里了，没有动任何东西",
                d.pkg.name
            ));
            cx.notify();
            return;
        };
        // 勾上的清理目录：**只认当下清单里还对得上的那些**。快照里勾了、
        // 重扫之后没了的路径，不猜一个继续删
        let picked: Vec<qingbird_core::cli::PackageDir> = pkg
            .dirs
            .iter()
            .filter(|x| d.picked.contains(&x.path))
            .cloned()
            .collect();
        let plan = match qingbird_core::cli::provider_of(pkg.provider) {
            Some(p) => p.plan_uninstall(&pkg, &picked),
            None => qingbird_core::cli::OpPlan::blocked(&pkg, "认不出这个包是从哪个包管理器装的"),
        };
        if plan.steps.is_empty() {
            // 给不出卸载命令：**不开执行**（一颗确认了却什么都没做的键
            // 比没有更糟）。收审计态 + 把 core 给的那句话说出来
            self.cli_audit = None;
            self.pending_cli_note = Some(plan.summary);
            cx.notify();
            return;
        }
        // 交棒：审计态收掉，② 卡换成执行态（步骤管道）
        self.cli_audit = None;
        self.spawn_cli_uninstall_run(cx, &pkg, &plan);
    }

    /// 升级一颗（详情卡那颗「升级」）。**不进 OpPlan**（设计 §6）：升级就是
    /// 一条命令，没有清理步骤可言。
    ///
    /// 阶段四 P0 起走 [`spawn_cli_upgrade`]（阶段化事件那条路）：② 卡
    /// run 态的五阶段管道 / 进度带 / 取消都接在真的命令事件上
    pub(crate) fn upgrade_cli_pkg(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.cli_busy_any() {
            return;
        }
        let Some(pkg) = self.cli.packages.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        let step = qingbird_core::cli::provider_of(pkg.provider).and_then(|p| p.upgrade_step(&pkg));
        let Some(step) = step else {
            // 这颗键只在"有新版且 provider 给得出命令"时画（`cli_tab.rs`）——
            // 真到了这里是状态变了（重扫之后 cargo 也有了 latest？不可能，
            // 但判据不该依赖"不可能"）
            self.pending_cli_note = Some(format!("{}：这个包管理器给不出升级命令", pkg.name));
            cx.notify();
            return;
        };
        let summary = format!(
            "升级 {}（{} → {}）",
            pkg.name,
            pkg.provider.label(),
            pkg.latest.clone().unwrap_or_else(|| "最新".into())
        );
        self.spawn_cli_upgrade(cx, summary, &pkg, &step);
    }

    /// 升级全部可更新的（⌘K 那条）。
    ///
    /// 目标 = **真有新版且 provider 给得出升级命令**的两重判据都过：
    /// cargo 压根不覆写 `outdated`（`latest` 恒 `None`），天然不在里面——
    /// "没有 latest 的升级键是一条骗人的键"（v4 铁律）
    pub(crate) fn upgrade_all_cli(&mut self, cx: &mut Context<Self>) {
        if self.cli_busy_any() {
            return;
        }
        let mut steps = Vec::new();
        for p in self.cli.packages.iter() {
            if p.latest.is_none() {
                continue;
            }
            let Some(pr) = qingbird_core::cli::provider_of(p.provider) else {
                continue;
            };
            if let Some(s) = pr.upgrade_step(p) {
                steps.push(s);
                self.cli.busy.insert(p.id.clone());
            }
        }
        if steps.is_empty() {
            self.pending_cli_note = Some("没有可升级的工具".to_string());
            cx.notify();
            return;
        }
        let n = steps.len();
        // `stop_on_command_failure = false`：一颗升不动不挡住其余那些
        self.spawn_cli_run(
            cx,
            format!("升级 {n} 个工具"),
            steps,
            false,
            CliAfter::Rescan,
        );
    }

    /// 清理共享缓存（共享缓存清理确认框那颗「清理」）。一颗一条地清与
    /// ⌘K 整批地清都走这儿——`targets` 几条就跑几步
    pub(crate) fn run_cli_clean(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.cli_clean_dlg.clone() else {
            return;
        };
        if d.running {
            return;
        }
        let plan = qingbird_core::cli::clean_plan(&d.targets);
        if plan.steps.is_empty() {
            self.cli_clean_dlg = None;
            self.pending_cli_dlg_close = true;
            self.pending_cli_note = Some("没有要清理的共享缓存".to_string());
            cx.notify();
            return;
        }
        if let Some(d) = self.cli_clean_dlg.as_mut() {
            d.running = true;
        }
        for t in d.targets.iter() {
            self.cli.shared_busy.insert(t.path.clone());
        }
        let summary = plan.summary.clone();
        self.spawn_cli_run(cx, summary, plan.steps, true, CliAfter::RemeasureShared);
    }
}

/// 一颗 CLI 计划跑完之后要做的收尾。**三种动作只有这两处分岔**
/// （卸载 / 升级改的是包清单 ⇒ 重扫；清理只动了共享缓存的内容 ⇒ 重量）
enum CliAfter {
    /// 整屏重扫（`scan_cli` 落地时会顺手再查一遍更新）
    Rescan,
    /// 只把共享缓存那一批体积重新量（目录还在、内容没了）
    RemeasureShared,
}
