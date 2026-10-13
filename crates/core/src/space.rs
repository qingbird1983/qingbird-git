//! 仓库磁盘占用扫描、大文件排查、可清理缓存目录与 .gitignore 建议。
//!
//! 纯同步实现：调用方（GUI）负责把它丢到后台线程，core 不管异步。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::git;
use crate::net::LOCAL_TIMEOUT;
use crate::repos;

/// 可清理的构建缓存目录名白名单（均为可再生的构建产物 / 依赖缓存）。
/// 同时用作两个判定：容量面板里的"可清理"、以及可以建议写进 `.gitignore` 的目录
const CACHE_DIRS: &[&str] = &[
    "target",
    "node_modules",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    "coverage",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".venv",
    "venv",
    ".gradle",
    "Debug",
    "Release",
    ".dart_tool",
];

/// 多大的文件值得单独拎出来（10 MB）
const BIG_FILE_MIN: u64 = 10 * 1024 * 1024;
/// "最大的文件"最多列几条，够用户定位膨胀来源即可
const BIG_FILE_KEEP: usize = 10;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BigFile {
    /// 相对仓库根目录的路径（分隔符统一成 `/`），展示用
    pub name: String,
    /// 所在文件夹的绝对路径，供"打开所在文件夹"用（只接受目录）
    pub dir: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IgnoreSuggestion {
    /// 顶层目录名
    pub name: String,
    /// 目录里已经有被 git 跟踪的文件：写进 `.gitignore` 不会取消跟踪，
    /// 得靠 `git rm -r --cached`。标出来是为了不让按钮给人"已经修好了"的错觉
    pub tracked: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpaceReport {
    /// 仓库文件夹总占用（字节）
    pub total: u64,
    /// 一级子目录 / 文件按大小排序
    pub dirs: Vec<DirEntry>,
    /// 最大的若干个工作区文件（不含 `.git` 与缓存目录）
    pub largest: Vec<BigFile>,
    /// 存在但没被 git 忽略的缓存目录
    pub ignorable: Vec<IgnoreSuggestion>,
}

impl SpaceReport {
    /// **可清理的字节数——全站唯一一处算式。**
    ///
    /// 这一格以前有三处各写一遍（仓库头那句摘要、空间页图例、头卡那四格读数），
    /// 而两处写的是 `is_cache`、一处写的是 `is_cache && size > 0`。
    /// 今天求和结果一样（0 不贡献），**但"哪些目录算可清理"这条规则已经有两个版本**——
    /// 下一轮只要有人给空目录加一句"也要列出来"，三处就会给出三个数。
    /// ⇒ 收成这一个方法，判据取**更严格那一版**（`size > 0`）
    pub fn cache_bytes(&self) -> u64 {
        self.dirs
            .iter()
            .filter(|d| d.is_cache && d.size > 0)
            .map(|d| d.size)
            .sum()
    }
}

/// 仓库内跑一条只读 git 命令（不涉及网络，用直连即可）
fn git_ok(repo: &str, args: &[&str]) -> bool {
    git::run_git(repo, args, None, LOCAL_TIMEOUT).success
}

/// 超长路径加 \\?\ 前缀，避免深层 node_modules / target 触发 Windows MAX_PATH 限制
fn verbatim_path(p: &Path) -> std::path::PathBuf {
    let s = p.as_os_str().to_string_lossy();
    if s.len() > 240 && !s.starts_with("\\\\?\\") {
        if let Some(rest) = s.strip_prefix("\\\\") {
            return std::path::PathBuf::from(format!("\\\\?\\UNC\\{rest}"));
        }
        std::path::PathBuf::from(format!("\\\\?\\{s}"))
    } else {
        p.to_path_buf()
    }
}

/// 去掉 \\?\ 前缀：它只是为了绕过 MAX_PATH 而加，不该出现在界面文案里，
/// 也不该传给资源管理器（那会拿它去判断目录是否存在）
fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix("\\\\?\\UNC\\") {
        return format!("\\\\{rest}");
    }
    s.strip_prefix("\\\\?\\").unwrap_or(&s).to_string()
}

/// 相对仓库根目录的展示路径，分隔符统一成 `/`。
/// 按"根目录 + 分隔符"整体剥离，而不是裸字符串前缀——否则 `/a/b` 会把 `/a/bc/d` 也当成子路径
fn rel_name(root_display: &str, file: &Path) -> String {
    let full = display_path(file);
    let head = format!("{}\\", root_display.trim_end_matches(['\\', '/']));
    full.strip_prefix(&head)
        .unwrap_or(&full)
        .replace('\\', "/")
}

/// 递归统计目录大小，顺带收集大文件。
///
/// collect 为 false 时只算大小、不收集：`.git` 与缓存目录的体积已经由 `dirs` 单独展示，
/// 混进"最大的文件"只会把真正值得处理的工作区文件挤下去。
/// 一次遍历同时做两件事——为找大文件再走一遍树会让大仓库的扫描时间翻倍。
fn walk_size(p: &Path, collect: bool, root_display: &str, big: &mut Vec<BigFile>) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    let mut total = 0u64;
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        if ft.is_dir() {
            // 符号链接 / junction 不跟进（read_dir 的 file_type 不跟随链接），避免循环与重复统计
            let sub = collect && !CACHE_DIRS.contains(&name.as_str());
            total += walk_size(&e.path(), sub, root_display, big);
        } else if ft.is_file() {
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            total += size;
            if collect && size >= BIG_FILE_MIN {
                big.push(BigFile {
                    name: rel_name(root_display, &e.path()),
                    // 这里 p 就是文件所在目录
                    dir: display_path(p),
                    size,
                });
            }
        }
    }
    total
}

