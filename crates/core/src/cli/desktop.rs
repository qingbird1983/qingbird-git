//! 桌面关联检测：这颗 CLI 工具跟桌面应用是什么关系（设计 §4、§7.1）。
//!
//! **只做"可执行文件同目录 GUI 主程序"这一个高置信信号**，依据是 PE 可选头里的
//! `Subsystem` 字段（2 = GUI、3 = 控制台）——它是这颗 exe **自己想装成什么样**，
//! 不是按名字/描述猜的（设计 §4 明写：中置信度启发式一档不做，误报会让用户卸错东西）。
//!
//! ⚠️ 另一颗高置信信号（开始菜单与桌面上的 `.lnk` 扫描）**本轮没做**，原因不是省事：
//! `windows-sys 0.61.2` 的 `Win32_UI_Shell` 只导出 `ShellLink` / `ShellLinkObject`
//! 两个 **CLSID 常量**，没有 `IShellLinkW` / `IPersistFile` 的接口与 vtable（实测 grep
//! 整个 `windows-sys` 源码树零命中）。要补上得先定下面两条路走哪条，两条都有代价：
//! ① 引 `windows` crate——它带接口定义，但是一整棵新的编译树；
//! ② 自己按 IDL 逐槽搓 vtable——槽位错一格就是**静静拿到别的函数的返回值**，
//!    而它"错得看不出来"，得先有一台拿真 `.lnk` 对照的台子。
//! ⇒ 这一格留在 `docs/roadmap/cli.md` 等拍，**不写靠猜的代码**。
//! 少报了只是少一行「命令行 + 桌面程序」的结论（`CliOnly` / `Unknown` 两种落法都不骗人）。
//!
//! 判据是纯函数 [`classify`]：外部世界（哪些 exe、各自什么子系统）由调用方带进来，
//! 单测在假世界里把真值表钉死——**不许真去盘上找一个 GUI 程序来当夹具**。

use super::icon;
use super::model::{CliPackage, PackageKind};
use std::io::Read;
use std::path::Path;
/// 只有单测的临时目录用得上（`scratch`）——收在这里是为了不让非测试构建多一条未用导入
#[cfg(test)]
use std::path::PathBuf;

/// 可选头内 `Subsystem` 的偏移：**PE32 与 PE32+ 都是 68**（两者前面那截字段宽度不同，
/// 但这一格对齐在同一处）。加上 `"PE\0\0"`(4) 与 COFF 头(20) 才是从 PE 签名起算的偏移。
const SUBSYSTEM_AT: usize = 4 + 20 + 68;
/// `IMAGE_SUBSYSTEM_WINDOWS_GUI`。控制台是 3，其余值（原生/驱动/EFI…）都不是桌面程序
const GUI_SUBSYSTEM: u16 = 2;

/// 探过的一颗 exe：只要判定用得着的两样，不留路径（判定不看它在哪儿，
/// 只问"是不是这颗包的"与"它是什么子系统"）。
pub(crate) struct Probe {
    /// 文件名词干与这颗包自报的某条命令同名（`code.cmd` 那条命令 vs `Code.exe`）
    pub(crate) own: bool,
    pub(crate) subsystem: Option<u16>,
}

/// 判定（**纯函数**）。
///
/// `dir` = 这颗包的安装目录（`None` = 拿不到，或它是好几颗包共用的目录）；
/// `exes` = 在那个目录里探到的 exe（共用目录时只放它自己那几条命令的）。
pub(crate) fn classify(dir: Option<&Path>, exes: &[Probe]) -> PackageKind {
    // 没有任何能归到它头上的证据：`CliOnly` 是一句**有值的结论**，
    // 没看过就说"纯命令行"是在替用户下判断（设计 §4 要的是两个高置信信号，不是猜）
    if dir.is_none() && exes.is_empty() {
        return PackageKind::Unknown;
    }
    let any_gui = exes
        .iter()
        .any(|e| e.subsystem == Some(GUI_SUBSYSTEM));
    if !any_gui {
        // 目录看过、命令探过，没有 GUI 主程序 ⇒ 这一颗就是纯命令行
        return PackageKind::CliOnly;
    }
    // 有 GUI 主程序，两种关系的分界是**它是不是那条命令本身**：
    // 是 ⇒ 用户敲的那条命令开的其实是桌面程序（桌面应用附带 CLI）；
    // 不是 ⇒ 这颗包同时装了命令行与桌面两副面孔（卸载会连带桌面应用）
    if exes
        .iter()
        .any(|e| e.own && e.subsystem == Some(GUI_SUBSYSTEM))
    {
        PackageKind::DesktopWithCli
    } else {
        PackageKind::CliWithDesktop
    }
}

