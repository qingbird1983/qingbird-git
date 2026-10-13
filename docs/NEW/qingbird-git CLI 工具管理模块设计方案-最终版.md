# qingbird-git CLI 工具管理模块设计方案（最终版）

> 基于 qingbird-git 现有架构的功能扩展设计。
> 本版取代同目录 1.0 草稿：修正了 1.0 与实际壳层不符的三处假前提，采纳了
> 「包级缓存清理归卸载流、不放空间页」的决策，UI 章节按真实壳层重写。
> 2.1 增补：CLI 屏首页布局按用户决策改为**沿用仓库屏骨架**（左栏工具列表
> → 右栏管理操作），不另起 DataTable 大版式——保操作习惯性（§10.1）。
> 2.2 增补：**工具图标同步显示**（关键决策）——有自带图标的 CLI 软件把
> 图标亮出来，看图比看名字快；三级降策略 + 磁盘缓存（§10.1 图标节）。
> 勘误与决策的完整依据见第 2 节与第 9 节。

---

## 1. 文档目的

在 qingbird-git（仓库管理台）上扩展 CLI 工具可视化管理能力：扫描本机已装的
开发工具（winget/scoop/npm/pipx/…），展示版本、缓存占用、桌面关联，支持
**带预览、带风险分级、带清理决策**的升级与卸载。差异化价值在第 10 节的
**仓库工具链双向关联**——这是通用 CLI 管理器做不到的。

---

## 2. 对 1.0 草稿的勘误（2026-10-13 逐条验真）

| 1.0 的说法 | 实测 | 最终版处理 |
|---|---|---|
| `gui/src/features/` 目录 | **不存在**，实际是平铺模块（shell/sidebar/overview/dialogs/…） | 改为平铺：`gui/src/cli_tab.rs` + `dialogs/cli.rs` |
| 侧边栏「仓库\|CLI 工具\|缓存与数据\|设置」横排入口 | 实际是**左侧竖排导航轨**（nav_rail，3 个 Nav 入口）+ 标题栏设置齿轮 | 改为导航轨第 4 个入口 `Nav::CliTools` |
| DockArea/DockLayout 可拖拽工作区 | 组件存在但现有壳是**固定三栏**，v4 设计真源即固定布局 | **不引入**，主区按现有两栏语言排 |
| 依赖 `tokio`/`smol`/`async_trait` | gpui 自带异步（§9.1 自己都说了）；core 现状纯同步+线程 | **全部不引**，Provider trait 改同步签名 |
| 「缓存清理并入空间页」（1.0 §10.3 + 评审建议） | 用户修正：包级缓存的心智是「随包生命周期走」，卸载时才决定删不删 | 包级目录清理**收进卸载对话框勾选**；空间页只留 Provider 级共享缓存（见 §9） |

1.0 中验真成立、直接保留的前提：两层架构、SQLite 缓存（`store.rs` rusqlite）、
⌘K 命令面板（`palette.rs`）、`QB_APPDATA` 测试隔离、操作锁串行、
release 是 GUI 子系统无 stderr、DataTable + TableDelegate（gpui-component
0.7.0 `src/table/` 真实存在，**本版 UI 改版后暂不使用，见 §10.1**）。

---

## 3. 架构

### 3.1 Crate 与模块落位

```text
crates/
  core/                        # 现有，不动
  cli-manager/                 # 【新增】纯 Rust，零 gpui 依赖
    src/
      lib.rs
      model.rs                 # CliPackage / ProviderId / PackageKind / PackageDir / OpPlan
      provider/
        mod.rs                 # PackageProvider trait（同步）
        scoop.rs npm.rs pipx.rs   # 阶段一：输出全是 JSON，解析最干净
        winget.rs              # 阶段二：解析最脏，后置
        cargo.rs brew.rs       # 阶段二+
      rules/
        mod.rs builtin.toml    # 目录规则库（TOML）
      scan.rs                  # 扫描协调器（线程并行，见 §7）
      plan.rs                  # OpPlan 生成（§8）
      execute.rs               # 命令执行与日志（§11）
      toolchain.rs             # 仓库工具链检测（§10）
gui/src/
  cli_tab.rs                   # 【新增】CLI 工具屏（左栏工具列表 + 右栏操作详情，仿仓库屏骨架）
  dialogs/cli.rs               # 【新增】升级/卸载确认对话框
```

