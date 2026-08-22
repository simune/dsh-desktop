//! 窗口导航守卫:顶层 WebView 只放行本地壳页面与当前 dsh loopback URL。
//! 对应 docs/01 §8(导航白名单)与方案 A-1。
//!
//! 平台差异(wry#1593):Windows 上 iframe 内导航通常**不**触发 on_navigation;
//! macOS/Linux 上 iframe 加载 dsh URL **会**触发。因此必须放行当前服务的
//! loopback URL,否则 mac 上 iframe 无法加载 → 标题栏下空白页。
//! 冲顶整页跳转由前端 iframe sandbox(无 allow-top-navigation)防御。
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

/// 是否为当前 running 服务的 loopback dsh URL(供 iframe 加载;macOS 必放行)。
fn is_current_dsh_url(app: &AppHandle, url: &Url) -> bool {
    let host = url.host_str().unwrap_or("");
    if url.scheme() != "http" && url.scheme() != "https" {
        return false;
    }
    if host != "127.0.0.1" && host != "localhost" {
        return false;
    }
    current_server_port(app) == url.port()
}

pub fn navigation_guard(app: &AppHandle, url: &Url) -> bool {
    if is_local_shell(url) {
        return true;
    }
    // macOS/Linux:iframe 导航会进 on_navigation,必须放行,否则内容区空白。
    // Windows:iframe 通常不进此回调;若有顶层冲顶,由 iframe sandbox 挡住。
    if is_current_dsh_url(app, url) {
        return true;
    }

    eprintln!("[nav-guard] 拦截导航: {url}");
    false
}

pub fn current_server_port(app: &AppHandle) -> Option<u16> {
    let mgr = app.try_state::<Arc<ServerManager>>()?;
    match mgr.state() {
        ServerState::Running { url } => url.rsplit(':').next()?.parse().ok(),
        _ => None,
    }
}
