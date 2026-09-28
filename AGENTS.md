# AGENTS.md — qingbird-git（青鸟 Git）

## 项目说明

Windows 桌面工具：本地 Git 仓库一键同步（Tauri 2 + React + TypeScript）。

主要目标：

- 本地仓库管理：添加过的仓库持久化记忆，下次启动自动加载
- 三键操作：检测云端更新 / 一键拉取（--ff-only）/ 一键推送
- 无感代理：git 网络操作直连失败时，自动探测本机代理（系统代理 → 上次成功缓存 → 常见端口 7897/7890/10808 等），
  仅对当次 git 子进程注入 `HTTPS_PROXY` 环境变量重试，不改系统代理、不影响其他程序

## 目录规则

- 正式源码放在：`src/`（React 前端）、`src-tauri/src/`（Rust 后端）
- 文档资料放在：`README.md`、本文件
- 临时实验放在：不要提交到仓库
- 测试文件放在：Rust 单元测试在各模块 `#[cfg(test)]` 中；前端纯逻辑测试放 `src/*.test.mjs`
  （node 自带 `node --test` + 原生 TS 类型擦除，不引入测试框架依赖）

## 常用命令

安装依赖：`pnpm install`

启动开发环境：`pnpm tauri dev`

运行测试：`cd src-tauri && cargo test`

前端测试：`pnpm test:web`（零依赖，node 自带测试运行器）
- `src/repoCmds.test.mjs` 运行命令面板的保存/预填逻辑
- `src/forkCard.test.mjs` 分叉卡片该出哪些按钮
- `src/pinnedCmds.test.mjs` 操作区自定义按钮的列表编辑规则（上限/空行/文案兜底）
- `src/commitDiff.test.mjs` 提交历史看 diff 的展示规则（行分类/行数标签/超长截断）
- `src/activity.test.mjs` 提交活跃度柱状图的数据规则（补齐窗口/本地时区日期键/柱高比例）
- `src/commands.test.mjs` ⌘K 命令面板的模糊匹配（完全/前缀/子序列三档、多词 AND、稳定排序）
- `src/hotkeys.test.mjs` 全局快捷键判定（输入框让位、无选中仓库不触发、修饰键判定）

代码检查：`pnpm build`（含 tsc 类型检查）

构建项目：`pnpm tauri build`

Rust 快速检查：`cd src-tauri && cargo check`

## 架构要点

- `src-tauri/src/git.rs`：git CLI 子进程封装（超时控制、输出捕获、网络错误分类、stderr 按 \r/\n 逐行流式回调供进度解析）
- `src-tauri/src/proxy.rs`：代理探测（Windows 注册表系统代理、常见端口扫描、HTTP CONNECT 实测）
- `src-tauri/src/notify.rs`：后台自动同步结果的系统通知。`toast_for` 是"结果 → 文案"的纯逻辑（`ok` 不发），
  主窗口可见且聚焦时静默；由 Rust 侧直接发，不依赖前端存活。
  开发模式下通知来源会显示成 PowerShell，装成 NSIS 包后才显示本应用名称与图标（插件在 Windows 上的已知行为）
