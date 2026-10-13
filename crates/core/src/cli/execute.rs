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

use std::path::Path;
use std::time::Duration;

use super::exec;
use super::plan::{OpPlan, Step, StepKind};

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

/// 跑一步卸载命令。起不来（程序不在 PATH 上）也算失败，且**不当成 Err 丢出去**——
/// 它只是这一步的结论，由 `on_step` 交出去
fn run_uninstall(step: &super::plan::Step) -> StepOutcome {
    let args: Vec<&str> = step.args.iter().map(String::as_str).collect();
    match exec::run_program(&step.program, &args, OP_TIMEOUT) {
        Ok(o) => StepOutcome {
            ok: o.ok(),
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
}
