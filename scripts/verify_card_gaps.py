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
        for tag, p, _sig in shots[n]:
            found, _nlines = border_gaps(p, vals)
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
    print("\n判读：", "全站卡间距一致" if not bad else "不一致：")
    for b in bad:
        print("   ⛔ " + b)
    print(f"帧在 {WORK} 下")
    return 0 if not bad else 1


if __name__ == "__main__":
    sys.exit(main())
