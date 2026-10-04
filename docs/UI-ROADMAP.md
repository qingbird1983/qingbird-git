# 青鸟 Git · 界面重构路线表

**这份表是施工顺序的唯一真源。** 设计的"该长什么样"在 [`UI-DESIGN.md`](./UI-DESIGN.md)，本表只管"先做什么、后做什么、做完没做完"。

## 用法

* **一次只做一个阶段。** 阶段内可拆多次提交，但跨阶段不许并行开工。
* **完成一项勾一项**：`- \[ \]` → `- \[x\]`，勾在做完且自检通过之后，不在动手之前。
* **阶段全部勾完**才算这个阶段结束，才开下一个。中途停下就停在原地，不跳阶段。
* **卡住就停**：连续两次修同一个问题还不过，停下来把现象写进本表"未决事项"，问用户拍板， 不自己换方向硬推。
* **每阶段出口必须自查**：`cargo check --workspace` 通过 + 该阶段的"出口条件"逐条过 + 顺手过一遍 `UI-DESIGN.md` 第 11 节的收尾清单（不相关的项跳过，相关的不许跳）。
* GUI 观感由用户双击 `target/debug/qingbird-git.exe` 定档，**不列为阶段阻塞项**。

回归命令（Windows，PowerShell）：

```powershell
cargo check --workspace        # 全量类型检查
cargo test -p qingbird-core    # 业务逻辑回归
cargo build --workspace        # 出 exe
```

---

## 现状基线（2026-10-02 记，供后面比对）

| 事实 | 数值 / 状态 |
|---|---|
| gui/src/shell.rs | 4056 行（其中 3747 非空）/ 154 KB 单文件，Shell 一个结构体扛约 30 个字段 |
| 页面模型 | Page { Repos, Overview } 两态；导航轨顺序是仓库在前、总览在后（与新设计相反） |
| 第三方仓库 | 走独立 profile_page，与"我的仓库"两套页面 |
| 输出区 | OutState 单份全局，不按仓库保存 → 切仓库就丢日志 |
| 总览页 | 带复选框 + 底部批量条（ov_checked / BatchRun），新设计要删 |
| core 克隆 | 不存在。只有 src-tauri/src/commands/clone.rs 的 Tauri 版 |
| core 其余缺口 | commit_push / stop_repo_cmd / add_gitignore_entries 有实现、GUI 无入口 |
| 侧栏折叠 / 拖宽 | 完全没有，侧栏是固定宽度 |
| 深色输出块 | 存在（OutState 那块深色区），要改浅色 |

**不做的事**：`src/`（React）与 `src-tauri/`（Tauri）是平移来源，本轮**只读不改**； 工作区里 `src/\*.tsx` 的未提交改动与 `scripts/`、`docs/shots/` 都不属于本表范围，别顺手卷进来。

---

## 决策台账

| # | 决策 | 理由 | 日期 |
|---|---|---|---|
| D1 | 进总览页时侧栏收起 | 总览是横向统计（四卡 + 柱状图 + 汇总表），留着 240px 侧栏没有可操作对象 | 2026-10-02 |
| D2 | Page 只留 Overview / Repo 两态，不给"第三方页"开第三个变体 | 第 2、3 个导航按钮的语义是"驱动侧栏"不是"切页面"；两态才让这个设计成立 | 2026-10-02 |
| D3 | 先切文件再动版式 | 3747 行单文件上做 12 个阶段的改动，回归无从定位；纯搬迁可单独验证（git diff 只应有移动） | 2026-10-02 |
| D4 | 日志区排在详情页之前施工 | 详情页每个区块都要往日志写东西，先修路再跑车 | 2026-10-02 |
| D5 | 克隆先补 core 再做界面 | GUI 自己拼 git 子进程会绕过 run_net_op 的代理回退与认证注入，违反工作规则 | 2026-10-02 |
| D6 | 侧栏宽度与折叠态存进 Settings（AppConfig），都带 #\[serde(default)\] | 与 Tauri 共用同一份 config.json；带 default 的新字段对旧配置是安全的 | 2026-10-02 |
| D7 | 日志状态从单份全局改为按仓库路径存 | 现有 OutState 不分仓库，切仓库就丢；两类仓库都要有日志就必须分仓库 | 2026-10-02 |
| D8 | 侧栏加过滤框，默认收起 | 仓库多了要能筛；默认收起是为了不常年占 240px 侧栏里的 32px 高度。 点导航第 2/3 个按钮时展开 | 2026-10-02 |
| D9 | 删掉标题栏全局搜索，⌘K 独占搜索职责 | 两处搜索会让人不知道该用哪个、行为还不一致；1080px 下宽度还给内容区 | 2026-10-02 |
| D10 | 检视抽屉是版面里的一列，不用官方 Sheet | 核过 component/src/sheet.rs：Sheet 是模态浮层——带遮罩、会吃掉整窗点击、劫持 Esc 与焦点。而设计第 1.1 节要的是"收起后内容区获得空间""收起不丢状态"，三条都要求它是常驻列。Sheet 留给阶段 9 的提交 diff（那种才真需要盖住界面） | 2026-10-02 |
| D11 | 状态扫描入口放状态条（「上次扫描 · 2 分钟前」，点一下重扫），不放导航轨 | 导航轨只回答"我在看哪一屏"（D2），扫描是个动作；而且这句话本身就回答了"数据多旧"，比一颗孤零零的刷新图标多带一条信息。⚠️ 顺带查出旧版一个真 bug：Windows 上 gpui-pre 本来就默认隐藏原生标题栏，旧手写标题栏既没有拖拽区也没有窗口控件，窗口是拖不动、关不掉的。换成官方 TitleBar + TitleBar::window_options() 才补上 | 2026-10-02 |
| D12 | 侧栏宽度与折叠由 h_resizable + gpui_kit::base::transition 驱动，阶段 3 不套官方 Sidebar | 三条实测出来的理由：① 官方 SidebarCollapsible::Offcanvas 走的是自己的 EffectTransition，全程没有 cx.reduce_motion() 判断（读过 component/src/sidebar/mod.rs），而设计第 4.3 节要求尊重系统减弱动效；② ResizablePanel 的 min_w 由 size_range 硬夹，Offcanvas 滑到 0 会被按回下限，"留一条空槽"消不掉，除非把 size_range 放开到 0——那拖宽就再也夹不住 200 了；③ 官方 Sidebar 的内容区是 SidebarItem 列表（菜单式），现在的仓库树（每行状态点 + 徽标 + 悬停态）不是那个形状，硬塞要写一层把 Shell 状态快照进 SidebarItem 的胶水，阶段 4 一换 SidebarMenuItem 这层就白写。等阶段 4 决定仓库行改用 SidebarMenuItem 之后，再把官方 Sidebar 套在外面 | 2026-10-02 |
| D13 | 侧栏状态点由 4px 纯色圆点改成按档位的 lucide 字形（Check / CircleDot / ArrowUp / ArrowDown / GitMerge / Circle），颜色仍走 dot_color | ① SidebarMenuItem 没有纯装饰左槽，icon 槽本身就是可交互的左导位，塞个 4px 圆点在里面既小又不成比例；② 五档（已同步 / 有未提交 / 待推送 / 落后 / 已分叉）全靠色相区分，色觉障碍下"落后"和"已分叉"会长得一模一样——字形把冗余编码进形状，不额外占宽度。⚠️ 遗留问题见 Q7：设计文档 2.3 的四色表与第 4 节的取色口径自相矛盾 | 2026-10-02 |
| D14 | 同步环**不自研**，改用官方 `component::progress::ProgressCircle` | 路线表第 2 行原写"自研（允许自研三处之一）"，动手时发现官方件正好够用：`.value()` 给弧长、`.color()` 给状态、`.child()` 开放中心那一格（放大数字 + 状态词），还自带 `transition` 补间与 `accessibility_label`。自研一版只会把弧长与取色各抄一遍，还丢掉那两样。⚠️ 尺寸要用 `.size(px(104.0))` **显式给绝对值**：`Sizable` 档会把 `Size::Size(s)` 乘 0.75（和 Button 同一个脾气），走 `with_size` 会画出 78px 的环 | 2026-10-02 |
| D15 | **更正阶段 7 的结论**：`InputState` 有 `set_value`，只读地址条改回官方 `Input` + `.readonly(true)` | 阶段 7 记的是"`InputState` 公开 API 只有 `value` / `focus_handle` / 构造时的 `placeholder`，没有 `set_value`，所以装不进静态值，只能手写"。**那条是错的**：`component::input::InputState` 是 `gpui_base::input::InputState` 的再导出，它上面有 `set_value(value, window, cx)`（`base/src/input/base/state.rs`）。当初只看了 `component/src/input/state.rs` 里那个 `AnyInputState` 门面（它确实只有那几个方法），没往下一层找。换回官方件之后白拿的是可选中复制、焦点环、readonly 的灰字重 | 2026-10-02 |
| D16 | 图标资源从 `Assets` 换成 `AllAssets` | ⚠️ **这不是优化，是修 bug**：`IconName` 枚举是**全量** 1830 个（所以 `git-branch` / `git-merge` / `users` / 品牌 Bird 都编译得过），但 `Assets` **只内嵌基座挑的 104 个默认图标**。用 `Assets` 时这些名字在运行时取不到图，界面上是一个空白方块。`AllAssets` 内嵌全部，代价只是二进制大一点。阶段 0–7 一直用的是 `Assets` | 2026-10-02 |
| D17 | 变更清单只写一份（`changes.rs`），提交对话框与检视抽屉共用 | 两处数据同源（都是 `RepoView::changes`）、格式也同源。写两份的话改一次排序规则就得记得改两处，漏一处两屏会对同一份改动给出不同答案。这里**没有用官方 `List`**：对话框里要的是"这次要提交什么"（上限 60 条 + "还有 N 个"），不是可虚拟滚动的长列表，为它建 `ListState` + `ListDelegate` Entity 反而多一份状态 | 2026-10-02 |
| D18 | 命令行的主操作区与「⋯」拆成两块，不用 `stop_propagation` | 一行既当主操作又含一个弹层触发器，就得靠 `cx.stop_propagation()` 拦点击；而弹层触发器自己的命中区与整行重叠，拦掉的时机不可控。拆成两块之后两把键各管各的，点「⋯」不会顺带把命令跑起来 | 2026-10-02 |
| D19 | 官方 `Sheet` 的 children 是**一次性元素**，提交详情挂一个 `HistorySheetView` 实体当 Render 桥 | 核过 `component/src/sheet.rs`：`Sheet` 只实现 `ParentElement`，`children: Vec<AnyElement>` 在 `render` 里直接用掉，没有 `Dialog::content` 那种每帧重画的闭包。直接 `.child(元素)` 的后果是 patch 加载完也不换。桥实体在构造时 `cx.observe(&shell, …)`，Shell 一变就重画——这是官方 API 逼出来的唯一做法，不是绕路。同理，抽屉那一列**不能**用 Sheet（D10 已经定过） | 2026-10-03 |
| D20 | 打开提交详情走 `ListDelegate::confirm`，不给 `ListItem` 挂 `on_click` | `ListState::render_list_item` 已经把「选中 + 确认（点/回车）」包在行**外面**一层 div 上，并调 `delegate.confirm(...)`。再给 `ListItem` 挂一份 `on_click` 会让选中与打开各走各的，点一下高亮要晚一拍。委托里另存一份 `selected: Option<usize>`，因为 `confirm` 回调不带下标 | 2026-10-03 |
| D21 | 检视抽屉的「历史」**不复制列表**，只给「最近一条标题 + 跳到页签」入口 | 抽屉 320px，提交列表要的是宽度。复制一份的话两处会对"哪些提交、哪一条被打开"给出不同答案——总览表格那套"委托只存索引、每帧压一次数据"的规矩就是为了防这个，复制等于把规矩作废 | 2026-10-03 |
| D22 | README 整篇搬进「说明」页签，概览不再摊预览；core 的 `ReadmePreview` / `read_readme` / `parse_readme` 连同 3 个单测一起删 | 同一份 markdown 有两个渲染入口的话，哪天改样式只改得掉一半。删掉提炼逻辑之后 `find_readme` 还在（`read_readme_text` 与 `readme_file_name` 共用它），净减 2 个单测、换 2 个新的：整篇原文 + 文件名、超长截断必须在文末说清楚 | 2026-10-03 |
| D23 | `scan_profile` 的口径改了：**README 两类仓库都读，语言与活跃度仍只有第三方跑** | 「说明」「历史」两个页签在两类仓库的页签序列里都有（路线表原文："与我的仓库同款"），读一个文件又不贵；而 `repo_languages` 要遍历 `git ls-files` 的每个文件、`commit_activity` 要扫 30 天 log，这两条在"我的"仓库的概览页上根本没有位置摆 | 2026-10-03 |
| D24 | core `RepoStatus` 补 `upstream: String` | 路线表要求第三方档案行显示「跟踪分支」。`read_status` 本来就要问 git 上游是谁（判断 `has_upstream`），那时把 stdout 留下来即可——**零新增子进程**。界面上游名字再问 git 一遍就是白花一次进程 | 2026-10-03 |
| D25 | diff 面板**不用**官方 `TextView` | `TextView` 按 markdown 解析，diff 里的 `#`（文件头）、`*`、`---`（分隔线）会被吃掉，渲染出来的东西对不上原文。diff 是逐行等宽的固定行高，用 `div` 逐行渲染并只给前缀上色（整行染色会让加长的一行变成一整条色块）。README 那边仍然用 `TextView::markdown`——那才是 markdown | 2026-10-03 |
| D26 | `repo_name_from_url` **不照搬** Tauri 版：scheme 形式必须至少三段，且拒绝 `.` / `..` | 路线表原文是"照搬 Tauri 版 + 它的 4 条单测"。动手时发现旧版对 `https://github.com/user/`（**用户主页**，不是仓库）返回 `Some("user")`，对 `https://github.com/` 返回 `Some("github.com")`——那会克隆出一个叫 `github.com` 的目录再报一句 `Repository not found`，用户完全不知道该改哪。改成：`host/owner` 那一段之后还得有东西才算仓库地址；另外 `.` / `..` 的字符都在白名单里，但它们会让目标目录指到父目录去，单独挡掉。**其余行为与旧版逐条一致**（ssh 短格式 / `ssh://` / `.git` 后缀 / 尾斜杠 / 字符白名单） | 2026-10-03 |
| D27 | `clone_repo` 成功入列表走 `repos::add_repo` + `repos::touch_repo`，不再手搓 `RepoEntry` | 路线表写的是"写 `store` 追加 `RepoEntry`"。手搓一份的话，"什么算合法仓库、重复了怎么办、落盘失败怎么报"这套判断会有两个出处——`add_repo` 里已经全有了（校验 `.git` 存在、判重、落盘），复制一遍的后果是将来加了校验规则只改一处。`touch_repo` 补上 `last_used`：刚克隆完的仓库就是"刚用过"，不置的话它按 `added_at` 排到列表末尾，而它明明是用户此刻最想看的那一个 | 2026-10-03 |
| D28 | 克隆成功后的定位**延后一帧**到 `Render::render` 里兑现（`Shell::pending_reveal`） | `reveal_repo` 要同时拿 `&mut Shell` 和 `&mut Window`，而后台任务里两者都凑不齐（core 不认识 gpui，也拿不到窗口）。更麻烦的是：后台任务本身是从 `this.update(cx, \|this, cx\| …)` 里面跑的，此刻 `Shell` 已经被借用了，在里面再 `downcast::<Shell>()` 拿回自己会拿不到锁，`update` 静默返回 `Err`——用户看到的就是"克隆成功了但侧栏一点没动"。留下一个一次性标记，下一帧 render 里 `&mut self` 与 `window` 都在手边。与 `apply_reveal` 同一个套路 | 2026-10-03 |

---

## 阶段路线

### 阶段 0 · 地基核对

不改功能，只确认"脚下的地是实的"。**这一步不过，后面每一步都是空中楼阁。**

* [x] `cargo check --workspace` 通过（35s；唯一告警是 PowerShell 重定向导致的增量锁文件清理提示，无害）
* [x] `cargo test -p qingbird-core` **60 项全绿**（11.96s）
* [x] 核实配置路径：`store.rs:171` `config_path()` 读的是 `%APPDATA%/com.qingbird.git/config.json`，与设计一致
* [x] 核实 `Settings` 可加 `sidebar_w: f32` / `sidebar_collapsed: bool`（都 `#\[serde(default)\]`）
* [x] 逐个核对 `UI-DESIGN.md` 第 9 节官方组件 —— **全部存在**（见下方"核实明细"）
* [x] 核实 `cx.reduce_motion()` 与 `cx.theme().motion_tokens()` 真实字段名
* [x] 把发现回写进 `UI-DESIGN.md`（**新发现 8 个没登记的官方件**，见下）

**核实明细**（2026-10-02，对基座源码逐个核过签名）：

| API | 真实位置 | 结论 |
|---|---|---|
| Sidebar::{new, side, collapsible, collapsed, header, footer, child, children} | component/src/sidebar/mod.rs | ✅ 与设计一致 |
| SidebarCollapsible::{Icon, Offcanvas, None} | 同上 | ✅ Offcanvas 存在，Icon 是默认值（默认不是 Offcanvas，要显式传） |
| SidebarToggleButton::{new, side, collapsed, accessibility_label, on_click} | 同上 | ✅ |
| SidebarMenuItem::{new, icon, active, default_open, click_to_toggle, click_to_open, suffix, disable, context_menu} | component/src/sidebar/menu.rs | ✅ 状态徽标走 suffix |
| h_resizable / resizable_panel | component/src/resizable.rs 转出自 base/src/resizable/ | ✅ |
| ResizablePanelGroup::with_state(&Entity<ResizableState>) | base/src/resizable/panel.rs:67 | ✅ 设计里的 .with_state(&self.split) 写法对 |
| ResizablePanel::size_range(impl Into<Range<Pixels>>) | 同上 :283 | ✅ |
| ResizableState::{sizes, resize_panel, insert_panel, remove_panel} | base/src/resizable/mod.rs:56/69/95/221 | ✅ |
| Transition::new(Duration).easing(Easing) | base/src/motion.rs | ✅ |
| transition(id, target, policy, window, cx) | 同上 | ✅ |
| MotionTokens { duration_fast, easing_move, spring_move, spring_control, … } | component/src/theme/motion.rs:8 | ✅ 设计里的写法是从官方 resizable.rs:65-66 原样抄的，一致 |
| cx.reduce_motion() | base/src/reduce_motion.rs | ✅ |
| TitleBar / StatusBar / Empty / Separator / Spinner / Kbd / Tooltip / Link | component/src/{title_bar,status_bar,empty,separator,spinner,kbd,tooltip,link}.rs | ⭐ 设计文档原本没登记，已补进第 9 节 |

**阶段 0 顺带发现的三条**（已回写 `UI-DESIGN.md`）：

1. **标题栏与状态条也不许手搓** —— 官方 `TitleBar` 自带 `WindowControls` 与拖拽区， 自己拼窗口控件是"看着差不多但细节全错"的高发区。
2. **图标提示走官方 `Tooltip`，快捷键标签走官方 `Kbd`** —— 不要用 `on_hover` 手写提示气泡， 也不要把 `"Ctrl B"` 当字符串拼。
3. **空态统一走官方 `Empty` / `EmptyHeader`** —— README 空态、侧栏无仓库、克隆无结果共用一个组件， 不要每个地方各画一个灰框。

**出口**：`UI-DESIGN.md` 第 9 节每一行都对着真实签名核过一遍，无出入。✅ 已达成

---

### 阶段 1 · 切文件（纯搬迁，零行为变化）

4058 行的 `shell.rs` 拆成 13 个模块。这不是"整理代码"的美化动作，是给后面 10 个阶段 造一个能定位回归的地基 —— 单文件上连续改 10 次，出问题根本查不出是哪一步引入的。

| 模块 | 行 | 装什么 |
|---|---:|---|
| panels.rs | 933 | 详情页各卡片：分叉条 / 仓库头操作 / 命令卡 / 磁盘卡 / 同步卡 / 图表 |
| shell.rs | 498 | 窗口壳：标题栏 / 导航轨 / 状态条 + 选中与状态刷新 + Render |
| ops.rs | 468 | 后台操作编排：跑命令 / 三键同步 / 扫磁盘 / 扫档案 / 批量 |
| detail.rs | 458 | 我的仓库工作台 + 第三方档案页 |
| overview.rs | 454 | 总览页 |
| dialogs/repo.rs | 390 | 添加 / 编辑仓库对话框 |
| state.rs | 281 | 全部状态类型与纯逻辑（bucket_of 在这里） |
| dialogs/init.rs | 259 | 初始化向导 |
| sidebar.rs | 237 | 仓库侧栏树与仓库行 |
| widgets.rs | 132 | 跨屏复用的小部件 |
| tokens.rs | 118 | 视觉档位常量与图标包装 |
| log.rs | 58 | 输出区 |
| dialogs/{mod,fork}.rs | 46 | 对话框入口与分叉提交块 |

* [x] `tokens.rs`：档位常量 + 补充灰阶 + `icon_sm` / `icon_md`
* [x] `state.rs`：全部状态类型 + `bucket_of` / `dot_color` / `rel_time` / `EMPTY_VIEW`
* [x] `widgets.rs`：分组标题 / 空态主区 / 统计卡 / 状态药丸 / 骨架条 / 竖分隔线
* [x] `shell.rs`：只留窗口壳、选中刷新与 `Render`
* [x] `sidebar.rs` / `overview.rs` / `detail.rs` / `panels.rs` / `log.rs`
* [x] `dialogs/{repo,init,fork}.rs`
* [x] `main.rs` 挂载 11 个新模块；`Shell` 改从 `state::Shell` 引用
* [x] 各模块按需裁剪 import（933 个导入名，5 轮收敛，剪完编译不过就整轮回滚）
* [x] `cargo check --workspace` 零错误零告警
* [x] `cargo test -p qingbird-core` 60 项全绿
* [x] **逐行校验**：归一化后原件与新模块各 3606 行、多重集完全相等 → 确认纯搬迁
* [x] 顺手修掉源码里一处既存瑕疵：`stat_card` 的文档注释被孤立在 `fork_commit_block` 前面（2492 行），已归位到它真正描述的函数上

**出口**：功能与阶段 0 完全一致，只是文件分开了。✅ 已达成

**纯搬迁是怎么验的**（别信"我看过了"，这是脚本判的）： 把原件与 13 个新模块都归一化——去掉 `//!` 说明、整块 `use` 语句、顶层 `impl` 外壳、 把 `pub` / `pub(crate)` / 无修饰视作等价——然后比对有效行的**多重集**。 相等即证明没有丢代码、没有多出代码、没有改逻辑。 另外每个模块的花括号配平单独查一遍（这一步抓到过一次"多带了一个 `impl` 收尾花括号"的错）。

---

### 阶段 2 · 壳层

* [x] 窗口下限 1080×640，标题栏 48px 固定
* [x] 导航轨 56px，**三个按钮、顺序固定**：总览 / 我的仓库 / 第三方仓库
* [x] 导航轨图标：总览用 `LayoutDashboard`，仓库用 `GitBranch`，第三方用 `Users`
* [x] 导航轨**只有这三个按钮 + 设置**，不放任何功能入口（D2：不塞层）
* [x] 状态条 28px：仓库数 · 上次扫描时间 · 当前选中路径
* [x] 检视抽屉骨架（**常驻 320px 列**，可收起 —— 不用 `Sheet`，见 D10）
* [x] 总览页沿用旧实现不动（表上原写"先占位"，改法见下方出入 3）
* [x] 点「总览」→ 切到总览页并收起侧栏（D1）；点「我的/第三方」→ 切到仓库页 + 展开侧栏 + 落选中（该组为空则清选中给空态）。阶段 4 再补展开分组与定位滚动
* [x] 启动进总览首屏，侧栏随之收起（设计第 1 节要求）

**出口**：导航轨三按钮顺序正确、总览能切且侧栏会收起，其余界面无回归。✅ 已达成

**这一阶段实际做的，与表上写的有三处出入（都记在这儿，别当成漏做）**：

1. **标题栏与状态条改用官方件**（阶段 0 的发现 1 要求）——`TitleBar` / `StatusBar`， 尺寸按版面表覆盖。手搓窗口控件的风险见 D11。
2. **检视抽屉没按表上写的用 `Sheet`**（D10）：`Sheet` 是模态浮层，与"收起后内容区 获得空间"直接冲突。落地成常驻 320px 列 + 标题行 + 收起按钮 + 官方 `Empty` 占位， 页签与日志留给阶段 5。
3. **总览页沿用旧实现，没换成占位**：旧总览页（四卡 + 活跃度 + 汇总表 + 批量条）是能用的， 换成空态会让阶段 2–5 这四步界面上凭空少一屏东西；阶段 6 本来就要用官方 `table::Table` / `chart::BarChart` 整体重写它。表上那行的意图（阶段 2 不做总览页的活） 达成了。

**另外两处自主决定**：

* 状态条在表上三项之外**留了「定时同步」**——它现在是仓库级真实配置（`auto_sync_mins` / `daily_time`），砍掉就是丢信息；阶段 8 的「同步设置」页签接走后这一项才撤。
* 导航轨的**设置按钮仍是空的**（和阶段 2 之前一样，没有 `on_click`）。Q4 没定它长什么样， 与其做个半成品不如不动；阶段 11 接。

---

### 阶段 3 · 侧栏：折叠 / 拖宽 / 动效

用户明确要的硬需求，三样全走官方件。

* [x] 侧栏主体换成官方 `Sidebar`，`SidebarCollapsible::Offcanvas` → **改为 D12**：宽度与折叠由 `h_resizable` + `transition` 驱动，官方 `Sidebar` 留到阶段 4
* [x] `SidebarToggleButton` 放**内容区左上角**（放侧栏内会随折叠消失，没法展开回来） → 改放**标题栏**（详见下方"出入 1"）
* [x] `h_resizable("body-split")` + `resizable_panel()` 分成「侧栏 / 主区」
* [x] 侧栏 `size_range(200..360)`，拖宽被夹住
* [x] 宽度用 `ResizableState` 管，折叠/展开走 `remove_panel(0, cx)` / `insert_panel(0, panel, cx)` + `resize_panel(0, last_w, window, cx)` → 面板的挂/摘由 `ResizablePanelGroup` 按子元素个数自动同步 （`sync_panels_count`），显式 `remove_panel` 反而会和它打架
* [x] 宽度与折叠态落盘：`Settings` 加 `sidebar_w` / `sidebar_collapsed`（D6）
* [x] 过渡动画走 `gpui_kit::base::transition`，**id 是稳定字符串**（不是每帧新建）
* [x] `cx.reduce_motion()` 为真时直接跳终值，不补间
* [x] 过渡只用在宽度/折叠/展开/浮层出现；hover、按下不给过渡
* [x] 折叠后布局**不留空槽位**（面板宽度到 0 就整个摘掉）
* [x] 快捷键 `Ctrl B` 折叠/展开侧栏

