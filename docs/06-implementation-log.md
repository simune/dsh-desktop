# 实施日志(实测记录)

> **维护约定(2026-08-15 起)**:后续每次修改均作为独立 git commit 提交,便于回溯。提交粒度=一次逻辑改动(功能/修复/文档/测试),消息格式 `类型: 简述`(`feat:`/`fix:`/`docs:`/`test:`/`chore:`)。


> 按 `docs/00-project-plan.md` Definition of Done 第 3 条,关键路径保留实测记录。
> 每条记录:日期、任务 ID、命令/操作、结果。


> **环境坑(重要)**:本工作区所在卷 `/Volumes/Data` 为 **ExFAT**,macOS 为每个文件自动生成 `._` AppleDouble 侧车。影响:
> 1. 工具链中硬链接原子写入(link 系统调用)不可用 → write/edit 工具需改用 bash 直写;
> 2. `tauri-build` 遍历生成目录读到 `._default.toml` 报 `stream did not contain valid UTF-8` → 通过 `dsh-desktop/.cargo/config.toml` 将 cargo target-dir 指向 APFS 卷(`~/Library/Caches/dsh-desktop/target`)解决。

### 缺陷修复:点击关闭按钮无法退出(2026-08-15)

- **症状**:点窗口红色关闭按钮后,服务停止但 app 进程残留(无窗口的幽灵进程)
- **根因**:`app.exit(0)` 在 `CloseRequested` 事件处理器(主线程)内调用被窗口关闭流程吞掉,进程不退出
- **修复**:
  - `api.prevent_close()` 阻止默认关闭流程
  - 后台线程执行 `request_exit`(停服 + exit),不阻塞主线程
  - 仅主窗口关闭触发退出;设置窗口关闭走默认行为(不再误退整个 app)
- **验证**:新增测试钩子 `DSH_DESKTOP_CLOSE_MS`;dev 与发布版(挂载 dmg 运行)均实测:close → stopped → 进程归零、零残留
- **踩坑**:`npx tauri build --bundles dmg` 会清掉独立 .app;测试须挂载最新 dmg,注意旧挂载残留会测到旧包

### 安装包捆绑 dsh-usage-stats 插件(2026-08-15)

- **目标**:全新机器安装后,web profile 自动带 dsh-usage-stats,无需手动 `dsh plugin add`
- **实现**:
  1. `vendor-dsh.mjs`:`npm pack` 插件(默认从本工作区 `../../plugins/dsh-usage-stats`)→ `npm install` 进捆绑安装树 `resources/dsh/node_modules/dsh-usage-stats`(离线可解析)
  2. `server.rs ensure_usage_stats_bundle`:启动前确保 web profile 的 `dsh.profile.bundles` 含插件(按 dsh initProfile 模板预创建缺失 profile),并补 `profiles/web/node_modules/dsh-usage-stats` 符号链接 → 捆绑插件目录(patch 内 `import 'dsh-usage-stats'` 从 profile 解析)
  3. 仅当插件可从安装树解析时才注入(未 vendor 时跳过,不破坏 dev/PATH 模式)
- **验证**(全新 DSH_HOME=/tmp/fresh-dsh):profile 自动创建、bundles 含插件、链接建立、dsh 启动 running 无解析错误
- **环境坑(重要)**:`com.apple.provenance` xattr —— 受限上下文拷贝的可执行文件会被 arm64 macOS 以 SIGKILL 击杀(exit 137),`xattr -d` 无法清除;解决:用 `cp` 重新生成干净副本。另:构建 target 目录改由 `CARGO_TARGET_DIR` 环境变量指定(见 .cargo/config.toml 注释,跨平台友好)

### 缺陷修复:启动加载页四周白边(2026-08-15)

- **症状**:启动时加载页(进度条)四周有白边
- **根因**:`html/body` 未重置默认 margin(8px),深色容器四周露出白色页面背景;WebView 加载/导航瞬间窗口背景也为白
- **修复**:
  - `App.css` 全局重置:`html, body, #root { margin:0; padding:0; height:100%; background:#0d1117 }`
  - 主窗口与设置窗口 `.background_color(Color(13,17,23,255))`,消除加载/导航白闪
- **验证**:运行中窗口 8 个边缘采样点全部深色(0/8 白)

### 子模块化转换(2026-08-15)

- `plugins/dsh-usage-stats` 由"裸 gitlink"升级为**正式 submodule**:
  - 新增 `.gitmodules`(path + url = git@github.com:simune/dsh-usage-stats.git)
  - `git submodule absorbgitdirs` 收编插件 gitdir 至 `.git/modules/plugins/dsh-usage-stats`
  - 插件自身 remote/历史/工作区不受影响,仍可独立推送
