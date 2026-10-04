"""量一列东西的行距——目测 27/28/31 三个值，不能就这么放过。

做法：在指定横向范围内逐行统计"与背景不同的像素数"，得到一条行剖面；
文字/图标的行会凸起来，背景行是 0。取每段凸起的**中心**当行心，
相邻行心之差就是行距。

只量**竖直**行距（列距另说）。用法：
    python scripts/rowpitch.py <in.png> <x0> <y0> <x1> <y1> [最小凸起高度]
输出：每个凸起段的 [起, 止, 中心] 与相邻中心的差。
"""

import os
import sys

from PIL import Image


def profile(img, x0, y0, x1, y1, bg_tol=8):
    """逐行统计偏离背景的像素数。背景取**区域内**的众数——取四角会取到
    卡片外面的画布色，那样整片卡都被判成"有内容"，剖面连成一段。"""
    from collections import Counter

    px = img.convert("RGB").load()
    c = Counter()
    for y in range(y0, y1, 3):
        for x in range(x0, x1, 3):
            c[px[x, y]] += 1
    bg = c.most_common(1)[0][0]

    out = []
    for y in range(y0, y1):
        n = 0
        for x in range(x0, x1):
            r, g, b = px[x, y]
            if abs(r - bg[0]) + abs(g - bg[1]) + abs(b - bg[2]) > bg_tol:
                n += 1
        out.append(n)
    return out, bg


def runs_of(prof, min_h):
    """把剖面切成连续凸起段"""
    segs, start = [], None
    for i, n in enumerate(prof):
        if n > 0 and start is None:
            start = i
        elif n == 0 and start is not None:
            if i - start >= min_h:
                segs.append((start, i - 1))
            start = None
    if start is not None and len(prof) - start >= min_h:
        segs.append((start, len(prof) - 1))
    return segs


def main():
    if len(sys.argv) < 6:
        print(__doc__)
        return 1
    path = sys.argv[1]
    x0, y0, x1, y1 = (int(v) for v in sys.argv[2:6])
    min_h = int(sys.argv[6]) if len(sys.argv) > 6 else 4
    img = Image.open(path)
    prof, bg = profile(img, x0, y0, x1, y1)
    segs = runs_of(prof, min_h)
    print("背景 RGB =", bg, "| 横向范围 x =", x0, "..", x1)
    print("段数 =", len(segs))
    prev = None
    gaps = []
    for i, (a, b) in enumerate(segs):
        c = (a + b) / 2
        gap = "" if prev is None else f"  Δ={c - prev:.1f}"
        if prev is not None:
            gaps.append(round(c - prev, 1))
        print(f"  {i:>2}: y {y0 + a:>4}..{y0 + b:<4} 中心 {y0 + c:>7.1f}  高 {b - a + 1:>2}{gap}")
        prev = c
    if gaps:
        uniq = sorted(set(gaps))
        print("行距取值 =", uniq, "->", "一致" if len(uniq) == 1 else f"**{len(uniq)} 种，不齐**")
    return 0


if __name__ == "__main__":
    sys.exit(main())
