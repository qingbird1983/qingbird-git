//! 数据目录发现（设计 §8：规则库 → 包管理器文件清单 → 环境变量推断）。
//!
//! ## 这个模块负责哪一半
//!
//! 规则库那份**数据**在 `rules.toml`（编进 exe，设计 §12「TOML 随版本走即可，
//! 不建更新通道」）；这里负责**解析**：把规则 + 环境 + "哪个目录真的在盘上"
//! 变成一串 [`PackageDir`]。
//!
//! ## 为什么解析要收成一堆入参
//!
//! [`Ctx`] 里没有一样东西是就地读的（环境变量、三个根目录、"这个路径在不在"
//! 全是入参）⇒ 单测能在一个**假世界**里跑完整条链路：给它一个环境、告诉它
//! 哪几个目录"存在"，断言解析出哪几条。**判据要的是这条规则对不对，
//! 不是"我这台机器上恰好有这几个目录"**——后者是把测试的前提外包给环境
//! （AGENTS 那一族，本项目栽过好几次）。
//!
//! ## 体积为什么是 `None` 起步
//!
//! 走盘是很贵的：本机 npm 缓存 **97391 个文件**、cargo registry **124073 个**，
//! 量一次要几秒。设计 §4 早就把它写成 `Option<u64>`（`None` = 界面显「计算中…」）
//! ⇒ 解析只出路径，体积由界面那条后台通道一趟一趟地填（`ops::scan_cli`），
//! 填完 notify。**进屏那一刻不等任何一次走盘**。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::cli::model::{CliPackage, DirKind, PackageDir};

/// 内置规则库。**它是数据不是代码**：加一条工具路径不该改逻辑
const RULES: &str = include_str!("rules.toml");

#[derive(Deserialize, Default)]
struct Rules {
    #[serde(default)]
    shared: Vec<SharedRule>,
    #[serde(default)]
    rule: Vec<ToolRule>,
}

/// 一份共享缓存（跨包共用，不属于任何一颗包）
#[derive(Deserialize)]
struct SharedRule {
    /// 界面上给它的名字（`npm 缓存` / `pnpm store`）。必填：汇总条上要按它认人
    label: String,
    /// 候选路径，**按序取第一个存在的**
    paths: Vec<String>,
}

/// 一条每颗工具的规则
#[derive(Deserialize)]
struct ToolRule {
    /// 匹配包名末段或它的任一命令名（**全等**，见 `rules.toml` 头）
    package: Vec<String>,
    /// 可选：限定包管理器（`npm` / `scoop`…）
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    cache: Vec<String>,
    #[serde(default)]
    config: Vec<String>,
    #[serde(default)]
    data: Vec<String>,
}

impl ToolRule {
    fn matches(&self, pkg: &CliPackage) -> bool {
        if let Some(want) = &self.provider {
            if !pkg.provider.key().eq_ignore_ascii_case(want) {
                return false;
            }
        }
        let last = pkg
            .name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&pkg.name)
            .to_ascii_lowercase();
        self.package.iter().any(|m| {
            let m = m.to_ascii_lowercase();
            m == last || pkg.executables.iter().any(|c| c.to_ascii_lowercase() == m)
        })
    }
}

/// 内置那份规则库。**解不出来就退成空表**（少报一行 vs 让整屏 panic——后者更坏），
/// 而"内置那份一定解得出来、且不是空表"由单测钉着：那样改坏 TOML 是**红一条测试**，
/// 不是"界面上悄悄少了几行"
fn rules() -> &'static Rules {
    static ONCE: std::sync::OnceLock<Rules> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| toml::from_str(RULES).unwrap_or_default())
}

/// 解析要的那点"外面的世界"——**全是入参**（见模块头）
pub struct Ctx<'a> {
    /// 环境变量（键统一按大写存：Windows 的进程环境大小写不敏感，
    /// 而 `Path` 那边各写各的，别指望拼写一致）
    vars: HashMap<String, String>,
    appdata: Option<String>,
    local_appdata: Option<String>,
    userprofile: Option<String>,
    /// "这个路径在不在"。**盒装而不是借用**：单测要在**构造它那一步**就把假世界
    /// 包进去（借用的话每个测试都得自己留一个活够久的闭包，判据会被生命周期噪音淹掉）
    exists: Box<dyn Fn(&Path) -> bool + 'a>,
}

