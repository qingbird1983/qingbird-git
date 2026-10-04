# AGENTS.md — qingbird-git（青鸟 Git）

## 项目说明

Windows 桌面工具：本地 Git 仓库一键同步。**Rust 原生 GPUI 界面**（gpui-kit 0.7）。

界面已从 Tauri 2 + React 整体换成 Rust 原生，旧实现（`src/` + `src-tauri/`）
在 2026-10-05 随路线表 Q6 一起退役删除。业务逻辑在 `crates/core`，界面在 `gui`，
**全部代码只在这两处**。配置仍共用同一份文件
（`%APPDATA%/com.qingbird.git/config.json`），所以历史用户的仓库列表与命令设置不会丢。

主要目标：

- 本地仓库管理：添加过的仓库持久化记忆，下次启动自动加载
- 三键操作：检测云端更新 / 一键拉取（--ff-only）/ 一键推送
- 无感代理：git 网络操作直连失败时，自动探测本机代理（系统代理 → 上次成功缓存 → 常见端口 7897/7890/10808 等），
  仅对当次 git 子进程注入 `HTTPS_PROXY` 环境变量重试，不改系统代理、不影响其他程序

## 目录规则

- **代码只进 `crates/`（纯业务，core 库）与 `gui/`（界面，GPUI）**。旧实现已退役，
  仓库里不再有第二个入口
- 文档资料放在：`README.md`、本文件、`docs/UI-ROADMAP.md`（**唯一施工真源**，
  路线与决策台账 D1–D34）、`docs/UI-DESIGN.md`（设计规范）
- 临时实验放在：不要提交到仓库
- 测试文件放在：Rust 单元测试在各模块 `#[cfg(test)]` 中（core 141 个、gui 32 个）

## 常用命令

全量检查：`cargo check --workspace`

后端单测：`cargo test -p qingbird-core`（业务逻辑的测试都在这，141 个）

GUI 侧单测：`cargo test -p qingbird-gui`（32 个，只放不依赖 gpui 初始化的纯逻辑；
GUI 大部分要窗口才能测，判据是抓帧 + 实机点，见下面的「界面怎么验」）

出 exe：`cargo build --workspace` → `target/debug/qingbird-git.exe`

**GUI 只能由用户双击 exe 验收**：从 agent shell 直接拉起的 GUI 进程会随命令结束被回收
  （2026-10-01 起 spike-guise 已删除，编译缓存挪到根 `target/`，走默认 target-dir；
  `.cargo/config.toml` 已删除）
- **GUI 只能由用户双击 exe 验收**：从 agent shell 直接拉起的 GUI 进程会随命令结束被回收
- **视觉规范（新增元素必须选档，不许再写裸数字）**：`gui/src/tokens.rs` 里有
  字号（FS_META/SEC/BODY/TITLE/HEAD/NUM = 11/12/13/16/18/20）、
  圆角（R_SM/MD/LG/XL = 4/6/8/12）、
  间距（GAP_DENSE/GAP_TIGHT/GAP/GAP_LG = 2/4/8/12）、
  图标（icon_sm 14 / icon_md 16）几组常量。
  早先就地硬编码长出过 9 种圆角、6 种按钮高度，视觉一眼就不齐。
  ⚠️ 写的是 `tokens.rs` 不是 `shell.rs`——阶段 1 拆文件时搬走过，这里一度没跟上。
  `GAP_DENSE`（2）不是"更小的 GAP_TIGHT"：前者是**一串同类元素相邻两项**的呼吸量
  （列表行、色块分段、同块内多行元信息），后者是**两个不同东西**之间的关系
  （图标与文字、标题与说明、柱间）。混用会出现"标题离说明比离正文还近"
- **卡外壳与卡内表头只有两个出处**：`widgets::card(t)`（白卡 + 1px 边 + `R_XL`
  + `p(GAP)`）与 `widgets::CardHeader`（`new(title)` / `.leading(AnyElement)` /
  `.note(text, danger)` / `.right(AnyElement)`（可多次）/ `.build(t)`）。
  **别再自己手搓一份**。阶段 13-1 起因就是查出来的分叉：空间页自己手搓了外壳
  （内边距 **16**），日志 / 说明用 `log::section`（**8**），历史 / 变更**根本没有卡**
  ——五个页签摆一屏里内边距有两个值、有的有壳有的没壳。`log::section` 已删，
  **不留转发层**（留一层转发只会让人以为它还能用）。右侧操作的分组关系**留给
  调用方**（`widgets::vdivider(t)` 插在中间）：各页的分法不一样，不该由表头决定
- **段标题挂到它管的那一段的表头右上，不另起一行浮在外面**：「重新扫描」挂
  「磁盘占用」、「一键清理」挂「构建缓存」——**它扫的 / 清的就是这一段**。
  说明页那排 `📄 README.md` 同理（原来浮在卡片外面，一屏两个头）
- **`.gap()` 禁写裸数字**：选 `GAP_DENSE` / `GAP_TIGHT` / `GAP` / `GAP_LG` 四档之一。
  阶段 11 收尾时扫出 13 处裸 `gap`（8 处该是 4px 的却写了 1–2px），已全部归位
- **按钮一律用官方命名档，禁写 `Size::Size(px(...))`**：官方 Button 的自定义 px 档
  会联动把文字放大到 16px、图标放大到 size×0.75（30px 按钮出 22.5px 图标）——
  整屏图标过大、字顶边的根因。用 `Size::Medium`（32 高/14 字/16 图标）、
  `Size::Small`（24 高/14 字/14 图标）；幽灵图标按钮的图标要显式
  `.text_color(tier3(t))`（Slate #5D5B54），默认继承的是墨色，观感"太深"
