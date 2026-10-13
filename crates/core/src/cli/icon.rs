//! CLI 工具图标：从可执行文件里提图标资源，落成磁盘上一张 PNG（阶段二第一件）。
//!
//! ## 它补的是三级降里的哪一级
//!
//! 设计（`docs/NEW/…-最终版.md` §10.1）三级降：① Provider 清单自报 → ② 主可执行
//! 文件图标资源提取 → ③「终字符块」（工具名前两个字符画块）。
//! **本模块做的是第 ② 级 + 它的磁盘缓存**；第 ③ 级在界面侧（`gui/src/cli_tab.rs`
//! 的 `monogram_block`），第 ① 级**这一版没写**——`PackageProvider` 现在压根没有
//! 「自报图标」这个出口，三个 provider 的输出里也没有图标字段 ⇒ 写它就是一个
//! 永不触发的分支（AGENTS：那比没有更糟，它会让人以为这条路验过了）。
//!
//! ## ⛔ 为什么是 `ExtractIconExW` 而不是设计稿写的 `SHGetFileInfoW`
//!
//! 设计稿 §10.1 写的是 `SHGetFileInfoW`。**实测（2026-10-13，本机 9 个 npm 全局包）
//! 它在这一档是错的**：npm 全局包的命令全是 `.cmd` / `.ps1` / 无扩展名的**垫片**，
//! 而壳对垫片返回的是**扩展名通用图标**——9 个 `.cmd` 取出来的像素逐字节相同
//! （同一份 16×16 的「批处理文件」图标）。照它写，界面上会是**一列九颗一模一样的
//! 脚本图标**，比终字符块还差。
//!
//! `ExtractIconExW` 的分辨率正好就是我们要的那一条：**返回这个文件自己带几个图标**，
//! 0 = 它没有图标资源。实测：`.cmd`/`.ps1`/垫片全部 **0**；`bun.exe`/`deno.exe`
//! （真 PE，自带资源）是 **2**；`.ico` 文件是 **1**。
//!
//! ## 从哪个文件提
//!
//! [`exe_candidates`]：拿 `install_path` 往下探（深度 2、跳过 `.git`/`node_modules`/
//! `target`、走满 `MAX_VISITED` 个目录项就收工），**文件名与命令名同名的排前面**
//! （`deno.exe` 赢过 `dist/vendor/fastlist.exe` 这类随包附带的小工具）。
//! 然后逐个用 `ExtractIconExW` 筛，**第一个有图标的就是它**。
//!
//! ## 磁盘缓存
//!
//! 文件名 `<包哈希>-<源哈希>.png`，两个哈希都是 64 位 FNV-1a（**自己写的、不是
//! `DefaultHasher`**——后者每个进程的种子不同，用它文件名每次启动都变，缓存等于没有）。
//! **源哈希里含 exe 的修改时间与大小** ⇒ exe 换版自动就是另一个名字，不会拿旧图标糊上去
//! （设计 §10.1 那条"键含 exe 修改时间/哈希"）。
//!
//! 落盘走**临时文件 + 改名**（与 `store::save_to` 同一条纪律）：半张 PNG 被
//! gpui 解出来是一块空白，而它的名字**已经在了** ⇒ 那台机器上会一直显示空白。
//! 写成功后把同一个包的旧名字删掉（`prune_siblings`，只删我们自己的命名形状）。
//!
//! ## 为什么目录是入参
//!
//! [`ensure`] / [`cached_index`] 的目录是**参数**，不由里面读 `%APPDATA%`——单测才能
//! 整条链路都跑在 `%TEMP%` 沙箱里（AGENTS：单测不许改进程级全局，
//! 而"跑一次 `cargo test` 就往用户目录塞东西"本项目已经栽过一次）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::cli::model::CliPackage;

/// 从安装目录往下探几层找 exe。实测够用：`deno.exe` 在第 0 层、
/// `bin/bun.exe` 第 1 层、`dist/vendor/fastlist.exe` 第 2 层
const MAX_DEPTH: usize = 2;

