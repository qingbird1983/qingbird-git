//! 提交历史：最近提交列表、文件级变更摘要、逐文件 patch。
//!
//! 照搬 `src-tauri/src/commands/history.rs` 的规则，但去掉了 Tauri 依赖
//! （`#[tauri::command]` 与"开终端看 diff"那条——那要 `shell::spawn_terminal`，
//! 而 core 不碰界面侧的东西；GUI 那边自己拼命令，见 `os.rs`）。
//!
//! **一个必须保留的规则**：合并提交走 `git diff <hash>^ <hash>`——
//! `git show <merge>` 默认什么都不输出，只给 `-m` / `--cc` 才有 diff。
//! 少了这条，"点开一个 merge 提交看到空白"就会变成一个查不出原因的现象。

use serde::Serialize;

use crate::git::run_git;
use crate::net::run_local;
use crate::repos::valid_repo;
use std::time::Duration;

/// 本地只读 git 命令的超时。历史页签是随手点的，不能让它把界面卡住
const LOCAL_TIMEOUT: Duration = Duration::from_secs(10);
/// 一次列多少条提交
const MAX_LOG: usize = 200;
/// 单个提交的变更文件上限，超了截断（真实数据能到几千个文件）
const MAX_FILES: usize = 500;
/// patch 文本上限。汉字按字符截，不按字节——按字节会把一个汉字劈成半个
const MAX_PATCH_CHARS: usize = 200_000;

/// `git log --pretty=format:...` 的字段顺序：短 hash / 时间 / 标题。
///
/// 字段间用 **tab** 分隔，而解析侧用 `splitn(3, '\t')`：**第三个字段吃掉
/// 剩下的全部内容**，所以标题里带 tab 也不会被切碎（`git commit -m "a\tb"`
/// 是合法的）。用空格或 `\x1f` 都得额外转义，tab 是 git 自己的惯例。
const LOG_FIELDS: &str = "--pretty=format:%h\t%ct\t%s";

/// 列表里的一条提交
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct CommitSummary {
    /// 短 hash（7 位）
    pub hash: String,
    /// 提交时间（秒）
    pub ts: u64,
    /// 标题
    pub subject: String,
}

/// 单个提交改到的文件
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct CommitFile {
    pub path: String,
    /// 新增行数。`None` = 二进制文件，git 的 `--numstat` 给的是 `-`
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
}

/// 单个提交的文件级摘要
#[derive(Serialize, Clone, Debug)]
pub struct CommitDetail {
    pub hash: String,
    pub subject: String,
    pub ts: u64,
    pub files: Vec<CommitFile>,
    /// 文件太多被截断
    pub truncated: bool,
}

/// 单个文件的 patch
#[derive(Serialize, Clone, Debug)]
pub struct CommitPatch {
    pub path: String,
    pub text: String,
    /// 文本太长被截断
    pub truncated: bool,
}

/// 校验并规范化一个 hash。
///
/// **只接受十六进制**，长度 4–64。这不是多余的洁癖：这个字符串会直接拼进
/// `git diff` 的参数位置，放任任意字符就等于给了一个"往命令行里塞东西"的通道。
/// 界面里它总是来自我们自己列出来的 `git log` 输出，但 core 的每个公开函数
/// 都要能独立站得住
pub fn safe_hash(hash: &str) -> Result<String, String> {
    let h = hash.trim();
    if h.len() < 4 || h.len() > 64 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("提交标识不合法".into());
    }
    Ok(h.to_string())
}

/// 解析 `git log` 的输出。**纯函数**，解析规则与 git 无关，单独测
pub fn parse_log(stdout: &str) -> Vec<CommitSummary> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        // 三个字段，第三段吃掉全部剩余内容（标题里可以有 tab）
        let mut it = line.splitn(3, '\t');
        let (Some(hash), Some(ts), Some(subject)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let Ok(ts) = ts.trim().parse::<u64>() else {
            continue;
        };
        out.push(CommitSummary {
            hash: hash.trim().to_string(),
            ts,
            subject: subject.to_string(),
        });
    }
    out
}

/// 最近提交列表。空仓库 / 不是仓库时给空数组（不是错误——空仓库本来就没有历史）
pub fn commit_log(repo_path: &str, limit: usize) -> Vec<CommitSummary> {
    if !valid_repo(repo_path) {
        return Vec::new();
    }
    let n = limit.clamp(1, MAX_LOG);
    let out = run_git(
        repo_path,
        &["log", LOG_FIELDS, &format!("-n{n}")],
        None,
        LOCAL_TIMEOUT,
    );
    if !out.success {
        return Vec::new();
    }
    parse_log(&out.stdout)
}

