"""32-12 的前置实测：状态检测合并成一条命令，到底省多少、还剩几条。

登记原文自己写着一条纪律——**"省多少毫秒我没量，落之前要在 `$TEMP` 合成仓库上跑
冷启动实测，别把命令语义当性能结论"**。这份脚本就是那条前置。

现场核出来的三条与登记不同，先记在这里（跑之前读代码读出来的，不是猜的）：
① `valid_repo` 是 `Path::join(".git").exists()`——**fs 检查，不是子进程**，
   登记把它算进了那个"6 次"；
② 32-2 之后 `read_status` 末尾多了一条 `git stash list`（那颗感叹号的数与清单
   同一个出处），登记写于它之前；
③  **`status --porcelain=v2 --branch --ahead-behind` 给不出 stash 数，
   也给不出 `remote get-url origin`** ⇒ "6 → 1" 这个设想在最乐观处也是 **6 → 3**。

所以这里量三个数，不是一句"合并会更快"：
  A = 现状那组（逐条，口径照 `git -C <path> …`）
  B = 合并候选那一条
  C = 合并之后仍然必须跑的两条（stash list + remote get-url origin）
⇒ 省下的是 A − (B + C)，进程数从 6 降到 3。

用法：
  python target/measure_status_cost.py make     # 造三颗夹具（含一颗 2 万文件的）
  python target/measure_status_cost.py time     # 跑计时
  python target/measure_status_cost.py clean
"""

import pathlib
import random
import shutil
import statistics
import subprocess
import sys
import time

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

TEMP = pathlib.Path("C:/Users/42527/AppData/Local/Temp")
ROOT = TEMP / "qbg3212-repos"
ROUNDS = 15
BIG_FILES = 20_000
BIG_DIRTY = 300