/// 存在但没被 git 忽略的缓存目录（已经忽略的不用再提示）。
/// 每个候选都查一次"是否已被跟踪"——写 `.gitignore` 对已跟踪文件是无效的
fn ignore_suggestions(repo: &str, dirs: &[DirEntry]) -> Vec<IgnoreSuggestion> {
    dirs.iter()
        .filter(|d| d.is_cache && d.size > 0)
        .filter(|d| !git_ok(repo, &["check-ignore", "-q", "--", &d.name]))
        .map(|d| IgnoreSuggestion {
            tracked: git_ok(repo, &["ls-files", "--error-unmatch", "--", &d.name]),
            name: d.name.clone(),
        })
        .collect()
}

/// 扫描仓库一级内容的磁盘占用。大仓库可能耗时数秒，调用方放后台线程执行。
pub fn scan_repo_space(path: &str) -> Result<SpaceReport, String> {
    let root = Path::new(path);
    if !root.is_dir() {
        return Err("文件夹不存在".into());
    }
    let root_v = verbatim_path(root);
    let root_display = display_path(root);
    let mut dirs: Vec<DirEntry> = Vec::new();
    let mut total = 0u64;
    let mut big: Vec<BigFile> = Vec::new();
    // `.git` 的体积走 dirs 展示，不该出现在"最大的文件"里抢位置
    let mut git_sink: Vec<BigFile> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&root_v) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Ok(ft) = e.file_type() else { continue };
            let is_git = name == ".git";
            let size = if ft.is_dir() {
                let sink: &mut Vec<BigFile> = if is_git { &mut git_sink } else { &mut big };
                let collect = !is_git && !CACHE_DIRS.contains(&name.as_str());
                walk_size(&e.path(), collect, &root_display, sink)
            } else if ft.is_file() {
                // 仓库根目录下的大文件同样要上榜（它不属于任何子目录）
                let s = e.metadata().map(|m| m.len()).unwrap_or(0);
                if s >= BIG_FILE_MIN {
                    big.push(BigFile {
                        name: name.clone(),
                        dir: root_display.clone(),
                        size: s,
                    });
                }
                s
            } else {
                0
            };
            total += size;
            dirs.push(DirEntry {
                // 根目录下的项，rel 就是它自己的名字
                rel: name.clone(),
                is_dir: ft.is_dir(),
                path: root.join(&name).to_string_lossy().to_string(),
                is_cache: ft.is_dir() && CACHE_DIRS.contains(&name.as_str()),
                // 数一层就够，判"空目录"用；再往下数没有意义
                child_count: if ft.is_dir() { child_count(&e.path()) } else { 0 },
                name,
                size,
            });
        }
    }
    dirs.sort_by(|a, b| {
        b.size
            .cmp(&a.size)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    big.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
    big.truncate(BIG_FILE_KEEP);

    let ignorable = if repos::valid_repo(path) {
        ignore_suggestions(path, &dirs)
    } else {
        Vec::new()
    };
    Ok(SpaceReport {
        total,
        dirs,
        largest: big,
        ignorable,
    })
}

/// 树的一行：仓库里的一个文件或文件夹
///
/// `rel` 既是展示用的相对路径，**也是树节点的 id**——界面上展开一棵子树
/// 靠的就是它，不用另建一张 id → 路径的表
/// 仓库里的一个文件或文件夹。**根目录那一层与往下每一层都是这一个类型**
///
/// 早先分成 `DirSize`（一级）与 `DirEntry`（树）两个，两个类型字段重叠、
/// 改一处忘了另一处就会对不上；合成一个之后根层直接喂给树，少一次转换
///
/// `rel` 既是展示用的相对路径，**也是树节点的 id**——界面上展开一棵子树
/// 靠的就是它，不用另建一张 id → 路径的表
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DirEntry {
    /// 相对仓库根，分隔符统一 `/`；根目录下的项就是它自己的名字
    pub rel: String,
    pub name: String,
    /// 绝对路径（去掉 `\\?\` 前缀），「在文件管理器中查看」要用
    pub path: String,
    pub is_dir: bool,
    /// 文件 = 自身字节数；文件夹 = **递归总量**
    pub size: u64,
    /// 文件夹里直接有几项。0 = 空目录，界面上据此决定画不画展开箭头
    pub child_count: u32,
    /// 在白名单里且是文件夹 → 界面上标「可清理」
    pub is_cache: bool,
}

/// 只数一层：判"是不是空目录"用
fn child_count(p: &Path) -> u32 {
    std::fs::read_dir(p).map(|rd| rd.flatten().count() as u32).unwrap_or(0)
}

/// 把相对路径拆成安全的 `PathBuf`
///
/// **不许往上跳、不许绝对路径、不许盘符**。`rel` 来自界面的树节点 id，
/// 而界面上那棵树又是磁盘列出来的——今天安全不等于明天安全，这里再挡一道。
fn safe_rel(rel: &str) -> Result<std::path::PathBuf, String> {
    let mut out = std::path::PathBuf::new();
    for seg in rel.split('/') {
        match seg {
            "" | "." => continue,
            ".." => return Err("路径不能往上跳".into()),
            s => {
                if s.contains('\\') || s.contains(':') {
                    return Err("路径里有非法字符".into());
                }
                out.push(s);
            }
        }
    }
    Ok(out)
}

/// 列一层子项，**不递归下钻**（文件夹的 `size` 是递归算的，那是一次 walk）。
///
/// 根目录那一层直接由 `scan_repo_space` 的 `dirs` 给出，不走这里——
/// 那一层的体积扫描刚做完，再走一遍等于把整个仓库重数一次。
/// 本函数是给"展开某一层"用的：大仓库点开一层通常只涉及几个目录
pub fn list_dir(repo: &str, rel: &str) -> Result<Vec<DirEntry>, String> {
    let root = Path::new(repo);
    if !root.is_dir() {
        return Err("仓库文件夹不存在".into());
    }
    let sub = safe_rel(rel)?;
    let dir = if sub.as_os_str().is_empty() {
        root.to_path_buf()
    } else {
        root.join(&sub)
    };
    if !dir.is_dir() {
        return Err("这个文件夹已经不在了".into());
    }

    let mut out: Vec<DirEntry> = Vec::new();
    let Ok(rd) = std::fs::read_dir(verbatim_path(&dir)) else {
        return Err("读不了这个文件夹".into());
    };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        // 符号链接 / junction 不跟进：跟进去可能绕成环，也可能跑出仓库
        if ft.is_symlink() {
            continue;
        }
        let rel_child = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let (is_dir, size) = if ft.is_dir() {
            let mut sink = Vec::new();
            (true, walk_size(&e.path(), false, "", &mut sink))
        } else if ft.is_file() {
            (false, e.metadata().map(|m| m.len()).unwrap_or(0))
        } else {
            continue;
        };
        out.push(DirEntry {
            rel: rel_child,
            is_cache: is_dir && CACHE_DIRS.contains(&name.as_str()),
            child_count: if is_dir { child_count(&e.path()) } else { 0 },
            name,
            path: display_path(&e.path()),
            is_dir,
            size,
        });
    }
    sort_entries(&mut out);
    Ok(out)
}