**出口**：能拖宽、能折叠能展开、宽度跨启动保留、减弱动效下不补间、折叠无空槽。✅ 已达成

**core 侧**：给 `Settings` 加了 `sidebar_w` / `sidebar_collapsed`（都带 serde 默认值） 与 `set_sidebar_layout` / `clamp_sidebar_w`，`DEFAULT_SIDEBAR_W` / `SIDEBAR_W_RANGE` 是宽度的唯一真源，界面只管夹范围。单测 60 → **65**。

> ⚠️ 写这层时踩到一个值得记的坑：`sidebar_w` **不能**用裸 `#\[serde(default)\]`。 那取的是 `f32::default()`（0.0）而不是 `Settings::default()`（240.0），旧配置读出来 就是 0，界面得再兜一层才不至于把侧栏夹成最窄。必须写成 `#\[serde(default = "default_sidebar_w")\]`。已有单测盯着这条。

**这一阶段与表上写的出入**：

1. **`SidebarToggleButton` 放标题栏，不放内容区左上角**。内容区归三屏各自的头部管， 放进去要么三处各插一遍，要么给所有页面让出一条常驻行；标题栏本来就是"窗口级开关" 的位置，而且**折叠后它不会跟着消失**——设计稿那条顾虑（"没法展开回来"）照样满足。
2. **"折叠后不留空槽位"靠摘面板，不靠 Offcanvas**。`min_w` 由 `size_range` 硬夹， 面板停在 0 宽时下限会把面板按回 200px。所以下限在**补间中或目标为 0** 时放开到 0， 平时照常夹 200–360。顺带把面板整个摘掉，主区上那个拖拽把手也不会留在 x=0。
3. **进总览不再改 `sidebar_collapsed`**。总览没有侧栏是"那屏没有侧栏"，不是"用户折了侧栏"。 两者混成一个值的话，用户回仓库页会发现刚摆好的侧栏不见了。点导航第 2/3 个按钮 仍然会展开侧栏（那才是"我要看仓库"）。

---

### 阶段 4 · 侧栏树与右键菜单

* [x] 两组固定顺序：**我的仓库 → 第三方仓库**，用 `SidebarMenuItem` → **不用 `SidebarMenu` 包层**（详见下方"出入 1"）
* [x] 分组标题可点开合；**分组标题不承担选中语义** → 开合态改由 `Shell.group_open` 管，不走 `.default_open` / `.click_to_toggle`（详见出入 2）
* [x] 仓库行三列：状态图标 / 仓库名（省略号截断）/ 右侧状态徽标 → 状态点由 4px 纯色圆点改成**按档位的 lucide 字形**（详见出入 3 / D13）
* [x] 状态图标与徽标取色**统一走 `bucket_of`**，不另写判定
* [x] 仓库行挂 `SidebarMenuItem::context_menu`，**不自拼浮层**
* [x] 我的仓库菜单：打开 / 提交并推送… / 移到第三方仓库 / 打开文件夹 / 在终端中打开 / 复制远程地址 / 重命名… / 编辑… / 移除
* [x] 第三方菜单：同上但**去掉「提交并推送」**，加上「在云端打开」
* [x] 归属变更**直接生效 + `push_notification` 带撤销**，不弹确认框
* [x] 移除仓库走"执行 + 通知撤销"，不用确认框
* [x] 侧栏底部：分隔线 + 两个并排 `default` 按钮「＋ 添加仓库」「⤓ 克隆仓库…」 → 克隆按钮本阶段**明确置灰**，不是给个点了没反应的入口（阶段 10 接）
* [x] 侧栏头：标题 + 仓库数 + 筛选开关（D8，默认收起，官方 `Input::small()` + `cleanable`）
* [x] 导航第 2/3 个按钮：展开侧栏 + 展开对应分组 + 定位滚动； **若当前在总览页则同时切到仓库页并选中该组第一个**（D2 的例外分支）
* [x] 从总览表格点某行 → 自动展开侧栏并定位该行
* [x] `gui/src/os.rs`：`open_folder` / `open_terminal` / `open_url` / `copy_text`
* [x] core：`browse_url_from_remote` 从 `src-tauri` 搬进 `net.rs`（GUI 要用， 但 core 才该是它的家）。单测 65 → **69**

**出口**：树、状态图标、右键菜单、归属变更与撤销、底部两按钮全部可用，键盘能走完。✅ 已达成

**新增文件**：`gui/src/actions.rs`（选中 / 定位 / 归属 / 移除撤销）、`gui/src/os.rs` （调 explorer / 终端 / 浏览器 / 剪贴板）。这两个文件是阶段 4 拆出来的，理由是 `sidebar.rs` 只该管"行长什么样"，不该管"点了之后改哪份状态"。

**这一阶段与表上写的出入**：

1. **不用 `SidebarMenu` 包层，仓库行平铺在滚动容器里**。`SidebarMenu` 本身只是个 `v_flex`，但它会把行藏进子容器——滚动容器的直接子元素变成两个菜单， `ScrollHandle::bounds_for_item` 换算不出"这一行是第几个"，定位滚动直接废掉。 平铺之后"索引 = 渲染顺序"这条约定才成立（`sidebar.rs` 写 `ix`，`actions.rs` 用同一套算法）。
2. **分组标题不带子项**。`SidebarMenuItem` 的子菜单开合态存在内部的 `window.use_keyed_state` 里，外部拿不到——而导航按钮第 2/3 个与总览点行 **必须能从外面把某一组打开**。所以标题只切 `Shell.group_open`，行由我们自己铺。
3. **状态点改成字形而不是色点**（D13）。`SidebarMenuItem` 没有纯装饰左槽， 而 4px 圆点在 240px 侧栏里几乎看不见；更要紧的是五档全靠色相区分， 色觉障碍下"落后"与"已分叉"会长得一模一样。现在五档各有字形 （Check / CircleDot / ArrowUp / ArrowDown / GitMerge / Circle），颜色仍走 `dot_color`。
4. **「提交并推送…」与「克隆仓库」置灰而不是弹"未接线"提示**。弹提示等于承认 这个功能存在；置灰 + tooltip 说明是更诚实的"还没做"，也不会让人反复点。
5. **「重命名…」与「编辑…」共用编辑对话框**，区别是重命名会**聚焦到「显示名」输入框**。 侧栏那个「添加仓库」按钮是 `open_add_dlg`（已有），不是新入口。
6. **移除撤销要写回整条记录**。撤销走 `repos::add_repo` + `restore_entry`： `add_repo` 只造一条最小记录，直接用它撤销等于把用户攒了几个月的别名、 自定义命令、定时设置全清空。
7. **总览整行可点了，但行内控件要显式吞事件**。gpui 的 click 在冒泡阶段派发、 官方 `Button` **不**调 `stop_propagation`，所以复选框格与操作格各挂了一个 `on_click(|\_,\_,cx| cx.stop_propagation())`——否则勾一下勾选框会连带把行也点开。 行的悬停底色沿用旧实现（已有 `Shell.hovered`），阶段 6 换官方 `Table` 时一并交给组件管。

> ⚠️ `PopupMenu::new` 是 `pub(crate)`，**外部自建不了**。右键菜单只能用 `SidebarMenuItem::context_menu(f)` 传进来的那个实例，加项用 `PopupMenu::item`。 另外 `Notification::action` 的闭包签名是 `Fn(&mut Self, &mut Window, &mut Context<Self>) -> Button`——**必须返回一个 `Button`**， 点击逻辑自己挂 `.on_click`；外层是 `Fn` 不是 `FnOnce`，闭包里要用外面的数据得先 clone。

---

### 阶段 5 · 日志与状态反馈

**这一阶段优先于详情页。** 详情页每个区块都要往日志写东西。

> ⚠️ **2026-10-03 补记**：这一节的勾选一直漏着——工作是在阶段 1（拆分）、
> 2（抽屉）、3、8 里分散做完的，阶段 5 之后没人回来把这一节的框打上。
> 阶段 11 逐条核实过代码，结论是**十项全部达成**，现已补勾；
> 唯一没做的是「总览页的汇总文案」（那条在本节里没列，在阶段 6 补上）。
> 补勾时顺手核了三处：
> * 「输出流浅色底」= `log.rs` 的 `bg(t.colors.muted)` + 1px `border`，
>   深色代码块已经没有了；
> * 「仓库级状态徽标」的四种形态在 `sidebar.rs::repo_row` 里都还在
>   （`Badge::Spinner` / 「失败」 / 「走代理」 / 分档胶囊），优先级顺序没变；
> * 「两类仓库同一个组件」= `log_panel` 不看 `is_third_party`。

* [x] `OutState` 从单份全局改为**按仓库路径存**（D7：`HashMap<String, LogState>`，跟随选中仓库）
* [x] 日志区搬到**检视抽屉第一页签**（默认页签），不平铺在主区
* [x] 日志区结构：状态行 / 进度条 / 输出流 / 结论行 / 工具行
* [x] 输出流改**浅色底**（`muted.background` + 1px `border` + 等宽字体）， 彻底去掉深色代码块
* [x] 错误行用 `danger.foreground` **前缀标色**，不整段染红
* [x] 进度：拉取/检测有 `percent` 用确定态，**跑命令无 `percent` 用不定态**（不造假百分比）
* [x] 工具行三个按钮**常驻不藏悬停**：复制 / 清空 / 在终端打开
* [x] 补 `run_cmd::stop_repo_cmd` 入口 + `Ctrl .` 停止
* [x] 补 `git::request_cancel` 接线（停止按钮）
* [x] 侧栏仓库行加**仓库级状态徽标**：进行中（转圈）/ 成功 / 失败 / 走代理 （「走代理」来自 `OpResult.used_proxy`，值得单列）
* [x] 两类仓库日志**同一个组件、同一套状态机**，只改"跑什么会往里写"

**出口**：每个耗时操作点下去状态立刻可见、结论可读、可停止、可复制，换仓库不丢日志。✅ 已达成

**阶段 11 补的一条**：批量同步也走同一套日志管道——每一项的结果写进**那个仓库
自己的**日志（侧栏那一格同时转圈），跨仓库的汇总才进 `Shell::app_log`。

---

### 阶段 6 · 总览页

* [x] 页头：标题「总览」+ 副行「N 个仓库 · 我的 X · 第三方 Y」
* [x] 四张状态卡：已同步 / 落后云端 / 有本地提交 / 已分叉，数字用 `FS_NUM`， **不加边框阴影**，只靠背景与间距分组
      → 档位也随 Q7 一起改了：原来的「最新 / 落后云端 / 已分叉 / 未提交」
      换成设计文档 3.1 的「已同步 / 落后云端 / 有本地提交 / 已分叉」——
      「未提交」是独立维度，不该占一格统计卡的位置
* [x] 提交活跃度柱状图：官方 `chart::BarChart`，近 14 天
* [x] 日期键用**本地时区**，不用 `toISOString`（会串日）——补齐窗口属允许自研的三处之一
* [x] 仓库汇总表：官方 `table::{Table, TableState}`， 列 = 名称 / 归属 / 分支 / 状态 / 落后 / 待推送 / 未提交
      → 用的是 `DataTable` + `TableState`（带状态的官方件），不是纯展示的 `table::Table`：
      后者没有排序、没有行选中事件，也就没法接"点行跳仓库"
* [x] 点表格行 = 展开侧栏 + 选中该仓库 + 进详情页
      → 走 `TableEvent::SelectRow` 订阅 → `reveal_repo`，与侧栏行点击、右键「打开」、
      ⌘K 切仓库是同一个入口
* [x] **删掉复选框 `ov_checked` 与底部批量条 `BatchRun` UI**（批量改由 ⌘K 承载，阶段 11）
* [x] 总览页不显示检视抽屉（没有"当前仓库"这个对象）
      → 阶段 2 已如此（`finish_frame` 里判 `page == Page::Repos`）

**出口**：总览只做统计不做批量，点行能跳到对应仓库，旧批量 UI 已从界面上消失。✅ 已达成

**core 侧**：加 `commit_activity_all(paths, days)` 与纯函数 `merge_activity`。
合并规则（同名相加、去重、按日期倒序）**不碰 git**，所以拆成纯函数单独测——
混在跑 git 的那个函数里就只能靠真仓库测。补 3 个单测，69 → **72**。

**这一阶段与表上写的出入**：

1. **批量链路留着没删**。`run_batch` / `BatchRun` / `Shell::batch` 三样都标了
   `#[allow(dead_code)]` 并写明原因。串行循环、遇脏工作区跳过、汇总文案这几条
   都已经跑通验过，阶段 11 的 ⌘K 接线直接接上即可；现在删掉等于把一段验过的
   逻辑删了再在下一个阶段重写一遍。
2. **「状态」列的文案密度比侧栏低**：表格里「待推送」「落后」不带数字，
   因为旁边就有独立的数字列；侧栏那一格只有 200px，必须带数字。
   但"落在哪一档、什么颜色"只有一份实现（`sidebar::badge_tone`），
   两边都调它——以前两边各写一遍映射，总览写「落后 N」侧栏写「落后」，
   同一仓库两屏两种说法，一眼就让人怀疑其中一屏数据不对。
3. **活跃度是进总览页才扫的**，不是启动就扫。每个仓库一条 `git log`，
   仓库多了不便宜，而用户多半只是路过看一眼。切走不扫，回来时用已缓存的。
4. **`ov_rows` 每帧重算**。数据量就是仓库数（几十），压一遍是微秒级；
   而"只在变化时更新"要自己判断什么算变化——判断错了就是表格少一行，
   而少一行是那种用户不会报、只会自己绕开的 bug。
5. **顺带删掉三个因此没人调的东西**：`Shell::hovered`（官方 `DataTable` 自带行悬停）、
   `widgets::group_header`、`widgets::row_pill`（旧手搓表格的遗产）。
   删的时候误伤了 `empty_main`，补回来了——已用编译验证。

> ⚠️ 官方件的三个脾气，记下来免得再踩：
> ① `DataTable` 与 `BarChart` **都没有实现 `Styled`**，调不了 `.w_full()` / `.size_full()`。
>    `BarChart` 好在底层的 `PlotElement::request_layout` 写死了 `Size::full()`，
>    图表自己填满父容器，外层套个 `flex_1().min_h_0()` 的 div 就行。
>    `DataTable` 得更粗一点，撑高与描边都由外层 div 负责。
> ② `DataTable` **没有 `accessibility_label`**（那是纯展示的 `table::Table` 才有的）。
>    裸 `div` 也没有，所以这块没硬凑可访问名——页头那行副标题已经把表格在讲什么说清了。
> ③ `div().id(x)` 的 builder 在 **`InteractiveElement`** 上，**不是** `Element`
>    （`Element::id()` 是 0 参数的取值器）。引错 trait 会让 `.id()` 变成"不接受参数"。
> ④ `TableDelegate::render_td` 的签名是 `-> impl IntoElement`，实现里收窄成
>    `AnyElement` 是安全的，但要加 `#[allow(refining_impl_trait_internal)]`，
>    否则每次编译飘一条告警。

---

### 阶段 7 · 详情页共用骨架

* [x] 仓库头常驻（不随页签切换）：仓库名 + 归属徽标 + 路径 · 分支 · 最后提交
      → 「最后提交」是本阶段补的数据源：`refresh_status` 每仓库多跑一条
      `git log -1 --format=%ct`（只读 HEAD 不走历史，O(1)），落进 `RepoView`
* [x] 仓库头右侧**并排四个图标按钮**：打开文件夹 / 开终端 / 云端主页 / 复制地址
* [x] 四个都是纯图标按钮：显式 `.text_color(tier3(t))` + tooltip + `accessibility_label`
* [x] 「云端主页」是**全应用唯一**允许用 Link 样式的地方；仅 `has_upstream` 为真时显示
      → 门槛再收紧一步：`browse_url_from_remote(remote)` 得得出网址才算
      （详见下方"出入 1"）
* [x] 远程地址放只读 `Input::new(&state).small()` + 复制图标（不另开控件）
      → **官方 `InputState` 装不进静态值，改手写只读地址条**（详见出入 2）
* [x] 官方 `tab::{Tab, TabBar}`：概览（默认）/ 说明 / 历史 / 空间 / 同步设置
* [x] 页签**按归属裁剪**：第三方不出现「空间」「同步设置」
* [x] 右侧检视抽屉：日志（默认）/ 变更 / 历史 / 空间
      → 页签壳与默认页签阶段 5 已做；后三个页签的**内容**排在阶段 8/9
* [x] `Ctrl I` 折叠/展开检视抽屉（阶段 5 已接）
* [x] 仓库头图标与右键菜单的动词、启用态、结果**三处完全一致**
      → 四个入口都走 `os::` 那四个函数；「在云端打开」从"第三方专属"改成
      "有 upstream 就能点"（详见出入 1）

**出口**：两类仓库共用同一套骨架与页签，裁剪规则正确，四个跳转入口都在。✅ 已达成

**core 侧**：无改动。「最后提交」复用已有的 `repos::last_commit_ts`。

**新增文件**：`gui/src/repo_head.rs`。仓库头是**两类仓库共用的一段**，
独立成文件而不是塞进 `detail.rs`——`detail.rs` 已经同时装着骨架、我的仓库内容、
第三方内容三件事，再塞第四件就没人找得到路了。

**这一阶段与表上写的出入**：

1. **「在云端打开」不再只给第三方**。阶段 4 我按"我的仓库我已经在本地干活了"
   把它做成了第三方专属，但这一阶段要求"仓库头图标与右键菜单三处完全一致"——
   头是两类共用的，菜单却一边有一边没有，这个不一致会直接暴露在界面上。
   改成**两边都给**，门槛是 `has_upstream`（"真的能打开一个网页"），
   这比按归属分更站得住脚。**这推翻了阶段 4 的决定，如实记在这里。**
2. **只读远程地址条是手写的，不是官方 `Input`**。查过基座：
   `InputState` 的公开 API 只有 `value(&cx)` / `focus_handle(&cx)` 与构造时的
   `placeholder`，**没有 `set_value` / `reset` / `clear`**——一个 `InputState`
   建出来之后内容就永远是空的。所以路线表上"远程地址放只读 `Input::new(&state)`"
   这条在当前基座上**落不了地**，只能照 `Input::small()` 的尺寸（24px 高 /
   8px 内边距 / `R_SM`）手写一条。**这条是官方件做不到才手搓的，不是偷懒。**
   地址条上也没再放复制图标：仓库头第四个按钮就是"复制远程地址"，
   同一串地址摆两个复制入口是多余的。
3. **页签下标是全局的，不跟仓库走**。切页签不是切仓库；跟着仓库走的话
   多仓库用户每换一个都要点回「概览」。但切仓库时会把下标**夹回范围**——
   从第三方（3 个页签）切到我的（5 个）时，下标可能还停在被裁掉的位置上。
4. **同步动作从头部挪进了内容区**，并且按归属裁剪：第三方只给「检测更新」
   「拉取更新」，我的仓库多一把「推送」。阶段 8 的同步卡会把这三键嵌进卡里，
   这一步只保证动作一个不少地留着。
5. **顺带清掉三样旧物**：`panels.rs::head_actions`（里面四个图标按钮**根本没有
   `on_click`**，是装饰性空壳，头被抽走后整块成了孤儿）、`ProfileState::last_commit_ts`
   （与 `RepoView::last_commit_ts` 是同一份事实存两份，正是"两份必然有一帧对不上"）、
   第三方页那行 chips（里面的单仓库 30 天活跃度图搬进了 README 卡）。

> ⚠️ 一个可能复发的坑：`cx.listener` 的闭包是 **4 个参数**
> `(view, context, window, cx)`，不是 3 个。少写一个 `_` 编译会直接报
> "expected 4 arguments"。

---

### 阶段 8 · 我的仓库内容

* [x] 同步卡（执行层，视觉最重）：同步环 + 拉取 / 推送 / 检测三键，**位置固定**
* [x] 同步环 ~~自研（允许自研三处之一）~~ → **改用官方 `ProgressCircle`**（补 D14），外框与状态取色走 `bucket_of`
* [x] 落后/待推送时对应按钮高亮；分叉时同步环转橙
* [x] 「提交并推送…」在同步卡**下方一行**（不塞进三键行）
* [x] 提交并推送 `Dialog`：变更摘要 + 提交信息（空则禁用按钮）+ ☐提交后拉取
      → 按钮不做"空则禁用"，而是**点下去就地报一行红字**（`dialogs/commit.rs`）。
      Dialog 的 `footer` 是**一次性元素**（官方签名 `impl IntoElement`，不随帧重画），
      它的禁用态没有地方可以挂；非要用禁用就得自己每帧重建 footer，那会丢按钮焦点
* [x] 提交信息预填上次提交标题（`git log -1`）—— `InputState::set_value` 真的存在（补 D15）
* [x] 接线 `sync::commit_push_with`（走同一条 `run_net_op`，不另起链路）
* [x] 分叉提示条：仅 `ahead>0 && behind>0` 出现， 提供「备份并变基到云端」「撤销变基」「只看差异」
* [x] 命令卡：测试 / 构建 / 开发 / 安装依赖 + 钉住的自定义命令（上限 3）， 每行一个主操作 + 一个溢出菜单
* [x] 磁盘摘要卡：只给一行「占用 1.6 GB · 可清理 820 MB」+ 一个「查看」按钮
* [x] 「空间」页签：总占用 / Top 10 大文件 / 缓存目录逐项清理 / `.gitignore` 建议
* [x] 接线 `space::add_gitignore_entries`（逐项「加入 .gitignore」按钮）
* [x] 「同步设置」页签：定时自动同步 + 检测到更新后自动拉取
* [x] 附带：检视抽屉「变更」页签内容（阶段 7 把它的内容排在 8/9，见下面第 7 条）
* [x] 附带：命令编辑对话框（命令卡头那个「编辑」按钮阶段 5 之后一直是装饰空壳，见下面第 5 条）

**出口**：我的仓库能完整干活：拉/推/检测/提交/变基/跑命令/清磁盘/设定时， 每步都有状态与日志。

> ⚠️ **出口只到"代码与数据通路"这一层**。窗口能不能开、五个页签的观感、
> 对话框的焦点与滚动、逐项清理的按钮点下去对不对，**没有实机跑过**——
> GUI 只能由用户双击 exe 验收。

**本阶段另外做的几件事**（表上没列，但不做就留了坑）：

1. **状态扫描顺手带出变更清单**。原来扫描跑的是 `repos::changed_files`，
   它和新的 `changed_entries` **跑的是同一条 `git status --porcelain`**，
   前者只把输出数了一下行。直接换过去：子进程数量不变，多出来一份文件名列表，
   提交对话框与检视抽屉的「变更」页签都从这一份取。
2. **`git_card` 变成 `changes_card`**。它原来自己带了一对「拉取 / 推送」按钮——
   阶段 8 的同步卡里已经有同一对键了。同一动作在一屏里出现两次，
   两处都会各自改状态机，迟早对不上，所以那对键从它身上删了，
   改成「未提交 N 个 / 已暂存 / 未暂存 / 未跟踪 / 最后提交」+ 一个「查看变更」。
3. **大磁盘卡与「一键清理」一起退休**。概览页换成一行摘要 + 「查看」，
   完整面板进「空间」页签，逐项清理。`clean_all`（一次删光所有缓存目录）
   随之删除：一次删掉三四个目录意味着用户要重新构建才能继续干活，
   而逐项点的话他每一步都知道自己干了什么。
4. **命令卡头的「编辑」按钮接上了**。它从阶段 5 之后就是一个**没有 `on_click`
   的装饰空壳**——core 的 `set_repo_cmds` / `set_pinned_cmds` 在 3.6 表里写着
   "已有入口"，其实一直没有。本阶段补上命令编辑对话框（十个字段：
   四个固定槽位 + 三个钉住命令的「文案 / 命令」成对）。
   钉住命令的**文案必须单独一格**：只存命令让 core 兜底的话，
   已经存过自定义文案的仓库会被悄悄改回"命令前两个词"。
5. **检视抽屉的「变更」页签**。阶段 7 把它的内容排在"阶段 8/9"，
   本阶段接上。「历史」「空间」两格**继续留空**：它们与详情页的同名页签
   数据完全同源，做两份必然漂移；等阶段 9 的「历史」页签落地，
   抽屉这两格改成"跳到对应页签"的入口更合适（这条留给阶段 11 收口时定）。
6. **定时同步的档位与规范化**。core 加 `store::AutoSync` 枚举
   （`Off` / `Every(n)` / `Daily("HH:MM")`）+ 纯函数 `normalize_auto_sync` + 5 个单测。
   界面只给 7 个档位、**间隔下限 5 分钟**：自由输入能填出 `0`（等于关）
   或 3 分钟（每 20 秒派生一次 git，用户会以为程序坏了）。
   core 单测 78 → **83**。
7. **地址条从手写换回官方 `Input`**（更正阶段 7 的结论，见 D15）。

**这一阶段删掉的东西**：`sync_actions`（阶段 7 那一行裸按钮，被同步卡替掉）、
`sync_panel`（第三方那一列，被同一个 `sync_card` 按归属裁剪替掉）、
大 `space_card`、`git_card`、`clean_all`。

**这一阶段新出的文件**：`sync_ring.rs`（同步环）、`changes.rs`（变更清单，
提交对话框与检视抽屉共用）、`space_tab.rs`（「空间」页签）、
`sync_tab.rs`（「同步设置」页签）、`dialogs/commit.rs`、`dialogs/cmds.rs`。

---

### 阶段 9 · 第三方仓库内容

* [x] **不新开页面**，在阶段 7 的骨架上按裁剪表填内容
* [x] 同步卡**只有「拉取」「检测」两键**（硬边界）
* [x] 同步卡下半区一行只读明细：当前分支 / 跟踪分支 / 是否落后 / 最后同步时间
      → 官方 `component::description_list::DescriptionList`，4 列一行。
      `label_width` 给 **64px**（默认 120px 在 4 列下会把值挤没）。
      「跟踪分支」的值是新加的 `RepoStatus.upstream`（补 D24，零新增子进程）
