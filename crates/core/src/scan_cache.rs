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

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::repos::{ActivityDay, LangStat, ReadmeAlt, ReadmePart};
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
    /// **库里这一份 `status` 是不是真读出来的**。
    ///
    /// ⚠️ 不能用 `status.is_repo` 代替它：`RepoStatus::default()` 和
    /// 「读过了、这个文件夹不是仓库」是同一个值，两者在这一层分不开。
    /// 而这条记录会被「只写体积」那一路（`persist_space_cache`）先建起来，
    /// 那时 `status` 还是空的——把它当成"读过的状态"灌进界面，
    /// 界面就会指着一份空读数说「这个文件夹不是 Git 仓库」。
    /// 老配置里没有这个字段 → 读成 `false` → 界面说「待扫描」：
    /// **少说一句已知，不多编一句未知**，这是故意的方向
    #[serde(default)]
    pub status_read: bool,
    /// 未提交文件数
    #[serde(default)]
    pub changed: u32,
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
    /// 根目录下**每一份**说明文档的正文（文件名 → 正文，阶段 18）
    ///
    /// 存全是为了切换语言时**不碰磁盘**：正文早就在手里，切一下就该换掉，
    /// 不该再白走一趟后台读盘——那正是这道闪烁的来源。每份都过
    /// `README_MAX_CHARS` 截断，写进来的不会是整本大部头
    #[serde(default)]
    pub readme_docs: BTreeMap<String, String>,
    /// 候选表（名字 + 判出来的语言 + 排序档）
    ///
    /// 存它是为了让「另一份是哪一份」也能脱离磁盘算（`repos::alt_of`）。
    /// **判据只有 core 那一份**——界面拿缓存里这张表去问同一个函数，
    /// 而不是自己再写一遍挑哪一份的规则
    #[serde(default)]
    pub readme_index: Vec<ReadmePart>,
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
-- CLI 工具扫描结果（阶段一）。与仓库缓存**同一份库文件**：建表 / 打开 /
-- busy_timeout 全走这一条路径，`cli::cache` 只管增删查（别另起一个 open）。
-- 一次扫描是一份快照 ⇒ 写入是「整表替换」（先 DELETE 再 INSERT）。
-- `executables` 存 JSON 数组文本（命令名列表，值就几个短串）。
CREATE TABLE IF NOT EXISTS cli_packages (
    id           TEXT PRIMARY KEY,
    provider     TEXT NOT NULL,
    name         TEXT NOT NULL,
    version      TEXT,
    latest       TEXT,
    install_path TEXT,
    kind         TEXT NOT NULL DEFAULT '',
    executables  TEXT NOT NULL DEFAULT '[]',
    scanned_at   INTEGER NOT NULL DEFAULT 0
);
";

/// 并发写这张表时，让第二个连接**等多久**再放弃（毫秒）。
///
/// ⚠️ 这一档在 32-9 之前一直是空转的：那颗全局 `OP_LOCK` 让同一时刻只有一个忙操作，
/// 而 `record_op` 又跑在主线程 ⇒ 两个连接从没同时写过。**批量并发之后**
/// "多路各开一个连接、各写一行"变成常态 ⇒ 抢不到写锁的那个连接直接 `SQLITE_BUSY`，
/// 而这一层的约定是「写失败返回 false、绝不 panic」（它是加速器）
/// ⇒ 症状是**操作历史静默少几条**：check 零错、单测全绿、界面上一点看不出来
/// （D73 同族——断点不在逻辑里，在"没人报"这件事上）。
///
/// 取 5 秒：一行 `INSERT` 持写锁的时间是毫秒级，5 秒足够排完一整批（上限 4 路 ×
/// 十几个仓库），而真出磁盘故障时它也不会把界面拖住太久
///
/// ⚠️ **`rusqlite` 0.40.2 自己在建连接时就把这一档设成了 5000**
/// （`inner_connection.rs:118` 的 `sqlite3_busy_timeout(db, 5000)`）。所以：
/// ① **这一句在当前依赖版本下与上游默认同值**，删掉它**不会有任何行为变化**，
/// 于是"把那句删掉"这种变异**打不红任何用例**——本轮实测正是如此；
/// ② 上游那个数是**实现细节不是契约**（它不在 `Connection::open` 的文档里，
/// 也没有任何版本号钉着），把它写成我们自己的常量就是不打算赌它；
/// ③ ⇒ 能证伪的判据不是"有没有那一句"，是**这一档是不是 0**：
/// `a_contended_write_waits_and_does_not_vanish` 把 `BUSY_TIMEOUT_MS` 设成 0
/// 会当场红（真实变异验证过），而它测的是"抢锁那一次到底记没记上"。
const BUSY_TIMEOUT_MS: u64 = 5000;