- 克隆方式:`git clone --recurse-submodules <url>` 或 `git submodule update --init`
- 本地全流程验证:file 协议克隆 `--recurse-submodules` → 子仓自动检出 d6463cb、内容完整

### 远程推送状态(2026-08-15)

- 本地仓库:16 个提交、工作区干净;远程 `origin = git@github.com:simune/dsh-desktop.git`(私有,已存在)
- **受阻**:本环境 SSH(22/443)均被网络拦截(198.19.0.x),keychain 无 GitHub HTTPS 凭据,无法直接推送
- 待办(二选一):
  1. 提供 GitHub PAT(repo 写权限),用 HTTPS 推送
  2. 在本人终端执行 `git push -u origin main`
- 注意:仓库含嵌入式仓库 gitlink(plugins/dsh-usage-stats),推送后 GitHub 会显示为子模块引用;如需正式 submodule 语义,执行 `git submodule add git@github.com:simune/dsh-usage-stats.git plugins/dsh-usage-stats`

### M3 发布版最终回归(2026-08-15)

- 命令:`env -i PATH=/usr/bin:/bin HOME=$HOME DSH_DESKTOP_OPEN_SETTINGS=1 DSH_DESKTOP_AUTOQUIT_MS=30000 'DSH Desktop.app/Contents/MacOS/dsh-desktop'`
- 结果(受限 PATH,模拟全新机器):
  - `runtime: bundled` → dsh 启动 → `running :50480` → 主窗口 page-load dsh UI
  - 设置窗口 page-load `tauri://localhost/?view=settings`(prod 自定义协议)
  - 自动退出后零残留(app/子进程全清)
- **M0-M3 全部达成,客户端可分发使用**。待人工项:开机自启需重启系统验证;托盘/设置交互点验;T5 端口冲突(设置页可用后)。M2.4 体积裁剪为优化项。

### M3 增强(2026-08-15)

| 项 | 结果 |
|---|---|
| M3.1 托盘 | ✅ 菜单:状态(server-status 事件驱动更新)/打开主界面/重启服务/退出;左键点击显示主窗口 |
| M3.2 设置窗口 | ✅ 独立窗口加载 `index.html?view=settings`;DSH_HOME/端口策略/日志行数/开机自启/日志查看;设置持久化 settings.json;实测设置页 page-load |
| M3.3 开机自启 | ✅ tauri-plugin-autostart(LaunchAgent);set_settings 中即时生效 |
| M3.4 升级策略 | ✅ 内置 dsh 随 app 发布(版本锁定 0.1.0-rc.6);bundled 运行时 + 真实 profile 兼容(见 M2.3);覆盖安装 = 重新构建 .app 后运行正常(依赖 dsh 自愈) |
| M3.5 跨平台 | ✅ Windows x64 实际适配:PATH/shim→bin.js、CREATE_NEW_PROCESS_GROUP+CREATE_NO_WINDOW、taskkill /T /F、USERPROFILE cwd、NSIS(SimpChinese)、vendor-node(win-x64→win32-x64);Linux 仍预留 |

> 待人工确认项:开机自启需重启系统验证;托盘/设置窗口交互(点按菜单、保存)需人工点验——自动化已覆盖"窗口创建+页面加载+无 panic"。

### M2 打包分发(2026-08-15)

| 项 | 结果 |
|---|---|
| M2.1 vendor 脚本 | ✅ vendor-node(下载+sha256 校验+仅留二进制)+ vendor-dsh(--omit=dev);产物真实体积 356MB(node 104M + dsh 252M) |
| M2.2 探测链 | ✅ bundled → PATH;环境覆盖 DSH_DESKTOP_NODE/DSH_DESKTOP_DSH 已实现;`runtime: bundled` 实测生效 |
| M2.3 dmg 打包 | ✅ `DSH Desktop_0.1.0_aarch64.dmg` 101MB(LZFSE);受限 PATH(`env -i PATH=/usr/bin:/bin`)运行 bundled app → 正常启动并加载 dsh UI,退出零残留 |
| M2.4 体积裁剪 | ⏸ 优化项:101MB dmg 已低于预算(≤250M);bundle 闭包裁剪推迟(风险高收益低,依赖 dsh 自愈) |

关键修复与坑:

