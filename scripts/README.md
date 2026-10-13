# scripts/ 脚本说明

这份文件管 `scripts/` 下每一个脚本**证明什么、怎么跑、怎么算红**——纪律（变异验证、行尾事故、证据三态）归 `AGENTS.md`，这里只写"这个脚本是什么"。

怎么用：**先找你要证明的那件事**（表里第二列），**再看它的判据列是不是真闸**。判据列只有两种取值：

- **真闸** —— 判据不过就非零；括号里点名的就是它 `sys.exit` / `raise SystemExit` 之前打印的那一句。
- **只报告，不拦任何东西** —— 判据本身不接退出码；凡非零出口**只覆盖"跑不起来 / 用法错"**（找不到窗口、参数不足、assert 前提）的，一律归到这里，并在括号里点明那个非零出口。

命令行一律在仓库根执行。共 36 个文件 = 35 个 `.py` + 1 个 `.ps1`。

---

## 1. 设计令牌闸

| 脚本 | 证明什么 | 怎么跑 | 判据（红在哪） |
| --- | --- | --- | --- |
| `check_design_tokens.py` | 扫 `gui/src/**.rs` 钉住七条规则：R1 字面色值 / R2 裸间距与圆角与字号 / R3 裸时长 / R4 `.bg(色)` / R5 文字冒充图标 / R6 同文件重复 `ElementId` / R7 直写 `colors.border` | `python scripts/check_design_tokens.py [-v]` | **真闸**：任一规则命中 ⇒ 「合计 N 处违规」`return 1`；干净 ⇒ 「设计令牌闸：gui/src 干净」`return 0` |

## 2. 构建产物与安装包校验

| 脚本 | 证明什么 | 怎么跑 | 判据（红在哪） |
| --- | --- | --- | --- |
| `build_installer.ps1` | 一条命令出安装包：从根 `Cargo.toml` 的 `[workspace.package].version` 读版本 → 编 release → 找 `ISCC.exe` → 补 BOM → `ISCC /DMyAppVersion=` | `pwsh scripts/build_installer.ps1 [-SkipBuild]` | **真闸**：`$ErrorActionPreference='Stop'`，找不到版本行 / 找不到 ISCC / 找不到 target 里的 exe 与 `app.ico` / cargo 编译失败 / ISCC 失败，任一 `throw` 即非零 |
| `pe_subsystem.py` | 读 PE Optional Header 偏移 68 那个 WORD，判 GUI(2) / console(3)——release 双击不许带黑终端 | `python scripts/pe_subsystem.py <exe> [更多 exe...]` | **真闸**：任一份 `Subsystem != 2` 或读不出 ⇒ 「`[BAD ] Subsystem=3 (WINDOWS_CUI) … <- console 子系统，双击会弹黑终端窗口`」`return 1`；缺参数 `return 2` |
| `check_d78_strings.py` | 核 release exe 里含本轮 D78 三句新文案、且旧文案「松开即可添加仓库」已消失（判据落在产物的 `.rdata` 上，不是构建退出码） | `python scripts/check_d78_strings.py <exe>` | **真闸**：`bad > 0` ⇒ 「`{bad} 条不符：这份 exe 不是本轮编出来的那一份（或改动没生效）`」`return 1`；缺参数 `return 2` |
| `extract_exe_icon.py` | 从 exe 逐档解 `RT_ICON` / `RT_GROUP_ICON` 渲成 PNG，另跑 `ExtractIconExW` 看系统认不认 | `python scripts/extract_exe_icon.py <exe> <out.png> [边长...]`（默认 16/24/32/48/64/128/256，出 `<out>_<n>.png` 与 `<out>_strip.png`） | **真闸**：缺参数 / 「`exe 没有资源节（.rsrc）—— 资源一个字都没进去`」/ 「`exe 里没有 RT_GROUP_ICON —— 图标没嵌进去（看到的是 Windows 通用那个）`」各 `raise SystemExit`；⚠️ `ExtractIconExW` 报 0 只打印「← 系统认不出，资源没生效」，**不红** |
| `verify_icon_sources.py` | 图标三处同源核对：① `gui/assets` 源图 ② `$OUT_DIR/app-tray.rgba`（build.rs Lanczos3 缩到 `TRAY_EDGE`）③ exe 资源里同档；两处 `include_bytes!` 现读必须同一文件 | `python scripts/verify_icon_sources.py [输出png]`（默认 `%TEMP%\qb_icon_three.png`） | **真闸**：`bad` 非空 ⇒ 「判读：⛔ …」`return 1`，红在这几条——「标题栏与托盘/桌面读的不是同一张源图」「托盘 RGBA 字节数与 `TRAY_EDGE²×4` 对不上（多半没重编）」「源图不是 build.rs 断言的 `SRC_EDGE²`」「两处尺寸不是同一个」「②③ 逐像素不同」；源图 alpha 只报数（判据在 gui 那两条单测） |
| `explorer_icon_check.py` | 让 Explorer 亲自画 exe 图标再抓图（覆盖系统图标缓存这一层），只在临时目录建 `.lnk`，不碰桌面 | `python scripts/explorer_icon_check.py <exe> <out.png>` | **只报告，不拦任何东西（唯一非零出口是跑不起来）**：缺参数 / 没等到 Explorer 窗口各 `raise SystemExit`；图标画成什么样它只出图 |
| `add_bom.py` | 给 `.iss` / `.ps1` 补 UTF-8 BOM（Inno 无 BOM 会按系统 ANSI 读，中文全乱码却照样报 "Successful compile"） | `python scripts/add_bom.py <文件...>` | **只报告，不拦任何东西**：无 `return` / `sys.exit` ⇒ 退出码恒 0；已有 BOM 打「已有 BOM，跳过」，否则写回并报字节数 |

