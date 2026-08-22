//! Server Manager:管理 dsh web 子进程全生命周期。
//! 对应 docs/02(状态机、启动/停止/重启算法、输出解析、健康检查)。
use crate::runtime::{self, Runtime};
use crate::settings::{AppSettings, PortPolicy};
use regex::Regex;
use serde::Serialize;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const START_TIMEOUT: Duration = Duration::from_secs(60);
static LOG_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// 初始化持久日志文件(追加模式,每行 flush)。由 setup 调用。
pub fn init_log_file(path: PathBuf) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    *LOG_FILE.lock().unwrap() = Some(path);
}
const GRACE_PERIOD: Duration = Duration::from_secs(5);
const MAX_CRASHES: u32 = 5;
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ServerState {
    Starting,
    Running { url: String },
    Stopping,
    Stopped,
    Error { code: String, message: String },
}

enum LoopMsg {
    Stop,
}

enum WaitOutcome {
    Ready(String),
    Exited(Option<i32>),
    Timeout,
    Stopped,
}

enum ServeOutcome {
    Stopped,
    Exited(Option<i32>),
}

pub struct ServerManager {
    state: Arc<Mutex<ServerState>>,
    logs: Arc<Mutex<VecDeque<String>>>,
    log_lines: usize,
    stop_tx: Mutex<Option<Sender<LoopMsg>>>,
    handle: Mutex<Option<JoinHandle<()>>>,
    app: AppHandle,
    settings: Arc<Mutex<AppSettings>>,
    resource_dir: PathBuf,
    /// 本地壳页面 URL(错误页回跳用)
    shell_url: String,
}

impl ServerManager {
    pub fn new(app: AppHandle, settings: Arc<Mutex<AppSettings>>, resource_dir: PathBuf) -> Self {
        let shell_url = if cfg!(debug_assertions) {
            "http://localhost:1420".to_string()
        } else {
            "tauri://localhost".to_string()
        };
        let log_lines = settings.lock().unwrap().log_lines.max(100);
        Self {
            state: Arc::new(Mutex::new(ServerState::Stopped)),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            log_lines,
            stop_tx: Mutex::new(None),
            handle: Mutex::new(None),
            app,
            settings,
            resource_dir,
            shell_url,
        }
    }

    pub fn start(&self) -> Result<(), String> {
        let mut h = self.handle.lock().unwrap();
        if h.is_some() {
            return Ok(()); // 已在运行
        }
        let (tx, rx) = mpsc::channel::<LoopMsg>();
        *self.stop_tx.lock().unwrap() = Some(tx);
        let app = self.app.clone();
        let state = self.state.clone();
        let logs = self.logs.clone();
        let log_lines = self.log_lines;
        let settings = self.settings.clone();
        let resource_dir = self.resource_dir.clone();
        let shell_url = self.shell_url.clone();
        *h = Some(std::thread::spawn(move || {
            server_loop(app, state, logs, log_lines, settings, resource_dir, shell_url, rx);
        }));
        Ok(())
    }

    pub fn stop(&self) {
        let tx = self.stop_tx.lock().unwrap().take();
        if let Some(tx) = tx {
            let _ = tx.send(LoopMsg::Stop);
        }
        if let Some(h) = self.handle.lock().unwrap().take() {
            let _ = h.join();
        }
    }

    pub fn restart(&self) -> Result<(), String> {
        self.stop();
        self.start()
    }

    pub fn state(&self) -> ServerState {
        self.state.lock().unwrap().clone()
    }

    pub fn logs(&self, tail: usize) -> Vec<String> {
        let logs = self.logs.lock().unwrap();
        logs.iter().rev().take(tail).rev().cloned().collect()
    }
}

// ---------- 服务线程 ----------

