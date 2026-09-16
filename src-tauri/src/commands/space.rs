//! 仓库磁盘占用扫描与可清理缓存目录。

use serde::Serialize;

/// 可清理的构建缓存目录名白名单（均为可再生的构建产物 / 依赖缓存）
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

#[derive(Serialize, Clone)]
pub struct DirSize {
    pub name: String,
    pub path: String,
    pub size: u64,
    /// 属于白名单内的可清理缓存目录
    pub is_cache: bool,
}

#[derive(Serialize, Clone)]
pub struct SpaceReport {
    /// 仓库文件夹总占用（字节）
    pub total: u64,
    /// 一级子目录 / 文件按大小排序
    pub dirs: Vec<DirSize>,
}

/// 超长路径加 \\?\ 前缀，避免深层 node_modules / target 触发 Windows MAX_PATH 限制
fn verbatim_path(p: &std::path::Path) -> std::path::PathBuf {
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

/// 递归统计目录大小；符号链接 / junction 不跟进，避免循环与重复统计
fn dir_size(p: &std::path::Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    let mut total = 0u64;
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            total += dir_size(&e.path());
        } else if ft.is_file() {
            total += e.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

/// 扫描仓库一级内容的磁盘占用（大仓库可能耗时数秒，放后台线程执行）
#[tauri::command]
pub async fn scan_repo_space(path: String) -> Result<SpaceReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = std::path::Path::new(&path);
        if !root.is_dir() {
            return Err("文件夹不存在".into());
        }
        let root_v = verbatim_path(root);
        let mut dirs: Vec<DirSize> = Vec::new();
        let mut total = 0u64;
        if let Ok(rd) = std::fs::read_dir(&root_v) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let Ok(ft) = e.file_type() else { continue };
                let size = if ft.is_dir() {
                    dir_size(&e.path())
                } else if ft.is_file() {
                    e.metadata().map(|m| m.len()).unwrap_or(0)
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
        Ok(SpaceReport { total, dirs })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 删除仓库下的构建缓存目录。只允许白名单内的一级子目录，避免误删任意内容。
#[tauri::command]
pub async fn delete_cache_dir(path: String, name: String) -> Result<u64, String> {
    if !CACHE_DIRS.contains(&name.as_str()) {
        return Err(format!("“{name}”不在可清理目录白名单中"));
    }
    let root = std::path::Path::new(&path);
    if !root.is_dir() {
        return Err("仓库文件夹不存在".into());
    }
    let dir_v = verbatim_path(&root.join(&name));
    let meta = std::fs::metadata(&dir_v).map_err(|_| "目录不存在".to_string())?;
    if !meta.is_dir() {
        return Err("目标不是文件夹".into());
    }
    let size = dir_size(&dir_v);
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::remove_dir_all(&dir_v).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                "删除失败：目录被占用（可能有编译 / 开发进程正在使用），请先关闭相关程序"
                    .to_string()
            } else {
                format!("删除失败：{e}")
            }
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(size)
}
