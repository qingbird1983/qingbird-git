//! 仓库管理：状态读取、增删改查。平移自 `src-tauri/commands/repos.rs`，去 Tauri 依赖。
//!
//! 只搬界面真正要用的那一部分：Screen 3（我的仓库工作台）需要仓库列表与实时状态，
//! 其余（初始化向导、一键扫描、语言统计、提交历史）随对应屏一起迁，不提前铺。

use crate::git;
use crate::store::{self, AppConfig, RepoEntry};
use crate::types::RepoStatus;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
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

/// 从 git URL 解析仓库名（用于克隆时决定目标目录名）。
///
/// 兼容三类写法：`https://host/owner/repo.git`、`ssh://git@host/owner/repo.git`、
/// 以及 ssh 短格式 `git@host:owner/repo.git`。末尾斜杠与 `.git` 后缀都去掉。
///
/// **返回 `None` 而不是"尽力给一个"**：这个名字会直接成为磁盘上的目录名，
/// 白名单之外的字符（`/ \ : * ? " < > |` 等）一律拒绝——让 `clone` 自己
/// 去撞 Windows 的文件名规则，错误信息对用户没有意义。
pub fn repo_name_from_url(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let tail = match url.rfind(':').filter(|_| url.contains('@')) {
        // ssh 短格式 user@host:owner/repo.git
        Some(pos) => &url[pos + 1..],
        None => {
            // https:// / ssh:// 之外也可能是裸的 host/owner/repo
            let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
            // host/owner 是一**页**，owner/repo 才是一个仓库。少于此就说明
            // 用户只贴了主页地址——拿它当目录名会克隆出一个叫 `github.com`
            // 的目录，错误信息对用户毫无意义，不如当场说"解析不出仓库名"
            if rest.split('/').count() < 3 {
                return None;
            }
            rest.rsplit('/').next()?
        }
    };
    // `rsplit('/')` 兜一遍：短格式里 owner/repo 两段都要能取到 repo
    let tail = tail.rsplit('/').next().unwrap_or(tail);
    let name = tail.strip_suffix(".git").unwrap_or(tail);
    let valid = !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-. ".contains(c));
    valid.then(|| name.to_string())
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
        // 上游的全名就是这条命令的 stdout，界面上要显示"跟踪分支"——
        // 只留一个 success 就把它扔了，等于让界面再问 git 一遍
        st.upstream = upstream.stdout.trim().to_string();
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

/// 一条未提交变更。给提交对话框的「变更摘要」用。
#[derive(Clone, Debug, PartialEq)]
pub struct ChangeEntry {
    /// git 的两字母状态码，如 `M ` / `??` / `A ` / `R `。原样留着，
    /// 界面上按字符分色比让 core 翻译一遍更不容易失真
    pub status: String,
    /// 路径。已暂存的给相对仓库根的路径，未跟踪的同理。
    /// 未跟踪目录带一个结尾斜杠（`scratch/`）——那是 git 在说"这是目录"，
    /// 底下可能有几十个文件，删的时候要整棵删
    pub path: String,
    /// 状态码里 **X 位**（第一个字符）非空格 = 在暂存区里。
    /// ⚠️ 是**第一个**不是第二个：git 的 `XY` 里 X 是索引、Y 是工作区，
    /// 而本字段判的是索引
    pub staged: bool,
    /// 状态码里含 `?` = 未跟踪（还没进过版本控制）
    pub untracked: bool,
}

/// 列出未提交变更。给提交对话框用——`changed_files` 只给个数，
/// 但"我要提交什么"这件事得让人看见文件名才敢按确定。
///
/// 排序交给界面（它要知道按哪种顺序讲），这里只给 git 的原始顺序。
pub fn changed_entries(repo_path: &str) -> Vec<ChangeEntry> {
    if !valid_repo(repo_path) {
        return Vec::new();
    }
    parse_porcelain(&run_local(repo_path, &QUOTE_PATH_OFF_STATUS).stdout)
}

/// 关掉路径转义的 `git status --porcelain` 参数。
///
/// ⚠️ **不带 `core.quotePath=false` 时，git 把非 ASCII 路径转义成八进制**
/// （`"\\346\\231\\256\\351\\200\\232.txt"`）。代价有两层：
/// ① 界面上列出来的是一串谁都认不出的数字（变更页签一直是这样）；
/// ② 阶段 17 拿这种路径去删，删的是一个**磁盘上根本不存在的文件名**。
///
/// Rust 侧一直是 `String::from_utf8_lossy` 解码（与控制台代码页无关），
/// 所以拿到的就是原始 UTF-8 字节，解析没有问题。
const QUOTE_PATH_OFF_STATUS: [&str; 4] = [
    "-c",
    "core.quotePath=false",
    "status",
    "--porcelain",
];

/// 解析 `git status --porcelain` 的输出。
///
/// 每行固定是 `XY <path>`：两个状态字符、一个空格、然后路径。
/// **纯函数**，不碰 git——解析规则（两字符状态、空格分隔、重命名箭头）
/// 跟 git 子进程一点关系没有，混在跑命令的函数里就只能靠真仓库测。
pub fn parse_porcelain(stdout: &str) -> Vec<ChangeEntry> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        // 长度至少 4：`XY` + 空格 + 至少一个字符
        if line.len() < 4 {
            continue;
        }
        let b = line.as_bytes();
        if b[2] != b' ' {
            continue; // 形状不对的行（理论上不该有）直接跳过
        }
        let status = line[..2].to_string();
        let path = line[3..].trim().to_string();
        if path.is_empty() {
            continue;
        }
        // 重命名时 git 给 `old -> new`，展示只要新的那个
        let path = match path.rsplit_once(" -> ") {
            Some((_, new)) => new.to_string(),
            None => path,
        };
        out.push(ChangeEntry {
            untracked: status.contains('?'),
            // `XY` 里 **X 是暂存区、Y 是工作区**。搞反了会把"我 add 过"
            // 报成"我改了但没 add"，而提交对话框的摘要正是靠这一列分组。
            // `?`（未跟踪）要单独排除：它压根不在索引里，X 位那个 `?`
            // 只是 git 表示"没见过这个文件"，不是"在暂存区里"
            staged: b[0] != b' ' && b[0] != b'?',
            status,
            path,
        });
    }
    out
}

/// 最近一次提交的标题（`git log -1 --pretty=%s`）。
/// 提交对话框拿它**预填**提交说明——用户多半是在改上一件事，
/// 从空开始想措辞比接着上一句改难得多。空仓库 / 不是仓库时给空串。
pub fn last_commit_subject(repo_path: &str) -> String {
    if !valid_repo(repo_path) {
        return String::new();
    }
    let out = run_local(repo_path, &["log", "-1", "--pretty=format:%s"]);
    if !out.success {
        return String::new();
    }
    out.stdout.trim().to_string()
}

// ---------- 仓库增删改查 ----------

pub fn list_repos() -> Vec<RepoEntry> {
    store::load().repos
}

