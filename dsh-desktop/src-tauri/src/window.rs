//! 窗口导航守卫:仅放行本地壳页面与 loopback dsh URL,其余交系统浏览器。
//! 对应 docs/01 §8(导航白名单)。
use crate::server::{ServerManager, ServerState};
use std::sync::Arc;
use tauri::{AppHandle, Manager, Url};

/// 是否为本地壳页面(tauri://localhost / http://tauri.localhost / dev localhost:1420)
pub fn is_local_shell(url: &Url) -> bool {
    let host = url.host_str().unwrap_or("");
    let scheme = url.scheme();
    (scheme == "tauri" && host == "localhost")
        || (scheme == "http" && host == "tauri.localhost")
        || (scheme == "http" && host == "localhost" && url.port() == Some(1420))
}

pub fn navigation_guard(app: &AppHandle, url: &Url) -> bool {
    if is_local_shell(url) {
        return true;
    }
    let host = url.host_str().unwrap_or("");
    let scheme = url.scheme();

    // 本地壳页面
    if scheme == "tauri" && host == "localhost" {
        return true;
    }
    if scheme == "http" && host == "tauri.localhost" {
        return true;
    }
    if scheme == "http" && host == "localhost" && url.port() == Some(1420) {
        return true; // dev 壳(devUrl)
    }

    // iframe 方案(方案 A-1):dsh UI 由 shell 页面内的 iframe 承载,顶层 WebView 只应停留在
    // 壳页面。任何顶层导航离开壳页面(如被某段 JS 误触发的 location 跳转到 dsh URL)都会
    // 破坏标题栏常驻布局,故一律拒绝,并打印日志便于排查来源。
    let _ = app;
    eprintln!("[nav-guard] 拦截顶层导航离开壳页面: {url}");
    false
}

pub fn current_server_port(app: &AppHandle) -> Option<u16> {
    let mgr = app.try_state::<Arc<ServerManager>>()?;
    match mgr.state() {
        ServerState::Running { url } => url.rsplit(':').next()?.parse().ok(),
        _ => None,
    }
}
