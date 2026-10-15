//! CLI 扫描结果的缓存（阶段一：一张 `cli_packages` 表；阶段二：+ 两张目录表）。
//!
//! 与仓库缓存**同一份库文件**（`scan_cache::db_path`）、**同一条打开路径**
//! （`scan_cache::open_at`，它已经把建表与 `busy_timeout` 都做了）。
//! 另起一份打开逻辑就是第二处会漂的东西：一处改了 busy_timeout、另一处没有，
//! 而"偶尔丢一条"这种症状没人查得动。表结构也跟着 `scan_cache::SCHEMA` 走。
//!
//! 与仓库缓存**同一条约定：加速器不是依赖**——库打不开 / 表损坏 / 磁盘满
//! 一律降级：读返 `None`（当成"还没扫过"）、写返 `false`（当成"没记上"），
//! 一次都不 panic。
//!
//! ## 为什么目录单独两张表
//!
//! `scan_cache::SCHEMA` 里写了完整理由，一句话：**`CREATE TABLE IF NOT EXISTS`
//! 不给已存在的表加列**，而老用户库里那张 `cli_packages` 是阶段一建的。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::cli::model::{CliPackage, DirKind, PackageDir, PackageKind, ProviderId};

/// 库路径（与仓库缓存同一份）
fn db_path() -> Option<PathBuf> {
    crate::scan_cache::db_path()
}

/// 库里那一份（界面首屏用它先渲染）
pub struct Cached {
    pub packages: Vec<CliPackage>,
    /// 共享缓存（provider 级）
    pub shared: Vec<PackageDir>,
    /// 扫于何时（秒级，0 = 没扫过）
    pub scanned_at: u64,
}

/// 读缓存。`None` = 没有 / 读不出来（界面当成"还没扫过"，照常起一轮扫描）。
pub fn load_cached() -> Option<Cached> {
    load_cached_in(&db_path()?)
}

/// 一条 `size` 列 → `Option<u64>`（NULL = 没算过；负数/超界当没算，不 panic）
fn size_of(v: Option<i64>) -> Option<u64> {
    v.and_then(|s| u64::try_from(s).ok())
}

/// [`load_cached`] 的实现，**库文件路径由调用方给**。
///
/// 拆一层与 `scan_cache::open_at` / `store::save_to` 同一个理由：
/// 单测不许动进程级 `APPDATA`，判据要在**沙箱库文件**上真跑 rusqlite
pub(crate) fn load_cached_in(db: &Path) -> Option<Cached> {
    let conn = crate::scan_cache::open_at(db)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, provider, name, version, latest, install_path, kind, executables, scanned_at
             FROM cli_packages",
        )
        .ok()?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, i64>(8)?,
            ))
        })
        .ok()?;

    let mut out = Vec::new();
    let mut scanned_at = 0u64;
    for row in rows.flatten() {
        let (id, provider, name, version, latest, install_path, kind, executables, ts) = row;
        // 认不出的 provider 键跳过那一行，而不是 panic（老库/未来版本写进来的）
        let Some(provider) = ProviderId::from_key(&provider) else {
            continue;
        };
        scanned_at = scanned_at.max(ts.max(0) as u64);
        out.push(CliPackage {
            id,
            provider,
            name,
            version,
            latest,
            install_path,
            executables: serde_json::from_str(&executables).unwrap_or_default(),
            kind: PackageKind::from_key(&kind),
            // 下面一次全查回来再挂上去
            program_size: None,
            dirs: Vec::new(),
        });
    }
    drop(stmt);

    // 每颗工具的目录。**一次全查**（不分包）——本机 9 颗包、几十条目录，
    // 按包逐条查只是多几十次往返
    let mut by_pkg: HashMap<String, Vec<PackageDir>> = HashMap::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT package_id, path, kind, label, size FROM cli_package_dirs ORDER BY package_id, path",
    ) {
        if let Ok(rows) = stmt.query_map([], read_dir_row) {
            for (pid, dir) in rows.flatten() {
                by_pkg.entry(pid).or_default().push(dir);
            }
        }
    }
    for p in out.iter_mut() {
        if let Some(d) = by_pkg.remove(&p.id) {
            p.dirs = d;
        }
    }

    // 共享缓存
    let mut shared = Vec::new();
    if let Ok(mut stmt) =
        conn.prepare("SELECT path, kind, label, size FROM cli_shared_dirs ORDER BY path")
    {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<i64>>(3)?,
            ))
        }) {
            for (path, kind, label, size) in rows.flatten() {
                let Some(kind) = DirKind::from_key(&kind) else {
                    continue;
                };
                shared.push(PackageDir {
                    path,
                    kind,
                    label,
                    size: size_of(size),
                });
            }
        }
    }

    Some(Cached {
        packages: out,
        shared,
        scanned_at,
    })
}

