//! 阶段三：OpPlan——一次卸载操作的**命令序列**（设计 §4）。
//!
//! 1.0 那份叫 `CommandPlan`（单命令），本版改成序列：卸载命令之后可能还跟着
//! 几条「勾选的清理目录」，它们按序执行（设计 §11）。所以这里的主角是
//! [`Step`]——每一步是**结构化的 `program + args`**，不是拼好的 shell 字符串
//! （与 core 里 git 命令封装同一条纪律：拼字符串就没法在预览里逐字给用户看，
//! 也没法在单测里逐参数断言）。
//!
//! ## 风险分级为什么只有三档
//!
//! 设计 §4 列了 `Safe / Moderate / High / Destructive` 四档，这里**只有三档**：
//! Destructive 没有一个诚实的产出者——"这会弄坏系统"从来不是我们手里的数据
//! 能回答的事（五个 provider 没有一个给"系统组件"信号），而按名字猜一个
//! 正是设计 §4 唯一不许犯的错（误报会让用户卸错东西）。等真有可靠信号时
//! 再补那一档，不先摆着。

use std::path::PathBuf;

use crate::cli::model::{CliPackage, DirKind, PackageDir, PackageKind};

/// 这一步的风险分级。
///
/// 顺序即大小（`Ord` 派生）：一个计划的风险 = 各步取最高，直接 `max` 即可。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum RiskLevel {
    /// 删缓存：可再生，不影响功能
    Safe,
    /// 卸一个纯命令行工具：装回来就还原
    Moderate,
    /// 会连带桌面程序，或者**我们不知道会连带什么**
    High,
}

impl RiskLevel {
    /// 界面上徽章里那句话。
    pub fn label(self) -> &'static str {
        match self {
            RiskLevel::Safe => "低",
            RiskLevel::Moderate => "中",
            RiskLevel::High => "高",
        }
    }
}

/// 这一步在干什么。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepKind {
    /// 跑包管理器的卸载命令
    Uninstall,
    /// 清一个目录的内容（保留目录本身）
    CleanupDir,
}

/// 计划里的一步。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Step {
    pub kind: StepKind,
    /// 程序名（PATH 上找得到的那一个：`npm` / `winget` / `scoop` / `pipx` / `cargo`）。
    ///
    /// ⚠️ **不是 `PathBuf`**（设计稿那么写）：我们手里从来没有这些程序的完整路径
    /// ——执行走 `cmd /C`，PATH 解析归 Windows。写 `PathBuf` 会让人以为这里存的
    /// 是一个文件路径，进而去 `is_file()` 它（那是 `None`，于是整条命令被跳过）。
    /// CleanupDir 这一步没有程序，留空串。
    pub program: String,
    pub args: Vec<String>,
    /// `CleanupDir` 时有值：`(目录, 类别)`。
    pub cleanup_dir: Option<(PathBuf, DirKind)>,
}

impl Step {
    /// 一条卸载命令。
    pub fn uninstall(program: &str, args: &[&str]) -> Self {
        Step {
            kind: StepKind::Uninstall,
            program: program.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            cleanup_dir: None,
        }
    }

    /// 一条清理步骤。
    pub fn cleanup(path: &str, kind: DirKind) -> Self {
        Step {
            kind: StepKind::CleanupDir,
            program: String::new(),
            args: Vec::new(),
            cleanup_dir: Some((PathBuf::from(path), kind)),
        }
    }

    /// 对话框里**等宽预览**的那一行（不可编辑）。
    ///
    /// 卸载步是原样的 `program args`；清理步说清"删内容、留目录"——
    /// 用户要能在点确认之前知道每一粒字节会动到什么。
    pub fn preview(&self) -> String {
        match self.kind {
            StepKind::Uninstall => {
                let mut s = self.program.clone();
                for a in &self.args {
                    s.push(' ');
                    s.push_str(a);
                }
                s
            }
            StepKind::CleanupDir => match &self.cleanup_dir {
                Some((dir, _)) => format!("删除 {} 的内容（保留目录本身）", dir.display()),
                // 结构上到不了这里（`Step::cleanup` 必带目录），但不 `expect`——
                // 预览是纯展示，不该有 panic 的路
                None => "（清理步骤缺目录）".to_string(),
            },
        }
    }
}