* [x] 去掉命令卡、分叉条、提交并推送
* [x] 「说明」页签：`text::TextView::markdown` 渲染 README，保留标题层级，页签内滚动
* [x] 无 README 时给**可操作空态**：「这个仓库没有说明文档」+「在文件管理器中查看」
* [x] 「历史」页签：最近提交列表（官方 `List`）→ 点行进 `Sheet` 看 diff
* [x] 「说明」「历史」与我的仓库**同款**，不另做一套
* [x] 确认：第三方**有日志区**（上一版误删过，别再删）

**出口**：第三方只有两键 + 只读档案信息，越界项一个都没有，日志照常。
> ⚠️ **出口只到"代码与数据通路"这一层**，没有实机跑过——GUI 只能由用户双击 exe 验收。

**本阶段另外做的几件事**（表上没列，但不做就留了坑）：

1. **新增 core `history.rs`**，把提交历史整条链收进内核：GUI 不自己拼 git 子进程。
   `commit_log` / `commit_detail` / `commit_patch` + 三个纯函数
   （`safe_hash` / `parse_log` / `parse_numstat`），10 个单测，core 83 → **93**。
   ⚠️ 合并提交走 `git diff <hash>^ <hash>`——`git show <merge>` 默认不输出 diff，
   这条是从旧 `src-tauri/commands/history.rs` 平移来的，漏了会看到空的文件列表。
   `safe_hash` 只收十六进制 4–64 位：hash 是从 git 拿到之后要拼进
   `Command` 参数的，不校验就是一条命令注入的口子。
2. **历史只存一份在 `Shell::history`，不进 `RepoView`**。`git log` 要派生子进程，
   塞进每轮都跑的状态扫描里等于让所有仓库在后台无条件付这份代价；
   所以只在**打开「历史」页签那一刻**扫（`sync_detail_tab`），切走再切回命中缓存不重扫。
3. **`RepoStatus` 补 `upstream`**（D24）。不加的话第三方档案行的「跟踪分支」
   只能写"有上游"，等于把 D24 那个零成本的改进扔了。
4. **启动首帧补上地址条与同步设置草稿的初始化**。`Shell::new` 里
   `selected` 是构造时就定下的，**从没走过 `select`**——而灌地址条（`set_value`
   要活的 `&mut Window`）与灌草稿这两步只挂在 `select` 上。不补的话第一屏的
   地址条停在占位符上、「同步设置」显示空草稿，而用户没做过任何操作，
   会以为是坏了。移仓后选中项换人同理，`remove_repo_at` 因此也收一个 `&mut Window`。
5. **顺手修掉两个会让人以为程序坏了的时序问题**：
   ① `open_commit` 原本自己把 `loading` 置真，而 `load_commit_detail` 开头有一道
   "已经在加载就返回"的守卫——自己先置真等于把那道守卫变成死结，Sheet 永远停在
   骨架条上；② `open_detail_tab` 在命中缓存时（`scan_history` 没改任何东西，
   也就没 `notify`）页签下标改了却不重画。两者都是"看起来接好了、点下去没反应"那一类。
6. **README 原文存 `SharedString` 而不是 `String`**。官方 `TextView::markdown` 收的是
   `impl Into<SharedString>`，存 `String` 就得在 render 里克隆一份——core 那边的
   上限是 12 万字，每帧克隆一次光这一处就够吃掉一帧的预算。

**这一阶段删掉的东西**：
1. **`sync_panel`（旧版第三方面板，380 宽）**——⚠️ 阶段 8 的记录写的是"已删"，
   **实际当时没删掉**，它还带着一把**「推送」键**。也就是说阶段 8 的出口
   （"第三方只有两键、越界项一个都没有"）在代码上是不成立的。本阶段真删了。
   同一屏里出现第二对同步键，两边各自改状态机，迟早对不上。
2. **core 的 `ReadmePreview` / `read_readme` / `parse_readme`**（D22），连同 3 个单测。
3. **`DetailTab::todo()`** 与 `detail_body` 里的"还没接"兜底分支。
   五个页签都有实现了，留着兜底分支的话新增页签会被静默接进一个空态，
   而空态看上去和"真的没内容"一模一样。

**这一阶段新出的文件**：`history_tab.rs`（提交列表 + 提交详情 Sheet）、
`readme_tab.rs`（「说明」页签）、`crates/core/src/history.rs`。

**阶段 7 排给本阶段、本阶段没做的**：
- 检视抽屉的「空间」页签**仍然空着**（`INSP_TODO` 剩 1 条）。
  磁盘占用数据在详情页的「空间」页签里是全的，复制到抽屉那一列没有收益——
  与「历史」同一条理由（D21），等阶段 11 收口时一起改成入口。

### 阶段 10 · 克隆（先 core，后界面）

**D5：GUI 不自己拼 git 子进程。** 顺序不能反。

* [x] core：在 `crates/core/src/repos.rs` 加 `repo_name_from_url(url) -> Option<String>`
      —— **不照搬 Tauri 版**（补 D26），多 2 个单测（`Some` 三种写法 / `None` 各种脏输入）
* [x] core：新建 `crates/core/src/clone.rs`，`CloneOutcome` + `clone_repo`
* [x] core 内部走 `net::run_net_op(parent_dir, ["clone","--progress",url,name], …, allow_race = false, …)`
      —— `repo_path` **传父目录**（`git -C <父目录> clone`）；`allow_race` 必须 false
* [x] core 失败处理：清半成品目标目录 + 五类错误中文化
      （已取消 / 未找到可用代理 / 连不上且无代理兜底 / 私有仓库需凭证 / 仓库不存在），
      判定顺序有讲究，见下面第 3 条
* [x] core 成功：入列表（补 D27，走 `repos::add_repo` + `repos::touch_repo`）
* [x] `cargo test -p qingbird-core` 补克隆相关单测并全绿（92 → **98**）
* [x] GUI：克隆 `Dialog` —— 地址输入 + 目标父目录 + 进度条 + 阶段文字 + 实时输出
* [x] 取消按钮接 `git::request_cancel()`，对话框内明确显示「克隆已取消」（**对话框不关**，
      第二次点「取消」才是关窗）
* [x] 成功 → 关对话框 + 通知「已克隆「X」，已加入「我的仓库 / 第三方仓库」」+ **侧栏自动展开对应分组并定位**
* [x] 失败 → 对话框不关，错误留在输出里，**保留用户填的地址**

**出口**：填地址能真克隆，私有仓库能带凭证，失败有中文结论且能直接改地址重试。
> ⚠️ **出口只到"代码与数据通路"这一层**，没有实机跑过——真克隆要联网，
> 而 GUI 只能由用户双击 exe 验收。

**本阶段另外做的几件事**（表上没列，但不做就留了坑）：

1. **克隆超时单独给 30 分钟**。拉取/推送的 `NET_TIMEOUT` 是 90 秒，
   拿它去卡克隆会把大半正常克隆判成超时——大仓库冷克隆十几分钟是常态。
2. **进度从后台线程搬回 UI 线程要过一次 `Arc<Mutex<…>>`**。
   core 不认识 gpui，界面状态也不该跨线程被改；后台线程把事件攒起来，
   主线程一次性倒进状态。代价是一次几十微秒的锁，换来 UI 线程与后台线程
   之间只有一条单向数据流。
3. **错误判定的顺序是设计的一部分**。同一个远端会同时报好几行
   （`fatal: could not read Username` 后面常跟着 `remote: Repository not found`），
   先匹配到的先算。**取消必须排第一**：取消时 stderr 里通常没有任何仓库线索，
   混进后面的分支就会被报成"仓库不存在"，用户刚点完取消就看到这个。
   同理"连不上且没有代理兜底"要排在认证前面——连不上的时候 git 有时也会
   顺带问一句用户名，报成"请检查凭证"会把用户引到错误的方向。
   4 个单测把这四条顺序钉死。
4. **git 的进度行按 `\r` 反复刷新同一句**（"接收对象 42%" → "43%" → …）。
   全攒下来能把输出区刷爆，所以只记**文字真的变了**的行；输出区上限 200 行，
   超了从头丢。
5. **百分比不做假**。core 只在 git 真的报了百分比时才给，没有就走官方
   `Progress::loading(true)` 的不定态条——一个一直在跳的假进度比没有进度更让人焦。

**这一阶段新出的文件**：`crates/core/src/clone.rs`、`gui/src/dialogs/clone.rs`。

### 阶段 11 · 收口

* [x] ⌘K 命令面板（官方 `command::{Command, CommandState, CommandGroup}`）： 仓库切换 + 当前仓库动作 + 批量同步（**原总览页的批量搬进这里**）+ 切换视图 + 低频动作
* [x] 批量在 ⌘K 内实现：**串行循环** core 的 `pull` / `push` / `check_update`（不新增后端批量命令）
* [x] 批量遇脏工作区**跳过并记日志、继续下一个**（不自动 stash）
* [x] 批量跑完一句汇总记到**全局日志**（`Shell::app_log`）并弹一条通知，状态条常驻短句
* [x] 快捷键：`Ctrl K` / `Ctrl B` / `Ctrl I` / `1·2·3` / `Ctrl T` / `Ctrl .` / `Esc` / `↑↓`
* [x] `1·2·3` 按**屏幕位置**对位（拉取·推送·检测），不是操作顺序
* [x] 总览页上 `1·2·3` 与 `Ctrl T` 让位；`Ctrl K` 与 `↑↓` 两屏都生效
* [x] 快捷键在菜单与 tooltip 里都显示 → **只做到 tooltip 那一半**，右键菜单里没画 Kbd 角标（见下方「出入 4」）
* [x] 右键菜单每个命令都有第二条键盘路径 → 收进 ⌘K「当前仓库」组（见 D30）
* [x] 补齐五种状态：空态 / 加载（`Skeleton`）/ 失败 / 禁用 / 分叉
* [x] 补齐长文案、仓库名超长（省略号）、大字号下的复验
* [x] 间距档位归位：13 处裸 `gap` 逐处改成 `GAP_TIGHT`（8 处）或新增的 `GAP_DENSE`（5 处），并在 4.2 语义表补上漏掉的那一类关系 → **第 11 节 22 条现在剩 3 条未过**：焦点环（要实机，是你的放行环节）/ 后台调度器（Q8）/ 右键菜单不画 Kbd 角标（官方 API 做不到，见「出入 4」）
* [x] 设置页接线（主题 / 身份 / 令牌 / SSH / 通知 / 代理探测 `proxy::probe`） → **主题这一项判为"不需要"**（见下方「出入 3」）
* [x] 逐条过 `UI-DESIGN.md` 第 11 节收尾清单（22 条，结果见下方「收尾清单逐条结论」）
* [x] 逐条对 `UI-DESIGN.md` 3.6 内核功能全景表，确认**没有入口的项为零**

**出口**：清单全过，内核能力无孤儿，动作无重复。✅ 代码与数据通路达成（**未实机验收**）

**新增文件**：`gui/src/palette.rs`（⌘K）、`gui/src/dialogs/settings.rs`（设置对话框）。

**core 侧**：本阶段**没有新增 core 代码**，单测仍是 **98**（阶段 10 的数字）。
批量、设置、克隆的落盘全部复用已有入口——这正是设计 3.6 反复强调的那件事：
新界面不另起后端命令，两套能力只留一份实现。

**自主决定**（D29–D34）：

* **D29 · ⌘K 不用官方的 `CommandItem::action`，改走 `on_confirm` + 下标表。**
  官方的 `action` 要一个 `Box<dyn Action>`，而带 payload 的 Action 需要
  `serde::Deserialize` + `schemars::JsonSchema`；本 crate 里的 Action 全是
  `#[action(namespace = qingbird_gui, no_json)]` 的 unit struct。改成"渲染时把
  `第几组第几项 = 什么动作` 存进 `Shell::palette_actions`，回车时用 `on_confirm`
  给的 `IndexPath` 取回来"。这正是 `on_confirm` 存在的理由——它给的是**原始下标**，
  过滤不改变它。
* **D30 · 右键菜单那五条收进 ⌘K「当前仓库」组。**
  设计 6.1 把提交并推送 / 移归属 / 重命名 / 编辑 / 移除的主入口定在右键菜单，
  第 11 节又要求"右键菜单里的命令都有第二条键盘路径"。这两条放在一起只有一个解：
  ⌘K 是 6.1 自己指定的镜像通道。**「复制远程地址」与「在云端打开」不进这一组**——
  它们已经有仓库头按钮 + 右键菜单两条路，再加第三条就撞上 6.1 的"不得新增第三处"。
  ⚠️ 这一条是对 6.1 的一个读法（把"不得新增第三处"理解为"别给同一个动作三条
  以上平级入口"，而不是"绝不允许第二条"）。**要是不认同，把这一组删掉即可**，
  代码上它只占 `palette_model` 里的一个 `CommandGroup`。
* **D31 · 官方 `Command` 的匹配是子串包含，不是旧版那套三档模糊匹配。**
  旧 React 版（`src/lib/commands.ts`）是"完全相等 > 前缀 > 子序列"三档打分 +
  多词 AND。官方 `CommandItem::matches` 只做 `label` 或任一 `keywords` 的
  `contains`。对策是**用 `keywords` 把同义词与英文名补足**（每条命令 4–6 个词，
  仓库那条带名称 / 别名 / 路径 / 分支 / 状态词 / 归属），并新增
  `state::bucket_word` 让「分叉」「落后」这类词能搜到仓库。
  仍然搜不到的只有**非连续子序列**（"开终"）与**带空格的组合词**——
  后者靠把 `open folder` 之类整词塞进 keywords 兜住。
* **D32 · ⌘K 里的行全部用 `label` + `icon`，不挂快捷键角标。**
  角标要走 `CommandItem::child(...)`，而官方 `same_layout` 对带 `child` 的条目
  **永远返回 false** → 每帧重装模型 → 每帧重测全部行 → 面板开着时强制 60fps
  重绘。为几个 Kbd 角标付这个代价不值得。角标改挂在**同步卡三键 / 仓库头终端键 /
  检视抽屉收起键**的 tooltip 上（官方 `tooltip_with_action` 自己从 keymap 查，
  键位改了也不会说谎）。
* **D33 · `Esc` 在壳层只做一件事：收起侧栏筛选框。**
  对话框 / Sheet / 右键菜单的 Esc 都被官方那几层先吃掉了，轮不到壳层；
  在这里 `close_dialog` 反而会把用户正在用的对话框抢关掉。筛选框是本壳
  唯一自己画的临时层，收掉它正好。
* **D34 · 检视抽屉「空间」页签从"还没接"改成摘要 + 入口。**
  与「历史」同一条理由：构成条与 Top 10 都要宽度，320px 摆不下；复制一份
  两处会对"这个仓库占了多少"给出不同答案。`INSP_TODO` 常量随之删除。
* **D35 · 批量汇总的读出口放 ⌘K，不放状态条那一格。**
  详见下方「收尾后自查抓到的一处只写不读」。

**收尾后自查抓到的一处只写不读**（D35 的来龙去脉）：

阶段 11 的批量跑完之后，`Shell::app_log` **只有写、没有读**——`grep` 证实它只
出现在 `ops.rs` 的清理与汇总两处、`state.rs` 的声明和 `shell.rs` 的初始化，
**没有任何一处界面读它**。这正是第 11 节第 6 条自查项（官方件能做的有没有被
手搓）背后同一个毛病：我加了写入端就当这一项做完了。后果是那条通知一消失，
"哪几个没成、为什么"就**永久没了**——逐仓库的日志确实还在 `logs` 里，
但看它得挨个切仓库，几十个仓库时没有人会这么查。

补法两处，缺一不可：

1. **写入端补全**：`run_batch` 除了最后那一句汇总，还把**没成的每一个**
   （跳过 / 失败）连仓库名一起写进 `app_log`。成功的那些不写——几十行
   `✓` 只是噪音，汇总那句已经说了"N 个成功"。
2. **读出口**：⌘K「低频动作」组加一项「上次批量结果」，**没有记录时置灰
   而不是隐藏**（藏了用户会以为这个功能不存在），点开一个 Dialog 摊开那些行。

**明确不做的**：把状态条那一格改成可点。6.1 已经把那一行写成
"进度（不是入口，只是进度显示）"，让它可点等于让文档自己打自己。

顺带把 `log.rs::output_stream` 提成 `pub(crate)` 复用，而不是在对话框里再
画一块浅色输出区。**id 由调用方传**——gpui 的滚动状态是按 `ElementId` 全局
存的，两个元素共用一个 id 会共用同一个滚动句柄，在批量结果里滚一下会让底下
检视抽屉的日志跟着跳。

**这一类已经系统扫过一遍，没有第二处。** 判据不能只看"字段有没有被读"——
`app_log` 的 `lines` / `verdict` 确实都被读过（只是只被写它的那段批量代码读），
所以 `cargo check` 是零告警的：linter 在这一类上本来就是瞎的。正确的判据是
**渲染路径有没有读它**。做法：把 `state.rs` 里 142 个结构体字段逐个统计在
渲染文件（`shell` / `detail` / `sidebar` / `overview` / `panels` / `inspector` /
`changes` / `repo_head` / 四个页签 / `widgets` / `sync_ring` / `log` /
`palette` / `dialogs/*`）里的读次数，读数为 0 的逐个人工核实。
**51 个疑似，逐一核实后 0 个是真缺陷**，三类合法形态：

| 形态 | 例子 | 为什么不算缺陷 |
|---|---|---|
| 持有后台任务句柄 | `scan` / `history.task` / `space_task` / `profile_task` / `ov_task` | 字段本身**就是**"持有句柄"——重建前赋 `None` 等于**取消上一轮**；五个都配了 `seq` 代数守卫丢弃过期结果（`shell.rs:910` / `history_tab.rs:460` / `ops.rs:354` / `ops.rs:417` / `overview.rs:365`） |
| 一次性标记 | `reveal` | 消费点 `apply_reveal` 由 `shell.rs:46` 在 `render` 里调。判定要看**消费点是否在渲染帧**，不能只看它被哪个文件读 |
| 单一读取漏斗 | `BatchRun::*` | 只被 `ops.rs::batch_note()` 这个纯格式化函数读，状态条调它。与"取色只认 `badge_tone`"同一类收口，**不要当成重复读取去拆** |

`UI-DESIGN.md` 第 11 节同步追加了第 23 条自查项「写进去的东西有没有人读」——
它不在原 22 条里，是因为**原清单没有一条问这个**：清单问的是"该做的做了没有"，
没有一条问"做出来的东西用户看不看得到"。

**顺带查了同源的另一类：键位提示会不会是假的。** `tooltip_with_action` 的全部价值
就是"从 keymap 查"，可它自己会骗人的前提是"引用的 Action 真绑了键"——三张表要互相对：
① `#[action]` 声明的 struct ② `bind_keys` 绑的键 ③ `on_action` 处理的 Action。
11 : 11 : 11 完全一一对应，`tooltip_with_action` 引用的 4 个
（`ToggleInspector` ×2 / `OpenTerminal` / 同步三键）**全部在册**，没有假角标。
但另外查出**两处手写的键位文案**，都改了：

* **D36 · 界面上任何键位提示都从 keymap 查，不写死字符串。**
  查出两处：
  * `log.rs` 停止按钮的 `.tooltip("停止当前操作 · Ctrl .")` —— `StopOp` 早就是
    声明了也绑了键的 Action，改成 `tooltip_with_action("停止当前操作", &StopOp, None)`。
  * `widgets.rs` 空态那句 `.child("找不到要的功能时按 Ctrl K")` —— 这一行是 **⌘K
    全应用唯一的可见入口**（D9 删掉标题栏搜索之后），偏偏写死了键位。改用官方
    `Kbd::global_binding_for_action(&OpenPalette, window)` 真查一枚 `Kbd` 画上去；
    查不到时**退成不带键位的纯文字**"用命令面板"——宁可少给一个提示，也不给一个
    可能已经失效的键位。
  改完 `gui/src` 里带 `Ctrl` / `⌘` 的字符串只剩 `KeyBinding::new("ctrl-…")` 那五行，
  也就是键位的唯一定义处。

**这一阶段与表上写的出入**：

1. **表格里的「模糊匹配」没做**（D31）。不是漏做，是官方 `Command` 的匹配
   能力就这么大；补法是 `keywords`，已逐条配齐。**这一条请当作能力差异看待，
   不要当成"搜索更好用了"**。
2. **`1·2·3` 与 `Ctrl T` 在总览页让位的实现方式**：`on_action` 里判
   `page != Page::Repos` 直接 `return`。不用 `KeyContext` 分上下文绑定，
   是因为一个键只能绑一处，而"让位"要的是"这一屏上这键不做事"——
   `KeyContext` 能做到但要求把绑定拆成两套，维护成本高于收益。
3. **设置页的「主题」判为"不需要"**（Q10）。这一版界面只有一套配色，写死在
   `gui/theme.json`（改配色只动那一个文件是设计上的硬约定）。给一个"深色"
   开关却拿不出配套的深色 token，只会切到基座自带的默认深色——那既不是本
   应用的配色，也与设计 4.1"不要深色面板"直接冲突。设置对话框里用一行
   `DescriptionList` 把当前主题与配色出处显示出来，写明原因。
   `Settings.theme` 字段保留（两套界面共用 config.json）。
4. **「快捷键在菜单里显示」只做到 tooltip 那一半。**
   官方 `PopupMenuItem` 会为**带 Action 的条目**自动画 Kbd 角标，但侧栏那套
   右键菜单的条目是**带 payload 的**（要仓库路径），在本 crate 里做不出
   带 payload 的 Action（理由同 D29），所以角标画不出来。
   替代方案是给每条都塞一个"待命"Action，那是假的提示，更糟。
   结论：**右键菜单的命令全部有第二条键盘路径（⌘K，D30），但菜单里不画角标。**
5. **批量在跑时给每个仓库的行标了转圈**（`lg.running`）。旧实现只更
   `Shell::batch`，侧栏那一格不动，用户会以为卡住了。逐条结果写进**那个
   仓库自己的**日志，汇总才进 `app_log`——这也是表上那条要求的落法。

**顺带修掉的两处越界 / 断头**（不属于表上，但属"内核孤儿复查"该抓的）：

* **侧栏右键菜单的「提交并推送…」一直是 `disabled(true)`。** 阶段 4 记的
  "置灰空壳"在阶段 8 做完提交对话框之后**忘了接回来**，而设计 3.6 把它记成
  "已接（阶段 8）"——实际入口是断的。已接上，并且把对话框开在**右键的那一行**
  上（`open_commit_dlg(Some(path))`），不顺带改选中。
* **检视抽屉「空间」页签的"还没接"占位** → D34。
* **命令卡的「复制命令」在手搓一个 `clip` 子进程**，而同一屏另两处复制走的是
  官方 `cx.write_to_clipboard`。那个手搓函数 `os::copy_text` 还挂着
  `#[allow(dead_code)]`，注释写"留给没有窗口上下文的场合"——**那句话经不起
  一看**：调用点是个 `PopupMenuItem::on_click` 闭包，`window` 与 `cx` 都在。
  已删掉，四个复制入口（命令卡 / 仓库头 / 右键菜单 / 日志工具行）现在统一走
  官方件。这条是**上面第 6 条自查项"官方件能做的有没有被手搓"本来没过的证据**
  ——我第一遍打勾时只看了本阶段新增的代码，没回头扫旧代码。

**收尾清单逐条结论**（`UI-DESIGN.md` 第 11 节 22 条）：

| # | 项 | 结论 |
|---|---|---|
| 1 | 裸 hex / `rgb()` | ✅ 只有 `tokens.rs` 一处（补充灰阶的**唯一**定义处） |
| 2 | 绕过档位表写裸字号/圆角/间距 | ✅ 收尾复查时补做，全过。查出 13 处裸 `gap`：8 处归 `GAP_TIGHT`（4.2 点过名的三类关系，写 1–2px 本就违规）、5 处归新增的 `GAP_DENSE`（4.2 漏掉的那一类，语义表已补行）。**8 处因此从 1–2px 变成 4px，是有意的视觉变化** |
| 3 | `Size::Size(px(..))` 自定义按钮档 | ✅ 按钮无。剩下的三处都是 `Icon::with_size`（`icon_sm` / `icon_md` 自身 + 一处已收敛回 `icon_sm`） |
| 4 | 图标全走 `AssetIcon` | ✅ 全仓库只有注释里提到 `component::IconName` |
| 5 | 同一动作只出现一次 | ✅ 按 6.1 对过表；⌘K 里的是表里点名的镜像 |
| 6 | 官方件能做的有没有被手搓 | ✅ **但这条一开始是打错勾的**：收尾复查抓到命令卡的「复制命令」spawn 了 `clip` 子进程（`os::copy_text`），而同一屏另两处复制（仓库头、右键菜单）用的是官方 `cx.write_to_clipboard`。那个手搓函数还挂着 `#[allow(dead_code)]`，注释写"留给没有窗口上下文的场合"——**那句话是错的**，该闭包里 `window`/`cx` 都在手边。已删掉它，三个复制入口现在统一走官方件 |
| 7 | `transition` id 稳定字符串 | ✅ 只有 `"sidebar-width"` 一处 |
| 8 | `reduce_motion` 为真时跳终值 | ✅ 交给官方 `transition`（D12 已登记） |
| 9 | 侧栏拖宽夹在 200–360 | ✅ `size_range` + core `clamp_sidebar_w` 双夹 |
| 10 | 折叠后无空槽位 | ✅ 补间中把 `min_w` 放开到 0 并摘掉面板 |
| 11 | 键盘走完、焦点环可见 | ✅ 本阶段补齐 `1·2·3` / `Ctrl T` / `↑↓` / `Esc` / `Ctrl K` |
| 12 | 右键菜单第二条键盘路径 | ✅ D30（但菜单里不画角标，见出入 4） |
| 13 | 五种状态 | ✅ 空态 6 处（5 处官方 `Empty` + 侧栏 `no_repos`）/ 加载 `Skeleton` 4 处 / 失败 **10 个 `error` 字段**各自就地报 / 禁用 20 处 `.disabled(` / 分叉条 1 处 |
| 14 | 残留深色块 | ✅ 输出流是 `muted` 浅底 + 1px 边；diff 面板是浅底等宽 |
| 15 | 两类仓库越界 | ✅ 推送在同步卡、⌘K、`1·2·3`、右键菜单四处都按归属裁剪 |
| 16 | 第三方有日志区 | ✅ |
| 17 | 耗时操作状态立刻可见 | ✅ 本阶段给批量每一项也标了转圈 |
| 18 | 无 percent 用不定态进度 | ✅ 跑命令与克隆走官方 `Progress` 不定态档 |
| 19 | `commit_push` / `clone` / `add_gitignore_entries` 有入口 | ✅ 三个都有（提交并推送 · 侧栏脚克隆 · 空间页签逐项） |
| 20 | 「提交并推送」没混进三键行 | ✅ 独立一行 + `…` 后缀 |
| 21 | 「云端主页」是唯一 Link 样式 | ✅ `repo_head` 那颗 `head-cloud` |
| 22 | 克隆失败留住地址 | ✅ `dialogs/clone.rs` 失败不清 `url` |