## 3. 抓帧与像素测量（取证基础设施）

| 脚本 | 证明什么 | 怎么跑 | 判据（红在哪） |
| --- | --- | --- | --- |
| `shoot.py` | 用 `PrintWindow(PW_RENDERFULLCONTENT)` 抓指定窗口的位图落 PNG；`--screen` 整屏兜底、`--list` 列可见窗口 | `python scripts/shoot.py [--title 子串] [--pid N] [--out docs/shots/current.png] [--screen] [--list] [--grab screen\|print]` | **只报告，不拦任何东西（非零只在操作失败）**：找不到窗口时 stderr 打「没找到标题含「…」的窗口。先跑 --list 看有哪些。」并 `raise SystemExit(2)`；窗口尺寸异常 / `GetDIBits` 失败 / `PrintWindow` 失败各 `raise SystemExit`；画面本身不判 |
| `frame.py` | 看截图四件事：`crop` 放大 / `stack` 拼段 / `diff` 比两图 / `sheet` 出图集，输出都落 `%TEMP%\qb_*.png` | `python scripts/frame.py crop <in.png> <x0> <y0> <x1> <y1> [放大倍数=2] [输出名]`（另有 `stack <x0> <y0> <x1> <y1> <帧...>` / `diff <a.png> <b.png> [阈值=12]` / `sheet <帧...>`） | **只报告，不拦任何东西（非零只在调用错）**：参数 < 3 或不会的动词 ⇒ `print(__doc__)` `return 1`；`stack`/`sheet` 没给帧 `return 1`；`diff` 两图尺寸不同 ⇒ 「尺寸不同」`return 1`；量到的数（改动像素 n/total、包围盒）只打印 |
| `click.py` | 按**客户区**坐标点 gpui 窗口（gpui 自绘标题栏，客户区原点 ≠ 窗口左上角） | `python scripts/click.py --hwnd <int> --at x,y [--button left\|right] [--wheel 格数] [--hold] [--first] [--no-focus] [--probe out.png] [--click]` | **只报告，不拦任何东西（唯一非零出口是操作失败）**：打印 CLICKED / HELD / PROBED；只有 `ClientToScreen` 失败 `raise SystemExit("ClientToScreen 失败")` |
| `steps.py` | 跑一串点击 token 并逐步抓帧（`a11y_stress.py` / `verify_grow.py` 的点击与找窗口都从它这里取） | `python scripts/steps.py "nav:105" "row:121,120" "wait:2" "tab:474" "shot:qb_a"`——token：`nav:<y>`（概览 71 / 我的 113 / 第三方 155）、`row:<x>,<y>`、`tab:<x>`（y=150）、`click:<x>,<y>[,settle]`、`wait:<秒>`、`shot:<名>`、`wheel:<x>,<y>,档数`；先设 `QB_APPDATA` | **只报告，不拦任何东西（非零是前提与安全闸）**：没窗口打「NO WINDOW」`return 1`；拿不到前台 `raise RuntimeError("click(...) 中止：拿不到前台…，不动鼠标")`（安全闸，不点别人窗口）。⚠️ 文件头明写**送不进按键**，⌘K / 快捷键这条路它验不到 |
| `scancols.py` | 逐列扫暗像素，给一个矩形找回内容块的像素 x 范围（不要从 `frame.py` 的放大图反算，倍数还会被显示再缩放，曾偏 52px） | `python scripts/scancols.py <png> <x0> <y0> <x1> <y1> [--thr 150]` | **只报告，不拦任何东西**：只 print run / center / width，无退出码 |
| `rowpitch.py` | 量一列东西的行距（逐行数"与背景不同的像素数"成剖面，取各凸起中心之差） | `python scripts/rowpitch.py <in.png> <x0> <y0> <x1> <y1> [最小凸起高度=4]` | **只报告，不拦任何东西（唯一非零出口是用法错）**：参数 < 6 个 ⇒ `print(__doc__)` `return 1`；量完只 print「行距取值 = […] -> 一致」或「**N 种，不齐**」并 `return 0` ⇒ 不齐也不红 |
| `shot_now.py` | 沙箱起 debug exe 抓总览页一帧，量 2026-10-08 那四条缩进；三道闸：`QB_APPDATA` 沙箱 + `assert env["APPDATA"] == SANDBOX` + 收尾 `check_real_cfg`；出图前打 exe 的 md5 + mtime | `python scripts/shot_now.py`（无参数；硬编码 `TMP=%TEMP%\qb_v4`、`EXE=target\debug\qingbird-git.exe`） | **只报告，不拦任何东西（唯一非零出口是 assert 前提）**：APPDATA 没指到沙箱、40 次没找到窗口各 assert；缩进对不对它只出图 |
| `shot_wb.py` | 抓「我的仓库」工作台（或第三方）一帧，沙箱里 `ui_scale` / `theme` / `inspector_open` / `sidebar_*` 一律抄真实 config | `python scripts/shot_wb.py [仓库名=demo-dirty] [输出名=wb] [view=mine\|third]`；地标表：`demo-clean` 108 / `demo-behind` 146 / `demo-ahead` 184 / `demo-dirty` 222 / `no-upstream` 260，导航轨 y 概览 50 / 我的 94 / 三方 138 / 设置 755 | **只报告，不拦任何东西（唯一非零出口是 assert 前提）**：沙箱里没这颗仓库、APPDATA 没指沙箱、找不到窗口、拿不到前台各 assert |
| `flyout_shot.py` | 点开托盘「隐藏的图标」折叠面板，按新出现的顶层窗口句柄抓那一块（面板高度随图标数变，固定坐标裁过一栽） | `python scripts/flyout_shot.py <out.png> [exe]`（给了 exe 就先启动，走 `qbguard` 隔离，收尾 `check_real_cfg`） | **只报告，不拦任何东西（非零只在跑不起来）**：缺参数、「找不到 `TrayNotifyWnd`」各 `raise SystemExit`；找不到面板只打印「没找到新出现的面板窗口」+ 顶层窗口清单后 `return` |