/// 这颗包跟桌面有没有关系。**会读盘**（走目录 + 每颗 exe 读 1 KB），调用方放后台线程。
///
/// `shared_install_dir` = 它的 `install_path` 是好几颗包**共用**的目录
/// （cargo 的 `$CARGO_HOME\bin`）：那种目录里躺着别人的 exe，走一圈会把邻居的桌面程序
/// 记在它头上 ⇒ 改成只按它自报的命令名取自己那一颗。
pub(crate) fn detect(pkg: &CliPackage, shared_install_dir: bool) -> PackageKind {
    let dir = pkg
        .install_path
        .as_deref()
        .map(Path::new)
        .filter(|d| d.is_dir());
    let exes = match dir {
        None => Vec::new(),
        Some(d) if shared_install_dir => probes_of_commands(d, &pkg.executables),
        Some(_) => probes_of_candidates(pkg),
    };
    classify(dir, &exes)
}

/// 共用目录里只认自己那几颗：`<命令>.exe`（cargo 的 bin 名就是命令名 + 扩展名）。
fn probes_of_commands(dir: &Path, commands: &[String]) -> Vec<Probe> {
    commands
        .iter()
        .map(|c| dir.join(format!("{c}.exe")))
        .filter(|p| p.is_file())
        .map(|p| Probe {
            own: true,
            subsystem: subsystem_of(&p),
        })
        .collect()
}

/// 私有目录里走一圈候选（**复用图标那一套**：深度 2、跳过 `.git` / `node_modules` /
/// `target`、4000 个目录项上限）——同一个"这颗包的 exe 在哪"只允许有一个出处。
fn probes_of_candidates(pkg: &CliPackage) -> Vec<Probe> {
    icon::exe_candidates(pkg)
        .iter()
        .map(|p| Probe {
            own: icon::matches_command(p, &pkg.executables),
            subsystem: subsystem_of(p),
        })
        .collect()
}

/// 读一颗 exe 的 PE 头拿 `Subsystem`。不是 PE（`.cmd` 垫片、脚本）或读不出来 → `None`。
fn subsystem_of(path: &Path) -> Option<u16> {
    let f = std::fs::File::open(path).ok()?;
    let mut head = Vec::with_capacity(1024);
    // 只要头那一截：`e_lfanew` 一般 ~0x80，1024 够（`take` 保证了不会把整个 exe 读进来）
    f.take(1024).read_to_end(&mut head).ok()?;
    subsystem_from_bytes(&head)
}

