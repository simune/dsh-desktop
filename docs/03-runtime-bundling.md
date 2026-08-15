# 运行时捆绑(方案 A)构建管线

> 上游:`dsh-tauri-plan.md` §3(运行时与打包)、§6(R1 体积风险);计划 M2.1/M2.2/M2.4。
> 本文件定义:resources 布局、vendor 脚本、体积预算与裁剪、启动探测链、跨平台矩阵、版本策略。

## 1. 目标与取舍

- **目标**:构建出完全自包含的 app —— 用户机器上不需要预装 node 或 dsh,从 Finder 启动(PATH 极简)也能跑。
- **取舍**(方案 A vs B/C):A 体积最大但最稳;B 体积最小但探测易碎;C 体积最优但 bun 兼容性有 spike 风险。**v1 走 A**,保留 B 作为探测链降级项,C 列为后续优化。

## 2. resources 布局(构建期生成)

```
src-tauri/resources/
├─ node/
│  └─ darwin-arm64/node          # Node 二进制(仅本平台,构建时下载)
├─ dsh/                          # dsh 安装树
│  ├─ lib/bin.js
│  ├─ node_modules/…             # --omit=dev 安装
│  └─ package.json
└─ runtime-manifest.json         # 版本/平台/校验和/构建时间
```

tauri.conf.json 资源映射(`docs/04` §2):`resources/` 全部进 app bundle 的 `Contents/Resources/`,运行时经 `app.path().resource_dir()` 定位。

## 3. vendor 脚本

### 3.1 `scripts/vendor-dsh.mjs`

```bash
# 等价命令(脚本内用 child_process 执行并校验)
mkdir -p src-tauri/resources/dsh
cd src-tauri/resources/dsh
npm install @deepseek-ai/dsh@0.1.0-rc.6 \
  --omit=dev --no-audit --no-fund --no-package-lock=false
```

- 版本**硬编码锁定**在脚本常量 + `package.json`(dependencies 固定 `0.1.0-rc.6`)。
- 用 `npm ci`(有 lockfile 时)保证可复现。
- 完成后把安装树根 `package.json` 的 version 写入 `runtime-manifest.json`。

### 3.2 `scripts/vendor-node.mjs`

按 `process.platform + process.arch` 下载官方 Node 二进制:

| 平台 | 下载源 | 产物 |
|---|---|---|
| darwin-arm64 | `https://nodejs.org/dist/v<VER>/node-v<VER>-darwin-arm64.tar.gz` | `resources/node/darwin-arm64/node` |
| darwin-x64 | 同上 x64 变体 | `resources/node/darwin-x64/node` |
| win32-x64 | `node-v<VER>-win-x64.zip` | `resources/node/win32-x64/node.exe` |
| linux-x64 | `node-v<VER>-linux-x64.tar.xz` | `resources/node/linux-x64/node` |

- Node 版本建议 LTS(当前 22.x),与 dsh 运行所需 ABI 兼容(本机实测运行于系统 node,选 LTS 即可)。
- 下载校验:比对 `SHASUMS256.txt` 或预置的 sha256;失败即构建失败(防静默坏包)。
- 解压后仅保留二进制本体(去掉 `include/`、`share/` 等),单文件体积约 60~90M。

### 3.3 `scripts/prune-dsh.mjs`(M2.4,优化项)

- 目标:只保留 **web profile 启动所需闭包**,删掉 `lib/` 中与 `--profile web` 无关的入口(如 headless/cmdline 相关 bundle)。
- 做法(需 spike):从 `lib/bin.js` 的 web 分支出发,静态扫描 `import()`/`require()` 依赖闭包,产出保留清单;对 `node_modules` 用 `npm prune --omit=dev` + 白名单删除。
- **铁律**:裁剪后必须通过 `docs/05` 的 M2 验证(全新环境启动),任何"删了跑不起来"都回退。
- 不阻塞主线:先做 `--omit=dev`,体积预算见 §4。

## 4. 体积预算

