//! 定时自动同步调度器：后台线程每 30 秒扫描一次配置，
//! 对用户开启了自动同步的仓库按各自间隔执行"检测云端更新（→ 自动拉取）"。
//! 只处理 auto_sync_mins > 0 的仓库；与用户操作通过 commands::try_begin_op 互斥。

use crate::{commands, store};
use serde::Serialize;
use std::time::Duration;
use tauri::Emitter;

/// 扫描周期：调度粒度的一半，间隔设置的最小档位（15 分钟）远大于它
const TICK_SECS: u64 = 30;
/// 启动后先等一轮再开始，避免开机启动时立刻抢网络
const STARTUP_DELAY_SECS: u64 = 60;
/// 相邻两个仓库的同步间隔，避免连续网络请求
const BETWEEN_REPOS_DELAY_SECS: u64 = 3;

/// 推送给前端的自动同步事件
#[derive(Serialize, Clone)]
pub struct AutoSyncEvent {
    pub path: String,
    /// 展示名（alias 优先），供提示语直接使用
    pub name: String,
    /// "pulled" 自动拉取成功 / "behind" 云端有更新待手动拉取 /
    /// "ok" 已是最新 / "skip" 有未提交变更跳过拉取 / "error" 失败
    pub kind: String,
    pub message: String,
    /// 自动拉取成功时的新提交列表与文件变更摘要
    pub commits: Vec<String>,
    pub diff_stat: String,
    /// 事件发生后的完整仓库列表（含最新 last_auto_check / last_auto_result）
    pub repos: Vec<store::RepoEntry>,
}

/// 启动常驻调度线程
pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(STARTUP_DELAY_SECS));
        loop {
            tick(&app);
            std::thread::sleep(Duration::from_secs(TICK_SECS));
        }
    });
}

/// 间隔模式到期判定：上次检查距今是否已超过设定间隔
fn interval_due(mins: u32, last_check: u64, now: u64) -> bool {
    mins > 0 && now.saturating_sub(last_check) >= u64::from(mins) * 60
}

/// 每天定时模式到期判定：今天的执行时刻已过，且本轮（今天该时刻之后）还没跑过。
/// tz 抽象成参数便于单测；实际调用传 chrono::Local。
fn daily_is_due<Tz: chrono::TimeZone>(
    daily_time: &str,
    now: chrono::DateTime<Tz>,
    last_check: u64,
) -> bool {
    let Some((h, m)) = store::parse_hhmm(daily_time) else {
        return false;
    };
    let Some(today_run) = now
        .date_naive()
        .and_hms_opt(h, m, 0)
        .and_then(|t| t.and_local_timezone(now.timezone()).single())
    else {
        return false;
    };
    let today_run = today_run.timestamp();
    now.timestamp() >= today_run && (last_check as i64) < today_run
}

/// 扫描一遍到期仓库并逐个同步；任何时刻只处理一个仓库（串行，不做并发轰炸）
fn tick(app: &tauri::AppHandle) {
    let cfg = store::load(app);
    let now = store::now_secs();
    let now_local = chrono::Local::now();
    let due: Vec<String> = cfg
        .repos
        .iter()
        .filter(|r| {
            interval_due(r.auto_sync_mins, r.last_auto_check, now)
                || daily_is_due(&r.daily_time, now_local, r.last_auto_check)
        })
        .map(|r| r.path.clone())
        .collect();

    for path in due {
        // 执行前重读配置：仓库可能刚被移除，或用户刚改了间隔 / 关了开关
        let Some(entry) = store::load(app).repos.into_iter().find(|r| r.path == path) else {
            continue;
        };
        if entry.auto_sync_mins == 0 {
            continue;
        }
        // 拿不到操作锁说明用户正在操作（或上一轮还没跑完），跳过本轮，下个 tick 再试
        let Some(_guard) = commands::try_begin_op() else {
            continue;
        };
        let event = run_one(app, entry);
        drop(_guard);
        let _ = app.emit("auto-sync-event", event);
        std::thread::sleep(Duration::from_secs(BETWEEN_REPOS_DELAY_SECS));
    }
}

/// 对单个仓库执行一次自动同步，并把结果写回配置
fn run_one(app: &tauri::AppHandle, entry: store::RepoEntry) -> AutoSyncEvent {
    let path = entry.path.clone();
    let name = if entry.alias.is_empty() {
        entry.name.clone()
    } else {
        entry.alias.clone()
    };

    let (kind, message, commits, diff_stat) = if !commands::valid_repo(&path) {
        (
            "error".to_string(),
            "仓库路径不存在（可能已被移动或删除）".to_string(),
            Vec::new(),
            String::new(),
        )
    } else {
        sync_repo(app, &path, entry.auto_pull)
    };

    // 结果写回配置（仓库可能刚被移除，找不到就跳过写回）
    let mut cfg = store::load(app);
    if let Some(e) = cfg.repos.iter_mut().find(|r| r.path == path) {
        e.last_auto_check = store::now_secs();
        e.last_auto_result = format!("{kind}:{message}");
    }
    let _ = store::save(app, &cfg);
    let repos = cfg.repos;

    AutoSyncEvent {
        name,
        path,
        kind,
        message,
        commits,
        diff_stat,
        repos,
    }
}

