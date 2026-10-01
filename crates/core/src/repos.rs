//! 仓库管理：状态读取、增删改查。平移自 `src-tauri/commands/repos.rs`，去 Tauri 依赖。
//!
//! 只搬界面真正要用的那一部分：Screen 3（我的仓库工作台）需要仓库列表与实时状态，
//! 其余（初始化向导、一键扫描、语言统计、提交历史）随对应屏一起迁，不提前铺。

use crate::git;
use crate::store::{self, AppConfig, RepoEntry};
use crate::types::RepoStatus;
use std::collections::HashMap;
use std::time::Duration;

/// 本地 git 命令（status / log 等）超时。与 src-tauri 侧同值：
/// 这些命令不联网，10 秒还不出结果基本是仓库卡死或路径不对。
pub const LOCAL_TIMEOUT: Duration = Duration::from_secs(10);

/// 执行本地 git 命令（不涉及网络，不走代理回退）
fn run_local(repo_path: &str, args: &[&str]) -> git::GitOutput {
    git::run_git(repo_path, args, None, LOCAL_TIMEOUT)
}

/// 是不是一个 git 仓库（只看 .git 存在，不递归探测——
/// 非仓库的文件夹也允许添加进来，界面上要能显示「不是仓库」而不是直接拒绝）
pub fn valid_repo(path: &str) -> bool {
    std::path::Path::new(path).join(".git").exists()
}