/// 打开并确保表结构。**打不开返回 None**——调用方照常走全量扫描。
fn open() -> Option<rusqlite::Connection> {
    open_at(&db_path()?)
}

/// [`open`] 的实现，**库文件路径由调用方给**。
///
/// 拆这一层跟 `store::save_to` 同一个理由：`db_path()` 读的是 `APPDATA`，
/// 而单测不许 `set_var` 那个进程级全局 ⇒ 「busy_timeout 到底设没设」这种判据
/// 不拆出来就**一条用例都写不了**，只能靠注释信一句
pub(crate) fn open_at(path: &std::path::Path) -> Option<rusqlite::Connection> {
    if let Some(dir) = path.parent() {
        // 目录不存在（首次运行）就建一个；建不出来也是 None，不硬撑
        std::fs::create_dir_all(dir).ok()?;
    }
    let conn = rusqlite::Connection::open(path).ok()?;
    // ⚠️ **必须排在 `execute_batch(SCHEMA)` 之前**：那两条
    // `CREATE TABLE IF NOT EXISTS` 在并发首次启动时**自己就要抢写锁**——
    // 建表抢不到就直接 `None`，等于"缓存今天没工作"，而它报不出来
    conn
        .busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS))
        .ok()?;
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
///
/// 成功时返回**写进去的那个时间戳**，调用方拿它更新自己那份内存状态。
/// ⚠️ **别让调用方自己再取一次 `now_secs()`**：那是两个独立的钟，
/// 跨过秒边界就对不上——刚跑完显示「3 分钟前」、重启后又变成「刚刚」。
/// 返回 `None` = 没记上，调用方不必在意。
pub fn record_op(repo_path: &str, op: &str, ok: bool, detail: &str) -> Option<u64> {
    record_op_at(&db_path()?, repo_path, op, ok, detail)
}

