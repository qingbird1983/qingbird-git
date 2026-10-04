//! 前后端 IPC 共享的数据结构（状态、进度、操作结果）。

use serde::{Deserialize, Serialize};

use crate::git;

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

/// 拉取成功后的更新内容：新拉下来的提交与文件变更摘要
#[derive(Serialize, Clone)]
pub struct UpdateChanges {
    pub commits: Vec<String>,
    pub diff_stat: String,
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
    // **白名单必须与 `net::parse_progress` 那份保持同步**（2026-10-05 修）：
    // 原来这里只有 7 条且漏了 `remote: Compressing` / `Unpacking` /
    // `Checking out`，于是"完成"那几行逃过过滤、又没被别处渲染，
    // 在日志里就成了半截。
    //
    // 判据是**百分比 < 100**：完成那一条（100% / done）是要留的，
    // 它正是"这一步做完了"的记录。两份各写一遍规则迟早漂——
    // 所以这里认不出的行**一律保留**（宁可多一行，也不要静默吞掉信息）。
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
                    .any(|k| body.starts_with(k) && percent_of(body) < 100.0)
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

/// git 进度行的前缀。与 `net::parse_progress` 的那份同源同值。
const PROGRESS_STAGES: &[&str] = &[
    "Updating files",
    "Checking out files",
    "Unpacking objects",
    "Counting objects",
    "Compressing objects",
    "Enumerating objects",
    "Receiving objects",
    "Resolving deltas",
    "Writing objects",
];

/// 从进度行里抠出百分比。抠不出（无 `%`）算 100——
/// **无百分比的行为一次性事件行，不是刷新行**，不该被滤掉。
fn percent_of(s: &str) -> f32 {
    let Some(idx) = s.find('%') else { return 100.0 };
    let Some(start) = s[..idx].rfind(|c: char| !c.is_ascii_digit() && c != '.') else {
        return 100.0;
    };
    s[start..idx].parse().unwrap_or(100.0)
}