与 core 同等地位、可独立测试。**约束**：新依赖（`windows` crate、PE 解析等）
引入前先验证**无 build script**——本机依赖带 build script 会破坏进程树内的
编译缓存复用（2026-09-30 实测坑，AGENTS 有记录）。

### 3.2 依赖关系

```text
gui ──→ cli-manager ──→ （rusqlite 复用 store 的库文件，不经 core）
 └────→ core
```

---

## 4. 数据模型

```rust
pub struct CliPackage {
    pub id: String,                    // "scoop:ripgrep"、"winget:Git.Git"
    pub provider: ProviderId,
    pub name: String,
    pub version: Option<String>,
    pub latest: Option<String>,        // 可更新版本；None = 已最新或未查询
    pub install_path: Option<PathBuf>,
    pub executables: Vec<PathBuf>,
    pub kind: PackageKind,
    pub dirs: Vec<PackageDir>,         // 惰性算大小
}

pub enum ProviderId { Winget, Scoop, Npm, Pipx, Cargo, BrewFormula, BrewCask, Manual }

pub enum PackageKind {
    CliOnly,          // 纯 CLI
    CliWithDesktop,   // CLI + 桌面同包（卸载连带桌面）
    DesktopWithCli,   // 桌面应用附带 CLI（卸载连带命令）
    Unknown,          // 提示用户确认
}

pub enum DirKind { Cache, Config, Data, SharedCache }

pub struct PackageDir {
    pub path: PathBuf,
    pub kind: DirKind,
    pub size: Option<u64>,             // 惰性；未算完 = None → UI 显「计算中…」
    pub shared: bool,                  // Provider 级共享（npm/_cacache 等）
}
```

1.0 的 `DesktopLink`/`Confidence` 结构砍掉：桌面关联检测先只做两个**高置信
信号**（快捷方式 `.lnk` 扫描 + 可执行文件同目录 GUI 主程序），结果直接落
`PackageKind`，不单独建实体。中置信度启发式（GUI 框架依赖、描述关键词）
一档**不做**——误报会让用户卸错东西。

**OpPlan（1.0 的 CommandPlan 修正版）——命令序列，不是单命令**：

```rust
pub struct OpPlan {
    pub steps: Vec<Step>,              // 卸载命令 + 勾选的清理步骤，按序执行
    pub risk: RiskLevel,               // 各步取最高
    pub summary: String,
    pub affected_packages: Vec<String>,
    pub reclaimable: Option<u64>,      // 勾选清理项合计，供对话框显示
}

pub struct Step {
    pub kind: StepKind,                // Uninstall / CleanupDir / Refresh
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cleanup_dir: Option<(PathBuf, DirKind)>,  // CleanupDir 时有值
}
```

结构化 `program + args`，禁止拼 shell 字符串——与 core 的 git 命令封装同一条
纪律。RiskLevel：`Safe / Moderate / High / Destructive`。

---

## 5. SQLite 缓存