- **Input 同理用 `.small()`**：`.xsmall()` 只有 20px 高、4px 水平内边距，字贴边；
  small = 24px 高 / 8px 内边距
- **图标只认一套**：`use gpui_kit::assets::IconName as AssetIcon`（完整 lucide 1830 个 + 品牌 Bird）。
  不要再用 `component::IconName`——那是另一套，线宽与视觉重量不同，且没有 GitBranch
- **hover 与动效**：gpui 没有 CSS 那样的 `:hover` 样式，悬停靠 `on_hover` 记状态
  （`Shell.hovered`，带 on_hover 的行返回类型变 `Stateful<Div>`）；
  加载占位用官方 `Skeleton`（自带呼吸动画），进度动画用 `with_animation`
- **快捷键一律走官方 Action + `cx.bind_keys`**，不自己 `on_key_down` 判键。
  Action 都是 `#[action(namespace = qingbird_gui, no_json)]` 的 unit struct
  （**带 payload 的 Action 要 `serde::Deserialize` + `schemars::JsonSchema`，
  这条链对这个 GUI 库不划算，别去碰**）。因此 ⌘K 里那些"作用于某个仓库"的命令
  走 `on_confirm(IndexPath)` + `Shell::palette_actions` 下标表，不走 `CommandItem::action`
- **要显示快捷键就用官方 `Button::tooltip_with_action(.., &Action, None)`**：
  它自己从 keymap 查 `Kbd` 画上去，键位改了不会说谎。手写「（Ctrl T）」进文案
  或手拼 `Kbd::new(Keystroke::parse(..))` 都是会漂的假提示。
  正文里（不是按钮 tooltip）要提键位，用 `Kbd::global_binding_for_action(&Action, &Window)`，
  它需要 `&Window`——那个函数得连 `window` 一起传进去。查不到就退成不带键位的纯文字，
  宁可少给提示也不给可能已失效的键位。改键位要同时对三张表：`#[action]` 声明 /
  `bind_keys` 绑的键 / `on_action` 处理的 Action
- **`Shell::update` 之内不能 `downcast::<Shell>()` 拿回自己**（拿不到锁，`update`
  静默失败）。需要 `&mut Shell` + `&mut Window` 的动作留一次性标记（如
  `pending_reveal`），由 `Render::render` 下一帧兑现
- **在窗口自己的事件里（订阅回调 / `on_click` 监听器）也不能
  `cx.update_window(..)` 把窗口抢回来**。`App::update_window_id` 取窗口靠
  `cx.windows.get_mut(id).and_then(Option::take)`（`app.rs:2888`），
  而此刻窗口已经被 take 走了 → `update(None)` 把**整个闭包丢掉** → 返回
  `Err`，外面那个 `.ok()` 再把错误吞掉。**三处都不出声**，
  表现为"行选中了但点了没反应"（2026-10-03 实机二轮，总览表格点行不跳转）。
  统一走一次性标记 → `Render::render` 兑现
- **代数守卫的「取号」与「发号」必须成对写**：本项目四处后台扫描
  （`scan_space` / `scan_profile` / `scan_overview_activity` / `load_readme_doc`）
  **一律先 `xxx_seq += 1` 再 `let seq = xxx_seq`**。写成先取号再发号，
  守卫拿的是加之前的值，回来一比永远对不上 → 闭包**静默早退** →
  `scanning` 这类标志永远落不回去 → 画面永久停在骨架屏。
  **编译器不报、单测不红、抓帧也只是"多了一屏占位"**，只有真点下去才看得见
  （2026-10-05 说明页中英切换）。新写一处后台任务，**先去看另外三处怎么写的**——
  同一份代码里就有现成的对照答案
- **`h_flex()` 与 `v_flex()` 在交叉轴上不对称，这是最容易漏的一条**：
  `h_flex()` 装的是 `items_center()`，`v_flex()` **不装任何交叉轴对齐**（= stretch）。
  一行里放一个列容器，列容器拿到的是**内容高度并被垂直居中**，不是行高。
  壳层那一行挂了导航轨 / 主区 / 检视抽屉三样，少一个 `.items_stretch()`
  三样一起塌成"整屏上方一大块空洞"。**新写 `h_flex` 先问一句：里面的子项
  要不要占满这一行的交叉轴**——要就 `.items_stretch()`，不要就 `.items_start()`
  （别用默认的 center，两张高度差很多的卡并排会被居中到顶边错开）
- **想改官方件某个状态的底色，先查清三件事**：① 那个 token 在 `gui/theme.json`
  里**配了没有**——没配就是落基座兜底值，通常与设计板差很远（`button.active.background`
  就是这么变成一块偏暖实底的，D60）；② 调用方的 `.bg()` 能不能压过它——**能**，
  官方在**每个状态层的最后**重放调用方样式（`button.rs:733` 与 `:795` 的
  `refine_style(&instance_style)`），但 `.when(cond, ..)` 的 `cond` 不能省，
  同一份样式在常态层也会被重放；③ **悬停是另一回事**——调用方再调一次 `.hover(..)`
  盖不掉，官方已在 `render` 里注册过（`button.rs:713`），`div.rs:843` 的
  `debug_assert!(hover_style already set)` 让 **debug 构建直接 panic**（实测退出码
  101），而它在 release 下又是空操作、会「静默地看起来能用」。要改悬停只能换
  `ButtonVariant::Custom`，**那是重新设计那颗按钮**（焦点环 / 描边逻辑都变），
  别当成改个色（已开 Q12）
