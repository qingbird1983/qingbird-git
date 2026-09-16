//! 提交历史详情：单条提交的文件级变更摘要，以及逐文件的 patch。
//!
//! 合并提交是这里的坑：`git show <merge>` 默认什么都不输出（只有 `-m` / `--cc` 才给 diff），
//! 所以先看父提交个数，合并提交改走 `git diff <hash>^ <hash>`——相对第一个父提交看变更，
//! 这也是各类 Git 客户端"点开合并提交"的做法。

use serde::Serialize;

use super::net::run_local;
use super::repos::valid_repo;
use super::shell::spawn_terminal;

/// 界面上最多列多少个文件：几万个文件的提交不该把界面卡死
const MAX_FILES: usize = 500;
/// patch 正文上限（字符数，中文按字符算）；超过就截断并给"去终端看"的出口
const MAX_PATCH_CHARS: usize = 200_000;

#[derive(Serialize, Clone, Debug)]
pub struct CommitFile {
    pub path: String,
    /// 二进制文件没有行数增减（git 的 numstat 输出「-」）
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
}

#[derive(Serialize, Clone, Debug)]
pub struct CommitDetail {
    pub hash: String,
    pub files: Vec<CommitFile>,
    /// 文件太多被截断
    pub truncated: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct CommitPatch {
    pub text: String,
    /// 太长被截断：界面据此给出「在终端查看完整 diff」
    pub truncated: bool,
}

/// 只接受 sha（十六进制，4..=64 位）。这个值会进 git 参数，必要时还会拼进终端命令，
/// 所以在入口挡一道，而不是指望调用方自觉
fn safe_hash(hash: &str) -> Result<String, String> {
    let h = hash.trim();
    if h.len() < 4 || h.len() > 64 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("提交标识不合法".into());
    }
    Ok(h.to_string())
}

/// 拼进终端命令前的转义：单引号是 PowerShell 的转义方式（`''`），
/// 换行/制表这类控制字符没法安全传递，直接拒绝
fn terminal_safe(s: &str, what: &str) -> Result<String, String> {
    if s.contains(['\n', '\r', '\0', '\t']) {
        return Err(format!("{what}含有无法安全传递的字符"));
    }
    Ok(s.replace('\'', "''"))
}

/// 该提交的父提交个数（0 = 初始提交）
fn parent_count(path: &str, hash: &str) -> usize {
    let out = run_local(path, &["rev-list", "--parents", "-n", "1", hash]);
    out.stdout.split_whitespace().count().saturating_sub(1)
}

/// "该提交相对第一个父提交"的差异命令参数（`git show` 或合并提交用的 `git diff`）
fn diff_args(path: &str, hash: &str, extra: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = if parent_count(path, hash) > 1 {
        vec!["diff".into(), format!("{hash}^"), hash.to_string()]
    } else {
        vec!["show".into(), "--format=".into(), hash.to_string()]
    };
    args.extend(extra.iter().map(|s| s.to_string()));
    args
}

fn run_diff(path: &str, args: &[String]) -> Result<String, String> {
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_local(path, &argv);
    if !out.success {
        return Err(format!("读取提交失败：{}", out.stderr.trim()));
    }
    Ok(out.stdout)
}

/// 解析 `--numstat` 输出：每行「增加\t删除\t路径」，二进制是「-\t-\t路径」。
/// 重命名会显示成「旧路径 => 新路径」，原样保留——git 自己也这么显示，硬拆容易拆错
fn parse_numstat(text: &str) -> Vec<CommitFile> {
    text.lines()
        .filter_map(|line| {
            let mut it = line.splitn(3, '\t');
            let a = it.next()?;
            let d = it.next()?;
            let p = it.next()?;
            let num = |s: &str| s.trim().parse::<u32>().ok();
            Some(CommitFile {
                path: p.trim().to_string(),
                additions: num(a),
                deletions: num(d),
            })
        })
        .collect()
}

/// 按字符截断（中文不会被切成半个字），并在末尾说明被截断了
fn clamp_patch(text: String) -> (String, bool) {
    if text.chars().count() <= MAX_PATCH_CHARS {
        return (text, false);
    }
    let mut out: String = text.chars().take(MAX_PATCH_CHARS).collect();
    out.push_str("\n…（diff 过长，此处已截断）\n");
    (out, true)
}

