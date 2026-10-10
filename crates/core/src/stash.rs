//! git stash：**谁藏的、藏了什么、怎么取回来**。
//!
//! ## 这一层为什么存在（32-2）
//!
//! 拉取前替用户 `stash push -u`、收尾 `stash pop`，pop 没成功时那些文件就**留在
//! stash 里**——而整个 GUI 从来没有一处读 `stash list`，于是"该去哪儿找"没有答案。
//! 补二十一修的是"pop 必达"，这一层修的是"没弹回来时看得见、取得回"。
//!
//! ## 为什么消息前缀也归这里
//!
//! 藏东西的两条路以前**各写一份措辞**：拉取侧是硬编码字面量（`sync.rs`），
//! 变基侧是常量（`fork.rs::STASH_MSG`）。界面要区分"青鸟替你收的 / 你自己存的"，
//! 判据就只能有一份 ⇒ 常量与识别谓词一起收进这个模块，两条路都从这里取。
//!
//! ⚠️ **序号不是身份**。`git stash drop stash@{1}` 之后，原来的第二条变成第一条，
//! 界面上任何旧快照都会指错东西——而 `drop` 不可逆。所以 `apply` / `drop` 都要求
//! 调用方把**消息原文**一起交进来，动手前当场重读一次清单比对（见 `resolve`）。

use serde::Serialize;

use crate::net::run_local;
use crate::repos::read_status;
use crate::sync::begin_user_op;
use crate::types::{combined, OpResult};

/// 自动暂存的消息**前缀**——全仓唯一出处。
///
/// 用它而不是整句匹配：拉取侧在 32-2 之前的措辞是 `qingbird-git auto stash`
/// （空格、不带冒号），用户仓库里可能还压着那条。按整句判会把**青鸟藏的**说成
/// "你自己存的"，那是把归属报错——比不报更坏。
pub const AUTO_PREFIX: &str = "qingbird-git";

/// 拉取前自动暂存那一条的说明。
///
/// ⚠️ 措辞从前的 `qingbird-git auto stash` 改成中文，是因为这条**会出现在界面上**
/// （清单里"青鸟替你收的"那一行的原文），英文短语在中文界面里读起来像系统日志。
/// 前缀没动 ⇒ 新旧两种措辞都被 [`is_auto`] 认得。
pub const MSG_PULL: &str = "qingbird-git: 拉取前自动暂存";

/// 变基前自动暂存那一条的说明。**沿用 `fork.rs` 原来的值，一字未改**——
/// 它本来就是中文，改它没有任何收益
pub const MSG_REBASE: &str = "qingbird-git: 变基前自动暂存";

/// 一条暂存。字段全部来自 `git stash list` 的一次调用，**不额外问 git**。
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StashEntry {
    /// `stash@{N}` 的那个 N（0 = 最近一条）
    pub index: u32,
    /// 藏的时候所在分支。解析不出来给空串，界面退成不显示分支
    pub branch: String,
    /// 说明正文：`On <分支>: ` 后面那一段。青鸟藏的就是 [`MSG_PULL`] / [`MSG_REBASE`]，
    /// 用户自己 `git stash` 的通常是 `WIP on <分支>: <sha> <提交标题>` 里那一段
    pub note: String,
    /// 时间，精确到分钟（`2026-10-10 14:02`）
    pub when: String,
    /// 是不是青鸟替用户藏的
    pub auto: bool,
    /// `git stash list` 给的那一行的**原文**。丢掉时的二次确认要打它——
    /// 只说"确定吗"等于让用户凭序号确认一个不可逆动作
    pub subject: String,
}

/// 这条说明是不是青鸟写的。判据是**前缀**，理由写在 [`AUTO_PREFIX`] 上面。
pub fn is_auto(note: &str) -> bool {
    note.starts_with(AUTO_PREFIX)
}