- `src-tauri/src/commands/`：Tauri commands，按领域拆分的模块目录，门面是 `mod.rs`
  （子模块声明、`pub use` 再导出、超时常量与全局操作互斥锁 `OP_LOCK`/`begin_user_op`）
  - `types.rs` 前后端共享的 IPC 数据结构（`RepoStatus`/`ProgressEvent`/`OpResult`…）
  - `net.rs` git 网络操作编排：直连→代理回退、代理竞速、认证注入、进度解析（`run_net_op` 是唯一入口）
  - `repos.rs` 仓库增删改查、状态、初始化、变更列表、提交历史、提交活动（近 14 天柱状图）、
    语言统计、一键扫描
  - `sync.rs` 三键操作与提交推送（拉取/克隆通过 `tauri::ipc::Channel` 推送进度事件）
  - `fork.rs` 分叉恢复向导：`ForkInfo` 组装（附两边的提交列表）+ 备份并变基 + 撤销变基。
    变基序列的 runner 是注入的（`LocalRun`/`RemoteRun`），单测用临时目录里的真实仓库跑真实 git
  - `create.rs` 云端新建仓库（GitHub / Gitee API）；`clone.rs` 克隆
  - `run_cmd.rs` 一键运行命令（测试/构建/开发/安装依赖，后台运行并流式回传输出）+
    操作区自定义按钮 `set_pinned_cmds`（上限 3 个，超了报错不静默截断）
  - `history.rs` 提交历史详情：文件级变更摘要（`--numstat`）与逐文件 patch。
    合并提交走 `git diff <hash>^ <hash>`——`git show <merge>` 默认不输出 diff
  - `settings.rs` 设置与凭证；`shell.rs` 快捷打开与代理探测（`spawn_terminal` 供"去终端看 diff"复用）；
    `space.rs` 磁盘占用、大文件 Top 10、缓存清理与 `.gitignore` 建议（写入只接受白名单目录名）
  - 新增 command 时：写进对应领域模块，再到 `lib.rs` 的 `generate_handler!` 注册；
    跨模块复用的私有项用 `pub(super)`，不要为了省事把所有东西都 `pub`
- `src-tauri/src/store.rs`：配置持久化（`%APPDATA%/com.qingbird.git/config.json`，含代理、主题、
  每仓库命令/定时设置/操作区按钮）
- `src-tauri/src/lib.rs`：托盘图标与菜单（显示/扫描/退出）、关窗最小化到托盘、启动时应用持久化主题、开机自启与系统通知插件注册
- 前端状态：zustand（`src/store.ts` 只放 state + actions）；
  `src/lib/` 放与 store 解耦的纯逻辑（`theme.ts` 主题、`sidebar.ts` 侧栏宽度、`autoSync.ts` 自动同步档位文案、
  `repos.ts` 排序、`appEvents.ts` 托盘与自动同步的后台事件桥接、
  `forkGuide.ts` 分叉卡片该出哪些按钮与文案、`pinnedCmds.ts` 操作区自定义按钮的列表编辑规则、
  `commitDiff.ts` 提交历史的 patch 行分类与截断展示、
  `activity.ts` 提交活跃度柱状图的数据整形（后端只给有提交的日子，缺的天在这里补 0，
  日期键必须用本地时区不能用 `toISOString`）、
  `commands.ts` ⌘K 命令面板的模糊匹配（`matchCommands` 是泛型的，别把调用方挂在命令上的字段抹平）、
  `hotkeys.ts` 全局快捷键判定（DOM 相关的焦点判定由调用方查好当参数传进来）；
  `src/repoCmds.ts` 一键运行命令的保存/预填纯逻辑（配 `pnpm test:web` 回归测试）
  - 这类模块里**不要**写返回 `?? []` 的帮手函数：每次渲染新建数组，拿它当 `useEffect` 依赖会让 effect 每轮重跑。
    需要默认值时由调用方声明模块级的空数组常量（见 `RepoPanel` 的 `EMPTY_PINNED`）
- **入口一多就把开关收进 store**：`cloneOpen` / `newRepoOpen` / `initPath` / `paletteOpen` 都在 store，
  不再用 App 的局部 state 往下传 props。同理 `pullSmart` / `stashPull` 把"脏工作区→stash 确认条"
  这层判断收进 store，按钮、⌘K、快捷键三个入口才能行为一致——**别在组件里另写一份拉取逻辑**
- 前端 UI：`src/components/`（`RepoList` 仓库列表 + `RepoItem` 单条目、`RepoPanel` 主工作区、
  `Inspector` 右侧检视抽屉（变更/历史/定时/空间四页签）、`panel/` 下的浮层与展示组件
  （`SyncRing` 同步环、`ActivityChart` 提交活跃度柱状图、`ForkCard` 分叉恢复卡片、
  `BusyPop`/`StashBar`/`LogArea`）、`CmdPanel`/`SpacePanel`/`HistoryModal`/各对话框）