// ---------- 初始化向导：把普通文件夹变成仓库 ----------

/// 文件夹顶层的一项（初始化时用来勾选排除项）
#[derive(Clone, Debug, PartialEq)]
pub struct DirItem {
    pub name: String,
    pub is_dir: bool,
}

/// 列出文件夹顶层内容。目录排前面，各自按名称排；`.git` 不列（本来就要忽略）。
pub fn list_dir_items(path: &str) -> Vec<DirItem> {
    let mut items: Vec<DirItem> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(path) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name == ".git" {
                continue;
            }
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            items.push(DirItem { name, is_dir });
        }
    }
    items.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    items
}

/// 把非 git 文件夹初始化为仓库：`git init` → 勾选的排除项写进 `.gitignore`
/// → 初始提交 → 加入管理列表。
///
/// 排除项只**追加**，不动已有 `.gitignore` 的内容（用户自己的规则不能被覆盖）。
pub fn init_repo(path: &str, excludes: &[String]) -> Result<RepoEntry, String> {
    let path = path.trim().to_string();
    let dir = std::path::Path::new(&path);
    if !dir.is_dir() {
        return Err("文件夹不存在".into());
    }
    if valid_repo(&path) {
        return Err("该文件夹已经是 git 仓库".into());
    }

    // -b main：老版本 git 不认这个参数，失败就退回不带参数的 init
    let init = run_local(&path, &["init", "-b", "main"]);
    if !init.success {
        let fallback = run_local(&path, &["init"]);
        if !fallback.success {
            return Err(format!("git init 失败：{}", fallback.stderr.trim()));
        }
    }

    if !excludes.is_empty() {
        let gi = dir.join(".gitignore");
        let mut content = String::new();
        if gi.exists() {
            content = std::fs::read_to_string(&gi).unwrap_or_default();
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
        }
        content.push_str("# 由青鸟 Git 添加的排除项\n");
        for e in excludes {
            content.push_str(e.trim());
            content.push('\n');
        }
        std::fs::write(&gi, content).map_err(|e| format!("写入 .gitignore 失败：{e}"))?;
    }

    run_local(&path, &["add", "-A"]);

    let id = crate::net::identity_config(&store::load());
    let mut argv: Vec<String> = id;
    argv.extend(["commit".to_string(), "-m".to_string(), "init: 初始提交".into()]);
    let argv: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
    let commit = run_local(&path, &argv);
    if !commit.success {
        // 没配 Git 身份是最常见的失败原因，说清楚而不是甩一句「提交失败」
        let msg = if commit.stderr.contains("Please tell me who you are")
            || commit.stderr.contains("unable to auto-detect email")
        {
            "仓库已初始化，但初始提交失败：还没有配置 Git 身份，请在「设置 → Git 身份」里填写姓名和邮箱"
        } else {
            "仓库已初始化，但初始提交失败"
        };
        return Err(format!("{msg}：{}", commit.stderr.trim()));
    }

    let mut cfg = store::load();
    let entry = RepoEntry {
        name: repo_name(&path),
        path: path.clone(),
        added_at: store::now_secs(),
        last_used: store::now_secs(),
        ..RepoEntry::default()
    };
    if !cfg.repos.iter().any(|r| r.path == path) {
        cfg.repos.push(entry.clone());
    }
    store::save(&cfg)?;
    Ok(entry)
}

