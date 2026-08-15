# DSH × Tauri 桌面客户端方案

## 0. 核心洞察

`dsh web` 本身就是一个"本地 HTTP 服务 + 浏览器 UI"架构:**服务端进程**(Node 跑 Cordis 插件树,内置 webserver)和**前端**(SPA,由同一服务端在 `/` 提供)。Tauri 客户端要做的事只有一件——**接管"谁来启动服务端、谁来承载 UI"这两个环节**:

1. **服务端**:客户端启动时自动 spawn 一个 `dsh web` 子进程,退出时负责关掉它(解决"每次手动跑 dsh web")
2. **UI**:Tauri 原生 WebView 窗口直接加载 `http://127.0.0.1:<port>`,替代浏览器标签页

**不需要改 dsh 任何代码,不需要改前端任何一行。** 已验证的关键事实:

| 事实 | 验证结果 |
|---|---|
| `dsh web` = `dsh --profile web` | bin.js 里 web 是硬编码别名 |
| profile 位置 | `$DSH_HOME/profiles/web`(DSH_HOME 默认 `~/.dsh`,可用环境变量覆盖) |
| 端口 | 默认 3080;`--port 0` 让 OS 分配空闲端口,`WebServer.port` 返回实际值 |
| URL 发现 | 启动完成后 stdout 打印 `dsh web: http://127.0.0.1:<port>` |
| 信任边界 | loopback 自带信任,WebView 加载无需额外 `--trusted-host` |
| 前端 dist | 打包在 `@deepseek-ai/dsh-web-frontend/dist`,随服务端分发 |
| 每次启动副作用 | 会重写 `profiles/web/cordis.yml`、自愈 `profiles/node_modules` 符号链接 → 客户端需对 DSH_HOME 有写权限 |
| 依赖树 | 342MB,含 node-pty / sharp / koffi 等原生 `.node` 模块 |

## 1. 总体架构

```
┌────────────────────────────────────────────────┐
│ Tauri 桌面客户端 (Rust)                          │
│  ┌──────────────────────────────────────────┐  │
│  │ WebView 窗口                              │  │
│  │  加载 http://127.0.0.1:<port>            │  │
│  │  (dsh 的 SPA,原生浏览器体验,无地址栏)      │  │
│  └──────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────┐  │
│  │ Server Manager (核心)                     │  │
│  │  · spawn dsh web 子进程                    │  │
│  │  · 解析 stdout 的 URL 行                   │  │
│  │  · TCP 健康检查 + 启动超时                  │  │
│  │  · SIGTERM→宽限→SIGKILL(按进程组清理)      │  │
│  │  · 崩溃自动重启 / 错误页                    │  │
│  └──────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────┐  │
│  │ 可选:托盘图标 / 设置窗口 / 开机自启          │  │
│  └──────────────────────────────────────────┘  │
└────────────────────────────────────────────────┘
        │ spawn (node <bundle>/lib/bin.js web --port 0)
        ▼
┌────────────────────────────────────────────────┐
│ dsh web 子进程 (Node)                            │
│  boot $DSH_HOME/profiles/web 插件树              │
│  ├─ webserver 127.0.0.1:<port>                  │
│  │   ├─ /   → SPA 前端 dist                     │
│  │   └─ /api → 网关 (fetch + SSE)               │
│  └─ 会话/存储/凭据沿用用户真实 ~/.dsh             │
└────────────────────────────────────────────────┘
```

## 2. 核心设计:Server Manager

### 2.1 启动时序
1. 客户端启动 → 定位 dsh 运行时(见 §3)
2. spawn: `node <dsh>/lib/bin.js web --port 0`(cwd 可配置,默认用户主目录)
3. 逐行读 stdout,匹配 `^dsh web: (http://127\.0\.0\.1:\d+)`
4. 对解析出的端口做 TCP 连接健康检查(确认 `/` 与 `/api` 可响应)
5. 创建 Tauri 窗口,`webview.navigate(url)` 加载
6. 超时(建议 60s)或子进程提前退出 → 展示 stderr 错误页

**为什么 `--port 0`**:用户可能正手动跑着一个 `dsh web`(比如现在的 3080),动态端口从根上避免冲突;也免去"端口被占换一个"的逻辑。

