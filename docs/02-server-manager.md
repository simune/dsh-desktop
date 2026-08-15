# Server Manager 详细设计

> 上游:`docs/01-architecture.md` §2(进程模型)与 §5(接口);`dsh-tauri-plan.md` §2。
> 本文件是 `src-tauri/src/server.rs` 的实现规格,含状态机、启动/停止/重启算法、输出解析、健康检查与测试要点。

## 1. 目标与边界

**职责**:管理 dsh web 子进程的完整生命周期,对外暴露状态与日志,保证"启动成功即 URL 可用、退出后零残留"。

**边界**:
- 不做:HTTP 代理、请求改写、凭据管理(全部留在 dsh 自身)。
- 不依赖:任何 dsh 内部 API(唯一事实来源是 stdout 的 URL 行 + TCP/HTTP 探测)。

## 2. 状态机

```
        start()                    URL 行解析 + 健康检查通过
Starting ───────────────► Running ──────────────► Stopping
   │  ▲                        │   ▲                 │
   │  │ 重启(退避后)           │   │ stop()/退出       │
   │  │                        │   │                 ▼
   │  └──────────┐             │   └──────────────► Stopped
   │  崩溃(非主动)│             │(SIGKILL 完成)
   │             │             ▼
   │        Stopped(临时)   Error ◄──── 连续崩溃≥5 / 超时 / spawn 失败
   │                          │
   └──── 用户点"重试" ─────────┘
```

转移表:

| 当前态 | 事件 | 动作 | 下一态 |
|---|---|---|---|
| Starting | 解析到 URL | 健康检查(TCP→HTTP) | (检查中) |
| (检查中) | 通过 | 广播 `running{url}` | Running |
| Starting/(检查中) | 超时 60s | 记 `E_START_TIMEOUT` | Error |
| Starting/(检查中) | 子进程退出 | 记退出码+stderr | Error |
| Running | 子进程退出(非主动) | 退避后 start() | Starting |
| Running | stop()(主动) | SIGTERM→宽限→SIGKILL | Stopping→Stopped |
| Running | 连续崩溃计数 ≥5 | 记 `E_CRASH_LIMIT` | Error |
| Error | `restart()` / 用户重试 | 清零崩溃计数,start() | Starting |
| 任意 | 单实例接管 | 复用已有窗口,不新建服务 | (不变) |

## 3. 启动流程(算法)

```
fn start(rt, cfg):
  let (node, dsh_dir) = resolve_runtime(cfg)          # 见 docs/03 §5
  let cmd = build_command(node, dsh_dir, cfg)
  child = cmd.spawn()  → 失败: E_SPAWN_FAILED

  state = Starting; 崩溃计数不在此清零(由 restart 清零)
  启动 stdout/stderr 两个读线程(或 async task)
  deadline = now + 60s
  loop:
    if let Some(url) = url_rx.try_recv():
      if health_check(url, deadline): → Running; return
      else if now > deadline: → E_HEALTH_FAIL
    if child.try_wait() == Some(status): → E_CHILD_EXITED(status)
    if now > deadline: → E_START_TIMEOUT
    sleep 50ms
```

### build_command(关键参数)

```
程序:  <node> <dsh_dir>/lib/bin.js web --port 0
stdin:  null(不向子进程写任何输入)
stdout: piped
stderr: piped
cwd:    cfg.cwd 或用户主目录(env::home_dir)
env:    继承客户端环境(含 DSH_HOME 透传),另注入:
        DSH_HOME = cfg.dsh_home 若设置
        NO_COLOR = "1"(可选,便于日志解析)
进程组: macOS/Linux: process_group(0)
        Windows: CREATE_NEW_PROCESS_GROUP(见 §6)
```

> 端口策略 `fixed`:把 `--port 0` 换成 `--port <n>`;解析逻辑不变,但需处理"端口被占 → 启动失败/自检"分支。

## 4. stdout / stderr 解析

### 4.1 URL 行提取

真实输出(已验证,`dsh-web-app/lib/index.js:107`):

```
dsh web: http://127.0.0.1:53001
dsh web: http://127.0.0.1:53001 (LAN: http://192.168.1.5:53001)   ← 有 LAN 候选时
```

匹配正则(Rust):

```rust
static URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^dsh web: (http://127\.0\.0\.1:\d+)").unwrap()
});
```

- 取捕获组 1 作为主 URL;行尾 LAN 后缀忽略。
- 同一进程可能打印多行(重启场景),**只取第一个成功**匹配;重复行忽略。
- 非匹配行(如错误堆栈)照常进日志缓冲,不参与解析。

### 4.2 逐行读取与缓冲

- 每行按 `\n` 切分,残留半行保留到下次读取(处理无换行结尾)。
- 合并缓冲:stdout 行加 `[out]`、stderr 行加 `[err]` 前缀,统一 `RingBuffer<String>`。
- 读取线程在子进程退出后读到 EOF 即结束;主流程用 `try_wait` 感知退出,不依赖读线程。

### 4.3 URL 提取后的健康检查

```
fn health_check(url, deadline) -> bool:
  port = parse_port(url)
  阶段1 TCP:  循环尝试 TcpStream::connect(("127.0.0.1", port)),
              每 200ms 一次,直到 deadline
  阶段2 HTTP: GET http://127.0.0.1:{port}/ ,200/3xx 即视为通过
              (连接成功但非 200 → 继续重试;dsh 前端需要时间就绪)
```