/// [`record_op`] 的实现，**库文件路径由调用方给**。
///
/// 拆法与 `open_at` / `store::save_to` 同一个理由：「并发写不丢记录」这条判据
/// 要在**同一个沙箱库文件**上开两个连接跑，而单测不许 `set_var` 那个进程级
/// `APPDATA` ⇒ 不拆就只能在真实配置目录上做实验。
pub(crate) fn record_op_at(
    db: &std::path::Path,
    repo_path: &str,
    op: &str,
    ok: bool,
    detail: &str,
) -> Option<u64> {
    let conn = open_at(db)?;
    let ts = crate::store::now_secs();
    conn.execute(
        "INSERT INTO op_history (ts, path, op, ok, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![ts as i64, repo_path, op, ok as i64, detail],
    )
    .ok()
    .map(|_| ts)
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

/// 哪些操作的成功记录算「这颗仓库和云端交换过东西」（32-11 那句问句的依据）。
///
/// 四算：**拉取**从云端拿、**推送**给云端送、**提交并推送**那条链本身就是送、
/// **变基**那条是"备份并变基到云端"，它一定从云端拿过东西。
///
/// ⛔ **`check` 不算**——这条是整个问句最容易废掉的地方：`check` 只是问云端
/// 有没有新提交，本地一个字节都没动，而**后台自动同步每隔几十分钟就在写一条
/// check 记录**（`scheduler`）。把它算进"同步过"，所有仓库永远都是"刚刚同步"，
/// 「哪几个三个月没同步」一条都筛不出来，而且界面上看起来完全正常。
/// ⛔ `undo_rebase` 也不算：那是本地历史回退，不碰网络。
///
/// ⚠️ 这一档**只有这一处**：SQL 里不写 op 的字面量（它按 `(path, op)` 取最大 ts，
/// 归类交给 [`is_sync_op`]），界面那一句也是读它。加一种新操作时忘了归类，
/// `gui` 那条 `every_op_key_is_classified_as_sync_or_not` 会红
pub const SYNC_OP_KEYS: [&str; 4] = ["pull", "push", "commit", "rebase"];

/// 明确**不算**同步的那几类。
///
/// 它与 [`SYNC_OP_KEYS`] 合起来是"全部已知操作键"的完整清单：`gui` 里那条
/// `every_op_key_is_classified_as_sync_or_not` 会遍历每个 `Op::key()`，
/// 落在两份清单之外的**直接红**。⚠️ 这里必须是两份而不是一份加"其他"兜底——
/// "其他"这一臂会把新增的、没人想过的操作默默算成"不算"，而那正是盲区本身
/// （判错的方向有两个：多算 ⇒ 那条问句永远筛不出东西；少算 ⇒ 正常的仓库被说成
/// 三个月没动，而**两个方向都不报错**）
pub const NON_SYNC_OP_KEYS: [&str; 2] = ["check", "undo_rebase"];

/// 落库的那个操作键算不算"与云端交换过"。**唯一的一份判断**：
/// SQL 里不写字面量（它按 `(path, op)` 取最大 ts），界面那一句也读它
pub fn is_sync_op(key: &str) -> bool {
    SYNC_OP_KEYS.contains(&key)
}

/// 每颗仓库**最后一次成功与云端交换**的秒级时间戳，键是 `git::repo_key` 归一后的路径。
///
/// ⚠️ 返回 `None` 的意思是**这张表读不出来**（库打不开、表坏了），
/// 而**不是**空表。两者绝对不能混：空表 = "每颗仓库都没有同步记录"，
/// 界面会把**所有**仓库都判成「久未同步」——那不是"缺一个数"，
/// 而是一屏假话（31.4 补十五给 `RepoStatus` 补的那一格 `status_read` 就是这一族，
/// 判据也一样：**没数据不许落进任何一个有值的档**）。
/// ⇒ 调用方拿到 `None` 要把那一档**画成不可用**，而不是画成"全都久未同步"
///
/// 一次 `GROUP BY` 拿全表，不逐颗查：十七颗仓库就是十七次打开连接，
/// 而这一层的约定是加速器、不是每次都值得跑一遍的东西
pub fn last_sync_times() -> Option<std::collections::HashMap<String, u64>> {
    last_sync_times_in(&db_path()?)
}

/// [`last_sync_times`] 的实现，**库文件路径由调用方给**（同 `open_at` / `save_to` /
/// `record_op_at`：单测不许 `set_var` 那个进程级 `APPDATA`，
/// 而这条判据必须**真跑 rusqlite**——SQL 的语义不是读代码能证的）
pub(crate) fn last_sync_times_in(
    db: &std::path::Path,
) -> Option<std::collections::HashMap<String, u64>> {
    let conn = open_at(db)?;
    // 按 `(path, op)` 取最大时间戳，**归类留给 `is_sync_op`**：
    // 把四类键写进这条 SQL 里就变成第二个出处，改一处漏一处
    let Ok(mut stmt) = conn.prepare(
        "SELECT path, op, MAX(ts) FROM op_history WHERE ok = 1 GROUP BY path, op",
    ) else {
        return None;
    };
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    });
    // ⚠️ **类型要写出来**：`out` 只靠返回值那一处推断的话，
    // 上面 `and_modify` 的闭包里 `*old` 推不出类型（E0282）
    let mut out: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    let Ok(it) = rows else { return None };
    for row in it.flatten() {
        let (path, op, ts) = row;
        if !is_sync_op(&op) {
            continue;
        }
        let key = crate::git::repo_key(&path);
        let ts = ts.max(0) as u64;
        // 同一颗仓库可能有多种写法各自成行，也可能同时有 pull 与 push 两类 ⇒ 取**最晚**
        // 那一次。⚠️ 必须是**一条链**：`and_modify` 会把 `key` 消费掉，
        // 分成两句写第二句就是 E0382（借用已移动的值）
        out.entry(key)
            .and_modify(|old| *old = (*old).max(ts))
            .or_insert(ts);
    }
    Some(out)
}

