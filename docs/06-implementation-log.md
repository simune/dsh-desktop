# 实施日志(实测记录)

> **维护约定(2026-08-15 起)**:后续每次修改均作为独立 git commit 提交,便于回溯。提交粒度=一次逻辑改动(功能/修复/文档/测试),消息格式 `类型: 简述`(`feat:`/`fix:`/`docs:`/`test:`/`chore:`)。


> 按 `docs/00-project-plan.md` Definition of Done 第 3 条,关键路径保留实测记录。
> 每条记录:日期、任务 ID、命令/操作、结果。


> **环境坑(重要)**:本工作区所在卷 `/Volumes/Data` 为 **ExFAT**,macOS 为每个文件自动生成 `._` AppleDouble 侧车。影响:
> 1. 工具链中硬链接原子写入(link 系统调用)不可用 → write/edit 工具需改用 bash 直写;
> 2. `tauri-build` 遍历生成目录读到 `._default.toml` 报 `stream did not contain valid UTF-8` → 通过 `dsh-desktop/.cargo/config.toml` 将 cargo target-dir 指向 APFS 卷(`~/Library/Caches/dsh-desktop/target`)解决。

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
