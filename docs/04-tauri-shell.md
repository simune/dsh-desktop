# Tauri v2 壳实现规格

> 上游:`dsh-tauri-plan.md` §4(Tauri 骨架);`docs/01-architecture.md` §3~§8。
> 本文件定义 `src-tauri/` 的工程配置、窗口管理、IPC 权限、托盘、设置页与安全细节,对应计划 M1.1/M1.6/M3.1~M3.3。

## 1. 工程骨架

### 1.1 根 `package.json`(节选)

```jsonc
{
  "name": "dsh-desktop",
  "private": true,
  "scripts": {
    "dev:ui": "vite",                    // 壳 UI(加载页/设置页)热更
    "tauri": "tauri",
    "dev": "tauri dev",
    "build:ui": "vite build",
    "build": "npm run vendor && npm run build:ui && tauri build",
    "vendor": "node scripts/vendor-node.mjs && node scripts/vendor-dsh.mjs"
  },
  "dependencies": { "@tauri-apps/api": "^2", "react": "^18", "react-dom": "^18" },
  "devDependencies": { "@tauri-apps/cli": "^2", "vite": "^5", "typescript": "^5" }
}
```

> 壳 UI 用 Vite + React;`vite.config.ts` 设 `base: './'`(打包后以相对路径加载,便于嵌入)。

### 1.2 `src-tauri/Cargo.toml`(节选)

```toml
[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-single-instance = "2"
tauri-plugin-opener = "2"
tauri-plugin-autostart = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
regex = "1"
dirs = "5"

[build-dependencies]
tauri-build = { version = "2", features = [] }
```

### 1.3 `src-tauri/tauri.conf.json`(节选)

```jsonc
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "DSH Desktop",
  "identifier": "dev.dsh.desktop",
  "build": {
    "beforeDevCommand": "npm run dev:ui",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "npm run build:ui",
    "frontendDist": "../dist"
  },
  "app": {
    "withGlobalTauri": false,                 // 安全:主窗口(远程内容)不注入全局 API
    "windows": [
      {
        "label": "main",
        "title": "DSH Desktop",
        "width": 1280,
        "height": 800,
        "url": "index.html",                  // 初始为本地加载页,就绪后 navigate
        "visible": true
      }
    ],
    "security": { "csp": null }               // 远程 dsh UI 自带 CSP;本地壳页面按需收紧
  },
  "bundle": {
    "resources": ["resources/**/*"],
    "macOS": { "minimumSystemVersion": "11.0" },
    "targets": ["dmg", "app"]
  },
  "plugins": { }
}
```

## 2. 窗口管理(`window.rs`)

| 窗口 | label | 内容 | IPC |
|---|---|---|---|
| 主窗口 | `main` | 加载页(本地)→ dsh UI(远程) → 错误页(本地) | 无(远程);本地页阶段有 |
| 设置窗口 | `settings` | 本地设置页 | 有(受限) |

- **导航切换**:`server-status` 事件驱动 —— `Starting` → 加载页;`Running{url}` → `webview.navigate(url)`;`Error` → 错误页(带错误码)。
- 重复导航保护:URL 相同不重复 navigate;navigate 失败 → `E_NAV_FAIL` 错误页。
- 设置窗口:懒创建(`WebviewWindowBuilder`),关闭即销毁,再次打开重建(设置值从持久化读,无状态丢失)。
- 主窗口关闭策略:关闭 = 退出整个 app(用户直觉:关窗口即停服务)。托盘存在时,关窗口 = 隐藏 + 服务继续?——**v1 决策:关主窗口即退出并停服务**,托盘"打开主界面"用于最小化/隐藏后找回;此决策可在设置页加"关闭时最小化到托盘"开关(列为 M3.2 增强项)。

## 3. IPC 与 capabilities(安全核心)

### 3.1 原则

1. **远程内容零权限**:主窗口加载的是 `http://127.0.0.1:<port>`(远程来源),capabilities 中**不包含**任何对该来源的授权 → 即使 dsh 前端被 XSS,也无法调用 Tauri command。
2. `withGlobalTauri: false`:不注入 `window.__TAURI__`,进一步压缩攻击面(`window.__DSH_BOOT__` 是 dsh 自身全局,与 Tauri 互不干扰,已由方案验证)。
3. **最小命令面**:Rust 侧只注册壳所需命令(见 `docs/01` §5.2),不注册 shell/fs 等通用插件命令。

### 3.2 `capabilities/shell.json`(示意)

```jsonc
{
  "identifier": "shell-local",
  "description": "仅本地壳窗口可用的最小权限",
  "windows": ["settings"],                 // 只挂到本地窗口;main 不在列表
  "permissions": [
    "core:default",
    "core:event:default",                  // 事件监听(server-status 等)
    "autostart:allow-enable",
    "autostart:allow-disable",
    "autostart:allow-is-enabled",
    "opener:default"
  ]
}
```

