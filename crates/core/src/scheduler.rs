//! 后台定时自动同步的**判定与执行**（路线表 Q8，阶段 12）。
//!
//! 这一层是纯业务：不认识 gpui，也不碰任何界面状态。定时器本身在 GUI 侧
//! （`gui/src/scheduler.rs`），它每隔 [`TICK_SECS`] 调一次 [`due_paths`]，
//! 再逐个 [`run_one`]。**到期判定全部写成纯函数并有单测**——这正是它该有的形状：
//! "每天 09:00 错过了一整天要不要补跑"这种问题，肉眼在界面上验不出来。
//!
//! ## 与旧 Tauri 版的关系
//!
//! 平移自 `src-tauri/src/scheduler.rs`（那份随 Tauri 退役），但**修了一处形状问题**：
//! 旧版 `run_one` 直接读写配置。写回这一步现在拆成纯函数 [`record_result`]
//! ——收 `&AppConfig` 返 `AppConfig`，落盘由调用方做。这样"结果写回配置"这条路径
//! 也能被单测盖到，而旧版它只能在真跑一次网络操作之后才被验证到。
//!
//! ## 互斥（32-8 起按仓库分）
//!
//! 调用方（GUI 侧定时器）必须**在 [`run_one`] 外层持有 [`sync::try_begin_op(path)`]**。
//! 拿不到就跳过**那一颗**仓库，不排队也不等待：用户正在操作它时后台就该让路。
//! 反过来用户操作会先 `git::request_cancel_for(同一个路径)` 再等槽，
//! 所以后台跑一半被中断是正常路径。
//!
//! ⚠️ 作用域从"全应用"收成"单仓库"之后，**跨仓库并发变成允许的了**，
//! 于是原本被全局锁顺带挡住的一层露出来：多处"读配置 → 改 → 写回"可以同时在跑
//! （这里的 `record_result` 落盘、`net.rs` 那两处代理写回）。
//! 那一条由 `store::save` 自己管（一把写锁 + 临时文件改名替换），不在这里加第二层。

use crate::repos;
use crate::store::{self, AppConfig, RepoEntry};
use crate::sync;
use chrono::{DateTime, Local, TimeZone};

/// 扫描周期。调度粒度的一半；间隔设置的最小档位（5 分钟）远大于它
pub const TICK_SECS: u64 = 30;
/// 启动后先等一轮再开始，避免开机启动时立刻抢网络
pub const STARTUP_DELAY_SECS: u64 = 60;
/// 相邻两个仓库的同步间隔，避免连续网络请求
pub const BETWEEN_REPOS_DELAY_SECS: u64 = 3;

/// 通知标题前缀，与托盘 tooltip 保持同一品牌名
const BRAND: &str = "青鸟 Git";
/// 通知正文上限（字符数，按字符而非字节截断以适配中文）；
/// 超长多半是 git 的报错输出，整条塞进系统通知只会被系统裁掉
const MAX_BODY_CHARS: usize = 180;

// ================= 结果种类 =================

/// 一次自动同步的结论。五种，**与旧版的字符串协议一一对应**
/// （`last_auto_result` 里存的是 `"<kind>:<message>"`，两套界面共用 config.json）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutoSyncKind {
    /// 已是最新 / 没有上游分支——**不打扰**用户
    Ok,
    /// 云端有更新但没开自动拉取，只提醒
    Behind,
    /// 自动拉取成功
    Pulled,
    /// 该拉，但本地有未提交变更，跳过
    Skip,
    /// 出错了
    Error,
}

impl AutoSyncKind {
    /// 落进 `last_auto_result` 的那一段
    pub fn as_str(self) -> &'static str {
        match self {
            AutoSyncKind::Ok => "ok",
            AutoSyncKind::Behind => "behind",
            AutoSyncKind::Pulled => "pulled",
            AutoSyncKind::Skip => "skip",
            AutoSyncKind::Error => "err",
        }
    }

    /// 解析落盘的那一段。**非法值返回 None 而不是猜一个**：
    /// 猜错会让一个失败的结果被当成成功显示出来
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "ok" => AutoSyncKind::Ok,
            "behind" => AutoSyncKind::Behind,
            "pulled" => AutoSyncKind::Pulled,
            "skip" => AutoSyncKind::Skip,
            "err" => AutoSyncKind::Error,
            _ => return None,
        })
    }

    /// 界面上的中文说法。「同步设置」页签那行摘要用它
    pub fn label(self) -> &'static str {
        match self {
            AutoSyncKind::Ok => "已是最新",
            AutoSyncKind::Behind => "云端有更新",
            AutoSyncKind::Pulled => "已自动拉取",
            AutoSyncKind::Skip => "有改动，已跳过",
            AutoSyncKind::Error => "失败",
        }
    }

    /// 这一轮值不值得发系统通知。**`Ok` 不发**——已是最新是常态，
    /// 每次都弹一条会让人把通知静音，然后连真的失败也一起错过
    pub fn worth_notifying(self) -> bool {
        self != AutoSyncKind::Ok
    }
}

