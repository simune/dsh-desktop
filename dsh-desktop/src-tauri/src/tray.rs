//! 托盘:状态显示、打开主界面、重启服务、退出。对应 docs/04 §4。
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager};

/// 托盘状态项(静态持有以便更新文本;TrayIcon 无菜单 getter)
static STATUS_ITEM: Mutex<Option<MenuItem<tauri::Wry>>> = Mutex::new(None);

pub fn setup(app: &App) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "tray-status", "DSH 启动中…", true, None::<&str>)?;
    *STATUS_ITEM.lock().unwrap() = Some(status.clone());
    let open = MenuItem::with_id(app, "tray-open", "打开主界面", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "tray-restart", "重启服务", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(app, &[&status, &open, &restart, &separator, &quit])?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("DSH Desktop")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                show_main(app);
            }
        })
        .build(app)?;
    Ok(())
}

/// 由 server-status 事件驱动更新托盘状态文本
pub fn update_status(text: &str) {
    if let Some(item) = STATUS_ITEM.lock().unwrap().as_ref() {
        let _ = item.set_text(text);
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