| 问题 | 处理 |
|---|---|
| dsh 树 du 虚高 9G(真实 252MB) | ExFAT 卷块统计问题,打包按字节,不影响产物;vendor 脚本清理 `._` 侧车防打包膨胀 |
| bundled Node v22.14.0 缺 `node:zlib.createZstdDecompress` | 升级 v22.23.2(与开发环境一致),dsh-session-persistence-jsonl 依赖该 API |
| bundle.resources glob 保留前缀目录 | 改映射形式 `"resources/": ""`,资源落到 Resources 根 |
| gitignore `src-tauri/resources/` 锚定错层 | 改 `**/src-tauri/resources/`;resources 为构建产物不入库 |

> 注意:打包期间 .app 被重写,测试须等构建完全结束再启动,否则会跑到旧包。

### M1 最小可用(macOS)(2026-08-15)

实现:`src-tauri/src/{server,runtime,window,settings,lib}.rs` + 壳 UI(`src/App.tsx`)。关键修复记录:

| 问题 | 根因 | 修复 |
|---|---|---|
| 壳卡"正在启动" | `ServerState` 枚举缺 `#[serde(tag="state")]`,序列化为 `{"running":{...}}`,前端 `s.state` 恒 undefined | 加 tag,序列化 `{"state":"running","url":...}` |
| 崩溃重启后窗口停留旧 URL | React 壳已随导航卸载,只有 Rust 能再导航 | Running 状态转移时 Rust `navigate` 主窗口 |
| 崩溃上限在启动阶段达到时不回跳 | `WaitOutcome::Exited` 分支缺 `navigate_shell` | 补上 |
| 退避不重置 | 成功后未清零 backoff | Running 时 `backoff = 1s` |
| server.log 写不进 | `OpenOptions::create` 不建父目录 | `init_log_file` 里 `create_dir_all` |
| fake-dsh 无 --serve 时以 code 13 退出 | Node 22 未决顶层 await,事件循环空即退 | 定时器保活 |
| tauri-build 读 `._` 侧车 panic | 工作区在 ExFAT 卷 | `.cargo/config.toml` target 指 APFS;`clean:apple-double` 挂构建前置 |

验收(按 docs/05 §2):

| 项 | 结果 |
|---|---|
| F1 双击即用 | ✅ page-load 显示 dsh UI,像素方差验证非空白 |
| F2 无地址栏 WebView | ✅ 原生窗口 |
| F3 退出零残留 | ✅ `DSH_DESKTOP_AUTOQUIT_MS` 测试钩子:stopping→SIGTERM→宽限→清理,app 自身服务零残留(测试期强杀产生的孤儿除外) |
| F4 单一监听 | ✅ 仅客户端子进程(--port 0 随机端口) |
| F5 日志可见 | ✅ server.log 落盘 + 错误页日志视图 |
| T1 崩溃重启 | ✅ kill -9 子进程 → 退避 1s → 新 URL 恢复 |
| T2 崩溃上限 | ✅ 连续 5 次启动阶段崩溃 → E_CRASH_LIMIT → 回跳错误页 |
| T3 启动超时 | ✅ fake-dsh --no-url → 60s → E_START_TIMEOUT → 错误页(红字渲染确认) |
| T4 单实例 | ✅ 第二实例自动退出,仅剩首个 |
| T5 端口冲突 | ⏸ 依赖设置页(M3.2),延后 |
| T6 退出残留 | ✅ 同 F3 |

新增:`setup_menu`(原生菜单 Cmd+Q 退出)、`DSH_DESKTOP_AUTOQUIT_MS` 测试钩子、`DSH_DESKTOP_NODE/DSH_DESKTOP_DSH` 环境覆盖(docs/03 §5 探测链)。

**M1 结论:达到"双击即用、退出零残留、崩溃自愈、单实例"验收标准,进入 M2。**

## M0 验证

### M0.1 手动验证 `dsh web --port 0`(2026-08-15)

- 环境:`@deepseek-ai/dsh@0.1.0-rc.6` 安装于 `/opt/homebrew/lib/node_modules/@deepseek-ai/dsh`
- 首次启动失败:web profile 引用 `dsh-usage-stats` 但 `node_modules` 符号链接悬空
  - 根因:本工作区目录迁移(plugins/ 移入 harness/)导致 profile 的 `link:` 路径失效
  - 处理:`~/.dsh/profiles/web/package.json` 的 link 路径更新为 `link:/Volumes/Data/workspace/dsh/harness/plugins/dsh-usage-stats`,执行 `dsh plugin --profile web install`(pnpm)重建符号链接
