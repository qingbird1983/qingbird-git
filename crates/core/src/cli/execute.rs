//! 阶段三：OpPlan 的执行（设计 §11）。
//!
//! 状态机 Idle → Planning → AwaitingConfirm → Running → Success/Failed 里，
//! **这个文件只负责 Running 那一段**：把计划里的步骤**串行**跑完，每步的结果
//! 通过回调交出去（界面侧搬进日志，与 `ops::drain_log_until_done` 同一条管道）。
//!
//! 三条纪律：
//! - **卸载失败即停**：包还原封不动在那儿，随后的清理一步都不该跑
//!   （工具还在，删它缓存是"计划外动作"——计划是"卸掉并清理"）
//! - **清理失败不连坐**：一个目录被占用，同批里别的照删（用户点清理就是要清掉
//!   能清的那些，与 `space::delete_cache_dirs` 同一条）
//! - **每步都经回调出去**：时间、命令、退出码、stdout/stderr 全在界面上看得见
//!   （设计 §11：release 无 stderr，显式捕获——`exec::run_program` 本来就收着）
//!
//! 阶段四起多一条**流式**入口 [`run_plan_live`]（② 卡卸载执行态，设计
//! §3.11.6）：那个每行、每次步骤状态切换都回调——plain 路（本文件的
//! [`run_steps`]）每步只回调一次结论，看不见步内的事。取消语义见那条
//! 函数的文档。

use std::path::Path;
use std::time::{Duration, Instant};

use super::exec;
use super::plan::{OpPlan, Step, StepKind};
use crate::cli::CancelToken;

/// 一条卸载/升级命令最多等多久。
///
/// 比 [`super::provider::LIST_TIMEOUT`]（30 秒，列清单用）宽一个量级：
/// `winget uninstall` 一个大家伙、`npm uninstall -g` 一棵大树都是分钟级。
/// 真挂住了也不至于把界面关在后台线程里——超时即杀、即报失败。
pub const OP_TIMEOUT: Duration = Duration::from_secs(180);

/// 一步跑完的样子。stdout/stderr 原文带上——**不在这儿转述**，
/// 用户要看得见包管理器自己说了什么（与 `run_sync` 那条"优先上 git 原文"同一条）。
pub struct StepOutcome {
    pub ok: bool,
    /// 退出码；被信号杀掉 / 起不来时为 `None`
    pub code: Option<i32>,
    pub program: String,
    pub args: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    /// 清理这一步释放的字节数（`CleanupDir` 才有；卸载步是 `None`）
    pub freed: Option<u64>,
    /// 清理步清的是**哪个目录**（卸载步是空串）。日志行要说得清动了什么——
    /// 清理步没有命令可拼，这一格就是它的"命令"
    pub target: String,
}

impl StepOutcome {
    /// 日志里那一行：`✓ npm uninstall -g pnpm` / `✗ winget uninstall …（退出码 1）`。
    ///
    /// **公开**：界面侧每步回调时直接把这一行搬进日志（`ops.rs`）——
    /// "把结构化步骤还原成文本"全仓只此一处，GUI 不再自己拼第二份
    /// （拼了就是同一句话两个出处：改一处漏一处，屏幕上的话与库里的对不上）。
    pub fn line(&self) -> String {
        let mark = if self.ok { "✓" } else { "✗" };
        let mut cmd = self.program.clone();
        for a in &self.args {
            cmd.push(' ');
            cmd.push_str(a);
        }
        // 清理步没有命令（program/args 空）⇒ 拼它清的那个目录
        if cmd.is_empty() {
            cmd = self.target.clone();
        }
        match (&self.freed, &self.code) {
            (Some(freed), _) => format!(
                "{mark} 已清理 {cmd}，释放 {}",
                crate::space::human_bytes(*freed)
            ),
            (None, Some(code)) if !self.ok => format!("{mark} {cmd}（退出码 {code}）"),
            _ => format!("{mark} {cmd}"),
        }
    }
}

/// 跑**一步**（升级那条路用它：设计 §6，一条命令直接执行，不进 OpPlan）。
/// 卸载 / 清理走 [`run_plan`]（多步、有停止规则）。
pub fn run_step(step: &Step) -> StepOutcome {
    match step.kind {
        StepKind::Uninstall => run_uninstall(step),
        StepKind::CleanupDir => run_cleanup(step),
    }
}

