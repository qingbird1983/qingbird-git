//! 卸载残留审计（阶段四待建 #2）：卸载命令跑完之后，**它有没有卸干净**。
//!
//! 设计 §3.11.8 第 2 条的"执行后结论"：目录 / 注册表 / 命令入口三路枚举
//! （`.lnk` 读目标待拍板——windows crate 会触发宿主管道拦截、手搓二进制
//! 格式太脆、PowerShell COM 一行命令又是一条子进程路，登记未做）。
//!
//! 三条纪律：
//!
//! - **只提示，不删**。残留清理是用户的第二个决定（那一行给路径就够了，
//!   删不删、怎么删是后面的事）——审计自己动手删就越权了。
//! - **没卸成不审计**。取消 / 失败时残留是"没卸成"的证据，不是"卸完
//!   剩的"——两件事一个名字会把后者冤枉成前者（GUI 收尾只在 ok 时调用）。
//! - **注册表解析判据未在真机取证**（沙箱拦 `reg.exe`，本机 2026-10-15
//!   只用 PowerShell cmdlet 验过数据源语义：HKCU 48 + HKLM 217 子键，
//!   `DisplayName` 形态 = "名字 版本"）：`reg query /s /f` 的输出格式按
//!   公开文档构造测试样例，**真机首跑时要核对**（winget 安装侧关键词
//!   同等待遇）。

use std::path::Path;
use std::time::Duration;

use super::exec;
use super::model::{CliPackage, ProviderId};

/// `reg query` 全树搜两个 root 的预算。本机 265 个子键秒级，留宽到列表
/// 超时同一档（真机磁盘慢 / 杀软扫描都有可能拖它）
const REG_TIMEOUT: Duration = Duration::from_secs(30);

/// 残留的三种出处。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResidueKind {
    /// 安装目录还在（卸载器常见的"删了大部分、留下个壳"）
    Directory,
    /// 卸载注册表键还在（「应用与功能」里会继续显示这颗"幽灵"）
    Registry,
    /// 命令入口还能解析到（PATH / Shim 没清，终端里敲名字还能跑）
    PathEntry,
}

impl ResidueKind {
    pub fn label(&self) -> &'static str {
        match self {
            ResidueKind::Directory => "目录",
            ResidueKind::Registry => "注册表",
            ResidueKind::PathEntry => "命令入口",
        }
    }
}

/// 一条残留。`path` 是给人看的定位（目录绝对路径 / 注册表键路径 /
/// 入口名），`detail` 是那句补充（大小、DisplayName、在哪个 root）。
#[derive(Clone, Debug, PartialEq)]
pub struct ResidueItem {
    pub kind: ResidueKind,
    pub path: String,
    pub size: Option<u64>,
    pub detail: String,
}

/// 卸载一颗包之后的三路枚举。`pkg` 用**执行前快照**（重扫之后清单里
/// 这颗包已经没了，快照是它存在过的唯一凭证）。`exes` 是该包的命令
/// 入口名（`CliPackage::executables`）。
pub fn audit(pkg: &CliPackage, exes: &[String]) -> Vec<ResidueItem> {
    let mut items = Vec::new();

    // ① 目录残留：install_path 还在就是还在（空壳也报——"空"本身就是
    // 卸载器没扫干净的证据）。大小复用空间页那颗走盘器（同一条课）
    if let Some(dir) = pkg.install_path.as_deref() {
        let p = Path::new(dir);
        if p.is_dir() {
            let files = count_entries(p);
            let size = super::dirs::size_of(p);
            items.push(ResidueItem {
                kind: ResidueKind::Directory,
                path: dir.to_string(),
                size: Some(size),
                detail: format!("{} 个条目仍在磁盘上", files),
            });
        }
    }

    // ② 注册表残留：两个 root 的 Uninstall 树里 DisplayName 还含这个名字
    // （HKCU 当装 / HKLM 全机装——两个都查，装在哪边是装的时候决定的）。
    // 匹配判据用**包含**（"7-Zip 26.03 (x64)" 含 "7-Zip"）：审计是提示人
    // 核对的，误报一行（用户看一眼 DisplayName 就知道）比漏报便宜
    for (root, label) in REG_ROOTS {
        for hit in query_uninstall_keys(root, &pkg.name) {
            items.push(ResidueItem {
                kind: ResidueKind::Registry,
                path: hit.key_path,
                size: None,
                detail: format!("{label} · DisplayName = {}", hit.display_name),
            });
        }
    }

    // ③ 命令入口残留：`where <exe>` 仍命中 ⇒ PATH / Shim 上还挂着。
    // 每个入口各查各的（查一个就断会把第二个漏掉——卸载器清一半是常态）
    for exe in exes {
        if exec::command_exists(exe) {
            items.push(ResidueItem {
                kind: ResidueKind::PathEntry,
                path: exe.clone(),
                size: None,
                detail: "卸载后命令入口仍可解析（PATH / Shim 未清）".to_string(),
            });
        }
    }

    items
}