> 本地加载页/错误页运行于 `tauri://localhost` 或 `http://localhost:1420`(dev),属本地来源,可在 capabilities 中另设 `"windows": ["main"]` + `"local": true` 的窄权限集(仅 `core:event:listen`),远程阶段不生效。实现时区分"主窗口的本地页面阶段"与"远程阶段",用 `on_navigation` + 运行时守卫双保险:command 内部校验来源 URL 非 loopback 即拒绝。

### 3.3 命令注册

```rust
tauri::Builder::default()
    .invoke_handler(tauri::generate_handler![
        get_server_status, get_logs, restart_server,
        get_settings, set_setting, toggle_autostart,
        open_settings, show_main, quit_app
    ])
```

## 4. 托盘(`tray.rs`)

| 菜单项 | 动作 |
|---|---|
| 状态(禁用项) | `DSH ● running :53001` / `○ stopped` / `! error`(随 `server-status` 刷新) |
| 打开主界面 | 显示/聚焦主窗口;若已销毁则重建 |
| 重启服务 | `ServerManager::restart()` |
| 设置… | 打开设置窗口 |
| 退出 | 完整退出序列(停服务 → 退 app) |

- 图标:菜单栏模板图标(`Template`),运行态加小圆点或角标(可选)。
- 托盘在 app 启动即创建(即使主窗口关闭,托盘仍在,服务可控)。

## 5. 设置页(壳 UI + `settings.rs`)

| 设置项 | 控件 | 持久化 key | 生效时机 |
|---|---|---|---|
| DSH_HOME | 路径选择 + 手动输入 | `dsh_home` | 下次启动服务;空 = 跟随环境变量/默认 |
| 端口策略 | 单选:自动(推荐)/ 固定 | `port_policy` | 下次启动服务 |
| 日志 | 环形缓冲行数 + 实时日志视图 | `log_lines` | 立即 |
| 开机自启 | 开关 | `autostart` | 立即(LoginItems) |

- `settings.rs`:JSON 存于 `app.path().app_config_dir()/settings.json`(macOS 为 `~/Library/Application Support/dev.dsh.desktop/`);读写加锁,写后原子替换。
- 日志视图:订阅 `server-log` 增量事件,或点"刷新"拉 `get_logs`。
- 保存按钮 + "恢复默认";DSH_HOME 修改提示"重启服务后生效"。

## 6. 外链拦截与 opener

```rust
.on_navigation(|window, url| {
    if window.label() == "main" && is_remote_dsh_url(url) { true }       // loopback 放行
    else if is_local_shell(url) { true }                                  // 加载页/错误页/设置页
    else { let _ = opener::open_url(url.as_str()); false }                // 其余交系统浏览器
})
```

- `is_remote_dsh_url`:host ∈ {`127.0.0.1`, `localhost`} 且端口为当前服务端口(动态端口下必须比对实际端口,防任意 loopback 端口导航)。
- `tauri-plugin-opener` 注册但只在拦截分支使用。

## 7. 单实例(`main.rs`)

```rust
.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
    // 二次启动:聚焦主窗口 + 通知托盘
    if let Some(w) = app.get_webview_window("main") { let _ = w.show(); let _ = w.set_focus(); }
}))
```

## 8. 开机自启

- `tauri-plugin-autostart`:`autostart::Manager::enable()/disable()`,macOS 落 LoginItems。
- 启动时按持久化设置同步一次 `is_enabled()` 与配置值(防用户在系统设置里手动改)。

## 9. 构建与运行命令

```bash
npm install                      # 前端依赖
cargo install tauri-cli --version ^2   # 或经 npm @tauri-apps/cli

npm run dev                      # 开发:壳 UI + tauri dev(未 vendor 时走 PATH 探测链)
npm run vendor                   # 生成 resources/(联网,需 node/dsh 源)
npm run build                    # vendor + 壳 UI + tauri build → dmg/app
```

- 开发期不强制 vendor(探测链自动落到系统 dsh),提交前跑一次 `npm run build` 全链路。

## 10. 与计划/验证的对应

| 计划任务 | 本文件章节 |
|---|---|
| M1.1 工程骨架 | §1~§2 |
| M1.6 加载/错误 UI | §2, §5 |
| M1.5 单实例 | §7 |
| M3.1 托盘 | §4 |
| M3.2 设置窗口 | §5 |
| M3.3 开机自启 | §8 |
| IPC 安全 / 外链 | §3, §6(对应 `docs/01` §7~§8) |
