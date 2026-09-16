# 分叉恢复向导 — 设计（2026-09-16）

> 对应 `docs/IMPROVEMENTS.md` 的 P0 #1（来源 FB-A1）。状态：**已实现**。
> 文末「实现与原设计的差异」记录了落地时改动的几处决定。

## 断点

`do_pull` 永远走 `git pull --ff-only`，分叉时（`ahead > 0 && behind > 0`）只返回一句话：

> 本地与云端历史分叉，ff 拉取失败，请手动处理

界面除了这句话没有任何出口，用户卡在"一键"哲学最后的断点上。

关键发现：`do_check` **本来就检测到了**这个状态，只是把结论混在普通提示里
（「云端有 N 个新提交，同时本地有 M 个未推送提交」），所以卡片不能只挂在"拉取失败"上。

## 触发时机（三处都落到同一张卡片）

| 场景 | ok | 来源 |
| --- | --- | --- |
| `check_update` 发现 `ahead>0 && behind>0` | true（检查本身没失败） | `do_check` |
| `pull --ff-only` 因分叉失败 | false | `do_pull` |
| `push` 因"云端有新提交"被拒 | false | `do_push` |

三处都返回 `OpResult.fork = Some(ForkInfo)`。分叉的判定用真实状态（`ahead`/`behind`），
**不去猜 stderr 里的措辞**——git 版本的提示语会变，但"两边各有对方没有的提交"这个事实不会。

## 数据契约

```rust
pub struct ForkInfo {
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub branch: String,
    pub upstream: String,             // 如 origin/main
    pub local_commits: Vec<String>,   // git log --oneline @{u}..HEAD（最多 20 条）
    pub remote_commits: Vec<String>,  // git log --oneline HEAD..@{u}（最多 20 条）
    pub blocked_reason: Option<String>, // 非空时卡片只说明原因、不给动作
}
```

`OpResult` 另有一个顶层字段 `backup_branch: Option<String>`：**只由变基动作返回**。
刻意不去磁盘上扫——扫描到的可能是很久以前的备份，那时"硬回退到备份"会丢掉用户后来提交的东西。
前端在推送成功或撤销成功时收起这个入口。

`blocked_reason` 的判定（`rebase-merge` / `rebase-apply` / `MERGE_HEAD` 任一存在）：
仓库正处于用户自己的变基或合并中途，向导一律不碰，只解释原因。

## 卡片

`src/components/panel/ForkCard.tsx`，位置在 `StashBar` 之后、`LogArea` 之前（即"日志区上方"）。

- 标题「本地与云端已分叉」
- 一行事实：本地 N 个提交未推送、云端 M 个新提交；工作区脏时补一句提示
- 两个可折叠列表：本地未推送的提交 / 云端新增的提交（这就是"只看差异"，不产生命令）
- 按钮：
  - **备份并变基到云端**（主按钮）：见下面的执行序列
  - **撤销变基**：仅当备份分支存在时出现，二次确认后 `reset --hard backup/qingbird-git`
- `blocked_reason` 非空时只显示原因，两个动作都不给

### 刻意不做

- 「本地领先 N、无分叉 → 直接推送」按钮：那种情况下界面上的「一键推送」本来就在，卡片里重复一次没有意义
- 自动 merge、force push（覆盖云端）、跨仓库批量处理

## 执行序列（备份并变基）

```
1. 前置检查：rebase/merge 中途 → 拒绝，什么都不做
2. git branch -f backup/qingbird-git HEAD      # 备份分支单一名，每次覆盖到"变基前"
3. 脏且用户选择暂存 → git stash push -u -m "qingbird-git: rebase 前自动暂存"
4. git pull --rebase --progress                # 走 run_net_op：直连→代理回退
5. 失败且确实进入了变基 → git rebase --abort    # 历史一动不动
6. 暂存过 → git stash pop                      # 冲突则如实报告，留给用户
```

- 备份分支用**单一名**而不是时间戳：避免 refs 越堆越多，用户在 git 里也看得见
- 第 5 步只在"确实处于变基中途"时 abort，不会误杀用户自己的变基
- 变基只可能是用户在卡片上的**显式动作**；默认拉取路径仍是 `--ff-only`，一字不改

## 可测性设计

变基序列是本功能风险最高的部分（改写历史），因此把它写成**注入 runner 的核心函数**：

```rust
type LocalRun<'a>  = &'a dyn Fn(&[&str]) -> git::GitOutput;
type RemoteRun<'a> = &'a dyn Fn(&[&str]) -> (git::GitOutput, Option<String>);

fn rebase_core(path: &str, before: &RepoStatus, stash: bool,
               local: LocalRun, remote: RemoteRun) -> RebaseOutcome
```

生产代码传入包装了 `run_local` / `run_net_op` 的闭包；单测传入直接调 `git::run_git` 的闭包，
在**临时目录里的真实仓库**（bare 远端 + 两个 clone）上跑真实 git，覆盖：

- 干净分叉：变基成功、`backup/qingbird-git` 指向变基前 HEAD、云端提交已并入
- 脏分叉 + 暂存：变基成功后改动回到工作区
- 真冲突：`rebase --abort` 生效，HEAD 与备份一致，无变基残留
- 备份分支命名与 `blocked_reason` 判定（纯函数）

## 前端接线

- `api.ts`：`ForkInfo` 类型、`OpResult.fork?`、`rebaseFork` / `undoRebase` 两个 invoke
- `store.ts`：`forks: Record<path, ForkInfo>`；check/pull 拿到 `r.fork` 就存，变基成功或撤销后清掉
- `src/lib/forkGuide.ts`：由 `ForkInfo` 推导"显示哪些按钮、文案怎么写"的纯逻辑，配
  `src/forkCard.test.mjs`（并入 `pnpm test:web`）
- 样式加在 `src/styles/panel.css`（已在 `@import` 链上，不新增文件）

## 收尾

- `AGENTS.md` 那行「拉取永远带 --ff-only，分叉时报错不自动 merge」补一句：变基仅限用户在卡片上显式触发
- `docs/IMPROVEMENTS.md` #1 标 ✅ 并写交付摘要；`docs/IMPROVEMENTS.md` 历史加一行

## 实现与原设计的差异

1. **卡片多了一种触发态**：原设计只考虑"分叉了"，实现后补上"刚变基完、备份还在"——
   此时已经不是分叉，但正是用户最可能后悔的时刻，卡片改成标题自适应（「上次变基的备份还在」），
   只给「撤销变基」。
2. **`backup_branch` 从 `ForkInfo` 挪到 `OpResult` 顶层**，并且只由变基动作写入（见上）。
   原设计想"扫磁盘上有没有备份分支"，那样会在重启后给出一个可能已经过期的撤销入口。
3. **`do_push` 也返回 fork 信息**：推送被拒（`fetch first` / `rejected`）本来就是"云端又前进了"，
   这时同一张卡片就是现成的出口，不必让用户再去点一次检测。
4. **提交列表各截断 20 条**：分叉几百个提交时不该把整页撑爆。
5. **测试用真实仓库而不是 mock**：`rebase_core` 注入 runner 的设计没有变，但单测直接在
   临时目录里建 bare 远端 + 两个 clone 跑真实 git（10 个用例），比断言"调用了哪些命令"更能说明问题。
   代价是这些用例必须持 `git::GIT_TEST_LOCK` 的读锁——`CANCELLED` 是进程级全局标记，
   取消测试一置位就会杀掉并行跑的其他测试的 git。
6. **前端多了一个 `forkCard.test.mjs`**（7 个用例）覆盖"该出哪些按钮"的纯逻辑。
