"""31.4 补二十五那条回归判据：**抽屉开与关，左栏那一列不许被抢宽**。

判据全部落在帧上量的数（窗口客户区宽、左栏白面宽、再加两个**逐项地标**），
不是"跑起来没报错"。

五跑八腿五十一判据（每跑各写一份 config 再启动，**状态走配置**；八腿走鼠标）：

  O **逐项地标**（32-26 那一档，2026-10-12；每一跑两条 ⇒ 共 10 条）：
                        撑宽算式进判据的一直是**项之和**（`41 + 8 + 侧栏 + 950`），
                        于是"把和拆成两项、两项各自被改"这一类错**结构上判不出**
                        （32-25 的 M3 实测：`NAV_W` 40→48 同时 `GAP` 8→0，
                        固定项仍 49、A 仍 1879，一条都不红）。
                        ⇒ 这一档把**每一项钉在帧上属于它自己的一格**：
                          · 左 = **侧栏那张卡的白面起点** 应 == `NAV_W` + 那条 1px 分界
                            （现读：40.0 + 1.0 ⇒ 41，五跑实测全是 **41**）
                          · 右 = **窗口右缘留白** 应 == `GAP`
                            （落点 `shell.rs:538` 唯一那一处 `.pr(px(GAP))`；
                             实测 = 客户宽 − 白面收尾 − 1，五跑全是 **8**）
                        ⚠️ 那两个"减 1 / 加 1"都不是凑出来的：先按白面阈值量给的是 **9**
                          （白面比外壳内缩一格——卡自己的 1px 右边框不是白的），
                          按"与画布色差"量给的是 **7**（停在**阴影外缘**，阴影与画布只差 9）。
                          ⇒ 把那一行原始色逐格打出来才看清 `1869 白 | 1870 边框 | 1871-78 阴影`。
                          ⛔ **第一版是拿容差 1 把 9 判过去的**——"稳定偏 1"被当抖动吞掉之后，
                          这条判据的分辨率永久少 1px 而登记写的是"各自钉住"。
                          ⇒ **期望与实测差 1 也要先问那一格像素是谁的**，说得出归属才写容差。
                        ⚠️ 这一档**判不出**的那一类要说破：两个期望值都从 `tokens.rs` 现读，
                          而界面也照 `tokens.rs` 画 ⇒ **两侧同源**，改值型错（含上面那个补偿性错）
                          任何像素判据都判不出。它负责的是**布局与算式解耦**那一类
                          （和不变、格会变）——实测见文件末尾的变异账 G1/G2/G3，
                          G4 就是"补偿那一档仍然不红"的实证。
                        ⚠️ 八腿的地标**只打印不判**：同态重复四次的"地标漂"由那两条极差
                          与过程抖动那一条负责，再加 16 条不多覆盖任何一类。
  A 抽屉开            → 记下窗口宽 wA、左栏白面 sA
  B 抽屉关            → **sB 必须等于 sA**（这条就是补二十五的原话）
  C 抽屉开（再跑一次）→ wC == wA 且 sC == sA（"再撑一次要撑到同一个数"，
                        不幂等的症状是窗口越开越宽）
  D 抽屉开 + 侧栏 +40 → 窗口长的部分**必须全部进了侧栏**：
                        `(wD − wA) − (sD − sA) ≈ 0`
                        这一档挡的就是那一轮修掉的病——拿中间那一列去垫侧栏。
                        ⚠️ 判据写成**两个增量的差**，不写成"中间列 == 950"：
                        后者要把轨宽、右缘宽这些壳层常数抄进脚本，
                        它们一改判据就悄悄失效（本项目为写死的数交过三次学费）
  E 抽屉关 + 侧栏 +40 → 与 D 只差抽屉那一列：`wD − wE ≈ inspector_w`（**读配置**，
                        2026-10-11 实测那一列是 560，不是从前抄的 320），
                        证"撑宽"这件事真的由抽屉参与，而不是窗口一直那么宽

  F–M 在 A 那一跑里**当场连着开合四轮**（收起 / 展开 × 4 = 八腿）
                        → 不另起进程，每腿都量点完的窗口宽与左栏白面。
                        为什么要补这些腿：A–E 的状态全由配置给，**进程起来就是
                        那一态**，容器宽度从头到尾没变过；而补二十五那条病
                        （每一格都留着宽度偏好时基座按百分比重分整组）
                        **只在容器变化那一下发生**——点一次开合 = 外层容器少了
                        `inspector_w` 那一列、窗口又跟着还宽一次。
                        ⇒ 五跑一条都判不到它。2026-10-12 的变异实测就是证据：
                        把 `keep_main_flexible` 里"每帧把中间那一格交回 flex"
                        改成恒不执行，**十二判据一条都不红**。
                        ⚠️ **为什么是八腿不是两腿**（同一天，路线表 32-24）：
                        白面宽在跑与跑之间自然抖 1px，而容差就是 1px ⇒ 两腿时
                        "保护关掉以后有没有每次漂一点"**根本判不出来**（V1 那一轮
                        红过一条又绿回来，就是这个分辨率下限咬人）。八腿把同一种
                        状态重复四次，于是补两条与"A"和"算式"都**无关**的判据：
                        **四次收起互相一致 + 四次展开互相一致**（极差）。
                        逐步漂的症状在单腿对 A 的判据里会淹在抖动带里，
                        在极差里不会——它要的是"这一态自己四次是不是同一个数"。
                        每腿三条判据（前提「config 那一格翻了」+ 左栏白面 == A +
                        宽 == 该态算式），八腿共 24 条，再加 2 条极差 +
                        1 条「末腿的宽 == A 的宽」（**不经过算式**，专抓漂移）+
                        1 条前提「八腿点完 config 的两列宽度没被应用改写」。
                        坐标按 a11y 名字现场量（那颗键带 `accessibility_label`），
                        生效判据读沙箱 config，**都不靠像素**

  N **过程抖动**（第八腿之后新加的那一档，2026-10-12，路线表 32-24）：
                        每腿点完之后**不等稳**，先连抓十帧逐帧量左栏白面，
                        判"过程中最坏那一帧偏离落定值多少"（容差 `SHAKE_MAX` 10px）。
                        为什么要有这一档——**上面四十条判据把这一类缺陷整条筛掉了**：
                        插桩打印 `ResizableState::sizes()` 跑同一套八腿两遍
                        （甲 = 保护在、乙 = 把 `keep_main_flexible` 改成恒不执行）量到的：
                          · 甲：侧栏那一格在启动动画之后**一次都不再变**
                            （九行全在启动段，八腿期间**零次**）；
                          · 乙：**每开合一次整组被按比例甩一遍**，侧栏从落定 318.7
                            **掉到 222.4**、**冲到 415.5**（负 96 与正 97 像素），
                            **然后再回到 318.7**；
                          · 两跑的**落定值只差 0.5 像素**。
                        ⇒ 所以那条保护防的不是"落定宽度算错"，是**过程里整组抖一下**，
                        而"等稳之后才量"的判据（含刚加的极差）**结构上看不到它**——
                        这就是变异 V1 四十条一条都不红的完整解释：
                        **坏的从来不是判据的严格度，是采样时机。**
                        实测（2026-10-12，干净版）：八腿的过程最坏偏离**全 0px**
                        ⇒ 这一档不是"给抖动留余量的软判据"，保护在的时候它是零

与上一版的三处不同（2026-10-11 按路线表 32-15 的尾巴重写）：
  ① **夹具自己造**（%TEMP 下一颗合成仓库 + 沙箱 config），不再吃 10-08 那轮
     留在 `%TEMP%\\qb_v4` 里的旧沙箱——上一版红就红在它读到的是别人的状态；
  ② **删掉拖把手那一步**：`sidebar_w` 是配置项，直接写进 config 就是"拖完之后"
     那一态，比模拟拖拽稳（拖拽要猜把手坐标，而把手位置随窗口漂）；
  ③ **收起态那一跑不必点键**：B / E 走 `inspector_open=false` 的配置就是收起态。
     但**会话中开合**那一档不在这条配置路径上，由 F–M 八腿点那颗键负责
     ⇒ 全程九次点击：导航轨「我的仓库」（y 现场扫）+ 八腿各一下，
     后八下的坐标按 a11y 名字现场量（D93 那条：写死的坐标改版之后全过期过）。

⚠️ **这次重写换来的稳，代价要说清**：它验的是**宽度规则**（谁要更多宽、由哪一列垫）。
2026-10-12 之前这一段写的是"点收起键那条路径不由这条脚本负责"——**那句已经不准了**：
F–M 八腿走的就是那颗键（点下去 → 量 → 点回来 → 量，重复四轮）。
现在仍然不负责的是**「拖把手」**：把手坐标随窗口宽漂（D93 那条），模拟拖拽红的
时候分不清是规则坏了还是坐标坏了，所以它归侧栏 / 抽屉的单测 + 他双击实机。
**别把它当"拖把手也回归过了"**。

## 变异账（2026-10-12，驱动 `target/mutate_grow2.py` / `mutate_grow3.py` /
## `mutate_grow4.py` / `mutate_grow5.py` / `probe_kmf.py` + `probe_split_sizes.py` /
## `mutate_3225.py` / `mutate_3225b.py`，全是一次性脚本、不入仓库）

三条变异 + 一次插桩对照 + 改版后重做的四条（P0 / M1 / M2 / M3，见下面 32-25 那一节），
各自单变量、跑完按 sha 逐字节还原：

  · **V2 实现侧 `MAIN_MIN_W` 950→100** ⇒ 红 8 条（算式四条 + 前提"只差抽屉那一列" +
    D 那两条 + D/E 差值那条）。**承重成立**。
  · **V3 只改脚本里那颗抄来的常数 950→100，二进制保持正确** ⇒ 红 5 条，
    **正好是"宽 == 算式给的数"那五条**（A / B / D / E / 当时那条叫 F 的腿），一条不多一条不少。
    这条证的是头注释答应过的那件事：四个壳层常数改版会红**并点名是哪一条**；
    没有它，V2 的红就分不清是"实现坏了"还是"算式本来不敏感"。
    ⚠️ V3 第一轮跑错了：驱动的还原**只在 `need_build` 分支里重编**，而 V3 标的是
    `False` ⇒ 二进制里还躺着 V2 的 100，两个变量一起动、两侧口径一致地错，
    那条结果作废（第二轮把"每条之前无条件还原 + 重编 + 回读确认变异串在盘上"写死）。
    ⛔ **V3 从 2026-10-12 那一版起已经没有对象了**——脚本里不再抄任何壳层常数（见上面
    32-25 那一节），它换来的性质改成"**改版会拒绝跑**"。别把它当成"这条判据还活着"。
  · **32-25 那一版（常数改成现读）的四条账**，驱动 `target/mutate_3225.py`（前三条）
    与 `target/mutate_3225b.py`（后三条，第二版把 M3 换成算式层面的自证）：
      · **P0 应当放行** ⇒ 整条脚本全跑 **四十一条全绿**，而头行打出
        「现读到的项：NAV_W + 1.0 + GAP + MAIN_MIN_W ⇒ 固定项 49.0」
        ⇒ **改成现读没有把判据改掉**（固定项之和仍等于从前抄的 41 + 8 = 49、
        A 仍算出 1879）——这是"重构不等于修复"那句话的实测版。
      · **M1 结构变了必须拒绝跑**（真函数里 `+ GAP` 改成 `- GAP`，二进制不动）
        ⇒ 退出码 1、**没有判读那一行**、报"不再是纯加法" ✓
        ⇒ 顶替 V3 的那条：改版不再靠"数过期来红"，靠"**不肯猜**"。
      · **M2 改名必须拒绝跑**（`tokens.rs` 的 `MAIN_MIN_W` 改名）⇒ 报"读不到常量 MAIN_MIN_W" ✓
      · **M3【预期不红，而且是有意登记的那种不红】**：`--parse-only` 那行把
        NAV_W 与 GAP 同时换成 48 与 0（**和不变**），现读算出的固定项**仍是 49**、
        两条 target 一个字不改 ⇒ **补偿性错误这一档没有判据负责**，
        要判得逐项量导航轨宽与列间距 ⇒ 登记成路线表 **32-26**。
        ⚠️ 这条最初想用**应用侧**变异证（真把 48/0 编进 exe 再跑八腿）——那条**作废**：
        合成值下应用窗口没起来（`AssertionError: A-open：窗口没出来`），
        于是它既不是绿也不是红，是**没跑**（AGENTS 那条三态判读第三次兑现）。
        ⚠️ 而第二版驱动又差点把这条**成立的**自证记成"没跑"：它的状态枚举只认
        "跑出判读"与"报拒绝跑"两种，而 `--parse-only` 天生不打『判读』⇒
        **状态表漏一档时，成立的证据长得像无效**，这条也留在账上。
        ⚠️ 自证那一行第一版还算错过一次数（把 `MAIN_MIN_W` 也加进"固定项之和"⇒ 打出 999），
        靠它自己那句"和必须仍等于 `SHELL_FIXED`"当场判红才露出来——
        **自证那条也要有判据，不然它只是一句印出来的话**
  · **V1 把 `shell.rs::keep_main_flexible` 的每帧交回改成恒不执行 ⇒ 前四跑都不红，
    第五跑精确红一条**。这一条查了五轮，每一步都值得记，因为**错的不是判据的严格度，
    是采样时机**：
      ① 十二条判据那版：不红。
      ② 十七条（A–G，含 F / G 两腿）：红过一条「B 与 A 左栏同宽」，单独重跑又全绿，
         而两跑白面只是 **±1 抖动带**里互换（基线 A 313 / B 312，V1 跑 A 312 / B 313）
         ⇒ 归因不了，登记成 32-24，**没有**写成"保护没用"。
      ③ 八腿四十条（这一版）：`target/grow_V1_8leg.log` —— **四十条一条都不红**，
         收起四次宽 [1319×4] 白面 [312×4]、展开四次宽 [1879×4] 白面 [312×4]，
         与基线的差只在 313 / 312 那一格。⇒ **极差判据也救不了它**，因为极差量的是
         **落定值**，而保护改的**不是落定值**。
      ④ 换尺子：临时插桩打印 `ResizableState::sizes()` 的浮点数，甲（保护在）/
         乙（保护关）各跑同一套八腿（`target/probe_kmf.py` + `probe_split_sizes.py`）。
         甲的内层树九行全在启动段、**八腿期间零次变化**；乙的内层树**七十三个区间**，
         每开合一次侧栏被按比例从落定 318.7 **甩到 222.4** 或 **415.5**（负 96 / 正 97 像素），
         再回到 318.7。两跑**落定值只差 0.5px**。⇒ 保护防的是**过程里整组抖一下**。
         ⚠️ 这一跑附带两条纪律：插桩自己要先证不改变行为（甲的八腿数与不插桩基线
         **逐条相同**），以及驱动**编译不过必须当场停**——第一版只断言"看到 Compiling 行"，
         而那次 `HashMap::new` 不是 const fn、**退出码 101 但确实"跑过"**，
         于是后面全打在旧产物上，"一条 KMF 都没有"长得就像"两边都不动"。
      ⑤ 按 ④ 补上「**过程抖动**」那一档（点完不等稳、连抓十帧取最坏偏离）后重跑
         （`target/mutate_grow5.py`，两跑连做）：**干净版四十一条全绿、过程最坏 0px**
         （八腿每一腿都是 0 ⇒ 这条不是"给抖动留余量的软判据"），
         **V1 精确红一条**、其余四十条继续绿：
           「开合那一下左栏不许整组被甩（过程最坏偏离落定值 ≤ 10.0px）」
           各腿抖幅 86 / 0 / 97 / 0 / 84 / 0 / 97 / 41 px（收起那几腿甩满，展开腿有两条
           抓空 ⇒ "抓不到"只说明**这十帧没赶上**，不说明没发生，别拿 0 那几腿当反证）。
      ⇒ 三条一起留在这一档上：
        **保护承重成立**（关掉它有一张判据会红，而且只红那一张）；
        **"等稳之后才量"是一种系统性筛除**，判一个瞬态效应要在过程中连抓若干帧；
        ⛔ 仍然**不许**因为"前四跑不红"就把 `keep_main_flexible` 删掉——
        那四跑的不红是**判据看不见**，不是**它不存在**（这一轮的教训就写在这儿）。
      ⚠️ 容差 10px 的两个依据：保护在的时候实测 **0px**，保护关的时候 **84–97px**；
        留 10 而不是 0，是因为**侧栏自己也在做补间**，十帧里可能抓到补间的中间值
        （那一档量到过 236 → 349）——把容差写成 0 就是把抖动判成缺陷。
    ⚠️ 顺带把**分辨率下限**那条一起留着：白面宽在跑与跑之间自然抖 **1px**，而落定值那几条
       容差就是 1px ⇒ 对 1px 级漂移没有区分度，只抓得住 ≥2px 的。
       新增判据时别把容差调到 0，那不是"更严"，那是把抖动判成缺陷。

八腿那一份的**承重**分两处：逐腿的「宽 == 该态算式」由 V3 证过（改名之前那条叫
「F 的宽 == 收起态算式给的数」，V3 跑的就是那个名字），
而**「开合那一下会不会把整组甩一把」这一档由上面 V1 的第 ⑤ 步证过**——
它现在是有牙的判据（V1 精确红那一条、其余四十条绿），不再是"没人负责"。

逐项地标那十条的**变异账**（2026-10-12，路线表 32-26，驱动 `target/mutate_3226.py`）：
  ⚠️ 这四跑都带 `GROW_CYCLES=0`（只跑五个落定态、共 22 条），因为八腿那一档由上面 V1
  负责、这一档不碰它 ⇒ **别把这几跑读成"八腿也重验过了"**。
  · **G1 布局删掉右缘那处 `.pr(px(GAP))`（算式照旧加 `GAP`）**
    ⇒ 精确红 5 条「窗口右缘留白 == 算式里的 GAP 8.0（帧上量到 0）」，其余 17 条全绿。
  · **G2 布局把导航轨收窄 8（`.w(px(NAV_W))` 改成 `NAV_W - 8.0`，算式照旧）**
    ⇒ 精确红 5 条「侧栏卡左缘 …（帧上量到 33）」，而右地标**仍是 8**
    ⇒ 两条地标**分得开是谁的锅**，这是"拆成项"换来的全部东西。
  · **G3 算式把那条边框从 `+ 1.0` 改成 `+ 9.0`（布局一个字没动）**
    ⇒ 精确红 5 条「侧栏卡左缘 == 49.0（帧上量到 41）」，而「宽 == 算式」那几条**继续绿**
    ——窗口宽由同一颗函数算 ⇒ 两侧同源那条老规律在这里反过来成了**证据**：
    **和判不出、格判得出**。
  · **G4 补偿性错（`NAV_W` 40→48 同时 `GAP` 8→0，和不变）【预期不红，有意登记】**：
    应用侧那一跑**连着两次没跑成**（这回是 `A-open：拿不到前台`，32-25 那回是"窗口没起来"）
    ⇒ 记**没跑**、不记绿（三态判读第四次兑现）。`GAP` 是全站间距档，置 0 把界面挤到
    窗口激活不了 ⇒ **这一档的应用侧取证结构上做不了**，不是没试。
    换一把不需要窗口的尺子证它的命题（`target/g4_parse.py`）：把变异后的 `tokens.rs`
    留给 `--parse-only`，看的是**两个地标的期望会不会跟着实现一起漂**。实测：
      现读到的项 `NAV_W=48.0 + 1.0 + GAP=0.0 + MAIN_MIN_W=950.0`、固定项**仍 49.0**、
      两条 target 仍 开 1879／关 1319，而「左（侧栏卡左缘）应 == **49.0**（48 + 边框 1）」、
      「右（窗口右缘留白）应 == **0.0**（GAP）」⇒ **期望与实测一起动**
      ⇒ 补偿性错在像素判据上**结构上不可判**（不是"还没做到"，也**不是**"地标判据无效"——
      它负责的是 G1/G2/G3 那一类：**布局与算式解耦**）。
    ⚠️ 这条判据第一版写错了两处，都留在账上：
      ① 它去断言**未变异时**那串"左 41.0→49.0"，而真变异后基线自己就是 49.0 ⇒
        **断言拿的是改动前的数**，成立的证据被判成失败（与"状态表漏一档"同族）；
      ② `finally` 里写了一句 `return`，它把 `try` 的返回值整个**吞掉**——
        三个布尔里有两个 False、驱动却报"退出码 0 = 成立"。
        ⇒ 还原的核对**只打印、不改判据**；判据的成败只在 `try` 里定一次。
      ③ 反过来也记一笔：`--parse-only` 自己那条"覆盖后 == 现值 + 8"的自证在**变异态**下
        必然不成立（它报 1），这种 1 是预期的 ⇒ **别把"退出码非 0"一律读成"这一条不成立"**。

用法：`python scripts/verify_grow.py`
"""
import ctypes
import datetime
import io
import json
import os
import re
import shutil
import subprocess
import sys
import time

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
import qbguard, shoot, steps  # noqa: E402
import a11y_stress as A  # noqa: E402  只用它的 a11y dump / 按名字取矩形 / 客户原点

