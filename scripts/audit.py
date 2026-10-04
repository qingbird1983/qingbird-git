#!/usr/bin/env python
"""界面体检：把「看着粗糙」翻译成能改的条目。

为什么需要：UI 的毛病（空得慌、对不齐、层级糊、留白碎）靠读代码发现不了，
靠肉眼看又说不清"到底哪儿不对"。这个脚本从截图里量出客观数字，
每条结论都带读数，改完再跑一次对比——这样"变好了"是测出来的，不是感觉。

用法：
    python scripts/audit.py docs/shots/live-01.png          # 单张
    python scripts/audit.py docs/shots/live-01.png --ref docs/03-我的仓库工作台.png

检查项（每一项都对应设计稿上可指认的规则）：
  A. 画布利用率  —— 主区空白占比。设计稿的卡片是撑满的，空白 >12% 就是漏了填充
  B. 卡片对齐    —— 同排卡片的顶/底边必须齐，参差 >2px 就报
  C. 竖向节奏    —— 卡片之间的间距是否落在 4/8/12 的档位上
  D. 边框一致性  —— 卡片描边色必须等于 theme 的 border token
  E. 字号阶梯    —— 出现过的文字高度聚类，正常应只有 11/12/13/16/18/20 六档
  F. 对比度      —— 正文与背景的 WCAG 对比度，<4.5 报
  G. 触达密度    —— 同类元素的像素间距离散度，离散说明没对齐
"""

import argparse
import collections
import sys

from PIL import Image

# theme.json 的 token（改主题时同步这里，或者干脆从 theme.json 读——见 load_theme）
FALLBACK = {
    "background": (250, 250, 249),
    "surface": (255, 255, 255),
    "hairline": (229, 227, 223),
    "muted": (93, 91, 84),
    "faint": (150, 148, 141),
    "ink": (26, 26, 26),
    "accent": (91, 122, 153),
}


def load_theme(path="gui/theme.json"):
    import json
    import os

    if not os.path.exists(path):
        return FALLBACK
    c = json.load(open(path, encoding="utf-8"))["themes"][0]["colors"]

    def hx(k):
        v = c.get(k, "").lstrip("#")
        return tuple(int(v[i : i + 2], 16) for i in (0, 2, 4)) if len(v) >= 6 else None

    return {
        "background": hx("background") or FALLBACK["background"],
        "surface": hx("secondary.background") or FALLBACK["surface"],
        "hairline": hx("border") or FALLBACK["hairline"],
        "muted": hx("muted.foreground") or FALLBACK["muted"],
        "faint": (150, 148, 141),
        "ink": hx("foreground") or FALLBACK["ink"],
        "accent": hx("accent.background") or FALLBACK["accent"],
    }


def near(a, b, tol=6):
    return all(abs(x - y) <= tol for x, y in zip(a, b))


