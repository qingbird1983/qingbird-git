//! 前后端 IPC 共享的数据结构（状态、进度、操作结果）。

use serde::Serialize;

use crate::git;

#[derive(Serialize, Clone, Default)]
pub struct RepoStatus {
    pub is_repo: bool,
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub has_upstream: bool,
    pub remote_url: String,
}

/// 拉取/克隆时推送给前端的进度事件
#[derive(Serialize, Clone)]
pub struct ProgressEvent {
    pub stage: String,
    pub detail: String,
    pub percent: Option<f32>,
}

/// 拉取成功后的更新内容：新拉下来的提交与文件变更摘要
#[derive(Serialize, Clone)]
pub struct UpdateChanges {
    pub commits: Vec<String>,
    pub diff_stat: String,
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
}

impl OpResult {
    pub(super) fn err(message: String, output: String) -> Self {
        OpResult {
            ok: false,
            message,
            output,
            used_proxy: None,
            status: None,
            need_stash: false,
            changes: None,
        }
    }
}

pub(super) fn combined(out: &git::GitOutput) -> String {
    // 进度刷新行（\r 重复输出）在进度条里已展示，日志里去掉避免刷屏
    const PROGRESS_NOISE: &[&str] = &[
        "Receiving objects",
        "Resolving deltas",
        "Updating files",
        "Counting objects",
        "Compressing objects",
        "Enumerating objects",
        "remote: Enumerating",
    ];
    let clean = |text: &str| -> String {
        text.split('\n')
            .filter(|line| {
                let l = line.trim();
                !l.is_empty() && !PROGRESS_NOISE.iter().any(|k| l.starts_with(k))
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

