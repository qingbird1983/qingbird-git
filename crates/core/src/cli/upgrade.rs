//! 阶段四 P0：**阶段化升级事件**（roadmap/cli.md 待建清单 #1，
//! UI-DESIGN §3.11.5 ② 卡 run 态）。
//!
//! 阶段三的升级是"一条命令 + 一句结论"：命令丢后台，跑完往日志推一行
//! `✓ winget upgrade …`。② 卡 run 态在那时候只能画一行"正在升级…"——
//! **画不了五阶段管道与进度带，因为背后没有事件**（v8 定稿时如实登记
//! "事件没接上之前画进度条就是骗人"）。本文件就是那条事件链：命令
//! **流式**跑（`exec::run_program_live`），每次阶段切换 / 百分比变化
//! 都产出一份 [`UpgradeProgress`] 快照，界面据此点亮管道。
//!
//! ## 五个阶段是哪来的、谁干的
//!
//! 设计稿五阶段（下载 → 校验 → 备份 → 替换 → 链接）对应的是**自管理
//! 安装流**（我们自己下载、验哈希、备份、替换、更新 shim）。P0 的事件
//! 机制跑在**包管理器代办**的升级上（`winget upgrade` 一条命令），所以
//! 阶段按**真实执行序**排，每一段都标得出是谁干的：
//!
//! | 阶段 | 谁干的 | 怎么知道的 |
//! |---|---|---|
//! | 备份 | 我们 | 跑命令前记录回滚点（版本 + 安装位置 + 命令入口） |
//! | 下载 | 包管理器 | 输出里认出来（`正在下载` / `Downloading` …） |
//! | 校验 | 包管理器 | 输出里认出来（哈希验证那一行） |
//! | 替换 | 包管理器 | 输出里认出来（安装那一行） |
//! | 链接 | 我们 | 跑完后复验命令入口还在 PATH 上 |
//!
//! **备份排第一是刻意的**：回滚点必须在旧版本被替换**之前**记录，顺序
//! 反了记下来的就是新版本。设计稿把备份排第三（自管理流的顺序），这个
//! 偏差登记在 `roadmap/cli.md` 阶段四。
//!
//! ## 两条"不猜"纪律
//!
//! 1. **认不出的阶段不猜**：命令跑完还没被任何一行输出点亮的中间段记
//!    [`PhaseState::Unreported`]（未报告）。npm 管道模式下全程只有一句
//!    `added 2 packages`（2026-10-15 本机实测：进度条只在 TTY 下出），
//!    它的下载 / 校验压根不上屏——画三个 ✓ 是编，画 ✗ 是冤。
//! 2. **百分比常态是 `None`**：winget / npm 的进度条都只在 TTY 下输出
//!    （本机实测 `winget download` 一个 40 MB 的包，管道里只有五行字，
//!    一根进度条都没有），而 GUI 起子进程一律管道 + `CREATE_NO_WINDOW`
//!    （`exec.rs` 模块头那条纪律）。所以拿不到百分比是常态：`None` 时
//!    界面走不定态扫光（`widgets::progress_readout` 同一判据），不造一个
//!    假的让人以为快好了。真有 provider 往管道里写 `45%` 时才点亮
//!    （解析器在 [`parse_percent`]）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::exec;
use super::model::CliPackage;
use super::plan::Step;
use super::provider::{provider_of, PackageProvider};

/// 升级的五个阶段。**顺序即执行序**（[`UpgradePhase::ALL`] 就是管道的画序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpgradePhase {
    /// 记录回滚点（我们干的，跑命令之前）
    Backup,
    /// 包管理器下载新版本
    Download,
    /// 包管理器校验（哈希 / 完整性）
    Verify,
    /// 包管理器替换安装
    Replace,
    /// 复验命令入口（我们干的，跑命令之后）
    Link,
}

impl UpgradePhase {
    pub const ALL: [UpgradePhase; 5] = [
        UpgradePhase::Backup,
        UpgradePhase::Download,
        UpgradePhase::Verify,
        UpgradePhase::Replace,
        UpgradePhase::Link,
    ];

    /// 界面上那一格的字。
    pub fn label(self) -> &'static str {
        match self {
            UpgradePhase::Backup => "备份",
            UpgradePhase::Download => "下载",
            UpgradePhase::Verify => "校验",
            UpgradePhase::Replace => "替换",
            UpgradePhase::Link => "链接",
        }
    }

    /// 在管道里的位置（0 起）。状态机靠它判断"这行输出说的是不是后面的事"——
    /// 乱序 / 重复的行不动状态（回退的段不取消已经跑完的段）
    pub fn index(self) -> usize {
        match self {
            UpgradePhase::Backup => 0,
            UpgradePhase::Download => 1,
            UpgradePhase::Verify => 2,
            UpgradePhase::Replace => 3,
            UpgradePhase::Link => 4,
        }
    }

    /// 中间那三段（包管理器干的）。收尾定界时它们与备份 / 链接不同：
    /// 从没被点亮的中间段记「未报告」，两端那两段是我们自己干的、
    /// 结局由我们写
    fn is_middle(self) -> bool {
        matches!(
            self,
            UpgradePhase::Download | UpgradePhase::Verify | UpgradePhase::Replace
        )
    }
}