from PIL import Image  # noqa: E402

u = shoot.user32
wt = shoot.wt
TEMP = os.environ["TEMP"]
FIX_ROOT = os.path.join(TEMP, "qb_grow_ui")
REPO = os.path.join(FIX_ROOT, "repo")
SANDBOX = os.path.join(TEMP, "qb_grow_app")
CFG = os.path.join(SANDBOX, "com.qingbird.git", "config.json")
EXE = os.path.join(ROOT, "target", "debug", "qingbird-git.exe")
DRAG_DELTA = 40.0   # "拖了 40" 的等价物：sidebar_w 直接 +40
# 会话中开合几轮（32-24：两腿时"每次漂一点"淹在 1px 抖动带里）。
# ⚠️ 可以经 `GROW_CYCLES` 覆盖：32-26 那四条变异要判的只是**五个落定态的逐项地标**，
#   八腿那一档上一轮（V1）已经变异验过了，每轮多花 5 分钟只为了重证一条没动的东西。
#   置 0 就整个跳过腿与它那六条判据（**不是把判据删了**，是这一跑不去碰它——
#   默认值仍是 4，日常与出口跑的都是八腿）
CYCLES = int(os.environ.get("GROW_CYCLES", "4"))
# 过程抖动允许多少像素。取数的两个依据（2026-10-12 插桩实测）：
#   · 关掉那层保护 ⇒ 侧栏 `sizes` 每轮被甩到 222.4 / 415.5，相对落定 318.7
#     是 **−96 / +97px** ⇒ 上限离这一档要留出量级差，否则这条判据抓不到缺陷
#   · 开着保护 ⇒ 启动动画之后 `sizes` 一次都不再变 ⇒ 理论上应当是 0，
#     但 `PrintWindow` 抓的是**任意一帧**，可能正好落在弹簧的中间帧上
#     （侧栏自己也在做补间，那一档量到过 236 → 349）⇒ 不许写 0
SHAKE_MAX = 10.0