/// 目录条目计数（一层；读不动按 0——大小已经带出来了，条数不是关键证据）。
fn count_entries(p: &Path) -> usize {
    std::fs::read_dir(p).map(|rd| rd.filter_map(Result::ok).count()).unwrap_or(0)
}

/// 一条注册表命中：键路径 + DisplayName 值。
#[derive(Clone, Debug, PartialEq)]
pub struct RegHit {
    pub key_path: String,
    pub display_name: String,
}

/// `reg query` 的两个搜索 root（键路径形态，`reg` 吃反斜杠）：
/// 当装在 HKCU、全机装在 HKLM（WoW6432Node 是 32 位程序那一侧，
/// winget 装的现代 CLI 几乎不落那边——先不查，偏差同 `.lnk` 登记）。
const REG_ROOTS: [(&str, &str); 2] = [
    (
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        "HKCU",
    ),
    (
        r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        "HKLM",
    ),
];

/// 在一个 Uninstall 树里搜 DisplayName 含 `name` 的卸载键。
/// 起不来（reg 不在——几乎不可能）按"没查到"处理：审计的失败不该
/// 把卸载成功这件事搅成错误，界面上少一行注册表结果而已。
fn query_uninstall_keys(root: &str, name: &str) -> Vec<RegHit> {
    match exec::run_program(
        "reg",
        &["query", root, "/s", "/f", name, "/v", "DisplayName"],
        REG_TIMEOUT,
    ) {
        Ok(out) if out.ok() => parse_reg_query(&out.stdout),
        _ => Vec::new(),
    }
}

/// 解析 `reg query <root> /s /f <name> /v DisplayName` 的 stdout。
///
/// 输出形态（按公开文档构造，**未真机取证**）：
///
/// ```text
/// HKEY_CURRENT_USER\Software\...\Uninstall\{guid}
///     DisplayName    REG_SZ    Some App 1.2.3
///
/// End of search: 1 match(es) found
/// ```
///
/// 判据：`HKEY_` 顶格开头的行 = 当前键路径（记着，等 DisplayName 行来配对）；
/// 含 `REG_SZ` 且以 `DisplayName` 开头的行 = 值（REG_SZ 之后整段都是值，
/// 值里允许空格）。配不上的就丢——**解析器吃不出格式里没有的东西**。
fn parse_reg_query(text: &str) -> Vec<RegHit> {
    let mut hits = Vec::new();
    let mut key_path: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("HKEY_") {
            key_path = Some(t.to_string());
            continue;
        }
        if let Some(rest) = t.strip_prefix("DisplayName") {
            if let Some((_ty, value)) = rest.trim().split_once("REG_SZ") {
                if let (Some(kp), true) = (&key_path, value.contains(' ')) {
                    hits.push(RegHit {
                        key_path: kp.clone(),
                        display_name: value.trim_start().to_string(),
                    });
                }
            }
        }
    }
    hits
}

/// 给审计出的残留计数一句中文结论（空表 = 没发现，也是一句实话）。
pub fn verdict(items: &[ResidueItem]) -> String {
    if items.is_empty() {
        "没有发现残留".to_string()
    } else {
        let dirs = items.iter().filter(|i| i.kind == ResidueKind::Directory).count();
        let regs = items.iter().filter(|i| i.kind == ResidueKind::Registry).count();
        let paths = items.iter().filter(|i| i.kind == ResidueKind::PathEntry).count();
        let mut parts = Vec::new();
        if dirs > 0 {
            parts.push(format!("目录 {dirs}"));
        }
        if regs > 0 {
            parts.push(format!("注册表 {regs}"));
        }
        if paths > 0 {
            parts.push(format!("命令入口 {paths}"));
        }
        format!("发现残留：{}", parts.join("、"))
    }
}

