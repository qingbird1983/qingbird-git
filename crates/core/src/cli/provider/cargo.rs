//! cargo 装出来的命令行工具（读 `$CARGO_HOME` 下那两本账）。
//!
//! ⚠️ 形态是**照本机真文件核过的**（`C:\Users\<user>\.cargo\.crates2.json`，2026-10-13）：
//!
//! ```json
//! {"installs":{"cargo-audit 0.22.2 (registry+https://github.com/rust-lang/crates.io-index)":
//!   {"version_req":null,"bins":["cargo-audit.exe"], …}}}
//! ```
//!
//! ——**包名与版本都挤在键里**（`<name> <version> (<source>)`），没有各自的字段；
//! `bins` 是它装出来的命令（带 `.exe`）。同一目录还有一份 v1 的 `.crates.toml`
//! （`[v1]` 段 + 同样形状的键 → 命令数组），老版本 cargo 只写它。
//!
//! **为什么不去问 `cargo install --list`**：那是给人看的文本（`name v0.1.0:` 若干行
//! 缩进的 exe），而这本账是 cargo 自己往返读的机器格式（`cargo install --list`
//! 读的也是它）。设计 §6 写的就是读 `.crates2.json`。

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::cli::model::{CliPackage, PackageKind, ProviderId};
use crate::cli::plan::Step;
use crate::cli::provider::PackageProvider;

/// cargo ≥ 1.42 写的账本（JSON，多几格安装时的 profile / target 细节）
const CRATES_V2: &str = ".crates2.json";
/// 更早版本写的账本（TOML，只有"键 → 命令"）
const CRATES_V1: &str = ".crates.toml";

pub struct Cargo;

impl PackageProvider for Cargo {
    fn id(&self) -> ProviderId {
        ProviderId::Cargo
    }

    /// **只读盘、不起子进程**：这颗 provider 的全部数据都来自 `$CARGO_HOME` 下那两本账，
    /// 账本不在就是"没 `cargo install` 过东西"——那正是"没有可列的包"。
    /// 为一个空表去起 `where cargo` 走一圈 PATH 是白花时间（装了 cargo 但一个包没装
    /// 的机器，列表本来也是空的）
    fn detect(&self) -> bool {
        let Some(home) = cargo_home() else {
            return false;
        };
        home.join(CRATES_V2).is_file() || home.join(CRATES_V1).is_file()
    }

    fn list(&self) -> Result<Vec<CliPackage>, String> {
        let Some(home) = cargo_home() else {
            return Ok(Vec::new());
        };
        let v2 = home.join(CRATES_V2);
        let mut pkgs = match std::fs::read_to_string(&v2) {
            Ok(text) => parse_crates2(&text)?,
            // 账本不在 = 空表（见 `detect`），**不是错**
            Err(e) if e.kind() == ErrorKind::NotFound => {
                let v1 = home.join(CRATES_V1);
                match std::fs::read_to_string(&v1) {
                    Ok(text) => parse_crates_toml(&text)?,
                    Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
                    Err(e) => return Err(format!("读不到 {}：{e}", v1.display())),
                }
            }
            Err(e) => return Err(format!("读不到 {}：{e}", v2.display())),
        };

        // 安装目录：cargo 把**所有** `cargo install` 出来的 exe 都放进这一格
        // （不是每包一个目录 ⇒ `shared_install_dir` 那个标记要立起来，见 trait 的注释）
        let bin = home.join("bin");
        if bin.is_dir() {
            let dir = bin.to_string_lossy().into_owned();
            for p in &mut pkgs {
                p.install_path = Some(dir.clone());
            }
        }
        Ok(pkgs)
    }

    /// 见 [`PackageProvider::shared_install_dir`]：cargo 的 exe 全在共用的 `$CARGO_HOME\bin`
    fn shared_install_dir(&self) -> bool {
        true
    }

    /// 卸载 = `cargo uninstall <包名>`（设计 §6）。**本机验过**：
    /// `cargo uninstall` 读同一本账（`.crates2.json`），卸完那一条自己消失。
    fn uninstall_step(&self, pkg: &CliPackage) -> Option<Step> {
        Some(Step::uninstall("cargo", &["uninstall", &pkg.name]))
    }

    // ⚠️ **不覆写 `upgrade_step`**（设计稿的表格写 `cargo install <n>`，这里不接）：
    // ① 上游 `cargo install` 对**已装**的包不是升级——它报 "already installed"
    //    （`cargo install-update` 这个第三方工具存在的原因正是这个），照设计稿写
    //    一条升不动的命令，屏上就是一个点了没反应的键；
    // ② 真正的升级（`cargo install --force <n>`）每次全量重编，而我们又没有
    //    `outdated` 那一格 ⇒ 没有 `latest` 的升级键同样是一条骗人的键
    //    （v4 铁律：不演拿不到的效果）。真要做由他拍。
}

