//! 侧栏动作：选中、定位、归属变更、移除撤销。
//!
//! 「直接生效 + 通知撤销，不弹确认框」是用户拍板的口径：归属只是一个可逆的
//! 分组标签，移除只删列表记录不动磁盘文件，两者都能一键回退。
//! 弹确认框反而会让人养成"闭着眼点确定"的习惯，真删错了连后悔的机会都没有。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Sizable, WindowExt};
use gpui_kit::{App, Context, Window};
use qingbird_core::repos;
use qingbird_core::store;
use qingbird_core::types::RepoStatus;

use crate::state::*;

impl Shell {
    /// 某仓库的 git 状态。还没扫到时给默认（`is_repo: false` → 待扫描档）
    pub(crate) fn status_of(&self, path: &str) -> RepoStatus {
        self.view(path)
            .map(|v| v.status.clone())
            .unwrap_or_default()
    }

    /// 过滤框的小写文本。空串 = 不过滤
    pub(crate) fn filter_text(&self, cx: &App) -> String {
        self.side_filter.read(cx).value().trim().to_lowercase()
    }

    /// 把某个仓库摆到眼前：切到仓库页 → 展开侧栏 → 展开它那一组 → 标记待定位。
    ///
    /// 侧栏行点击、右键菜单的「打开」、总览表格点行、从 ⌘K 切仓库，
    /// 全走这一个入口——四条路径的"看到哪儿"必须一致。
    pub(crate) fn reveal_repo(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.entry(&path) else {
            return;
        };
        let group = if entry.is_third_party {
            Group::Third
        } else {
            Group::Mine
        };
        self.page = Page::Repos;
        self.sidebar_collapsed = false;
        // 只展开**目标那一组**：另一组是用户自己折的，为了跳一个仓库
        // 把他摆好的侧栏也撑开，等于每次跳转都在改他的桌面
        self.group_open[group.ix()] = true;
        self.side_focus = group;
        self.reveal = Some(path.clone());
        self.select(path, window, cx);
    }

    /// 把侧栏滚到 `reveal` 那一行。每帧调一次，滚完就清空。
    ///
    /// ⚠️ **只数当前 `side_focus` 那一组**——侧栏只画那一组（`sidebar.rs`）。
    /// 早先侧栏画两组时这里也是遍历两组，两边"行号 = 渲染顺序"才对得上；
    /// 侧栏改成只画一组后这里不改，就会滚到一个不存在的行上（表现为"跳过去但没滚到"）。
    /// 改侧栏的渲染结构时**这里必须一起改**。
    pub(crate) fn apply_reveal(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.reveal.take() else {
            return;
        };
        // 目标不在当前这一组就别定位了：那行根本没渲染出来，滚过去是空的。
        // `reveal_repo` 会先把 `side_focus` 切到目标那一组，所以正常路径下
        // 走到这里时目标一定在这一组里；这一判是给"归属刚被改掉"这类路径兜底
        let Some(entry) = self.entry(&target) else {
            return;
        };
        if entry.is_third_party != self.side_focus.is_third() {
            return;
        }
        let filter = self.filter_text(cx);
        // 第 0 行是分组标题自己（`sidebar.rs` 里 `ix` 从 1 起）
        let mut ix = 1usize;
        for r in &self.repos {
            if r.is_third_party != self.side_focus.is_third() {
                continue;
            }
            if !crate::sidebar::matches_filter(r, &self.status_of(&r.path), &filter) {
                continue;
            }
            if r.path == target {
                // 目标行被过滤掉了就放弃定位：硬滚到一个看不见的行没有意义
                self.tree_scroll.scroll_to_top_of_item(ix);
                return;
            }
            ix += 1;
        }
    }
}