/// 一个阶段此刻的样子。**六态各有各的诚实理由**，没有"默认 Done"这种懒档：
///
/// - [`Pending`](PhaseState::Pending) 等待：还没轮到（○）
/// - [`Running`](PhaseState::Running) 进行中：正在发生（●）
/// - [`Done`](PhaseState::Done) ✓ 带这一段自己的用时
/// - [`Failed`](PhaseState::Failed) ✗ 带用时（命令起不来 / 退出码非零）
/// - [`Cancelled`](PhaseState::Cancelled) 已取消：用户按停时正卡在这一段
/// - [`Unreported`](PhaseState::Unreported) 未报告：命令整体成功了，但包
///   管理器**全程没报**这一段（画 ✓ 是编，画 ✗ 是冤）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PhaseState {
    Pending,
    Running,
    Done(Duration),
    Failed(Duration),
    Cancelled,
    Unreported,
}

impl PhaseState {
    /// 跑完了没有（收尾判 `ok` 用）。**只有 `Done` 算**——未报告不算完，
    /// 它只是"没人告诉我们它完了"
    pub fn is_done(self) -> bool {
        matches!(self, PhaseState::Done(_))
    }

    /// 这一态用了多久（Done / Failed 才有；其余 `None`）。
    pub fn elapsed(self) -> Option<Duration> {
        match self {
            PhaseState::Done(d) | PhaseState::Failed(d) => Some(d),
            _ => None,
        }
    }
}

/// 管道里的一格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PhaseStatus {
    pub phase: UpgradePhase,
    pub state: PhaseState,
}

/// 一次升级的进度快照。**后台线程产、UI 线程 120ms 轮询搬**（GUI 侧
/// `LogEvent::CliUpgrade`，与仓库侧 `LogEvent::Stage` 同一条轮询管道）。
#[derive(Clone, PartialEq, Debug)]
pub struct UpgradeProgress {
    pub phases: [PhaseStatus; 5],
    /// 包管理器自己报的百分比。**`None` = 不定态**（拿不到就不造，模块头纪律 2）
    pub percent: Option<f32>,
    /// 进度带那一行字：此刻在干什么（"正在下载…"）。
    /// 与 [`UpgradeProgress::percent`] 独立：一个是阶段话，一个是数字
    pub detail: String,
    /// 用户点了取消（命令可能还在被杀掉的路上）
    pub cancelled: bool,
}

impl UpgradeProgress {
    /// 开局：五格全「等待」。第一份快照由这个来——**不是"备份进行中"**，
    /// 备份是我们跑的第一件事，它自己的进行态下一秒就到
    pub fn new() -> Self {
        UpgradeProgress {
            phases: UpgradePhase::ALL.map(|phase| PhaseStatus {
                phase,
                state: PhaseState::Pending,
            }),
            percent: None,
            detail: String::new(),
            cancelled: false,
        }
    }

    /// 某一格此刻的状态（每帧现读，不缓存）。
    pub fn state_of(&self, phase: UpgradePhase) -> PhaseState {
        self.phases
            .iter()
            .find(|s| s.phase == phase)
            .map(|s| s.state)
            // 结构上到不了（五格恒定）；真到了当"等待"——那是所有状态里
            // 唯一不声称任何事的
            .unwrap_or(PhaseState::Pending)
    }

    /// 置某一格的状态。
    pub fn set(&mut self, phase: UpgradePhase, state: PhaseState) {
        if let Some(s) = self.phases.iter_mut().find(|s| s.phase == phase) {
            s.state = state;
        }
    }

    /// 正在进行的那一段（管道上那颗 ●）。没有 = 没有一段在被输出点亮
    /// （命令在跑但包管理器还没报阶段，或已经收尾）
    pub fn current(&self) -> Option<UpgradePhase> {
        self.phases
            .iter()
            .find(|s| s.state == PhaseState::Running)
            .map(|s| s.phase)
    }

    /// 「第 k/n 步」：k = 第一格没完成的位置 + 1（从 1 数）。全完成了
    /// 给 (5, 5)。**没有正在进行的段时不表示进行到第几步**——那个数
    /// 挂在"当前段"上，没有当前段就没有第几步（调用方据此不画）
    pub fn step_no(&self) -> Option<(usize, usize)> {
        let n = self.phases.len();
        let k = self
            .phases
            .iter()
            .position(|s| !s.state.is_done())
            .map(|i| i + 1)
            .unwrap_or(n);
        Some((k, n))
    }
}

impl Default for UpgradeProgress {
    fn default() -> Self {
        Self::new()
    }
}

