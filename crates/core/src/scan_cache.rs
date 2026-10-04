//! 扫描结果缓存 + 操作历史（阶段 16）。
//!
//! ⚠️ **别叫它 `history.rs`**——那个名字已经被"提交历史"模块占了
//! （`git log` 的提交列表 / 详情 / patch）。2026-10-05 差点覆盖掉它，
//! `write` 工具在结果里明明白白写了 `overwrote existing file`，是我漏看。
//! **覆盖已有文件时那句提示要看**——它是唯一的免费保险。
//!
//! ## 为什么是 SQLite
//!
//! 最初我建议用 JSON：数据量小（17 个仓库 × 几百 KB），纯读缓存
//! `serde_json` 解析只要 5–15ms，而引入 SQLite 会给这个纯 Rust 栈
//! **加上唯一的 C 依赖**。用户明确要「记录这个仓库历史、更新了什么」
//! 之后结论反转——那是从「覆盖写一条」变成**持续追加**，JSON 会越攒越大、
//! 越来越难维护，那是 SQLite 的主场。
//!
//! ## 它是加速器，不是依赖
//!
//! **所有函数在库打不开 / 表损坏 / 磁盘满时一律降级**：
//! 读返回 `None`（当成"没扫过"），写返回 `false`（当成"没记上"）。
//! 一次都不让它 panic、也不让调用方为它写错误分支——
//! 判据是「缓存丢了顶多慢一点」，不该是「应用起不来」。
//!
//! ## 两张表
//!
//! - `scan_cache`：每个仓库一行，**覆盖**写。启动读它上屏，页面秒稳
//! - `op_history`：**追加**，永不覆盖。回答"这个仓库被更新过什么"
//!
//! ## 为什么不把 README 单独存成文件
//!
//! 这一阶段一开始写的是「README 存 `%APPDATA%` 下的文本文件，不进库」，
//! 理由是「大段纯文本进数据库只让读写都变慢」。**那个理由不成立**：
//! 读写一样快，却要多一套文件生命周期（建目录、清孤儿、并发写、编码处理）。
//! 统一进库，少一处会出错的地方。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::repos::{ActivityDay, LangStat, ReadmeAlt};
use crate::run_cmd::CmdDetect;
use crate::space::SpaceReport;
use crate::types::RepoStatus;

/// 一个仓库的扫描结果快照。**只放 core 的类型**——GUI 那些
/// `ProfileState` / `SpaceState` 里带着 `SharedString` 之类 gpui 的东西，
/// 塞不进这里也不该塞。界面拿缓存去填自己的状态。
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct RepoCache {
    /// git 状态：分支 / 领先落后 / 是否脏 / 上游
    #[serde(default)]
    pub status: RepoStatus,
    /// 未提交文件数
    #[serde(default)]
    pub changed: u32,
    /// 最后一次提交的秒级时间戳
    #[serde(default)]
    pub last_commit_ts: u64,
    /// 自动识别出来的四条命令
    #[serde(default)]
    pub detected: CmdDetect,
    /// 磁盘占用报告
    #[serde(default)]
    pub space: Option<SpaceReport>,
    /// 语言构成
    #[serde(default)]
    pub langs: Vec<LangStat>,
    /// 近 30 天有提交的日子
    #[serde(default)]
    pub activity: Vec<ActivityDay>,
    /// 当前显示的是哪一份 README
    #[serde(default)]
    pub readme: Option<String>,
    /// README 正文
    #[serde(default)]
    pub readme_text: Option<String>,
    /// 「另一份」是哪个文件、哪种语言。**存整个 `ReadmeAlt` 而不是只存
    /// 文件名**——里面的 `zh` 才是"切回来时怎么判语言"的依据，
    /// 只留文件名会把它丢掉，中英往返就废了（这正是阶段 18 要修的那条）
    #[serde(default)]
    pub readme_alt: Option<ReadmeAlt>,
    /// 这份快照是**什么时候**扫出来的。缓存必须带时间戳，
    /// 否则界面上没法告诉用户"这是多久前的数"
    #[serde(default)]
    pub scanned_at: u64,
}

