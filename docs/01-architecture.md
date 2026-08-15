# DSH × Tauri 桌面客户端 · 架构落地设计

> 上游:`dsh-tauri-plan.md` §1(总体架构)、§4(Tauri 骨架);计划:`docs/00-project-plan.md`。
> 本文定义 v1 的组件边界、进程模型、目录结构、关键接口与安全模型,是各模块详细设计(`02`~`04`)的总纲。

## 1. 系统上下文

```
┌────────────────────────────────────────────────────────────┐
│ macOS 桌面客户端(dsh-desktop)                               │
│                                                            │
│  ┌──────────┐   ┌──────────┐   ┌───────────────────────┐  │
│  │ 主窗口    │   │ 设置窗口  │   │ Server Manager        │  │
│  │ (远程 dsh │   │ (本地壳   │   │ spawn/解析/健康检查/   │  │
│  │  UI,     │   │  UI,     │   │ 进程组清理/崩溃重启     │  │
│  │  无 IPC) │   │  有 IPC) │   └──────────┬────────────┘  │
│  └────┬─────┘   └────┬─────┘              │ spawn         │
│       │ navigate     │ invoke commands    ▼               │
│  ┌────▼──────────────────────────────────────────────┐    │
│  │ Tauri 核心(事件总线:server-status / server-error)   │    │
│  └───────────────────────────────────────────────────┘    │
└────────────────────────────────────────────────────────────┘
        │ spawn: node <dsh>/lib/bin.js web --port 0
        ▼
┌────────────────────────────────────────────────────────────┐
│ dsh web 子进程(Node,Cordis 插件树)                          │
│  · webserver 127.0.0.1:<port>  → / (SPA) + /api (网关)      │
│  · 读写 $DSH_HOME/profiles/web(cordis.yml 重写、符号链接自愈)│
│  · stdout: "dsh web: http://127.0.0.1:<port>"               │
└────────────────────────────────────────────────────────────┘
```

**核心不变式**:客户端**绝不修改** dsh 代码、dsh 前端、用户 profile;它只负责"起进程、读一行 URL、开窗口、关进程"。

## 2. 进程模型与生命周期

### 2.1 启动序列

```
App 启动
  ├─ 单实例检查(已有实例 → 聚焦主窗口,退出本进程)
  ├─ 解析配置(DSH_HOME、端口策略、是否已托管运行中服务)
  ├─ resolve_runtime() → (node, dsh_dir)(探测链见 docs/03)
  ├─ ServerManager::start()
  │    ├─ spawn(node dsh/bin.js web --port 0,进程组,管道)
  │    ├─ 读线程:逐行扫 stdout → 正则提取 URL → 通知主线程
  │    └─ 健康检查:TCP 连 127.0.0.1:port(重试)→ HTTP GET /(200)
  ├─ 主窗口创建(先显示本地加载页)
  ├─ 收到 URL → webview.navigate(url)
  └─ 广播 server-status: running / error
```

### 2.2 退出序列

```
主窗口关闭 / 托盘"退出" / Cmd+Q
  ├─ 标记 user_intent = true(阻止"崩溃重启"逻辑误触发)
  ├─ ServerManager::stop()
  │    ├─ 发 SIGTERM(仅子进程)
  │    ├─ 等 5s(宽限期,让 dsh 自己 closeAllConnections)
  │    ├─ 仍存活 → 按进程组 SIGKILL(连带孙进程)
  ├─ 关闭设置窗口 / 托盘图标
  └─ 退出 App(ExitCode 0)
```

### 2.3 崩溃重启序列

```
子进程退出且 user_intent == false
  ├─ 记录退出码与 stderr 尾部
  ├─ 等退避(1s,2s,4s,…,上限 30s)
  ├─ 连续崩溃 < 5 次 → 重新走 2.1(主窗口切回加载页)
  └─ 连续崩溃 ≥ 5 次 → 进入 Error 态,主窗口导航到错误页
```

## 3. 模块划分与职责

