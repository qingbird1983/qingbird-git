# CODE-AUDIT — 全仓臃肿与死代码审查台账（2026-10-12）

> ## 清欠结果（2026-10-13，改完这批后本文件可删）
>
> - **已落地**（提交 `85cef47` P1 纯删除 19 项 / `9e902b5` P2 四项合并 /
>   `f6e0a83` titlebar_toggle+status_mark / `40bf0bc` hover_face+empty_state+删 scanrow.py）：
>   §1 除长函数拆分外全部、§2 全部、§3.1（`footer(left)` 左槽接口已解决）、
>   §3.2（`RESTORE_TIMEOUT` 已改名）、§4 待拍的弹簧表与悬停面/空态两条。
> - **复核后不做的**（不是漏掉，是台账条目本身站不住或被设计决策覆盖）：
>   `progress_bar` 是防 2026-10-08 文案漂移的真构件，不删；`ghost_btn` 剩余
>   18 处全是文字键或正当少数派，「⛔ 文字键不进」即设计决策；
>   `dialogs/mod.rs:3` 注释在 footer 收进本模块后已自洽。
> - **用户拍板暂不做**（2026-10-13）：§1 第 7/8 条长函数拆分（11 个 GUI + 4 个 core）、
>   §3.4 rustfmt/clippy。**删除本文件前把这两条搬去 `docs/UI-ROADMAP.md` 未决事项。**
> - **文档漂移 §4 已改表**之外，本轮新增落地更正：v4 §6 悬停面/空态两行、
>   §7 空态规格行（GLYPH+line → empty_state+tier4）、AGENTS 弹簧表改指路 v4 §4.4。

⚠️ **这是一次性审查台账，不是规范，也不是真源。** 它登记的每一条都带
`路径:行号` 证据，可以逐条复核；**看完就该删**（或按结论改完这批后删）。
规范在 `docs/CODE.md`（下笔）与 `docs/DOCS.md`（文档），纪律在 `AGENTS.md`。

**这次的审查口径**：
- **只报不改**（代码一条都没动），**只读**。真正改掉的是 **§4 文档漂移**里那几条
  （它们不是"建议"，是文档在骗人，且改动是删除/更正，不碰实现）。
- 行号是**2026-10-12 实测**的，代码一改就漂；复核时以函数名/常量为准，行号只做定位。
- 前置证据：`cargo check --workspace --message-format short` **零错零警、退出码 0**；
  `cargo test --workspace` **379 passed / 0 failed / 0 ignored（234 core + 145 gui），退出码 0**。
  所以下面没有一条是"编译不过/有告警/测试红"能报出来的。
- 审查方式：`crates/core` 逐个 item 查引用者；`gui` 250+ 函数逐个查调用点；
  两条独立子代理清单**互不知情**，重叠的部分在下面注明。

---

## 1. 臃肿（按净收益从大到小）

