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
| M3.5 跨平台预留 | ✅ 代码层:process_group(tool: unix)/taskkill /T(win)、platform_dir/node_exe cfg 分支、运行时探测链抽象;不实际适配 Windows/Linux |

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