/// 回滚点：升级**之前**这颗包的样子（备份阶段的产物）。
///
/// P0 只记录、不复制文件（复制安装目录是待建 #6「升级前自动备份 + 一键
/// 回滚」的事）——包管理器装的包，回滚本来就是"装回旧版"，这份记录回答
/// "旧版是哪一个"。升级失败时结论那句"当前仍是 1.1.42"也从这里来
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RollbackPoint {
    pub version: Option<String>,
    pub install_path: Option<String>,
    pub executables: Vec<String>,
    /// 记录于（秒级时间戳）
    pub taken_at: u64,
}

impl RollbackPoint {
    /// 日志里那一行：`· 回滚点：pnpm 1.1.42`。没有版本就说没有版本——
    /// 不写"未知版本"那种假装记录过的话
    pub fn line(&self, name: &str) -> String {
        match &self.version {
            Some(v) => format!("· 回滚点：{name} {v}"),
            None => format!("· 回滚点：{name}（没能读到当前版本）"),
        }
    }
}

/// 取消旗号。后台执行器每个阶段边界都看它一眼；界面点「取消升级」即置位。
///
/// **`Clone` 是廉价的**（里面就一个 `Arc`）：界面存一份、后台线程拿一份，
/// 两边看得是同一个字节。`Default` 给"没人会取消"的开局用
#[derive(Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// 置位。**幂等**——第二次点取消（或收尾帧的清理）不会把它复位
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// 给 [`exec::run_program_live`] 的那一个（它只认裸旗号，不认这个类型）
    pub(crate) fn flag(&self) -> &AtomicBool {
        &self.flag
    }
}

/// 一次升级跑完的结论。
pub struct UpgradeOutcome {
    /// 整趟成功了（命令这一程 + 链接复验都过）。**取消 / 超时 / 起不来
    /// 都是 `false`**——调用方用 [`cancelled`](Self::cancelled) 与
    /// [`error`](Self::error) 分辨是哪一种
    pub ok: bool,
    pub cancelled: bool,
    /// 退出码；被信号杀掉 / 起不来 / 超时为 `None`
    pub code: Option<i32>,
    /// 起不来 / 超时的一句话（跑而不成是另一条：progress 里那一格是 ✗）
    pub error: Option<String>,
    pub progress: UpgradeProgress,
    pub rollback: RollbackPoint,
    pub stdout: String,
    pub stderr: String,
}