/// 界面上显示的那句说明：把"是谁存的"那一截去掉，只留"存的是什么"。
///
/// ⚠️ **两种分隔符都要剥**（`": "` 与 `" "`）。前缀我们改过一次措辞
/// （`qingbird-git auto stash` → `qingbird-git: 拉取前自动暂存`），
/// 只认新那一种的话，用户现存的那条会显示成 `qingbird-git auto stash` 一整串——
/// **归属认得出、显示却认不出**，那就是把同一件事写了两份判据
pub fn human_note(note: &str) -> &str {
    let rest = note.strip_prefix(AUTO_PREFIX).unwrap_or(note);
    rest.trim_start_matches([' ', ':']).trim()
}

/// 把 `git stash list` 的 `%gs` 拆成（分支，说明）。
///
/// git 给的形状有两种（`Documentation/refs` 里 reflog subject 的写法）：
/// - `On <分支>: <用户或我们给的消息>` —— `stash push -m` 走这一条
/// - `WIP on <分支>: <sha> <那条提交的标题>` —— 不带 `-m` 时 git 自己编的
///
/// ⚠️ **认不出来时不许猜**：整行当说明、分支留空。宁可界面上少一段分支名，
/// 也不能把一条用户自己的 stash 判成"青鸟藏的"（`is_auto` 吃的是拆出来的说明段，
/// 拆错就会误判归属）
fn split_subject(gs: &str) -> (String, String) {
    for head in ["WIP on ", "On "] {
        if let Some(rest) = gs.strip_prefix(head) {
            // 分支与说明之间是**第一个** `: `。分支名里不许有冒号（git 的 ref 规则），
            // 所以"第一个"就是唯一解，不用回溯
            if let Some((branch, note)) = rest.split_once(": ") {
                return (branch.to_string(), note.to_string());
            }
            return (String::new(), gs.to_string());
        }
    }
    (String::new(), gs.to_string())
}

/// `--date=iso` 给的是 `2026-10-10 14:02:33 +0800`，界面上要的是到分钟。
///
/// 按**字节**切前 16 个字符：这一段一定是 ASCII（ISO 日期），而 `char_indices`
/// 那种写法会在非 ASCII 内容上多绕一层——真给非 ASCII 占了，`get(..16)` 落在
/// 字符中间会返回 `None`，那时整串照给，**不 panic**
fn to_minute(s: &str) -> String {
    s.get(..16).unwrap_or(s).trim().to_string()
}

/// 列出这个仓库的全部暂存（最近的在前）。不是仓库 / 一条没有 ⇒ 空表。
///
/// 一次 `git stash list` 拿全三样（序号 / 原文 / 时间），**不逐条再问 git**。
/// ⚠️ 这里**故意不给"涉及几个文件"**：`git stash show` 默认只算已跟踪那一半，
/// 而青鸟藏的那一条是 `stash push -u`（连未跟踪一起收走）⇒ 那个数会**少报**，
/// 恰好少报在最需要看准的那一档上。要补就得连 `stash@{N}^3` 一起算，那是另一条命令
pub fn list(repo_path: &str) -> Vec<StashEntry> {
    // ⚠️ **日期只用 `%ci`，不要加 `--date=iso` 配 `%cd`**。
    // 真 git 2.55 实测：`--date` 不只格式化 `%cd`，**它连 `%gd` 那截选择器一起改掉**——
    // `stash@{0}` 变成 `stash@{2026-10-10 19:49:29 +0800}` ⇒ 序号解析全部落空 ⇒
    // `parse_list` 按"认不出来就跳过"把每一条都丢掉 ⇒ **清单永远空**，
    // 而 `cargo check` 零错、纯解析单测全绿、界面上那颗感叹号一次都不出现。
    // `%ci` 本身就是固定的 ISO 形状且不受 locale 影响，`to_minute` 切前 16 个字符就够。
    //
    // %x09 是制表符：让分隔符不可能出现在内容里（分支名与提交标题都能带空格）
    let out = run_local(
        repo_path,
        &["stash", "list", "--format=%gd%x09%gs%x09%ci"],
    );
    if !out.success {
        return Vec::new();
    }
    parse_list(&out.stdout)
}

