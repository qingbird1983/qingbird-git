# 改进计划 — qingbird-git

> 合并源：原 `IMPROVEMENTS.md`（工程质量 / 缺陷） + 已归档 `2026-09-14-feature-backlog.md`（产品演进）。
> 每项标注规模：S = 一天内、M = 两三天、L = 一周上下、H = 按需。
> 状态：✅ 已完成 / 🛠 进行中 / ⏳ 待办。

---

## 类型图例

- 🔧 **技术债**：不改会越来越痛（重构 / 测试 / 跨平台）
- ✨ **新功能**：用户价值
- 🐛 **缺陷**：已知坏掉但还没修的部分

---

## P0：核心同步链路闭环

### 1. ✅ ✨ 分叉恢复向导（M）【来源 FB-A1】

**已交付（2026-09-16）**：设计见 `docs/2026-09-16-fork-guide-design.md`。要点：

- **两处都能出卡片**（这是原提案没写、但实际最重要的发现）：`do_check` 其实早就检测到 `ahead>0 && behind>0`，
  只是把结论混在普通提示里；现在它与 `pull --ff-only` 失败、`push` 被拒三处都返回结构化 `OpResult.fork`，
  前端不做"消息里含不含『分叉』"这种字符串匹配
- 新增 `src-tauri/src/commands/fork.rs`：`ForkInfo` 组装（含本地未推送 / 云端新增两个提交列表，各取前 20 条）+
  `rebase_fork`（备份并变基）+ `undo_rebase`（撤销变基）；前端 `ForkCard` 坐在日志区上方，
  「只看差异」不出命令，只有「撤销变基」需要二次确认
- **安全设计**：每次变基前强制把"变基前"固定到 `backup/qingbird-git`；冲突自动 `rebase --abort` 复原，
  历史一动不动；仓库正处于用户自己的变基/合并中途时（`rebase-merge`/`rebase-apply`/`MERGE_HEAD` 存在）
  卡片只解释原因、一个动作都不给；备份分支名只由变基动作返回（不去磁盘上扫——扫描到的旧备份一旦"硬回退"
  会丢掉用户后来提交的东西），推送成功后收起撤销入口
- **可测性**：变基序列写成注入 runner 的 `rebase_core`，单测在临时目录的**真实仓库**（bare + 两个 clone）
  上跑真实 git，10 个用例覆盖：分叉判定、快进可拉取时不误报、干净变基后备份位置与重放顺序、
  脏工作区暂存与恢复、未同意暂存时原样不动、冲突中止复原（含"脏 + 冲突"最坏组合）、
  中途变基时拦下、撤销回到变基前
- 顺带修掉一个测试隔离缺陷：`CANCELLED` 是进程级全局标记，取消测试会杀掉并行跑的其他测试的 git
  （表现为"没有退出码、stderr 为空"），现在用 `git::GIT_TEST_LOCK` 读写锁隔开
- 验收：`cargo test` 34/34（新增 10 个）；`cargo check --all-targets` 零 warning；`pnpm test:web` 13/13；
  `pnpm build` 通过。**未做**：自动 merge、force push、跨仓库批量处理

**现状断点**：拉取永远 `--ff-only`，分叉时只报错，用户卡住没有出口——这是"一键"哲学最后的断点。

**方案**：拉取失败且检测到分叉时，操作日志区上方出一张错误卡片，给出安全出口：

| 场景检测 | 建议动作 |
| --- | --- |
| 本地领先 N 个提交、无分叉 | 「直接推送」按钮（等价 push） |
| 已分叉、本地干净 | 「rebase 到云端后快进」（`git pull --rebase`，失败可回滚） |
| 已分叉、本地脏 | 「先建备份分支（backup/<date>）再 rebase」 |
| 用户不想动 | 「只看差异说明」，展开 ahead/behind 提交列表 |

**实现要点**：`git.rs` 增加 `merge_base` / `rebase` 封装；复用 `changes[selected]` 的提交列表展示；所有动作前自动建 `backup/` 分支可一键回退。