/// 一次找候选最多看多少个目录项。防某个包目录特别大（几千个文件）时
/// 把后台那条线程占住——它是跟扫描同批跑的，用户正等着列表出来
const MAX_VISITED: usize = 4000;

/// 往下探时**不进**的目录：都不是"这个包自己的程序"待的地方，
/// 而它们恰恰是最容易很大的那几个
const SKIP_DIRS: [&str; 3] = [".git", "node_modules", "target"];

/// 磁盘上已经缓存好的图标：`包 id → PNG 路径`（每个包只留最近的那一张）。
///
/// **首屏专用**：一次 `read_dir` 扫出全部，不碰 Windows API、也不去 stat 那颗 exe。
/// 界面拿它先把图标画上，后台那条提取再逐颗核对（exe 换过版就换一张新的）。
///
/// 传包清单进来而不是只传目录：**命名那套规则（`<包哈希>-…`）只留在这一个文件里**，
/// 调用方不该为了查一次缓存去拼一次包哈希。
pub fn cached_icons(pkgs: &[CliPackage], dir: &Path) -> HashMap<String, PathBuf> {
    let idx = cached_index(dir);
    pkgs.iter()
        .filter_map(|p| idx.get(&pkg_key(&p.id)).map(|path| (p.id.clone(), path.clone())))
        .collect()
}

/// 上面那颗的底层：按**文件名里的包哈希**索引（同一个包有多张时取最新的那张）。
fn cached_index(dir: &Path) -> HashMap<String, PathBuf> {
    let mut out: HashMap<String, PathBuf> = HashMap::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out; // 目录还不存在 = 还没提过任何一颗，正常
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(prefix) = name.strip_suffix(".png").and_then(|n| n.split('-').next()) else {
            continue;
        };
        // 只认我们自己的命名形状（16 位十六进制 + 横杠 + …），别人的文件一概不碰
        if prefix.len() != 16 || !prefix.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // 同一个包有多张（上一次的没来得及删）时取**最新的那张**
        let newer = match out.get(prefix) {
            None => true,
            Some(old) => newest(&path, old),
        };
        if newer {
            out.insert(prefix.to_string(), path);
        }
    }
    out
}

/// `a` 是不是比 `b` 新（读不出来的一方算旧）。
fn newest(a: &Path, b: &Path) -> bool {
    let ma = std::fs::metadata(a).and_then(|m| m.modified()).ok();
    let mb = std::fs::metadata(b).and_then(|m| m.modified()).ok();
    match (ma, mb) {
        (Some(x), Some(y)) => x > y,
        (Some(_), None) => true,
        _ => false,
    }
}

/// 确保这一颗包的图标已经落盘，返回那张 PNG 的路径。
///
/// `None` = **它没有自带图标**（画终字符块）——"没提出来"与"它本来就没有"是
/// 同一件事在界面上的落法，所以不拆两个返回值。
///
/// 阻塞（要 stat + 可能过 GDI + 写盘）——**调用方放后台线程**。
pub fn ensure(pkg: &CliPackage, dir: &Path) -> Option<PathBuf> {
    let prefix = pkg_key(&pkg.id);
    for exe in exe_candidates(pkg) {
        // stat 拿不到就不试这一个（文件正被换掉、权限不给……都不是我们该报的错）
        let Some(src) = source_key(&exe) else {
            continue;
        };
        let path = dir.join(format!("{prefix}-{src}.png"));
        if path.is_file() {
            // 命中：exe 的 mtime 与大小都在名字里，名字一样就是同一版
            return Some(path);
        }
        // 没有图标资源（`.cmd` 垫片那一类）→ 试下一个候选
        let Some((w, h, rgba)) = extract_rgba(&exe) else {
            continue;
        };
        if !write_png(&path, w, h, &rgba) {
            continue;
        }
        prune_siblings(dir, &prefix, &path);
        return Some(path);
    }
    None
}