- 健康检查**通过**的判定 = TCP 连通 **且** `/` 返回 2xx/3xx。`/api` 由 dsh UI 自测,客户端不强求。
- 阶段 2 失败但未到 deadline → 继续;到 deadline → `E_HEALTH_FAIL`。

## 5. 停止与清理

```
fn stop():
  user_intent = true(先置位,防止回调触发重启)
  若 child 已退出: 直接 Stopped,return
  child.kill()                    # Unix: SIGTERM;Windows: 见下
  deadline = now + 5s
  loop:
    if child.try_wait().is_some(): → Stopped, return
    if now > deadline: break
    sleep 100ms
  kill_process_group(child)       # 兜底,连孙进程一起
  wait 500ms → Stopped
```

- **进程组清理(macOS/Linux)**:spawn 时 `process_group(0)` 使子进程成为新进程组组长;兜底时 `kill(-child_pid, SIGKILL)` 整组击杀。防 bash 工具派生的孙进程成孤儿。
- **Windows**:spawn 时 `CREATE_NEW_PROCESS_GROUP`;优雅期用 `GenerateConsoleCtrlEvent(CTRL_BREAK)`(尽力);兜底 `taskkill /T /F /PID <pid>`(递归杀树)。
- 平台差异收敛到两个函数:`spawn_with_group(cmd) -> Child` 与 `kill_group(child)`,用 `#[cfg(target_os=...)]` 隔离,其余代码平台无关(M3.5 的跨平台预留点)。

## 6. 崩溃重启策略

| 参数 | 值 |
|---|---|
| 判定 | 非 `user_intent` 的子进程退出 |
| 退避 | 1s, 2s, 4s, 8s, 16s, 封顶 30s(每次重启后加倍,成功后清零) |
| 最大连续崩溃 | 5 次 |
| 超限动作 | 进入 Error,广播 `server-error{E_CRASH_LIMIT, history}` |

重启时:主窗口若在显示 dsh UI,先切回加载页(事件驱动,避免用户看到白屏/错误连接);新一轮 URL 就绪后再导航。

## 7. 单实例与"复用已有服务"

- 单实例:`tauri-plugin-single-instance` 在 App 层(见 `docs/04` §8),Server Manager 不感知。
- 复用已有服务:仅在"检测到本客户端托管的服务仍在运行"(如崩溃恢复中窗口被关)时,`start()` 前先检查 `state` 与旧 child 存活;不检测外部进程(端口随机,无从知晓;固定端口策略下可在设置页提示端口占用)。

## 8. Rust 代码骨架(实现参考)

```rust
// server.rs —— 核心结构
pub struct ServerManager {
    child: Option<Child>,
    state: Arc<Mutex<ServerState>>,
    log: Arc<Mutex<RingBuffer<String>>>,
    url_tx: mpsc::Sender<Option<String>>,   // 解析线程 → 主流程
    crash_count: u32,
    user_intent: Arc<AtomicBool>,
    app_handle: AppHandle,                  // 用于 emit 事件
}

// 事件(emit 到前端)
// server-status: { state: "running", url, port }
// server-status: { state: "error", code, message, logs_tail }
// server-log:    { lines: [...] }            // 增量,设置页日志视图用

pub enum ServerError {
    RuntimeNotFound(String),
    SpawnFailed(String),
    StartTimeout,
    ChildExited { code: Option<i32>, stderr_tail: Vec<String> },
    HealthFailed,
    CrashLimit { history: Vec<Option<i32>> },
}
```

实现要点:
- `child.wait()` 用独立线程,`try_wait` 轮询由主流程驱动即可,避免阻塞。
- 所有跨线程状态经 `Arc<Mutex<..>>`,事件经 `mpsc` + `app.emit`,不在锁内做 IO。
- RingBuffer 可自实现(固定容量 VecDeque)或引入 `ringbuffer` crate。

## 9. 测试要点

### 9.1 单元测试

| 用例 | 输入 | 断言 |
|---|---|---|
| URL 解析 | `dsh web: http://127.0.0.1:53001` | 提取 `http://127.0.0.1:53001` |
| URL 解析(LAN) | `... (LAN: http://192.168.1.5:53001)` | 仍提取 127.0.0.1 主 URL |
| URL 解析(干扰行) | 前面若干任意日志行 | 跳过,直到匹配行 |
| 半行拼接 | 两段读入拼成完整行 | 正确解析 |
| 端口解析 | URL → 53001 | 正确 |
| 退避序列 | 连续崩溃计数 | 1,2,4,8,16,30 封顶 |

### 9.2 集成测试(用假 dsh 脚本)

`tests/fake-dsh.mjs`:按需输出指定行序列,可注入延迟/崩溃/垃圾输出,用于验证:
- 正常:打印 URL → 起本地 200 服务 → 健康检查通过。
- 崩溃:URL 后立即 `process.exit(1)` → 触发重启。
- 静默:不打印 URL → 60s 超时 → Error。
- 垃圾输出:先打 100 行噪声再打 URL。

### 9.3 手动场景(纳入 `docs/05` 的 M1 验收)

1. `kill -9` 子进程 → 自动重启并恢复。
2. 退出 App → `pgrep -f "bin.js web"` 为空。
3. 连续 `kill -9` 5 次 → 错误页,点"重试"恢复。
4. 端口策略 fixed=3080 且被占用 → 明确报错而非挂死。
