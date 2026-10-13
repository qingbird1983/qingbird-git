# qingbird-git CLI 工具管理模块设计方案

> 基于 qingbird-git 现有架构的功能扩展设计文档
>
> ⚠️ **本稿已被取代（2026-10-13）**：最终版在同目录
> `qingbird-git CLI 工具管理模块设计方案-最终版.md`。本稿保留作讨论史——
> 其中 `features/` 目录、横排侧栏入口、DockArea、tokio/async_trait、
> 「缓存清理并入空间页」五处在最终版被勘误或修正，勿照本稿实施。

---

## 1. 文档目的

本文档描述如何在现有 [qingbird-git](https://github.com/qingbird1983/qingbird-git) 项目基础上，扩展出 CLI 工具可视化管理能力。设计严格遵循项目现有的两层架构（`crates/core` 纯业务逻辑 + `gui/` gpui-kit 界面），复用已有的 SQLite 缓存、操作日志、磁盘扫描和命令执行基础设施。

---

## 2. 现状分析

### 2.1 项目现有架构

```text
qingbird-git/
  crates/
    core/                    # 纯 Rust 业务逻辑，不认识 gpui
      src/
        ...                  # Git 仓库扫描、状态查询、体积计算、操作编排
  gui/
    src/
      features/              # 各功能模块视图
      app.rs                 # App Shell
      ...
  Cargo.toml                 # workspace
```

### 2.2 可复用的基础设施

| 能力 | 现有实现 | CLI 管理的复用方式 |
|---|---|---|
| 分层架构 | `crates/core` + `gui/` | 新增 `crates/cli-manager` 遵循同样模式 |
| SQLite 缓存 | 仓库列表、体积、状态 | 新增 `cli_packages` / `cli_package_dirs` 表 |
| 操作日志 | 逐仓库落进操作历史 | CLI 升级/卸载复用同一日志 UI |
| 子进程执行 | git 命令封装 | 包管理器命令复用同样的编排模式 |
| 磁盘扫描 | 递归计算仓库大小 | 目录大小计算直接复用 |
| 命令面板 | ⌘K 全局命令面板 | 新增“CLI 工具”分组 |
| 快捷键 | 可自定义 keymap | 为 CLI 操作分配快捷键 |
| 环境隔离 | `QB_APPDATA` 变量 | CLI 扫描测试复用同样机制 |

### 2.3 设计原则

- **架构一致**：新模块完全对齐现有 core/gui 两层分离模式
- **只读优先**：先做扫描和展示，再开放写操作
- **操作可预览**：所有写操作在执行前显示完整命令和影响范围
- **不重新实现包管理**：调用官方包管理器命令，不直接操作文件系统或注册表
- **复用优先**：SQLite、日志、磁盘扫描、命令面板全部复用现有实现

---

## 3. 模块划分

### 3.1 新增 Crate

```text
crates/
  core/                      # 现有
  cli-manager/               # 【新增】CLI 管理业务逻辑（纯 Rust）
    src/
      lib.rs
      model.rs               # CliPackage, PackageKind, InstallSource, PackageDir
      provider/
        mod.rs               # PackageProvider trait
        winget.rs
        scoop.rs
        brew.rs
        npm.rs
        pipx.rs
        cargo.rs
      rules/
        mod.rs
        builtin.toml         # 内置规则库
      scan.rs                # 扫描协调器
      plan.rs                # 操作计划生成
      execute.rs             # 命令执行与日志
      toolchain.rs           # 仓库工具链检测
```

### 3.2 新增 GUI Feature

```text
gui/
  src/
    features/
      cli_manager/           # 【新增】CLI 管理界面
        mod.rs
        view.rs              # 主表格视图（DataTable）
        detail.rs            # 详情面板
        dialogs.rs           # 升级/卸载确认对话框
        log.rs               # 操作日志面板
        toolchain.rs         # 仓库工具链页签
    app.rs                   # App Shell：组合 git 和 cli_manager
```

### 3.3 依赖关系

```text
gui (App Shell)
 ├── features/git           # 现有
 ├── features/cli_manager ──→ crates/cli-manager
 └── crates/core            # 现有
```

`crates/cli-manager` 不依赖 gpui-kit，与 `crates/core` 保持同等地位，可独立测试。

---

## 4. 数据模型

### 4.1 核心结构体

```rust
pub struct CliPackage {
    pub id: String,                    // 唯一标识，如 "winget:Git.Git"
    pub provider: ProviderId,
    pub name: String,
    pub version: Option<String>,
    pub latest: Option<String>,
    pub install_path: Option<PathBuf>,
    pub executables: Vec<PathBuf>,
    pub kind: PackageKind,
    pub desktop_links: Vec<DesktopLink>,
    pub dirs: Vec<PackageDir>,
}

pub enum ProviderId {
    Winget,
    Scoop,
    BrewFormula,
    BrewCask,
    Npm,
    Pipx,
    Cargo,
    Manual,          // 手动安装，无法自动管理
}

pub enum PackageKind {
    CliOnly,             // 纯 CLI
    CliWithDesktop,      // CLI + 桌面同包（如 Docker Desktop + docker）
    DesktopWithCli,      // 桌面应用附带 CLI（如 VS Code + code）
    Unknown,
    SystemComponent,     // 系统组件，不出现在用户列表
}

pub enum DirKind {
    Cache,               // 可重建缓存
    Config,              // 配置、登录信息
    Data,                // 用户数据、数据库
    State,               // 状态、日志
    SharedCache,         // 共享缓存（如 npm、pip 全局缓存）
}

pub struct PackageDir {
    pub path: PathBuf,
    pub kind: DirKind,
    pub size: Option<u64>,   // 惰性计算
    pub shared: bool,
}

pub struct DesktopLink {
    pub path: PathBuf,
    pub link_type: DesktopLinkType,
    pub confidence: Confidence,
}

pub enum Confidence {
    High,     // 确定性判断
    Medium,   // 启发式判断
    Low,      // 需要用户确认
}
```

### 4.2 SQLite 缓存扩展

复用现有数据库，新增两张表：

```sql
CREATE TABLE IF NOT EXISTS cli_packages (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT,
    latest TEXT,
    install_path TEXT,
    kind TEXT,
    last_scanned_at INTEGER
);

CREATE TABLE IF NOT EXISTS cli_package_dirs (
    package_id TEXT NOT NULL,
    path TEXT NOT NULL,
    kind TEXT NOT NULL,
    size INTEGER,
    shared INTEGER DEFAULT 0,
    FOREIGN KEY (package_id) REFERENCES cli_packages(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_cli_packages_provider ON cli_packages(provider);
CREATE INDEX IF NOT EXISTS idx_cli_package_dirs_package ON cli_package_dirs(package_id);
```

**扫描策略**：复用现有“先读缓存渲染，再后台刷新”模式。启动时立即从 SQLite 读取渲染列表，同时 `background_spawn` 触发全量扫描，完成后更新 UI 和缓存。

---

## 5. Provider 抽象层

### 5.1 统一接口

```rust
#[async_trait::async_trait]
pub trait PackageProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    async fn detect(&self) -> bool;
    async fn list(&self) -> Result<Vec<CliPackage>>;
    async fn outdated(&self) -> Result<Vec<UpdateInfo>>;

    async fn plan_upgrade(&self, pkg: &CliPackage) -> Result<CommandPlan>;
    async fn plan_uninstall(&self, pkg: &CliPackage) -> Result<CommandPlan>;
    async fn files(&self, pkg: &CliPackage) -> Result<Vec<PathBuf>>;
}
```

### 5.2 命令计划

**核心原则**：生成结构化命令计划，而非拼接 shell 字符串。执行时使用 `Command::new(plan.program).args(plan.args)`，避免注入和转义问题。这与现有 `crates/core` 中 git 命令封装的模式一致。

```rust
pub struct CommandPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    pub needs_admin: bool,
    pub risk: RiskLevel,
    pub summary: String,
    pub affected_packages: Vec<String>,  // 受影响的其他包
}

pub enum RiskLevel {
    Safe,        // 无副作用
    Moderate,    // 可能影响其他工具
    High,        // 可能移除共享组件
    Destructive, // 可能丢失数据
}
```

### 5.3 各包管理器命令映射

| Provider | 列表命令 | 升级命令 | 卸载命令 |
|---|---|---|---|
| winget | `winget list` | `winget upgrade --id <id>` | `winget uninstall --id <id>` |
| scoop | `scoop export` | `scoop update <name>` | `scoop uninstall <name>` |
| brew formula | `brew info --json --installed` | `brew upgrade <name>` | `brew uninstall <name>` |
| brew cask | `brew info --json=v2 --installed` | `brew upgrade --cask <name>` | `brew uninstall --cask <name>` |
| npm | `npm ls -g --depth=0 --json` | `npm update -g <name>` | `npm uninstall -g <name>` |
| pipx | `pipx list --output json` | `pipx upgrade <name>` | `pipx uninstall <name>` |
| cargo | 读取 `$CARGO_HOME/.crates2.json` | `cargo install <name>` | `cargo uninstall <name>` |

**关于 winget**：winget 的 `list` 不直接支持 JSON 输出，建议用 PowerShell 模块 `Get-WinGetPackage` 或解析 `winget export` 的输出。

---

## 6. 扫描与发现

### 6.1 扫描流程

```text
1. 检测可用的 Provider（并行）
      ↓
2. 对每个可用 Provider 调用 list()（并行）
      ↓
3. 合并去重
      ↓
4. 过滤系统组件
      ↓
5. 补充元数据：
   - 数据目录大小（惰性）
   - 桌面关联检测
   - 可执行文件路径
      ↓
6. 写入 SQLite 缓存
      ↓
7. 更新 UI（通过 Entity notify）
```

### 6.2 系统组件过滤

排除以下命令：

- Windows：`cmd`、`powershell`、`where`、`reg`、`net`、`sc`
- macOS/Linux：`ls`、`grep`、`cat`、`bash`、`zsh`、`systemctl`

过滤依据：

- 安装路径在系统目录（`/usr/bin`、`/bin`、`C:\Windows\System32`）
- `apt-mark showmanual` / `brew leaves` 之外的依赖项
- 包描述包含 “system”、“core”、“base”

### 6.3 PATH 扫描辅助

| 路径模式 | 推测来源 |
|---|---|
| `~\scoop\apps\...` | scoop |
| `%AppData%\npm` | npm |
| `~/.local/bin` 且 pipx 记录存在 | pipx |
| `~/.cargo/bin` | cargo |
| `/opt/homebrew/bin` | Homebrew |
| `/snap/bin` | snap |

---

## 7. 桌面关联检测

### 7.1 高置信度信号

| 信号 | 检测方式 |
|---|---|
| 包管理器类型 | `brew cask` 通常是桌面应用 |
| 桌面文件 | Windows `.lnk`、macOS `.app`、Linux `.desktop` |
| PE 子系统 | Windows 可执行文件 PE 头 Subsystem 字段（GUI=2，Console=3） |
| 文件关联 | Windows `HKCR\Applications`、macOS LaunchServices |
| 同安装目录 | Docker Desktop 和 `docker` CLI 在同一目录 |

### 7.2 中置信度信号

- 依赖 GUI 框架（Qt、GTK、Electron）
- 安装目录在 `Program Files`、`/Applications`
- 包描述包含 “Desktop”、“GUI”、“Tray”

### 7.3 结论标签与操作建议

| 标签 | 说明 | 卸载建议 |
|---|---|---|
| `CliOnly` | 纯 CLI 工具 | 可直接卸载 |
| `CliWithDesktop` | CLI + 桌面同包 | 警告：卸载将同时移除桌面应用 |
| `DesktopWithCli` | 桌面应用附带 CLI | 警告：卸载将同时移除 CLI 命令 |
| `Unknown` | 无法确定 | 提示用户确认 |
| `SystemComponent` | 系统组件 | 不显示在用户列表中 |

---

## 8. 数据目录发现

### 8.1 规则库设计

内置 TOML 规则库，支持远程更新：

```toml
[[rule]]
provider = "npm"
package = "typescript"
kind = "cli-only"
cache = ["~/.npm/_cacache"]
config = ["~/.npmrc"]
shared_cache = true

[[rule]]
provider = "winget"
package = "Docker.DockerDesktop"
kind = "cli-with-desktop"
data = ["%APPDATA%/Docker", "%LOCALAPPDATA%/Docker"]
cache = ["%LOCALAPPDATA%/Docker/wsl"]
desktop_links = ["%ProgramFiles%/Docker/Docker/Docker Desktop.exe"]
```

### 8.2 目录发现优先级

1. 规则库匹配（最高优先级）
2. 包管理器文件清单（`brew list --formula <pkg>`、`dpkg -L <pkg>`）
3. 运行时监控（Procmon、`lsof`、`strace`）
4. 可执行文件字符串扫描
5. 环境变量推断（`XDG_CACHE_HOME`、`CARGO_HOME`、`NPM_CONFIG_CACHE`）

### 8.3 目录安全级别

| 类型 | 可清理性 | UI 提示 |
|---|---|---|
| Cache | ✅ 可安全清理 | “可清理，不影响功能” |
| SharedCache | ⚠️ 影响多个工具 | “清理将影响 N 个工具” |
| Config | ❌ 不要自动删除 | “包含配置和登录信息” |
| Data | ❌ 不要自动删除 | “包含用户数据，删除不可恢复” |
| State | ⚠️ 可提示清理 | “包含日志，清理后不影响功能” |

---

## 9. GUI 集成

### 9.1 异步任务模型

gpui-kit 内置异步运行时，不需要引入 smol 或 tokio：

- `cx.spawn()`：前台任务，在 UI 线程运行，可通过 `AsyncApp` 更新 Entity
- `cx.background_spawn()`：后台任务，在工作线程运行，返回自有数据
- `Task<()>` 存储在 struct 中时，drop 会自动取消任务

```rust
impl CliManagerView {
    fn scan_all(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            let packages = cx.background_spawn(async move {
                scan_all_providers().await
            }).await;

            this.update(cx, |state, cx| {
                state.packages = packages;
                cx.notify();
            }).ok();
        }).detach();
    }
}
```

升级/卸载使用 `background_spawn` 执行命令，通过通道将输出流式传回前台更新日志面板。

### 9.2 主列表：DataTable

使用 gpui-kit 的 DataTable 组件，实现 `TableDelegate` trait 作为数据源，用 `TableState` 管理表格状态。

| 列 | 内容 | 宽度 |
|---|---|---|
| 名称 | 包名 + 来源图标 | 200px |
| 版本 | 当前版本 | 100px |
| 最新 | 可更新版本（高亮） | 100px |
| 来源 | winget/scoop/npm 等 | 100px |
| 类型 | CLI-only / CLI+桌面 | 120px |
| 缓存 | 目录大小 | 80px |
| 操作 | 更新/卸载/打开目录按钮 | 120px |

过滤标签页：`[全部]` `[可更新]` `[有缓存]` `[桌面关联]` `[手动安装]`

### 9.3 布局：DockLayout

使用 gpui-kit 的 DockArea/DockLayout 构建可拖拽、可持久化的工作区布局。推荐侧边栏新增两个入口：

```text
┌──────────────────────────────────────────────────┐
│  仓库  │  CLI 工具  │  缓存与数据  │  设置        │  ← 侧边栏
├──────────────────────────────────────────────────┤
│                                                  │
│             主内容区（DataTable）                 │
│                                                  │
├──────────────────────────────────────────────────┤
│             详情面板（可折叠）                    │
│  - 可执行命令  - 安装路径  - 桌面关联             │
│  - 数据目录    - 升级/卸载按钮                    │
├──────────────────────────────────────────────────┤
│             操作日志（底部面板）                  │
└──────────────────────────────────────────────────┘
```

### 9.4 对话框

gpui-kit 的 `open_window()` 自动用 Root 包裹内容视图，Root 管理浮层。升级/卸载的二次确认对话框使用内置组件。

**升级确认对话框**：

```text
┌─────────────────────────────────────┐
│  确认升级 ripgrep                    │
│                                     │
│  当前版本：14.1.0                    │
│  目标版本：14.2.0                    │
│                                     │
│  将执行：                            │
│  scoop update ripgrep               │
│                                     │
│  影响：仅更新 ripgrep                │
│                                     │
│          [取消]  [确认升级]          │
└─────────────────────────────────────┘
```

**卸载确认对话框**（含桌面关联警告）：

```text
┌─────────────────────────────────────┐
│  ⚠ 确认卸载 Docker Desktop           │
│                                     │
│  此操作将同时移除：                  │
│  - Docker Desktop 桌面应用           │
│  - docker CLI 命令                   │
│  - docker-compose CLI 命令           │
│                                     │
│  将执行：                            │
│  winget uninstall Docker.DockerDesktop│
│                                     │
│  数据目录（不会被删除）：             │
│  %APPDATA%/Docker (1.2 GB)           │
│                                     │
│          [取消]  [确认卸载]          │
└─────────────────────────────────────┘
```

### 9.5 命令面板与快捷键

- `⌘K` 命令面板新增分组：“CLI 工具” → “扫描更新”、“升级全部”、“清理缓存”
- 每个 CLI 详情页支持快捷键触发升级/卸载
- 遵循现有可自定义 keymap 机制

---

## 10. 与 Git 仓库模块的联动

### 10.1 仓库工具链检测

在仓库详情页解析以下文件：

| 检测文件 | 用途 |
|---|---|
| `.tool-versions` | asdf/mise 版本管理 |
| `mise.toml` | mise 版本管理 |
| `rust-toolchain.toml` | Rust 工具链版本 |
| `.nvmrc` / `.node-version` | Node.js 版本 |
| `package.json` 的 `engines` | Node/npm 版本要求 |
| `pyproject.toml` | Python 版本和依赖 |
| `go.mod` | Go 版本 |

### 10.2 双向关联展示

**仓库详情页新增“工具链”页签**：

```text
此仓库需要：
  Node.js >= 20    当前 18.19    可更新
  pnpm 9           未安装        可安装
  Rust stable      当前 stable   正常
```

**CLI 工具详情页反向显示**：

```text
ripgrep 14.1
被以下仓库使用：project-alpha, project-beta
```

### 10.3 磁盘清理联动

现有“标出可安全删除的构建缓存，支持逐项清理与一键清理”功能可纳入 CLI 缓存：

- **仓库构建缓存**：安全，随时可删（现有）
- **CLI 共享缓存**（如 `~/.npm/_cacache`）：提示“清理将影响 N 个工具”
- **CLI 配置/数据目录**：不提供自动清理入口

---

## 11. 操作执行与安全

### 11.1 执行流程

```text
Idle → Scanning → Planning → AwaitingConfirm → Running → Success/Failed
```

1. **Planning**：生成 `CommandPlan`，不执行
2. **AwaitingConfirm**：显示命令、影响范围、风险等级
3. **Running**：在 `background_spawn` 中执行，流式读取输出
4. **Success/Failed**：记录退出码和错误输出，更新缓存

### 11.2 执行约束

- 升级/卸载**串行执行**，避免 `brew`、`apt`、`npm` 并发锁冲突
- 与现有操作锁复用同一把锁，避免和 Git 操作并发
- 需要管理员权限时调用系统授权（Windows UAC、macOS `osascript`、Linux `pkexec`），不保存密码
- 每次操作记录日志：时间、命令、退出码、stdout/stderr
- 执行前记录关键配置目录路径（仅记录，不复制）

### 11.3 缓存清理策略

- 默认只清理 `Cache` 级别目录
- `SharedCache` 需显示受影响的其他工具列表
- `Config` 和 `Data` 不提供自动清理入口
- 清理前计算可释放空间

---

## 12. 实施路线

### 阶段一：只读扫描与展示

- 新建 `crates/cli-manager`，实现 `PackageProvider` trait 和 3～5 个 Provider
- 复用 SQLite 缓存，实现扫描结果存储
- 在 `gui/features/cli_manager/` 中用 DataTable 展示列表
- 支持打开安装目录、复制命令、过滤、排序

### 阶段二：数据目录与桌面关联

- 实现 TOML 规则库
- 复用现有磁盘扫描逻辑计算目录大小
- 实现桌面关联检测（PE 解析、快捷方式扫描、`.desktop` 解析）
- 在详情面板展示目录分类和大小

### 阶段三：升级与卸载

- 实现 `plan_upgrade` / `plan_uninstall`
- 用 gpui-kit 对话框做二次确认
- 复用操作日志 UI
- 实现 SQLite 缓存更新

### 阶段四：仓库工具链联动

- 在仓库详情页解析工具链文件
- 实现仓库 ↔ CLI 工具的双向关联
- 实现工具链状态提示

### 阶段五：缓存清理与远程规则

- 实现安全缓存清理
- 实现远程规则库更新
- 支持自定义 Provider（手动安装的工具）

---

## 13. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|---|---|---|
| 包管理器输出格式变化 | 解析失败 | 优先使用 JSON 输出；维护解析器版本适配 |
| gpui-kit API 变化 | UI 层需要修改 | 业务逻辑与 UI 分离，UI 层保持薄 |
| 跨平台差异大 | 开发成本高 | 先做当前平台，逐步扩展 |
| 权限提升复杂 | 操作失败 | 不保存密码，调用系统授权 API |
| 共享缓存误清理 | 影响多个工具 | 显示影响范围，默认不清理共享缓存 |
| 手动安装的工具无法管理 | 用户困惑 | 标记为“手动安装”，提供自定义命令入口 |
| winget 无原生 JSON 输出 | 解析脆弱 | 使用 PowerShell 模块或 `winget export` 辅助 |

### 项目特定注意事项

- **测试写盘隔离**：CLI 扫描测试必须确保不往真实目录写数据，复用 `QB_APPDATA` 环境变量隔离机制
- **debug/release 行为差异**：release 是纯 GUI 子系统，没有 stderr，CLI 命令执行的错误输出需要显式捕获并写入 `%APPDATA%\com.qingbird.git\app.log`
- **串行执行**：CLI 升级/卸载走应用现有的操作锁，避免与 Git 操作并发
- **winget 解析**：优先使用 PowerShell 模块或 `winget export` 辅助，避免脆弱地解析表格输出

---

## 14. 关键依赖清单

| 依赖 | 用途 |
|---|---|
| `gpui-kit` | 应用入口、异步运行时、主题 |
| `gpui-component` | DataTable、DockLayout、对话框、Root |
| `serde` + `serde_json` | 解析包管理器 JSON 输出 |
| `toml` | 规则库解析 |
| `rusqlite` | 本地缓存（复用现有） |
| `tokio` / `smol` | 异步命令执行（如需） |
| `windows` crate | Windows 注册表、PE 解析、快捷方式 |
| `objc2` / `core-foundation` | macOS LaunchServices、`.app` 检测 |
| `dirs` / `directories` | 跨平台目录定位 |

---

## 15. 总结

qingbird-git 现有的 core/gui 两层分离架构、SQLite 缓存、操作日志、磁盘扫描和命令执行基础设施，为扩展 CLI 管理提供了理想条件。新增 `crates/cli-manager` feature crate 即可，最大的增量价值在于**仓库工具链检测**——这是通用 CLI 管理器做不到的，能让 qingbird-git 从“Git 仓库查看器”升级为“本地开发环境控制台”。

---

*文档版本：1.0 | 最后更新：2026-10-12 | 对应项目：[qingbird-git](https://github.com/qingbird1983/qingbird-git)*