/// 整理用户粘进来的路径：资源管理器「复制文件地址」给的是 `C:\repo`，
/// 复制文件链接给的可能是 `file:///C:/repo`，拖拽进来时常带引号与尾部斜杠。
/// 统一归一成能直接喂给 git 的形态（Windows 上正斜杠也没问题）。
pub fn normalize_path_input(s: &str) -> String {
    let s = s.trim().trim_matches('"').trim_matches('\'');
    let s = match s.strip_prefix("file:///") {
        Some(rest) => rest, // file:///C:/repo → C:/repo
        None => s,
    };
    let s = s.trim().to_string();
    // 尾部斜杠去掉（根目录 C:/ 除外）：git 命令看到 C:/repo/ 也能跑，
    // 但列表里同一个仓库会存成两条路径，之后去重、比对全都对不上
    if s.len() > 3 {
        s.trim_end_matches(['/', '\\']).to_string()
    } else {
        s
    }
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
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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

/// 多个仓库的提交活动合并成一条时间线。给总览页的柱状图用。
///
/// 与 `commit_activity` 的分工：单个仓库的图是「这个项目我干了多少活」，
/// 合并之后是「这阵子我一共干了多少活」——总览要回答的是后者。
/// **不补空白天**：哪天空着就在图上留空，调用方自己补齐更省事。
pub fn commit_activity_all(repo_paths: &[String], days: u32) -> Vec<ActivityDay> {
    let srcs: Vec<Vec<ActivityDay>> = repo_paths
        .iter()
        .map(|p| commit_activity(p, days))
        .collect();
    merge_activity(srcs)
}

/// 把若干条「日期 → 计数」合并成一条，按日期倒序（新的在前）。
///
/// 拆成独立纯函数是为了能单测：合并规则（同名相加、去重、排序）跟 git 一点关系没有，
/// 混在跑 git 的那个函数里就只能靠真仓库测。日期串固定 `YYYY-MM-DD`，
/// 字典序等于时间序，直接排就行，不用引日期库。
fn merge_activity(srcs: Vec<Vec<ActivityDay>>) -> Vec<ActivityDay> {
    let mut acc: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for src in srcs {
        for d in src {
            *acc.entry(d.date).or_insert(0) += d.count;
        }
    }
    let mut out: Vec<ActivityDay> = acc
        .into_iter()
        .map(|(date, count)| ActivityDay { date, count })
        .collect();
    out.sort_by(|a, b| b.date.cmp(&a.date));
    out
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
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LangStat {
    pub language: String,    pub percent: f32,
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

/// GitHub Linguist 给每种语言的显示色（`popular.yml` 里的 `#rrggbb`），
/// 返回不带 `#` 的 6 位大写 hex。
///
/// **为什么放在 core 而不是界面**：色值是一份**数据**（GitHub 的事实），
/// 不是设计档位。放界面就得在 `gui` 里再抄一份表，而两份表迟早会有一处
/// 抄错颜色——用户看到"TypeScript 是绿的"只会觉得是这软件配色乱，
/// 不会想到该去改哪一处。
///
/// **只覆盖 `lang_of_ext` 认得的那批语言**，与它一一对应：
/// 这张表多列了不参与统计的语言，那会在界面上变成永远走不到的死分支。
/// 认不出来的（含合并项「其他」）返回 `None`，由界面回落到主题色板
pub fn lang_color(lang: &str) -> Option<u32> {
    Some(match lang {
        "Rust" => 0xDEA584,
        "TypeScript" => 0x3178C6,
        "JavaScript" => 0xF1E05A,
        "Python" => 0x3572A5,
        "Go" => 0x00ADD8,
        "Java" => 0xB07219,
        "Kotlin" => 0xA97BFF,
        "Swift" => 0xF05138,
        "Ruby" => 0x701516,
        "PHP" => 0x4F5D95,
        "C" => 0x555555,
        "C++" => 0xF34B7D,
        "C#" => 0x178600,
        "Objective-C" => 0x438EFF,
        "Scala" => 0xC22D40,
        "Dart" => 0x00B4AB,
        "Lua" => 0x000080,
        "HTML" => 0xE34C26,
        "Vue" => 0x41B883,
        "Svelte" => 0xFF3E00,
        "CSS" => 0x563D7C,
        "SCSS" => 0xC6538C,
        "Less" => 0x1D365D,
        "Shell" => 0x89E051,
        "PowerShell" => 0x012456,
        "SQL" => 0xE38C00,
        "Zig" => 0xEC915C,
        "Solidity" => 0xAA6746,
        _ => return None,
    })
}

/// 拆一个根目录文件名：判断它是不是说明文档、作者有没有在名字里写明语言。
///
/// **大小写不敏感**（`README.md` 与 `readme.md` 是同一个）——大小写归一在这一层
/// 做，不摊给三个调用方各记一次。
///
/// 返回 `(排序档, 语言)`：
///
/// * 排序档越小越优先。**没有语言后缀的排在带后缀的前面**——
///   `README.md` 是默认那一份，`README_zh.md` 只在它不存在时才顶上
///   （用户明确说了要中/英切换，但没说"默认打开中文"）。
/// * 语言是**作者写下来的**：`Some(true)` 中文 / `Some(false)` 英文；
///   `None` 是没写，要靠正文判。
///
/// 认得的写法：`README.md` `readme` `README.txt` `README_zh.md` `README_CN.md`
/// `README-en.md` `README.zh-CN.md` `README_中文.md`。**不认得的语言后缀
/// 一律不算说明文档**——`README_api.md` 那种是 API 片段，不是给人读的整篇
fn readme_parts(name: &str) -> Option<(u8, Option<bool>)> {
    let lower = name.to_lowercase();
    let rest = lower.strip_prefix("readme")?;
    // 先摘扩展名，剩下的中段才是语言后缀
    let (mid, ext_rank) = match rest.rfind('.') {
        Some(i) => match &rest[i..] {
            ".md" | ".markdown" => (&rest[..i], 0u8),
            "" => (&rest[..i], 1),
            ".txt" => (&rest[..i], 2),
            _ => return None,
        },
        None => (rest, 1),
    };
    let tag = mid.trim_start_matches(['_', '-', '.', ' ']);
    let lang = if tag.is_empty() {
        None
    } else {
        // `zh-CN` / `zh_CN` 取第一段
        let t = tag.split(['-', '_', '.']).next().unwrap_or("");
        if matches!(t, "zh" | "cn" | "chs" | "cht" | "zhtw" | "中文") {
            Some(true)
        } else if matches!(t, "en" | "eng" | "english") {
            Some(false)
        } else {
            return None;
        }
    };
    Some((ext_rank + if lang.is_some() { 4 } else { 0 }, lang))
}

/// 正文中段判是不是中文。**只看开头 4000 字**——README 的头几行就是标题与
/// 摘要，那几行足够定性，整篇扫一遍是白花 IO。
///
/// 判据是汉字与拉丁字母的比例：中文 README 的汉字量和字母量同一量级，
/// 英文那份几乎为零。**阈值取 1:3 是有意放宽**——判错的后果只是
/// 「切换键两边其实是同一种语言」，而判得太严会让真正双语的仓库**没有**
/// 切换入口，那才是把用户要的功能弄没了。
///
/// ⚠️ **不把假名算进来**：日文文档会被判成"英文"而不是"中文"。这是个
/// 二选一模型（只有中 / 英两档），没有第三档给日文；但标成"中文"是**摆在
/// 界面上的错话**，标成"英文"顶多是在有切换键时不够准
fn text_is_zh(text: &str) -> bool {
    let head: String = text.chars().take(4000).collect();
    let cjk = head
        .chars()
        .filter(|c| matches!(*c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF))
        .count();
    let latin = head.chars().filter(|c| c.is_ascii_alphabetic()).count();
    cjk * 3 >= latin
}

/// 这个文件名算不算一份说明文档（`README.md` / `README_zh.md` / …）。
///
/// 界面上判断"这条链接是**切到另一份文档**、还是打开别的文件"要用它。
/// **命名规则只有 core 这一处**——界面自己再写一遍 `starts_with("readme")`
/// 的话，改了这边那边就会把一个真文档当成普通文件弹给系统
pub fn is_readme_name(name: &str) -> bool {
    readme_parts(name).is_some()
}

/// 候选表里的一条
///
/// **公开出来是为了让「另一份是哪一份」能脱离磁盘算**（`alt_of`）：
/// 界面切语言时正文已经在内存里，判据也该一起在内存里——不碰磁盘
/// 才谈得上"同步切换"。字段与内部的排序规则一一对应，**不要在别处
/// 另写一遍挑哪一份的规则**，那才是两处迟早漂的地方
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ReadmePart {
    /// 根目录下的文件名
    pub name: String,
    /// 判出来的语言（`true` 中文 / `false` 英文）
    pub zh: bool,
    /// 作者在**文件名**里写明了语言（`README_zh.md`）。比读正文猜可靠，
    /// 所以同一语言有多份时它排前面
    pub explicit: bool,
    /// 排序档，越小越优先
    pub rank: u8,
}

/// 扫仓库根目录，把能当说明文档的文件列出来，**默认那一份排在最前**
///
/// 判语言分两步：先看文件名后缀；没有后缀的（`README.md` 本身到底是中文还是
/// 英文，只有读一遍才知道）再读开头 4000 字判一次。**只读没有后缀的那几份**，
/// 带后缀的靠文件名就够了，不必为了判语言去读它
fn readme_cands(repo_path: &str) -> Vec<ReadmePart> {
    let Ok(rd) = std::fs::read_dir(repo_path) else {
        return Vec::new();
    };
    let mut cands: Vec<ReadmePart> = Vec::new();
    for entry in rd.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let Some((rank, lang)) = readme_parts(&name) else {
            continue;
        };
        let (zh, explicit) = match lang {
            Some(zh) => (zh, true),
            None => {
                let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
                (text_is_zh(&text), false)
            }
        };
        cands.push(ReadmePart {
            name,
            zh,
            explicit,
            rank,
        });
    }
    cands.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.name.cmp(&b.name)));
    cands
}

/// 仓库根目录下的说明文档候选表（默认那一份排在最前）
///
/// 界面把它**连同每一份的正文一起缓存**：切换语言时那份正文早就在内存里，
/// 不该再白走一趟后台读盘——那正是阶段 18 那道闪烁的来源
pub fn readme_index(repo_path: &str) -> Vec<ReadmePart> {
    if !valid_repo(repo_path) {
        return Vec::new();
    }
    readme_cands(repo_path)
}

