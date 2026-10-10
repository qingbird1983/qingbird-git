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
  路线与决策台账 D1–D92）、`docs/UI-DESIGN.md`（设计规范）、
  `docs/UI-DESIGN-v4.md`（**v4 视觉契约**：颜色 / 圆角 / 间距 / 阴影 / 动效 /
  图标这一类的唯一真源）。⚠️ **一条视觉规则只允许有一个家**——那六类数值**只写在 v4 里**，
  路线表只登记"为什么这么定"和"分几步落"，**不复述数值**（复述一次就多一处会漂的真源）
- 临时实验放在：不要提交到仓库
- 测试文件放在：Rust 单元测试在各模块 `#[cfg(test)]` 中（core 179 个、gui 93 个）
- ⛔ **改 `docs/*.md` 的一段只用 `Edit`，不许"读全文 → 替换 → 写全文"**
  （2026-10-07 实锤，见路线表 D92：一次性内联 Python 脚本把**整篇**路线表的
  「换行+前导空白」压成一个空格、全文 `\` 翻倍成 `\\`，**4193 行 → 2816 行**）。
  **最阴的地方是文字一个字都没丢**——归一化比对能证明这点，于是
  "读一遍看看有没有少东西"这类自查**天然查不到它**：坏的不是文字是**结构**
  （多行 bullet 并成一行、引用块只剩首行还带 `>`、`\[ \]` 渲染成 `\\[ \\]`）。
  ⇒ **改完报三个数**（都是结构计数，不是目测）：`wc -l`、`grep -c '^>'`、双反斜杠数。
  与「PowerShell here-string 会毁行尾」（路线表 22-8）是同一条坑的两种触发方式：
  **都是拿"全文重写"去改局部**。另一条连带：**一次性脚本要落盘再跑**——
  那一轮的脚本是内联的、没留文件，出事连成因都指不出，只能从损坏范围反推
  ⚠️ **那三个数里有一个会自己动**：反斜杠那一档，只要**登记这三个数的那句话里
  写了反斜杠**，改这一句就把这个数改了（2026-10-10 同一档数写了 153 / 146 / 147 三次才对，
  全是边写边被自己改掉）。⇒ **登记句里不写反斜杠**，改完当场复量一次为准，别凭"加了多少"
  ⚠️ **这一档的名字要写准**：`grep -o` 数反斜杠那条在 Git Bash 里给的是**反斜杠总数**，
  不是"连续两处"的成对数（本轮一度把它叫成「成对反斜杠」——实测 `README.md` 总数 8、
  连续两处 **0**；`docs/UI-ROADMAP.md` 总数 145、连续两处 **3**）。
  D92 要挡的那类损坏（每个反斜杠翻倍）**照样挡得住**——翻倍会把总数直接翻倍。
  ⇒ 判据有效，但**别拿它的数值去推断"有几对转义"**；要那个数就现场用
  字节级脚本数（`scripts/measure_backslash.py` 一次给全四种测法：行数 / 块引用行 /
  反斜杠总数 / 连续两处 / CRLF，并与 `HEAD` 版并排比）
  ⚠️ **工作树的行尾不是仓库级常量，是"这个文件最后一次被谁怎么写"的函数**（同一天量出来的）：
  本机 `core.autocrlf = true` 且**无 `.gitattributes`** ⇒ `.rs` 检出就是 CRLF
  （`net.rs` 1073 行 CRLF / 0 条裸 LF），`docs/UI-ROADMAP.md` 这几轮一直是 LF，
  而本轮编辑过 `README.md` 与 `AGENTS.md` 之后**它们整份变成 CRLF**（83 → 144 行，CRLF 也 144）。
  ⇒ 补二十八那句「write_text 把整份文件 LF 翻成 CRLF」**方向可能是记反的**：
  本来就是 CRLF 的文件被按 LF 写才是 CRLF→LF。**观察没错**（sha 对不上而文字一字未丢），
  **方向必须现场量**——数一下 `\r\n` 与裸 `\n` 各有几处，再决定往哪边还原。
  ⇒ **翻行尾不会进仓库**：`git diff --numstat README.md` 只报 72 增 / 11 删，
  若行尾被当成内容变化，旧文件那 83 行会整份进删除侧 ⇒ autocrlf 在 diff 与 add 两侧都归一化。
  **只有"逐字节比 sha"这种工作树层面的判据会被它骗**
  ⇒ **字节级锚点（变异脚本、一次性改源码的脚本）先量目标文件此刻的行尾再写**；
  方向写反的症状是 assert 报**"锚点 0 次"**——本轮就因此**一轮变异都没跑成**
  ⇒ **变异脚本必须把还原写进 `finally`**：第一版是顺序语句，第二轮 assert 抛出时
  第一轮的变异**还留在源码里**，靠手动反向替换才还原。
  **还原之后要再跑一遍全绿**，只比 sha 不算完成
- ⛔ **单测里不许 `std::env::set_var` 去改进程级全局**（`APPDATA` 尤其）。
  cargo 默认**多线程**跑同一个二进制里的用例，而别的模块也在改同一个变量
  （`scan_cache` 自己带了一把 `APPDATA_LOCK`，注释里明写「并行跑会
  『偶发红、重跑又好了』」）。⇒ **把被依赖的那个值收成函数参数**，
  单测整条链路都在沙箱里跑，**并行安全由构造保证，不靠一把运行时锁**。
  `diag` 那一版是这么写的：`log_path_in(Option<&str>)` / `note_at(..)` / `note_to(&Path, ..)`。
  **连带两条纪律**：① 单测跑完要**核对它没往真实目录写**（本项目：
  `cargo test` 连跑 5 遍，真实 `app.log` 长度必须 260→260 不变）；
  ② **"跑绿一次"对 flaky 零证明力**——本项目那条用例第一次跑就是绿的，
  绿是侥幸，隔一轮改别处重跑才红，而红的原因是**别的用例**
- ⛔ **测试的前提不能外包给环境**（2026-10-09 实锤，路线表 31.4 补二十五⑥）：
  `cancel_kills_running_git` 拿黑洞地址 `10.255.255.1` 造"命令还在跑"这个前提，
  而本机现在对它 **0 毫秒就拒绝连接** ⇒ git 在 400ms 的取消窗口之前就走了，
  **红的是前提，不是取消功能**——这种红隔天会被当噪音关掉。
  ⇒ 换成**本机监听一个端口、`accept` 之后一个字节都不回**（`git::hanging_url()` +
  `hold_the_connection()`，`sync.rs` 那条同款一起换）。
  **判据：这条测试要的那个状态，是不是它自己造出来的**。拿"外部世界会怎么反应"当夹具，
  等于把测试挂在别人的实现上。改完做变异验证（把 `is_cancelled()` 弄成恒 `false`
  ⇒ 精确红在 `assert!(!out.timed_out)`），还原 sha 逐字节相同、grep 零残留
- ⛔ **「跑完测试核对它没往真实目录写」这条闸 2026-10-09 真的抓到了事，抓出的是一条
  在用户数据里长了两周的垃圾**：`repos::init_repo` 的 doc 明写「`git init` → 写 `.gitignore`
  → 初始提交 → **加入管理列表**」，末尾无条件 `store::save`，而 `init_tests` 那三条
  要在 `$TEMP` 建**真的 git 仓库**才能验 `.gitignore` 追加与初始提交 ⇒
  **每跑一次 `cargo test` 就往用户 `config.json` 塞一条临时仓库**，两周下来他列表里
  43 条有 **40 条是 `qbg-init-` 垃圾**（他真实的只有 3 条）。
  `cargo check` 零错零警、core 184 / gui 109 全绿、令牌闸干净——**没有一闸报它：
  它不坏任何功能，只往用户数据里长垃圾，所以只有"比末端指纹"这一条路能抓到它**。
  落法：把「登记进列表」做成 `init_repo` 的**必填参数**（生产 `true`、测试一律 `false`，
  忘传**编译不过** = D78；"登记"不能从函数里整个拿走，它是生产那一条调用路径的职责），
  守卫写成**测试进程里的末端断言**（跑完查列表里查不到这一条路径），
  **不写成脚本比 mtime**——脚本那条只能我跑，断言谁跑都在。⇒ **凡是函数名带"做一件真实的事"
  （init / add / save / remove）又要进单测的，先问它末尾有没有 `store::save`**；
  同一批里独立的第二处是 `repos::with_repo`（仓库不在列表里时也无条件写盘，注释却写"不改配置"）
  ——**同一个问题在两处长出来，说明它不是漏抄而是缺这一问**
  ⚠️ **验这条的变异本身会写盘** ⇒ 变异那一次**在外层 shell 把 `APPDATA` 指到 `%TEMP` 沙箱**再跑
  （不是单测代码里 `set_var`，与上面那条纪律不冲突），验完比真实 config 指纹证明一个字节都没多

## 常用命令

全量检查：`cargo check --workspace`

后端单测：`cargo test -p qingbird-core`（业务逻辑的测试都在这，141 个）

GUI 侧单测：`cargo test -p qingbird-gui`（32 个，只放不依赖 gpui 初始化的纯逻辑；
GUI 大部分要窗口才能测，判据是抓帧 + 实机点，见下面的「界面怎么验」）

设计令牌闸：`python scripts/check_design_tokens.py`（阶段 31.3 起与 `cargo check` 同级，
**每阶段出口都要跑**）。七条规则：字面色值 / 裸间距圆角字号 / 裸时长 / `.bg(色)` /
文字冒充图标 / 同文件重复 `ElementId` / **直写 `colors.border`**（v4 的 `line` 档只认
`tokens::edge(t)`，R7 只放 `tokens.rs`）。白名单只放定义处（`tokens.rs`、`theme.rs`、
图标像素判据那两处）。
- ⚠️ **R7 抓的不是错值**：`edge(t)` 与 `t.colors.border` 是同一个色，显示永远不会错。
  它挡的是"同一个属性两个名字"——**读代码看不出这两拨东西是不是一个用途**，
  于是"把这一档调深一点"时改一处漏一处（阶段 31.4 补十五查实：改前 5 别名 / 6 直写）
- ⚠️ **它抓的这一类编译器看不见**：两处各自都对，错在"同一属性有两个出处"。
  本项目为它交过四次学费（9 种圆角、13 处裸 gap、`inspector_reopen` 同帧两颗、
  ✓/✗ 字符当图标）
- ⚠️ **新闸必须变异验证，而且要连"应当放行"一起验**：探针里塞 6 条违规 + 3 条
  合法（注释里的色值、`.p(0)`、中文文案里的 `→`），确认 6 报 3 不报，删完 grep 零残留。
  **误报的闸会被人当噪音关掉，那比没有闸更糟**——R5 第一版把箭头也算假图标就是这个下场

出 exe（调试）：`cargo build --workspace` → `target/debug/qingbird-git.exe`（~66 MB）

出 exe（发布）：`cargo build --workspace --release` → `target/release/qingbird-git.exe`
（~32 MB，**约 5 分半**——gpui 那一大棵树。**release 与 debug 是两条产物路径**，
出安装包只能用 release 那条，别拿 debug 的凑）

出安装包：`pwsh scripts/build_installer.ps1`（顺带编 release；
已编过只想重打包就加 `-SkipBuild`）→ `dist/qingbird-git-Setup-0.1.0.exe`（~9 MB）
- **发版流程（2026-10-10 第一次跑通 v0.1.0，顺序固定）**：
  ① **版本号写在三处、必须一起改**——`gui/Cargo.toml`、`crates/core/Cargo.toml`、
  `installer.iss` 的 `#define MyAppVersion`。⚠️ **只改一两处不会有任何报错**，
  后果是"包里写的版本与界面读出来的版本不是同一个数"（`cargo` 那份进 `env!`/关于框，
  `.iss` 那份进文件名与注册表显示名）。
  ② 提交 → ③ **附注**标签 `git tag -a vX.Y.Z`（附注才带说明，轻量标签在 Release 上读不出内容）
  → ④ `git push origin main vX.Y.Z` → ⑤ 在**标签那条提交**上 `pwsh scripts/build_installer.ps1`
  → ⑥ `gh release create vX.Y.Z dist/qingbird-git-Setup-X.Y.Z.exe --verify-tag`
      （⚠️ **标签带 `v`、包名不带 `v`**：`installer.iss` 里
      `OutputBaseFilename=qingbird-git-Setup-{#MyAppVersion}` 而 `MyAppVersion` 是裸的 `0.1.0`，
      照 `Setup-vX.Y.Z.exe` 去拼会得到一个不存在的文件）
  （本机 `gh` 已登录 `qingbird1983`、token scope 含 `repo`，所以建 Release 不用动手点网页）。
  ⚠️ `dist/` 与 `target/` 都在 `.gitignore` 里：**安装包从不进仓库**，唯一的去处是 Release 资产。
  ⚠️ **末端校验是把 Release 上那个包下回来与本地比 sha256**——它一并挡住"挂错文件"与
  "直链还指向上一个版本"，比看网页显示的文件大小强（v0.1.0 实测 `fecc576f…` 两侧逐字节相同）。
  ⛔ **安装包自己从不真装一遍**（装是有副作用的动作，归用户双击）：我能报的只有三条——
  包是新的（时间戳 + sha）、PE 头对（`python scripts/pe_subsystem.py target/release/qingbird-git.exe`
  必须 `Subsystem=2`，读产物不读代码）、挂对文件（上面那条 sha 回比）。
  **"装完能跑"必须由他说**，别写成已验证。
  ⚠️ 无代码签名 ⇒ 首次运行会撞 SmartScreen，这条**要写进 Release 正文的「已知边界」**，
  否则别人以为是病毒。
