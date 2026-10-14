//! winget provider：解析 `winget list` 那张**给人看的表格**。
//!
//! ## 为什么不读 JSON
//! 上游没有机器可读输出：`src/AppInstallerCLICore/Commands/ListCommand.cpp` 的
//! `GetArguments()` 里没有 `--output`/JSON 档（Issue #3051「Winget List - add
//! machine-parseable output」还开着），`WorkflowBase.cpp` 的 `ReportListResult`
//! 直接走 `OutputInstalledPackagesTable`。设计稿 §6 写的 PowerShell
//! `Get-WinGetPackage` 要一个本机没装的 PSGallery 模块（`Microsoft.WinGet.Client`），
//! 不值得为它去装模块 ⇒ 落地改成**解析表格**（这条偏离登记在 `docs/roadmap/cli.md`）。
//!
//! ## 表格长什么样（照上游源码核的，不是照记忆写的）
//! `TableOutput.cpp`：列宽 `MaxLength` = max(表头, **各数据行**) 的**显示宽度**；
//! 每个单元格后面补 `MaxLength - 宽度 + 1` 个空格（**末列不补**）；
//! 表头行之后打**一整行 `-`**（长度 = ΣMaxLength + 非末列的 1）；
//! 只有 `GetConsoleWidth()` 有值且表比控制台宽时才截断（**我们的派生进程没有控制台
//! ⇒ nullopt ⇒ 不截断**，所以不用管结尾那个 `…`）。五列依次是
//! Name / Id / Version / Available / Source（zh-CN 的表头文案从
//! `Localization/Resources/zh-CN/winget.resw` 核过：名称 / ID / 版本 / 可用 / 源）。
//! ⇒ **判据：拿表头各 token 的显示列位当切片起点切数据行**，不是按"两个以上空格"切
//! ——空单元格会补出 `MaxLength + 1` 个空格，与列分隔分不开。
//!
//! ## 三条不演的
//! 1. **进度线**：winget 用 `\r` 原地重画转圈，所以 `stdout` 不能按 `'\n'` 切——
//!    `'\n'` 与 `'\r'` 都当行分界。
//! 2. **命令名留空**：这张表**没有命令列**（`winget list` 不给 bins），而"从包名猜命令"
//!    是明令禁止的（见 `model.rs` 那句注释）⇒ `executables` 是空表。
//!    连带图标提取与桌面关联这两条要"命令名 / 安装目录"的路，winget 这一档都**没有证据**。
//! 3. **认不出就整趟报错，不给半张表**：列位对不上的行一律不猜（宽度表不认那行里的字符时
//!    会出现），只要有一行对不上就返回 `Err`——与"不按名字猜目录"同一条纪律：
//!    少报只是少一行，错报在阶段三就是删别人的东西。
//!
//! ## 两条只有真输出才看得出来（本机 v1.29.380 实测，2026-10-13）
//!
//! ⚠️ 沙箱里 `winget.exe` 一执行就返回 `-1978335231`（`0x8A150001`）且零输出；
//! **完全权限下它跑得起来**，`winget list` 给 241 行 / 46 KB ⇒ 下面三条是量出来的：
//!
//! 1. **238 行数据里只有 103 行带「源」**，剩下 135 行的 Id 长成
//!    `ARP\Machine\X64\{GUID}`——那是卸载注册表里的记录，winget 说不出它属于哪个源。
//!    ⇒ **只收带源的那 103 行**（`Row::ArpOnly`）：`ARP\…` 不是 winget 的身份，
//!    阶段三拿它 `winget upgrade --id` 也升不动。
//!    ⚠️ 代价是**少报**：MSI 装的 Node.js 正是 ARP 行，会漏掉。
//!    **这条取舍与那三个数登记在 `docs/roadmap/cli.md`，要不要放开由他拍。**
//! 2. **同一个 Id 会出现两行**（本机微信 = `XPFCKBRNFZQ62G`，`4.1.15.13` 与
//!    `3.9.12.55` 两个 msstore 版本并存）。`CliPackage::id` 是界面选中、图标缓存、
//!    落库三处的键，重复会让后一行悄悄盖掉前一行 ⇒ **去重，留先打的那条**。
//! 3. 顺带验了两个"应该没有"：整份输出 **0 个 VT 序列**（重定向之后 winget 不画进度线）、
//!    **0 个 `…`**（没有控制台 ⇒ `GetConsoleWidth()` 拿不到宽度 ⇒ 不截断）。
//!    `strip_ansi` 与"不截断"这两条留着当兜底，但本机数据里一次都没被用到。
//!
//! 4. ⚠️ **宽度表差一格，这是已知上界不是容差**：winget 用 ICU 的 east-asian-width 排表，
//!    Rust 侧 `unicode-width` 对少数**表情符号形态**的字符算 2 列、ICU 算 1 列 ⇒ 那种行
//!    会整体错一位。本机 238 行里这样的是 **2 行**——都是 winget 自己输出时就已经损坏的行
//!    （名字里带 `\u{fffd}` 那几个怪字），而且**恰好都是 ARP 行**、被上面第 1 条丢掉了
//!    ⇒ 落到界面上的 83 条是准的。旁证是另一条**独立**判据：按"行尾是不是 winget/msstore"
//!    数出来的有源行数 = 103，与按切片读源格数出来的 103 **逐行一致（不一致 0 行）**。
//!    ⇒ 真要出现"带源的行被切歪"，判据会把它当 `Unknown` 整趟报错：**宁可不出数，不给错数**。
//!
//! ⇒ 单测夹具用**真输出里的行**（`REAL_ROWS`）在测试里现摆（`render`），
//! 摆法本身拿那份真输出逐字节验过一遍（见 `render` 的注释）。