/// `list` 的纯解析那一半。**单独拆出来是为了让四种形状都有单测**——
/// 走真 git 的那一条只能测"能跑通"，测不到"认不出来时不猜"
pub fn parse_list(stdout: &str) -> Vec<StashEntry> {
    let mut items = Vec::new();
    for line in stdout.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let mut cols = line.split('\t');
        let (Some(sel), Some(gs), Some(date)) = (cols.next(), cols.next(), cols.next()) else {
            // 少一列 = git 换了格式或我们拼错了 format。**跳过而不是拿默认值填**：
            // 填出来的每一条都是"用户看不见的一条暂存"，跳过的后果只是清单少一行
            continue;
        };
        // 序号只从 `stash@{N}` 这个形状里取，取不到就跳过（同上：不猜）
        let Some(index) = sel
            .strip_prefix("stash@{")
            .and_then(|r| r.strip_suffix('}'))
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let (branch, note) = split_subject(gs);
        items.push(StashEntry {
            index,
            branch,
            when: to_minute(date),
            auto: is_auto(&note),
            note,
            subject: gs.to_string(),
        });
    }
    items
}

/// 有几条暂存。**就是 `list` 的长度**，不另起一条命令——
/// 界面上那颗感叹号说的数，必须和点开后的清单**同一个出处**，否则会出现
/// 「说有三份、点开只有两份」这种当场打脸的不一致
pub fn count(repo_path: &str) -> usize {
    list(repo_path).len()
}

/// 动手之前**当场**重读一次清单，确认第 `index` 条还是用户点的那一条。
///
/// 为什么必须有：`drop` 之后所有序号整体前移，界面拿着旧快照点第二条就会动到第三条，
/// 而 `drop` **不可逆**。⇒ 序号只用来定位，**消息原文才是身份**。
/// 对不上时返回 `None`，调用方说"清单已刷新，请再看一眼"，**不动任何东西**
fn resolve(repo_path: &str, index: u32, expect: &str) -> Option<StashEntry> {
    list(repo_path)
        .into_iter()
        .find(|e| e.index == index && e.subject == expect)
}

/// 一次取回 / 丢掉的结论。**四个字段各写一遍会漂**，所以收成一颗：
/// `status` 一定带上（工作区被改过了，界面要立刻拿到新状态，不能等下一次刷新）
fn outcome(ok: bool, message: String, output: String, repo_path: &str) -> OpResult {
    OpResult {
        ok,
        message,
        output,
        used_proxy: None,
        status: Some(read_status(repo_path)),
        need_stash: false,
        changes: None,
        fork: None,
        backup_branch: None,
    }
}

/// git 失败时的**第一行原因**。
///
/// ⚠️ 为什么不直接把 git 原文丢进 `output` 就完事：`OpResult::output` 在 GUI
/// **零读者**（路线表 32-2 调研账里那条），写进去等于没写。取回失败最常见的原因
/// 就是"工作区里有同名改动"，那句不说出来用户只看到"失败"两个字，只能反复点
fn first_reason(out: &crate::git::GitOutput) -> String {
    let text = if out.stderr.trim().is_empty() {
        out.stdout.as_str()
    } else {
        out.stderr.as_str()
    };
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("git 没有说明原因")
        .to_string()
}

/// 取回第 `index` 条。**用户入口**：先拿操作锁，再交给 [`do_apply`]。
///
/// 分工照 `pull_repo` / `do_pull` 那一套：**锁只在"用户点出来的那一条路"上**，
/// 无锁的那一半才是可测的。为什么值得多一层：`begin_user_op` 撞锁时会先
/// `git::request_cancel()` 再等十秒，测试里撞上它不只这条会红，
/// **正在并行跑的其他 git 用例会被同一个标记一起带倒**（表现为"没有退出码、
/// stderr 为空"那种查不到的 flaky）
pub fn apply_one(repo_path: &str, index: u32, expect: &str) -> OpResult {
    let Some(_guard) = begin_user_op() else {
        return OpResult::err("有操作正在进行，请稍后再取回".into(), String::new());
    };
    do_apply(repo_path, index, expect)
}