- 打包用 **Inno Setup 6**，脚本 `installer.iss` 在仓库根。本机已装
  （`%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe`；NSIS / WiX / cargo-wix
  / cargo-bundle **一个都没有**）。**按用户安装、免 UAC**，装到
  `{localappdata}\Programs\青鸟 Git`
- **应用是真正的单文件 exe**：运行期一个文件都不读（`theme.json` 走
  `include_str!`、图标走 `include_bytes!`、托盘位图来自 `OUT_DIR`），
  所以 `[Files]` 只有它一条。**用户配置在 `%APPDATA%` 下、不在安装目录里，
  卸载不会带走**
- ⚠️ **`installer.iss` 与 `packaging/lang/zhCN.isl` 必须带 UTF-8 BOM**。
  没有 BOM，Inno 按系统 ANSI（本机 GBK）读，中文全变乱码，
  而 ISCC **照样报 "Successful compile"**。`build_installer.ps1` 每次都会补
- ⚠️ **`OutputDir` / `MessagesFile` 相对 ISCC 的工作目录解析，不是相对 .iss**
  ——所以那两条路径写成"相对仓库根"，脚本里先 `Set-Location` 到仓库根。
  写 `..\dist` 过一次，包吐到 `F:\AIwork\dist\`（**仓库外面**）
- 中文向导来自社区翻译
  [kira-96/Inno-Setup-Chinese-Simplified-Translation](https://github.com/kira-96/Inno-Setup-Chinese-Simplified-Translation)
  （官方发行里没有；`raw.githubusercontent.com` 在本机不通，**`git clone` 是通的**）
- ⛔ **`gui/src/main.rs` 必须有 `#![cfg_attr(not(debug_assertions),
  windows_subsystem = "windows")]`**。少了它 Rust 默认编出 **console 子系统**
  （PE 头 `Subsystem=3`），Windows 启动就**分配一个控制台** ⇒ 从资源管理器双击
  / 从开始菜单点，必带一个黑终端窗口（2026-10-05 实机装完才发现）。
  **必须用 `cfg_attr` 不写死**：release 是用户那份（要 GUI 子系统），
  **debug 要保留控制台**——它才是 agent 自己的验收产物（`steps.py` 抓帧、
  `startup_probe.py`、panic 栈、`eprintln!` 诊断全靠 stderr），
  写死会让 debug 的 panic 消息**直接丢失**，「闪退」变成查不了的事。
  判据不取「代码写了」而取产物：`python scripts/pe_subsystem.py <exe>`
  读 PE Optional Header 偏移 68 那个 WORD，**2 = GUI / 3 = console**
- ⛔ **release 走 GUI 子系统 ⇒ 没有 stderr**。凡是「出错了但界面上看不见」的地方
  写 `crate::diag::note(..)`（落 `%APPDATA%\com.qingbird.git\app.log`，带时间戳，
  超 512KB 轮转成 `app.log.1`，**永不 panic 永不阻塞**），**别再写 `eprintln!`**
  ——它在 debug 里好好的，装到用户机器上就凭空消失（托盘建不出来会变成
  「完全静默、什么都不发生」）。`main()` 第一行的 `diag::install_panic_hook()`
  解决同一个问题的 panic 版本；**只写日志、不弹 `MessageBox`**（那会让一个后台
  panic 变成模态框卡死整条消息泵）
- **`os.rs` 派生子进程一律加 `CREATE_NO_WINDOW`**（`explorer` / `cmd /c start` /
  浏览器 / **选文件夹的 `powershell`**——最后这条会挂黑框**阻塞 200–400ms**
  等用户在选）。**唯一例外是 `open_terminal`**：`wt` / `cmd start powershell`
  **故意不加**，用户点的就是「开终端」，加了功能等于作废。
  判据是「**这颗操作交付给用户的东西本身是不是一个窗口**」

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
- **卡外壳、卡头、卡内盒——四类构件各有唯一出处**：`widgets::card(t)`（竖排段卡：
  白卡 + 1px 边 + `r_xl` + **`p(GAP_XL)`** + `sh_2`）、`widgets::card_shell(el, t)`
  （**只给外壳那五样**，方向与内边距留给调用方——统计卡横排、活跃度卡内边距 12，
  三类卡方向不同，硬并会让 `card()` 长出一堆开关参数）、`widgets::CardHeader`
  （`new(title)` / `.leading(AnyElement)` / `.note(text, danger)` / `.right(AnyElement)`
  （可多次）/ `.build(t)`）、以及卡内盒两颗 `widgets::list_box` / `widgets::sunken_box`
  （**分档判据在 v4 §2.1**：里面点得动的是列表盒、不给底；只读内容才是下沉盒
  `card-sunken` 底）。**别再自己手搓一份**。阶段 13-1 起因就是查出来的分叉：空间页
  自己手搓了外壳（内边距 **16**），日志 / 说明用 `log::section`（**8**），历史 / 变更
  **根本没有卡**——五个页签摆一屏里内边距有两个值、有的有壳有的没壳。`log::section`
  已删，**不留转发层**（留一层转发只会让人以为它还能用）。右侧操作的分组关系**留给
  调用方**（`widgets::vdivider(t)` 插在中间）：各页的分法不一样，不该由表头决定
  - ⚠️ **2026-10-08 又数出两轮同类分叉**：卡外壳五样有 **3 个出处**（`card` /
    `stat_card` / 总览活跃度卡），卡内盒 `r_lg` 有 **7 处**、三个 bg 档两个 border 档
    （其中 `dialogs/init.rs` 与 `purge.rs` 是逐字相同的两份拷贝，还错用了**画布**档
    `background`）。⇒ **判"统一了没有"要数"同一属性还剩几个出处"，不是数新构件
    被引用了几次**。查法就是那两条 grep：`rounded\(px\(r_xl` / `rounded\(px\(r_lg`
  - ⚠️ **`.bg(t.colors.popover)` 不是下沉档**：theme.json 里它两条主题都等于**卡面白**
    （浅 #FFFFFF / 深 #16262B），写出来是空操作却让人以为"这格比卡深一档"。
    下沉只认 `muted.background`（v4 的 `card-sunken`）
  - ⚠️ **v4 那两档灰各只有一个取值入口**：`line` 走 `tokens::edge(t)`、`line-soft` 走
    `tokens::track(t)`，**都不许直写 `t.colors.*`**（闸 R7 钉着）。根因不是漏改：
    `edge()` 的注释以前写着"次按钮描边"，于是分隔线与中性状态点**以为不归它管**、
    各自直写 token，同一个值就长了两个名字（补十五实测：改前 5 别名 / 6 直写，改后 11 : 0）。
    ⇒ **给一个"档"起名字时别把用途写死**，否则第二个用途来了会另起一个名字
- ⚠️ **往一个共用槽里搬内容，先问这个槽原来装的都是什么**（2026-10-08 实机：日志卡那句状态话
  从"独占一行"上移进 `CardHeader::note` 那个附注位）。那个槽此前八处装的全是短句，
  所以它**没写 `min_w_0` / `truncate`**；而搬进去的这句最坏情况是**失败原因**（几十字，
  `op_line` 压过也长）⇒ 长句会把表头右侧那组键**顶出卡外**，症状是"键不见了"而不是"句子被截断"。
  落法：附注那一格补 `.min_w_0().truncate()`（对现有八处是空操作）。
  **同一族的第二种触发是"原位置自带保护、新位置不带"**：那一行原来有定高 + `truncate`，
  搬进表头之后这两层都没了。**"搬"不是只搬文字，判据/上限/截断都得逐层对一遍**
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
- ⛔ **`.loading(true)` 的按钮完全不响应指针**（基座 `button.rs:1679`，**有单测锁着**：
  `assert!(!Button::new("test").loading(true).interactive())`，原文 "A loading button
  must be as inert as a disabled one"）。2026-10-05 踩实：停止键写了
  `.loading(lg.running)`，而它**只在 running 时出现** ⇒ **这键一出现就是死的**，
  而它正是构建卡死时唯一的救命键。`cargo check` 零错、单测全绿、**抓帧完全正常**
  （按钮在、颜色对、hover 也变）——失效的不是某个分支，是整颗按钮。
  **判据：`.loading()` 与「这颗键自己就是那个进行中操作的终止手段」互斥**——
  `loading` 答"这个操作在进行中，别再点"，停止答"我来结束它"，语义相反。
  同理别给它 `.disabled(running)`。"正在跑"不用它表达：键**只在 running 时出现**，
  存在本身就是指示器。⇒ 路线表 **D64**
- **图标只认一套**：`use gpui_kit::assets::IconName as AssetIcon`（完整 lucide 1830 个 + 品牌 Bird）。
  不要再用 `component::IconName`——那是另一套，线宽与视觉重量不同，且没有 GitBranch
- **hover 与动效**：gpui 没有 CSS 那样的 `:hover` 样式，悬停靠 `on_hover` 记状态
  （`Shell.hovered`，带 on_hover 的行返回类型变 `Stateful<Div>`）；
  加载占位用官方 `Skeleton`（自带呼吸动画），进度动画用 `with_animation`
- ⛔ **要「到位后过冲再收住」的动效，官方两条弹簧档都做不到，别硬用**
  （2026-10-05 实机：「侧栏滑出后没有回弹效果」）。欠阻尼弹簧的第一次过冲
  ≈ 行程的 `exp(-πζ/√(1-ζ²))`：

  | 档 | ζ | 过冲 | 侧栏 220px 行程上的实际过冲 |
  |---|---|---|---|
  | 官方 `spring_control` | **1.00**（`Spring::new` 的默认，**没被覆写**） | 0% | 0px，压根不过冲 |
  | 官方 `spring_move` | 0.85 | 0.63% | **1.4px**——算得出，**看不见** |
  | `tokens::SPRING_BOUNCE_DAMPING` | 0.60 | 9.5% | **21px** |

  ⇒ `theme.json` **没有 motion 段**（键只有 name/mode/font/mono_font/radius/
  radius.lg/colors），动效只能落在代码里，所以用 `tokens::spring_bounce()`。
  **判据：位移越大越要低阻尼**；小位移反过来**必须**高阻尼（0.6 阻尼下 4px
  的过冲会变成「抖了一下」）。`tokens.rs` 有单测把过冲钉在 **5%–15%**——
  这类缺陷 check 零错、单测全绿、弹簧确实在跑、抓帧看着正常，
  **任何以「有没有报错 / 有没有在动」为判据的自查都拦不住它**，
  只能把「过冲量本身」写成可计算的断言
- ⛔ **侧栏与检视抽屉是同一套写法的两份拷贝，改一处必须回头改另一处**
  （2026-10-05 实机：左栏修好，用户立刻报「右栏还是会弹簧一下」）。
  两边各有一个**代次字段**（`sidebar_gen` / `insp_gen`），各自在
  `commit_*_width` 里自增。**这两个代次不是漏抄、不能"统一成一个"**——
  它们是两个独立的 `ResizableState`，动画 id 若共用，两个弹簧会抢同一份
  keyed state、互相把对方拽走。**判据：凡是"两处用了同一套写法"，
  改完一处要逐处搜第二份，并在字段注释里写明"为什么它们必须各留一份"**，
  否则下一个人会当重复代码去合并
- ⛔ **`spring()` 的内部 position 外部改不了**——它在 `window.use_keyed_state`
  里，且**静止在目标上时直接返回、不推进**（`motion.rs:609`）。
  ⇒ **凡是由「拖拽把手」这类直接操作改出来的值，一律要把弹簧重新就地锚定**，
  办法是**换一份 id**（新 id 的初值就是 `position = target`，`motion.rs:586-592`）。
  本项目用 `Shell::sidebar_gen` 代次后缀，**只在 `commit_sidebar_width`
  （拖拽松手）里自增**。不换的后果实测是：面板被**拽回旧宽度再弹过来**，
  **每次拖拽必现**。换 keyed state **不会泄漏**——`window.rs:3347` 每帧
  `next_frame.clear()`，当帧没被访问的直接回收
- ⛔ **外部文件拖放的 payload 类型是 `ExternalPaths` 不是 `PathBuf`**。
  写 `on_drop::<PathBuf>` **一行都不报错，只是永远不触发**——`PathBuf` 是
  「本应用自己往外拖文件」时的 payload（`gpui-pre/src/window.rs:8203` 那个
  测试用的就是它，很容易看成是外部拖放）。外部拖进来的走
  `ExternalPaths`（`interactive.rs:694`），取路径用 `.paths()`。
  底下三层早通、只是没人挂监听：平台每个窗口都 `RegisterDragDrop`
  （`gpui-pre-windows/src/window.rs:1517`）、`Entered` 把 payload 塞进
  `cx.active_drag`（`gpui-pre/src/window.rs:5478`）、`Submit` 被翻成一次
  `MouseUpEvent`（`:5499`）所以元素级 `on_drop` 天然收得到；光标侧对 `CF_HDROP`
  直接给 `DROPEFFECT_COPY`（`gpui-pre-windows/…:1169`），**不用挂 `can_drop`**。
  **「窗口任意位置都能放」就是把 `on_drop` 挂根 div**：drop 监听在**冒泡阶段
  + 命中即触发**并 `stop_propagation`（`div.rs:2897-2924`），本项目没有任何
  子元素注册 drop 监听，事件一路冒到根
- **拖入/悬停那类标志有三个写点，缺一不可**：`on_drag_move` 置位、
  `on_file_drop_exit` 清位、**`on_drop` 也要清位**。第三个最容易漏——
  漏了的话那个提示层（多半是 `.absolute().inset_0()`）会**一直挂着并盖住
  整个界面**。判据：凡是用它控制一个整屏覆盖层的标志，把三个写点一起数一遍
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
- ⛔ **官方件自带的默认样式读的是它自己的 token 语义，换到我们配色下会串档——
  接一个富文本/组件件，先把它默认读哪个 token 查出来**（2026-10-09 实锤，路线表
  31.4 补二十六）。markdown **行内代码**的底色是 `theme.accent`
  （`gpui-component/text/mod.rs:59`，上游单测钉着），shadcn 语义里那是**一档浅灰**
  （悬停面、小胶囊），而 v4 把 `accent.background` 定成**品牌深青 #0E4F55**
  （深色 #3FB9C4）。⇒ 用户看到的是「接近黑的色块把字埋了」，而**代码里没有一个值是错的**：
  两边各自都对，错在一个名字两套语义。`t.colors.accent` 有 **18 处、10 个文件**在读
  （侧栏选中行、总览徽标、拖放描边、设置页…），它就是品牌色本体 ⇒ **改 token 是改半个界面**，
  落法是**在调用侧覆盖那一格**（`TextView::style()` 是叠加式的，只填 `inline_code` 两个字段：
  底 `muted.background` = v4 的 `card-sunken`、字 `foreground`）。
  **判据：接任何一个"自带一整套排版"的件（`TextView` / `DataTable` / `List`），
  先问它的每一档默认色读的是哪个 token、那个 token 在我们板子上是什么档**——
  编译器不报、单测不红，只有帧上看得见
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
  - ⚠️ **同一族的第三种：一个合法的调用「在哪一帧」发生，决定它有没有作用，而 API 不报
    任何信号**（2026-10-09 实测）。`ResizableState::resize_panel(ix, ..)` 的三条分支是
    `ix >= sizes.len()` 直接 return / `ix+1 < len` 走 handle / `ix > 0` 走"从前一个兄弟取空间"
    （`resizable/mod.rs:76-87`）。侧栏面板是**同一帧的渲染阶段**才由 `sync_panels_count`
    补进 `sizes` 的，所以"在挂载边沿把它钉回意图宽"那一次调用**三条分支一条都不命中
    = 静默 no-op**（当场临时打一行 `before=[1232.0]`——只有一项——才看出来）。
    ⇒ **调状态实体的写 API 之前，先把它那一刻的输入量出来**（`sizes().len()`、
    这一帧是不是刚被 sync 补上），别假设"我挂在挂载那一帧，所以状态已经就绪"
- ⛔ **「没实现」和「缺功能」不是一回事，先分清是哪个再决定要不要动那一层**
  （D74 的前置教训）。2026-10-06 实测：`ssh -T git@github.com` **无代理 4.1 秒**
  认证成功、`git ls-remote` 强制 `http.proxy=` **6.3 秒**成功 ⇒ **SSH 走 22 端口
  不被墙，github 的 SSH 远端直连就是通的**，而 `git.rs` 从来没构造过
  `ProxyCommand`。我上一轮把「两版皆无所以是盲区」写进"查清但不动"的三条里，
  措辞是**描述现状**，位置放得却像**缺陷清单**。⇒ **自查清单里每一项都要标清
  它是「现象」还是「缺陷」**，否则下一个人会拿着"两条皆无"当证据去实现
  一个新功能——**为一个不存在的问题加新功能比不修更坏**，因为它会让人以为
  这个方向被验证过。判据是**去测那条路本身通不通**（一条只读的
  `git ls-remote` / `ssh -T` 就够），不是读代码看有没有那个分支
- **赛跑（`race_direct_vs_proxy`）里不先验车**：起跑前对候选做一次完整
  `probe()`（CONNECT github.com:443，建连 5s + 读 8s）等于把**本该并行的事先
  串行做完一遍**，代理软件没在跑时那十几秒纯白等——而那恰恰是最需要它早点
  让位的场景。`proxy::tcp_reachable` 只做 TCP 可达（2 秒）；端口不通的那一路
  几毫秒退赛，赢的那一路不受影响。**手填代理与 `last_good_proxy` 两档干脆不探**：
  手填探不通说明用户填错了，那该报错而不是悄悄换一个
  - ⛔ **赛跑对「会改仓库的写操作」是有害的，2026-10-08 实测坐实（未修，见路线表
      31.4 补二十⑥）**：`do_pull` 走 `NetMode::Auto` 时 `race_direct_vs_proxy`
      会**同时起两条 `git pull --ff-only`**，两个 git 进程一起写同一个
      `.git/FETCH_HEAD` 与 ref ⇒ **两条都 `fatal: Cannot fast-forward to multiple
      branches.`**，第三趟串行兜底才成功。复现：同一夹具串行一条 = 正常，
      并行两条 = 全 fatal。⇒ 症状是"每次拉取日志里先撞两次墙再说成功"。
      **看到那两行 fatal 别去改 pull 的参数**（参数没错），要动的是
      "写操作不许赛跑"这一条策略
- **改了阶段提示的文案，要回头搜认旧措辞的判据**：`route_of` 认的是
  「**代理并行**」这个连续子串，而赛跑文案从「直连与**已缓存代理**并行尝试中…」
  改成「直连与**代理 127.0.0.1:7897** 并行尝试中…」后，**代理与并行被地址隔开**、
  子串不成立 ⇒ **赛跑阶段全程把路径标签说成「直连」**。编译器不报、单测不红
  （除非你正好断言了那句文案）
- ⛔ **信息在中间层被静默截断，编译器与单测都拦不住，因为"字段有值"（D73）**。
  用户报「老版本进度条上有**直连/代理、网速、百分比**，你现在就一个光进度条，
  **很多线没接上**」——查实断点**在中间结构体的字段声明里，不在逻辑里**：
  `ops.rs` 的 `LogEvent::Stage` **只声明了 `stage` 与 `percent`**，
  core 解析好的 `detail`（网速）**在这一层被整条丢掉**；`LogState` 也没有
  `detail` 字段；`log.rs::progress_bar()` **只画条**——`stage` 塞进
  `accessibility_label`（读屏专用，**肉眼不可见**）、`percent` 塞进 `bar.value()`。
  **core 一行都没错**（`route_of()` 拼路径标签、`transfer_detail()` 抽网速，
  连 core 单测都断言了 `detail == "已传输 1.2 MiB · 2.3 MiB/s"`）。
  ⇒ **判据是「末端有没有用出来」，不是「上游有没有填」**：
  **字段没接全不是错误**——上游照常构造少填一个，编译器不报；上游单测照绿，
  因为那一层确实有值；界面能跑能点。**缺陷只在"末端看不见"这一点上**。
  - **同项目内的不一致是最强线索**：`clone.rs` 那份进度块**三样全有**，
    同一个 `ProgressEvent`、同一个 core ⇒ 证明**不是设计取舍，是某条线压根没写过**。
    这是上面那条「两处用了同一套写法」的**反面教材**：不是漏抄，是没写。
  - **拿到"某项信息不见了"，从源头一路读到人眼能看见的那一格**，
    逐个中间结构体问"它有没有这一格"。断点常常在**字段声明**里。
  - **端到端钉一条断言，别只在上游钉**：走完真实回调链路，断言它**到了事件上**。
  - ⛔ **D73 的第三种断法：字段有、末端也画了，但中间「没人搬」——搬运是有时钟这一维的**
    （2026-10-10 实锤，路线表 31.4 补二十八）。用户报「克隆最终是成功的，但过程中
    进度条一直在左右滑、永远显示『准备中…』，百分比 / 网速 / 是代理还是直连一样没有」。
    断点既不在 core 的解析里、也不在 GUI 的字段声明里，而在**时序**上：
    `dialogs/clone.rs::run_clone` 把 `ProgressSink` 倒进界面那一次调用**挂在 `clone_repo`
    返回之后**，而克隆是分钟级的 ⇒ 全程没人搬；成功那一刻 `finish_clone` 紧接着就把
    对话框关了，那一批等于从没画过。**拉取那条链路（`ops.rs` 两条循环）一直有 120ms
    轮询，克隆这一条从来没抄** ⇒ 这是「两处用了同一套写法」的**第三型**：
    不是改一处忘另一处，是**第二处压根没写**，而它不报编译错、单测不红、
    抓帧看着"进度条在动"，**看着像在工作**。
    ⇒ **判据：凡是「后台跑一个分钟级操作 + 界面显示它的进度」，先问谁在搬、多久搬一次**，
    不是只问"字段有没有值"。连带：`percent.is_none()` 会走官方不定态流光条 ⇒
    **「进度条在滑」是这条缺陷的确认信号而不是它在工作**（画得对，喂进去的一直是空）。
    落法三条一起：轮询间隔收成一颗 `ops::PROGRESS_POLL`（第三条链路要同节奏就必须
    只许一个数）；搬运规则收成一颗 `pump()` 让轮询与收尾共用，结构上消灭
    "只在结束时才搬"那条第二路径；**循环给两条退出**——`done`，**或对话框已经没了**
    （用户取消后 git 还在后台跑、`done` 不会来，只判前者就是给不存在的界面空转）。
    ⚠️ **`cx.spawn` 那条循环本身测不到**（Windows 无 `VisualTestContext`）：
    单测只钉得住它依赖的搬运规则，"循环真在转"只有帧能证 ⇒ 拆 `CloneProgress`
    （`CloneDialog` 带 `Entity<InputState>` 构造不出来，那条判据原本一条单测都写不了）
- ⛔ **必填信息没进签名 ⇒ 每条调用路径都得自己记得补，其中一条一定漏**（D78）。
  用户报「在『第三方仓库』页拖文件夹进来，仓库进了『我的仓库』」。查实断点
  **同样在签名里，不在那一行逻辑里**：`repos::add_repo(path)` **压根没有
  「归属」这个参数**，它造一条 `RepoEntry::default()`（`is_third_party = false`），
  **谁要归属谁自己去补 `store::set_third_party`** ⇒ 于是四条添加路径里
  **三条忘了或写错**：拖放没补（就是这个 bug）、克隆没补（**core 不认识界面**，
  压根拿不到「用户当时在看哪一组」）、添加弹窗补了但**默认值写死 `false`**、
  撤销移除**先写一条 false 再由 `restore_entry` 盖回**（最终对但落盘两次）。
  ⇒ **这是 D73 的镜像变体**：D73 断在**读**的方向（末端没画出来），
  这条断在**写**的方向（源头就没填）。**两者的断点都在字段/参数声明里，
  都不在逻辑里**，排查手法一样：沿每一条调用路径问"这一步知不知道这件事"。
  - **修法不是给漏的那条补一句，是把字段提升成必填参数**（`add_repo(path, third)`）
    ——漏传**编译不过**，编译器替我们记住。判据是**让"忘"没有选项**，
    而不是"这次记得写"。
  - **老版本（`11d56d0` 的 `api.addRepo(path)`）同样没有这个参数**
    ⇒ **两版共有的老毛病，不是重写引入的回归**。**动手前查一次老版本**，
    别把账算到重写头上（"现象 vs 缺陷"那条的前置：先确定它是哪一类的缺陷）。
  - **「用户眼睛看着的那一组」≠「当前选中的那个」**。判据用 `side_focus`
    （侧栏标题上写着的那组）——侧栏只画一组，**跨组的选中项不会被切走**
    （只有 `reveal_repo` 才切），拿 `selected` 判会**整整差一组**。
    **判据函数的签名里就不要有那个错的入参**，编译期比注释管用。
  - **「用户看不见的状态」要在界面上说出来**。归属是不可见的 ⇒ 拖动时那张
    薄纱卡上**写出「松开即加入『第三方仓库』」**。不写的话用户只能拖完去
    列表核对、发现错了再右键搬。
  - **平台按窗口给的光标改不掉时，别假装能放**：Windows 的复制光标是
    `RegisterDragDrop` 那层直接回 `DROPEFFECT_COPY`（按窗口，不按元素），
    所以"这一屏不接受拖放"**只能靠不画落点提示 + 松手时说清去哪放**来实现。
    基座虽有 `can_drop`（`div.rs:579`/`:1233`），但它只挡派发、不改光标，
    **判据仍该落在一个可单测的纯函数里**——拆成"框架谓词 + 渲染条件"两处
    才是新的不一致。
- ⛔ **非客户区不是「点击永远不派发到子元素」，是「抬起那一步不派发」**
  （D76）。Windows 侧 `handle_nc_mouse_down_msg` **会把非客户区的按下翻成
  client 事件派发、且不消费**（`gpui-pre-windows/src/events.rs:1071`），
  所以元素级 `on_mouse_down` **挂得上**、`on_click` 挂不上。
  **这个区分决定了两件事**：① 官方 `TitleBar` 用不了的真正原因是它的按钮
  走 `on_click`；② 想在 `WindowControlArea::Drag` 那块挂逻辑，
  **只能用 `on_mouse_down`**，别去写 `on_click` 然后等它触发
- ⛔ **在 `WindowControlArea::Drag` 上按了鼠标，抬起会永久丢失**（D76）。
  机制：`HTCAPTION`（`events.rs:976`）→ 抬起被 `DefWindowProc` 的
  `SC_MOVE` 系统移动循环吃掉（它捕获了鼠标）⇒ **永远不会有
  `WM_NCLBUTTONUP`** ⇒ 依赖 `MouseUpEvent` 收尾的东西**全部卡住**。
  受害者是文字框选：`end()` 只挂在 `MouseUpEvent` 上
  （`text_selection.rs:2807`/`:2834`），门是 `is_selecting`（`:1904`），
  而移动处理器（`:2817`）**根本没检查 `pressed_button`** ⇒ 卡住之后
  每个鼠标移动都在扩选区。
  ⇒ 修法在拖拽区挂 `on_mouse_down`，**两条一起上**：
  `TextSelection::end(window, cx)` 收掉在途的 +
  `GlobalState::suppress_text_selection(cx)` 不让它起跑新的。
  **别去改基座那个处理器**——门本身是对的，是我们这处「抬起不来」把它卡住的
- **字号是一组「步进要能看见」的档，不是六个随手取的数**（D77）。
  阶段 28 的判据：`tokens.rs` 里六档**底部三档两两差 ≥ 2px，三档都要满足**，
  由单测 `the_bottom_three_tiers_are_far_enough_apart_to_read_as_levels` 钉住。
  根因是老的 **11/12/13 只差 1px** ⇒ 182 处里 135 处（74%）挤在最小两档，
  而「三级」肉眼分不出来、读起来是「小一号的正文」。
  ⇒ **每档是六档全套整数，不要改成「一套值 × 一个系数」**：
  系数一乘 `12×0.9` 与 `14×0.9` 只差 1.8，层级又没了。
  **⚠️ 字号是运行期可变的 ⇒ 行高与胶囊高度必须写成字号的函数**
  （`row_h(text_px)` / `pill_h()`），**不许再写死**：
  切字这种缺陷 check 零错、单测全绿、抓帧看着也只是「有点挤」，
  只能在结构上让它不可能发生。**官方件不随我们的档长**
  （`Checkbox` 恒 24px），那类行高可以留
- **新增共用构件时先确认名字没被占**：2026-10-05 加的 `progress_readout`
  一度和 `clone.rs` 里本地那个 `progress_block` 撞名——后者其实是**进度区**
  （读数 + 实时输出流），是**包含关系不是重复**。**两个都留着、名字分开**，
  别看到同名就当重复去合并
  - ⚠️ **反面也要查：新写的"解析 / 聚合"类函数，先确认没有现成的那一颗**。
    同一天在 `sync.rs` 里给拉取结果写了一份 `sum_numstat(输出文本)`，
    **跑测试时才看见** `history.rs` 早就有一颗 `parse_numstat`（提交详情页在用，
    还带两条单测、`-\t-` 已经解析成 `None`）⇒ 当场删掉新写的那份，改成
    `totals_of(&history::parse_numstat(..))`：**解析一处、加总一处**。
    判据不是"名字撞没撞"，是**"这个格式的活儿有没有人已经干过"**——
    函数名不同（`sum_numstat` vs `parse_numstat`）照样是第二个出处
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
  `on_click` 调 `remove_window()` 看着一样，但**不是"绕过 WM_CLOSE 就唤不醒"**——
  那条是错的，见下面「两条查错的结论」；**但仍然别自己处理**：`WM_CLOSE` 会走
  `on_window_should_close`，而关窗询问面板（阶段 22）就挂在那条路上，
  自己 `on_click` 调 `remove_window()` 等于把这一档整个绕过去。
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
- **图标是**一份源图派三份**（`gui/assets/app-icon.png` → `gui/build.rs`），
  改图标只改那一张，**别再去别处画**：

  | 位置 | 要什么 | 怎么来 |
  |---|---|---|
  | 标题栏左上角 | PNG | `include_bytes!` 原始 PNG（`app_assets.rs`） |
  | 系统托盘 | **原始 RGBA 字节** | build.rs Lanczos3 缩到 32 → `app-tray.rgba`（`tray.rs` 只 `include_bytes!`） |
  | 桌面 / 任务栏 / Alt-Tab | exe 里的 **`.ico` 资源** | build.rs 编七档 PNG 条目 → `app.ico` → `embed-resource` |

  **构建期解，运行期零新依赖**：`image` 与 `embed-resource` 本来就在
  `Cargo.lock` 里（`image` 是 `gpui-pre` 的传递依赖），进 `[build-dependencies]`
  不带来额外编译成本，**主依赖表一个图像库都没加**。托盘原来那套手绘矢量
  （-160 行）已删，**别再加回来**。
  ⚠️ **`embed_resource::compile()` 吃的是 `.rc` 脚本，不是 `.ico`**（D68）——
  喂 `.ico` 进去 `rc.exe` 不报错、只产出一个**零条目的空 `.res`**，
  **构建全绿而 exe 里一个图标都没有**。更早一层的判据是**文件头 32 字节**：
  `DataSize=0` + `HeaderSize=0x20` = 空资源文件（`app.lib` 32 字节 vs 正常的 121332）。
  ⇒ build.rs 里先落一份 `1 ICON DISCARDABLE "…"` 再交给它；路径一律换正斜杠
  （rc 的预处理里 `\` 是续行符）
  ⚠️ **源图在 2026-10-07 换成了「透明底贴纸」**：青色箱子本体 + 白描边，
  **四角 alpha=0**（旧那张是"白底 + 圆角深墨方块、四角 alpha 252 满幅不透明"）。
  后果：托盘上是**箱子本体**而不是深色方块（深浅两条任务栏都靠自己的白描边撑边界）、
  标题栏那枚不再露白角。**"想要透明圆角得先有一张带 alpha 的源图"这一条已经兑现了**——
  但**仍然不能在托盘这头做遮罩**，那会让三处又不一致。
  钉它的两条单测跟着翻了：`tray_icon_is_a_transparent_sticker`（四角必须透明 +
  圆心必须实心）与 `tray_icon_is_not_blank`（**判据从"不透明占比"换成"包围盒 ≥70%"**，
  因为贴纸图的占比会随圆角与描边漂：新图实测 57.5%、旧图 100%，而包围盒实测 26×28）
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
- ⛔ **绝对定位的负 `inset` 会被 gpui 丢掉，往父盒子外面探只能靠负 `margin`**
  （2026-10-08 帧上实测：同一行并排两根 3×20 的条、纵向写法完全一样，只有 `left`
  一正一负 ⇒ `left(px(0.))` 那根出、`left(px(-10.))` 那根**一像素都没出**）。
  仓库里的正解样板是 `widgets::card_split` 的 `.mx(px(-GAP_XL))`：铺满整卡宽靠负 margin
  **扩盒子**、再用等量 `px` 把内容收回原位。侧栏行那根品牌竖条是同一条 idiom 的第二次使用
- ⛔ **零内容高的裸 `div` 上 gpui 不画 `border`——要一条线就给一个 1px 的实体**
  （2026-10-08 帧上实测：`widgets::card_split` 是**同一颗函数**，"承载一行控件"那一处
  （头卡下面那排页签）画得好好的——y=158 整行 line-soft；而"单纯断一段"那一处
  （同步卡里「语言技术栈」上面）**26 行全白、一像素都没出**，因为那个 div 内容高是 0）。
  症状是"少了一条线"：`cargo check` 零错、单测全绿、抓帧看着也只是"这儿有点空"。
  ⇒ 落法是把线画成**一个 child**：`.child(div().h(px(1.0)).w_full().bg(track(t)))`，
  **两种用法一起修好**（父层 `mx(-GAP_XL) + px(GAP_XL)` 已经把这一格撑到卡边，
  与原来那条 border 同宽，页签那一处复量过仍在 y=158）。
  与「`.min_h(120)` 被紧跟的 `.min_h_0()` 覆盖」同族：**基座里"按 CSS 直觉该有 1px"的东西，
  帧上没有就是不画**，判据是逐行打色不是"代码写了"
- ⛔ **「同一格位换内容」要占位不许二选一：`opacity(0.)` 占位 + `absolute().inset_0()` 盖上去**
  （2026-10-08 落"进度条从日志卡搬进语言技术栈那一格"时定的）。
  直觉写法 `if busy { A } else { B }` **在这条路上必错**：两块高度不同（那一格实测 **83px**、
  进度块 **27px**）⇒ 卡片在操作开始与结束**各变一次高**，下面那张卡上下跳 56px，
  而"卡片高度别变、把高度还给日志"正是同一轮里用户要的。三条基座事实：
  ① `Styled::opacity` 文档原文 "this element **and its children**"（`styled.rs:746`），
  `div.rs:2538` 真的带进绘制 ⇒ **占位但不画**；② **`hidden()` 不是"看不见"是 `Display::None`**
  ——**连位置一起没了**，用它等于回到二选一；③ **正 `inset` 有效**（被丢的是负 inset，上面那条），
  所以覆盖层走 `absolute().inset_0()`。⇒ 落点收成 `widgets::slot_cover(base, cover)`，
  附带好处是**格位高度自动跟随内容**（图例折行时那一格跟着变高），不需要新增任何一个高度数。
  ⚠️ **占位那块不许放可点的东西**：`opacity(0)` 仍参与命中测试，能点的东西放进来就是
  "看不见却能按"（目前唯一落点是纯只读的语言段 ⇒ 成立；**以后要往这块加交互先回来读这句**）
- ⛔ **一个东西有两个落点 ⇒ 它的文案只许有一个出处，而"哪个落点"要钉成真值表**
  （2026-10-08 同一条链上抓出来的两条，都是不报错的错）。进度块现在有两个家
  （同步卡里语言段那一格 / 语言段被关掉时的日志卡）：
  - **文案漂**：日志卡那侧是 `unwrap_or("准备中…")`，新加的同步卡那侧写成 `unwrap_or_default()`
    ⇒ **同一个操作在两个落点上，一个有话说、一个是空的**（第一版抓帧里那一格只剩一根光条，
    就是这个空串顶出来的 19px 空行）。修法不是补那一句，是**让两处调同一颗函数**
    （`log::progress_bar(t, id, lg)`，`id` 参数化）——与 D73 同族的镜像：**不是逻辑错，
    是"同一件事有两个出处"**
  - **落点判据**：写错的方向有两个、**两个都不报错**——两处都画 = 同一操作两根进度条
    （两处的 `ElementId` 不同，**连"同帧两颗同 id"那个 debug panic 都不触发**）；
    两处都不画 = 进度彻底看不见。⇒ 收成纯函数 `detail.rs::progress_slot_is_lang(sync_on, lang_on)`，
    单测把**四种开关组合逐一钉住**（`&&` 改成 `||` 当场红）。`log_panel(.., progress_here)`
    这一个参数**由调用方给**，不在里面自己读 store
- ⛔ **`overflow_y_scroll` 的裁剪盒是元素自己的边框盒，而且两个轴都裁**
  （`style.rs:639 overflow_mask`：`(x=Visible, y=Scroll)` 那一臂取 `min.x..max.x` =
  整个横向，只有设了不透明边框才内缩）。⇒ **挂在滚动容器里的东西，探出容器边框盒
  就等于不存在**，而且症状是"少了一小块"而不是报错。侧栏竖条那次的成因是两层叠加：
  先是负 `left` 被丢，改成 `left_0` 之后又发现气口写成行的 `px` 会把包含块一起推走
  （绝对定位的包含块是**父元素 padding 盒**，`px` 不减 padding 盒左缘），
  再加竖条在 `children` 里排在淡底**前面** ⇒ 被淡底不透明底色盖住。
  **三条都是只读代码看不出来的**，判据是逐列打色，不是"有没有渲染"
- ⛔ **`h_resizable` 一组里只要每一格都留下了宽度偏好，基座就在容器变宽变窄时按百分比重分整组**
  （2026-10-09 实锤，路线表 **31.4 补二十五**）。症状：他设的 `sidebar_w` = 263 **从来没生效过**
  （被重分成 360、630），抽屉一开中间那一列只剩 **282px**——`cargo check` 零错、单测全绿、
  抓帧看着只是"中间窄了点"。⇒ 基座自己写了早退条件：**只要有一格没有宽度偏好
  （`panel.size == None`）整组就不做比例重分**（「Flex already fits the container, so there
  is nothing here to adjust」）。落法收成一颗 `Shell::keep_main_flexible`：**每帧**
  `reset_panel(中间那一格)`。三个坑：① **必须每帧**——`update_panel_size` 下一帧就把量出来的
  宽写回去锁死；② 外层那一句要写在 `insert_panel` **之后**（挂载分支会把整组重分成 `Some`）；
  ③ 拖拽进行中不用回避——基座算出的中间格 = 容器 − 侧栏，与 flex 自己算的同一个数。
  **回归判据 = `target/verify_grow.py` 那条"抽屉开与关两帧左栏白面同宽"**
- ⛔ **用户给的 px 数：规则的出处是他，数值的出处必须是实测**（同一条补二十五）。
  他的令是「中间不足 **870** 就向右撑窗口」——规则照做，但 870 那一档帧上照样把右边那排键
  切掉一半（`measure_mincontent.py` 一档 20px 压出来：**948 干净 / 928 开切**）⇒ 下限取 950。
  **规则对了、输入用错，症状一模一样**，所以"照抄他给的数"不是尊重需求，是把缺陷钉进常量。
  两个出处都写进 `tokens::MAIN_MIN_W` 的注释，别只留一个
- ⛔ **记账字段不许在"什么都没做"的那一帧被抹平**（同一条，红过一次才写对）：
  `give_shell_width_back` 第一版写成"先算目标、把 `shell_grown_w` 清零、再看要不要 resize"，
  而抽屉还开着的那些帧正好**一段都还不出去**（需求 == 当前宽）⇒ 欠账被清零，
  等真收起抽屉时窗口永远停在撑过的宽。判据：**改了哪个字段，就要有"这次改动生效了"的证据**；
  没生效的那一帧一个字段都不该动。同理 `shell_grow_pending` 要"兑现了就清"——
  它是"欠着一次还没生效的请求"，不是"上次请求过的值"，语义写错就成了挡下一次请求的钉子
- **托盘框架不带**（gpui-pre 0.3.7 全量搜索无 tray API），要自己挂 `tray-icon`；
  系统通知用 gpui-kit 的（`window.push_notification(.. .system())`），
  但未打包的 exe 必须先 `cx.set_app_identity`，否则发不出去。
- **Windows 上"关窗不退出"要分两件事：拦不拦得住关闭 / 拦不拦得住退出。**
  真正要的是后者，那只由 `QuitMode` 决定
  （`app.rs:2922-2926`）：`Default` 在非 macOS 上**等于** `LastWindowClosed`
  （= 最后一个窗口关了就退出），`Explicit` 才是"只在我调 `App::quit` 时退"。
  设法：`gpui_kit::application().with_quit_mode(gpui_kit::QuitMode::Explicit)`
  （`QuitMode` 经 `gpui_kit::*` = `gpui::*` 转出），或运行期 `cx.set_quit_mode(..)`。
- ⛔ **"Windows 上没有拦截关闭的钩子，它不存在"——这条是错的**（2026-10-05 查实）。
  当年只查了两层，**漏了 `PlatformWindow` trait 那一层，而它在平台 crate 里**
  （`gpui-pre-windows`，不翻那整个 crate 根本看不见）：
  ① `TitleBar::on_close_window(..)` 第一行 `if cfg!(target_os = "linux")`——✅ 真的
  ② `remove_window()` 只是 `self.removed = true`、没有 prevent 参数——✅ 真的
  ③ **漏的**：`gpui-pre/src/window.rs:6658` `pub fn on_window_should_close(cx,
  f: Fn(&mut Window, &mut App) -> bool)`，Windows 侧 `events.rs:110` 消息泵里
  **`WM_CLOSE => self.handle_close_msg()`** 真的调它，`:336-341` 返回 `false` 时
  回 `Some(0)`（DefWindowProc 不跑）⇒ **窗口不关**。已用它做了关窗询问面板
  （`dialogs/close_prompt.rs`，见路线表阶段 22 / D65）。
  ⇒ **判据：拿到"做不到"先写下"具体做不到的是哪一步"，再问那个能力挂在哪一层。**
  组件层没给钩子 ≠ 框架没给；常见的一层之差：本 crate vs **平台 crate**
- ⛔ **"自己 `on_click` 调 `remove_window()` 会绕过 `WM_CLOSE`，托盘唤不醒"
  ——这条也是错的**。托盘唤起是 `tray.rs::show_or_open`：它只看 `cx.windows()`
  里有没有窗口，**有就 `activate_window()`、没有就 `open_main_window()` 新开一个**，
  **与窗口当初是怎么关的毫无关系**。同一条错误推论的第二个受害者
- ⛔ **置一次性标记之后必须 `cx.notify()`**：`Entity::update` **不会**自动安排
  下一帧。2026-10-05 第三次为这条交学费——`on_window_should_close` 的回调里
  置了 `pending_app_quit` 却没 notify，而那次 `WM_CLOSE` 已被回调返回的 `false`
  挡住，没有别的东西会触发重绘 ⇒ 标记躺在字段里没人消费，
  表现为「点了完全退出，既不关窗也不退出」。`cargo check` 零错、单测全绿、
  **抓帧完全正常**
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
- ⛔ **抓帧 PNG 与客户区坐标的换算从前记反了**（2026-10-10 实测更正）。
  正确的一条是 **`帧 = 客户 + (8, 0)`——偏的是 x，不是 y**：
  `GetWindowRect` = 60,40..1435,868（1375×828，**正好等于 PNG 尺寸**），
  而 `ClientToScreen(0,0)` = 68,40。从前按「y+8」换算，每一击**往左偏 8px**，
  症状是"点了没反应"而不是"点错了地方"。
  ⇒ **别信任何一条写进文档的像素偏移，用之前现场量一次**（三行 ctypes 就够）
- ⛔ **一颗键的位置不能从"移到它上面"的 hover bbox 反推**（同一条链上翻的车）。
  本轮把「保存」定成 (815,724)，来源是一次 hover 的 bbox (791,709)-(839,739)
  ——**那个 bbox 量的是「取消」的悬停面**（相邻两颗键，间距 18px）。
  按下去等于点取消：弹窗关掉、一个字段都不存，事后 `pinned_cmds` 还是 `[]`。
  **"点了没存"和"保存功能坏了"在帧上长得一模一样，只有量坐标能分开它们。**
  判据：量**那颗键自己的墨**（深色胶囊逐行打色），不要量"我以为是它的那次变化"
- ⛔ **`mr(负)` 探出这一格只能挂在裸 Div 上**。写在 `List` 上（它自己 `render`
  不认外面的 margin）或写在带 `.w_full()` 的那一格上（"宽 = 父内容宽"写死了，
  `mr` 再负也不动右缘）都是**探出 0**——`cargo check` 零错、单测全绿，
  帧上只是"条偏了 5px"。⇒ 统一成"裸 Div 探出 + 滚动件在里面铺满"。
  连带一条基座事实：**滑块画在滚动盒自己边框盒右缘往里 9..4**，四页同一系数，
  各页位置不同**完全是盒子右缘不同**，不是画条的代码不同 ⇒ 调这一档要动盒子不要动条
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
  ⚠️ **`QB_APPDATA` 是脚本之间的约定，只有 `qbguard` 认识它**——由它翻译成
  子进程的 `APPDATA`，**应用读的是 `APPDATA` 本身**（`store::config_path()`
  就是 `std::env::var("APPDATA")`）。⇒ **新写任何起子进程的脚本，
  `env` 一律 `qbguard.launch_env()`，不自己拼**；并断言
  `env["APPDATA"] == 沙箱` 再 `Popen`（`startup_smoke.py` 2026-10-05 就死在这：
  另起炉灶只设了 `QB_APPDATA`，15 秒里改写了用户真实 `config.json`——
  **闸门是有效的，正是它的收尾指纹核对报出来的；坏的不是纪律，是我绕过纪律**）；
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
- ⛔ **自动找控件的三条判据（2026-10-10 一次踩三条，全在 32-2 那条取证脚本里）**：
  - **按色相方向扫，不按"离某个 RGB 多近"扫**：模态遮罩把背景压暗之后，画布灰正好
    落进 danger 胶囊那档粉的 ±22 里 ⇒ 扫出来两个框横跨全屏。带底色的控件要按
    "R 明显高于 G 与 B" 那种**方向**判，任何一档灰的三通道是相等的
  - **连通块 + 尺寸档，不用列剖面**：列剖面只适合"整行都是它"的东西（`rowpitch.py`
    量行距）。找一颗 14px 的图标时，图表柱子与正文暖色抗锯齿会把那一列拉到
    y 80..312，那颗点被淹在里面，扫出来 0 个 —— 症状是"控件不存在"而不是"扫法不对"
  - **阈值按量出来的数量级定，不拍一个数**：本轮"对话框开起来"是 123736 个采样点，
    "悬停气泡出现"是 42 个。第一版阈值写 30，于是"光标停在点上顺手把气泡唤出来"
    被误判成"还开得出对话框"。**红的是判据的分辨率**
  - 连带两条：**窗口出现 ≠ 画好了**（`find_window` 只看尺寸，抢跑会拍到一张 3KB 空帧
    ⇒ 拍到有内容为止）；**`steps.py` 头部那份导航坐标随窗口宽度漂**（1296 宽实测
    53/95/137，不是 71/113/155）⇒ 每次跑先扫一遍导航轨，别信写死的数（D93 同条）
- ⛔ **变异脚本的还原要覆盖"整个变异集"，不是只还原本条改的那个文件**
  （2026-10-10 实测）：四条变异分两文件，每条开头只 `write(base[file])` ⇒
  C 改 `sync.rs` 时 `stash.rs` 里 B 的变异还在，D 反之 ⇒ 红名单里混进**上一轮的条**，
  "红在预期的那一条"这个判据当场失真。`finally` 那层还原是对的，**每条之间也要还原**
- ⛔ **格式串 / 解析类判据，纯解析单测不够，必须有一组真跑被解析方**：
  `git stash list --date=iso --format=%gd…` 里 **`--date` 会把 `%gd` 那截选择器一起改掉**
  （`stash@{0}` → `stash@{日期}`）⇒ 序号取不出来、清单永远空，而五条纯解析单测全绿、
  `cargo check` 零错、命令本身成功。**只有真 git 能证**（同 D73：末端拿不到数）
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
  - **2026-10-10 第 3、4 次确认，而且把"为什么不是渲染问题"钉住了**：前台确认是本应用
    （`GetForegroundWindow() == hwnd`、类名 `Zed::Window`），`SendInput` 两种编码各打一遍
    ——`KEYEVENTF_UNICODE`（出 `WM_CHAR`，gpui `events.rs:155` 确实处理它）与 VK 码
    （出 `WM_KEYDOWN`）——**与上一帧逐像素差都是 0**，而同一格里预填的暗色文字拍得到。
    ⇒ 不是"打进去但看不见"，是**按键进不来**
  - **要验一个输入框"能不能编辑"，走鼠标那条路**：gpui-component 的 `Input` 自带右键菜单
    （`input.rs:617-645` 剪切/复制/粘贴/全选，**粘贴项的禁用条件就是 `!editable`**）。
    实测：右键会开一个真的 `#32768` 顶层菜单（**它是独立窗口，要按矩形去点第几项**，
    不能按帧坐标点），点「粘贴」之后框里出现剪贴板内容 + 焦点环 + 光标。
    **双击选词也生效**（选中高亮 3185px）——这两条都不需要键盘
- **要验「拉取成功」这类结果，合成仓库必须配一个真远端**。
  `mkfixture16.py` 那三个仓库**都没有远端**（纯离线看界面），拉取一定失败——
  拿它验 `op_history` 等于验了个寂寞。做法：本地 **bare 仓库**当远端
  （`git init --bare` + `file://` 路径，不联网、不碰用户任何远端），
  ⚠️ **且必须在工作副本克隆完「之后」再往远端推一笔**——克隆时就把远端那几条
  一起拿走了，ahead/behind 都是 0，点拉取是"没有新提交可拉取"，**那是空跑**。
  要验失败那一档就再造一个「远端推一笔 + 本地也提交一笔」的分叉副本
- **验「托盘里那枚图标」别用截图差分，连栽了三次，每次错法都不同**（2026-10-05）：
  ① 截**整条任务栏**求差 → 应用一启动任务栏中间就多出一个窗口按钮，
  差出 **525px 宽**一长条，把那枚 16px 小图标淹在里面
  ② 只截**通知区**求差 → 收窄对了，差出来的是**输入法「中/英」在闪的 15×15**
  ③ 点开折叠面板肉眼看 → 一屏 8 个陌生图标，**认错一个就得出了"图标没出来"**
  ⇒ **根因是托盘图标默认在「隐藏的图标」折叠面板里**，通知区那一截启动前后
  **完全一样**；而差分法**分不清"新增"与"闪了一下"**。
  **机器判据是 `Shell_NotifyIconGetRect`**：`tray-icon` 用的是无 GUID 的老式身份
  （`NOTIFYICONDATAW` 只填 `hWnd` + `uID`），而 `uID` 是**进程内自增计数器**
  （`platform_impl/windows/mod.rs:97`），扫 0–7 就能撞上，成功即返回它的屏幕矩形。
  面板按 `TopLevelWindowForOverflowXamlIsland` 这个顶层窗口的矩形抓——
  **别按固定坐标裁**，面板高度随图标数量变，裁到的会是桌面壁纸，看着像"没开"。
  同理 `FindWindowW("TrayNotifyWnd")` **返回空**：它是 `Shell_TrayWnd` 的
  **子窗口**，得用 `FindWindowExW(parent, ..)`。工具：`scripts/tray_icon_query.py`
  / `flyout_shot.py` / `explorer_icon_check.py`（后者验桌面——"PE 里存了什么"与
  "Windows 会画什么"之间还隔着**系统图标缓存**，两头都得过）
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
  ⚠️ **这一族的根到阶段 31.4 补十五才治上**：`RepoStatus::default()`（**还没问过 git**）
  与「问过 git、结论是这个文件夹不是仓库」**是同一个值**，所以"我不知道"在结构体里
  **没有一格可以承载**，界面只能从两句假话里挑一句说（同步卡「未识别为 Git 仓库」、
  头卡「该仓库还没有配置远端」）。⇒ 那一格叫 `RepoView::has_status`（库里是
  `RepoCache::status_read`），判据收成 `Shell::status_read(path)`，**同步卡与头卡两处都读它**。
  两个坑：① **别拿"有没有这条缓存"当判据**——只写体积那一路也会先建一条 `status` 全空的记录；
  ② **别拿 `status` 的内容猜**（那就是缺陷本身）。加这种字段的好处当场兑现了一次：
  `EMPTY_VIEW` 那处**直接编不过**（E0063），这就是 D78「让『忘』没有选项」的形式
- ⛔ **`alive` 不是"做到了"，坐标写死在脚本里就会过期**（2026-10-07 实锤，路线表 **D93**）。
  给设置页做 a11y 走查时，16 步全 `alive`、stderr 0 字节，**而证据帧显示对话框根本没打开**：
  ① `scripts/a11y_stress.py` 自己的 `click()` 内部是 `for _ in range(2): down/up`——**那是双击**，
  "打开一个浮层"的键被双击 = 开完立刻被遮罩那一下关掉；② 重校准后又发现 10 步里
  `NAV_MINE=(28,105)` **正好落在两颗按钮之间那 12px 的气口里**、`TAB_Y=150` 是页签还在
  卡外时的位置——**v4 改版之后没有一个坐标复核过**。
  ⇒ 三条规矩：**点击复用 `steps.click`**（单击 + 前台确认），
  **每一步抓一帧并与上一步求 diff**（零变化要点名，见那份脚本的新 `step()`），
  **改版之后脚本里的坐标要重量一遍**（`steps.py` 头部那份 `68/105/142` 已按
  `71/113/155` 更正）。
  ⚠️ 判据的差别就是这条的全部价值：`alive` 的语义是**没崩**，
  而"进我的仓库 / 五个页签 / 展开右栏"那 10 步**从来没生效过**，
  在 `alive` 这一列里全是**通过**。⇒ **凡是只判"有没有报错"的自动化走查，
  都要问一句"它怎么知道那一步真的发生了"**
- **`FormRows` 的控件列是 `h_flex + flex_1 + min_w_0 + justify_end`，两半都有人踩**
  （阶段 31.5）。`justify_end` 让开关、单选组、按钮这类**按内容宽**的控件贴到行末
  （不贴就是"标签右边一小坨、右侧全空"，用户说的"没排完"就是这个）；
  ⇒ 反过来**想占满整列的控件必须自己写 `w_full()`**——不给的话里面任何 `flex_1`
  塌成 0 宽，抓帧里「手动地址」那一格只剩一个椭圆。
  ⚠️ 也别把这一列写成 `div()`：裸 `Div` 是列方向，`justify_end` 会往**下**推，
  看着像"完全没生效"
- **目测出来的「不齐」先量再改**：`scripts/rowpitch.py` 在指定横向范围内做**逐行
  像素剖面**，切出每段凸起的中心再算相邻行距。空间页树那一轮我目测以为行距有
  27/28/31 三种、打算去调，实测**统一 28.0**（开头那个 28.5 是首行被滚动视口裁掉
  造成的测量偏差）。**"看着不齐"常常是量法不对**——没量就改，等于拿目测去改一个
  本来就对的东西。量法：背景色取**区域内**的众数（取四角会取到卡片外的画布色，
  那样整片卡都被判成"有内容"，剖面连成一段）
  - ⚠️ **但"逐行剖面"量不了一个盒子有多高**：它给的是"与众数不同"的**凸起段**，
    而两个叠着的 `sunken_box` 之间那 12px 气口里夹着**上一个盒的下边框**（1px `track`），
    于是段起点会提前十几像素。2026-10-08 量「这次带回」那一行时虚高到 48px，
    逐行认底色（`card-sunken` 那一档）实测是 **34px**——**画面没变，量法变了 14px**。
    ⇒ 量"有内容"用剖面，量"这一块是什么、到哪为止"要**认色不认凸起**
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
| `changes.rs` `widgets.rs` | 变更清单 / 跨屏小部件。⚠️ **这一行以前还列着 `sync_ring.rs`（"同步环，自研三处之一"）——那颗文件从来没有存在过**：`git log --diff-filter=D` 查不到删除记录，环是画在 `panels::sync_card` 里的，而它 2026-10-05 已按用户口径「占地方还难看」下线（`panels.rs` 里那句「没有圆环」才是现状）。**文件地图里写一个不存在的文件，比少写一个更坏**——它会让人去找、找不到之后再"重新实现"一遍 |
| `ops.rs` | 后台编排：跑命令、三键、扫描、**批量同步**、提交并推送、同步设置 |
| `scan_cache.rs` | **阶段 16 的界面侧接线**：读缓存填 `views`/`space`/`profile`、扫完写回缓存。⚠️ 三个后台扫描一律**先灌缓存再刷**；判据是 `scanning && 还没有内容`，写成光看 `scanning` 会把刚上屏的缓存顶掉 |
| `tray.rs` | **托盘图标 + 三项菜单 + 左键唤起 / 窗口没了就重开**（Q11）。窗口选项也从这里出，首次开窗与托盘重开共用同一份 |
| `scheduler.rs` | **进程级定时器**（Q8）。`App::spawn` 不绑窗口，窗口关着也照跑；每轮 `try_begin_op` 与用户操作互斥 |
| `dialogs/` | `repo` `init` `fork` `clone` `cmds` `commit` `settings` `batch_log` 八个官方 `Dialog` |
| `app_assets.rs` | 资产源 = `AllAssets` + 一张应用位图 `gui/assets/app-icon.png`。**改图标只改这一张**——托盘与桌面那两份由 `gui/build.rs` 从它派生（见上面「一份源图派三份」） |
| `build.rs`（`gui/`） | 把那一张源图派生成托盘 RGBA（32×32）+ 七档 `.ico`，并用 `embed-resource` 嵌进 exe。**换图标不需要改它**；改的是"要几档、什么尺寸、怎么嵌" |

  ⚠️ **`assets/icon-source.png`（1024 那张）没有任何代码读它**——`app_assets.rs`
  与 `build.rs` 都只读 `gui/assets/app-icon.png`（128），`gen-icons.mjs` 退役后
  已无展开脚本。它是**纯存档**。**照着它重画并存回 1024 那张，重新构建什么都不会变。**
  重设计的正确落法：直接替换 `gui/assets/app-icon.png`（**必须 128×128**，
  `build.rs` 与那条单测都钉着这个尺寸），顺带把 1024 那张当草稿更新或直接删掉。

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