## 4. A11y 与托盘 / 窗口走查

| 脚本 | 证明什么 | 怎么跑 | 判据（红在哪） |
| --- | --- | --- | --- |
| `uia_probe.py` | 用 .NET `UIAutomation` 当一次无障碍客户端（`AutomationElement.FromHandle` 后取属性 / `FindAll`），逼 gpui 把 a11y 树建起来，好让 `Duplicate a11y node id` 断言有机会响 | `python scripts/uia_probe.py`（无参数，硬编码 debug exe；先 `taskkill qingbird-git.exe`；`RUST_BACKTRACE=full`；点 NAV_MINE=(28,105) 与 ROW1=(121,144)；stderr 落 `%TEMP%\qb_uia_err.txt`，dump 落 `%TEMP%\qb_a11y_*.json`） | **只报告，不拦任何东西（唯一非零出口是 NO WINDOW）**：重复 id / panic 只从 stderr 挑含 qingbird / Duplicate / panicked 的行打印 |
| `a11y_stress.py` | 在 **a11y 已激活**的条件下把整个界面走一遍（五页签 / 右栏开关点两下 / 设置四分类逐个点开 / 关设置 / 再走两屏），专抓潜伏的重复 `ElementId`；控件按 a11y 名字**现场量**，不靠像素扫 | `python scripts/a11y_stress.py [open\|closed\|both]`（默认 `both`；用 `target/debug/qingbird-git.exe`；夹具 `%TEMP%\qb_a11y` 自己现造；真实配置指纹前后比对） | **真闸**：有问题 ⇒ 「判读：有问题 N 条」`return 1`，红在——某一步 `DIED` / `EXC` / `ALREADY`（断言如「标题栏没找到右栏开关」「点完 label 没换成…」「类别标题至少该有三行」「点 y=… 之后整棵树与上一屏逐字相同」「关不掉设置」「点了取消，设置对话框还在」），或 stderr 里出现 `panicked` / `Duplicate` |
| `close_probe.py` | 阶段 22 专用探针：`PostMessageW` 发 WM_CLOSE，看关窗三档真怎么走 | 先 `$env:QB_APPDATA="$env:TEMP\qb_close"`（不设就打印「`!! 没设 QB_APPDATA —— 会碰到真实配置，先停下`」`return 2`），再 `python scripts/close_probe.py <ask\|remember-tray\|remember-exit\|plain-tray\|cancel\|tray\|exit>` | **只报告，不拦任何东西（非零只在没设沙箱 / 没找到窗口）**：结论（窗口在 / 进程在 / `config.close_action` / 该不该弹面板）只打印「[1]…[4]」 |
| `tray_icon_query.py` | 用 `Shell_NotifyIconGetRect` 问系统托盘图标在不在、在哪（老式 hWnd + uID 身份，uID 是进程内自增，扫 0..7） | 先设 `QB_APPDATA`，`python scripts/tray_icon_query.py <exe> <out.png> [uID...]` | **只报告，不拦任何东西（唯一非零出口是缺参数）**：没有 `tray_icon_app` 窗口时打印「→ 托盘 crate 连隐藏窗口都没建起来，图标必然不在托盘里」后 `return` |
| `tray_icon_check.py` | 折叠面板开 / 关各拍一张求差，找出「有应用才多出来的那一格」 | 先设 `QB_APPDATA`，`python scripts/tray_icon_check.py <exe> <out.png> <before.png> <after.png>` | **只报告，不拦任何东西（非零只在跑不起来）**：「找不到 `TrayNotifyWnd`」/ 缺参数 / 两次点不开折叠面板各 `raise SystemExit`；两张一样时只打印「→ 完全一样：托盘里没多出任何图标」后 `return` |
| `p2_dlg_shots.py` | 把 10 个开得起来的对话框逐个走进去（a11y 名字定位）各抓一帧，两套逐帧 diff——合并「实机看得见的地方」（底栏 / 开壳 / 按钮链）前后的像素复验；抓不到的两个（⌘K 入口、要打字的向导）在 docstring 里如实登记并改走代码级论证；**docstring 里有判读口径**：列弹簧 1px 相位漂移会产生 43k/82k/127k 三档假差异，先量分割线 x 同相位再 diff | `python scripts/p2_dlg_shots.py <tag>`（抓一套到 `target/p2_frames_<tag>/`；另有 `diff` 子命令比 before/after；沙箱 `QB_APPDATA` + qbguard 收尾指纹） | **真闸**：任一走查步骤断言失败（入口键不在树里 / 主键没出现 / 关闭后主键还在 / 指纹变了）即抛异常非零；像素 diff 本身只打印，归因靠 docstring 的判读口径 |
| `probe_window_geometry.py` | 连续启动两次记录窗口矩形 / 客户区 / DPI / 监视器，证明帧差异**不在几何层**（2026-10-13 实测：(752,202) 1919×820 逐运行恒定，1px 漂移在 app 内部分数布局的弹簧相位里） | `python scripts/probe_window_geometry.py <tag1> [tag2...]`（每 tag 启动一轮；沙箱同 qbguard） | **只报告，不拦任何东西（唯一非零出口是跑不起来）**：每轮打几何快照，结论只打印 |