#[allow(clippy::too_many_arguments)]
fn server_loop(
    app: AppHandle,
    state: Arc<Mutex<ServerState>>,
    logs: Arc<Mutex<VecDeque<String>>>,
    log_lines: usize,
    settings: Arc<Mutex<AppSettings>>,
    resource_dir: PathBuf,
    shell_url: String,
    rx: Receiver<LoopMsg>,
) {
    let mut crash_count = 0u32;
    let mut backoff = Duration::from_secs(1);
    let mut stopped = false;

    while !stopped {
        // 每次尝试取设置快照(DSH_HOME/端口策略修改在下次重启生效)
        let settings = settings.lock().unwrap().clone();
        // 1. 定位运行时
        let rt = match runtime::resolve_runtime(&settings, &resource_dir) {
            Ok(rt) => rt,
            Err(e) => {
                let msg = format!("无法定位运行时:{e}");
                fail(&app, &state, &logs, log_lines, "E_RUNTIME_NOT_FOUND", &msg);
                break;
            }
        };
        log(&logs, log_lines, &format!("[app] runtime: {}", rt.source));

        // 1.1 确保 web profile 的 bundles 包含捆绑插件(安装树可解析时)
        if let Some(dsh_home) = resolve_dsh_home(&settings) {
            let dsh_pkg_dir = rt.dsh_bin.parent().and_then(|p| p.parent()); // <dsh>/lib/bin.js -> <dsh>
            if let Some(pkg) = dsh_pkg_dir {
                ensure_usage_stats_bundle(&dsh_home, pkg, &logs, log_lines);
            }
        }

        set_state(&state, ServerState::Starting);
        emit_status(&app, &state);
        log(&logs, log_lines, "[app] starting dsh web (--port 0)");

        // 2. spawn
        let mut child = match spawn_dsh(&rt, &settings, &logs, log_lines) {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("spawn 失败:{e}");
                fail(&app, &state, &logs, log_lines, "E_SPAWN_FAILED", &msg);
                break;
            }
        };

        // 3. 等待 URL + 健康检查
        let url = match wait_ready(&mut child, &logs, log_lines, &rx) {
            WaitOutcome::Ready(url) => url,
            WaitOutcome::Stopped => {
                stopped = true;
                let _ = cleanup_child(&mut child);
                set_state(&state, ServerState::Stopped);
                emit_status(&app, &state);
                break;
            }
            WaitOutcome::Exited(code) => {
                let msg = format!("启动阶段子进程退出 code={code:?}");
                log(&logs, log_lines, &format!("[app] {msg}"));
                let _ = cleanup_child(&mut child);
                crash_count += 1;
                if !restart_or_fail(
                    &app, &state, &logs, log_lines, &rx, &mut stopped,
                    &mut crash_count, &mut backoff, code,
                ) {
                    // 崩溃超限:回到本地错误页(窗口此刻显示的是旧 dsh URL)
                    navigate_shell(&app, &shell_url);
                    break;
                }
                continue;
            }
            WaitOutcome::Timeout => {
                let msg = format!("启动超时(>{}s)", START_TIMEOUT.as_secs());
                fail(&app, &state, &logs, log_lines, "E_START_TIMEOUT", &msg);
                let _ = cleanup_child(&mut child);
                break;
            }
        };

        // 4. 运行态
        crash_count = 0;
        backoff = Duration::from_secs(1); // 成功后重置退避(docs/02 §6)
        set_state(&state, ServerState::Running { url: url.clone() });
        emit_status(&app, &state);
        log(&logs, log_lines, &format!("[app] running: {url}"));
        // 方案 A-1:dsh UI 由壳页面 iframe 承载,顶层 WebView 不主动离开壳。
        // 只 emit server-status / 更新 get_server_status,由前端挂 iframe。
        // 勿在此 navigate 到 dsh URL(会整页替换壳、弄丢自绘标题栏)。

        let serve = loop {
            if rx.recv_timeout(Duration::from_millis(200)).is_ok() {
                log(&logs, log_lines, "[app] stopping (user request)");
                graceful_stop(&mut child, &logs, log_lines);
                set_state(&state, ServerState::Stopped);
                emit_status(&app, &state);
                break ServeOutcome::Stopped;
            }
            match child.try_wait() {
                Ok(Some(status)) => break ServeOutcome::Exited(status.code()),
                Ok(None) => {}
                Err(e) => {
                    log(&logs, log_lines, &format!("[app] try_wait error: {e}"));
                    break ServeOutcome::Exited(None);
                }
            }
        };

        match serve {
            ServeOutcome::Stopped => stopped = true,
            ServeOutcome::Exited(code) => {
                crash_count += 1;
                log(
                    &logs,
                    log_lines,
                    &format!("[app] child exited code={code:?} (crash #{crash_count})"),
                );
                set_state(&state, ServerState::Stopped);
                emit_status(&app, &state);
                if !restart_or_fail(
                    &app, &state, &logs, log_lines, &rx, &mut stopped,
                    &mut crash_count, &mut backoff, code,
                ) {
                    // 崩溃超限:回到本地错误页(窗口此刻显示的是远程 UI)
                    navigate_shell(&app, &shell_url);
                    break;
                }
            }
        }
    }
}

