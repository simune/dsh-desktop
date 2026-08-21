mod runtime;
mod server;
mod settings;
mod tray;
mod window;

use server::ServerManager;
use settings::AppSettings;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

/// dsh 的 settings.yaml 里 ui-theme.preference 字段(light/dark/system)。
/// shell 的标题栏/加载页跟随该值,与 dsh 网页内主题设置保持一致
/// (用户可在 dsh 内把主题设为深色,此时系统主题可能仍为亮色)。
const THEME_PREF_SYSTEM: &str = "system";

/// 解析客户端实际使用的 DSH_HOME(与 server.rs::resolve_dsh_home 同规则:
/// 设置 > 环境变量 DSH_HOME > 默认 ~/.dsh)。
/// Windows 默认目录用 USERPROFILE(USERPROFILE\.dsh);HOME 在 Windows 上通常为空,
/// 若只查 HOME 会得到空路径,导致双击启动(无会话级 DSH_HOME)读不到主题设置。
fn dsh_home_path(app: &AppHandle) -> PathBuf {
    if let Some(s) = app.try_state::<Arc<Mutex<AppSettings>>>() {
        if let Some(h) = &s.lock().unwrap().dsh_home {
            return PathBuf::from(h);
        }
    }
    if let Some(h) = std::env::var_os("DSH_HOME") {
        return PathBuf::from(h);
    }
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .map(|h| PathBuf::from(h).join(".dsh"))
            .or_else(|| {
                std::env::var_os("HOMEDRIVE")
                    .zip(std::env::var_os("HOMEPATH"))
                    .map(|(d, p)| PathBuf::from(d).join(p).join(".dsh"))
            })
            .unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".dsh"))
            .unwrap_or_default()
    }
}

/// 读取 dsh 的 settings.yaml 中 `ui-theme.preference` 值。
/// 文件缺失/字段缺失返回 "system"(跟随系统主题)。
fn read_theme_preference(home: &Path) -> String {
    let content = match std::fs::read_to_string(home.join("settings.yaml")) {
        Ok(c) => c,
        Err(_) => return THEME_PREF_SYSTEM.to_string(),
    };
    let mut in_theme = false;
    for line in content.lines() {
        let t = line.trim();
        if t == "ui-theme:" {
            in_theme = true;
            continue;
        }
        if in_theme {
            if let Some(v) = t.strip_prefix("preference:") {
                return v.trim().trim_matches('"').to_string();
            }
            // 离开了 ui-theme 段(遇到新的无缩进顶层键或注释边界)
            if !t.is_empty() && !t.starts_with('#') && line.starts_with(|c: char| !c.is_whitespace()) {
                return THEME_PREF_SYSTEM.to_string();
            }
        }
    }
    THEME_PREF_SYSTEM.to_string()
}

#[tauri::command]
fn get_theme_preference(app: AppHandle) -> String {
    read_theme_preference(&dsh_home_path(&app))
}

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
    let settings_win = tauri::WebviewWindowBuilder::new(
        &app,
        "settings",
        tauri::WebviewUrl::External(url.parse::<tauri::Url>().map_err(|e| e.to_string())?),
    )
    .title("DSH Desktop 设置")
    .inner_size(540.0, 680.0)
    .min_inner_size(480.0, 560.0)
    .decorations(false)
    .shadow(true)
    .on_navigation(move |url| window::is_local_shell(url))
    .on_new_window(move |url, _features| {
        use tauri::webview::NewWindowResponse;
        if window::is_local_shell(&url) {
            NewWindowResponse::Allow
        } else {
            let _ = tauri_plugin_opener::open_url(url.as_str(), None::<&str>);
            NewWindowResponse::Deny
        }
    })
    .build()
    .map_err(|e| e.to_string())?;
    // 窗口底色跟随系统主题(与前端 CSS prefers-color-scheme 一致,避免加载前黑/白闪)
    let theme = settings_win.theme().unwrap_or(tauri::Theme::Dark);
    let bg = match theme {
        tauri::Theme::Dark => tauri::window::Color(21, 21, 23, 255),
        tauri::Theme::Light => tauri::window::Color(255, 255, 255, 255),
        _ => tauri::window::Color(21, 21, 23, 255),
    };
    let _ = settings_win.set_background_color(Some(bg));
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
    // 退出前隐藏主窗口:停服过程会短暂发出 stopped 状态,若窗口仍可见会闪现
    // "服务已停止"页(用户已点关闭,不应再看到内容变化)。隐藏后停服+退出。
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
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

            // 主题联动:轮询 dsh 的 settings.yaml 中 ui-theme.preference,
            // 变化时 emit theme-changed 给前端(标题栏/加载页跟随 dsh 内主题设置)
            {
                use tauri::Emitter;
                let app_handle = app.handle().clone();
                let mut last: Option<String> = None;
                std::thread::spawn(move || {
                    let home = dsh_home_path(&app_handle);
                    loop {
                        let pref = read_theme_preference(&home);
                        if last.as_deref() != Some(pref.as_str()) {
                            let _ = app_handle.emit("theme-changed", &pref);
                            last = Some(pref);
                        }
                        std::thread::sleep(Duration::from_millis(400));
                    }
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
            let nav_handle = app.handle().clone();
            let main_win = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("DSH Desktop")
            .inner_size(1280.0, 800.0)
            .min_inner_size(900.0, 600.0)
            .decorations(false)
            .shadow(true)
            .on_navigation(move |url| window::navigation_guard(&nav_handle, url))
            // iframe 方案:dsh 内 target=_blank / window.open 的新窗口请求。
            // 本地壳放行(弹新窗口),dsh loopback 放行(允许内部弹窗),其余交系统浏览器。
            .on_new_window(move |url, _features| {
                use tauri::webview::NewWindowResponse;
                if window::is_local_shell(&url)
                    || (url.host_str() == Some("127.0.0.1") || url.host_str() == Some("localhost"))
                        && window::current_server_port(&app_handle) == url.port()
                {
                    NewWindowResponse::Allow
                } else {
                    let _ = tauri_plugin_opener::open_url(url.as_str(), None::<&str>);
                    NewWindowResponse::Deny
                }
            })
            .build()?;
            // 窗口底色跟随系统主题(与前端 CSS prefers-color-scheme 一致,避免加载前黑/白闪)
            let theme = main_win.theme().unwrap_or(tauri::Theme::Dark);
            let bg = match theme {
                tauri::Theme::Dark => tauri::window::Color(21, 21, 23, 255),
                tauri::Theme::Light => tauri::window::Color(255, 255, 255, 255),
                _ => tauri::window::Color(21, 21, 23, 255),
            };
            main_win.set_background_color(Some(bg))?;
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
            open_settings,
            get_theme_preference
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