use std::collections::HashSet;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{PackageProvider, LIST_TIMEOUT};
use crate::cli::exec;
use crate::cli::model::{CliPackage, PackageKind, ProviderId};
use crate::cli::plan::Step;
use crate::cli::provider::winget_id_of;
use crate::cli::upgrade::UpgradePhase;

/// 五个表头，**顺序就是切片的顺序**。zh-CN 与 en-US 两套：中文机器上 winget 打的是前者，
/// 别人机器上可能是后者；两套都不认就报错（不猜、不硬凑）。
const ZH_HEAD: [&str; 5] = ["名称", "ID", "版本", "可用", "源"];
const EN_HEAD: [&str; 5] = ["Name", "Id", "Version", "Available", "Source"];

pub struct Winget;

impl PackageProvider for Winget {
    fn id(&self) -> ProviderId {
        ProviderId::Winget
    }

    fn detect(&self) -> bool {
        exec::command_exists("winget")
    }

    fn list(&self) -> Result<Vec<CliPackage>, String> {
        let out = exec::run_program("winget", &["list"], LIST_TIMEOUT)?;
        // 退出码不为 0 但表已经打出来时照样用（`npm.rs` 同一条纪律）；两样都空才算真失败
        if !out.ok() && out.stdout.trim().is_empty() {
            return Err(format!(
                "winget list 退出码 {:?}：{}",
                out.code,
                first_line(&out.stderr)
            ));
        }
        parse_list(&out.stdout)
    }

    /// 升级 = `winget upgrade --id <Id> --silent`（设计 §6）。
    ///
    /// ⚠️ `--silent` 是设计稿没写的一颗：我们的子进程**没有控制台**
    /// （`CREATE_NO_WINDOW` + stdin 为 null），交互式卸载器/安装器在那儿只会挂死。
    /// 挂了的表现是这一步等到 `OP_TIMEOUT` 被杀——不如一开始就别让它问。
    /// `--accept-package-agreements` 那两颗**没加**：没在本机验过（沙箱里 winget
    /// 起不来），加一颗没验过的旗不如让它在日志里报"请先接受协议"。
    ///
    /// ⚠️ Id 从 [`CliPackage::id`] 的后半段取（`winget:Git.Git` → `Git.Git`）：
    /// `list()` 那一趟就是把显示名与 Id 分开存的（表里 Name 列不是 Id）
    fn upgrade_step(&self, pkg: &CliPackage) -> Option<Step> {
        let id = winget_id_of(pkg)?;
        Some(Step::uninstall("winget", &["upgrade", "--id", id, "--silent"]))
    }

    /// 卸载 = `winget uninstall --id <Id> --silent`（设计 §6；`--silent` 同上）。
    fn uninstall_step(&self, pkg: &CliPackage) -> Option<Step> {
        let id = winget_id_of(pkg)?;
        Some(Step::uninstall("winget", &["uninstall", "--id", id, "--silent"]))
    }