/// 这个包的图标要从哪几个文件里找（按"最可能是它自己"排序）。
///
/// 分两段：**与命令名同名的 exe** 在前，其余 `*.exe` 按路径排在后。
/// 收成一颗独立函数是为了能单测顺序——**没量过顺序的搜索 = 谁先撞上算谁**。
///
/// `pub(crate)` 是因为 [`crate::cli::desktop`] 要用同一趟走盘（"这颗包的 exe 在哪"
/// 只允许有一个出处）；它**不**走共用安装目录——那一档由调用方自己按命令名取。
pub(crate) fn exe_candidates(pkg: &CliPackage) -> Vec<PathBuf> {
    let Some(dir) = pkg.install_path.as_deref().map(Path::new) else {
        return Vec::new();
    };
    if !dir.is_dir() {
        return Vec::new();
    }

    let mut matched = Vec::new();
    let mut others = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    let mut visited = 0usize;

    'walk: while let Some((at, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&at) else {
            continue;
        };
        for entry in rd.flatten() {
            visited += 1;
            if visited > MAX_VISITED {
                break 'walk;
            }
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if ft.is_dir() {
                if depth < MAX_DEPTH && !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push((entry.path(), depth + 1));
                }
            } else if name.ends_with(".exe") {
                let path = entry.path();
                if matches_command(&path, &pkg.executables) {
                    matched.push(path);
                } else {
                    others.push(path);
                }
            }
        }
    }

    matched.sort();
    others.sort();
    matched.extend(others);
    matched
}

/// 这颗 exe 的文件名词干是不是这颗包自报的某条命令（小写比，只去一层 `.exe`）。
///
/// **两个消费方**：挑图标候选（同名的排前）与 [`crate::cli::desktop`] 判
/// "这支 GUI 主程序就是那条命令"。判据只写一份——各写一遍就会各漂一遍，
/// 而两处给出的结论不一样时，界面会说"这颗包是纯命令行"、图标却从它那颗 exe 上取。
pub(crate) fn matches_command(exe: &Path, executables: &[String]) -> bool {
    let Some(stem) = exe
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .and_then(|n| n.strip_suffix(".exe").map(str::to_string))
    else {
        return false;
    };
    executables
        .iter()
        .any(|c| c.to_ascii_lowercase() == stem)
}

/// 一个 exe 的"这一版"指纹：路径 + 修改时间 + 大小。
fn source_key(exe: &Path) -> Option<String> {
    let meta = std::fs::metadata(exe).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut buf = exe.to_string_lossy().to_ascii_lowercase().into_bytes();
    buf.extend_from_slice(&mtime.to_le_bytes());
    buf.extend_from_slice(&meta.len().to_le_bytes());
    Some(format!("{:016x}", fnv1a(&buf)))
}

/// 包 id 的稳定哈希（**跨进程稳定**是这条的全部要求，见模块头）。
fn pkg_key(id: &str) -> String {
    format!("{:016x}", fnv1a(id.as_bytes()))
}