# 两列的宽度**都从配置里读**，脚本里一个都不钉（2026-10-11 的更正）：
# 第一版把侧栏写成 263、注释还写着"与真实那份一致"——真实那份是 **360**，
# 那句是假话；而抽屉那一列我照旧抄了个 320，实测是 **560**，两条判据当场红。
# ⇒ 两列同源同法，别再"一个读配置、一个抄常数"（同一件事两个出处就是两个会漂的数）
BASE: dict = {}

# ── 壳层常数：**一个都不抄，全部从代码现读**（2026-10-12，路线表 32-25）
#
# 从前这里写着四个数（41 / 8 / 950 / 1280），而判据把它们**加在一起**用
# ⇒ "41 改成 49 同时 8 改成 0"那种互相补偿的错**一条判据都判不出来**（和不变）。
# ⚠️ 当时还在这一档下面写过一句"`EDGE_W` 那名字（主区右缘气口）是假话，那个 8 是**列间距**"——
# **那句是第二句假话**（2026-10-12 被 32-26 的量测推翻，见 `split_landmarks` 的头注）：
# `GAP` 那一档在算式里**就是窗口右缘留白**（真源那张表明写，帧上地标实测 8），
# 卡间那 6px 长在面板自己的 `flex_basis` 里、不在这条算式里。
# ⇒ 抄数改注释都只是治标：**把算式本身从代码里读出来**这一条照做，
# 但**名字要从布局的出处读，不能从"哪个数凑得上"倒推**（同一个动作我犯过两次、方向相反）
def src_of(*parts) -> str:
    path = os.path.join(ROOT, *parts)
    assert os.path.isfile(path), f"真源文件不见了：{path}"
    return io.open(path, encoding="utf-8-sig").read()


def read_tokens_const(name: str) -> float:
    m = re.search(r"const %s\s*:\s*f32\s*=\s*([\d.]+)\s*;" % re.escape(name),
                  src_of("gui", "src", "tokens.rs"))
    if not m:
        raise SystemExit(
            f"gui/src/tokens.rs 里读不到常量 {name} ⇒ 那一档改名、搬走或不再是 f32 字面量，"
            "而这条脚本的算式跟着变了。**不许猜一个数继续跑**，去看它")
    return float(m.group(1))