```
safe-delete shrink 11 个对话框各抄一遍同一条底栏. 替换为: dialogs::footer(t, primary, cancel) 一处. 证据: gui/src/dialogs/commit.rs:232-263 · purge.rs:293-321 · init.rs:139-174 · cmds.rs:464-500 · clone.rs:603-644 · settings.rs:1326-1400 · stash.rs:455-472 · batch_log.rs:134-160 · close_prompt.rs:128-172 · clean.rs:174-203 · sync.rs:163（Button::new("cancel"/"close"/"go") 共 23 处）
verify-first shrink 10 个 build_*_dlg 重写同一段开壳. 替换为: dialogs::shell(title, w) 一处. 证据: commit.rs:99-121 与 purge.rs:78-100 逐字同形；clone.rs:130 · cmds.rs:144 · stash.rs:224 · repo.rs:324 · settings.rs:252 · batch_log.rs:37
safe-delete shrink gui/src/ops.rs:120-167 与 :261-308 是逐字相同的两份 48 行. 替换为: drain_log_until_done(this, cx, buf, done, path)（两块收尾 :168-189/:309-361 同结构一并收）. 证据: pwsh 逐行 -ceq 实测 identical=True
safe-delete shrink 全仓 28 处 .custom(ghost_variant(cx)) 各跟 5-7 行同链. 替换为: widgets::ghost_btn(id, icon, t) 一处（其中 20 处在这六文件之外）. 证据: changes.rs:55,69 · history_tab.rs:96 · log.rs:194,209,222,235,627 · overview.rs:631,660 · panels.rs:99,132,391 · readme_tab.rs:359,388 · repo_head.rs:279,302,655 · shell.rs:730,768,800,1159,1215 · sidebar.rs:312 · space_tab.rs:474,658 · widgets.rs:418
safe-delete shrink 三颗标题栏开关是同段三份. 替换为: titlebar_toggle(icon, action, on, cx). 证据: gui/src/shell.rs:714-739 theme_toggle · :754-774 sidebar_toggle · :787-806 inspector_toggle
safe-delete shrink 四颗 wash 底函数只差两个 rgb 常量. 替换为: wash(rgb_light, rgb_dark, t) 一处. 证据: gui/src/tokens.rs:100-107 orange_bg · :118-124 blue_bg · :127-133 red_bg · :139-145 green_bg
verify-first shrink 11 个 >150 行函数（纯拆，不减行数，但它们是"改一处要读五百行"的根因）. 替换为: 每个抽 2-4 段私有函数. 证据: gui/src/shell.rs:262 finish_frame 335 行 · palette.rs:278 palette_model 303 · panels.rs:237 sync_card 282 · shell.rs:1291 Shell::new 281 · space_tab.rs:38 space_body 270 · shell.rs:45 render 217 · panels.rs:29 fork_card 208 · ops.rs:196 run_sync 182 · overview.rs:590 overview_page 166 · sidebar.rs:357 repo_row 160 · repo_head.rs:474 addr_seg 152
verify-first shrink crates/core 四个长函数. 替换为: git.rs 抽 spawn_in_job / pump_streams / poll_until_done；net.rs 把 race 与 run_net_op 的共同骨架抽出来. 证据: crates/core/src/git.rs:285 run_git_inner 205 行 · net.rs:518 run_net_op 194 · net.rs:341 race_direct_vs_proxy 107 · sync.rs:316 do_pull 113；另 run_cmd.rs:353 run_repo_cmd 106 · fork.rs:213 rebase_core 93 · run_cmd.rs:238 detect_cmds 93
safe-delete shrink 「别名 trim、空则退仓库名」全仓 4 份. 替换为: 调 core 的 scheduler::display_name（它已是唯一出处，只是没被导出去用）. 证据: crates/core/src/scheduler.rs:258-262 定义 → 只被同文件 :275 调；gui/src/overview.rs:477 · gui/src/dialogs/repo.rs:297 · gui/src/shell.rs:1694 各写一遍
safe-delete shrink 进度阶段白名单两份（9 条英文 vs 9 条中文）. 替换为: 一份（谁在用谁的那份为准，先定语义再删）. 证据: crates/core/src/types.rs:190-200 与 net.rs:133-143；types.rs:152 注释自认「必须与 net::parse_progress 那份保持同步」
safe-delete shrink 百分比解析两份，且返回型不同（100.0 vs None）. 替换为: 一份 Option<f32>（`percent_of` 那个失败返 100.0 的分支是"把解析失败显示成 100%"）. 证据: crates/core/src/types.rs:204-210 percent_of 与 net.rs:128-132 闭包 pct
safe-delete shrink LOCAL_TIMEOUT 有 3 个同值定义（10s）. 替换为: 一处 pub const，另两处 import. 证据: crates/core/src/net.rs:12 · repos.rs:15 · history.rs:19；crates/core/src/space.rs:10 import 的反而是 repos 那份
safe-delete shrink crates/core/src/repos.rs:18-20 私有 run_local 是 net.rs:325-327 的逐字副本. 替换为: use crate::net::run_local（history.rs:14 已经是这个做法）
safe-delete shrink crates/core/src/types.rs:204-210 percent_of 的失败分支. 替换为: 返 Option 让调用方决定（失败时不是 100%）. 证据: 同一功能在 net.rs:128-132 已返 None
safe-delete delete gui/src/log.rs:137 log_follow_bottom 是单行包装. 替换为: 直接把那一行内联（唯一调用点 :82）
safe-delete delete gui/src/log.rs:314 progress_bar 是转发层. 替换为: 调用点直接用底层（:104 log.rs、panels.rs:479）
safe-delete delete gui/src/ops.rs:1198 run_stash_apply / :1203 run_stash_drop 是 stash_op 的纯转发. 替换为: 调用点直接用 stash_op(:1215). 证据: 调用点 dialogs/stash.rs:374 / :413
safe-delete delete gui/src/ops.rs:725 batch_take_next 只有一个调用点. 替换为: 内联（:716）
safe-delete delete gui/src/widgets.rs:247 pill_icon / :271 pill_out 是 pill_with(:275) 的定值包装. 替换为: 调用点传参（同族的 widgets.rs:241 pill 有 6 个调用点，保留）
safe-delete delete gui/src/dialogs/init.rs:139-140 init_footer(t) 收 t 却不用（靠 `let _ = t;` 消音）. 替换为: 去掉参数
```