复用现有库，新增两张表（1.0 的 DDL 保留，`desktop_links` 列砍掉）：

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
```

渲染策略与仓库列表同款：**先读缓存渲染，后台线程重扫，完成后 notify 刷新**。

---

## 6. Provider 抽象（同步 trait，gpui 侧包线程）

```rust
pub trait PackageProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn detect(&self) -> bool;                       // PATH 探测，毫秒级
    fn list(&self) -> Result<Vec<CliPackage>>;      // 秒~十秒级，调用方放线程
    fn outdated(&self) -> Result<Vec<UpdateInfo>>;
    fn plan_uninstall(&self, pkg: &CliPackage, cleanup: &[PackageDir]) -> Result<OpPlan>;
}
```

- **不用 async_trait**：core 全仓纯同步，异步没有第二个消费者。gpui 侧
  `cx.background_spawn` 包同步调用，与现有 git 操作编排同构。
- **升级不做独立 plan**：升级 = `cargo install`/`scoop update` 一条命令，
  无清理步骤可言，直接执行 + 日志即可，不值得进 OpPlan 流程。

| Provider | 列表 | 升级 | 卸载 |
|---|---|---|---|
| scoop | `scoop export`（JSON） | `scoop update <n>` | `scoop uninstall <n>` |
| npm | `npm ls -g --depth=0 --json` | `npm update -g <n>` | `npm uninstall -g <n>` |
| pipx | `pipx list --output json` | `pipx upgrade <n>` | `pipx uninstall <n>` |
| winget | PowerShell `Get-WinGetPackage`（**无原生 JSON**，解析最脏，后置） | `winget upgrade --id` | `winget uninstall --id` |
| cargo | 读 `$CARGO_HOME/.crates2.json` | `cargo install <n>` | `cargo uninstall <n>` |
| brew | `brew info --json --installed` | `brew upgrade [--cask] <n>` | `brew uninstall [--cask] <n>` |

系统组件过滤保留 1.0 §6.2 的口径（系统目录 + 已知命令名单），`Manual`
（手动安装、PATH 扫到但无 Provider 认领）只展示不管理。

---

## 7. 扫描

流程保留 1.0 §6.1（检测 → 并行 list → 合并去重 → 过滤 → 元数据 → 缓存 → 通知），
实现口径按同步化修正：Provider 之间**线程并行**（`std::thread` + channel，
与 core 现行一致），单个 Provider 内部串行调外部命令。

PATH 推测表保留 1.0 §6.3（`~\scoop\apps\…` → scoop 等），用于给 `Manual`
包标注「疑似来源」。

---

## 8. 数据目录发现与安全级别

规则库（TOML，1.0 §8.1 保留）→ 包管理器文件清单 → 环境变量推断，三级降。
**运行时监控（Procmon/lsof）不做**——成本与收益不成比例，砍掉。

| 类型 | 可清理性 | UI 呈现 |
|---|---|---|
| Cache | ✅ 可清理 | 卸载对话框**默认勾选** |
| SharedCache | ⚠️ 跨包 | 不进卸载对话框（见 §9），CLI 屏右栏底部汇总条显示 |
| Config | ❌ 不自动删 | 对话框**灰显禁勾** + 红字「含登录信息」 |
| Data | ❌ 不自动删 | 对话框**灰显禁勾** + 红字「删除不可恢复」 |

---

## 9. 清理归属（本版核心决策）

**清理跟着包的生命周期走，不跟磁盘盘点走。**

| 对象 | 清理入口 | 理由 |
|---|---|---|
| **包级**目录（Cache/Config/Data） | **卸载确认对话框勾选，随卸载一并执行** | 心智 = 「这个工具不要了，删干净」；事后单独找入口反而困惑 |
| **Provider 级共享缓存**（`~/.npm/_cacache`、pip cache、cargo registry） | **CLI 屏右栏底部汇总条**（显示占用 + 清理按钮） | 不属于任何一个包——卸载 typescript 不该清 npm 全局缓存 |
| 仓库构建缓存 | 空间页（**现状不变**） | 与包无关 |
| 空间页与 CLI 的关系 | 空间页**显示** CLI 共享缓存占用（只读），清理动作在 CLI 屏 | 盘点归盘点，动作归动作 |

---

## 10. GUI

### 10.1 布局（沿用仓库屏骨架：左栏列表 → 右栏操作）

**决策（2026-10-13 用户拍板）**：CLI 屏不另起炉灶。直接复用「我的仓库 /
第三方仓库」的既有布局语言——左栏列出扫到的全部 CLI 工具（名称 + 图标），
点一个，右栏出这个工具的管理操作（升级 / 卸载 / 数据目录…）。操作路径与
仓库屏完全同构，**零学习成本**；2.0 稿的「DataTable 全量表格 + 选中行详情」
大版式作废。

```text
┌──┬───────────────┬─────────────────────────────────┐
│导│ CLI 工具       │ ripgrep            [可更新 14.1.2]│
│轨│ ▾ scoop (9)   │ ─────────────────────────────── │
│ ①│   ● ripgrep   │ 版本   14.1.1                    │
│ ②│     fd        │ 来源   scoop · 已装 D:\scoop\…   │
│ ③│     fzf       │ 命令   rg.exe                    │
│ ④│ ▾ npm (5)     │ ┌ 数据目录 ─────────────────┐    │
│  │   pnpm        │ │ 缓存   812 MB   可清理 ☑  │    │
│  │   typescript  │ │ 配置   灰显 · 含登录信息   │    │
│  │ ▸ pipx (3)    │ │ 数据   灰显 · 删不可恢复   │    │
│  │   …           │ └───────────────────────────┘    │
│  │               │ [升级]   [卸载…]                 │
│⚙ │ [重新扫描]     │ ─────────────────────────────── │
│  │               │ 共享缓存 npm 1.4 GB [清理]       │
│  │               │ 操作日志（收起态，复用 log.rs）  │
└──┴───────────────┴─────────────────────────────────┘
 ①总览 ②我的仓库 ③第三方 ④CLI 工具（新增，终端图标）
