mod runtime;
mod server;
mod settings;
mod tray;
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

/// 将 server-status 事件负载转成托盘状态文本
fn status_text(payload: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(payload).unwrap_or_default();
    let state = v["state"].as_str().unwrap_or("unknown");
    match state {
        "running" => {
            let url = v["url"].as_str().unwrap_or("");
            let port = url.rsplit(':').next().unwrap_or("");
            format!("DSH ● :{port}")
        }
        "starting" => "DSH … 启动中".into(),
        "error" => "DSH ✗ 服务错误".into(),
        _ => "DSH ○ 已停止".into(),
    }
}

fn request_exit(app: &AppHandle) {
    if let Some(m) = app.try_state::<Arc<ServerManager>>() {
        m.stop();
    }
    app.exit(0);
}

/// macOS 原生菜单:应用菜单 + 退出(Cmd+Q)。退出走完整停服流程。
fn setup_menu(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};

    let quit = MenuItem::with_id(app, "quit", "退出 DSH Desktop", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(
        app,
        &[
            &PredefinedMenuItem::about(app, None, None)?,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    app.set_menu(menu)?;
    Ok(())
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
            let _ = setup_menu(app);
            let _ = tray::setup(app);

            // 托盘状态联动
            {
                use tauri::Listener;
                let app_handle = app.handle().clone();
                app_handle.listen("server-status", move |event| {
                    let text = status_text(event.payload());
                    tray::update_status(&text);
                });
            }

            // 测试钩子:DSH_DESKTOP_AUTOQUIT_MS=<ms> 时,启动 ms 毫秒后走完整退出流程
            // (验证 F3:退出后零残留;也作为 CI/验收回归工具)
            if let Ok(ms) = std::env::var("DSH_DESKTOP_AUTOQUIT_MS") {
                if let Ok(ms) = ms.parse::<u64>() {
                    let app = app.handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(ms));
                        request_exit(&app);
                    });
                }
            }

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
        .on_menu_event(|app, event| match event.id().as_ref() {
            "quit" => request_exit(app),
            "tray-open" => tray::show_main(app),
            "tray-restart" => {
                if let Some(m) = app.try_state::<Arc<ServerManager>>() {
                    let _ = m.restart();
                }
            }
            _ => {}
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