// ================= 到期判定（纯函数，全部有单测） =================

/// 间隔模式到期判定：上次检查距今是否已超过设定间隔
pub fn interval_due(mins: u32, last_check: u64, now: u64) -> bool {
    mins > 0 && now.saturating_sub(last_check) >= u64::from(mins) * 60
}

/// 这个仓库的定时同步**是不是还开着**。
///
/// ⚠️ **两种模式都要看，只判 `auto_sync_mins > 0` 是错的。**
/// 「每天定时」在 `normalize_auto_sync` 里落盘的是
/// `(auto_sync_mins = 0, daily_time = "HH:MM")`——**间隔是 0，时间点非空**。
/// 只看间隔的话，"用户刚关了"那个守卫会把**每天定时这一整档跳掉**，
/// 用户设好了、显示也正常、就是永远不跑。
///
/// 这个坑是从旧 Tauri 版原样搬过来的（`src-tauri/src/scheduler.rs:117-119`
/// 有一模一样的 `if entry.auto_sync_mins == 0 { continue }`），**阶段 12 顺手修掉了**。
/// 判据写在 core 里而不是留在 GUI 的定时器里，就是为了让它有单测可盖。
pub fn is_enabled(e: &RepoEntry) -> bool {
    e.auto_sync_mins > 0 || !e.daily_time.trim().is_empty()
}