- **同一个 `ElementId` 在一帧里出现两次 = debug 构建 panic，且平时完全看不出来**
  （`Duplicate a11y node id`）。2026-10-05 实锤过一次，机制与查法都在下面：
  - **判据是整条 id 路径，不只是最后一段**。`GlobalElementId` 是「**有 id 的
    祖先**」串起来的链（`element.rs:230-235` 对整条链做 hash），**没有 id 的
    容器是透明的**。所以两个同名 id 的按钮放在两个不同的、但都不带 id 的父容器里，
    照样算出同一个 `NodeId` → 撞车。「它们在不同分支上」不构成辩护
  - **只有元素「有 id」且「有 role」才会生成节点**（`element.rs:368-402`）。
    官方 `Button` 两条都占（`button.rs:765` 的 `.role(..)`），所以**每颗 Button
    都算一个**；自己写的 `div()` 不设 `.role(..)` 就不会参与
  - **那棵树只在「系统/读屏软件接入」时才建**（`a11y.rs:212` 的 `active_flag`，
    由 `window.rs:1626-1647` 的平台回调置位）。没有环境变量、没有公开 API 能强开
    ——`Application::new_inaccessible` 恰恰是把它**关**掉。所以这类 bug 在开发时
    表现为**"偶发"**：平时整棵树不建，断言根本不跑；某帧有 UIA 客户端探了一下
    窗口才炸。**别把"我跑了一晚上没崩"当它不存在**
  - **要复现就自己当那个 UIA 客户端**：`scripts/uia_probe.py` 用 .NET
    `UIAutomation` 碰一下窗口即可（`AutomationElement.FromHandle(hwnd)` 后取一个
    属性）；`scripts/a11y_stress.py` 在激活之后再把整个界面走一遍，
    专抓潜伏在别的状态里的同类重复
  - **撞车了要知道是哪两个**：基座其实**已经把第一次出现那个节点的
    `element_id` 与 `source_location` 记下来了**（`element.rs:391` 的
    `record_node_info`），只是 `can_push` 撞车时没读它。想看就得把 `gpui-pre`
    vendor 一份到 `vendor/` + `[patch.crates-io]`，在 `can_push` 里补两行
    `eprintln!` 读 `self.node_info.get(&id)`——**核完把 patch 与 `vendor/` 一起删**
  - 2026-10-05 那次的实情：`inspector_reopen` 被 `repo_head.rs` 与 `sync_card`
    各调了一次，两颗都是 `Button::new("inspector-open")`，右栏一收起就同帧出现。
    **这和「同一个功能在两处各摆一个入口」是同一类错误**——用户嫌常用命令段
    重复、要删掉它的时候，同一个毛病已经以崩溃的形式存在了。
    **凡是"条件出现"的入口（收起时才露的键、悬浮才现的键），先确认一帧里只挂一次**
- **基座的 builder 方法分「哪条路会读它」和「它编译得过」两件事**，后者不代表
  前者成立。已踩过的例子：`Column::text_right()` 在 `DataTable` + `TableState`
  上是**空操作**（只有 `table::Table` 那条路读 `align`），而同一列上的
  `p_0()` 却生效；`.icon(icon_md(..))` 传进 `Button` 的**尺寸会被丢弃**
  （`button.rs:756-761` 无条件 `icon.with_size(icon_size)`，而 `icon_size`
  只在内部的 `content_style(..)` 那条路上能被外面塞值）。
  **查过不生效就连调用带注释一起删**——留着是句会骗人的话
- **官方 `TitleBar` 用不了，已自研（`shell.rs::title_bar`）——别想着把它换回去**。
  官方把**所有** `children` 塞进一个带 `window_control_area(WindowControlArea::Drag)`
  的 `h_flex`（`title_bar.rs:371-398`），Windows 侧 `handle_hit_test_msg` 把它翻成
  `HTCAPTION`（`gpui-pre-windows/src/events.rs:976`）——**非客户区命中，点击交给系统
  拖窗口，永远不派发到子元素**。表现就是子按钮画得出来但 `on_click` 永不触发。
  官方公开 API（`new`/`h`/`bg`/`border_color`/`pl`/`child`/`children`/`on_close_window`）
  **没有"这块别算拖拽区"的开关**，`WindowControls` 也是私有 enum（`:248`）。
  自研时必须接住官方的三样：
  ① **窗口控件只挂 `window_control_area`、不自己处理点击**（官方 `:220-222` 的
  `when(is_windows, ..)`，它的 `on_click` 只在 `is_linux` 臂里）。自己
  `on_click` 调 `remove_window()` 会绕过 `WM_CLOSE`，而 `tray.rs` 的托盘唤起与
  `QuitMode::Explicit` 全按"系统关窗 → 销毁窗口 → 进程留着"设计，绕过去托盘唤不醒。
  ② 图标用 `WindowMinimize`/`WindowMaximize`/`WindowRestore`/`WindowClose`
  （`title_bar.rs:148-151`，lucide 一套里就有），最大化/还原按 `window.is_maximized()`
  翻，悬停面同源 `secondary_hover`/`danger`（`:175-183`）。**别画成空白块**——
  宽度照官方 34px（`tokens::TITLE_BAR_CTRL_W`），那是系统约定。
  ③ **拖拽区要补回**（基座规范 "Preserve its drag region"）：左边这组排除，
  **右边空白仍是 `WindowControlArea::Drag`**；双击缩放补 `zoom_window()`
  （官方只在 Linux/macOS 挂，`:344-349`）。
  ⚠️ **`on_double_click` 来自 `InteractiveElementExt`（gpui-component 的扩展 trait），
  且只在 `Stateful<E>` 上有**——那一行必须先 `.id("title-bar")`，裸 `Div` 是 E0599。
  窗口状态（`is_maximized` / `window_controls`）**只能从 `&mut Window` 拿**，
  `Context<T>` 没有 `window()` 方法，所以 `title_bar` 签名带 `window`。
