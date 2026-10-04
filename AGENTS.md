# AGENTS.md — qingbird-git（青鸟 Git）

## 项目说明

Windows 桌面工具：本地 Git 仓库一键同步（Tauri 2 + React + TypeScript）。

**迁移中**：界面正在从 Tauri 2 + React 整体换成 Rust 原生（GPUI）。`src/` + `src-tauri/` 是旧实现，
只作为平移来源，**不再加新功能**；新代码只进 `crates/`（纯业务）与 `gui/`（界面）。
迁移完成后 `src-tauri` 整体退役。新旧两套共用同一份配置文件
（`%APPDATA%/com.qingbird.git/config.json`），所以切换时仓库列表与命令设置不会丢。

主要目标：

- 本地仓库管理：添加过的仓库持久化记忆，下次启动自动加载
- 三键操作：检测云端更新 / 一键拉取（--ff-only）/ 一键推送
- 无感代理：git 网络操作直连失败时，自动探测本机代理（系统代理 → 上次成功缓存 → 常见端口 7897/7890/10808 等），
  仅对当次 git 子进程注入 `HTTPS_PROXY` 环境变量重试，不改系统代理、不影响其他程序

## 目录规则

- **新代码只进 `crates/`（纯业务，core 库）与 `gui/`（界面，GPUI）**。
  `src/`（React 前端）与 `src-tauri/src/`（Tauri 后端）是**只读迁移源**，
  界面需求一律在 `crates/` + `gui/` 里实现，不去改那两处
- 文档资料放在：`README.md`、本文件、`docs/UI-ROADMAP.md`（**唯一施工真源**，
  路线与决策台账 D1–D34）、`docs/UI-DESIGN.md`（设计规范）
- 临时实验放在：不要提交到仓库
- 测试文件放在：Rust 单元测试在各模块 `#[cfg(test)]` 中（core 98 个）；
  旧前端的纯逻辑测试放 `src/*.test.mjs`（node 自带 `node --test`，随旧代码一起只读）

## 常用命令

安装依赖：`pnpm install`

启动开发环境：`pnpm tauri dev`

运行测试：`cd src-tauri && cargo test`

前端测试：`pnpm test:web`（零依赖，node 自带测试运行器）
- `src/repoCmds.test.mjs` 运行命令面板的保存/预填逻辑
- `src/forkCard.test.mjs` 分叉卡片该出哪些按钮
- `src/pinnedCmds.test.mjs` 操作区自定义按钮的列表编辑规则（上限/空行/文案兜底）
- `src/commitDiff.test.mjs` 提交历史看 diff 的展示规则（行分类/行数标签/超长截断）
- `src/activity.test.mjs` 提交活跃度柱状图的数据规则（补齐窗口/本地时区日期键/柱高比例）
- `src/commands.test.mjs` ⌘K 命令面板的模糊匹配（完全/前缀/子序列三档、多词 AND、稳定排序）
- `src/hotkeys.test.mjs` 全局快捷键判定（输入框让位、无选中仓库不触发、修饰键判定、非仓库视图禁用三键）
- `src/overview.test.mjs` 总览页的数据规则（分档优先级、待处理排序、环的扇区百分比、一键批量的选行与汇总文案）

代码检查：`pnpm build`（含 tsc 类型检查）

构建项目：`pnpm tauri build`

Rust 快速检查：`cd src-tauri && cargo check`

新 GUI（gpui-kit）侧命令：

- 全量检查：`cargo check --workspace`
- 后端单测：`cargo test -p qingbird-core`（业务逻辑的测试都在这，迁移期的回归靠它）
- GUI 侧单测：`cargo test -p qingbird-gui`（目前只有 `tray.rs` 里两个纯函数测试；
  GUI 代码大部分要窗口才能测，所以这里只放不依赖 gpui 初始化的纯逻辑）
