//! 仓库磁盘占用扫描、大文件排查、可清理缓存目录与 .gitignore 建议。
//!
//! 纯同步实现：调用方（GUI）负责把它丢到后台线程，core 不管异步。

use std::path::Path;

use serde::Serialize;

use crate::git;
use crate::repos::{self, LOCAL_TIMEOUT};

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

#[derive(Serialize, Clone)]
pub struct DirSize {
    pub name: String,
    pub path: String,
    pub size: u64,
    /// 属于白名单内的可清理缓存目录
    pub is_cache: bool,
}

#[derive(Serialize, Clone)]
pub struct BigFile {
    /// 相对仓库根目录的路径（分隔符统一成 `/`），展示用
    pub name: String,
    /// 所在文件夹的绝对路径，供"打开所在文件夹"用（只接受目录）
    pub dir: String,
    pub size: u64,
}

#[derive(Serialize, Clone)]
pub struct IgnoreSuggestion {
    /// 顶层目录名
    pub name: String,
    /// 目录里已经有被 git 跟踪的文件：写进 `.gitignore` 不会取消跟踪，
    /// 得靠 `git rm -r --cached`。标出来是为了不让按钮给人"已经修好了"的错觉
    pub tracked: bool,
}

#[derive(Serialize, Clone)]
pub struct SpaceReport {
    /// 仓库文件夹总占用（字节）
    pub total: u64,
    /// 一级子目录 / 文件按大小排序
    pub dirs: Vec<DirSize>,
    /// 最大的若干个工作区文件（不含 `.git` 与缓存目录）
    pub largest: Vec<BigFile>,
    /// 存在但没被 git 忽略的缓存目录
    pub ignorable: Vec<IgnoreSuggestion>,
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
fn ignore_suggestions(repo: &str, dirs: &[DirSize]) -> Vec<IgnoreSuggestion> {
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
    let mut dirs: Vec<DirSize> = Vec::new();
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
            dirs.push(DirSize {
                path: root.join(&name).to_string_lossy().to_string(),
                is_cache: ft.is_dir() && CACHE_DIRS.contains(&name.as_str()),
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
    std::fs::remove_dir_all(&dir_v).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            "删除失败：目录被占用（可能有编译 / 开发进程正在使用），请先关闭相关程序".to_string()
        } else {
            format!("删除失败：{e}")
        }
    })?;
    Ok(size)
}

/// 只算大小、不收集大文件（删除前统计释放量用）
fn dir_size(p: &Path) -> u64 {
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