/// 把计划跑完。返回 `false` = 有步骤失败（调用方据此报结论）。
///
/// `on_step` 每步回调一次——**同步执行、同步回调**（都在调用方的后台线程上）。
/// 界面侧拿到的是已经结束的一步，不是"进行中"：进度搬运那套（共享缓冲 +
/// 轮询）由界面自己决定要不要接，core 不替它选。
pub fn run_plan(plan: &OpPlan, on_step: impl FnMut(&StepOutcome)) -> bool {
    // 卸载计划：卸载步失败即停（后面的清理一步都不跑）
    run_steps(&plan.steps, true, on_step)
}

/// 把一串步骤跑完（[`run_plan`] 的步骤级入口）。
///
/// `stop_on_command_failure` 是两种语义的分界：
/// - **卸载 / 清理计划传 `true`**——卸载没成功，随后的清理一步都不跑
///   （工具还在，删它缓存是"计划外动作"，见模块头第一条）
/// - **一批各自独立的升级传 `false`**——一颗升不动不该挡住其余那些
///   （用户点「升级全部」就是要清掉能升的那些，与 `space::delete_cache_dirs`
///   的"逐项独立"同一条）。单条升级传哪个都一样（只有一步）
pub fn run_steps(
    steps: &[Step],
    stop_on_command_failure: bool,
    mut on_step: impl FnMut(&StepOutcome),
) -> bool {
    let mut all_ok = true;
    for step in steps {
        let outcome = run_step(step);
        let ok = outcome.ok;
        on_step(&outcome);
        if !ok {
            all_ok = false;
            if stop_on_command_failure && step.kind == StepKind::Uninstall {
                break;
            }
        }
    }
    all_ok
}

// ---------- 阶段四：卸载执行态（② 卡步骤管道，设计 §3.11.6） ----------

/// 一步此刻的状态。与升级那六态同族、两档不同：
///
/// - [`Cancelled`](StepState::Cancelled) 已取消：用户按停了——正在跑的
///   被杀掉，还没轮到的连同记上（它们没跑是因为用户喊停，不是因为失败）
/// - [`Skipped`](StepState::Skipped) 未执行：计划因**前一步失败**而停，
///   根本没跑到这一步（卸载失败即停，模块头第一条纪律）
///
/// 这两档必须分开：取消是用户的选择，未执行是计划的断裂。混成一档
/// 「已取消」的话，一个自己失败的计划会被说成"用户取消了"
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepState {
    Pending,
    Running,
    Done(Duration),
    Failed(Duration),
    Cancelled,
    Skipped,
}

impl StepState {
    /// 这一步成功了没有（收尾判 `ok` 用）
    pub fn is_done(self) -> bool {
        matches!(self, StepState::Done(_))
    }

    /// 这一态用了多久（Done / Failed 才有；其余 `None`）
    pub fn elapsed(self) -> Option<Duration> {
        match self {
            StepState::Done(d) | StepState::Failed(d) => Some(d),
            _ => None,
        }
    }
}

/// 管道里的一行。**`label` 由 core 拼**——界面不自己拼（同一句话两个出处，
/// 本项目最贵的错；与 `StepOutcome::line` 同一条纪律）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StepStatus {
    pub label: String,
    pub state: StepState,
    /// 一句话细节：清理成功的释放字节 / 失败原因。等待与进行中是空
    pub detail: String,
}

/// 一份卸载计划执行中的快照。后台线程产、UI 120ms 轮询搬（GUI 侧
/// `LogEvent::CliUngrade`，与升级的 `LogEvent::CliUpgrade` 同一条轮询管道）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PlanProgress {
    pub steps: Vec<StepStatus>,
    /// 已释放合计（各清理步 `freed` 之和）。结论那句"清理释放 x"从这儿来
    pub freed: u64,
    /// 用户点了取消（命令可能还在被杀掉的路上）
    pub cancelled: bool,
}

impl PlanProgress {
    /// 开局：每步一行全「等待」，标签从计划里来。第一份快照由这个来——
    /// 界面在后台开跑之前就能画出整条管道（全是等待格）
    pub fn new(steps: &[Step]) -> Self {
        PlanProgress {
            steps: steps
                .iter()
                .map(|s| StepStatus {
                    label: step_label(s),
                    state: StepState::Pending,
                    detail: String::new(),
                })
                .collect(),
            freed: 0,
            cancelled: false,
        }
    }