/// 崩溃后决定:继续重启(true)或超限进入错误态(false)。
#[allow(clippy::too_many_arguments)]
fn restart_or_fail(
    app: &AppHandle,
    state: &Arc<Mutex<ServerState>>,
    logs: &Arc<Mutex<VecDeque<String>>>,
    log_lines: usize,
    rx: &Receiver<LoopMsg>,
    stopped: &mut bool,
    crash_count: &mut u32,
    backoff: &mut Duration,
    code: Option<i32>,
) -> bool {
    if *crash_count >= MAX_CRASHES {
        let msg = format!("连续崩溃 {MAX_CRASHES} 次,停止自动重启(最近退出码 {code:?})");
        log(logs, log_lines, &format!("[app] {msg}"));
        set_state(state, ServerState::Error {
            code: "E_CRASH_LIMIT".into(),
            message: msg,
        });
        emit_status(app, state);
        return false;
    }
    log(logs, log_lines, &format!("[app] restart in {backoff:?}"));
    let deadline = Instant::now() + *backoff;
    while Instant::now() < deadline {
        if rx.recv_timeout(Duration::from_millis(100)).is_ok() {
            *stopped = true;
            set_state(state, ServerState::Stopped);
            emit_status(app, state);
            return true;
        }
    }
    *backoff = (*backoff * 2).min(MAX_BACKOFF);
    true
}

fn fail(
    app: &AppHandle,
    state: &Arc<Mutex<ServerState>>,
    logs: &Arc<Mutex<VecDeque<String>>>,
    log_lines: usize,
    code: &str,
    msg: &str,
) {
    log(logs, log_lines, &format!("[app] ERROR {code}: {msg}"));
    set_state(state, ServerState::Error {
        code: code.to_string(),
        message: msg.to_string(),
    });
    emit_status(app, state);
}

fn navigate_webview(app: &AppHandle, url: &str) {
    if let Some(w) = app.get_webview_window("main") {
        let url: tauri::Url = url
            .parse()
            .unwrap_or_else(|_| "about:blank".parse().unwrap());
        let _ = w.navigate(url);
    }
}

fn navigate_shell(app: &AppHandle, shell_url: &str) {
    navigate_webview(app, shell_url);
}

// ---------- spawn / 解析 / 健康检查 ----------

