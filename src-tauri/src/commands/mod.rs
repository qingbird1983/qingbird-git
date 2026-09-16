//! Tauri commands 的门面：按领域拆到子模块，这里只做组装与再导出。
//!
//! 每个子模块负责一个领域，`pub use` 把其中的公开项重新导出到本模块，
//! 因此 `commands::xxx` 的调用路径（`lib.rs` 的 `generate_handler!`、
//! `scheduler.rs` 的 `try_begin_op` / `do_check` / `do_pull`）保持不变。
//!
//! 模块划分：
//! - `types`  前后端共享的 IPC 数据结构
//! - `net`    git 网络操作编排（直连→代理回退、认证注入、进度解析）
//! - `repos`  仓库增删改查、状态、初始化、变更、语言统计、一键扫描
//! - `sync`   三键操作与提交推送
//! - `fork`   分叉恢复向导（备份并变基、撤销变基）
//! - `history` 提交历史的文件级摘要与逐文件 patch
//! - `create` 云端新建仓库
//! - `clone`  克隆远端仓库
//! - `run_cmd` 一键运行命令（测试 / 构建 / 开发 / 安装依赖）
//! - `settings` 设置与 Git 凭证
//! - `shell`  快捷打开与代理探测
//! - `space`  磁盘占用扫描与缓存清理

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

mod clone;
mod create;
mod fork;
mod history;
mod net;
mod repos;
mod run_cmd;
mod settings;
mod shell;
mod space;
mod sync;
mod types;

// 对外（前端 IPC + 后台调度器）的入口统一从各领域模块再导出，
// 这样 `commands::xxx` 的路径不变：lib.rs 的 generate_handler! 与
// scheduler.rs 的 do_check / do_pull / valid_repo 都不用改。
//
// 这里必须是 glob：`#[tauri::command]` 除了生成命令函数本身，还会在同一模块里
// 生成一个隐藏的 `__cmd__xxx` 项，generate_handler! 两个都要引用；逐个列名
// 只会导出命令函数，隐藏项引用不到，编译期会报 `cannot find __cmd__xxx`。
pub use clone::*;
pub use create::*;
pub use fork::*;
pub use history::*;
pub use repos::*;
pub use run_cmd::*;
pub use settings::*;
pub use shell::*;
pub use space::*;
pub use sync::*;

/// 本地 git 命令（status / log 等）超时
pub(super) const LOCAL_TIMEOUT: Duration = Duration::from_secs(10);
/// 网络 git 操作超时
pub(super) const NET_TIMEOUT: Duration = Duration::from_secs(300);
/// 检查云端更新只做 fetch + 对比，超时比完整拉取短得多
pub(super) const CHECK_TIMEOUT: Duration = Duration::from_secs(90);

/// 全局网络操作互斥锁：用户操作与后台自动同步互斥。
/// 自动同步只 try_lock（拿不到就跳过本轮），用户操作优先并可打断后台同步。
static OP_LOCK: Mutex<()> = Mutex::new(());

/// 后台自动同步尝试进入操作区：用户正在操作时返回 None（跳过本轮）
pub fn try_begin_op() -> Option<MutexGuard<'static, ()>> {
    OP_LOCK.try_lock().ok()
}

/// 用户网络操作进入操作区：若被后台自动同步占用，先请求取消其 git 子进程
/// 再等锁（最多约 10 秒），等不到说明有异常占用，返回 None
pub fn begin_user_op() -> Option<MutexGuard<'static, ()>> {
    if let Ok(g) = OP_LOCK.try_lock() {
        return Some(g);
    }
    crate::git::request_cancel();
    for _ in 0..50 {
        if let Ok(g) = OP_LOCK.try_lock() {
            return Some(g);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}