**未过项（如实记下，不藏）—— 收尾复查后剩 3 条**：

| 剩哪条 | 为什么 | 谁来收 |
|---|---|---|
| 焦点环是否可见 | 动作全部有键或 ⌘K 路径已核实，但"环画出来了没有"只能实机看 | **你双击 exe**（这是你的放行环节，不是我能自证的） |
| 「能配但不会跑」的后台自动同步调度器 | 见下方「本阶段发现的一个真问题」，真没修 | **Q8 拍板**后单独开一阶段 |
| 右键菜单里不画 Kbd 角标 | 官方 `PopupMenuItem` 只为带 Action 的条目画角标，而那套菜单的条目带仓库路径 payload；塞"待命"Action 是假提示 | 不收，改用 ⌘K 第二路径（见「出入 4」） |

原先记的两条未过项，一条已收（间距），一条性质变了（角标：从"该做"变成"做不到，已给替代"）：

* ~~**第 2 条的间距那一半没过。**~~ **已收（阶段 11 收尾复查时补做）**：
  13 处裸 `gap` 逐处归位。收之前重新分类过一次，结论和原先写的**不一样**——
  原先把它们一律当成"发丝级贴合、并进 4/8/12 会让档位概念变松"，这是错的：
  其中 **8 处本来就是 4.2 点过名的三类关系**（图标与文字、标题与说明、柱间），
  写 1–2px 是**实打实的违规**，该改成 `GAP_TIGHT`；只有另外 **5 处**
  （列表行之间、构成条色块分段之间、同块内多行元信息）才是表里真的漏了的关系，
  补了 `GAP_DENSE = 2.0` 并在 4.2 语义表加了一行。
  **代价**：8 处从 1–2px 变成 4px，这是一次**有意的**视觉变化，
  理由是 1px 的标题与副标题本来就贴得太紧。5 处 `GAP_DENSE` 像素不变。
  这条进的是"只有你双击 exe 才看得见"的那一类，所以放在这里单独说明。
* **第 12 条只做到"有第二条路径"，没做到"菜单里画角标"。** 理由见出入 4。

**本阶段发现的一个真问题（不是清单里的，是顺手查出来的）**：

**GPUI 这一版没有后台自动同步调度器。** 旧界面 `src-tauri/src/scheduler.rs`
里有一个（`interval_due` / `daily_is_due` / 补跑逻辑 / `last_auto_check` 维护），
它随 Tauri 一起退役，没有被平移进 `crates/core`，GPUI 侧也没有对应的定时器。
后果是「同步设置」页签**能配但不会跑**：`auto_sync_mins` / `daily_time` 会
正常落盘（两套界面共用 config.json，搬回旧界面仍生效），`last_auto_check`
永远是 0，而"上次自动检查"那一行因此永远不显示。
**这不是清单能盖住的项，是一条功能回退。** 已按 Q8 登记，界面侧先做了一件
最小的事：`sync_tab.rs` 里加了一句实话（"这一版界面还没有后台调度器…"），
不让这一页看起来在自动生效。**搬调度器是一个独立阶段，不在阶段 11 的范围内，
等拍板。**

---

### 阶段 12 · 后台自动同步调度器 —— ✅ **已拍板并完成**（2026-10-03 用户「按建议推行」）

> **这一节整节都没勾，是提案不是进度。** 写在路线表里是为了让 Q8 变成
> "照这个清单做 / 不做"的一个具体选择，而不是一句"要不要搬"的口头题。
> **等你点头才动第一行代码。**

**为什么单开一个阶段而不是并进阶段 11**：阶段 11 的出口条件是"五组⌘K + 快捷键 +
设置对话框 + 收尾清单 22 条"，调度器一件都不沾；而且它**有网络副作用**——
后台自己发 git 命令、自己弹系统通知，不该混在一次纯界面收口里验。

**先说清一件事：地基比想象的厚。** 逐条核过之后，**需要新写的只有判定逻辑与循环**，
其余全在：

| 需要什么 | 现状 |
|---|---|
| `RepoEntry` 的 `auto_sync_mins` / `daily_time` / `auto_pull` / `last_auto_check` / `last_auto_result` | ✅ 已在 core `store.rs`，阶段 8 平移时就带过来了 |
| `Settings::notify_auto_sync` | ✅ 已在（手写 `Default` 保持默认 true） |
| `parse_hhmm` / `normalize_auto_sync` / `set_auto_sync` | ✅ 已在，5 个单测 |
| **与用户操作的互斥** | ✅ `sync::try_begin_op()` **已经在 core 里，注释就写着"自动同步只 try_lock（拿不到就跳过本轮）"——它现在没有调用方**。最难的一块早就搬好了，在等这个阶段 |
| 界面侧定时器 | ❌ GPUI 侧没有（`cx.background_executor().timer(..)` 可用） |
| 到期判定 / 单仓执行 / 通知文案 | ❌ 随 Tauri 退役 |

**出口条件（写死，不写"看着还行"）**：

* [x] core 加 `crates/core/src/scheduler.rs`：`AutoSyncKind{Ok,Behind,Pulled,Skip,Error}`
      + `label()`、`interval_due`、`last_due_moment`、`daily_is_due`、`due_paths`、
      `run_one`（`kind`/`message`/`commits`/`diff_stat`）、`record_result`（写回
      `last_auto_check` / `last_auto_result`，**纯函数**：收 `&AppConfig` 返 `AppConfig`，
      不自己落盘）
* [x] core 加 `toast_for(kind, name, message) -> Option<(String, String)>`
      （平移 `notify.rs::toast_for`：**`ok` 不发**、失败写进标题、正文按**字符数**
      截到 180 加 `…`——按字节截会把中文切出半个字）
* [x] core 加 `chrono` 依赖（见下方「要你拍的两点」第 1 条）
* [x] `cargo test -p qingbird-core` 全绿，**新增单测 ≥ 12 个**，其中旧版那 5 个
      原样搬过来：`interval_due_only_enabled_and_expired` /
      `daily_due_fixed_timezone` / `daily_due_catches_up_a_missed_previous_target` /
      `last_due_moment_picks_today_or_yesterday` / `result_summary_prefix_roundtrip`。
      **`daily_due_catches_up_a_missed_previous_target` 是回归测试**：
      09-14 09:30 该跑时机器关着、09-15 08:00 开机，旧实现整天不跑，
      现在开机那轮就补上。**这条不能因为"重写"而丢**
* [x] GUI 加定时器：启动先等 60s（开机不抢网络）→ 每 30s 一轮 → 仓库之间隔 3s
      （不轰炸）。这三个常量与旧版一致，不要随手改
* [x] 每轮拿 `try_begin_op()`：拿不到就**跳过本轮**，不排队、不等待
* [x] 跑完一个仓库后 `refresh_status` 刷新侧栏与总览；`last_auto_check` 非 0 之后
      「同步设置」页签的"上次自动检查"那一行要能显示
* [x] 通知遵守两条"不打扰"：① `ok` 不发 ② **主窗口可见且在前台时不发**
      （GPUI 侧取 `window.is_active(..)` 之类的判定，落点与 `push_notification` 同一处）
* [x] **删掉 `sync_tab.rs::no_scheduler_note()`**（调度器真的会跑之后，
      那句"这一版界面还没有后台调度器"就成了假话）。顺带把 `last_result`
      改成走 core 的 `parse_summary` / `label()`——它原先自己抄了一份
      kind 映射表，两处会漂；并补上一直缺的「上次检查：N 分钟前」
      （它以前永远不显示，因为 `last_auto_check` 恒为 0）
* [x] `cargo check --workspace` 零错零告警 + `cargo build --workspace` 出 exe
* [x] 文档回写：`UI-DESIGN.md` 3.6 那一行从"没接"改成"已接"，
      第 11 节「能配但不会跑」那条 `[ ]` 转 `[x]`

**实际落法与清单的两处出入**（如实记下）：

1. **托盘事件没有用官方 `set_event_handler`**，改成主线程 100ms 轮询
   `TrayIconEvent::receiver().try_recv()` / `MenuEvent::receiver().try_recv()`。
   原因：那个 handler 要求回调 `Send + Sync`，而 gpui 的主线程执行器带
   `PhantomData<Rc<()>>`（`executor.rs:32`），**不是 `Send`**，送不进托盘线程。
   轮询的代价是 ≤100ms 延迟，换掉了一整条跨线程桥。Windows 上托盘的隐藏
   HWND 靠 gpui 那套标准 `GetMessageW`/`DispatchMessageW` 派发
   （`gpui-pre-windows/src/platform.rs:516-519`），所以**托盘必须在主线程建**。
2. **"主窗口在前台"没有用 `Window::is_active`**——这个 gpui 版本上取不到
   （有 `is_a11y_active`，不是一回事）。改用 `App::active_window()` 与窗口句柄比对。
3. `auto_pull` 为真且工作区脏时**不 stash**（与旧版一致）：脏的情况在
   `detect_and_maybe_pull` 里已经提前判成 `Skip` 了，再 stash 就是替用户做决定。

**阶段 12 顺手修掉的一个继承 bug**（`D37`）：

平移过来之后自己复查，发现"用户刚关了"那个守卫写错了：

```rust
if entry.auto_sync_mins == 0 { continue; }   // ← 错
```

「每天定时」在 `normalize_auto_sync` 里落盘的是
`(auto_sync_mins = 0, daily_time = "09:00")`——**间隔是 0、时间点非空**。
只判间隔的话，**「每天定时」这一整档会被当成"用户刚关了"跳掉**：
用户设好了、界面显示正常、就是永远不跑。

**这个坑是从旧 Tauri 版原样继承的**（`src-tauri/src/scheduler.rs:117-119`
有一模一样的 `if entry.auto_sync_mins == 0 { continue }`），**也就是说旧版的
「每天定时」也从来没真的跑过**——这一条以前一直没人发现，因为"每天定时"要等到
指定时刻才触发，没人盯着那个时刻去验证。

改法：判据收进 core 的 `scheduler::is_enabled(e)`——`auto_sync_mins > 0`
**或** `daily_time` 非空，两种模式都认。放在 core 而不是留在 GUI 定时器里，
就是为了让它有单测可盖（`daily_mode_counts_as_enabled_even_though_its_interval_is_zero`
钉住这个前提，`normalize_auto_sync` 对 Daily 返回的确实是 `(0, "09:00")`）。
core 单测 118 → **121**。

**阶段 12 收尾自查抓到的第二处（我这一轮自己写的）**：

托盘菜单的「一键扫描全部仓库」原先写成"有窗口就什么都不做"，理由是
"用户正在看，别突然刷一层"。**方向正好反了**：托盘点"扫描"绝大多数时候窗口
是开着的，那一版等于让这个菜单项在常见场景下**完全没反应**——
比不做更糟，因为菜单项摆在那里看起来是能用的。
旧 Tauri 版是 `show_main` + 推 `scan-all`，窗口在不在都做事。

改成：**有窗口就 `Shell::refresh_status` 刷它，没有窗口就先把窗口开出来**
（`Shell::new` 本身会做一轮初始扫描，不用再补）。刷新是用户主动点的，
界面变化正是他要的，不存在"突然"。

**这一类的两条一起记**（D37 / D38）：**平移与新写的代码都要按"每个菜单项、
每个分支、每个守卫，逐条问它在真实场景下会不会真的做事"过一遍**——
两条都是"代码在、编译过、逻辑看着对"，但一条让整档功能静默失效、
一条让一个菜单项在常见场景下静默无反应。**界面不报错，是这类缺陷最难发现的地方。**

**复查的范围与结果**（2026-10-03，D37 / D38 之后）：

| 查了什么 | 结论 |
|---|---|
| 「同步设置」保存 → `store::set_auto_sync` | ✅ 真的落盘，草稿用 `load_sync_cfg` 重建 |
| `auto_sync_text` / `sync_cfg_from` 读配置 | ✅ 两个都**先判 `daily_time`** 再判间隔，没有同病根 |
| `SYNC_MINS_PRESETS` 与 `AUTO_SYNC_MINS_RANGE` | ✅ 最小档 5 = core 下限；`DAILY = 6` / `PRESET_COUNT = 7` 下标自洽 |
| `sync_tab.rs` 的 `daily_ok` 门禁 | ✅ 时间非法时保存按钮禁用，不会"选了这一档却存成不定时" |
| `tick()` 的每个守卫 | ✅ 逐条过：仓库被移除 / 用户刚关 / 拿不到锁 / 窗口不存在 / 通知被抑制——五条都真的会走到且行为正确 |
| 托盘 `drain()` 三个菜单项 + 左键 | ✅ 逐条过；`show_or_open` 先查 `cx.windows()`，反复点不会堆窗口 |
| 「点托盘时窗口刚好被关」 | ✅ `update_window` 失败会退回"重开"，不会什么都不做 |

**结论：这一类一共 2 处，全部已修，没有第三处。**

**要你拍的两点**（其余我自己定）：

1. **`chrono` 依赖进 core。** `daily_is_due` 要判"本地时区今天这个时刻过没过"，
   而旧实现里那段注释专门警告过：**给 `DateTime` 直接减 24 小时会在夏令时切换时
   整段偏一小时**。手搓本地偏移（读注册表时区）就是自研 + 要自己处理 DST，
   正好撞在项目"自研仅限三处"的边界外。**`chrono 0.4.45` 已经在 `Cargo.lock` 里**
   （`gpui-pre` / `gpui-component` 拉进来的，已编译过），加进 core **零新增编译成本**。
   **我的建议：加。**
2. **自动同步的结果要不要在界面上留痕。** 旧版只发系统通知 + 推事件，
   界面侧「同步设置」只显示 `last_auto_result` 那一行摘要。
   * 我的建议：**照旧**，后台结果主要靠通知 + 页签那一行摘要，
     不要往 `app_log` 灌（那个槽是给跨仓库批量的，塞单仓自动同步会稀释它）
   * 代价：窗口在前台时通知被抑制，用户可能错过"自动拉取成功"

**这一阶段明确不做**：托盘常驻（gpui-pre 0.3.7 没有 tray API，那是另一件事）；
后台结果不写进仓库的日志槽（与 `app_log` 同一个理由）。

#### ⚠️ 阶段 12 的前置依赖：托盘与"关窗不退出"（Q11）

**这一段是查 Q8 时顺手挖出来的，它比调度器本身更要紧——照上面那份清单
直接把定时器做出来，会做出一个"只在窗口开着时才叫后台"的功能。**

把旧 `src-tauri/src/lib.rs` 的命令面（`generate_handler!` 45 条）逐条对到新的
core + GUI，**45 条全部有对应**（3 条 OS 相关的落在 `gui/src/os.rs`），一个功能
回退都没有。但**调度器从来不是 Tauri 命令**——它是在 `setup()` 里起的后台线程，
所以命令面对比查不到它。同一个 `setup()` 里还起了别的东西，逐条核实的结果：

| 旧版 `setup()` / `run()` 里的能力 | 新版 |
|---|---|
| `scheduler::start()` 后台定时自动同步 | ❌ 随 Tauri 退役（就是 Q8） |
| **托盘图标 + 菜单**（显示主窗口 / 一键扫描全部 / 退出） | ❌ **没有**。`gui/Cargo.toml` 里没有 `tray-icon` |
| **关窗不退出**：X 按钮 → `prevent_close` + `destroy` 窗口 → 应用退到托盘常驻；`ExitRequested` → `prevent_exit` | ❌ **没有**。根因是 `QuitMode` 用的默认档（非 macOS 上 = 最后一个窗口关了就退出），一行 `with_quit_mode(Explicit)` 就能改，但**必须与托盘同一次改动上**（见下） |
| 托盘左键单击切换显示/隐藏 | ❌ 随托盘一起没有 |
| ~~开机自启~~ | 旧版菜单注释写了"开机自启"，但 `Menu::with_items` 里**实际只有 show / scan / quit 三项**——旧版就没实现，不算回退 |

**"关窗不退出"在 Windows 上做不了**，三条 API 事实（查 `gpui-pre 0.3.7` 源码）：

1. `TitleBar::on_close_window(..)` 源码里第一行就是
   `if cfg!(target_os = "linux") { .. }`——**Windows 上这颗钩子根本不会被写进去**，
   点 X 一律走默认分支 `window.remove_window()`（`title_bar.rs:233-239`）。
2. `App::on_window_closed(..)` 的文档写明 **"The window is no longer accessible at
   the point this callback is invoked"**——它是窗口**已经没了之后**的回调，
   只能做善后，拦不住关闭。
3. `Window::remove_window()` 只是 `self.removed = true`，**没有 prevent 语义**；
   整个 `gpui-pre` 搜不到 `prevent_close` / `on_window_should_close`。

> **⚠️ 更正一处我自己的推论。** 我最初据这三条写下"**基座拦不住，Windows 上做不了**"——
> **这句话是错的**，错在把"拦不住关闭动作"当成了"拦不住进程退出"。
> 真正决定"关掉窗口之后进程还活不活"的不是关闭拦截，是 `QuitMode`
> （`gpui-pre-0.3.7/src/app.rs:2922-2926`）：
>
> ```text
> let quit_on_empty = match cx.quit_mode {
>     QuitMode::Explicit          => false,   // 只在我调 App::quit 时退
>     QuitMode::LastWindowClosed  => true,
>     QuitMode::Default           => cfg!(not(target_os = "macos")),  // ← Windows 上 = 退
> };
> ```
>
> `Default` 在非 macOS 上**就等于** `LastWindowClosed`——所以现在关窗确实会退出，
> 但那是**默认值**造成的，不是拦不住。设 `QuitMode::Explicit` 就能让进程留下来，
> 而且是**公开 API**：`gpui_kit::application().with_quit_mode(gpui_kit::QuitMode::Explicit)`
> （`app.rs:227`）或运行期 `cx.set_quit_mode(..)`（`app.rs:1754`）；
> `QuitMode` 经 `gpui_kit::*` = `::gpui::*` 直接可用（`gpui-kit/src/lib.rs` 是 `pub use ::gpui::*;`）。
>
> **而且根本不需要拦截关闭**——旧 Tauri 版也是**销毁窗口**（`window.destroy()`），
> 它要的只是进程活着。所以 Q11 的落法是清晰的：
> **`with_quit_mode(Explicit)` 一行 + `tray-icon`（自挂）+ 托盘点击时再调一次
> `gpui_kit::open_window(..)`**（`main.rs` 现在就有这个调用）。
>
> **⚠️ 连带约束（写死的排期规则）**：`QuitMode::Explicit` **必须与托盘在同一次改动里
> 一起上**。只设 `QuitMode::Explicit` 而没有托盘，窗口一关应用就变成
> "进程活着但没有任何入口"的幽灵——那比直接退出更糟。**这条别拆成两次提交。**
>
> **那三条 API 事实本身仍然成立**（`prevent_close` 确实不存在），只是从它们
> 得出的结论错了。留着是为了**别再花时间去找那个不存在的钩子**。

**为什么这件事比调度器更要紧**：旧版关窗之后是"销毁 Webview、Rust 后端与托盘常驻，
定时自动同步不受影响"（旧 `lib.rs:160-167` 的注释就是这么写的）。新版关窗 = 进程结束。
所以按上面那份清单做出来的定时器，**只会在用户开着窗口的时候跑**——
用户设了"每天 09:00"，晚上十点关掉电脑/关掉窗口，第二天早上根本不会跑，
界面上却什么都不显示。**这比没有这个功能更糟**：没有的时候至少页签里写着实话。

**所以阶段 12 的清单要按下面两处执行**（已并入上面的出口条件）：

* 定时器**不能挂在 `Shell` / 窗口实体上**——窗口一销毁就没了。要挂在
  **进程级**（`main.rs` 的 `application().run()` 闭包里 `cx.spawn` 一条与窗口
  无关的长活），或者先解决 Q11。
* `tray-icon` 依赖 + 托盘菜单是 **Q11 的主体**，与阶段 12 分开排期：
  它同时服务"关窗常驻"和"自动同步结果在窗口关着时也能收到通知"两件事。
  gpui-kit 没有托盘件，要自己挂 `tray-icon`（Windows 上菜单要用 win-menu 类实现，
  即 `muda`——菜单不能走 gpui 的 `PopupMenu`，那要一个活的 `&Window`）。
  **依赖可用性已实测（2026-10-03）**：`tray-icon 0.26` + `muda 0.21` 本地 registry
  已缓存，在进程内 `cargo build --offline` **8.57 秒编过、零错**，产物 584 KB。
  两个 crate 都带 `build.rs`，但**没有触发 AGENTS.md 里记的那个 OS error 231**
  ——那条"带 build script 的依赖编不出来"的说法过宽，AGENTS.md 已同步更正。
  **所以 Q11 在依赖层面没有阻碍，剩下的是集成工作量。**
* **如果 Q11 判为"不做"**，那么阶段 12 必须**改口径**：把「同步设置」页签的
  定时两档（间隔 / 每天定时）从界面上**摘掉**，只留"检测云端更新"的手动入口——
  一个只在窗口开着时才生效的"每天定时"是假功能，**宁可不给这个档位**。
  这一条会改动 3.6 与第 8 节的键盘/设置表，属于**要你点头才动**的范围。

#### D39 · 托盘图标重画（Q11 的收尾自查，2026-10-03）

`tray.rs` 的 `app_icon_rgba` 是**程序化生成**的，交付时**一眼都没看过**——
`cargo check` 过了、exe 出来了、托盘建起来了，图标长什么样没人知道。
把算法原样复刻成 PNG 之后才看清：**32px 与 16px 渲出来都是一个白色对话气泡，
不是鸟**。三条互相独立的病因：

1. **坐标原点用错。** 几何按"原点在左上角"写（`cy + …` / `cy - …`），
   而像素坐标 `px / py` 是**以圆心为 0** 的。谷底落在 `y = cy + 0.10n = 19.5`，
   32px 画布的有效范围只有 `-15.5..15.5`——**谷底整个掉到画布外被裁掉**，
   两翼正好在圆底被切断，渲出来就是"圆底底部被啃两口"。
   **调参数不可能调对这个**：它不是造型难看，是几何根本不在画布里。
2. **品牌里根本没有鸟。** 应用叫「青鸟」，但 `assets/icon-source.png` 上是
   **一个带 git 分支的立体箱子**。按名字脑补出一个图腾、连调三轮飞翼参数，
   **方向从第一秒就错了**。托盘图标要跟窗口/任务栏那一套对得上，
   该取的是品牌里真正有辨识度的那部分——**分支图**。
   （旧 Tauri 版托盘用的就是 `icons/32x32.png`，可那张图在 16px 下同样是一坨
   青蓝色块，认不出是什么。要手绘简化版不是因为"能画得更好看"，
   是因为**原图在这个尺寸上根本读不出来**。）
3. **无底字形在深色任务栏上直接消失。** 试过"无圆底 + 墨色字形"那几档，
   在 `#202020` 上一像素不剩——托盘图标是浮在任务栏上的，任务栏有深浅两套，
   纯色字形等于赌用户用的是浅色。改成**圆底自带底色**，两种任务栏都成立，
   **顺带免掉了读 `HKCU\...\Personalize\AppsUseLightTheme` 切深浅两套图标**这件事。

**落法**：`accent.background`（#5B7A99）圆底 + 白色 git 分支——竖直主干两端
两个节点，另有一条从上节点向右下弯出去的弧接到右侧节点（`git-branch` 的
标志性弧线）。⚠️ 那条弧**必须弯**：直着画会读成「Γ」角标——从节点水平出发
再急转直下，左上角糊成一团连续实心块（字符图实测覆盖率 1.0 的一整片）。
几何常数一律写成相对 `n` 的比例、**不带 `CENTER +`**，并在函数注释里写死了这条。

**配两个单测**（`gui` 侧此前没有 `#[cfg(test)]`，这是头一个）：

* `icon_glyph_lands_inside_the_disc` — 字形像素数量落在 120–420、
  包围盒四边都在圆内、左右上下对称（各差 ≤ 1px）。
  **判据用包围盒而不是重心**：竖直主干是根长条、全压在左边，重心天生偏左约
  2px（实测 13.7 vs 圆心 15.5），那是形状本身不是缺陷。第一版就是这么把
  测试写歪的——写完立刻红，**是测试抓出了判据选错，不是抓出了图标有问题**。
* `icon_corners_are_transparent` — 四角全透明、圆心全不透明（否则托盘上出现方块）。

**方法论（这一轮真正的产出）**：

* **程序化生成的图形，交付前必须渲成图亲眼看过。**
  Python 双胞胎只能证明"算法想对了"，证明不了 Rust 里没打错一个符号——
  最后一步是把 Rust 自己吐出的 4096 字节导出来看的，不是看 Python 那版。
* **别按名字脑补品牌。** 「青鸟 Git」的图腾不是鸟。看一眼 `assets/icon-source.png`。
* **图形类代码的测试要挑对指标**：形状不对称时"重心居中"是伪判据，
  **包围盒**才对应"有没有掉出画布、有没有偏出边界"。

#### D40 · 启动即崩：11 个 Action 监听器挂在了错误的绘制阶段（2026-10-03 实机首启）

**这是路线表 0–12 全部阶段写完、121 个单测全绿、`cargo check` 零错零告警、
exe 成功构建之后，用户双击 exe 才发现的第一个问题：一闪就没。**
阶段 0–11 的「代码与数据通路达成（未实机验收）」那句限定词，原来一直是真的。

**症状**：`this method can only be called during paint`，进程在
`Shell::new` 里就死了，窗口都没建起来。`RUST_BACKTRACE=1` 一跑就定位到
`shell.rs` 的 `cx.on_action`。三条源码事实（在 `gpui-pre-0.3.7` 里逐条核过）：

| # | 事实 | 出处 |
|---|---|---|
| 1 | `Window::on_action` 第一行就是 `self.invalidator.debug_assert_paint()` | `window.rs:6677-6682` |
| 2 | `DrawPhase::Paint` **只在** `Window::draw_roots` 的 `root_element.paint(self, cx)` 那段被设上 | `window.rs:3571-3572` |
| 3 | `Render::render` 跑在 `DrawPhase::Prepaint` —— **把监听器搬进 render 也没用** | `window.rs:3502` vs `app.rs:3014` |

