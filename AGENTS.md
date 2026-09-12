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
- 测试文件放在：Rust 单元测试在各模块 `#[cfg(test)]` 中

## 常用命令

安装依赖：`pnpm install`

启动开发环境：`pnpm tauri dev`

运行测试：`cd src-tauri && cargo test`

代码检查：`pnpm build`（含 tsc 类型检查）

构建项目：`pnpm tauri build`

Rust 快速检查：`cd src-tauri && cargo check`

## 架构要点

- `src-tauri/src/git.rs`：git CLI 子进程封装（超时控制、输出捕获、网络错误分类、stderr 按 \r/\n 逐行流式回调供进度解析）
- `src-tauri/src/proxy.rs`：代理探测（Windows 注册表系统代理、常见端口扫描、HTTP CONNECT 实测）
- `src-tauri/src/commands.rs`：Tauri commands（仓库 CRUD、三键操作、克隆、一键扫描、代理回退编排、主题设置；
  拉取/克隆通过 `tauri::ipc::Channel` 推送进度事件）
- `src-tauri/src/store.rs`：配置持久化（`%APPDATA%/com.qingbird.git/config.json`，含代理、主题）
- `src-tauri/src/lib.rs`：托盘图标与菜单（显示/扫描/退出）、关窗最小化到托盘、启动时应用持久化主题、开机自启插件注册
- 前端状态：zustand（`src/store.ts`），UI 组件在 `src/components/`（仓库列表支持拖拽添加、克隆对话框、
  拉取进度条、更新内容卡片、设置弹窗含主题分段控件与自启开关）
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
