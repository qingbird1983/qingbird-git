//! 后台定时自动同步的**定时器与主线程接线**（路线表 Q8，阶段 12）。
//!
//! 判定与执行全在 `qingbird_core::scheduler`（纯业务、98+20 个单测），
//! 这里只做三件事：**按节奏唤醒、把工作丢到后台线程、把结果送回界面**。
//!
//! ## 为什么这个任务必须挂在进程级而不是窗口级
//!
//! `Context<Shell>::spawn` 出来的任务**跟着窗口一起死**。窗口一关（点 X、
//! 或者托盘没把它叫回来），后台同步就跟着停——那不叫后台自动同步，
//! 那叫"你盯着的时候它才跑"。
//!
//! 所以这里用 [`App::spawn`]：它拿到 `&mut AsyncApp`，**不绑任何窗口也不绑任何实体**，
//! 任务活到进程结束。托盘 + `QuitMode::Explicit`（`main.rs`）保证进程在窗口关掉之后
//! 还活着，两者缺一不可——**没有托盘就别设 `QuitMode::Explicit`**，那会让应用变成
//! 一个没有任何入口的幽灵进程。
//!
//! ## 互斥
//!
//! 每跑一个仓库前先 [`sync::try_begin_op`]：拿不到就跳过本轮，**不排队也不等待**。
//! 反向是用户操作优先——`begin_user_op` 会先 `git::request_cancel` 打断后台，
//! 再等锁。抢不到锁就跳过的语义与旧 Tauri 版一致。

use gpui_kit::component::notification::Notification;
use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, AppContext as _, AsyncApp};
use qingbird_core::scheduler::{self, AutoSyncResult};
use qingbird_core::store;
use qingbird_core::sync;
use std::time::Duration;

use crate::state::Shell;

/// 起那条常驻的定时任务。**必须在主线程调，且不依赖任何窗口**
pub(crate) fn start(cx: &mut App) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        // 开机启动时不立刻抢网络：先等一轮再开始
        cx.background_executor()
            .timer(Duration::from_secs(scheduler::STARTUP_DELAY_SECS))
            .await;
        loop {
            cx.background_executor()
                .timer(Duration::from_secs(scheduler::TICK_SECS))
                .await;
            tick(cx).await;
        }
    })
    .detach();
}

/// 跑一轮：挑出到期仓库，逐个同步，串行
async fn tick(cx: &mut AsyncApp) {
    let now = store::now_secs();
    let cfg = cx
        .background_executor()
        .spawn(async { store::load() })
        .await;
    let due = scheduler::due_paths(&cfg, now);
    if due.is_empty() {
        return;
    }
    let notify_on = cfg.settings.notify_auto_sync;

    for path in due {
        // 跑之前重读配置：仓库可能刚被移除，或用户刚改了间隔 / 关了开关
        let fresh = cx
            .background_executor()
            .spawn(async { store::load() })
            .await;
        let Some(entry) = fresh.repos.into_iter().find(|r| r.path == path) else {
            continue;
        };
        if !scheduler::is_enabled(&entry) {
            continue; // 用户刚关了。**两种模式都要认**——见 core 的 is_enabled
        }
        // 拿不到操作锁说明用户正在操作（或上一轮还没跑完），跳过本轮，下个 tick 再试
        let Some(_guard) = sync::try_begin_op() else {
            continue;
        };
        let e = entry.clone();
        let result = cx
            .background_executor()
            .spawn(async move { scheduler::run_one(&e) })
            .await;
        drop(_guard);

        persist(cx, &result).await;
        apply_to_ui(cx, &result, notify_on);

        // 相邻两个仓库之间留一下，别连着轰炸网络
        cx.background_executor()
            .timer(Duration::from_secs(scheduler::BETWEEN_REPOS_DELAY_SECS))
            .await;
    }
}

/// 把结果写回配置。**重新 load 之后再改**而不是拿 `tick` 开头那份：
/// 这一轮跑了可能几十秒，期间用户改过别的仓库设置，
/// 拿旧快照落盘会把那些改动一起覆盖掉
async fn persist(cx: &mut AsyncApp, r: &AutoSyncResult) {
    let path = r.path.clone();
    let kind = r.kind;
    let message = r.message.clone();
    let res = cx
        .background_executor()
        .spawn(async move {
            let cfg = store::load();
            let next = scheduler::record_result(cfg, &path, kind, &message);
            store::save(&next)
        })
        .await;
    if let Err(e) = res {
        crate::diag::note(&format!("自动同步结果写回配置失败：{e}"));
    }
}

/// 把结果送到界面上：刷新状态 + 按两条「不打扰」规则决定要不要发系统通知
///
/// **窗口关着不是错误**：那正是「后台」二字的含义。同步照跑、配置照写，
/// 只是没法弹通知（通知要经窗口）。托盘上的 tooltip 之外没有别的出口，
/// 这一条如实记在路线表阶段 12 里
fn apply_to_ui(cx: &mut AsyncApp, r: &AutoSyncResult, notify_on: bool) {
    let wins = cx.update(|cx: &mut App| cx.windows());
    let Some(w) = wins.first().copied() else {
        return;
    };
    let kind = r.kind;
    let name = r.name.clone();
    let message = r.message.clone();
    // 「主窗口在前台」= 它就是当前活动窗口。`Window::is_active` 在这个
    // gpui 版本上取不到，用 `App::active_window()` 比句柄更直接
    let focused = cx.update(|cx: &mut App| cx.active_window()) == Some(w);
    let _ = cx.update_window(w, move |_, window, cx| {
        // 拉取成功改了 ahead/behind，状态要重扫：侧栏徽标与总览统计都靠它
        if let Some(Some(shell)) = window.root::<Shell>() {
            shell.update(cx, |shell, cx| shell.refresh_status(cx));
        }
        // 两条「不打扰」：① `ok` 不发（已是最新是常态，弹多了会被静音，
        // 连真的失败也一起错过）② 主窗口在前台时不发（人就在看着）
        if !notify_on || !kind.worth_notifying() {
            return;
        }
        if focused {
            return;
        }
        if let Some((title, body)) = scheduler::toast_for(kind, &name, &message) {
            window.push_notification(Notification::new().title(title).message(body).system(), cx);
        }
    });
}