- **标题栏那组的顺序：图标 + 「青鸟 Git」+ 侧栏开关**（开关在**文字后面**，
  用户明确要求）。三者必须并成**同一个** `h_flex`——官方那行是 `justify_between`，
  分成两个子项会被推到两头，开关会贴到窗口控件上
- **要对齐两条并排的构件，就让偏移量成为派生量**。标题栏图标左边距
  `TITLE_BAR_PAD_L = NAV_W/2 − APP_ICON/2`（`tokens.rs`），
  改 `NAV_W` 或 `APP_ICON` 都会自动跟上——而不是"再目测调两像素"
- **位图（PNG）走 `img()` + 自建 `AssetSource`，别拖图像解码依赖**：
  gpui 自带的 `ImageAssetLoader` 就会解 PNG（`elements/img.rs:669` 的
  `image::guess_format` → `PngDecoder`，`image` 已经在它的依赖树里）。
  `gui/src/app_assets.rs::AppAssets` 只比 `AllAssets` 多认一条路径，其余全转交。
  ⚠️ `gpui_kit::component::Icon` 只会 `svg().data(..)`，**喂 PNG 是空白方块**
- **官方 `Button` 的图标档是从按钮档推出来的，两条都要查**：
  `Size::Size(v)` → 图标 `v×0.75` 且文字 16px（AGENTS 禁用的那条路）；
  其余档原样传给图标，而 icon-only 外框是 `XSmall=20 / Small=24 /
  Medium=Large=32`。**`Large` 的框和 `Medium` 一样是 32px，图标却 16→24px**——
  "框不变、图标变大"只有这一档可调

## 新 GUI 的地基与坑（gpui-kit）

- 依赖只有 `gpui-kit = "0.7"`（内部固定 `gpui = gpui-pre =0.3.7`）。组件走
  `gpui_kit::component::*`，布局用 `gpui_kit::base::{h_flex, v_flex}`。
  2026-10-01 从 `guise-ui 1.9` + `gpui 0.2.2` 换过来（guise 残留已全部清理，
  分支同步改名 `redesign/gpui`）——guise 组件面太薄，gpui-kit 有
  75+ 组件、Table/VirtualList/Input 齐全、且上游 CI 覆盖 Windows。
- **UI 开发前先读基座自带的规范**（本地 `F:\AI data\work\gpui-component\skills\`）：
  `gpui-kit/SKILL.md`（Coding Guides：conventions / recipes）与
  `gpui-kit-design-guides/SKILL.md`（Design Guides）。禁止凭训练数据猜 API。
- 启动三步不能少：`gpui_kit::init(cx)`（组件初始化，必须最先）→ 应用主题 → `gpui_kit::open_window`。
  `open_window` 内部会用 `Root` 包一层，Dialog/Sheet/通知/菜单这些浮层靠它才能用。
- 主题唯一真源是 `gui/theme.json`（iTerm2 风格的点号 token），改配色只动这一个文件。
  应用方式：`serde_json` 解析成 `ThemeSet` → `Theme::update(cx, |t| t.apply_config(&cfg))`。
  规范板的 token 名与渲染稿对不上：画布是 `#FAFAF9`、卡片是 `#FFFFFF`（以渲染稿为准）。
- `Theme` 的 120 个色 token 里**没有 `warning`**（只有 `button_warning`），
  「落后/分叉」的橙色借用 `chart_3`（已在 theme.json 定成 #DD5B00）。
- `overflow_y_scroll` / `on_click` / `id` 只在 `Stateful<E>` 上：滚动容器与可点行都要先 `.id(...)`。
- **托盘框架不带**（gpui-pre 0.3.7 全量搜索无 tray API），要自己挂 `tray-icon`；
  系统通知用 gpui-kit 的（`window.push_notification(.. .system())`），
  但未打包的 exe 必须先 `cx.set_app_identity`，否则发不出去。
- **Windows 上"关窗不退出"拦不住关闭动作，但拦得住进程退出**——两件事，
  分清了就不会得出"做不了"的错结论。查过 `gpui-pre-0.3.7` 源码的三条事实：
  ① `TitleBar::on_close_window(..)` 第一行就是 `if cfg!(target_os = "linux")`，
  Windows 上这颗钩子不会被写进去，点 X 一律 `window.remove_window()`；
  ② `App::on_window_closed(..)` 的文档写明"窗口在该回调被调用时已不可访问"——
  它是关闭**之后**的回调，拦不住；③ `remove_window()` 只是 `self.removed = true`，
  整个 gpui-pre 搜不到 `prevent_close`。**别再花时间找"阻止关闭"的那个钩子了，
  它不存在。**
  **但要的是"关掉窗口之后进程别退出"**，那只由 `QuitMode` 决定
  （`app.rs:2922-2926`）：`Default` 在非 macOS 上**等于** `LastWindowClosed`
  （= 最后一个窗口关了就退出），`Explicit` 才是"只在我调 `App::quit` 时退"。
  设法：`gpui_kit::application().with_quit_mode(gpui_kit::QuitMode::Explicit)`
  （`QuitMode` 经 `gpui_kit::*` = `gpui::*` 转出），或运行期 `cx.set_quit_mode(..)`。
  旧 Tauri 版同样是**销毁窗口**（`window.destroy()`），它要的也只是进程活着。
  ⚠️ **连带约束**：`QuitMode::Explicit` 必须与托盘**同一次改动**里一起上——
  只设它而没有托盘，窗口一关应用就变成"活着但没有任何入口"的幽灵，比直接退出更糟。
  路线表 Q11 / 阶段 12 都记着这条。