/// 目录表的一行 → `(package_id, PackageDir)`。认不出的类别键跳过那一行
fn read_dir_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(String, PackageDir)> {
    let pid: String = r.get(0)?;
    let kind: String = r.get(2)?;
    let size: Option<i64> = r.get(4)?;
    Ok((
        pid,
        PackageDir {
            path: r.get(1)?,
            kind: DirKind::from_key(&kind).unwrap_or(DirKind::Cache),
            label: r.get(3)?,
            size: size_of(size),
        },
    ))
}

/// 写整份扫描结果（**覆盖**）。返回 false = 没记上，调用方不必在意。
///
/// 三个参数都必填（不留一个"从别处拿"的口子）：三者必须**同一次扫描**的
/// ——拼错一次就是"库里的目录跟包对不上"，而那种症状没人查得动
pub fn save_cached(packages: &[CliPackage], shared: &[PackageDir], scanned_at: u64) -> bool {
    match db_path() {
        Some(d) => save_cached_in(&d, packages, shared, scanned_at),
        None => false,
    }
}

/// [`save_cached`] 的实现，**库文件路径由调用方给**。
pub(crate) fn save_cached_in(
    db: &Path,
    packages: &[CliPackage],
    shared: &[PackageDir],
    scanned_at: u64,
) -> bool {
    let Some(mut conn) = crate::scan_cache::open_at(db) else {
        return false;
    };
    let Ok(tx) = conn.transaction() else {
        return false;
    };
    // **整表替换**：一次扫描就是一份快照，卸载掉的包必须从列表里消失，
    // 否则界面上会留着一个已经不在盘上的幽灵。三张表一起换
    // （目录先删：它是子表）
    for t in ["cli_package_dirs", "cli_shared_dirs", "cli_packages"] {
        if tx.execute(&format!("DELETE FROM {t}"), []).is_err() {
            return false;
        }
    }
    let ts = scanned_at as i64;
    for p in packages {
        let exes = serde_json::to_string(&p.executables).unwrap_or_else(|_| "[]".into());
        if tx
            .execute(
                "INSERT OR REPLACE INTO cli_packages
                   (id, provider, name, version, latest, install_path, kind, executables, scanned_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                rusqlite::params![
                    p.id,
                    p.provider.key(),
                    p.name,
                    p.version,
                    p.latest,
                    p.install_path,
                    p.kind.key(),
                    exes,
                    ts
                ],
            )
            .is_err()
        {
            return false;
        }
        for d in &p.dirs {
            if tx
                .execute(
                    "INSERT OR REPLACE INTO cli_package_dirs
                       (package_id, path, kind, label, size)
                     VALUES (?1,?2,?3,?4,?5)",
                    rusqlite::params![
                        p.id,
                        d.path,
                        d.kind.key(),
                        d.label,
                        d.size.map(|s| s as i64)
                    ],
                )
                .is_err()
            {
                return false;
            }
        }
    }
    for d in shared {
        if tx
            .execute(
                "INSERT OR REPLACE INTO cli_shared_dirs (path, kind, label, size, scanned_at)
                 VALUES (?1,?2,?3,?4,?5)",
                rusqlite::params![d.path, d.kind.key(), d.label, d.size.map(|s| s as i64), ts],
            )
            .is_err()
        {
            return false;
        }
    }
    tx.commit().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    // 夹具里用它当"一次扫描的三样东西"的袋子（生产那侧 `save_cached` 收三个参数，
    // 不再吃东西了 ⇒ 它只在测试里出现，就只在这里 import）
    use crate::cli::model::ScanReport;

    /// 沙箱库：不碰 `APPDATA`、不动进程级全局，目录带 tag + 序号 ⇒ 并行安全
    /// （AGENTS 那条"测试前提不许外包给环境"）
    fn sandbox(tag: &str) -> (PathBuf, PathBuf) {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("qb-cli-cache-{tag}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("cache.db");
        (dir, db)
    }

    fn dir(path: &str, kind: DirKind, size: Option<u64>) -> PackageDir {
        PackageDir {
            path: path.into(),
            kind,
            label: String::new(),
            size,
        }
    }

    fn sample() -> ScanReport {
        let mut report = ScanReport {
            scanned_at: 1_700_000_123,
            ..Default::default()
        };
        report.packages = vec![
            CliPackage {
            program_size: None,
                id: "npm:pnpm".into(),
                provider: ProviderId::Npm,
                name: "pnpm".into(),
                version: Some("11.5.3".into()),
                latest: None,
                install_path: Some("C:\\x\\pnpm".into()),
                executables: vec!["pnpm".into(), "pnpx".into()],
                kind: PackageKind::Unknown,
                // 一条**算过体积**的、一条**还没算**的
                dirs: vec![
                    dir("C:\\Users\\x\\AppData\\Local\\pnpm\\store", DirKind::Cache, Some(2831613952)),
                    dir("C:\\Users\\x\\.pnpmrc", DirKind::Config, None),
                ],
            },
            CliPackage {
            program_size: None,
                id: "scoop:ripgrep".into(),
                provider: ProviderId::Scoop,
                name: "ripgrep".into(),
                version: Some("14.1.1".into()),
                latest: None,
                install_path: None,
                executables: vec![],
                kind: PackageKind::Unknown,
                dirs: vec![],
            },
        ];
        report.shared = vec![PackageDir {
            path: "C:\\Users\\x\\AppData\\Local\\npm-cache".into(),
            kind: DirKind::SharedCache,
            label: "npm 缓存".into(),
            size: Some(19_782_123_520),
        }];
        report
    }

    #[test]
    fn packages_round_trip_through_the_database() {
        let (dir_path, db) = sandbox("roundtrip");
        let r = sample();
        assert!(save_cached_in(&db, &r.packages, &r.shared, r.scanned_at));
        let back = load_cached_in(&db).expect("刚写的却读不到");
        assert_eq!(back.scanned_at, 1_700_000_123);
        assert_eq!(back.packages.len(), 2);
        let pnpm = back.packages.iter().find(|p| p.name == "pnpm").unwrap();
        assert_eq!(pnpm.version.as_deref(), Some("11.5.3"));
        assert_eq!(pnpm.provider, ProviderId::Npm);
        assert_eq!(
            pnpm.executables,
            vec!["pnpm".to_string(), "pnpx".to_string()],
            "命令名列表要活过序列化（它是右栏「命令」那一格的数据）"
        );
        // 目录这一趟往返：**两类都要活下来**——算过的带体积、没算的还是 None
        assert_eq!(pnpm.dirs.len(), 2, "两条目录都要回来");
        let store = pnpm
            .dirs
            .iter()
            .find(|d| d.path.ends_with("store"))
            .expect("store 那条");
        assert_eq!(store.kind, DirKind::Cache);
        assert_eq!(store.size, Some(2_831_613_952));
        let rc = pnpm
            .dirs
            .iter()
            .find(|d| d.kind == DirKind::Config)
            .expect("配置那条");
        assert_eq!(rc.size, None, "没算过的要留成 None（界面据此显「计算中…」）");
        // 共享缓存那条：**名字是它的身份**（汇总条按名字认人）
        assert_eq!(back.shared.len(), 1);
        assert_eq!(back.shared[0].label, "npm 缓存");
        assert_eq!(back.shared[0].kind, DirKind::SharedCache);
        assert_eq!(back.shared[0].size, Some(19_782_123_520));
        let _ = std::fs::remove_dir_all(&dir_path);
    }

    /// **覆盖**而不是追加：写两次只有第二批。
    /// 追加的话卸载掉的包会永远留在列表里
    #[test]
    fn saving_twice_replaces_the_whole_snapshot() {
        let (dir_path, db) = sandbox("replace");
        let first = sample();
        assert!(save_cached_in(&db, &first.packages, &first.shared, first.scanned_at));
        let one = vec![first.packages.into_iter().next().unwrap()];
        assert!(save_cached_in(&db, &one, &[], 200));
        let back = load_cached_in(&db).unwrap();
        assert_eq!(back.packages.len(), 1, "第二批只有一条，旧的那条必须消失");
        assert_eq!(back.scanned_at, 200);
        assert!(
            back.shared.is_empty(),
            "这一批没有共享缓存 ⇒ 上一批那条也要消失（不然会是幽灵）"
        );
        let _ = std::fs::remove_dir_all(&dir_path);
    }

    /// 坏库要降级：读返 `None`、写返 `false`，不 panic
    #[test]
    fn a_corrupt_database_degrades_instead_of_panicking() {
        let (dir_path, db) = sandbox("corrupt");
        std::fs::write(&db, b"this is not a sqlite file at all").unwrap();
        assert!(load_cached_in(&db).is_none());
        let r = sample();
        assert!(!save_cached_in(&db, &r.packages, &r.shared, r.scanned_at));
        let _ = std::fs::remove_dir_all(&dir_path);
    }

    /// 空库（打得开、没扫过）读出来是空清单，不是 `None`——
    /// 界面据此区分"还没扫过"与"扫过、但一个 CLI 都没装"
    #[test]
    fn an_empty_but_readable_database_is_empty_not_none() {
        let (dir_path, db) = sandbox("empty");
        // 先建好表（open_at 会建），再读
        drop(crate::scan_cache::open_at(&db));
        let back = load_cached_in(&db).expect("空库该读得出来");
        assert!(back.packages.is_empty());
        assert!(back.shared.is_empty());
        assert_eq!(back.scanned_at, 0);
        let _ = std::fs::remove_dir_all(&dir_path);
    }

    /// **阶段一那份老库**（`cli_packages` 表，没有目录两张表）也要读得动：
    /// 这正是"两张新表而不是加列"要挡的那件事——给老表加列在 SQLite 上
    /// **不报错也不生效**，症状是"老用户永远看不到数据目录"
    #[test]
    fn a_stage_one_database_still_reads() {
        let (dir_path, db) = sandbox("stageone");
        {
            // 手搓一份阶段一模样的库：只有 cli_packages
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE cli_packages (
                    id TEXT PRIMARY KEY, provider TEXT NOT NULL, name TEXT NOT NULL,
                    version TEXT, latest TEXT, install_path TEXT,
                    kind TEXT NOT NULL DEFAULT '', executables TEXT NOT NULL DEFAULT '[]',
                    scanned_at INTEGER NOT NULL DEFAULT 0);
                 INSERT INTO cli_packages VALUES
                   ('npm:pnpm','npm','pnpm','11.5.3',NULL,NULL,'unknown','[\"pnpm\"]',170);",
            )
            .unwrap();
        }
        let back = load_cached_in(&db).expect("老库要读得动");
        assert_eq!(back.packages.len(), 1);
        assert_eq!(back.scanned_at, 170);
        assert!(
            back.packages[0].dirs.is_empty(),
            "老库里没有目录 ⇒ 空表（界面显「还没有发现」），不是读失败"
        );
        // 而且**写得进去**：open_at 会把两张新表建出来
        let r = sample();
        assert!(
            save_cached_in(&db, &r.packages, &r.shared, 300),
            "老库上要能写"
        );
        let again = load_cached_in(&db).unwrap();
        assert_eq!(again.packages[0].dirs.len(), 2, "老库升上来也该存得下目录");
        let _ = std::fs::remove_dir_all(&dir_path);
    }
}