/// 树里一层的顺序：**目录在前、再按名称（大小写不敏感）**，与资源管理器一致。
///
/// **必须是这一个函数，不能在两处各写一遍。** 根层是白捡 `scan_repo_space`
/// 的 `dirs`（那一层按体积降序，因为它是"谁最占地方"那个榜），子层才走
/// `list_dir`。两边各排各的，同一棵树里就会出现"根层按大小、子层按名字"。
/// 根层要名称序就在 `tree_reset` 里调它一下，不必重扫。
pub fn sort_entries(entries: &mut [DirEntry]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// 把缓存目录追加进仓库的 `.gitignore`，返回真正写入的条目。
///
/// 只接受白名单里的顶层目录名——这个函数会写用户的仓库文件，不能变成"调用方能往
/// `.gitignore` 里写任意内容"的通道。已经在忽略规则里的条目会跳过。
pub fn add_gitignore_entries(path: &str, entries: &[String]) -> Result<Vec<String>, String> {
    if !repos::valid_repo(path) {
        return Err("该文件夹不是 git 仓库".into());
    }
    let root = Path::new(path);
    let mut wanted: Vec<String> = Vec::new();
    for raw in entries {
        let name = raw.trim().to_string();
        if !CACHE_DIRS.contains(&name.as_str()) {
            return Err(format!("“{name}”不在可写入 .gitignore 的白名单中"));
        }
        if !root.join(&name).is_dir() {
            return Err(format!("目录“{name}”不存在"));
        }
        // 已经被忽略就不用再写一条
        if git_ok(path, &["check-ignore", "-q", "--", &name]) {
            continue;
        }
        if !wanted.contains(&name) {
            wanted.push(name);
        }
    }
    if wanted.is_empty() {
        return Ok(Vec::new());
    }

    let gi = root.join(".gitignore");
    let mut content = std::fs::read_to_string(&gi).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str("# 由青鸟 Git 添加的排除项\n");
    for n in &wanted {
        content.push_str(n);
        content.push('\n');
    }
    std::fs::write(&gi, content).map_err(|e| format!("写入 .gitignore 失败：{e}"))?;
    Ok(wanted)
}

/// 删除仓库下的构建缓存目录，返回删掉的字节数。
///
/// 只允许白名单内的一级子目录，避免误删任意内容。目录被编译 / 开发进程占用时
/// Windows 会拒绝删除，要把原因讲清楚而不是只丢一句 errno。
pub fn delete_cache_dir(path: &str, name: &str) -> Result<u64, String> {
    if !CACHE_DIRS.contains(&name) {
        return Err(format!("“{name}”不在可清理目录白名单中"));
    }
    let root = Path::new(path);
    if !root.is_dir() {
        return Err("仓库文件夹不存在".into());
    }
    let dir_v = verbatim_path(&root.join(name));
    let meta = std::fs::metadata(&dir_v).map_err(|_| "目录不存在".to_string())?;
    if !meta.is_dir() {
        return Err("目标不是文件夹".into());
    }
    let size = dir_size(&dir_v);
    std::fs::remove_dir_all(&dir_v).map_err(|e| explain_delete_error(&e))?;
    Ok(size)
}

/// 把删除失败翻译成一句人话。**阶段 17 的「清掉多出来的文件」也用这一份**——
///
/// 两处都在做不可逆的删除，错误文案分叉了就等于有两套"怎么跟用户解释失败"，
/// 而用户看到的那一句才是他判断"为什么没删掉"的唯一依据。
///
/// ⚠️ **不能只判 `ErrorKind::PermissionDenied`**：Windows 上"另一个程序正在用
/// 这个文件"是 `ERROR_SHARING_VIOLATION`(32)，Rust 把它归成 `Uncategorized`，
/// 不落到 `PermissionDenied` 那个臂里——于是最常见的那一种占用恰恰落进
/// 兜底分支，用户看到的是 `(os error 32)` 这种等于没说的东西。
/// 所以按 `raw_os_error()` 显式认这两个号。
pub fn explain_delete_error(e: &std::io::Error) -> String {
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const ERROR_DIR_NOT_EMPTY: i32 = 145;
    let raw = e.raw_os_error();
    if e.kind() == std::io::ErrorKind::PermissionDenied || raw == Some(ERROR_ACCESS_DENIED) {
        return "删除失败：没有权限（目录只读，或当前账户不是管理员）".into();
    }
    if raw == Some(ERROR_SHARING_VIOLATION) {
        return "删除失败：目录被占用（可能有编译 / 开发进程正在使用），请先关闭相关程序".into();
    }
    if raw == Some(ERROR_DIR_NOT_EMPTY) {
        return "删除失败：目录里还有删不掉的内容（通常也是被某个进程占用）".into();
    }
    format!("删除失败：{e}")
}

/// 一次清掉多个构建缓存目录的结果
#[derive(Serialize, Clone, Debug, Default)]
pub struct CleanOutcome {
    /// 成功删掉的名字（按传入顺序）
    pub removed: Vec<String>,
    /// 成功删掉的总量
    pub freed: u64,
    /// 没删成的：`(目录名, 原因)`
    pub failed: Vec<(String, String)>,
}

impl CleanOutcome {
    /// 一句话结论，给日志与状态条用。**用 `human_bytes` 不用 `human_size`**：
    /// 一次只清掉 `dist` 时释放量就几百 KB，`human_size` 会写成「释放 0 MB」
    pub fn summary(&self) -> String {
        if self.failed.is_empty() {
            return format!(
                "已清掉 {} 个目录，释放 {}",
                self.removed.len(),
                human_bytes(self.freed)
            );
        }
        format!(
            "已清掉 {} 个目录，释放 {}；{} 个没删成：{}",
            self.removed.len(),
            human_bytes(self.freed),
            self.failed.len(),
            self.failed
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join("、")
        )
    }
}

/// 一次删掉若干个构建缓存目录。**逐项独立**：`target` 被编译进程占用而删不掉，
/// 不该让同一批里的 `node_modules` 也留着——用户点了"全清"就是要全清
pub fn delete_cache_dirs(path: &str, names: &[String]) -> CleanOutcome {
    let mut out = CleanOutcome::default();
    for n in names {
        match delete_cache_dir(path, n) {
            Ok(bytes) => {
                out.removed.push(n.clone());
                out.freed += bytes;
            }
            Err(e) => out.failed.push((n.clone(), e)),
        }
    }
    out
}

/// 只算大小、不收集大文件（删除前统计释放量用）
///
/// ⚠️ **`pub` 是给 CLI 那边复用的**（`cli::dirs::size_of`：量一个数据目录多大）——
/// "走一棵树求和"这件事在仓库里只许有一个出处，别在 `cli` 里再写一份。
/// 走盘可能很贵（CLI 缓存实测 9.7 万个文件），**调用方自己保证在后台线程里跑**
pub fn dir_size(p: &Path) -> u64 {
    let mut sink = Vec::new();
    walk_size(p, false, "", &mut sink)
}

/// 字节数 → 人类可读（设计稿里是「860 MB」「1.8 GB」这种粒度）
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let mb = bytes as f64 / KB / KB;
    if mb < 1024.0 {
        return format!("{:.0} MB", mb);
    }
    format!("{:.1} GB", mb / 1024.0)
}