---

### 2. ✅ ✨ 托盘系统通知（S，性价比最高）【来源 FB-A2】

**已交付（2026-09-16）**：新增 `src-tauri/src/notify.rs`，由 Rust 侧直接发 Windows 通知（不依赖前端存活）；
调度器每轮结束后按结果发——`behind`「云端有 N 个新提交，可手动拉取」、`pulled`「自动拉取成功（N 个新提交）」、
`skip`（本地脏跳过）、`error`（标题直接写「同步失败」，正文是失败原因）。两条不打扰规则：`ok`（已是最新 / 无上游）
不发；主窗口可见且聚焦时静默（关窗后 Webview 已销毁，正是最该通知的场景）。
设置弹窗「系统」区加总开关 `set_notify_auto_sync`（即时生效，**打开时立刻发一条示例通知**，不用等下一次自动同步就能确认系统弹不弹）。
配置项 `Settings.notify_auto_sync` 默认开；为让"存量配置缺字段"与"新建配置"结果一致，`Settings` 改为手写 `Default`。
接入 `tauri-plugin-notification`（Cargo 依赖 + `lib.rs` 注册）；只在 Rust 侧调用，前端不需要权限声明与 capability。
验收：`cargo test` 24/24（新增 5 个 `toast_for` 文案与截断单测）；`cargo check --all-targets` 零 warning
（顺带清掉 `scheduler.rs` 测试里一个从未被使用的 `entry` 辅助函数）；`pnpm test:web` 6/6；`pnpm build` 通过。

**与原提案的差异**：不做"点击通知唤起主窗口并定位到该仓库"——`tauri-plugin-notification` 桌面端未暴露通知点击/激活事件
（Windows 侧要注册 COM 激活器，等于把应用做成 MSIX 包），本期通知只承担"告知"。免打扰时段同样留作二期。
**Windows 注意**：`pnpm tauri dev` 下通知来源显示成 PowerShell；NSIS 安装包会创建带 AppUserModelID 的快捷方式
（已在 `installer.nsi` 确认 `SetLnkAppUserModelId`），装好后才显示本应用名称与图标。

**原方案**：事件同时发 Windows 系统通知（tauri-plugin-notification）：

- 「xxx 有 3 个新提交」→ 点击唤起主窗口并定位到该仓库
- 「xxx 已自动同步完成」/「同步失败：…」按 `last_auto_result` 汇总
- 设置弹窗加总开关 + 免打扰时段（可选，二期）

**实现要点**：Rust 侧 scheduler 直接发通知（不依赖前端存活）；点击通知跳转需要 notification 插件的 click 事件 + 唤窗逻辑。

---

### 3. ✅ ✨ 批量勾选拉取（M）【来源 FB-A3】

**已交付（2026-09-28）**：形状改了——没做"每行复选框 + 拉取所选 (n)"，
改成**总览页「待处理」卡头部的「全部拉取 N」/「全部推送 M」**两枚按钮（按动作分开，
不合并成一条"全部处理"：用户点之前就该知道接下来是拉还是推）。批次期间卡头换成
`拉取中 2/5` + 「停止」，跑完一句汇总记全局日志。

- **没有新增后端 command**：`runBatch` 就是前端串行循环现有的 `pullFor` / `pushFor`。
  代理回退、分叉识别、脏工作区判定、进度事件全在那两条路径里，
  再写一个 `pull_many` 只会让批量与单个的行为随时间漂移
- **批量不替用户做决定**：工作区有改动的仓库**跳过 + 记一行日志 + 继续下一个**，
  不自动 stash——总览页上没人能逐个确认暂存，替他暂存风险太大
- **分叉与失联不进队列**（`batchPlan` 只收行内动作与批次一致的行）：
  一键改写历史或去动一个不存在的目录，都不该由一个按钮顺带完成