| 模块 | 文件 | 职责 | 依赖 |
|---|---|---|---|
| 生命周期编排 | `src/main.rs` | 初始化配置、装配各模块、注册 Tauri commands、退出编排 | 全部 |
| Server Manager | `src/server.rs` | 子进程 spawn / stdout 解析 / 健康检查 / 停止 / 重启;状态机与日志环形缓冲 | settings |
| 运行时探测 | `src/runtime.rs` | `resolve_runtime()`:bundled → 配置 → PATH;校验可执行 | 无 |
| 窗口管理 | `src/window.rs` | 主窗口导航、加载页/错误页切换、`on_navigation` 外链拦截、设置窗口管理 | server, settings |
| 设置持久化 | `src/settings.rs` | DSH_HOME、端口策略、开机自启、日志行数等读写(JSON 文件于 `~/Library/Application Support/<app>/settings.json`) | 无 |
| 托盘 | `src/tray.rs` | 状态菜单、打开主界面、重启服务、退出 | server |
| 壳 UI(前端) | `src/`(React) | 加载页、错误页、设置页;仅本地页面持有 Tauri IPC | 无 |

## 4. 目录结构(落地目标态)

```
dsh-desktop/
├─ package.json                  # 前端脚本;devDeps: @tauri-apps/cli, vite, react
├─ vite.config.ts                # 壳 UI 构建;base: './'
├─ index.html
├─ src/                          # 壳 UI(React + TS)
│  ├─ main.tsx
│  ├─ pages/
│  │  ├─ Loading.tsx             # 启动加载页(显示阶段文案:spawning/health-check)
│  │  ├─ Error.tsx               # 错误页(错误码 + stderr 尾部 + 重试按钮)
│  │  └─ Settings.tsx            # 设置页(DSH_HOME/端口策略/日志/自启)
│  └─ lib/ipc.ts                 # invoke 封装(类型化)
├─ scripts/
│  ├─ probe-dsh.mjs              # M0.2 原型:spawn + 解析 URL(可演进为测试工具)
│  ├─ vendor-node.mjs            # M2.1:按平台下载 Node 二进制到 resources/node
│  ├─ vendor-dsh.mjs             # M2.1:npm i --omit=dev 到 resources/dsh
│  └─ prune-dsh.mjs              # M2.4:web profile bundle 闭包裁剪
├─ src-tauri/
│  ├─ Cargo.toml
│  ├─ build.rs                   # 可选:构建期触发 vendor 脚本/资源校验
│  ├─ tauri.conf.json
│  ├─ capabilities/
│  │  └─ shell.json              # 仅本地壳窗口的权限(remote 不授任何权限)
│  ├─ resources/                 # 构建期生成(M2.1),不提交 git
│  │  ├─ dsh/                    # dsh 安装树(--omit=dev)
│  │  └─ node/<arch>/node        # Node 二进制
│  └─ src/
│     ├─ main.rs
│     ├─ server.rs
│     ├─ runtime.rs
│     ├─ window.rs
│     ├─ settings.rs
│     └─ tray.rs
└─ dist/                         # 壳 UI 产物(tauri 打包时内嵌)
```

> `resources/` 由构建脚本生成,加入 `.gitignore`;`probe-dsh.mjs` 保留为回归测试工具。

## 5. 关键接口(草案)

### 5.1 Server Manager(Rust)

```rust
pub enum ServerState { Starting, Running { url: String }, Stopping, Stopped, Error(ServerError) }

pub struct ServerManager {
    state: Arc<Mutex<ServerState>>,
    log: Arc<Mutex<RingBuffer<String>>>,   // stdout+stderr 合并,默认 2000 行
    tx: mpsc::Sender<ServerEvent>,         // 事件:UrlReady / StateChanged / Crashed
}

impl ServerManager {
    pub fn start(&mut self, rt: &Runtime, cfg: &AppSettings) -> Result<(), ServerError>;
    pub fn stop(&mut self);                 // SIGTERM → 宽限 → 进程组 SIGKILL
    pub fn restart(&mut self) -> Result<(), ServerError>;
    pub fn state(&self) -> ServerState;
    pub fn logs(&self, tail: usize) -> Vec<String>;
}
```

事件通过 Tauri `emit` 广播:`server-status { state, url?, port?, error? }`。

### 5.2 Tauri commands(仅壳 UI 可调)

