//! qingbird-git 纯逻辑后端。
//!
//! 从 src-tauri 平移而来，去 Tauri 依赖：gui（guise）与 src-tauri 过渡期共用。
//! 迁移完成后 src-tauri 整体退役，本 crate 成为唯一的业务层。

// 骨架期：部分 API 先于调用方迁移（sync/commands 逻辑后续平移），暂未使用
#![allow(dead_code)]

pub mod git;
pub mod proxy;
pub mod store;
pub mod types;