- 后端 `OP_LOCK` 本来就只允许一个网络操作；批次期间 `busy` 始终非空
  （`pullFor`/`pushFor` 是在同一调用栈里"设 busy → await → 清 busy"），
  所以不用给批处理单开互斥令牌。停止标记放 store 模块级而不是 state：
  每跑完一项就整体覆写 `batch`，state 里的 `stopped` 会被下一次 set 抹掉
- 验收：`pnpm test:web` 68/68（新增 9 个：`batchPlan` 只收同类行且排除分叉/失联、
  队列顺序沿用待处理排序、无对应行返回 null、按钮文案带数量、
  `batchSummary` 只在非零时报告跳过与失败、跳过原因跟动作走、停止文案、停在最后一个上算完成）

**原方案**：仓库列表每行加复选框（或 Shift 连选），头部出现「拉取所选 (n)」；后端把 `pull_repo` 排队逐个执行（复用 `scan_all` 的串行队列 + 互斥锁模式），日志区按仓库分组输出。

---

## P1：工程基线（防以后改不动）

### 4. 🛠 🔧 前端单元测试（H）【来源 IMP-1】

**已交付（2026-09-16，部分）**：没有引入 vitest——改用 node 自带的 `node --test` + 原生 TS 类型擦除跑
`src/*.test.mjs`（命令 `pnpm test:web`），零新增依赖。
当前只覆盖运行命令面板的保存/预填纯逻辑（`src/repoCmds.ts`，含"改一行不串改另外三行"的回归）；
**本条的核心目标（`store.ts` 状态流转覆盖 ≥ 80%）尚未开始**，仍是待办。

**问题**：项目目前完全没有前端单测，所有逻辑依赖手动验证，回归风险高。Zustand store（`src/store.ts`）包含复杂的状态机（busy/cloning 互斥、代理回退 Toast 计时、日志折叠等），最容易引入隐性 Bug。

**目标**：覆盖 `store.ts` 核心状态流转与工具函数 + `api.ts` 类型不退化 + 测试框架与 Vite 无缝集成。

**实现**：

| 步骤 | 内容 |
|------|------|
| 1 | 引入 `vitest`（已与 Vite 原生集成，无需额外配置） |
| 2 | 在 `src/` 下建立 `__tests__/` 目录 |
| 3 | 先写 `store.test.ts`：测试 `sortRepos`、`applyThemeClass`、`autoSyncLabel` 等纯函数；再用 `vi.mock` 伪造 `api` 模块后测试异步 action |
| 4 | 覆盖边界：busy 时拦截新操作、Toast 自动消失定时器、日志折叠 `\r` 规则 |
| 5 | 在 `package.json` scripts 中加 `"test"` 和 `"test:watch"` |

**验收**：`pnpm test` 全过；`src/store.ts` 行覆盖 ≥ 80%。

---

### 5. ✅ 🔧 commands.rs 模块化拆分（M）【来源 IMP-2】

**已交付（2026-09-16）**：`commands.rs`（2686 行）已删除，改为 `src-tauri/src/commands/` 目录，
`mod.rs` 79 行（< 100 行验收线）+ 10 个领域模块：`types` / `net`(579) / `repos`(434) / `sync`(395) /
`run_cmd`(423) / `create`(285) / `clone`(149) / `settings`(87) / `shell`(132) / `space`(148)。
未采用下面提案的 `cmd_*.rs` 扁平命名，改用领域名 + 目录模块（`commands::repos::list_repos`），
`mod.rs` 用 `pub use <模块>::*` 保持 `commands::*` 路径不变，因此 **`lib.rs` 的 `generate_handler!`
与 `scheduler.rs` 零改动**，IPC 名称与行为均无变化；9 个单测按被测函数归入各模块的 `#[cfg(test)]`。
验收：`cargo check` 零 warning + `cargo test` 19/19 通过；另做了逐项纯搬移比对（100 个顶层项零缺失）。

**问题**：`src-tauri/src/commands.rs` 约 2000 行，承载所有 Tauri command，阅读和维护成本随功能增长持续上升。

