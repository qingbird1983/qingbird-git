//! 阶段 17：把仓库里「多出来的 / 改动了的文件」清回云端状态。
//!
//! ## 两类东西，两种做法——**这个分流是本模块存在的全部理由**
//!
//! | 类别 | 做法 | 能不能撤销 |
//! |---|---|---|
//! | **未跟踪**（`??`，从没进过版本控制） | **真删**（`remove_file` / `remove_dir_all`） | 不能 |
//! | **已跟踪但被改 / 被删 / 被 add 过**（`M ` / ` D` / `D ` / `A `…） | **`git restore` 恢复成 HEAD** | 能（HEAD 就在那儿） |
//!
//! ⚠️ **已跟踪的那一类删文件是错的**，而且错得很安静：删掉之后 git 只是把它
//! 记成「本地删除」，那**仍然是本地变更**——用户看到的"这个仓库有本地改动"
//! 一个都没少，还平白丢掉了内容。
//!
//! ## 三条实测出来、不能想当然的规则
//!
//! **① 必须「先删未跟踪、后恢复已跟踪」，顺序反了会真的删掉刚恢复回来的文件。**
//! porcelain 会把一个**同时**满足两件事的目录报成两条：
//! `D  sub/inner.md`（已跟踪、暂存删了）与 `?? sub/`（这个目录现在整体未跟踪）。
//! 两项都勾上时：
//! - 先删后恢复 → `sub/` 连目录带文件删掉，`restore` 再把 `inner.md` 放回来 ✅
//! - 先恢复后删 → `restore` 把 `inner.md` 放回 `sub/`，紧接着 `remove_dir_all(sub/)`
//!   **把刚放回来的文件又删了**，仓库从此挂着一个 ` D sub/inner.md` ❌
//!   （2026-10-05 实测：`先恢复后删` 之后 `sub/inner.md` 在磁盘上 = `False`）
//!
//! **② 动手前要重扫一次并取交集。** `git restore` 碰上**任何一个**它不认识的
//! 路径就整条退出（退出码 1，同一批里别的路径**一个都没恢复**）。而对话框开着
//! 的时候清单可能已经过时了（后台扫描回来了、用户自己在终端里改了）。
//! 所以两份清单都拿**当下的** `changed_entries` 过一遍，对不上的记进 `skipped`
//! ——**静默地什么都不做比删错更需要防**。
//!
//! **③ `git status --porcelain` 默认把非 ASCII 路径转义成八进制**
//! （`"\346\231\256\351\200\232.txt"`）。拿这种路径去删，删的是一个
//! **磁盘上根本不存在的文件名**。所以 `changed_entries` 已经带
//! `-c core.quotePath=false`（Rust 侧一直是 `from_utf8_lossy` 按 UTF-8 解，
//! 拿到的是原始字节；这跟控制台代码页无关）。
//!
//! ## 安全阀
//!
//! `resolve_in_repo` 拒绝一切**不是仓库内相对路径**的输入（盘符、UNC、`..`），
//! 并对 `.git` 一票否决。这个模块做的是**不可逆的删除**，闸门放在最前面，
//! 不是放在"反正 git 给的路径不会是那样"。
//!
//! ## 谁会调它
//!
//! 只给**第三方仓库**（阶段 17 的 17-4）：那些仓库本地会无意间多出文件
//! （AI 分析留下的日志之类），而本项目不替它们提交，于是每次同步都提示
//! 「有本地变更」。自己的仓库要做这件事有正经工具链，不该在这里给一条
//! 「一键丢工作区」的路。

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::git;
use crate::repos::{self, ChangeEntry};
use crate::space::explain_delete_error;

const LOCAL_TIMEOUT: Duration = Duration::from_secs(60);

/// 一次清理的结果
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct PurgeOutcome {
    /// 真删掉的（未跟踪）
    pub deleted: Vec<String>,
    /// 恢复成 HEAD 的（已跟踪）
    pub restored: Vec<String>,
    /// **没动**：勾的时候在，动手时已经不是那个状态了
    pub skipped: Vec<String>,
    /// 没做成：`(路径, 人话原因)`
    pub failed: Vec<(String, String)>,
}

