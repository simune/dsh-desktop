# DSH × Tauri 桌面客户端 · 项目计划

> 依据:根目录 `dsh-tauri-plan.md`(方案)
> 本文档将方案转成可执行计划:WBS 任务分解、依赖关系、验收标准、排期与风险登记。
> 配套落地文档:`docs/01-architecture.md` ~ `docs/05-verification.md`。

## 1. 目标与范围

**目标**:把 `dsh web`(本地 HTTP 服务 + 浏览器 UI)包装成一个 macOS 桌面应用 —— 双击启动、自动拉起/关闭 dsh 服务端、原生 WebView 承载 UI,不修改 dsh 与前端任何代码。

**v1 范围**(继承方案 §7 决策):

| 决策点 | 选择 |
|---|---|
| 运行时方案 | A:捆绑 Node runtime + dsh 依赖树(自包含、版本锁定) |
| 目标平台 | 先只做 macOS(架构留跨平台接口) |
| 客户端壳 | 壳 + 设置窗口 + 托盘 + 开机自启 |
| 分发方式 | 个人自用,本地打包 dmg/app,不做签名公证 |

**明确不做**(v1 不投入):
- 签名 / 公证 / 上架
- Windows / Linux 适配(仅代码层面预留)
- bun `--compile` 单文件化(方案 C,列为后续优化,不阻塞主线)
- 修改 dsh 源码、修改 dsh 前端

## 2. 交付物与仓库布局

代码仓库建议直接落在本工作区:

```
dsh/
├─ dsh-tauri-plan.md          # 方案(已有)
├─ docs/                      # 计划与落地文档(本次新增)
│  ├─ 00-project-plan.md      # 本文档:计划
│  ├─ 01-architecture.md      # 架构落地设计
│  ├─ 02-server-manager.md    # Server Manager 详细设计
│  ├─ 03-runtime-bundling.md  # 运行时捆绑(方案 A)构建管线
│  ├─ 04-tauri-shell.md       # Tauri v2 壳实现规格
│  └─ 05-verification.md      # 验收与验证清单
├─ plugins/                   # 插件工作区(已有,与本项目无关)
└─ dsh-desktop/               # 客户端工程(M0 起创建)
   ├─ package.json
   ├─ src/                    # 本地壳 UI(设置页/加载页,轻量 React)
   ├─ scripts/                # 构建期脚本(vendor-node / vendor-dsh / prune)
   ├─ src-tauri/
   │  ├─ Cargo.toml
   │  ├─ tauri.conf.json
   │  ├─ capabilities/
   │  ├─ resources/           # 构建期生成:dsh 安装树 + node 二进制
   │  └─ src/
   │     ├─ main.rs           # 生命周期编排、Tauri commands
   │     ├─ server.rs         # Server Manager
   │     ├─ window.rs         # WebView 导航、错误页、外链拦截
   │     ├─ settings.rs       # 设置持久化(DSH_HOME、端口策略等)
   │     └─ tray.rs           # 托盘
   └─ dist/                   # 壳 UI 构建产物
```

## 3. 已核实的关键事实(落地依据)

以下事实已在本机 `@deepseek-ai/dsh@0.1.0-rc.6`(安装于 `/opt/homebrew/lib/node_modules/@deepseek-ai/dsh`)上验证:

| 事实 | 验证结果 |
|---|---|
| 版本 | `0.1.0-rc.6`,bin 入口 `lib/bin.js` |
| `web` 别名 | bin.js 硬编码:`web` = `--profile web`(L91-94) |
| 启动命令 | `node <dsh>/lib/bin.js web --port 0` |
| URL 输出行 | `dsh web: http://127.0.0.1:<port>`(`LOOPBACK_HOST` 恒为 `127.0.0.1`);有 LAN 候选时行尾追加 ` (LAN: http://<ip>:<port>)` |
| 端口 | 默认 3080;`--port 0` 由 OS 分配,实际端口读 `ctx.webServer.port` |
| profile 位置 | `$DSH_HOME/profiles/web`(DSH_HOME 默认 `~/.dsh`,环境变量可覆盖) |
| 前端 dist | `@deepseek-ai/dsh-web-frontend/dist`,由 `dsh-host-frontend-static` 提供 |
| 依赖树体积 | 342M,含 node-pty / sharp / koffi / landlock-run 等原生 `.node` 模块 |
| 信任边界 | loopback IP 字面量自带信任,无需 `--trusted-host` |

## 4. WBS 任务分解

### M0 验证(方案 §5 的 M0)