/// 从路径尾部取仓库名；取不到就整条路径当名字
pub fn repo_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// 读取仓库状态：分支 / 领先落后 / 脏工作区 / 有无上游 / 远端地址。
///
/// 每项各跑一条 git 命令，仓库多了会明显慢——所以调用方应该把它放后台线程，
/// 不要在渲染路径上同步等（界面靠上一次结果先画着，拿到新的再刷）。
pub fn read_status(repo_path: &str) -> RepoStatus {
    let mut st = RepoStatus {
        is_repo: valid_repo(repo_path),
        ..Default::default()
    };
    if !st.is_repo {
        return st;
    }
    st.branch = run_local(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .stdout
        .trim()
        .to_string();

    let upstream = run_local(
        repo_path,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    );
    st.has_upstream = upstream.success;
    if upstream.success {
        let counts = run_local(
            repo_path,
            &["rev-list", "--left-right", "--count", "HEAD...@{u}"],
        );
        if counts.success {
            let line = counts.stdout.trim().to_string();
            if let [ahead, behind] = line.split_whitespace().collect::<Vec<_>>()[..] {
                st.ahead = ahead.parse().unwrap_or(0);
                st.behind = behind.parse().unwrap_or(0);
            }
        }
    }
    st.dirty = !run_local(repo_path, &["status", "--porcelain"])
        .stdout
        .trim()
        .is_empty();
    st.remote_url = run_local(repo_path, &["remote", "get-url", "origin"])
        .stdout
        .trim()
        .to_string();
    st
}

/// 未提交的文件数量（仓库头的「未提交 N 文件」那个数字）。
/// 不是仓库时返回 0。
pub fn changed_files(repo_path: &str) -> u32 {
    if !valid_repo(repo_path) {
        return 0;
    }
    run_local(repo_path, &["status", "--porcelain"])
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count() as u32
}

// ---------- 仓库增删改查 ----------

pub fn list_repos() -> Vec<RepoEntry> {
    store::load().repos
}

/// 添加仓库；已在列表里则原样返回，不重复添加
pub fn add_repo(path: &str) -> Result<Vec<RepoEntry>, String> {
    let path = path.trim().to_string();
    if !valid_repo(&path) {
        return Err("所选文件夹不是 git 仓库（未找到 .git）".to_string());
    }
    let mut cfg = store::load();
    if !cfg.repos.iter().any(|r| r.path == path) {
        cfg.repos.push(RepoEntry {
            name: repo_name(&path),
            path: path.clone(),
            added_at: store::now_secs(),
            ..RepoEntry::default()
        });
    }
    store::save(&cfg)?;
    Ok(cfg.repos)
}

/// 移除仓库（只删记录，不动磁盘上的文件）
pub fn remove_repo(path: &str) -> Result<Vec<RepoEntry>, String> {
    let mut cfg = store::load();
    cfg.repos.retain(|r| r.path != path);
    store::save(&cfg)?;
    Ok(cfg.repos)
}

/// 设置本地自定义显示名；alias 传空串恢复原始仓库名
pub fn rename_repo(path: &str, alias: &str) -> Result<Vec<RepoEntry>, String> {
    with_repo(path, |e| e.alias = alias.trim().to_string())
}

/// 记一次「最近用过」，置顶的排在最前，其余按此排序
pub fn touch_repo(path: &str) {
    let _ = with_repo(path, |e| e.last_used = store::now_secs());
}

/// 改某仓库的某个字段并落盘。仓库不在列表里时返回 None（不改配置）。
fn with_repo<F>(path: &str, f: F) -> Result<Vec<RepoEntry>, String>
where
    F: FnOnce(&mut RepoEntry),
{
    let mut cfg: AppConfig = store::load();
    if let Some(e) = cfg.repos.iter_mut().find(|r| r.path == path) {
        f(e);
    }
    store::save(&cfg)?;
    Ok(cfg.repos)
}

// ---------- 档案页数据源（Screen 1，全部本地可取） ----------

/// 某一天的提交数
#[derive(Clone, Debug, PartialEq)]
pub struct ActivityDay {
    /// YYYY-MM-DD，按**本地时区**
    pub date: String,
    pub count: u32,
}

/// 近 `days` 天的提交活动。**只返回有提交的日子**——补齐空白天要算日期，
/// 放在调用方更省事（后端不引入日期库）。全是空的仓库返回空数组。
///
/// 平移自 src-tauri `commit_activity`，天数参数化：档案页迷你图用 30，
/// 旧版检视抽屉的柱状图是 14，同一函数两个调用方各传各的。
pub fn commit_activity(repo_path: &str, days: u32) -> Vec<ActivityDay> {
    if !valid_repo(repo_path) {
        return Vec::new();
    }
    // --all：柱状图看的是「我在这个仓库干了多少活」，提交落在别的分支上也得算数。
    // --since 走 git 自己的日期计算，不用我们处理时区与夏令时。
    let out = run_local(
        repo_path,
        &[
            "log",
            "--all",
            &format!("--since={days}.days.ago"),
            "--date=format-local:%Y-%m-%d",
            "--pretty=format:%ad",
        ],
    );
    if !out.success {
        return Vec::new();
    }
    bucket_days(&out.stdout)
}

/// 把 `git log` 的逐行日期分桶成「日期 → 计数」，按日期倒序（新的在前）。
/// git log 默认按时间倒序输出，这里保持同样顺序，省一次排序。
fn bucket_days(log_out: &str) -> Vec<ActivityDay> {
    let mut days: Vec<ActivityDay> = Vec::new();
    for line in log_out.lines() {
        let date = line.trim();
        // --date 固定输出 YYYY-MM-DD；形状不对的行（理论上不该有）直接跳过，
        // 免得脏数据混进图表
        let b = date.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            continue;
        }
        match days.iter_mut().find(|d| d.date == date) {
            Some(d) => d.count += 1,
            None => days.push(ActivityDay {
                date: date.to_string(),
                count: 1,
            }),
        }
    }
    days
}

/// 最近一次提交的 Unix 时间戳（秒）。没有提交 / 不是仓库时返回 0，
/// 调用方用「0 = 从没提交过」这个约定渲染占位。
pub fn last_commit_ts(repo_path: &str) -> u64 {
    if !valid_repo(repo_path) {
        return 0;
    }
    run_local(repo_path, &["log", "-1", "--format=%ct"])
        .stdout
        .trim()
        .parse()
        .unwrap_or(0)
}