**这一组的小计**（两张清单可能有少量重叠，取区间）：**约 -500 ~ -700 行**，
其中"纯删除"约 -250 行（转发层 / 包装 / 定值 / 逐字副本），
"合并后减少"约 -250 行（底栏与开壳），其余是拆分带来的可读性收益（不减行数）。

## 2. 死代码（都确认过"零读者"，但仍然要按下面的口径复核）

```
safe-delete delete gui/src/drag_drop.rs:104 pub dup 只写不读. 证据: 唯一写点 :188（Verdict::Already 分支）；is_quiet() :119-121 与 summary() :124-141 都不读它；全仓 grep \bdup\b 只有 :104 / :188 / :324（单测构造 DropReport{dup:2} 且只断言 is_quiet()）
safe-delete delete crates/core/src/lib.rs:10-11 crate 级 #![allow(dead_code)] 及其理由. 替换为: 删掉，让编译器重新盯住全 core（抽查过的 pub 项都有真实读者：MAX_CONCURRENT_OPS→gui/src/ops.rs:659 · state.rs:1597/2032；sync::try_begin_op→gui/src/scheduler.rs:85）. 证据: 那句注释的理由是"骨架期…暂未使用"，已过期
safe-delete delete crates/core/src/repos.rs:1079-1086 clamp_readme 的 Option 是假的. 证据: 两个 return 都是 Some
safe-delete delete crates/core/src/store.rs:307-309 一句 doc 挂错了函数. 替换为: 挂到 :348 fn default_sidebar_w
safe-delete shrink pub 但跨模块零读者四项. 替换为: 降成 pub(crate) 或删. 证据: crates/core/src/net.rs:12 LOCAL_TIMEOUT · sync.rs:22 CHECK_TIMEOUT · net.rs:196-221 BASE64_CHARS/base64_encode（生产只用 net.rs:257 那条）· scheduler.rs:258 display_name（同文件 :275 用）
safe-delete shrink 三个 core 常量在 GUI 侧各有一个别名. 替换为: 直接用 core 的（别名只被 shell.rs:235/387/389 读）. 证据: gui/src/tokens.rs:849/850/857 = qingbird_core::store::INSPECTOR_W_RANGE/.SIDEBAR_W_RANGE 的 .0/.1；tokens.rs:842 注释自称同一份定义
verify-first delete gui/src/dialogs/mod.rs:3 的模块注释说错. 证据: 它称 14 个 dialog 共用 dlg_footer，实际定义在 dialogs/repo.rs:164、唯一调用 repo.rs:383
verify-first shrink gui/src/changes.rs:226 status_mark 与 widgets.rs:320 status_mark 同名不同义. 替换为: 改名（前者收 &ChangeEntry，后者收 bool）
safe-delete delete-or-complete scripts/scanrow.py 是个跑不起来的存根. 替换为: 补成能跑（它要做的事在文件头那段 docstring 里写全了）或直接删. 证据: 实测 1276 字节 / 25 行、三引号只出现 1 次（未闭合）、零代码，`python scripts/scanrow.py` 直接 `SyntaxError: unterminated triple-quoted string literal (detected at line 25)`（退出码 1）；全历史只有一次提交 f05a184（2026-10-05 阶段 20），而 docs/UI-ROADMAP.md:2576 写着"顺带加了一个工具"并给出用法与深色阈值口径 ⇒ **文档在描述一个不存在的工具**
```

⚠️ **不是死代码，别删**（差点被"零读者"误判的四类）：
- `gui/src/state.rs` 的 6 个只写不读字段（`:844`、`:893`、`:917`、`:920`、`:952`、
  以及 `HistoryState.task` `:450`）：它们是 **Task 句柄，靠 drop 取消**。
- `crates/core/src/git.rs:110 GitOutput.code`：只有测试读，但那是**刻意的观测面**。
- `crates/core/src/net.rs:196-221` 手搓 base64(26 行)：**标准库没有**，换要加依赖，
  自带 7 条单测 ⇒ 保留。
- `crates/core/src/scan_cache.rs:158 BUSY_TIMEOUT_MS`：SQLite busy 超时，是真实参数。
- `gui/src/inspector.rs`（73 行空壳）：**用户刻意保留**（"先标成开发中，未来规划给 AI 用"）。