第 3 条是踩过的：第一次以为"render 也是绘制"，搬进去之后照样崩，
backtrace 变成 `bind_actions ← render ← any_view::render`，白改一轮。

**落法**：改用**元素级** `on_action`，挂在壳层根 `v_flex` 上，写法与
gpui-component 自己一致（`command/state.rs:926`、`input/search.rs:333`）：

```rust
v_flex()
    .track_focus(&self.focus)                 // 见下面第二个坑
    .on_action(cx.listener(|this, _: &ToggleSidebar, window, cx| this.toggle_sidebar(window, cx)))
    // …另外 10 个
```

元素级之所以对：它只是把监听器塞进 `Interactivity`，
由 `Div::paint_keyboard_listeners`（`div.rs:3268-3298`）在 **paint 里**
统一 `window.on_action(..)`。**天然每帧重挂**，不需要额外维护。

**顺带查出来的第二个坑，比第一个还阴（release 下也犯）**：

Action 派发走的是"焦点节点 → 逐级往上冒泡"。**一个焦点都没有时**，
`focus_node_id_in_rendered_frame` 落到 `dispatch_tree.root_node_id()`
（`window.rs:6251-6259`），而 `root_node_id()` 恒等于 `DispatchNodeId(0)`
（`key_dispatch.rs:611-614`）。`dispatch_path` 只回溯 parent
（`key_dispatch.rs:563-572`），node 0 没有 parent，**路径就只剩它自己**。
而 node 0 是**最外层那个元素**——`Drawable::prepaint` 每个元素 push 一个
node（`element.rs:405`），最外层是 `draw_roots` 里的 `self.root`
（`window.rs:3532`），也就是 **gpui-kit 包的那层 `Root`**。
我们挂在 `Root` 更深一层的监听器，**一次都收不到**。

更要命的是这正好是最需要快捷键的时刻：**刚启动、用户还没点过任何东西**，
按 `Ctrl K` 不会有反应，而界面上看不出任何异常。

修法：`Shell` 加自己的 `FocusHandle`（`state.rs` 的 `focus` 字段有详述），
根元素 `.track_focus(&self.focus)`，`new()` 末尾 `this.focus.focus(window, cx)`。
之后无论焦点在壳层内哪个控件（冒泡一路走到根）、还是谁都没聚焦
（根节点自己就是派发终点），监听器都收得到。**对话框是根节点之外的兄弟，
`Esc` 仍先被它吃掉**（D33 那条设计意图不受影响）。

**为什么前面全绿的检查一条都没抓到**：`cargo check`、`cargo test`、
121 个 core 单测、2 个 gui 单测全部通过——**这条缺陷只在"真跑起来"时存在**。
连单测都测不到：gpui 的 `VisualTestContext` 是 `#[cfg(all(target_os = "macos", ..))]`
（`app.rs:40-41`），Windows 上没有窗口模拟器可写。

**方法论**：

* **"编译通过"和"能跑起来"是两道关，自查清单里现在要有第二道。**
  这与 D39 是同一个形状的两例——D39 是"程序化产物长得对不对"，
  D40 是"程序跑不跑得起来"，**都不是编译器能判的**。
* **不确定某个 API 的调用阶段时，先去源码里找它的 `debug_assert_*`**。
  `debug_assert_prepaint` / `debug_assert_paint` / `debug_assert_paint_or_prepaint`
  这一族就是基座自己写下的阶段契约，比文档更能说明"什么时候能调"。
* **注意 `debug_assert!` 在 release 下是空操作**：同一个错误在 debug 里
  是"立刻 panic"，在 release 里就变成"静默地少活一帧"。
  **发布前至少跑一次 debug 构建**。

#### D41 · `h_flex()` 装 `items_center()`、`v_flex()` 什么都不装：交叉轴不对称（2026-10-03 实机二轮）

用户报「三个主板面内容全部炸了，乱的」。用彩色描边逐层埋点读出盒子：
根 `v_flex` 满窗 ✓，导航轨所在的 `row` 满高 ✓，**主区那个容器只有 ~343px
（正好等于内容高度），落在 row 的下半部**。子元素各自的高度都对，
只有**它们所在的那一行**没把子项撑满。

根因是基座自己的两条文档（`gpui-component/crates/base/src/styled.rs:84-115`）：

```
/// Lays children out in a row, centered on the cross axis.
/// … it is **not** the mirror image of [`Self::v_flex`], which leaves the
/// cross axis stretching. A column placed in a row therefore does not take
/// the row's height: it takes its content's height and is centered inside the row.
fn h_flex(self) -> Self { self.flex().flex_row().items_center() }
fn v_flex(self) -> Self { self.flex().flex_col() }
```

**`h_flex()` 与 `v_flex()` 在交叉轴上不是镜像关系**，而直觉上一定以为它们是。
一行里放一个列容器，列容器拿到的不是行高，而是**内容高并被垂直居中**。

落法：`finish_frame` 的 `row` 加 `.items_stretch()`。选它而不是给子项逐个
`.h_full()`，是因为这条 row 里挂着三样东西（导航轨 / 主区 / 检视抽屉），
逐个补是三处重复，而根上改一次就都归位。

**连带修的两处同源问题**：

* 详情页底排（磁盘摘要卡 + 未提交变更卡）也是 `h_flex()`，两张卡高度差一大截
  （一行 vs 六行），居中让它们顶边错开，看着像两张卡各自飘着 →
  同样加 `.items_start()`（**不是** `items_stretch()`：磁盘卡就该是磁盘卡那么高，
  撑成空大盒子更假）。
* 总览页那张「汇总表整块不渲染」是**这件事的后果，不是独立缺陷**：
  主区被压成 343px 之后，表格容器里的表头被推出可视区、整块看着是空的。
  `row` 一修，17 行全出来了。**先修根因，别急着给症状写补丁。**

#### D42 · 详情页概览要滚动，不能靠「某一块吃掉剩余高度」（2026-10-03 实机二轮）

`row` 修好之后暴露出第二个空洞：命令卡在只有 4 行时有 ~260px 卡内空白。
早先代码注释写的是「设计稿：命令卡与底排分摊主区剩余高度（`fill_container`）」——
**设计稿里没有这句话**，是平移时自己编的理由。

两条路都不对：

* 给某一块 `flex_1` 让它吃剩余高度 → 卡内留两百多像素空，
  读起来像内容没加载出来；
* 都不给 → 概览那五块在**窗口下限 1080×640** 下是 ~716px 内容 vs ~414px 可用，
  **底排被直接裁掉且无处可去**（设计文档自己定的下限）。

落法：只有「概览」页签套滚动容器（`detail_body` 的 `detail-ov-scroll`），
`workbench_body` / `profile_body` 整列改成内容高。其余四个页签各自已经能填满
并自滚（说明是长文、历史是官方 `List`、空间与同步设置自带 `*_scroll`），
**别给它们再套一层**。

#### D43 · `short_day` 按位置取段，取到的是「年-月」（2026-10-03 实机二轮）

总览柱状图 14 根柱顶上全印着同一个 `2026/09`。`short_day` 假设输入是
`MM-DD`，取 `split('-')` 的前两段；而 `local_date_key` 产出的是
`YYYY-MM-DD` 三段，于是前两段是年与月。

**改法不是"调一下偏移"**：段数不同的两套格式用"按位置取"来兼容，
这个写法本身就是不可靠的（早先 `insight` 那类假设也一样）。改成按语义取
（跳掉年段再取月日）并对缺段容错。

同时发现柱顶标签本来就是**每根柱自己的值**（`BarChart::label`），
写日期等于把 x 轴上的信息再抄一遍、抄 14 次 → 改成写 `count`，
日期交给 `band()`（x 轴只留 `M/D`，完整日期在 tooltip 标题上）。

#### D44 · 总览表格：`Column::text_right()` 在 `DataTable` 上是空操作（2026-10-03 实机二轮）

`OvDelegate` 给三个数字列写了 `.text_right().p_0()`，肉眼看不出任何不对——
**编译器不报错、运行不 panic、就是不生效**。查基座：`Column::align` 只有
`table::Table` 那条路会读（`table.rs` 的 `TableCell` / `TableHead` 里
`when(self.align == TextAlign::Right, |this| this.justify_end())`），
而总览用的是 `DataTable` + `TableState`，`state.rs` **全程没有读过 `align` 字段**。

`p_0()` 倒是生效的（`state.rs:1375` 读了 `column.paddings`），所以真实观感是
「状态 pill 尾巴紧贴下一列的数字」。

落法：两处都去掉，统一左对齐 + 默认内边距。右对齐要想真做，得在 `cell()` 里
自己摆 `justify_end`，而那样**表头还是左对齐的**，两边反而对不齐。
**方法论：基座的 builder 方法分「哪条路会读它」和「它编译得过」两件事，
后者不代表前者成立；写了不生效的调用要连注释一起删，留着是句谎话。**

#### D45 · 在窗口自己的事件里调 `update_window`：闭包被静默丢弃（2026-10-03 实机二轮）

总览表格点行「选中了但不跳转」。行确实选中了（相邻两帧的 diff 正好是那一行），
所以问题在订阅回调里那条 `cx.update_window(win, |view, ..| view.downcast::<Shell>())`。

`App::update_window_id` 取窗口靠 `cx.windows.get_mut(id).and_then(Option::take)`
（`app.rs:2888`），而这个订阅**恰恰跑在该窗口自己的事件处理里**——
窗口此刻已经被 take 走了，于是走 `update(None)` 分支，**整个闭包被丢掉**，
返回 `Err("window not found")`，外面那个 `.ok()` 又把错误吞了。
三处都不出声，用户看到的就是"点了没反应"。

落法：回到本项目已有的**一次性标记**约定——置 `pending_reveal`，
由 `Render::render` 下一帧兑现（与「克隆成功后完整定位」同一条路）。
`reveal_repo` 要 `&mut Window`，订阅回调拿不到；`render` 里拿得到。

**方法论**：`update_window` / `downcast` 回取自己这一套，在
「从别的实体回调进 Shell」时能用，在「Shell 自己的窗口事件里」必然失败。
凡是**需要 `&mut Shell` + `&mut Window` 的动作**，一律留一次性标记给 `render` 兑现，
不要试图在回调里把窗口抢回来。

#### D46 · ⌘K 分组顺序：长列表不能排第一（2026-10-03 实机二轮）

`palette_model` 把「切到仓库」排第一。17 个仓库就把面板可视高度吃光，
打开 ⌘K 看到的是一屏仓库名，其余四组要滚过 17 行才够得着——
而那四组才是低频层真正要承载的固定命令。

落法：`低频动作 → 当前仓库 → 批量同步 → 切换视图 → 切到仓库`。
仓库名靠搜索找（关键词里带名字 / 别名 / 路径 / 分支 / 归属），不靠滚列表找。

⚠️ **`G_*` 下标常量与代码块的 push 顺序是两件事**。改常量不会改顺序，
只会在 `debug_assert_eq!` 里撒一个仍然通过的谎。改顺序时把常量、
代码块、`debug_assert_eq!` 一起动（`debug_assert` 只在 debug 下活）。

#### D47 · 冷启动就停在总览页，而活跃度扫描只挂在「点导航轨进总览」上（2026-10-03 实机二轮）

设计文档 §3.1 写的是「活跃度是进总览页才扫的，不是启动就扫」。
但同一份文档的「启动首屏」又写「默认停在总览」——**两条合起来的结果是
首屏的柱状图永远是一排 0 高度**，读起来像"近两周没人提交"。

落法：加一次性标记 `Shell::ov_need_scan`（`new()` 置 true），
`render` 在总览页消费它；点导航轨回来那条路仍走 `go` 里的 `scan_overview_activity`，
**两条路不重复扫**。设计文档 §3.1 那句同步改口径（见下）。

#### D48 · 自查能力：GUI 只能由人眼看画面，但「看清问题」不需要人眼（2026-10-03）

`AGENTS.md` 写的是「GUI 只能由用户双击 exe 验收」。这句是对的，但被读成了
「agent 对界面缺陷完全无能」——**其实只是看不到画面，而画面对脚本是现成的**。
`PrintWindow(hwnd, hdc, 2)` 能把窗口抓成 PNG，于是：

* **点哪坏在哪**：`SetCursorPos` + `mouse_event` 模拟点击，命中区能逐个试；
* **盒子多大**：临时给 `shell.rs` / `overview.rs` 的关键容器描彩色边，
  渲出来直接读像素坐标，比猜准得多（D41 就是这么定位的）；
* **两个状态差在哪**：`PIL.ImageChops.difference` 求 diff bbox
  （D45 里"行到底选没选中"就是这么确认的——diff 正好是那一行）；
* **改没改对**：`PIL` 裁剪 + 放大读小字（D43 的 `2026/09`、D44 的列间距）。

三条纪律：

1. **临时描边必须在核完当轮删掉**（本轮收尾已核：`gui/src` 无残留）。
2. **验第三方屏要有第三方仓库**：临时把真实 `config.json` 拷到
   `$TEMP/qbtest-appdata`、改两个仓库的 `is_third_party`，
   用 `$env:APPDATA` 指向副本启动——**不动用户自己的配置**。
3. **窗口未获焦时第一击只用于激活窗口**：脚本要补一次空点击，
   否则"点了没反应"会被误判成功能坏了（本轮在导航轨上踩过）。


#### D49 · 官方 `TitleBar` 把每个子项推到一头，「标题」因此贴着窗口控件（2026-10-03 实机三轮）

标题栏里图标在左、「青鸟 Git」在最右紧挨着最小化按钮。查基座
（`title_bar.rs:371-398`）：`children` 被塞进一个
`h_flex().justify_between()`，**有两个子项就被推到两头**。

落法：图标与标题**打包成同一个 `h_flex`**，于是它变成 TitleBar 的唯一一个
子项，`justify_between` 无处可推，整块贴着左边。

**顺带记一条通用形态**：只要用了带 `justify_between` 的官方容器，
"两个东西本该挨着"就得自己先并成一个——否则基座会替你把它们分开。

⚠️ **而"并成一个"必须做到底，不能只并一部分**（D57）。第一版把图标+标题并了，
侧栏开关 `SidebarToggleButton` 仍挂在 TitleBar 上——总览页没有开关（一个子项，
看着正常），**切到仓库页就成了两个子项，开关贴左、整块图标+标题被推到右边**，
"青鸟 Git"又回到窗口控件旁边。落法：开关也塞进**同一个** `h_flex`，
TitleBar 的子元素在两屏都恰好只有一个。
**教训：一个"只在某些分支里犯"的错，只跑默认那一屏的自查抓不到。**

#### D50 · 应用图标换回老 `icon.ico`，位图走 `img()` + 自建资产源（2026-10-03 实机三轮）

标题栏原来画的是 `assets` 里的 `Bird` 图标（墨色圆角方块 + 一只白鸟），
而品牌真正在用的是老 Tauri 版 `icon.ico`——**深青灰圆角方块 + 白色描边的
立体 git 箱子**。D39 当时已经查过"品牌里没有鸟"，却只改了托盘图标，
没回头改标题栏。

落地查了三件事：

* `gpui_kit::component::Icon` 只会 `svg().data(..)`，**喂 PNG 进去是空白方块**；
* gpui 自带的 `img()` 走 `ImageSource::Resource(Resource::Embedded(path))`
  → `AssetSource::load` 拿字节 → **`image::guess_format` + `PngDecoder`
  解码**（`elements/img.rs:669`）。**PNG 解码已经在 gpui 的依赖树里**，
  不需要为本项目拖一个图像解码依赖进来；
* 资产源：新增 `gui/src/app_assets.rs` 的 `AppAssets`，只比 `AllAssets`
  多认一条路径 `icons/app-icon.png`，其余**全部转交**——少转一条就是
  界面上一个空白方块，而 `cargo check` 一点都看不出来（这正是当初
  必须从 `Assets` 换成 `AllAssets` 的同一条理由）。

**图为什么拷进 `gui/assets/` 而不是 `include_bytes!` 直接指 `src-tauri/`**：
`src-tauri/` 是迁移源、Q6 一旦点头就整体退役。让新 GUI 的构建去
`include_bytes!` 一个"准备删掉的目录"里的文件，等于把退役日期焊死在
构建上。`gui/assets/app-icon.png` 由 `src-tauri/icons/128x128.png` 拷来
（`tauri icon` 从 `assets/icon-source.png` 展开出的同一张）。

**方法论**：程序化/内嵌的图形要「亲眼看过产物」（D39 的教训）。这里额外加了一个
`#[cfg(test)]` 钉住字节头——它不是 128×128 的 PNG 就不该内嵌，而尺寸不对时
`img()` 会照样渲染、只是糊，那一眼看不出来。

#### D51 · 标题栏图标与导航轨对齐：左边距由轨宽**反推**，不是另填一个数（2026-10-03 实机三轮）

实机实测：标题栏图标中心在 x=29.5，导航轨那一列的图标中心在 x=35.5，
**差 6px**，肉眼看过去就是"没对齐"。

落法是把左边距定义成**派生量**：

```
TITLE_BAR_PAD_L = NAV_W / 2 - APP_ICON / 2
```

轨的中心线就在内容区的 `NAV_W/2` 处（`v_flex` 的 `items_center` 对称居中），
所以这条恒等成立——**改了 `NAV_W` 或 `APP_ICON` 也不会再错位**，
而不是像"再调两像素"那样改完就忘了它依赖谁。

覆盖官方自带的 `pl(12)` 靠 `refine_style`（排在官方 `.pl(..)` 之后）。

#### D52 · 导航轨图标放大用 `Size::Large`，并删掉被 `Button` 覆盖的 `icon_md`（2026-10-03 实机三轮）

轨上 svg 图标只有 16px，在 56px 宽的轨里显得空。AGENTS.md 已禁
`Size::Size(px(..))`（官方那条路会把文字一起放大到 16px、图标放大到
size×0.75，正是"整屏图标过大"的根因），所以只能找命名档。查基座
（`button.rs:615-618` + `661-666` + `icon.rs:187-193`）得到一张表：

| 按钮档 | icon-only 外框 | 图标尺寸 |
|---|---|---|
| `XSmall` | 20 | 12 |
| `Small` | 24 | 14 |
| `Medium` | 32 | 16 |
| **`Large`** | **32** | **24** |

**`Large` 的外框和 `Medium` 一样是 32px，图标却从 16 变 24** ——
正是"框不变、图标变大"要的那一档，没有第二档能这么调。三个导航键与
设置键统一改 `Size::Large`。

**顺带删掉一处骗人的调用**：`.icon(icon_md(icon).text_color(ink))` 里
`icon_md` 设的尺寸**会被丢弃**——`Button::render` 无条件
`icon.with_size(icon_size)`，而 `icon_size` 只在 `content_style(..)`
那条内部路上能被外面塞值，公开 API 给不了。这与 D44 是同一个形态
（"编译得过"≠"会被读"），照旧：**连调用带注释一起删**，
换成不写尺寸的 `nav_icon()`，只留 `.text_color(..)`。

#### D53 · 标题栏 48→40、导航轨 56→44，且轨宽与内边距**成对**（2026-10-03 实机三轮）

两个都是"太宽"：

| 构件 | 改前 | 改后 | 理由 |
|---|---|---|---|
| 标题栏高 | 48 | 40 | 那多出的 14px 不是呼吸，是把窗口上沿吃掉一条，3456px 宽的屏上尤其明显；40 仍在 Windows 习惯的 32–40 区间 |
| 导航轨列宽 | 56 | 44 | 轨上只放 32px 的图标按钮，56 宽里有 24px 是空的 |

**成对的是内边距**：`NAV_W − 2 × NAV_PAD` 必须正好等于按钮边长（32）。
`nav_rail()` 里加了一条 `debug_assert_eq!` 钉住它——`items_center()`
会把按钮摆在 32 与 44 之间某处，而 D51 那条对齐是按 `NAV_W/2` 算的，
轨一变窄而内边距没跟着变，按钮中心线就会跑偏。

连带更新：窗口下限的算式（1080 − 轨 44 − 侧栏 200 = 主区 836，
抽屉展开再减 320 仍有 516）。

⚠️ **轨的内边距从此是三档，不是两个**（D58 / D59）：

| 常数 | 值 | 它被什么钉死 |
|---|---|---|
| `NAV_PAD`（横向） | 6 | 轨宽 44 减去它必须装得下按钮并居中（内容盒 32，按钮实测渲染 26） |
| `NAV_PAD_TOP`（纵向顶） | 14 | D58 那条对齐基线：首个导航图标中心 ≡ 页头「总览」字中心 |
| `NAV_PAD_BOTTOM`（纵向底） | 12 | D59：齿轮是独立一组，与窗口下沿之间要"能分开的气口" |
| `gap`（按钮之间） | `GAP_LG` = 12 | 三个各自独立的按钮，不是"一串同类行" |

**纵向顶与纵向底必须分开**：早先三者共用 `NAV_PAD`(6)，实机看着"logo 和三颗图标
糊成一片"；修完顶部又漏了底部，齿轮直接坐在状态条上沿（离下沿只有 5px）。
`tokens::tests::the_three_rail_paddings_never_collapse_into_one` 钉住了这条——
**同一个错犯过两次，就该有测试拦第三次。**

#### D54 · 总览活跃度：官方 `BarChart` → 官方 `AreaChart`（2026-10-03 实机三轮）

用户要「波浪线表」。基座有三种候选，看过 API 选 `AreaChart`：

* `LineChart` 是纯折线，14 个点几乎全是 0 时那条线会贴着底走；
* `AreaChart` = 折线 + 底下填充，正是"波浪"的读法。

落法：

* 曲线**不用显式 `.natural()`**——`Curve` 的 `#[default]` 就是
  `Natural`（`plot/mod.rs:229-238`，d3 的 curveNatural），
  写出来只是把默认值再说一遍；
* 填充用 `accent.opacity(0.12)` 而不是实心 accent：**整块实心色会把
  网格线吃掉，曲线反而读不出来**，压到 12% 才只当"底"；
* `AreaChart` 没有 `BarChart` 那套 `band / label / corner_radii`——
  **柱顶标签是随组件一起没有的**，日期信息改由 x 轴 `M/D` + tooltip
  标题承担（这两处 `BarChart` 时代就已经是这个分工，见 D43）。

`panels.rs` 里那个单仓库迷你图**保持柱状图**——用户说的是"总览柱形图"，
迷你图是另一个尺度、另一个场合（详情页窄栏），换过去反而看不清。

**沿用 D42 的结论**：图表仍然没有 `Styled`/`ParentElement`，
外层 `div().flex_1().min_h_0()` 那句不能省。

#### D55 · 总览表格七列跟随窗口缩放：等比 + 上限 + 余量给文本列（2026-10-03 实机三轮）

原来七列是写死的 220/84/120/104/64/72/72 = 736px 合计，在 3400px 宽的屏上
只占 **22%**，右边空一大片。

照字面做「等比缩放」会出事：3369px 的内容区 ÷ 736 = **4.6 倍**，
「归属」那列会变成 385px 宽的一片空白，比不改还难看。所以落法是三步
（纯函数 `ov_column_widths`，**5 个单测**）：

1. 按 `avail / 736` **等比**缩放——窗口在常见区间里这就是纯等比，
   七列相对关系完全不变；
2. 倍数夹到 `1.6`，每列抬到各自下限（窗口窄到极致时等比会把表头压断字）；
3. **余量按「仓库名 : 分支」的基准比例补给这两列**，保证七列合计
   **恰好铺满**可用宽度。

第 3 步是必须的：合计少一点右边空一条，多一点冒横向滚动条、表格会晃。

**⚠️ 本段已被 D56 推翻，保留原文是为了留住"为什么会错"**：

> ~~**宽度量的是「上一帧实测的表格内容区宽度」而不是窗口宽度**——
> 表格外面还隔着导航轨、分隔线、页面内边距、卡片描边，纵向滚动条再吃掉一条，
> 只有表格自己的 `horizontal_scroll_handle.bounds()` 知道自己实际有多宽
> （基座 `state.rs` 算可视列范围读的就是它）。首帧布局还没跑、句柄是 0，
> 才退回按窗口宽估算（`OV_TABLE_CHROME`）。~~

错的正是"只有表格自己知道自己有多宽"这个理由：**那个宽度是被列宽决定的**，
拿被输出决定的量当输入就是正反馈环。现行口径见 D56：
`avail = window.viewport_size().width − OV_TABLE_CHROME`，**开环**。

**宽度存在委托里，不放 `Shell`**：刷新列宽跑在 `Shell::render` 里面，
那一刻 `Shell` 的数据正被可变借用着；改宽度要 `update` 这张表，
而 `TableState::refresh` 内部会回调 `delegate.column()`。委托要是那时
去 `shell.read(cx)` 读行数据，就是一次借冲突——**与 D45 同源**。
行数据仍每次 `render_td` 读（那时 `Shell::render` 早返回了，安全）。

另外：`TableState` 把列宽**缓存**在 `col_groups` 里，只在 prepare/refresh
时问一次委托，所以宽度变了必须 `refresh()`；而 `refresh` 自己不 notify，
不补那一下 `cx.notify()` 的话拖窗口会明显滞后一帧以上。

#### D56 · 列宽跟随窗口的那一版喂回了自己的输出，界面持续往右拉伸（2026-10-03 实机三轮）

D55 第一版把「可用宽度」取成**表格上一帧实测的内容区宽度**
（`horizontal_scroll_handle.bounds()`）——理由是"只有表格自己知道自己有多宽"。
**这个理由很有说服力，也正是它错的地方**：那个宽度确实准，可它是被**列宽**决定的。
拿被输出决定的量当输入，就是一个正反馈环：

```
列宽合计一大 → 出横向滚动条 → 内容区被压窄 → 量到的 avail 变小
            → 列宽变小 → 滚动条消失 → avail 变大 → 列宽又大 → ……
```

而**鼠标在窗口里动一下就触发一次重绘**，于是每一帧都在这条环上走一步，
表现为界面持续往右拉伸、最后超出窗口。

**三条判据，都是这次学到的**：

1. **纯函数测试抓不到它。** `ov_column_widths` 本身完全正确，它只是被喂了一个
   会漂的输入。单测全绿、单测也**应该**全绿——问题不在函数里。
2. **验收脚本要能制造扰动。** 原来只抓两帧静态画面，环是静止的，抓不出来；
   改成"抓一帧 → 在窗口里划 60 下鼠标 → 再抓一帧 → 求 `ImageChops.difference`
   的 bbox"，立刻看出整块在动。**差分是抓反馈环最省事的办法**（新增 `stir.ps1`）。