    /// 阶段观测（阶段四 P0）。**下载 / 校验两侧是 2026-10-15 本机
    /// `winget download` 抓回的真输出**（中文系统，`winget --version` =
    /// v1.29.380）：`正在下载 <url>` → 下载，`已成功验证安装程序哈希` → 校验，
    /// `已下载安装程序: <路径>` → 下载。安装侧（`正在安装` / `Installing`）
    /// **沙箱里抓不到**（winget upgrade 会真装东西，老账：要真机点一次），
    /// 按官方本地化模式补的双语关键词，标注为未取证——抓到的与推断的
    /// 在注释里分开写，别让后来人以为一起验过。
    ///
    /// ⚠️ **必须双语**：winget 的输出随系统语言变。只匹配英文关键词在
    /// 中文系统上永远不中，管道会全程「未报告」——那是把"没接上"画成
    /// "没有"。判序：**先哈希、再下载、后安装**——`已成功验证安装程序
    /// 哈希` 里含着"安装程序"四个字，松的安装关键词会把它抢走
    fn phase_of_line(&self, line: &str) -> Option<UpgradePhase> {
        const VERIFY: &[&str] = &[
            "已成功验证安装程序哈希", // 真输出（中文）
            "Successfully verified installer hash",
            "verified installer hash",
        ];
        const DOWNLOAD: &[&str] = &[
            "正在下载",           // 真输出（中文）
            "已下载安装程序",     // 真输出（中文）
            "Downloading",
            "Downloaded installer",
        ];
        const REPLACE: &[&str] = &[
            "正在安装", // 推断（沙箱抓不到安装侧）
            "Installing",
            "已成功安装", // 推断
            "Successfully installed",
        ];
        if VERIFY.iter().any(|k| line.contains(k)) {
            return Some(UpgradePhase::Verify);
        }
        if DOWNLOAD.iter().any(|k| line.contains(k)) {
            return Some(UpgradePhase::Download);
        }
        if REPLACE.iter().any(|k| line.contains(k)) {
            return Some(UpgradePhase::Replace);
        }
        None
    }
}

/// 表格文本 ⇒ 包列表。
fn parse_list(text: &str) -> Result<Vec<CliPackage>, String> {
    let clean = strip_ansi(text.strip_prefix('\u{feff}').unwrap_or(text));
    let lines: Vec<&str> = clean.split(['\n', '\r']).map(str::trim_end).collect();
    let dash = lines
        .iter()
        .position(|l| is_dash_line(l))
        .ok_or("winget list 的输出里没有表格分隔线，认不出这是 winget 的表")?;
    let header = lines[..dash]
        .iter()
        .rev()
        .find(|l| !l.trim().is_empty())
        .ok_or("winget list 的表头是空的")?;
    let starts = header_starts(header)
        .ok_or_else(|| format!("认不出 winget 的表头（winget 换语言了？）：{}", header.trim()))?;

    let mut out = Vec::new();
    let mut ids: HashSet<String> = HashSet::new();
    let mut bad = 0usize;
    for line in &lines[dash + 1..] {
        if line.trim().is_empty() {
            continue;
        }
        match parse_row(line, &starts) {
            Row::Pkg(p) => {
                // 同一个 Id 会出现两行（本机微信两个 msstore 版本并存）。`id` 是界面选中、
                // 图标缓存、落库三处的键 ⇒ 留先打的那一条（本机新版本在前），
                // 而不是让后一行悄悄盖上——那一颗包的版本号会自己变
                if ids.insert(p.id.clone()) {
                    out.push(p);
                }
            }
            // 只在卸载注册表里的那 135 行：不是错，是不要（文件头第 1 条）
            Row::ArpOnly => {}
            Row::Unknown => bad += 1,
        }
    }
    if bad > 0 {
        return Err(format!(
            "winget list 有 {bad} 行对不上列位（列宽算法不认那几行里的字符），已认出的 {} 行一并作废——不给你看半张表",
            out.len()
        ));
    }
    Ok(out)
}

/// 一整行都是 `-` 才算分隔线。要求 ≥10 列：真表的宽度至少几十列，
/// 而"包名恰好是一串横线"那种数据行不该被当成表头分隔线。
fn is_dash_line(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 10 && t.chars().all(|c| c == '-')
}

/// 从表头里认出五个列名，返回它们各自的**显示列位**——不是字节位：
/// 表头自己带汉字（`名称` 两个词 = 四列），按字节算就全错位了。
fn header_starts(header: &str) -> Option<[usize; 5]> {
    for names in [ZH_HEAD, EN_HEAD] {
        let mut starts = [0usize; 5];
        let mut from = 0usize;
        let mut all = true;
        for (i, name) in names.iter().enumerate() {
            match header[from..].find(name) {
                Some(rel) => {
                    let at = from + rel;
                    starts[i] = header[..at].width();
                    from = at + name.len();
                }
                None => {
                    all = false;
                    break;
                }
            }
        }
        if all {
            return Some(starts);
        }
    }
    None
}

