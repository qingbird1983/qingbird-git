//! 把后台自动同步的结果发成 Windows 系统通知。
//!
//! 后台值守的仓库结果只推给前端是没人看的——主窗口一关（Webview 销毁、托盘常驻），
//! 事件就没有接收方了。这里由 Rust 侧直接发系统通知，不依赖前端存活。
//!
//! 两条"不打扰"的规则：
//! 1. 结果无事发生（`ok`：已是最新 / 无上游）不发；
//! 2. 主窗口可见且正在前台时，用户自己看得见结果，也不发。
//!
//! Windows 注意：`tauri-plugin-notification` 只在应用被安装后才会显示本应用的名字与图标
//! （安装包创建的快捷方式注册了 AppUserModelID）。`pnpm tauri dev` 下通知的来源会显示成
//! PowerShell，这是插件在开发模式下的已知表现，不是配置错误。

use crate::scheduler::AutoSyncEvent;
use tauri::Manager;
use tauri_plugin_notification::NotificationExt;

/// 通知正文上限（字符数，按字符而非字节截断以适配中文）；超长多半是 git 的报错输出，
/// 整条塞进系统通知只会被系统裁掉，不如自己截断留个省略号
const MAX_BODY_CHARS: usize = 180;

/// 通知标题前缀，与托盘 tooltip 保持同一品牌名
const BRAND: &str = "青鸟 Git";

/// 结果 → 通知文案，返回 (标题, 正文)；`None` 表示这一轮不值得打扰
pub fn toast_for(kind: &str, name: &str, message: &str) -> Option<(String, String)> {
    if kind == "ok" {
        return None;
    }
    // 失败时把结论写进标题：toast 折叠时只看得到标题
    let title = if kind == "error" {
        format!("{BRAND} · {name} 同步失败")
    } else {
        format!("{BRAND} · {name}")
    };
    Some((title, clamp(message)))
}

/// 按字符数截断超长正文
fn clamp(s: &str) -> String {
    if s.chars().count() <= MAX_BODY_CHARS {
        return s.to_string();
    }
    let mut out: String = s.chars().take(MAX_BODY_CHARS).collect();
    out.push('…');
    out
}

/// 主窗口可见且正在前台时返回 false——用户就在看这个界面，再弹系统通知是打扰。
/// 窗口不存在（关窗后 Webview 已销毁、应用退到托盘）时返回 true，那正是最该通知的场景
fn should_notify(app: &tauri::AppHandle) -> bool {
    match app.get_webview_window("main") {
        Some(w) => !(w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false)),
        None => true,
    }
}

/// 发送一条自动同步结果通知（开关由调用方判定，见 `scheduler::tick`）
pub fn auto_sync(app: &tauri::AppHandle, event: &AutoSyncEvent) {
    if !should_notify(app) {
        return;
    }
    let Some((title, body)) = toast_for(&event.kind, &event.name, &event.message) else {
        return;
    };
    // 通知发不出去（系统策略禁用等）不该影响同步本身，忽略错误
    let _ = app.notification().builder().title(title).body(body).show();
}

/// 设置里打开通知开关时发一条示例：用户当场就能确认系统会不会弹出来，
/// 不用等下一次自动同步。这是用户主动触发的，所以不走 `should_notify` 的前台静默
pub fn sample(app: &tauri::AppHandle) {
    let body = "检测到新提交 / 自动拉取完成 / 同步失败时，会在这里提醒";
    let _ = app
        .notification()
        .builder()
        .title(format!("{BRAND} · 通知已开启"))
        .body(body)
        .show();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_result_is_silent() {
        assert!(toast_for("ok", "demo", "已是最新").is_none());
        assert!(toast_for("ok", "demo", "当前分支没有上游分支，跳过").is_none());
    }

    #[test]
    fn failure_puts_conclusion_in_title() {
        let (title, body) = toast_for("error", "demo", "自动拉取失败：网络超时").unwrap();
        assert_eq!(title, "青鸟 Git · demo 同步失败");
        assert_eq!(body, "自动拉取失败：网络超时");
    }

    #[test]
    fn normal_results_keep_message_as_body() {
        for kind in ["behind", "pulled", "skip"] {
            let (title, body) = toast_for(kind, "demo", "云端有 3 个新提交，可手动拉取").unwrap();
            assert_eq!(title, "青鸟 Git · demo", "{kind} 的标题不该带失败字样");
            assert_eq!(body, "云端有 3 个新提交，可手动拉取");
        }
    }

    #[test]
    fn long_body_is_clamped_without_splitting_chars() {
        let long = "错".repeat(MAX_BODY_CHARS + 50);
        let (_, body) = toast_for("error", "demo", &long).unwrap();
        assert_eq!(body.chars().count(), MAX_BODY_CHARS + 1, "截断后为上限字符 + 省略号");
        assert!(body.ends_with('…'));
    }

    #[test]
    fn body_at_limit_is_untouched() {
        let exact = "改".repeat(MAX_BODY_CHARS);
        let (_, body) = toast_for("behind", "demo", &exact).unwrap();
        assert_eq!(body, exact, "刚好到上限不该被截断");
    }
}
