//! 软件内自更新的 GUI 侧（Q44，2026-10-13 拍板：全自动版）。
//!
//! 状态机：`Idle → Checking → (UpToDate | Available) → Downloading → Ready`
//! （任一步可进 `Failed`，带着**退回哪一格**的 `retry`，重试键长在哪一格就回哪格）。
//!
//! ## 三条落位约定
//!
//! 1. **状态挂在 `Shell` 上而不是设置对话框上**：下载是好几十秒的长活，
//!    用户中途关掉设置弹窗是常态——状态在对话框上等于"关掉就失联"，
//!    再打开要么看到 Idle（其实还在下），要么后台白跑。挂在 Shell 上，
//!    弹窗关了下载照走，重开弹窗进度还在
//! 2. **进度靠轮询文件大小**：core 下载只管把文件写到临时目录，这边每
//!    400ms 量一次文件长度除以 API 给的 `asset_size`——不在 core 里解析
//!    curl 进度条（stderr 的 `#` 行格式会漂），可测的那半（总大小）有真源
//! 3. **拉起安装器 = spawn + 立即退出**：安装器要覆盖本应用自己的 exe，
//!    文件锁必须在安装器动手前交出来。spawn 是分离式的（DETACHED_PROCESS），
//!    不随本进程一起死

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, Context, Div, Entity, FontWeight, IntoElement, ParentElement, Styled,
};
use qingbird_core::update::{self, Release};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::state::Shell;
use crate::tokens::*;
use crate::widgets::{meta_text, FormRows};
use crate::dialogs::settings::SetCard;

/// 进度轮询的间隔。比 ops.rs 的 120ms 慢：那边是"输出行要看着连续"，
/// 这里只有一个百分比数字，400ms 足够顺眼，重绘少四分之三
const POLL: Duration = Duration::from_millis(400);

/// 自更新这一条链的当前状态。见模块头的状态机图
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) enum UpdFlow {
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// 有新版本，等用户点「下载并安装」
    Available(Release),
    Downloading {
        rel: Release,
        got: u64,
        /// 实际在用的下载路（`http://host:port`）；None = 直连
        via: Option<String>,
    },
    /// 下载完成，等用户点「安装并重启」
    Ready {
        path: PathBuf,
        tag: String,
    },
    /// 卡在哪一步。`retry` 记着**退回哪一格**——重试键的形态由它决定
    /// （Idle → 重试键是「检查更新」；Available → 是「重新下载」）
    Failed {
        msg: String,
        retry: Box<UpdFlow>,
    },
}

