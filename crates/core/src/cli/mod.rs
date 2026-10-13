//! CLI 工具扫描（阶段一：只读）。
//!
//! 扫本机的包管理器（阶段一：scoop / npm / pipx），列出装了哪些 CLI 工具、
//! 各自的版本、安装位置、提供的命令。**只读**——升级 / 卸载 / 数据目录 /
//! 桌面关联 / 图标提取都是后续阶段的事。
//!
//! ## 为什么在 core 而不另起一个 crate
//!
//! 它要派生外部命令（`scoop export` / `npm ls -g` / `pipx list`）+ 读那份
//! SQLite 缓存，而这两件事 core 已经各有一套带纪律的实现：子进程一律加
//! `CREATE_NO_WINDOW`（复用 `git::CREATE_NO_WINDOW`）、缓存一律"打不开就降级、
//! 绝不 panic"。另起一个 crate 就等于把这两条纪律复制成第二份——改一处漏一处，
//! 而两边各自看着都对（本项目为"同一属性两个出处"交过四次学费）。
//!
//! ## 一次扫描长什么样
//!
//! [`scan`] 把每个**探测得到**的 provider 丢到一条线程上跑（它们各是一条外部
//! 命令、彼此无关），回来合并成一份 [`ScanReport`]。某个 provider 失败
//! **不影响**别的——它那句话进 `errors`，界面上当成一条提示。

mod cache;
mod exec;
mod model;
mod provider;

/// CLI 工具图标（阶段二第一件：从 exe 提图标 + 磁盘缓存）。
///
/// 这一档**不 re-export 到 `cli` 根**（别的模块都是 `pub use` 上来的）：
/// 它的两个入口叫 `ensure` / `cached_index`，摊到 `cli` 这一层就成了
/// `qingbird_core::cli::ensure(..)` —— 看不出是对谁 ensure。留着 `cli::icon::`
/// 这一段，读调用处的人不用回来看定义。
pub mod icon;

pub use cache::{load_cached, save_cached};
pub use model::{CliPackage, PackageKind, ProviderId, ScanReport};
pub use provider::{providers, PackageProvider};

use crate::store::now_secs;

/// 扫一遍本机的 CLI 工具。
///
/// 阻塞（要跑几条外部命令）——**调用方放后台线程**。
pub fn scan() -> ScanReport {
    let mut handles = Vec::new();
    for p in provider::providers() {
        // 探测放在**起线程之前**：没装的 provider 直接跳过，既省一条线程，
        // 也让 `where` 那几条探测串行做完（每条也就几毫秒）
        if !p.detect() {
            continue;
        }
        let id = p.id();
        handles.push((id, std::thread::spawn(move || p.list())));
    }

    let mut packages = Vec::new();
    let mut errors = Vec::new();
    for (id, h) in handles {
        match h.join() {
            Ok(Ok(mut ps)) => packages.append(&mut ps),
            Ok(Err(e)) => errors.push(format!("{}：{e}", id.label())),
            Err(_) => errors.push(format!("{}：扫描线程异常退出", id.label())),
        }
    }

    sort_packages(&mut packages);
    ScanReport {
        packages,
        errors,
        scanned_at: now_secs(),
    }
}

/// 列表的排序：**先按 provider 的固定顺序，再按名字**。
///
/// 与界面左栏的分组口径是同一份——分组按 provider 分，组内再按名字。
/// 收成一颗函数是为了让"provider 的先后"只有一个出处：界面拿
/// [`ProviderId::all`] 排组、这里拿它排包，改一处两处一起动。
pub(crate) fn sort_packages(ps: &mut [CliPackage]) {
    let order = |id: ProviderId| {
        ProviderId::all()
            .iter()
            .position(|p| *p == id)
            .unwrap_or(usize::MAX)
    };
    ps.sort_by(|a, b| {
        order(a.provider)
            .cmp(&order(b.provider))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(id: &str, provider: ProviderId, name: &str) -> CliPackage {
        CliPackage {
            id: id.into(),
            provider,
            name: name.into(),
            version: None,
            latest: None,
            install_path: None,
            executables: Vec::new(),
            kind: PackageKind::Unknown,
        }
    }

    /// 排序：**先按 provider 的固定顺序**（scoop < npm < pipx），组内再按名字。
    /// 名字比较**不分大小写**——否则 `Bun` 会排到 `awk` 后面
    #[test]
    fn packages_sort_by_provider_then_name() {
        let mut ps = vec![
            pkg("npm:zlib", ProviderId::Npm, "zlib"),
            pkg("scoop:jq", ProviderId::Scoop, "jq"),
            pkg("npm:Bun", ProviderId::Npm, "Bun"),
            pkg("pipx:black", ProviderId::Pipx, "black"),
            pkg("scoop:fd", ProviderId::Scoop, "fd"),
        ];
        sort_packages(&mut ps);
        let got: Vec<&str> = ps.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            got,
            vec!["scoop:fd", "scoop:jq", "npm:Bun", "npm:zlib", "pipx:black"]
        );
    }
}
