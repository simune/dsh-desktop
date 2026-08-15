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
    // 远程 dsh:loopback 且端口与当前服务一致
    if host == "127.0.0.1" || host == "localhost" {
        if let Some(port) = current_server_port(app) {
            if url.port() == Some(port) {
                return true;
            }
        }
    }
    // 其余:系统浏览器打开
    let _ = tauri_plugin_opener::open_url(url.as_str(), None::<&str>);
    false
}

fn current_server_port(app: &AppHandle) -> Option<u16> {
    let mgr = app.try_state::<Arc<ServerManager>>()?;
    match mgr.state() {
        ServerState::Running { url } => url.rsplit(':').next()?.parse().ok(),
        _ => None,
    }
}