    /// 「第 k/n 步」：k = 第一行没完成的位置 + 1（从 1 数）。全完成了给
    /// (n, n)；中途停下（取消 / 失败）给停下来的那一行——那句话挂在一个
    /// 真实的位置上，不是说说的
    pub fn step_no(&self) -> Option<(usize, usize)> {
        let n = self.steps.len();
        let k = self
            .steps
            .iter()
            .position(|s| !s.state.is_done())
            .map(|i| i + 1)
            .unwrap_or(n);
        Some((k, n))
    }

    /// 置某一行。下标越界当没发生（结构上到不了；真到了不 panic）
    fn set(&mut self, ix: usize, state: StepState, detail: String) {
        if let Some(s) = self.steps.get_mut(ix) {
            s.state = state;
            s.detail = detail;
        }
    }
}

/// 管道行上那一格的 label。卸载步说清是**谁家的卸载命令**（npm / winget …
/// 与预览里那一行同一个程序名）；清理步说清类别 + 目录名——同类可能有多
/// 份（`npm 缓存` 与 `pnpm store` 都是缓存），目录名是唯一的区分
fn step_label(step: &Step) -> String {
    match step.kind {
        StepKind::Uninstall => format!("卸载命令 · {}", step.program),
        StepKind::CleanupDir => match &step.cleanup_dir {
            Some((dir, kind)) => {
                let name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| dir.to_string_lossy().into_owned());
                format!("清理{} · {}", kind.label(), name)
            }
            // 结构上到不了（`Step::cleanup` 必带目录）；真到了就说缺了什么
            None => "清理（缺目录）".to_string(),
        },
    }
}

/// 一次卸载跑完的结论。`ok = false` 且有步骤没失败 = 被取消（调用方用
/// [`cancelled`](Self::cancelled) 分辨）
pub struct PlanOutcome {
    pub ok: bool,
    pub cancelled: bool,
    /// 清理步释放的字节合计（没勾清理 = 0）
    pub freed: u64,
}

