//! 从资源管理器把文件夹拖进窗口 → 加进仓库列表。
//!
//! ## 这条路为什么是"补回来"而不是"新增"
//!
//! 旧 Tauri 版有（`src/App.tsx:121`，走 `getCurrentWebview().onDragDropEvent`），
//! GPUI 重写时整条漏掉了——`gui/` 里一处拖拽代码都没有。用户装完才发现。
//! 底下三层其实**早就通**，只是没人挂监听：
//!
//! | 层 | 事实 |
//! |---|---|
//! | 平台 | `gpui-pre-windows/src/window.rs:1517` **每个窗口都调了 `RegisterDragDrop`**，`DragEnter/Over/Leave/Drop` 全实现（`:1150`） |
//! | 框架 | `FileDropEvent::Entered{paths}` → `cx.active_drag.value = Arc<ExternalPaths>`（`gpui-pre/src/window.rs:5478`）；`Submit` 被翻成一次 `MouseUpEvent`（`:5499`），所以元素级 `on_drop` 天然收得到 |
//! | 光标 | 平台侧对 `CF_HDROP` 直接给 `DROPEFFECT_COPY`（`gpui-pre-windows/…/window.rs:1169`），**不用自己挂 `can_drop`** |
//!
//! ## ⚠️ payload 类型是 `ExternalPaths` 不是 `PathBuf`
//!
//! 写 `on_drop::<PathBuf>` 一行都不报错，只是**永远不触发**。
//! `PathBuf` 是「本应用自己往外拖文件」时的 payload（`gpui-pre/src/window.rs:8203`
//! 那个测试用的是它，很容易看成是外部拖放）。
//! 外部拖进来的 payload 是 `ExternalPaths`（`interactive.rs:694`），取路径用 `.paths()`。
//!
//! ## 为什么挂根 div 就是"窗口任意位置"
//!
//! drop 监听在**冒泡阶段 + 命中即触发**，触发后 `cx.stop_propagation()`
//! （`gpui-pre/src/elements/div.rs:2897-2924`）。所以最深的那个挂着匹配监听的元素
//! 独吞这个事件。根 div 是 `size_full()`，命中区铺满整窗；本项目没有任何子元素
//! 注册 drop 监听（官方件注册的是它们自己的 payload 类型，`TypeId` 对不上，
//! 连 `stop_propagation` 都到不了），于是事件一路冒到根 = 拖到哪都能放。
//!
//! ⚠️ **"整窗"指的是仓库页那一屏**：总览页不收（D78，判据 `state::drop_target`）。
//!
//! ## 归属：曾经不是参数，所以曾经会被忘掉（D78）
//!
//! 2026-10-06 实机 bug：在「第三方仓库」页拖文件夹进来，仓库进了「我的仓库」。
//! 根因不在拖放这一行，而在 `repos::add_repo` 的**签名里没有"归属"这个参数**——
//! 它造一条默认记录（`is_third_party = false`），谁要归属谁自己去补
//! `store::set_third_party`。于是**每条添加路径都得自己记得补**：
//!
//! | 路径 | 补了吗 | 结果 |
//! |---|---|---|
//! | 本模块拖放 | **没补** | 一律进「我的」← 就是这个 bug |
//! | `dialogs/repo.rs` 添加弹窗 | 补了，但默认值写死 `third: false` | 弹窗默认也永远勾「我的」 |
//! | `actions.rs` 撤销移除 | 先写错再由 `restore_entry` 原样写回 | 最终对，但**落盘两次** |
//! | `clone.rs` 克隆完成 | 没补（core 拿不到界面状态） | 一律进「我的」，通知文案也跟着说错 |
//!
//! 老版本（`11d56d0` 的 `api.addRepo(path)`）同样没有这个参数，**不是重写引入的回归**。
//! 修法不是给拖放补上那一句，而是让 `add_repo(path, third)` 把归属变成**必填参数**——
//! 漏传编译不过，编译器替我们记住（D78）。

use std::path::PathBuf;

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::v_flex;
use gpui_kit::component::{Theme, WindowExt};
use gpui_kit::{div, px, AnyElement, Context, IntoElement, ParentElement, Styled, Window};
use qingbird_core::repos;
use qingbird_core::run_cmd;

