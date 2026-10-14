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
use crate::cli::plan::{assemble, OpPlan, Step, UpdateInfo};

/// 单个 provider 那条外部命令最多等多久。
///
/// `npm ls -g` 有时要几秒（要读全局目录），给宽一点；真挂住了也不至于把
/// 整个扫描拖死。取 30 秒：比本机实测的正常耗时高一个量级，又短到
/// 用户不会以为界面卡了
pub(crate) const LIST_TIMEOUT: Duration = Duration::from_secs(30);

/// 查更新的那条命令最多等多久（阶段三）。
///
/// 比列清单宽：`npm outdated` 要走一遍 registry（网络），慢机器 / 弱网上
/// 是秒级到十几秒。超时 = "这次不知道谁可更新"（`latest` 留 `None`），
/// **不是错误**——查不动更新不该让列表本身失败
pub(crate) const OUTDATED_TIMEOUT: Duration = Duration::from_secs(30);

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

    // ---------- 阶段三：升级 / 卸载 / 查更新 ----------

    /// 有哪些包可更新（`latest` 那一列）。
    ///
    /// ⚠️ **返回空表而不是 `Result`**（设计稿写的是 `Result`）：查不动更新
    /// 不等于这一台坏了——`latest` 的语义本来就是"已最新**或未查询**"
    /// （模型注释原话）。返回 `Err` 会把"没查到"升级成一次扫描失败。
    ///
    /// 默认空表：查不了就说不知道。**现在只有 npm 覆写**
    /// （`npm outdated -g --json` 是机器可读的）；winget 的"可用版本"在
    /// `list()` 那一趟就免费带回来了，不在这儿再查一遍。
    fn outdated(&self) -> Vec<UpdateInfo> {
        Vec::new()
    }

    /// 升级这一颗的那条命令。`None` = 这个 provider 升不了它。
    ///
    /// 升级**不进 OpPlan**（设计 §6）：它就是一条命令，没有清理步骤可言，
    /// 直接执行 + 日志即可。cargo 不覆写这一颗——它查不到"有没有新版"
    /// （outdated 空表），没有 `latest` 的升级键是一条骗人的键
    /// （v4 铁律：不演拿不到的效果）。
    fn upgrade_step(&self, _pkg: &CliPackage) -> Option<Step> {
        None
    }

    /// 卸载这一颗的**那条命令**。认不出它（id 缺前缀那种）返回 `None`。
    ///
    /// 与 `upgrade_step` 分开是因为卸载要走 OpPlan（可能带清理步骤），
    /// 而组装逻辑（风险、合计、清单）五家完全相同 ⇒ 收在默认的
    /// [`plan_uninstall`](Self::plan_uninstall) 里，各家只答"命令是什么"
    fn uninstall_step(&self, pkg: &CliPackage) -> Option<Step>;

    /// 卸载计划 = 卸载命令 + 勾选的清理目录（设计 §8）。
    ///
    /// 默认实现就是那套五家通用的组装；provider 只实现 [`uninstall_step`](Self::uninstall_step)。
    /// `cleanup` 是用户在对话框里勾上的那几条（只有 `Cache` 能被勾上）。
    fn plan_uninstall(&self, pkg: &CliPackage, cleanup: &[crate::cli::model::PackageDir]) -> OpPlan {
        match self.uninstall_step(pkg) {
            Some(step) => assemble(pkg, step, cleanup),
            None => OpPlan::blocked(pkg, "这个包管理器给不出卸载命令"),
        }
    }

    // ---------- 阶段四 P0：阶段化升级事件 ----------

    /// 升级输出里的这一行，说明升级进到了哪个阶段（`cli::upgrade` 的五阶段
    /// 管道靠它供血）。`None` = 这行说明不了阶段。
    ///
    /// **默认 `None`：认不出来就不猜。** 从没被任何一行点亮的中间段，在
    /// 命令成功后记「未报告」——按印象猜一个阶段，屏幕上就是一个对不上
    /// 的 ✓。这正是设计 §4 唯一不许犯的错在这一屏的形状：**同一段过程
    /// 两个真相**（我们以为的与包管理器实际干的），界面信的那个必须是
    /// 后者。
    ///
    /// 覆写它的 provider 请拿**真输出**喂单测：winget 那几条是 2026-10-15
    /// 本机 `winget download` 抓的原文（**中文系统**——winget 的输出随
    /// 系统语言变，只匹配英文关键词在中文机器上永远不中）；npm 那条是
    /// 本机 `npm install --prefix <tmp>` 管道模式下的全程输出。没抓过真
    /// 输出的（scoop / pipx）**先不写观测器**：中段记「未报告」比按
    /// GitHub 上的印象写几个关键词诚实
    fn phase_of_line(&self, _line: &str) -> Option<crate::cli::upgrade::UpgradePhase> {
        None
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

/// 按 provider 取一个实例（界面侧拼卸载计划 / 升级命令用）。
///
/// 认不出的 provider（未来版本写进库的键）返回 `None`——调用方就当
/// "这一颗什么都做不了"，不 panic
pub fn provider_of(id: ProviderId) -> Option<Box<dyn PackageProvider>> {
    providers().into_iter().find(|p| p.id() == id)
}

/// 一颗包在 winget 那边的 Id（`winget:Git.Git` → `Git.Git`）。
/// 别的 provider 的 `id` 前缀就是自己的 key，用同一个函数反查也行，
/// 但只有 winget 的卸载/升级要 `--id` 那个值，别的用的都是包名
pub(crate) fn winget_id_of(pkg: &CliPackage) -> Option<&str> {
    pkg.id.strip_prefix("winget:")
}