/// FNV-1a 64。自己写而不是 `DefaultHasher`：后者的种子每个进程都不同，
/// 用它算文件名等于每次启动都换一批名字、缓存永远不命中
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 写一张 RGBA 的 PNG。走**临时文件 + 改名**：半张图的名字已经在的话，
/// 界面会一直解出一块空白（名字命中就不再重提）。
fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> bool {
    if rgba.len() != (w as usize) * (h as usize) * 4 {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    let tmp = path.with_extension("tmp");
    let ok = (|| -> std::io::Result<()> {
        let file = std::fs::File::create(&tmp)?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc
            .write_header()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(())
    })();
    if ok.is_ok() && std::fs::rename(&tmp, path).is_ok() {
        return true;
    }
    // 没成的那一路把临时文件收干净（改名没跑成、编码中途失败都在这里）
    let _ = std::fs::remove_file(&tmp);
    false
}

/// 删掉同一个包的旧图标（只删我们自己那套命名：`<包哈希>-*.png`，且不删刚写的这张）。
///
/// 不删的话每换一版 exe 就多攒一张，几年下来这个目录只长不缩。
/// **它不是"清理用户文件"**：这个目录是我们自己建的、文件名是我们自己定的、
/// 内容随时能重算出来。
fn prune_siblings(dir: &Path, prefix: &str, keep: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path == keep {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&format!("{prefix}-")) && name.ends_with(".png") {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// `HICON` → `(宽, 高, RGBA)`。`None` = **这个文件没有自带图标**
/// （或者它那一路 GDI 没走通——两者对调用方是同一件事：换下一个候选）。
#[cfg(windows)]
fn extract_rgba(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    use std::os::windows::ffi::OsStrExt as _;
    use std::ptr;
    use windows_sys::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
    };
    use windows_sys::Win32::UI::Shell::ExtractIconExW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, GetIconInfo, ICONINFO, HICON,
    };

    /// GDI 对象不是 GC 管的：每提一颗图标漏一个 HICON/HBITMAP/HDC，
    /// 扫几百个包就是几百个泄漏。三条 guard 保证每条出口都收干净
    struct IconGuard(HICON);
    impl Drop for IconGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { DestroyIcon(self.0) };
            }
        }
    }
    struct BitmapGuard(HBITMAP);
    impl Drop for BitmapGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { DeleteObject(self.0 as HGDIOBJ) };
            }
        }
    }
    struct DcGuard(HDC);
    impl Drop for DcGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { ReleaseDC(ptr::null_mut(), self.0) };
            }
        }
    }

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut large: HICON = ptr::null_mut();
    let mut small: HICON = ptr::null_mut();
    // ⚠️ 只认这个**数量**：0 = 这个文件没有图标资源（垫片就是这一档）。
    // 别改成"看 hIcon 是不是空"——那正是 SHGetFileInfoW 那条错路
    let n = unsafe { ExtractIconExW(wide.as_ptr(), 0, &mut large, &mut small, 1) };
    if n == 0 || (large.is_null() && small.is_null()) {
        // 数量为 0 时句柄本该是空的，但真给了也得收掉
        let _ = IconGuard(if large.is_null() { small } else { large });
        return None;
    }
    let icon = if !large.is_null() { large } else { small };
    let _icon_guard = IconGuard(icon);
    if !large.is_null() && !ptr::eq(large, icon) {
        unsafe { DestroyIcon(large) };
    }
    if !small.is_null() && !ptr::eq(small, icon) {
        unsafe { DestroyIcon(small) };
    }

    let mut info: ICONINFO = unsafe { std::mem::zeroed() };
    if unsafe { GetIconInfo(icon, &mut info) } == 0 {
        return None;
    }
    let color = BitmapGuard(info.hbmColor);
    let mask = BitmapGuard(info.hbmMask);
    if color.0.is_null() {
        // 单色图标（只有掩码那一张）。这种资源这年头只出现在很老的 DLL 里，
        // 不值得为它长一条分支——没有图标就画终字符块
        return None;
    }

    // 尺寸**量出来**，不要假设 32×32：`ExtractIconExW` 给的是"系统大图标"那一档，
    // 高 DPI 下是 48×48
    let mut bm: BITMAP = unsafe { std::mem::zeroed() };
    let got = unsafe {
        GetObjectW(
            color.0 as HGDIOBJ,
            std::mem::size_of::<BITMAP>() as i32,
            &mut bm as *mut BITMAP as *mut _,
        )
    };
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth as u32, bm.bmHeight as u32);

    let hdc = unsafe { GetDC(ptr::null_mut()) };
    if hdc.is_null() {
        return None;
    }
    let _dc_guard = DcGuard(hdc);

    let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w as i32;
    // 负数 = 自上而下给像素，省掉一次翻转
    bmi.bmiHeader.biHeight = -(h as i32);
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let mut px = vec![0u8; (w as usize) * (h as usize) * 4];
    let lines = unsafe {
        GetDIBits(
            hdc,
            color.0,
            0,
            h,
            px.as_mut_ptr() as *mut _,
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };
    if lines == 0 {
        return None;
    }
    for p in px.chunks_exact_mut(4) {
        p.swap(0, 2); // BGRA → RGBA
    }

    // 透明那一档：32 位图标自带 alpha；**低于 32 位的图标 alpha 全是 0**，
    // 必须从 AND mask 补（mask 为白 = 透明）。不补的话这类图标会画成一块黑
    let alpha_unusable = bm.bmBitsPixel < 32 || px.chunks_exact(4).all(|p| p[3] == 0);
    if alpha_unusable {
        let mut mbmi: BITMAPINFO = unsafe { std::mem::zeroed() };
        mbmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        mbmi.bmiHeader.biWidth = w as i32;
        mbmi.bmiHeader.biHeight = -(h as i32);
        mbmi.bmiHeader.biPlanes = 1;
        mbmi.bmiHeader.biBitCount = 32;
        mbmi.bmiHeader.biCompression = BI_RGB;
        let mut mpx = vec![0u8; (w as usize) * (h as usize) * 4];
        let got = unsafe {
            GetDIBits(
                hdc,
                mask.0,
                0,
                h,
                mpx.as_mut_ptr() as *mut _,
                &mut mbmi,
                DIB_RGB_COLORS,
            )
        };
        if got != 0 {
            for (p, m) in px.chunks_exact_mut(4).zip(mpx.chunks_exact(4)) {
                p[3] = if m[0] == 0 && m[1] == 0 && m[2] == 0 {
                    0xff
                } else {
                    0
                };
            }
        }
    }

    Some((w, h, px))
}