/// 一行数据切片之后的三种归宿。
enum Row {
    /// 是我们的（带源：`winget` / `msstore`）
    Pkg(CliPackage),
    /// **「源」那格是空的** = 它只在卸载注册表里（Id 长成 `ARP\Machine\X64\{GUID}`）。
    /// 不是错、是不要（本机 238 行里有 135 行这样，见文件头第 1 条）
    ArpOnly,
    /// 列位对不上（宽度表不认那行里的字符）——攒起来整趟报错
    Unknown,
}

/// 一行数据 ⇒ 一个包。
fn parse_row(line: &str, starts: &[usize; 5]) -> Row {
    let cols = split_at_columns(line, starts);
    // 「源」空着就不必再看别的：这一条不是 winget 管得动的包
    if cols[4].trim().is_empty() {
        return Row::ArpOnly;
    }
    let id_raw = &cols[1];
    let id = id_raw.trim();
    let name = cols[0].trim();
    // 三种"对不上"：名字 / ID 那一格是空的；ID 里混进了空格（winget 的 ID 从来不带空格）；
    // 或者**某一格的起点落在词内部**。最后这条是这套切法唯一的自我检查：
    // 正确渲染的表里，每个格子后面至少有 1 个补齐的空格 ⇒ 每个列位前面那个字符必然是空格，
    // 而宽度算错时（汉字按 1 列算）边界就会切进词里、前一格末尾是词的最后一个字符。
    if id.is_empty() || name.is_empty() || id.contains(char::is_whitespace) {
        return Row::Unknown;
    }
    for c in &cols[..4] {
        if !c.chars().last().is_some_and(char::is_whitespace) {
            return Row::Unknown;
        }
    }
    let version = cols[2].trim();
    let available = cols[3].trim();
    Row::Pkg(CliPackage {
        // 稳定身份用 winget 的 Id（阶段三 `winget uninstall --id` 要它），
        // 而 `name` 放**给人看的显示名**（表里那一列）
        id: format!("winget:{}", id),
        provider: ProviderId::Winget,
        name: name.to_string(),
        version: (!version.is_empty()).then(|| version.to_string()),
        // 「可用」那一列就是模型里那个 `latest`：空 ⇒ None（已经是最新的）
        latest: (!available.is_empty()).then(|| available.to_string()),
        // 这张表既没有安装目录也没有命令列 ⇒ 不猜（见文件头第 2 条）
        install_path: None,
        executables: Vec::new(),
        kind: PackageKind::default(),
        dirs: Vec::new(),
    })
}

/// 按**显示列位**把一行切成五格（含补齐的空格，调用方自己 `trim`）。
/// 第一格起点之前的字符（表歪了才会有）丢掉。
fn split_at_columns(line: &str, starts: &[usize; 5]) -> [String; 5] {
    let mut out: [String; 5] = std::array::from_fn(|_| String::new());
    let mut col = 0usize;
    let mut idx = 0usize;
    for ch in line.chars() {
        while idx + 1 < starts.len() && col >= starts[idx + 1] {
            idx += 1;
        }
        if col >= starts[idx] {
            out[idx].push(ch);
        }
        col += UnicodeWidthChar::width(ch).unwrap_or(1);
    }
    out
}

/// 把 VT 序列（winget 画进度线时会带）剥掉：`ESC` 之后吞到第一个 ASCII 字母为止。
/// 只够覆盖 `\x1b[?25l` / `\x1b[32m` / `\x1b[0m` 这一族——不做完整 VT 解析。
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut esc = false;
    for ch in s.chars() {
        if esc {
            if ch.is_ascii_alphabetic() {
                esc = false;
            }
            continue;
        }
        if ch == '\u{1b}' {
            esc = true;
            continue;
        }
        out.push(ch);
    }
    out
}

