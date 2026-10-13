//! 对话框：添加/编辑仓库、初始化向导、分叉恢复、克隆、命令编辑、提交并推送、设置、批量结果。
//!
//! 各自独立成文件的原因是它们共用同一套 Dialog 外壳（本模块的 `footer` 按钮排布、
//! Esc 关闭、焦点归位），但内容互不相干。拆开是为了改一个对话框时 diff 里
//! 不会带出另外两个。

pub(crate) mod batch_log;
pub(crate) mod clean;
pub(crate) mod cli;
pub(crate) mod clone;
pub(crate) mod close_prompt;
pub(crate) mod cmds;
pub(crate) mod commit;
pub(crate) mod fork;
pub(crate) mod init;
pub(crate) mod purge;
pub(crate) mod repo;
pub(crate) mod settings;
pub(crate) mod settings_keys;
pub(crate) mod stash;
pub(crate) mod sync;

use gpui_kit::base::h_flex;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogContent;
use gpui_kit::component::WindowExt as _;
use gpui_kit::{
    px, AnyElement, App, Context, Div, Entity, IntoElement, ParentElement as _, SharedString,
    Styled as _, Window,
};

use crate::state::Shell;
use crate::tokens::GAP;

/// 对话框底栏：**这一套按钮排布的全仓唯一出处**。
///
/// 十二处底栏原来各抄一遍 `h_flex().w_full().justify_*.gap(px(GAP))`
/// （审查台账 §1 第一条，2026-10-12）。合并时定的接口是**左槽 + 右排**：
///
/// - `left` 有内容时两端对齐（`justify_between`），`None` 时整排右对齐
///   （`justify_end`）。⚠️ **左槽必须整块给进来**（一颗 Button 或一小块
///   `h_flex` 都行）：`justify_between` 是按 flex 子项摊空位的，把左槽的键与
///   右排的键并排传进来，右排中间那颗会被推到正中间——repo 那颗「移除仓库」
///   早年就是这么被挤跑的（台账 §3.1 拍板时定的这条）。
/// - `actions` 是右侧那一排动作键，键间缝隙这里给（`GAP`）；与左槽之间那一档
///   由外层 `justify_between` 负责，不额外垫。
pub(crate) fn footer(left: Option<AnyElement>, actions: impl IntoIterator<Item = Button>) -> Div {
    let actions = h_flex().items_center().gap(px(GAP)).children(actions);
    let row = h_flex().w_full().items_center().gap(px(GAP));
    match left {
        Some(l) => row.justify_between().child(l).child(actions),
        None => row.justify_end().child(actions),
    }
}

/// 底栏那颗「取消」：**先清掉本对话框的状态，再关窗**。
///
/// 为什么必须先清：`Dialog` 关掉之后状态字段还留在 `Shell` 上，下一次同一个
/// 入口进来会读到上一次的残留（repo 那颗最明显：路径还填着上一回的）。
///
/// `clear` 由调用方给（每个对话框的字段不同），收 `&mut Context` 是为了让
/// repo / init 那种"清完还要 notify 一帧"的写法有地方放，其余传 `_` 即可。
/// 用 `Fn` 而不是 `FnOnce`：按钮点一次不消失（关窗失败、或同一颗键被复用），
/// 第二次点击不该 panic。
///
/// 尺寸档（`.with_size(Size::Small)` 之类）与禁用态由调用方在后面接着加——
/// 各对话框那两样本来就不一致（台账 §1 第一条：23 处按钮里只有一半带尺寸）。
pub(crate) fn dlg_cancel(
    id: &'static str,
    label: &'static str,
    this: &Entity<Shell>,
    clear: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
    gv: ButtonCustomVariant,
) -> Button {
    let this = this.clone();
    Button::new(id)
        .label(label)
        .custom(gv)
        .on_click(move |_, window, cx| {
            this.update(cx, |shell, cx| clear(shell, cx));
            window.close_dialog(cx);
        })
}

/// 开壳：十个 `build_*_dlg` 逐字同形的那一段（审查台账 §1 第二条）。
///
/// ## 为什么三个参数都是闭包
///
/// 官方 root **每帧都会重跑整个 builder**（`root.rs:200`
/// `dialog = (active_dialog.builder)(dialog, window, cx)`）——不只是
/// content 闭包。两件事靠这个才成立：
/// - 设置弹窗里拨主题开关，**开着的弹窗跟着一起变色**（`settings.rs`
///   模块头第 4 条：外壳变了、弹窗还是老的，看着就像坏了）；
/// - repo 那个框点「添加」而路径不是 git 仓库时转初始化向导，
///   **标题要当场从「添加仓库」换成「初始化为 Git 仓库」**。
///
/// 所以 `title` / `footer` 都按帧重算：早先 `commit.rs` 那句"footer 是
/// 一次性元素"的注释是**看错了签名得出的**（`Dialog::footer` 收
/// `impl IntoElement` 不假，但调它的 builder 每帧重跑，元素每帧都是新的）。
///
/// `w` 给 `None` = 官方默认宽（448）。全仓只有设置页显式给宽（880），
/// 理由见 `settings.rs` 那一大段。
pub(crate) fn shell<Fe>(
    window: &mut Window,
    cx: &mut App,
    title: impl Fn(&mut App) -> SharedString + 'static,
    w: Option<f32>,
    content: impl Fn(DialogContent, &mut Window, &mut App) -> DialogContent + 'static,
    footer: impl Fn(&mut App) -> Fe + 'static,
) where
    Fe: IntoElement,
{
    // content 要被 `Dialog::content` **按值收走**，而外层 builder 是 `Fn`
    // （每帧重跑，见头注释）⇒ 包一层 `Rc`，每帧 clone 那个指针（便宜），
    // 闭包本体始终是同一份。footer 每帧只是**调用**（`footer(cx)`），
    // 不涉及移动，不用包
    //
    // ⚠️ **footer 的返回类型必须是泛型 `IntoElement`，不能图省事收 `AnyElement`**
    // （2026-10-12 抓帧 diff 实测）：收 `AnyElement` 时调用方要 `.into_any_element()`，
    // 而 `AnyElement` 在官方组件里多裹一层，**外卡宽度会差 2px**（对话框左右缘
    // 各外扩 1px，右缘边框 1360-1364 → 1361-1365，mask 上是一条通高的竖线）。
    // 没设 `w` 的对话框宽度由内容撑开，footer 这层的测量差一点整卡就宽一点。
    // 与官方 `.footer()` 签名同口径（`impl IntoElement`）才量得出同一张卡
    let content = std::rc::Rc::new(content);
    window.open_dialog(cx, move |dialog, _window, cx| {
        let content = content.clone();
        let mut d = dialog.title(title(cx));
        if let Some(w) = w {
            d = d.w(px(w));
        }
        d.content(move |c, w, cx| content(c, w, cx))
            .footer(footer(cx))
    });
}