/// `$CARGO_HOME`，没设就退 `~\.cargo`（cargo 自己的规矩）。两边都不认识返回 `None`。
fn cargo_home() -> Option<PathBuf> {
    let from_env = std::env::var("CARGO_HOME")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    from_env.or_else(|| {
        std::env::var("USERPROFILE")
            .ok()
            .map(|h| Path::new(&h).join(".cargo"))
    })
}

/// `.crates2.json` 的输出 → 包清单。纯解析，单测喂**本机真文件**。
pub(crate) fn parse_crates2(text: &str) -> Result<Vec<CliPackage>, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("无法解析 .crates2.json：{e}"))?;
    let Some(installs) = v.get("installs").and_then(|i| i.as_object()) else {
        // 没有 `installs` = 一本空账（cargo 真写的是 `{"installs":{}}`）。
        // **这是"空"，不是"错"**
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (spec, info) in installs {
        let Some((name, version)) = split_spec(spec) else {
            continue;
        };
        let bins = info
            .get("bins")
            .and_then(|b| b.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str()).map(command_name).collect())
            .unwrap_or_default();
        out.push(pkg(name, version, bins));
    }
    Ok(out)
}

/// `.crates.toml`（v1 账本）→ 包清单。纯解析。
pub(crate) fn parse_crates_toml(text: &str) -> Result<Vec<CliPackage>, String> {
    let v: toml::Value =
        toml::from_str(text).map_err(|e| format!("无法解析 .crates.toml：{e}"))?;
    let Some(v1) = v.get("v1").and_then(|x| x.as_table()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (spec, bins) in v1 {
        let Some((name, version)) = split_spec(spec) else {
            continue;
        };
        let bins = bins
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str()).map(command_name).collect())
            .unwrap_or_default();
        out.push(pkg(name, version, bins));
    }
    Ok(out)
}

/// 账本的键 → `(包名, 版本)`。键的形状是 `<name> <version> (<source>)`。
///
/// ⚠️ 两处都不能贪方便：① 源那段**可能含空格**（`(path+file:///C:/my project/x)`），
/// 所以按**第一个** ` (` 切掉源——按最后一个切会把路径里的空格切进版本里；
/// ② 名字里没有空格（crate 名是标识符），版本也没空格，所以剩下的头两段就是
/// 名字与版本，`splitn(2)` 后面全归版本
fn split_spec(spec: &str) -> Option<(String, Option<String>)> {
    let spec = spec.trim();
    let head = match spec.find(" (") {
        Some(i) => &spec[..i],
        None => spec,
    };
    let mut it = head.splitn(2, ' ');
    let name = it.next()?.trim();
    if name.is_empty() {
        return None;
    }
    let version = it.next().map(str::trim).filter(|s| !s.is_empty());
    Some((name.to_string(), version.map(str::to_string)))
}

/// 账本里 `bins` 那一项 → 命令名（`cargo-audit.exe` → `cargo-audit`）。
///
/// 去掉扩展名是为了与别的 provider 同形（npm 给的是 `pnpm`），
/// 也让 [`crate::cli::icon`] 拿它跟 exe 的文件名词干对齐
fn command_name(bin: &str) -> String {
    let bin = bin.trim();
    match bin.len().checked_sub(4) {
        Some(i) if bin[i..].eq_ignore_ascii_case(".exe") => bin[..i].to_string(),
        _ => bin.to_string(),
    }
}

fn pkg(name: String, version: Option<String>, bins: Vec<String>) -> CliPackage {
    CliPackage {
        id: format!("{}:{}", ProviderId::Cargo.key(), name),
        provider: ProviderId::Cargo,
        name,
        version,
        latest: None,
        install_path: None,
        executables: bins,
        // 桌面关联归扫描协调器统一算（`cli::desktop`）——provider 只管"有哪些包"
        kind: PackageKind::Unknown,
        program_size: None,
        release_notes: None,
        dirs: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠️ 本机真文件（2026-10-13，`C:\Users\42527\.cargo\.crates2.json`）。
    /// **只删了每颗包那一段 `rustc` 长文本**（解析不读它，留着这一格只是为了
    /// 证明真文件里还有我们不读的字段也不影响）；四个包、键的形状、`bins`
    /// 都是一个字节没改的
    const REAL_V2: &str = r#"{"installs":{
      "cargo-audit 0.22.2 (registry+https://github.com/rust-lang/crates.io-index)":{"version_req":null,"bins":["cargo-audit.exe"],"features":[],"all_features":false,"no_default_features":false,"profile":"release","target":"x86_64-pc-windows-msvc"},
      "cargo-llvm-cov 0.8.7 (registry+https://github.com/rust-lang/crates.io-index)":{"version_req":null,"bins":["cargo-llvm-cov.exe"],"features":[],"all_features":false,"no_default_features":false,"profile":"release","target":"x86_64-pc-windows-msvc"},
      "cargo-tarpaulin 0.37.0 (registry+https://github.com/rust-lang/crates.io-index)":{"version_req":null,"bins":["cargo-tarpaulin.exe"],"features":[],"all_features":false,"no_default_features":false,"profile":"release","target":"x86_64-pc-windows-msvc"},
      "resvg 0.47.0 (registry+https://github.com/rust-lang/crates.io-index)":{"version_req":null,"bins":["resvg.exe"],"features":[],"all_features":false,"no_default_features":false,"profile":"release","target":"x86_64-pc-windows-msvc"}}}"#;

    /// ⚠️ 同目录那本 v1 账，**逐字照抄**（没用 `rustc` 那种可省字段，它本来只有这些）
    const REAL_V1: &str = r#"[v1]