### 2.2 生命周期
- **退出**:窗口关闭 → 子进程 SIGTERM → 5s 宽限 → SIGKILL。用**进程组**清理(macOS/Linux 用 `std::os::unix::process::CommandExt::process_group`,Windows 用 `CREATE_NEW_PROCESS_GROUP` + `taskkill /T`),防止 bash 工具派生的孙进程变成孤儿
- **崩溃恢复**:监听子进程退出码,非主动退出时自动重启并回到步骤 2
- **单实例**:`tauri-plugin-single-instance`,避免双开各起一个服务端
- **二次启动**:若检测到已有本客户端托管的服务端,直接复用

### 2.3 用户数据与环境
- **绝不动用户的 profile**:复用真实 `~/.dsh`(profile、插件、凭据、会话全部沿用),客户端不捆绑、不覆盖 profile
- **尊重 `DSH_HOME` 环境变量**,并在设置里提供覆盖项
- **环境透传**:子进程继承客户端环境;`.env` 分层加载逻辑(cwd 层 → DSH_HOME 层)由 dsh 自身完成,客户端只需选好 cwd;`DSH_TOOLS_MODE`、`DSH_TELEMETRY_DISABLED` 等开关透传
- **写权限**:每次启动 dsh 会重写 `profiles/web/cordis.yml` 并自愈 `profiles/node_modules` 符号链接 → 客户端进程需对 DSH_HOME 可写(见 §6 风险)

## 3. 运行时与打包(关键决策)

dsh 是 Node 程序且依赖树含原生模块,运行时来源有三种,影响体积与健壮性:

### 方案 A(推荐):捆绑 Node runtime + dsh 安装树
- 构建时 `npm install @deepseek-ai/dsh@0.1.0-rc.6` 到 `src-tauri/resources/dsh/`,Node 二进制按平台作为 resource/`externalBin` 打进安装包
- 启动:`<resources>/node <resources>/dsh/lib/bin.js web --port 0`
- 优点:完全自包含,不依赖用户机器上有 node/dsh;Finder 启动时 PATH 缺失也不怕;可锁定 dsh 版本
- 代价:体积大(342MB 依赖树 + ~60-90MB/node 平台二进制),可用 `npm i --omit=dev` 与"只保留 web profile 所需 bundle 闭包"裁剪(列为优化项,目标 ≤200MB)

### 方案 B(轻量):复用系统 node + 全局 dsh
- 启动时探测 `node`(常见路径:homebrew/nvm/fnm/系统),dsh 缺失时客户端自动执行 `npm i -g @deepseek-ai/dsh`
- 优点:体积最小;缺点:macOS 从 Finder 启动时 PATH 极简,探测逻辑易碎;版本不可控
- 适合"只给自己机器用"的快速版本

### 方案 C(探索):`bun build --compile` 把 dsh 打成单文件二进制
- 体积最优(单一可执行文件);但依赖树含 node-pty/sharp/koffi 原生模块 + 大量 `import()` 动态加载(哈希文件名),bun 兼容性需 spike 验证,失败概率存在
- 建议作为后续优化,不阻塞主线

> 建议:先按 A 做通,若接受体积再评估 C。三种方案的 Server Manager 接口一致,运行时定位做成"探测链":`bundled → 用户配置 → 系统 PATH`,未来可平滑切换。

## 4. Tauri 应用骨架(tauri v2)

```
dsh-desktop/
├─ package.json              # 前端脚本 + @tauri-apps/cli
├─ src/                      # 可选:客户端本地壳 UI(设置页/启动页),轻量 React
├─ src-tauri/
│  ├─ Cargo.toml
│  ├─ tauri.conf.json        # 窗口配置,主窗口 url 运行时动态设置
│  ├─ capabilities/          # 权限(尽量少开;shell 插件仅白名单命令)
│  ├─ resources/dsh/         # 方案 A:dsh 安装树(构建时生成)
│  ├─ resources/node/        # 方案 A:Node 二进制(构建时下载)
│  └─ src/
│     ├─ main.rs             # 生命周期编排
│     ├─ server.rs           # Server Manager:spawn/解析/健康检查/kill
│     ├─ window.rs           # WebView 导航、错误页、外链拦截
│     └─ tray.rs             # 可选:托盘
```