use crate::state::{Group, RepoView, Shell};
use crate::tokens::{
    fs_body, fs_sec, icon_md, tier3, GAP, GAP_LG, r_xl,
};

/// 一个拖入项的判定结果。**纯逻辑，不碰磁盘**——IO 由调用方做完再传进来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// 已经在列表里。静默跳过：重复拖同一个仓库不是错误
    Already,
    /// 路径是空的 / 不存在 / 不是文件夹
    NotADir,
    /// 是文件夹但不是 git 仓库
    NotARepo,
    /// 可以加
    CanAdd,
}

/// 按「已在列表 → 是不是目录 → 是不是 git 仓库」三档判一个拖入项。
///
/// **顺序不能换**：一个已经在列表里的路径不该再去探盘（那是一次白花的 IO），
/// 而"不是目录"必须排在"不是仓库"前面——`valid_repo` 对不存在的路径也会返回
/// false，先判它就会被误报成"不是 git 仓库"，给出"可在此初始化"这种无用建议。
pub(crate) fn classify(
    path: &str,
    already_in_list: bool,
    is_dir: bool,
    valid_repo: bool,
) -> Verdict {
    if already_in_list {
        Verdict::Already
    } else if path.is_empty() || !is_dir {
        Verdict::NotADir
    } else if !valid_repo {
        Verdict::NotARepo
    } else {
        Verdict::CanAdd
    }
}

/// 一批拖入项的汇总。给用户看的那句话由 [`DropReport::summary`] 算。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DropReport {
    pub added: usize,
    /// 已在列表里，静默跳过
    pub dup: usize,
    /// 路径不存在或不是文件夹
    pub not_dir: usize,
    /// 是文件夹但不是 git 仓库
    pub not_repo: usize,
    /// `add_repo` 写配置失败（路径合法但落盘失败）
    pub failed: usize,
}

impl DropReport {
    /// 全都顺利 ⇒ 不出声。
    ///
    /// **判据：成功的反馈由界面本身承担**——仓库出现在左栏就是反馈。
    /// 再弹一个通知是重复打断视线，项目里已经有这个先例
    /// （`repo_head.rs` 复制地址刻意不用通知，就地换对号两秒）。
    pub(crate) fn is_quiet(&self) -> bool {
        self.not_dir == 0 && self.not_repo == 0 && self.failed == 0
    }

    /// 没问题的部分不写进这句话——只说哪儿没成。
    pub(crate) fn summary(&self) -> Option<String> {
        if self.is_quiet() {
            return None;
        }
        let mut parts: Vec<String> = Vec::new();
        match self.not_repo {
            // 单个 → 正好对应「可以直接在这初始化」，文案要说清下一步
            1 => parts.push("拖入的文件夹不是 git 仓库，可在此初始化".to_string()),
            n => parts.push(format!("有 {n} 个拖入项不是 git 仓库（未找到 .git）")),
        }
        if self.not_dir > 0 {
            parts.push(format!("{} 个拖入项不是文件夹", self.not_dir));
        }
        if self.failed > 0 {
            parts.push(format!("{} 个加不进列表（配置写不进去）", self.failed));
        }
        Some(parts.join("；"))
    }
}