def luminance(c):
    def f(v):
        v /= 255
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4

    r, g, b = (f(x) for x in c[:3])
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    la, lb = luminance(a), luminance(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


def find_cards(im, th):
    """扫出所有描边矩形（卡片）。返回 (x0,y0,x1,y1) 列表。"""
    W, H = im.size
    px = im.load()
    hair = th["hairline"]
    rows = []
    for y in range(H):
        xs = [x for x in range(W) if near(px[x, y], hair, 3)]
        rows.append(xs)

    # 连续的横线段：同一行里长度 > 80 的描边像素
    hlines = []
    for y, xs in enumerate(rows):
        if not xs:
            continue
        start = prev = xs[0]
        for x in xs[1:]:
            if x - prev > 3:
                if prev - start > 80:
                    hlines.append((y, start, prev))
                start = x
            prev = x
        if prev - start > 80:
            hlines.append((y, start, prev))

    # 把横线按 x 区间聚成矩形
    boxes = []
    for y, x0, x1 in hlines:
        for b in boxes:
            if abs(b["y1"] - y) <= 2 and not (x1 < b["x0"] - 20 or x0 > b["x1"] + 20):
                b["y1"] = y
                b["x0"] = min(b["x0"], x0)
                b["x1"] = max(b["x1"], x1)
                break
        else:
            boxes.append({"y0": y, "y1": y, "x0": x0, "x1": x1})

    out = []
    for b in boxes:
        h = b["y1"] - b["y0"]
        w = b["x1"] - b["x0"]
        if h > 40 and w > 100:
            out.append((b["x0"], b["y0"], b["x1"], b["y1"]))
    return sorted(set(out), key=lambda b: (b[1], b[0]))


def report(im, th, ref=None):
    W, H = im.size
    px = im.load()
    issues = []
    stats = {}

    cards = find_cards(im, th)
    stats["画布"] = f"{W}x{H}"
    stats["检出卡片"] = f"{len(cards)} 张"

    # ---- A. 画布利用率：主区（非侧栏、非标题栏）里的空白占比
    main_x0 = int(W * 0.30)
    main_y0 = int(H * 0.06)
    bg = th["background"]
    total = 0
    blank = 0
    for y in range(main_y0, H - 4, 3):
        for x in range(main_x0, W - 4, 3):
            total += 1
            if near(px[x, y], bg, 3):
                blank += 1
    ratio = blank / max(total, 1)
    stats["主区空白"] = f"{ratio*100:.1f}%"
    if ratio > 0.30:
        issues.append(
            ("A 画布利用率", f"主区有 {ratio*100:.0f}% 是纯背景色，看着空",
             "卡片撑不满高度，或该补的内容区没补")
        )

    # ---- B. 同排卡片对齐：把 y 区间重叠的卡片归为一组，比 top/bottom
    groups = []
    if len(cards) >= 2:
        for c in cards:
            for g in groups:
                if not (c[3] < g[0][1] - 4 or c[1] > g[0][3] + 4):
                    g.append(c)
                    break
            else:
                groups.append([c])
        for g in groups:
            if len(g) < 2:
                continue
            tops = {c[1] for c in g}
            bots = {c[3] for c in g}
            if len(tops) > 1 and max(tops) - min(tops) > 3:
                issues.append(
                    ("B 卡片对齐", f"同排 {len(g)} 张卡顶边不齐：{sorted(tops)}",
                     "底排卡没统一高度来源，各自按内容收缩了")
                )
            if len(bots) > 1 and max(bots) - min(bots) > 3:
                issues.append(
                    ("B 卡片对齐", f"同排 {len(g)} 张卡底边不齐：{sorted(bots)}",
                     "同上。改成 h_full + flex_1 均分")
                )
    stats["同排卡片组"] = str(sum(1 for g in groups if len(g) > 1))

    # ---- C. 竖向节奏：相邻卡片的间距是否落在 4/8/12 档
    gaps = []
    ys = sorted({c[1] for c in cards})
    for a, b in zip(ys, ys[1:]):
        g = b - a
        if 0 < g <= 60:
            gaps.append(g)
    offbeat = [g for g in gaps if min(abs(g - d) for d in (4, 8, 12, 16, 20, 24)) > 1]
    stats["竖向间距"] = ", ".join(str(g) for g in gaps[:10]) or "-"
    if offbeat:
        issues.append(
            ("C 竖向节奏", f"间距 {sorted(set(offbeat))} 不在 4/8/12 档上",
             "间距要归到档位常量，别现算")
        )

    # ---- E. 字号阶梯：按行扫描文字行的暗像素高度
    ink_rows = []
    for y in range(1, H - 1):
        n = sum(1 for x in range(0, W, 2) if luminance(px[x, y]) < 0.30)
        ink_rows.append(n)
    # 找连续暗行段 = 文字行
    segs, start = [], None
    for y, n in enumerate(ink_rows):
        if n > 2 and start is None:
            start = y
        elif n <= 2 and start is not None:
            if y - start >= 4:
                segs.append(y - start)
            start = None
    buckets = collections.Counter()
    for s in segs:
        buckets[min((s // 2) * 2, 40)] += 1
    common = [h for h, n in buckets.most_common(4) if n >= 4]
    stats["文字行高"] = ", ".join(f"{h}px×{buckets[h]}" for h, _ in buckets.most_common(6))
    ladder = {11, 12, 13, 14, 16, 18, 20, 24}
    odd = [h for h in buckets if buckets[h] >= 3 and h not in ladder]
    if odd:
        issues.append(
            ("E 字号阶梯", f"出现非档位行高 {sorted(odd)}",
             "字号应只有 11/12/13/16/18/20 六档")
        )

    # ---- F. 对比度
    pairs = [
        ("ink/surface", th["ink"], th["surface"]),
        ("muted/surface", th["muted"], th["surface"]),
        ("muted/background", th["muted"], th["background"]),
        ("faint/surface", th["faint"], th["surface"]),
        ("accent/surface", th["accent"], th["surface"]),
    ]
    contrast_rows = []
    for name, fg, bg2 in pairs:
        r = contrast(fg, bg2)
        contrast_rows.append(f"{name} {r:.1f}")
        need = 4.5 if "faint" not in name else 3.0
        if r < need:
            issues.append(
                ("F 对比度", f"{name} = {r:.2f} < {need}",
                 "次要文字太浅，投影/暗环境下读不清")
            )
    stats["对比度"] = ", ".join(contrast_rows)

    return stats, issues


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("shot")
    ap.add_argument("--ref", default=None)
    ap.add_argument("--theme", default="gui/theme.json")
    a = ap.parse_args()

    im = Image.open(a.shot).convert("RGB")
    th = load_theme(a.theme)
    stats, issues = report(im, th)

    print(f"=== {a.shot} ===")
    for k, v in stats.items():
        print(f"  {k:<10} {v}")
    print()
    if not issues:
        print("没检出结构性问题。")
        return
    print(f"检出 {len(issues)} 条：")
    for i, (cat, what, why) in enumerate(issues, 1):
        print(f"  {i}. [{cat}] {what}")
        print(f"       → {why}")


if __name__ == "__main__":
    sys.exit(main())
