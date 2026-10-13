//! CLI 扫描结果的缓存（阶段一：一张 `cli_packages` 表）。
//!
//! 与仓库缓存**同一份库文件**（`scan_cache::db_path`）、**同一条打开路径**
//! （`scan_cache::open_at`，它已经把建表与 `busy_timeout` 都做了）。
//! 另起一份打开逻辑就是第二处会漂的东西：一处改了 busy_timeout、另一处没有，
//! 而"偶尔丢一条"这种症状没人查得动。表结构也跟着 `scan_cache::SCHEMA` 走。
//!
//! 与仓库缓存**同一条约定：加速器不是依赖**——库打不开 / 表损坏 / 磁盘满
//! 一律降级：读返 `None`（当成"还没扫过"）、写返 `false`（当成"没记上"），
//! 一次都不 panic。

use std::path::{Path, PathBuf};

use crate::cli::model::{CliPackage, PackageKind, ProviderId};

/// 库路径（与仓库缓存同一份）
fn db_path() -> Option<PathBuf> {
    crate::scan_cache::db_path()
}

/// 读缓存。`None` = 没有 / 读不出来（界面当成"还没扫过"，照常起一轮扫描）。
///
/// 返回 `(包清单, 扫于何时)`——时间戳取全表最大值（每行都存同一个数）。
pub fn load_cached() -> Option<(Vec<CliPackage>, u64)> {
    load_cached_in(&db_path()?)
}

/// [`load_cached`] 的实现，**库文件路径由调用方给**。
///
/// 拆一层与 `scan_cache::open_at` / `store::save_to` 同一个理由：
/// 单测不许动进程级 `APPDATA`，判据要在**沙箱库文件**上真跑 rusqlite
pub(crate) fn load_cached_in(db: &Path) -> Option<(Vec<CliPackage>, u64)> {
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
        });
    }
    Some((out, scanned_at))
}

/// 写整份扫描结果（**覆盖**）。返回 false = 没记上，调用方不必在意。
pub fn save_cached(packages: &[CliPackage], scanned_at: u64) -> bool {
    match db_path() {
        Some(d) => save_cached_in(&d, packages, scanned_at),
        None => false,
    }
}

/// [`save_cached`] 的实现，**库文件路径由调用方给**。
pub(crate) fn save_cached_in(db: &Path, packages: &[CliPackage], scanned_at: u64) -> bool {
    let Some(mut conn) = crate::scan_cache::open_at(db) else {
        return false;
    };
    let Ok(tx) = conn.transaction() else {
        return false;
    };
    // **整表替换**：一次扫描就是一份快照，卸载掉的包必须从列表里消失，
    // 否则界面上会留着一个已经不在盘上的幽灵
    if tx.execute("DELETE FROM cli_packages", []).is_err() {
        return false;
    }
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
                    scanned_at as i64
                ],
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

    fn sample() -> Vec<CliPackage> {
        vec![
            CliPackage {
                id: "npm:pnpm".into(),
                provider: ProviderId::Npm,
                name: "pnpm".into(),
                version: Some("11.5.3".into()),
                latest: None,
                install_path: Some("C:\\x\\pnpm".into()),
                executables: vec!["pnpm".into(), "pnpx".into()],
                kind: PackageKind::Unknown,
            },
            CliPackage {
                id: "scoop:ripgrep".into(),
                provider: ProviderId::Scoop,
                name: "ripgrep".into(),
                version: Some("14.1.1".into()),
                latest: None,
                install_path: None,
                executables: vec![],
                kind: PackageKind::Unknown,
            },
        ]
    }

    #[test]
    fn packages_round_trip_through_the_database() {
        let (dir, db) = sandbox("roundtrip");
        assert!(save_cached_in(&db, &sample(), 1_700_000_123));
        let (back, ts) = load_cached_in(&db).expect("刚写的却读不到");
        assert_eq!(ts, 1_700_000_123);
        assert_eq!(back.len(), 2);
        let pnpm = back.iter().find(|p| p.name == "pnpm").unwrap();
        assert_eq!(pnpm.version.as_deref(), Some("11.5.3"));
        assert_eq!(pnpm.provider, ProviderId::Npm);
        assert_eq!(
            pnpm.executables,
            vec!["pnpm".to_string(), "pnpx".to_string()],
            "命令名列表要活过序列化（它是右栏「命令」那一格的数据）"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **覆盖**而不是追加：写两次只有第二批。
    /// 追加的话卸载掉的包会永远留在列表里
    #[test]
    fn saving_twice_replaces_the_whole_snapshot() {
        let (dir, db) = sandbox("replace");
        assert!(save_cached_in(&db, &sample(), 100));
        let one = vec![sample().into_iter().next().unwrap()];
        assert!(save_cached_in(&db, &one, 200));
        let (back, ts) = load_cached_in(&db).unwrap();
        assert_eq!(back.len(), 1, "第二批只有一条，旧的那条必须消失");
        assert_eq!(ts, 200);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 坏库要降级：读返 `None`、写返 `false`，不 panic
    #[test]
    fn a_corrupt_database_degrades_instead_of_panicking() {
        let (dir, db) = sandbox("corrupt");
        std::fs::write(&db, b"this is not a sqlite file at all").unwrap();
        assert!(load_cached_in(&db).is_none());
        assert!(!save_cached_in(&db, &sample(), 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 空库（打得开、没扫过）读出来是空清单，不是 `None`——
    /// 界面据此区分"还没扫过"与"扫过、但一个 CLI 都没装"
    #[test]
    fn an_empty_but_readable_database_is_empty_not_none() {
        let (dir, db) = sandbox("empty");
        // 先建好表（open_at 会建），再读
        drop(crate::scan_cache::open_at(&db));
        let (back, ts) = load_cached_in(&db).expect("空库该读得出来");
        assert!(back.is_empty());
        assert_eq!(ts, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
