mod runtime;
mod server;
mod settings;
mod window;

use server::ServerManager;
use settings::AppSettings;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
fn get_server_status(mgr: State<'_, Arc<ServerManager>>) -> server::ServerState {
    mgr.state()
}

#[tauri::command]
fn get_logs(mgr: State<'_, Arc<ServerManager>>, tail: Option<usize>) -> Vec<String> {
    mgr.logs(tail.unwrap_or(200))
}

#[tauri::command]
fn restart_server(mgr: State<'_, Arc<ServerManager>>) -> Result<(), String> {
    mgr.restart()
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    request_exit(&app);
}

fn request_exit(app: &AppHandle) {
    if let Some(m) = app.try_state::<Arc<ServerManager>>() {
        m.stop();
    }
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .setup(|app| {
            let cfg_dir = app.path().app_config_dir()?;
            let settings_path = cfg_dir.join("settings.json");
            server::init_log_file(cfg_dir.join("server.log"));
            let settings = Arc::new(AppSettings::load(&settings_path));
            app.manage(settings.clone());
            let resource_dir = app.path().resource_dir().unwrap_or_default();
            let manager = Arc::new(ServerManager::new(
                app.handle().clone(),
                settings,
                resource_dir,
            ));
            app.manage(manager.clone());
            let _ = manager.start();

            // 主窗口程序化创建(以挂载导航守卫)
            let app_handle = app.handle().clone();
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("DSH Desktop")
            .inner_size(1280.0, 800.0)
            .on_navigation(move |url| window::navigation_guard(&app_handle, url))
            .build()?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                request_exit(window.app_handle());
            }
        })
        .on_page_load(|_window, payload| {
            println!("[app] page-load url={}", payload.url());
        })
        .invoke_handler(tauri::generate_handler![
            get_server_status,
            get_logs,
            restart_server,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