/// 「这颗仓库多久没和云端交换过」那四档（32-11 那句问句的判据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncAge {
    /// ⚠️ **整张表读不出来**（库打不开、表坏了）。这一档**不许**落进下面任何一档：
    /// 把它算成 `Never` 就等于对每一颗仓库都说"三个月没同步"，
    /// 而真实的知识量是"我一个字都不知道"（31.4 补十五给 `RepoStatus` 补
    /// `status_read` 那一格是同一族，判据也一样：**没数据不进任何有值的档**）
    Unknown,
    /// 读到了，但这颗仓库**没有一次成功的同步记录**：新加的、克隆完没动过的、
    /// 或者一直在失败的（失败记录 `ok = 0` 不算交换过）
    Never,
    /// `stale_days` 之内交换过
    Recent,
    /// 超过 `stale_days` 没交换过
    Stale,
}

/// 「三个月」那一档的天数。
///
/// 取 90 = 3 × 30。⚠️ 不按月历算：那要引入月份长度与时区，而这一档的用途是
/// "半年没动过的挑出来"这类**粗筛**，差两天天不影响结论；而一旦写成
/// "三个自然月"，测试就得跟着日历走（那是另一条外包给环境的前提）
pub const STALE_DAYS: u64 = 90;

/// 纯函数版判据：把 [`SyncAge`] 算出来。
///
/// 为什么不留给界面自己比时间戳：这一档有**四个**读者（那颗筛选片、片上的计数、
/// 空态那句"没有久未同步的仓库"、以后可能要的表格列），写成四处 `now - ts > …`
/// 迟早有一处漂（本项目为这类"同一规则两处各写一遍"交过四次学费）。
/// ⚠️ 时间戳来自数据库、`now` 来自系统时钟，两者都可能是脏的 ⇒
/// 用 `saturating_sub`：表里存了个未来时间（用户改过系统时钟）时
/// 落进 `Recent`，**不 panic 也不算成"久未同步"**
pub fn sync_age(
    times: &Option<std::collections::HashMap<String, u64>>,
    repo_path: &str,
    now: u64,
    stale_days: u64,
) -> SyncAge {
    let Some(times) = times else {
        return SyncAge::Unknown;
    };
    let Some(last) = times.get(&crate::git::repo_key(repo_path)) else {
        return SyncAge::Never;
    };
    let limit = stale_days.saturating_mul(86_400);
    if now.saturating_sub(*last) > limit {
        SyncAge::Stale
    } else {
        SyncAge::Recent
    }
}