```

对应关系与复用清单：

| 仓库屏 | CLI 屏 | 复用方式 |
|---|---|---|
| 分组行（我的仓库 / 第三方） | **Provider 行**（scoop / npm / pipx…） | 同款可折叠分组行 + 计数徽章；组内有可更新项时行尾点数 |
| 仓库行 | **工具行**（图标 + 名称） | **图标走三级降策略（见下）**；可更新工具行右侧小圆点徽章 |
| 主区详情（头部 → 信息 → 操作） | **右栏详情**（名称/版本徽章 → 版本·来源·命令·数据目录 → 升级/卸载按钮） | 同款层级、同款 token |
| 空态 | 「未发现 CLI 工具，[重新扫描]」 | `widgets::empty_state` |
| 底部「克隆仓库」按钮位 | 左栏底部「重新扫描」按钮 | 同位同款 |

- **导航轨加第 4 个入口** `Nav::CliTools`；竖排 4px gap 规则不受影响。
  CLI 屏左栏是**屏内自己的列表列**（仿仓库侧栏），不复用也不驱动仓库侧栏
  的折叠状态，两者互不干扰

**工具图标（关键决策，2026-10-13 用户拍板）**：扫出来的 CLI 软件凡是**自带
图标的，把图标同步显示出来**——识别图标比读名字快，左栏一列图标扫一眼就
知道自己装了什么。来源三级降：

| 级 | 来源 | 说明 |
|---|---|---|
| 1 | Provider 清单自报 | scoop manifest / 安装器携带的图标等，Provider 拿得到就用 |
| 2 | **主可执行文件图标资源提取** | `SHGetFileInfoW` 取 HICON → GDI 转 PNG 落盘；这是覆盖面最大的一级 |
| 3 | 「终字符块」 | 工具名首 1~2 字符画块（如 `rg`），与前两级同尺寸占位 |

- **异步提取、先字后图**：图标提取放后台线程（与扫描同批派发），行内先
  渲染终字符块，图标就绪后 notify 刷新替换——与「先读缓存渲染、后台重扫」
  同一套体验语言，**图标绝不阻塞列表首屏**
- **磁盘缓存**：`QB_APPDATA/icons/<包 id 哈希>.png`；键含 exe 修改时间/
  哈希，exe 换版自动重提，不进 SQLite（文件缓存自洽，库表不加列）
- **尺寸**：左栏工具行 16px，右栏详情头部放大到 24px 同源复用
- 大量 CLI exe（Go/Rust 编译居多）**本就无图标资源**：静默落到终字符块，
  不算错误、不弹提示；提取异常同样静默回退
- 阶段一先只出终字符块；图标提取依赖 `windows` crate，归**阶段二**（§12）

- **不占检视栏（inspector）**：CLI 屏不需要 AI 检视
  ⛔ **2026-10-13 用户否决这一条**：实现时他先问「最右边是预留的 ai 助理 栏 你觉得要不要」，
  我按这一行给了"不摆"的三条理由，他答「**需要** / **百分之百的要了**」⇒ 现状是
  **CLI 屏有右栏（同一条 `insp-split` 面板），顶栏那颗右开关与仓库屏是同一颗**。
  上面那一行**已作废**，留着是为了让你看见它被谁、在哪一天推翻的。
  连带作废的还有下面那条的"折叠"那一半：左栏现在是 `body-split` 的 0 号面板，
  **可拖宽、可折叠**（收起态 `cli_collapsed`，与仓库侧栏的 `sidebar_collapsed` 各一份、
  互不干扰），宽度与仓库侧栏**同源**（同一个 `sidebar_w`）——详见 `docs/UI-ROADMAP.md`
  的「CLI 工具管理」那一节里 2026-10-13 的边距一比一落地账
- 列表列宽度策略与仓库侧栏同档（定宽即可，先不做拖宽/折叠——工具名比
  仓库名短得多，折叠收益小；实施后若挤再补）
- 数据目录三档呈现沿用 §8 表格口径；「计算中…」状态下条目照显、按钮可点
  （执行前必有数字）
- **共享缓存汇总条**：Provider 级共享缓存（`~/.npm/_cacache` 等）不属于任何
  一个包，放右栏底部汇总条显示占用 + 清理按钮（§9 的落点）
- **卸载对话框**（`dialogs::shell` 形态）四段信息层级：同时移除清单 →
  等宽命令预览（不可编辑）→ 风险徽章 → **可勾选清理清单**（Cache 默认勾 /
  Config·Data 灰显禁勾 + 红字原因 / 底部「本次可释放约 N MB」，
  size 未算完显「计算中…」但允许勾选——勾选是意向，执行前必有数字）
- ⌘K 加「CLI 工具」分组（重新扫描 / 升级全部 / 清理共享缓存）

### 10.2 仓库工具链关联（差异化价值，但不动页签序列）

工具链检测（`.tool-versions` / `mise.toml` / `rust-toolchain.toml` /
`.nvmrc` / `package.json engines` / `pyproject.toml` / `go.mod`）做成
**概览页工具链卡**，不做详情页新页签——页签序列是 v4 设计真源，一个不动。

```text
此仓库需要：Node.js ≥ 20   当前 18.19   可更新
           pnpm 9          未安装       可安装