## 5. 启动冒烟与偶发红

| 脚本 | 证明什么 | 怎么跑 | 判据（红在哪） |
| --- | --- | --- | --- |
| `startup_smoke.py` | 启动冒烟五项：进程存活 / 可见窗口 / 未挂起 / stderr 字节数 == 0 / `app.log` 无「PANIC at」/ 真实 config 未动 | 先设 `QB_APPDATA`（脚本自己还会 `mkdtemp` 覆盖），`python scripts/startup_smoke.py [秒数=15]`；**必须用 debug exe**（release 是 GUI 子系统，没有 stderr） | **真闸**：`ok = (alive and 有窗口 and not hung and len(err)==0 and not panicked and not touched)` 为假 ⇒ 「❌ 有问题，见上」`return 1`；全过 ⇒ 「✅ 全过」`return 0`；找不到 exe 也 `return 1` |
| `startup_probe.py` | 量「窗口出现 → 内容上屏」多久（阶段 16 秒开出口）：逐帧与最后一帧逐像素差分，差多少说明那一刻还在骨架屏 | 先设 `QB_APPDATA` 指向副本，`python scripts/startup_probe.py --tag <冷/热标记> [--extra-wait 秒]`（帧落 `%TEMP%\qb16_<tag>`；采样点 `SAMPLES_MS=[0,100,250,450,800,1500,3000,6000,12000,25000]`，基准是**窗口出现**而非进程启动） | **只报告，不拦任何东西（非零只在跑不起来）**：25 秒没出现窗口 `return 2`、可比帧 < 2 `return 1`；「每帧与最后一帧的差 %」只打印 ⇒ 秒开与否不判红 |
| `hunt_flaky_core.py` | 连跑 N 轮 `cargo test`，把**偶发红**的用例名与 panic 段抓出来（「重跑就绿」对 flaky 零证明力） | `python scripts/hunt_flaky_core.py [轮数=6] [filter] [包名=qingbird-core]`（`qingbird-core` 带 `--lib`，其余带 `--bins`） | **只报告，不拦任何东西**：只 print「==== 偶发红的名单 ====」与每轮退出码 ⇒ 退出码恒 0 |