/// 某条提交的文件级变更摘要（合并提交按第一个父提交算）
#[tauri::command]
pub fn commit_detail(path: String, hash: String) -> Result<CommitDetail, String> {
    if !valid_repo(&path) {
        return Err("该文件夹不是 git 仓库".into());
    }
    let h = safe_hash(&hash)?;
    let text = run_diff(&path, &diff_args(&path, &h, &["--numstat"]))?;
    let mut files = parse_numstat(&text);
    let truncated = files.len() > MAX_FILES;
    files.truncate(MAX_FILES);
    Ok(CommitDetail {
        hash: h,
        files,
        truncated,
    })
}

/// 某个文件在这条提交里的 patch
#[tauri::command]
pub fn commit_patch(path: String, hash: String, file: String) -> Result<CommitPatch, String> {
    if !valid_repo(&path) {
        return Err("该文件夹不是 git 仓库".into());
    }
    let h = safe_hash(&hash)?;
    let text = run_diff(&path, &diff_args(&path, &h, &["--", &file]))?;
    let (text, truncated) = clamp_patch(text);
    Ok(CommitPatch { text, truncated })
}

/// 界面上截断的 diff 给一个真出口：在仓库目录打开终端并执行同样的查看命令
#[tauri::command]
pub fn open_commit_diff_in_terminal(
    path: String,
    hash: String,
    file: String,
) -> Result<(), String> {
    if !valid_repo(&path) {
        return Err("该文件夹不是 git 仓库".into());
    }
    let h = safe_hash(&hash)?;
    let f = terminal_safe(&file, "文件路径")?;
    // 与本模块读取 diff 时同一条判断：合并提交要看相对第一个父提交的差异
    let cmd = if parent_count(&path, &h) > 1 {
        format!("git diff {h}^ {h} -- '{f}'")
    } else {
        format!("git show {h} -- '{f}'")
    };
    spawn_terminal(&path, Some(&cmd))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    struct Temp {
        root: PathBuf,
        _git_lock: std::sync::RwLockReadGuard<'static, ()>,
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn sh(dir: &Path, args: &[&str]) -> crate::git::GitOutput {
        crate::git::run_git(
            &dir.to_string_lossy(),
            args,
            None,
            std::time::Duration::from_secs(20),
        )
    }

    fn ok(dir: &Path, args: &[&str]) -> String {
        let out = sh(dir, args);
        assert!(out.success, "git {args:?} 失败：{}", out.stderr.trim());
        out.stdout.trim().to_string()
    }

    /// 起一个真实仓库（这些用例要在真 git 上看 numstat 与合并提交的行为）
    fn repo(name: &str) -> Temp {
        let git_lock = crate::git::GIT_TEST_LOCK
            .read()
            .unwrap_or_else(|e| e.into_inner());
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("qbg-hist-{}-{n}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        ok(&root, &["init", "-q", "--initial-branch=main"]);
        ok(&root, &["config", "user.email", "t@example.com"]);
        ok(&root, &["config", "user.name", "t"]);
        ok(&root, &["config", "commit.gpgsign", "false"]);
        ok(&root, &["config", "core.autocrlf", "false"]);
        Temp {
            root,
            _git_lock: git_lock,
        }
    }

    fn write(dir: &Path, name: &str, body: &[u8]) {
        let p = dir.join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, body).unwrap();
    }

    fn commit(dir: &Path, msg: &str) -> String {
        ok(dir, &["add", "-A"]);
        ok(dir, &["commit", "-q", "-m", msg]);
        ok(dir, &["rev-parse", "HEAD"])
    }

    fn detail(t: &Temp, hash: &str) -> CommitDetail {
        commit_detail(t.root.to_string_lossy().to_string(), hash.to_string()).unwrap()
    }

    fn patch(t: &Temp, hash: &str, file: &str) -> CommitPatch {
        commit_patch(
            t.root.to_string_lossy().to_string(),
            hash.to_string(),
            file.to_string(),
        )
        .unwrap()
    }

    #[test]
    fn numstat_lines_are_parsed_including_binary() {
        let files = parse_numstat("12\t3\tsrc/main.rs\n-\t-\tlogo.png\n0\t7\tdocs/旧 文档.md\n");
        assert_eq!(files.len(), 3);
        assert_eq!((files[0].additions, files[0].deletions), (Some(12), Some(3)));
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(
            (files[1].additions, files[1].deletions),
            (None, None),
            "二进制文件没有行数增减"
        );
        assert_eq!(files[2].path, "docs/旧 文档.md", "中文与空格都要原样保留");
        // 杂行（如空行）直接跳过，不该 panic
        assert!(parse_numstat("").is_empty());
        assert!(parse_numstat("not a numstat line").is_empty());
    }

    #[test]
    fn long_patch_is_clamped_on_a_char_boundary() {
        let long = "改".repeat(MAX_PATCH_CHARS + 100);
        let (text, truncated) = clamp_patch(long);
        assert!(truncated);
        // 正文恰好留下前 MAX_PATCH_CHARS 个字符，且每个都是完整的「改」
        assert!(
            text.chars().take(MAX_PATCH_CHARS).all(|c| c == '改'),
            "截断处不能切出半个字"
        );
        assert_eq!(text.chars().nth(MAX_PATCH_CHARS), Some('\n'), "截断后紧跟说明行");
        assert!(text.ends_with("（diff 过长，此处已截断）\n"));

        // 没超上限时原样返回，不加任何东西
        let (short, truncated) = clamp_patch("abc".into());
        assert_eq!(short, "abc");
        assert!(!truncated);
        let (exact, truncated) = clamp_patch("改".repeat(MAX_PATCH_CHARS));
        assert_eq!(exact.chars().count(), MAX_PATCH_CHARS);
        assert!(!truncated, "刚好到上限不该被截断");
    }

    #[test]
    fn unsafe_hash_is_refused() {
        assert!(safe_hash("abc1234").is_ok());
        assert!(safe_hash("  abc1234  ").is_ok(), "首尾空白容忍");
        for bad in ["", "abc", "zzzzzz", "abc; rm -rf /", "abc\n1234", &"a".repeat(65)] {
            assert!(safe_hash(bad).is_err(), "{bad:?} 应被拒绝");
        }
    }

    #[test]
    fn terminal_arguments_are_escaped_or_refused() {
        // 单引号转义成两个，才不会被 PowerShell 当成字符串结尾
        assert_eq!(terminal_safe("it's.rs", "文件路径").unwrap(), "it''s.rs");
        assert!(terminal_safe("bad\nname.rs", "文件路径").is_err());
        assert!(terminal_safe("bad\tname.rs", "文件路径").is_err());
    }

    #[test]
    fn detail_and_patch_describe_a_normal_commit() {
        let t = repo("normal");
        write(&t.root, "src/main.rs", b"fn main() {}\n");
        write(&t.root, "docs/readme.md", b"hello\n");
        let hash = commit(&t.root, "first");

        let d = detail(&t, &hash);
        let names: Vec<&str> = d.files.iter().map(|f| f.path.as_str()).collect();
        assert!(names.contains(&"src/main.rs") && names.contains(&"docs/readme.md"));
        assert!(!d.truncated);

        let p = patch(&t, &hash, "src/main.rs");
        assert!(p.text.contains("+fn main() {}"), "patch 里应能看到新增行：{}", p.text);
        assert!(!p.truncated);
    }

    #[test]
    fn merge_commit_shows_changes_against_first_parent() {
        // `git show <merge>` 默认什么都不输出，这是本模块最容易被忽略的坑
        let t = repo("merge");
        write(&t.root, "base.txt", b"base\n");
        commit(&t.root, "base");
        ok(&t.root, &["checkout", "-q", "-b", "feature"]);
        write(&t.root, "feature.txt", b"feature\n");
        commit(&t.root, "feature work");
        ok(&t.root, &["checkout", "-q", "main"]);
        write(&t.root, "main.txt", b"main\n");
        commit(&t.root, "main work");
        ok(&t.root, &["merge", "--no-ff", "-q", "-m", "merge feature", "feature"]);
        let merge = ok(&t.root, &["rev-parse", "HEAD"]);
        assert_eq!(parent_count(&t.root.to_string_lossy(), &merge), 2, "这是个合并提交");

        let names: Vec<String> = detail(&t, &merge).files.iter().map(|f| f.path.clone()).collect();
        assert!(
            names.contains(&"feature.txt".to_string()),
            "合并提交要能看到它带进来的文件，实际：{names:?}"
        );
        let p = patch(&t, &merge, "feature.txt");
        assert!(p.text.contains("+feature"), "合并提交的 patch 不该是空的：{}", p.text);
    }

    #[test]
    fn detail_of_unknown_commit_is_an_error_not_a_panic() {
        let t = repo("bad");
        write(&t.root, "a.txt", b"a\n");
        commit(&t.root, "a");
        let err = commit_detail(t.root.to_string_lossy().to_string(), "deadbeef".into()).unwrap_err();
        assert!(!err.is_empty());
    }
}
