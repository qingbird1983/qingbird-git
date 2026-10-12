//! 前后端 IPC 共享的数据结构（状态、进度、操作结果）。

use serde::{Deserialize, Serialize};

use crate::git;
use crate::net::{percent_of, PROGRESS_STAGES};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct RepoStatus {
    pub is_repo: bool,
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub has_upstream: bool,
    /// 上游分支全名，如 `origin/main`。空 = 没有上游
    ///
    /// 这一条不额外跑命令：`read_status` 本来就要问 git 上游是谁
    /// （判断 `has_upstream`），那时候把 stdout 留下来即可
    pub upstream: String,
    pub remote_url: String,
    /// 有几条暂存（`git stash list` 的行数）。32-2 的那个出口靠它决定
    /// 仓库名字后面那颗橙色三角要不要出现。
    ///
    /// ⚠️ **`#[serde(default)]` 不是模板话**：这份结构会进 `scan_cache` 的 JSON，
    /// 老库里存的没有这一格 ⇒ 没有 default 就是**整条缓存反序列化失败**，
    /// 后果不是少一个数，是每个人的秒开缓存集体作废
    #[serde(default)]
    pub stash_count: usize,
    /// **领先/落后这一档读不出来**（32-12）。
    ///
    /// `git status --porcelain=v2 --branch --ahead-behind` 里那个 `--ahead-behind`
    /// 需要 **git ≥ 2.32**（2021-06）。低于它时这一条命令照样跑，但输出里没有
    /// `# branch.ab` 那一行 ⇒ `ahead`/`behind` 都会是 0，而 **0/0 与"没算过"
    /// 在两个数上长得一模一样**——这正是本项目最恨的那一档
    /// （「没扫过不许写成已同步」，31.4 补十五的 `has_status` 是同一族）。
    ///
    /// ⚠️ **极性是 `unknown = true` 而不是 `read = true`**：这份结构会进 `scan_cache`
    /// 的 JSON，老库里存的没有这一格 ⇒ `#[serde(default)]` 落成 `false`（= 数字可信），
    /// **升级之后每个人原来的缓存照旧显示数字**。反过来命名会让所有人的领先/落后
    /// 一夜之间变成「–」——那是把一次无害的字段新增变成一次全员降级
    #[serde(default)]
    pub ab_unknown: bool,
}

/// 拉取/克隆时推送给前端的进度事件
#[derive(Serialize, Clone)]
pub struct ProgressEvent {
    pub stage: String,
    pub detail: String,
    pub percent: Option<f32>,
    /// **git 原文**这一行。日志区用它逐行上屏——用户要的是"像终端那样"，
    /// 而不是把 git 输出翻译成 `· 阶段 细节` 的二次转述。
    /// 解析不出来的行 `stage` / `detail` 留空，但 `raw` 一定非空
    pub raw: String,
    /// 该不该进日志。**进度刷新行（百分比 < 100）为 false**——
    /// 那类行每秒十几条，全进日志会把真正的记录冲掉；
    /// 完成那一条（100%）与无百分比的事件行为 true
    pub to_log: bool,
}

/// 一次拉取带回的变更量：改了几个文件、加了多少行、删了多少行。
///
/// ⚠️ **这三个数从 `git diff --numstat` 的制表符数字列聚合，不从 `--stat`
/// 那句「4 files changed, 5 insertions(+), 2 deletions(-)」里抠**：
/// 那句是给人看的措辞，会随 git 版本与语言环境变，而「前两列是数字」这个结构
/// 不会。与 `do_pull` 里"分叉用 ahead/behind 判、不猜 stderr 措辞"是同一条纪律
#[derive(Serialize, Clone, Copy, Default, Debug, PartialEq)]
pub struct DiffTotals {
    pub files: usize,
    pub insertions: usize,
    pub deletions: usize,
}

impl DiffTotals {
    /// 增删行数都是 0：只有二进制文件被换掉（`-\t-`）、或整趟没改动行数时成立。
    /// 界面据此**退成不带 `+A −B` 的那句**，而不是写「+0 −0」这种假精确
    pub fn has_line_counts(&self) -> bool {
        self.insertions > 0 || self.deletions > 0
    }
}