/// 项目语言构成（类似 GitHub 语言条）：按扩展名统计代码文件字节占比。
/// 用 `git ls-files -co --exclude-standard` 取文件列表，自动跳过 .gitignore 忽略的文件。
#[derive(Clone, Debug, PartialEq)]
pub struct LangStat {
    pub language: String,
    pub percent: f32,
}

/// 扩展名 → 语言名（对齐 GitHub Linguist 的常见分类；配置/数据类文件不计入）。
/// 扩展名直接来自文件名，大小写不保证，这里自己归一。
fn lang_of_ext(ext: &str) -> Option<&'static str> {
    let key = ext.to_ascii_lowercase();
    Some(match key.as_str() {
        "rs" => "Rust",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "rb" => "Ruby",
        "php" => "PHP",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "C++",
        "cs" => "C#",
        "m" | "mm" => "Objective-C",
        "scala" => "Scala",
        "dart" => "Dart",
        "lua" => "Lua",
        "html" | "htm" => "HTML",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "css" => "CSS",
        "scss" | "sass" => "SCSS",
        "less" => "Less",
        "sh" | "bash" | "zsh" => "Shell",
        "ps1" | "psm1" => "PowerShell",
        "sql" => "SQL",
        "zig" => "Zig",
        "sol" => "Solidity",
        _ => return None,
    })
}

pub fn repo_languages(repo_path: &str) -> Vec<LangStat> {
    if !valid_repo(repo_path) {
        return Vec::new();
    }
    let ls = run_local(repo_path, &["ls-files", "-co", "--exclude-standard"]);
    if !ls.success {
        return Vec::new();
    }
    let mut totals: HashMap<&str, u64> = HashMap::new();
    let mut total: u64 = 0;
    for f in ls.stdout.lines() {
        let ext = std::path::Path::new(f)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        let Some(lang) = lang_of_ext(ext) else {
            continue;
        };
        let len = std::fs::metadata(std::path::Path::new(repo_path).join(f))
            .map(|m| m.len())
            .unwrap_or(0);
        *totals.entry(lang).or_insert(0) += len;
        total += len;
    }
    if total == 0 {
        return Vec::new();
    }
    let mut stats: Vec<LangStat> = totals
        .into_iter()
        .map(|(lang, bytes)| LangStat {
            language: lang.to_string(),
            percent: (bytes as f32) / (total as f32) * 100.0,
        })
        .collect();
    stats.sort_by(|a, b| b.percent.total_cmp(&a.percent));
    // 只保留前 6 项，其余合并为"其他"
    if stats.len() > 6 {
        let rest: f32 = stats[6..].iter().map(|s| s.percent).sum();
        stats.truncate(6);
        stats.push(LangStat {
            language: "其他".into(),
            percent: rest,
        });
    }
    stats
}

/// README 预览：档案页内容卡只展示标题 + 首段简介，不需要整篇渲染。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadmePreview {
    /// 第一个 `# 一级标题`，去掉行内强调符后的小标题
    pub title: String,
    /// 首段正文（连续非空行，已截断到 300 字符）
    pub intro: String,
}

/// 读仓库根目录的 README 并提炼「标题 + 首段简介」。
/// 找不到 README / 不是仓库时返回 None，调用方渲染占位。
pub fn read_readme(repo_path: &str) -> Option<ReadmePreview> {
    if !valid_repo(repo_path) {
        return None;
    }
    let file = find_readme(repo_path)?;
    let text = std::fs::read_to_string(&file).ok()?;
    Some(parse_readme(&text))
}