- 出 exe：`cargo build --workspace` → `target/debug/qingbird-git.exe`
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
  ③ **窗口未获焦时第一击只用于激活窗口**，脚本要补一次空点击，
  否则"点了没反应"会被误判成功能坏了。
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
| `tray.rs` | **托盘图标 + 三项菜单 + 左键唤起 / 窗口没了就重开**（Q11）。窗口选项也从这里出，首次开窗与托盘重开共用同一份 |
| `scheduler.rs` | **进程级定时器**（Q8）。`App::spawn` 不绑窗口，窗口关着也照跑；每轮 `try_begin_op` 与用户操作互斥 |
| `dialogs/` | `repo` `init` `fork` `clone` `cmds` `commit` `settings` `batch_log` 八个官方 `Dialog` |
| `app_assets.rs` | 资产源 = `AllAssets` + 一张应用位图 `gui/assets/app-icon.png`（老 `icon.ico` 里那张）。**改图标只改 `gui/assets/` 下这一张**，别再改 `src-tauri/icons/` |

**后台自动同步调度器已经搬过来了**（`crates/core/src/scheduler.rs` + `gui/src/scheduler.rs`，
阶段 12 / Q8）：`interval_due` / `daily_is_due` / 补跑逻辑 / `last_auto_check` 维护都在，
`「同步设置」`页签不再"能配但不会跑"。改它之前先看那份文件的头注释——
**定时器必须用 `App::spawn`（拿 `&mut AsyncApp`）而不是 `Context<Shell>::spawn`**，
后者跟着窗口一起死。

## 架构要点（旧实现 `src/` + `src-tauri/`，只读参考）
> ⚠️ **这一整节描述的是迁移源，不是当前界面。** 当前实现见下面「新 GUI 文件地图」。
> 留在这里是因为它记录了不少 git 行为上的坑与理由（代理回退、合并提交 diff、
> 凭证注入…），那些结论平移到 `crates/core` 时仍然成立。

- `src-tauri/src/git.rs`：git CLI 子进程封装（超时控制、输出捕获、网络错误分类、stderr 按 \r/\n 逐行流式回调供进度解析）
- `src-tauri/src/proxy.rs`：代理探测（Windows 注册表系统代理、常见端口扫描、HTTP CONNECT 实测）
- `src-tauri/src/notify.rs`：后台自动同步结果的系统通知。`toast_for` 是"结果 → 文案"的纯逻辑（`ok` 不发），
  主窗口可见且聚焦时静默；由 Rust 侧直接发，不依赖前端存活。
  开发模式下通知来源会显示成 PowerShell，装成 NSIS 包后才显示本应用名称与图标（插件在 Windows 上的已知行为）
- `src-tauri/src/commands/`：Tauri commands，按领域拆分的模块目录，门面是 `mod.rs`
  （子模块声明、`pub use` 再导出、超时常量与全局操作互斥锁 `OP_LOCK`/`begin_user_op`）
  - `types.rs` 前后端共享的 IPC 数据结构（`RepoStatus`/`ProgressEvent`/`OpResult`…）
  - `net.rs` git 网络操作编排：直连→代理回退、代理竞速、认证注入、进度解析（`run_net_op` 是唯一入口）
  - `repos.rs` 仓库增删改查、状态、初始化、变更列表、提交历史、提交活动（近 14 天柱状图）、
    语言统计、一键扫描
  - `sync.rs` 三键操作与提交推送（拉取/克隆通过 `tauri::ipc::Channel` 推送进度事件）
  - `fork.rs` 分叉恢复向导：`ForkInfo` 组装（附两边的提交列表）+ 备份并变基 + 撤销变基。
    变基序列的 runner 是注入的（`LocalRun`/`RemoteRun`），单测用临时目录里的真实仓库跑真实 git
  - `create.rs` 云端新建仓库（GitHub / Gitee API）；`clone.rs` 克隆
  - `run_cmd.rs` 一键运行命令（测试/构建/开发/安装依赖，后台运行并流式回传输出）+
    操作区自定义按钮 `set_pinned_cmds`（上限 3 个，超了报错不静默截断）
  - `history.rs` 提交历史详情：文件级变更摘要（`--numstat`）与逐文件 patch。
    合并提交走 `git diff <hash>^ <hash>`——`git show <merge>` 默认不输出 diff
  - `settings.rs` 设置与凭证；`shell.rs` 快捷打开与代理探测（`spawn_terminal` 供"去终端看 diff"复用）；
    `space.rs` 磁盘占用、大文件 Top 10、缓存清理与 `.gitignore` 建议（写入只接受白名单目录名）
  - 新增 command 时：写进对应领域模块，再到 `lib.rs` 的 `generate_handler!` 注册；
    跨模块复用的私有项用 `pub(super)`，不要为了省事把所有东西都 `pub`