/// 某个提交的标题与时间。`git_log` 的完整列表里已经有，但 Sheet 打开时
/// 只知道 hash，再单跑一条比重跑整份列表便宜
pub fn commit_head(repo_path: &str, hash: &str) -> Result<CommitSummary, String> {
    let h = safe_hash(hash)?;
    let out = run_git(
        repo_path,
        &["log", LOG_FIELDS, "-n", "1", &h],
        None,
        LOCAL_TIMEOUT,
    );
    if !out.success {
        return Err(format!("读取提交失败：{}", out.stderr.trim()));
    }
    parse_log(&out.stdout)
        .into_iter()
        .next()
        .ok_or_else(|| "找不到这个提交".to_string())
}

/// 这个提交有几个父。0 = 还没有第一次提交，>1 = 合并提交
fn parent_count(path: &str, hash: &str) -> usize {
    let out = run_local(path, &["rev-list", "--parents", "-n", "1", hash]);
    out.stdout.split_whitespace().count().saturating_sub(1)
}

/// "拿这个提交跟它父提交比"的 git 参数。
///
/// 合并提交走 `git diff <hash>^ <hash>`，普通提交走 `git show --format= <hash>`。
/// 拆成纯函数是因为"合并提交要用另一套参数"这条规则最容易在改代码时丢掉，
/// 而它丢掉之后的症状是"点开一个 merge 提交看到空白"——从界面上完全看不出原因
pub fn diff_args(hash: &str, parents: usize, extra: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = if parents > 1 {
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

/// 解析 `--numstat`：每行 `新增行数\t删除行数\t路径`。
///
/// 二进制文件两个数字都是 `-`，`parse::<u32>()` 会失败——**那正是要存成 `None`
/// 的情况**（界面上要显示"二进制"，不能显示 0）
pub fn parse_numstat(text: &str) -> Vec<CommitFile> {
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

/// patch 文本太长时截断，末尾留一句说明
pub fn clamp_patch(text: String) -> (String, bool) {
    if text.chars().count() <= MAX_PATCH_CHARS {
        return (text, false);
    }
    let mut out: String = text.chars().take(MAX_PATCH_CHARS).collect();
    out.push_str("\n…… diff 文本太长，已截断 …………\n");
    (out, true)
}

/// 单个提交的文件级摘要
pub fn commit_detail(repo_path: &str, hash: &str) -> Result<CommitDetail, String> {
    if !valid_repo(repo_path) {
        return Err("该文件夹不是 git 仓库".into());
    }
    let h = safe_hash(hash)?;
    let parents = parent_count(repo_path, &h);
    let args = diff_args(&h, parents, &["--numstat"]);
    let text = run_diff(repo_path, &args)?;
    let mut files = parse_numstat(&text);
    let truncated = files.len() > MAX_FILES;
    files.truncate(MAX_FILES);
    let head = commit_head(repo_path, &h).unwrap_or(CommitSummary {
        hash: h.clone(),
        ts: 0,
        subject: String::new(),
    });
    Ok(CommitDetail {
        hash: h,
        subject: head.subject,
        ts: head.ts,
        files,
        truncated,
    })
}

/// 单个文件在该提交里的 patch
pub fn commit_patch(repo_path: &str, hash: &str, file: &str) -> Result<CommitPatch, String> {
    if !valid_repo(repo_path) {
        return Err("该文件夹不是 git 仓库".into());
    }
    let h = safe_hash(hash)?;
    // 文件名是**原样**交给 git 的，不拼进 shell；`--` 之后的参数一律按路径解析，
    // 文件名以 `-` 开头也不会被当成选项
    let parents = parent_count(repo_path, &h);
    let args = diff_args(&h, parents, &["--", file]);
    let text = run_diff(repo_path, &args)?;
    let (text, truncated) = clamp_patch(text);
    Ok(CommitPatch {
        path: file.to_string(),
        text,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // 只测纯逻辑。`commit_log` / `commit_detail` 会派生 git 子进程，
    // 拿真实仓库测要持 `git::GIT_TEST_LOCK` 的读锁（见仓库里其他派生 git 的测试）

    #[test]
    fn parse_log_reads_hash_time_and_subject() {
        let out = "a1b2c3d\t1700000000\t修一下登录页\n\
                   e4f5a6b\t1699999999\tfeat: 加了个按钮\n";
        let got = parse_log(out);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].hash, "a1b2c3d");
        assert_eq!(got[0].ts, 1_700_000_000);
        assert_eq!(got[0].subject, "修一下登录页");
        assert_eq!(got[1].subject, "feat: 加了个按钮");
    }

    #[test]
    fn parse_log_keeps_a_tab_inside_the_subject() {
        // 标题里带 tab 是合法的（`git commit -m "a\tb"`）。
        // 用单 tab 分隔的话这一行会被切成四段、后面的全丢
        let out = "a1b2c3d\t1700000000\tfix\t带 tab 的标题\n";
        let got = parse_log(out);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].subject, "fix\t带 tab 的标题");
    }

    #[test]
    fn parse_log_skips_junk_instead_of_guessing() {
        // git 偶尔会在输出里混进警告行；时间解析不出来的直接丢，
        // 宁可少一条也不要给界面上塞一条时间戳是 0 的假记录
        let out = "warning: something\na1b2c3d\tnot-a-number\tsubject\n\
                   b2c3d4e\t1700000001\t真的那条\n";
        let got = parse_log(out);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].subject, "真的那条");
    }

    #[test]
    fn safe_hash_only_accepts_hex() {
        assert_eq!(safe_hash("a1b2c3d").unwrap(), "a1b2c3d");
        // 太短：git 的短 hash 至少 4 位，3 位可能撞库
        assert!(safe_hash("abc").is_err());
        assert!(safe_hash("").is_err());
        // 非十六进制 —— 这个函数的值就是"不会把任意字符串拼进 git 参数"
        assert!(safe_hash("a1b2;rm -rf /").is_err());
        assert!(safe_hash("--upload-pack=touch x").is_err());
        // 过长：sha256 也才 64
        assert!(safe_hash(&"a".repeat(65)).is_err());
    }

    #[test]
    fn merge_commits_diff_against_the_first_parent() {
        // 合并提交走 show 会输出空白（默认不给 diff），这是最容易丢掉的一条规则
        let merge = diff_args("abc1234", 2, &[]);
        assert_eq!(merge, vec!["diff", "abc1234^", "abc1234"]);

        let normal = diff_args("abc1234", 1, &[]);
        assert_eq!(normal, vec!["show", "--format=", "abc1234"]);

        // 根提交没有父，show 只能拿它自己当对比对象
        let root = diff_args("abc1234", 0, &[]);
        assert_eq!(root, vec!["show", "--format=", "abc1234"]);
    }

    #[test]
    fn diff_args_appends_extra_after_the_separator() {
        // `--` 必须在文件名之前：否则一个叫 `--stat` 的文件会被 git 当成选项
        let a = diff_args("abc1234", 1, &["--", "src/main.rs"]);
        assert_eq!(a, vec!["show", "--format=", "abc1234", "--", "src/main.rs"]);
    }

    #[test]
    fn numstat_keeps_binary_files_without_line_counts() {
        let text = "12\t3\tsrc/main.rs\n-\t-\tassets/logo.png\n0\t7\tsrc/old.rs\n";
        let got = parse_numstat(text);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].additions, Some(12));
        assert_eq!(got[0].deletions, Some(3));
        // 二进制：两个数字都是 `-`，存成 None，界面上才能显示"二进制"
        // 而不是谎报"改了 0 行"
        assert_eq!(got[1].additions, None);
        assert_eq!(got[1].deletions, None);
        assert_eq!(got[1].path, "assets/logo.png");
        assert_eq!(got[2].deletions, Some(7));
    }

    #[test]
    fn numstat_ignores_a_path_containing_a_tab_prefix() {
        // 重命名时 git 可能把路径写成 `old => new` 或 `{a => b}/c.rs`，
        // 整体当路径，不能再拆
        let text = "1\t1\tsrc/{old => new}/mod.rs\n";
        let got = parse_numstat(text);
        assert_eq!(got[0].path, "src/{old => new}/mod.rs");
    }

    #[test]
    fn patch_under_the_limit_is_returned_untouched() {
        let (text, cut) = clamp_patch("x".repeat(1000));
        assert!(!cut);
        assert_eq!(text.len(), 1000);
    }

    #[test]
    fn patch_over_the_limit_is_cut_on_a_char_boundary() {
        // 全部用汉字：按字节截会把最后一个汉字劈成半个，界面上出现乱码方块
        let src = "改".repeat(MAX_PATCH_CHARS + 50);
        let (text, cut) = clamp_patch(src);
        assert!(cut);
        // 被截下来的那一段必须全是完整汉字——半个字的表现就是替换符
        let kept: String = text.chars().take(MAX_PATCH_CHARS).collect();
        assert_eq!(kept, "改".repeat(MAX_PATCH_CHARS));
        // 后面接的是那句说明，末尾以换行收尾
        assert!(text.contains("已截断"));
        assert!(text.ends_with('\n'));
    }
}
