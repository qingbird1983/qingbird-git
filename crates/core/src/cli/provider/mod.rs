//! Provider 抽象：一个包管理器 = 一个实现。
//!
//! 签名是**同步**的——core 全仓纯同步，异步没有第二个消费者；
//! 界面侧用 `background_executor().spawn` 把 [`PackageProvider::list`] 包进线程，
//! 与现有 git 操作编排同构。

mod npm;
mod pipx;
mod scoop;

pub use npm::Npm;
pub use pipx::Pipx;
pub use scoop::Scoop;

use std::time::Duration;

use crate::cli::model::{CliPackage, ProviderId};

/// 单个 provider 那条外部命令最多等多久。
///
/// `npm ls -g` 有时要几秒（要读全局目录），给宽一点；真挂住了也不至于把
/// 整个扫描拖死。取 30 秒：比本机实测的正常耗时高一个量级，又短到
/// 用户不会以为界面卡了
pub(crate) const LIST_TIMEOUT: Duration = Duration::from_secs(30);

/// 一个包管理器。
pub trait PackageProvider: Send + Sync {
    fn id(&self) -> ProviderId;

    /// 探测这个包管理器装没装（`where <cmd>`，毫秒级、无副作用）
    fn detect(&self) -> bool;

    /// 列出这个包管理器装的全部包。**失败返回 `Err`**，由扫描协调器收进
    /// [`crate::cli::ScanReport::errors`]，**不影响别的 provider**。
    ///
    /// 阻塞（要跑外部命令）——调用方放后台线程。
    fn list(&self) -> Result<Vec<CliPackage>, String>;
}

/// 阶段一固定的三个 provider。
pub fn providers() -> Vec<Box<dyn PackageProvider>> {
    vec![Box::new(Scoop), Box::new(Npm), Box::new(Pipx)]
}