fn spawn_dsh(
    rt: &Runtime,
    settings: &AppSettings,
    logs: &Arc<Mutex<VecDeque<String>>>,
    log_lines: usize,
) -> Result<Child, String> {
    let mut cmd = Command::new(&rt.node);
    cmd.arg(&rt.dsh_bin).arg("web");
    // --no-open: 桌面 WebView 是唯一界面,禁止 dsh 再调起系统浏览器(dsh web 默认会 open)。
    // 仅对 bundled 运行时传(版本固定 0.1.0-rc.8+,确定支持);path/config 回退时的外部 dsh
    // 可能是旧版(如 0.1.0-rc.6 不支持 --no-open),传了会 unknown option 崩溃。
    if rt.source == "bundled" {
        cmd.arg("--no-open");
    } else {
        log(logs, log_lines, "[app] runtime 非 bundled,不传 --no-open(兼容旧 dsh)");
    }
    cmd.arg("--port");
    match &settings.port_policy {
        PortPolicy::Auto => {
            cmd.arg("0");
        }
        PortPolicy::Fixed { port } => {
            cmd.arg(port.to_string());
            log(logs, log_lines, &format!("[app] port policy: fixed {port}"));
        }
    }
    cmd.current_dir(&settings.cwd.clone().map(PathBuf::from).unwrap_or_else(user_home));
    if let Some(home) = &settings.dsh_home {
        cmd.env("DSH_HOME", home);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        // 子进程成为新进程组组长,兜底时 kill(-pid) 整组清理(防孙进程孤儿)
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        // CREATE_NEW_PROCESS_GROUP(0x200):与 taskkill /T 配合按树清理;
        // CREATE_NO_WINDOW(0x08000000):node.exe 是控制台程序,避免从 GUI 进程
        // spawn 时弹出黑色控制台窗口。参见 docs/02 §6 跨平台进程组策略。
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    cmd.spawn().map_err(|e| e.to_string())
}

/// 用户主目录:macOS/Linux 用 HOME;Windows 用 USERPROFILE(回退 HOMEDRIVE+HOMEPATH)。
/// 作为子进程默认 cwd(与 dsh 自身行为一致;设置项 cwd 优先)。
fn user_home() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOMEDRIVE")
                    .zip(std::env::var_os("HOMEPATH"))
                    .map(|(d, p)| PathBuf::from(d).join(p))
            })
            .unwrap_or_else(|| PathBuf::from("C:\\"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"))
    }
}

#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn wait_ready(
    child: &mut Child,
    logs: &Arc<Mutex<VecDeque<String>>>,
    log_lines: usize,
    rx: &Receiver<LoopMsg>,
) -> WaitOutcome {
    // stdout 解析线程:逐行匹配 URL
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => return WaitOutcome::Exited(None),
    };
    let (url_tx, url_rx) = mpsc::channel::<String>();
    let logs_c = logs.clone();
    let reader = std::thread::spawn(move || {
        let re = Regex::new(r"^dsh web: (http://127\.0\.0\.1:\d+)").unwrap();
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            log(&logs_c, log_lines, &format!("[out] {line}"));
            if let Some(cap) = re.captures(&line) {
                if url_tx.send(cap[1].to_string()).is_err() {
                    break;
                }
            }
        }
    });
    // stderr → 日志
    if let Some(stderr) = child.stderr.take() {
        let logs_c = logs.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(line) = line {
                    log(&logs_c, log_lines, &format!("[err] {line}"));
                }
            }
        });
    }

    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if let Ok(url) = url_rx.try_recv() {
            log(logs, log_lines, &format!("[app] url 行解析成功:{url}"));
            if health_check(&url, deadline) {
                let _ = reader.thread().id(); // reader 线程随 stdout EOF 自行结束
                return WaitOutcome::Ready(url);
            }
            log(logs, log_lines, "[app] 健康检查未通过,继续等待…");
        }
        if rx.recv_timeout(Duration::from_millis(100)).is_ok() {
            return WaitOutcome::Stopped;
        }
        match child.try_wait() {
            Ok(Some(status)) => return WaitOutcome::Exited(status.code()),
            Ok(None) => {}
            Err(_) => return WaitOutcome::Exited(None),
        }
        if Instant::now() > deadline {
            return WaitOutcome::Timeout;
        }
    }
}