/// 取回的实现（**不碰操作锁**，所以单测能直接调它）。
///
/// ⛔ **不能用裸 `pop`**：pop 拿的是栈顶那一条，用户在清单里点第二条会把第一条
/// 倒进工作区——**取错了东西比取不到更坏**（后者只是没生效，前者污染工作区还看不出来）。
/// 也不用 `pop stash@{N}`：那会把这一条从清单里删掉，而"同一份改动往两个分支各放一次"
/// 就再也做不到了。⇒ 取回后**这一条留着**，界面上要说清，并配 [`drop_one`] 作为另一半
fn do_apply(repo_path: &str, index: u32, expect: &str) -> OpResult {
    let Some(entry) = resolve(repo_path, index, expect) else {
        return stale_list(index);
    };
    let out = run_local(repo_path, &["stash", "apply", &format!("stash@{{{index}}}")]);
    let log = combined(&out);
    // 界面上那一行显示的是 `human_note`（剥掉"是谁存的"那一截）。这里的结论句
    // **必须走同一颗函数**：帧上实测第一版这句打的是整条 `qingbird-git: 拉取前自动暂存`，
    // 而同一屏清单那一行打的是 `拉取前自动暂存`——同一格内容两个出处，早晚漂
    let shown = human_note(&entry.note);
    if !out.success {
        return outcome(
            false,
            format!("取回「{shown}」失败：{}", first_reason(&out)),
            log,
            repo_path,
        );
    }
    outcome(
        true,
        format!("已取回「{shown}」（这一条仍留在清单里，要清掉请点「丢掉」）"),
        log,
        repo_path,
    )
}

/// 丢掉第 `index` 条。**用户入口**，锁的理由同 [`apply_one`]。
/// **不可逆**，所以界面上必须二次确认并打出 `entry.subject` 原文
/// （确认框在 `gui/src/dialogs/stash.rs`，这一层只管执行与身份比对）。
pub fn drop_one(repo_path: &str, index: u32, expect: &str) -> OpResult {
    let Some(_guard) = begin_user_op() else {
        return OpResult::err("有操作正在进行，请稍后再丢".into(), String::new());
    };
    do_drop(repo_path, index, expect)
}

/// 丢掉那一步的实现（不碰锁，理由见 [`apply_one`]）。
fn do_drop(repo_path: &str, index: u32, expect: &str) -> OpResult {
    let Some(entry) = resolve(repo_path, index, expect) else {
        return stale_list(index);
    };
    let out = run_local(repo_path, &["stash", "drop", &format!("stash@{{{index}}}")]);
    let log = combined(&out);
    // 与 `do_apply` 同一颗 `human_note`：结论句与清单那一行不许有两个显示形状
    let shown = human_note(&entry.note);
    if !out.success {
        return outcome(false, format!("丢掉失败：{}", first_reason(&out)), log, repo_path);
    }
    outcome(true, format!("已丢掉「{shown}」，无法撤销"), log, repo_path)
}