## 3. 需人拍（不是"该不该删"，是"删哪一个"）

1. **`gui/src/dialogs/repo.rs:164 dlg_footer` 与另外 11 处底栏能不能合并**：
   repo.rs 那颗是 `justify_between`（左槽留给「移除仓库」），11 处是 `justify_end`。
   **合并前必须先定接口**（左槽怎么表达），否则会把「移除仓库」挤到右边。
   —— 顺带：`repo.rs:161-162` 有一句孤儿 doc 挂在那里。
2. **`crates/core/src/purge.rs:59` 那个 60s 超时**：别的 `LOCAL_TIMEOUT` 都是 10s。
   它多半是"另一件事"（回滚/恢复），**建议改名 `RESTORE_TIMEOUT` 而不是并成 10s**。
3. **§1 里那些"合并成一处"的项要不要真做**：它们都在**实机看得见的地方**
   （底栏、按钮、wash 底），改完全部要抓帧复验。收益是 -250 行与"下次改一处只改一处"。
4. **`rustfmt` / `clippy` 要不要引入**（`docs/CODE.md` §待补）：现在 CI **不跑**这两样，
   也没有配置文件。引入要先跑一次全仓 `cargo fmt`（一次性大 diff）。

## 4. 文档漂移台账（本次**已改**的，与**待拍**的）

### 已改（2026-10-12，全部只动文档，未碰任何代码，`cargo check` 前后无关）

