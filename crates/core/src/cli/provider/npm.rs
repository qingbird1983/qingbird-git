//! npm 全局包（`npm ls -g --depth=0 --json`）。
//!
//! 输出是最干净的一种：一个根对象，全局包落在 `dependencies` 里。

use std::path::Path;

use super::LIST_TIMEOUT;
use crate::cli::exec;
use crate::cli::model::{CliPackage, PackageKind, ProviderId};
use crate::cli::provider::PackageProvider;

pub struct Npm;

impl PackageProvider for Npm {
    fn id(&self) -> ProviderId {
        ProviderId::Npm
    }

    fn detect(&self) -> bool {
        exec::command_exists("npm")
    }

    fn list(&self) -> Result<Vec<CliPackage>, String> {
        let out = exec::run_program("npm", &["ls", "-g", "--depth=0", "--json"], LIST_TIMEOUT)?;
        // ⚠️ npm 在有 peer 依赖冲突时会**非零退出但照样打出完整 JSON**
        // ⇒ 只要 stdout 里有 JSON 就用它，不因为退出码把整台 provider 判失败
        let text = if out.stdout.trim().is_empty() {
            out.stderr.trim()
        } else {
            out.stdout.trim()
        };
        if text.is_empty() {
            return Err(format!("没有输出（退出码 {:?}）", out.code));
        }
        let mut pkgs = parse_npm_ls(text)?;

        // 安装目录与命令名要把盘读一圈：`npm root -g` 一次拿到所有包的父目录
        let root = exec::run_program("npm", &["root", "-g"], LIST_TIMEOUT)
            .ok()
            .map(|o| o.stdout.trim().to_string())
            .filter(|s| !s.is_empty());
        for p in &mut pkgs {
            if let Some(root) = &root {
                let dir = Path::new(root).join(&p.name);
                if dir.is_dir() {
                    p.install_path = Some(dir.to_string_lossy().into_owned());
                }
                p.executables = package_bins(&dir);
            }
        }
        Ok(pkgs)
    }
}

/// `npm ls -g --depth=0 --json` 的输出 → 包清单（名字 + 版本）。
///
/// 纯解析，单测直接喂本机抓下来的原文（判据是**真输出**，不是我以为的形态）。
pub(crate) fn parse_npm_ls(text: &str) -> Result<Vec<CliPackage>, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("无法解析 npm 输出：{e}"))?;
    let Some(deps) = v.get("dependencies").and_then(|d| d.as_object()) else {
        // 没有 `dependencies` = 一个全局包都没装（npm 只回了根对象 `{name:"npm"}`）。
        // **这是"空"，不是"错"**
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (name, info) in deps {
        out.push(CliPackage {
            id: format!("{}:{}", ProviderId::Npm.key(), name),
            provider: ProviderId::Npm,
            name: name.clone(),
            version: info
                .get("version")
                .and_then(|x| x.as_str())
                .map(str::to_string),
            latest: None,
            install_path: None,
            executables: Vec::new(),
            kind: PackageKind::Unknown,
        });
    }
    Ok(out)
}

/// 一个 npm 全局包提供的命令名，读它自己的 `package.json` 的 `bin`。
fn package_bins(pkg_dir: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(pkg_dir.join("package.json")) else {
        return Vec::new();
    };
    bins_from_package_json(&text, pkg_dir.file_name().and_then(|s| s.to_str()).unwrap_or(""))
}

/// 纯解析：从 `package.json` 的 `bin` 字段列命令名。
///
/// `bin` 有两种形态：字符串（只提供一个命令，名字取**包名末段**）或对象
/// （键就是命令名）。⚠️ **不从包名猜命令名**——实测 npm 的 `@wenyan-md/cli`
/// 提供的是 `wenyan`、`@moonshot-ai/kimi-code` 提供的是 `kimi`，包名与命令名
/// 经常对不上。
pub(crate) fn bins_from_package_json(text: &str, fallback_name: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    match v.get("bin") {
        Some(serde_json::Value::String(_)) => {
            let last = fallback_name
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(fallback_name);
            if last.is_empty() {
                Vec::new()
            } else {
                vec![last.to_string()]
            }
        }
        Some(serde_json::Value::Object(m)) => m.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠️ 这段是**本机真 npm 抓下来的原文**（2026-10-13，`npm ls -g --depth=0 --json`）。
    /// 纯解析单测只能证明"我以为的形态解析得动"，证不了"npm 真的这么吐"——
    /// 本项目为这条交过学费（`--date` 把 `%gd` 选择器改掉那一回）。
    const REAL: &str = r#"{
      "name": "npm",
      "dependencies": {
        "@larksuite/cli": { "version": "1.0.19", "overridden": false },
        "bun": { "version": "1.3.14", "overridden": false },
        "pnpm": { "version": "11.5.3", "overridden": false }
      }
    }"#;

    #[test]
    fn parses_a_real_npm_ls_payload() {
        let ps = parse_npm_ls(REAL).unwrap();
        assert_eq!(ps.len(), 3, "三个全局包一条都不能少");
        // 名字与版本按真输出对
        let pnpm = ps.iter().find(|p| p.name == "pnpm").expect("pnpm 应在");
        assert_eq!(pnpm.version.as_deref(), Some("11.5.3"));
        assert_eq!(pnpm.provider, ProviderId::Npm);
        assert_eq!(pnpm.id, "npm:pnpm");
        // scoped 包名要原样保留（含 @ 与 /）
        assert!(ps.iter().any(|p| p.name == "@larksuite/cli"));
    }

    /// 一个全局包都没装时 npm 只回根对象 ⇒ **是空，不是错**。
    /// 判成错的话界面上会挂一条"npm 扫描失败"，而其实什么都没坏
    #[test]
    fn no_dependencies_is_empty_not_an_error() {
        let ps = parse_npm_ls(r#"{"name":"npm"}"#).unwrap();
        assert!(ps.is_empty());
    }

    /// 坏 JSON 才是错——但错也是**这一台**的错，由协调器收进 errors
    #[test]
    fn malformed_json_is_an_error() {
        assert!(parse_npm_ls("npm ls crashed: EACCES").is_err());
    }

    /// `bin` 的对象形态：键就是命令名
    #[test]
    fn bin_object_yields_its_keys() {
        let names = bins_from_package_json(
            r#"{"name":"@wenyan-md/cli","bin":{"wenyan":"./bin/wenyan.js"}}"#,
            "cli",
        );
        assert_eq!(names, vec!["wenyan".to_string()]);
    }

    /// `bin` 的字符串形态：命令名取**包名末段**（不是包名整串）
    #[test]
    fn bin_string_uses_the_last_name_segment() {
        let names = bins_from_package_json(r#"{"bin":"./cli.js"}"#, "@scope/my-tool");
        assert_eq!(names, vec!["my-tool".to_string()]);
    }

    /// 没有 `bin`（纯库，如 `@wenyan-md/core`）⇒ 没有命令，不猜一个出来
    #[test]
    fn no_bin_means_no_commands() {
        assert!(bins_from_package_json(r#"{"name":"@wenyan-md/core"}"#, "core").is_empty());
    }
}
