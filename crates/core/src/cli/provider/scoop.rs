//! scoop 已装应用（`scoop export`）。
//!
//! ⚠️ 形态是**去上游源码核过的**，不是猜的：`scoop export` 输出一个对象
//! `{ buckets: [...], apps: [...] }`，`apps` 里每项带
//! `Name / Version / Source / Updated / Info`（`libexec/scoop-export.ps1`
//! 调 `scoop-list.ps1` 产出）。**注意是对象不是数组**——1.0 草稿按数组写的。

use std::path::{Path, PathBuf};

use super::LIST_TIMEOUT;
use crate::cli::exec;
use crate::cli::model::{CliPackage, PackageKind, ProviderId};
use crate::cli::provider::PackageProvider;

pub struct Scoop;

impl PackageProvider for Scoop {
    fn id(&self) -> ProviderId {
        ProviderId::Scoop
    }

    fn detect(&self) -> bool {
        exec::command_exists("scoop")
    }

    fn list(&self) -> Result<Vec<CliPackage>, String> {
        let out = exec::run_program("scoop", &["export"], LIST_TIMEOUT)?;
        let text = if out.stdout.trim().is_empty() {
            out.stderr.trim()
        } else {
            out.stdout.trim()
        };
        if text.is_empty() {
            return Err(format!("没有输出（退出码 {:?}）", out.code));
        }
        let mut pkgs = parse_scoop_export(text)?;

        // 安装目录与命令名：scoop 的 app 装在 `~\scoop\apps\<name>\current`
        let apps_root = scoop_apps_root();
        for p in &mut pkgs {
            if let Some(root) = &apps_root {
                let dir = root.join(&p.name).join("current");
                if dir.is_dir() {
                    p.install_path = Some(dir.to_string_lossy().into_owned());
                    p.executables = app_bins(&dir);
                }
            }
        }
        Ok(pkgs)
    }
}

/// `~\scoop\apps`（用户级安装根）。不存在返回 `None`。
///
/// 不查全局安装（`%ProgramData%\scoop`）：那一档少见，先只覆盖用户级；
/// 真需要时在 `list` 里并一份进来即可（`install_path` 那一格已经能承载它）
fn scoop_apps_root() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE").ok()?;
    let dir = Path::new(&home).join("scoop").join("apps");
    dir.is_dir().then_some(dir)
}

/// `scoop export` 的输出 → 包清单。纯解析，单测喂**上游源码定的形态**。
pub(crate) fn parse_scoop_export(text: &str) -> Result<Vec<CliPackage>, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("无法解析 scoop 输出：{e}"))?;
    // 只为兼容旧写法留一臂：真有版本直接吐数组时也认
    let items = match &v {
        serde_json::Value::Array(a) => a.as_slice(),
        serde_json::Value::Object(_) => v
            .get("apps")
            .and_then(|a| a.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[]),
        _ => return Err("scoop 输出既不是对象也不是数组".into()),
    };
    let mut out = Vec::new();
    for it in items {
        let Some(name) = field(it, &["Name", "name"]) else {
            continue;
        };
        out.push(CliPackage {
            id: format!("{}:{}", ProviderId::Scoop.key(), name),
            provider: ProviderId::Scoop,
            name,
            version: field(it, &["Version", "version"]),
            latest: None,
            install_path: None,
            executables: Vec::new(),
            kind: PackageKind::Unknown,
            // 数据目录归扫描协调器统一解析（`cli::dirs::for_package`）
            dirs: Vec::new(),
        });
    }
    Ok(out)
}

