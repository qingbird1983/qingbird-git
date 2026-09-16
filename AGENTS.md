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

前端测试：`pnpm test:web`（`src/repoCmds.test.mjs`，运行命令面板的保存逻辑）

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
  - `repos.rs` 仓库增删改查、状态、初始化、变更列表、提交历史、语言统计、一键扫描
  - `sync.rs` 三键操作与提交推送（拉取/克隆通过 `tauri::ipc::Channel` 推送进度事件）
  - `create.rs` 云端新建仓库（GitHub / Gitee API）；`clone.rs` 克隆
  - `run_cmd.rs` 一键运行命令（测试/构建/开发/安装依赖，后台运行并流式回传输出）
  - `settings.rs` 设置与凭证；`shell.rs` 快捷打开与代理探测；`space.rs` 磁盘占用与缓存清理
  - 新增 command 时：写进对应领域模块，再到 `lib.rs` 的 `generate_handler!` 注册；
    跨模块复用的私有项用 `pub(super)`，不要为了省事把所有东西都 `pub`
- `src-tauri/src/store.rs`：配置持久化（`%APPDATA%/com.qingbird.git/config.json`，含代理、主题、每仓库命令/定时设置）
- `src-tauri/src/lib.rs`：托盘图标与菜单（显示/扫描/退出）、关窗最小化到托盘、启动时应用持久化主题、开机自启与系统通知插件注册
- 前端状态：zustand（`src/store.ts` 只放 state + actions）；
  `src/lib/` 放与 store 解耦的纯逻辑（`theme.ts` 主题、`sidebar.ts` 侧栏宽度、`autoSync.ts` 自动同步档位文案、
  `repos.ts` 排序、`appEvents.ts` 托盘与自动同步的后台事件桥接）；
  `src/repoCmds.ts` 一键运行命令的保存/预填纯逻辑（配 `pnpm test:web` 回归测试）
- 前端 UI：`src/components/`（`RepoList` 列表 + `RepoItem` 单条目、`RepoPanel` 主面板骨架 +
  `panel/` 下的浮层组件、`CmdPanel`/`SpacePanel`/各对话框）
- 样式：`src/App.css` 只留文件头与 `@import`，实体在 `src/styles/*.css`；
  **`@import` 顺序即级联顺序**，新增样式文件要插在正确位置
- 图标：`scripts/gen-icons.mjs` 程序化生成（PNG/ICO），改图标后运行 `node scripts/gen-icons.mjs`

## 工作规则

- git 网络操作必须走 `run_net_op`（直连→代理回退），不要另起网络调用
- 拉取永远带 `--ff-only`，分叉时报错不自动 merge
- 代码中不得硬编码任何凭证；用户主动填写的访问令牌 / SSH 私钥路径只存本地配置文件
  （`%APPDATA%/com.qingbird.git/config.json`），认证通过当次 git 子进程的
  `-c http.extraheader`（GitHub/Gitee 令牌）或 `GIT_SSH_COMMAND`（私钥）注入，不落盘到仓库

## 禁止事项

不要把 API Key、Token、密码、私钥写进项目文件。

不要在未确认的情况下执行删除、覆盖、迁移、发布等高风险操作。