**目标**：按职责拆分为多个子模块文件；公共函数（`run_net_op` / `do_pull` / `do_check` 等）仍集中在易发现位置；保持现有 Tauri handler 注册不变。

**方案结构**：

```
src-tauri/src/
├── commands.rs          ← 仅保留 generate_handler! 注册 + 模块声明
├── cmd_repo.rs          ← list_repos / add_repo / remove_repo / rename_repo / pin_repo / set_repo_cmds / set_auto_sync
├── cmd_net.rs           ← run_net_op / do_check / do_pull / do_push / clone_repo / commit_push / check_update / cancel_busy_op
├── cmd_local.rs         ← repo_status / list_changes / recent_commits / repo_languages
├── cmd_init.rs          ← init_repo / list_dir_items / create_repo
├── cmd_settings.rs      ← get_settings / set_settings / set_theme / get_credentials / set_credentials / probe_proxy
├── cmd_ops.rs           ← open_in_folder / open_in_terminal / open_readme / open_repo_website
├── cmd_cmd.rs           ← run_repo_cmd / stop_repo_cmd / set_repo_cmds / detect_cmds
├── cmd_space.rs         ← scan_repo_space / delete_cache_dir
└── cmd_scan.rs          ← scan_all
```

**验收**：`cargo check` + `cargo test` 全过；`commands.rs` 压缩至 < 100 行；无公开 API 行为变化。

---

### 6. 🔧 macOS / Linux 孤儿进程回收（M）【来源 IMP-3】

**问题**：`git.rs` 中 Windows 专属的 Job Object 整树回收在非 Windows 平台是空实现：

```rust
#[cfg(not(windows))]
pub(crate) fn terminate_job_tree(_job: isize) {}
```

取消/超时后，`git remote-https` / `index-pack` 等孙进程会成为孤儿继续消耗资源，且目录无法删除（macOS/Linux 上常见）。

**方案 A（推荐）**：SIGTERM 进程组——子进程创建时 `setpgid` 加入独立进程组；取消时向整个进程组发 `SIGTERM`（负 PID 表示进程组）；需引入 `libc` crate（Windows 用条件编译隔离）。

**方案 B**：子进程 PID 列表追踪——不依赖进程组，用 `std::process::Child` 记录孙进程 PID，逐个 kill。更精细但更复杂。

**验收**：macOS/Linux 上取消长时间 fetch 后 `ps aux | grep git` 不再残留；Windows 行为不受影响。

---

## P2：体验延伸（先 S 后 M）

### 7. 🛠 ✨ 容量面板扩展（S×3，可拆着做）【来源 FB-B1】

**已交付（2026-09-16）**：前两项做完，占用趋势（第三项）留作二期。

- **大文件 Top 10**：`scan_repo_space` 现在一次遍历里同时算体积与收集大文件
  （为找大文件再走一遍树会让大仓库扫描时间翻倍）。刻意**排除 `.git` 与缓存目录**——
  它们的体积已经在上面按目录列出来了，混进榜单只会把真正该处理的工作区文件挤下去。
  阈值 10 MB、最多 10 条；行内显示文件名 + 所在目录（长路径截断时丢掉的是不重要的部分），
  点击打开所在文件夹（后端直接返回父目录，避免前端拆路径）
- **.gitignore 提示**：扫描时顺带用 `git check-ignore` 找出"存在于白名单（`CACHE_DIRS`）但没被忽略"
  的缓存目录，面板给出「加入 .gitignore」按钮（两步确认，因为是改用户仓库里的文件）。
  **额外查了一次"是否已被跟踪"**：已被跟踪的目录写 `.gitignore` 是无效的，
  标成「已被跟踪」并提示 `git rm -r --cached`，不让按钮给人"已经修好了"的错觉。
  写入只追加（保留原有规则，缺尾换行会先补上），且只接受白名单内的顶层目录名——
  这个命令会写用户的仓库文件，不能变成"前端能往 .gitignore 里写任意内容"的通道