impl PurgeOutcome {
    /// 一句话结论。**成功与失败要同时看得见**——只报成功等于骗人
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.deleted.is_empty() {
            parts.push(format!("删除 {} 个", self.deleted.len()));
        }
        if !self.restored.is_empty() {
            parts.push(format!("恢复 {} 个", self.restored.len()));
        }
        if !self.skipped.is_empty() {
            parts.push(format!("{} 个已经不是那个状态了", self.skipped.len()));
        }
        if !self.failed.is_empty() {
            parts.push(format!("{} 个没成功", self.failed.len()));
        }
        if parts.is_empty() {
            return "没有需要清理的内容".to_string();
        }
        parts.join(" · ")
    }
}

/// 把一份变更清单分成「该真删」与「该恢复」两份。**纯函数**——分流是本模块
/// 最重要的判据，必须能在单测里钉住，不能只能靠真仓库看
pub fn split_for_purge(changes: &[ChangeEntry]) -> (Vec<String>, Vec<String>) {
    let mut untracked = Vec::new();
    let mut tracked = Vec::new();
    for c in changes {
        if c.untracked {
            untracked.push(c.path.clone());
        } else {
            tracked.push(c.path.clone());
        }
    }
    (untracked, tracked)
}

/// 按用户勾选的两份清单清理。`untracked` 真删，`tracked` 恢复成 HEAD。
///
/// 顺序固定是**先删后恢复**，理由见文件头 ①。
pub fn purge_changes(
    repo_path: &str,
    untracked: &[String],
    tracked: &[String],
) -> PurgeOutcome {
    let mut out = PurgeOutcome::default();
    if !repos::valid_repo(repo_path) {
        let msg = "不是 git 仓库".to_string();
        for p in untracked.iter().chain(tracked.iter()) {
            out.failed.push((p.clone(), msg.clone()));
        }
        return out;
    }

    // ② 动手前重扫一次。清单可能已经过时，而一个过时的路径会让整条
    // `restore` 退出且什么都不恢复
    let now = repos::changed_entries(repo_path);
    let (now_untracked, now_tracked) = split_for_purge(&now);

    // ---- 第一步：删未跟踪的 ----
    for p in untracked {
        if !now_untracked.contains(p) {
            out.skipped.push(p.clone());
            continue;
        }
        match delete_one(repo_path, p) {
            Ok(()) => out.deleted.push(p.clone()),
            Err(e) => out.failed.push((p.clone(), e)),
        }
    }

    // ---- 第二步：恢复已跟踪的 ----
    let restorable: Vec<String> = tracked
        .iter()
        .filter(|p| now_tracked.contains(p))
        .cloned()
        .collect();
    for p in tracked {
        if !now_tracked.contains(p) {
            out.skipped.push(p.clone());
        }
    }
    if !restorable.is_empty() {
        match restore_tracked(repo_path, &restorable) {
            Ok(()) => out.restored = restorable,
            Err(e) => {
                for p in restorable {
                    out.failed.push((p, e.clone()));
                }
            }
        }
    }
    out
}

/// 删掉一个未跟踪的路径。**目录要整棵删**——porcelain 报未跟踪目录时给的是
/// `scratch/` 这种带斜杠的一条，底下可能有几十个文件
fn delete_one(repo_path: &str, rel: &str) -> Result<(), String> {
    let full = resolve_in_repo(repo_path, rel)?;
    // 斜杠只是 git 告诉你"这是目录"的记号，join 之前先去掉
    let meta = std::fs::symlink_metadata(&full)
        .map_err(|e| format!("删除失败：找不到「{rel}」（{e}）"))?;
    if meta.is_dir() {
        std::fs::remove_dir_all(&full).map_err(|e| explain_delete_error(&e))
    } else {
        std::fs::remove_file(&full).map_err(|e| explain_delete_error(&e))
    }
}

