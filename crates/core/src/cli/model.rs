//! CLI 工具的共享数据模型（阶段一：只读扫描展示）。

use serde::{Deserialize, Serialize};

/// 这个包是从哪个包管理器扫出来的。
///
/// 阶段一只做三个**输出全是 JSON**的 provider（解析最干净）；
/// winget / cargo / brew 属阶段二、值也是那时才加——**枚举里现在没有它们**，
/// 免得有人以为已经支持了。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ProviderId {
    Scoop,
    Npm,
    Pipx,
}

impl ProviderId {
    /// 稳定键：写进库里、拼进 [`CliPackage::id`]。
    /// ⚠️ **改它等于换一批记录**（与 `Op::key` 同一条纪律：落库的键不是文案）。
    pub fn key(self) -> &'static str {
        match self {
            ProviderId::Scoop => "scoop",
            ProviderId::Npm => "npm",
            ProviderId::Pipx => "pipx",
        }
    }

    /// 界面上显示的名字。阶段一恰好与 [`key`](Self::key) 同形，
    /// 但这是两件事（一个落库、一个给人看），留着两个出口以免以后想改文案时
    /// 顺手把库里的键也改了。
    pub fn label(self) -> &'static str {
        match self {
            ProviderId::Scoop => "scoop",
            ProviderId::Npm => "npm",
            ProviderId::Pipx => "pipx",
        }
    }

    /// 从落库的键反查。认不出返回 `None`（库里存着下个版本写的键时跳过那一行，
    /// 而不是 panic）
    pub fn from_key(s: &str) -> Option<Self> {
        match s {
            "scoop" => Some(ProviderId::Scoop),
            "npm" => Some(ProviderId::Npm),
            "pipx" => Some(ProviderId::Pipx),
            _ => None,
        }
    }

    /// **固定顺序**：列表分组、包排序、扫描派发都按它。
    ///
    /// 收成一颗是为了让"provider 的先后"只有一个出处——界面按它排组、
    /// [`crate::cli::sort_packages`] 按它排包，两处各写一份迟早对不上。
    pub fn all() -> [ProviderId; 3] {
        [ProviderId::Scoop, ProviderId::Npm, ProviderId::Pipx]
    }
}

/// 这个包跟桌面应用有没有关系（卸载会不会连带东西）。
///
/// ⚠️ **阶段一永远是 [`Unknown`](PackageKind::Unknown)**：桌面关联检测
/// （快捷方式扫描 + PE 主程序）属阶段二。现在**不在界面上显示它**，
/// 存一个"没分类"比存一个"猜的分类"诚实——`CliOnly` 是一句有值的结论，
/// 而阶段一还没有证据下这个结论。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PackageKind {
    #[default]
    Unknown,
    /// 纯 CLI
    CliOnly,
    /// CLI + 桌面同包（卸载连带桌面）
    CliWithDesktop,
    /// 桌面应用附带 CLI（卸载连带命令）
    DesktopWithCli,
}

impl PackageKind {
    pub fn key(self) -> &'static str {
        match self {
            PackageKind::Unknown => "unknown",
            PackageKind::CliOnly => "cli_only",
            PackageKind::CliWithDesktop => "cli_with_desktop",
            PackageKind::DesktopWithCli => "desktop_with_cli",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "cli_only" => PackageKind::CliOnly,
            "cli_with_desktop" => PackageKind::CliWithDesktop,
            "desktop_with_cli" => PackageKind::DesktopWithCli,
            _ => PackageKind::Unknown,
        }
    }
}

/// 一个扫出来的 CLI 包。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CliPackage {
    /// 稳定 id：`"<provider>:<name>"`。同名、不同 provider 不会撞
    /// （`scoop:ripgrep` 与 `npm:ripgrep` 是两条）
    pub id: String,
    pub provider: ProviderId,
    /// 包名（`ripgrep` / `@larksuite/cli`）。provider 内部唯一
    pub name: String,
    pub version: Option<String>,
    /// 可更新到的版本。
    /// ⚠️ **阶段一永远是 `None`**（不查更新，那是阶段三）——字段留着是为了
    /// 阶段三不必迁移库表，界面读 `None` 时**不画"可更新"徽标**。
    #[serde(default)]
    pub latest: Option<String>,
    /// 安装目录（拿不到就是 `None`）
    #[serde(default)]
    pub install_path: Option<String>,
    /// 这个包提供的**命令名**（`rg` / `wenyan`）。
    /// ⚠️ 从包自己的 `bin` 元数据读，**不是从包名猜**——npm 实测
    /// `@wenyan-md/cli` 提供的是 `wenyan`、`@moonshot-ai/kimi-code` 提供的是 `kimi`
    #[serde(default)]
    pub executables: Vec<String>,
    #[serde(default)]
    pub kind: PackageKind,
}

/// 一次扫描的结果。
#[derive(Clone, Debug, Default)]
pub struct ScanReport {
    /// 扫到的全部包（已按 [`crate::cli::sort_packages`] 排好）
    pub packages: Vec<CliPackage>,
    /// 某个 provider 跑失败的一句话（`npm ls` 报错、超时……）。
    /// **不影响别的 provider**——一个挂了其余照常出，这句话在界面上当成一条提示
    pub errors: Vec<String>,
    /// 这一份是什么时候扫的（秒级时间戳）
    pub scanned_at: u64,
}