/// 一次卸载操作的计划。
#[derive(Clone, Debug)]
pub struct OpPlan {
    /// 按序执行的步骤：卸载命令在前，勾选的清理在后（设计 §11 串行）。
    pub steps: Vec<Step>,
    /// 各步取最高。
    pub risk: RiskLevel,
    /// 一句话说明这个计划要干什么（对话框标题下面那句）。
    pub summary: String,
    /// 「同时移除」清单（设计 §10.1 四段信息层级的第一段）：
    /// 桌面关联那两档说明会连带什么；没证据就说没证据。
    pub affected: Vec<String>,
    /// 勾选清理项合计字节。**有任一项还没量过 = `None`**
    /// （界面显「计算中…」，但允许勾选——勾选是意向，执行前必有数字）。
    pub reclaimable: Option<u64>,
}

impl OpPlan {
    /// 卸不了的一个计划（provider 给不出卸载命令）。`steps` 是空的 ⇒
    /// **调用方不该开确认框**——一颗确认了却什么都没做的键比没有更糟。
    pub fn blocked(pkg: &CliPackage, why: &str) -> Self {
        OpPlan {
            steps: Vec::new(),
            risk: RiskLevel::High,
            summary: format!("{}：{why}", pkg.name),
            affected: Vec::new(),
            reclaimable: None,
        }
    }
}

/// 一次卸载的风险由**桌面关联**定（设计 §13「卸载连带影响」那一行）。
///
/// `Unknown` 按高的一档报：没证据不是"没关系"，是"不知道会带走什么"——
/// 在这一屏上偏向谨慎的那一边（模型注释：「没证据」是一种结论）。
pub fn uninstall_risk(kind: PackageKind) -> RiskLevel {
    match kind {
        PackageKind::CliOnly => RiskLevel::Moderate,
        PackageKind::CliWithDesktop | PackageKind::DesktopWithCli => RiskLevel::High,
        PackageKind::Unknown => RiskLevel::High,
    }
}

/// 「同时移除」清单那几行字（设计 §10.1 第一段）。
///
/// 抽成函数而不是各 provider 各写一遍：这四句话**说错方向不是显示问题**
/// （`CliWithDesktop` 说成"只卸命令行"会让用户以为不碰桌面），而它们全部
/// 从同一张 `PackageKind` 表分出来。
pub fn affected_notes(kind: PackageKind) -> Vec<String> {
    match kind {
        PackageKind::CliOnly => vec!["仅命令行工具，不牵连桌面程序".to_string()],
        PackageKind::CliWithDesktop => {
            vec!["这个包还带一个桌面程序，卸载会一并移除它".to_string()]
        }
        PackageKind::DesktopWithCli => {
            vec!["这是一个桌面应用，它附带的那条命令也会一起移除".to_string()]
        }
        PackageKind::Unknown => {
            vec!["桌面关联未确定——这个卸载可能带走桌面程序，确认前先想一下".to_string()]
        }
    }
}

