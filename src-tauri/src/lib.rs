mod commands;
mod git;
mod proxy;
mod scheduler;
mod store;

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    Manager,
};

fn apply_saved_theme(app: &tauri::AppHandle) {
    let cfg = store::load(app);
    let theme = match cfg.settings.theme.as_str() {
        "dark" => Some(tauri::Theme::Dark),
        "light" => Some(tauri::Theme::Light),
        _ => None,
    };
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_theme(theme);
    }
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
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
            commands::cancel_busy_op,
            commands::commit_push,
            commands::list_changes,
            commands::recent_commits,
            commands::repo_languages,
            commands::clone_repo,
            commands::create_repo,
            commands::scan_all,
            commands::get_settings,
            commands::set_settings,
            commands::set_theme,
            commands::get_credentials,
            commands::set_credentials,
            commands::open_in_folder,
            commands::open_in_terminal,
            commands::open_readme,
            commands::open_repo_website,
            commands::stop_repo_cmd,
            commands::set_repo_cmds,
            commands::set_auto_sync,
            commands::pin_repo,
            commands::detect_cmds,
            commands::run_repo_cmd,
            commands::scan_repo_space,
            commands::delete_cache_dir,
            commands::probe_proxy
        ])
        .setup(|app| {
            let handle = app.handle();
            apply_saved_theme(handle);
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
                            }
                        }
                    }
                })
                .build(handle)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // 点关闭按钮时隐藏到托盘常驻，从托盘菜单退出
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