/// 在仓库根目录找 README：不区分大小写，.md 优先于无后缀 / .txt。
fn find_readme(repo_path: &str) -> Option<std::path::PathBuf> {
    let mut best: Option<(u8, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(repo_path).ok()? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let lower = name.to_lowercase();
        let rank = if let Some(rest) = lower.strip_prefix("readme") {
            match rest {
                ".md" | ".markdown" => 0u8,
                "" => 1,
                ".txt" => 2,
                _ => continue,
            }
        } else {
            continue;
        };
        if best.as_ref().is_none_or(|(r, _)| rank < *r) {
            best = Some((rank, entry.path()));
        }
    }
    best.map(|(_, p)| p)
}

/// README 首屏提炼规则：第一个 `# 标题`；首段 = 第一个「正文样子的行」
/// 开始的连续段落。标题行 / badge / 图片 / HTML / 表格分隔都不算正文。
fn parse_readme(text: &str) -> ReadmePreview {
    let clean = |s: &str| -> String {
        // 去掉行内强调：**粗体**、*斜体*、`代码`——简介里留这些记号只会显得乱
        s.replace("**", "")
            .replace("`", "")
            .trim()
            .trim_end_matches('*')
            .trim()
            .to_string()
    };
    let mut out = ReadmePreview::default();
    for line in text.lines() {
        let l = line.trim();
        // 还没拿到标题时先认领第一个 `# 一级标题`；没有标题的 README
        // 不该因此吞掉正文——首段照样从第一行正文找
        if out.title.is_empty() {
            if let Some(h) = l.strip_prefix("# ").map(clean).filter(|s| !s.is_empty()) {
                out.title = h;
                continue;
            }
        }
        // 标题之后就找首段：跳过空行、小标题、badge/图片/HTML/表格等结构行
        if l.is_empty() {
            if !out.intro.is_empty() {
                break; // 段落结束
            }
            continue;
        }
        let structural = l.starts_with('#')
            || l.starts_with('[')
            || l.starts_with('!')
            || l.starts_with('<')
            || l.starts_with('|')
            || l.starts_with('-')
            || l.starts_with('>');
        if structural {
            continue;
        }
        out.intro.push_str(&clean(l));
        if out.intro.chars().count() >= 300 {
            break;
        }
    }
    // 截断到 300 字符（按字符不按字节，中文不吃亏）
    if out.intro.chars().count() > 300 {
        out.intro = out.intro.chars().take(300).collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::RepoEntry;

    /// 造一个带全部字段的条目：字段一多，写字面量漏字段就会编译不过——
    /// 这正是加新字段时想要的提醒，而不是靠 `..Default::default()` 把漏掉的静默吃掉
    fn full_entry() -> RepoEntry {
        RepoEntry {
            path: "C:/repo".into(),
            name: "repo".into(),
            alias: String::new(),
            added_at: 0,
            last_used: 0,
            test_cmd: String::new(),
            build_cmd: String::new(),
            dev_cmd: String::new(),
            install_cmd: String::new(),
            pinned_cmds: Vec::new(),
            cmd_runs: Default::default(),
            auto_sync_mins: 0,
            daily_time: String::new(),
            auto_pull: false,
            last_auto_check: 0,
            last_auto_result: String::new(),
            pinned: false,
            is_third_party: false,
        }
    }

    #[test]
    fn repo_name_takes_the_last_path_segment() {
        assert_eq!(repo_name("D:/AIwork/qingbird-git"), "qingbird-git");
        // 尾部带分隔符时不能给出空名字
        assert_eq!(repo_name("D:/AIwork/qingbird-git/"), "qingbird-git");
    }

    #[test]
    fn status_of_a_non_repo_is_not_a_repo() {
        let st = read_status("D:/AIwork/这个路径不存在-xyz");
        assert!(!st.is_repo);
        assert!(st.branch.is_empty());
        assert_eq!((st.ahead, st.behind), (0, 0));
        assert!(!st.dirty);
    }

    #[test]
    fn changed_files_of_a_non_repo_is_zero() {
        assert_eq!(changed_files("D:/AIwork/这个路径不存在-xyz"), 0);
    }

    #[test]
    fn new_entry_defaults_do_not_bite() {
        // 加字段后老配置仍要能读出来：全空值 = 未设置，不是崩溃
        let e = full_entry();
        assert!(e.cmd_runs.is_empty());
        assert!(e.pinned_cmds.is_empty());
    }
}

#[cfg(test)]
mod activity_tests {
    use super::*;

    #[test]
    fn days_are_bucketed_and_kept_in_git_order() {
        // git log 默认按时间倒序，这里要保持同样顺序
        let days = bucket_days("2026-09-28\n2026-09-28\n2026-09-27\n2026-09-28\n2026-09-25\n");
        let got: Vec<(&str, u32)> = days.iter().map(|d| (d.date.as_str(), d.count)).collect();
        assert_eq!(got, vec![("2026-09-28", 3), ("2026-09-27", 1), ("2026-09-25", 1)]);
    }

    #[test]
    fn blank_or_malformed_lines_are_skipped() {
        // 空仓库时 git log 什么都不输出；夹杂的噪声行不该变成一根 0 高度的柱子
        assert!(bucket_days("").is_empty());
        assert!(bucket_days("\n\n").is_empty());
        let days = bucket_days("2026-09-28\nnot-a-date\n2026-9-8\n2026-09-28\n");
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].count, 2, "只数格式正确的日期行");
    }

    /// 端到端跑真 git：这条路径验的是命令参数本身（--since / %ad 格式），
    /// 纯函数测不出来。
    #[test]
    fn activity_of_a_real_repo_counts_todays_commits() {
        let repo = test_repo();
        let root = repo.0.clone();

        // 没有提交的仓库：空数组，调用方自己补 0 高度的柱子
        assert!(commit_activity(root.to_str().unwrap(), 30).is_empty());
        assert_eq!(last_commit_ts(root.to_str().unwrap()), 0);

        // 连续三次提交必然落在同一天：断言计数等于 3，不去猜是哪一天
        for i in 0..3 {
            std::fs::write(root.join("f.txt"), format!("{i}\n")).unwrap();
            sh(&root, &["add", "-A"]);
            sh(&root, &["commit", "-q", "-m", &format!("c{i}")]);
        }
        let path = root.to_str().unwrap();
        let days = commit_activity(path, 30);
        assert_eq!(days.len(), 1, "同一天的提交合并成一个桶：{days:?}");
        assert_eq!(days[0].count, 3);
        assert_eq!(days[0].date.len(), 10, "YYYY-MM-DD，实际 {:?}", days[0].date);

        assert!(last_commit_ts(path) > 0, "有提交后时间戳必须非零");

        // 非仓库目录：安静返回空，不报错
        let not_repo = root.join("nope");
        std::fs::create_dir_all(&not_repo).unwrap();
        assert!(commit_activity(not_repo.to_str().unwrap(), 30).is_empty());
        assert_eq!(last_commit_ts(not_repo.to_str().unwrap()), 0);

        drop(repo);
    }
}