- **程序化画的图形（托盘图标之类），交付前必须渲成图亲眼看过**——
  `cargo check` 过了、exe 出来了，不等于它长得对。2026-10-03 的托盘图标
  就是这么翻的车：程序化生成后**一眼都没看**就交付，实际渲出来是个对话气泡。
  两条配套规矩：① **看的是产物本身**，不是 Python 双胞胎——双胞胎只证明算法
  想对了，证明不了 Rust 里没打错一个符号（做法：临时 `#[test]` 把 RGBA 写文件，
  渲成 PNG 看，核完删掉）；② **别按名字脑补品牌**，「青鸟 Git」的 logo 上
  **没有鸟**（是带 git 分支的立体箱子），照着名字画图腾方向从第一秒就错了。
  图形类测试的指标也要挑对：形状不对称时「重心居中」是伪判据，用**包围盒**。
  详见路线表 D39。
- **Action 监听器一律用元素级 `div().on_action(cx.listener(..))`，不要用 `Context::on_action`。
  后者第一行是 `debug_assert_paint()`，只在 `DrawPhase::Paint` 期间允许调；
  而 `Paint` 只在 `Window::draw_roots` 的 `root_element.paint(..)` 那段设上
  （`window.rs:3571`），`Shell::new` 与 `Render::render` 跑的都**不是**那个阶段。
  2026-10-03 首次实机启动就是 11 处 `cx.on_action` 挂在 `new()` 里 → **双击 exe 闪退**。
  元素级由 `Div::paint_keyboard_listeners`（`div.rs:3268`）在 paint 里统一注册，天然每帧重挂。
  **⚠️ 附带一条**：Action 派发走「焦点节点 → 逐级上冒」，一个焦点都没有时落到
  `DispatchNodeId(0)`（gpui-kit 包的那层 `Root`），挂得更深的监听器一次都收不到——
  而「刚启动还没点过任何东西」正是最该按 `Ctrl K` 的时刻。
  所以壳层根元素必须 `.track_focus(&self.focus)` 并在 `new()` 末尾真的去 `focus.focus(window, cx)`。
  **⚠️ `debug_assert!` 在 release 下是空操作**：同一个错在 debug 里是立刻 panic，
  在 release 里变成「静默地少活一帧」——**发布前至少跑一次 debug 构建**。
  另：gpui 的 `VisualTestContext` 是 macOS 专属，Windows 上写不了窗口级单测。详见路线表 D40。
- **「GUI 只能由用户双击 exe 验收」说的是"定版"要人看，不是"agent 看不见界面"。**
  画面是脚本能拿的：`PrintWindow(hwnd, hdc, 2)` 抓 PNG，配合 `SetCursorPos` +
  `mouse_event` 模拟点击、`PIL.ImageChops.difference` 求相邻两帧 diff、
  裁剪放大读小字、**临时给关键容器描彩色边再渲出来读盒子尺寸**——
  2026-10-03 实机二轮那七条缺陷（整屏塌陷 / 卡内空洞 / 柱顶标签全错 /
  列间距塌掉 / ⌘K 挤成一屏 / 点行不跳转 / 首屏图表全 0）**六条都是这么查出来的**，
  没有一条是 `cargo check` 或单测能判的。落法见路线表 D48。三条纪律：
  ① **临时描边核完当轮删掉**；② **验第三方屏要把 config 拷到 `$TEMP` 下、
  用 `$env:APPDATA` 指向副本启动**，不许动用户自己的配置；
  ⚠️ **但光在 shell 里设 `$env:APPDATA` 是不够的**——2026-10-05 实锤：
  真实 config 的 `inspector_open` 被抓帧进程从 false 写成了 true
  （`Copy-Item` 保留 mtime，副本的时间戳反而成了证据）。根因是
  `steps.py` 靠 `Popen` 继承 shell 的环境变量、**脚本自身没有隔离入口**，
  于是"记得设"这种约定迟早会漏。**现在一律设 `$env:QB_APPDATA` 指向副本**：
  `scripts/qbguard.py` 会覆盖 `APPDATA`、不设就打警告、收尾再按
  `mtime+size+sha256` 核对真实 config 并在不一致时报警（`steps.py` /
  `uia_probe.py` / `a11y_stress.py` / `startup_probe.py` 都走它，**别再各写一遍**）；
  ③ **窗口未获焦时第一击只用于激活窗口**，脚本要补一次空点击，
  否则"点了没反应"会被误判成功能坏了。
  ④ **用户 17 个真实仓库一律不许动，验收只在 `$TEMP` 现造的合成仓库上做**。
  ⚠️ 其中 **`F:\Warehouse\AIwork\Locowrk`（98 GB）尤其要注意**——
  用户明确说过**它不是本项目、是工作上的项目，更不许动**。
  这条不只是"别拿它当测试素材"：空间页的**一键清理**会对选中的仓库
  逐项删 `node_modules` / `target` / `dist`，在 Locowrk 上误点一次就是
  真删几十 GB 的构建产物、无法撤销。凡是要触发清理、批量同步、递归
  扫描体积这类**有代价的操作**，先确认目标不在这个列表里。
