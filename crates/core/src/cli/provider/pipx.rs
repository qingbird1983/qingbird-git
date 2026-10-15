//! pipx 已装应用（`pipx list --json`）。
//!
//! ⚠️ **设计稿写的 `pipx list --output json` 是另一种输出**，本轮去上游源码核过：
//! pipx 的 `list` 有两个 JSON 口——`--json` 给的是「`install-all` 会读回来的
//! 版本化快照」（`{venvs:{…}, pipx_spec_version}`，见上游 `pipx_metadata_file.py`
//! 的 `_RawSpecFile`），`--output json` 给的是一个 result envelope（另一种外壳）。
//! 这里用 **`--json`**：它是被别的命令往返读的那一份，最稳。
//!
//! 每个 venv 的形状（同样来自上游 `PipxMetadata::to_dict`）：
//! `venvs[<venv 名>].metadata.main_package.{package, package_version, apps, …}`。

use std::path::{Path, PathBuf};

use super::LIST_TIMEOUT;
use crate::cli::exec;
use crate::cli::model::{CliPackage, PackageKind, ProviderId};
use crate::cli::plan::Step;
use crate::cli::provider::PackageProvider;

pub struct Pipx;

impl PackageProvider for Pipx {
    fn id(&self) -> ProviderId {
        ProviderId::Pipx
    }

    fn detect(&self) -> bool {
        exec::command_exists("pipx")
    }

    fn list(&self) -> Result<Vec<CliPackage>, String> {
        let out = exec::run_program("pipx", &["list", "--json"], LIST_TIMEOUT)?;
        let text = if out.stdout.trim().is_empty() {
            out.stderr.trim()
        } else {
            out.stdout.trim()
        };
        if text.is_empty() {
            return Err(format!("没有输出（退出码 {:?}）", out.code));
        }
        let mut pkgs = parse_pipx_list(text)?;

        // 安装目录：JSON 里没有 venv 的绝对路径（键只是目录名），自己拼一份
        if let Some(root) = pipx_venvs_root() {
            for p in &mut pkgs {
                let dir = root.join(&p.name);
                if dir.is_dir() {
                    p.install_path = Some(dir.to_string_lossy().into_owned());
                }
            }
        }
        Ok(pkgs)
    }

    /// 升级 = `pipx upgrade <venv 名>`（设计 §6；venv 名就是包名那一格，
    /// `list` 的注释说明过：pipx 的 upgrade/uninstall 收的正是它）。
    ///
    /// ⚠️ **本机没装 pipx** ⇒ 命令形状照上游文档定的，没在本机跑过（同 scoop 那笔账）。
    fn upgrade_step(&self, pkg: &CliPackage) -> Option<Step> {
        Some(Step::uninstall("pipx", &["upgrade", &pkg.name]))
    }

    /// 卸载 = `pipx uninstall <venv 名>`（设计 §6）。同上，本机未验。
    fn uninstall_step(&self, pkg: &CliPackage) -> Option<Step> {
        Some(Step::uninstall("pipx", &["uninstall", &pkg.name]))
    }
}

/// `~\\.local\\pipx\\venvs`（`PIPX_HOME` 能覆盖）。不存在返回 `None`。
fn pipx_venvs_root() -> Option<PathBuf> {
    let base = std::env::var("PIPX_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("USERPROFILE")
                .ok()
                .map(|h| Path::new(&h).join(".local").join("pipx"))
        })?;
    let dir = base.join("venvs");
    dir.is_dir().then_some(dir)
}

/// `pipx list --json` 的输出 → 包清单。纯解析，单测喂**上游源码定的形态**。
pub(crate) fn parse_pipx_list(text: &str) -> Result<Vec<CliPackage>, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("无法解析 pipx 输出：{e}"))?;
    // 主形态：顶层 `venvs`。留一臂兼容 result envelope（`{"result":{"venvs":…}}`）
    let venvs = v.get("venvs").or_else(|| v.get("result").and_then(|r| r.get("venvs")));
    let Some(venvs) = venvs.and_then(|x| x.as_object()) else {
        // 一个 venv 都没装时 pipx 会警告并可能给空/无 venvs——当成"空"，不是错
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (name, entry) in venvs {
        let mp = entry.get("metadata").and_then(|m| m.get("main_package"));
        out.push(CliPackage {
            id: format!("{}:{}", ProviderId::Pipx.key(), name),
            provider: ProviderId::Pipx,
            // 用 venv 目录名当包名：pipx 的 upgrade/uninstall 收的正是它
            // （`main_package.package` 是 PyPI 包名，两者在带后缀安装时会不同）
            name: name.clone(),
            version: mp
                .and_then(|p| p.get("package_version"))
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            latest: None,
            install_path: None,
            executables: mp
                .and_then(|p| p.get("apps"))
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            kind: PackageKind::Unknown,
            // 数据目录由扫描协调器统一解析（`cli::dirs::for_package`）——
            // provider 只负责"有哪些包"，**规则库不该有第二个出处**
            program_size: None,
            release_notes: None,
            dirs: Vec::new(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 上游 `PipxMetadata::to_dict` 定的形态（截掉对本层无用的键）
    const REAL: &str = r#"{
      "venvs": {
        "black": {
          "metadata": {
            "main_package": {
              "package": "black",
              "package_version": "24.4.2",
              "apps": ["black", "blackd"],
              "app_paths": [["black"], ["blackd"]],
              "include_apps": true
            },
            "python_version": "3.11.5"
          }
        }
      },
      "pipx_spec_version": "0.1"
    }"#;

    #[test]
    fn parses_the_snapshot_shape_pipx_emits() {
        let ps = parse_pipx_list(REAL).unwrap();
        assert_eq!(ps.len(), 1);
        let b = &ps[0];
        assert_eq!(b.name, "black");
        assert_eq!(b.version.as_deref(), Some("24.4.2"));
        assert_eq!(b.executables, vec!["black".to_string(), "blackd".to_string()]);
        assert_eq!(b.id, "pipx:black");
        assert_eq!(b.provider, ProviderId::Pipx);
    }

    /// 一个 venv 都没有 ⇒ 空，不是错
    #[test]
    fn no_venvs_is_empty_not_an_error() {
        assert!(parse_pipx_list(r#"{"venvs":{},"pipx_spec_version":"0.1"}"#).unwrap().is_empty());
        // 连 venvs 键都没有（旧版/警告路径）
        assert!(parse_pipx_list(r#"{}"#).unwrap().is_empty());
    }

    /// result envelope 外壳也认（免得上游哪天把 `--json` 换成 envelope 就哑火）
    #[test]
    fn a_result_envelope_is_tolerated() {
        let wrapped = r#"{"result":{"venvs":{"ruff":{"metadata":{"main_package":{"package_version":"0.6.0","apps":["ruff"]}}}}}}"#;
        let ps = parse_pipx_list(wrapped).unwrap();
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].name, "ruff");
    }

    /// 版本空串要当成"没有版本"，不要画一个空版本
    #[test]
    fn blank_version_reads_as_none() {
        let t = r#"{"venvs":{"x":{"metadata":{"main_package":{"package_version":"","apps":[]}}}}}"#;
        assert_eq!(parse_pipx_list(t).unwrap()[0].version, None);
    }
}