/// 检测 +（可选）自动拉取的核心判定，返回 (kind, message, commits, diff_stat)
fn sync_repo(
    app: &tauri::AppHandle,
    path: &str,
    auto_pull: bool,
) -> (String, String, Vec<String>, String) {
    let r = commands::do_check(app, path, None);
    if !r.ok {
        return ("error".into(), r.message, Vec::new(), String::new());
    }
    let Some(st) = &r.status else {
        return ("error".into(), "无法读取仓库状态".into(), Vec::new(), String::new());
    };
    if !st.has_upstream {
        return ("ok".into(), "当前分支没有上游分支，跳过".into(), Vec::new(), String::new());
    }
    if st.behind == 0 {
        return ("ok".into(), "已是最新".into(), Vec::new(), String::new());
    }
    if st.ahead > 0 {
        return (
            "error".into(),
            "本地与云端历史分叉，需手动处理，不自动拉取".into(),
            Vec::new(),
            String::new(),
        );
    }
    let behind_msg = format!("云端有 {} 个新提交", st.behind);
    if !auto_pull {
        return ("behind".into(), format!("{behind_msg}，可手动拉取"), Vec::new(), String::new());
    }
    if st.dirty {
        return (
            "skip".into(),
            format!("{behind_msg}，但本地有未提交变更，已跳过自动拉取"),
            Vec::new(),
            String::new(),
        );
    }

    let pr = commands::do_pull(app, path, false, None);
    if pr.ok {
        if let Some(changes) = pr.changes {
            let n = changes.commits.len();
            (
                "pulled".into(),
                format!("自动拉取成功（{n} 个新提交）"),
                changes.commits,
                changes.diff_stat,
            )
        } else {
            ("pulled".into(), "自动拉取成功".into(), Vec::new(), String::new())
        }
    } else {
        ("error".into(), format!("自动拉取失败：{}", pr.message), Vec::new(), String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn entry(mins: u32, last_check: u64) -> store::RepoEntry {
        store::RepoEntry {
            path: "C:/repo".into(),
            name: "repo".into(),
            alias: String::new(),
            added_at: 0,
            last_used: 0,
            test_cmd: String::new(),
            build_cmd: String::new(),
            auto_sync_mins: mins,
            daily_time: String::new(),
            auto_pull: false,
            last_auto_check: last_check,
            last_auto_result: String::new(),
            pinned: false,
        }
    }

    #[test]
    fn interval_due_only_enabled_and_expired() {
        let now = store::now_secs();
        assert!(!interval_due(0, 0, now), "关闭 → 不调度");
        assert!(!interval_due(15, now - 14 * 60, now), "未到期");
        assert!(interval_due(15, now - 16 * 60, now), "已到期");
        assert!(interval_due(60, 0, now), "从未跑过 → 立即到期");
    }

    #[test]
    fn parse_hhmm_validation() {
        assert_eq!(store::parse_hhmm("09:30"), Some((9, 30)));
        assert_eq!(store::parse_hhmm("9:5"), Some((9, 5)));
        assert_eq!(store::parse_hhmm("24:00"), None);
        assert_eq!(store::parse_hhmm("9:60"), None);
        assert_eq!(store::parse_hhmm("abc"), None);
        assert_eq!(store::parse_hhmm(""), None);
    }

    #[test]
    fn daily_due_fixed_timezone() {
        // 固定 +08:00 时区：2026-09-14 当天，执行时刻设为 09:30
        let tz = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let run_today = tz.with_ymd_and_hms(2026, 9, 14, 9, 30, 0).unwrap();
        let before_run = tz.with_ymd_and_hms(2026, 9, 14, 8, 0, 0).unwrap();
        let after_run = tz.with_ymd_and_hms(2026, 9, 14, 10, 0, 0).unwrap();
        // 昨天 20:00 跑过
        let last_yesterday = tz.with_ymd_and_hms(2026, 9, 13, 20, 0, 0).unwrap().timestamp() as u64;
        // 今天跑完之后的时刻
        let last_today = (run_today.timestamp() + 300) as u64;

        assert!(!daily_is_due("09:30", before_run, last_yesterday), "还没到今天的执行时刻");
        assert!(daily_is_due("09:30", after_run, last_yesterday), "过了执行时刻且今天没跑过 → 到期");
        assert!(!daily_is_due("09:30", after_run, last_today), "今天执行时刻之后已跑过 → 不重复");
        assert!(!daily_is_due("bad", after_run, last_yesterday), "非法时间串 → 永不到期");
        assert!(!daily_is_due("", after_run, last_yesterday), "空串 → 非每天定时模式");
    }

    #[test]
    fn result_summary_prefix_roundtrip() {
        let summary = format!("{}:{}", "pulled", "自动拉取成功（3 个新提交）");
        let (kind, msg) = summary.split_once(':').unwrap();
        assert_eq!(kind, "pulled");
        assert_eq!(msg, "自动拉取成功（3 个新提交）");
    }
}