impl Shell {
    /// 一个拖入项一个仓库。**逐条走「添加仓库」那套既有分支**，不另写一套。
    ///
    /// 与 `dialogs/repo.rs::submit` 同源：不是 git 仓库不是用户的错，
    /// 转初始化向导；已经在列表里的静默跳过。
    /// **不自己写一套判据**是这个模块存在的意义——两套判据迟早漂。
    ///
    /// ## `third` 是**传进来**的，不是这里问出来的
    ///
    /// 归属判据（`state::drop_target`）由 `shell.rs` 的落手闸门算好后交给本函数，
    /// 这里**不再读一次 `side_focus`**。两处各读一次看着像小事，但改了一处
    /// 忘了另一处就是"提示说进第三方、实际落进我的"——界面自己打自己的脸。
    ///
    /// ⚠️ **同步执行**（`valid_repo` / `add_repo` 都碰磁盘）。这与
    /// `dialogs/repo.rs` 那条路一致，不算新引入的卡顿来源；
    /// 拖一次通常是一两个目录，代价可接受。真变成高频路径再上
    /// `background_executor`，而不是先把它异步化再到处加 `seq` 代数守卫。
    pub(crate) fn on_paths_dropped(
        &mut self,
        paths: &[PathBuf],
        third: Group,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 兜底闸门：这一屏不收就别落盘。正常路径上 `shell.rs` 已经拦过一道，
        // 这里再来一道是因为本函数是 `pub(crate)`——以后谁从别处调它，
        // 漏掉判据的后果是**悄悄把仓库写进配置**，那种错没人会发现。
        if self.drop_target().is_none() {
            return;
        }
        let third = third.is_third();
        let mut report = DropReport::default();
        let mut first_not_repo: Option<String> = None;
        let mut last_added: Option<String> = None;

        for p in paths {
            let raw = p.to_string_lossy().to_string();
            let path = repos::normalize_path_input(&raw);
            let already = !path.is_empty() && self.repos.iter().any(|r| r.path == path);
            let is_dir = !path.is_empty() && std::path::Path::new(&path).is_dir();
            // 已经在列表里就不再探盘：这一次 IO 纯属白花
            let valid = already || (!path.is_empty() && repos::valid_repo(&path));

            match classify(&path, already, is_dir, valid) {
                Verdict::Already => report.dup += 1,
                Verdict::NotADir => report.not_dir += 1,
                Verdict::NotARepo => {
                    report.not_repo += 1;
                    if first_not_repo.is_none() {
                        first_not_repo = Some(path.clone());
                    }
                }
                Verdict::CanAdd => {
                    // 与 `dialogs/repo.rs` 同一条：加成功才谈得上后面的视图/命令识别，
                    // 失败就当没加过，绝不留下半成品。
                    // **归属随这一次写入落盘**，不再事后补第二句（D78）
                    match repos::add_repo(&path, third) {
                        Ok(_) => {
                            report.added += 1;
                            // 新仓库的命令要重新识别（启动时只识别了列表里已有的）
                            self.views
                                .entry(path.clone())
                                .or_insert_with(RepoView::blank)
                                .detected = run_cmd::detect_cmds(&path);
                            last_added = Some(path.clone());
                        }
                        Err(_) => report.failed += 1,
                    }
                }
            }
        }

        if report.added > 0 {
            self.repos = repos::list_repos();
            self.logs.remove(last_added.as_deref().unwrap_or(""));
            if let Some(p) = last_added {
                self.selected = Some(p);
            }
            self.refresh_status(cx);
            self.scan_space(cx);
            self.scan_profile(cx);
            cx.notify();
        }

        // 旧版同行为：**只有一个**无效项时直接开初始化向导，
        // 用户最可能的意图就是"把这里变成仓库"。多个就别自作主张，逐条报错
        if report.not_repo == 1 {
            if let Some(p) = first_not_repo {
                self.enter_init(p, cx);
            }
        }
        if let Some(msg) = report.summary() {
            window.push_notification(msg, cx);
        }
    }
}