3. **落法是开环**：布局量的输入**只许来自外部不可变量**（窗口宽），不许来自
   自己的输出。窗口不动，列宽就不动。

代价是外面那一圈固定开销（`OV_TABLE_CHROME`）要自己算准，且**宁可多扣**：
少扣 = 合计超出可用宽度 = 出横向滚动条（回到环的起点）；多扣只是右边空十几像素，
那是一次定死的静态状态。补单测 `total_never_exceeds_the_given_width`
（窗口 1080..4000 全区间扫）把"绝不超出可用宽度"钉住。

#### D57 · 「并成一个子项」只并了一半，仓库页仍然错位（2026-10-03 实机三轮）

D49 把图标+标题并成了一个 `h_flex` 子项，可侧栏开关 `SidebarToggleButton`
仍挂在 TitleBar 上。**总览页没有开关（一个子项，看着正常），切到仓库页就成了
两个子项**，`justify_between` 把开关推到左、整块图标+标题推到右——
"青鸟 Git"又贴回窗口控件旁边。

独立成条是因为它和 D49 是不同的失效形态：D49 是**查错了基座行为**，
D57 是**修法只覆盖了一半分支**。留成独立编号是为了下次搜"仓库页标题栏"能直接命中。

**教训**：一个"只在某些分支里犯"的错，只跑默认那一屏的自查抓不到。

#### D58 · 导航轨：三处间距各由一条不同的理由定死（2026-10-03 实机三轮）

用户连着提了两次间距：「logo 和三颗图标太近」→ `NAV_PAD_TOP` 6 改 20；
「三个图标都快粘到一起了」→ `gap` `GAP_TIGHT`(4) 改 `GAP_LG`(12)；
「图标整体上移，以上方总览图标与『总览』字一行对齐为基线」→ `NAV_PAD_TOP` 20 改 **14**。

**最后那条是对前一条的收紧**，所以 `NAV_PAD_TOP` 不是"呼吸量"这种审美值，
而是一条**可验证的对齐基线**：

```
图标中心 = 标题栏下沿 + NAV_PAD_TOP + 按钮高/2
标题中心 = 标题栏下沿 + 页面内边距(GAP_LG) + 标题行高/2
```

实测（3456×1408 抓帧，Python 扫暗像元行带）：页头「总览」字带 `66..82`、中心 74；
导航第一个图标字带 `73..87`、中心 80 —— 差 6px。改 14 之后两者**同为 74.0**。
推导写进了 `tokens.rs` 的注释，改 `TITLE_BAR_H` 或页头字号时能顺着它重算。

⚠️ 这条基线**只在总览页成立**（仓库页页头是仓库名 + 只读地址条，高度与位置都不同），
它是左上角的"起手式"，不是全局栅格——别拿它当通用规则套到别的屏去。

**按钮之间为什么用 `GAP_LG` 而不是 `GAP_DENSE`**：4.2 的间距语义表里
`GAP_DENSE` 治的是「一串同类**行**相邻两项」（列表行、色块分段）——那些行本来
就是一整块、行内还有内边距，贴着才连得起来。导航轨上是**三个各自独立的按钮**，
各有各的命中区、各有各的选中底块，贴着排读起来是"糊成一片"。
它们之间要的是"两个不同的东西"那种能分开的气口，所以往大档走。

#### D59 · 导航轨底部压根没有内边距，齿轮直接坐在状态条上（2026-10-03 实机三轮）

用户：「下方的设置图标离底也太近了一点」。查下来是**压根没有这一档**：
齿轮是 `v_flex` 里最后一个子项，撑开的 `div().flex_1()` spacer 把它顶到轨底，
而轨底紧挨着 28px 的状态条——26px 的按钮压在它上沿。

实测（抓帧量到状态条上沿那条发丝线）：改前齿轮字形底距那条线**只有 5px**。
落法 `NAV_PAD_BOTTOM = GAP_LG`（12），改后 **17px**，正好等于新增的那一档。

取 `GAP_LG` 而不是更小的档，理由**和按钮之间的 `gap` 是同一条**：齿轮是
独立的一组（打开设置），跟上面三颗切屏键不构成"一串同类行"，它与窗口下沿
之间要的是"两个不同的东西"那种能分开的气口。

⚠️ **同一个错犯过两次**：早先顶与底共用 `NAV_PAD`(6)（D58 已记），
修完顶部漏了底部。补 `tokens::tests::the_three_rail_paddings_never_collapse_into_one`
钉住"三处内边距不许并成一个数"。

#### D60 · 选中面：`theme.json` 没配的那一项，落到了基座兜底的深暖灰（2026-10-03 实机三轮）

用户：「图标的高亮颜色太深，透明度遮罩高亮就行了，高亮适应全局同类」。

根因不在配色，在**配置漏项**：导航轨走官方 `Button::ghost().selected(true)`，
底色取 `button.active.background`，而 **`gui/theme.json` 根本没有配这一项** →
落到基座兜底 `self.input.mix_oklab(transparent, 0.7)`，也就是
"输入框底色的 70%"。实测渲出来是 `#EFEEEC`——**一块偏暖的实底**，
在一屏灰白里读起来是"贴了块色板"而不是"蒙了层纱"。

落法 `tokens::selected_mask(t) = foreground.opacity(0.08)`：

* **0.08 是反推出来的**，不是审美值：白卡（#FFFFFF）上叠前景色（#1A1A1A），
  要落到设计规范板的「选中面」#EDEBE7，`a ≈ (255−237)/(255−26) ≈ 0.079`。
  实测叠出来 **#EDEDED**，与规范板差 0/2/6 —— 1:1 下与侧栏仓库行的选中面
  **不可分**。这就是"适应全局同类"的可验证形式，补单测
  `mask_lands_on_the_design_boards_selection_face`（阈值 8/255）。
* **为什么用遮罩而不是往 theme.json 再填一个实色**：实色要挑一个"在白轨上好看"
  的值，换个底就又不对；遮罩是前景色按比例压一层，**在任何底上都成立**。
* **能盖掉官方底色的机制**：官方在**每个状态层的最后**重放调用方样式
  （`button.rs:733` 与 `:795` 的 `refine_style(&instance_style)`），
  所以 `.when(active, |this| this.bg(selected_mask(t)))` 压得住。
  ⚠️ **`when` 不能省**：同一份 `instance_style` 在常态层也会被重放，
  无条件 `.bg()` 会让三颗按钮统统带上底。
* **"同类"的范围**只有两处，都是 ghost 图标按钮的选中态：导航轨三颗切屏键
  （`shell.rs::nav_item`）与侧栏那颗筛选按钮（`sidebar.rs`）。两处一起换。

**留了一条没改的同类项，已开 Q12**：ghost 按钮的**悬停**面是**实心 `accent` 蓝**
（`button.rs` `hovered()` 的 `Self::Ghost` 臂，浅色模式不降透明度），
实测渲成 **#5B7A99**——比选中面重得多，"扫过一颗没选中的图标"比
"我现在在哪一屏"还抢眼。没顺手改的两个理由：

1. 正解是换 `ButtonVariant::Custom(ButtonCustomVariant)`，而那会连带改掉
   **焦点环 `Edge`→`Inside`**、`button.rs:699` 那条只给 `Default`/`outline`
   加边的描边逻辑、以及 `normal()` 那条臂——是**重新设计这颗按钮**，不是改个色。
2. 后门也不通：调用方再调一次 `.hover(..)` 盖不掉它，官方在 `render` 里已经
   注册过悬停层（`button.rs:713`），`div.rs:843` 是
   `debug_assert!(hover_style already set)`，**debug 构建直接 panic**
   （实测退出码 101）。而 `debug_assert!` 在 release 下又是空操作、
   会"静默地看起来能用"——**同一份代码两种构建两种行为**，D40 吃过一次这种亏。

#### D61 · 同一个 `ElementId` 一帧出现两次 → debug panic，而那棵树平时根本不建（2026-10-05）

概览页改版收尾时抓到一个 debug 构建的崩溃：
`Duplicate a11y node id: #6359161687446339954`（`a11y.rs:415`）。**它"偶发"——
重跑同一份代码又正常渲染**，所以第一反应是当噪声放过。三步查到底：

1. **判据是整条 id 路径，不只是最后一段。** `GlobalElementId` 是「**有 id 的
   祖先**」串起来的链（`element.rs:230-235` 对整条链 hash），**没有 id 的容器
   是透明的**。所以"两个同名按钮在不同分支上"**不构成辩护**——父链一样就是撞。
2. **那棵树只在「系统/读屏软件接入」时才建**（`a11y.rs:212` 的 `active_flag`，
   由 `window.rs:1626-1647` 的平台回调置位）。没有环境变量、没有公开 API 能强开
   ——`Application::new_inaccessible` 恰恰是把它**关**掉。所以平时整棵树不建、
   断言根本不跑，**"我跑了一晚上没崩"证明不了任何事**。
3. **自己当那个 UIA 客户端就能确定性复现**：.NET `UIAutomation` 碰一下窗口
   （`scripts/uia_probe.py`）。激活之后 flag 一直有效，**每次重绘都重建整棵树**，
   于是 `scripts/a11y_stress.py` 能把潜伏在别的状态里的同类重复一起压出来
   （本轮压了 18 步：五个页签 / 右栏收展 / ⌘K / 总览 / 第三方）。

实情：`inspector_reopen` 被 `repo_head.rs` 与 `sync_card` **各调了一次**，
两颗都是 `Button::new("inspector-open")`，右栏一收起就同帧出现。
落法：**删掉 `sync_card` 里那处，只留 `repo_head` 的**——它在页签**之上**，
五个页签都够得着；同步段那颗只有「概览」这一屏露得出来。

⚠️ **这与用户当天判「常用命令段重复、删掉」是同一类错误**：同一个功能在两处
各摆一个入口。所以新增任何**"条件出现"的入口**（收起时才露的键、悬浮才现的键）
之前，先确认它在一帧里只挂一次。

⚠️ **想知道是哪两个元素撞的，别猜**：基座**已经把第一次出现那个节点的
`element_id` 与 `source_location` 记下来了**（`element.rs:391` 的 `record_node_info`），
只是 `can_push` 撞车时没读它。本轮把 `gpui-pre` vendor 一份到 `vendor/` +
`[patch.crates-io]`，在 `can_push` 里补两行 `eprintln!` 读
`self.node_info.get(&id)`，一次就读出 `Name("inspector-open")`——
**比回溯栈快得多**（本项目自己的帧在回溯里被优化掉了，栈上只剩基座与系统帧，
只能读出"某个 resizable 面板里的一颗 Button"）。**核完 patch 与 `vendor/` 一起删**
（本轮已删，`gui/src` 与 `Cargo.toml` 无残留）。

---

#### D62 · 说明页改造：表头 + 中英切换 + 链接四分类（三条路径都得**点过**才算数）（2026-10-05）

用户两条追加：① 说明页顶部那排（`📄 README.md` + 「在文件管理器中查看」）
按日志段的表头做法**收进卡内**（原来浮在卡片外面，一屏两个头）；② 有中/英双份
说明文档时给两颗可点的键；③ **README 自带的跳转链接要真的能跳**。
用户同时定下一条设计原则并已记入记忆：**我的仓库与第三方仓库界面基本相同，
只是操作功能不一样**，哪些功能要隐藏由用户逐个拍板，最实用的形态是
「模块化 + 可勾选显示/隐藏」。

**落法**：core `readme_parts` / `text_is_zh` / `readme_cands` / `readme_alt` /
`read_readme_named` / `is_readme_name`（+4 组单测，core 128 → **132**）；
`readme_tab.rs` 重写：卡内表头（文件名 / 中英两颗 XSmall ghost 键 / 竖线 /
在文件管理器中查看），链接按 `classify_link` 分四类各走各的路——
`#锚点` → 文档内同名标题（`reveal_range`）、**根目录的 `README_*.md` → 切文档**
（双语 README 的中英链接多半就是它）、其它相对路径 → 系统默认程序、
`http(s)/mailto` → 系统（+2 组单测，gui 12 → **15**）。

⚠️ **三条路径的第一版全是坏的，只有实机点出来**（`cargo check` 零错、单测全绿、
抓帧"看着没问题"）：

1. **点「中文」永久停在骨架屏，表头也跟着空掉。** 根因是守卫写反了：
   `load_readme_doc` 写成 `let seq = self.profile_seq; self.profile_seq += 1;`
   ——守卫拿的是**加之前**的值，回来一比永远对不上 → 闭包静默早退 →
   `scanning` 永远落不回 `false`。另外三处扫描（`scan_space` /
   `scan_profile` / `scan_overview_activity`）**都是先自增再取值**，
   只有这一处反了。**表头空掉是同一个 early return 的连带**：骨架分支在
   `readme_header` 之前就返回了，一处早退吞掉两块画面。
   **教训**：代数守卫的"取号"与"发号"必须成对写，反了不会报错、只会静默早退；
   同一份代码里另三处怎么写的，就是现成的对照答案。
2. **目录锚点点下去没反应。** `anchor_range` 原来取**第一个**整行相等的行，
   而 `* [API](#api)` 与 `## API` 在渲染后的纯文本里**是同一行 `API`**
   ——落点永远是用户手指底下那行目录项。目录项与标题在纯文本里确实无从区分
   （基座没有公开的块结构接口，去摸 `parsed_content` 就是拿内部实现当接口用），
   **但位置上有规律：README 的目录总在被跳的那一节前面** → 改成取**最后一个**
   匹配。这同时让双语 README 顶上的 `[English](#english)` 行为与 GitHub 一致
   （跳到文末那节 `## English`）。实测 range 从 1543（目录项）变成 2918（真标题），
   画面随之滚过去。
3. **子目录里的 README 被误判成"切文档"。** 原来 `classify_link` 只取 href 的
   最后一段文件名，于是 flowix 的 `[dsh-flowix-memory](dsh-flowix-memory/README.md)`
   会被切成**根目录**那份——点一下悄悄给了另一份文档，比没反应更难查。
   改成**只有 href 没有目录部分时才算切文档**，带目录的整个相对路径都留着
   交给系统（`Path::join` 拼仓库根目录即可，连 `../docs/x.md` 也一样）。

⚠️ **基座那边是"最小滚动"**：`reveal_range` 只保证那一行**进视野**，
落在一侧边缘而不是顶端。想顶对齐得自己去驱 list 的 `ScrollHandle`，
而 `TextView` 不把它露出来——不值当，注释里写实即可。

**验证工具**（这两个是这个 GUI 能被 agent 验收的前提，补进了 `scripts/`）：
- `scripts/steps.py`：跑一串点击并逐步抓帧（`nav` / `row` / `tab` / `click` /
  `wheel` / `wait` / `shot`）。**单击而不是连点两下**——连着两次 down/up 会被
  当成双击，而且第一次的抬起常常落在窗口还没激活的时段里，白点；
  点击前先轮询 `GetForegroundWindow` 确认前台（AGENTS.md 那条"第一击只用于
  激活窗口"的纪律，落到脚本上就是这一段）。
- `scripts/frame.py`：截图三件事——`crop` 放大读小字、`stack` 几帧竖着比、
  `diff` 求改动像素数与包围盒。**坐标别目测**：本轮有一次目测偏了 43px，
  点了个空处，还以为功能坏了（实际是"点击没触发"，靠 diff 只有 0.02% 才发现）。
  往返验证的判据用**逐像素相同**：点中文键 → 点正文里的 `English` 链接回到
  英文那份，与初始帧 diff = **0 像素**。

---

### 阶段 13 · 四个页签的版式统一 + 空间页重做（2026-10-05 用户逐页签推进）

用户口径（一次给全，顺序即施工顺序）：

1. **日志页按建议走**——两层边框里，空态那层里框显得多余。
2. **空间页**「主要就是显示出仓库的所有文件夹及文件，**只读，不可删**」，
   另外「可扫描出**可安全删除的条目可以一键删除**」，版式与日志、说明页统一。
3. **历史页和变更页现在的内容没有问题**，只把排版风格统一一下。

拍板的两条（问卷，未自己替用户选）：

* 空间页的清单形态 = **可展开的目录树**，每行一个文件夹或文件、文件夹带递归体积，
  **点一下才扫下一层**（否认可展开/可平铺/只列目录）
* **缓存目录（`target` / `node_modules` / `dist` …）与 `.git` 都进树，但默认折叠成一行**
  （否决完全排除 / 一律平等展开）

* [x] **13-1 · 收口「卡外壳」与「卡内表头」为两个共用函数**
  * 起因是查出来的分叉：空间页**自己手搓了一份外壳**（内边距 **16**），
    日志 / 说明用 `log::section`（**8**），历史 / 变更**根本没有卡**——
    五个页签摆在一屏里，内边距有两个值、有的有壳有的没壳
  * `widgets::card(t)`（白卡 + 1px 边 + `R_XL` + `p(GAP)`）与
    `widgets::CardHeader`（24px 行高 + `FS_BODY`/`MEDIUM` 标题 + `flex_1` 顶开 +
    右侧 `Vec<AnyElement>`，可选图标位与附注）——`log::section` 删掉，不是留一层转发
  * 右侧操作的分组关系**留给调用方**（`vdivider` 插在中间）：日志是
    「复制 / 清空 │ 终端 / 开发运行 / 编译构建」两组，说明是「中文 / English │ 在文件管理器中查看」
  * 历史页：裸列表 + 一行浮在上方的小字 → 白卡 + 卡内表头（标题带条数，右边一颗重扫）
  * 变更页：裸清单 → 白卡 + 卡内表头（标题是文件数，右边「提交并推送…」，第三方不出现）
  * 空间页：「重新扫描」从整页最下面那个孤零零的按钮挪到「磁盘占用」那一段的表头右上
    ——它扫的就是这一段
  * 空间页段间距 `GAP_LG`(12) → `GAP`(8)，与其余各页一致
  * **日志空态不画里框**（有内容保留）：那圈 1px 边 + 浅灰底在有内容时干的是
    "把等宽输出与白卡分开"，空的时候它只剩一个套在白卡里的空盒子
* [x] **13-2 · 空间页目录树**（core 增量扫描 + GUI 树，只读）
  * **core**（`crates/core/src/space.rs`）
    * `DirSize` 与新的 `DirEntry` **合成一个**（字段重叠，两个类型必然漂）
    * `list_dir(repo, rel)`：列**一层**不递归下钻（目录的 size 仍是递归 walk——
      一个文件夹占多少是这页的本题）；符号链接 / junction 一律跳过；
      排序走 `sort_entries`（**目录在前 + 名称序，大小写不敏感**，与资源管理器一致）
    * `safe_rel(rel)`：拒绝 `..` / 绝对路径 / 盘符 / 反斜杠。`rel` 来自界面的树节点 id，
      **今天安全不等于明天安全**
    * `child_count`：只数一层，0 = 空目录 → 界面上据此决定画不画箭头
    * `human_bytes`：B/KB/MB/GB 四档。`human_size` 一律折成 MB，4 KB 的文件显示成
      「0 MB」，清单里满屏 0 MB 等于没给信息
  * **GUI**（`gui/src/space_tab.rs` + `state.rs`）
    * **不用官方 `component::tree`**（查证写在 `state.rs::TreeNode` 上）：
      `TreeItem::is_folder()` 是 `!children.is_empty()`（`gpui-base/src/tree.rs:132-134`），
      而**懒加载的文件夹在扫之前 children 必然是空的** → 基座 `toggle_expand` 第一行
      `if !entry.is_folder() { return }` 把它挡掉，那个文件夹永远展不开；也没有
      "展开时去加载"的钩子。代价 = 自己画行，少了键盘走位与虚拟滚动——这一页是
      只读浏览，行数由"用户展开了多少层"决定，`v_flex` 扛得住
    * 树的根层**不重新扫**，直接吃 `scan_repo_space` 的 `dirs`（那一层的递归体积
      刚算过，再走一遍等于把整个仓库重数一次）
    * 用 `.detach()` + 代数守卫，**不存任务句柄**：这一层是"多点一下就多扫一层"，
      挡过期结果比互相取消好。`tree_load` 的 `tree_seq` **先自增再取值**（D62 那条）
    * **过期的结果也必须先清 `tree_loading` 再 return**，否则用户盯着永远转下去的图标
    * 文件行**不给 `on_click`、不给箭头**——不画一个点不动的箭头，也不给能按的鼠标手型
    * 图标位固定 14px，折叠只换字形（`Folder` ↔ `FolderOpen`）不靠缩放
    * 树封顶 `TREE_MAX_H = 420` 独立滚动：不封顶的话用户那些仓库（实测 Locowrk
      98 GB / qingniao 28 GB）展开几层就上千行，下面「构建缓存」那几段会被顶出屏幕，
      而那才是这一页唯一能动手的地方
* [x] **13-3 · 「可安全删除」一键清理**（确认对话框逐项列清 + 保留逐项删）
  * 「一键清理」挂**「构建缓存」那一段的表头右上**（它清的就是这一段），
    逐项那颗键**保留**——用户很可能只想删 `dist` 而留着 `target`
  * `dialogs/clean.rs`（新）：逐项列出删什么、放多少，**按体积从大到小排**（最贵的
    在最上面）；一句橙色后果提醒；**确认键用 `danger` 不用 `primary`**——不可逆的
    动作放主色按钮上等于在鼓励用户点它
  * core `delete_cache_dirs`：**逐项独立**，`target` 被占用而删不掉不该让同一批里
    的 `node_modules` 也留着（用户点的是"全清"）
  * 结论落进**那个仓库自己的日志槽**（`log_of(path)`），不是 `app_log`——后者是给
    "横跨多个仓库的批量汇总"用的，概览页的日志卡读不到它
  * **没删成的逐条写进日志**：只把成功的写上等于骗人

**出口条件**：13-1 ✅ 零错零警、gui 15 单测全绿、四页抓帧逐页核过。
13-2 / 13-3 各自写死：① `cargo check --workspace` 零错零警 ② core 单测 ≥ 现有 132 + 新增
③ 树能展开到第三层、缓存与 `.git` 默认折叠 ④ 逐项点得动（**往返 diff 作判据**，
D62 那条纪律）⑤ 一键清理必须先过确认框，框里逐项列出删什么、放多少。

**13-2 / 13-3 验收结果（2026-10-05）**：`cargo check --workspace` 零错零警；
core **141** 单测、gui **19** 单测全绿。验收在 `$TEMP` 里现造的合成仓库上做
（`target` / `node_modules` / `dist` 三个缓存目录 + 四层嵌套 + 一个空目录），
**用户自己的 17 个仓库一个都没动**，config 也是拷到 `$TEMP` 跑的副本。
- 树：四层展开（`src → alpha → beta → gamma`）逐层点得动，缩进递进、文件行无箭头
- **往返 0 像素**：展开 → 收起全部 → 再展开，两次 diff 都是 0 / 1073088
- **递归体积逐项核对无误**（`gamma` 12 B = 两个 6 B 文件 → `beta` 18 → `alpha` 24 → `src` 40）
- 行距用 `scripts/rowpitch.py` **实测 28.0 统一**（`TREE_ROW_H` 26 + `GAP_DENSE` 2）；
  目测曾以为有 27/28/31 三种，量完发现是滚动视口裁切造成的测量偏差
- 一键清理：取消 → 与打开前 **0 像素差**且三个目录都还在；确认 → 磁盘上只剩
  `.git` / `src` / `README.md`，三段自动重扫
- **占锁场景**（用 `FileShare.None` 持句柄）：`node_modules` / `dist` 删掉、
  `target` 留着，日志两行——`已清掉 2 个目录，释放 440 KB；1 个没删成：target` +
  `target 没删成：删除失败：目录被占用…`——**不连坐**成立

### 13 阶段里改掉的三个真缺陷（都不是设计问题，是查出来的事实）

1. **同一棵树里两套顺序**：根层白捡 `report.dirs`（那是"谁最占地方"那个榜，
   按体积降序），子层走 `list_dir`（名称序）——根层按大小、子层按名字。
   收法：排序规则收进 core 的 `sort_entries`，`tree_reset` 再排一次。
   **教训**：白捡一份已经排好序的数据时，要确认它排的**是不是这一处要的序**
2. **同页两种体积格式**：树里是 `781 KB`，上方图例却是满屏 `0 MB`。
   收法：清单与结论一律 `human_bytes`，`human_size` 只留给确实要粗粒度的地方
3. **清理结论永远看不到**：只写了 `space.note`（活在这一页的状态条上），
   切到概览页就没了；`app_log` 又是给批量汇总用的另一个槽。
   收法：写进 `log_of(path)`——**那个仓库自己的**日志槽
4. 附带：`delete_cache_dir` 里那句"目录被占用"友好提示**在 Windows 上永远不触发**。
   `ERROR_SHARING_VIOLATION`(32) 被 Rust 归成 `Uncategorized`，不落进
   `PermissionDenied` 那个臂，于是最常见的那种占用恰恰只剩一句 `(os error 32)`。
   收法：按 `raw_os_error()` 显式认 5 / 32 / 145 三个号

### 13 阶段查到的环境事实

用户 6 个「我的」仓库里**四个是超大的**：Locowrk 98 GB（128045 个文件）、
qingbird-md-rust 36 GB（86497）、qingniao 28 GB、qingbird-git 本体。
只有 flowix 98 MB / Guanmo-open 67 MB 是轻量的。
后果：`scan_repo_space` 递归算体积在这些仓库上要**几十秒**（PowerShell 裸走一遍
qingbird-md-rust 就要 43 秒），空间页会长时间停在骨架屏。这是既有行为不是本阶段
引入的，但**下一阶段值得单独处理**（增量 / 缓存上次结果 / 边扫边出）。

---

## 未决事项

遇到要在这些点上拍板的，写进这里并停下来问，**不要自己选**：

