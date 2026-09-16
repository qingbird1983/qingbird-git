mod commands;
mod git;
mod notify;
mod proxy;
mod scheduler;
mod store;

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    Manager,
};

/// 读取持久化主题并应用到窗口（setup 与重建窗口共用）
fn apply_saved_theme(win: &tauri::WebviewWindow) {
    let cfg = store::load(win.app_handle());
    let theme = match cfg.settings.theme.as_str() {
        "dark" => Some(tauri::Theme::Dark),
        "light" => Some(tauri::Theme::Light),
        _ => None,
    };
    let _ = win.set_theme(theme);
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
        return;
    }
    // 窗口已被销毁（关窗 = 释放 Webview 省内存）：按 tauri.conf.json 的窗口配置重建
    let cfg = app.config().app.windows.first().cloned();
    if let Some(cfg) = cfg {
        if let Ok(win) = tauri::webview::WebviewWindowBuilder::from_config(app, &cfg)
            .and_then(|b| b.build())
        {
            apply_saved_theme(&win);
            let _ = win.set_focus();
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // 后台自动同步结果的系统通知（Rust 侧直接发，见 notify.rs）
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_repos,
            commands::add_repo,
            commands::remove_repo,
            commands::rename_repo,
            commands::list_dir_items,
            commands::init_repo,
            commands::repo_status,
            commands::check_update,
            commands::pull_repo,
            commands::push_repo,
            commands::rebase_fork,
            commands::undo_rebase,
            commands::cancel_busy_op,
            commands::commit_push,
            commands::list_changes,
            commands::recent_commits,
            commands::commit_detail,
            commands::commit_patch,
            commands::open_commit_diff_in_terminal,
            commands::repo_languages,
            commands::clone_repo,
            commands::create_repo,
            commands::scan_all,
            commands::get_settings,
            commands::set_settings,
            commands::set_theme,
            commands::set_notify_auto_sync,
            commands::get_credentials,
            commands::set_credentials,
            commands::open_in_folder,
            commands::open_in_terminal,
            commands::open_readme,
            commands::open_repo_website,
            commands::stop_repo_cmd,
            commands::set_repo_cmds,
            commands::set_pinned_cmds,
            commands::set_auto_sync,
            commands::pin_repo,
            commands::detect_cmds,
            commands::run_repo_cmd,
            commands::scan_repo_space,
            commands::delete_cache_dir,
            commands::add_gitignore_entries,
            commands::probe_proxy
        ])
        .setup(|app| {
            let handle = app.handle();
            if let Some(win) = handle.get_webview_window("main") {
                apply_saved_theme(&win);
            }
            // 后台定时自动同步：只处理用户开启的仓库
            scheduler::start(handle.clone());

            // 托盘菜单：显示主窗口 / 一键扫描全部 / 开机自启 / 退出
            let show = MenuItem::with_id(handle, "show", "显示主窗口", true, None::<&str>)?;
            let scan = MenuItem::with_id(handle, "scan", "一键扫描全部仓库", true, None::<&str>)?;
            let quit = MenuItem::with_id(handle, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(
                handle,
                &[
                    &show,
                    &scan,
                    &PredefinedMenuItem::separator(handle)?,
                    &quit,
                ],
            )?;

            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(tauri::include_image!("icons/32x32.png"))
                .tooltip("青鸟 Git — 本地仓库一键同步")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "scan" => {
                        use tauri::Emitter;
                        show_main(app);
                        let _ = app.emit("scan-all", ());
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button, button_state: tauri::tray::MouseButtonState::Up, .. } = event {
                        if button == tauri::tray::MouseButton::Left {
                            let app = tray.app_handle();
                            if let Some(win) = app.get_webview_window("main") {
                                if win.is_visible().unwrap_or(false) && win.is_focused().unwrap_or(false) {
                                    let _ = win.hide();
                                } else {
                                    show_main(app);
                                }
                            } else {
                                // 窗口已销毁（关窗后托盘常驻），点击托盘重建
                                show_main(app);
                            }
                        }
                    }
                })
                .build(handle)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // 点关闭按钮：销毁 Webview 释放内存（WebView2 常驻是大头），应用退到托盘只留 Rust 后端；
            // 点击托盘图标/菜单时按 conf 重建窗口。定时自动同步等后台任务不受影响。
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.destroy();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // 主窗口销毁后应用零窗口运行（托盘常驻）：拦下"全部窗口已关"触发的默认退出
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            let _ = app;
        });
}