- **抓帧只证明"渲染出来了"，证明不了"点得动"**——这两道关要分开过。
  2026-10-05 说明页那三条交互路径（中英切换 / 正文内链切文档 / 目录锚点跳转）
  **第一版全都是坏的**，而当时 `cargo check` 零错、单测全绿、抓帧看着完全正常：
  骨架屏那处甚至还"多渲染了一屏占位"，看着像加载中。判据只能是**点下去 + 比两帧**。
  现成工具在 `scripts/`：`steps.py` 跑一串点击逐步抓帧
  （`nav` / `row` / `tab` / `click` / `wheel` / `wait` / `shot`），
  `frame.py` 做三件事（`crop` 放大读小字 / `stack` 几帧竖排 / `diff` 求改动像素与包围盒）。
  用它们时三条纪律：① **单击而不是连点两下**（连着两次 down/up 会被当成双击，
  且第一次的抬起常落在窗口还没激活的时段里）；点击前先轮询 `GetForegroundWindow`
  确认前台；② **坐标别目测**——`crop` 放大后按倍数反算，本轮有一次偏了 43px，
  点了个空处还差点当成功能坏了（靠 `diff` 只有 0.02% 才发现是没点中）；
  ③ **往返验证拿"逐像素相同"当判据**：点中文键 → 点正文里的 `English` 链接回来，
  与初始帧 `diff` = 0 像素，这才叫通。
  ⚠️ 阶段 16 又踩了一次同款：从 `crop` 放大的图上反算页签 x，目测 603、
  实测 **551**，偏了 52px，连点三次都点不中（其中一次还把别的页签点开了，
  差点当成功能坏了）。**量文字位置用逐列扫暗像素，别从放大图反算**——
  `crop` 的缩放倍数在显示时被缩放过一次，肉眼按倍数算回去必错。
  `frame.py crop <src> <x0> <y0> <x1> <y1> <倍数> <out>`（**x1/y1 是绝对坐标，不是宽高**）
- **量「启动多快」用 `scripts/startup_probe.py`**，别用「最后那张帧」当判据——
  最后那张只证明「扫完了能显示」，**证不了「打开就显示」**。它按**窗口出现**
  为基准逐帧抓（不是进程启动：GPUI 建窗要 ~0.9–1.5 秒，那段还没画，
  跟秒开无关，按进程计时会把"还没建窗"算成"慢"），每帧跟稳定态求逐像素差。
  ⚠️ 合成仓库**必须有一个文件很多的**（阶段 16 用了 2 万文件），
  否则冷热两轮都是几百毫秒就稳，差别小到抓帧看不出主次
- ⚠️ **但"把样本撑大来拉开测量窗口"有个反噬：撑大后的产物往往与真实
  场景行为相反，照它下结论会改错东西**。2026-10-05 说明页切换闪烁那一轮：
  正常大小的 README 下点完第一帧只变 **280 px（就是表头那一句时间）**，
  抓帧太快抓不到中间态，于是把英文那份 README 撑到 **45 MB** 想把时间窗拉开——
  结果抓到一个「表头已换、正文还是旧的」中间帧，看着像**表头与正文错开一帧**，
  差点照着这个去改布局。**那是超大文档的排版耗时**（`TextView::set_text`
  之后布局要占好几帧），真实场景里表头与正文是同一帧换完的。
  ⇒ **撑大样本只为"看得见"，任何关于结构 / 顺序 / 帧对齐的结论都要回正常
  样本上复验一遍**。量时间用撑大的样本，量结构用真实的样本
- **一个标志两件事，是本项目反复出现的一类缺陷**（D35 是它的通用形态）。
  2026-10-05 又抓到一处具体形态：`load_readme_doc` 借用了
  `profile.scanning`——而那个标志是**整份档案扫描**在用的，
  **四处都在读它**（骨架屏 / 表头那句 / 仓库头活跃度芯片 / 语言构成条）。
  借去用的后果是点一次中英切换会让两个毫不相干的地方一起说「统计中…」，
  读回来还会写 `false`，**哪怕整份扫描还在跑**。
  ⇒ **新开一个后台任务时，先数一遍它要写的那个标志有几个人在读**；
  读的人超过一个（且不是同一个原因），就是该拆的信号。
  ⚠️ 配套：守卫早退**之前**也要先把自己的标志落回去（与 `scan_space` 的
  `tree_loading` 同理）——过期结果不清标志，那句话会**永远**停在屏幕上
- ⚠️ **`scripts/steps.py` 送不进按键**，`keybd_event` 与 `PostMessageW` 都试过：
  窗口明明是前台（`GetForegroundWindow` 对得上）、鼠标点击也照样生效，
  但按下去应用毫无反应（`ctrl-b` 在**有侧栏**的「我的」页面上侧栏纹丝不动，
  逐像素差 **0**）。⇒ **⌘K 与全部快捷键这条路在脚本里验不到**
  （批量拉取/推送/检测只挂在 ⌘K 上，所以 `run_batch` 的落库**未经实机点击验证**，
  路线表里如实记着）。**别为此加一个 `key:` 动词**：留一个"看着能用、其实静默
  无效"的动词比没有更坏，它会让人以为那条路验过了
- **要验「拉取成功」这类结果，合成仓库必须配一个真远端**。
  `mkfixture16.py` 那三个仓库**都没有远端**（纯离线看界面），拉取一定失败——
  拿它验 `op_history` 等于验了个寂寞。做法：本地 **bare 仓库**当远端
  （`git init --bare` + `file://` 路径，不联网、不碰用户任何远端），
  ⚠️ **且必须在工作副本克隆完「之后」再往远端推一笔**——克隆时就把远端那几条
  一起拿走了，ahead/behind 都是 0，点拉取是"没有新提交可拉取"，**那是空跑**。
  要验失败那一档就再造一个「远端推一笔 + 本地也提交一笔」的分叉副本
