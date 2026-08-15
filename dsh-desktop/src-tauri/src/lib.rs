mod runtime;
mod server;
mod settings;
mod tray;
mod window;

use server::ServerManager;
use settings::AppSettings;
use std::sync::{Arc, Mutex};
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

#[tauri::command]
fn get_settings(settings: State<'_, Arc<Mutex<AppSettings>>>) -> AppSettings {
    settings.lock().unwrap().clone()
}

#[tauri::command]
fn set_settings(
    app: AppHandle,
    settings: State<'_, Arc<Mutex<AppSettings>>>,
    new: AppSettings,
) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    if new.autostart {
        let _ = app.autolaunch().enable();
    } else {
        let _ = app.autolaunch().disable();
    }
    let path = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("settings.json");
    new.save(&path)?;
    *settings.lock().unwrap() = new;
    Ok(())
}

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    let url = if cfg!(debug_assertions) {
        "http://localhost:1420/?view=settings"
    } else {
        "tauri://localhost/?view=settings"
    };
    let app_handle = app.clone();
    tauri::WebviewWindowBuilder::new(
        &app,
        "settings",
        tauri::WebviewUrl::External(url.parse::<tauri::Url>().map_err(|e| e.to_string())?),
    )
    .title("DSH Desktop 设置")
    .inner_size(540.0, 680.0)
    .background_color(tauri::window::Color(13, 17, 23, 255))
    .on_navigation(move |url| window::is_local_shell(url))
    .build()
    .map_err(|e| e.to_string())?;
    let _ = app_handle;
    Ok(())
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
/// Windows/Linux 不设应用菜单(托盘承担退出入口;Windows 窗口菜单不符合惯例)。
#[cfg(target_os = "macos")]
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
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
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
            let settings = Arc::new(Mutex::new(AppSettings::load(&settings_path)));
            app.manage(settings.clone());
            let resource_dir = app.path().resource_dir().unwrap_or_default();
            // Windows 上 current_exe() 可能返回 \\?\ 长路径前缀,node 无法处理 argv 中的该前缀
            // (会把路径误解析为裸盘符导致 EISDIR),用 dunce 归一化为普通路径。
            let resource_dir = dunce::simplified(&resource_dir).to_path_buf();
            let manager = Arc::new(ServerManager::new(
                app.handle().clone(),
                settings,
                resource_dir,
            ));
            app.manage(manager.clone());
            let _ = manager.start();
            #[cfg(target_os = "macos")]
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

            // 测试钩子:DSH_DESKTOP_CLOSE_MS=<ms> 时,模拟点击主窗口关闭按钮(走 CloseRequested 路径)
            if let Ok(ms) = std::env::var("DSH_DESKTOP_CLOSE_MS") {
                if let Ok(ms) = ms.parse::<u64>() {
                    let app = app.handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(ms));
                        if let Some(w) = app.get_webview_window("main") {
                            println!("[test] closing main window…");
                            let _ = w.close();
                        }
                    });
                }
            }

            // 测试钩子:DSH_DESKTOP_OPEN_SETTINGS=1 时,启动 2s 后打开设置窗口
            if std::env::var("DSH_DESKTOP_OPEN_SETTINGS").is_ok() {
                let app = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    let _ = open_settings(app);
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
            .background_color(tauri::window::Color(13, 17, 23, 255))
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
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 仅主窗口关闭 = 退出 app;设置窗口关闭走默认行为
                if window.label() == "main" {
                    api.prevent_close(); // 阻止默认关闭流程(避免 exit 被窗口事件吞掉)
                    let app = window.app_handle().clone();
                    // 后台线程执行停服+退出,不阻塞主线程事件循环
                    std::thread::spawn(move || request_exit(&app));
                }
            }
        })
        .on_page_load(|_window, payload| {
            println!("[app] page-load url={}", payload.url());
        })
        .invoke_handler(tauri::generate_handler![
            get_server_status,
            get_logs,
            restart_server,
            quit_app,
            get_settings,
            set_settings,
            open_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