| ID | 任务 | 依赖 | 产出 | 验收标准 |
|---|---|---|---|---|
| M0.1 | 手动跑通 `dsh web --port 0`,确认 stdout URL 行与端口语义 | 本机已装 dsh | 实测记录 | `dsh web: http://127.0.0.1:<随机端口>` 出现在 stdout;`--port 0` 生效 |
| M0.2 | URL 解析原型:小脚本逐行读 stdout、正则提取 URL | M0.1 | `scripts/probe-dsh.mjs` 原型 | 能拿到端口并 TCP 连上;输出行带 LAN 后缀也能正确提取 |
| M0.3 | Tauri v2 空壳工程初始化;主窗口加载外部 HTTP URL | 本机 Rust 工具链 | `dsh-desktop/` 空壳 | `npm run tauri dev` 后窗口能显示 dsh UI |
| M0.4 | WebView 兼容性冒烟:SSE 流式输出、fetch、键盘输入 | M0.3 | 冒烟记录 | 对话流式渲染正常、方向键/复制粘贴正常 |

> M0 全过 → 进入 M1。M0.1/M0.2 可用半天完成,M0.3/M0.4 半天到一天。

### M1 最小可用(macOS)

| ID | 任务 | 依赖 | 产出 | 验收标准 |
|---|---|---|---|---|
| M1.1 | 工程骨架落地:tauri.conf.json 窗口配置、capabilities 最小集、main.rs 编排 | M0.3 | src-tauri 骨架 | 主窗口先显示本地加载页,server 就绪后导航 |
| M1.2 | Server Manager:spawn → stdout 解析 → TCP 健康检查 | M0.2, M1.1 | `server.rs` 核心 | 启动时序全通;60s 超时与失败路径有明确表现(错误页) |
| M1.3 | 退出清理:窗口关闭 → SIGTERM → 5s 宽限 → SIGKILL,按进程组清理 | M1.2 | 生命周期代码 | 退出后 `pgrep -f "bin.js web"` 无残留,无孤儿孙进程 |
| M1.4 | 崩溃重启:`kill -9` 子进程后自动重启(带退避),超限进错误页 | M1.2 | 重启策略 | 手动 `kill -9` 后服务自动恢复;连续崩溃 5 次后停在错误页 |
| M1.5 | 单实例:`tauri-plugin-single-instance`,二次启动聚焦已有窗口 | M1.1 | 单实例代码 | 双开时第二个实例不创建新服务端 |
| M1.6 | 加载/错误 UI:加载页、stderr 错误页(可查看日志) | M1.2 | 壳 UI 初版 | 启动中显示加载页;启动失败显示原因与日志 |

**M1 验收**:双击启动 → 窗口显示 dsh UI;退出无残留进程;`kill -9` 自动恢复;双开只一个实例。**M1 结束即有可用产品。**

### M2 打包分发

| ID | 任务 | 依赖 | 产出 | 验收标准 |
|---|---|---|---|---|
| M2.1 | 方案 A 资源捆绑脚本:`vendor-dsh.mjs`(npm i --omit=dev)+ `vendor-node.mjs`(下载平台 Node 二进制) | M1.2 | resources/ 生成物 + 脚本 | 构建期自动生成 `resources/dsh/` 与 `resources/node/`,版本锁定并记录校验和 |
| M2.2 | 启动探测链:bundled → 用户配置 → 系统 PATH | M1.2 | `resolve_runtime()` | 优先用捆绑运行时;缺失时按配置/PATH 降级,均有日志 |
| M2.3 | dmg 打包 + 全新机器验证 | M2.1, M2.2 | `target/release/bundle/dmg/*.dmg` | 在干净用户/机器上装可跑,从 Finder 启动不依赖 PATH |
| M2.4 | 体积裁剪(优化项):web profile bundle 闭包、prune 无用原生模块 | M2.1 | 裁剪脚本 | 体积 ≤ 250M(当前 342M,`--omit=dev` 先行);不破坏启动 |

### M3 增强

| ID | 任务 | 依赖 | 产出 | 验收标准 |
|---|---|---|---|---|
| M3.1 | 托盘:状态显示(运行中/端口)、打开主界面、重启服务、退出 | M1.2 | tray.rs | 托盘菜单四项功能可用,状态与真实服务同步 |
| M3.2 | 设置窗口:DSH_HOME、端口策略(自动/固定)、日志查看、开机自启开关 | M1.6 | settings.rs + 设置页 UI | 四项设置可读写并持久化,重启后生效 |
| M3.3 | 开机自启(LoginItems) | M3.2 | 自启代码 | 开关持久化,重启系统后自动启动客户端 |
| M3.4 | 升级策略:内置 dsh 版本随 app 发布;替换 app 后旧符号链接自愈验证 | M2.x | 发布检查清单 | 覆盖安装后首次启动正常(依赖 dsh 自愈) |
| M3.5 | 跨平台预留:进程组清理抽象、路径抽象;Windows/Linux 仅留 TODO 接口 | M1.x | 抽象层 | 代码中平台分支集中且可测,不实际适配 |