/// 在仓库根目录找 README：不区分大小写，.md 优先于无后缀 / .txt，
/// **没有语言后缀的优先于带后缀的**（`README.md` 是默认那一份）
fn find_readme(repo_path: &str) -> Option<std::path::PathBuf> {
    let cands = readme_cands(repo_path);
    let first = cands.first()?;
    Some(std::path::Path::new(repo_path).join(&first.name))
}

/// 「另一语言那份说明文档」。界面上「中文 / English」两颗键就是从这里来的。
///
/// **参数是当前正在显示的那一份**，不是"主文档"——用户切到英文之后再点一次
/// 就要切回来，所以判据必须是"此刻屏幕上这份是哪种语言"。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ReadmeAlt {
    /// 另一个语言那份的文件名（根目录下）
    pub file: String,
    /// 那一份是中文（`true`）还是英文（`false`）
    pub zh: bool,
}

/// 在**已经拿到的候选表**上算「另一语言那份」。**纯函数，不碰磁盘。**
///
/// 抽出来是因为它现在有两条调用路径：扫仓库时从磁盘取候选表，
/// 界面切语言时从内存里取缓存的那份。**判据只有这一份实现**——
/// 界面自己再写一遍"另一种语言是哪一份"，改了一处忘了另一处，
/// 切过去就切不回来（或者切回来时挑了第三份同语言的）
pub fn alt_of(cands: &[ReadmePart], current: &str) -> Option<ReadmeAlt> {
    // 当前这份是哪种语言。
    //
    // ⚠️ **必须从候选表里查，不能"文件名没写后缀就当英文"**：`README.md`
    // 绝大多数中文项目是**中文正文 + 无后缀**，当成英文的话另一档算错、
    // 直接返回 None，切换入口整个消失（2026-10-05 单测抓到的）。
    // 候选表里那份的语言已经按正文判过了，直接取
    let cur_zh = cands
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(current))
        .map(|c| c.zh)
        .or_else(|| readme_parts(current).and_then(|(_, lang)| lang))
        .unwrap_or(false);
    // 另一档里挑一份：**作者写明语言后缀的优先**（他明确表达过意图，
    // 比我们从正文猜的可靠），其次才是排序档
    let other = cands
        .iter()
        .filter(|c| c.zh != cur_zh && !c.name.eq_ignore_ascii_case(current))
        .min_by_key(|c| (!c.explicit, c.rank, c.name.clone()));
    other.map(|c| ReadmeAlt {
        file: c.name.clone(),
        zh: c.zh,
    })
}

/// 从磁盘取候选表再算「另一份」。界面上那份走内存版（`alt_of`）
pub fn readme_alt(repo_path: &str, current: &str) -> Option<ReadmeAlt> {
    if !valid_repo(repo_path) {
        return None;
    }
    alt_of(&readme_cands(repo_path), current)
}

/// 读根目录下**每一份**说明文档的正文（文件名 → 正文）
///
/// 与 `read_readme_named` 的区别是它一次给全：界面要一次把「中文 / English」
/// 两颗键背后的正文都握在手里，切的时候才能同步换。**每份都过
/// `README_MAX_CHARS` 截断**——不然一份 45 MB 的 README 会整个进内存、
/// 再整个进 SQLite 缓存
pub fn readme_docs(repo_path: &str) -> BTreeMap<String, String> {
    if !valid_repo(repo_path) {
        return BTreeMap::new();
    }
    readme_cands(repo_path)
        .into_iter()
        .filter_map(|c| {
            let full = std::path::Path::new(repo_path).join(&c.name);
            let text = std::fs::read_to_string(full).ok()?;
            clamp_readme(text).map(|t| (c.name, t))
        })
        .collect()
}

/// README 原文的上限。`TextView::markdown` 要把整篇解析成布局，
/// 一份十万字的 README 会在打开页签的那一帧卡住界面。
/// 超了截断并在末尾留一句，界面照实告诉用户"没显示完"
pub const README_MAX_CHARS: usize = 120_000;

/// 按上限截一段原文，超了就在末尾留一句说明
fn clamp_readme(text: String) -> Option<String> {
    if text.chars().count() <= README_MAX_CHARS {
        return Some(text);
    }
    let mut out: String = text.chars().take(README_MAX_CHARS).collect();
    out.push_str("\n\n…… README 太长，已截断 …………\n");
    Some(out)
}

/// 读仓库根目录的 README **原文**（给「说明」页签整篇渲染用）。
///
/// 与 `read_readme` 的区别是这一个不做提炼：那边给概览卡一个标题 + 首段，
/// 这边要整篇 markdown。找不到 / 不是仓库时返回 None，调用方渲染可操作空态
pub fn read_readme_text(repo_path: &str) -> Option<String> {
    if !valid_repo(repo_path) {
        return None;
    }
    let file = find_readme(repo_path)?;
    clamp_readme(std::fs::read_to_string(&file).ok()?)
}

/// 读**指定**那一份说明文档（`readme_alt` 给的文件名）。
///
/// 名字由 `readme_alt` 从根目录扫出来，不是用户输入，所以不防穿越——
/// 但仍要确认它在根目录里、且确实是个说明文档名：调一次 `readme_parts`
/// 就够，代价可忽略，防的是"哪天有人把路径拼进去"那种改坏
pub fn read_readme_named(repo_path: &str, name: &str) -> Option<String> {
    if !valid_repo(repo_path) {
        return None;
    }
    if readme_parts(name).is_none() {
        return None;
    }
    clamp_readme(std::fs::read_to_string(std::path::Path::new(repo_path).join(name)).ok()?)
}