- 验收：`cargo test` 42/42（新增 8 个：大文件收集与排序上限、非仓库不查 git、
  未跟踪目录被建议→写入→不再提示、已跟踪目录被标记、白名单外条目被拒且不落盘、
  原有内容与缺尾换行两种 .gitignore 边界）；`pnpm build` 通过

**原方案**：后端 `scan_repo_space` 已在遍历目录，顺手扩展：

- **大文件 Top 10**：扫描时顺带记录最大的文件（>10MB），列表展示 + 双击打开所在文件夹
- **.gitignore 提示**：发现 `node_modules` / `dist` / `build` 等目录存在但未被忽略时，给「加入 .gitignore」一键按钮
- **占用趋势**（二期）：每次扫描结果按日期存 localStorage，画迷你趋势线，直观看到哪个仓库在膨胀

---

### 8. ✅ ✨ 提交历史看 diff（M）【来源 FB-B2】

**已交付（2026-09-16）**：新增 `commands/history.rs`（`commit_detail` 文件级摘要 + `commit_patch` 单文件 patch），
HistoryModal 改成左列提交、右列明细（弹窗加宽到 900px）。

- **合并提交是这里最容易踩的坑**：`git show <merge>` 默认什么都不输出（只有 `-m` / `--cc` 才给 diff），
  不处理的话点开合并提交就是一片空白。所以先看父提交个数，合并提交改走 `git diff <hash>^ <hash>`
  （相对第一个父提交看变更）——这也是各类 Git 客户端"点开合并提交"的做法。单测专门锁了这条
- 文件列表带 `+N −M`，二进制文件标「二进制」而不是 `+0 −0`
- patch 按增 / 删 / 块头 / 元信息 / 上下文逐行上色（`patchLineKind`）；
  顺序上文件头 `+++`/`---` 必须优先于增删行判断，否则会被染成增删色
- **两处截断都给出真出口**：后端正文上限 200k 字符、界面只渲染前 1500 行（几千个 div 足够卡住 webview），
  任一触发就出现「在终端看完整 diff」——在仓库目录打开终端并执行同一条查看命令。
  为此把 `shell.rs` 里的终端启动抽成 `spawn_terminal(path, command)`，`open_in_terminal` 也改走它
- **安全**：hash 只接受十六进制（4–64 位）；文件路径拼进终端命令前对单引号做 PowerShell 转义
  并拒绝控制字符——这条命令的文件名来自界面，不能直接裸拼进 shell
- 验收：`cargo test` 54/54（新增 7 个：numstat 解析含二进制与中文路径、按字符截断、
  hash 与终端转义校验、普通提交的摘要与 patch、**合并提交相对第一个父提交**、未知提交报错不 panic）；
  `pnpm test:web` 21/21（新增 3 个）；`cargo check --all-targets` 零 warning；`pnpm build` 通过

**原方案**：HistoryModal 现在只有提交列表。点击一条 → `git show <hash> --stat` 展示文件级变更摘要，再点文件看 patch（`git show <hash> -- <path>`）。注意大 diff 截断展示 + 「用外部工具打开」兜底。

---

### 9. ✅ ✨ 自定义命令按钮（S）【来源 FB-B3】

**已交付（2026-09-16）**：

- `RepoEntry` 加 `pinned_cmds: Vec<PinnedCmd>`（`{label, cmd}`）+ `set_pinned_cmds` 命令。
  上限 3 个，**超了直接报错而不是静默截断**——用户以为钉上了却没出现，比报错更糟
- 编辑过程中留的空行直接丢掉（那是在填的正常中间态），文案留空时用命令前两个词兜底
  （中文按字符截断，不会切出半个字）
- 前端把"列表编辑 + 落盘前整理"的规则单独抽到 `src/lib/pinnedCmds.ts`（上限 / 空行 / 文案兜底），
  与后端 `sanitize_pinned` / `derive_pinned_label` 同一套规则，配 `src/pinnedCmds.test.mjs`