fn health_check(url: &str, deadline: Instant) -> bool {
    let port: u16 = match url.rsplit(':').next().and_then(|p| p.parse().ok()) {
        Some(p) => p,
        None => return false,
    };
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    // 阶段1:TCP 连通
    let mut tcp_ok = false;
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok() {
            tcp_ok = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if !tcp_ok {
        return false;
    }
    // 阶段2:HTTP GET / 返回 2xx/3xx
    loop {
        if http_get_ok(port) {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn http_get_ok(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
    let req = format!("GET / HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 256];
    match s.read(&mut buf) {
        Ok(n) => {
            let head = String::from_utf8_lossy(&buf[..n]);
            head.starts_with("HTTP/1.0 2")
                || head.starts_with("HTTP/1.1 2")
                || head.starts_with("HTTP/1.0 3")
                || head.starts_with("HTTP/1.1 3")
        }
        Err(_) => false,
    }
}

// ---------- 停止与清理 ----------

fn graceful_stop(child: &mut Child, logs: &Arc<Mutex<VecDeque<String>>>, log_lines: usize) {
    #[cfg(unix)]
    {
        // std Child::kill() 在 unix 是 SIGKILL(立即终止主进程);
        // 孙进程由下方 kill_process_group 兜底(宽限后整组清理)
        log(logs, log_lines, "[app] SIGKILL → 宽限 5s → 兜底进程组 SIGKILL");
        let _ = child.kill();
        let deadline = Instant::now() + GRACE_PERIOD;
        while Instant::now() < deadline {
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    #[cfg(windows)]
    {
        // Windows:node.exe 是控制台程序,taskkill 不带 /F 对它无效
        // ("can only be terminated forcefully"),直接 /T /F 递归杀树;
        // 不先 TerminateProcess 主进程——否则孙进程会成孤儿(docs/02 §6)。
        log(logs, log_lines, "[app] taskkill /T /F → 等待退出(宽限 5s)");
        taskkill_tree(child.id());
        let deadline = Instant::now() + GRACE_PERIOD;
        while Instant::now() < deadline {
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    kill_process_group(child);
    let _ = child.wait();
}

fn cleanup_child(child: &mut Child) {
    if let Ok(Some(_)) = child.try_wait() {
        return;
    }
    kill_process_group(child);
    let _ = child.wait();
}

fn kill_process_group(child: &Child) {
    #[cfg(unix)]
    {
        let pid = child.id();
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{pid}")])
            .status();
    }
    #[cfg(windows)]
    {
        taskkill_tree(child.id());
    }
}

/// Windows: taskkill /T /F 递归杀树;CREATE_NO_WINDOW 避免从 GUI 调起时闪控制台。
#[cfg(windows)]
fn taskkill_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

// ---------- 工具 ----------

fn log(logs: &Arc<Mutex<VecDeque<String>>>, max: usize, line: &str) {
    let mut l = logs.lock().unwrap();
    l.push_back(format!("{} {line}", timestamp()));
    while l.len() > max {
        l.pop_front();
    }
    if let Some(p) = LOG_FILE.lock().unwrap().as_ref() {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
        {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }
}

fn timestamp() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}.{:03}", d.subsec_millis())
}

fn set_state(state: &Arc<Mutex<ServerState>>, s: ServerState) {
    *state.lock().unwrap() = s;
}

fn emit_status(app: &AppHandle, state: &Arc<Mutex<ServerState>>) {
    let s = state.lock().unwrap().clone();
    println!("[app] status -> {}", serde_json::to_string(&s).unwrap_or_default());
    let _ = app.emit("server-status", s);
}

// ---------- 捆绑插件:web profile bundles 确保 ----------

const PROFILE_PATCH_TEMPLATE: &str = "# Your patch layer for this dsh profile, applied after every bundle layer:
# a top-level YAML array of loader patch entries (id-targeted config
# overrides, disables, and insert lists; `!!js` expressions allowed).
[]
";

const PROFILE_PNPM_WORKSPACE: &str = "packages:
  - .

nodeLinker: hoisted
autoInstallPeers: false
";

/// 解析客户端实际使用的 DSH_HOME(设置 > 环境变量 > 默认 ~/.dsh)
fn resolve_dsh_home(settings: &AppSettings) -> Option<PathBuf> {
    if let Some(h) = &settings.dsh_home {
        return Some(PathBuf::from(h));
    }
    if let Some(h) = std::env::var_os("DSH_HOME") {
        return Some(PathBuf::from(h));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".dsh"))
}

/// 从安装锚点(按 Node node_modules 向上查找)解析包目录
fn resolve_from(anchor_pkg_dir: &Path, name: &str) -> Option<PathBuf> {
    let mut dir = Some(anchor_pkg_dir);
    while let Some(d) = dir {
        let cand = d.join("node_modules").join(name);
        if cand.join("package.json").is_file() {
            return Some(cand);
        }
        dir = d.parent();
    }
    None
}

/// 确保 web profile 的 bundles 含捆绑插件(可解析时),并按 dsh 模板预创建 profile。
/// 幂等;不触碰用户已有配置(仅追加缺失 bundle)。
fn ensure_usage_stats_bundle(
    dsh_home: &Path,
    dsh_pkg_dir: &Path,
    logs: &Arc<Mutex<VecDeque<String>>>,
    log_lines: usize,
) {
    const BUNDLE: &str = "dsh-usage-stats";
    let Some(plugin_dir) = resolve_from(dsh_pkg_dir, BUNDLE) else {
        return; // 本安装树没有该插件(如未 vendor),不向 profile 追加
    };
    let profile_dir = dsh_home.join("profiles").join("web");
    let manifest_path = profile_dir.join("package.json");

    // 读取或按 dsh initProfile 模板创建
    let mut manifest: serde_json::Value = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| default_profile_manifest()),
        Err(_) => default_profile_manifest(),
    };

    // 确保 bundles 数组包含 BUNDLE
    let bundles = manifest
        .pointer_mut("/dsh/profile/bundles")
        .and_then(|v| v.as_array_mut());
    let mut changed = false;
    match bundles {
        Some(arr) => {
            if !arr.iter().any(|b| b.as_str() == Some(BUNDLE)) {
                arr.push(serde_json::Value::String(BUNDLE.into()));
                changed = true;
            }
        }
        None => {
            manifest["dsh"]["profile"]["bundles"] =
                serde_json::json!(["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app", BUNDLE]);
            changed = true;
        }
    }

    if changed {
        if let Some(dir) = manifest_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n";
        if std::fs::write(&manifest_path, text).is_ok() {
            log(logs, log_lines, &format!("[app] profile bundles 已确保包含 {BUNDLE}"));
        }
    }

    // 补齐 dsh 初始化所需的其余文件(不存在时)
    if !profile_dir.join("cordis.patch.yml").exists() {
        let _ = std::fs::write(profile_dir.join("cordis.patch.yml"), PROFILE_PATCH_TEMPLATE);
    }
    if !profile_dir.join("pnpm-workspace.yaml").exists() {
        let _ = std::fs::write(profile_dir.join("pnpm-workspace.yaml"), PROFILE_PNPM_WORKSPACE);
    }

    // 补 profile 内 node_modules 链接:插件 patch 会按名 import,须从 profile 可解析。
    // 链接已存在但目标无效(如指向旧构建路径或旧空包)时也要重建,否则 dsh 启动会
    // ERR_MODULE_NOT_FOUND。有效性判断:通过链接能解析到 lib/index.js。
    let nm_dir = profile_dir.join("node_modules");
    let link = nm_dir.join(BUNDLE);
    let link_valid = link.join("lib").join("index.js").is_file();
    if !link_valid {
        let _ = std::fs::create_dir_all(&nm_dir);
        // 清掉旧的悬空链接(reparse point 用 remove_dir_all 删除,不含目录则直接删)
        let _ = std::fs::remove_dir_all(&link);
        let _ = std::fs::remove_file(&link);
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            if symlink(&plugin_dir, &link).is_ok() {
                log(logs, log_lines, &format!("[app] profile node_modules 已链接 {BUNDLE}"));
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::symlink_dir;
            if symlink_dir(&plugin_dir, &link).is_ok() {
                log(logs, log_lines, &format!("[app] profile node_modules 已链接 {BUNDLE}"));
            } else if junction::create(&plugin_dir, &link).is_ok() {
                // symlink_dir 需要管理员/开发者模式;非特权环境回退到 junction(无需提权,NTFS 即可)
                log(logs, log_lines, &format!("[app] profile node_modules 已联接(junction) {BUNDLE}"));
            } else {
                log(
                    logs,
                    log_lines,
                    &format!("[app] 警告:无法创建 {BUNDLE} 链接(symlink 与 junction 均失败)"),
                );
            }
        }
    }
}

fn default_profile_manifest() -> serde_json::Value {
    serde_json::json!({
        "name": "dsh-profile-web",
        "private": true,
        "dependencies": {},
        "dsh": { "profile": { "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"] } }
    })
}
