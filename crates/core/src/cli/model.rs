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

/// 一颗工具（或一个包管理器）在盘上留下的目录是哪一类。
///
/// 四档的"可不可以自动删"来自设计 §8.3 那张安全级别表。**这一段现在唯一的用处
/// 是把那句话写到界面上去**（消费 `auto_deletable` 的是阶段三的卸载对话框），
/// 所以它得有一格字段承载、有一条判据钉着——不能只活在这段注释里。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum DirKind {
    /// 纯缓存：删了不影响功能，下次用会重建
    Cache,
    /// 配置：可能含登录信息，**不自动删**
    Config,
    /// 用户数据：删不可恢复，**不自动删**
    Data,
    /// **跨包共享**的缓存（不属于任何一颗包）。设计 §9：它不进卸载流，只上汇总条。
    /// ⚠️ 这一档**就是"共享"那个标记**——不再另开一个 `shared: bool`
    /// （同一件事两个出处，改一处漏一处的那种）
    SharedCache,
}

impl DirKind {
    /// 稳定键：落库用。**改它等于换一批记录**（与 `Op::key` 同一条纪律）
    pub fn key(self) -> &'static str {
        match self {
            DirKind::Cache => "cache",
            DirKind::Config => "config",
            DirKind::Data => "data",
            DirKind::SharedCache => "shared_cache",
        }
    }

    /// 从落库的键反查。认不出返回 `None`（库里存着下个版本写的键时跳过那一行）
    pub fn from_key(s: &str) -> Option<Self> {
        match s {
            "cache" => Some(DirKind::Cache),
            "config" => Some(DirKind::Config),
            "data" => Some(DirKind::Data),
            "shared_cache" => Some(DirKind::SharedCache),
            _ => None,
        }
    }

    /// 界面上显示的类别名
    pub fn label(self) -> &'static str {
        match self {
            DirKind::Cache => "缓存",
            DirKind::Config => "配置",
            DirKind::Data => "数据",
            DirKind::SharedCache => "共享缓存",
        }
    }

    /// 「卸载时顺手清理」可不可以默认勾上（设计 §8.3）。
    pub fn auto_deletable(self) -> bool {
        matches!(self, DirKind::Cache)
    }

    /// 界面上的那句话（安全级别）。**四档都有**——"可清理"也是要写给用户看的一句话。
    ///
    /// 抽成函数而不是写在界面里：**同一句话至少有两个落点**（右栏详情卡、
    /// 阶段三的卸载对话框），各写一遍就是两处会漂的东西
    pub fn note(self) -> &'static str {
        match self {
            DirKind::Cache => "可清理",
            DirKind::Config => "含登录信息，不自动删",
            DirKind::Data => "删除不可恢复，不自动删",
            DirKind::SharedCache => "跨包共用，不进卸载流程",
        }
    }

    /// 那句话要不要用警示色（只有"不能自动删"的两档要）
    pub fn note_is_warning(self) -> bool {
        matches!(self, DirKind::Config | DirKind::Data)
    }
}

/// 一颗工具（或一份共享缓存）在盘上的一个目录。
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct PackageDir {
    /// 绝对路径。给人看的那一份（界面直接显示它）
    pub path: String,
    pub kind: DirKind,
    /// 界面上给这一条起的名。**共享缓存那一栏要它**：`npm 缓存` 与 `pnpm store`
    /// 是两样东西，只按 provider 分组会把后者说成前者的。空 = 用调用方的默认名
    #[serde(default)]
    pub label: String,
    /// 体积。**`None` = 还没算**（界面显「计算中…」）。
    ///
    /// 大小是**惰性**算的，这不是优化洁癖：本机 npm 缓存 97k 个文件、走一圈要几秒，
    /// 压在列表那一趟里就是"进屏先等几秒"——而设计 §4 早就写成 `Option<u64>`
    #[serde(default)]
    pub size: Option<u64>,
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
    /// 这颗工具在盘上留下的目录（缓存 / 配置 / 数据）。
    ///
    /// ⚠️ **规则库里没写它，这里就是空的**——不按名字猜（`rules.toml` 头那两行
    /// 反例：`%LOCALAPPDATA%\feishu` 1.7 GB 是飞书客户端的数据，不是 `feishu-cli` 的）。
    /// 少报只是少一行；错报在阶段三就是删别人的东西
    #[serde(default)]
    pub dirs: Vec<PackageDir>,
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
    /// 共享缓存（provider 级，不属于任何一颗包）。设计 §9：它们只上右栏底部那条汇总
    pub shared: Vec<PackageDir>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 四档目录的**真值表**：稳定键往返、类别名、能不能自动删、界面那句话 +
    /// 那句话要不要警示色。
    ///
    /// 为什么值得一条单测：这四样东西会被**界面至少两个落点**读（右栏详情卡、
    /// 阶段三的卸载对话框），而它们全是从同一张表里分出来的——表里那一行错一格，
    /// 症状是"某一类目录被默认勾上了"（那是删用户东西的那一档）
    #[test]
    fn the_dir_kind_truth_table() {
        let all = [
            DirKind::Cache,
            DirKind::Config,
            DirKind::Data,
            DirKind::SharedCache,
        ];
        assert_eq!(all.len(), 4, "档数变了就把这张表补全");
        for k in all {
            // 落库的键必须往返得回来（认不出的键会让那一行被跳过 ⇒ 静默少一条）
            assert_eq!(DirKind::from_key(k.key()), Some(k), "{:?} 的键往返断了", k);
            assert!(!k.label().is_empty());
            assert!(!k.note().is_empty());
        }
        // 认不出的键是 None，不是 panic（老库/未来版本写进来的）
        assert_eq!(DirKind::from_key("nope"), None);

        // 只有纯缓存能自动删；另外三档都不许
        assert!(DirKind::Cache.auto_deletable());
        assert!(!DirKind::Config.auto_deletable());
        assert!(!DirKind::Data.auto_deletable());
        assert!(!DirKind::SharedCache.auto_deletable());

        // 警示色只给"不能自动删"的两档：配置与数据
        assert!(!DirKind::Cache.note_is_warning());
        assert!(DirKind::Config.note_is_warning());
        assert!(DirKind::Data.note_is_warning());
        assert!(!DirKind::SharedCache.note_is_warning());

        // 四句话必须**各不相同**：共用一句会让"含登录信息"这种要紧话
        // 出现在"可清理"那一行上
        let notes: std::collections::HashSet<&str> = all.iter().map(|k| k.note()).collect();
        assert_eq!(notes.len(), 4, "四档的话不该有重复：{notes:?}");
    }
}