- `CmdPanel` 下半部分加编辑器（文案 + 命令 + 移除 / 添加 / 保存），`RepoPanel` 操作区渲染钉住的按钮；
  点击跑的是与「跑测试」同一条后台管道（`run_repo_cmd`），输出照旧实时进日志窗口
- `.actions` 改成可换行、`.action-btn` 给了 108px 下限：钉满 3 个时共 8 个按钮换到第二行，
  窄窗口下也不会被压成挤在一起的窄条
- 踩到并规避了一个 React 陷阱：`entry?.pinned_cmds ?? []` 里新建的 `[]` 每次渲染都是新引用，
  当作 `useEffect` 依赖会让 effect 每轮重跑（`RepoPanel` 的 `EMPTY_LOGS` 已有同类注释）。
  所以刻意**没有**提供 `pinnedFromEntry()` 这类包一层 `?? []` 的帮手，改由调用方用模块级常量
- 验收：`cargo test` 45/45（新增 3 个：文案兜底与按字符截断、超上限报错、空行丢弃与去空白）；
  `pnpm test:web` 18/18（新增 6 个）；`cargo check --all-targets` 零 warning；`pnpm build` 通过

**原方案**：运行命令面板已支持 test/build/dev 检测与自定义保存。扩展：允许把任意命令"钉"成操作区按钮（上限 3 个），点击即跑并流式写日志——`run_repo_cmd` 管道现成，纯前端 + 一列配置字段。

---

### 10. ✅ 🔧 自动同步跨天漏跑修复（L）【来源 IMP-4】

**已交付（2026-09-16）**：根因比原描述更简单——`daily_is_due` 只盯**今天**那个时刻
（`now >= 今天的时刻 && last_check < 今天的时刻`），于是"错过的那一天"根本没有进入判断：
09-14 09:30 该跑时机器关着，09-15 08:00 开机时 `now < 今天的 09:30` → 不触发，
09-14 一整天没同步，还得干等到 09-15 09:30。

改成回看**最近一个已经到点的执行时刻**（`last_due_moment`：今天过了就是今天，没过就是昨天），
`last_check` 没覆盖它就该跑。于是开机那一轮就会补上，之后照常按点跑。

- **防抖靠的就是 `last_auto_check`**：补跑会把它刷到当前时刻，而它必然大于"昨天那个时刻"，
  所以连续关机几天也只补一次，不会攒成一串。**原方案里提的 `last_daily_run_date` 字段因此没有加**——
  再加一个时间戳只会和 `last_auto_check` 表达同一件事，多一处要对齐的状态
- 昨天按"日期减一天再本地化"算，而不是给 `DateTime` 减 24 小时：后者遇到夏令时切换会整体偏一小时
- 验收：`cargo test` 47/47。新增/改写 3 个用例（全部用固定 +08:00 时区，不依赖跑测试那台机器）：
  错过昨天那次 → 开机补跑、昨天那次跑过 → 不多跑、刚好到点 → 该跑、补完不再重复、
  连续关一周也只补一次、以及 `last_due_moment` 的跨月边界（9 月 1 日没到点要落到 8 月 31 日）

**原描述**：`scheduler.rs` 的 `daily_is_due` 只检查"今天是否已跑"，若程序在目标时间点后关闭，下次开机可能不会补跑。

---

## P3：长期演进（装机量 / 用户反馈起来再排）

### 11. ✨ 配置导出 / 导入（S）【来源 FB-C1】

设置弹窗加「导出配置 / 导入配置」：仓库路径列表 + 主题 + 定时设置打包成一个 JSON 文件。**明确不导出令牌与私钥路径**（安全红线）。换机场景：新机器装好 → 导入 → 按列表逐个提示「路径不存在，选择新位置或克隆」。

---

### 12. ✨ 仓库标签 / 分组（M）【来源 FB-C2】