/// 把已跟踪的路径恢复成 HEAD。索引与工作区**一起**恢复：
/// 只恢复工作区的话，`git add` 过的那份还留在索引里，仍然算本地变更
fn restore_tracked(repo_path: &str, paths: &[String]) -> Result<(), String> {
    // 路径从 stdin 走（`--pathspec-from-file=-` + `--pathspec-file-nul`）：
    // ① 不受 Windows 命令行 32k 上限约束——一个改了几百个文件的仓库
    //    按参数传会被系统直接截断
    // ② NUL 分隔，路径里有空格、换行、引号都不用转义
    let mut payload: Vec<u8> = Vec::new();
    for p in paths {
        // 跳过越界的：安全阀在删除那边也要留一道，不能只信一路
        if resolve_in_repo(repo_path, p).is_err() {
            continue;
        }
        payload.extend_from_slice(p.as_bytes());
        payload.push(0);
    }
    if payload.is_empty() {
        return Ok(());
    }
    let out = git::run_git_stdin(
        repo_path,
        &[
            "restore",
            "--source=HEAD",
            "--staged",
            "--worktree",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
        ],
        &payload,
        LOCAL_TIMEOUT,
    );
    if out.success {
        Ok(())
    } else {
        let msg = out.stderr.trim().to_string();
        Err(format!(
            "恢复失败：{}",
            if msg.is_empty() { "git 没给出原因" } else { &msg }
        ))
    }
}