/// 取第一个非空字符串字段（scoop 用大写 `Name`，留小写一档以防上游改）
fn field(v: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(|x| x.as_str()) {
            if !s.trim().is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// 一个 scoop app 提供的命令名。
///
/// 优先读它自己的 `manifest.json` 的 `bin`；没有就扫 app 目录里的可执行文件。
/// ⚠️ **不是从 app 名猜**——scoop 的 app 名与命令名经常对不上
/// （app `ripgrep` 提供的是 `rg`）
fn app_bins(app_dir: &Path) -> Vec<String> {
    if let Ok(text) = std::fs::read_to_string(app_dir.join("manifest.json")) {
        let names = bins_from_manifest(&text);
        if !names.is_empty() {
            return names;
        }
    }
    scan_executables(app_dir)
}

/// 纯解析：scoop manifest 的 `bin` 字段。
///
/// 三种形态：字符串、字符串数组、`[["cmd","path"], ...]` 的二元组数组。
pub(crate) fn bins_from_manifest(text: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match v.get("bin") {
        Some(serde_json::Value::String(s)) => out.push(s.clone()),
        Some(serde_json::Value::Array(a)) => {
            for it in a {
                match it {
                    // ["cmd", "path"] 那种：命令名是第一个元素
                    serde_json::Value::Array(pair) => {
                        if let Some(serde_json::Value::String(s)) = pair.first() {
                            out.push(s.clone());
                        }
                    }
                    serde_json::Value::String(s) => out.push(s.clone()),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    out.retain(|s| !s.trim().is_empty());
    out
}

/// 扫一个目录（含 `bin` 子目录一层）里的可执行文件，去扩展名当命令名。
/// 兜底用，破碎输出无害——只是多/少列几个命令名
fn scan_executables(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut push_dir = |d: &Path| {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            match p.extension().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()) {
                Some(ext) if matches!(ext.as_str(), "exe" | "cmd" | "bat" | "ps1") => {
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        out.push(stem.to_string());
                    }
                }
                _ => {}
            }
        }
    };
    push_dir(dir);
    push_dir(&dir.join("bin"));
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 上游 `scoop-export.ps1` 定死的形态：对象 + `apps` 数组
    const REAL: &str = r#"{
      "buckets": [{"Name":"main","Source":"https://github.com/ScoopInstaller/Main","Updated":"x"}],
      "apps": [
        {"Name":"ripgrep","Version":"14.1.1","Source":"main","Updated":"2024-01-01","Info":""},
        {"Name":"fd","Version":"10.2.0","Source":"main","Updated":"2024-01-01","Info":""}
      ]
    }"#;

    #[test]
    fn parses_the_object_shape_scoop_actually_emits() {
        let ps = parse_scoop_export(REAL).unwrap();
        assert_eq!(ps.len(), 2, "apps 里两条都要出");
        let rg = ps.iter().find(|p| p.name == "ripgrep").unwrap();
        assert_eq!(rg.version.as_deref(), Some("14.1.1"));
        assert_eq!(rg.id, "scoop:ripgrep");
        assert_eq!(rg.provider, ProviderId::Scoop);
    }

    /// 一个 app 都没装时 `apps` 是空数组 ⇒ 空，不是错
    #[test]
    fn empty_apps_is_empty_not_an_error() {
        assert!(parse_scoop_export(r#"{"buckets":[],"apps":[]}"#).unwrap().is_empty());
    }

    /// 兼容旧/意外形态：直接给数组也认（免得上游哪天改成数组就整台哑火）
    #[test]
    fn a_bare_array_is_tolerated() {
        let ps = parse_scoop_export(r#"[{"Name":"jq","Version":"1.7"}]"#).unwrap();
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].name, "jq");
    }

    /// manifest 的 `bin` 三种形态都要认
    #[test]
    fn manifest_bin_variants() {
        assert_eq!(bins_from_manifest(r#"{"bin":"rg.exe"}"#), vec!["rg.exe".to_string()]);
        assert_eq!(
            bins_from_manifest(r#"{"bin":["fd.exe","rg.exe"]}"#),
            vec!["fd.exe".to_string(), "rg.exe".to_string()]
        );
        assert_eq!(
            bins_from_manifest(r#"{"bin":[["rg","rg.exe"]]}"#),
            vec!["rg".to_string()],
            "二元组取命令名（第一个元素）"
        );
        assert!(bins_from_manifest(r#"{"version":"1.0"}"#).is_empty());
    }
}