`RepoEntry` 加 `tags: Vec<String>`，列表支持按标签筛选（标签栏放在搜索框下）；与置顶叠加排序。适合仓库 15+ 以后再做。

---

### 13. ✨ 应用自更新（S）【来源 FB-C3】

已出 NSIS 安装包，接 `tauri-plugin-updater`：GitHub Releases 托管 `latest.json` + 安装包，启动时静默检查、发现新版提示跳转下载（签名配置好后也可直接内更新）。

---

### 14. ✨ 全局快捷键呼出（S）【来源 FB-C4】

`tauri-plugin-global-shortcut`，默认 `Alt+Shift+G` 唤起/隐藏主窗口，设置里可改。

---

### 15. 🔧 SSH 私钥集成系统密钥存储（L）【来源 IMP-5】

**问题**：目前 SSH 私钥路径以明文存储在 `config.json` 中，路径本身不敏感但暴露了私钥位置。

**目标**：Windows 上尝试使用 Windows Credential Manager 或 DPAPI 加密存储 SSH 密钥路径；保持向后兼容（旧配置明文路径仍可读取）。

**实现要点**：新增 `ssh_key_encrypted: Option<Vec<u8>>` 字段（DPAPI 加密后的字节）；保存时 `CryptProtectData`；读取时优先读加密字段，解密失败时降级读旧明文字段。

**注意**：Windows API 绑定需额外依赖，macOS/Linux 保持现有行为；建议作为独立 PR。

---

## 执行顺序建议

按"先见肉、后加固、再拓展"的节奏：

| 轮次 | 优先级 | 目标 | 推荐起点 |
|------|--------|------|---------|
| **轮 1 ✅ 已完成** | P0 | 见肉 + 闭环核心链路 | #2 托盘通知（S）+ #1 分叉向导（M） |
| **轮 2（#5 已完成）** | P1 | 防以后改不动 | #4 前端单测（H）+ #5 commands 拆分（M） |
| **轮 3 ✅ 已完成** | P2 上半 | 体验延伸 | #7 容量扩展 #9 自定义按钮（都是 S） |
| **轮 4 ✅ 已完成** | P2 下半 | 体验延伸 + 小缺陷 | #8 diff + #10 漏跑修复 |
| **轮 5（建议下一步）** | P1 跨平台 | 修跨平台缺陷 | #6 孤儿进程回收 |
| 轮 6+ | P3 | 装机量起来再做 | 增量推进 |

### 下一步建议

轮 1、轮 3、轮 4 已完成，**轮 2 只剩 #4**（`store.ts` 状态流转覆盖 ≥ 80%）。
注意本文件里给的老方案是"引入 vitest"：2026-09-16 已改用 node 自带 `node --test` +
原生 TS 类型擦除（零新增依赖），继续沿用即可——把纯逻辑继续往 `src/lib/` 抽
（`forkGuide.ts`、`pinnedCmds.ts`、`commitDiff.ts`、`overview.ts` 都是这么做的，共 68 个用例），
比为了覆盖率去 mock 一个 500 行的 zustand store 更划算。

**轮 5** 是 #6 macOS / Linux 孤儿进程回收：Windows 靠 Job Object 整树回收，
非 Windows 平台是空实现（`terminate_job_tree` 什么都不做），取消/超时后
`git remote-https` / `index-pack` 会变孤儿。方案 A（进程组 + SIGTERM，需引入 `libc`）
更简单；这条只有在非 Windows 平台上才能真验证，**当前环境（Windows）做不了端到端验收**，
要么只写实现 + 单测，要么等有 macOS/Linux 环境再做。

**轮 6+** 是 P3（配置导出/导入、仓库标签、应用自更新、全局快捷键、SSH 私钥进系统密钥库），
其中 #11 配置导出导入与 #14 全局快捷键都是 S 级，装机量起来后可以随手做。

完成后在本文件对应条目加 ✅，并写一行简短的交付摘要。

---

## 历史