/// `now` 之前（含）最近一次的执行时刻（秒）；今天还没到点就是昨天那次。
/// tz 抽象成参数便于单测；实际调用传 `Local`
pub fn last_due_moment<Tz: TimeZone>(now: DateTime<Tz>, h: u32, m: u32) -> Option<i64> {
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
pub fn daily_is_due<Tz: TimeZone>(daily_time: &str, now: DateTime<Tz>, last_check: u64) -> bool {
    let Some((h, m)) = store::parse_hhmm(daily_time) else {
        return false;
    };
    match last_due_moment(now, h, m) {
        Some(target) => (last_check as i64) < target,
        None => false,
    }
}

/// 扫一遍配置，挑出到期的仓库路径。**顺序即配置里的顺序**（串行执行，不并发）
pub fn due_paths_at<Tz: TimeZone>(cfg: &AppConfig, now: u64, now_local: DateTime<Tz>) -> Vec<String> {
    cfg.repos
        .iter()
        .filter(|r| {
            interval_due(r.auto_sync_mins, r.last_auto_check, now)
                // `DateTime<Tz>` 不是 Copy，闭包里 clone 一份——
                // 每个仓库一份，仓库数就是仓库数
                || daily_is_due(&r.daily_time, now_local.clone(), r.last_auto_check)
        })
        .map(|r| r.path.clone())
        .collect()
}

/// [`due_paths_at`] 的本地时区版本，给定时器用
pub fn due_paths(cfg: &AppConfig, now: u64) -> Vec<String> {
    due_paths_at(cfg, now, Local::now())
}

// ================= 结论写回配置（纯函数） =================

/// 把结论压成 `last_auto_result` 存的那一串：`"<kind>:<message>"`
///
/// **用 `:` 而不是 `::`**：message 是我们自己写的、不含换行；
/// 解析时 `split_once(':')` 拿到的第二段就是完整 message（见 [`parse_summary`]）
pub fn summary(kind: AutoSyncKind, message: &str) -> String {
    format!("{}:{}", kind.as_str(), message)
}

/// [`summary`] 的逆操作。`kind` 段非法时返回 None
pub fn parse_summary(s: &str) -> Option<(AutoSyncKind, &str)> {
    let (k, msg) = s.split_once(':')?;
    Some((AutoSyncKind::from_str(k)?, msg))
}

/// 把一次结果写回配置里那个仓库的条目。**纯函数**：收一份、还一份，不落盘。
///
/// 仓库可能刚被移除，这时找不到就原样返回——调用方落盘时写回的是
/// 「它读到的那份配置」，不能拿一个过期快照覆盖掉用户刚做的改动
pub fn record_result(mut cfg: AppConfig, path: &str, kind: AutoSyncKind, message: &str) -> AppConfig {
    if let Some(e) = cfg.repos.iter_mut().find(|r| r.path == path) {
        e.last_auto_check = store::now_secs();
        e.last_auto_result = summary(kind, message);
    }
    cfg
}

// ================= 通知文案（纯函数） =================

/// 结果 → 通知文案，返回 `(标题, 正文)`；`None` 表示这一轮不值得打扰
///
/// 失败时把结论写进**标题**：系统通知折叠起来只看得见标题，
/// 正文里的报错在折叠态一个字都读不到
pub fn toast_for(
    kind: AutoSyncKind,
    name: &str,
    message: &str,
) -> Option<(String, String)> {
    if !kind.worth_notifying() {
        return None;
    }
    let title = if kind == AutoSyncKind::Error {
        format!("{BRAND} · {name} 同步失败")
    } else {
        format!("{BRAND} · {name}")
    };
    Some((title, clamp(message)))
}

/// 按**字符数**截断超长正文。按字节截会把中文切出半个字
fn clamp(s: &str) -> String {
    if s.chars().count() <= MAX_BODY_CHARS {
        return s.to_string();
    }
    let mut out: String = s.chars().take(MAX_BODY_CHARS).collect();
    out.push('…');
    out
}

// ================= 单仓执行 =================

/// 一次自动同步的完整结果
#[derive(Clone, Debug)]
pub struct AutoSyncResult {
    pub path: String,
    /// 展示名（alias 优先）
    pub name: String,
    pub kind: AutoSyncKind,
    pub message: String,
    /// 自动拉取成功时的新提交列表
    pub commits: Vec<String>,
    pub diff_stat: String,
}

/// 仓库的显示名：有自定义名用自定义名
pub fn display_name(e: &RepoEntry) -> String {
    if e.alias.trim().is_empty() {
        e.name.clone()
    } else {
        e.alias.trim().to_string()
    }
}

/// 对单个仓库跑一次自动同步。**调用方必须已经持有 `sync::try_begin_op(&entry.path)`**
///
/// 这里调的是 [`sync::do_check`] / [`sync::do_pull`] 而不是
/// `check_update` / `pull_repo`——后两个会自己去拿 `begin_user_op(同一个路径)`，
/// 而调用方已经占着**这颗仓库**的槽了，再去拿就是等满 10 秒然后失败
/// （⚠️ 32-8 之前那条理由写作"锁被自己占住"，作用域是整个应用；
/// 现在它只挡同一颗仓库——**换句话说别的仓库再也不会被这里跳过了**）
pub fn run_one(entry: &RepoEntry) -> AutoSyncResult {
    let path = entry.path.clone();
    let name = display_name(entry);
    let (kind, message, commits, diff_stat) = if !repos::valid_repo(&path) {
        (
            AutoSyncKind::Error,
            "仓库路径不存在（可能已被移动或删除）".to_string(),
            Vec::new(),
            String::new(),
        )
    } else {
        detect_and_maybe_pull(&path, entry.auto_pull)
    };
    AutoSyncResult {
        path,
        name,
        kind,
        message,
        commits,
        diff_stat,
    }
}

/// 检测 +（按设置）自动拉取的判定。返回 `(kind, message, commits, diff_stat)`
///
/// **分叉一律不自动处理**：`ahead > 0` 时报 Error 而不拉不推。
/// 拉取永远 `--ff-only`，分叉时 git 自己会拒绝；这里提前报出来是为了让
/// 通知里有一句人能看懂的话，而不是把 git 的英文报错甩给用户
fn detect_and_maybe_pull(
    path: &str,
    auto_pull: bool,
) -> (AutoSyncKind, String, Vec<String>, String) {
    let r = sync::do_check(path, None);
    if !r.ok {
        return (AutoSyncKind::Error, r.message, Vec::new(), String::new());
    }
    let Some(st) = &r.status else {
        return (
            AutoSyncKind::Error,
            "无法读取仓库状态".into(),
            Vec::new(),
            String::new(),
        );
    };
    if !st.has_upstream {
        return (
            AutoSyncKind::Ok,
            "当前分支没有上游分支，跳过".into(),
            Vec::new(),
            String::new(),
        );
    }
    if st.behind == 0 {
        return (AutoSyncKind::Ok, "已是最新".into(), Vec::new(), String::new());
    }
    if st.ahead > 0 {
        return (
            AutoSyncKind::Error,
            "本地与云端历史分叉，需手动处理，不自动拉取".into(),
            Vec::new(),
            String::new(),
        );
    }
    let behind_msg = format!("云端有 {} 个新提交", st.behind);
    if !auto_pull {
        return (
            AutoSyncKind::Behind,
            format!("{behind_msg}，可手动拉取"),
            Vec::new(),
            String::new(),
        );
    }
    if st.dirty {
        return (
            AutoSyncKind::Skip,
            format!("{behind_msg}，但本地有未提交变更，已跳过自动拉取"),
            Vec::new(),
            String::new(),
        );
    }

    // 不 stash：脏的情况上面已经挡住了，这里再 stash 就是替用户做决定
    let pr = sync::do_pull(path, false, None);
    if !pr.ok {
        return (
            AutoSyncKind::Error,
            format!("自动拉取失败：{}", pr.message),
            Vec::new(),
            String::new(),
        );
    }
    match pr.changes {
        Some(changes) if !changes.commits.is_empty() => {
            let n = changes.commits.len();
            (
                AutoSyncKind::Pulled,
                format!("自动拉取成功（{n} 个新提交）"),
                changes.commits,
                changes.diff_stat,
            )
        }
        _ => (
            AutoSyncKind::Pulled,
            "自动拉取成功".into(),
            Vec::new(),
            String::new(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn tz8() -> FixedOffset {
        FixedOffset::east_opt(8 * 3600).unwrap()
    }
    fn at(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<FixedOffset> {
        tz8().with_ymd_and_hms(y, m, d, h, mi, 0).unwrap()
    }

    // ---- 到期判定（平移旧版那三个，另加边界） ----

    #[test]
    fn interval_due_only_enabled_and_expired() {
        let now = store::now_secs();
        assert!(!interval_due(0, 0, now), "关闭 → 不调度");
        assert!(!interval_due(15, now - 14 * 60, now), "未到期");
        assert!(interval_due(15, now - 16 * 60, now), "已到期");
        assert!(interval_due(60, 0, now), "从未跑过 → 立即到期");
    }

    #[test]
    fn interval_due_is_exact_on_the_boundary() {
        // 边界必须含等号：差一秒就不跑会让"每 5 分钟"实际上是 6 分钟
        let now = 1_000_000;
        assert!(interval_due(5, now - 5 * 60, now));
        assert!(!interval_due(5, now - 5 * 60 + 1, now));
    }

    #[test]
    fn interval_due_survives_a_backwards_clock() {
        // 系统时间被往回拨时 now < last_check，saturating_sub 不能回绕成巨大值
        let last = 2_000_000;
        assert!(!interval_due(15, last, 1_000_000));
    }

    #[test]
    fn daily_due_fixed_timezone() {
        let run_today = at(2026, 9, 14, 9, 30);
        let before_run = at(2026, 9, 14, 8, 0);
        let after_run = at(2026, 9, 14, 10, 0);
        // 昨天 20:00 跑过（晚于昨天 09:30 那次，所以昨天那次已被覆盖）
        let last_yesterday = at(2026, 9, 13, 20, 0).timestamp() as u64;
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
        assert!(
            daily_is_due("09:30", after_run, last_yesterday),
            "过了执行时刻且今天没跑过 → 到期"
        );
        assert!(
            !daily_is_due("09:30", after_run, last_today),
            "今天执行时刻之后已跑过 → 不重复"
        );
        assert!(!daily_is_due("bad", after_run, last_yesterday), "非法时间串 → 永不到期");
        assert!(!daily_is_due("", after_run, last_yesterday), "空串 → 非每天定时模式");
    }

    #[test]
    fn daily_due_catches_up_a_missed_previous_target() {
        // **回归测试**：09-14 09:30 该跑时机器关着，09-15 08:00 才开机（还没到今天的 09:30）。
        // 旧实现只在"今天的时刻已过"时才判到期，于是 09-14 整天没同步，
        // 还要干等到 09-15 09:30；现在开机那一刻就会补一次。
        let boot = at(2026, 9, 15, 8, 0);
        let stamp = |d: u32, h: u32, mi: u32| at(2026, 9, d, h, mi).timestamp() as u64;

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
        // 已过点 → 今天那次
        assert_eq!(
            last_due_moment(at(2026, 9, 15, 10, 0), 9, 30),
            Some(at(2026, 9, 15, 9, 30).timestamp())
        );
        // 没到点 → 昨天那次
        assert_eq!(
            last_due_moment(at(2026, 9, 15, 8, 0), 9, 30),
            Some(at(2026, 9, 14, 9, 30).timestamp())
        );
        // 跨月：9 月 1 日没到点，要落到 8 月 31 日
        assert_eq!(
            last_due_moment(at(2026, 9, 1, 8, 0), 9, 30),
            Some(at(2026, 8, 31, 9, 30).timestamp())
        );
    }

    #[test]
    fn last_due_moment_rejects_an_impossible_clock_time() {
        // 24:00 解析得出来（parse_hhmm 允许 0..=23，这里直接给 24），
        // 但它不是一个真实时刻 → None，daily_is_due 据此判"永不到期"而不是 panic
        assert_eq!(last_due_moment(at(2026, 9, 15, 10, 0), 24, 0), None);
    }

    // ---- summary 往返 ----

    #[test]
    fn result_summary_prefix_roundtrip() {
        let s = summary(AutoSyncKind::Pulled, "自动拉取成功（3 个新提交）");
        assert_eq!(s, "pulled:自动拉取成功（3 个新提交）");
        let (k, m) = parse_summary(&s).expect("应能解析回来");
        assert_eq!(k, AutoSyncKind::Pulled);
        assert_eq!(m, "自动拉取成功（3 个新提交）");
    }

    #[test]
    fn a_message_containing_colons_survives_the_roundtrip() {
        // 结论里可能有冒号（路径、URL）。只能切**第一个**冒号
        let s = summary(AutoSyncKind::Error, "自动拉取失败：fatal: C:/repo");
        let (_, m) = parse_summary(&s).unwrap();
        assert_eq!(m, "自动拉取失败：fatal: C:/repo");
    }

    #[test]
    fn parse_summary_rejects_an_unknown_kind() {
        // 猜一个 kind 会把失败显示成成功，宁可解析不出来
        assert!(parse_summary("weird:xxx").is_none());
        assert!(parse_summary("没有冒号").is_none());
    }

    #[test]
    fn every_kind_round_trips_through_its_string_form() {
        for k in [
            AutoSyncKind::Ok,
            AutoSyncKind::Behind,
            AutoSyncKind::Pulled,
            AutoSyncKind::Skip,
            AutoSyncKind::Error,
        ] {
            assert_eq!(AutoSyncKind::from_str(k.as_str()), Some(k));
            assert!(!k.label().is_empty());
        }
    }

    // ---- 通知 ----

    #[test]
    fn ok_never_toasts() {
        assert!(toast_for(AutoSyncKind::Ok, "myrepo", "已是最新").is_none());
    }

    #[test]
    fn failure_puts_the_conclusion_in_the_title() {
        // 通知折叠起来只看得见标题，报错必须在标题里
        let (title, body) = toast_for(AutoSyncKind::Error, "myrepo", "自动拉取失败：xxx").unwrap();
        assert!(title.contains("同步失败"), "标题应含结论：{title}");
        assert!(title.contains("myrepo"));
        assert!(title.contains(BRAND));
        assert_eq!(body, "自动拉取失败：xxx");
    }

    #[test]
    fn toast_body_is_clamped_by_characters_not_bytes() {
        // 180 个汉字 = 540 字节。按字节截会把中文切出半个字
        let long = "错".repeat(300);
        let (_, body) = toast_for(AutoSyncKind::Error, "r", &long).unwrap();
        assert_eq!(body.chars().count(), MAX_BODY_CHARS + 1, "应留一个省略号");
        assert!(body.ends_with('…'));
        assert!(body.chars().all(|c| c == '错' || c == '…'), "不能有半个汉字");
    }

    #[test]
    fn short_body_is_untouched() {
        let (_, body) = toast_for(AutoSyncKind::Behind, "r", "云端有 2 个新提交").unwrap();
        assert_eq!(body, "云端有 2 个新提交");
    }

    // ---- 写回配置 ----

    fn entry(path: &str) -> RepoEntry {
        RepoEntry {
            path: path.to_string(),
            name: "repo".into(),
            ..Default::default()
        }
    }

    #[test]
    fn record_result_writes_back_the_matching_repo_only() {
        let cfg = AppConfig {
            repos: vec![entry("a"), entry("b")],
            ..Default::default()
        };
        let out = record_result(cfg, "b", AutoSyncKind::Pulled, "自动拉取成功");
        assert_eq!(out.repos[0].last_auto_check, 0, "a 不该被动过");
        assert!(out.repos[1].last_auto_check > 0);
        assert_eq!(out.repos[1].last_auto_result, "pulled:自动拉取成功");
    }

    #[test]
    fn record_result_on_a_removed_repo_returns_the_config_untouched() {
        // 定时器跑的过程中用户把仓库移除了：找不到就原样返回，
        // 落盘时不能拿这份快照覆盖掉用户刚删掉的那条
        let cfg = AppConfig {
            repos: vec![entry("a")],
            ..Default::default()
        };
        let out = record_result(cfg, "gone", AutoSyncKind::Ok, "已是最新");
        assert_eq!(out.repos.len(), 1);
        assert_eq!(out.repos[0].last_auto_check, 0);
    }

    #[test]
    fn display_name_prefers_the_alias() {
        let mut e = entry("a");
        e.alias = "  我的仓库 ".into();
        assert_eq!(display_name(&e), "我的仓库", "别名两端空白要修掉");
        e.alias = "   ".into();
        assert_eq!(display_name(&e), "repo", "空白别名等同没有");
    }

    // ---- 到期筛选 ----

    #[test]
    fn due_paths_picks_only_enabled_and_expired_repos() {
        let now = store::now_secs();
        let mut off = entry("off");
        off.auto_sync_mins = 0;
        let mut due = entry("due");
        due.auto_sync_mins = 15;
        due.last_auto_check = now - 20 * 60;
        let mut fresh = entry("fresh");
        fresh.auto_sync_mins = 15;
        fresh.last_auto_check = now;

        let cfg = AppConfig {
            repos: vec![off, due, fresh],
            ..Default::default()
        };
        let got = due_paths_at(&cfg, now, at(2026, 9, 15, 10, 0));
        assert_eq!(got, vec!["due".to_string()]);
    }

    // ---- 开没开：两种模式都要认 ----

    #[test]
    fn daily_mode_counts_as_enabled_even_though_its_interval_is_zero() {
        // **回归测试**。`normalize_auto_sync` 对「每天定时」落盘的是
        // `(0, "09:00")`——间隔是 0。老实现只判 `auto_sync_mins == 0`，
        // 于是「每天定时」这一整档被当成"用户刚关了"跳掉：
        // 设好了、界面显示正常、就是永远不跑。
        let (mins, daily) = crate::store::normalize_auto_sync(&crate::store::AutoSync::Daily(
            "09:00".into(),
        ));
        assert_eq!(mins, 0, "前提：每天定时的间隔确实是 0");
        assert_eq!(daily, "09:00");

        let mut e = entry("daily");
        e.auto_sync_mins = mins;
        e.daily_time = daily;
        assert!(
            is_enabled(&e),
            "每天定时必须算开着——这是老实现漏掉的那一档"
        );
    }

    #[test]
    fn off_is_the_only_state_that_is_not_enabled() {
        let (mins, daily) = crate::store::normalize_auto_sync(&crate::store::AutoSync::Off);
        let mut e = entry("off");
        e.auto_sync_mins = mins;
        e.daily_time = daily;
        assert!(!is_enabled(&e), "「不定时」两个字段都是空，必须判关");
    }

    #[test]
    fn interval_mode_is_enabled_and_blank_daily_time_does_not_matter() {
        let mut e = entry("every");
        e.auto_sync_mins = 15;
        e.daily_time = String::new();
        assert!(is_enabled(&e));
        // 非法时间串按关闭处理（`normalize_auto_sync` 已经这么做了），
        // 但如果配置是被外部写坏的，这里不能反过来把它判成开着
        e.daily_time = "  ".into();
        assert!(is_enabled(&e), "间隔模式本来就开着，daily_time 是脏的也不该改变这一点");
    }

    #[test]
    fn a_never_run_repo_is_due_immediately() {        // 刚加进来的仓库 last_auto_check = 0，若判成"15 分钟前跑的"，
        // 用户开了定时之后要干等 15 分钟才看到第一次同步
        let mut e = entry("new");
        e.auto_sync_mins = 60;
        let cfg = AppConfig {
            repos: vec![e],
            ..Default::default()
        };
        assert_eq!(
            due_paths_at(&cfg, store::now_secs(), at(2026, 9, 15, 10, 0)),
            vec!["new".to_string()]
        );
    }
}