## 6. 视觉契约出口判据（一次量一档）

| 脚本 | 证明什么 | 怎么跑 | 判据（红在哪） |
| --- | --- | --- | --- |
| `verify_dark_five.py` | 32-17：每屏**明暗两档各抓一帧**（两档各 17 帧），把 v4 §8「深色档五条成立条件」逐条量出来；深色档名从 `gui/theme.json` 现读并先验"那一跑真的暗" | `python scripts/verify_dark_five.py`；只重量判据（复用盘上帧）加 `--measure-only`；用 debug exe，夹具自己造并断言 `.git` 在场，帧落 `%TEMP%\qb_dark` | **真闸**：`fails` 非空 ⇒ 「判读：没过：{fails}」`return 1`；红在 ⓪ 每帧是不同的一屏（逐字节撞车）、⓪b 四档在整档里都出现过（不出现=判据自己瞎了）、① 反差 ≥ 12/255、② 分界靠 `line-soft`、③ 品牌色提亮、④ wash 深底 + 亮字、⑤ 文字比底亮 |
| `verify_card_gaps.py` | 阶段 31「卡间距一致」的全站证据：抓 4 屏 × 浅/深两档，逐段竖剖数 `line-soft` 边框线，量两卡之间那条带高 | `python scripts/verify_card_gaps.py [--measure-only]`（只重量则复用盘上帧与 `rects.json`）；`import verify_dark_five as V` 复用沙箱 / 抓帧 / 档位 | **真闸**：`bad` 非空 ⇒ `return 1`，红在「同一屏里卡间带高有 N 种［…］」与自检「`{档}`：整档 N 屏一屏都没量到横贯线 ⇒ 档位读错或帧不是这份代码画出来的」；「这一屏竖着只叠了 N 张卡 ⇒ 不适用」是合法不红。基线：卡间带 27 条全是 6px、每档横贯线 233 条 |
| `verify_type_tiers.py` | 阶段 21 出口三条一次量完：① 三级明暗（一级 `foreground` / 二级 `body(t)` / 三级 `tier3` 各自有点，相邻亮度差 ≥ 12/255）② 胶囊高度三档字号各量一次 ③ 头卡里「文字下沿 → `card_split` 线 → 页签上沿」的气口（期望 16 = `GAP_TIGHT` 4 + `GAP_LG` 12） | `python scripts/verify_type_tiers.py [--measure-only]`（四跑 = 浅 small/medium/large + 深 medium） | **真闸**：`bad` 非空 ⇒ `return 1`；含两条前置自证——八帧 sha 两两不同（否则「三档各量一次」变成「一档量了三次」）、每套主题 medium 帧的画布档精确 > 5000 颗（否则主题没生效）；⚠️ ③ 超阈值只打印「⚠️ 比期望大 4px 以上：这一档登记数值，不判红」 |
| `verify_grow.py` | 阶段 31.4 补二十五那条回归判据：**抽屉开与关，左栏那一列不许被抢宽**；五跑八腿五十七条判据，全部落在帧上量的数（P 压窄探针 / O 逐项地标 / F–M 八腿 / N 过程抖动） | `python scripts/verify_grow.py`；`--parse-only` 只从源码现读壳层项、不起进程；环境变量 `GROW_CYCLES` 覆盖八腿轮数（默认 4） | **真闸**：有任何判据不成立 ⇒ 「判读：」后 `return 1`；`--parse-only` 发现结构对不上（不再是纯加法 / 读不到 `MAIN_MIN_W`）也 `return 1`——**拒绝跑，不猜** |