```

反向关联（CLI 右栏详情「被 N 个仓库使用」）数据来自 toolchain.rs 的检测结果
缓存，随仓库扫描一并更新。

---

## 11. 执行与安全

状态机保留 1.0 §11.1（Idle → Planning → AwaitingConfirm → Running → Success/Failed）。
执行约束：

- OpPlan 各步**串行**，与 Git 操作共用现有操作锁
- UAC 提权调系统 API，不存密码
- 每步记日志：时间、命令、退出码、stdout/stderr → `app.log`（release 无
  stderr，显式捕获）
- 清理目录**只删内容不删目录本身**，删前记路径入日志

---

## 12. 实施路线（修正版）

| 阶段 | 内容 | 关键点 |
|---|---|---|
| 一 | scoop + npm + pipx 只读扫描展示 | 输出全是 JSON；Nav 入口 + cli_tab（列表栏+详情栏）+ 缓存表 |
| 二 | winget + cargo + 目录规则库 + 桌面关联 + **工具图标提取** | winget 解析最脏放这；`windows` crate 先验无 build script（图标提取同批验） |
| 三 | 升级 + 卸载（OpPlan + 确认对话框 + 共享缓存清理） | 对话框三档交互；串行锁 |
| 四 | 仓库工具链双向关联 | 概览页卡，不动页签 |
| — | ~~远程规则库更新~~ | **砍掉**：单机工具，TOML 随版本走即可，不建更新通道 |

---

## 13. 风险（在 1.0 基础上增删）

| 风险 | 处理 |
|---|---|
| 包管理器输出格式变化 | 优先 JSON；winget 用 PowerShell 模块 |
| **新依赖带 build script** | 引入前逐个验证（本机编译缓存硬约束） |
| PE 解析 / 注册表访问的复杂度 | 桌面关联砍到两个高置信信号，不做中置信度 |
| 卸载连带影响（CliWithDesktop） | 对话框「同时移除」清单明示 + 风险徽章 |
| 共享缓存误清 | SharedCache 不进卸载流，独立汇总条 + 影响包数提示 |
| 手动安装工具 | `Manual` 只展示，标注疑似来源 |
| 图标提取失败 / exe 无图标资源 | 三级降静默回退终字符块，不阻塞扫描与任何操作（§10.1） |

---

## 14. 依赖清单（修正版）

| 依赖 | 用途 | 备注 |
|---|---|---|
| gpui-kit / gpui-component | 现有 | DataTable 本版暂不用（§10.1），组件留着 |
| serde + serde_json | Provider JSON 输出解析 | core 已有 |
| toml | 规则库 | |
| rusqlite | 复用 store 的库 | 不新增依赖实例 |
| windows | `.lnk`、注册表、PE 子系统、**图标提取**（SHGetFileInfoW + GDI 转 PNG） | **引入前验无 build script** |
| dirs | 目录定位 | |
| ~~tokio / smol / async-trait~~ | — | **不引**（§6） |
| ~~objc2 / core-foundation~~ | — | **先不做 macOS**，单平台落地后再说 |

---

*文档版本：2.2（最终版）| 2026-10-13 | 取代同目录 1.0 草稿。2.1 变更：
CLI 屏首页布局改沿用仓库屏骨架（左栏列表 → 右栏操作），DataTable 版式作废。
2.2 变更：工具图标三级降策略 + 磁盘缓存 + 异步先字后图（§10.1）。
卸载对话框 v2 设计以 §10.1 文字描述为准落实现。*