/// winget 的包在注册表里登记的名字通常带版本后缀（"Git 2.55.0"），
/// 裸 `name` 也够用；npm / cargo 等一般没有注册表键，查了就是空表——
/// 不按 provider 分流（多一条分支就多一处要跟 provider 对齐的地方）。
pub fn audit_pkg(pkg: &CliPackage) -> Vec<ResidueItem> {
    if pkg.provider == ProviderId::Winget || !pkg.executables.is_empty() {
        audit(pkg, &pkg.executables)
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::model::PackageKind;

    fn pkg_named(name: &str) -> CliPackage {
        CliPackage {
            program_size: None,
            release_notes: None,
            id: format!("winget:{name}"),
            provider: ProviderId::Winget,
            name: name.to_string(),
            version: Some("1.0.0".into()),
            latest: None,
            install_path: None,
            executables: Vec::new(),
            kind: PackageKind::CliOnly,
            dirs: Vec::new(),
        }
    }

    /// reg query 的输出形态按公开文档构造（沙箱拦 reg.exe，真机首跑核对）。
    /// 样例刻意含三段：一个命中（值带空格与版本）、下一个键的头、收尾行。
    const SAMPLE: &str = "\
HKEY_LOCAL_MACHINE\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\7-Zip
    DisplayName    REG_SZ    7-Zip 26.03 (x64)

HKEY_LOCAL_MACHINE\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{guid-x}
    InstallLocation    REG_SZ    C:\\Program Files\\x

End of search: 1 match(es) found.
";

    #[test]
    fn parses_a_hit_and_skips_other_values() {
        let hits = parse_reg_query(SAMPLE);
        assert_eq!(hits.len(), 1, "InstallLocation 不是 DisplayName，不收");
        assert!(hits[0].key_path.ends_with("\\7-Zip"));
        assert_eq!(hits[0].display_name, "7-Zip 26.03 (x64)");
    }

    #[test]
    fn zero_matches_and_garbage_are_both_empty_not_errors() {
        assert!(parse_reg_query("End of search: 0 match(es) found.").is_empty());
        assert!(parse_reg_query("").is_empty());
        // 乱输出：没有 HKEY_ 头的 DisplayName 行没处挂，丢掉不炸
        assert!(parse_reg_query("DisplayName    REG_SZ    orphan").is_empty());
    }

    #[test]
    fn directory_residue_truth_table() {
        let base = std::env::temp_dir().join(format!("qb-residue-{}", std::process::id()));
        let dir = base.join("leftover");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"hello").unwrap();

        let mut p = pkg_named("leftover-app");
        p.install_path = Some(dir.to_string_lossy().into_owned());
        let items = audit(&p, &[]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, ResidueKind::Directory);
        assert_eq!(items[0].size, Some(5));
        assert!(items[0].detail.contains('1'), "一个条目");

        // 目录没了（卸载器删干净了）⇒ 这一路空手而归
        std::fs::remove_dir_all(&base).unwrap();
        let items = audit(&p, &[]);
        assert!(items.is_empty(), "目录不在就不报，不编一条");
    }

    /// 命令入口两向：真在 PATH 上的必中、幽灵必不中——不看机器下菜碟
    /// （`cmd` 在任何 Windows 上都在，`qb-no-such-*` 谁的机器上都不该有）。
    #[test]
    fn path_entry_hits_and_misses() {
        let p = pkg_named("cmd-shim-probe");
        let items = audit(&p, &["cmd".to_string(), "qb-no-such-program-42".to_string()]);
        let hits: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == ResidueKind::PathEntry)
            .map(|i| i.path.as_str())
            .collect();
        assert_eq!(hits, vec!["cmd"], "幽灵不命中，cmd 必命中");
    }

    #[test]
    fn verdict_counts_by_kind() {
        assert_eq!(verdict(&[]), "没有发现残留");
        let items = vec![
            ResidueItem {
                kind: ResidueKind::Directory,
                path: "C:\\x".into(),
                size: Some(1),
                detail: String::new(),
            },
            ResidueItem {
                kind: ResidueKind::PathEntry,
                path: "x".into(),
                size: None,
                detail: String::new(),
            },
        ];
        assert_eq!(verdict(&items), "发现残留：目录 1、命令入口 1");
    }
}