/// 把一份计划**流式**跑完（卸载执行态；设计 §3.11.6「七步管道 + 取消」）。
///
/// 与 [`run_plan`] 的分工：那个每**步**回调一次结论（一步一结束），界面
/// 拿到的每一步都已结束；这个每**行**、每次**步骤状态切换**都回调——
/// ② 卡的步骤管道（等待 → 进行中 → ✓耗时 / ✗ / 已取消 / 未执行）要的是
/// "正在发生"，plain 路那根管道看不见步内的事。
///
/// 停止与取消的口径：
/// - **卸载步失败即停**：剩余步骤记 [`StepState::Skipped`]（没跑到，不是
///   失败——画 ✗ 是冤）
/// - **取消**：正在跑的卸载命令被杀掉记 [`StepState::Cancelled`]，还没
///   轮到的连同记上；结论 `cancelled = true`。**取消与跑完撞车以跑完
///   为准**：旗号晚到一步而命令恰好成功，那一步记 ✓、后续才记取消
/// - **清理步失败不连坐**：同批里别的清理照跑（用户点清理就是要清掉能
///   清的那些，与 `space::delete_cache_dirs` 同一条）
/// - **清理步内部断不了**：`clean_dir_contents` 一路删到底，没有步内检查
///   点 ⇒ 取消落在两次步之间；正在跑的卸载命令里头断得了
///   （`exec::run_program_live` 盯旗号杀进程树）
///
/// `on_line` 收两种行：命令的**实时输出**（winget 的下载 / 安装行）与每步
/// 结束的结论行（`StepOutcome::line` 同一格式——终端卡里两种行混排正是
/// 终端的本来样子）。`on_progress` 每次快照变化调一次（与
/// [`crate::cli::upgrade::run_upgrade`] 同一条纪律）
pub fn run_plan_live(
    plan: &OpPlan,
    cancel: &CancelToken,
    mut on_line: impl FnMut(&str),
    mut on_progress: impl FnMut(&PlanProgress),
) -> PlanOutcome {
    let mut progress = PlanProgress::new(&plan.steps);
    let mut all_ok = true;
    let mut stopped_by_cancel = false;

    for (ix, step) in plan.steps.iter().enumerate() {
        // 步间取消检查（清理步内部断不了，见函数文档第四条）
        if cancel.is_cancelled() {
            stopped_by_cancel = true;
            progress.cancelled = true;
            for s in progress.steps.iter_mut().skip(ix) {
                s.state = StepState::Cancelled;
            }
            on_line("· 卸载已取消");
            on_progress(&progress);
            break;
        }

        progress.set(ix, StepState::Running, String::new());
        on_progress(&progress);
        let t0 = Instant::now();
        let outcome = match step.kind {
            StepKind::Uninstall => run_uninstall_live(step, cancel, &mut on_line),
            StepKind::CleanupDir => run_cleanup(step),
        };

        // 这一步的终态。三种"没成"分开说：取消（用户按停，命令被杀）/
        // 超时（被杀但旗号没置）/ 跑而不成（退出码非零或起不来）
        let (state, detail) = if step.kind == StepKind::Uninstall && outcome.code.is_none() {
            if cancel.is_cancelled() {
                (StepState::Cancelled, String::new())
            } else {
                (
                    StepState::Failed(t0.elapsed()),
                    format!("超过 {} 秒", OP_TIMEOUT.as_secs()),
                )
            }
        } else if outcome.ok {
            let detail = match outcome.freed {
                Some(f) if f > 0 => format!("释放 {}", crate::space::human_bytes(f)),
                _ => String::new(),
            };
            (StepState::Done(t0.elapsed()), detail)
        } else {
            // 失败原因：退出码（与 `StepOutcome::line` 同一格式）优先；
            // 起不来 / 清理出错时 stderr 里是 core 给的那句话
            let e = outcome
                .stderr
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim();
            let detail = match outcome.code {
                Some(c) => format!("退出码 {c}"),
                None if !e.is_empty() => e.to_string(),
                None => "没成功".to_string(),
            };
            (StepState::Failed(t0.elapsed()), detail)
        };

        let ok = state.is_done();
        progress.set(ix, state, detail);
        if let Some(f) = outcome.freed {
            progress.freed += f;
        }
        // 每步结论行进终端卡——**与 plain 路同一格式**（`StepOutcome::line`
        // 是全仓唯一把步骤还原成文本的地方）
        on_line(&outcome.line());
        on_progress(&progress);

        if !ok {
            all_ok = false;
            if state == StepState::Cancelled {
                // 用户按停：还没轮到的连同记「已取消」
                stopped_by_cancel = true;
                progress.cancelled = true;
                for s in progress.steps.iter_mut().skip(ix + 1) {
                    s.state = StepState::Cancelled;
                }
                on_line("· 卸载已取消");
                on_progress(&progress);
                break;
            }
            if step.kind == StepKind::Uninstall {
                // 卸载失败即停：剩余记「未执行」（没跑到，不是失败）
                for s in progress.steps.iter_mut().skip(ix + 1) {
                    s.state = StepState::Skipped;
                }
                on_progress(&progress);
                break;
            }
            // 清理失败不连坐：继续下一步
        }
    }

    PlanOutcome {
        ok: all_ok && !stopped_by_cancel,
        cancelled: stopped_by_cancel,
        freed: progress.freed,
    }
}

/// 跑一步卸载命令。起不来（程序不在 PATH 上）也算失败，且**不当成 Err 丢出去**——
/// 它只是这一步的结论，由 `on_step` 交出去。
///
/// 阶段四起它走 [`run_uninstall_live`]（`on_line` 空转）：卸载执行态要按行
/// 看输出，两条路共用同一个实现——再留一条"等跑完拿全部输出"的旁支就是
/// 同一件事两个出处
fn run_uninstall(step: &Step) -> StepOutcome {
    run_uninstall_live(step, &CancelToken::new(), |_| {})
}

/// [`run_uninstall`] 的流式版：每行输出经 `on_line` 出去（卸载执行态按行
/// 回显到终端卡），取消旗号透传给 `exec::run_program_live`
fn run_uninstall_live(step: &Step, cancel: &CancelToken, mut on_line: impl FnMut(&str)) -> StepOutcome {
    let args: Vec<&str> = step.args.iter().map(String::as_str).collect();
    match exec::run_program_live(&step.program, &args, OP_TIMEOUT, cancel.flag(), |l| {
        on_line(l)
    }) {
        Ok(o) => StepOutcome {
            // `ok` 只认退出码 0：被杀（取消 / 超时）的 `code` 是 `None`
            ok: o.code == Some(0),
            code: o.code,
            program: step.program.clone(),
            args: step.args.clone(),
            stdout: o.stdout,
            stderr: o.stderr,
            freed: None,
            target: String::new(),
        },
        Err(e) => StepOutcome {
            ok: false,
            code: None,
            program: step.program.clone(),
            args: step.args.clone(),
            stdout: String::new(),
            stderr: e,
            freed: None,
            target: String::new(),
        },
    }
}