- **凡是要靠 `&self` 才能调的显示判据，抽成纯函数再单测**（`ov_status_text`、
  `badge_kind_of`、`scanned_note`、`op_line` 都是这么来的）。Shell 建起来要 gpui
  上下文，于是这类文案**留在方法里就只能靠抓帧看**——而抓帧只能证明"渲染成那样了"。
  阶段 16 的句式就是这么漏过去的：设计时写成「上次{操作} {成功/失败} · {detail}」，
  抓帧拍出来是「上次拉取 成功 · **拉取成功** · 刚刚」（core 的成功消息本身就是
  "拉取成功"，一句话说两遍）。抽成 `op_line` 之后这就成了一条单测
- **GBK 控制台会编不出 `✓` / `→`**：`print("…未被触碰 ✓")` 抛
  `UnicodeEncodeError`，**炸在收尾那一句**，前面抓的帧全白跑而报错还指向
  一个看着无关的地方。`qbguard.py` 已在 import 时把 stdout/stderr
  `reconfigure(encoding="utf-8", errors="replace")`，**新脚本 import 它就顺手拿到**
- **「没扫过」不许写成「已同步」**（阶段 16 抓帧查实）。阶段 16 起启动只刷选中的
  那一个仓库，于是**启动那一刻其余每一行都是没扫过的**；而总览「状态」列把
  `Bucket::Unknown` 落进 `BadgeKind::None` 兜底臂 → 写「已同步」。
  **`cargo check` 零错、`bucket_of` 的单测全绿**（它判得**没错**，错的是显示那侧
  没接住 `Unknown`）——**只有抓帧看得见**。收法：判据抽成**纯函数**
  （`ov_status_text`）才单测得掉，中性态给**中性灰**不许穿行动色。
  ⚠️ 凡是新增「按某状态分档显示」的地方，先问一句**没数据的那档显示什么**
- **目测出来的「不齐」先量再改**：`scripts/rowpitch.py` 在指定横向范围内做**逐行
  像素剖面**，切出每段凸起的中心再算相邻行距。空间页树那一轮我目测以为行距有
  27/28/31 三种、打算去调，实测**统一 28.0**（开头那个 28.5 是首行被滚动视口裁掉
  造成的测量偏差）。**"看着不齐"常常是量法不对**——没量就改，等于拿目测去改一个
  本来就对的东西。量法：背景色取**区域内**的众数（取四角会取到卡片外的画布色，
  那样整片卡都被判成"有内容"，剖面连成一段）
- **白捡一份已经排好序的数据时，先确认它排的是不是「这一处要的序」**。
  空间页树的根层白捡 `scan_repo_space` 的 `dirs`（那一层的递归体积刚算过，不值得
  重扫），而那个 `dirs` 是**「谁最占地方」那个榜、按体积降序**；子层走 `list_dir`
  是名称序——于是同一棵树里根层按大小、子层按名字。`cargo check` 与单测都看不出来
  （两处各自都"对"）。收法：排序规则收成**一个**函数（core 的 `sort_entries`），
  根层再排一次；**两处各写一遍的排序规则，迟早有一处会漂**
- **结论要落进「会被读到的那一个槽」**。本项目有三个看起来都像"日志"的槽：
  `space.note`（只活在空间页状态条上）、`app_log`（**只给横跨多仓库的批量汇总**）、
  `log_of(path)`（那个仓库自己的日志，概览页的日志卡读它）。一键清理原先只写
  `space.note`，切到概览页结论就永远没了——而概览页才是用户默认会去看的地方。
  **写之前先问"谁会读它"**，别问"哪个槽名字像"
- **不可逆 / 高代价的 Windows 错误码要显式认，不能只判 `ErrorKind`**。
  「目录被另一个程序占用」是 `ERROR_SHARING_VIOLATION`(32)，Rust 归成
  `Uncategorized`，**不落进 `PermissionDenied` 那个臂**——于是最常见的那种占用恰恰
  落进兜底分支，用户看到的是 `(os error 32)` 这种等于没说的东西。
  按 `raw_os_error()` 显式认 5 / 32 / 145 三个号
- **中文字体要显式注册** TTF：Windows 家族名是本地化的，写英文名会「查不到」；
  且默认等宽字体 Consolas 不含汉字，代码区中文会掉字重。设计上要用 Noto Sans SC + Sarasa Gothic SC。
  ⚠️ **挂等宽字体的容器里不要放中文 UI 文案**：连空态提示语一起放进去就会掉字重
  （`log.rs` 的输出流空态已改回 `t.font_family` + 居中）。
- 编译坑：本机 WorkBuddy 宿主会拦截新编译 exe 创建命名管道（OS error 231），
  **个别**带 build script 的依赖在进程内编不出来。绕行办法是用计划任务脱离进程树构建
  （`Register-ScheduledTask` + `Start-ScheduledTask`，输出重定向到日志文件）。
  ⚠️ **但"带 build script 就编不出来"这个说法是错的，别拿它当挡箭牌**：
  2026-10-03 实测 `tray-icon 0.26` + `muda 0.21`（两个都带 `build.rs`）
  在进程内 `cargo build --offline` **8.57s 编过、零错**，产物 584 KB。
  遇到新的带 build script 的依赖时**先直接编一次**，别预判它会失败。

## 新 GUI 文件地图（`crates/core` + `gui/src`，当前实现）

`docs/UI-ROADMAP.md` 是**唯一施工真源**：阶段 0–11 已全部勾完，决策台账 D1–D34、
未决事项 Q1–Q10 都在那两份文档里。**改界面先读路线表对应阶段，别凭记忆改。**

core（纯业务，不认识 gpui，单测 98 个都在这）：