/// 从 PE 字节里取 `Subsystem`（**纯函数**：单测喂手写的头，不去盘上找真 exe）。
fn subsystem_from_bytes(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 0x40 || !bytes.starts_with(b"MZ") {
        return None;
    }
    let lfanew = u32::from_le_bytes(bytes[0x3C..0x40].try_into().ok()?) as usize;
    if !bytes.get(lfanew..)?.starts_with(b"PE\0\0") {
        return None;
    }
    let at = lfanew + SUBSYSTEM_AT;
    let b = bytes.get(at..at + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

/// 探到的 exe 落到盘上的那些（临时目录用；收尾会删干净）
#[cfg(test)]
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "qb-desktop-{}-{}",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::model::ProviderId;

    const GUI: Option<u16> = Some(2);
    const CONSOLE: Option<u16> = Some(3);
    const NOT_PE: Option<u16> = None;

    fn probe(own: bool, subsystem: Option<u16>) -> Probe {
        Probe { own, subsystem }
    }

    /// 真值表。四档里三档由这里定（`Unknown` 只留给"什么都没看到"）。
    ///
    /// 为什么值得一条单测：这四个结论会写进界面（右栏「桌面关联」那一行）与阶段三的
    /// 卸载提示，**说错方向不是显示问题**——`DesktopWithCli` 说成 `CliWithDesktop`
    /// 会让用户以为"只卸命令行不影响桌面"。而它们全是同一张表分出来的：
    /// 表里那一臂错一格，症状是"某一类工具的建议卸法反了"。
    #[test]
    fn the_kind_truth_table() {
        let dir = Path::new("C:\\pkg");

        // 什么都没看 = 不下结论
        assert_eq!(classify(None, &[]), PackageKind::Unknown);
        // 目录看过了、里面没有 GUI 主程序 ⇒ 纯命令行（npm 那种只有 JS 的包走这条）
        assert_eq!(classify(Some(dir), &[]), PackageKind::CliOnly);
        assert_eq!(
            classify(Some(dir), &[probe(true, CONSOLE), probe(false, NOT_PE)]),
            PackageKind::CliOnly
        );
        // 有 GUI 但不是那条命令 ⇒ 同包另带一支桌面程序
        assert_eq!(
            classify(Some(dir), &[probe(false, GUI), probe(true, CONSOLE)]),
            PackageKind::CliWithDesktop
        );
        // 那条命令本身就是桌面程序 ⇒ 桌面应用附带 CLI
        assert_eq!(
            classify(Some(dir), &[probe(true, GUI)]),
            PackageKind::DesktopWithCli
        );
        // 同名的那颗是 GUI 就算桌面为主（哪怕另外还有一颗同名的控制台 exe）
        assert_eq!(
            classify(Some(dir), &[probe(true, GUI), probe(true, CONSOLE)]),
            PackageKind::DesktopWithCli
        );
        // 有候选就不是"什么都没看到"（共用目录那一档：`dir` 有、exes 来自按命令名取的）
        assert_eq!(classify(None, &[probe(true, CONSOLE)]), PackageKind::CliOnly);
    }

    /// PE 规范里 `Subsystem` 在**可选头偏移 68**（PE32 与 PE32+ 同一处）。
    ///
    /// ⚠️ 这一条**故意手抄规范、不引用 `SUBSYSTEM_AT`**：拿常量验常量就是把期望值外包给
    /// 真源自己，把常量改成 70 一个测试都不会红（2026-10-13 变异 M2 实测过——当时
    /// `fake_pe` 也引用同一个常量，两侧一起挪，全绿）。
    #[test]
    fn the_subsystem_offset_comes_from_the_spec() {
        assert_eq!(SUBSYSTEM_AT, 4 + 20 + 68, "4 = PE 签名、20 = COFF 头、68 = 可选头里那一格");
    }

    /// `Subsystem` 那两字节的位置：PE32 与 PE32+ 的可选头大小不同，但这一格同偏移。
    #[test]
    fn subsystem_comes_from_the_pe_optional_header() {
        // PE 头里 Magic 的两档（0x10B / 0x20B）不影响 Subsystem 偏移——夹具按同一个
        // 位置造，只要 lfanew 不同就够证明"跟着 e_lfanew 走"而不是写死一个偏移
        assert_eq!(subsystem_from_bytes(&fake_pe(0x80, 2)), Some(2));
        assert_eq!(subsystem_from_bytes(&fake_pe(0x100, 3)), Some(3));
        assert_eq!(subsystem_from_bytes(&fake_pe(0x40, 2)), Some(2));

        // 不是 PE 的四条：`.cmd` 垫片、空、只有 MZ、e_lfanew 指到文件外
        assert_eq!(subsystem_from_bytes(b"@echo off\r\n"), None);
        assert_eq!(subsystem_from_bytes(&[]), None);
        assert_eq!(subsystem_from_bytes(b"MZ"), None);
        let mut out_of_range = fake_pe(0x80, 2);
        out_of_range[0x3C..0x40].copy_from_slice(&0xffff_u32.to_le_bytes());
        assert_eq!(subsystem_from_bytes(&out_of_range), None);
        // 戳在 PE 签名上、但可选头被截断
        let mut truncated = fake_pe(0x80, 2);
        truncated.truncate(0x80 + SUBSYSTEM_AT);
        assert_eq!(subsystem_from_bytes(&truncated), None);
    }

    /// 读**真** PE：本进程自己就是 cargo 编出来的 exe（控制台子系统）。
    /// 前面那条只证明"跟手写夹具自洽"，这条把偏移钉在真文件上。
    #[test]
    fn a_real_exe_reads_back_its_real_subsystem() {
        let me = std::env::current_exe().expect("拿不到自己的路径");
        assert_eq!(
            subsystem_of(&me),
            Some(3),
            "cargo test 的产物是控制台子系统（这条红了先看夹具还是先看这里）"
        );
    }

    /// 走盘那一趟（`detect` 的私有目录分支）：目录里那颗 GUI exe 要能被认出来，
    /// 且"是不是那条命令"按**文件名词干**判。
    #[test]
    fn a_private_dir_is_walked_for_gui_programs() {
        let dir = scratch("private");
        std::fs::write(dir.join("code.exe"), fake_pe(0x80, 2)).unwrap();
        std::fs::write(dir.join("helper.exe"), fake_pe(0x80, 3)).unwrap();
        std::fs::write(dir.join("task.cmd"), b"@echo off\n").unwrap();

        let mut pkg = fake_pkg(&dir, &["code"]);
        // `code` 那条命令指的就是那颗 GUI 程序 ⇒ 桌面为主
        assert_eq!(detect(&pkg, false), PackageKind::DesktopWithCli);

        // 同一份目录，命令换成一个别的名字 ⇒ GUI 是"另带一支"
        pkg.executables = vec!["helper".into()];
        assert_eq!(detect(&pkg, false), PackageKind::CliWithDesktop);

        // 目录里没有 GUI exe（只剩 `.cmd` 垫片那种）⇒ 纯命令行
        pkg.executables = vec!["code".into()];
        std::fs::remove_file(dir.join("code.exe")).unwrap();
        assert_eq!(detect(&pkg, false), PackageKind::CliOnly);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 共用目录（cargo 的 `$CARGO_HOME\bin`）：邻居那颗 GUI 程序**不许**记到它头上。
    #[test]
    fn a_shared_dir_only_counts_its_own_commands() {
        let dir = scratch("shared");
        std::fs::write(dir.join("mine.exe"), fake_pe(0x80, 3)).unwrap();
        std::fs::write(dir.join("neighbour.exe"), fake_pe(0x80, 2)).unwrap();

        let pkg = fake_pkg(&dir, &["mine"]);
        assert_eq!(
            detect(&pkg, true),
            PackageKind::CliOnly,
            "共用目录里别人的 GUI 程序不算它的桌面关联"
        );
        // 同一份目录按私有走一圈就会误报——这正是 `shared_install_dir` 存在的理由
        assert_eq!(detect(&pkg, false), PackageKind::CliWithDesktop);

        // 自己那条命令就是 GUI ⇒ 桌面为主
        std::fs::write(dir.join("mine.exe"), fake_pe(0x80, 2)).unwrap();
        assert_eq!(detect(&pkg, true), PackageKind::DesktopWithCli);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 手搓一个够读到 `Subsystem` 的最小 PE：DOS 头 + `e_lfanew` + 签名 + 那两字节。
    ///
    /// ⚠️ 这里的 `4 + 20 + 68` 是**照规范手写的**，不许换成 `SUBSYSTEM_AT`：引用同一个常量
    /// 就是"两侧一起动"，把常量改错一条测试都不会红（理由与判据同
    /// `the_subsystem_offset_comes_from_the_spec`）。
    fn fake_pe(lfanew: usize, subsystem: u16) -> Vec<u8> {
        let at = lfanew + 4 + 20 + 68;
        let mut b = vec![0u8; at + 2];
        b[..2].copy_from_slice(b"MZ");
        b[0x3C..0x40].copy_from_slice(&(lfanew as u32).to_le_bytes());
        b[lfanew..lfanew + 4].copy_from_slice(b"PE\0\0");
        b[at..at + 2].copy_from_slice(&subsystem.to_le_bytes());
        b
    }

    fn fake_pkg(dir: &Path, commands: &[&str]) -> CliPackage {
        CliPackage {
        program_size: None,
        release_notes: None,
            id: "npm:demo".into(),
            provider: ProviderId::Npm,
            name: "demo".into(),
            version: Some("1.0.0".into()),
            latest: None,
            install_path: Some(dir.to_string_lossy().into_owned()),
            executables: commands.iter().map(|c| c.to_string()).collect(),
            kind: PackageKind::Unknown,
            dirs: Vec::new(),
        }
    }
}