- **主窗口**:创建时不设 url,等 Server Manager 就绪后 `navigate(http://127.0.0.1:<port>)`;加载失败显示本地错误页
- **两个 WebView 并存**:主界面是远程 dsh UI;设置页可用第二个窗口/同窗口导航到本地壳 UI(tauri 自带前端),用于 DSH_HOME 选择、端口策略、开机自启、日志查看
- **IPC 安全**:主 WebView 不注入高权限命令(`withGlobalTauri` 关闭或最小化),避免 dsh 前端 XSS 提权到 Rust;`window.__DSH_BOOT__` 与 Tauri 注入互不干扰(已验证两者是不同全局)
- **外链**:`on_navigation` 拦截非 loopback 导航交给系统浏览器(tauri-plugin-opener)

## 5. 实施里程碑

| 阶段 | 内容 | 验收 |
|---|---|---|
| **M0 验证** | 手动跑通 `dsh web --port 0` 的 URL 行解析;Tauri 空壳加载外部 http URL | 脚本能拿到端口;窗口能显示 dsh UI |
| **M1 最小可用(macOS)** | Server Manager 全流程:spawn→解析→健康检查→导航→退出清理→崩溃重启;单实例 | 双击即用,退出无残留进程 |
| **M2 打包分发** | 方案 A 资源捆绑;dmg 打包;签名/公证(可选);裁剪体积 | 全新机器可装可跑 |
| **M3 增强** | 托盘/开机自启;设置窗口(DSH_HOME、端口策略、日志);自动升级(内置 dsh 版本随 app 升级);跨平台(Windows/Linux) | 体验完整的桌面应用 |

## 6. 风险与对策

| 风险 | 对策 |
|---|---|
| 体积 342MB+ | `--omit=dev`、按 web profile bundle 闭包裁剪;评估 bun compile(方案 C) |
| macOS App Sandbox 会挡 `~/.dsh` 写入 | 默认不启用沙箱;若要上架,加 home-dir 读写 entitlement;或改走"客户端把 DSH_HOME 迁到容器目录"的开关 |
| `profiles/node_modules` 符号链接指向 app bundle,全局 dsh 与客户端交替使用会互相改指向 | dsh 每次启动自愈,可接受;文档说明 |
| app 升级后 bundle 路径变化导致旧符号链接悬空 | 下次启动自愈,无影响 |
| 子进程残留(孙进程/SSE 连接) | 进程组 kill + 宽限升级;退出时 `closeAllConnections` 由 dsh 自身处理 |
| WebView 兼容(SSE/fetch/键盘) | macOS WKWebView 支持;M0 即验证 |
| 版本锁定 | dsh 版本随 app 发布;`package.json` 固定 `0.1.0-rc.6` |

## 7. 决策记录(已确认)

| 决策点 | 选择 | 影响 |
|---|---|---|
| 运行时方案 | **A:捆绑 Node + dsh 依赖树** | 自包含、版本锁定;接受 ~400MB 体积,后续裁剪 |
| 目标平台 | **先只做 macOS** | 架构上留好跨平台接口(进程组清理等) |
| 客户端壳范围 | **壳 + 设置窗口 + 托盘/开机自启** | v1 含原生壳 UI(设置页)、托盘菜单、LoginItems |
| 分发方式 | **个人自用,本地打包** | 不做签名公证,直接 dmg/app;省去证书流程 |

### 据此细化的 v1 范围
- **壳 UI(本地 WebView)**:设置页承载 ① DSH_HOME 路径(默认 `~/.dsh`,尊重环境变量)② 端口策略(自动 `--port 0` / 固定端口)③ 日志查看(子进程 stdout/stderr 环形缓冲)④ 开机自启开关
- **托盘**:显示服务状态(运行中/端口)、"打开主界面"、"重启服务"、"退出(停止服务)"菜单
- **主界面**:独立窗口加载 `http://127.0.0.1:<port>`;托盘"打开主界面"聚焦/重建该窗口

## 8. 实施顺序(M0 → M3,确认后从 M0 开始)