/// 拖入时的落点提示。**只在真的悬着一个外部文件拖拽、且这一屏收拖放时画**
/// （`self.dragging && drop_target().is_some()`，见 `shell.rs`）。
///
/// 整窗一层薄纱 + 居中一张卡：语义是"这个窗口哪儿都能放"，
/// 与 `on_drop` 挂在根 div 上是同一件事的两种说法。
///
/// **卡上要写出落进哪一组**（`group` 是 `drop_target` 算好的那个）：
/// 用户的预期是"在哪个类别拖就进哪个分类"，而"归属"是**不可见的**——
/// 不写在提示上，他就只能拖完再去列表里核对，发现错了再右键搬。
/// 提示上写出来，这条规则就从"心里想的"变成"界面说的"。
pub(crate) fn drop_overlay(t: &Theme, group: Group) -> AnyElement {
    let (label, _) = group.label_and_icon();
    v_flex()
        .absolute()
        .inset_0()
        .items_center()
        .justify_center()
        .bg(t.colors.background.opacity(0.82))
        .child(
            v_flex()
                .items_center()
                .justify_center()
                .gap(px(GAP))
                .rounded(px(r_xl(t)))
                .border_1()
                .border_color(t.colors.accent)
                .bg(t.colors.popover)
                .px(px(GAP_LG * 4.0))
                .py(px(GAP_LG * 2.0))
                .child(icon_md(AssetIcon::Folder).text_color(t.colors.accent))
                .child(
                    div()
                        .text_size(px(fs_sec()))
                        .text_color(t.colors.foreground)
                        .child(format!("松开即加入「{label}」")),
                )
                .child(
                    div()
                        .text_size(px(fs_body()))
                        .text_color(tier3(t))
                        .child("已在这个列表里的会跳过；不是仓库的会问你要不要初始化"),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn already_in_the_list_is_skipped_without_probing() {
        // already=true 时 valid/is_dir 一律给 false（调用方压根没探盘），
        // 判据仍必须是 Already 而不是 NotADir
        assert_eq!(classify("F:\\a", true, false, false), Verdict::Already);
    }

    /// 顺序判据：`valid_repo(false)` 对一个**不存在的路径**也返回 false。
    /// 若先判仓库就会把它误报成「不是 git 仓库，可在此初始化」——
    /// 而对不存在的路径提"要不要初始化"是废话。
    #[test]
    fn a_missing_path_is_not_reported_as_a_non_repo() {
        assert_eq!(classify("F:\\nope", false, false, false), Verdict::NotADir);
        assert_eq!(classify("", false, false, false), Verdict::NotADir);
    }

    #[test]
    fn a_plain_folder_is_offered_for_initialization() {
        assert_eq!(
            classify("F:\\plain", false, true, false),
            Verdict::NotARepo
        );
    }

    #[test]
    fn a_real_repo_can_be_added() {
        assert_eq!(classify("F:\\repo", false, true, true), Verdict::CanAdd);
    }

    /// 判据是**真的没话说**才安静。全成功与"只有重复"都该安静。
    #[test]
    fn silence_only_when_nothing_went_wrong() {
        assert!(DropReport { added: 3, ..Default::default() }.is_quiet());
        assert!(DropReport { added: 1, dup: 2, ..Default::default() }.is_quiet());
        assert!(DropReport { not_repo: 1, ..Default::default() }.summary().is_some());
        assert!(DropReport { not_dir: 1, ..Default::default() }.summary().is_some());
        assert!(DropReport { failed: 1, ..Default::default() }.summary().is_some());
    }

    /// 旧版行为：单个无效项要**说清下一步**（可以在这初始化），
    /// 多个只报数量——多个的时候弹一个初始化向导是自作主张。
    #[test]
    fn one_bad_folder_offers_initialization_and_several_only_count() {
        let one = DropReport { not_repo: 1, ..Default::default() }
            .summary()
            .unwrap();
        assert!(one.contains("可在此初始化"), "{one}");

        let many = DropReport { not_repo: 3, ..Default::default() }
            .summary()
            .unwrap();
        assert!(many.contains('3'), "{many}");
        assert!(!many.contains("可在此初始化"), "{many}");
    }

    /// 成功的那部分**不进文案**——界面里仓库已经出现在左栏了，
    /// 再说一遍是重复反馈。
    #[test]
    fn the_summary_mentions_only_what_failed() {
        let s = DropReport {
            added: 2,
            not_repo: 1,
            ..Default::default()
        }
        .summary()
        .unwrap();
        assert!(!s.contains("2 个"), "{s}");
    }

    /// 多类失败要**都说到**，漏掉一条用户就少知道一件事。
    #[test]
    fn every_failure_kind_is_reported() {
        let s = DropReport {
            not_repo: 1,
            not_dir: 2,
            failed: 1,
            ..Default::default()
        }
        .summary()
        .unwrap();
        assert!(s.contains("不是 git 仓库"), "{s}");
        assert!(s.contains("不是文件夹"), "{s}");
        assert!(s.contains("配置写不进去"), "{s}");
    }
}