/// 实际读到的是仓库根目录下的哪个 README 文件名（`README.md` 之类）。
/// 界面上说"这份仓库的说明文档叫 README.md"比什么都不说要可信
pub fn readme_file_name(repo_path: &str) -> Option<String> {
    if !valid_repo(repo_path) {
        return None;
    }
    find_readme(repo_path).map(|p| {
        p.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::RepoEntry;

    /// `lang_of_ext` 的每一条臂各取一个能命中它的扩展名。
    ///
    /// **它同时是「语言色表不许漏项」的守门样本**：`lang_color` 对这里任何
    /// 一个语言返回 `None`，这条测试就红。加新语言时把它的扩展名也补进来——
    /// 漏补的后果是那一段没有固定色、退回主题色板循环，
    /// 而那在界面上看着只是"颜色换了"，没人会知道是漏了
    const LANG_SAMPLES: [(&str, &str); 28] = [
        ("rs", "Rust"),
        ("ts", "TypeScript"),
        ("js", "JavaScript"),
        ("py", "Python"),
        ("go", "Go"),
        ("java", "Java"),
        ("kt", "Kotlin"),
        ("swift", "Swift"),
        ("rb", "Ruby"),
        ("php", "PHP"),
        ("c", "C"),
        ("cpp", "C++"),
        ("cs", "C#"),
        ("m", "Objective-C"),
        ("scala", "Scala"),
        ("dart", "Dart"),
        ("lua", "Lua"),
        ("html", "HTML"),
        ("vue", "Vue"),
        ("svelte", "Svelte"),
        ("css", "CSS"),
        ("scss", "SCSS"),
        ("less", "Less"),
        ("sh", "Shell"),
        ("ps1", "PowerShell"),
        ("sql", "SQL"),
        ("zig", "Zig"),
        ("sol", "Solidity"),
    ];

    #[test]
    fn every_counted_language_has_a_github_color() {
        let mut seen = std::collections::BTreeSet::new();
        for (ext, want) in LANG_SAMPLES {
            let got = lang_of_ext(ext).unwrap_or_else(|| panic!("{ext} 不该被跳过"));
            assert_eq!(got, want, "扩展名 {ext} 认出的语言名变了，语言色表要一起改");
            let c = lang_color(got)
                .unwrap_or_else(|| panic!("{} 缺 GitHub 色，会退回主题色板", got));
            assert!(
                (0x000000..=0xFFFFFF).contains(&c),
                "{} 的色值 {c:#x} 不是 6 位 hex",
                got
            );
            assert!(seen.insert(got), "{} 在样本里出现了两次", got);
        }
        // 色值两两不同：抄错一行时它会撞上另一行，测试当场抓住
        let colors: std::collections::BTreeSet<_> =
            LANG_SAMPLES.iter().map(|(_, l)| lang_color(l).unwrap()).collect();
        assert_eq!(colors.len(), LANG_SAMPLES.len(), "色表里有两条撞成了同一个色");
    }

    #[test]
    fn the_merged_bucket_has_no_fixed_color() {
        // 「其他」是 `repo_languages` 合并出来的，里面混着好几种语言，
        // 给它一个固定色等于让用户以为"那块是某一种语言"
        assert_eq!(lang_color("其他"), None);
        assert_eq!(lang_color("Fortran"), None);
        assert_eq!(lang_color(""), None);
    }

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
    fn new_entry_defaults_do_not_bite() {        // 加字段后老配置仍要能读出来：全空值 = 未设置，不是崩溃
        let e = full_entry();
        assert!(e.cmd_runs.is_empty());
        assert!(e.pinned_cmds.is_empty());
    }
}

#[cfg(test)]
mod porcelain_tests {
    use super::*;

    #[test]
    fn porcelain_splits_status_and_path() {
        let got = parse_porcelain("M  src/main.rs\n?? target/\nA  new.txt\n");
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].status, "M ");
        assert_eq!(got[0].path, "src/main.rs");
        assert!(got[0].staged, "第二字符非空格 = 已暂存");
        assert!(!got[0].untracked);
        assert!(got[1].untracked, "?? 是未跟踪");
        assert!(!got[1].staged);
        assert_eq!(got[2].path, "new.txt");
    }

    /// `XY` 两列的分工：**X 是暂存区、Y 是工作区**。
    /// 搞反了会把"我 add 过"报成"我改了但没 add"
    #[test]
    fn staged_flag_comes_from_the_index_column() {
        // 已 add 过、之后又改了 → `AM`：X=A 在暂存区，Y=M 在工作区
        let got = parse_porcelain("AM build.rs\n");
        assert!(got[0].staged, "X 列非空格 = 进了暂存区");
        // 删了但没 add → ` D`：X 是空格，说明没进暂存区
        let got = parse_porcelain(" D gone.txt\n");
        assert!(!got[0].staged, "X 列是空格 = 只在工作区改了");
    }

    /// 重命名时 git 给 `旧 -> 新`，展示只要新的那个
    #[test]
    fn renames_keep_only_the_new_path() {
        let got = parse_porcelain("R  old/a.rs -> new/b.rs\n");
        assert_eq!(got[0].path, "new/b.rs");
    }

    /// 空输出与形状不对的行都不能变成一条假变更
    #[test]
    fn malformed_lines_are_skipped() {
        assert!(parse_porcelain("").is_empty());
        let got = parse_porcelain("\nXX\nM  ok.rs\n"); // "XX" 太短，第二行形状不对
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].path, "ok.rs");
    }

    /// 端到端：真仓库里改一个文件、add 一个，`changed_files` 与明细条数必须一致。
    /// 验的是"两处读的是同一份 git 输出"，解析规则上面已经纯函数验过了
    #[test]
    fn entries_of_a_real_repo_match_the_count() {
        let repo = test_repo();
        let root = repo.0.clone();
        std::fs::write(root.join("a.txt"), "x").unwrap();
        std::fs::write(root.join("b.txt"), "y").unwrap();
        sh(&root, &["add", "b.txt"]);

        let count = changed_files(root.to_str().unwrap());
        let entries = changed_entries(root.to_str().unwrap());
        assert_eq!(count as usize, entries.len());
        let staged = entries.iter().filter(|e| e.staged).count();
        assert_eq!(staged, 1, "只有 b.txt 进了暂存区");
        let untracked = entries.iter().filter(|e| e.untracked).count();
        assert_eq!(untracked, 1, "a.txt 还没被 git 知道");

        sh(&root, &["add", "."]);
        sh(&root, &["commit", "-q", "-m", "第一条提交"]);
        assert_eq!(
            last_commit_subject(root.to_str().unwrap()),
            "第一条提交",
            "提交标题要给提交对话框预填用"
        );

        drop(repo);
    }

    /// 空仓库（还没有任何提交）不该 panic，标题给空串
    #[test]
    fn subject_of_a_fresh_repo_is_empty() {
        let repo = test_repo();
        let root = repo.0.clone();
        assert_eq!(last_commit_subject(root.to_str().unwrap()), "");
        assert!(changed_entries(root.to_str().unwrap()).is_empty());
        drop(repo);
    }
}

#[cfg(test)]
mod init_tests {
    use super::*;