/// 跑一次阶段化升级（P0 的核心入口）。
///
/// 四步：**备份**（记录回滚点）→ **命令**（流式 + 阶段检测 + 可取消）→
/// **收尾定界**（进行中的那段收计时；从没被点亮的中间段记未报告）→
/// **链接**（命令成功才跑：复验命令入口还在不在 PATH 上）。
///
/// 两个回调都在调用方的后台线程上同步调（与 `execute::run_plan` 的
/// `on_step` 同一条纪律）：
/// - `on_line` 每行输出调一次（**含我们自己发的阶段行**——界面整条搬进
///   终端卡，用户看见的和库里发生的是同一串）
/// - `on_progress` 每次**快照变化**调一次（阶段切换 / 百分比变化 / 取消）
///
/// 两个回调都是 `FnMut`：调用方多半要往缓冲里攒东西（测试直接、界面经
/// `Arc<Mutex>`），`Fn` 会逼着他们套一层 `RefCell`
pub fn run_upgrade(
    pkg: &CliPackage,
    step: &Step,
    cancel: &CancelToken,
    mut on_line: impl FnMut(&str),
    mut on_progress: impl FnMut(&UpgradeProgress),
) -> UpgradeOutcome {
    let mut progress = UpgradeProgress::new();

    // ① 备份：回滚点。**在旧版本被替换之前**——顺序反了记下来的就是新版本
    let t_backup = Instant::now();
    let rollback = RollbackPoint {
        version: pkg.version.clone(),
        install_path: pkg.install_path.clone(),
        executables: pkg.executables.clone(),
        taken_at: crate::store::now_secs(),
    };
    on_line(&rollback.line(&pkg.name));
    progress.set(
        UpgradePhase::Backup,
        PhaseState::Done(t_backup.elapsed()),
    );
    progress.detail = "已记录回滚点".to_string();
    on_progress(&progress);

    // ② 命令：流式跑。observer 按 provider 取——**认不出阶段的 provider
    //    不猜**（trait 默认 None），它的中段回头记「未报告」
    let observer: Option<Box<dyn PackageProvider>> = provider_of(pkg.provider);
    let args: Vec<&str> = step.args.iter().map(String::as_str).collect();
    // 状态机三件套：点亮到哪一段了、每段什么时候开始的、快照变没变
    let mut cur: Option<UpgradePhase> = None;
    let mut started = [Instant::now(); 5];
    let live = exec::run_program_live(
        &step.program,
        &args,
        super::execute::OP_TIMEOUT,
        cancel.flag(),
        |line| {
            on_line(line);
            let mut changed = false;
            // 阶段检测：认不出来就当没这行（维持原状）。**只向前**——
            // 乱序 / 重复的行不动状态机
            if let Some(ph) = observer.as_ref().and_then(|o| o.phase_of_line(line)) {
                if cur.is_none_or(|c| ph.index() > c.index()) {
                    // 上一段收计时（它一直在 Running，此刻有了终点）
                    if let Some(c) = cur {
                        progress.set(c, PhaseState::Done(started[c.index()].elapsed()));
                    }
                    cur = Some(ph);
                    started[ph.index()] = Instant::now();
                    progress.set(ph, PhaseState::Running);
                    changed = true;
                }
            }
            // 百分比：有就覆盖，没有就不动（不定态时保持 None，不挂旧数）
            if let Some(p) = parse_percent(line) {
                progress.percent = Some(p);
                changed = true;
            }
            if changed {
                on_progress(&progress);
            }
        },
    );

    // ③ 收尾定界。三种终局各自的诚实写法。**共同的不变式：管道里恰好
    //    一个终态标记**——成功时那是最后点亮的那段（✓），失败 / 取消时
    //    是断裂的那一段。一段都没点亮过时，断裂点落在包管理器那三段里
    //    的哪里无从得知，记在第一段（下载）是它诚实的上界：命令确实跑了
    //    且断了，具体断在哪一下，日志里那行 ✗ 与 error 说
    let (command_ok, cancelled, code, error) = match &live {
        Err(e) => (false, false, None, Some(e.clone())),
        Ok(o) => {
            let killed = o.code.is_none();
            let cancelled = killed && cancel.is_cancelled();
            // 被杀也给 code None；取消旗号没置位就是超时（exec 那头杀的）
            let error = if killed && !cancelled {
                Some(format!(
                    "{} 超过 {} 秒",
                    step.program,
                    super::execute::OP_TIMEOUT.as_secs()
                ))
            } else {
                None
            };
            (o.ok() && !killed, cancelled, o.code, error)
        }
    };
    if cancelled {
        let c = cur.unwrap_or(UpgradePhase::Download);
        progress.set(c, PhaseState::Cancelled);
        progress.cancelled = true;
        progress.detail = "已取消".to_string();
        on_line("· 升级已取消");
    } else if command_ok {
        // 成功：点亮过的那段收计时。**一段都没点亮 ⇒ 三段全记未报告**
        // （下面共用那段）——不拿 Done 冒充"我们看见它跑完了"
        if let Some(c) = cur {
            progress.set(c, PhaseState::Done(started[c.index()].elapsed()));
        }
        progress.detail = "命令跑完了，正在复验命令入口…".to_string();
    } else {
        let c = cur.unwrap_or(UpgradePhase::Download);
        progress.set(c, PhaseState::Failed(started[c.index()].elapsed()));
        // 失败那一行与 `execute::StepOutcome::line` 同形（✗ + 命令 + 退出码）：
        // 管道里可能一格 ✗ 都没有（没有任何输出点亮过段落），**结论必须
        // 在终端日志里看得见**——两处各说各的是本项目最贵的错
        let shown = format!("✗ {} {}", step.program, args.join(" "));
        on_line(&match code {
            Some(c) => format!("{shown}（退出码 {c}）"),
            None => shown,
        });
        progress.detail = error
            .clone()
            .unwrap_or_else(|| "命令没成功，详见操作日志".to_string());
    }
    // 三种终局共用：从没被点亮的中间段记「未报告」。包管理器什么都没
    // 告诉我们——那就不该留着四格"等待"（等一个不会来的东西），也不该
    // 拿 Done 冒充。**备份与链接不算**：那两段是我们自己干的，结局由
    // 我们写，不欠包管理器一句话
    for ph in UpgradePhase::ALL.iter().filter(|p| p.is_middle()) {
        if progress.state_of(*ph) == PhaseState::Pending {
            progress.set(*ph, PhaseState::Unreported);
        }
    }
    // 包管理器的百分比到这儿就停了：后面的链接复验是另一回事，
    // 挂着一个 100% 会让那条 bar 看着"早就完了"
    progress.percent = None;
    on_progress(&progress);

    // ④ 链接：复验命令入口。**命令成功不等于装好了**——shim 没更新 /
    //    PATH 没改，屏幕上该看见这一格是红的。取消 / 失败不跑（没有
    //    "链接成功了"这一说）
    if command_ok && !cancelled {
        let t_link = Instant::now();
        progress.set(UpgradePhase::Link, PhaseState::Running);
        progress.detail = "正在复验命令入口…".to_string();
        on_progress(&progress);
        let missing: Vec<&str> = pkg
            .executables
            .iter()
            .filter(|e| !exec::command_exists(e))
            .map(|s| s.as_str())
            .collect();
        let (state, line) = if pkg.executables.is_empty() {
            (
                PhaseState::Done(t_link.elapsed()),
                "· 链接：这颗包没有记录命令入口，跳过复验".to_string(),
            )
        } else if missing.is_empty() {
            (
                PhaseState::Done(t_link.elapsed()),
                format!("· 链接：命令入口正常（{}）", pkg.executables.join("、")),
            )
        } else {
            (
                PhaseState::Failed(t_link.elapsed()),
                format!(
                    "· 链接：找不到 {}——升级之后命令入口不见了",
                    missing.join("、")
                ),
            )
        };
        on_line(&line);
        progress.set(UpgradePhase::Link, state);
        progress.detail = "复验完成".to_string();
        on_progress(&progress);
    }

    let ok = command_ok && !cancelled && progress.state_of(UpgradePhase::Link).is_done();
    let (stdout, stderr) = match live {
        Ok(o) => (o.stdout, o.stderr),
        Err(_) => (String::new(), String::new()),
    };
    UpgradeOutcome {
        ok,
        cancelled,
        code,
        error,
        progress,
        rollback,
        stdout,
        stderr,
    }
}

