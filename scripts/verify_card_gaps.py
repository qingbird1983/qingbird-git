"""阶段 31 出口那条「卡间距一致」的第一次全站证据（2026-10-11，随 32-19 拆出来的第三格）。

为什么单开一份脚本：32-19 收口时把那条出口拆成三格——反差与深色五条都有数了，
而**卡间距这一档只有补八那一轮在 x=1250 竖剖过总览一列**（带高 6）。
一列一处不构成"一致"，所以那一格当时没跟着勾。

## ⚠️ 两份真源是打架的，这份脚本不预设哪个对
v4 §3.2 写「卡间 = `GAP_LG` 12」，而补八那一轮按他当场的口径把两层统一到了
`GAP_TIGHT` 4（实测带高 6 = 4 + 两卡各 1px 边框）。⇒ 这里只报**量到的数**，
查出来是"文档过期"还是"实现漂了"再登记，别把脚本写成去迎合某一个数。

## 怎么区分「两张卡之间」与「卡里两个盒之间」
夹在中间的那一条露的是**画布**档（被两张卡的阴影压暗）就是卡间；露的是**卡面**档就是卡内。

## 三次判据返工（每一版都是判据错，不是界面错）
① "间隙众数完全等于 canvas 才算卡间" ⇒ 浅色四屏里三屏量到 **0 条**，而脚本照样报
   "全站卡间距一致"——**判据坏被报成通过**。成因：两卡之间那几行画布被阴影压暗了
   （浅 216..220，而 canvas 是 232），"完全相等"一条都认不上。
② 换成就近归类（离哪档近算哪档）⇒ 量出 18 种"带高"，里面全是 1..10 的**文字行距**：
   因为"卡段"是按 card 档的连续段取的，一列穿过卡面被文字与图表打断成十几段。
   纵向闭运算救不了——卡间气口只有 6px，闭运算会把它一起填掉。
③ ⇒ 正解是量**边框线**：每张卡上下各有一条横贯的 `line-soft`，两卡之间就是"上卡下边 +
   气口 + 下卡上边"这样一对挨着的线。补八那句"带高 6"本来就是这么量的，
   我绕了两圈才回到同一个口径。

## 2026-10-12 补的一臂：「测量到底发生了没有」要独立判

原来对"量到 0 条"的处理是：再看一眼这一屏竖着叠了几张卡（`card_tops`），
一张都不多就报"不适用"。看着周全，**其实两颗吃的都是 `line-soft` 那一档**：
档位读错 / 帧不是这份代码画的时候，两条一起瞎 ⇒ 每一屏都"只叠了 0 张卡"、
每一屏都"不适用"，脚本**照样退出码 0**。这是"匹配面为空却打印通过"的第三种形态。
⇒ 现在每一档都打印两个计数（横贯线 / 卡间带），并且**整档一屏都没量到横贯线就算瞎**，
   直接红。G2 那条变异就是这条臂的证据。

## 变异账（2026-10-12 实测，驱动 `target/mutate_gaps2.py`，一次性不入仓库）

  · **P0 / P9 应当放行**：未变异全跑两次（变异前、还原并重编之后各一次）都退出码 0，
    量的数与 10-11 那一轮逐字相同——4 屏 × 两档，**卡间带 27 条全是 6px**
    （总览 12 / 我的仓库-概览 10 / 空间页 5；第三方仓库那一屏只叠一张卡 ⇒ 不适用），
    每档**横贯线 233 条**（这个计数是本轮才开始打印的）。
    **P1** = 未变异走 `--measure-only` 也必须全绿：G2 那类"只改期望"的廉价变异整个
    建立在重量模式上，先证明它与全跑给同一个结论，否则重量模式是个假入口。
  · **G1 末端侧**（要重编）：`detail.rs` 里 `overview_body` 那一层卡间 4→8、外层保持 4
    ⇒ 红 2 条：`qingbird-light / 01-我的仓库-概览` 与 `qingbird-dark / 同一屏`，
    各报「同一屏里卡间带高有 2 种 [6, 10]」并逐条打出带高明细 ⇒ **点名的正是被改那一屏**。
    ⚠️ 设计第一版我写的是"点名总览那一屏"——**错的**：改的是详情页的概览页签，
    不是全局总览页；登记按帧上印出来的名字写，别按我想的写。
    ⚠️ 也不选"把全站卡间一起改成 12"那种：这条脚本判的是**同屏自相一致**，
    整体漂移它故意不管（上面"两份真源打架"那一节）。要证它有牙就得造不一致。
  · **G2 期望侧**（不重编）：脚本里那两行 `line-soft` 档**同时**换成文字色
    （两处一起换才叫"这一档读错"，只换一处就成了两个变量）⇒ 文字色在 200px 的段里
    凑不出 55% 的横贯 ⇒ 匹配面为空 ⇒ **红在本轮新加的那一臂**，两档各一条：
    「整档 4 屏一屏都没量到横贯线 ⇒ 档位读错或帧不是这份代码画出来的，判据是瞎的」。
    这一条同时就是上面那节的证据：**补这一臂之前它是假通过的**。

用法：python scripts/verify_card_gaps.py                  # 抓 4 屏 × 两档 + 量
      python scripts/verify_card_gaps.py --measure-only    # 只重量（帧留在盘上）
"""