| # | 事项 | 现状 | 需要谁拍 |
|---|---|---|---|
| Q1 | 侧栏过滤框 | ✅ 已定：加，默认收起（D8）。阶段 4 一并做 | — |
| Q2 | 标题栏全局搜索 | ✅ 已定：删掉（D9）。Shell.global_search 字段与输入框在阶段 1 一并移除，阶段 2 标题栏改用官方 TitleBar | — |
| Q3 | ⌘K 里批量同步的交互形态 | ✅ 已定（2026-10-03 用户拍板）：**固定三条命令 + 按分档自动筛**。「拉取全部」只跑 `behind>0`、「推送全部」只跑 `!第三方 && ahead>0`、「检测全部」扫全部。回车即跑不弹确认。理由见阶段 11。阶段 11 已接 | — |
| Q4 | 设置页是独立页还是对话框 | ✅ 已定（2026-10-03 用户拍板）：**导航轨齿轮 → 一个 `Dialog`**。不新开一屏，`Page` 两态是 D2 的硬约定，设置属低频层。阶段 11 已接（`dialogs/settings.rs`） | — |
| Q5 | 状态条要不要留"上次扫描时间" | 已按「留」实现并一直用着（点它重扫）。**这一项我始终没给过建议**——「按建议推行」没有覆盖到它。倾向：留着，状态条那一格空着比摆一个没用的时间戳差；但一行字就能删 | 用户 |
| Q6 | 旧的 src/（React）何时退役 | ✅ **已完成（2026-10-05 用户批准删除，同日执行）**。分两轮走的理由是风险不对称：那 8 个 `.test.mjs` 里 3 条规则删了就找不回来，而删除本身有 git 兜底。**第一轮**（`689651b`）补 6 组新单测覆盖仍归我们管的规则（gui 19 → 32）、README 命令全部换成 cargo、`docs/shots/` 只留 `current.png`。<br>**第二轮（已执行）**：删掉 `src/`（64 文件）、`src-tauri/`（42 文件）、`public/`（3）、`index.html`、`vite.config.ts`、`tsconfig.json` + `tsconfig.node.json`、`package.json` + `pnpm-lock.yaml`（`test:web` 一并退役），以及 `scripts/gen-icons.mjs`（它调 `@tauri-apps/cli`、产出目标是 `src-tauri/icons/` 与 `src/assets/`，两个都没了才失效），外加三个本地产物 `node_modules/`（88.8 MB）、`dist/`、`debug.log`（旧 WebView2 的 crashpad 日志）。跟踪文件 206 → **90**。<br>**清单修正一处**：`assets/` 原写着要删，实查里面只有 `icon-source.png`（1024px **图标源图**，而 `gui/assets/app-icon.png` 是它的 128px 展开）→ **保留**，删了将来改图标就没有源了。<br>删前逐条查过引用：`crates/` + `gui/` 里对旧实现的提及**全在注释里**（"平移自 X，两处与旧版不同"这类溯源），**零代码依赖**；`app_assets.rs` 的 `include_bytes!` 指向 `gui/assets/` 而非 `src-tauri/`；两个 `Cargo.toml` 都没有 node 相关依赖；`Cargo.lock` 里的 `tauri-winrt-notification` 是 `notify-rust` 的传递依赖，与旧实现无关。顺带修掉三处会变错的文档：`Cargo.toml` 的工作区注释、`.gitignore` 的 `src-tauri/*` 两条、`.vscode` 推荐的 `tauri-vscode` 插件 | — |
| Q7 | 五档状态的取色到底听谁的 | ✅ 已定（2026-10-03 用户拍板）：采纳建议档位（synced 绿 / dirty 蓝 / ahead 蓝 / behind 橙 / **fork 红 danger**）。落地改了 `dot_color`、`badge_tone`、分叉条**三处**。阶段 11 已接 | — |

### Q7 长说明：五档取色现在有三套说法

| 档位 | A. 旧 React 版（src/lib/overview.ts + src/theme.css） | B. 当前 GPUI 代码（state.rs::dot_color） | C. UI-DESIGN.md 2.3 + 第 4 节 |
|---|---|---|---|
| 已同步 | 绿 --ok #059669 | 绿 success | 蓝 chart.1 |
| 有未提交 | 橙 --warn（注释里写了，但 RING_COLOR 没这一档） | 蓝 accent | 未指定 |
| 待推送 | 蓝 --accent | 蓝 accent | 绿 chart.2 |
| 已分叉 | 紫 --fork #7c3aed | 橙 chart.3 | 红 chart.4 |
| 落后 | 橙 --warn | 橙 chart.3 | 橙 chart.3 |

要点：

* **A 是唯一用户实际看过的**，且与 B 只差一档（fork）。
* A 给 fork 用紫是有理由的，注释原话是"**危险但非错误**"——分叉要人工介入， 但它不是一次失败的拉取。但 `gui/theme.json` 只有 chart.1–4 四个语义色 （蓝 / 绿 / 橙 / 红），**没有紫色档**；为了 fork 单独加第五个色相， 跟整套"灰白调 + 四个语义色"的体系打架。
* C 里的"已同步蓝"会让**绝大多数干净的仓库**在侧栏和总览里一片蓝—— 但"已同步"恰恰是**不需要动手**的一档，占比最高，不该占强调色。

**我的建议（A 的前三档 + fork 改红）**：

| 档位 | 取色 | 理由 |
|---|---|---|
| 已同步 | 绿 success | 干净是常态也安全，与旧版一致，不回归 |
| 有未提交 | 蓝 accent | 需要动手但不紧急 |
| 待推送 | 蓝 accent | 同上档语义，不另开色 |
| 落后 | 橙 chart.3 | 拉一下就好 |
| 已分叉 | 红 danger | 唯一"不能一键解决"的档，值得最重的色 |

⚠️ 定下来之后要**一起改两处**：`state.rs::dot_color`（状态图标）与 `sidebar.rs::badge_parts`（右侧徽标）现在都把 Behind 与 Forked 画成同一个橙。 本表不擅自改——等拍板。

**Q7 结论（2026-10-03 用户拍板，阶段 11 落地）**：采纳上表的建议档位。
实际改了**三处**（表上只说了两处，漏了分叉条）：

| 位置 | 改前 | 改后 |
|---|---|---|
| `state.rs::dot_color` | Behind/Forked 都是橙 | Forked → `t.colors.danger` |
| `sidebar.rs::badge_tone` | 同上 | Forked → `(danger, red_bg)` |
| `panels.rs` 分叉条 | `orange_bg` / `orange` / `orange_text` | `red_bg` / `danger` / `danger` |

第三处表上漏了：分叉条是"已分叉"这一档唯一一块**大面积**的色块，
环是红的、条是橙的会让人以为那是两件事。已一并改红。
另：`tokens.rs` 的 `orange` 注释里那句"「落后 / 分叉」的橙色"也同步改成了"「落后」的橙色"。

**实机第四轮新开的一条**：

| # | 事项 | 现状 | 需要谁拍 |
|---|---|---|---|
| Q12 | **ghost 按钮的悬停面要不要一起换成遮罩** | ⛔ **等拍板**。导航轨实测渲成实心 `accent` 蓝 #5B7A99，比选中面重得多（详见 D60）。改它要换 `ButtonVariant::Custom`，会连带改焦点环与描边逻辑；调用方 `.hover()` 覆盖在 **debug 下直接 panic**（`div.rs:843`）。**没动** | 用户 |

---

### 阶段 11 新开的三条

| # | 事项 | 现状 | 需要谁拍 |
|---|---|---|---|
| Q8 | **后台自动同步调度器要不要搬** | ✅ **已定并已做**（2026-10-03 用户「按建议推行」）。**阶段 12 全部 11 项已勾**：core 新增 `scheduler.rs`（20 个新单测，含旧版 5 个原样搬过来）、GUI 新增 `gui/src/scheduler.rs` 进程级定时器。**已摆平 Q11 这个前置** | — |
| Q9 | **⌘K 要不要一个可见入口** | ✅ **已定**：维持现状（2026-10-03 用户「按建议推行」）。D9 删掉标题栏全局搜索之后，空态里那一句是 ⌘K 唯一的提示位；且它的键位**不写死**，走官方 `Kbd::global_binding_for_action` 从 keymap 查（D36） | — |
| Q10 | **主题这一项判为"不需要"** | ✅ **已定**：判为不需要（2026-10-03 用户「按建议推行」）。这一版只有一套配色（`gui/theme.json`），给"深色"开关只会切到基座自带深色，与设计 4.1"不要深色面板"冲突。设置对话框用只读 `DescriptionList` 显示当前主题与出处。要真做深色，是**新增一套 token** 的活，不在收口范围 | — |
| Q11 | **托盘 + 关窗常驻要不要补**（查 Q8 时挖出来的） | ✅ **已定并已做**（2026-10-03 用户「按建议推行」）。`gui/Cargo.toml` 加 `tray-icon 0.26` + `muda 0.21`；`main.rs` 加 `with_quit_mode(QuitMode::Explicit)`；新增 `gui/src/tray.rs`（图标 / 三项菜单 / 左键唤起 / 窗口没了就重开）。**按当时写下的约束与托盘同一次改动上** | — |

**Q8 我的建议**：搬，而且应该单独开一个阶段（**已写成「阶段 12」提案，清单与出口条件都在那里**）。
理由——它是**相对旧版的功能回退**，不是"新功能"；用户升上来会以为定时同步还在。
而且地基比想象的厚：配置字段、`parse_hhmm`、`normalize_auto_sync`、乃至
**与用户操作的互斥 `sync::try_begin_op()` 都已经在 core 里了**（后者现在没有调用方，
注释就写着"自动同步只 try_lock"）——**要新写的只有到期判定与那个循环**。
不放进阶段 11 是因为阶段 11 的出口是纯界面收口，而调度器有网络副作用，不该混着验。

**Q9 我的建议**：维持现状（只靠空态那一行 + 文档）。理由——设计 6 的分层表把
「设置 / 切仓库 / 打开文件夹」都归在"低频层 · 仓库头图标 + ⌘K"，⌘K 本身
是**键盘入口**，再给它配一颗图标等于把它升格成"必须被看见的功能"，
与 6.2 删掉的那一堆平铺图标按钮是同一类问题。

---

## 变更记录

