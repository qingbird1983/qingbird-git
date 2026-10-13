//! Provider 抽象：一个包管理器 = 一个实现。
//!
//! 签名是**同步**的——core 全仓纯同步，异步没有第二个消费者；
//! 界面侧用 `background_executor().spawn` 把 [`PackageProvider::list`] 包进线程，
//! 与现有 git 操作编排同构。

mod cargo;
mod npm;
mod pipx;
mod scoop;
mod winget;

pub use cargo::Cargo;
pub use npm::Npm;
pub use pipx::Pipx;
pub use scoop::Scoop;
pub use winget::Winget;

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

    /// 这个包管理器**自己的共享缓存**在哪儿（设计 §8 第 2 级「包管理器文件清单」）。
    ///
    /// 默认空表：`rules.toml` 里那条"环境变量 + 文档默认值"已经把常见情况覆盖了。
    /// **只有"问它自己比查默认值更准"的那一个才覆写它**——现在只有 npm：
    /// 它的 `cache` 能在 `.npmrc` 里改，而默认路径那时会落空 ⇒ 少报一整条 18 GB。
    ///
    /// 阻塞（可能要跑命令）——调用方放后台线程。
    fn cache_dirs(&self) -> Vec<crate::cli::model::PackageDir> {
        Vec::new()
    }

    /// 这个 provider 的 `install_path` 是不是**好几颗包共用**的目录。
    ///
    /// 默认 `false`：scoop / npm / pipx 的安装目录各自一格，里面出现的 exe 就是
    /// 这颗包自己的。**cargo 覆写成 `true`**：它把所有 `cargo install` 出来的 exe
    /// 都丢进同一个 `$CARGO_HOME\bin`，把整目录当"这颗包自己的"会让同目录里
    /// 别人的 GUI 主程序算到**每一颗** cargo 包头上——而那是"卸载会带走一个桌面
    /// 程序"这种会让用户卸错东西的误报（设计 §4 唯一不许犯的错）。
    ///
    /// 消费它的是 [`crate::cli::desktop`]
    fn shared_install_dir(&self) -> bool {
        false
    }
}

/// 阶段二的五个 provider（顺序见 [`ProviderId::all`]）。
pub fn providers() -> Vec<Box<dyn PackageProvider>> {
    vec![
        Box::new(Winget),
        Box::new(Scoop),
        Box::new(Npm),
        Box::new(Pipx),
        Box::new(Cargo),
    ]
}