def sh(cwd, *args, check=True):
    r = subprocess.run(
        ["git", "-C", str(cwd), *args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if check and r.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} 失败：{r.stderr[:300]}")
    return r


def rm_tree(p):
    """**不许 ignore_errors**：夹具清不干净就会拿到旧证据（AGENTS 32-10 那条）"""
    if not p.exists():
        return
    for root, files in os_walk(p):
        for name in files:
            try:
                (root / name).chmod(0o777)
            except OSError:
                pass
    shutil.rmtree(p)
    if p.exists():
        raise RuntimeError(f"删不掉 {p}")


def os_walk(p):
    import os

    for root, _dirs, files in os.walk(p):
        yield pathlib.Path(root), files


def make_repo_with_remote(name, big):
    """本地 bare 仓库当远端（不联网、不碰用户任何远端）"""
    rm_tree(ROOT / name)
    rm_tree(ROOT / f"{name}-remote")
    bare = ROOT / f"{name}-remote"
    bare.mkdir(parents=True)
    sh(bare, "init", "--bare", "-q", "--initial-branch=main")
    work = ROOT / name
    sh(bare.parent, "clone", "-q", str(bare), str(work))
    sh(work, "config", "user.email", "t@t")
    sh(work, "config", "user.name", "t")
    if big:
        for i in range(BIG_FILES):
            d = work / f"d{i % 50}"
            d.mkdir(parents=True, exist_ok=True)
            (d / f"f{i}.js").write_text(f"export const x = {i};\n", encoding="utf-8")
    else:
        (work / "a.txt").write_text("x\n", encoding="utf-8")
    sh(work, "add", "-A")
    sh(work, "commit", "-q", "-m", "base")
    sh(work, "push", "-q", "-u", "origin", "main")
    if big:
        # 脏：改一批 + 造一批未跟踪，让 `status` 那一趟有活干
        for i in range(500):
            (work / f"d{i % 50}" / f"f{i}.js").write_text(
                f"export const x = {i} + 1;\n", encoding="utf-8"
            )
        for i in range(BIG_DIRTY):
            (work / "untracked").mkdir(exist_ok=True)
            (work / "untracked" / f"u{i}.tmp").write_text("y\n", encoding="utf-8")
    return work


def make_repo_no_remote(name):
    rm_tree(ROOT / name)
    p = ROOT / name
    p.mkdir(parents=True)
    sh(p, "init", "-q", "--initial-branch=main")
    sh(p, "config", "user.email", "t@t")
    sh(p, "config", "user.name", "t")
    (p / "a.txt").write_text("x\n", encoding="utf-8")
    sh(p, "add", "a.txt")
    sh(p, "commit", "-q", "-m", "base")
    return p


def timed(path, args):
    t0 = time.perf_counter()
    r = sh(path, *args, check=False)
    return (time.perf_counter() - t0) * 1000.0, r


# 现状那组：照 read_status 的构成逐条列（有上游的那一颗）
CURRENT = [
    ["rev-parse", "--abbrev-ref", "HEAD"],
    ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    ["rev-list", "--left-right", "--count", "HEAD...@{u}"],
    ["status", "--porcelain"],
    ["remote", "get-url", "origin"],
    ["stash", "list", "--format=%gd%x09%gs%x09%ci"],
]
MERGED = ["status", "--porcelain=v2", "--branch", "--ahead-behind"]
STILL_NEEDED = [
    ["remote", "get-url", "origin"],
    ["stash", "list", "--format=%gd%x09%gs%x09%ci"],
]


def bench(path, label):
    print(f"\n== {label}  ({path.name}) ==")
    for _ in range(2):  # 预热：第一轮含磁盘冷读，不算进中位数
        sh(path, *CURRENT[3], check=False)
    a_total, b_total, c_total = [], [], []
    for _ in range(ROUNDS):
        a = 0.0
        for args in CURRENT:
            ms, _r = timed(path, args)
            a += ms
        a_total.append(a)
        ms, r = timed(path, MERGED)
        b_total.append(ms)
        merged_ok = r.returncode == 0
        c = 0.0
        for args in STILL_NEEDED:
            ms, _r = timed(path, args)
            c += ms
        c_total.append(c)
    per = []
    for args in CURRENT:
        xs = [timed(path, args)[0] for _ in range(5)]
        per.append((statistics.median(xs), " ".join(args)))
    for ms, args in sorted(per, reverse=True):
        print(f"   单条  {ms:7.1f} ms  git {args}")
    print(f"   A 现状整组    中位 {statistics.median(a_total):7.1f} ms（{len(CURRENT)} 条子进程）")
    print(f"   B 合并那一条  中位 {statistics.median(b_total):7.1f} ms  成功={merged_ok}")
    print(f"   C 合并后仍需  中位 {statistics.median(c_total):7.1f} ms（{len(STILL_NEEDED)} 条）")
    print(f"   ⇒ 净省 {statistics.median(a_total) - statistics.median(b_total) - statistics.median(c_total):.1f} ms，进程数 {len(CURRENT)} → {1 + len(STILL_NEEDED)}")


def semantics():
    """合并那条命令的语义核对：给不给 branch/upstream/ab/变更行，非仓库是不是直接 fatal"""
    print("\n== 语义核对（决定「能不能替」，比快慢更前置）==")
    p = ROOT / "small"
    r = sh(p, *MERGED, check=False)
    head = [ln for ln in r.stdout.splitlines() if ln.startswith("# branch")][:6]
    for ln in head:
        print("   ", ln)
    changed = [ln for ln in r.stdout.splitlines() if ln.startswith("?") or ln.startswith("1 ")]
    print(f"    变更行数（含未跟踪）= {len(r.stdout.splitlines()) - len(head)}")
    print(f"    样例：{changed[0][:60] if changed else '（干净）'}")
    # 非仓库目录：退出码要能替代 valid_repo
    notrepo = ROOT / "not-a-repo"
    notrepo.mkdir(exist_ok=True)
    r2 = sh(notrepo, *MERGED, check=False)
    print(f"    非仓库：退出码 {r2.returncode}，stderr 首行 {r2.stderr.splitlines()[0][:60] if r2.stderr else ''}")
    # 无上游：--ahead-behind 那一档给什么
    r3 = sh(ROOT / "noup", *MERGED, check=False)
    ab = [ln for ln in r3.stdout.splitlines() if "branch.ab" in ln or "branch.upstream" in ln]
    print(f"    无上游仓库：branch 段 = {ab or '（没有这两行）'}")


def make_repo_diverged(name, local=1):
    """分叉夹具：远端多 2 笔、本地多 `local` 笔 ⇒ 期望 ahead=`local` / behind=2。
    **这一档是 32-12 的生死判据**：两种读法的正负号方向必须当场对过一次，
    对错了界面会把「领先」与「落后」说反，而 check 零错、单测全绿。
    跑两颗（local=1 与 local=0）：**只有两档不对称，把两边写反才会被抓住**——
    1/2 写反成 2/1 一眼能看，而 0/2 写反成 2/0 是"落后说成领先"，症状更坏"""
    rm_tree(ROOT / name)
    rm_tree(ROOT / f"{name}-remote")
    bare = ROOT / f"{name}-remote"
    bare.mkdir(parents=True)
    sh(bare, "init", "--bare", "-q", "--initial-branch=main")
    a = ROOT / f"{name}-a"
    b = ROOT / name
    rm_tree(a)
    sh(bare.parent, "clone", "-q", str(bare), str(a))
    sh(a, "config", "user.email", "t@t")
    sh(a, "config", "user.name", "t")
    (a / "seed.txt").write_text("s\n", encoding="utf-8")
    sh(a, "add", "-A")
    sh(a, "commit", "-q", "-m", "seed")
    sh(a, "push", "-q", "-u", "origin", "main")
    sh(bare.parent, "clone", "-q", str(bare), str(b))
    sh(b, "config", "user.email", "t@t")
    sh(b, "config", "user.name", "t")
    # 远端那 2 笔由 a 推上去
    for i in range(2):
        (a / f"r{i}.txt").write_text("r\n", encoding="utf-8")
        sh(a, "add", "-A")
        sh(a, "commit", "-q", "-m", f"remote {i}")
    sh(a, "push", "-q", "origin", "main")
    # ⚠️ **必须 fetch**：b 是在那两笔推上去之前 clone 的，不 fetch 就不知道远端多了东西，
    # behind 恒为 0 —— 第一次跑就是这样，于是" ahead/behind 方向"这一档**根本没验到**，
    # 而它是 32-12 的生死判据。`read_status` 同样只读本地 ref（不 fetch），
    # 所以 fetch 之后这两条命令看到的才是同一份世界
    sh(b, "fetch", "-q", "origin")
    # 本地那 `local` 笔只在 b
    for i in range(local):
        (b / f"mine{i}.txt").write_text("m\n", encoding="utf-8")
        sh(b, "add", "-A")
        sh(b, "commit", "-q", "-m", f"local {i}")
    return b


def compare_one(path, expect):
    old = sh(path, "rev-list", "--left-right", "--count", "HEAD...@{u}", check=False)
    new = sh(path, *MERGED, check=False)
    ab = [ln for ln in new.stdout.splitlines() if "branch.ab" in ln]
    nums_old = old.stdout.split()
    nums_new = ab[0].replace("# branch.ab +", "").replace(" -", " ").split() if ab else []
    ok = nums_old == nums_new == [str(x) for x in expect]
    print(f"    {path.name}  期望 ahead/behind = {expect}   现状 {nums_old}   合并 {nums_new}"
          f"   ⇒ {'对上 ✓' if ok else '不对 !!'}")
    return ok


def equivalence():
    """合并命令能不能替现状那几条——**语义等价**，不是快慢"""
    print("\n== 等价判据 1：领先落后的正负号（**两档不对称才抓得住写反**）==")
    ok1 = compare_one(make_repo_diverged("diverged", local=1), (1, 2))
    ok2 = compare_one(make_repo_diverged("behindonly", local=0), (0, 2))
    print(f"    ⇒ 方向 {'一致 ✓' if (ok1 and ok2) else '有出入 !! —— 合并前必须先解决这一档'}")

    print("\n== 等价判据 2：脏仓库的变更计数（big：500 改 + 300 未跟踪）==")
    big = ROOT / "big"
    plain = sh(big, "status", "--porcelain", check=False).stdout
    v2 = sh(big, *MERGED, check=False).stdout
    n_plain = len([ln for ln in plain.splitlines() if ln.strip()])
    n_v2 = len([ln for ln in v2.splitlines() if ln and not ln.startswith("#")])
    print(f"    status --porcelain 行数 = {n_plain}；v2 非 # 行数 = {n_v2}"
          f"  ⇒ {'一致 ✓' if n_plain == n_v2 else '不一致 !!'}")
    changed = sum(1 for ln in plain.splitlines() if not ln.startswith("??"))
    v2_1 = sum(1 for ln in v2.splitlines() if ln.startswith("1 "))
    v2_u = sum(1 for ln in v2.splitlines() if ln.startswith("u "))
    v2_q = sum(1 for ln in v2.splitlines() if ln.startswith("? "))
    print(f"    已跟踪改动 {changed} 行；v2 里 1 档 {v2_1}、u 档 {v2_u}、? 档 {v2_q}"
          f"  ⇒ 未跟踪 {v2_q}（期望 {BIG_DIRTY}）")

    print("\n== 等价判据 3：没有提交的那颗仓库（unborn HEAD）==")
    rm_tree(ROOT / "nocommit")
    p = ROOT / "nocommit"
    p.mkdir(parents=True)
    sh(p, "init", "-q", "--initial-branch=main")
    r = sh(p, *MERGED, check=False)
    print(f"    退出码 {r.returncode}；前四行 {r.stdout.splitlines()[:4]}")
    old_branch = sh(p, "rev-parse", "--abbrev-ref", "HEAD", check=False)
    print(f"    现状那条 rev-parse 给 {old_branch.stdout.strip()!r}（退出码 {old_branch.returncode}）"
          "  ⇒ 界面上那颗仓库现在显示分支名就是这个值")

    print("\n== 等价判据 4：非仓库目录（valid_repo 是 fs 检查，合并命令能不能替它）==")
    notrepo = ROOT / "not-a-repo"
    notrepo.mkdir(exist_ok=True)
    r = sh(notrepo, *MERGED, check=False)
    print(f"    退出码 {r.returncode}；stdout 空 = {not r.stdout.strip()}")
    print("    ⚠️ 现状 `valid_repo` 只看 `.git` 存不存在，**不起进程**；"
          "换成「命令失败即非仓库」省下的是那一次 fs 吗？不是——它本来就比 35ms 便宜得多。"
          "合并的动机只能是少跑那几条 git，不是替掉 valid_repo")


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "make"
    if mode == "make":
        ROOT.mkdir(parents=True, exist_ok=True)
        v = subprocess.run(["git", "--version"], capture_output=True, text=True)
        print(v.stdout.strip())
        make_repo_with_remote("small", big=False)
        print("  small 好了（干净、有上游）")
        make_repo_with_remote("big", big=True)
        print(f"  big 好了（{BIG_FILES} 个跟踪文件、500 改 + {BIG_DIRTY} 未跟踪、有上游）")
        make_repo_no_remote("noup")
        print("  noup 好了（有提交、没有上游）")
    elif mode == "time":
        v = subprocess.run(["git", "--version"], capture_output=True, text=True)
        print(v.stdout.strip(), f" 每档 {ROUNDS} 轮取中位数")
        bench(ROOT / "small", "小仓库（干净、有上游）")
        bench(ROOT / "big", "大仓库（2 万文件、脏、有上游）")
        semantics()
    elif mode == "sign":
        # 只跑方向那一档（`equiv` 会连 2 万文件那颗一起重建，那要几分钟）
        print("== 领先落后的正负号：两档不对称 ==")
        ok1 = compare_one(make_repo_diverged("diverged", local=1), (1, 2))
        ok2 = compare_one(make_repo_diverged("behindonly", local=0), (0, 2))
        print(f"    ⇒ {'两种读法方向一致 ✓' if (ok1 and ok2) else '方向有出入 !!'}")
        sys.exit(0 if (ok1 and ok2) else 1)
    elif mode == "equiv":
        equivalence()
    elif mode == "clean":
        for d in sorted(ROOT.parent.glob("qbg3212-*")):
            rm_tree(d)
            print("  删掉", d)
    else:
        print("不懂这个模式")
        sys.exit(2)


main()