- `src-tauri/src/store.rs`：配置持久化（`%APPDATA%/com.qingbird.git/config.json`，含代理、主题、
  每仓库命令/定时设置/操作区按钮）
- `src-tauri/src/lib.rs`：托盘图标与菜单（显示/扫描/退出）、关窗最小化到托盘、启动时应用持久化主题、开机自启与系统通知插件注册
- 前端状态：zustand（`src/store.ts` 只放 state + actions）；
  `src/lib/` 放与 store 解耦的纯逻辑（`theme.ts` 主题、`sidebar.ts` 侧栏宽度、`autoSync.ts` 自动同步档位文案、
  `repos.ts` 排序、`appEvents.ts` 托盘与自动同步的后台事件桥接、
  `forkGuide.ts` 分叉卡片该出哪些按钮与文案、`pinnedCmds.ts` 操作区自定义按钮的列表编辑规则、
  `commitDiff.ts` 提交历史的 patch 行分类与截断展示、
  `activity.ts` 提交活跃度柱状图的数据整形（后端只给有提交的日子，缺的天在这里补 0，
  日期键必须用本地时区不能用 `toISOString`）、
  `commands.ts` ⌘K 命令面板的模糊匹配（`matchCommands` 是泛型的，别把调用方挂在命令上的字段抹平）、
  `hotkeys.ts` 全局快捷键判定（DOM 相关的焦点判定由调用方查好当参数传进来）、
  `overview.ts` 总览页的数据整形（`bucketOf` 是全站仓库分档的唯一口径：
  侧栏徽标、⌘K 状态点、同步环、总览分布环都从这里取色与取文案，
  分叉不再单独探测——`ahead>0 && behind>0` 就是，与后端 `ForkInfo` 一致；
  一键批量的选行与收尾文案也在这里：`batchPlan` 只收同类动作的行、
  没有对应行返回 null 让按钮不出现，`batchSummary` 只在非零时才报跳过与失败）；
  `src/repoCmds.ts` 一键运行命令的保存/预填纯逻辑（配 `pnpm test:web` 回归测试）
  - 这类模块里**不要**写返回 `?? []` 的帮手函数：每次渲染新建数组，拿它当 `useEffect` 依赖会让 effect 每轮重跑。
    需要默认值时由调用方声明模块级的空数组常量（见 `RepoPanel` 的 `EMPTY_PINNED`）
- **入口一多就把开关收进 store**：`cloneOpen` / `newRepoOpen` / `initPath` / `paletteOpen` 都在 store，
  不再用 App 的局部 state 往下传 props。同理 `pullSmart` / `stashPull` 把"脏工作区→stash 确认条"
  这层判断收进 store，按钮、⌘K、快捷键三个入口才能行为一致——**别在组件里另写一份拉取逻辑**
- 前端 UI：`src/components/`（`NavRail` 最左导航轨、`RepoList` 仓库列表 + `RepoItem` 单条目、
  `RepoPanel` 主工作区、`Overview` 总览页、
  `Inspector` 右侧检视抽屉（变更/历史/定时/空间四页签）、`panel/` 下的浮层与展示组件
  （`SyncRing` 同步环、`ActivityChart` 提交活跃度柱状图、`ForkCard` 分叉恢复卡片、
  `BusyPop`/`StashBar`/`LogArea`）、`CmdPanel`/`SpacePanel`/`HistoryModal`/各对话框）
- 工作区有两屏，切换开关是 store 里的 `view`（`"repos"` / `"overview"`）：
  `App.tsx` 按它挂 `RepoPanel` 还是 `Overview`，导航轨与 ⌘K 都只调 `setView`，别在组件里各存一份
