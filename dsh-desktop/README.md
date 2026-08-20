# DSH Desktop（DSH × Tauri 桌面客户端）

## 关于
DeepSeek Harness 的桌面客户端（dsh-desktop），将 dsh Web 服务捆绑到原生桌面壳中：启动时自动拉起内置的 node + dsh 并用 Tauri WebView 承载 SPA，简化桌面部署与使用体验；适配 Windows 打包（NSIS/MSI），便于发布本地安装包。

DeepSeek Harness 的桌面客户端：启动时自动拉起内置的 `dsh web` 服务，并用原生 WebView 窗口承载 UI（替代手动跑 `dsh web` + 浏览器标签页）。

- 服务端：客户端启动时 spawn bundled 的 node + dsh（`--port 0` 动态端口），退出时按进程组清理
- UI：Tauri v2 WebView 窗口，直接加载 `http://127.0.0.1:<port>`（dsh 的 SPA）
- 运行时探测链：`bundled(resources) → DSH_DESKTOP_NODE/DSH_DESKTOP_DSH 环境变量 → 系统 PATH`
- 配套设计文档：[dsh-tauri-plan.md](../dsh-tauri-plan.md)、[docs/](../docs/)

## 环境要求（Windows）

| 依赖 | 说明 |
| --- | --- |
| Rust 工具链 | `rustup` + stable（MSVC 目标 `x86_64-pc-windows-msvc`） |
| Visual Studio | VS2022+（含"使用 C++ 的桌面开发"工作负载，提供 `link.exe`） |
| Windows SDK | 随 VS 安装（提供 `kernel32.lib`、`ucrt` 等，缺失会导致 LNK1181） |
| Node ≥ 20 | 开发/构建用（运行时走 bundled，见下） |

> 首次构建 `tauri build` 会从 GitHub 下载 WiX / NSIS 工具链（缓存于 `%LOCALAPPDATA%\tauri`）；网络受限时需手动放置，参见 `docs/05-verification.md` 常见问题。

## 构建

```powershell
cd dsh-desktop
npm install                      # 首次：安装前端依赖
npm run vendor                   # 生成 src-tauri/resources/node/win32-x64/node.exe + resources/dsh/（dsh 依赖树）
npm run build:app                # 前端构建 + tauri build（Windows 默认只打 NSIS，见下）
```

产物位于 `src-tauri/target/release/bundle/`。

### MSI 与 NSIS：只打一种即可（推荐 NSIS）

同一份产物两种格式，**功能等价，只需生成其中一种**。实测打包耗时（本机，仅 bundler 环节、工具链已缓存）：

| 格式 | 命令 | 打包耗时 | 产物大小 | 说明 |
| --- | --- | --- | --- | --- |
| **NSIS**（推荐） | `npm run build:app:nsis` | **≈ 4 分钟** | **52 MB** | 简体中文安装界面，WebView2 引导安装，安装到当前用户 |
| MSI | `npm run build:app:msi` | ≈ 17 分钟 | 103 MB | 仅 en-US，WiX light 压缩很慢 |

- **Windows 上默认只生成 NSIS**：`src-tauri/tauri.windows.conf.json` 已把 `bundle.targets` 固定为 `["nsis"]`（macOS/Linux 不受影响，仍按各自平台默认打包）
- 如需 MSI：`npm run build:app:msi` 或 `npx tauri build --bundles msi`
- 手工指定格式可随时切换，无需改配置

## 常用命令

| 命令 | 作用 |
| --- | --- |
| `npm run dev` | 前端 Vite 开发服务器（壳 UI） |
| `npm run dev:app` | `tauri dev`：窗口 + 热更新（本地壳 UI） |
| `npm run build` | 仅前端构建（tsc + vite → `dist/`） |
| `npm run vendor` | 生成 bundled 运行时（node + dsh 依赖树，含原生模块） |
| `npm run build:app:nsis` | 完整构建并只打 NSIS 安装包（推荐） |
| `npm run build:app:msi` | 完整构建并只打 MSI 安装包 |
| `npm run build:app` | 完整构建（Windows 下等价于 `build:app:nsis`） |

## 运行时验证（bundled 全链路）

启动 `src-tauri/target/release/dsh-desktop.exe` 后，日志位于 `%APPDATA%\dev.dsh.desktop\server.log`，出现以下序列即正常：

```
[app] runtime: bundled
[out] dsh web: http://127.0.0.1:<port>
[app] url 行解析成功:http://127.0.0.1:<port>
[app] running: http://127.0.0.1:<port>
```

> Windows 注意事项：Rust `current_exe()` 可能返回 `\\?\` 长路径前缀，node 无法处理该前缀（会把 argv 路径误解析为盘符导致 EISDIR）。客户端已在 `lib.rs` 用[...]