def read_shell_terms():
    """解析 `required_shell_w` 里那句"固定项之和"，返回(除 MAIN_MIN_W 之外的和, MAIN_MIN_W, 项列表, 每一项的值)。

    ⛔ **只认纯加法**：出现减法、乘除、括号或函数调用就 `SystemExit`。
    这一条是整个改动的牙齿——它把"改版会红并点名"换成了"**改版会拒绝跑并点名**"，
    前者靠脚本里那颗抄来的数过期（V3 证过），后者靠形状本身（新变异 32-25-M1/M2 证）
    """
    body = re.search(r"pub\(crate\) fn required_shell_w[^{]*\{\s*let mut w\s*=\s*([^;]+);",
                     src_of("gui", "src", "shell.rs"))
    if not body:
        raise SystemExit("shell.rs::required_shell_w 里那句 `let mut w = ...;` 读不到 "
                         "⇒ 那颗函数改名了、或者不再用累加写法。这条脚本的前提没了，先去看它")
    expr = body.group(1).strip()
    for bad in ("-", "*", "/", "("):
        if bad in expr:
            raise SystemExit(f"required_shell_w 的固定项不再是纯加法（读到的式子：{expr}）"
                             "⇒ 脚本不肯猜，去把这条判据改成与那颗函数同形")
    terms = [t.strip() for t in expr.split("+") if t.strip()]
    assert terms, f"required_shell_w 的式子拆不出任何项：{expr!r}"
    main_min, fixed, values = None, 0.0, {}
    for t in terms:
        v = float(t) if re.fullmatch(r"[\d.]+", t) else read_tokens_const(t)
        values[t] = v
        if t == "MAIN_MIN_W":
            main_min = v
        else:
            fixed += v
    if main_min is None:
        raise SystemExit(f"required_shell_w 的加法项里没有 MAIN_MIN_W（项列表：{terms}）"
                         "⇒ 中间那一列的下限换了名字或挪了位置，这条判据要跟着改")
    return fixed, main_min, terms, values


def read_initial_w() -> float:
    """窗口初始客户区宽：规则的另一半（"需求不到初始就不收"）用它，同样现读"""
    m = re.search(r"WindowBounds::centered\(\s*gpui_kit::size\(\s*gpui_kit::px\(([\d.]+)\)",
                  src_of("gui", "src", "tray.rs"))
    if not m:
        raise SystemExit("gui/src/tray.rs 里开窗那句初始宽读不到 ⇒ 规则的另一半搬家了，先看它")
    return float(m.group(1))


SHELL_FIXED, MAIN_MIN, SHELL_TERMS, SHELL_VALUES = read_shell_terms()
INITIAL_W = read_initial_w()


def split_landmarks():
    """把现读到的加法项分到**帧上两个各自独立的地标**（2026-10-12，路线表 32-26）。

    为什么要有这一档（32-25 的 M3 撞出来的）：上面那些判据量的是"宽 == 若干项之**和**"，
    于是把 `NAV_W` 调宽的同时把 `GAP` 调没，任何一条都不红。这一档把和**拆开**，
    让每一项在帧上占一格自己的像素地标：

      · 左地标 = 侧栏那张卡的**左缘**（客户 x）
                = 公式里除 `GAP` / `MAIN_MIN_W` 之外所有项之和 ⇒ 今天就是 `NAV_W` + 那条
                  `nav_rail` 的 `border_r_1`（实测 41 = 40 + 1，帧上白面起点逐条量到过）
      · 右地标 = 主区/抽屉最右白面到**客户区右缘**之间那段留白
                = 公式里的 `GAP` 项（落点是 `shell.rs:538` 唯一那一处 `.pr(px(GAP))`，
                  实测 8，抽屉开合两态同一个数）

    ⚠️ **`GAP` 这一项是"窗口右缘留白"，不是列间距**（2026-10-12 读真源读出来的更正，
    推翻我在 32-25 登记里写的那句「那个 8 是列间距 `GAP`」）：
    `shell.rs:2074` 那张表明写「窗口右缘留白 | `GAP`」，2078 行还专门警告
    「**列与列之间没有"再扣一条缝"这一项**」——帧上实测卡间是 6px（400 − 394），
    它长在面板自己的 `flex_basis` 里，压根不在这条算式里。
    ⇒ 也就是说 32-25 之前那个名字 `EDGE_W`（"主区右缘气口"）**语义是对的**，
    我当时把它判成假话、改口成"列间距"，**是照着另一个总量又编了一次名字**。
    教训同一条，触发方式不同：**名字要从布局的出处读，不能从"哪个数凑得上"倒推**。

    ⛔ 结构不对就**拒绝跑**（沿用 `read_shell_terms` 那一档的口径）：
    项集合必须恰好是 `NAV_W` / `GAP` / `MAIN_MIN_W` 加**一颗**数值字面量。
    多了少了都说明"固定项的含义变了"，那时这两个地标的对应关系要**重推**，
    不是让脚本硬凑一个数继续判。

    ⚠️ 这一档**判不出**什么也要说破：两项都由 `tokens.rs` 现读、界面也照 `tokens.rs` 画，
    ⇒ 同时把 `NAV_W` 改 48、`GAP` 改 0 时，**期望值与实测值一起动**，地标照样绿
    （32-25 的 M3"预期不红"就是这一档）。它负责的是**另一类**真实漂移：
    **改了布局而没改算式**（或反过来）——那种错和不变，帧上的格却变了，只有拆开才看得见。
    """
    lit = [t for t in SHELL_TERMS if re.fullmatch(r"[\d.]+", t)]
    names = {t for t in SHELL_TERMS if not re.fullmatch(r"[\d.]+", t)}
    if names != {"NAV_W", "GAP", "MAIN_MIN_W"}:
        raise SystemExit(f"required_shell_w 的常量项集合是 {sorted(names)}（要 "
                         "['GAP', 'MAIN_MIN_W', 'NAV_W']）⇒ 固定项的含义变了，"
                         "两个地标各自对应哪一项要重新推，脚本不肯猜")
    if len(lit) != 1:
        raise SystemExit(f"required_shell_w 里的数值字面量有 {len(lit)} 颗（{lit}，要恰好 1 颗 = "
                         "那条 `nav_rail` 右边框）⇒ 左地标那一格的构成变了，先去看布局")
    return SHELL_VALUES["NAV_W"] + float(lit[0]), SHELL_VALUES["GAP"], lit[0]


LAND_LEFT, LAND_RIGHT, RAIL_BORDER_LIT = split_landmarks()


def sum_with_overrides(overrides=None):
    """把现读到的项**按给定覆盖值**再加一遍（`--parse-only` 那条"盲区自证"用）。

    ⚠️ 与 `read_shell_terms` **同一口径**：`MAIN_MIN_W` 不进"固定项之和"，
    第一版我在这颗函数里把它一起加了 ⇒ 覆盖成 48 与 0 之后算出 **999**，
    而它自己那句"和必须仍是 SHELL_FIXED"当场判红（退出码 1）才露出来。
    **自证那条也要有判据，不然它只是一句印出来的话**
    """
    overrides = overrides or {}
    total = 0.0
    for t in SHELL_TERMS:
        if t == "MAIN_MIN_W":
            continue
        if t in overrides:
            total += overrides[t]
        elif t in SHELL_VALUES:
            total += SHELL_VALUES[t]
        else:
            total += float(t)
    return total


def read_range(name):
    """侧栏的合法夹取范围**从 core 现读**，不抄进脚本（抄一次就多一处会漂的真源）。

    ⚠️ 这一条不是洁癖，是本轮红出来的：他的 `sidebar_w` 正好是上限 **360**，
    而 D/E 两跑要"侧栏 +40"，脚本往配置里写 400 ⇒ 应用按 `clamp_sidebar_w`
    **夹回 360** ⇒ 那两跑与 A 是**同一个状态**，三条判据红而宽度规则一个字没错。
    **红的是前提，不是实现**（AGENTS「测试的前提不能外包给环境」的同族：
    这一回前提不是环境给的，是**我写进配置里的那个数被应用改掉了**）。
    ⇒ 落法：基线取"离上限还留着 delta 余量"的那个数，余量与上限都从 core 读。"""
    src = io.open(os.path.join(ROOT, "crates", "core", "src", "store.rs"),
                  encoding="utf-8-sig").read()
    m = re.search(r"pub const %s\s*:\s*\(f32, f32\)\s*=\s*\(\s*([\d.]+)\s*,\s*([\d.]+)\s*\)\s*;"
                  % re.escape(name), src)
    assert m, f"store.rs 里读不到 {name}——夹范围的形状变了，这条脚本的前提没了，先去看它"
    return float(m.group(1)), float(m.group(2))


def unlock(func, path, exc):
    os.chmod(path, 0o777)
    func(path)