/// 序号与原文对不上时的那句话。**两个都打出来**：用户点的是屏幕上那条，
/// 而屏幕已经旧了——只说"出错了"他不知道发生了什么
fn stale_list(index: u32) -> OpResult {
    OpResult::err(
        format!("第 {index} 条已经不是刚才那一条了（清单已刷新），请重新看一眼再动手"),
        String::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_wordings_we_ever_used_count_as_ours() {
        // 新旧两套措辞都必须认。只认新那套 ⇒ 用户仓库里现存的那条会被说成
        // "你自己存的"，而这条判据正是界面上"青鸟替你收的 / 你自己存的"那两行的唯一出处
        assert!(is_auto("qingbird-git: 拉取前自动暂存"));
        assert!(is_auto("qingbird-git: 变基前自动暂存"));
        assert!(is_auto("qingbird-git auto stash"), "32-2 之前的旧措辞");
    }

    #[test]
    fn a_users_own_message_is_not_ours() {
        for note in ["WIP on main: 5d0a2b1 修一个 bug", "先放一下", "lint on main: x"] {
            assert!(!is_auto(note), "{note} 不该被判成青鸟藏的");
        }
        // 反方向的误判也要钉：**结尾**带这个词不算
        assert!(!is_auto("backup qingbird-git"));
    }

    #[test]
    fn the_two_auto_messages_share_one_prefix() {
        // 常量若被改得不再共前缀，`is_auto` 会静默漏判其中一条 ⇒ 界面把青鸟藏的
        // 说成用户自己存的。这条断言把"共前缀"钉住，改任何一条措辞都得一起想清楚
        assert!(MSG_PULL.starts_with(AUTO_PREFIX));
        assert!(MSG_REBASE.starts_with(AUTO_PREFIX));
    }

    #[test]
    fn the_owner_prefix_is_stripped_in_both_wordings_we_ever_wrote() {
        // 新措辞（冒号）与旧措辞（空格）都要剥干净。只剥一种 ⇒ 界面上那一条
        // 前面挂着一整串 `qingbird-git auto stash`，而归属那一栏却认得它
        assert_eq!(human_note(MSG_PULL), "拉取前自动暂存");
        assert_eq!(human_note(MSG_REBASE), "变基前自动暂存");
        assert_eq!(human_note("qingbird-git auto stash"), "auto stash");
        // 不是青鸟存的：一个字都不许动（"先放一下" 里没有任何前缀可剥）
        assert_eq!(human_note("先放一下"), "先放一下");
        assert_eq!(human_note("WIP api"), "WIP api");
    }

    #[test]
    fn subject_splits_into_branch_and_note() {
        let (b, n) = split_subject("On main: qingbird-git: 拉取前自动暂存");
        assert_eq!(b, "main");
        assert_eq!(n, "qingbird-git: 拉取前自动暂存");
        let (b, n) = split_subject("WIP on dev: 5d0a2b1 修一个 bug");
        assert_eq!(b, "dev");
        assert_eq!(n, "5d0a2b1 修一个 bug");
        // 分支名里带不下冒号，但**形状不认识**时不许猜：整行留给说明、分支空着
        let (b, n) = split_subject("something else entirely");
        assert_eq!(b, "");
        assert_eq!(n, "something else entirely");
    }

    #[test]
    fn a_list_line_parses_into_one_entry() {
        let items = parse_list("stash@{0}\tOn main: qingbird-git: 拉取前自动暂存\t2026-10-10 14:02:33 +0800\n");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].index, 0);
        assert_eq!(items[0].branch, "main");
        assert_eq!(items[0].when, "2026-10-10 14:02");
        assert!(items[0].auto);
    }

    #[test]
    fn a_newer_entry_is_not_mislabeled_because_of_its_position() {
        // 序号从 `stash@{N}` 里读，**不按行号数**：git 给的就是 N，
        // 按行号数的话，将来清单加了过滤（只显示青鸟藏的）就会把序号全指错
        let items = parse_list(
            "stash@{2}\tWIP on dev: 1111111 甲\t2026-09-27 09:31:00 +0800\n\
             stash@{1}\tOn main: 先放一下\t2026-10-08 21:10:00 +0800\n",
        );
        assert_eq!(vec![2, 1], items.iter().map(|e| e.index).collect::<Vec<_>>());
        assert_eq!(items[1].branch, "main");
        assert!(!items[0].auto);
    }

    #[test]
    fn a_malformed_line_is_skipped_not_filled_with_defaults() {
        // 少一列 / 序号形状不对 ⇒ 跳过。**填默认值会造出一条"界面上看得见、
        // 但点下去动的是别的东西"的行**，那比清单少一行严重得多
        let items = parse_list(
            "stash@{0}\tOn main: 只两列\n\
             not-a-selector\tOn main: x\t2026-10-10 14:02:33 +0800\n\
             stash@{3}\tWIP on main: 2222222 乙\t2026-10-09 08:00:00 +0800\n",
        );
        assert_eq!(items.len(), 1, "只有形状完整的那一条该留下");
        assert_eq!(items[0].index, 3);
    }

    #[test]
    fn an_empty_or_failed_listing_is_an_empty_list() {
        assert!(parse_list("").is_empty());
        assert!(parse_list("\n\n").is_empty());
    }

    #[test]
    fn the_minute_cut_never_panics_on_short_or_non_ascii() {
        assert_eq!(to_minute("2026-10-10 14:02:33 +0800"), "2026-10-10 14:02");
        assert_eq!(to_minute("abc"), "abc");
        // 落在字符中间：`get(..16)` 给 None ⇒ 整串照给，不 panic
        assert_eq!(to_minute("2026-10-10 14:0２:33"), "2026-10-10 14:0２:33");
    }

    // ── 下面这一组**真跑 git** ────────────────────────────────────────────────
    //
    // ⚠️ 为什么纯解析那几条不够：**这一组第一次跑就把一条会让功能整个失效的错抓出来了**。
    // 格式串原本写的是 `--date=iso` + `%cd`，git 接受、命令成功、`parse_list` 的
    // 五条纯解析单测全绿——但真 git 把 `%gd` 也按 `--date` 展开了，选择器变成
    // `stash@{2026-10-10 19:49:29 +0800}` ⇒ 序号取不出来 ⇒ **清单永远空**、
    // 界面上那颗感叹号一次都不出现。"末端拿不到数"这件事只有真 git 能证（D73）

    use crate::git;
    use std::path::{Path, PathBuf};

    struct Fixture {
        root: PathBuf,
        work: PathBuf,
        /// 持到测试结束：这些用例都在派生 git，不能和取消测试（全局 CANCELLED）并行
        _git_lock: std::sync::RwLockReadGuard<'static, ()>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn ok(dir: &Path, args: &[&str]) -> git::GitOutput {
        let out = git::run_git(&dir.to_string_lossy(), args, None, std::time::Duration::from_secs(30));
        assert!(
            out.success,
            "git {args:?} 在 {} 失败（code={}, timed_out={}）：\n{}",
            dir.display(),
            out.code,
            out.timed_out,
            out.stderr.trim()
        );
        out
    }

    /// 造一个"有一条青鸟藏的暂存"的仓库：改一个已跟踪文件 + 造一个未跟踪文件，
    /// 然后 `stash push -u`。**未跟踪那一条必须一起造**——`-u` 收走的就是它，
    /// 而"取回之后文件回没回来"要看它才算数
    fn parked() -> Fixture {
        let _git_lock = crate::git::GIT_TEST_LOCK
            .read()
            .unwrap_or_else(|e| e.into_inner());
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("qbg-stash-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        ok(&root, &["init", "-q", "--initial-branch=main", "repo"]);
        let work = root.join("repo");
        // 身份要配进仓库：`stash push` 自己也要建 commit，没身份整条失败
        ok(&work, &["config", "user.email", "t@t"]);
        ok(&work, &["config", "user.name", "t"]);
        // 换行处理钉死：跑测试那台机器若 `core.autocrlf=true`，检出会带 CRLF，
        // 之后的 apply 就会报内容冲突（`fork.rs` 那组夹具为同一件事踩过）
        ok(&work, &["config", "core.autocrlf", "false"]);
        std::fs::write(work.join("tracked.txt"), "原始\n").unwrap();
        ok(&work, &["add", "tracked.txt"]);
        ok(&work, &["commit", "-q", "-m", "base"]);
        std::fs::write(work.join("tracked.txt"), "改了没提交\n").unwrap();
        std::fs::write(work.join("scratch.txt"), "未跟踪\n").unwrap();
        ok(&work, &["stash", "push", "-u", "-m", MSG_PULL]);
        Fixture { root, work, _git_lock }
    }

    fn path(f: &Fixture) -> String {
        f.work.to_string_lossy().to_string()
    }

    #[test]
    fn a_real_stash_fills_every_field_the_ui_needs() {
        let f = parked();
        let items = list(&path(&f));
        assert_eq!(items.len(), 1, "格式串若不被 git 接受，这里就是空表");
        let e = &items[0];
        assert_eq!(e.index, 0);
        assert_eq!(e.branch, "main", "分支要从 %gs 里拆出来");
        assert_eq!(e.note, MSG_PULL, "说明段应当正好是我们写进去的那句");
        assert!(e.auto, "青鸟藏的必须判成 auto");
        assert_eq!(e.subject, format!("On main: {MSG_PULL}"));
        // 时间到分钟：16 个字符（`2026-10-10 14:02`），秒与时区都切掉了
        assert_eq!(e.when.chars().count(), 16, "when={}", e.when);
        assert_eq!(e.when.matches(':').count(), 1, "只该剩时:分那一个冒号：{}", e.when);
        assert!(!e.when.contains("+08"), "时区该被切掉：{}", e.when);
    }

    #[test]
    fn count_and_the_list_are_the_same_number_from_the_same_place() {
        // 感叹号上写的数与点开后的清单**必须同出处**，否则会出现"说有三份、点开两份"
        let f = parked();
        let p = path(&f);
        assert_eq!(count(&p), list(&p).len());
        assert_eq!(count(&p), 1);
    }

    #[test]
    fn taking_back_returns_both_files_and_keeps_the_entry() {
        let f = parked();
        let p = path(&f);
        let items = list(&p);
        let r = do_apply(&p, items[0].index, &items[0].subject);
        assert!(r.ok, "{}", r.message);
        // 两样都回到工作区：已跟踪那处改动 + 未跟踪那个文件
        assert_eq!(std::fs::read_to_string(f.work.join("tracked.txt")).unwrap(), "改了没提交\n");
        assert!(f.work.join("scratch.txt").exists(), "未跟踪的那个也要回来");
        // ⚠️ **这一条仍留在清单里**（apply 不是 pop）。这条断言钉的是"取回语义"：
        // 谁哪天把它改成 pop，界面上那句"还留着"就成了假话，而清单会自己少一条
        assert_eq!(list(&p).len(), 1, "取回之后清单不该变空");
        // ⚠️ 结论句里那一条的名字**必须与清单那一行同一个形状**（剥掉"是谁存的"那一截）。
        // 帧上实测第一版这句是整条 `qingbird-git: 拉取前自动暂存`，而同一屏清单那行是
        // `拉取前自动暂存` ——同一格内容两个出处，本项目为这一类交过四次学费
        assert!(r.message.contains("已取回「拉取前自动暂存」"), "{}", r.message);
        assert!(!r.message.contains(AUTO_PREFIX), "结论句不许带归属前缀：{}", r.message);
    }

    #[test]
    fn a_stale_snapshot_cannot_make_us_touch_the_wrong_entry() {
        // 序号不是身份：藏两条，然后拿**旧快照**去丢第二条
        let f = parked();
        let p = path(&f);
        std::fs::write(f.work.join("tracked.txt"), "第二轮改动\n").unwrap();
        ok(&f.work, &["stash", "push", "-u", "-m", MSG_REBASE]);
        let stale = list(&p);
        assert_eq!(stale.len(), 2);
        // 假装用户点的是"当时那条"，但清单已经变了：把 expect 换成另一条的原文
        let r = do_drop(&p, stale[1].index, "这条早就不存在了");
        assert!(!r.ok);
        assert!(r.message.contains("清单已刷新"), "{}", r.message);
        assert_eq!(list(&p).len(), 2, "拒掉的时候一条都不许少");
    }

    #[test]
    fn dropping_really_removes_that_one_and_shifts_the_rest() {
        let f = parked();
        let p = path(&f);
        std::fs::write(f.work.join("tracked.txt"), "第二轮改动\n").unwrap();
        ok(&f.work, &["stash", "push", "-u", "-m", MSG_REBASE]);
        let items = list(&p);
        // 丢掉最新那条（index 0，变基前那句），留下拉取那条
        let r = do_drop(&p, 0, &items[0].subject);
        assert!(r.ok, "{}", r.message);
        let after = list(&p);
        assert_eq!(after.len(), 1);
        // ⚠️ **剩下的那条序号从 1 变成 0**：这就是为什么每次动手之后界面必须重读清单，
        // 拿着旧序号点第二条会动到别的东西——而 drop 不可逆
        assert_eq!(after[0].index, 0);
        assert_eq!(after[0].note, MSG_PULL);
    }
}