- **2026-09-28**：#3 批量勾选拉取完成，但换了形状——没做仓库列表的复选框，
  改成总览页「待处理」卡头部的「全部拉取 N」/「全部推送 M」（UI 重构阶段 4 收尾时补的，
  路线图里一直挂着这项）。不新增后端 command：前端串行循环现有 `pullFor` / `pushFor`，
  代理回退与分叉识别只此一份；脏工作区跳过并记日志、不自动 stash、不中断整批；
  分叉与失联不进队列。`pnpm test:web` 68/68、`cargo test` 57/57、`pnpm build` 通过
- **2026-09-16（深夜 5）**：**轮 4 收官**。#8 提交历史看 diff 完成——新增 `commands/history.rs`
  （文件级摘要 + 单文件 patch，合并提交按第一个父提交算），HistoryModal 改成左列提交右列明细，
  diff 过长时给「在终端看完整 diff」的出口（`shell.rs` 抽出 `spawn_terminal`）。
  `cargo test` 54/54、`pnpm test:web` 21/21
- **2026-09-16（深夜 4）**：#10 自动同步跨天漏跑修复完成——`daily_is_due` 改为回看
  「最近一个已到点的执行时刻」，错过某天、第二天在时刻之前开机时会补跑一次；
  防抖靠 `last_auto_check` 本身，原方案里的 `last_daily_run_date` 字段确认不需要。
  `cargo test` 47/47
- **2026-09-16（深夜 3）**：**轮 3 收官**。#9 自定义命令按钮完成——`pinned_cmds` 配置字段 +
  `set_pinned_cmds`（上限 3 个，超了报错不静默截断）、`CmdPanel` 编辑器与 `RepoPanel` 操作区按钮，
  列表编辑规则抽到 `lib/pinnedCmds.ts` 并与后端同一套（`pnpm test:web` 18/18）；
  操作区改为可换行以容纳钉住的按钮
- **2026-09-16（深夜 2）**：**轮 3 开工**。#7 容量面板扩展前两项交付——大文件 Top 10
  （排除 `.git` 与缓存目录，点击打开所在文件夹）与 .gitignore 建议
  （`git check-ignore` 判定 + 已跟踪目录单独标记 + 白名单校验的追加写入），
  `cargo test` 42/42
- **2026-09-16（深夜）**：**轮 1 收官**。#1 分叉恢复向导完成——新增 `fork.rs`（`ForkInfo` + 备份并变基 + 撤销变基）
  与 `ForkCard`，检测/拉取/推送三处失败都能落到同一张卡片；每次变基强制先建 `backup/qingbird-git`、
  冲突自动中止复原，10 个用例在临时真实仓库上验证；顺带修掉取消测试会杀掉并行测试 git 的隔离缺陷。
  #2 托盘系统通知完成——新增 `notify.rs` 由 Rust 侧直接发通知（不依赖前端存活），
  `ok` 结果与"主窗口在前台"两种情况静默，设置弹窗加总开关且打开时发示例通知便于当场验证
- **2026-09-16**：#5 `commands.rs` 模块化拆分完成（原文件 2686 行 → `commands/` 10 个领域模块），
  `App.css`（2040 行）按域拆到 `src/styles/`、`store.ts` 抽出 `src/lib/`、面板浮层抽出子组件；
  #4 部分交付（`pnpm test:web` 零依赖测试跑起来，store 覆盖率未做）；
  另修复运行命令面板「保存测试会串改构建」的缺陷，并让开发/安装依赖可自定义保存
- **2026-09-15**：合并原 `IMPROVEMENTS.md`（5 项技术债/缺陷） + `feature-backlog.md`（10 项新功能/构想），统一为 P0–P3 共 15 项。原 `feature-backlog.md` 已归档到 `docs/archive/`。
- **2026-09-14**：内存优化已实现（关闭窗口销毁 Webview，托盘常驻仅保留 Rust 后端），见 `lib.rs` 的 `show_main` / `RunEvent::ExitRequested`。