/// 跑一步清理。
fn run_cleanup(step: &super::plan::Step) -> StepOutcome {
    let (dir, _kind) = match &step.cleanup_dir {
        Some(d) => d,
        // 结构上到不了（`Step::cleanup` 必带目录）；真到了就当这步失败，
        // 不 panic——执行器不该有第三条腿
        None => {
            return StepOutcome {
                ok: false,
                code: None,
                program: String::new(),
                args: Vec::new(),
                stdout: String::new(),
                stderr: "清理步骤缺目录（结构上不该出现）".to_string(),
                freed: None,
                target: String::new(),
            }
        }
    };
    let shown = dir.display().to_string();
    match super::clean::clean_dir_contents(Path::new(dir)) {
        Ok(freed) => StepOutcome {
            ok: true,
            code: Some(0),
            program: String::new(),
            args: Vec::new(),
            stdout: String::new(),
            stderr: String::new(),
            freed: Some(freed),
            target: shown,
        },
        Err(e) => StepOutcome {
            ok: false,
            code: None,
            program: String::new(),
            args: Vec::new(),
            stdout: String::new(),
            stderr: e,
            freed: None,
            target: shown,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::model::{CliPackage, DirKind, PackageDir, PackageKind, ProviderId};
    use crate::cli::plan::{RiskLevel, Step};

    fn pkg() -> CliPackage {
        CliPackage {
            id: "npm:pnpm".into(),
            provider: ProviderId::Npm,
            name: "pnpm".into(),
            version: Some("11.5.3".into()),
            latest: None,
            install_path: None,
            executables: vec!["pnpm".into()],
            kind: PackageKind::CliOnly,
            dirs: Vec::new(),
        }
    }

    fn cache_dir(path: &str, size: Option<u64>) -> PackageDir {
        PackageDir {
            path: path.into(),
            kind: DirKind::Cache,
            label: String::new(),
            size,
        }
    }

    /// 一条纯清理计划（不起子进程、不碰 PATH）在**真临时目录**上跑通：
    /// 两步都成功、字节照实、日志行带 ✓。
    ///
    /// 计划**手搓**（不走 `assemble`）：这条要验的就是"清理段自己跑得通"，
    /// 借 `assemble` 拼一个假卸载步进来只会把两条用例的判据搅在一起
    #[test]
    fn a_cleanup_plan_runs_and_reports_freed_bytes() {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("qb-cli-exec-{n}"));
        let _ = std::fs::remove_dir_all(&base);
        for (sub, bytes) in [("c1", 100u64), ("c2", 50)] {
            let d = base.join(sub);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("x.bin"), vec![0u8; bytes as usize]).unwrap();
        }
        let plan = OpPlan {
            steps: vec![
                Step::cleanup(&base.join("c1").to_string_lossy(), DirKind::Cache),
                Step::cleanup(&base.join("c2").to_string_lossy(), DirKind::Cache),
            ],
            risk: RiskLevel::Safe,
            summary: "清理两个目录".into(),
            affected: Vec::new(),
            reclaimable: Some(150),
        };
        let mut lines = Vec::new();
        let ok = run_plan(&plan, |o| lines.push(o.line()));
        assert!(ok, "纯清理计划该全成");
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with('✓'), "{:?}", lines);
        assert!(lines[0].contains("100 B"), "{:?}", lines);
        assert!(!base.join("c1").join("x.bin").exists(), "c1 清空了");
        assert!(!base.join("c2").join("x.bin").exists(), "c2 清空了");
        assert!(base.join("c1").is_dir() && base.join("c2").is_dir(), "目录本身留着");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// **失败即停**：卸载步失败 ⇒ 后面的清理一步都不跑。
    /// 判据是"这一步没跑"（回调只收到一条），不是"报了错"——
    /// 报错但照样清理正是这条纪律要挡的那件事
    #[test]
    fn a_failed_uninstall_stops_the_plan() {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("qb-cli-exec-stop-{n}"));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("x.bin"), vec![0u8; 10]).unwrap();
        let plan = crate::cli::plan::assemble(
            &pkg(),
            // 一个 PATH 上必然没有的程序 ⇒ 卸载步必败
            Step::uninstall("qb-no-such-program-42", &["uninstall"]),
            &[cache_dir(base.to_string_lossy().as_ref(), Some(10))],
        );
        let mut seen = Vec::new();
        let ok = run_plan(&plan, |o| seen.push(o.ok));
        assert!(!ok);
        assert_eq!(seen, vec![false], "只该跑第一步：清理被挡住了");
        assert!(base.join("x.bin").exists(), "清理一步都没跑 ⇒ 文件还在");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 日志行的形状：失败带退出码、清理带释放字节。
    /// （纯函数层面的断言——`line()` 是唯一把步骤还原成文本的地方）
    #[test]
    fn outcome_lines_carry_the_facts() {
        let bad = StepOutcome {
            ok: false,
            code: Some(1),
            program: "winget".into(),
            args: vec!["uninstall".into(), "--id".into(), "Git.Git".into()],
            stdout: String::new(),
            stderr: "access denied".into(),
            freed: None,
            target: String::new(),
        };
        let line = bad.line();
        assert!(line.starts_with('✗'), "{line}");
        assert!(line.contains("退出码 1"), "{line}");
        assert!(line.contains("winget uninstall --id Git.Git"), "{line}");

        let good = StepOutcome {
            ok: true,
            code: Some(0),
            program: String::new(),
            args: Vec::new(),
            stdout: String::new(),
            stderr: String::new(),
            freed: Some(2048),
            target: "C:\\Users\\x\\.deno".into(),
        };
        let line = good.line();
        assert!(line.starts_with('✓'), "{line}");
        assert!(line.contains("2 KB"), "清理行要报释放字节：{line}");
        assert!(
            line.contains(".deno"),
            "清理行要说出清的是哪个目录（它没有命令可拼）：{line}"
        );
    }

    /// `stop_on_command_failure = false`（一批各自独立的升级）：**头一步失败
    /// 不连坐**。判据是"第二步照跑"（回调收到两条）——与 `run_plan` 那条
    /// 「卸载失败即停」故意相反，两边各自的理由写在 `run_steps` 的注释里
    #[test]
    fn a_batch_of_upgrades_does_not_stop_at_the_first_failure() {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("qb-cli-exec-batch-{n}"));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("x.bin"), vec![0u8; 7]).unwrap();
        let steps = vec![
            // PATH 上必然没有的程序 ⇒ 这一步必败
            Step::uninstall("qb-no-such-program-42", &["update"]),
            Step::cleanup(&base.to_string_lossy(), DirKind::Cache),
        ];
        let mut seen = Vec::new();
        let ok = run_steps(&steps, false, |o| seen.push(o.ok));
        assert!(!ok, "有失败 ⇒ 整体报失败");
        assert_eq!(seen, vec![false, true], "第二步照跑：一颗升不动不挡住其余");
        assert!(!base.join("x.bin").exists(), "第二步真的执行了");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 流式计划：两步清理都跑通，**每步都有计时**、释放字节进 detail 与
    /// 合计、✓ 结论行与 plain 路同一格式。快照序列钉着"每步恰好两次
    /// 状态切换"（Running → 终态）——多一次少一次都是状态机在乱发
    #[test]
    fn a_live_plan_times_each_step_and_sums_freed() {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("qb-cli-live-{n}"));
        let _ = std::fs::remove_dir_all(&base);
        for (sub, bytes) in [("c1", 100u64), ("c2", 50)] {
            let d = base.join(sub);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("x.bin"), vec![0u8; bytes as usize]).unwrap();
        }
        let plan = OpPlan {
            steps: vec![
                Step::cleanup(&base.join("c1").to_string_lossy(), DirKind::Cache),
                Step::cleanup(&base.join("c2").to_string_lossy(), DirKind::Cache),
            ],
            risk: RiskLevel::Safe,
            summary: "清理两个目录".into(),
            affected: Vec::new(),
            reclaimable: Some(150),
        };
        let mut snaps: Vec<PlanProgress> = Vec::new();
        let mut lines: Vec<String> = Vec::new();
        let out = run_plan_live(
            &plan,
            &CancelToken::new(),
            |l| lines.push(l.to_string()),
            |p| snaps.push(p.clone()),
        );
        assert!(out.ok, "纯清理计划该全成");
        assert!(!out.cancelled);
        assert_eq!(out.freed, 150);

        let shown: Vec<String> = snaps
            .iter()
            .map(|s| {
                s.steps
                    .iter()
                    .map(|x| format!("{:?}", x.state))
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .collect();
        assert_eq!(snaps.len(), 4, "每步恰好两次切换：{shown:?}");
        assert!(matches!(snaps[0].steps[0].state, StepState::Running));
        assert!(matches!(snaps[0].steps[1].state, StepState::Pending));
        assert!(matches!(snaps[1].steps[0].state, StepState::Done(_)));
        assert!(matches!(snaps[1].steps[1].state, StepState::Pending));

        let last = snaps.last().unwrap();
        assert!(
            last.steps
                .iter()
                .all(|s| s.state.is_done() && s.state.elapsed().is_some()),
            "终局两步都 Done 且都带着用时：{last:?}"
        );
        assert!(
            last.steps[0].detail.contains("100 B"),
            "清理成功那行带释放字节：{:?}",
            last.steps[0]
        );
        assert_eq!(last.freed, 150);
        assert!(
            last.steps[0].label.contains("清理")
                && last.steps[0].label.contains("缓存")
                && last.steps[0].label.contains("c1"),
            "标签说得清这一步是什么：{:?}",
            last.steps[0]
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with('✓') && l.contains("已清理") && l.contains("100 B")),
            "✓ 结论行与 plain 路同一格式：{lines:?}"
        );
        assert!(base.join("c1").is_dir(), "目录本身留着");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 取消：**真进程**（`ping -n 30` 自己要跑 ~29 秒）200ms 后按停 ⇒
    /// 正跑着的那步记「已取消」、没轮到的连同记（不是未执行）、调用必须
    /// 远短于进程自己跑完的时间——杀得掉才算取消，杀不掉这道测试会挂住
    #[test]
    fn cancel_marks_the_running_step_and_everything_after_it_cancelled() {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("qb-cli-live-cancel-{n}"));
        let _ = std::fs::remove_dir_all(&base);
        for sub in ["c1", "c2"] {
            let d = base.join(sub);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("x.bin"), vec![0u8; 10]).unwrap();
        }
        let plan = OpPlan {
            steps: vec![
                // 先跑一步真清理（取消落到它后面）
                Step::cleanup(&base.join("c1").to_string_lossy(), DirKind::Cache),
                // 真进程：`cmd /C ping -n 30`，取消时它还在跑
                Step::uninstall("ping", &["-n", "30"]),
                // 不该跑到
                Step::cleanup(&base.join("c2").to_string_lossy(), DirKind::Cache),
            ],
            risk: RiskLevel::Moderate,
            summary: "卸载并清理".into(),
            affected: Vec::new(),
            reclaimable: Some(20),
        };
        let cancel = CancelToken::new();
        let killer = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs_f64(0.3));
            killer.cancel();
        });
        let t0 = Instant::now();
        let mut last: Option<PlanProgress> = None;
        let out = run_plan_live(&plan, &cancel, |_| {}, |p| last = Some(p.clone()));
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "取消没杀掉进程树（ping 自己要跑 29 秒）：{:?}",
            t0.elapsed()
        );
        assert!(out.cancelled, "取消的结论");
        assert!(!out.ok, "取消不算成功");
        let p = last.expect("至少该有一份快照");
        assert!(
            matches!(p.steps[0].state, StepState::Done(_)),
            "取消前跑完的清理记 ✓：{:?}",
            p.steps[0]
        );
        assert!(
            matches!(p.steps[1].state, StepState::Cancelled),
            "正跑着被杀的卸载命令记已取消：{:?}",
            p.steps[1]
        );
        assert!(
            matches!(p.steps[2].state, StepState::Cancelled),
            "没轮到的连同记已取消（用户喊停，不是计划断裂）：{:?}",
            p.steps[2]
        );
        assert!(!base.join("c1").join("x.bin").exists(), "第一步清掉了");
        assert!(
            base.join("c2").join("x.bin").exists(),
            "第三步根本没跑：文件还在"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 卸载步失败即停的流式版：剩余记「**未执行**」——它与「已取消」是两
    /// 回事（一个是计划自己断了，一个是用户喊停），混一档会把失败说成
    /// 用户取消。清理一步都没跑（文件还在）
    #[test]
    fn a_failed_uninstall_marks_the_rest_skipped() {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("qb-cli-live-stop-{n}"));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("x.bin"), vec![0u8; 10]).unwrap();
        let plan = OpPlan {
            steps: vec![
                // PATH 上必然没有的程序 ⇒ cmd 起得来、它报"不是内部或外部
                // 命令"退出码 1 ⇒ 跑而不成（不是 Err）
                Step::uninstall("qb-no-such-program-42", &["uninstall"]),
                Step::cleanup(base.to_string_lossy().as_ref(), DirKind::Cache),
            ],
            risk: RiskLevel::Moderate,
            summary: "卸载并清理".into(),
            affected: Vec::new(),
            reclaimable: Some(10),
        };
        let mut last: Option<PlanProgress> = None;
        let out = run_plan_live(
            &plan,
            &CancelToken::new(),
            |_| {},
            |p| last = Some(p.clone()),
        );
        assert!(!out.ok);
        assert!(!out.cancelled, "失败不是取消");
        let p = last.expect("至少该有一份快照");
        assert!(
            matches!(p.steps[0].state, StepState::Failed(_)),
            "跑而不成记 ✗：{:?}",
            p.steps[0]
        );
        assert!(
            p.steps[0].detail.contains("退出码 1"),
            "失败原因说退出码（与日志行同一格式）：{:?}",
            p.steps[0]
        );
        assert!(
            matches!(p.steps[1].state, StepState::Skipped),
            "没跑到的记未执行：{:?}",
            p.steps[1]
        );
        assert!(base.join("x.bin").exists(), "清理一步都没跑");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 旗号预置（用户在手速上赢了后台线程一步）：一步都不跑，全记已取消
    #[test]
    fn cancel_before_the_first_step_marks_everything_cancelled() {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("qb-cli-live-pre-{n}"));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("x.bin"), vec![0u8; 10]).unwrap();
        let plan = OpPlan {
            steps: vec![Step::cleanup(base.to_string_lossy().as_ref(), DirKind::Cache)],
            risk: RiskLevel::Safe,
            summary: "清理".into(),
            affected: Vec::new(),
            reclaimable: Some(10),
        };
        let cancel = CancelToken::new();
        cancel.cancel();
        let mut lines: Vec<String> = Vec::new();
        let mut last: Option<PlanProgress> = None;
        let out = run_plan_live(
            &plan,
            &cancel,
            |l| lines.push(l.to_string()),
            |p| last = Some(p.clone()),
        );
        assert!(out.cancelled);
        assert!(!out.ok);
        assert_eq!(out.freed, 0, "一步都没跑 ⇒ 什么都没释放");
        let p = last.expect("至少该有一份快照");
        assert!(
            matches!(p.steps[0].state, StepState::Cancelled),
            "{:?}",
            p.steps[0]
        );
        assert!(lines.iter().any(|l| l.contains("已取消")), "{lines:?}");
        assert!(base.join("x.bin").exists(), "一步都没跑");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 标签与「第 k/n 步」是纯函数：不碰盘、不碰进程。**未执行不算完成**——
    /// 那句话必须挂在"还没完成的那一行"上，包括没跑到的那些
    #[test]
    fn plan_progress_labels_and_step_no_are_pure() {
        let plan = OpPlan {
            steps: vec![
                Step::uninstall("winget", &["uninstall", "--id", "Git.Git"]),
                Step::cleanup(r"C:\Users\x\.deno", DirKind::Cache),
            ],
            risk: RiskLevel::Moderate,
            summary: "卸载并清理".into(),
            affected: Vec::new(),
            reclaimable: None,
        };
        let mut p = PlanProgress::new(&plan.steps);
        assert_eq!(p.steps.len(), 2);
        assert!(
            p.steps[0].label.contains("winget"),
            "卸载步带程序名：{:?}",
            p.steps[0]
        );
        assert!(
            p.steps[1].label.contains("缓存") && p.steps[1].label.contains(".deno"),
            "清理步带类别 + 目录名：{:?}",
            p.steps[1]
        );
        assert!(
            p.steps
                .iter()
                .all(|s| s.state == StepState::Pending && s.detail.is_empty()),
            "开局全等待、无细节"
        );
        assert_eq!(p.step_no(), Some((1, 2)));

        p.steps[0].state = StepState::Done(Duration::ZERO);
        assert_eq!(p.step_no(), Some((2, 2)), "第二行还没完成");
        p.steps[1].state = StepState::Done(Duration::ZERO);
        assert_eq!(p.step_no(), Some((2, 2)), "全完成给 (n, n)");
        p.steps[0].state = StepState::Skipped;
        assert_eq!(
            p.step_no(),
            Some((1, 2)),
            "未执行不算完成：第几步挂在没跑到的那一行"
        );
    }
}