impl Shell {
    /// 「检查更新」。后台跑 core 的检查（内含直连→代理回退），
    /// 结果落回 `upd`；比当前版本新才报 Available
    pub(crate) fn check_for_update(&mut self, cx: &mut Context<Self>) {
        // 检查/下载正在跑时再点检查 = 叠两条后台链，结果互相覆盖
        if matches!(
            self.upd,
            UpdFlow::Checking | UpdFlow::Downloading { .. }
        ) {
            return;
        }
        self.upd = UpdFlow::Checking;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async { update::fetch_latest() })
                .await;
            this.update(cx, |this, cx| {
                this.upd = match res {
                    Ok(rel) if update::is_newer(current_version(), &rel.tag) => {
                        UpdFlow::Available(rel)
                    }
                    Ok(_) => UpdFlow::UpToDate,
                    Err(e) => UpdFlow::Failed {
                        msg: e,
                        retry: Box::new(UpdFlow::Idle),
                    },
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 「下载并安装」的第一半：下载到临时目录。
    ///
    /// 两条后台链：下载链（阻塞跑 core）+ 轮询链（量文件大小回写状态）。
    /// 收尾只认轮询链——它看到 `done` 之后的那一拍才写终态，
    /// 下载链自己**不碰** `self.upd`（两条链都写就是竞态）
    pub(crate) fn start_download(&mut self, rel: Release, cx: &mut Context<Self>) {
        if matches!(
            self.upd,
            UpdFlow::Checking | UpdFlow::Downloading { .. }
        ) {
            return;
        }
        let dest = std::env::temp_dir().join(&rel.asset_name);
        // 上一次失败留下的半个文件先清掉，轮询的大小才是这一轮的
        let _ = std::fs::remove_file(&dest);
        self.upd = UpdFlow::Downloading {
            rel: rel.clone(),
            got: 0,
            via: None,
        };
        cx.notify();

        let signals = Arc::new(update::DownloadSignals::default());
        let done = Arc::new(AtomicBool::new(false));
        let result: Arc<Mutex<Option<Result<(), String>>>> = Arc::new(Mutex::new(None));

        // 下载链：只写 result 与 done，不碰 Shell
        let dl_signals = signals.clone();
        let dl_rel = rel.clone();
        let dl_dest = dest.clone();
        let dl_result = result.clone();
        let dl_done = done.clone();
        cx.spawn(async move |_, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { update::download_installer(&dl_rel, &dl_dest, &dl_signals) })
                .await;
            if let Ok(mut slot) = dl_result.lock() {
                *slot = Some(res);
            }
            dl_done.store(true, Ordering::SeqCst);
        })
        .detach();

        // 轮询链：每 400ms 把文件大小与下载路搬进状态；done 之后写终态收摊
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL).await;
            let finished = done.load(Ordering::SeqCst);
            let got = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
            let via = signals
                .via
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let res = result
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            this.update(cx, |this, cx| {
                let UpdFlow::Downloading { rel, .. } = &this.upd else {
                    return; // 用户把状态拨走了（理论上进不来），不覆写
                };
                let rel = rel.clone();
                this.upd = match res {
                    Some(Ok(())) => UpdFlow::Ready {
                        path: dest.clone(),
                        tag: rel.tag.clone(),
                    },
                    Some(Err(e)) => UpdFlow::Failed {
                        msg: e,
                        retry: Box::new(UpdFlow::Available(rel)),
                    },
                    None => UpdFlow::Downloading { rel, got, via },
                };
                cx.notify();
            })
            .ok();
            if finished {
                break;
            }
        })
        .detach();
    }

    /// 「安装并重启」：分离式拉起安装器（/SILENT 只显示进度页），
    /// 本应用随即退出——文件锁交出来，安装器才覆盖得了自己的 exe
    pub(crate) fn install_and_quit(&mut self, cx: &mut Context<Self>) {
        let UpdFlow::Ready { path, tag } = &self.upd else {
            return;
        };
        let path = path.clone();
        let tag = tag.clone();
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        match std::process::Command::new(&path)
            .arg("/SILENT")
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
            .spawn()
        {
            Ok(_) => {
                // 退出动作放到异步里：tray 的「退出」也是这条路（`App::quit`）
                cx.spawn(async move |_, cx| {
                    cx.update(|cx: &mut App| cx.quit());
                })
                .detach();
            }
            Err(e) => {
                self.upd = UpdFlow::Failed {
                    msg: format!("无法启动安装器：{e}"),
                    retry: Box::new(UpdFlow::Ready { path, tag }),
                };
                cx.notify();
            }
        }
    }
}

/// 字节数 → "N.N MB"（进度文案用）。
fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

/// 这张卡在当前状态下的**一句话状态**（挂卡头 note 格，与代理卡同款）。
fn flow_note(flow: &UpdFlow) -> Option<String> {
    Some(match flow {
        UpdFlow::Idle => "从 GitHub Releases 检查新版本".to_string(),
        UpdFlow::Checking => "正在检查…".to_string(),
        UpdFlow::UpToDate => "已是最新版本".to_string(),
        UpdFlow::Available(rel) => format!("发现新版本 {}", rel.tag),
        UpdFlow::Downloading { got, rel, via } => {
            let part = if rel.asset_size > 0 {
                format!(
                    "{}%（{} / {}）",
                    got * 100 / rel.asset_size.max(1),
                    mb(*got),
                    mb(rel.asset_size)
                )
            } else {
                mb(*got)
            };
            match via {
                Some(p) => format!("下载中 {part} · 经代理 {p}"),
                None => format!("下载中 {part} · 直连"),
            }
        }
        UpdFlow::Ready { tag, .. } => format!("{tag} 下载完成"),
        UpdFlow::Failed { .. } => "更新失败".to_string(),
    })
}

