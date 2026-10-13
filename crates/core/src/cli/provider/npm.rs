//! npm 全局包（`npm ls -g --depth=0 --json`）。
//!
//! 输出是最干净的一种：一个根对象，全局包落在 `dependencies` 里。

use std::path::Path;

use super::{LIST_TIMEOUT, OUTDATED_TIMEOUT};
use crate::cli::exec;
use crate::cli::model::{CliPackage, DirKind, PackageDir, PackageKind, ProviderId};
use crate::cli::plan::{Step, UpdateInfo};
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

    /// npm 的缓存目录**问它自己**（设计 §8 第 2 级：包管理器文件清单）。
    ///
    /// 为什么不只靠 `rules.toml` 那条环境变量 + 默认值：`cache` 是 npm 的一个配置项，
    /// 用户能在 `.npmrc` 里改到别处去，那时默认路径不存在 ⇒ 规则那条落空 ⇒
    /// **屏幕上会少一整条 18 GB 的缓存**（本机 `npm config get cache` 实测就是默认那个，
    /// 所以两条路在这一台上给出同一个答案；差别只在改过配置的机器上）。
    ///
    /// 拿不到就返回空表 —— 规则库那一份照常生效，**不因为这条挂了就少报**
    fn cache_dirs(&self) -> Vec<PackageDir> {
        let path = exec::run_program("npm", &["config", "get", "cache"], LIST_TIMEOUT)
            .ok()
            .map(|o| o.stdout.trim().to_string())
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("undefined"))
            .filter(|s| Path::new(s).is_dir());
        match path {
            Some(path) => vec![PackageDir {
                path,
                kind: DirKind::SharedCache,
                // 名字留空：有名字的那一份在 `rules.toml`（合并时按路径去重，
                // 有名字的那条会留下来）。**同一句话不许有两个出处**
                label: String::new(),
                size: None,
            }],
            None => Vec::new(),
        }
    }

    /// 查哪些全局包有新版：`npm outdated -g --depth=0 --json`（设计 §6）。
    ///
    /// ⚠️ 与 `npm ls` 同一条纪律：**有可更新项时退出码非零但 JSON 完整**——
    /// 照样用 stdout，不拿退出码判成败。输出形状（上游 `lib/commands/outdated.js`
    /// 的 `--json` 分支）：`{"<包名>": {"current": …, "wanted": …, "latest": …}}`。
    fn outdated(&self) -> Vec<UpdateInfo> {
        let Ok(out) = exec::run_program(
            "npm",
            &["outdated", "-g", "--depth=0", "--json"],
            OUTDATED_TIMEOUT,
        ) else {
            return Vec::new();
        };
        let text = if out.stdout.trim().is_empty() {
            out.stderr.trim()
        } else {
            out.stdout.trim()
        };
        parse_npm_outdated(text)
    }

    /// 升级 = `npm update -g <包名>`（设计 §6：一条命令，不进 OpPlan）。
    fn upgrade_step(&self, pkg: &CliPackage) -> Option<Step> {
        Some(Step::uninstall("npm", &["update", "-g", &pkg.name]))
    }

    /// 卸载 = `npm uninstall -g <包名>`（设计 §6）。
    fn uninstall_step(&self, pkg: &CliPackage) -> Option<Step> {
        Some(Step::uninstall("npm", &["uninstall", "-g", &pkg.name]))
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
            // 数据目录归扫描协调器统一解析（`cli::dirs::for_package`），
            // provider 只管"有哪些包"——规则库不该有第二个出处
            dirs: Vec::new(),
        });
    }
    Ok(out)
}

/// `npm outdated -g --depth=0 --json` 的输出 → 可更新清单（阶段三）。
///
/// 纯解析，单测喂**本机真输出**（判据是真形态，不是我以为的形态）。
/// 形状：`{"<包名>": {"current": …, "wanted": …, "latest": …, …}}`。
/// 解析不动 / 不是对象 / 空输出 ⇒ 空表（"不知道谁可更新"，不是错误）。
pub(crate) fn parse_npm_outdated(text: &str) -> Vec<UpdateInfo> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(obj) = v.as_object() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, info) in obj {
        let latest = info
            .get("latest")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty());
        let current = info.get("current").and_then(|x| x.as_str());
        // current == latest 的那条不进清单：outdated 理论上只列真有新版的，
        // 但 npm 自己（跟着 node 走的那颗）偶尔会把同版本也列进来——
        // 进清单就是屏上多一个"可更新"而升完还是那个版本
        let (Some(latest), Some(current)) = (latest, current) else {
            continue;
        };
        if latest == current {
            continue;
        }
        out.push(UpdateInfo {
            // 键与 `CliPackage::id` 同形（`npm:<包名>`）：合并按它对，不按名字
            id: format!("{}:{}", ProviderId::Npm.key(), name),
            latest: latest.to_string(),
        });
    }
    out
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

    /// ⚠️ 这段是**本机真输出**（2026-10-14，`npm outdated -g --depth=0 --json`，
    /// PATH 上是 WorkBuddy 托管的那颗 node 22.22.2）。纯解析单测只能证明
    /// "我以为的形态解析得动"，证不了 npm 真的这么吐——本项目为这条交过学费。
    const REAL_OUTDATED: &str = r#"{
      "corepack": {
        "current": "0.34.6",
        "wanted": "0.36.0",
        "latest": "0.36.0",
        "dependent": "global",
        "location": "C:\\Users\\x\\node\\node_modules\\corepack"
      },
      "npm": {
        "current": "10.9.7",
        "wanted": "12.2.0",
        "latest": "12.2.0",
        "dependent": "global",
        "location": "C:\\Users\\x\\node\\node_modules\\npm"
      }
    }"#;

    #[test]
    fn parses_a_real_outdated_payload() {
        let ups = parse_npm_outdated(REAL_OUTDATED);
        assert_eq!(ups.len(), 2, "两个可更新包一条都不能少");
        let npm = ups.iter().find(|u| u.id == "npm:npm").expect("npm 应在");
        assert_eq!(npm.latest, "12.2.0");
        // 键与 `CliPackage::id` 同形：合并按 id 对，scoped 包名原样进键
        assert!(ups.iter().any(|u| u.id == "npm:corepack"));
    }

    /// `current == latest` 的那条不进清单：屏上多一个"可更新"而升完还是
    /// 那个版本，是一条骗人的徽标（npm 对自己偶尔会这样）
    #[test]
    fn an_already_current_entry_is_not_an_update() {
        let ups = parse_npm_outdated(
            r#"{"pnpm":{"current":"11.5.3","wanted":"11.5.3","latest":"11.5.3"}}"#,
        );
        assert!(ups.is_empty(), "同版本不该算可更新");
    }

    /// 坏输出 / 空 / 不是对象 ⇒ 空表（"不知道谁可更新"不是错误）
    #[test]
    fn unparsable_outdated_is_empty_not_an_error() {
        assert!(parse_npm_outdated("").is_empty());
        assert!(parse_npm_outdated("npm outdated crashed").is_empty());
        assert!(parse_npm_outdated("[1,2,3]").is_empty());
        // 一个可更新的都没有：npm 打 `{}`
        assert!(parse_npm_outdated("{}").is_empty());
    }
}