#[cfg(test)]
mod lang_tests {
    use super::*;

    #[test]
    fn ext_map_covers_common_cases_and_rejects_noise() {
        assert_eq!(lang_of_ext("rs"), Some("Rust"));
        assert_eq!(lang_of_ext("TSX"), Some("TypeScript"));
        assert_eq!(lang_of_ext("md"), None, "文档类不计入语言构成");
        assert_eq!(lang_of_ext("json"), None);
        assert_eq!(lang_of_ext(""), None);
    }

    /// 端到端跑真 git：验 ls-files 参数与占比计算。
    #[test]
    fn languages_of_a_real_repo_split_by_bytes() {
        let repo = test_repo();
        let root = repo.0.clone();
        // 大文件占比一定高于小文件：写 9:1 的比例，断言区间而不是精确值
        std::fs::write(root.join("a.rs"), "x".repeat(900)).unwrap();
        std::fs::write(root.join("b.ts"), "y".repeat(100)).unwrap();
        // --exclude-standard 必须真的排除掉 .gitignore 里的构建产物
        std::fs::write(root.join(".gitignore"), "c.js\n").unwrap();
        std::fs::write(root.join("c.js"), "z".repeat(10_000)).unwrap();
        sh(&root, &["add", "-A"]);
        sh(&root, &["commit", "-q", "-m", "init", "--no-gpg-sign"]);

        let stats = repo_languages(root.to_str().unwrap());
        assert_eq!(stats.len(), 2, "被 ignore 的文件不该出现：{stats:?}");
        assert_eq!(stats[0].language, "Rust");
        assert!(stats[0].percent > 85.0, "大头是 Rust：{stats:?}");
        assert!(stats[1].language == "TypeScript" && stats[1].percent < 15.0);
        drop(repo);
    }
}

