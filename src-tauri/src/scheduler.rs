//! 定时自动同步调度器：后台线程每 30 秒扫描一次配置，
//! 对用户开启了自动同步的仓库按各自间隔执行"检测云端更新（→ 自动拉取）"。
//! 只处理 auto_sync_mins > 0 的仓库；与用户操作通过 commands::try_begin_op 互斥。
//! 每轮结果推给前端（`auto-sync-event`），并按设置开关同时发系统通知（见 `notify.rs`）。
//! "每天定时"模式在时刻没跑到时会补跑一次（见 `daily_is_due`）。

use crate::{commands, notify, store};
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

/// now 之前（含）最近一次的执行时刻（秒）；今天还没到点就是昨天那次。
/// tz 抽象成参数便于单测；实际调用传 chrono::Local。
fn last_due_moment<Tz: chrono::TimeZone>(
    now: chrono::DateTime<Tz>,
    h: u32,
    m: u32,
) -> Option<i64> {
    let at = |date: chrono::NaiveDate| {
        date.and_hms_opt(h, m, 0)
            .and_then(|t| t.and_local_timezone(now.timezone()).single())
    };
    let today = at(now.date_naive())?;
    if now.timestamp() >= today.timestamp() {
        return Some(today.timestamp());
    }
    // 昨天先按"日期减一天"再本地化，而不是给 DateTime 减 24 小时——
    // 后者遇到夏令时切换会整体偏一小时
    Some(at(now.date_naive().pred_opt()?)?.timestamp())
}

/// 每天定时模式到期判定：**最近一个已经到点的执行时刻**还没被跑过，就补跑一次。
///
/// 只盯"今天这个时刻"会整段漏掉一天：09-14 09:30 该跑时机器关着，
/// 09-15 08:00 开机时 `now < 今天的 09:30` → 不触发，于是 09-14 一次都没同步，
/// 还得干等到 09-15 09:30。改成回看"最近一个已到点的时刻"（今天过了算今天，
/// 没过算昨天）之后，开机那一轮就会补上。
///
/// 补跑会把 `last_auto_check` 刷新到当前时刻，而它只可能大于"昨天那个时刻"，
/// 所以连续关机几天也只补一次，不会攒成一串。
fn daily_is_due<Tz: chrono::TimeZone>(
    daily_time: &str,
    now: chrono::DateTime<Tz>,
    last_check: u64,
) -> bool {
    let Some((h, m)) = store::parse_hhmm(daily_time) else {
        return false;
    };
    match last_due_moment(now, h, m) {
        Some(target) => (last_check as i64) < target,
        None => false,
    }
}

/// 扫描一遍到期仓库并逐个同步；任何时刻只处理一个仓库（串行，不做并发轰炸）
fn tick(app: &tauri::AppHandle) {
    let cfg = store::load(app);
    // 通知总开关在设置弹窗里；关掉后仍然照常推前端事件，只是不弹系统通知
    let notify_on = cfg.settings.notify_auto_sync;
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
        // 前端不在（关窗后 Webview 已销毁）时系统通知是唯一的出口，所以不依赖 emit 的成败
        if notify_on {
            notify::auto_sync(app, &event);
        }
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
        // 昨天 20:00 跑过（晚于昨天 09:30 那次，所以昨天那次已被覆盖）
        let last_yesterday = tz.with_ymd_and_hms(2026, 9, 13, 20, 0, 0).unwrap().timestamp() as u64;
        // 今天跑完之后的时刻
        let last_today = (run_today.timestamp() + 300) as u64;

        assert!(
            !daily_is_due("09:30", before_run, last_yesterday),
            "今天还没到点、昨天那次也跑过了 → 不触发"
        );
        assert!(
            daily_is_due("09:30", run_today, last_yesterday),
            "刚好到点、还没跑过 → 该跑（判定含等号）"
        );
        assert!(daily_is_due("09:30", after_run, last_yesterday), "过了执行时刻且今天没跑过 → 到期");
        assert!(!daily_is_due("09:30", after_run, last_today), "今天执行时刻之后已跑过 → 不重复");
        assert!(!daily_is_due("bad", after_run, last_yesterday), "非法时间串 → 永不到期");
        assert!(!daily_is_due("", after_run, last_yesterday), "空串 → 非每天定时模式");
    }

    #[test]
    fn daily_due_catches_up_a_missed_previous_target() {
        // 回归：09-14 09:30 该跑时机器关着，09-15 08:00 才开机（还没到今天的 09:30）。
        // 旧实现只在"今天的时刻已过"时才判到期，于是 09-14 整天没同步，
        // 还要干等到 09-15 09:30；现在开机那一刻就会补一次。
        let tz = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let boot = tz.with_ymd_and_hms(2026, 9, 15, 8, 0, 0).unwrap();
        let stamp = |d: u32, h: u32, mi: u32| {
            tz.with_ymd_and_hms(2026, 9, d, h, mi, 0).unwrap().timestamp() as u64
        };

        let missed_yesterday = stamp(13, 9, 31); // 最后一次跑在 09-13
        let covered_yesterday = stamp(14, 9, 31); // 09-14 那次跑过了

        assert!(
            daily_is_due("09:30", boot, missed_yesterday),
            "昨天那次没跑 → 开机就该补"
        );
        assert!(
            !daily_is_due("09:30", boot, covered_yesterday),
            "昨天那次跑过了 → 不该多跑一次"
        );

        // 补跑会把 last_auto_check 刷到当前时刻，而它只可能大于"昨天那个时刻"：
        // 所以紧接着的下一轮、以及连续关机好几天，都只补这一次
        let after_catchup = (boot.timestamp() + 30) as u64;
        assert!(!daily_is_due("09:30", boot, after_catchup), "刚补过就不再补");
        let last_week = stamp(8, 9, 31);
        assert!(daily_is_due("09:30", boot, last_week), "关了一周也只补一次（本次）");
        assert!(
            !daily_is_due("09:30", boot, after_catchup),
            "补完之后不管上次跑得多早，都不再重复补"
        );
    }

    #[test]
    fn last_due_moment_picks_today_or_yesterday() {
        let tz = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let at = |d: u32, h: u32, mi: u32| {
            tz.with_ymd_and_hms(2026, 9, d, h, mi, 0).unwrap().timestamp()
        };
        // 已过点 → 今天那次
        assert_eq!(
            last_due_moment(tz.with_ymd_and_hms(2026, 9, 15, 10, 0, 0).unwrap(), 9, 30),
            Some(at(15, 9, 30))
        );
        // 没到点 → 昨天那次
        assert_eq!(
            last_due_moment(tz.with_ymd_and_hms(2026, 9, 15, 8, 0, 0).unwrap(), 9, 30),
            Some(at(14, 9, 30))
        );
        // 跨月：9 月 1 日没到点，要落到 8 月 31 日
        assert_eq!(
            last_due_moment(tz.with_ymd_and_hms(2026, 9, 1, 8, 0, 0).unwrap(), 9, 30),
            Some(
                tz.with_ymd_and_hms(2026, 8, 31, 9, 30, 0)
                    .unwrap()
                    .timestamp()
            )
        );
    }

    #[test]
    fn result_summary_prefix_roundtrip() {
        let summary = format!("{}:{}", "pulled", "自动拉取成功（3 个新提交）");
        let (kind, msg) = summary.split_once(':').unwrap();
        assert_eq!(kind, "pulled");
        assert_eq!(msg, "自动拉取成功（3 个新提交）");
    }
}
