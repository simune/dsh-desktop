# DSH 插件工作区

本目录是 DeepSeek Harness（DSH）插件的统一管理工作区。

## 📄 文档

| 文档 | 说明 |
| --- | --- |
| [dsh-tauri-plan.md](./dsh-tauri-plan.md) | DSH × Tauri 桌面客户端方案（核心洞察、架构、运行时决策、里程碑、风险） |
| [docs/00-project-plan.md](./docs/00-project-plan.md) | 项目计划：WBS 任务分解、依赖关系、验收标准、排期、风险登记 |
| [docs/01-architecture.md](./docs/01-architecture.md) | 架构落地设计：组件边界、进程模型、目录结构、接口、安全模型 |
| [docs/02-server-manager.md](./docs/02-server-manager.md) | Server Manager 详细设计：状态机、启动/停止/重启算法、输出解析、测试要点 |
| [docs/03-runtime-bundling.md](./docs/03-runtime-bundling.md) | 运行时捆绑（方案 A）构建管线：vendor 脚本、体积预算、探测链、版本策略 |
| [docs/04-tauri-shell.md](./docs/04-tauri-shell.md) | Tauri v2 壳实现规格：窗口、IPC 权限、托盘、设置页、单实例、安全 |
| [docs/05-verification.md](./docs/05-verification.md) | 验收与验证清单：M0~M3 验收步骤、故障注入、发布前检查 |

> 桌面客户端工程（`dsh-desktop/`）自计划 M0 起在本工作区根目录创建，与 `plugins/` 插件工作区相互独立。

## 🔗 克隆与子模块

`plugins/dsh-usage-stats` 是独立 git 仓库,以 **submodule** 形式关联(见 `.gitmodules`)。克隆时同步拉取:

```sh
git clone --recurse-submodules <harness仓库地址>
# 或克隆后再执行:
git submodule update --init
```

> 子模块 URL 为 `git@github.com:simune/dsh-usage-stats.git`(SSH)。更新子模块:`git submodule update --remote plugins/dsh-usage-stats`(可选,平时跟随其自身仓库演进即可)。

## 📁 目录结构

```
dsh/
├── README.md                 # 本文件：工作区说明与约定
└── plugins/                  # 所有 DSH 插件及插件源码均存放于此
    └── <plugin-name>/        # 每个插件一个目录，目录名与插件名（npm 包名）一致
        ├── src/              # 插件源码
        ├── lib/              # 构建产物
        ├── tests/            # 测试
        ├── package.json      # 插件包定义
        └── ...               # 该插件的其余文件（README、配置等）
```

## 📌 目录约定

1. **所有 DSH 插件及其源码相关内容统一放在 `plugins/` 目录下**，不在工作区根目录或其他位置存放插件代码。
2. 每个插件按照**插件名称**（即 npm 包名，如 `dsh-usage-stats`）创建独立子目录：`plugins/<plugin-name>/`。
3. 新增、修改、构建、安装、卸载等所有针对插件的操作，**均在本工作区目录下进行**，具体命令在对应插件目录中执行。

## 📦 当前插件

| 插件 | 版本 | 说明 |
| --- | --- | --- |
| [dsh-usage-stats](./plugins/dsh-usage-stats/README.md) | 0.1.12 | DeepSeek Harness Web UI 的轻量使用统计插件：Token 总量、每日趋势、活跃热力图、模型分布及数据导出 |

## 🚀 常见操作

所有操作都在对应插件目录 `plugins/<plugin-name>/` 下执行。

### 安装依赖

```sh
cd plugins/<plugin-name>
npm install
```

### 构建

```sh
npm run build          # tsdown 构建，产物输出到 lib/
```

### 类型检查与测试

```sh
npm run typecheck      # TypeScript 类型检查
npm run test           # vitest 单元测试
npm run check          # 类型检查 + 测试 + 构建
```

### 安装到 Harness

```sh
# 在插件目录构建完成后，回到工作区根目录或任意位置，通过 dsh CLI 安装
dsh plugin --profile web add <plugin-name>
```

更新或卸载：

```sh
dsh plugin --profile web update <plugin-name>
dsh plugin --profile web remove <plugin-name>
```

> 具体安装/更新命令以各插件 README 中的说明为准（例如 `dsh-usage-stats` 使用 `--profile web`）。

## ➕ 新增插件

1. 在 `plugins/` 下创建与插件名一致的目录：`mkdir plugins/<plugin-name>`
2. 在目录内初始化包并编写源码（`src/`、`package.json` 等），可参考现有插件 `plugins/dsh-usage-stats/` 的结构。
3. 完成构建与测试后，按上文方式安装到 Harness。
4. 在「当前插件」表中补充新插件条目。
