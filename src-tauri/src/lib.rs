mod cc_sync;
mod commands;
mod engine;
mod quota;
mod scheduler;
mod state;
mod store;

use state::{AppState, Inner};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use store::Config;
use tauri::Manager;

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let open = MenuItem::with_id(app, "open", "显示主窗口", true, None::<&str>)?;
    let act = MenuItem::with_id(app, "activate_all", "立即全部激活", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &act, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().expect("missing icon").clone())
        .tooltip("额度火花 QuotaSpark")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
            "activate_all" => {
                let shared = app.state::<AppState>().0.clone();
                commands::activate_all_spawn(app.clone(), shared);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config_path = store::data_dir().join("config.json");
    let config = Config::load(&config_path);
    let logs = store::load_log_tail(200);
    let app_state = AppState(Arc::new(Mutex::new(Inner {
        config,
        config_path,
        logs,
        quota: HashMap::new(),
    })));

    tauri::Builder::default()
        // 单实例锁：防止两个实例共用一份 config.json 互相覆盖（旧实例的
        // 内存状态会把已删除的供应商写回磁盘）。必须是第一个注册的插件。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::get_providers,
            commands::get_logs,
            commands::sync_from_cc,
            commands::save_provider,
            commands::delete_provider,
            commands::set_schedule,
            commands::activate_now,
            commands::query_quota,
            commands::get_autostart,
            commands::set_autostart,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // 启动定时调度（每 30 秒检查一次触发时间）
            {
                let shared = handle.state::<AppState>().0.clone();
                scheduler::spawn_scheduler(handle.clone(), shared);
            }

            // 启动时预取一次支持的供应商额度
            {
                let handle2 = handle.clone();
                let shared = handle.state::<AppState>().0.clone();
                tauri::async_runtime::spawn(async move {
                    let providers: Vec<store::ProviderConfig> = {
                        let st = shared.lock().unwrap();
                        st.config.providers.clone()
                    };
                    for p in providers {
                        if quota::detect_kind(&p.base_url).is_some() {
                            commands::fetch_and_store_quota(handle2.clone(), shared.clone(), p)
                                .await;
                        }
                    }
                });
            }

            build_tray(&handle)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // 关闭窗口 = 隐藏到托盘，保持调度器运行
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