impl<'a> Ctx<'a> {
    /// 真跑的时候用这个：读进程环境、真去问盘
    pub fn live() -> Ctx<'static> {
        let mut vars = HashMap::new();
        for (k, v) in std::env::vars() {
            vars.insert(k.to_ascii_uppercase(), v);
        }
        let get = |name: &str| vars.get(name).cloned();
        Ctx {
            appdata: get("APPDATA"),
            local_appdata: get("LOCALAPPDATA"),
            userprofile: get("USERPROFILE"),
            vars,
            exists: Box::new(|p: &Path| p.is_dir()),
        }
    }

    fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .get(&name.to_ascii_uppercase())
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
    }

    /// 展开一条候选路径里的占位符。`None` = 这一条用不了（变量没设 / 根拿不到）
    /// ⇒ 调用方去试下一条候选。
    ///
    /// 支持的写法：`$VAR`（只在**整段路径的最前面**，免得把路径中间别的 `$` 也当变量）、
    /// `~/`、`%APPDATA%` / `%LOCALAPPDATA%` / `%USERPROFILE%`。
    /// 出来一律是**反斜杠**：这一份是给人看的，而同一屏里别的路径（npm 给的
    /// `install_path`）就是反斜杠——两种写法并排着看，迟早有人当成两个目录
    fn expand(&self, raw: &str) -> Option<PathBuf> {
        let s = raw.replace('\\', "/");
        // `$VAR` 那一段先换成它的值（值本身也可能是 `/` 分隔的）
        let s = match s.strip_prefix('$') {
            Some(rest) => {
                let (name, tail) = rest.split_once('/').unwrap_or((rest, ""));
                let val = self.var(name)?.replace('\\', "/");
                if tail.is_empty() {
                    val
                } else {
                    format!("{val}/{tail}")
                }
            }
            None => s,
        };
        // 再认三个根与 `~`
        let (root, tail) = match s.strip_prefix('%') {
            Some(rest) => {
                let (name, tail) = rest.split_once('%')?;
                let root = match name.to_ascii_uppercase().as_str() {
                    "APPDATA" => self.appdata.clone(),
                    "LOCALAPPDATA" => self.local_appdata.clone(),
                    "USERPROFILE" => self.userprofile.clone(),
                    _ => None,
                }?;
                (root, tail.trim_start_matches('/').to_string())
            }
            None => match s.strip_prefix("~") {
                Some(t) => (self.userprofile.clone()?, t.trim_start_matches('/').to_string()),
                None => {
                    // 既不是变量也不是根写法：本身就应当是绝对路径
                    return Some(PathBuf::from(s.replace('/', "\\")));
                }
            },
        };
        let sep = if tail.is_empty() { "" } else { "\\" };
        Some(PathBuf::from(format!(
            "{}{sep}{}",
            root.trim_end_matches(['/', '\\']),
            tail.replace('/', "\\")
        )))
    }
}

/// 一颗工具的目录。**只认规则库里写了背书的那几条**；一条都没写就是空表。
pub fn for_package(pkg: &CliPackage, ctx: &Ctx) -> Vec<PackageDir> {
    let mut out = Vec::new();
    for r in rules().rule.iter().filter(|r| r.matches(pkg)) {
        for (kind, paths) in [
            (DirKind::Cache, &r.cache),
            (DirKind::Config, &r.config),
            (DirKind::Data, &r.data),
        ] {
            if let Some(d) = first_existing(paths, kind, ctx) {
                out.push(d);
            }
        }
    }
    out
}

/// 机器上的共享缓存。**与 provider 装没装无关**——存在就是占用（一份没卸载干净的
/// pip 缓存也真占着盘，藏起来只是让用户找不到它）
pub fn shared(ctx: &Ctx) -> Vec<PackageDir> {
    rules()
        .shared
        .iter()
        .filter_map(|r| {
            first_existing(&r.paths, DirKind::SharedCache, ctx).map(|mut d| {
                d.label = r.label.clone();
                d
            })
        })
        .collect()
}