impl Shell {
    /// 换归属：直接落盘 + 通知撤销。第三方 ↔ 我的
    pub(crate) fn set_group(
        &mut self,
        path: &str,
        third: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(e) = store::set_third_party(path, third) {
            window.push_notification(format!("改归属失败：{e}"), cx);
            return;
        }
        if let Some(e) = self.repos.iter_mut().find(|r| r.path == path) {
            e.is_third_party = third;
        }
        // 选中的是这个仓库时，焦点跟着它走，否则导航高亮与主区内容会错位
        if self.selected.as_deref() == Some(path) {
            self.side_focus = if third { Group::Third } else { Group::Mine };
        }
        let me = cx.entity();
        let undo_path = path.to_string();
        window.push_notification(
            Notification::info(if third {
                format!("已移到第三方仓库 · {}", self.title(path))
            } else {
                format!("已移回我的仓库 · {}", self.title(path))
            })
            // `action` 的闭包要**返回一个 Button**，点击逻辑得自己挂上去。
            // 撤销的语义是"把上一步反过来"，所以这里只有一个按钮，不给"确认/取消"那套
            // 外层是 `Fn`（通知可能被反复重建），所以要用的数据得在里层借，不能直接搬走
            .action(move |_, _, _| {
                let me = me.clone();
                let p = undo_path.clone();
                Button::new("undo-group")
                    .label("撤销")
                    .small()
                    .primary()
                    .on_click(move |_, window, cx| {
                        me.update(cx, |this, cx| {
                            if let Err(e) = store::set_third_party(&p, !third) {
                                window.push_notification(format!("撤销失败：{e}"), cx);
                                return;
                            }
                            if let Some(e) = this.repos.iter_mut().find(|r| r.path == p) {
                                e.is_third_party = !third;
                            }
                            if this.selected.as_deref() == Some(p.as_str()) {
                                this.side_focus = if third { Group::Mine } else { Group::Third };
                            }
                            cx.notify();
                        });
                    })
            }),
            cx,
        );
        cx.notify();
    }

    /// 移除仓库：只删列表记录（不动磁盘文件），带一键撤销
    pub(crate) fn remove_repo_undoable(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self.title(path);
        let Some(prev) = self.entry(path).cloned() else {
            return;
        };
        if let Err(e) = repos::remove_repo(path) {
            window.push_notification(format!("移除失败：{e}"), cx);
            return;
        }
        self.repos = repos::list_repos();
        self.views.remove(path);
        self.logs.remove(path);
        if self.selected.as_deref() == Some(path) {
            self.selected = self.repos.first().map(|r| r.path.clone());
        }
        self.dlg = None;

        let me = cx.entity();
        window.push_notification(
            Notification::warning(format!("已移除 {name}（磁盘文件没动）")).action(
                move |_, _, _| {
                    let me = me.clone();
                    let prev = prev.clone();
                    Button::new("undo-remove")
                        .label("撤销")
                        .small()
                        .primary()
                        .on_click(move |_, window, cx| {
                            me.update(cx, |this, cx| {
                                // 撤销走 add_repo：它会重新校验路径并补一条最小记录。
                                // **归属传 `prev` 的**：不加这个参数的话这里会先
                                // 写一条 `is_third_party = false` 的新记录，
                                // 再由下面的 `restore_entry` 盖回去——最终对，
                                // 但中间那次写是错的（且两次落盘，D78）
                                if let Err(e) = repos::add_repo(&prev.path, prev.is_third_party) {
                                    window.push_notification(format!("撤销失败：{e}"), cx);
                                    return;
                                }
                                // 再把记录原样写回：add_repo 造不出别名、命令、定时那些，
                                // 只留一条"新"记录等于把用户攒了几个月的设置清空
                                if let Err(e) = restore_entry(&prev) {
                                    window
                                        .push_notification(format!("撤销后还原设置失败：{e}"), cx);
                                }
                                this.repos = repos::list_repos();
                                this.views
                                    .entry(prev.path.clone())
                                    .or_insert_with(RepoView::blank);
                                cx.notify();
                            });
                        })
                },
            ),
            cx,
        );
        cx.notify();
    }
}

/// 把一条记录原样写回配置。
fn restore_entry(prev: &store::RepoEntry) -> Result<(), String> {
    let mut cfg = store::load();
    match cfg.repos.iter_mut().find(|e| e.path == prev.path) {
        Some(slot) => {
            *slot = prev.clone();
        }
        None => cfg.repos.push(prev.clone()),
    }
    store::save(&cfg)
}
