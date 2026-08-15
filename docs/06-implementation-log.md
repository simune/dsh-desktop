# 实施日志(实测记录)

> **维护约定(2026-08-15 起)**:后续每次修改均作为独立 git commit 提交,便于回溯。提交粒度=一次逻辑改动(功能/修复/文档/测试),消息格式 `类型: 简述`(`feat:`/`fix:`/`docs:`/`test:`/`chore:`)。


> 按 `docs/00-project-plan.md` Definition of Done 第 3 条,关键路径保留实测记录。
> 每条记录:日期、任务 ID、命令/操作、结果。


> **环境坑(重要)**:本工作区所在卷 `/Volumes/Data` 为 **ExFAT**,macOS 为每个文件自动生成 `._` AppleDouble 侧车。影响:
> 1. 工具链中硬链接原子写入(link 系统调用)不可用 → write/edit 工具需改用 bash 直写;
> 2. `tauri-build` 遍历生成目录读到 `._default.toml` 报 `stream did not contain valid UTF-8` → 通过 `dsh-desktop/.cargo/config.toml` 将 cargo target-dir 指向 APFS 卷(`~/Library/Caches/dsh-desktop/target`)解决。

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