## 7. 体检与测量（只报数）

| 脚本 | 证明什么 | 怎么跑 | 判据（红在哪） |
| --- | --- | --- | --- |
| `audit.py` | 界面体检：从截图量出客观数字（A 画布利用率 / B 卡片对齐 / C 竖向节奏 / D 边框一致性 / E 字号阶梯 / F 对比度 / G 触达密度），把「看着粗糙」翻译成能改的条目 | `python scripts/audit.py docs/shots/live-01.png`；要与设计稿对比加 `--ref docs/03-我的仓库工作台.png`；`--theme` 默认 `gui/theme.json` | **只报告，不拦任何东西**：`main()` 没有返回值 ⇒ `sys.exit(main())` 恒 0；检到问题只打印「检出 N 条」 |
| `measure_status_cost.py` | 32-12 的前置实测：量「状态检测合并成一条命令」省多少——A 现状逐条 / B 合并候选 / C 合并后仍必须的两条（`stash list` + `remote get-url origin`），进程数 6→3；夹具 `%TEMP%\qbg3212-repos`（含一颗 2 万文件的仓库） | `python scripts/measure_status_cost.py make\|time\|sign\|equiv\|clean`（默认 `make`；`time` 每档 15 轮取中位数） | **真闸只在 `sign` 模式**：方向不一致 ⇒ `sys.exit(0 if (ok1 and ok2) else 1)`（红在「领先/落后的正负号两种读法方向一致」）；不懂的模式 `sys.exit(2)`；`make` / `time` / `equiv` / `clean` 恒 0 ⇒ 这四个模式**只报告，不拦任何东西** |
| `measure_mincontent.py` | 量 `MAIN_MIN_W` 的真值：中间那一列压到多窄开始切内容；每档取主区卡的右边界 1px 边框 x 与最右内容 x，气口掉到 14 以下 = 压过头 | `python scripts/measure_mincontent.py`（无参数；`TMP=%TEMP%\qb_v4`、`demo-dirty`、debug exe，逐档 `drag_right_border(dx=20)` 共 6 档） | **只报告，不拦任何东西（唯一非零出口是 assert 前提）**：沙箱配置 / `find_window` / `ensure_focus` 各 assert；量到的「切了 +N / 没切」只打印 |
| `measure_backslash.py` | D92「改完报三个数」的测量语义核对：对 `README.md` / `docs/UI-ROADMAP.md` / `AGENTS.md` 各出「行数 / 块引用行 / 反斜杠总数 / 连续两处 / CRLF」，工作树与 `git show HEAD:` 并排 | `python scripts/measure_backslash.py`（无参数） | **只报告，不拦任何东西**：无退出码 |