/// 缓存库放在配置旁边：`%APPDATA%/com.qingbird.git/cache.db`。
/// 与 `config.json` 同目录——用户备份/迁移时是一套，
/// 不会出现"配置拷走了、缓存还留在旧机器"这种半迁移状态。
pub fn db_path() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    Some(PathBuf::from(appdata)
        .join("com.qingbird.git")
        .join("cache.db"))
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS scan_cache (
    path       TEXT PRIMARY KEY,
    scanned_at INTEGER NOT NULL,
    payload    TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS op_history (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    ts      INTEGER NOT NULL,
    path    TEXT NOT NULL,
    op      TEXT NOT NULL,
    ok      INTEGER NOT NULL,
    detail  TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_op_history_path ON op_history(path, id DESC);
";

/// 打开并确保表结构。**打不开返回 None**——调用方照常走全量扫描。
fn open() -> Option<rusqlite::Connection> {
    let path = db_path()?;
    if let Some(dir) = path.parent() {
        // 目录不存在（首次运行）就建一个；建不出来也是 None，不硬撑
        std::fs::create_dir_all(dir).ok()?;
    }
    let conn = rusqlite::Connection::open(path).ok()?;
    conn.execute_batch(SCHEMA).ok()?;
    Some(conn)
}

/// 读一个仓库的缓存快照。**没有 / 读不出来都是 None**，语义统一成
/// "这个仓库没扫过"——界面上那就是骨架屏，跟没缓存时一模一样。
pub fn load_cache(repo_path: &str) -> Option<RepoCache> {
    let conn = open()?;
    let payload: String = conn
        .query_row(
            "SELECT payload FROM scan_cache WHERE path = ?1",
            [repo_path],
            |r| r.get(0),
        )
        .ok()?;
    // payload 坏掉（中途被 kill、格式变了）**不能当错误**，
    // 丢掉这份缓存重扫就是了
    serde_json::from_str(&payload).ok()
}

/// 写入一个仓库的快照（覆盖同路径那一行）。
/// 返回 false = 没记上，**调用方不必在意**。
pub fn save_cache(repo_path: &str, cache: &RepoCache) -> bool {
    let Some(conn) = open() else { return false };
    let Ok(payload) = serde_json::to_string(cache) else { return false };
    conn.execute(
        "INSERT OR REPLACE INTO scan_cache (path, scanned_at, payload) VALUES (?1, ?2, ?3)",
        rusqlite::params![repo_path, cache.scanned_at as i64, payload],
    )
    .is_ok()
}

/// 删掉一个仓库的缓存（移除仓库时调用，否则库里会攒下孤儿行）。
pub fn drop_cache(repo_path: &str) -> bool {
    let Some(conn) = open() else { return false };
    conn.execute("DELETE FROM scan_cache WHERE path = ?1", [repo_path])
        .is_ok()
}

/// 记一条操作历史。**只记"用户主动发起且有结果"的操作**——
/// 后台自动同步会自己跑，全记的话一个月就是几万条噪音。
/// 返回 false = 没记上，调用方不必在意。
pub fn record_op(repo_path: &str, op: &str, ok: bool, detail: &str) -> bool {
    let Some(conn) = open() else { return false };
    conn.execute(
        "INSERT INTO op_history (ts, path, op, ok, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            crate::store::now_secs() as i64,
            repo_path,
            op,
            ok as i64,
            detail
        ],
    )
    .is_ok()
}

/// 某个仓库的操作历史，最近的排在前面。给界面画时间线用。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OpRecord {
    pub ts: u64,
    pub path: String,
    pub op: String,
    pub ok: bool,
    pub detail: String,
}