/// 组一个卸载计划：卸载命令 + 勾选的清理目录（设计 §8、§10.1）。
///
/// `cleanup` 是用户在对话框里勾上的那几条（只有 `Cache` 能被勾上，
/// `Config` / `Data` 灰显禁勾——见 [`DirKind::auto_deletable`]）。
pub fn assemble(pkg: &CliPackage, uninstall: Step, cleanup: &[PackageDir]) -> OpPlan {
    let mut steps = vec![uninstall];
    // 合计字节：**有一项没量过就整栏是 `None`**。拿"量过的那些"加出一个数来
    // 等于把"我还不知道"说成一个结论（Q39 / 32-11 那一族）
    let mut reclaimable = Some(0u64);
    for d in cleanup {
        steps.push(Step::cleanup(&d.path, d.kind));
        reclaimable = match (reclaimable, d.size) {
            (Some(acc), Some(s)) => Some(acc + s),
            _ => None,
        };
    }
    let risk = uninstall_risk(pkg.kind);
    let mut summary = format!("卸载 {}（{}）", pkg.name, pkg.provider.label());
    if !cleanup.is_empty() {
        summary.push_str(&format!("，并清理 {} 个数据目录", cleanup.len()));
    }
    OpPlan {
        steps,
        risk,
        summary,
        affected: affected_notes(pkg.kind),
        reclaimable,
    }
}

/// 组一个**纯清理**计划（共享缓存那一批用，设计 §9）。
///
/// 与 [`assemble`] 的分工：那个是"卸一个包 + 顺带清它自己的目录"，风险跟着
/// 桌面关联走；这个不卸载任何东西，所以**风险恒 `Safe`**——删缓存可再生，
/// 这是目录四档（[`DirKind::auto_deletable`]）早就定过性的事，这里不再判第二次。
/// `summary` / `affected` 也全是自己的话，不借 `assemble` 那套"同时移除"：
/// 这里移除的只是缓存，说"同时移除桌面程序"是谎报。
///
/// 合计字节的口径与 `assemble` **逐字相同**（有一项没量过 = `None`）——
/// 抽不出来的原因是两句 `summary` 不同，但判据必须一致：两处各写一个
/// "把不知道加成知道"的 bug，正是本项目学费最重的那一族。
pub fn clean_plan(targets: &[PackageDir]) -> OpPlan {
    let mut steps = Vec::with_capacity(targets.len());
    let mut reclaimable = Some(0u64);
    for d in targets {
        steps.push(Step::cleanup(&d.path, d.kind));
        reclaimable = match (reclaimable, d.size) {
            (Some(acc), Some(s)) => Some(acc + s),
            _ => None,
        };
    }
    let summary = if targets.len() == 1 {
        format!("清理共享缓存：{}", targets[0].path)
    } else {
        format!("清理 {} 个共享缓存目录", targets.len())
    };
    OpPlan {
        steps,
        risk: RiskLevel::Safe,
        summary,
        // 只删缓存：说一句"可再生"就够，不编连带影响
        affected: vec!["缓存可再生，删除不影响已安装工具的使用".to_string()],
        reclaimable,
    }
}

/// 一颗包可更新到哪儿（`PackageProvider::outdated` 的返回单元）。
///
/// `id` 是 [`CliPackage::id`] 那种稳定键（`npm:pnpm`）——合并按它对，
/// 不按名字（名字在 scoped 包上会重复）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInfo {
    pub id: String,
    pub latest: String,
}

/// 依赖倒查（待建 #3，设计 §3.11.6 审计页第五段）：卸载 `pkg` 之后，
/// 清单里**跟着失效**的那些包。
///
/// 判据只有一条，而且只用扫描已经拿到的数据：`other` 的安装目录**严格
/// 位于 `pkg` 的安装目录之下**（`winget:Node.js` = `C:\Program Files\nodejs`，
/// npm 全局包 = `C:\Program Files\nodejs\node_modules\…` ⇒ 卸 Node.js
/// 它们全废）。路径比较先把两边统一成小写 + `\` 分隔再比前缀——Windows
/// 大小写不敏感、资源管理器两种斜杠都给，界面不能因为写法不同就漏报。
///
/// **查不出来就不警告**（本项目"不猜"纪律在这条上的形状）：
///
/// - 有一边 `install_path` 是 `None` ⇒ 不判；
/// - 管理的包不在管理器自己的目录之下（设计稿例子里 bun 管的包在
///   `~/.bun`，而 bun 本体装在 npm 全局目录下）⇒ 查不出，**不编一条
///   警告出来**。这条范围边界登记在 roadmap 偏差表。
///
/// 兄弟目录不算（卸 `nodejs\node_modules\typescript` 废不掉同级的
/// `@wenyan-md/cli`），自身排除，返回按清单原序。
pub fn dependents_of(pkg: &CliPackage, all: &[CliPackage]) -> Vec<CliPackage> {
    let Some(root) = pkg.install_path.as_deref().map(normalize_path) else {
        return Vec::new();
    };
    all.iter()
        .filter(|other| {
            other.id != pkg.id
                && other
                    .install_path
                    .as_deref()
                    .map(|p| under(&normalize_path(p), &root))
                    .unwrap_or(false)
        })
        .cloned()
        .collect()
}