/// 非 Windows 平台：没有这一档能力（本项目只在 Windows 上落地）。
/// 留一个空实现是为了这一层在别的平台上仍然编得过
#[cfg(not(windows))]
fn extract_rgba(_path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::model::{PackageKind, ProviderId};

    /// 单测的沙箱目录：**每个进程一个**（并行跑同一个二进制时不会互踩），
    /// 收尾删净。⚠️ 绝不碰 `%APPDATA%`——目录是传进来的
    fn sandbox(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qb_cli_icon_{}_{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建沙箱");
        dir
    }

    fn pkg(id: &str, install: Option<&Path>, cmds: &[&str]) -> CliPackage {
        CliPackage {
            id: id.into(),
            provider: ProviderId::Npm,
            name: id.into(),
            version: None,
            latest: None,
            install_path: install.map(|p| p.to_string_lossy().into_owned()),
            executables: cmds.iter().map(|s| s.to_string()).collect(),
            kind: PackageKind::Unknown,
            dirs: Vec::new(),
        }
    }

    /// 手写一张 .ico（32 位或 24 位各一档）。**夹具自己造**，不拿系统里的
    /// `notepad.exe` 当素材——那是把测试的前提外包给环境（AGENTS 那一条）
    fn make_ico(w: usize, h: usize, bitcount: u16, rgba: &[u8], transparent: &[bool]) -> Vec<u8> {
        // 颜色区：ICO 里是**自下而上**的 BGRA(32) / BGR(24)
        let mut xor = Vec::new();
        for y in (0..h).rev() {
            for x in 0..w {
                let i = (y * w + x) * 4;
                xor.push(rgba[i + 2]);
                xor.push(rgba[i + 1]);
                xor.push(rgba[i]);
                if bitcount == 32 {
                    xor.push(rgba[i + 3]);
                }
            }
            while xor.len() % 4 != 0 {
                xor.push(0);
            }
        }
        // AND mask：1 = 透明（每行按 4 字节对齐）
        let stride = w.div_ceil(32) * 4;
        let mut mask = Vec::new();
        for y in (0..h).rev() {
            let mut row = vec![0u8; stride];
            for x in 0..w {
                if transparent[y * w + x] {
                    row[x / 8] |= 1 << (7 - (x % 8));
                }
            }
            mask.extend(row);
        }
        let mut img = Vec::new();
        img.extend(40u32.to_le_bytes()); // biSize
        img.extend((w as i32).to_le_bytes());
        img.extend(((h * 2) as i32).to_le_bytes()); // 高度 = 颜色 + 掩码
        img.extend(1u16.to_le_bytes()); // biPlanes
        img.extend(bitcount.to_le_bytes());
        img.extend(0u32.to_le_bytes()); // BI_RGB
        img.extend(((xor.len() + mask.len()) as u32).to_le_bytes());
        img.extend([0u8; 16]);
        img.extend(&xor);
        img.extend(&mask);

        let mut out = Vec::new();
        out.extend(0u16.to_le_bytes()); // reserved
        out.extend(1u16.to_le_bytes()); // 1 = icon
        out.extend(1u16.to_le_bytes()); // 条目数
        out.push(w as u8);
        out.push(h as u8);
        out.push(0); // 调色板
        out.push(0);
        out.extend(1u16.to_le_bytes());
        out.extend(bitcount.to_le_bytes());
        out.extend((img.len() as u32).to_le_bytes());
        out.extend(22u32.to_le_bytes()); // 6 + 16
        out.extend(&img);
        out
    }

    /// 一张 32×32 的图：左半红不透明、右半绿半透明
    fn pattern_32() -> (Vec<u8>, Vec<bool>) {
        let (w, h) = (32usize, 32usize);
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                if x < w / 2 {
                    rgba[i] = 200;
                    rgba[i + 1] = 40;
                    rgba[i + 2] = 40;
                    rgba[i + 3] = 255;
                } else {
                    rgba[i] = 40;
                    rgba[i + 1] = 200;
                    rgba[i + 2] = 40;
                    rgba[i + 3] = 128;
                }
            }
        }
        (rgba, vec![false; w * h])
    }

    /// 32 位图标：**alpha 就在图里**，提取后逐点对得上（含半透明那半边）
    #[test]
    #[cfg(windows)]
    fn a_32bit_icon_keeps_its_own_alpha() {
        let dir = sandbox("a32");
        let (rgba, transparent) = pattern_32();
        let ico = dir.join("tool.ico");
        std::fs::write(&ico, make_ico(32, 32, 32, &rgba, &transparent)).unwrap();

        let (w, h, got) = extract_rgba(&ico).expect("32 位图标应当提得出来");
        assert_eq!((w, h), (32, 32));
        // 左上角那颗
        assert_eq!(&got[0..4], &[200, 40, 40, 255], "左上角应是那个红");
        // 右上角那颗（半透明那半边，alpha 必须原样留住）
        let i = 31 * 4;
        assert_eq!(&got[i..i + 4], &[40, 200, 40, 128], "右上角应是那个半透明绿");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 24 位图标：**alpha 是空的**，透明那一档必须从 AND mask 补出来——
    /// 不补的话这类图标在界面上是一块黑
    #[test]
    #[cfg(windows)]
    fn a_24bit_icon_gets_its_alpha_from_the_and_mask() {
        let dir = sandbox("a24");
        let (w, h) = (32usize, 32usize);
        let rgba = {
            let mut v = vec![0u8; w * h * 4];
            for p in v.chunks_exact_mut(4) {
                p[0] = 10;
                p[1] = 20;
                p[2] = 30;
                p[3] = 255;
            }
            v
        };
        // 只让左上角那一颗透明
        let mut transparent = vec![false; w * h];
        transparent[0] = true;
        let ico = dir.join("tool24.ico");
        std::fs::write(&ico, make_ico(w, h, 24, &rgba, &transparent)).unwrap();

        let (gw, gh, got) = extract_rgba(&ico).expect("24 位图标应当提得出来");
        assert_eq!((gw, gh), (32, 32));
        assert_eq!(got[3], 0, "mask 标了透明的那一颗 alpha 必须是 0");
        let i = 31 * 4;
        assert_eq!(got[i + 3], 255, "mask 没标的必须是不透明");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 没有图标资源的文件（npm 全局包那些 `.cmd` 垫片就是这一档）⇒ **不提**。
    /// 这条判据是整件事的地基：换回 `SHGetFileInfoW` 的话这里会拿到一颗通用脚本图标
    #[test]
    #[cfg(windows)]
    fn a_file_without_icon_resources_yields_nothing() {
        let dir = sandbox("noicon");
        let cmd = dir.join("tool.cmd");
        std::fs::write(&cmd, "@echo off\r\nnode cli.js %*\r\n").unwrap();
        assert!(
            extract_rgba(&cmd).is_none(),
            "没有图标资源的文件必须返回 None（别画通用脚本图标）"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 候选顺序：**与命令名同名的 exe 排在前**，其余按路径排在后；
    /// 跳过 `.git`/`node_modules`/`target`
    #[test]
    fn candidates_put_the_matching_exe_first_and_skip_heavy_dirs() {
        let dir = sandbox("cand");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("dist/vendor")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
        std::fs::write(dir.join("bin/deno.exe"), b"x").unwrap();
        std::fs::write(dir.join("dist/vendor/fastlist.exe"), b"x").unwrap();
        std::fs::write(dir.join("node_modules/pkg/deep.exe"), b"x").unwrap();
        std::fs::write(dir.join("readme.txt"), b"x").unwrap();

        let p = pkg("npm:deno", Some(&dir), &["deno"]);
        let got = exe_candidates(&p);
        assert_eq!(
            got.first().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()),
            Some("deno.exe".to_string()),
            "与命令名同名的排第一"
        );
        assert!(
            !got.iter().any(|p| p.ends_with("deep.exe")),
            "node_modules 底下的不算候选"
        );
        assert_eq!(got.len(), 2, "只有两个候选：deno.exe 与 fastlist.exe");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 名字形状：`<16 位包哈希>-<16 位源哈希>.png`。两个哈希都要**跨进程稳定**
    /// （`DefaultHasher` 每个进程换种子 ⇒ 缓存永远不命中）
    #[test]
    fn the_file_name_is_stable_across_processes() {
        assert_eq!(pkg_key("npm:pnpm"), pkg_key("npm:pnpm"));
        assert_ne!(pkg_key("npm:pnpm"), pkg_key("npm:pn"));
        assert_eq!(pkg_key("npm:pnpm").len(), 16);
        // 拿一个写死的期望值钉住算法本身：改哈希函数（或者换成 DefaultHasher）
        // 会让这一条红，而不是让"缓存偶尔不命中"这种没人看得见的现象漂出去
        assert_eq!(fnv1a(b"npm:pnpm"), fnv1a(b"npm:pnpm"));
        assert_eq!(format!("{:016x}", fnv1a(b"a")), "af63dc4c8601ec8c");
    }

    /// 缓存键必须**跟着那颗 exe 换版走**（名字里两样：修改时间、大小）——
    /// 这是整条缓存设计的地基：不变的话"换了版还画着旧图标"，
    /// 而这件事界面上看不出来（旧图标看着也很正常）
    #[test]
    fn the_source_key_follows_the_exe_that_it_points_at() {
        let dir = sandbox("srckey");
        let a = dir.join("tool.exe");
        std::fs::write(&a, b"one").unwrap();
        let k1 = source_key(&a).expect("stat 得到");
        assert_eq!(source_key(&a).as_deref(), Some(k1.as_str()), "同一个文件两次要一样");

        // 换版：**同样长度**的两份内容（3 字节）——只有修改时间会变。
        // 这一条专盯"名字里含 mtime"这件事（不含的话两次会算出同一个名字，
        // 于是换了版还画着旧图标，而旧图标看着也很正常）
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&a, b"two").unwrap();
        assert_ne!(
            source_key(&a).as_deref(),
            Some(k1.as_str()),
            "只有修改时间变也要换键"
        );

        // 内容长度也是键的一部分。⚠️ 它**没有独立判据**：上面那条已经能抓住换版
        // （文件系统的时间戳精度不够时它才顶上），所以别读成"这一项被测过"
        std::fs::write(&a, b"longer-content-here").unwrap();
        assert_ne!(source_key(&a).as_deref(), Some(k1.as_str()));

        // 路径也是键的一部分（两个包各自的工具程序不会互相顶掉）
        let b = dir.join("other.exe");
        std::fs::write(&b, b"one").unwrap();
        assert_ne!(source_key(&b), source_key(&a));

        // stat 不到（文件不在了）⇒ 没有键，调用方据此跳过这个候选
        assert!(source_key(&dir.join("nope.exe")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 落盘那张图**真是一张 PNG**（签名 + IHDR 里的宽高），不是"写成功了"就算数
    #[test]
    fn the_written_file_is_a_png_of_the_right_size() {
        let dir = sandbox("png");
        let path = dir.join("x.png");
        let mut rgba = vec![0u8; 8 * 8 * 4];
        for p in rgba.chunks_exact_mut(4) {
            p[3] = 255;
        }
        assert!(write_png(&path, 8, 8, &rgba), "写入应当成功");
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            &bytes[..8],
            &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
            "必须是 PNG"
        );
        assert_eq!(&bytes[12..16], b"IHDR");
        let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        assert_eq!((w, h), (8, 8), "IHDR 里的宽高要对");
        // 尺寸对不上的输入要拒（宁可没图标，也别写一张畸形的图）
        assert!(!write_png(&dir.join("bad.png"), 8, 8, &rgba[..16]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 缓存索引：只认自己那套命名，同一个包有多张时取最新的那张
    #[test]
    fn cached_index_reads_only_our_own_names() {
        let dir = sandbox("index");
        let key = pkg_key("npm:pnpm");
        // 同一个包两张：旧的那张 mtime 早一点
        let old = dir.join(format!("{key}-00000000deadbeef.png"));
        let new = dir.join(format!("{key}-11111111deadbeef.png"));
        std::fs::write(&old, b"old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&new, b"new").unwrap();
        // 别人的文件：名字不合形状，一概不理
        std::fs::write(dir.join("what-is-this.png"), b"x").unwrap();
        std::fs::write(dir.join("readme.txt"), b"x").unwrap();

        let idx = cached_index(&dir);
        assert_eq!(idx.len(), 1, "只该认下我们自己那一张");
        assert_eq!(idx.get(&key).map(|p| p.file_name().unwrap().to_owned()), Some(
            new.file_name().unwrap().to_owned()
        ));
        // 调用方那一层：**按包 id 取**，不用自己拼哈希
        let by_id = cached_icons(&[pkg("npm:pnpm", None, &[]), pkg("npm:bun", None, &[])], &dir);
        assert_eq!(by_id.get("npm:pnpm"), Some(&new));
        assert!(by_id.get("npm:bun").is_none(), "没提过图的包不该有这一条");
        // 目录不存在时是"空"，不是错
        assert!(cached_index(&dir.join("nope")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 旧的那张要删掉，别人的文件一个都不许碰
    #[test]
    fn pruning_removes_only_our_own_stale_files() {
        let dir = sandbox("prune");
        let key = pkg_key("npm:pnpm");
        let other = pkg_key("npm:bun");
        let keep = dir.join(format!("{key}-22222222deadbeef.png"));
        let stale = dir.join(format!("{key}-00000000deadbeef.png"));
        let otherpkg = dir.join(format!("{other}-33333333deadbeef.png"));
        let foreign = dir.join("notes.txt");
        let tmp = dir.join(format!("{key}-44444444deadbeef.tmp"));
        for p in [&keep, &stale, &otherpkg, &foreign, &tmp] {
            std::fs::write(p, b"x").unwrap();
        }

        prune_siblings(&dir, &key, &keep);

        assert!(keep.is_file(), "刚写的那张必须还在");
        assert!(!stale.exists(), "同一个包的旧图要删掉");
        assert!(otherpkg.is_file(), "别的包的不能碰");
        assert!(foreign.is_file(), "别人的文件不能碰");
        assert!(tmp.is_file(), "不是 .png 的不能碰（临时文件由写入那一路收）");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `install_path` 缺失 / 不是目录时：没有候选，不报错
    #[test]
    fn no_install_path_means_no_candidates() {
        assert!(exe_candidates(&pkg("npm:x", None, &["x"])).is_empty());
        let missing = std::env::temp_dir().join("qb_cli_icon_definitely_missing_dir");
        assert!(exe_candidates(&pkg("npm:x", Some(&missing), &["x"])).is_empty());
    }
}