/// 拉取成功后的更新内容：新拉下来的提交与文件变更摘要
#[derive(Serialize, Clone)]
pub struct UpdateChanges {
    pub commits: Vec<String>,
    pub diff_stat: String,
    /// 数字摘要（界面上那句「这次带回 3 个提交 +120 −30，5 个文件」读它）。
    /// 与 `diff_stat` 并存不冗余：那一份是**逐文件的文本图**（自动同步的通知读它），
    /// 这一份是**三个数**（拉取结果那一行读它）
    pub totals: DiffTotals,
}

/// 分叉信息：本地与云端各自都有对方没有的提交（`ahead > 0 && behind > 0`）。
/// 由后端一次性算好——前端不做"消息里含不含『分叉』"这类字符串匹配。
#[derive(Serialize, Clone)]
pub struct ForkInfo {
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub branch: String,
    /// 上游分支全名，如 `origin/main`
    pub upstream: String,
    /// 本地未推送的提交（`@{u}..HEAD --oneline`）
    pub local_commits: Vec<String>,
    /// 云端新增的提交（`HEAD..@{u} --oneline`）
    pub remote_commits: Vec<String>,
    /// 非空时卡片只说明原因、不给动作：仓库正处于用户自己的变基/合并中途，向导不碰
    pub blocked_reason: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct OpResult {
    pub ok: bool,
    /// 面向用户的一句话结论
    pub message: String,
    /// git 原始输出（日志区展示）
    pub output: String,
    /// 走代理成功时记录所用代理，None 表示直连
    pub used_proxy: Option<String>,
    pub status: Option<RepoStatus>,
    /// pull 时本地有未提交变更，提示用户可选择 stash
    pub need_stash: bool,
    /// 拉取成功时的更新内容（新提交 + 文件变更摘要）
    pub changes: Option<UpdateChanges>,
    /// 检测/拉取发现分叉时的结构化信息，卡片据此出动作
    pub fork: Option<ForkInfo>,
    /// 本次变基刚建好的备份分支名：有这个值界面才提供"撤销变基"。
    /// 刻意只由变基动作返回，不去磁盘上扫——扫描到的可能是很久以前的备份，
    /// 那时"硬回退到备份"会丢掉用户后来提交的东西
    pub backup_branch: Option<String>,
}

impl OpResult {
    pub(crate) fn err(message: String, output: String) -> Self {
        OpResult {
            ok: false,
            message,
            output,
            used_proxy: None,
            status: None,
            need_stash: false,
            changes: None,
            fork: None,
            backup_branch: None,
        }
    }
}

pub(crate) fn combined(out: &git::GitOutput) -> String {
    // 进度刷新行（\r 重复输出）在进度条里已展示，日志里去掉避免刷屏
    //
    // **白名单就是 `net::PROGRESS_STAGES` 那一份**（2026-10-05 修）：原来这里
    // 另抄了一份 7 条的，漏了 `remote: Compressing` / `Unpacking` /
    // `Checking out`，于是"完成"那几行逃过过滤、又没被别处渲染，
    // 在日志里就成了半截。合成一份之后两边不可能再漂。
    //
    // 判据是**百分比 < 100**：完成那一条（100% / done）是要留的，
    // 它正是"这一步做完了"的记录。抠不出百分比的行**一律保留**
    // （`percent_of` 返 `None` ⇒ 不当刷新行；宁可多一行，也不要静默吞掉信息）。
    let clean = |text: &str| -> String {
        text.split('\n')
            .filter(|line| {
                let l = line.trim();
                if l.is_empty() {
                    return false;
                }
                let body = l.strip_prefix("remote:").map(str::trim_start).unwrap_or(l);
                !PROGRESS_STAGES
                    .iter()
                    .any(|(k, _)| body.starts_with(k) && percent_of(body).is_some_and(|p| p < 100.0))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut s = String::new();
    let stdout = clean(&out.stdout);
    let stderr = clean(&out.stderr);
    if !stdout.is_empty() {
        s.push_str(&stdout);
        s.push('\n');
    }
    if !stderr.is_empty() {
        s.push_str(&stderr);
        s.push('\n');
    }
    s
}