| command | 入参 | 返回 | 说明 |
|---|---|---|---|
| `get_server_status` | - | `{state, url?, port?}` | 轮询/初始化用 |
| `get_logs` | `tail?` | `String[]` | 环形缓冲尾部 |
| `restart_server` | - | `()` | 托盘/设置页调用 |
| `get_settings` / `set_setting` | `key, value` | `AppSettings` | DSH_HOME、端口策略、自启等 |
| `toggle_autostart` | `enabled: bool` | `bool` | LoginItems |
| `open_settings` / `show_main` | - | `()` | 窗口管理 |
| `quit_app` | - | `()` | 走完整退出序列 |

### 5.3 配置模型(`AppSettings`,JSON 持久化)

```jsonc
{
  "dsh_home": null,            // null = 跟随环境变量/默认 ~/.dsh
  "port_policy": "auto",       // "auto" | { "fixed": 3080 }
  "cwd": null,                 // 子进程工作目录,null = 用户主目录
  "autostart": false,
  "log_lines": 2000
}
```

环境优先级:命令行/环境变量(`DSH_HOME`) > 设置文件 > 默认值。设置页修改后立即持久化,端口策略在下次启动服务时生效。

## 6. 错误处理与错误页

### 6.1 错误码表

| 码 | 含义 | 触发 | 错误页动作 |
|---|---|---|---|
| `E_RUNTIME_NOT_FOUND` | 探测链无可用 node/dsh | resolve_runtime 全失败 | 展示探测详情 + 设置页入口 |
| `E_SPAWN_FAILED` | spawn 失败(权限/路径) | start() | 展示错误 + 重试 |
| `E_START_TIMEOUT` | 60s 未就绪 | 超时 | 展示 stderr 尾部 |
| `E_CHILD_EXITED` | 启动阶段子进程退出 | 提前退出 | 展示退出码 + stderr |
| `E_CRASH_LIMIT` | 连续崩溃 ≥5 次 | 重启策略 | 展示最近 5 次退出码 + 日志 |
| `E_HEALTH_FAIL` | 端口有 URL 但健康检查不过 | 健康检查 | 展示日志 + 重试 |
| `E_NAV_FAIL` | WebView 加载失败 | navigate 失败 | 展示 URL + 重试 |

错误页为本地页面(有 IPC),按钮:`重试`、`查看日志`、`打开设置`。

### 6.2 日志规范

- 子进程 stdout 与 stderr **合并**写入环形缓冲(行尾带 `[out]` / `[err]` 前缀)。
- 每条日志带时间戳 `HH:MM:SS.mmm`。
- 客户端自身事件(启动/停止/重启/错误)以 `[app]` 前缀同缓冲记录。
- 缓冲上限默认 2000 行,超限丢弃最旧;设置页可调。

## 7. 安全模型

| 威胁 | 对策 |
|---|---|
| dsh 前端被 XSS 后提权到 Rust | 主窗口加载的是**远程**内容,capabilities **不授予**任何 IPC;`withGlobalTauri: false`;主窗口无 `invoke` 能力 |
| DNS rebinding | dsh 服务端只信任 IP 字面量 Host(`127.0.0.1`);客户端导航仅允许 loopback URL(见 8) |
| 外链跳转 | `on_navigation` 拦截:非 loopback 交给系统浏览器(`tauri-plugin-opener`) |
| 本地端口被恶意页面占用 | 端口策略默认 `--port 0` 随机分配;固定端口仅用户显式开启 |
| 设置文件篡改 | 设置 JSON 只含非敏感项(不含凭据);凭据始终留在 `~/.dsh` |

## 8. 导航白名单(主窗口)

允许导航的目标 URL(正则):

```
^http://127\.0\.0\.1:\d+(/.*)?$
^http://localhost:\d+(/.*)?$      # 仅当 dsh 配置了 localhost 别名时,默认只认 127.0.0.1
```

其余一律拦截 → `tauri-plugin-opener` 打开系统浏览器;`about:blank` / 本地 `tauri://` 页面(加载页/错误页/设置页)放行。

## 9. 与上游方案的对应关系

| 方案章节 | 落地位置 |
|---|---|
| §1 总体架构 | 本文 §1~§2 |
| §2 Server Manager | `docs/02-server-manager.md` |
| §3 运行时与打包 | `docs/03-runtime-bundling.md` |
| §4 Tauri 骨架 / IPC 安全 / 外链 | `docs/04-tauri-shell.md` + 本文 §7~§8 |
| §5~§7 里程碑/风险/决策 | `docs/00-project-plan.md` |