## 5. 依赖关系与关键路径

```
M0.1 → M0.2 ─┐
M0.3 → M0.4 ─┴→ M1.1 → M1.2 → M1.3 → M1.4 ─→ M1 验收
              │        └──→ M1.5(M1.1 后任意)
              └──────────→ M1.6(依赖 M1.2)
M1.2 ─→ M2.1 → M2.2 → M2.3 → M2.4(优化)
M1.2 ─→ M3.1 / M3.2(M3 与 M2 并行)
```

- **关键路径**:M0.3 → M1.1 → M1.2 → M1.3 → M1 验收 → M2.1 → M2.2 → M2.3
- M3 与 M2 无强依赖,可在 M2.1 完成后并行推进。
- M2.4(裁剪)与 M3.x 均为优化项,不阻塞主线。

## 6. 排期建议(个人项目,人日估算)

| 阶段 | 人日 | 建议节奏 |
|---|---|---|
| M0 验证 | 0.5 ~ 1 | 第 1 周前半 |
| M1 最小可用 | 4 ~ 6 | 第 1 周后半 ~ 第 2 周 |
| M2 打包分发 | 2 ~ 4 | 第 3 周 |
| M3 增强 | 4 ~ 8 | 第 4 ~ 5 周(与 M2.4 并行) |
| 合计 | 11 ~ 19 人日 | 兼职约 4 ~ 6 周 |

> 估算假设:熟悉 Rust + Tauri;不熟悉则 M1.1/M1.3 各 +1 天。每阶段完成即按 `docs/05-verification.md` 过验收。

## 7. 风险登记(方案 §6 细化)

| # | 风险 | 触发信号 | 缓解动作 | 状态 |
|---|---|---|---|---|
| R1 | 体积 342M+ | M2.1 后 du 结果 | `--omit=dev` 先行;bundle 闭包裁剪(M2.4);评估 bun compile(方案 C) | 开放 |
| R2 | macOS Sandbox 挡 `~/.dsh` 写入 | 沙箱开启时启动即失败 | 默认不开沙箱;若上架再加 home-dir entitlement | 开放(决策:不开) |
| R3 | `profiles/node_modules` 符号链接指向 app bundle,升级/交替使用悬空 | 覆盖安装后首次启动异常 | 依赖 dsh 每次启动自愈;文档说明;M3.4 验证 | 开放 |
| R4 | 子进程残留(孙进程/SSE) | 退出后 `pgrep` 有残留 | 进程组 kill + 宽限升级(M1.3);SSE 关闭由 dsh 自身处理 | 开放 |
| R5 | 启动超时/端口异常 | 60s 未就绪 | 错误页展示 stderr;M0.2 原型先行验证解析 | 开放 |
| R6 | WebView 兼容问题(SSE/键盘) | M0.4 冒烟失败 | WKWebView 支持 SSE/fetch;键盘问题记录并绕过 | 开放(预计低) |
| R7 | dsh 版本升级破坏兼容 | 升级 dsh 后启动异常 | 版本锁定 `0.1.0-rc.6`;升级随 app 发布、先验证再发 | 开放 |
| R8 | 双开各起服务端 | 连点两次图标 | 单实例(M1.5);二次启动复用 | 开放 |
| R9 | Finder 启动 PATH 缺失导致 runtime 探测失败 | M2.3 全新机器验证 | 捆绑运行时优先(M2.2 探测链) | 开放 |

## 8. Definition of Done

一个任务"完成"须满足:

1. 实现符合对应落地文档(`docs/01` ~ `docs/04`)的接口与行为约定;
2. 通过 `docs/05-verification.md` 中该任务的验收步骤;
3. 关键路径上有实测记录(命令 + 输出),非"目测可用";
4. 不破坏已有验收项(回归:退出无残留、单实例、重启恢复);
5. 涉及的行为决策(偏离本计划之处)回写本计划或方案文档。

## 9. 验收流程(每阶段出口)

| 阶段出口 | 必过项(详见 05-verification.md) |
|---|---|
| M0 | 脚本能拿到端口;空壳窗口显示 dsh UI;SSE 冒烟通过 |
| M1 | 双击即用;退出零残留;崩溃自愈;单实例 |
| M2 | 全新环境安装可跑;Finder 启动无 PATH 依赖;体积记录 |
| M3 | 托盘/设置/自启/日志全可用;覆盖安装正常 |