| 项 | 现状 | 目标 |
|---|---|---|
| dsh 依赖树(全量) | 342M(实测) | - |
| `--omit=dev` 后 | 预计 260~300M(待实测) | ≤ 300M(v1 可接受) |
| prune 闭包裁剪后(M2.4) | - | ≤ 200M(优化目标) |
| Node 二进制 | 60~90M/平台 | 计入总量 |
| app 本体(壳 UI + Rust) | < 20M | - |
| **安装包总量(估算)** | - | v1 ≈ 350~400M;裁剪后 ≤ 250M |

> 体积只影响安装包大小与启动解压,不影响运行正确性;R1 风险按此表跟踪,每阶段记录实测值到本表。

## 5. 启动探测链(runtime.rs)

```
resolve_runtime(cfg) -> (node_path, dsh_dir):
  1. bundled(优先)
     node  = resource_dir()/node/<platform>/node
     dsh   = resource_dir()/dsh
     校验: node 可执行(file 存在 + 有 x 权限)、dsh/lib/bin.js 存在
     通过 → 返回,记日志 "runtime: bundled"
  2. 用户配置
     cfg.node_path / cfg.dsh_dir 显式指定 → 校验同上
  3. 系统 PATH(降级)
     which node;which dsh(dsh 安装树由 `dsh` 命令定位:
     `readlink -f $(which dsh)` → <dsh>/lib/bin.js 所在树)
  全失败 → E_RUNTIME_NOT_FOUND,错误页展示三项探测详情
```

- 顺序即优先级:bundled 缺失(开发期未 vendor)自动落到 PATH,开发体验好,不必先跑 vendor。
- 每次探测结果记 `[app]` 日志,便于排障。
- 探测只做存在性与可执行性检查,不做版本断言(bundled 由构建期锁定)。

## 6. 跨平台矩阵(构建期)

| 阶段 | macOS(arm64) | macOS(x64) | Windows(x64) | Linux |
|---|---|---|---|---|
| v1 目标 | ✅ 主线 | 可交叉构建(同 pipeline) | ✅ 已适配 | 预留 |
| Node vendor | ✅ | ✅ 同脚本 | ✅(`win-x64`→`win32-x64`) | 脚本就绪 |
| 进程组清理 | ✅ | ✅ | `taskkill /T /F` | ✅ 同 Unix |
| 安装包 | ✅ dmg | ✅ dmg | ✅ NSIS | 待适配(deb/AppImage) |

> v1 产出 darwin-arm64 / darwin-x64 / win32-x64;Linux 仅脚本与 cfg 预留。Windows vendor:官方包名 `win-x64`,落地目录 `win32-x64`(与 `runtime.platform_dir()` 一致)。

## 7. tauri.conf 相关配置

```jsonc
{
  "bundle": {
    "resources": ["resources/**/*"],     // 全部随包分发
    "macOS": {
      "minimumSystemVersion": "11.0",
      "dmg": { "windowSize": { "width": 640, "height": 400 } }
    }
  }
}
```

- 不启用 app sandbox(需写 `~/.dsh`);`entitlements` 保持默认(无沙箱)。
- `resources` 过大时,可改 `externalBin` 只对 node 二进制(可选,非必须)。

## 8. 版本与升级策略

- **锁定**:dsh 固定 `0.1.0-rc.6`(方案 §7);升级 = 修改 vendor 脚本版本常量 → 重新 vendor → 全量回归(`docs/05`)。
- **升级体验**:内置 dsh 随 app 分发,升级 dsh 即换新版 app(覆盖安装);旧 app 生成的 `profiles/node_modules` 符号链接指向旧 bundle 路径 → 新版本首次启动由 dsh 自愈逻辑重建(方案 §6 R3,已在 M3.4 验证)。
- 不引入自动更新器(v1 个人自用);如需,后续评估 `tauri-plugin-updater`。

## 9. 验证清单(M2 出口)

1. `npm run vendor` 产出 resources 且 `runtime-manifest.json` 记录版本/校验和。
2. 删除本机 PATH 中的 node/dsh(或用干净用户)仍可启动 —— 证明 bundled 生效。
3. `du -sh target/release/bundle/dmg/*.dmg` 记录体积,对照 §4 预算。
4. 覆盖安装(新版本 app 拖入 Applications)后首次启动正常。
5. 开发态(未 vendor)启动自动落到系统 PATH,`[app] runtime: PATH` 日志可见。
