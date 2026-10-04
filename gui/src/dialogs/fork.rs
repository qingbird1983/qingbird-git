//! 分叉恢复对话框里的提交列表块。

// 图标只认这一套：assets 里是完整 lucide，外加品牌 Bird。
// 早先同时用了 component::IconName 和 assets::IconName——两套的线宽与视觉重量不同，
// 混在同一屏里一眼就能看出不一致。
use gpui_kit::base::v_flex;
use gpui_kit::component::Theme;
use gpui_kit::{div, px, Div, ParentElement, Styled};

use crate::tokens::*;

/// 分叉卡差异区的提交清单块。空列表要点明"没有"而不是留白（与旧版 forkGuide 一致）。
pub(crate) fn fork_commit_block(is_local: bool, commits: &[String], t: &Theme) -> Div {
    let n = commits.len();
    let title = match (is_local, n) {
        (true, 0) => "本地没有未推送的提交".to_string(),
        (true, _) => format!("本地未推送的提交（{n}）"),
        (false, 0) => "云端没有新提交".to_string(),
        (false, _) => format!("云端新增的提交（{n}）"),
    };
    v_flex()
        .w_full()
        .gap(px(GAP_TIGHT))
        .child(
            div()
                .text_size(px(FS_SEC))
                .text_color(tier3(t))
                .child(title),
        )
        .children(commits.iter().map(|c| {
            div()
                .font_family(t.mono_font_family.clone())
                .text_size(px(FS_META))
                .text_color(body(t))
                .child(c.clone())
        }))
}