"cargo-audit 0.22.2 (registry+https://github.com/rust-lang/crates.io-index)" = ["cargo-audit.exe"]
"cargo-llvm-cov 0.8.7 (registry+https://github.com/rust-lang/crates.io-index)" = ["cargo-llvm-cov.exe"]
"cargo-tarpaulin 0.37.0 (registry+https://github.com/rust-lang/crates.io-index)" = ["cargo-tarpaulin.exe"]
"resvg 0.47.0 (registry+https://github.com/rust-lang/crates.io-index)" = ["resvg.exe"]
"#;

    #[test]
    fn parses_a_real_crates2_account() {
        let ps = parse_crates2(REAL_V2).unwrap();
        assert_eq!(ps.len(), 4, "四颗包一条都不能少");
        let resvg = ps.iter().find(|p| p.name == "resvg").expect("resvg 应在");
        // 版本是从**键**里拆出来的（文件里没有单独的版本字段）
        assert_eq!(resvg.version.as_deref(), Some("0.47.0"));
        assert_eq!(resvg.id, "cargo:resvg");
        assert_eq!(resvg.provider, ProviderId::Cargo);
        // `bins` 去掉 `.exe` 才是命令名（与 npm/scoop 的那些同形）
        assert_eq!(resvg.executables, vec!["resvg".to_string()]);
        assert_eq!(
            ps.iter()
                .find(|p| p.name == "cargo-llvm-cov")
                .unwrap()
                .executables,
            vec!["cargo-llvm-cov".to_string()]
        );
    }

    /// v1 那本（只有键 → 命令数组）读出来要跟 v2 同形：这也是"老账本还在写"那一档的判据
    #[test]
    fn parses_the_v1_toml_account() {
        let ps = parse_crates_toml(REAL_V1).unwrap();
        assert_eq!(ps.len(), 4);
        let audit = ps.iter().find(|p| p.name == "cargo-audit").unwrap();
        assert_eq!(audit.version.as_deref(), Some("0.22.2"));
        assert_eq!(audit.executables, vec!["cargo-audit".to_string()]);
    }

    /// 空账 / 空表 ⇒ **空，不是错**（判成错的话界面上会挂一条"cargo 扫描失败"）
    #[test]
    fn an_empty_account_is_empty_not_an_error() {
        assert!(parse_crates2(r#"{"installs":{}}"#).unwrap().is_empty());
        assert!(parse_crates2("{}").unwrap().is_empty());
        assert!(parse_crates_toml("[v1]\n").unwrap().is_empty());
        assert!(parse_crates_toml("").unwrap().is_empty());
    }

    /// 坏文件才是错——错由协调器收进 errors，**不影响别的 provider**
    #[test]
    fn malformed_accounts_are_errors() {
        assert!(parse_crates2("not json at all").is_err());
        assert!(parse_crates_toml("[[[").is_err());
        assert!(parse_crates_toml("[v1]\n\"a\" = ").is_err());
    }

    /// 键的两种切法：带源（且源里含空格）与不带源
    #[test]
    fn spec_splitting_handles_paths_with_spaces() {
        // ① 源那段含空格：按**第一个** ` (` 切，路径里的空格不许跑进版本
        assert_eq!(
            split_spec("my-tool 0.1.0 (path+file:///C:/my project/x)"),
            Some(("my-tool".to_string(), Some("0.1.0".to_string())))
        );
        // ② 没写源（老账本的意外形态）：名字之后全是版本
        assert_eq!(
            split_spec("ripgrep 14.1.1"),
            Some(("ripgrep".to_string(), Some("14.1.1".to_string())))
        );
        // ③ 名字就这么大（没有版本）：不算错，只是没版本
        assert_eq!(split_spec("solo"), Some(("solo".to_string(), None)));
        // ④ 空键 / 只有空格 ⇒ 跳过这一行
        assert_eq!(split_spec("   "), None);
    }

    /// `command_name` 只吃扩展名，不吃别的（`foo.exe.exe` 那种不许一次削两层）
    #[test]
    fn command_name_only_strips_one_extension() {
        assert_eq!(command_name("cargo-audit.exe"), "cargo-audit");
        assert_eq!(command_name("Cargo-Audit.EXE"), "Cargo-Audit");
        assert_eq!(command_name("cargo-audit"), "cargo-audit");
        assert_eq!(command_name("foo.exe.exe"), "foo.exe");
        // 比 `.exe` 还短：`checked_sub` 那一步不许下溢
        assert_eq!(command_name("exe"), "exe");
    }
}