- 快捷键（判定在 `src/lib/hotkeys.ts`，监听挂在 `App.tsx`）：
  `Ctrl K` 命令面板（任何焦点下都生效）/ `Ctrl T` 当前仓库开终端 /
  `1·2·3` 对应同步卡上**从左到右**的拉取·推送·检测 / `↑↓` 上下切仓库。
  键位约定：`1/2/3` 按屏幕位置对位而非操作顺序——用户是先看见按钮再按位置的键。
  改键位先改 `hotkeys.ts` 再改 `App.tsx`，规则测试在 `src/hotkeys.test.mjs`
- 工作区是横向三列：`work-main`（同步卡 + 命令卡）/ `.stats-col` 176px 统计窄列
  （活跃度柱状图 + 语言构成，卡片在这列里竖排）/ `Inspector` 抽屉 320px。
  再往中间加东西会挤掉主区，要新图形统计就先考虑塞进 `.stats-col` 或抽屉。
  窗口下限 `minWidth: 1000`（860 是阶段 2 引入抽屉后的旧值，那时主区只剩 108px，
  104px 的同步环会顶出右缘）——**要往中间再加一列（如导航轨）之前先重算这条预算**
- 动作分三层（重构后的核心约定，改动界面时别破坏这个分层）：
  **执行层**只有 `RepoPanel` 的同步卡（同步环 + 拉取/推送/检测三键）；
  **命令层**是命令卡列表行（测试/构建/开发/安装依赖 + 钉住的自定义命令）；
  **查看层**全部收进 `Inspector` 抽屉与日志抽屉，不再在主面板平铺图标按钮；
  **低频层**是 `⌘K` 命令面板——「打开文件夹 / 终端 / 说明 / 切仓库」这类不常用但得存在的东西
  统一放这儿，别往主面板加图标按钮。新增功能先想清楚它属于哪一层，再决定放哪
- 样式：`src/App.css` 只留文件头与 `@import`，实体在 `src/styles/*.css`；
  **`@import` 顺序即级联顺序**，新增样式文件要插在正确位置
  （`inspector.css` 放最后：工作区新结构要能覆盖 `panel.css`）
- 图标：源图是 `assets/icon-source.png`（1024px，改图标只改这一张），
  `node scripts/gen-icons.mjs` 调 Tauri 官方 `tauri icon` 展开成 `src-tauri/icons/` 全套
  并同步一份到 `src/assets/icon.png`（标题栏 logo 与空态图用）；不要手改产物，
  也不要把 iOS / Android / icns 产物提交进来（只发 Windows NSIS）

## 工作规则

- git 网络操作必须走 `run_net_op`（直连→代理回退），不要另起网络调用
- 拉取永远带 `--ff-only`，分叉时报错不自动 merge。
  唯一的例外是用户在「本地与云端已分叉」卡片上**显式**点「备份并变基到云端」——
  那条路径每次都会先把"变基前"固定到 `backup/qingbird-git`，冲突自动 `rebase --abort` 复原；
  默认拉取链路与后台自动同步都不许改写历史
- 会派生 git 子进程的 Rust 测试要持 `git::GIT_TEST_LOCK` 的读锁：
  `CANCELLED` 是进程级全局标记，取消测试（持写锁）一置位就会杀掉并行跑的其他测试的 git
- 代码中不得硬编码任何凭证；用户主动填写的访问令牌 / SSH 私钥路径只存本地配置文件
  （`%APPDATA%/com.qingbird.git/config.json`），认证通过当次 git 子进程的
  `-c http.extraheader`（GitHub/Gitee 令牌）或 `GIT_SSH_COMMAND`（私钥）注入，不落盘到仓库

## 禁止事项

不要把 API Key、Token、密码、私钥写进项目文件。

不要在未确认的情况下执行删除、覆盖、迁移、发布等高风险操作。