/// 把 provider 自报的那一份与规则库那一份合起来。
///
/// **按路径去重**：同一个目录两边都报是**正常**的（provider 说"我的缓存在这儿"、
/// 规则库说"默认就在这儿"），报两行才是错的。合并时**留着有名字的那一条**——
/// 名字只有规则库那侧有（`npm 缓存` / `pnpm store`），丢了就只剩一根路径
pub fn merge_shared(from_provider: Vec<PackageDir>, from_rules: Vec<PackageDir>) -> Vec<PackageDir> {
    let mut out: Vec<PackageDir> = Vec::new();
    for d in from_provider.into_iter().chain(from_rules) {
        match out.iter_mut().find(|e| e.path.eq_ignore_ascii_case(&d.path)) {
            Some(e) => {
                if e.label.is_empty() {
                    e.label = d.label;
                }
            }
            None => out.push(d),
        }
    }
    out
}

/// 一条规则的候选里**第一个存在的**。都不存在 = "这台机器上没有它"（不是错）
fn first_existing(paths: &[String], kind: DirKind, ctx: &Ctx) -> Option<PackageDir> {
    paths.iter().find_map(|raw| {
        let p = ctx.expand(raw)?;
        if (ctx.exists)(&p) {
            Some(PackageDir {
                path: p.to_string_lossy().into_owned(),
                kind,
                label: String::new(),
                size: None,
            })
        } else {
            None
        }
    })
}

/// 量一个目录的体积。**复用空间页那颗走盘器**——再写一份就是同一件事两个出处
/// （本项目为这条交过四次学费）
pub fn size_of(p: &Path) -> u64 {
    crate::space::dir_size(p)
}

/// 把上一份算好的体积按路径接回来（免得每次重扫都退回「计算中…」，那会闪一下）。
/// 纯函数：路径（Windows 上大小写不敏感）对上才接，对不上就留 `None` 重算。
pub fn carry_sizes(fresh: &mut [CliPackage], old: &[CliPackage]) {
    for p in fresh.iter_mut() {
        let Some(oldp) = old.iter().find(|o| o.id == p.id) else {
            continue;
        };
        carry_dir_sizes(&mut p.dirs, &oldp.dirs);
    }
}