| 文件 | 改了什么 | 证据 |
|---|---|---|
| `docs/UI-DESIGN.md` | **删掉整张"主色板"与"徽标 wash 底"表**（换成一句指路 v4 §1） | 那套 #FAFAF9 / #37352F / #E5E3DF / #5B7A99 在 `gui/theme.json` 里 **grep 零命中**；真实：画布 `#E8EDF0`、描边 `#D7E0E4`、`accent.background` `#0E4F55` |
| `docs/UI-DESIGN.md` | **删掉"档位表"与"间距语义"两张表**（换成指路 v4 §3） | 表里的 FS_BODY 13 / R_LG 8 / R_XL 12 全是旧值：字号走 `UiScale::metrics()` 三档，`R_LG`/`R_XL` 读 `theme.json` 的 `radius`/`radius.lg` = **14 / 20** |
| `docs/UI-DESIGN-v4.md` §3.2 | **卡间由 12 改为 6**，并登记"这是照实现更正文档" | 2026-10-11 竖剖实测全站 27 处卡间带都是 6；`gui/src/tokens.rs:441-449` 记着用户 2026-10-08 定的「卡间统一 6」；路线表 32-23 |
| `docs/UI-DESIGN-v4.md` §3.2 | 补进"区块之间 = 两档 `GAP_LG`"（原在 UI-DESIGN 4.2 那句里，随表一起收进来） | 同上，避免随作废的表一起丢失 |
| `docs/UI-DESIGN-v4.md` §3.5 | `ICON_LG` 改成真实名字 **`GLYPH_ICON`** | `gui/src/tokens.rs:612 GLYPH_ICON 20`；全仓没有 `ICON_LG` |
| `AGENTS.md` | **删掉重复的那一行**「GUI 只能由用户双击 exe 验收」（原来在 :377 与 :380 各一份） | 亲读确认是同句重复 |
| `AGENTS.md` | 「视觉规范」那条**不再复述数值**，改成指路 v4 §3 | 它原来抄的字号 11/12/13/16/18/20 与圆角 4/6/8/12 全是旧值，且违反它自己的「一条视觉规则只允许有一个家」 |
| `README.md` | 安装包示例路径去掉硬写的 `0.1.0`（改 `<版本号>`） | 真实版本是根 `Cargo.toml` 的 `0.3.0`；AGENTS 自己写着"写过 0.1.0 的那句在 v0.2.0 之后就是假话" |
| `README.md` | 两条测试命令去掉硬写的条数（188 / 121） | `AGENTS.md` 自己的口径是"不复述具体数字" |
| `AGENTS.md` | 测试条数也不再写死（原来写着 core 221 / gui 137） | `cargo test --workspace` 实测 **234 + 145**，两套数都不对了；同一份文件里同时存在 221/137 与 README 的 188/121 就是"三条数各自漂" |
| `docs/UI-ROADMAP.md` | 在「深浅两套的几条定值依据」那张表前加一条注：**那是阶段 20-1 的值，现在只认 `gui/theme.json`** | 那张表里画布写着 `#FAFAF9`，而 `theme.json` 早已是 `#E8EDF0`；路线表自称"唯一施工真源"，一张看着像现状的旧表最容易被照着抄 |
| `gui/src/tokens.rs` | 一句注释指向的小节已被删除（`UI-DESIGN.md` 4.2「间距语义」）⇒ 改指 `UI-DESIGN-v4.md` 3.2 | 表作废并换成指路；留着旧指向就是一句会骗人的话（`docs/CODE.md` §5 那条） |
| `docs/UI-DESIGN.md` + `docs/UI-DESIGN-v4.md` | 两套铁律去重并划清编号作用域：`UI-DESIGN.md` §0 改叫「界面三条铁律」、`v4` §1 改叫「视觉四条铁律」，各自加一句"编号只在本文件内"+ 写明另一份在哪；**真正重复的「不许出现裸数字」只在 v4 §1 写全**，UI-DESIGN 那份改成指路 v4 §3、只留"组件尺寸只用官方命名档"这半条界面侧约束 | 两套并成一个清单就必须把结构规则搬进视觉契约文档 = 搬错家（`docs/DOCS.md` §2.1）；而编号撞车的实测代价见下一行 |
| `gui/src/log.rs` | `:152` 的 `（v4 铁律 3）` 改成 `（docs/UI-DESIGN.md §0 铁律 3）` | **这是编号撞车已经造成的真错误**：它要说的是"同一动作一屏两个入口"（UI-DESIGN 铁律 3），而 v4 铁律 3 是「不演拿不到的效果」——引用指错了家，读代码的人会去 v4 找一条不相干的规则 |
| `docs/DOCS.md` | §2「指路，不复述」补一条：**引用要带文档名，编号只在本文件内有意义**，并登记上面那次实锤 | 家分对了但引用不带文档名，等于把编号当成全局的——两份都从 1 编号时必然指错 |
| `README.md:32-33` | 并发与锁口径改成"最多 **4 路并发**（`sync::MAX_CONCURRENT_OPS`）+ 互斥是**每仓库一把锁**（`sync::try_begin_op(path)`）+ 配置写另有 `store::SAVE_LOCK`"，并注明旧那句是 32-8 / 32-9 之前的形状 | 旧句**两半都过期**："整个应用一把锁"在 32-8 已下沉到每仓库，"下沉登记在阶段 32"那两个阶段已经落完（`gui/src/ops.rs:659` 的注释写着"最多 `MAX_CONCURRENT_OPS` 路，完成一颗补一颗"） |
| `docs/UI-DESIGN-v4.md:472` | §7 表里的表单标签列宽 `104` 改成 `tokens::FORM_LABEL_W`（**112**）+ 一句"别写死 104" | **真源自己内部矛盾**：同一份文件 §8 登记表（`:535`）早就写着"`tokens.rs` = 112 ✅ 已落"，§7 那张表没跟上；常量注释里写着 112 是被"最长那条标签不断字"夹出来的 |
| `docs/UI-ROADMAP.md:4442` + `:7992` | 两处复述的 `112` 改成指路 `tokens::FORM_LABEL_W` | 本文件 §4 那条"数值不进路线表"对它自己同样成立：同一个数原本有 **4 个出处**（`tokens.rs:796` / `v4:535` / 路线表两处），是"改一处漏一处"的典型 |
| `AGENTS.md:281` 之后 | 给"一份报告不是闸"那条加 **2026-10-12 更正**：它举的例子 `scripts/verify_icon_sources.py` 已经修好（`:202 return 0 if not bad else 1` + `:206 sys.exit(main())`），现在是一颗真闸 | **自己核过**：`git log -S 'sys.exit(main())'` 查到那两行是 `c37bdd4`（2026-10-12「32 出口那一格…那份图标脚本原来根本不会红」）补的；留着旧措辞就是"拿一个已经修好的例子当现状" |
| `docs/UI-ROADMAP.md:2576` | 给"顺带加了一个工具 `scripts/scanrow.py`"加 **2026-10-12 更正**：那个工具从来没写完 | 见 §2 最后一条：文件是未闭合三引号的存根，跑不起来；文档在描述一个不存在的工具 |
| `scripts/README.md` | **新建**（107 行：八组 + 已知缺口） | `docs/DOCS.md` §6 原来登记这是缺口；35 个脚本名与目录逐字比对过（35 行 / 35 唯一 / 缺 0 多 0），判据列只许"真闸 / 只报告"两档 |
| `docs/DOCS.md` §1 + §6 | 清单表加 `scripts/README.md` 一行；§6 从"登记缺口"改成"已补 + 加脚本同轮动表 + 不搬目录" | 文件补上了，规范里那句"现状登记为缺口"就变成了假话 |