/// 路径比较前的归一：全小写 + `/` 换 `\` + 去掉结尾分隔符。
/// 只用于前缀比对，**不碰文件系统**——拿归一结果去读盘是另一回事。
fn normalize_path(p: &str) -> String {
    let mut s = p.to_ascii_lowercase().replace('/', "\\");
    while s.ends_with('\\') {
        s.pop();
    }
    s
}

/// `child` 是否严格位于 `root` 之下（两边都已 [`normalize_path`]）：
/// 前缀相等且再多一个分隔符——`c:\a` 不是 `c:\ab` 的前缀包含。
fn under(child: &str, root: &str) -> bool {
    child.starts_with(root) && child[root.len()..].starts_with('\\')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::model::ProviderId;

    fn pkg(kind: PackageKind) -> CliPackage {
        CliPackage {
        program_size: None,
            id: "npm:pnpm".into(),
            provider: ProviderId::Npm,
            name: "pnpm".into(),
            version: Some("11.5.3".into()),
            latest: None,
            install_path: None,
            executables: vec!["pnpm".into()],
            kind,
            dirs: Vec::new(),
        }
    }

    fn cache_dir(path: &str, size: Option<u64>) -> PackageDir {
        PackageDir {
            path: path.into(),
            kind: DirKind::Cache,
            label: String::new(),
            size,
        }
    }

    /// 风险真值表：四档桌面关联 → 三档风险。
    /// 这条钉的是"方向"：`CliWithDesktop` 说成低风险会让用户放心卸掉桌面程序。
    #[test]
    fn the_risk_truth_table() {
        assert_eq!(uninstall_risk(PackageKind::CliOnly), RiskLevel::Moderate);
        assert_eq!(uninstall_risk(PackageKind::CliWithDesktop), RiskLevel::High);
        assert_eq!(uninstall_risk(PackageKind::DesktopWithCli), RiskLevel::High);
        assert_eq!(
            uninstall_risk(PackageKind::Unknown),
            RiskLevel::High,
            "没证据 ⇒ 按高的报（不知道会带走什么）"
        );
    }

    /// 「同时移除」四句话必须**各不相同**：共用一句会让"会带走桌面程序"
    /// 出现在"纯命令行"那一行上（同 `the_package_kind_truth_table` 那条判据）
    #[test]
    fn affected_notes_are_four_distinct_sentences() {
        let all = [
            PackageKind::Unknown,
            PackageKind::CliOnly,
            PackageKind::CliWithDesktop,
            PackageKind::DesktopWithCli,
        ];
        let notes: std::collections::HashSet<String> = all
            .iter()
            .flat_map(|k| affected_notes(*k))
            .collect();
        assert_eq!(notes.len(), 4, "四档各一句，不许重复：{notes:?}");
        // "未确定"那一档必须说出"没确定"，不能说得像结论
        assert!(affected_notes(PackageKind::Unknown)[0].contains("未确定"));
    }

    /// 依赖倒查真值表（待建 #3）：卸载宿主 → 目录在它之下的包跟着失效。
    /// 这条钉三个方向：**父子命中、兄弟不连坐、查不出不猜**——
    /// 卸 `nodejs\node_modules\typescript` 若废掉了同级包，审计页会
    /// 吓得用户不敢卸一个普通 CLI；反过来 Node.js 那条真警告漏了，
    /// 用户卸完才发现 `rg` 没了。
    #[test]
    fn dependents_need_to_live_under_the_uninstalled_root() {
        let mut node = pkg(PackageKind::CliOnly);
        node.id = "winget:Node.js".into();
        node.install_path = Some("C:\\Program Files\\nodejs".into());
        let mut ts = pkg(PackageKind::CliOnly);
        ts.id = "npm:typescript".into();
        ts.install_path = Some("c:/program files/nodejs/node_modules/typescript".into());
        let mut wenyan = pkg(PackageKind::CliOnly);
        wenyan.id = "npm:@wenyan-md/cli".into();
        // 兄弟：同在 node_modules 下但不在 typescript 之下
        wenyan.install_path = Some("C:\\Program Files\\nodejs\\node_modules\\@wenyan-md\\cli".into());
        let mut outside = pkg(PackageKind::CliOnly);
        outside.id = "cargo:eza".into();
        outside.install_path = Some("C:\\Users\\u\\.cargo\\bin".into());

        // 卸 Node.js：npm 全局包全在 nodejs 之下 ⇒ 两条都命中（这正是
        // 设计稿那条"卸了宿主，N 个全局 CLI 跟着废"的真实形状）；
        // 大小写与正反斜杠写法不挡路（比较前统一过）、外部目录不连坐
        let deps = dependents_of(&node, &[ts.clone(), wenyan.clone(), outside.clone()]);
        assert_eq!(
            deps.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["npm:typescript", "npm:@wenyan-md/cli"],
        );

        // 卸一个普通 CLI：同级包不连坐（反向吓人是这条规则最贵的错法）
        assert!(dependents_of(&ts, &[wenyan.clone(), node.clone()]).is_empty());

        // 有一边拿不到安装目录 ⇒ 不判（宁可不警告，不猜）
        let mut ghost = pkg(PackageKind::CliOnly);
        ghost.id = "npm:ghost".into();
        ghost.install_path = None;
        assert!(dependents_of(&ghost, &[ts.clone()]).is_empty());
        assert!(dependents_of(&node, &[ghost]).is_empty());
    }

    /// 前缀包含的边界：`c:\a` 不是 `c:\ab` 的前缀包含（差一个分隔符）。
    /// 这条不钉住的话，装在 `C:\tools` 旁边的 `C:\tools2` 会被算进 `C:\tools` 名下。
    #[test]
    fn prefix_requires_a_separator_boundary() {
        assert!(!under(r"c:\ab", r"c:\a"));
        assert!(under(r"c:\a\ts", r"c:\a"));
        assert!(under(r"c:\a\", r"c:\a"), "结尾分隔符归一后剥掉，仍是包含");
    }

    /// 计划装配：卸载步在前、清理步按序随后；`program + args` 是结构化的。
    #[test]
    fn assemble_orders_uninstall_then_cleanups() {
        let plan = assemble(
            &pkg(PackageKind::CliOnly),
            Step::uninstall("npm", &["uninstall", "-g", "pnpm"]),
            &[cache_dir("C:\\c1", Some(100)), cache_dir("C:\\c2", Some(50))],
        );
        assert_eq!(plan.steps.len(), 3, "一条卸载 + 两条清理");
        assert_eq!(plan.steps[0].kind, StepKind::Uninstall);
        assert_eq!(plan.steps[0].program, "npm");
        assert_eq!(plan.steps[0].args, vec!["uninstall", "-g", "pnpm"]);
        assert_eq!(plan.steps[1].kind, StepKind::CleanupDir);
        assert_eq!(plan.steps[2].kind, StepKind::CleanupDir);
        assert_eq!(plan.risk, RiskLevel::Moderate);
        assert_eq!(plan.reclaimable, Some(150));
        assert!(plan.summary.contains("pnpm"));
        assert!(plan.summary.contains("2 个数据目录"));
    }

    /// 合计字节：**有一项没量过，整栏就是 `None`**。
    /// 拿量过的加出一个数 = 把"我还不知道"说成结论（这条是本函数最要紧的一条）
    #[test]
    fn reclaimable_is_none_when_any_size_is_unknown() {
        let plan = assemble(
            &pkg(PackageKind::CliOnly),
            Step::uninstall("npm", &["uninstall", "-g", "pnpm"]),
            &[cache_dir("C:\\c1", Some(100)), cache_dir("C:\\c2", None)],
        );
        assert_eq!(plan.reclaimable, None, "有一条没量过就不给合计");
        // 一个都没勾 = 0，不是 None（"勾了零个"与"还不知道"是两句话）
        let none = assemble(
            &pkg(PackageKind::CliOnly),
            Step::uninstall("npm", &["uninstall", "-g", "pnpm"]),
            &[],
        );
        assert_eq!(none.reclaimable, Some(0));
        assert_eq!(none.steps.len(), 1, "没有清理可勾就只跑卸载");
    }

    /// 预览行：卸载是原样命令；清理说清"留目录"。
    #[test]
    fn preview_lines_show_what_will_happen() {
        let u = Step::uninstall("winget", &["uninstall", "--id", "Git.Git", "--silent"]);
        assert_eq!(u.preview(), "winget uninstall --id Git.Git --silent");
        let c = Step::cleanup("C:\\Users\\x\\.deno", DirKind::Cache);
        assert!(c.preview().contains("保留目录本身"), "清理步必须说清留目录");
        assert!(c.preview().contains(".deno"));
    }

    /// 卸不了的计划：`steps` 空 ⇒ 调用方据此不开确认框。
    #[test]
    fn a_blocked_plan_has_no_steps() {
        let plan = OpPlan::blocked(&pkg(PackageKind::Unknown), "认不出这个 id");
        assert!(plan.steps.is_empty());
        assert_eq!(plan.risk, RiskLevel::High, "给不出命令时按高的报");
        assert!(plan.summary.contains("认不出这个 id"));
    }

    /// 纯清理计划：**风险恒 Safe**（删缓存可再生，目录四档定过性的事不判第二次），
    /// 步数 = 目标数，且全是清理步
    #[test]
    fn clean_plan_is_always_safe() {
        let plan = clean_plan(&[
            cache_dir("C:\\npm-cache", Some(1024)),
            cache_dir("C:\\scoop-cache", None),
        ]);
        assert_eq!(plan.risk, RiskLevel::Safe, "不卸载任何东西，没有升级风险可言");
        assert_eq!(plan.steps.len(), 2);
        assert!(plan.steps.iter().all(|s| s.kind == StepKind::CleanupDir));
        assert_eq!(plan.reclaimable, None, "有一条没量过就不给合计（与 assemble 同口径）");
    }

    /// 合计字节与 `assemble` **逐字同口径**：全量过才给数，一个都没勾给 0 不给 None
    #[test]
    fn clean_plan_reclaimable_matches_assemble() {
        let measured = clean_plan(&[
            cache_dir("C:\\a", Some(100)),
            cache_dir("C:\\b", Some(250)),
        ]);
        assert_eq!(measured.reclaimable, Some(350));
        let empty = clean_plan(&[]);
        assert_eq!(empty.reclaimable, Some(0));
        assert!(empty.steps.is_empty());
        assert!(empty.summary.contains("0"));
    }

    /// 摘要按条数变话术：一条说路径，多条说条数（对话框标题下那句话）
    #[test]
    fn clean_plan_summary_varies_with_count() {
        let one = clean_plan(&[cache_dir("C:\\npm-cache", Some(1))]);
        assert!(one.summary.contains("npm-cache"));
        let many = clean_plan(&[cache_dir("C:\\a", Some(1)), cache_dir("C:\\b", Some(1))]);
        assert!(many.summary.contains("2"));
    }
}
