//! qingbird-git 纯逻辑后端。
//!
//! 原本与旧 Tauri 版平移共存（两套界面共用同一份 config），2026-10-05 起
//! 旧实现已整体退役，本 crate 成为唯一的业务层。
//!
//! 约定：这一层不碰任何 GUI 框架，也不碰 IPC。
//! 流式输出这类要回调界面的地方一律用闭包（`run_cmd::run_repo_cmd`），
//! 由 gui 侧自己接回 UI 线程——业务层不该知道 gpui 的存在。

// 骨架期：部分 API 先于调用方迁移（sync/history/space 等后续平移），暂未使用
#![allow(dead_code)]

pub mod clone;
pub mod fork;
pub mod git;
pub mod history;
pub mod net;
pub mod proxy;
pub mod repos;
pub mod run_cmd;
pub mod scan_cache;
pub mod scheduler;
pub mod space;
pub mod store;
pub mod sync;
pub mod types;
