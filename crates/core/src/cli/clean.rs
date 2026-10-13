//! 阶段三：清理目录——**只删内容，不删目录本身**（设计 §11）。
//!
//! 用在两处：卸载对话框里勾上的 `Cache` 目录、右栏底部共享缓存的「清理」。
//! 两者共用这一个出口——再写一份"删目录"的就是同一件事两个出处。

use std::path::Path;

/// 删掉 `dir` 里的全部内容，**保留 `dir` 自己**。返回释放的字节数。
///
/// 为什么保留目录：那是工具自己的家（`~/.deno`、`%LOCALAPPDATA%\npm-cache`）。
/// 缓存的意义是"里面的东西可再生"，目录本身不是缓存——删掉它，有的工具
/// 启动时报一句"目录不存在"再重建，有的直接失败。删前那一行路径由调用方
/// 记进日志（设计 §11：删前记路径入日志）。
///
/// **逐项独立**：一个文件被进程占用删不掉，不影响同目录里别的项
/// （与 `space::delete_cache_dirs` 同一条——用户点清理就是要清掉能清的那些）。
/// 整目录不在 = 没什么可清，**不是错**（缓存可能已经被别的手段清过）。
pub fn clean_dir_contents(dir: &Path) -> Result<u64, String> {
    if !dir.is_dir() {
        return Ok(0);
    }
    let rd = std::fs::read_dir(dir).map_err(|e| format!("读不了 {}：{e}", dir.display()))?;
    let mut freed = 0u64;
    let mut failed: Vec<String> = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        let is_dir = p.is_dir();
        // 先量后删：量的是**这一项**的体积，删成功的才记上
        // （整目录先量再删的话，删到一半失败会把没删的也算成"已释放"）。
        // ⚠️ 文件走 `metadata().len()`、目录才走 `dirs::size_of`——那颗走盘器
        // 对**文件**返回 0（`read_dir` 一个文件是错的），照它量会少报一整格
        let bytes = if is_dir {
            super::dirs::size_of(&p)
        } else {
            std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0)
        };
        let r = if is_dir {
            std::fs::remove_dir_all(&p)
        } else {
            std::fs::remove_file(&p)
        };
        match r {
            Ok(()) => freed += bytes,
            Err(e) => failed.push(format!("{}：{e}", p.display())),
        }
    }
    if failed.is_empty() {
        Ok(freed)
    } else {
        Err(format!(
            "{} 项没删成（{}）",
            failed.len(),
            failed.join("；")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("qb-cli-clean-{name}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    /// 只删内容：目录自己还在，里面清空，字节数照实报。
    #[test]
    fn only_the_contents_go_away() {
        let dir = scratch("contents");
        std::fs::write(dir.join("a.bin"), vec![0u8; 100]).unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("b.bin"), vec![0u8; 50]).unwrap();

        let freed = clean_dir_contents(&dir).expect("该清得动");
        assert_eq!(freed, 150, "释放的字节要照实（两项之和）");
        assert!(dir.is_dir(), "目录本身必须留着");
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "里面要空");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 目录不在 = 没什么可清，**不是错**（返回 0）。
    #[test]
    fn a_missing_directory_is_empty_not_an_error() {
        let gone = std::env::temp_dir().join("qb-cli-clean-never-existed-42");
        let _ = std::fs::remove_dir_all(&gone);
        assert_eq!(clean_dir_contents(&gone).unwrap(), 0);
    }

    /// 空目录也是 0、也不是错。
    #[test]
    fn an_empty_directory_is_zero() {
        let dir = scratch("empty");
        assert_eq!(clean_dir_contents(&dir).unwrap(), 0);
        assert!(dir.is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 目标是个**文件**（不是目录）⇒ 0，不是错。
    /// 这条守的是 `is_dir` 那一档：规则库/provider 给出一条文件的路径时
    /// （配置项有时是个文件不是目录），清理要当"没什么可清"而不是炸掉。
    #[test]
    fn a_file_target_is_zero_not_an_error() {
        let f = scratch("afile").join("file.txt");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(clean_dir_contents(&f).unwrap(), 0);
        assert!(f.is_file(), "文件自己不许被删");
        let _ = std::fs::remove_file(&f);
    }

    /// 删不动的项要报出来，且**不影响已删的那些**（逐项独立）。
    ///
    /// 夹具是**自己造出来的**：拿 `CreateFileW` 以 `FILE_SHARE_READ`（不放行 DELETE）
    /// 持有一个文件 ⇒ 这个文件真的删不掉。不拿"系统里某个被占用的文件"当夹具——
    /// 那是把测试前提外包给外部世界（AGENTS：外部世界会怎么反应，不由测试决定）。
    #[test]
    fn failures_are_reported_and_do_not_block_the_rest() {
        let dir = scratch("locked");
        let locked = dir.join("locked.bin");
        std::fs::write(&locked, vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("free.bin"), vec![0u8; 50]).unwrap();
        let hold = hold_open(&locked);
        assert!(!hold.is_null(), "夹具要真的把文件持住");

        let err = clean_dir_contents(&dir).expect_err("持住的那个删不掉");
        close_handle(hold);
        // 报的错里要有：条数 + 路径（用户得知道是哪一项没删成）
        assert!(err.contains("1 项没删成"), "条数要说出来：{err}");
        assert!(err.contains("locked.bin"), "路径要说出来：{err}");
        // 逐项独立：旁边那个自由的被删掉了
        assert!(!dir.join("free.bin").exists(), "别的项不该被连坐");
        assert!(dir.join("locked.bin").exists(), "持住的那个还在");
        assert!(dir.is_dir(), "目录本身还在");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 拿住一个文件、**不放行删除**（`FILE_SHARE_READ`：没有 `FILE_SHARE_DELETE`）。
    #[cfg(windows)]
    fn hold_open(path: &Path) -> windows_sys::Win32::Foundation::HANDLE {
        use windows_sys::Win32::Foundation::GENERIC_READ;
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, OPEN_EXISTING,
        };
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        }
    }

    #[cfg(windows)]
    fn close_handle(h: windows_sys::Win32::Foundation::HANDLE) {
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe { CloseHandle(h) };
    }
}