**改完的结构计数**（本项目的判据，见 `AGENTS.md` 目录规则那条 ⛔；只报绝对值无意义，
必须与 `HEAD` 并排比）：

| 文件 | 行数 工作树 / HEAD | 引用行 工作树 / HEAD | 反斜杠 工作树 / HEAD |
|---|---|---|---|
| `docs/UI-DESIGN.md` | **1459 / 1495**（-36） | 131 / 129（+2） | 65 / 65 |
| `docs/UI-DESIGN-v4.md` | **615 / 602**（+13） | 38 / 32（+6） | 4 / 4 |
| `docs/UI-ROADMAP.md` | **8044 / 8034**（+10） | 95 / 91（+4） | 146 / 146 |
| `AGENTS.md` | **1619 / 1607**（+12） | 0 / 0 | 39 / 39 |
| `README.md` | **147 / 144**（+3） | 0 / 0 | 8 / 8 |
| `gui/src/log.rs`、`gui/src/tokens.rs` | 911 / 911、1296 / 1296 | 0 / 0 | 2 / 2、7 / 7 |

**读法**（`AGENTS.md` 目录规则那条 ⛔ 要三个数，且明写"只报绝对值无意义，必须与 `HEAD` 并排比"）：
引用行 +N **恰好等于**新加的 `>` 行数、反斜杠逐一相等、`git diff --numstat` 的行数增减
与上表逐个吻合（`AGENTS.md` 16/9、`README.md` 5/3、`v4` 21/8、`UI-DESIGN` 21/57、`ROADMAP` 5/0；`gui/src/log.rs` 1/1 是纯注释替换，行数没动）
⇒ **没有发生 D92 那类"整篇被压平"的损坏**（那种损坏会让行数掉三位数以上）。
⚠️ 工作树里 `AGENTS.md` / `README.md` / `UI-DESIGN.md` 是 CRLF，另三份是 LF——
**行尾按文件不同**（与 `docs/CODE.md` §1 那条实测同源）；stderr 那句
"LF will be replaced by CRLF" 是 `core.autocrlf = true` 且无 `.gitattributes` 的正常现象，
`git diff --numstat` 不把行尾算成内容变化。

### 待拍（**没改**，因为"哪一份作准"属于用户的判断）

```
verify-first shrink AGENTS.md:456-458 抄了 v4 的弹簧表（v4 才是动效的家）. 证据: 同一张表两处；且 AGENTS.md:460「theme.json 没有 motion 段」与 v4:281 的更正并存
verify-first shrink docs/UI-DESIGN-v4.md 的悬停面与空态"还没有唯一出处". 证据: v4:404 自认悬停面现状 4 出处 3 档；v4:407 自认空态现状 3 种写法
verify-first shrink 跨文档拆分的整体方案（AGENTS.md 只留路由 + docs/PITFALLS.md + pitfalls/*.md）. 证据: AGENTS.md 1619 行里超过一半是 gpui 与发布踩坑史；**拆分要一次做完并逐条验证引用不断**，属于专门一轮，不在本次顺手做
verify-first complete-or-delete scripts/scanrow.py（见 §2 最后一条）. 证据: 要么补完（路线表 2576 记着它的用法与深色阈值口径），要么删掉，别停在"一段没闭合的三引号"
```

---

## 净账

**净可删: 约 -500 ~ -700 行（代码，其中纯删除 -250 行），-0 deps。**
另：文档侧在**已跟踪文件**上净 **+2 行**（`docs/UI-DESIGN.md` 1495 → 1459 = **-36**，
其余四份的更正与登记合计 **+38**：`AGENTS.md` +12、`v4` +13、`README.md` +3、
`ROADMAP.md` +10），并新建四份：`docs/CODE.md`（150 行）、`docs/DOCS.md`（100 行）、
本文件（154 行）、`scripts/README.md`（107 行）——**这四份是补缺口，不是臃肿**。

`deps` 之所以是 **-0**：core 6 个与 gui 6 个运行期依赖**逐个查过都有读者**，
删任何一个都要先删功能；`build-deps` 两个（`image` / `embed-resource`）都是构建期真实用途。