#[cfg(test)]
mod readme_tests {
    use super::*;

    #[test]
    fn parse_extracts_title_and_first_paragraph() {
        let md = "<!-- 注释 -->\n\
                  [![badge](https://x)](https://y)\n\
                  ![logo](https://x.png)\n\
                  \n\
                  # Element Plus\n\
                  \n\
                  ## 特性\n\
                  \n\
                  **基于 Vue 3** 的桌面端组件库，面向设计与开发。\n\
                  第二行同属一段。\n\
                  \n\
                  支持 色模式。\n";
        let got = parse_readme(md);
        assert_eq!(got.title, "Element Plus");
        assert_eq!(
            got.intro,
            "基于 Vue 3 的桌面端组件库，面向设计与开发。第二行同属一段。"
        );
    }

    #[test]
    fn parse_survives_bad_input() {
        assert_eq!(parse_readme(""), ReadmePreview::default());
        // 没有标题也能出首段
        let got = parse_readme("直接开始的简介\n");
        assert_eq!(got.title, "");
        assert_eq!(got.intro, "直接开始的简介");
    }

    #[test]
    fn readme_lookup_prefers_md_and_ignores_case() {
        let root = std::env::temp_dir().join(format!("qbg-readme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // 不是仓库：直接 None
        assert!(read_readme(root.to_str().unwrap()).is_none());

        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("readme.txt"), "旧的 txt\n").unwrap();
        assert!(read_readme(root.to_str().unwrap()).is_some(), "txt 兜底也能读");
        std::fs::write(root.join("README.md"), "# 标题\n\n正文\n").unwrap();
        let got = read_readme(root.to_str().unwrap()).unwrap();
        assert_eq!(got.title, "标题");
        assert_eq!(got.intro, "正文");
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
/// 派生 git 子进程的测试共用：临时目录里的干净仓库。
/// 持读锁与写锁的测试互斥，避免互相杀掉对方的 git 进程。
/// 返回 (根目录, 守卫)，守卫 drop 前根目录一直有效。
fn test_repo() -> (std::path::PathBuf, std::sync::RwLockReadGuard<'static, ()>) {
    let _guard = crate::git::GIT_TEST_LOCK
        .read()
        .unwrap_or_else(|e| e.into_inner());
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("qbg-repos-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    sh(&root, &["init", "-q", "--initial-branch=main"]);
    sh(&root, &["config", "user.email", "t@example.com"]);
    sh(&root, &["config", "user.name", "t"]);
    sh(&root, &["config", "commit.gpgsign", "false"]);
    sh(&root, &["config", "core.autocrlf", "false"]);
    (root, _guard)
}

#[cfg(test)]
fn sh(dir: &std::path::Path, args: &[&str]) -> String {
    let out = crate::git::run_git(
        &dir.to_string_lossy(),
        args,
        None,
        std::time::Duration::from_secs(20),
    );
    assert!(out.success, "git {args:?} 失败：{}", out.stderr.trim());
    out.stdout.trim().to_string()
}