/// 字节数 → 带单位，**小文件也读得出差别**
///
/// `human_size` 一律折到 MB，4 KB 的文件会显示成「0 MB」——而目录树那一列
/// 满屏都是几 KB 的源码文件，全是 "0 MB" 等于没给信息。两档并存：
/// 大数字用 `human_size`（粒度粗一点更好读），清单用这个
pub fn human_bytes(bytes: u64) -> String {
    let (num, unit) = human_parts(bytes);
    format!("{num} {unit}")
}

/// 同一份换算，但**数值与单位分开给**。
///
/// 为什么要拆成两个函数而不是让调用方去 `split_once(' ')`：
/// 头卡那四格读数要把单位排成小号字（`118.0` 大字 + `GB` 小字），
/// 而字符串里再截一次就等于**把"哪一档用几位小数"这条规则抄第二遍**——
/// 今天两处都是 B/KB/MB/GB，将来加一档 TiB，改了这一处忘了那一处，
/// 界面上就会出现「占用 1.2 MB」与「1.2 TiB」并排。
/// ⇒ `human_bytes` 现在只是它的一层薄壳，**规则只有一份**
pub fn human_parts(bytes: u64) -> (String, &'static str) {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * KB;
    let b = bytes as f64;
    if b < KB {
        return (format!("{bytes}"), "B");
    }
    if b < MB {
        return (format!("{:.0}", b / KB), "KB");
    }
    if b < MB * 1024.0 {
        return (format!("{:.1}", b / MB), "MB");
    }
    (format!("{:.1}", b / (MB * 1024.0)), "GB")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 临时目录，Drop 时清理；同时持有 git 测试锁（部分用例要起真仓库）
    struct Temp {
        root: PathBuf,
        _git_lock: std::sync::RwLockReadGuard<'static, ()>,
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn temp(name: &str) -> Temp {
        let git_lock = crate::git::GIT_TEST_LOCK
            .read()
            .unwrap_or_else(|e| e.into_inner());
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "qbg-space-{}-{n}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Temp {
            root,
            _git_lock: git_lock,
        }
    }

    /// 造一个指定大小的稀疏文件：不真占磁盘，也就不用为了测试写 10MB 数据
    fn sparse(p: &Path, size: u64) {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).unwrap();
        }
        let f = std::fs::File::create(p).unwrap();
        f.set_len(size).unwrap();
    }

    fn scan(t: &Temp) -> SpaceReport {
        scan_repo_space(&t.root.to_string_lossy()).unwrap()
    }

    /// 起一个真的 git 仓库：下面几条要在真实 `git check-ignore` 上跑
    fn init_repo(t: &Temp) {
        let p = t.root.to_string_lossy().to_string();
        for args in [
            vec!["init", "-q", "--initial-branch=main"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "t"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            let out = git::run_git(&p, &args, None, std::time::Duration::from_secs(20));
            assert!(out.success, "git {args:?} 失败：{}", out.stderr.trim());
        }
    }

    #[test]
    fn big_files_ignore_cache_dirs_git_and_small_files() {
        let t = temp("big");
        sparse(&t.root.join("movie.mp4"), BIG_FILE_MIN + 1);
        sparse(&t.root.join("big-top.bin"), BIG_FILE_MIN + 3);
        sparse(&t.root.join("src").join("blob.bin"), BIG_FILE_MIN * 2);
        sparse(&t.root.join("small.txt"), 10);
        sparse(&t.root.join("node_modules").join("huge.js"), BIG_FILE_MIN * 5);
        sparse(&t.root.join(".git").join("objects").join("pack"), BIG_FILE_MIN * 9);

        let rep = scan(&t);
        let names: Vec<&str> = rep.largest.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["src/blob.bin", "big-top.bin", "movie.mp4"],
            "按大小降序；缓存目录与 .git 不参与，根目录下的大文件要参与"
        );
        // 打开所在文件夹要的是目录，不能把文件路径丢过去
        assert!(rep.largest[0].dir.ends_with("src"), "dir = {}", rep.largest[0].dir);
        assert!(
            Path::new(&rep.largest[0].dir).is_dir(),
            "返回的目录要能被「打开所在文件夹」用"
        );
        assert_eq!(
            rep.largest[1].dir,
            display_path(&t.root),
            "根目录下的文件，所在文件夹就是仓库根目录"
        );
    }

    #[test]
    fn big_files_are_capped() {
        let t = temp("cap");
        for i in 0..(BIG_FILE_KEEP + 5) {
            sparse(&t.root.join(format!("f{i:02}.bin")), BIG_FILE_MIN + 1);
        }
        let rep = scan(&t);
        assert_eq!(rep.largest.len(), BIG_FILE_KEEP);
        // 保留的是最大的那些：这里大小相同，按名字升序取前 10
        assert_eq!(rep.largest[0].name, "f00.bin");
        assert_eq!(rep.largest[BIG_FILE_KEEP - 1].name, "f09.bin");
    }

    #[test]
    fn empty_dir_is_not_worth_suggesting() {
        let t = temp("empty");
        std::fs::create_dir_all(t.root.join("node_modules")).unwrap();
        assert!(
            !t.root.join(".git").exists(),
            "非仓库不查 git，也就没有建议"
        );
        assert!(scan(&t).ignorable.is_empty());
    }

    #[test]
    fn untracked_cache_dir_is_suggested_then_written_and_disappears() {
        let t = temp("suggest");
        init_repo(&t);
        sparse(&t.root.join("node_modules").join("dep.js"), 1024);
        let path = t.root.to_string_lossy().to_string();

        let rep = scan(&t);
        assert_eq!(rep.ignorable.len(), 1, "只该建议 node_modules");
        assert_eq!(rep.ignorable[0].name, "node_modules");
        assert!(!rep.ignorable[0].tracked, "还没跟踪过");

        let written = add_gitignore_entries(&path, &["node_modules".into()]).unwrap();
        assert_eq!(written, vec!["node_modules"]);
        let gi = std::fs::read_to_string(t.root.join(".gitignore")).unwrap();
        assert!(gi.contains("node_modules"), "写进 .gitignore：{gi}");
        assert!(gi.starts_with("# 由青鸟 Git"), "带来源说明，用户看得懂：{gi}");

        // 已经忽略了就不再提示，重复点也不会写第二条
        assert!(scan(&t).ignorable.is_empty());
        assert!(
            add_gitignore_entries(&path, &["node_modules".into()])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn tracked_cache_dir_is_flagged_instead_of_silently_offered() {
        let t = temp("tracked");
        init_repo(&t);
        sparse(&t.root.join("dist").join("app.js"), 1024);
        let path = t.root.to_string_lossy().to_string();
        let out = git::run_git(
            &path,
            &["add", "-A"],
            None,
            std::time::Duration::from_secs(20),
        );
        assert!(out.success);

        let rep = scan(&t);
        let d = rep
            .ignorable
            .iter()
            .find(|i| i.name == "dist")
            .expect("仍要列出来");
        assert!(d.tracked, "已跟踪的目录要标出来，否则按钮会骗人");
    }

    #[test]
    fn entry_outside_whitelist_is_refused() {
        let t = temp("reject");
        init_repo(&t);
        let path = t.root.to_string_lossy().to_string();
        for bad in ["../../etc/passwd", "src", "some-random-dir"] {
            let err = add_gitignore_entries(&path, &[bad.into()]).unwrap_err();
            assert!(err.contains("白名单"), "{bad} 应被拒绝：{err}");
        }
        assert!(!t.root.join(".gitignore").exists(), "拒绝时不写任何文件");
    }

    #[test]
    fn existing_gitignore_content_is_preserved() {
        let t = temp("preserve");
        init_repo(&t);
        std::fs::write(t.root.join(".gitignore"), "*.log\n# 手工写的\n").unwrap();
        sparse(&t.root.join("build").join("out.bin"), 1024);
        let path = t.root.to_string_lossy().to_string();

        add_gitignore_entries(&path, &["build".into()]).unwrap();
        let gi = std::fs::read_to_string(t.root.join(".gitignore")).unwrap();
        assert!(gi.starts_with("*.log\n# 手工写的\n"), "原有内容一字不动：{gi}");
        assert!(gi.ends_with("build\n"));
    }

    #[test]
    fn missing_newline_at_end_is_fixed_before_appending() {
        let t = temp("nonewline");
        init_repo(&t);
        std::fs::write(t.root.join(".gitignore"), "*.log").unwrap();
        sparse(&t.root.join("build").join("out.bin"), 1024);
        let path = t.root.to_string_lossy().to_string();

        add_gitignore_entries(&path, &["build".into()]).unwrap();
        let gi = std::fs::read_to_string(t.root.join(".gitignore")).unwrap();
        assert!(
            gi.starts_with("*.log\n#"),
            "缺换行时不能和上一行粘在一起：{gi}"
        );
    }

    /// 一键清理：**逐项独立**。`target` 删不掉（被占用）不能连坐掉
    /// `node_modules`——用户点的是"全清"，不是"全成或全废"
    #[test]
    fn delete_cache_dirs_keeps_going_after_one_fails() {
        let t = temp("cleanall");
        sparse(&t.root.join("target").join("big.bin"), 5_000);
        sparse(&t.root.join("node_modules").join("dep.js"), 2_000);
        sparse(&t.root.join("dist").join("b.js"), 1_000);
        // 不存在 → 报"目录不存在"，但不影响另外两项
        let p = t.root.to_string_lossy().to_string();
        let names = vec![
            "target".to_string(),
            "nope".to_string(),
            "node_modules".to_string(),
            "dist".to_string(),
        ];
        let out = delete_cache_dirs(&p, &names);
        assert_eq!(out.removed, vec!["target", "node_modules", "dist"]);
        assert_eq!(out.freed, 8_000, "只累加成功的那几项");
        assert_eq!(out.failed.len(), 1);
        assert_eq!(out.failed[0].0, "nope");
        assert!(!t.root.join("target").exists());
        assert!(!t.root.join("node_modules").exists());
        // 白名单外的名字走 delete_cache_dir 那道闸
        let out2 = delete_cache_dirs(&p, &["src".to_string()]);
        assert!(out2.removed.is_empty() && out2.freed == 0);
        assert!(out2.failed[0].1.contains("白名单"));
    }

    #[test]
    fn clean_outcome_summary_spells_out_both_halves() {
        let ok = CleanOutcome {
            removed: vec!["target".into(), "dist".into()],
            freed: 1024 * 1024 * 900,
            failed: vec![],
        };
        // 走 `human_bytes` 之后 MB 档是 `{:.1}`，所以是 "900.0 MB" 而不是
        // `human_size` 那个 "900 MB"——清单与结论用同一个函数才不会出现
        // 同一页上两种粒度
        assert_eq!(ok.summary(), "已清掉 2 个目录，释放 900.0 MB");
        let mixed = CleanOutcome {
            removed: vec!["dist".into()],
            // 几百 KB 的量：`human_size` 在这里会写成「释放 0 MB」，
            // 而用户刚点了删除，看到 0 会以为什么都没删掉
            freed: 440 * 1024,
            failed: vec![("target".into(), "目录被占用".into())],
        };
        let s = mixed.summary();
        assert!(s.contains("释放 440 KB"), "{}", s);
        assert!(s.contains("1 个没删成") && s.contains("target"), "{}", s);
        assert!(s.starts_with("已清掉 1 个目录"), "{}", s);
    }

    /// Windows 上"被别的进程占用"是 `ERROR_SHARING_VIOLATION`(32)，Rust 归成
    /// `Uncategorized`——**不显式认这个号，最常见的那种占用就只剩一句
    /// `(os error 32)`**
    #[test]
    fn a_locked_directory_says_so_in_words() {
        let e = std::io::Error::from_raw_os_error(32);
        assert_ne!(e.kind(), std::io::ErrorKind::PermissionDenied, "前提");
        let msg = explain_delete_error(&e);
        assert!(msg.contains("被占用"), "{}", msg);
        assert!(!msg.contains("os error"), "别把 errno 甩给用户：{}", msg);

        let denied = explain_delete_error(&std::io::Error::from_raw_os_error(5));
        assert!(denied.contains("权限"), "{}", denied);

        let other = explain_delete_error(&std::io::Error::from_raw_os_error(9999));
        assert!(other.contains("删除失败"), "{}", other);
    }

    /// 根层是白捡 `scan_repo_space` 的 `dirs`（按体积降序），子层走 `list_dir`
    /// （名称序）。两边各排各的，同一棵树里就会"根层按大小、子层按名字"，
    /// 所以 `tree_reset` 必须再排一次——而**排序规则只能有这一个出处**
    #[test]
    fn sort_entries_is_the_one_place_the_level_order_is_decided() {
        let mk = |name: &str, is_dir: bool, size: u64| DirEntry {
            rel: name.to_string(),
            name: name.to_string(),
            path: String::new(),
            is_dir,
            size,
            child_count: 0,
            is_cache: false,
        };
        let mut v = vec![
            mk("small_dir", true, 1),
            mk("Zebra.rs", false, 999),
            mk("Big", true, 500),
            mk("apple.rs", false, 1),
        ];
        sort_entries(&mut v);
        let names: Vec<&str> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Big", "small_dir", "apple.rs", "Zebra.rs"],
            "目录在前；同类按名称序。**大小写不敏感**——apple 排在 Zebra 前，\
             按 ASCII 码排的话 'Z'(0x5A) 在 'a'(0x61) 前面就该反过来了。\
             体积再大也不参与排序，「谁最占地方」由构成条回答"
        );
    }

    /// 树的一层：目录在前、名称序；体积递归；缓存目录带标记；空目录 child_count = 0
    #[test]
    fn list_dir_sorts_dirs_first_and_marks_cache_dirs() {
        let t = temp("tree");
        sparse(&t.root.join("zeta.rs"), 10);
        sparse(&t.root.join("alpha.rs"), 20);
        sparse(&t.root.join("beta").join("inner.bin"), BIG_FILE_MIN);
        sparse(&t.root.join("node_modules").join("dep.js"), BIG_FILE_MIN * 2);
        std::fs::create_dir_all(t.root.join("empty")).unwrap();
        let p = t.root.to_string_lossy().to_string();

        let got = list_dir(&p, "").unwrap();
        let names: Vec<&str> = got.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["beta", "empty", "node_modules", "alpha.rs", "zeta.rs"],
            "目录在前，同类按名称序（大小写不敏感）"
        );
        let by = |n: &str| got.iter().find(|e| e.name == n).unwrap();
        assert!(by("beta").is_dir);
        assert_eq!(by("beta").size, BIG_FILE_MIN, "文件夹的体积是递归总量");
        assert_eq!(by("beta").child_count, 1);
        assert_eq!(by("empty").child_count, 0, "空目录要能判出来，界面上据此不画箭头");
        assert!(by("node_modules").is_cache, "白名单里的要标可清理");
        assert!(!by("beta").is_cache);
        assert!(!by("alpha.rs").is_dir && !by("alpha.rs").is_cache, "文件不是目录，也不该被标可清理");
        assert_eq!(by("alpha.rs").size, 20);
        assert_eq!(by("beta").rel, "beta", "rel 是相对根的路径，也是树节点 id");
        assert_eq!(by("beta").path, display_path(&t.root.join("beta")));

        // 再列一层：rel 决定去哪
        let deeper = list_dir(&p, "beta").unwrap();
        assert_eq!(deeper.len(), 1);
        assert_eq!(deeper[0].rel, "beta/inner.bin", "子项的 rel 带上父级");
        assert!(Path::new(&deeper[0].path).is_file());
    }

    /// 白名单之外的同名目录、以及 `.git` 本身，都**不是**可清理项
    #[test]
    fn only_whitelisted_dirs_are_marked_cache() {
        let t = temp("cache-mark");
        sparse(&t.root.join(".git").join("HEAD"), 5);
        sparse(&t.root.join("build").join("out.bin"), 5);
        sparse(&t.root.join("src").join("build.rs"), 5);
        let p = t.root.to_string_lossy().to_string();
        let got = list_dir(&p, "").unwrap();
        let cache: Vec<&str> = got.iter().filter(|e| e.is_cache).map(|e| e.name.as_str()).collect();
        assert_eq!(cache, vec!["build"], "只有白名单里的目录名算数");
        // 同名的**文件**也不该被当成缓存目录
        sparse(&t.root.join("target"), 5);
        let got = list_dir(&p, "").unwrap();
        assert!(
            !got.iter().any(|e| e.name == "target" && e.is_cache),
            "白名单只管目录：同名文件不算"
        );
    }

    /// `rel` 是界面上来的，**不许它往上跳或指到绝对路径**
    #[test]
    fn list_dir_refuses_to_escape_the_repo() {
        let t = temp("escape");
        std::fs::create_dir_all(t.root.join("ok")).unwrap();
        let p = t.root.to_string_lossy().to_string();
        for bad in ["..", "../..", "ok/../..", "C:/Windows", r"..\..", r"ok\..\.."] {
            let e = list_dir(&p, bad).unwrap_err();
            assert!(e.contains("不能往上跳") || e.contains("非法字符"), "{bad} 该被拒：{e}");
        }
        // 正常的一层还是能列
        assert!(list_dir(&p, "ok").is_ok());
        // 不存在的目录要说清楚，而不是空列表
        assert!(list_dir(&p, "nope").is_err());
    }

    /// 根目录那一层由 `scan_repo_space` 直接给出：**child_count 也要填**
    #[test]
    fn scan_report_carries_child_count_for_the_tree_root() {
        let t = temp("rootcount");
        sparse(&t.root.join("src").join("main.rs"), 5);
        std::fs::create_dir_all(t.root.join("empty")).unwrap();
        let rep = scan(&t);
        let src = rep.dirs.iter().find(|d| d.name == "src").unwrap();
        assert_eq!(src.child_count, 1);
        assert_eq!(rep.dirs.iter().find(|d| d.name == "empty").unwrap().child_count, 0);
        // 根目录下的文件 child_count 恒为 0
        sparse(&t.root.join("README.md"), 5);
        let rep = scan(&t);
        assert_eq!(rep.dirs.iter().find(|d| d.name == "README.md").unwrap().child_count, 0);
    }

    /// 文件级体积：三档边界都要卡对，**满屏 0 MB 等于没给信息**
    #[test]
    fn human_bytes_keeps_small_files_readable() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1023), "1023 B");
        assert_eq!(human_bytes(1024), "1 KB");
        assert_eq!(human_bytes(1536), "2 KB", "1536/1024 = 1.5，四舍五入到 2");
        assert_eq!(human_bytes(4 * 1024), "4 KB", "4 KB 绝不能显示成 0 MB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(human_bytes(1536 * 1024 * 1024), "1.5 GB");
        // 同一批数走粗粒度那档：小文件全是 0 MB —— 这就是为什么要有两档
        assert_eq!(human_size(4 * 1024), "0 MB");
    }

    /// 拆开的数值与单位**必须拼回原来那句话**。
    ///
    /// 这条断言钉的是"两档只有一个出处"：头卡那四格读数吃 `human_parts`，
    /// 其余地方吃 `human_bytes`，两者一旦各写一套换算规则，
    /// 同一份磁盘数据会在同一屏上说出两个数（本项目为这类缺陷交过四次学费）
    #[test]
    fn human_parts_reassembles_into_human_bytes_at_every_tier() {
        for n in [0u64, 1, 1023, 1024, 1536, 4 * 1024, 1024 * 1024, 1536 * 1024 * 1024] {
            let (num, unit) = human_parts(n);
            assert_eq!(format!("{num} {unit}"), human_bytes(n), "{n} 两档对不上");
        }
        assert_eq!(human_parts(118 * 1024 * 1024 * 1024), ("118.0".into(), "GB"));
        assert_eq!(human_parts(4 * 1024), ("4".into(), "KB"));
    }

    /// 清理缓存：只认白名单，且真的把目录删掉、报出释放量
    #[test]
    fn delete_cache_dir_removes_whitelisted_dir_and_reports_freed_size() {
        let t = temp("clean");
        sparse(&t.root.join("target").join("debug").join("app.exe"), 4096);
        sparse(&t.root.join("src").join("keep.rs"), 128);
        let path = t.root.to_string_lossy().to_string();

        let freed = delete_cache_dir(&path, "target").unwrap();
        assert!(freed >= 4096, "释放量要覆盖文件本身：{freed}");
        assert!(!t.root.join("target").exists(), "target 应被删掉");
        assert!(t.root.join("src").join("keep.rs").exists(), "源码不能动");
    }

    #[test]
    fn delete_cache_dir_refuses_anything_outside_whitelist() {
        let t = temp("cleanguard");
        std::fs::create_dir_all(t.root.join("src")).unwrap();
        let path = t.root.to_string_lossy().to_string();
        for bad in ["src", "..", ".git"] {
            let err = delete_cache_dir(&path, bad).unwrap_err();
            assert!(err.contains("白名单"), "{bad} 应被拒绝：{err}");
        }
        assert!(t.root.join("src").exists(), "拒绝时不删任何东西");
    }

    #[test]
    fn human_size_switches_to_gb_above_one_gibibyte() {
        assert_eq!(human_size(860 * 1024 * 1024), "860 MB");
        assert_eq!(human_size(90 * 1024 * 1024), "90 MB");
        assert_eq!(human_size(1536 * 1024 * 1024), "1.5 GB");
    }
}
