//! CLI 工具扫描（**只读**）与阶段三的管理动作（升级 / 卸载 / 清理）。
//!
//! 扫本机的包管理器（winget / scoop / npm / pipx / cargo），列出装了哪些 CLI 工具、
//! 各自的版本、安装位置、提供的命令，并按 PE 子系统判一次**桌面关联**
//! （[`desktop`]，设计 §4 的两个高置信信号之一）。
//!
//! 阶段三起这里还管三件管理动作：**升级**（一条命令直接执行，设计 §6）、
//! **卸载**（[`plan`] 的 OpPlan + [`execute`] 串行跑完）、**清理**
//! （[`clean`]，只删内容不删目录，设计 §11）。
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
//! 查更新（[`check_updates`]）是**另一趟**：它要走网络，压在这一趟里就是
//! "进屏先等几秒"，所以扫描落地之后再由界面单独派发（与图标提取同一套语言：
//! 先渲染、后台再刷）。

mod cache;
mod clean;
mod exec;
mod model;
mod plan;
mod provider;
mod upgrade;

/// CLI 工具图标（阶段二第一件：从 exe 提图标 + 磁盘缓存）。
///
/// 这一档**不 re-export 到 `cli` 根**（别的模块都是 `pub use` 上来的）：
/// 它的两个入口叫 `ensure` / `cached_index`，摊到 `cli` 这一层就成了
/// `qingbird_core::cli::ensure(..)` —— 看不出是对谁 ensure。留着 `cli::icon::`
/// 这一段，读调用处的人不用回来看定义。
pub mod dirs;
pub mod icon;

/// 桌面关联检测（阶段二第三件：PE 子系统那一档高置信信号）。
///
/// 与 `icon` 同理**不 re-export 到 `cli` 根**：入口叫 `detect`，
/// 摊上去就成了 `qingbird_core::cli::detect(..)` —— 看不出在检测什么。
pub mod desktop;

/// 阶段三的执行器。**不摊到 `cli` 根**：入口 `run_plan` 摊上去就是
/// `cli::run_plan(..)`，看不出跑的是谁的计划；界面经 `cli::execute::run_plan`
/// 调它，计划那几格类型从 `cli::plan::` 来（根上那一层 `pub use` 是给它们的）。
/// `clean` 那一格相反——它只有执行器一个读者，留成私有模块
pub mod execute;