| 文件 | 职责 |
|---|---|
| `crates/core/src/git.rs` | git CLI 子进程封装、超时、`request_cancel` |
| `crates/core/src/net.rs` | 网络操作编排：直连→代理回退、认证注入、进度解析（`run_net_op` 唯一入口） |
| `crates/core/src/repos.rs` | 仓库增删改查、`read_status`（含 `upstream`）、README、语言、活跃度 |
| `crates/core/src/sync.rs` | 三键 + 提交推送 + `begin_user_op` 互斥锁 |
| `crates/core/src/clone.rs` | 克隆（`CloneOutcome` + 五类错误中文化） |
| `crates/core/src/history.rs` | 提交列表 / 详情 / patch + 三个纯解析函数 |
| `crates/core/src/scan_cache.rs` | **阶段 16 的缓存层**：`scan_cache`（覆盖写）+ `op_history`（追加）两张表。⚠️ **别叫它 `history.rs`**——那个名字已被上一行的提交历史占了，`write` 工具当时明写了 `overwrote existing file` 是我漏看。**它是加速器不是依赖**：库打不开/表损坏/磁盘满一律降级（读返 `None`、写返 `false`），一次都不 panic |
| `crates/core/src/space.rs` `run_cmd.rs` `fork.rs` `proxy.rs` `scheduler.rs` `store.rs` `types.rs` | 磁盘 / 一键命令 / 变基 / 代理探测 / **后台自动同步的到期判定与执行** / 配置持久化 / 共享类型 |

GUI（`gui/src`）：

| 文件 | 职责 |
|---|---|
| `shell.rs` | 窗口壳、导航轨、状态条、**全部快捷键 Action 与绑定** |
| `state.rs` | `Shell` 状态 + 纯逻辑（`bucket_of` / `dot_color` / `BatchKind` / `log_of`） |
| `tokens.rs` | **视觉档位唯一真源**（字号/圆角/间距/图标/壳层宽度/选中面遮罩）；带 2 个 `#[cfg(test)]`：遮罩落在规范板 8/255 内、三处轨内边距不许并成一个数 |
| `palette.rs` | ⌘K 命令面板（`palette_model` 同时产出条目与 `palette_actions` 下标表） |
| `detail.rs` `panels.rs` `repo_head.rs` | 详情页骨架 / 同步卡·命令卡·分叉条·图表 / 仓库头 |
| `sidebar.rs` `actions.rs` `os.rs` | 侧栏树与右键菜单 / 定位·归属·移除撤销 / 调系统 |
| `overview.rs` `inspector.rs` `log.rs` | 总览页 / 检视抽屉四页签 / 日志区 |
| `history_tab.rs` `readme_tab.rs` `space_tab.rs` `sync_tab.rs` | 四个详情页签 |
| `changes.rs` `sync_ring.rs` `widgets.rs` | 变更清单 / 同步环（自研三处之一）/ 跨屏小部件 |
| `ops.rs` | 后台编排：跑命令、三键、扫描、**批量同步**、提交并推送、同步设置 |
| `scan_cache.rs` | **阶段 16 的界面侧接线**：读缓存填 `views`/`space`/`profile`、扫完写回缓存。⚠️ 三个后台扫描一律**先灌缓存再刷**；判据是 `scanning && 还没有内容`，写成光看 `scanning` 会把刚上屏的缓存顶掉 |
| `tray.rs` | **托盘图标 + 三项菜单 + 左键唤起 / 窗口没了就重开**（Q11）。窗口选项也从这里出，首次开窗与托盘重开共用同一份 |
| `scheduler.rs` | **进程级定时器**（Q8）。`App::spawn` 不绑窗口，窗口关着也照跑；每轮 `try_begin_op` 与用户操作互斥 |
| `dialogs/` | `repo` `init` `fork` `clone` `cmds` `commit` `settings` `batch_log` 八个官方 `Dialog` |
| `app_assets.rs` | 资产源 = `AllAssets` + 一张应用位图 `gui/assets/app-icon.png`（源图 `assets/icon-source.png` 的 128px 展开）。**改图标只改 `gui/assets/` 下这一张** |

**后台自动同步调度器已经搬过来了**（`crates/core/src/scheduler.rs` + `gui/src/scheduler.rs`，
阶段 12 / Q8）：`interval_due` / `daily_is_due` / 补跑逻辑 / `last_auto_check` 维护都在，
`「同步设置」`页签不再"能配但不会跑"。改它之前先看那份文件的头注释——
**定时器必须用 `App::spawn`（拿 `&mut AsyncApp`）而不是 `Context<Shell>::spawn`**，
后者跟着窗口一起死。

## 工作规则

- git 网络操作必须走 `run_net_op`（直连→代理回退），不要另起网络调用
- 拉取永远带 `--ff-only`，分叉时报错不自动 merge。
  唯一的例外是用户在「本地与云端已分叉」卡片上**显式**点「备份并变基到云端」——
  那条路径每次都会先把"变基前"固定到 `backup/qingbird-git`，冲突自动 `rebase --abort` 复原；
  默认拉取链路与后台自动同步都不许改写历史
- 会派生 git 子进程的 Rust 测试要持 `git::GIT_TEST_LOCK` 的读锁：
  `CANCELLED` 是进程级全局标记，取消测试（持写锁）一置位就会杀掉并行跑的其他测试的 git
- 代码中不得硬编码任何凭证；用户主动填写的访问令牌 / SSH 私钥路径只存本地配置文件
  （`%APPDATA%/com.qingbird.git/config.json`），认证通过当次 git 子进程的
  `-c http.extraheader`（GitHub/Gitee 令牌）或 `GIT_SSH_COMMAND`（私钥）注入，不落盘到仓库

## 禁止事项

不要把 API Key、Token、密码、私钥写进项目文件。

不要在未确认的情况下执行删除、覆盖、迁移、发布等高风险操作。