- 快捷键（判定在 `src/lib/hotkeys.ts`，监听挂在 `App.tsx`）：
  `Ctrl K` 命令面板（任何焦点下都生效）/ `Ctrl T` 当前仓库开终端 /
  `1·2·3` 对应同步卡上**从左到右**的拉取·推送·检测 / `↑↓` 上下切仓库。
  键位约定：`1/2/3` 按屏幕位置对位而非操作顺序——用户是先看见按钮再按位置的键。
  总览页上 `1/2/3` 与 `Ctrl T` 让位（那屏没有这三个键，`inReposView` 管这件事），
  `Ctrl K` 与 `↑↓` 两屏都生效。
  改键位先改 `hotkeys.ts` 再改 `App.tsx`，规则测试在 `src/hotkeys.test.mjs`
- 工作区是横向三列：`work-main`（同步卡 + 命令卡）/ `.stats-col` 176px 统计窄列
  （活跃度柱状图 + 语言构成，卡片在这列里竖排）/ `Inspector` 抽屉 320px。
  再往中间加东西会挤掉主区，要新图形统计就先考虑塞进 `.stats-col` 或抽屉。
  窗口下限 `minWidth: 1080`——导航轨 40px 是常驻列，得算进预算里：
  1080 − 轨 40 − 侧栏 300 − 统计列 176 − 抽屉 320 = 主区 244px，
  比加轨之前的 204px 还宽裕（860 / 1000 是更早的旧值）——
  **要往中间再加一列之前先按这个式子重算，别凭感觉调**
- 动作分四层（重构后的核心约定，改动界面时别破坏这个分层）：
  **执行层**只有 `RepoPanel` 的同步卡（同步环 + 拉取/推送/检测三键）；
  **命令层**是命令卡列表行（测试/构建/开发/安装依赖 + 钉住的自定义命令）；
  **查看层**全部收进 `Inspector` 抽屉与日志抽屉，不再在主面板平铺图标按钮；
  **低频层**是 `⌘K` 命令面板——「打开文件夹 / 终端 / 说明 / 切仓库」这类不常用但得存在的东西
  统一放这儿，别往主面板加图标按钮。新增功能先想清楚它属于哪一层，再决定放哪。
  导航轨不属于任何一层：它只回答"我在看哪一屏"，外加「添加 / 扫描」两颗全局动作，
  **别往里面塞功能入口**
- 跨仓库操作一律走 store 的按路径版本（`pullFor` / `pushFor`）：
  总览页点某一行不用先把仓库切过来。`pull` / `push` 只是委托给它们，
  别在组件里为总览再写一份拉取逻辑。这两个 action 的日志一律记到**目标仓库**名下
- **跨仓库批量也是前端串行循环 `pullFor` / `pushFor`，不新增后端 command**：
  代理回退、分叉识别、脏工作区判定都已经在那两条路径里，再写一份 `pull_many`
  只会让两边行为漂移。`runBatch` 的约定：遇脏工作区**跳过并记日志、继续下一个**
  （不自动 stash——那是替用户做决定），跑完一句汇总记 `_app` 全局日志
  （批次横跨多个仓库，记到"当前选中"名下会让人以为只动了一个）；
  停止标记 `batchStop` 放在 store 模块级而非 state 里——每跑完一项就整体覆写 `batch`，
  state 里的 `stopped` 会被下一次 set 抹掉
- 样式：`src/App.css` 只留文件头与 `@import`，实体在 `src/styles/*.css`；
  **`@import` 顺序即级联顺序**，新增样式文件要插在正确位置
  （`inspector.css` 放最后：工作区新结构要能覆盖 `panel.css`；
  `overview.css` 排在它前面：导航轨与总览自成一套，类名不与别处重叠）
- 图标：源图是 `assets/icon-source.png`（1024px，改图标只改这一张），
  `node scripts/gen-icons.mjs` 调 Tauri 官方 `tauri icon` 展开成 `src-tauri/icons/` 全套
  并同步一份到 `src/assets/icon.png`（标题栏 logo 与空态图用）；不要手改产物，
  也不要把 iOS / Android / icns 产物提交进来（只发 Windows NSIS）

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