/// 设置页「软件更新」卡。
pub(crate) fn update_card(
    t: &Theme,
    st: &crate::dialogs::settings::SettingsState,
    this: &Entity<Shell>,
    gv: ButtonCustomVariant,
) -> Div {
    let flow = st.upd.clone();
    let mut card = SetCard::new("软件更新").info(
        "set-info-update",
        "检查新版本 → 应用内下载安装包 → 自动拉起安装器并退出。\
         下载走与 git 同一套代理探测：直连不通自动换代理再试。\
         安装包来自 GitHub Releases。",
    );
    if let Some(n) = flow_note(&flow) {
        card = card.note(n);
    }

    // 动作键：形态由状态决定，一颗函数出「这一格该摆的键」
    let me = this.clone();
    let action: gpui_kit::AnyElement = match &flow {
        UpdFlow::Checking => h_flex()
            .items_center()
            .gap(px(GAP_TIGHT))
            .child(Spinner::new().with_size(Size::XSmall).color(tier4(t)))
            .child(
                div()
                    .text_size(px(fs_meta()))
                    .text_color(tier4(t))
                    .child("检查中"),
            )
            .into_any_element(),
        UpdFlow::Downloading { .. } => h_flex()
            .items_center()
            .gap(px(GAP_TIGHT))
            .child(Spinner::new().with_size(Size::XSmall).color(tier4(t)))
            .child(
                div()
                    .text_size(px(fs_meta()))
                    .text_color(tier4(t))
                    .child("下载中，关掉这个窗口也不中断"),
            )
            .into_any_element(),
        UpdFlow::Available(rel) => {
            let rel = rel.clone();
            Button::new("upd-download")
                .label(format!("下载并安装（{}）", mb(rel.asset_size)))
                .with_size(Size::Small)
                .on_click(move |_, _, cx| {
                    let rel = rel.clone();
                    me.update(cx, |this, cx| this.start_download(rel, cx));
                })
                .into_any_element()
        }
        UpdFlow::Ready { .. } => Button::new("upd-install")
            .label("安装并重启")
            .with_size(Size::Small)
            .on_click(move |_, _, cx| {
                me.update(cx, |this, cx| this.install_and_quit(cx));
            })
            .into_any_element(),
        UpdFlow::Failed { retry, .. } => match &**retry {
            UpdFlow::Available(rel) => {
                let rel = rel.clone();
                Button::new("upd-retry-dl")
                    .label("重新下载")
                    .custom(gv)
                    .with_size(Size::Small)
                    .on_click(move |_, _, cx| {
                        let rel = rel.clone();
                        me.update(cx, |this, cx| this.start_download(rel, cx));
                    })
                    .into_any_element()
            }
            _ => Button::new("upd-retry-check")
                .label("重试检查")
                .custom(gv)
                .with_size(Size::Small)
                .on_click(move |_, _, cx| {
                    me.update(cx, |this, cx| this.check_for_update(cx));
                })
                .into_any_element(),
        },
        // Idle / UpToDate：同一颗「检查更新」
        _ => Button::new("upd-check")
            .label("检查更新")
            .custom(gv)
            .with_size(Size::Small)
            .on_click(move |_, _, cx| {
                me.update(cx, |this, cx| this.check_for_update(cx));
            })
            .into_any_element(),
    };

    card.build(
        t,
        gv,
        FormRows::new()
            .row(
                "当前版本",
                h_flex()
                    .items_center()
                    .gap(px(GAP))
                    .child(
                        div()
                            .text_size(px(fs_sec()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(body(t))
                            .child(crate::dialogs::settings::version_text()),
                    )
                    .child(action),
            )
            .build(t),
    )
    // 失败的详情：一行红字挂在卡底，别只让卡头说"更新失败"四个字
    .when_some(
        match &flow {
            UpdFlow::Failed { msg, .. } => Some(msg.clone()),
            _ => None,
        },
        |b, msg| {
            b.child(
                v_flex()
                    .w_full()
                    .child(
                        div()
                            .text_size(px(fs_meta()))
                            .text_color(t.colors.danger)
                            .child(msg),
                    )
                    .child(meta_text(
                        "多半是网络问题：代理软件没在运行，或 GitHub 暂时连不上。",
                        t,
                    )),
            )
        },
    )
}

/// 当前运行的版本号。**`env!("CARGO_PKG_VERSION")` 全仓库仍只有一处**——
/// 原来只住在 `settings::version_text` 里，自更新要比对版本，把它收成
/// 这颗纯函数，`version_text` 改为调它：显示与比对是**同一个数**。
pub(crate) fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