import os
import subprocess
import sys
import time
from collections import Counter

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.stdout.reconfigure(encoding="utf-8", errors="replace")

import qbguard  # noqa: E402
import steps as _steps  # noqa: E402
import shoot as _shoot  # noqa: E402
import a11y_stress as A  # noqa: E402
import verify_dark_five as V  # noqa: E402  ← 沙箱、抓帧、八档现读全部复用它
from PIL import Image  # noqa: E402

WORK = os.path.join(os.environ["TEMP"], "qb_gaps")
SHOTS = ("00-总览", "01-我的仓库-概览", "02-空间页", "03-第三方仓库")


def shot_dir(theme):
    return os.path.join(WORK, "dark" if "dark" in theme else "light")


def grab_all(theme):
    # 夹具自己造 + 自己验（与 `verify_dark_five.py` 同一处修法，2026-10-12）：
    # `build_sandbox` 只把路径写进 repos，那颗仓库到底是不是个真 git 仓库它不管——
    # 上一轮的夹具一旦被清理，这里就会拿到一个"没有 .git 的空文件夹"，
    # 而卡间距那几屏照样量得出数 ⇒ **红 nowhere，测的是另一屏**。
    A.build_fixture()
    assert os.path.isdir(os.path.join(A.FIXTURE, ".git")), \
        f"夹具 {A.FIXTURE} 没有 .git ⇒ 这一跑量的不是仓库那一屏"
    A.build_sandbox(False, theme=theme)
    subprocess.run(["taskkill", "/F", "/IM", "qingbird-git.exe"], capture_output=True)
    time.sleep(0.8)
    env = qbguard.launch_env()
    assert env["APPDATA"] == A.SANDBOX, f"沙箱没生效：{env['APPDATA']}"
    proc = subprocess.Popen([A.EXE], cwd=A.REPO, env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    d = shot_dir(theme)
    if os.path.isdir(d):
        A.clean(d)          # 旧帧留着，"四屏"就变成"目录里有四个文件"
    os.makedirs(d, exist_ok=True)
    out = []
    try:
        hwnd = A.find_window(proc.pid)
        assert hwnd and _steps.ensure_focus(hwnd)
        clx, cly = A.client_origin(hwnd)
        offx, offy = clx - A.window_rect(hwnd)[0], cly - A.window_rect(hwnd)[1]

        def tap(name, kind=None, settle=1.6):
            n = A.one(A.dump(hwnd), name, kind)
            _steps.click(hwnd, n.cx - clx, n.cy - cly, settle=settle)

        def grab(tag):
            nodes = A.dump(hwnd)
            p = os.path.join(d, tag + ".png")
            _shoot.capture(hwnd, p)
            out.append((tag, p, A.tree_sig(nodes)))

        _steps.click(hwnd, 700 - offx, 300 - offy, settle=1.0)
        assert _steps.ensure_focus(hwnd)
        grab(SHOTS[0])
        tap("我的仓库"); grab(SHOTS[1])
        tap("空间", "TabItem"); grab(SHOTS[2])
        tap("第三方仓库"); grab(SHOTS[3])
    finally:
        proc.kill()
        time.sleep(0.5)
    return out


def near(px, a, b):
    """把一列像素归到两档里较近的那一档；两档都差很远算其他。"""
    da, db = V.delta(px, a), V.delta(px, b)
    if min(da, db) > 40:
        return "其他"
    return "canvas" if da <= db else "card"


def border_gaps(png, vals, seg_w=200, soft_min=0.55, max_gap=24):
    """量「两张卡之间那一条带」的带高，含上下两条边框线（与补八那个 6 同口径）。

    做法见文件头第 ③ 条。返回 [(段左缘, 上线 y, 下线 y, 带高)]。
    """
    a = np.asarray(Image.open(png).convert("RGB"), dtype=np.int16)
    w = a.shape[1]
    soft = np.array(vals["line-soft"], dtype=np.int16)
    out, total_lines = [], 0
    for x0 in range(20, max(w - 120, 21), seg_w):
        x1 = min(x0 + seg_w, w - 20)
        if x1 - x0 < 60:
            continue
        hit = (a[:, x0:x1] == soft).all(axis=2).sum(axis=1)
        ys = np.flatnonzero(hit >= soft_min * (x1 - x0))
        lines = []
        for y in ys:
            if lines and y - lines[-1][1] <= 1:
                lines[-1][1] = int(y)
            else:
                lines.append([int(y), int(y)])
        total_lines += len(lines)
        for (b1, e1), (b2, _) in zip(lines, lines[1:]):
            gap = b2 - e1 - 1
            if gap <= 0 or gap > max_gap:
                continue
            mid = (x0 + x1) // 2
            seg = [tuple(int(v) for v in a[y, mid]) for y in range(e1 + 1, b2)]
            top = Counter(seg).most_common(1)[0][0]
            if near(top, vals["canvas"], vals["card"]) != "canvas":
                continue
            # ⛔ 还要求**两条线的外侧都是卡面**：只有这样才能说这两条线是
            # "一张卡的下边"与"下一张卡的上边"。少了这一条会把**标题栏下沿**那条线
            # 也算进来（`title_bar.background` 与画布同值 ⇒ 中间夹的确实是画布系），
            # 于是每一屏都凭空多出一条 10px 的"卡间带"，而它量的是标题栏到首卡。
            # 实测：加上这一条之后全站只剩 6px 一种。
            above = tuple(int(v) for v in a[max(e1 - 2, 0), mid])
            below = tuple(int(v) for v in a[min(b2 + 1, a.shape[0] - 1), mid])
            if near(above, vals["canvas"], vals["card"]) != "card":
                continue
            if near(below, vals["canvas"], vals["card"]) != "card":
                continue
            out.append((x0, e1, b2, b2 - e1 + 1))
    return out, total_lines


def card_tops(png, vals, seg_w=200, soft_min=0.55):
    """这一屏**竖着叠了几张卡**：取各列段里"卡的上边线"条数的最大值。

    一条线算作某张卡的上边，当且仅当它**紧邻上方是画布系、紧邻下方是卡面**。
    为什么要这一颗：`border_gaps` 量到 0 条有两种可能——"这一屏只有一张卡"（合法，
    不该红）与"判据坏了"（该红）。⚠️ 第一版拿"横贯线总数 ≤ 2"当代理，**当场被
    第三方仓库那一屏打脸**：那里有 11 条横贯线，全是**表格行分隔线**
    （`table.row.border` 与 `line-soft` 同一个值），而那一屏确实只有一张卡。
    ⇒ 代理判据要能被反例打穿才算查过；这里换成直接数"卡的上边"。
    """
    a = np.asarray(Image.open(png).convert("RGB"), dtype=np.int16)
    w, h = a.shape[1], a.shape[0]
    soft = np.array(vals["line-soft"], dtype=np.int16)
    best = 0
    for x0 in range(20, max(w - 120, 21), seg_w):
        x1 = min(x0 + seg_w, w - 20)
        if x1 - x0 < 60:
            continue
        hit = (a[:, x0:x1] == soft).all(axis=2).sum(axis=1)
        ys = np.flatnonzero(hit >= soft_min * (x1 - x0))
        lines = []
        for y in ys:
            if lines and y - lines[-1][1] <= 1:
                lines[-1][1] = int(y)
            else:
                lines.append([int(y), int(y)])
        mid = (x0 + x1) // 2
        n = 0
        for b, e in lines:
            if b < 2 or e + 2 >= h:
                continue
            above = tuple(int(v) for v in a[b - 2, mid])
            below = tuple(int(v) for v in a[e + 2, mid])
            if near(above, vals["canvas"], vals["card"]) == "canvas" and \
               near(below, vals["canvas"], vals["card"]) == "card":
                n += 1
        best = max(best, n)
    return best


def main():
    all_hex = V.hexes()
    dk = [n for n in all_hex if "dark" in n][0]
    lt = [n for n in all_hex if "light" in n][0]
    os.makedirs(WORK, exist_ok=True)
    shots = {}
    before = qbguard.real_cfg_fp()
    try:
        if "--measure-only" in sys.argv:
            shots = {n: [(p[:-4], os.path.join(shot_dir(n), p), None)
                         for p in sorted(os.listdir(shot_dir(n))) if p.endswith(".png")]
                     for n in (lt, dk)}
        else:
            for n in (lt, dk):
                print(f"\n== 抓帧：{n} ==")
                shots[n] = grab_all(n)
    finally:
        qbguard.check_real_cfg(before)

    print("\n================ 逐段竖剖：两张卡之间那一条带 ================")
    bad = []
    for n in (lt, dk):
        vals = all_hex[n]
        print(f"\n-- {n} --")
        n_lines = n_bands = 0
        for tag, p, _sig in shots[n]:
            found, nlines = border_gaps(p, vals)
            n_lines += nlines
            n_bands += len(found)
            if not found:
                # 量到 0 条有两种可能：**这一屏只叠了一张卡**（合法，不该红），
                # 或判据坏了（该红）。⚠️ 既不能直接报红（第三方仓库那屏就是一张卡 +
                # 一张表，红它是假红），也不能直接跳过（那正是"没测到被报成通过"）。
                ncard = card_tops(p, vals)
                if ncard <= 1:
                    print(f"  {tag:18s} 不适用：这一屏竖着只叠了 {ncard} 张卡 "
                          f"⇒ 没有「卡与卡之间」这一层")
                else:
                    bad.append(f"{n} / {tag}：量到 0 条卡间带，而这一屏竖着叠了 "
                               f"{ncard} 张卡 ⇒ 判据有问题，不能当通过")
                    print(f"  {tag:18s} ⛔ 0 条（{ncard} 张卡）")
                continue
            hist = Counter(g[3] for g in found)
            print(f"  {tag:18s} 卡间带 {len(found)} 条，带高分布 " +
                  "  ".join(f"{h}px×{c}" for h, c in sorted(hist.items())))
            if len(hist) > 1:
                bad.append(f"{n} / {tag}：同一屏里卡间带高有 {len(hist)} 种 "
                           f"{sorted(hist)}（逐条：{found}）")
        # ⛔ **"不适用"这一档自己会替判据遮丑**（2026-10-12 变异验出来的洞）：
        # 上面那句"0 条 ⇒ 先看这一屏叠了几张卡"看着周全，其实 `card_tops` 与
        # `border_gaps` **吃的是同一颗档位**（`line-soft`）——档位读错的时候两条一起瞎，
        # 于是每一屏都"只叠了一张卡"、每一屏都"不适用"，脚本照样退出码 0。
        # ⇒ 加一条**独立于判据分支**的自检：整档一屏都没量到横贯线，就是判据瞎了，
        #   不许当通过。计数必须打出来（AGENTS：计数型断言要把数报出来，
        #   "0 条 = 匹配面为空"与"40 条全部可解释"是两句话）
        print(f"  小计：横贯线 {n_lines} 条 / 卡间带 {n_bands} 条")
        if n_lines == 0:
            bad.append(f"{n}：整档 {len(shots[n])} 屏一屏都没量到横贯线 ⇒ "
                       "档位读错或帧不是这份代码画出来的，判据是瞎的，不能当通过")
    print("\n判读：", "全站卡间距一致" if not bad else "不一致：")
    for b in bad:
        print("   ⛔ " + b)
    print(f"帧在 {WORK} 下")
    return 0 if not bad else 1


if __name__ == "__main__":
    sys.exit(main())