def build_fixture():
    """一颗最小的合成仓库。⚠️ 不许拿用户那 17 颗当素材（AGENTS 明写）"""
    if os.path.isdir(FIX_ROOT):
        shutil.rmtree(FIX_ROOT, onerror=unlock)
    assert not os.path.exists(FIX_ROOT), "夹具没清干净，读到的可能是上一跑的旧状态"
    os.makedirs(FIX_ROOT)
    subprocess.run(["git", "init", "-q", "--initial-branch=main", REPO], check=True)
    for k, v in (("user.email", "t@t"), ("user.name", "t"), ("core.autocrlf", "false")):
        subprocess.run(["git", "-C", REPO, "config", k, v], check=True)
    with open(os.path.join(REPO, "base.txt"), "w", encoding="utf-8") as f:
        f.write("base\n")
    subprocess.run(["git", "-C", REPO, "add", "-A"], check=True)
    subprocess.run(["git", "-C", REPO, "commit", "-q", "-m", "base"], check=True)
    print(f"夹具就绪：{REPO}")


def build_sandbox():
    """settings 抄真实那份（主题、缩放、两列的宽度这些不许跟着变），repos 只留夹具这一条"""
    real = json.load(open(os.path.join(os.environ["APPDATA"], "com.qingbird.git",
                                       "config.json"), encoding="utf-8-sig"))
    if os.path.isdir(SANDBOX):
        shutil.rmtree(SANDBOX, onerror=unlock)
    os.makedirs(os.path.dirname(CFG))
    json.dump(real, open(CFG, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
    rs = real.get("settings", {})
    for key, fallback in (("sidebar_w", 263.0), ("inspector_w", 320.0)):
        raw = rs.get(key)
        # 那两个兜底值只在"配置里根本没有这一格"时用得上（老配置可能没有）。
        # **用了哪一个当场打印**，不许静默把抄的数当成他配的数
        BASE[key] = float(raw) if raw else fallback
        print(f"   从真实 config 读 {key}："
              f"{'读到 ' + format(float(raw), '.0f') if raw else '没有这一格，用兜底 ' + format(fallback, '.0f')}"
              f"（这两个数脚本里不钉）")


def write_case(inspector_open: bool, sidebar_w: float):
    cfg = json.load(open(CFG, encoding="utf-8"))
    cfg["settings"]["inspector_open"] = inspector_open
    cfg["settings"]["sidebar_collapsed"] = False
    cfg["settings"]["sidebar_w"] = sidebar_w
    cfg["selected_path"] = REPO
    # ⚠️ 沙箱那份是从真实 config **整份拷**来的（为了 settings 一致），
    # 所以这里必须把 repos 换成只有夹具这一颗——留着用户那 17 颗的话，
    # 量的时候侧栏画的是他的仓库列表，而这一屏本来就不该碰它们
    cfg["repos"] = [
        {
            "path": REPO, "name": "grow-fixture", "alias": "",
            "added_at": 1791600000, "last_used": 1791600000,
            "test_cmd": "", "build_cmd": "", "dev_cmd": "", "install_cmd": "",
            "pinned_cmds": [], "cmd_runs": {}, "auto_sync_mins": 0,
            "daily_time": "", "auto_pull": False, "last_auto_check": 0,
            "last_auto_result": "", "is_third_party": False,
        }
    ]
    json.dump(cfg, open(CFG, "w", encoding="utf-8"), ensure_ascii=False, indent=1)


def white_runs(png, y, x0, x1):
    """帧上 [x0,x1) 里连续的白面段（三通道都 >=248），返回 (起, 宽) 列表"""
    im = Image.open(png).convert("RGB")
    px = im.load()
    x1 = min(x1, im.size[0])
    out, start = [], None
    for x in range(x0, x1):
        c = px[x, y]
        white = c[0] >= 248 and c[1] >= 248 and c[2] >= 248
        if white and start is None:
            start = x
        elif not white and start is not None:
            if x - start >= 8:
                out.append((start, x - start))
            start = None
    if start is not None and x1 - start >= 8:
        out.append((start, x1 - start))
    return out


def win_w(hwnd):
    r = wt.RECT()
    u.GetClientRect(hwnd, ctypes.byref(r))
    return r.right - r.left


def park_cursor(hwnd):
    """把鼠标挪出窗口：hover 淡底会留在帧上，把"两帧应当一样"这条判据污染掉"""
    r = wt.RECT()
    u.GetWindowRect(hwnd, ctypes.byref(r))
    x = r.left - 60 if r.left - 60 > 0 else r.right + 60
    y = r.top - 60 if r.top - 60 > 0 else r.bottom + 60
    u.SetCursorPos(x, y)
    time.sleep(0.5)


def nav_bands(png):
    """导航轨上每颗图标的 y——**现场扫，不写死**（D93 那条）"""
    im = Image.open(png).convert("RGB")
    px = im.load()
    runs = []
    for y in range(30, im.size[1] - 90):
        n = sum(1 for x in range(8, 46) if sum(px[x, y][:3]) / 3 < 150)
        if n >= 3:
            if runs and y - runs[-1][1] <= 6:
                runs[-1][1] = y
            else:
                runs.append([y, y])
    return [(a + b) // 2 for a, b in runs]


def wait_stable(hwnd, prev, tag, rounds=10, budget=30):
    """抓到"画面不再变"为止，返回最后一帧路径。

    抽成一颗函数是给 A–E 五跑与 F/G 两腿**共用**的：两腿之后也要等窗口把
    还宽 / 再撑宽那两段跑完才能量，各写一遍就是两个会漂的数（本项目为
    "同一件事两个出处"交过四次学费）。
    """
    from PIL import ImageChops
    cur = prev
    for i in range(rounds):
        time.sleep(0.8)
        cur = os.path.join(FIX_ROOT, f"{tag}-{i + 1}.png")
        shoot.capture(hwnd, cur)
        d = ImageChops.difference(Image.open(prev).convert("RGB"),
                                  Image.open(cur).convert("RGB"))
        n = sum(1 for y in range(0, d.size[1], 3) for x in range(0, d.size[0], 3)
                if d.getpixel((x, y)) != (0, 0, 0))
        if n < budget:
            break
        prev = cur
    return cur


def measure(png):
    """一帧量两个数：窗口客户区宽与左栏白面宽。A–E 与 F/G 同一颗函数。"""
    runs = white_runs(png, 300, 40, 1200)
    return runs[0][1] if runs else 0, (runs[0][0] if runs else "-")


def land_marks(png, offx, cw):
    """一帧量**两个逐项地标**（客户坐标）：(侧栏白面起点, 最右那格外壳到客户右缘的留白)。

    与 `measure` 的分别要说清：`measure` 量的是白面**宽**（一个差值），
    而这里量的是白面的**位置**——一个绝对起点与一段绝对留白。
    ⇒ 它能判出 `measure` 结构上判不出的那一类：整组往右挪 8px、白面宽不变。

    两个地标的取法都是**这一帧上逐行打色量出来的**（A 帧 y=300 那一条，客户 x 从 34 到 41 与
    从 1863 到 1878 全打了一遍）：

        … 34-38 阴影渐变(230→227) | 39-40 `track`(232,238,240) | **41 起白面 255**
        … 白面到 1869 | 1870 卡的右边框 `track` | 1871-1878 阴影渐变回画布(231,236,239)

    ⇒ 左地标 = **白面起点** = 41 = `NAV_W` + 那条 1px 分界（算式里的字面量项，现读）
    ⇒ 右地标 = 白面收尾**再让开一格卡自己的右边框** = 客户宽 − 白面收尾 − 1 = 8 = `GAP`

    ⚠️ 第一版这里写成"从右往左找第一个与画布不同的像素"，量出来是 **7** 而不是 8：
    它停在的是**阴影的外缘**（阴影从外壳往外约 8px 连续渐变，最外那圈与画布差 9，
    一过阈值就被认成边界）。⇒ **像素判据要挑"这一格里唯一的那档色"**
    （白面 255 在壳层这一带只属于卡面），"离背景够不同"这类判据在带阴影的界面上
    会把阴影当成盒子。同款坑：AGENTS「同一档色在一屏上不止一处 ⇒ 判据要限定到它自己那一格」
    ⚠️ 白面阈值 ≥248 是浅色档（卡面 #FFFFFF）；这一跑固定浅色——深色档卡面是 #16262B，
      绝对亮度判据整条作废（路线表 32-19 那条），要量深色得另写一套按档读的判据
    ⚠️ 扫描窗与 `measure` 不同：从客户 0 扫到客户 `cw`，**不抄那个 40 当起点**——
      `measure` 里那个 40 是"跳过导航轨"的扫描起点（不是判据输入），但它抄的正是 `NAV_W`
      那一档；地标这条从 0 起扫，于是连"起点抄对没有"也一起判掉
    """
    runs = white_runs(png, 300, offx, offx + cw)
    if not runs:
        return None, None
    left = runs[0][0] - offx
    right = cw - (runs[-1][0] + runs[-1][1] - offx) - 1
    return left, right


def mid_shake(hwnd, tag, n=10, gap=0.04):
    """点完**立刻连抓 n 帧**，逐帧量左栏白面，返回(最小, 最大, 序列)。

    为什么要它（2026-10-12，路线表 32-24 的插桩取证炸出来的）：插桩打印
    `ResizableState::sizes()` 对比"保护在 / 保护关"两跑，量到的是——
      · 保护在：侧栏那一格在启动动画之后**一次都不再变**（九行全在启动段）；
      · 保护关：**每开合一次整组被按比例甩一遍**，侧栏从 318.7 **掉到 222.4**
        或**冲到 415.5**（−96px / +97px），然后再回到 318.7。
    ⇒ 所以那条保护防的不是"落定宽度算错"，是**过程里整组抖一下**。
      而两跑的**落定值只差 0.5px** ⇒ 等稳之后才量的判据（A–E 与八腿的极差）
      **结构上永远看不到它**——这才是变异 V1 一条都不红的完整解释。
    ⇒ 这一档唯一能常驻的帧上判据就是"**不等稳**、把过程中间帧抓出来比"：
      抓的是同一次开合里白面的**最大偏离**，不是平均值。

    ⚠️ 抓不抓得到取决于窗口：重分那几帧只有 ~150ms，`PrintWindow` 一帧约
      30–80ms ⇒ 十帧连着抓是刻意把采样密度拉到比窗口还细。
      **若基线量出来中间帧与落定值一样**，不能写成"没有抖"，只能写成
      "这一档帧上抓不到，判据落在插桩上"（AGENTS：判据抓不到 ≠ 它不存在）
    """
    vals = []
    for i in range(n):
        p = os.path.join(FIX_ROOT, f"mid-{tag}-{i}.png")
        shoot.capture(hwnd, p)
        s, _x0 = measure(p)
        vals.append(s)
        time.sleep(gap)
    return min(vals), max(vals), vals


def toggle_leg(label, hwnd, seq):
    """会话中点标题栏那颗「收起右栏 / 展开右栏」，量点完之后的两个数。

    三条纪律：
    · **坐标按名字现场量**——那颗键带 `accessibility_label`（`shell.rs:794/796`），
      矩形由系统给，不目测、不写死（D93；a11y 走查 2026-10-11 就是靠这一条
      把三处过期坐标换掉的）。
    · **生效判据不靠像素**：点下去会把 `inspector_open` 写回沙箱 config
      （`store::set_inspector_layout`），翻了才叫点中了——"alive / 画面动过"
      都不算（AGENTS：`alive` 的语义只是没崩）。
    · **点完要等稳**（`wait_stable`）：还宽与再撑宽是两次 `Window::resize`，
      抢量会量到中间态。
      ⚠️ 但**这一轮的中间态本身就是要量的东西**（见 `mid_shake`）⇒
      顺序改成"点击（settle 压到 0.05，不等稳）→ 读 config 确认点中了 →
      连抓十帧量过程 → 再等稳量落定值"。
    """
    clx, cly = A.client_origin(hwnd)
    n = A.one(A.dump(hwnd), label, "Button")
    before = os.path.join(FIX_ROOT, f"live-{seq}-before.png")
    shoot.capture(hwnd, before)
    steps.click(hwnd, n.cx - clx, n.cy - cly, settle=0.05)
    open_after = json.load(open(CFG, encoding="utf-8-sig"))["settings"]["inspector_open"]
    mid_lo, mid_hi, mid_vals = mid_shake(hwnd, seq)
    wait_stable(hwnd, before, f"live-{seq}")
    park_cursor(hwnd)
    p = os.path.join(FIX_ROOT, f"live-{seq}-{label}.png")
    shoot.capture(hwnd, p)
    s, x0 = measure(p)
    w = win_w(hwnd)
    ll, lr = land_marks(p, frame_offset(hwnd)[0], w)
    print(f"  腿 {seq}「{label}」：客户区宽 {w}，左栏白面宽 {s}（起于 {x0}），"
          f"config 的 inspector_open = {open_after}")
    print(f"      逐项地标：侧栏卡左缘 {ll}、右缘留白 {lr}")
    print(f"      过程十帧白面 {mid_vals} ⇒ 最坏偏离落定值 "
          f"{max(abs(mid_lo - s), abs(mid_hi - s))}px")
    return {"w": w, "s": s, "open_after": open_after, "frame": p,
            "land": (ll, lr),
            "mid_lo": mid_lo, "mid_hi": mid_hi, "mid_vals": mid_vals,
            "shake": max(abs(mid_lo - s), abs(mid_hi - s))}


def run_case(tag, inspector_open, sidebar_w, toggles=()):
    write_case(inspector_open, sidebar_w)
    env = qbguard.launch_env()
    assert env["APPDATA"] == SANDBOX, f"沙箱没生效：APPDATA={env.get('APPDATA')}"
    subprocess.run(["taskkill", "/F", "/IM", "qingbird-git.exe"], capture_output=True)
    time.sleep(0.7)
    p = subprocess.Popen([EXE], cwd=ROOT, env=env,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        hwnd = steps.find_window(p.pid)
        assert hwnd, f"{tag}：窗口没出来"
        assert steps.ensure_focus(hwnd), f"{tag}：拿不到前台"
        f0 = os.path.join(FIX_ROOT, f"{tag}-0.png")
        shoot.capture(hwnd, f0)
        bands = []
        for i in range(12):
            bands = nav_bands(f0)
            if len(bands) >= 2:
                break
            time.sleep(0.5)
            shoot.capture(hwnd, f0)
        ox, oy = frame_offset(hwnd)
        # 第二颗 =「我的仓库」。x/y **各减各的偏移**：从前记成"偏的是 y"点过 8px 的亏
        #（AGENTS：帧 = 客户 + (8, 0)），而这一档每次现场量，不抄那个数
        steps.click(hwnd, 28 - ox, bands[1] - oy, settle=2.2)
        wait_stable(hwnd, f0, tag)
        park_cursor(hwnd)
        f = os.path.join(FIX_ROOT, f"{tag}-final.png")
        shoot.capture(hwnd, f)
        w = win_w(hwnd)
        # 左栏白面：从导航轨右边一点起扫，取第一段（第一段就是侧栏那张卡的白面）
        s, x0 = measure(f)
        ll, lr = land_marks(f, ox, w)
        print(f"  {tag}：客户区宽 {w}，左栏白面宽 {s}（起于 {x0}）｜"
              f"逐项地标：侧栏卡左缘 {ll}、右缘留白 {lr}")
        res = {"w": w, "s": s, "frame": f, "live": [], "cfg_after": {}, "land": (ll, lr)}
        # 两腿就在这一跑的会话里点，不另起进程：要的就是"容器变过"那个事件序列
        for ix, label in enumerate(toggles, 1):
            res["live"].append(toggle_leg(label, hwnd, f"{tag}-{ix}"))
        # ⚠️ **config 要在这一跑的末尾立刻读，不许留到所有跑结束后再读**
        #（2026-10-12 我把它写在判据段里，当场红一条：A–E 五跑复用同一个沙箱文件，
        # 每条跑开头都 `write_case` 覆盖 ⇒ 判据读到的是 **E 那一次**写的
        # `sidebar_w = 360`，于是拿 A 的八腿去问"为什么 config 是 E 的数"。
        # **红的是读法，不是实现**——AGENTS「测试的前提不能外包给环境」的同族：
        # 这一回前提不是环境给的，是**别的跑给的**）
        if toggles:
            res["cfg_after"] = json.load(open(CFG, encoding="utf-8-sig"))["settings"]
        return res
    finally:
        p.terminate()
        try:
            p.wait(timeout=5)
        except Exception:
            pass


def frame_offset(hwnd):
    r = wt.RECT()
    u.GetWindowRect(hwnd, ctypes.byref(r))
    pt = wt.POINT(0, 0)
    u.ClientToScreen(hwnd, ctypes.byref(pt))
    return (pt.x - r.left, pt.y - r.top)


def main():
    if "--parse-only" in sys.argv:
        # 只报"从代码读到了什么"，不起进程、不动夹具——给变异账与"盲区自证"用
        print("   现读到的项：" + " + ".join(f"{t}={SHELL_VALUES[t]}" for t in SHELL_TERMS))
        print(f"   固定项之和 {SHELL_FIXED}｜中间下限 {MAIN_MIN}｜初始宽 {INITIAL_W}")
        print(f"   逐项地标：左（侧栏卡左缘）应 == {LAND_LEFT}"
              f"（NAV_W {SHELL_VALUES['NAV_W']} + 边框 {RAIL_BORDER_LIT}）"
              f"｜右（窗口右缘留白）应 == {LAND_RIGHT}（GAP）")
        print(f"   侧栏 320 抽屉 560 时：开 {max(INITIAL_W, SHELL_FIXED + 320.0 + MAIN_MIN + 560.0)}"
              f"／关 {max(INITIAL_W, SHELL_FIXED + 320.0 + MAIN_MIN)}")
        comp = sum_with_overrides({"NAV_W": 48.0, "GAP": 0.0})
        t_open = max(INITIAL_W, comp + 320.0 + MAIN_MIN + 560.0)
        t_shut = max(INITIAL_W, comp + 320.0 + MAIN_MIN)
        l2 = 48.0 + float(RAIL_BORDER_LIT)
        print(f"   ⚠️ **盲区自证**：把 NAV_W 与 GAP 同时换成 48.0 与 0.0（**和不变**）"
              f"⇒ 固定项还是 {comp}、两条 target 仍是 开 {t_open}／关 {t_shut}；"
              f"而两个地标的**期望**也一起漂（左 {LAND_LEFT}→{l2}、右 {LAND_RIGHT}→0.0）"
              f"⇒ **这一档【预期不红】：两侧同源，改值型错任何像素判据都判不出**"
              f"（32-26 的地标负责的是另一类：**布局与算式解耦**——那种错和不变、格会变，"
              f"实测见变异账 G1/G2/G3，G4 就是这条不红的实证）。这条打印不是判据，"
              f"是**说明这一档到今天仍然没有判据负责，而且结构上不会有**")
        return 0 if (abs(comp - SHELL_FIXED) < 1e-6
                     and t_open == max(INITIAL_W, SHELL_FIXED + 320.0 + MAIN_MIN + 560.0)
                     and t_shut == max(INITIAL_W, SHELL_FIXED + 320.0 + MAIN_MIN)
                     and abs(l2 - (LAND_LEFT + 8.0)) < 1e-6) else 1
    os.environ["QB_APPDATA"] = SANDBOX
    st = os.stat(EXE)
    print(f"产物：{EXE}  mtime={datetime.datetime.fromtimestamp(st.st_mtime)}  size={st.st_size}")
    build_fixture()
    build_sandbox()
    before = qbguard.real_cfg_fp()
    try:
        lo, hi = read_range("SIDEBAR_W_RANGE")
        # 基线要留出 DRAG_DELTA 的余量，否则 D/E 那一跑会被 clamp 夹回同一个宽，
        # 等于两跑测的是同一个状态而判据以为它们差 40
        sw = max(lo, min(BASE["sidebar_w"], hi - DRAG_DELTA))
        print(f"   侧栏合法区间（从 core 现读）{lo}–{hi}；"
              f"他配的是 {BASE['sidebar_w']} → 基线取 {sw}，D/E 取 {sw + DRAG_DELTA}")
        if sw != BASE["sidebar_w"]:
            print(f"   ⚠️ 他那个数离上限只剩 {hi - BASE['sidebar_w']}，"
                  f"不够放 {DRAG_DELTA} 的增量 ⇒ 基线往下挪，五跑仍测同一条规则")

        a = run_case("A-open", True, sw,
                     toggles=("收起右栏", "展开右栏") * CYCLES)
        b = run_case("B-closed", False, sw)
        c = run_case("C-open-again", True, sw)
        d = run_case("D-open-wider", True, sw + DRAG_DELTA)
        e = run_case("E-closed-wider", False, sw + DRAG_DELTA)

        insp = BASE["inspector_w"]

        def need(open_, sidebar):
            return SHELL_FIXED + sidebar + MAIN_MIN + (insp if open_ else 0.0)

        def target(open_, sidebar):
            """规则的完整形状：**宽 = max(初始宽, 需求)**。
            那条规则只会"向右撑"，需求低于初始宽时它一个像素都不收——
            所以只写 `== need` 会在"需求小于初始"那一跑上假红（上一版就红在这里，
            而我当时把它写成 `>=`，等于把这条规则的另一半从判据里漏掉了）。
            `max` 让两种情形都由同一条判据负责，换谁的 sidebar_w 都不用改脚本"""
            return max(INITIAL_W, need(open_, sidebar))

        # 把**读到的项**原样打出来（不是打我一个汇总数）：这一行就是"现读真的读了"的自证，
        # 少了它，"从代码现读"与"抄了一个恰好对的数"在输出上完全无法区分
        print(f"   算式（从 `required_shell_w` 现读到的项）：{' + '.join(SHELL_TERMS)}"
              f" ⇒ 固定项 {SHELL_FIXED} + 侧栏 + 中间下限 {MAIN_MIN} + 抽屉 {insp}，"
              f"再与初始 {INITIAL_W}（从 `tray.rs` 现读）取大；"
              f"侧栏基线 {sw}（读配置）→ A 该是 {target(True, sw)}、B 该是 {target(False, sw)}")
        # 八腿的数：少了腿要**当场点名"前提没造出来"**，不许留到判据里 IndexError
        legs = a["live"]
        assert len(legs) == 2 * CYCLES, \
            f"会话中开合只跑出来 {len(legs)} 腿（要 {2 * CYCLES}） ⇒ 判据不能判"
        ok = [
            # ── 两条**前提**（放在判据之前，并把名字写成"前提"）：
            # 状态走配置不等于配置里的值真的进了界面——应用会夹它（`clamp_sidebar_w`），
            # 而"两跑其实是同一个状态"若由后面的规则判据去发现，报出来的就是"规则坏了"。
            # 上一轮那三条红就是这么来的：红的是前提，不是实现
            ("前提：D 的侧栏比 A 宽 ≈ 40（没被 clamp 夹掉）",
             abs((d["s"] - a["s"]) - DRAG_DELTA) <= 3),
            ("前提：A 与 B 只差抽屉那一列（宽差 ≈ inspector_w，收起真的生效）",
             abs((a["w"] - b["w"]) - insp) <= 2),
            # ①② 就是补二十五那条原话：抽屉开与关，左栏那一列不许被抢
            ("B 与 A 左栏同宽（抽屉不抢左栏）", abs(b["s"] - a["s"]) <= 1),
            ("C 与 A 窗口同宽（再撑一次撑到同一个数）", abs(c["w"] - a["w"]) <= 2),
            ("C 与 A 左栏同宽", abs(c["s"] - a["s"]) <= 1),
            # ④ 撑宽这件事要能算出来，不是"看起来变宽了"
            ("A 的宽 == 算式给的数", abs(a["w"] - target(True, sw)) <= 2),
            ("B 的宽 == 算式给的数（含『需求不到初始就不收』那一半）",
             abs(b["w"] - target(False, sw)) <= 2),
            # ⑤ 侧栏变宽的 40 必须由窗口承担。判据取**两个增量的差**，
            # 不去反算中间列的绝对宽（那要把轨宽与右缘宽抄成脚本里的数）
            ("D 侧栏 +40 之后窗口确实长了", (d["w"] - a["w"]) >= 20),
            ("D 长的部分全部进了侧栏（没拿中间列垫）",
             abs((d["w"] - a["w"]) - (d["s"] - a["s"])) <= 3),
            ("D 的宽 == 算式给的数", abs(d["w"] - target(True, sw + DRAG_DELTA)) <= 2),
            ("E 的宽 == 算式给的数", abs(e["w"] - target(False, sw + DRAG_DELTA)) <= 2),
            # 头注释答应过的一条：D 与 E 只差抽屉那一列，证撑宽真的由抽屉参与
            # （宽差要读配置里那一列，不许抄一个 320 进来——32-15 就红在抄的数上）
            ("D 与 E 只差抽屉那一列（宽差 ≈ inspector_w）",
             abs((d["w"] - e["w"]) - insp) <= 2),
        ]

        # ── 会话中开合抽屉的八腿：A–E 判不到的那一档（容器宽度变化那一下）
        for ix, leg in enumerate(legs, 1):
            want_open = ix % 2 == 0          # 奇数腿「收起」、偶数腿「展开」
            mode = "展开" if want_open else "收起"
            ok.append((f"前提：第 {ix} 腿（{mode}）点到了"
                       f"（config 的 inspector_open = {want_open}）",
                       leg["open_after"] is want_open))
            ok.append((f"第 {ix} 腿（{mode}）左栏没被抢宽（白面 == A 的 {a['s']}）",
                       abs(leg["s"] - a["s"]) <= 1))
            ok.append((f"第 {ix} 腿（{mode}）的宽 == {mode}态算式给的数"
                       f"（还宽只还我们自己撑的那一段）",
                       abs(leg["w"] - target(want_open, sw)) <= 2))

        if CYCLES:
            closed = [l for ix, l in enumerate(legs, 1) if ix % 2 == 1]
            opened = [l for ix, l in enumerate(legs, 1) if ix % 2 == 0]
            # 这两条**既不跟 A 比、也不跟算式比**，判的是"同一种态重复四次，彼此是不是
            # 同一个数" ⇒ 它专抓「每开合一次漂一点」，而这一档在两腿时判不出来
            # （单腿对 A 的差落在 1px 抖动带里，V1 那一轮红一条又绿回来就是证据）。
            # ⚠️ 容差的依据要写清，不是拍一个数：
            #   · 宽 —— 基线里 A / C / G 三次展开都是 1879、B / F 两次收起都是 1319，
            #     **从没抖过**，它是 `GetClientRect` 的整数读数，不是像素测量 ⇒ 极差容差 1
            #   · 白面 —— 是像素量出来的，跑与跑之间抖 1 ⇒ 四次采样的最坏极差就是 2
            # ⇒ 这一档的**分辨率下限**：抓得住"每轮累积到 ≥3px"的系统性漂移，
            #   抓不住 ≤2px 的。历史上补二十五那条病是 **100px 级**（263 被重分成 630）。
            # ⚠️ **两条都要判白面，不能只判宽**：补二十五那个症状是**列分配**被按比例重分
            # （帧上量到过 `split = [630, 282]`，左栏从 263 变 630），它动的是**白面 s**；
            # 而窗口宽 w 是我们自己 `resize` 出来的，压根不经过 `ResizableState`，
            # 所以"只量宽"的那两条判据对这一档病**天然是瞎的**（写的时候没想清，
            # 是读完 `required_shell_w` 才发现：那颗纯函数的输入是**意图宽**，
            # 列漂了它照样算出同一个 w）
            ok.append(("四次「收起」彼此的宽极差 ≤ 1，且白面极差 ≤ 2（逐步漂就在这一条上，不经算式）",
                       max(l["w"] for l in closed) - min(l["w"] for l in closed) <= 1
                       and max(l["s"] for l in closed) - min(l["s"] for l in closed) <= 2))
            ok.append(("四次「展开」彼此的宽极差 ≤ 1，且白面极差 ≤ 2（同上）",
                       max(l["w"] for l in opened) - min(l["w"] for l in opened) <= 1
                       and max(l["s"] for l in opened) - min(l["s"] for l in opened) <= 2))
            # 这条也**不经过算式**：末腿（第 2×CYCLES 腿，展开态）要回到 A 起来时的宽。
            # 逐腿那条「宽 == 算式」依赖脚本抄的四个壳层常数，这条不依赖 ——
            # 常数过期时前者红、后者不红（V3 实测就是这个分工），
            # 而"实现把往返做坏了"时两者都红，报出来的话因此能分开归因
            ok.append((f"末腿（第 {2 * CYCLES} 腿）的宽 == A 起来时的宽（不经算式，专抓往返不回来）",
                       legs[-1]["w"] == a["w"]))
            # ── **过程抖动**：插桩取证（32-24）证明关掉那层保护以后，每开合一次
            # `ResizableState` 整组会被按比例甩一遍（侧栏 318.7 → 222.4 / 415.5，
            # 再回到 318.7），而**落定值只差 0.5px** ⇒ 上面所有"等稳之后量"的判据
            # 一条都抓不到它。这一条量的是"过程中最坏那一帧偏落定值多少"，
            # 是这一档**唯一能在帧上常驻**的判据
            worst = max(l["shake"] for l in legs)
            print(f"   过程抖动最坏 {worst}px（容差 {SHAKE_MAX}px）：各腿 "
                  + " ".join(f"{ix}.{l['shake']}" for ix, l in enumerate(legs, 1)))
            ok.append((f"开合那一下左栏不许整组被甩（过程最坏偏离落定值 ≤ {SHAKE_MAX}px）",
                       worst <= SHAKE_MAX))
            # 前提：算式用的两个数必须是**当下真在用的**那两列宽。会话中点那颗键会把布局
            # 写回 config（`set_inspector_layout`），如果应用顺手把 `inspector_w` 或
            # `sidebar_w` 也改了，那上面所有「宽 == 算式」都在拿旧的数判新的状态。
            # ⚠️ 读的时机在 `run_case` 里（八腿点完、进程还在），**不在这里**——见那里那段
            cfg_after = a["cfg_after"]
            ok.append(("前提：八腿点完（进程还活着时）config 的两列宽度没被应用改写"
                       f"（inspector_w 仍是 {insp}、sidebar_w 仍是 {sw}）",
                       abs(float(cfg_after.get("inspector_w") or 0.0) - insp) <= 1
                       and abs(float(cfg_after.get("sidebar_w") or 0.0) - sw) <= 1))

        # ── **逐项地标**（2026-10-12，路线表 32-26；32-25 的 M3 那一档撞出来的）：
        # 上面所有条判的都是"宽 == 若干项之**和**"⇒ 把和拆成两项的补偿性错看不见。
        # 这一组把每一项各自钉在帧上的一格：`NAV_W`+边框钉"侧栏卡左缘"、`GAP` 钉"右缘留白"，
        # 于是**改了布局没改算式**（或反过来）那一类漂移当场红在本项上，
        # 而"宽 == 算式"那几条照样绿（和没变）⇒ 报出来的红能归因，不是一句"宽不对"。
        # ⚠️ 容差 1px 与白面宽同源：白面边缘是硬边，量不出抖动，但 `PrintWindow` 抓的
        #   帧之间有 1px 级抗锯齿差（32-24 实测过）⇒ 不许写 0。
        # ⚠️ 八腿的地标**只打印不判**：同一态重复四次的"地标漂"由那两条极差判据
        #   （白面 == A）与过程抖动那条负责，再加 16 条只让判读表更长、不多覆盖任何一类
        print(f"   逐项地标（期望从算式现读：左 {LAND_LEFT} = NAV_W {SHELL_VALUES['NAV_W']}"
              f" + 那条边框 {RAIL_BORDER_LIT}｜右 {LAND_RIGHT} = GAP）："
              + "  ".join(f"{k}.{v['land'][0]}/{v['land'][1]}"
                          for k, v in (("A", a), ("B", b), ("C", c), ("D", d), ("E", e))))
        for k, v in (("A", a), ("B", b), ("C", c), ("D", d), ("E", e)):
            ll, lr = v["land"]
            ok.append((f"{k} 的侧栏卡左缘 == 算式里除 GAP/MAIN_MIN 之外各项之和 {LAND_LEFT}"
                       f"（帧上量到 {ll}；轨宽与那条边框各自承重，不再只看和）",
                       ll is not None and abs(ll - LAND_LEFT) <= 1))
            ok.append((f"{k} 的窗口右缘留白 == 算式里的 GAP {LAND_RIGHT}"
                       f"（帧上量到 {lr}；这一项的落点是 `shell.rs` 那处 `.pr(px(GAP))`）",
                       lr is not None and abs(lr - LAND_RIGHT) <= 1))
        print("\n判读：")
        for name, passed in ok:
            print(f"   {'通过' if passed else '不通过'}  {name}")
        print(f"   参考：A {a['w']}/{a['s']}  B {b['w']}/{b['s']}  C {c['w']}/{c['s']}"
              f"  D {d['w']}/{d['s']}  E {e['w']}/{e['s']}")
        # 八腿的数**逐条打出来**：极差判据只给"红/不红"，而"是不是逐步漂"要看序列
        #（32-24 就是因为只看两条腿的两对数，才把归因交给了抖动带）
        if CYCLES:
            print("   八腿（宽/白面）：" + "  ".join(
                f"{ix}.{l['w']}/{l['s']}" for ix, l in enumerate(legs, 1)))
            print(f"   收起四次：宽 {[l['w'] for l in closed]} 白面 {[l['s'] for l in closed]}｜"
                  f"展开四次：宽 {[l['w'] for l in opened]} 白面 {[l['s'] for l in opened]}")
        else:
            print("   （这一跑 `GROW_CYCLES=0`：会话中开合那六条判据整个跳过，"
                  "日常与出口跑的是八腿 ⇒ 别把这一跑读成「八腿也验过了」）")
        return 0 if all(p for _, p in ok) else 1
    finally:
        after = qbguard.real_cfg_fp()
        print(f"真实 config 指纹 before={before} after={after}｜"
              f"{'一致' if before == after else '**不一致，查是谁写的**'}")


if __name__ == "__main__":
    sys.exit(main())