## 8. 共用隔离模块

| 脚本 | 证明什么 | 怎么跑 | 判据（红在哪） |
| --- | --- | --- | --- |
| `qbguard.py` | 抓帧脚本共用的隔离闸门（被 `import` 的模块，不是入口）：`launch_env()` 把 `APPDATA` 覆盖成 `QB_APPDATA` 并打印警告；`check_real_cfg(before)` 按 (mtime, size, sha256[:16]) 核真实 `%APPDATA%\com.qingbird.git\config.json` 有没有被动过、返回 bool | 不单独跑；被 `steps` / `uia_probe` / `a11y_stress` / `startup_probe` / `startup_smoke` / `close_probe` / `tray_*` / `flyout_shot` / `shot_*` / `measure_mincontent` 引用 | **无判据**：模块没有退出码；`check_real_cfg` 的返回值由调用方决定要不要红（多数调用方只打印） |

---

## 已知缺口

**① 进程退出码恒 0 的纯报告脚本**（接进 CI 或用 `&&` 串在提交前，**拦不住任何东西**）：

`add_bom.py`、`audit.py`、`hunt_flaky_core.py`、`measure_backslash.py`、`scancols.py`，以及 `measure_status_cost.py` 的 `make` / `time` / `equiv` / `clean` 四个模式。

**② 有非零出口，但那出口不覆盖它量的那件事**（串进 CI 只能拦住"没跑起来"，判据本身一个字都不接退出码）：

`click.py`、`close_probe.py`、`explorer_icon_check.py`、`flyout_shot.py`、`frame.py`、`measure_mincontent.py`、`rowpitch.py`、`shoot.py`、`shot_now.py`、`shot_wb.py`、`startup_probe.py`、`steps.py`、`tray_icon_check.py`、`tray_icon_query.py`、`uia_probe.py`——它们的非零出口全是"找不到窗口 / 参数不足 / 沙箱前提没成立"，量到的结论（图标画得对不对、行距齐不齐、托盘有没有多一格、启动几秒上屏）一律只打印。

**③ 部分判断只报不判**（整条脚本是真闸，但有几格明确不负责，看到"全过"别当成这几格也过了）：

- `extract_exe_icon.py`：`ExtractIconExW` 报 0 不红。
- `verify_type_tiers.py`：③ 那条超阈值只登记数值。
- `verify_dark_five.py`：单帧层面 `MUST_APPEAR` 四档只报数（由整档自检 ⓪b 兜底）。
- `verify_grow.py`：`--parse-only` 的盲区自证只打印；八腿的地标只打印。