/// 读某个仓库的操作历史，`limit` 条。
pub fn op_history(repo_path: &str, limit: usize) -> Vec<OpRecord> {
    let Some(conn) = open() else { return Vec::new() };
    let Ok(mut stmt) = conn.prepare(
        "SELECT ts, path, op, ok, detail FROM op_history
         WHERE path = ?1 ORDER BY id DESC LIMIT ?2",
    ) else {
        return Vec::new();
    };
    // `&str` 与 `i64` 混在一个数组里是编不过的（数组元素必须同类型），
    // 得走 `params!` 让每个参数各自带自己的类型
    let rows = stmt.query_map(rusqlite::params![repo_path, limit as i64], |r| {
        Ok(OpRecord {
            ts: r.get::<_, i64>(0)? as u64,
            path: r.get(1)?,
            op: r.get(2)?,
            ok: r.get::<_, i64>(3)? != 0,
            detail: r.get(4)?,
        })
    });
    match rows {
        Ok(it) => it.filter_map(Result::ok).collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 缓存往**临时目录**里写，不碰用户真实的 `%APPDATA%/…/cache.db`。
    /// 单测跑在 `cargo test` 里，用真实路径会污染用户数据——
    /// 那正是 2026-10-05 抓帧污染 config 的同一类错。
    ///
    /// ⚠️ 动 `APPDATA` 是**进程级全局**的，所以这些用例必须**单线程跑**
    /// （见模块级 `serial` 说明），否则与别的测试互相踩。
    struct TempDb {
        dir: PathBuf,
        old: Option<String>,
    }

    impl TempDb {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("qb-cache-test-{tag}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let old = std::env::var("APPDATA").ok();
            std::env::set_var("APPDATA", &dir);
            TempDb { dir, old }
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            if let Some(v) = self.old.take() {
                std::env::set_var("APPDATA", v);
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// 所有动 `APPDATA` 的用例共用一把锁：**改环境变量是进程级的**，
    /// 并行跑的话一个用例把 `APPDATA` 还原、另一个还在用它的库，
    /// 失败会表现为"单测偶发红、重跑又好了"——最难查的那种。
    static APPDATA_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn sample(_path: &str) -> RepoCache {
        RepoCache {
            status: RepoStatus {
                is_repo: true,
                branch: "main".into(),
                ahead: 1,
                behind: 2,
                dirty: true,
                has_upstream: true,
                upstream: "origin/main".into(),
                remote_url: "https://example/x.git".into(),
            },
            changed: 3,
            last_commit_ts: 1_700_000_000,
            langs: vec![LangStat {
                language: "Rust".into(),
                percent: 100.0,
            }],
            activity: vec![ActivityDay {
                date: "2026-10-05".into(),
                count: 4,
            }],
            readme: Some("README.md".into()),
            readme_text: Some("# 中文标题\n\n带 emoji ✅ 的正文".into()),
            readme_alt: Some(ReadmeAlt {
                file: "README_en.md".into(),
                zh: false,
            }),
            scanned_at: 1_700_000_123,
            ..Default::default()
        }
    }

    #[test]
    fn cache_round_trips_through_the_database() {
        let _g = APPDATA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TempDb::new("roundtrip");
        let c = sample("F:\\a\\b");
        assert!(save_cache("F:\\a\\b", &c));
        let back = load_cache("F:\\a\\b").expect("刚写的却读不到");
        assert_eq!(back.status.behind, 2);
        assert_eq!(back.changed, 3);
        assert_eq!(back.langs.len(), 1);
        assert_eq!(
            back.readme_text.as_deref(),
            Some("# 中文标题\n\n带 emoji ✅ 的正文")
        );
        assert_eq!(back.scanned_at, 1_700_000_123);
    }

    /// **覆盖写**而不是追加：同一个仓库扫两次，库里仍然只有一行。
    /// 追加的话每次启动都涨一行，库会无限变大。
    #[test]
    fn saving_twice_replaces_rather_than_appends() {
        let _g = APPDATA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TempDb::new("replace");
        let mut c = sample("F:\\x");
        assert!(save_cache("F:\\x", &c));
        c.status.behind = 99;
        c.scanned_at = 1_700_000_999;
        assert!(save_cache("F:\\x", &c));
        let back = load_cache("F:\\x").unwrap();
        assert_eq!(back.status.behind, 99, "要的是后一次的值");
        assert_eq!(back.scanned_at, 1_700_000_999);
    }

    #[test]
    fn missing_repo_reads_as_none_not_as_an_error() {
        let _g = APPDATA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TempDb::new("missing");
        assert!(load_cache("F:\\根本没扫过").is_none());
        // 语义统一成"没扫过"，界面那层就不必区分"没有"与"读坏了"
    }

    #[test]
    fn op_history_is_append_only_and_newest_first() {
        let _g = APPDATA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TempDb::new("ophist");
        assert!(record_op("F:\\y", "pull", true, "拉了 3 个提交"));
        assert!(record_op("F:\\y", "push", false, "远端领先"));
        assert!(record_op("F:\\other", "check", true, ""));
        let h = op_history("F:\\y", 10);
        assert_eq!(h.len(), 2, "只返回该仓库的");
        assert_eq!(h[0].op, "push", "最近的在前");
        assert!(!h[0].ok);
        assert_eq!(h[1].detail, "拉了 3 个提交");
    }

    #[test]
    fn op_history_respects_the_limit() {
        let _g = APPDATA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TempDb::new("oplimit");
        for i in 0..10 {
            record_op("F:\\z", "check", true, &format!("第 {i} 次"));
        }
        assert_eq!(op_history("F:\\z", 3).len(), 3);
    }

    #[test]
    fn dropping_a_repo_leaves_no_orphan_row() {
        let _g = APPDATA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TempDb::new("drop");
        assert!(save_cache("F:\\gone", &sample("F:\\gone")));
        assert!(drop_cache("F:\\gone"));
        assert!(load_cache("F:\\gone").is_none());
    }

    /// **缓存库坏掉要降级，不能崩**——它是加速器不是依赖。
    /// 往 `cache.db` 塞一段不是 SQLite 的字节，模拟文件损坏。
    #[test]
    fn a_corrupt_database_degrades_instead_of_panicking() {
        let _g = APPDATA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TempDb::new("corrupt");
        let dir = _db.dir.join("com.qingbird.git");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cache.db"), b"this is not a sqlite file at all").unwrap();
        // 读：当成"没扫过"
        assert!(load_cache("F:\\whatever").is_none());
        // 写：当成"没记上"，返回 false 而不是 panic
        assert!(!save_cache("F:\\whatever", &sample("F:\\whatever")));
        assert!(!record_op("F:\\whatever", "pull", true, ""));
        assert!(op_history("F:\\whatever", 5).is_empty());
    }
}