/// [`carry_sizes`] 对一份目录表的那一半（共享缓存那份也走它）
pub fn carry_dir_sizes(fresh: &mut [PackageDir], old: &[PackageDir]) {
    for d in fresh.iter_mut() {
        if let Some(o) = old
            .iter()
            .find(|o| o.path.eq_ignore_ascii_case(&d.path) && o.kind == d.kind)
        {
            d.size = o.size;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::model::{PackageKind, ProviderId};

    /// 一个**假世界**：给环境、给"哪些目录存在"，其余什么都不碰
    fn world<'a>(vars: &[(&str, &str)], roots: &[(&str, &str)], existing: &'a [&'a str]) -> Ctx<'a> {
        let mut map = HashMap::new();
        for (k, v) in vars {
            map.insert(k.to_ascii_uppercase(), (*v).to_string());
        }
        let get = |name: &str| {
            roots
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_string())
        };
        Ctx {
            appdata: get("APPDATA"),
            local_appdata: get("LOCALAPPDATA"),
            userprofile: get("USERPROFILE"),
            vars: map,
            exists: Box::new(move |p: &Path| {
                let s = p.to_string_lossy().replace('/', "\\");
                existing.iter().any(|e| e.eq_ignore_ascii_case(&s))
            }),
        }
    }

    fn pkg(name: &str, cmds: &[&str], provider: ProviderId) -> CliPackage {
        CliPackage {
            id: format!("{}:{name}", provider.key()),
            provider,
            name: name.into(),
            version: None,
            latest: None,
            install_path: None,
            executables: cmds.iter().map(|s| s.to_string()).collect(),
            kind: PackageKind::Unknown,
            dirs: vec![],
        }
    }

    /// 内置那份规则库**必须解得出来、且不是空表**。
    /// 没有这一条的话，改坏 TOML 的表现是"界面上悄悄少了几行"——
    /// 而 `rules()` 那处降级（解不出就空表）正好会把这件事盖住
    #[test]
    fn the_builtin_rule_book_parses_and_is_not_empty() {
        let r = rules();
        assert!(!r.shared.is_empty(), "共享缓存一条规则都没有？");
        assert!(!r.rule.is_empty(), "每颗工具的规则一条都没有？");
        for s in &r.shared {
            assert!(!s.label.trim().is_empty(), "共享缓存必须有名字（汇总条按它认人）");
            assert!(!s.paths.is_empty(), "{}：候选路径是空的", s.label);
        }
        for t in &r.rule {
            assert!(!t.package.is_empty(), "工具规则必须有 package 匹配键");
            assert!(
                !(t.cache.is_empty() && t.config.is_empty() && t.data.is_empty()),
                "{:?}：一条路径都没写",
                t.package
            );
        }
    }

    /// 本机实测的**两个反例**：名字对上了、东西不是它的。
    /// 这一条是整份规则库的地基判据——它红了就说明有人开始按名字猜了
    #[test]
    fn the_two_measured_false_positives_are_not_rules() {
        // 假世界：这两个目录都存在（本机真的存在），另外把"什么都存在"也试一遍
        let exists = [
            r"C:\Users\x\AppData\Local\feishu",
            r"C:\Users\x\.feishu-cli",
            r"C:\Users\x\.kimi-code",
            r"C:\Users\x\AppData\Local\kimi-code",
        ];
        let ctx = world(
            &[],
            &[
                ("APPDATA", r"C:\Users\x\AppData\Roaming"),
                ("LOCALAPPDATA", r"C:\Users\x\AppData\Local"),
                ("USERPROFILE", r"C:\Users\x"),
            ],
            &exists,
        );

        let feishu = pkg("feishu-cli", &["feishu"], ProviderId::Npm);
        let got = for_package(&feishu, &ctx);
        assert_eq!(got.len(), 1, "feishu-cli 只该有一条（它源码里写死的那条）");
        assert_eq!(got[0].path, r"C:\Users\x\.feishu-cli");
        assert_eq!(got[0].kind, DirKind::Config);
        assert!(
            !got.iter().any(|d| d.path.ends_with(r"\Local\feishu")),
            "⛔ %LOCALAPPDATA%\\feishu 是**飞书桌面版**的 1.7 GB，不是 feishu-cli 的"
        );

        let kimi = pkg("@moonshot-ai/kimi-code", &["kimi"], ProviderId::Npm);
        assert!(
            for_package(&kimi, &ctx).is_empty(),
            "kimi 那两条（~/.kimi-code 127.8 MB、%LOCALAPPDATA%\\kimi-code）没有背书 ⇒ 一条都不许报"
        );
    }

    /// 候选**按序取第一个存在的**：环境变量指到别处去了，工具真的在用那一个
    #[test]
    fn candidates_take_the_first_that_exists() {
        let only_default = [r"C:\Users\x\AppData\Local\deno"];
        let ctx = world(
            &[],
            &[
                ("APPDATA", r"C:\Users\x\AppData\Roaming"),
                ("LOCALAPPDATA", r"C:\Users\x\AppData\Local"),
                ("USERPROFILE", r"C:\Users\x"),
            ],
            &only_default,
        );
        let deno = pkg("deno", &["deno"], ProviderId::Npm);
        let got = for_package(&deno, &ctx);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].path, r"C:\Users\x\AppData\Local\deno");
        assert_eq!(got[0].kind, DirKind::Cache);

        // 环境变量设了**且那个目录存在** ⇒ 它赢（用户指到别处去了）
        let both = [
            r"D:\deno-home",
            r"C:\Users\x\AppData\Local\deno",
        ];
        let ctx = world(
            &[("DENO_DIR", r"D:\deno-home")],
            &[("APPDATA", r"C:\Users\x\AppData\Roaming"), ("LOCALAPPDATA", r"C:\Users\x\AppData\Local"), ("USERPROFILE", r"C:\Users\x")],
            &both,
        );
        let got = for_package(&deno, &ctx);
        assert_eq!(got[0].path, r"D:\deno-home", "环境变量在前，存在就它赢");
        assert_eq!(got.len(), 1, "只报一条，不是候选两条都报");

        // 一个都不存在 ⇒ 空表（不是报一条 0 字节的幽灵）
        let ctx = world(&[], &[("USERPROFILE", r"C:\Users\x")], &[]);
        assert!(for_package(&deno, &ctx).is_empty());
    }

    /// 包名末段与命令名都能匹配；`provider` 限定只在该写的时候起作用
    #[test]
    fn matching_is_exact_and_uses_the_last_name_segment_or_the_commands() {
        let exists = [r"C:\Users\x\.feishu-cli"];
        let ctx = world(
            &[],
            &[("USERPROFILE", r"C:\Users\x"), ("APPDATA", r"C:\Users\x\AppData\Roaming")],
            &exists,
        );
        // 名字末段命中
        assert_eq!(for_package(&pkg("feishu-cli", &[], ProviderId::Npm), &ctx).len(), 1);
        // 命令名命中（`@larksuite/cli` 这种情况：包名末段是 `cli`，认不出来）
        assert_eq!(for_package(&pkg("@x/y", &["feishu-cli"], ProviderId::Npm), &ctx).len(), 1);
        // **全等**：`feishu-cli-extra` 不许命中（前缀/模糊匹配就是把反例请回来）
        assert!(for_package(&pkg("feishu-cli-extra", &[], ProviderId::Npm), &ctx).is_empty());
        // 命令名里带 `.cmd` 的写法也不许命中（那个后缀不是命令名的一部分）
        assert!(for_package(&pkg("other", &["feishu-cli.cmd"], ProviderId::Npm), &ctx).is_empty());
    }

    /// 占位符展开：`$VAR` / `~/` / `%APPDATA%`，以及"变量没设 ⇒ 这一条用不了"
    #[test]
    fn placeholders_expand_or_are_skipped() {
        let ctx = world(
            &[("CARGO_HOME", r"D:\cargo")],
            &[
                ("APPDATA", r"C:\Users\x\AppData\Roaming"),
                ("LOCALAPPDATA", r"C:\Users\x\AppData\Local"),
                ("USERPROFILE", r"C:\Users\x"),
            ],
            &[],
        );
        assert_eq!(
            ctx.expand("$CARGO_HOME/registry").unwrap().to_string_lossy(),
            r"D:\cargo\registry"
        );
        assert_eq!(
            ctx.expand("%LOCALAPPDATA%/npm-cache").unwrap().to_string_lossy(),
            r"C:\Users\x\AppData\Local\npm-cache"
        );
        assert_eq!(ctx.expand("~/.npmrc").unwrap().to_string_lossy(), r"C:\Users\x\.npmrc");
        // 没设的变量 ⇒ None（调用方跳过这一条，去试下一个候选）
        assert!(ctx.expand("$NOPE_DIR/x").is_none());
        // `~` 与 `%ROOT%` 也拿不到时同样是 None
        let bare = world(&[], &[], &[]);
        assert!(bare.expand("~/x").is_none());
        assert!(bare.expand("%APPDATA%/x").is_none());
        // 绝对路径原样（反斜杠归一，与同屏其他路径一个写法）
        assert_eq!(
            ctx.expand("C:/a/b").unwrap().to_string_lossy(),
            r"C:\a\b"
        );
    }

    /// 共享缓存：**存在就报**（与 provider 装没装无关），名字从规则里带出来
    #[test]
    fn shared_caches_are_reported_by_existence_and_carry_their_label() {
        let exists = [r"C:\Users\x\AppData\Local\npm-cache"];
        let ctx = world(
            &[],
            &[("LOCALAPPDATA", r"C:\Users\x\AppData\Local"), ("USERPROFILE", r"C:\Users\x")],
            &exists,
        );
        let got = shared(&ctx);
        assert_eq!(got.len(), 1, "假世界里只有 npm 缓存存在");
        assert_eq!(got[0].label, "npm 缓存");
        assert_eq!(got[0].kind, DirKind::SharedCache);
        assert_eq!(got[0].size, None, "体积起步是 None（惰性算）");
        assert!(
            got[0].path.to_lowercase().contains("npm-cache"),
            "报的是真路径：{}",
            got[0].path
        );
    }

    /// 同一个目录两边都报（provider 自报那条与规则库那条撞上）⇒ **只留一行**，
    /// 且要留着**有名字**的那一条——名字丢了，汇总条上就只剩一根光路径
    #[test]
    fn merging_dedupes_by_path_and_keeps_the_label() {
        let from_provider = vec![PackageDir {
            path: r"C:\Users\x\AppData\Local\npm-cache".into(),
            kind: DirKind::SharedCache,
            label: String::new(),
            size: None,
        }];
        let from_rules = vec![
            PackageDir {
                path: r"c:\users\x\AppData\Local\npm-cache".into(),
                kind: DirKind::SharedCache,
                label: "npm 缓存".into(),
                size: None,
            },
            PackageDir {
                path: r"C:\Users\x\.cargo\registry".into(),
                kind: DirKind::SharedCache,
                label: "cargo registry".into(),
                size: None,
            },
        ];
        let got = merge_shared(from_provider, from_rules);
        assert_eq!(got.len(), 2, "同一个目录（大小写不同）只留一行");
        assert_eq!(got[0].label, "npm 缓存", "要留着有名字的那一条");
        assert_eq!(got[1].label, "cargo registry");
    }

    /// 体积接力：**路径与类别都对上**才接；对不上就留 `None` 重算。
    /// 不接的后果是每次重扫都闪一下「计算中…」；接错的后果是显示别人目录的体积
    #[test]
    fn sizes_are_carried_over_only_for_the_same_path() {
        let mut fresh = vec![
            PackageDir { path: r"C:\a".into(), kind: DirKind::Cache, label: String::new(), size: None },
            PackageDir { path: r"C:\b".into(), kind: DirKind::Cache, label: String::new(), size: None },
            PackageDir { path: r"C:\c".into(), kind: DirKind::Config, label: String::new(), size: None },
        ];
        let old = vec![
            // 大小写不同也算同一个（Windows 上路径大小写不敏感）
            PackageDir { path: r"c:\A".into(), kind: DirKind::Cache, label: String::new(), size: Some(11) },
            PackageDir { path: r"C:\b".into(), kind: DirKind::Config, label: String::new(), size: Some(22) },
            PackageDir { path: r"C:\c".into(), kind: DirKind::Config, label: String::new(), size: Some(33) },
        ];
        carry_dir_sizes(&mut fresh, &old);
        assert_eq!(fresh[0].size, Some(11), "路径对上（大小写无关）要接");
        assert_eq!(fresh[1].size, None, "路径对上但**类别不同** ⇒ 不接");
        assert_eq!(fresh[2].size, Some(33));
    }

    /// 整棵包表那一层：按 `id` 找上一份，再接各自目录的体积
    #[test]
    fn carrying_sizes_goes_by_package_id() {
        let mut fresh = vec![pkg("deno", &["deno"], ProviderId::Npm)];
        fresh[0].dirs = vec![PackageDir {
            path: r"C:\Users\x\AppData\Local\deno".into(),
            kind: DirKind::Cache,
            label: String::new(),
            size: None,
        }];
        let mut old = vec![pkg("deno", &["deno"], ProviderId::Npm)];
        old[0].dirs = vec![PackageDir {
            path: r"C:\Users\x\AppData\Local\deno".into(),
            kind: DirKind::Cache,
            label: String::new(),
            size: Some(4096),
        }];
        carry_sizes(&mut fresh, &old);
        assert_eq!(fresh[0].dirs[0].size, Some(4096));

        // 别的 id 不接
        let mut fresh2 = vec![pkg("bun", &["bun"], ProviderId::Npm)];
        fresh2[0].dirs = fresh[0].dirs.clone();
        fresh2[0].dirs[0].size = None;
        carry_sizes(&mut fresh2, &old);
        assert_eq!(fresh2[0].dirs[0].size, None);
    }
}