/// 错误信息里只放第一行非空（winget 出错时第一行是正文，后面是提示），并且有上限
fn first_line(s: &str) -> String {
    s.split(['\n', '\r'])
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("（没有输出）")
        .chars()
        .take(200)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真输出里的行（本机 `winget list` v1.29.380，2026-10-13 抓的那 241 行里抄的）：
    /// **值是真的，空格是 [`render`] 现摆的**。
    ///
    /// 第 6、7 行是 ARP 项（源空 ⇒ 不收）；第 8、9 行是同一个 Id 的两个版本（⇒ 去重）。
    /// 这两件事都是真输出里撞出来的，不是设想出来的形态。
    const REAL_ROWS: [[&str; 5]; 9] = [
        ["7-Zip 26.03 (x64)", "7zip.7zip", "26.03", "26.04", "winget"],
        ["Git", "Git.Git", "2.55.0.2", "2.55.0.5", "winget"],
        ["GitHub CLI", "GitHub.cli", "2.93.0", "2.102.0", "winget"],
        ["Copilot CLI", "GitHub.Copilot", "v1.0.63", "v1.0.96", "winget"],
        [
            "Python 3.12.8 (64-bit)",
            "Python.Python.3.12",
            "3.12.8",
            "3.12.10",
            "winget",
        ],
        [
            "Autodesk Advanced Modeling Tools for 3ds Max 2025 1.7.0.8",
            "ARP\\Machine\\X64\\{8209FFA2-E654-4DF7-8105-376905629BFD}",
            "1.7.0.8",
            "",
            "",
        ],
        ["2345看图王", "ARP\\Machine\\X64\\2345Pic", "11.5.0.11632", "", ""],
        ["微信", "XPFCKBRNFZQ62G", "4.1.15.13", "", "msstore"],
        ["微信", "XPFCKBRNFZQ62G", "3.9.12.55", "", "msstore"],
    ];

    /// 各列宽 = max(表头, 各数据行) 的**显示宽度**（上游 `TableOutput.cpp` 的 `MaxLength`）
    fn widths(rows: &[[&str; 5]]) -> [usize; 5] {
        let mut w = [0usize; 5];
        for (i, h) in ZH_HEAD.iter().enumerate() {
            w[i] = h.width();
        }
        for r in rows {
            for i in 0..5 {
                w[i] = w[i].max(r[i].width());
            }
        }
        w
    }

    /// 照上游那条渲染规则把 `rows` 摆成一张表（zh 表头）。
    ///
    /// **为什么在测试里现摆而不是贴一张字面表**：字面表要我把几十个空格一字不差地敲进
    /// 源码——2026-10-13 真敲歪过一次（`版本` 的列位差 1），而歪掉的那一格在 diff 里
    /// 跟正常空格长得一样。
    ///
    /// ⚠️ 这个摆法**拿真输出验过**：本机那份 `winget list` 的 238 行数据先按列位切成格子、
    /// 再用它摆回去，与原件**逐字节相同**（那条用例要那台机器上的文件，验完就删了）。
    /// 留在这里的判据不依赖外部文件：`the_renderer_slices_back_into_the_same_cells`。
    fn render(rows: &[[&str; 5]]) -> String {
        let w = widths(rows);
        let mut out = String::new();
        let put = |out: &mut String, cells: &[&str; 5]| {
            for (i, c) in cells.iter().enumerate() {
                out.push_str(c);
                // 补齐 = MaxLength - 宽度 + 1；**末列不补**（上游 `SpaceAfter = false`）
                if i + 1 < cells.len() {
                    for _ in 0..(w[i] - c.width() + 1) {
                        out.push(' ');
                    }
                }
            }
            out.push('\n');
        };
        put(&mut out, &ZH_HEAD);
        // 分隔线 = Σ列宽 + 每个非末列后面那 1 个空格
        out.push_str(&"-".repeat(w.iter().sum::<usize>() + 4));
        out.push('\n');
        for r in rows {
            put(&mut out, r);
        }
        out
    }

    /// 夹具的自我检查：表头认得出、列位递增、**每条数据行按列位切回来与源头一字不差**。
    /// 少了最后这一条，"摆歪了"与"切歪了"会互相抵消成一片绿。
    #[test]
    fn the_renderer_slices_back_into_the_same_cells() {
        let table = render(&REAL_ROWS);
        let head = table.lines().next().expect("表头");
        let starts = header_starts(head).expect("zh 表头该认得出");
        assert_eq!(starts[0], 0, "第一列从第 0 列起");
        for i in 1..5 {
            assert!(starts[i] > starts[i - 1], "列位该递增：{starts:?}");
        }
        let dash = table.lines().nth(1).expect("分隔线");
        assert!(dash.chars().all(|c| c == '-'));
        for (i, r) in REAL_ROWS.iter().enumerate() {
            let line = table.lines().nth(2 + i).expect("数据行");
            let cols = split_at_columns(line, &starts);
            let got: Vec<&str> = cols.iter().map(|c| c.trim()).collect();
            assert_eq!(got, r.to_vec(), "第 {i} 行切回来变了：{line:?}");
        }
    }

    #[test]
    fn parses_the_real_rows() {
        let ps = parse_list(&render(&REAL_ROWS)).expect("这张表该认得出");
        assert_eq!(ps.len(), 6, "9 行去掉 2 行 ARP、1 行重复的微信");

        let zip = &ps[0];
        assert_eq!(zip.id, "winget:7zip.7zip");
        assert_eq!(zip.name, "7-Zip 26.03 (x64)");
        assert_eq!(zip.version.as_deref(), Some("26.03"));
        assert_eq!(zip.latest.as_deref(), Some("26.04"), "「可用」那列就是 latest");
        assert_eq!(zip.provider, ProviderId::Winget);
        assert!(zip.executables.is_empty(), "表里没有命令列 ⇒ 不留空话去猜");
        assert_eq!(zip.install_path, None);
        assert_eq!(zip.kind, PackageKind::Unknown);

        // 空「可用」是 None，不是空串（界面按 None 不画"可更新"）
        assert_eq!(ps[1].latest.as_deref(), Some("2.55.0.5"), "Git 也有新版");

        // 汉字包名：切列位按显示宽度算，一个汉字两列
        let wechat = ps.iter().find(|p| p.name == "微信").expect("汉字那一行");
        assert_eq!(wechat.id, "winget:XPFCKBRNFZQ62G");
        assert_eq!(
            wechat.version.as_deref(),
            Some("4.1.15.13"),
            "同一个 Id 的两行留先打的那条（本机新版本在前）"
        );
        assert_eq!(wechat.latest, None);

        // ARP 那两行（源空）一条都不收
        assert!(
            !ps.iter().any(|p| p.id.contains("ARP")),
            "只在卸载注册表里的那些不是 winget 的身份"
        );
    }

    /// en-US 表头也要认（别人机器上 winget 打的是它）。这里只验**表头识别**，
    /// 不摆一张英文表：本机 locale 是 zh、抓不到真英文表，摆一张假的只会多一处会漂的期望值。
    #[test]
    fn recognises_the_en_header_too() {
        let starts = header_starts("Name Id Version Available Source").expect("英文表头该认得出");
        assert_eq!(starts, [0, 5, 8, 16, 26]);
    }

    /// 进度线（ANSI + `\r` 原地重画）不能把表带歪
    #[test]
    fn survives_the_progress_line() {
        let noisy = format!("\u{1b}[?25l  55%\r{}", render(&REAL_ROWS));
        let ps = parse_list(&noisy).expect("进度线剥掉之后照样认");
        assert_eq!(ps.len(), 6);
    }

    /// **认不出就整趟报错**：一行对不上 ⇒ 连已认出的那几行也不返回（不给半张表）。
    /// 造法：让第一列**正好占满列宽**（补齐空格只剩 1 个），再把那 1 个空格删掉——
    /// 列位就切进了词里，前一格末尾是词的最后一个字符。
    #[test]
    fn refuses_to_show_half_a_table() {
        let rows = [
            ["AAAA", "x.y", "1", "", "winget"],
            ["BB", "z.w", "2", "", "winget"],
        ];
        let table = render(&rows);
        let mut lines: Vec<&str> = table.lines().collect();
        let bad = lines[3].replacen(' ', "", 1);
        assert_ne!(bad, lines[3], "那一格后面该正好只剩 1 个补齐空格");
        lines[3] = bad.as_str();
        let mixed = format!("{}\n", lines.join("\n"));
        let err = parse_list(&mixed).expect_err("有一行对不上就该整趟报错");
        assert!(
            err.contains("1 行对不上列位") && err.contains("1 行一并作废"),
            "错话要把两头的数都说出来：{err}"
        );

        // 没有分隔线 / 表头认不出（换了个我们不认的语言）
        assert!(parse_list("winget：找不到已安装的程序包").is_err());
        let ja = "名前             ID                    バージョン  利用可能  ソース\n----------------------------------------------------------------\nfoo bar\n";
        assert!(parse_list(ja).is_err());
    }

    /// 空表（一台机器一个包都没装）是 Ok(vec![])，不是错误
    #[test]
    fn an_empty_table_is_not_an_error() {
        let none: [[&str; 5]; 0] = [];
        assert_eq!(parse_list(&render(&none)).expect("空表也是表").len(), 0);
    }

}