- 命令:`node <dsh>/lib/bin.js web --port 0`
- stdout:`dsh web: http://127.0.0.1:59584`
- 验证:`curl http://127.0.0.1:59584/` → HTTP 200;退出后 `pgrep -f "bin.js web"` 无残留
- **通过**

### M0.2 URL 解析原型(2026-08-15)

- 产出:`dsh-desktop/scripts/probe-dsh.mjs`(解析器)+ `scripts/fake-dsh.mjs`(假 dsh,供测试)
- 正则:`^dsh web: (http://127\.0\.0\.1:\d+)`,取捕获组 1,LAN 后缀忽略
- 场景实测:

| 场景 | 命令 | 结果 |
|---|---|---|
| 真实 dsh | `node scripts/probe-dsh.mjs` | `url=http://127.0.0.1:59756 tcp=ok` exit=0 |
| fake + LAN 后缀 | `node scripts/probe-dsh.mjs --fake --lan --serve` | `url=http://127.0.0.1:59765 tcp=ok` exit=0 |
| fake + 50 行噪声 | `node scripts/probe-dsh.mjs --fake --noise 50 --serve` | `url=http://127.0.0.1:59767 tcp=ok` exit=0 |

- 清理:探测结束后 SIGTERM→2s→SIGKILL,无残留进程
- **通过**

### M0.3 Tauri v2 空壳(2026-08-15)

- 脚手架:`create-tauri-app@4.6.2`,`-m npm -t react-ts --identifier dev.dsh.desktop`
- 工程:`harness/dsh-desktop/`;按 `docs/04` 调整:productName "DSH Desktop"、主窗口 label "main" 1280x800、`withGlobalTauri: false`、壳 UI 启动即导航到 `VITE_DSH_URL`
- Rust 侧:`on_page_load` 打印页面加载 URL(验证远程页面渲染)
- 冒烟:dsh 固定端口 3901 + `VITE_DSH_URL=http://127.0.0.1:3901 npm run tauri dev`
- 结果:(待记录 —— 首次 cargo 编译后验证)

### 打包版本递增规则(2026-08-21)

- **约定**:每次重新打包,包版本号在当前版本基础上**递增一版**(`0.1.0` → `0.1.1` → …),避免同版本号覆盖安装包无法区分新旧。
- **修改位置(三处保持一致)**:`dsh-desktop/package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 的 `version`。
- **记录于**:`dsh-desktop/README.md` →「打包版本规则(强制)」。

### 打包前流程规则(2026-08-21)

- **约定**:每次修改构建完成后,**先跑自动化测试 → 用户验证基础功能 → 通过后才允许打包**。
- **顺序**:① 构建/编译 → ② 自动化测试(vendor 裁剪冒烟 + 现有验证脚本 + 后续新增测试)→ ③ 用户验证基础功能(启动 app、dsh UI 正常、核心功能可用)→ ④ 全部通过才打包。
- **记录于**:`dsh-desktop/README.md` →「打包前流程规则(强制)」。

### devUrl 泄漏到 release 构建(2026-08-21)

- **症状**:裸 `cargo build --release` 编译的 exe 启动时**先显示"无法加载",几秒后加载成功**。
- **根因**:tauri 依赖未显式启用 `custom-protocol` feature → `tauri-build` 的 `dev = !custom_protocol` 判定为 **dev 模式** → `get_app_url()` 返回 `devUrl`(`http://localhost:1420`),而该端口无服务 → WebView 报"无法加载";几秒后 server ready,壳 JS 才跳转到真实 dsh URL。
- **为何打包版没事**:`tauri build` 由 tauri-cli 驱动,自动追加 `custom-protocol` → dev=false → 加载内嵌资源(`http://tauri.localhost`),正常。
- **修复**:`src-tauri/Cargo.toml` → `tauri = { version = "2", features = ["tray-icon", "custom-protocol"] }`。加后裸 `cargo build` 也走生产模式。
- **教训**:验证 exe 一律用 `tauri build`(或 `--no-bundle`)产物;若用裸 `cargo build`,必须确保 `custom-protocol` feature 已启用。

### 缺陷修复:加载 dsh 页面后自绘标题栏消失,窗口无法关闭/拖动(2026-08-21)