/// 这一档要不要被「久未同步」那片筛出来。**两处都读它**：计数与过滤，
/// 各写一份的话片上的数字和点进去看到的行会对不上
pub fn sync_age_is_stale(age: SyncAge) -> bool {
    matches!(age, SyncAge::Stale | SyncAge::Never)
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
                stash_count: 0,
                // 32-12 新加的那一格也要走这条往返断言：它是**极性挑过**的
                // （`unknown = true` 才让老库读成"数字可信"），没验就等于没挑
                ab_unknown: true,
            },
            changed: 3,
            // ⚠️ 这两格是 32-10 补进样例的：GUI 那一侧「缓存里的快捷命令能不能
            // 灌进界面」现在**只认 `status_read`**，所以"这一份能不能活过一次
            // 序列化"就是那条闸门的地基。以前样例里两者都不填（都落
            // `..Default::default()`），于是往返测试压根没验过它们
            status_read: true,
            detected: CmdDetect {
                test_cmd: "cargo test".into(),
                build_cmd: "cargo build --release".into(),
                ..Default::default()
            },
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
        assert!(
            back.status_read,
            "「这份 status 是真读出来的」必须活过一次序列化——GUI 的 detected 闸门认的就是它"
        );
        assert_eq!(back.detected.test_cmd, "cargo test");
        assert_eq!(back.detected.build_cmd, "cargo build --release");
        assert!(
            back.status.ab_unknown,
            "「领先/落后读不出来」这一格必须活过一次序列化——它是界面显示数字还是「–」的判据"
        );
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

    /// 老库（**payload 里没有 `ab_unknown` 这一格**）读回来必须还是"数字可信"。
    ///
    /// 这一条钉的是 32-12 那个**命名极性**：字段叫 `ab_unknown` + `serde(default)`，
    /// 老 payload 缺它 ⇒ `false` ⇒ 升级之后每个人原来的领先/落后照旧显示数字。
    /// 当初若命名成 `ab_read`，同一次升级会把所有人的那一格一夜变成「–」，
    /// 而那**看起来不像缺陷、只像"升级了一下"**——最难查回来的一种回归
    #[test]
    fn an_old_payload_without_the_new_fields_still_reads_its_numbers() {
        let _g = APPDATA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TempDb::new("oldpayload");
        let db = db_path().expect("TempDb 已把 APPDATA 指到沙箱");
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(SCHEMA).unwrap();
            conn.execute(
                "INSERT INTO scan_cache (path, scanned_at, payload) VALUES (?1,?2,?3)",
                rusqlite::params![
                    "F:\\old",
                    1_700_000_000i64,
                    r#"{"status":{"is_repo":true,"branch":"main","ahead":2,"behind":1,
                       "dirty":false,"has_upstream":true,"upstream":"origin/main",
                       "remote_url":"x"},"scanned_at":1700000000}"#
                ],
            )
            .unwrap();
        }
        let back = load_cache("F:\\old").expect("老 payload 读不出来");
        assert_eq!(back.status.ahead, 2, "老库里那两个数照旧");
        assert_eq!(back.status.behind, 1);
        assert!(
            !back.status.ab_unknown,
            "缺这一格 ⇒ 数字可信。极性选反的话这次升级会让所有人那一格变成「–」"
        );
        assert!(
            !back.status_read,
            "老库里没有「读过 git」那一格 ⇒ 只能当没读过（少说一句已知，不多编一句未知）"
        );
    }

    #[test]
    fn op_history_is_append_only_and_newest_first() {
        let _g = APPDATA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _db = TempDb::new("ophist");
        let t1 = record_op("F:\\y", "pull", true, "拉了 3 个提交").unwrap();
        let t2 = record_op("F:\\y", "push", false, "远端领先").unwrap();
        record_op("F:\\other", "check", true, "").unwrap();
        let h = op_history("F:\\y", 10);
        assert_eq!(h.len(), 2, "只返回该仓库的");
        assert_eq!(h[0].op, "push", "最近的在前");
        assert!(!h[0].ok);
        assert_eq!(h[1].detail, "拉了 3 个提交");
        // 返回的时间戳必须**就是读回来的那个**：界面拿它更新内存里那一份，
        // 对不上就会出现"刚跑完显示 3 分钟前、重启后又变回刚刚"
        assert_eq!(h[0].ts, t2);
        assert_eq!(h[1].ts, t1);
        assert!(t2 >= t1, "后一条不会比前一条更早");
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
        // 写：当成"没记上"，返回 None 而不是 panic
        assert!(!save_cache("F:\\whatever", &sample("F:\\whatever")));
        assert_eq!(record_op("F:\\whatever", "pull", true, ""), None);
        assert!(op_history("F:\\whatever", 5).is_empty());
    }

    /// 这一档必须是**我们写的那个值**——但它**证明不了"那一句 busy_timeout 有没有执行"**。
    ///
    /// ⚠️ 如实登记（本轮变异验证撞出来的）：`rusqlite` 0.40.2 建连接时自己就设了
    /// 5000（`inner_connection.rs:118`），所以**把 `open_at` 里那三行整个删掉，
    /// 这条照样绿**——M4 实测"跑了 1 条、失败 0 条"。它的价值只剩"钉住数值本身"：
    /// 万一上游那个实现细节变了，这条会红。**能证伪这一层保护的只有下面那条**。
    #[test]
    fn the_busy_timeout_is_actually_set_on_a_new_connection() {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("qbg-cache-busy-{}-{n}", std::process::id()));
        let file = dir.join("cache.db");
        let conn = open_at(&file).expect("临时库该打得开");
        let ms: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            ms,
            BUSY_TIMEOUT_MS as i64,
            "这一档不是我们说的那个值 ⇒ 并发写要么白等要么直接放弃，\
             而这一层不会报，只会静默少记一条"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **抢不到写锁的那一次必须记上，不是静默消失**——32-9 批量并发之后
    /// "多路各开一个连接、各写一行"变成常态，这条才是那一层保护的行为判据。
    ///
    /// 判据取**末端拿没拿到那条数**，不取"代码里有没有那句 `busy_timeout`"
    /// （后者正是本项目反复交学费的那类自查）。变异把 `BUSY_TIMEOUT_MS` 设成 0
    /// ⇒ 等待那一路当场红：这一条是**有证明力的**。
    ///
    /// 不需要 `APPDATA_LOCK`：全程只碰这一个沙箱文件，`record_op_at` 的路径是参数。
    #[test]
    fn a_contended_write_waits_and_does_not_vanish() {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("qbg-cache-contend-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("cache.db");

        // 先把表建好，免得"等锁"与"等建表"混成一件事
        drop(open_at(&file).expect("临时库该打得开"));

        // 占住写锁：一条真 `INSERT` 挂在没提交的事务上
        let holder = open_at(&file).expect("占锁的连接该打得开");
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        holder
            .execute(
                "INSERT INTO op_history (ts, path, op, ok, detail) VALUES (1, 'holder', 'pull', 1, '')",
                [],
            )
            .unwrap();

        // 另一路同时来写——它只能等，等不到就**什么都不报**地丢掉
        let waiter_file = file.clone();
        let waiter = std::thread::spawn(move || record_op_at(&waiter_file, "waiter", "pull", true, "拉到 2 个提交"));
        std::thread::sleep(std::time::Duration::from_millis(300));
        holder.execute_batch("COMMIT").unwrap();
        let ts = waiter.join().unwrap();

        assert!(
            ts.is_some(),
            "被抢锁的那一次没记上 ⇒ 等待时间形同 0，\
             而这一层是加速器、它不报错，界面上永远看不出来"
        );
        // 两条都得在：占锁那条与等待那条，一个都不能少
        let rows: i64 = open_at(&file)
            .expect("复查的连接该打得开")
            .query_row("SELECT COUNT(*) FROM op_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 2, "并发写丢记录：历史里少一条 = 用户看不见那次操作");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ============ 32-11「久未同步」那三颗螺丝 ============

    /// 一个**只属于这条用例**的沙箱库。不碰 `APPDATA`、不动进程级全局，
    /// 目录带 tag 加序号 ⇒ 并行跑不会互相抢（AGENTS 那条"前提不许外包给环境"）
    fn sandbox(tag: &str) -> (PathBuf, PathBuf) {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("qb-sync-times-{tag}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("cache.db");
        (dir, db)
    }

    /// 手工塞一条**指定时间戳**的记录。
    /// `record_op_at` 用的是 `now_secs()`，而"取最晚那一笔""整 90 天"这些判据
    /// 都要求 ts 是我给定的数——所以这一层必须能直接写表
    fn insert_op(db: &std::path::Path, path: &str, op: &str, ok: bool, ts: u64) {
        let conn = open_at(db).expect("沙箱库该打得开");
        conn.execute(
            "INSERT INTO op_history (ts, path, op, ok, detail) VALUES (?1, ?2, ?3, ?4, '')",
            rusqlite::params![ts as i64, path, op, ok as i64],
        )
        .unwrap();
    }

    /// ⛔ **检测成功、拉取失败都不算"同步过"**。
    ///
    /// 这条是整个问句的生死判据：后台自动同步每隔几十分钟就在写一条 `check` 成功
    /// 记录（`scheduler`），认了它 ⇒ 每一颗仓库永远"刚刚同步过"，「哪几个三个月
    /// 没同步」**一条都筛不出来**，而界面上那颗片上的数字看起来完全正常。
    /// 失败那一笔故意写得比成功那笔**更新**：只看时间不看类别就会错取它
    #[test]
    fn a_check_or_a_failed_attempt_is_not_a_sync() {
        let (_dir, db) = sandbox("not-sync");
        let p = "F:\\never-synced";
        insert_op(&db, p, "check", true, 5_000);
        insert_op(&db, p, "pull", false, 4_000);
        insert_op(&db, p, "push", true, 1_000);
        let times = last_sync_times_in(&db).expect("沙箱库该读得出来");
        assert_eq!(
            times.get(&crate::git::repo_key(p)).copied(),
            Some(1_000),
            "算进「同步过」的只能是成功过的拉取/推送那一类"
        );
    }

    /// 同仓库多种操作都算 ⇒ 取**最晚**那一次，而不是"最后写进来的那一次"
    #[test]
    fn the_latest_successful_sync_wins_across_kinds() {
        let (_dir, db) = sandbox("latest");
        let p = "F:\\both";
        insert_op(&db, p, "pull", true, 1_000);
        insert_op(&db, p, "commit", true, 2_000);
        insert_op(&db, p, "push", true, 2_500);
        let times = last_sync_times_in(&db).unwrap();
        assert_eq!(times.get(&crate::git::repo_key(p)).copied(), Some(2_500));
    }

    /// ⚠️ **"读不出来"与"一条记录都没有"必须是两个值**。
    ///
    /// 前者的知识量是"我一个字都不知道"，后者才是"每颗都没有同步记录"。
    /// 混起来的后果不是少一个数：那片筛选片一点下去，整张表变成
    /// "看起来全都三个月没同步"——31.4 补十五给 `RepoStatus` 补 `status_read`
    /// 那一格是同一族，判据也一样：**没数据不许落进任何一个有值的档**
    #[test]
    fn an_unreadable_database_is_not_a_blank_table() {
        let (dir, db) = sandbox("unreadable");
        std::fs::write(&db, b"this is not a sqlite file at all").unwrap();
        assert!(
            last_sync_times_in(&db).is_none(),
            "坏库被当成空表 ⇒ 界面会把所有仓库都说成久未同步"
        );
        let (dir2, db2) = sandbox("empty-but-readable");
        assert_eq!(
            last_sync_times_in(&db2).map(|m| m.len()),
            Some(0),
            "打得开、一条记录都没有 ⇒ 这才是空表"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    /// 同一颗仓库的三种写法必须并成一条。
    /// 不并的后果：库里两条各算一次"最晚"，界面按归一键去查 → 查不到 → 判成
    /// `Never`，一颗上周刚推过的仓库被说成"从没同步过"
    #[test]
    fn one_path_three_spellings_is_one_entry() {
        let (_dir, db) = sandbox("spellings");
        insert_op(&db, "F:\\Repo", "pull", true, 1_000);
        insert_op(&db, "f:/repo/", "push", true, 3_000);
        insert_op(&db, " F:\\repo ", "push", true, 2_000);
        let times = last_sync_times_in(&db).unwrap();
        assert_eq!(times.len(), 1, "同一路径三种写法各成了一条");
        assert_eq!(
            times.get(&crate::git::repo_key("F:\\Repo")).copied(),
            Some(3_000),
            "并成一条之后还要取最晚那一笔"
        );
    }

    /// 四档 + 两个边界。边界取"整 90 天算没过期"，两侧都算的话界面上会出现
    /// 两个互相矛盾的数（片上的计数与点进去的行对不上）
    #[test]
    fn sync_age_splits_four_ways() {
        let day = 86_400u64;
        let key = crate::git::repo_key("F:\\a");
        let mut m = std::collections::HashMap::new();
        m.insert(key, 1_000_000u64);
        let times: Option<std::collections::HashMap<String, u64>> = Some(m);
        let none: Option<std::collections::HashMap<String, u64>> = None;
        let now = 1_000_000 + 90 * day;
        assert_eq!(
            sync_age(&times, "F:\\a", now, STALE_DAYS),
            SyncAge::Recent,
            "正好 90 天那一头算没过期"
        );
        assert_eq!(sync_age(&times, "F:\\a", now + 1, STALE_DAYS), SyncAge::Stale);
        assert_eq!(sync_age(&times, "F:\\missing", now, STALE_DAYS), SyncAge::Never);
        assert_eq!(sync_age(&none, "F:\\a", now, STALE_DAYS), SyncAge::Unknown);
        // 表里存了个未来时间（用户改过系统时钟、或跨机器拷过库）⇒ 不许 panic，
        // 也不算成"久未同步"
        assert_eq!(sync_age(&times, "F:\\a", 500, STALE_DAYS), SyncAge::Recent);
    }

    /// 那一颗片到底筛谁。**四档逐一钉住**，因为两个方向都会错：
    /// 多放进 `Unknown` = 库读不出来时一片下去整张表都说成久未同步；
    /// 少放进 `Never` = "克隆下来就再没动过"和"一直在失败"那一批永远挑不出来
    #[test]
    fn only_stale_and_never_are_picked_up() {
        assert!(!sync_age_is_stale(SyncAge::Unknown));
        assert!(sync_age_is_stale(SyncAge::Never));
        assert!(!sync_age_is_stale(SyncAge::Recent));
        assert!(sync_age_is_stale(SyncAge::Stale));
    }
}