/// 从一行输出里解析包管理器报的百分比。**取行内最后一个 `NN%`**——
/// 进度条回车刷新时一行里会有好几个，最后一个是当前值。
///
/// 认不出来给 `None`：**没有百分比不比假的百分比好**（GUI 侧
/// `widgets::progress_readout` 同一判据：`None` 走不定态扫光）。
/// 不引 regex：core 没有这个依赖，而"数字段后紧跟 %"就够认了——
/// `26.04`（版本号）、`24.6 MB`（体积）都不跟 %，天然不进
pub fn parse_percent(line: &str) -> Option<f32> {
    // \r 分段：进度条的中间刷新态取最后一段（与 `exec::pump_lines` 同一条）
    let tail = line.rsplit('\r').next().unwrap_or(line);
    let bytes = tail.as_bytes();
    let mut best: Option<f32> = None;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() || bytes[i] == b'.' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            // 数字段后紧跟 % 才算（`12%` ✓；`12 %` 不认——包管理器不那么写）
            if i < bytes.len() && bytes[i] == b'%' {
                if let Ok(v) = tail[start..i].parse::<f32>() {
                    if (0.0..=100.0).contains(&v) {
                        best = Some(v);
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::model::{PackageKind, ProviderId};

    fn pkg() -> CliPackage {
        CliPackage {
        program_size: None,
        release_notes: None,
            id: "winget:Bun.Bun".into(),
            provider: ProviderId::Winget,
            name: "Bun".into(),
            version: Some("1.1.42".into()),
            latest: Some("1.1.43".into()),
            install_path: Some("C:\\Users\\x\\.bun".into()),
            executables: vec!["bun".into()],
            kind: PackageKind::CliOnly,
            dirs: Vec::new(),
        }
    }

    /// 开局：五格全「等待」，没有一段在进行——**没有当前段就没有
    /// "第几步"**（`step_no` 此时也不挂数）
    #[test]
    fn a_fresh_progress_has_nothing_running() {
        let p = UpgradeProgress::new();
        assert!(p.current().is_none());
        assert!(p.percent.is_none());
        for ph in UpgradePhase::ALL {
            assert_eq!(p.state_of(ph), PhaseState::Pending);
        }
    }

    /// 「第 k/n 步」跟着第一格未完成的走：三段 Done ⇒ 第 4/5 步。
    #[test]
    fn step_no_counts_the_first_unfinished_phase() {
        let mut p = UpgradeProgress::new();
        for ph in [UpgradePhase::Backup, UpgradePhase::Download, UpgradePhase::Verify] {
            p.set(ph, PhaseState::Done(Duration::from_secs(1)));
        }
        assert_eq!(p.step_no(), Some((4, 5)));
        p.set(UpgradePhase::Replace, PhaseState::Done(Duration::ZERO));
        p.set(UpgradePhase::Link, PhaseState::Done(Duration::ZERO));
        assert_eq!(p.step_no(), Some((5, 5)), "全完成也挂 5/5，不归None");
    }

    /// 百分比解析：**认不出来就不给**，不猜。
    /// 判据全是"什么不算百分比"：版本号、体积、纯文字——
    /// 把它们读成进度就是屏幕上多一个假进度
    #[test]
    fn percent_parsing_refuses_to_guess() {
        assert_eq!(parse_percent("  ████░░░░ 42%"), Some(42.0));
        assert_eq!(parse_percent("[####----] 45%"), Some(45.0));
        assert_eq!(parse_percent("100% of 24.6 MB"), Some(100.0));
        assert_eq!(parse_percent("added 2 packages in 4s"), None);
        assert_eq!(parse_percent("版本 26.04"), None);
        assert_eq!(parse_percent("24.6 MB / 24.6 MB"), None);
        assert_eq!(parse_percent("下载 bun-1.1.43"), None);
        // 一行里好几个：取最后一个是当前值（进度条回车刷新）
        assert_eq!(parse_percent("12% 34%"), Some(34.0));
        // 越界的不认（150% 不是进度，是别的东西）
        assert_eq!(parse_percent("150%"), None);
    }

    /// winget 观测器：**喂 2026-10-15 本机 `winget download` 抓回的原文**
    /// （中文系统，`winget --version` = v1.29.380）。判据是真输出，
    /// 不是我以为的形态——只匹配英文关键词在中文系统上永远不中，
    /// 管道会全程「未报告」，那是把"没接上"画成"没有"
    #[test]
    fn winget_observer_reads_real_captured_output() {
        let w = crate::cli::provider::Winget;
        // 下载（中文原文）
        assert_eq!(
            w.phase_of_line("正在下载 https://www.7-zip.org/a/7z2604-x64.msi"),
            Some(UpgradePhase::Download)
        );
        // 校验：中文原文。⚠️ 它含"安装程序"四个字，但说的是哈希验证
        assert_eq!(
            w.phase_of_line("已成功验证安装程序哈希"),
            Some(UpgradePhase::Verify)
        );
        // 下载完成（中文原文）
        assert_eq!(
            w.phase_of_line("已下载安装程序: C://Users//x//7z.msi"),
            Some(UpgradePhase::Download)
        );
        // 认不出来的行：None（Found 行 / 许可行 / 空行）
        assert_eq!(w.phase_of_line("已找到 7-Zip [7zip.7zip] 版本 26.04"), None);
        assert_eq!(w.phase_of_line("此应用程序由其所有者授权给你。"), None);
        // 英文形态（别的语言区）：同一批关键词的另一半
        assert_eq!(
            w.phase_of_line("Downloading https://example.com/x.msi"),
            Some(UpgradePhase::Download)
        );
        assert_eq!(
            w.phase_of_line("Successfully verified installer hash"),
            Some(UpgradePhase::Verify)
        );
        assert_eq!(
            w.phase_of_line("Installing..."),
            Some(UpgradePhase::Replace)
        );
    }

    /// 观测器的默认档：**认不出就不猜**。一个没覆写的 provider 对任何
    /// 输出都答 None——它的中段在命令成功后记「未报告」，而不是按印象
    /// 点一段亮
    #[test]
    fn the_default_observer_never_guesses() {
        let p = crate::cli::provider::Pipx;
        assert_eq!(p.phase_of_line("Downloading something"), None);
        assert_eq!(p.phase_of_line("Installing"), None);
        assert_eq!(p.phase_of_line("installed package black 1.2.3"), None);
    }

    /// npm 观测器：管道模式下 npm 只有一句 `added N packages`
    /// （2026-10-15 本机实测 `npm install --prefix <tmp>` 全程输出）。
    /// 那句话是 reify（落盘那一步）的结论 ⇒ 替换。下载 / 校验在管道里
    /// 不上屏 ⇒ 那两段「未报告」，不画 ✓
    #[test]
    fn npm_observer_reads_the_reify_line() {
        let n = crate::cli::provider::Npm;
        assert_eq!(
            n.phase_of_line("added 2 packages in 4s"),
            Some(UpgradePhase::Replace)
        );
        assert_eq!(
            n.phase_of_line("removed 2 packages, and changed 5 packages in 12s"),
            Some(UpgradePhase::Replace)
        );
        // 别的行说明不了阶段（deprecated 警告里也有 package 字样，
        // 但它不是以动作开头的结论行）
        assert_eq!(
            n.phase_of_line("npm warn deprecated left-pad@1.3.0: use padStart"),
            None
        );
        assert_eq!(n.phase_of_line("up to date"), None);
    }

    /// 取消旗号：置位、可读、Clone 后同一字节。
    #[test]
    fn cancel_token_is_one_shared_flag() {
        let t = CancelToken::new();
        assert!(!t.is_cancelled());
        let t2 = t.clone();
        t.cancel();
        assert!(t2.is_cancelled(), "克隆体看得见同一个字节");
        // 幂等：再取消一次不复位
        t.cancel();
        assert!(t.is_cancelled());
    }

    /// **真进程**上的取消：`ping -n 30` 跑 30 秒，200ms 后取消 ⇒
    /// 命令必须被杀掉、进度记取消。判据是"它真的停了"——拿一个 200ms
    /// 就跑完的假命令验取消，验的只是旗号本身，不是杀进程那条路。
    ///
    /// ping 的输出里没有任何阶段标记 ⇒ **没有任何一段被点亮**：取消时
    /// 就没有"正卡在哪一段"可说（不猜一个）。中间段按三种终局共用的
    /// 那条定界记「未报告」；链接没跑（没有"取消之后还链接成功了"）
    #[test]
    fn cancelling_kills_a_real_long_running_command() {
        let cancel = CancelToken::new();
        let c2 = cancel.clone();
        // 后台线程当"用户"：200ms 后点取消
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            c2.cancel();
        });
        let step = Step::uninstall("ping", &["-n", "30", "127.0.0.1"]);
        let t0 = Instant::now();
        let mut lines = Vec::new();
        let out = run_upgrade(
            &pkg(),
            &step,
            &cancel,
            |l| lines.push(l.to_string()),
            |_| {},
        );
        assert!(t0.elapsed() < Duration::from_secs(10), "取消必须真把进程杀了：{:?}", t0.elapsed());
        assert!(out.cancelled, "取消旗号置位且进程被杀了");
        assert!(!out.ok);
        assert!(out.progress.cancelled);
        assert!(lines.iter().any(|l| l.contains("升级已取消")), "{lines:?}");
        // 备份完成了（我们干的，跑在命令之前）
        assert!(out.progress.state_of(UpgradePhase::Backup).is_done());
        // 没有任何一段被输出点亮过 ⇒ 终态标记落在第一段（诚实的上界），
        // 后两段记未报告；链接没跑
        assert_eq!(
            out.progress.state_of(UpgradePhase::Download),
            PhaseState::Cancelled,
            "一段都没点亮时，断裂标记记在第一段"
        );
        for ph in [UpgradePhase::Verify, UpgradePhase::Replace] {
            assert_eq!(
                out.progress.state_of(ph),
                PhaseState::Unreported,
                "{ph:?}：没被点亮过的段落在任何终局下都记未报告"
            );
        }
        assert_eq!(out.progress.state_of(UpgradePhase::Link), PhaseState::Pending);
    }

    /// 一条短命令跑通的完整形状：备份 ✓ → 命令（认不出阶段 ⇒ 中段
    /// 全部「未报告」）→ 链接 ✗（`where bun` 在这台机器上大概率没有）。
    /// **这一条同时钉住"未报告"的定界**：命令成功了，但没有任何一行
    /// 输出点亮中间段——记未报告，不是 Done，更不是 Pending。
    /// 链接红 ⇒ 整趟不 ok：**命令成功不等于装好了**（shim 没更新 /
    /// PATH 没改，屏幕上该看见这一格是红的）
    #[test]
    fn a_plain_command_finishes_with_unreported_middle() {
        // 命令入口用一个 PATH 上必然没有的名字 => 链接段确定性失败
        // （不拿"本机大概没装 bun"当判据——测试不许看机器下菜碟）
        let mut p = pkg();
        p.executables = vec!["qb-no-such-program-42".into()];
        let step = Step::uninstall("cmd", &["/C", "echo", "hello"]);
        let cancel = CancelToken::new();
        let mut lines = Vec::new();
        let out = run_upgrade(&p, &step, &cancel, |l| lines.push(l.to_string()), |_| {});
        assert!(!out.cancelled);
        // 我们自己发的阶段行都在日志里（回滚点 + 链接结论）
        assert!(lines.iter().any(|l| l.contains("回滚点")), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("链接")), "{lines:?}");
        // 中段三格：未报告（不是 Done——没有任何输出点亮过它们）
        for ph in [UpgradePhase::Download, UpgradePhase::Verify, UpgradePhase::Replace] {
            assert_eq!(
                out.progress.state_of(ph),
                PhaseState::Unreported,
                "{ph:?} 该记未报告"
            );
        }
        // 链接复验真的跑了且失败了（这颗包的 bun 在这台机器上不存在）
        assert!(matches!(
            out.progress.state_of(UpgradePhase::Link),
            PhaseState::Failed(_)
        ));
        assert!(!out.ok, "链接红 ⇒ 整趟不 ok（结论由调用方说，这里只钉状态）");
    }

    /// 链接复验过的形状：一颗**真在 PATH 上**的命令（`cmd` 自己）。
    /// 与上一条一对：上一条钉"找不到 ⇒ ✗"，这一条钉"找得到 ⇒ ✓"，
    /// 两头的判据都落到实处，不留"应该没问题"
    #[test]
    fn the_link_check_passes_for_a_real_executable() {
        let mut p = pkg();
        p.executables = vec!["cmd".into()];
        let step = Step::uninstall("cmd", &["/C", "echo", "hi"]);
        let cancel = CancelToken::new();
        let out = run_upgrade(&p, &step, &cancel, |_| {}, |_| {});
        assert!(out.ok, "命令成功 + 入口在 ⇒ 整趟成功");
        assert!(out.progress.state_of(UpgradePhase::Link).is_done());
        // 每段都有用时（Done 带 Duration）——"分步计时"四个字落在类型上
        for ph in UpgradePhase::ALL {
            if ph == UpgradePhase::Download
                || ph == UpgradePhase::Verify
                || ph == UpgradePhase::Replace
            {
                continue; // 中段未报告（echo 没有阶段标记）
            }
            assert!(
                out.progress.state_of(ph).elapsed().is_some(),
                "{ph:?} 该带耗时"
            );
        }
    }

    /// 跑而失败：PATH 上没有的那个程序。注意：经 `cmd /C` 起，**cmd 自己
    /// 起得来**，是它在输出里说"'qb-no-such-program-42' 不是内部或外部命令"
    /// 并退出码 1——所以走的是"跑而不成"（code Some(1)），不是"起不来"
    /// （Err）那条路。判据：不 panic、结论说人话、管道上第一段记 ✗
    /// （一段都没点亮时断裂标记的落点），✗ 行进日志
    #[test]
    fn a_command_that_runs_and_fails_is_a_verdict_not_a_panic() {
        let step = Step::uninstall("qb-no-such-program-42", &["upgrade"]);
        let cancel = CancelToken::new();
        let mut lines = Vec::new();
        let out = run_upgrade(&pkg(), &step, &cancel, |l| lines.push(l.to_string()), |_| {});
        assert!(!out.ok);
        assert!(!out.cancelled, "失败不是取消");
        assert_eq!(out.code, Some(1), "cmd 报的退出码");
        assert!(out.error.is_none(), "跑而不成不走 Err 那条路");
        // 备份照样完成了（它跑在命令之前）；命令那一段 ✗
        assert!(out.progress.state_of(UpgradePhase::Backup).is_done());
        assert!(matches!(
            out.progress.state_of(UpgradePhase::Download),
            PhaseState::Failed(_)
        ));
        // ✗ 行在日志里：命令 + 退出码（与 execute::StepOutcome::line 同形）
        assert!(
            lines.iter().any(|l| l.starts_with('✗') && l.contains("退出码 1")),
            "{lines:?}"
        );
    }

    /// 回滚点的内容与日志行：升级前那颗包的样子**原样**记下来。
    /// 判据是"没有版本就说没有"——不写"未知版本"那种假装记录过的话
    #[test]
    fn the_rollback_point_records_the_pre_upgrade_state() {
        let p = pkg();
        let rp = RollbackPoint {
            version: p.version.clone(),
            install_path: p.install_path.clone(),
            executables: p.executables.clone(),
            taken_at: 1,
        };
        assert_eq!(rp.line("Bun"), "· 回滚点：Bun 1.1.42");
        let empty = RollbackPoint {
            version: None,
            install_path: None,
            executables: Vec::new(),
            taken_at: 1,
        };
        assert!(empty.line("Bun").contains("没能读到当前版本"), "{:?}", empty.line("Bun"));
    }

    /// 端到端：一条命令回显两行**真的** winget 观测器关键词（英文那半），
    /// 管道上 下载 ✓ → 校验 ✓ 依次点亮、各带计时，第三段（替换）从没被
    /// 点亮 ⇒ 未报告。判据落在 `run_upgrade` 这一层：输出 → 检测 → 推进
    /// → 收计时 整条链，不是只验 `phase_of_line` 那一格
    /// （`cmd /C echo A & echo B` 是被子进程回显的两行，冒充包管理器输出）
    #[test]
    fn the_phase_machine_advances_on_real_observer_lines() {
        let step = Step::uninstall(
            "cmd",
            &[
                "/C",
                "echo",
                "Downloading",
                "https://x/y.msi",
                "&",
                "echo",
                "Successfully",
                "verified",
                "installer",
                "hash",
            ],
        );
        // 命令入口用 cmd（必然在 PATH 上）=> 链接段确定性成功，
        // 与未报告的中段分开断言，不互相借判据
        let mut p = pkg();
        p.executables = vec!["cmd".into()];
        let cancel = CancelToken::new();
        let mut snaps: Vec<UpgradeProgress> = Vec::new();
        let out = run_upgrade(&p, &step, &cancel, |_| {}, |p| snaps.push(p.clone()));
        // 管道走过的顺序：下载先亮，校验后亮（快照序列里出现过）
        let seen: Vec<UpgradePhase> = snaps.iter().filter_map(|p| p.current()).collect();
        assert!(seen.contains(&UpgradePhase::Download), "{seen:?}");
        assert!(seen.contains(&UpgradePhase::Verify), "{seen:?}");
        assert!(
            seen.iter().position(|p| *p == UpgradePhase::Verify)
                > seen.iter().position(|p| *p == UpgradePhase::Download),
            "校验在下载之后才亮：{seen:?}"
        );
        // 终局：下载 ✓ 校验 ✓（各带计时），替换未报告，链接 ✗（bun 不在 PATH）
        assert!(out.progress.state_of(UpgradePhase::Download).is_done());
        assert!(out.progress.state_of(UpgradePhase::Verify).is_done());
        assert!(
            out.progress
                .state_of(UpgradePhase::Download)
                .elapsed()
                .is_some(),
            "✓ 要带这一段的用时（分步计时）"
        );
        assert_eq!(
            out.progress.state_of(UpgradePhase::Replace),
            PhaseState::Unreported
        );
        assert!(
            out.progress.state_of(UpgradePhase::Link).is_done(),
            "cmd 在 PATH 上 => 链接复验过"
        );
        assert!(out.ok, "命令成功 + 链接过 => 整趟成功");
    }
}