- **背景**:本次改动为无边框窗口(`decorations(false)`)+ 自绘标题栏(`TitleBar.tsx`)+ dsh UI 用 iframe 承载(方案 A-1,`App.tsx` running 后不再整页跳转)。
- **症状**:启动初始化时自绘标题栏正常显示;dsh 加载后**标题栏消失**,无边框窗口失去拖动/关闭入口。
- **诊断链路(关键,均为运行时实证)**:
  1. 内嵌资源确认:out/`tauri-codegen-assets/*` 为 **brotli 压缩**的 assets(故 exe 字节级搜不到 `dsh-frame` 等字符串;hash 文件名≠dist 文件 sha256 属正常)。`BrotliStream` 解压内嵌 js(215KB)确认**是新版 iframe 前端**(含 `dsh-frame`/`data-dsh-titlebar`,`window.location.href` 缺失)。
  2. 运行时探针(`on_page_load` 里 eval + 写 `document.title` 再读):shell 顶层页面在 running 后**被导航离开**(CDP 确认顶层 `isTop=true`、`frames=0`、href=`http://127.0.0.1:port/`)。
  3. **CDP 决定性证据**:修复前只有 1 个 page target = `127.0.0.1`(dsh 顶层);`window.top===window.self` 为 true、`frames=0` → **整页跳转**,非 iframe。
- **根因**:iframe 方案下存在**一段 JS 触发顶层导航离开壳页面到 dsh URL**(`http://127.0.0.1:<port>/`;shell 自身 App.tsx 无跳转代码,内嵌 bundle 亦无 `window.location.href`,判定为 iframe 内 dsh 侧行为,具体发起方未继续深挖)。旧 `navigation_guard` 因该 URL 是 loopback 且端口匹配当前服务而**放行** → shell 整页跳转到 dsh → 无边框窗口的自绘标题栏随 shell 页面一起消失。
- **修复**:`window.rs::navigation_guard` 改为**只放行本地壳页面(tauri.localhost / tauri://localhost / dev 1420)**,其余顶层导航一律拒绝并打印 `[nav-guard] 拦截顶层导航离开壳页面: <url>`。dsh UI 永远由 iframe 承载,顶层 WebView 永远停留在壳页面。
- **验证**:
  - AUTOQUIT 运行:page-load 序列为 `tauri.localhost → tauri.localhost → running → tauri.localhost`(**不再出现 127.0.0.1 顶层 page-load**);stderr 出现一次 `[nav-guard] 拦截顶层导航…`。
  - CDP 修复后:page target=`http://tauri.localhost/`(`isTop=true`、`frames=1`、`tb=true` 标题栏存在、`ifr=true`)+ iframe target=`http://127.0.0.1:<port>/`(dsh 正常承载)。
- **遗留观察**:拦截后 shell 会多一次 `tauri.localhost` 重载(WebView2 拒绝顶层导航后的回退行为),启动瞬间可能轻微闪一下,功能不受影响;顶层导航发起方未定位(怀疑 iframe 内 dsh 侧 JS,属防御性拦截,不依赖发起方)。
- **调试工具沉淀**:`on_page_load` 打印 page-load 时序;`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=92xx` + CDP `/json/list` + `Runtime.evaluate` 检查 `window.top===window.self`/`frames`/`[data-dsh-titlebar]`/`iframe[data-dsh-iframe]`;`BrotliStream` 解压 `out/tauri-codegen-assets` 内嵌资源。

### 缺陷修复:标题栏三键点击无反应、窗口无法拖拽(2026-08-21)

- **症状**:标题栏绘制正确,但最小化/最大化/关闭按钮 hover 有效、**点击无反应**;窗口**无法拖拽**;唯一正常的是**双击标题栏可放大/还原**。
- **根因**:Tauri v2 capabilities 的 `core:default` **只含窗口读取类权限**(is-maximized/is-minimized/theme 等)与 `allow-internal-toggle-maximize`,**不含写操作权限** → `win.minimize()/maximize()/unmaximize()/close()` 与 `start_dragging()` 的 IPC 被权限拒绝 → 三键与拖拽无反应;双击最大化走的是 `internal-toggle-maximize`(default 已含),故双击有效。此症状组合(双击有效、单击/拖拽全挂)是"窗口写权限缺失"的典型特征。
- **修复**:
  - `capabilities/default.json` 补 `core:window:allow-minimize` / `allow-maximize` / `allow-unmaximize` / `allow-close` / `allow-start-dragging`。
  - `TitleBar.tsx` `.tb-controls` 加 `data-tauri-drag-region="false"`,让按钮区域不被标题栏拖拽区吞掉 mousedown。
- **验证**(release exe + CDP 实测):`invoke("plugin:window|maximize")` → OK → `is_maximized=true` → `unmaximize` → `false`;`minimize`/`start_dragging` → OK;shell DOM `.tb-controls` 的 `data-tauri-drag-region="false"` 已生效。实际鼠标交互(点击/拖动)待用户实测确认。