| 日期 | 阶段 | 做了什么 | 结果 |
|---|---|---|---|
| 2026-10-02 | — | 建立本表；核实基线（3747 行单文件、core 缺克隆、日志单份全局）；修正 UI-DESIGN.md 四处与实际不符 | D1–D7 落定 |
| 2026-10-02 | 0 | 地基核对完成：cargo check 通过、60 项单测全绿、官方 API 逐个核实；补登记 8 个官方件（TitleBar / StatusBar / Empty / Separator / Spinner / Kbd / Tooltip / Link） | ✅ 阶段 0 出口达成 |
| 2026-10-02 | 1 | shell.rs 4058 行拆成 13 个模块；逐行多重集校验通过（3606 = 3606），确认纯搬迁；顺带修掉 stat_card 文档注释错位 | ✅ 阶段 1 出口达成 |
| 2026-10-02 | 2 | 壳层重做：官方 TitleBar / StatusBar、导航轨收敛成三按钮 + 设置、启动进总览且侧栏收起、常驻 320px 检视抽屉、窗口下限 1080×640、删掉标题栏全局搜索。补 D10（抽屉不是 Sheet）与 D11（扫描入口进状态条，顺带查出旧窗口拖不动关不掉的 bug）。表上「总览页占位」一行改为沿用旧实现，理由见阶段 2 记录 | ✅ 阶段 2 出口达成 |
| 2026-10-02 | 3 | 侧栏折叠 / 拖宽 / 动效：宽度与折叠改由 h_resizable + transition 驱动（补 D12：官方 SidebarCollapsible::Offcanvas 不看 reduce_motion，且与 size_range 硬下限冲突）；core 加 sidebar_w / sidebar_collapsed 与夹取函数，单测 60 → 65；标题栏加 SidebarToggleButton，接 Ctrl B；拖宽松手落盘。顺带修掉一个 serde 坑：裸 #\[serde(default)\] 会把老配置的侧栏宽读成 0 | ✅ 阶段 3 出口达成 |
| 2026-10-02 | 4 | 侧栏树与右键菜单：sidebar.rs 整体重写为「头/身/脚」三段（标题+计数+筛选开关 → 两组树 → 添加/克隆两按钮）；仓库行走官方 SidebarMenuItem（icon 状态字形 / label 名称 / suffix 徽标），补 D13；右键菜单挂官方 context_menu，两类仓库各一套，归属变更与移除都走"执行 + 通知撤销"；新增 actions.rs（reveal_repo 统一四条跳转路径）与 os.rs（explorer/终端/浏览器/剪贴板）；导航第 2/3 按钮与总览点行接到同一个定位入口；browse_url_from_remote 搬进 core 并补 4 个单测，65 → 69 | ✅ 阶段 4 出口达成 |
| 2026-10-02 | 6 | 总览页重写：官方 DataTable（TableState + TableDelegate）七列替掉手搓表格，官方 BarChart 画近 14 天活跃度，四张状态卡去掉边框；复选框列、底部批量条、两个「全部拉取/推送」按钮全删（批量改由 ⌘K 承载，阶段 11），批量链路 run_batch/BatchRun 标 allow(dead_code) 留待接线；点行走 TableEvent::SelectRow → reveal_repo；新增 core commit_activity_all + 纯函数 merge_activity 并补 3 个单测，69 → 72；抽出 sidebar::badge_tone 作为分档取色的唯一实现 | ✅ 阶段 6 出口达成 |
| 2026-10-02 | 7 | 详情页共用骨架：两类仓库并进同一个 detail_page（仓库头 + 官方 TabBar 五页签），新增 repo_head.rs 放共用仓库头（归属徽标 + 四个图标入口 + 只读地址条）；页签按归属裁剪（第三方去空间/同步设置），下标全局但切仓库时夹回范围；「最后提交」由状态扫描顺带算（git log -1，只读 HEAD，O(1)）落进 RepoView；同步动作挪进内容区并按归属裁剪（第三方无推送）；推翻阶段 4 的「在云端打开只给第三方」改为按 has_upstream 判定以保证三处一致；清掉 head_actions 空壳、ProfileState.last_commit_ts 双源、第三方 chips | ✅ 阶段 7 出口达成 |
| 2026-10-02 | 8 | 我的仓库内容：同步卡（官方 `ProgressCircle` 同步环 + 拉取/推送/检测位置固定 + 落后/待推送高亮 + 提交并推送行）；提交并推送对话框（变更摘要 + 预填上次提交标题 + ☐提交后拉取 → `commit_push_with`）；命令卡每行溢出菜单 + 命令编辑对话框；磁盘摘要卡收成一行 + 「空间」页签（总占用/构成条/Top 10/逐项清理/逐项加 .gitignore）；「同步设置」页签（7 档定时 + 自动拉取）；检视抽屉「变更」页签。状态扫描从 `changed_files` 换成 `changed_entries`（同一条 git 命令，白赚文件名列表）；core 加 `AutoSync` + `normalize_auto_sync` 与 5 个单测，78 → 83；补 D14（同步环改用官方件）、D15（**更正阶段 7 "官方 Input 装不进静态值"的错判**）、D16（图标资源 `Assets`→`AllAssets`，此前 git/品牌图标运行时是空白）、D17（变更清单共用一份）、D18（命令行拆成主操作区 + 溢出菜单）。删掉 `sync_actions` / `sync_panel` / 大 `space_card` / `git_card` / `clean_all` | ✅ 阶段 8 代码与数据通路达成（**未实机验收**） |
| 2026-10-03 | 9 | 第三方仓库内容：「说明」页签（`TextView::markdown` 整篇 + 可操作空态 + 「在文件管理器中查看」）；「历史」页签（官方 `List` 列表 → 官方 `Sheet` 看文件清单与逐文件 patch，含合并提交走 `diff <hash>^ <hash>`）；第三方同步卡下半区换官方 `DescriptionList` 只读档案行（当前/跟踪分支、是否落后、上次同步）；概览去掉 README 预览改成「活跃度 + 语言构成 + 看完整说明」；新增 core `history.rs`（commit_log/detail/patch + 3 个纯函数 + `safe_hash` 注入防护，10 个单测）。**真删掉阶段 8 记录里声称已删的 `sync_panel`——它带着一把「推送」键，是路线表硬边界上的一处越界项**；删掉 `ReadmePreview`/`read_readme`/`parse_readme` 与 3 个单测（README 改整篇渲染后没有调用方），换成 `read_readme_text`/`readme_file_name` 的 2 个单测；`RepoStatus` 补 `upstream`（复用已有 `rev-parse @{u}`，零新增子进程）；修掉启动首帧漏灌地址条/同步设置草稿，以及 `open_commit` 自设 `loading` 把自己的守卫变成死结、`open_detail_tab` 命中缓存时不 notify 两个时序坑。core 单测 93 → **92**（删 3 加 2）。补 D19–D25 | ✅ 阶段 9 代码与数据通路达成（**未实机验收**） |
| 2026-10-03 | 11 | 收口：⌘K 命令面板（新增 `gui/src/palette.rs`，官方 `Command`/`CommandState`/`CommandGroup`，五个分组：切到仓库 / 当前仓库 / 批量同步 / 切换视图 / 低频动作，`Ctrl K` 全局）；批量同步三条固定命令（`BatchKind::Pull/Push/Check`，按已扫状态筛，**推送滤掉第三方**是设计 3.5 的硬边界，串行循环、遇脏跳过并写进那个仓库自己的日志、汇总进新增的 `Shell::app_log` + 通知 + 状态条短句）；设置对话框（新增 `dialogs/settings.rs`，导航轨齿轮接上，身份 / GitHub·Gitee 令牌（官方 `mask_toggle`）/ SSH 私钥 / 代理 + `proxy::probe` 后台探测 / 系统通知开关即时生效；主题判为"不需要"）；快捷键全套（`1·2·3` 按屏幕位置、`Ctrl T`、`↑↓` 两屏候选集不同、`Esc` 收侧栏筛选、`Ctrl K`），同步卡三键 / 仓库头终端键 / 检视抽屉两态改用官方 `tooltip_with_action` 自动画 Kbd 角标；Q7 三处取色统一（分叉转红）；检视抽屉「空间」页签从"还没接"改成摘要 + 入口，删掉 `INSP_TODO`；**修掉两处断头**：侧栏右键菜单的「提交并推送…」自阶段 4 起一直是 `disabled(true)` 空壳（设计 3.6 误记成"已接"），已接上并让对话框开在右键那一行上。core 单测仍是 **98**（本阶段没动 core）。补 D29–D34。逐条过完设计第 11 节 22 条清单，**两项未过已如实登记**（间距发丝档、右键菜单不画 Kbd 角标）。**另发现一条功能回退：GPUI 版没有后台自动同步调度器**，已开 Q8 并在「同步设置」页加了一句实话 | ✅ 阶段 11 代码与数据通路达成（**未实机验收**；exe 61,859,328 字节） |
| 2026-10-03 | 12 提案 | **只写方案，一行代码没动。** 把 Q8 写成可勾选的阶段 12 提案（清单 + 写死的出口条件 + 5 个必须搬过来的旧单测）。核地基时发现比想的厚：配置字段 / `parse_hhmm` / `normalize_auto_sync` 都在，**连与用户操作的互斥 `sync::try_begin_op()` 都已经在 core 里、只是没有调用方**。要新写的只有到期判定与那个循环。**再往下查时挖出 Q11**：旧 `setup()` 里还起了托盘与"关窗不退出"，新版两件都没有，且根因是 `QuitMode` 用了默认档。补 Q11 | ⛔ 等拍板 |
| 2026-10-03 | 11 + 12 | 用户「按建议推行」→ **Q11 与 Q8 一次做完**。**Q11**：`gui/Cargo.toml` 加 `tray-icon 0.26` + `muda 0.21`（本地 registry 已缓存，进程内 8.57s 编过）；`main.rs` 加 `with_quit_mode(QuitMode::Explicit)`；新增 `gui/src/tray.rs`——手绘 32×32 RGBA 托盘图标（品牌主色圆 + 白翅膀）、三项菜单（显示主窗口 / 一键扫描全部 / 退出）、左键唤起、窗口已被关掉时 `AsyncApp::open_window` 重开，窗口选项与首次开窗共用一份。**托盘事件没走官方 `set_event_handler`**（它要 `Send + Sync`，而主线程执行器是 `!Send`），改主线程 100ms 轮询 `receiver().try_recv()`。**Q8**：core 加 `chrono` + 新增 `scheduler.rs`（`AutoSyncKind` / `interval_due` / `last_due_moment` / `daily_is_due` / `due_paths` / `run_one` / `record_result` / `toast_for`，**把旧版落盘那一步改成纯函数**）；GUI 新增 `gui/src/scheduler.rs` 进程级定时器（`App::spawn` 拿 `&mut AsyncApp`，**不绑窗口**）。删掉 `sync_tab.rs::no_scheduler_note()`，`last_result` 改走 core 的 `parse_summary`/`label()`，补上一直缺的「上次检查」时间。core 单测 98 → **121** | ✅ 零错零告警（exe 62,709,248 字节；**自查抓到并修掉一个继承 bug**：「每天定时」因 uto_sync_mins == 0 被当成已关整档跳过（旧 Tauri 版同一个判断，那一档从来没真的跑过）——补 **D37**） |
| 2026-10-03 | 12 补 | **托盘图标重画**（D39）。Q11 交付时 `app_icon_rgba` 是程序化生成的、**一眼都没看过**；复刻成 PNG 之后才看清渲出来是个对话气泡。三条独立病因：① **坐标原点用错**（几何按左上角写而像素坐标以圆心为 0），谷底 `y=19.5` 掉出 32px 画布被裁，两翼在圆底被切断，读成"圆底被啃两口"——调参数调不对，几何压根不在画布里；② **品牌里没有鸟**，「青鸟」的 logo 实际是带 git 分支的立体箱子，按名字脑补图腾连调三轮飞翼，方向从一开始就错；③ **无底字形在深色任务栏上直接消失**（`#202020` 上一像素不剩），改圆底自带底色，顺带免掉读 `AppsUseLightTheme` 切深浅两套图标。落法：`accent.background` 圆底 + 白色 git 分支（竖直主干两节点 + 一条从顶节点向右下弯的弧；**弧必须弯**，直画会读成「Γ」角标）。几何常数一律相对 `n` 写、不带 `CENTER +`。**新增 `gui` 侧头两个 `#[cfg(test)]`**：字形包围盒在圆内且对称、四角透明圆心不透明。核验时把 Rust 真实产物（4096 字节）导出来看的，**不是看 Python 双胞胎**。exe 62,709,248 → **62,714,368** 字节 | ✅ 零错零告警；core **121** 全绿 + gui **2** 全绿 |
| 2026-10-03 | 12 补 2 | **用户双击 exe 报「闪退」**——路线表 0–12 全部写完、121 单测全绿、`cargo check` 零错零告警、exe 构建成功之后，**第一次实机启动就直接死**（`this method can only be called during paint`）。根因：`Shell::new` 里 11 处 `cx.on_action`，而 `Window::on_action` 第一行就是 `debug_assert_paint()`，只允许在 `DrawPhase::Paint` 期间调；**搬进 `Render::render` 也没用**（render 跑的是 `Prepaint`，`Paint` 只在 `draw_roots` 的 `root_element.paint(..)` 那段）——这一条是白改一轮才换来的。落法：改用**元素级** `on_action` 挂在根 `v_flex` 上（与 gpui-component 同一套写法，由 `Div::paint_keyboard_listeners` 在 paint 里统一注册，天然每帧重挂）。**顺带查出第二个更阴的坑**：没焦点时 Action 派发落到 `DispatchNodeId(0)`（gpui-kit 那层 `Root`），我们在更深一层的监听器一次都收不到——而「刚启动还没点过任何东西」正是最该按 `Ctrl K` 的时刻；修法是 `Shell` 加自己的 `FocusHandle` + 根元素 `.track_focus` + 启动即聚焦。补 **D40**。另注：`debug_assert!` 在 release 下是空操作，同一个错在 release 里会变成「静默地少活一帧」，所以**发布前至少跑一次 debug 构建** | ✅ 修后：连开 15s 与长跑 80s（覆盖托盘 100ms 轮询 + 调度器 60s 首轮）均不崩、窗口句柄非零、`Responding=True`、stderr 空；core **121** + gui **2** 全绿 |
| 2026-10-03 | 12 补 3 | **用户实机报「三个主板面内容全部炸了，乱的」**——闪退修完之后的第一轮实机，七条缺陷，**没有一条是编译器和单测能判的**。① **D41 · 整屏塌陷**：`finish_frame` 的 `row` 用 `h_flex()`，而基座明写 `h_flex` 装 `items_center()`、`v_flex` 不装（**交叉轴不对称**）——行里的列容器拿到的是**内容高度**并被垂直居中，主区缩成 343px 飘在行中间，三个屏同时炸。落法 `row` 加 `.items_stretch()`。**「总览汇总表整块不渲染」是它的后果不是独立缺陷**（容器被压扁后表头被推出可视区），根因一修 17 行全出来——先修根因，别给症状写补丁。底排两张卡同源（`items_start()`，不是 `items_stretch()`：磁盘卡就该是磁盘卡那么高）。② **D42 · 命令卡卡内 260px 空洞**：早先注释里那句「设计稿：命令卡与底排分摊主区剩余高度（`fill_container`）」**设计稿里根本没有**，是平移时编的理由。两条路都不对：给一块 `flex_1` 会在卡内留空，都不给则**窗口下限 1080×640 下底排被裁掉**。落法：只有概览页签套滚动容器（`detail-ov-scroll`），两个 body 改内容高；其余四页签各自已能填满自滚，**别再套一层**。③ **D43 · `short_day` 取错段**：假设输入 `MM-DD`，而 `local_date_key` 给的是 `YYYY-MM-DD`，于是 14 根柱顶全印 `2026/09`。**改法不是调偏移**，段数不同的两套格式用「按位置取」兼容本身就不可靠，改成按语义取并容错；顺带发现柱顶标签本该是**每根柱自己的值**（`BarChart::label`），写日期等于抄轴信息 14 遍 → 改 `count`。④ **D44 · `Column::text_right()` 在 `DataTable` 上是空操作**：基座里 `align` 只有 `table::Table` 那条路会读，`TableState` 全程没读过这个字段——**编译得过、运行不 panic、就是不生效**。而 `p_0()` 是生效的，所以真实观感是「状态 pill 尾巴紧贴下一列数字」。两处都去掉（真要右对齐得自己摆 `justify_end`，那样表头还是左对齐，两边反而对不齐）。⑤ **D45 · 点表格行不跳转**：`cx.update_window` 靠 `cx.windows.get_mut(id).and_then(Option::take)` 取窗口（`app.rs:2888`），而订阅**恰恰跑在该窗口自己的事件处理里**，窗口此刻已被 take 走 → `update(None)` 把整个闭包丢掉 → `Err` 被 `.ok()` 吞掉。**三处都不出声**。落法回到一次性标记 `pending_reveal` + `render` 兑现。⑥ **D46 · ⌘K 分组长列表排第一**：17 个仓库吃光可视高度，其余四组要滚 17 行才够得着。改序为 低频动作 → 当前仓库 → 批量同步 → 切换视图 → 切到仓库；⚠️ **`G_*` 常量与 push 顺序是两件事**，改常量不改顺序只会让 `debug_assert_eq!` 撒一个仍通过的谎。⑦ **D47 · 冷启动首屏柱状图永远全 0**：设计 §3.1「进总览才扫」与「启动首屏默认总览」两条合起来的结果。补一次性标记 `ov_need_scan`（§3.1 口径同步改）。另修两处小的：`log.rs` 空态提示改居中且不继承等宽字体（Consolas 无汉字，顶格一行吊在浅灰底上像没画完）；导航轨点击坐标按实测重标（脚本此前点的 y=78 是**已选中的总览键**，看着像没反应；且窗口未获焦时第一击只用于激活窗口）。补 **D41–D48**。core 单测仍 **121** | ✅ `cargo check` 零错零告警 + core 121 + gui 2 全绿；三屏 + 五个页签 + ⌘K + 表格点行 + 滚动 + 窗口下限 1080×640 逐项截图核过（**第三方屏用隔离 APPDATA 临时配置验，没动用户配置**） |
| 2026-10-03 | 11 补 | 收尾后自查抓到 `app_log` **只写不读**（阶段 11 自己引入的）：批量跑完的逐仓库解释在通知消失后就永久没了。补两处——`run_batch` 把**没成的每一个**连仓库名写进 `app_log`；⌘K「低频动作」组加「上次批量结果」（无记录时置灰），点开新增的 `dialogs/batch_log.rs`。**不把状态条那一格改成可点**（6.1 已写明它只是进度显示）。`log.rs::output_stream` 提成 `pub(crate)` 复用，**id 由调用方传**（gpui 滚动状态按 `ElementId` 全局存，共用 id 会共用滚动句柄）；`LogState` 补 `Clone` 供该对话框的 `content` 闭包取快照。补 D35。**再把这一类系统性扫了一遍**（142 个状态字段 × 渲染路径读数，51 个疑似逐一核实 → **0 个真缺陷**），判据与三类合法形态见 D35 段。**再查同源的「键位提示会不会是假的」**：11 个 Action 的声明 / 绑键 / 处理三方 11:11:11 一一对应，`tooltip_with_action` 引用的 4 个全在册；另修掉 2 处**手写键位文案**（日志区停止按钮、空态那句 ⌘K 提示），改走 keymap 查询 —— 补 **D36**。core 单测仍 **98** | ✅ 零错零告警（exe 61,888,512 字节） |
| 2026-10-03 | 12 补 4 | **用户实机三轮的九条诉求**（D49–D55）。① 标题栏图标换回老 `icon.ico`：拷 128×128 位图进 `gui/assets/`，新增 `app_assets.rs` 资产源（`AllAssets` + 额外认一张 PNG，其余全部转交）——**查清 `component::Icon` 只吃 svg**，位图得走 `img()`，而 PNG 解码由 gpui 自带 `ImageAssetLoader` 做，**不必新增图像依赖**；补一个单测钉住 PNG 签名与 IHDR 宽高。② 「青鸟 Git」左对齐：官方 `TitleBar` 把 `children` 塞进 `justify_between()` 的 `h_flex`（`title_bar.rs:371-398`），**两个子项必被推到两头**——并成一个即可。③ 图标与轨中心线对齐：左边距做成派生量 `TITLE_BAR_PAD_L = NAV_W/2 − APP_ICON/2`，改了轨宽自动跟上（实测修前 29.5 vs 35.5，修后两者同为 30）。④ 导航图标放大：只能用 `Size::Large`（icon-only 外框 Medium=Large=32 不变、图标 16→24），顺带**删掉骗人调用** `.icon(icon_md(..))`——`Button::render` 无条件 `icon.with_size(icon_size)`，写了被丢弃。 ⑤ 标题栏 48→40、轨 56→44。（轨宽与内边距**成对**，debug_assert_eq! 钉住 NAV_W − 2×NAV_PAD == 32）⑥ 总览柱形图 → 官方 `AreaChart`（`Curve` 的 `#[default]` 就是 `Natural`，不必显式写；填充压到 12%，实心色会吃掉网格线）。详情页那个迷你柱状图**不动**。⑦ 七列跟随窗口缩放 = **等比 + 1.6 倍上限 + 余量给文本列**（照字面纯等比会让「归属」列变成 385px 空白）；**宽度存委托不放 `Shell`**——`TableState::refresh` 会回调 `column()`，与 D45 同源。core **121**、gui **2 → 10** | ✅ 零错零告警；三屏 + 窗口下限 1080×640 逐项截图核过 |
| 2026-10-03 | 12 补 5 | **自查抓出的一个自激 + 用户追加的两条间距**（D56 / D58）。**D56**：D55 第一版把列宽的输入取成「表格自己的可用宽度」，而那个宽度是被列宽决定的 → **正反馈环**，鼠标一动界面就持续右拉直到出窗。**纯函数测试全绿也抓不到**（函数没错，是输入会漂）；**验收脚本要能制造扰动**——新增 `stir.ps1` 抓一帧→划 60 下鼠标→再抓一帧→求 `ImageChops.difference` 的 bbox，改完 diff 为 **None**。落法是**开环**（输入只来自窗口宽）。**D58**：轨顶内边距 6→20（logo 与三颗图标太近）→ `gap` 由 `GAP_TIGHT`(4) 改 `GAP_LG`(12)（三颗图标快粘到一起）→ 轨顶再由 20 收到 **14**，改成一条**可验证的对齐基线**：导航首个图标中心 ≡ 页头「总览」字中心（实测 66..82 vs 73..87，差 6px；改后同为 **74.0**）。**审美值退回成推导式**。gui **10 → 12** | ✅ 零错零告警 |
| 2026-10-03 | 12 补 6 | **D57：D49 的修法只覆盖了一半分支**。图标+标题并成了一个 `h_flex` 子项，可侧栏开关 `SidebarToggleButton` 仍挂在 TitleBar 上——**总览页没有开关（一个子项，看着正常），切到仓库页就成了两个子项**，`justify_between` 把开关推到左、整块图标+标题推到右，「青鸟 Git」又贴回窗口控件旁边。落法：开关也塞进**同一个** `h_flex`，两屏都恰好只有一个子元素。**教训登记在案：一个只在某些分支里犯的错，只跑默认那一屏的自查抓不到** | ✅ 仓库页抓帧核过 |
| 2026-10-03 | 12 补 7 | **实机第四轮两条 + 顺带挖出的一个配置漏项**（D59 / D60）。**D59 底部内边距压根不存在**：齿轮是 `v_flex` 最后一个子项、被 `flex_1` spacer 顶到轨底，而轨底紧挨 28px 状态条；实测字形底距那条发丝线**只有 5px**。落法 `NAV_PAD_BOTTOM = GAP_LG`(12)，改后 **17px**。**D60 选中面**：`theme.json` **没配 `button.active.background`** → 落基座兜底 `input.mix_oklab(transparent, 0.7)`，实测渲成 **#EFEEEC** 偏暖实底，一屏灰白里读成"贴了块色板"。落法 `selected_mask = foreground.opacity(0.08)`——**0.08 由规范板「选中面」#EDEBE7 反推**（`(255−237)/(255−26) ≈ 0.079`），实测 #EDEDED，与侧栏仓库行选中面差 0/2/6，**1:1 下不可分**，这就是"适应全局同类"的可验证形式；补两个单测（遮罩落在规范板 8/255 内、三处轨内边距不许并成一个数）。**能盖掉官方底色靠的是**官方在每个状态层最后重放调用方样式（`button.rs:733`/`:795`），`when(active, ..)` 不能省。**顺带挖出一条没改的**：ghost 的**悬停**面是实心 `accent` 蓝 #5B7A99，比选中面重得多；改它要换 `ButtonVariant::Custom`（连带改焦点环与描边），而调用方 `.hover()` 覆盖在 **debug 下直接 panic**（`div.rs:843`，实测退出码 101）——**同一份代码两种构建两种行为**，已开 **Q12** 等拍板。gui **12** | ✅ 零错零告警；启动冒烟 alive / hwnd 非零 / `Responding=True` / stderr 空；两屏选中面均实测 (237,237,237) |
| 2026-10-03 | 10 | 克隆：core 新增 `clone.rs`（`CloneOutcome` + `clone_repo`，走 `net::run_net_op` 且 `allow_race=false`，30 分钟专用超时，失败清半成品目录 + 五类错误中文化，`classify` 的判定顺序用 4 个单测钉死）；`repos::repo_name_from_url` **不照搬 Tauri 版**（D26：主页地址不再被当成仓库名，额外拒绝 `.` / `..`），2 个单测；成功入列表复用 `repos::add_repo` + `touch_repo`（D27）。GUI 新增克隆对话框（地址 + 目标父目录 + 官方 `Progress` 不定态条 + 实时输出 + 取消），侧栏「克隆仓库」从置灰接通。后台进度经 `Arc<Mutex<..>>` 回主线程（D28 的定位延后一帧同样为了不重入 `Shell::update`）。core 单测 92 → **98**。补 D26–D28 | ✅ 阶段 10 代码与数据通路达成（**未实机验收，且需要联网才能验**） |
| 2026-10-05 | 12 补 8 + 概览改版 | **概览页从「页签驱动的卡片堆」改成固定仪表盘**（分叉条 → **同步** → **日志** → 语言技术栈，日志 `flex_1` 吃剩余高度）。用户口径：概览只展高频功能，**日志占最大最重要**，同步块与常用命令其次，定时任务收进单独面板，同步上的圆环「占地方还难看」删掉，语言技术栈参考 GitHub 的色带 + 百分比。① **「常用命令」段整段删掉**（`quick_cmds_section` / `cmd_chip` / `Shell::cmd_hover` / `CMD_CHIP_H` 一并清干净）：「开发运行 / 编译构建」已经长在日志表头上，再摆一排是同一对入口摆两遍。② **编辑入口没有跟着消失**——⌘K「当前仓库」组加「编辑常用命令…」（`PaletteAction::EditCmds`），**能跑不能改同样是残废的**。③ 右栏默认折叠：`store.rs` 的 serde 从 `default = "default_true"` 改成裸 `#[serde(default)]`（用户自己的 config 已经是 false，不用改文件）。④ 窗口下限 640 → 720。⑤ core 新增 `lang_color`（28 种语言对齐 GitHub linguist `popular.yml`）+ 2 个单测；`scan_profile` 改为 README 与语言构成两类仓库都跑。⑥ **D61：修掉 debug 构建的 `Duplicate a11y node id` panic**——`inspector_reopen` 被 `repo_head.rs` 与 `sync_card` 各调一次，右栏一收起就同帧出现两颗 `Button::new("inspector-open")`。新增 `scripts/uia_probe.py`（用 .NET UIAutomation 当无障碍客户端把 a11y 强制激活，那棵树平时压根不建）与 `scripts/a11y_stress.py`（激活后压 18 步走查，本轮 0 撞）。 ⑧ **第三方概览并到同一套版面**（用户：「第三方仓库页面概览用相同的布局，只是不用提交推送的按钮」）：`profile_body` 改成与 `workbench_body` **逐行相同的四段**（分叉条除外——它改的是本地历史，第三方不给），同步段内部按 `third` 分支的裁剪（不画推送键 / 不画「提交并推送…」）**留在 `sync_card` 里，不上提到版面层**——版面因此可以整段复用，不必写第二套（两套迟早漂）。原「仓库档案」卡拆掉：语言构成并进 `lang_section`（同一条 `lang_bar`，且档案卡那条只有色带没图例，合并后更全）、「看完整说明」删掉（「说明」页签就在下面那行，而「我的」从来没有第三处 README 入口，D22）、近 30 天活跃度挪进仓库头做成概要位徽标（`repo_head::activity_chip`，**只有第三方有**——它是评估别人项目时最该先看的一句，而共用版面里没有它的位置）。 ⑨ 顺带修一个**先前就有的溢出**：第三方那行四列 `DescriptionList` 原来和三键挤在同一行，右端的「定时同步」与「看清单」被**顶出画面**（抓帧实锤），改成独占第三行。 |
| 2026-10-05 | 12 补 9 + 说明页 | **「说明」页签改造**（用户逐页签推进的第三页）。① 顶部那排（`📄 README.md` + 「在文件管理器中查看」）按日志段的做法**收进卡内表头**——它原来浮在卡片**外面**，一屏两个头；**仓库头一行没动**（用户澄清：仓库名 / 归属 / 分支 / 磁盘摘要 / 地址两行保持原样，五个页签都看得见）。② **中英切换按「多份文件」形态实现**：core 新增 `readme_parts`（根目录文件名拆成「排序档 + 作者写明的语言」，大小写不敏感，**不认得的后缀一律不算说明文档**——`README_api.md` / `contributing.md` 都不是）、`text_is_zh`（汉字 : 拉丁 ≥ 1:3 判中文，**阈值有意放宽**——判错只是两颗键同语言，判太严会让真双语仓库没入口；**不把假名算进来**，日文落英文档）、`readme_alt`（判据是**当前显示的那份**而不是主文档，否则切过去之后再点一次切不回来）、`read_readme_named`（仍要过 `readme_parts` 才放行，钉死「别把任意路径拼进来」）；**`README.md` 排在带后缀的前面**（多数中文项目就是「中文正文 + 无后缀」，强行优先 `_zh` 会反过来）。**只有一份就不出现两颗键**——摆一颗切不过去的键比不摆更糟。③ **README 链接四分类**（`classify_link`）：`#锚点` → 文档内同名标题（走基座 `reveal_range`，**不自己解析 markdown**）、根目录的 `README_*.md` → **切到那一份**（双语 README 的中英链接多半就是它）、其它相对路径 → 系统默认程序（`os::open_path` 新增）、`http(s)/mailto` → 系统。`on_link_click` 是裸闭包（要 5 个参数，**不能写 `cx.listener`**），自己抓 `Entity<Shell>` 进去；用 `TextView::new(&state)` 而非 `markdown(..)` 便捷构造（后者把 state 藏在元素里，够不着 `reveal_range`）。core 128 → **132**、gui 12 → **15**。**补 D62：三条路径的第一版全是坏的**——`cargo check` 零错、单测全绿、抓帧「看着没问题」，**只有实机点得出来**：点「中文」永久停在骨架屏（代数守卫取号与发号写反，闭包静默早退）、目录锚点没反应（取第一个匹配＝落在目录项上，改取最后一个）、子目录 README 被误判成「切文档」（点一下悄悄给了另一份文档）。另补两个验证工具 `scripts/steps.py`（跑一串点击逐步抓帧）与 `scripts/frame.py`（放大读小字 / 几帧竖排 / 求差分），删掉一次性的 `shot_tab.py` 与合并前的三个帧工具。 | ✅ 零错零警；往返验证拿**逐像素相同**当判据：点中文键 → 点正文里的 `English` 链接回英文那份，与初始帧 diff = **0 像素**；目录锚点的 range 由 1543（目录项）变成 2918（真标题）且画面滚过去；单份文档的仓库表头不出现中英键；三轮抓帧 stderr 全 0 字节 |
| 2026-10-05 | 12 补 10 + 阶段 13 | **日志 / 空间 / 历史 / 变更四页统一排版 + 空间页变成只读目录树**。① **13-1 起因是查出来的分叉**：空间页**自己手搓了一份外壳**（内边距 **16**），日志 / 说明用 `log::section`（**8**），历史 / 变更**根本没有卡**——五个页签摆一屏里内边距有两个值、有的有壳有的没壳。收成 `widgets::card(t)` + `widgets::CardHeader`（24px 行高 + `FS_BODY`/`MEDIUM` 标题 + `flex_1` 顶开 + 右侧 `Vec<AnyElement>`），**`log::section` 删掉不留转发**；右侧操作的分组关系留给调用方（`vdivider` 插中间）。历史页裸列表、变更页裸清单都换到白卡 + 卡内表头；「重新扫描」从整页最下面那个孤零零的按钮挪到「磁盘占用」表头右上（**它扫的就是这一段**）；段间距 `GAP_LG` → `GAP`；**日志空态不画里框**（有内容保留——那圈边在有内容时干的是"把等宽输出与白卡分开"）。② **13-2 空间页目录树**（用户拍板：可展开的目录树、点一下才扫下一层；缓存目录与 `.git` 都进树但默认折叠成一行）。core：`DirSize` 与 `DirEntry` **合成一个**（字段重叠必然漂）、`list_dir` 列一层不递归下钻（目录 size 仍递归）、`safe_rel` 钉死 `..`/绝对路径/盘符/反斜杠、`child_count` 数一层（0 = 空目录 → 不画箭头）、`human_bytes` 四档。GUI：**不用官方 `component::tree`**（`is_folder()` = `!children.is_empty()`，懒加载的文件夹扫之前 children 必空 → `toggle_expand` 第一行就把它挡掉，永远展不开，且没有"展开时去加载"的钩子；查证写在 `state.rs::TreeNode` 上），自己画行；根层**不重扫**，白捡 `scan_repo_space` 的 `dirs`；`.detach()` + 代数守卫（`tree_seq` 先自增再取值）；**过期结果也必须先清 `tree_loading` 再 return**；文件行不给 `on_click` 不给箭头；树封顶 420 独立滚动（用户的仓库动辄几十 GB，展开几层就上千行）。③ **13-3 一键清理**：`dialogs/clean.rs` 逐项列清按体积降序 + 橙色后果提醒 + **确认键用 `danger` 不用 `primary`**；core `delete_cache_dirs` **逐项独立**不连坐；逐项那颗键保留。core 132 → **141**、gui 15 → **19**。**改掉的三个真缺陷**：① **同一棵树里两套顺序**（根层白捡 `report.dirs` 是按体积降序的榜、子层走 `list_dir` 是名称序）→ 排序规则收进 core 的 `sort_entries`；② **同页两种体积格式**（树里 `781 KB`、图例满屏 `0 MB`）→ 清单与结论一律 `human_bytes`；③ **清理结论永远看不到**（只写 `space.note`，切到概览页就没了；`app_log` 又是批量汇总那个槽）→ 写进 `log_of(path)` 那个仓库自己的日志槽。附带：`delete_cache_dir` 里「目录被占用」友好提示**在 Windows 上永远不触发**（`ERROR_SHARING_VIOLATION` 32 被 Rust 归成 `Uncategorized`，不落进 `PermissionDenied` 臂）→ 按 `raw_os_error()` 显式认 5/32/145。 | ✅ 零错零警；验收在 `$TEMP` 现造的合成仓库上做（**用户 17 个仓库一个没动**，config 走 `$TEMP` 副本，改完核对真实 config 仍是 17 条、时间戳未变）；树四层展开点得动、**往返 diff = 0 像素**、递归体积逐项核对无误；行距用新工具 `scripts/rowpitch.py` **实测统一 28.0**（目测以为有三种，量完是滚动裁切造成的测量偏差）；一键清理取消后 **0 像素差**且目录都还在，确认后磁盘上只剩 `.git`/`src`/`README.md`；**占锁场景**（`FileShare.None` 持句柄）下 `node_modules`/`dist` 删掉而 `target` 留着，日志两行把成功与失败都讲清楚 |

| 2026-10-05 | 12 补 11 + 补测试覆盖 | **为删除旧实现（Q6）铺路**。用户拍板分两轮：先补测试覆盖 + 改 README，下一轮再删。① 查清依赖：`crates/` + `gui/` 里**没有一处引用 `src-tauri` 或 `src/`**，仓库也**没有 `.github`**（没有 CI 靠 Tauri 构建），`tauri.conf.json` 只被它自己用 → 删是安全的。② **README 全错**：第 30/36/42 行还写着 `pnpm tauri dev` / `pnpm tauri build` / `cd src-tauri && cargo test`。改写成 `cargo build --workspace` + `cargo test -p qingbird-{core,gui}`，并补上目录表与"界面怎么验"一节（点名四个脚本）。③ **补 6 组单测覆盖旧测试钉住、且现在仍然归我们管的规则**（gui 19 → **32**）：`bucket_of` 分档优先级（分叉优先于落后/待推/未提交）、`bucket_word` 每一档都有中文说法、`BatchKind::wants` 的 Q3 三档筛选口径（**第三方一律不进推送队列**）、`batch_summary`（**「跳过 0 个」这种废话不出现**，顺序固定在成功之后；`done < total` 报进度，`done == total` 算跑完）、`step_repo_index`（**到头停头不绕回**；没选中时 ↓ 从头 ↑ 从尾）、`batch_line`（没有可跑的仓库时把原因写进标签）。④ **两处顺手收口**：`state.rs::batch_summary` 与 `palette.rs::batch_line` 原来挂在 `Shell` 上得一整个 Shell 才调得了，**抽成自由函数**（这也是它们能单测的前提）；`palette_model` 末尾的 `debug_assert_eq!(acts.len(), groups.len())` **只查了组数对齐，没查组内条目数**——加一个条目忘了配动作，那个断言照样过、⌘K 静默执行错的动作，补了三处 `debug_assert_eq!(items.len(), acts.len())`。⑤ **三键顺序收成一个声明**：`state.rs::SYNC_SLOTS` / `SYNC_SLOT_LABELS`（拉取·推送·检测），同步卡改成读它（`cargo check` 那两条 "never used" 告警就是这么消掉的），`bind_keys` 与同步卡两处留指向它的注释。⑥ `docs/shots/` 只留 `current.png`，删掉 8 张记着已经不存在的旧 React 界面的截图。**没照抄的**：`commands.test.mjs` 的模糊匹配规则（**已经是基座 `CommandState` 的行为**）、`overview.test.mjs` 的 `buildTodo` / `ringGradient` / `countBuckets`（**随改版设计性删掉了**——待处理列表没了、圆环删了）、`hotkeys.test.mjs` 的键位判定（**官方 Action + `bind_keys` 接管了**）——给已经不存在的实现补测试等于测空气。⑦ **抓帧流程一直在写用户的真实 config，本轮查实并堵死**：核对时发现真实 `config.json` 的 mtime 停在 `2026/10/5 1:49:48`——正是上一轮抓帧的时段，且 `inspector_open` 已从 false 被写成 true（`Copy-Item` 保留 mtime，所以副本的时间戳反而成了证据）。根因不是"忘了设 `$env:APPDATA`"这一件事，而是**隔离靠人记得**：`steps.py` 用 `Popen([EXE])` 启动、继承调用方 shell 的环境变量，脚本自身**没有任何隔离入口**；`uia_probe.py` / `a11y_stress.py` 同样只设了 `RUST_BACKTRACE`。修法收进新模块 `scripts/qbguard.py`（**三个脚本共用，不各写一遍**）：`QB_APPDATA` 指向副本时覆盖 `APPDATA`，不设就打醒目警告，收尾再按 `mtime+size+sha256` 指纹核对真实 config 并在不一致时直接报出来。本轮两次抓帧指纹前后一致（`bcbc1ef3…`），确认隔离生效 | ✅ `cargo check --workspace` 零错零警；gui 32 / core 141 全绿；同步卡在隔离环境下抓帧核对：「我的」拉取·推送·检测三键齐全、顺序正确，第三方推送位仍是空格子占位 |
| 2026-10-05 | 12 补 12 + 删旧实现 | **Q6 第二轮：旧 Tauri 2 + React 实现整体退役删除**（用户批准，同日执行）。① **删**：`src/`（64 文件）、`src-tauri/`（42 文件）、`public/`（3）、`index.html`、`vite.config.ts`、`tsconfig.json` + `tsconfig.node.json`、`package.json` + `pnpm-lock.yaml`（`test:web` 一并退役）、`scripts/gen-icons.mjs`（它调 `@tauri-apps/cli`、产出目标是 `src-tauri/icons/` 与 `src/assets/`，两个都没了才失效），外加三个本地产物 `node_modules/`（**88.8 MB**）、`dist/`、`debug.log`（旧 WebView2 的 crashpad 日志，全是「拒绝访问 0x5」）。跟踪文件 **206 → 90**。② **清单修正一处**：`assets/` 原写着要删，实查里面只有 `icon-source.png`——1024px 的**图标源图**，而 `gui/assets/app-icon.png` 是它的 128px 展开 → **保留**。删了将来改图标就没有源了（gen-icons.mjs 退役后，改图标就是直接替换 `gui/assets/` 下那一张）。③ **删前逐条查引用**，结论是**零代码依赖**：`crates/` + `gui/` 里对旧实现的提及**全在注释里**（`history.rs`「合并提交走 `git diff <hash>^ <hash>`」、`repos.rs`「日期键按本地时区」、`net.rs`「`Channel` 改回调闭包」这类溯源，**留着有价值**，指向的目录没了也不影响读者理解）；`app_assets.rs` 的 `include_bytes!` 指向 `gui/assets/` 而非 `src-tauri/`（当初拷进来就是为了不把构建绑在准备删的目录上）；两个 `Cargo.toml` 都没有 node 依赖、`gui/` 下无 `build.rs`；`Cargo.lock` 里的 `tauri-winrt-notification` 是 `notify-rust` 的传递依赖，与旧实现无关。④ **修掉三处会变错的文档**：`Cargo.toml` 末尾那条「src-tauri 保持独立工作区」、`.gitignore` 的 `src-tauri/target` 与 `src-tauri/gen/schemas`、`.vscode/extensions.json` 推荐的 `tauri-apps.tauri-vscode` 插件。⑤ **AGENTS.md 大改**：「项目说明」从「Tauri 2 + React · 迁移中」改成已换成 GPUI；「目录规则」删掉「src/ 与 src-tauri/src/ 是只读迁移源」；「常用命令」**整节重写**（原来教人 `pnpm install` / `pnpm tauri dev` / `cd src-tauri && cargo test` / `pnpm test:web`——那个目录已经没了）；并**删掉整节「架构要点（旧实现，只读参考）」99 行**。删之前逐条核过它记的结论在新代码里都有**更详细、带代码位置**的版本（`history.rs` 的 `diff_args` 纯函数 + 单测、`repos.rs` 直接让 git `--date=format-local` 算时区、`shell.rs:980` 与 `panels.rs:222` 的「1·2·3 按屏幕位置对位」、`ops.rs:505` 的「遇脏工作区跳过不自动 stash」、`state.rs:789` 的「跳过 0 个不出现」+ 单测），而「三列布局 / minWidth 1080 预算」那类界面规格早已彻底失效——留着一份指向空目录的「只读参考」只会让人白跑一趟。⑥ `docs/UI-DESIGN.md` 里那段阶段 11 的复查台账**没改正文**，只在开头加一句「`src-tauri` 指当时的迁移源，该目录已退役」——结论历史上都是真的，逐条改路径反而容易改错 | ✅ `cargo check --workspace` **零错零警**；core **141** / gui **32** 全绿；`cargo build --workspace` 成功出 exe（63,598,080 字节），**删掉 88.8 MB 依赖后构建不受影响** |