    #[test]
    fn dir_items_put_folders_first_and_hide_git() {
        let root = std::env::temp_dir().join(format!("qbg-items-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("beta")).unwrap();
        std::fs::write(root.join("aaa.txt"), "").unwrap();
        std::fs::write(root.join("zzz.txt"), "").unwrap();

        let items = list_dir_items(root.to_str().unwrap());
        assert_eq!(
            items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
            vec!["beta", "aaa.txt", "zzz.txt"],
            "目录在前、名称升序、.git 不出现"
        );
        assert!(items[0].is_dir && !items[1].is_dir);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn init_rejects_missing_dir_and_existing_repo() {
        let _guard = crate::git::GIT_TEST_LOCK
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("qbg-init-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // 不存在的目录
        assert_eq!(init_repo(root.to_str().unwrap(), &[]).unwrap_err(), "文件夹不存在");
        // 已经是仓库的目录：不能再 init 一次（那会重置已有历史）
        std::fs::create_dir_all(&root).unwrap();
        sh(&root, &["init", "-q"]);
        assert_eq!(
            init_repo(root.to_str().unwrap(), &[]).unwrap_err(),
            "该文件夹已经是 git 仓库"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 端到端：普通文件夹 → 仓库 + .gitignore + 初始提交。
    /// 初始提交要 Git 身份（设置的或全局的），本机没配时走错误分支——
    /// 两种结果都合法，前面几步（init / .gitignore）在两种情况下都验了。
    #[test]
    fn init_makes_a_repo_and_writes_excludes() {
        let _guard = crate::git::GIT_TEST_LOCK
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("qbg-init-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::write(root.join("node_modules/x.js"), "1").unwrap();
        std::fs::write(root.join("app.rs"), "fn main() {}").unwrap();

        match init_repo(
            root.to_str().unwrap(),
            &["node_modules".to_string(), "dist".to_string()],
        ) {
            Ok(entry) => {
                assert!(valid_repo(root.to_str().unwrap()));
                assert_eq!(entry.name, "qbg-init-".to_owned() + &std::process::id().to_string());
                // 排除项进了 .gitignore，且被 git 真的忽略掉
                let gi = std::fs::read_to_string(root.join(".gitignore")).unwrap();
                assert!(gi.contains("node_modules") && gi.contains("dist"));
                assert!(!run_local(root.to_str().unwrap(), &["status", "--porcelain"])
                    .stdout
                    .contains("node_modules"));
            }
            Err(e) => {
                // 没配 Git 身份时 init 与 .gitignore 都已生效，只卡在提交
                assert!(e.starts_with("仓库已初始化，但初始提交失败"), "实际错误：{e}");
                assert!(valid_repo(root.to_str().unwrap()));
                assert!(std::fs::read_to_string(root.join(".gitignore"))
                    .unwrap()
                    .contains("node_modules"));
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 已有的 .gitignore 内容不能被我们的追加冲掉
    #[test]
    fn init_appends_to_existing_gitignore() {
        let _guard = crate::git::GIT_TEST_LOCK
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!("qbg-init-gi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".gitignore"), "*.log\n").unwrap();
        let _ = init_repo(root.to_str().unwrap(), &["dist".to_string()]);
        let gi = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(gi.starts_with("*.log"), "原有规则要留着：{gi}");
        assert!(gi.contains("dist"));
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn pasted_paths_are_normalized() {
        // 资源管理器复制出来的裸路径
        assert_eq!(normalize_path_input("F:/AIwork/qingbird-git"), "F:/AIwork/qingbird-git");
        // 反斜杠版本保持可用（git 接受），只去尾部
        assert_eq!(normalize_path_input(r"F:\AIwork\qingbird-git\"), r"F:\AIwork\qingbird-git");
        // 带引号
        assert_eq!(normalize_path_input(r#""F:/repo""#), "F:/repo");
        // 文件链接
        assert_eq!(normalize_path_input("file:///F:/repo"), "F:/repo");
        // 前后空格
        assert_eq!(normalize_path_input("  F:/repo\n"), "F:/repo");
    }

    #[test]
    fn roots_keep_their_separator() {
        // C:/ 被削成「C:」就变成相对路径了
        assert_eq!(normalize_path_input("C:/"), "C:/");
        assert_eq!(normalize_path_input(r"C:\"), r"C:\");
    }

    #[test]
    fn empty_input_stays_empty() {
        assert_eq!(normalize_path_input(""), "");
        assert_eq!(normalize_path_input("   "), "");
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

    /// 合并规则：同名相加、去重、**按日期倒序**。
    /// 纯函数，不碰 git，所以不必持 `GIT_TEST_LOCK`
    #[test]
    fn merging_sums_same_days_and_sorts_newest_first() {
        let a = vec![
            ActivityDay { date: "2026-10-02".into(), count: 3 },
            ActivityDay { date: "2026-10-01".into(), count: 1 },
        ];
        let b = vec![
            ActivityDay { date: "2026-10-02".into(), count: 4 },
            ActivityDay { date: "2026-09-30".into(), count: 2 },
        ];
        let merged = merge_activity(vec![a, b]);
        assert_eq!(merged.len(), 3, "同一天只能出一条");
        assert_eq!(merged[0].date, "2026-10-02");
        assert_eq!(merged[0].count, 7, "3 + 4 要相加");
        assert_eq!(merged[1].date, "2026-10-01");
        assert_eq!(merged[2].date, "2026-09-30", "较旧的天要排在后面");
    }

    /// 空输入与全空输入都给空数组——图上没柱子，别给一条 count=0 的假数据
    #[test]
    fn merging_nothing_yields_nothing() {
        assert!(merge_activity(vec![]).is_empty());
        assert!(merge_activity(vec![Vec::new(), Vec::new()]).is_empty());
    }

    /// 端到端：两个真仓库的提交数应该相加。验的是「每个仓库都跑到」，
    /// 不是合并规则（那条上面已经纯函数验过了）
    #[test]
    fn activity_of_several_repos_is_summed() {
        let repo = test_repo();
        let root = repo.0.clone();
        let a = root.join("a");
        let b = root.join("b");
        for d in [&a, &b] {
            std::fs::create_dir_all(d).unwrap();
            sh(d, &["init", "-q", "--initial-branch=main"]);
            sh(d, &["config", "user.email", "t@example.com"]);
            sh(d, &["config", "user.name", "t"]);
            sh(d, &["config", "commit.gpgsign", "false"]);
            std::fs::write(d.join("f.txt"), "x").unwrap();
            sh(d, &["add", "."]);
            sh(d, &["commit", "-q", "-m", "one"]);
        }
        let paths = vec![
            a.to_string_lossy().to_string(),
            b.to_string_lossy().to_string(),
        ];
        let one = commit_activity(&paths[0], 30);
        let both = commit_activity_all(&paths, 30);
        assert!(!one.is_empty(), "刚提交的当天应该有记录");
        assert_eq!(
            both.iter().map(|d| d.count).sum::<u32>(),
            one.iter().map(|d| d.count).sum::<u32>() * 2
        );

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



    /// 「说明」页签要的是整篇原文：标题层级、表格、代码块都得原样进去，
    /// 所以这里既不提炼也不截短（只有超长上限那一条）
    #[test]
    fn readme_text_returns_whole_file_and_its_name() {
        let root = std::env::temp_dir().join(format!("qbg-readme-text-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // 不是仓库：两项都是 None，界面上给可操作空态而不是一片空白
        std::fs::create_dir_all(&root).unwrap();
        assert!(read_readme_text(root.to_str().unwrap()).is_none());
        assert!(readme_file_name(root.to_str().unwrap()).is_none());

        std::fs::create_dir_all(root.join(".git")).unwrap();
        // 只有 readme.txt：兜底也要认，界面照实说读到的是哪个文件
        std::fs::write(root.join("readme.txt"), "旧的 txt\n").unwrap();
        assert_eq!(
            readme_file_name(root.to_str().unwrap()).as_deref(),
            Some("readme.txt")
        );
        assert_eq!(
            read_readme_text(root.to_str().unwrap()).as_deref(),
            Some("旧的 txt\n")
        );

        // README.md 优先于 readme.txt，且大小写不敏感
        let md = "# 标题\n\n| 列 |\n|---|---|\n| a |\n";
        std::fs::write(root.join("README.md"), md).unwrap();
        assert_eq!(
            readme_file_name(root.to_str().unwrap()).as_deref(),
            Some("README.md")
        );
        assert_eq!(read_readme_text(root.to_str().unwrap()).as_deref(), Some(md));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 超长 README 必须截断并**在文末说清楚截断了**。整篇丢给渲染器会在
    /// 打开页签的那一帧卡住界面，而静默截断等于骗用户
    #[test]
    fn readme_text_truncates_and_says_so() {
        let root = std::env::temp_dir().join(format!("qbg-readme-long-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let long: String = "字".repeat(README_MAX_CHARS + 500);
        std::fs::write(root.join("README.md"), &long).unwrap();

        let got = read_readme_text(root.to_str().unwrap()).unwrap();
        assert!(got.starts_with("字".repeat(100).as_str()), "开头必须是原文");
        assert!(got.contains("已截断"), "截断了必须在文末留一句");
        assert!(
            got.chars().count() <= README_MAX_CHARS + 200,
            "截断后长度应贴近上限，而不是把全文端上去"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `readme_parts` 是整套中英切换的入口，名字解析错一处，
    /// 后面「认不出说明文档」「切错语言」两类毛病都会从它冒出来
    #[test]
    fn readme_name_parsing_covers_the_conventions() {
        // 不带后缀：默认那一份，语言留给正文判
        assert_eq!(readme_parts("readme.md"), Some((0, None)));
        assert_eq!(readme_parts("README.markdown"), Some((0, None)));
        assert_eq!(readme_parts("readme"), Some((1, None)));
        assert_eq!(readme_parts("readme.txt"), Some((2, None)));
        // 带语言后缀：分词都在语言上，且排序档一律大于无后缀的
        for n in ["readme_zh.md", "README_CN.md", "readme-en.md", "README_中文.md"] {
            let (rank, lang) = readme_parts(&n.to_lowercase()).unwrap_or_else(|| panic!("{n}"));
            assert!(lang.is_some(), "{n} 应认出语言");
            assert!(rank >= 4, "{n} 带后缀，必须排在 README.md 之后");
        }
        assert_eq!(readme_parts("readme_zh.md"), Some((4, Some(true))));
        assert_eq!(readme_parts("readme_en.md"), Some((4, Some(false))));
        // 地区后缀只取第一段
        assert_eq!(readme_parts("readme.zh-cn.md"), Some((4, Some(true))));
        // 不是说明文档：贡献指南、许可、API 片段都不该被当正文
        for n in ["contributing.md", "readme_api.md", "readme.rstx", "licence"] {
            assert_eq!(readme_parts(n), None, "{n} 不该被算成说明文档");
        }
        // 界面判"这条链接是切文档还是开文件"靠的是同一个口径
        assert!(is_readme_name("README_zh.md"));
        assert!(!is_readme_name("CONTRIBUTING.md"));
    }

    /// 中英双份：`README.md` 是中文、`README_en.md` 是英文，默认读前者，
    /// 切换给后者。**切过去之后再问一次要切回中文**——判据必须是"屏幕上
    /// 这份是什么语言"，不是"主文档是什么语言"
    #[test]
    fn readme_alt_switches_between_a_bilingual_pair() {
        let root = std::env::temp_dir().join(format!("qbg-readme-2lang-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let p = root.to_str().unwrap();
        let zh = "# 青鸟\n\n这是中文说明。\n";
        let en = "# Qingbird\n\nEnglish docs.\n";
        std::fs::write(root.join("README.md"), zh).unwrap();
        std::fs::write(root.join("README_en.md"), en).unwrap();

        // 默认那一份是 README.md（无后缀的优先）
        assert_eq!(readme_file_name(p).as_deref(), Some("README.md"));
        assert_eq!(read_readme_text(p).as_deref(), Some(zh));

        let a = readme_alt(p, "README.md").expect("有第二份就该给切换");
        assert_eq!(a.file, "README_en.md");
        assert!(!a.zh, "README_en.md 是英文");
        assert_eq!(read_readme_named(p, &a.file).as_deref(), Some(en));

        // 切到英文之后再问：要给回中文那一份
        let back = readme_alt(p, "README_en.md").expect("切过去就该能切回来");
        assert_eq!(back.file, "README.md");
        assert!(back.zh, "README.md 正文是中文");
    }

    /// ⚠️ **这条是阶段 18 的地基**：「另一份是哪一份」现在有两条路——
    /// 扫仓库时从磁盘取候选表（`readme_alt`），切换语言时从内存取缓存的那份
    /// （`alt_of`）。**两条必须给出同一个答案**，否则会出现"扫的时候说有
    /// 第二份、切的时候说没有"这类只在实机上才显形的分叉。
    /// 所以这里拿同一个目录，把两条路的结果逐个比过去
    #[test]
    fn alt_of_on_a_cached_index_agrees_with_the_disk_lookup() {
        let root = std::env::temp_dir().join(format!("qbg-readme-cached-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let p = root.to_str().unwrap();
        // 三份，且**两份中文**：候选多于两颗键能覆盖的范围，那份多出来的
        // 中文文档是"内存版会不会挑错"的照妖镜
        std::fs::write(root.join("README.md"), "# 标题\n\n这是中文正文。\n").unwrap();
        std::fs::write(root.join("README_zh.md"), "# 标题\n\n另一份中文。\n").unwrap();
        std::fs::write(root.join("README_en.md"), "# Title\n\nEnglish body.\n").unwrap();
        // 一份不认得的：`README_api.md` 不算说明文档，整张表里不该有它
        std::fs::write(root.join("README_api.md"), "# API\n\nfragment\n").unwrap();

        let index = readme_index(p);
        let names: Vec<&str> = index.iter().map(|c| c.name.as_str()).collect();
        // 同档（都是 .md + 语言后缀）按文件名排，所以 en 在 zh 前面
        assert_eq!(names, vec!["README.md", "README_en.md", "README_zh.md"]);
        assert!(
            !names.contains(&"README_api.md"),
            "不认得的后缀不算说明文档"
        );

        for cur in ["README.md", "README_zh.md", "README_en.md"] {
            assert_eq!(
                alt_of(&index, cur),
                readme_alt(p, cur),
                "内存版与磁盘版在 {cur} 上挑出了不同的一份"
            );
        }
        // ⚠️ **往返不是回到原处，而是落到"作者写明语言"的那一份**。
        // 三份候选里有两份中文，规则是"另一档里作者写明后缀的优先"，
        // 于是 README.md → English → **README_zh.md**，而不是回到 README.md。
        // 这是既有规则一直如此的行为（两版判据完全一致，见上面那条），
        // 如实记在这里：只有**恰好两份**候选时中英往返才是闭合的，
        // 而界面本来就只有两颗键——多出来那份同语言的文档谁也够不着
        let a = alt_of(&index, "README.md").expect("有两档语言就该给切换");
        assert_eq!(a.file, "README_en.md");
        let b = alt_of(&index, &a.file).expect("切过去就该能切回来");
        assert_eq!(b.file, "README_zh.md", "作者写明 _zh 的那份优先");
        assert!(b.zh);
        let c = alt_of(&index, &b.file).expect("再切一次");
        assert_eq!(c.file, "README_en.md");

        // 恰好两份时往返闭合——这才是界面上那颗键的日常情形
        let pair = [
            ReadmePart {
                name: "README.md".into(),
                zh: true,
                explicit: false,
                rank: 0,
            },
            ReadmePart {
                name: "README_en.md".into(),
                zh: false,
                explicit: true,
                rank: 4,
            },
        ];
        let p1 = alt_of(&pair, "README.md").unwrap();
        assert_eq!(p1.file, "README_en.md");
        assert_eq!(alt_of(&pair, &p1.file).unwrap().file, "README.md");
    }

    /// 切换语言时正文要一次握在手里，所以 `readme_docs` 得把**每一份**都给全，
    /// 少给一份就等于那颗键还得回后台读盘——那正是要消掉的闪烁。
    /// 顺带钉住超长那份也过截断（否则 45 MB 的 README 会整个进 SQLite 缓存）
    #[test]
    fn readme_docs_gives_every_candidate_and_clamps_the_huge_one() {
        let root = std::env::temp_dir().join(format!("qbg-readme-docs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let p = root.to_str().unwrap();
        let zh = "# 标题\n\n中文正文。\n";
        std::fs::write(root.join("README.md"), zh).unwrap();
        std::fs::write(root.join("README_en.md"), "# Title\n\nEnglish.\n").unwrap();
        let huge = "a".repeat(README_MAX_CHARS + 5000);
        std::fs::write(root.join("README_zh.md"), &huge).unwrap();

        let docs = readme_docs(p);
        assert_eq!(docs.len(), 3, "三份候选都要有正文：{docs:?}");
        assert_eq!(docs.get("README.md").map(String::as_str), Some(zh));
        assert_eq!(
            docs.get("README_en.md").map(String::as_str),
            Some("# Title\n\nEnglish.\n")
        );
        let clipped = docs.get("README_zh.md").expect("超长那份也在表里");
        assert!(
            clipped.chars().count() <= README_MAX_CHARS + 40,
            "超长那份必须截断，不能整个进缓存"
        );
        assert!(clipped.contains("已截断"), "截断了就要在文末说清楚");
    }

    /// 只有一份就没有切换键——`readme_alt` 返回 None，界面上那颗键不出现。
    /// 同时验一下**没写语言后缀的那份靠正文判**：README.md 是中文、另一份是
    /// 带 `_en` 后缀的英文，切换方向要判对
    #[test]
    fn readme_alt_is_none_for_a_single_file() {
        let root = std::env::temp_dir().join(format!("qbg-readme-1lang-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let p = root.to_str().unwrap();
        std::fs::write(root.join("README.md"), "# Title\n\nOnly English here.\n").unwrap();
        assert_eq!(readme_alt(p, "README.md"), None);

        // 反过来：README.md 写成中文，另一份带 _en 后缀
        std::fs::write(root.join("README.md"), "# 标题\n\n只有中文。\n").unwrap();
        std::fs::write(root.join("README_en.md"), "# Title\n\nEnglish.\n").unwrap();
        let a = readme_alt(p, "README.md").expect("正文是中文就该切到英文那一份");
        assert_eq!(a.file, "README_en.md");
        assert!(!a.zh);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `read_readme_named` 只放行"确实是个说明文档名"的输入。
    /// 它不是对外的用户输入，但**规则要在这里钉死**：哪天有人把任意路径
    /// 拼进来，这条就是拦住它的最后一道
    #[test]
    fn readme_named_refuses_anything_but_a_readme() {
        let root = std::env::temp_dir().join(format!("qbg-readme-named-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let p = root.to_str().unwrap();
        std::fs::write(root.join("README.md"), "# ok\n").unwrap();
        std::fs::write(root.join("secret.txt"), "不该被读到\n").unwrap();

        assert!(read_readme_named(p, "README.md").is_some());
        assert!(read_readme_named(p, "secret.txt").is_none());
        assert!(read_readme_named(p, "..\\..\\Windows\\win.ini").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 克隆时要拿 URL 里的最后一段当目录名，三类写法都得认
    #[test]
    fn repo_name_from_url_handles_three_forms() {
        assert_eq!(
            repo_name_from_url("https://github.com/user/hello-world.git"),
            Some("hello-world".into())
        );
        assert_eq!(
            repo_name_from_url("https://gitee.com/user/hello-world"),
            Some("hello-world".into())
        );
        assert_eq!(
            repo_name_from_url("git@github.com:user/repo.git"),
            Some("repo".into())
        );
        assert_eq!(
            repo_name_from_url("ssh://git@github.com/user/repo.git"),
            Some("repo".into())
        );
    }

    /// 解析不出来 / 名字里带 Windows 非法字符时一律 None：
    /// 这个字符串会直接变成磁盘目录名，宁可让 clone 去报错
    #[test]
    fn repo_name_from_url_rejects_unusable_names() {
        // 两边空白 + 结尾斜杠都裁掉
        assert_eq!(
            repo_name_from_url("  https://github.com/a/b.git  "),
            Some("b".into())
        );
        assert_eq!(
            repo_name_from_url("https://github.com/a/b/"),
            Some("b".into())
        );
        // 裸地址（没有 scheme）同样认
        assert_eq!(repo_name_from_url("github.com/a/b"), Some("b".into()));
        // **主页地址不是仓库地址**。旧 Tauri 版对 `.../user/` 返回
        // `Some("user")`，那样会克隆出一个叫 `user` 的目录再报
        // "Repository not found"；少一段直接说"解析不出仓库名"更有用
        assert_eq!(repo_name_from_url("https://github.com/user/"), None);
        assert_eq!(repo_name_from_url("https://github.com/"), None);
        assert_eq!(repo_name_from_url("github.com/user"), None);
        assert_eq!(repo_name_from_url("github.com"), None);
        // 空串与纯空白
        assert_eq!(repo_name_from_url(""), None);
        assert_eq!(repo_name_from_url("   "), None);
        // `.git` 剥完就空了
        assert_eq!(repo_name_from_url("https://github.com/a/.git"), None);
        // Windows 文件名非法字符
        assert_eq!(repo_name_from_url("https://h/a/b:c"), None);
        assert_eq!(repo_name_from_url("https://h/a/b*c"), None);
        assert_eq!(repo_name_from_url("https://h/a/b|c"), None);
        // 路径穿越：`.` 与 `..` 的字符都"合法"，但它们会让目标目录
        // 指到父目录去，必须单独挡掉
        assert_eq!(repo_name_from_url("https://h/a/.."), None);
        assert_eq!(repo_name_from_url("https://h/a/."), None);
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