/// 把一个相对路径落到仓库内的绝对路径上。**任何越界的写法都在这里被挡掉**
///
/// 挡的是：绝对路径、`C:foo` 这种盘符相对路径、UNC、`..`、以及 `.git` 本身。
/// 最后一条看着多余（porcelain 不会报 `?? .git/`），但本模块做的是不可逆删除，
/// 闸门不该建立在"上游不会给坏数据"上。
pub fn resolve_in_repo(repo_path: &str, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Err("路径是空的".into());
    }
    let p = Path::new(rel);
    let mut first = String::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => {
                if first.is_empty() {
                    first = s.to_string_lossy().to_string();
                }
            }
            // `./a.txt` 就是 `a.txt`，放行无妨
            Component::CurDir => {}
            // `..` / 盘符 / UNC 根 一律拒
            _ => return Err(format!("路径「{rel}」越出了仓库范围")),
        }
    }
    if first.eq_ignore_ascii_case(".git") {
        return Err("不能动 .git 目录".into());
    }
    Ok(Path::new(repo_path).join(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn e(status: &str, path: &str) -> ChangeEntry {
        let b = status.as_bytes();
        ChangeEntry {
            status: status.to_string(),
            path: path.to_string(),
            staged: b[0] != b' ' && b[0] != b'?',
            untracked: status.contains('?'),
        }
    }

    /// 一个真仓库，造出阶段 17 要处理的全部分类
    struct Repo {
        root: PathBuf,
    }

    impl Repo {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("qb17purge-{name}"));
            // git 的 object 文件是只读的，Windows 上 `remove_dir_all` 会
            // PermissionError —— 失败就算了，留给临时目录清理
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let r = Self { root };
            r.git(&["init", "-q"]);
            r.write("mod.txt", "原始\n");
            r.write("del.txt", "原始\n");
            r.write("sub/inner.md", "原始\n");
            r.git(&["add", "-A"]);
            r.git(&["commit", "-q", "-m", "初始"]);
            r
        }

        fn write(&self, rel: &str, content: &str) {
            let full = self.root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            if let Some(dir) = full.parent() {
                std::fs::create_dir_all(dir).unwrap();
            }
            std::fs::write(full, content).unwrap();
        }

        fn exists(&self, rel: &str) -> bool {
            self.root
                .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
                .exists()
        }

        fn read(&self, rel: &str) -> String {
            std::fs::read_to_string(self.root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)))
                .unwrap()
        }

        /// 必须成功的一条（造仓库用）。**失败时把 git 自己说的原因带出来**——
        /// 裸 assert 只会得到一个"git ["add", "-A"] 失败了"，等于没说是哪一步
        fn git(&self, args: &[&str]) {
            let out = Command::new("git")
                .arg("-C")
                .arg(&self.root)
                .args(["-c", "user.name=t", "-c", "user.email=t@x.invalid"])
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} 失败: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        /// 允许失败的一条（造某种状态用）
        fn try_git(&self, args: &[&str]) -> bool {
            Command::new("git")
                .arg("-C")
                .arg(&self.root)
                .args(["-c", "user.name=t", "-c", "user.email=t@x.invalid"])
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        }

        fn path(&self) -> String {
            self.root.to_string_lossy().to_string()
        }

        fn status(&self) -> String {
            let out = Command::new("git")
                .arg("-C")
                .arg(&self.root)
                .args(["-c", "core.quotePath=false", "status", "--porcelain"])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).to_string()
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// 分流判据：`??` 真删，其余恢复。**这是本模块最要紧的一条**
    #[test]
    fn untracked_is_deleted_and_the_rest_is_restored() {
        let (un, tr) = split_for_purge(&[
            e("??", "ai.log"),
            e("??", "scratch/"),
            e(" M", "mod.txt"),
            e(" D", "del.txt"),
            e("D ", "sub/inner.md"),
            e("A ", "added.txt"),
        ]);
        assert_eq!(un, vec!["ai.log", "scratch/"]);
        assert_eq!(tr, vec!["mod.txt", "del.txt", "sub/inner.md", "added.txt"]);
        // `A `（别人 add 过）在 HEAD 里根本不存在，restore 会把它撤掉——
        // 它**不是**未跟踪，走真删就是删一个 git 记得的文件
        assert!(!un.contains(&"added.txt".to_string()));
    }

    /// 真删未跟踪的、恢复已跟踪的，**两类都真的生效**
    #[test]
    fn both_kinds_land_on_different_operations() {
        let r = Repo::new("both");
        // 未跟踪：一个文件 + 一个目录
        r.write("ai.log", "日志\n");
        r.write("scratch/nested/t.tmp", "x");
        // 已跟踪：改了 + 删了 + 暂存删了
        r.write("mod.txt", "改了\n");
        std::fs::remove_file(r.root.join("del.txt")).unwrap();
        r.git(&["rm", "-q", "--cached", "sub/inner.md"]);

        let out = purge_changes(
            &r.path(),
            &["ai.log".to_string(), "scratch/".to_string()],
            &[
                "mod.txt".to_string(),
                "del.txt".to_string(),
                "sub/inner.md".to_string(),
            ],
        );
        assert!(out.failed.is_empty(), "不该有失败: {:?}", out.failed);
        assert_eq!(out.deleted, vec!["ai.log", "scratch/"]);
        assert_eq!(out.restored.len(), 3);
        assert!(out.skipped.is_empty(), "{:?}", out.skipped);
        // 真删的两个不在了
        assert!(!r.exists("ai.log"));
        assert!(!r.exists("scratch"));
        // 恢复的三个回来了，且**内容回到了 HEAD 那一份**
        //
        // ⚠️ 判据是"改动没了"而不是逐字节相等：这台机器上 git 检出按 CRLF
        // 落盘（core.autocrlf），拿 HEAD 里那份 `\n` 去比必然对不上。
        // 那是 git 的事，不是这一层该管的
        let back = r.read("mod.txt");
        assert!(back.contains("原始"), "恢复的不是 HEAD 那份: {back:?}");
        assert!(!back.contains("改了"), "改动还在: {back:?}");
        assert!(r.exists("del.txt"));
        assert!(r.exists("sub/inner.md"));
        // 整个仓库回到干净
        assert_eq!(r.status().trim(), "", "还有残留: {:?}", r.status());
    }

    /// ⚠️ **先删后恢复**这条顺序是实锤出来的，不是风格问题：
    /// 反过来做会把**刚恢复回来的文件又删掉**
    #[test]
    fn delete_must_run_before_restore() {
        let r = Repo::new("order");
        // sub/inner.md 已跟踪且被暂存删了 → porcelain 同时报 `D  sub/inner.md`
        // 与 `?? sub/`（这个目录现在整体未跟踪）。两项都勾上
        r.git(&["rm", "-q", "--cached", "sub/inner.md"]);
        r.write("sub/junk.tmp", "垃圾\n");
        let s = r.status();
        assert!(s.contains("D  sub/inner.md"), "实况变了: {s:?}");
        assert!(s.contains("?? sub/"), "实况变了: {s:?}");

        let out = purge_changes(
            &r.path(),
            &["sub/".to_string()],
            &["sub/inner.md".to_string()],
        );
        assert!(out.failed.is_empty(), "{:?}", out.failed);
        // **这一条就是顺序的判据**：文件在磁盘上
        assert!(
            r.exists("sub/inner.md"),
            "先恢复后删会把刚放回来的文件又删掉——所以顺序不能反"
        );
        assert_eq!(r.status().trim(), "");
    }

    /// 过时的清单不许让 git 整条退出
    ///
    /// ⚠️ `git restore` 碰上**任何一个**它不认识的路径就整条放弃
    /// （退出码 1，同一批里别的路径一个都没恢复）。所以动手前要重扫取交集，
    /// 对不上的记 `skipped`——**静默地什么都不做比删错更需要防**
    #[test]
    fn a_stale_list_is_skipped_not_passed_to_git() {
        let r = Repo::new("stale");
        r.write("mod.txt", "改了\n");
        let out = purge_changes(
            &r.path(),
            &[],
            &["mod.txt".to_string(), "早就被人删掉的.txt".to_string()],
        );
        assert!(out.failed.is_empty(), "{:?}", out.failed);
        assert_eq!(out.restored, vec!["mod.txt"], "有效的那一个照常恢复");
        assert_eq!(out.skipped, vec!["早就被人删掉的.txt"]);
        assert_eq!(r.status().trim(), "");
    }

    /// 没勾的东西一个字都不许动——**这是「默认全不勾」的后端保证**，
    /// 不依赖界面那一层忘了传空数组
    #[test]
    fn an_empty_request_changes_nothing() {
        let r = Repo::new("empty");
        r.write("ai.log", "日志\n");
        r.write("mod.txt", "改了\n");
        let before = r.status();
        let out = purge_changes(&r.path(), &[], &[]);
        assert_eq!(out.summary(), "没有需要清理的内容");
        assert!(out.deleted.is_empty() && out.restored.is_empty());
        assert_eq!(r.status(), before, "空清单却动了工作区");
        assert!(r.exists("ai.log"));
    }

    /// 越界与 `.git` 一律挡在门外。**这是不可逆删除前的闸门**
    #[test]
    fn paths_outside_the_repo_are_refused() {
        let root = "C:\\some\\repo";
        for bad in [
            "",
            "   ",
            "..",
            "..\\..\\Windows",
            "a/../../b",
            "\\绝对路径",
            "C:\\Windows",
            "C:相对盘符",
            "//server/share",
        ] {
            assert!(
                resolve_in_repo(root, bad).is_err(),
                "「{bad}」竟然通过了安全阀"
            );
        }
        // 正常相对路径放行
        assert!(resolve_in_repo(root, "a.txt").is_ok());
        assert!(resolve_in_repo(root, "sub/deep/a.txt").is_ok());
        assert!(resolve_in_repo(root, "./a.txt").is_ok());
        // `.git` 及其子路径一票否决
        assert!(resolve_in_repo(root, ".git").is_err());
        assert!(resolve_in_repo(root, ".git/config").is_err());
        assert!(resolve_in_repo(root, ".GIT").is_err());
        // 名字里带 git 的普通文件不受影响
        assert!(resolve_in_repo(root, "github.md").is_ok());
    }

    /// 结论那句话**成功与失败必须同时看得见**
    #[test]
    fn the_summary_never_hides_the_failures() {
        let ok = PurgeOutcome {
            deleted: vec!["a".into(), "b".into()],
            restored: vec!["c".into()],
            ..Default::default()
        };
        assert_eq!(ok.summary(), "删除 2 个 · 恢复 1 个");
        let bad = PurgeOutcome {
            deleted: vec!["a".into()],
            failed: vec![("b".into(), "目录被占用".into())],
            ..Default::default()
        };
        let s = bad.summary();
        assert!(s.contains("删除 1 个") && s.contains("1 个没成功"), "{s}");
        assert_eq!(
            PurgeOutcome::default().summary(),
            "没有需要清理的内容"
        );
    }

    /// 被占用的文件删不掉要说出人话，**不能是 `(os error 32)`**
    #[test]
    fn a_undeletable_file_reports_a_reason() {
        let r = Repo::new("busy");
        std::fs::write(r.root.join("busy.log"), "占着\n").unwrap();
        // 目录当文件删 → 系统会拒绝
        let out = purge_changes(&r.path(), &["不存在的.log".to_string()], &[]);
        assert_eq!(out.deleted.len(), 0);
        assert_eq!(out.failed.len(), 0);
        assert_eq!(out.skipped.len(), 1, "清单过期 → 跳过而不是报失败");
    }
}