pub use cache::{load_cached, save_cached};
pub use model::{CliPackage, DirKind, PackageDir, PackageKind, ProviderId, ReleaseNotes, ScanReport};
pub use plan::{clean_plan, dependents_of, OpPlan, RiskLevel, Step, StepKind, UpdateInfo};
pub use provider::{provider_of, providers, PackageProvider};
/// 发行说明抓取（§3.11.1 就绪态"变更三统计 + 发布说明一行"，阶段四 #4）。
/// **只有 winget 有**——`winget show` 的输出里带发行说明；npm 没有可靠的
/// per-version changelog，不给别的 provider 编一份出来（GUI 侧据此不画）。
pub use provider::release_notes;
/// 阶段四 P0 的阶段化升级事件。**摊到 `cli` 根**：界面侧（`ops.rs` /
/// `state.rs`）每帧读的就是 `cli::UpgradeProgress` 那一格，入口
/// `cli::upgrade::run_upgrade` 留着那段路径——"跑一次升级"摊成
/// `cli::run_upgrade(..)` 看不出跑的是什么的升级
pub use upgrade::{
    run_upgrade, CancelToken, PhaseState, PhaseStatus, RollbackPoint, UpgradeOutcome,
    UpgradePhase, UpgradeProgress,
};

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
        // 共享缓存也在**同一条 provider 线程**里取：npm 那一档要再问它自己一次
        // （`npm config get cache`，设计 §8 第 2 级「包管理器文件清单」）。
        // 串在收集结果那一趟上会让它白白多等一条命令
        //
        // 桌面关联同样落在这条线程上：它要读盘（走一遍安装目录 + 每颗 exe 读 1 KB），
        // 串到收集那一趟就是"全部包排队等"——而 provider 之间本来就是并行的。
        // `shared_install_dir` 要在 `p` 被搬进线程**之前**问：那一档决定桌面关联
        // 敢不敢看整个目录（cargo 的 `$CARGO_HOME\bin` 里躺着别人的 exe）
        let shared_dir = p.shared_install_dir();
        handles.push((
            id,
            std::thread::spawn(move || {
                let mut listed = p.list();
                if let Ok(ps) = listed.as_mut() {
                    for pkg in ps.iter_mut() {
                        pkg.kind = desktop::detect(pkg, shared_dir);
                    }
                }
                (listed, p.cache_dirs())
            }),
        ));
    }

    let mut packages = Vec::new();
    let mut errors = Vec::new();
    let mut reported = Vec::new();
    for (id, h) in handles {
        match h.join() {
            Ok((Ok(mut ps), dirs)) => {
                packages.append(&mut ps);
                reported.extend(dirs);
            }
            Ok((Err(e), dirs)) => {
                errors.push(format!("{}：{e}", id.label()));
                // 列表挂了不代表它不知道自己的缓存在哪——自报的那份照样收
                reported.extend(dirs);
            }
            Err(_) => errors.push(format!("{}：扫描线程异常退出", id.label())),
        }
    }

    // 数据目录（设计 §8 的三级降）落到这里：规则库 + provider 自报 + 环境变量推断。
    // ⚠️ **体积这一趟不算**（`size` 全是 `None`，界面显「计算中…」）——走盘归界面
    // 那条后台通道（`ops::scan_cli`）：本机 npm 缓存 9.7 万个文件，压在这一趟里
    // 就是"进屏先等几秒"，而这一趟本来只该等几条命令
    let ctx = dirs::Ctx::live();
    for p in packages.iter_mut() {
        p.dirs = dirs::for_package(p, &ctx);
    }
    let shared = dirs::merge_shared(reported, dirs::shared(&ctx));

    sort_packages(&mut packages);
    ScanReport {
        packages,
        errors,
        scanned_at: now_secs(),
        shared,
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

/// 查一遍哪些包可更新（阶段三）。阻塞（要走网络）——**调用方放后台线程**。
///
/// 与 [`scan`] 分开的两趟：这一走 registry，慢且会失败；压进扫描那一趟就是
/// "进屏先等几秒"，而扫描本该只等几条本地命令。界面落地后单独派发
/// （与图标提取同一套语言：先渲染、后台再刷）。
///
/// provider 之间**线程并行**（与 `scan` 同构）；某个查不动 = 它的更新
/// 一律"不知道"（`latest` 留 `None`），**不进 `errors`**——查不到更新不是故障。
pub fn check_updates() -> Vec<plan::UpdateInfo> {
    let mut handles = Vec::new();
    for p in provider::providers() {
        // 不先 `detect()`：查更新这一步对没装的 provider 就是空表
        // （它们压根没覆写 `outdated`），而起 `where` 是白花时间
        handles.push(std::thread::spawn(move || p.outdated()));
    }
    let mut out = Vec::new();
    for h in handles {
        if let Ok(mut ups) = h.join() {
            out.append(&mut ups);
        }
    }
    // 不去重：`UpdateInfo::id` 带 provider 前缀（`npm:pnpm`），两个 provider
    // 报同一颗包的概率是零；真去重反而多一处"谁留谁不留"要解释
    out
}

/// 把查到的更新**合并进**包清单（按 [`CliPackage::id`] 对）。
///
/// 纯函数：哪些包、哪些更新都由调用方给，单测在假世界里钉"对得上才写"——
/// 对不上（provider 卸了/装了）就保持原样，**不按名字猜一个写上**
pub fn merge_updates(packages: &mut [CliPackage], updates: &[plan::UpdateInfo]) {
    for p in packages.iter_mut() {
        if let Some(u) = updates.iter().find(|u| u.id == p.id) {
            p.latest = Some(u.latest.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(id: &str, provider: ProviderId, name: &str) -> CliPackage {
        CliPackage {
        program_size: None,
        release_notes: None,
            id: id.into(),
            provider,
            name: name.into(),
            version: None,
            latest: None,
            install_path: None,
            executables: Vec::new(),
            kind: PackageKind::Unknown,
            dirs: Vec::new(),
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

    /// 合并查到的更新：**按 id 对，对不上就原样**。
    /// 判据是"对不上的不动"——按名字猜一个写上，屏幕就会多一个假的可更新徽标
    /// （那是让用户点一次升不动的升级）
    #[test]
    fn merge_updates_matches_by_id_only() {
        use crate::cli::plan::UpdateInfo;
        let mut ps = vec![
            pkg("npm:pnpm", ProviderId::Npm, "pnpm"),
            pkg("npm:ripgrep", ProviderId::Npm, "ripgrep"),
        ];
        merge_updates(
            &mut ps,
            &[
                UpdateInfo {
                    id: "npm:pnpm".into(),
                    latest: "11.6.1".into(),
                },
                // 清单里没有的这一条：不该把谁改掉
                UpdateInfo {
                    id: "npm:gone".into(),
                    latest: "9.9.9".into(),
                },
            ],
        );
        assert_eq!(ps[0].latest.as_deref(), Some("11.6.1"));
        assert_eq!(ps[1].latest, None, "没被点到的那颗保持原样");
    }
}
