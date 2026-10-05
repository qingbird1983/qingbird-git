"""按暗像素逐列扫，给一组矩形找出内容块的像素范围。

`frame.py crop` 的缩放倍数在显示时还会被缩放一次，**从放大图反算 x 必错**
（阶段 16 量页签时偏过 52px，连点三次都点不中）。量文字位置一律用这个。

用法：python scripts/scancols.py <png> <x0> <y0> <x1> <y1> [--thr 150]
"""
import sys

from PIL import Image


def main():
    a = sys.argv[1:]
    thr = 150
    if "--thr" in a:
        i = a.index("--thr")
        thr = int(a[i + 1])
        a = a[:i]
    src = a[0]
    x0, y0, x1, y1 = map(int, a[1:5])
    im = Image.open(src).convert("L")
    W, H = im.size
    px = im.load()
    print(f"image {W}x{H}")
    runs = []
    cur = None
    for x in range(x0, min(x1, W)):
        n = sum(1 for y in range(y0, min(y1, H)) if px[x, y] < thr)
        if n:
            cur = [x, x] if cur is None else [cur[0], x]
        else:
            if cur:
                runs.append(tuple(cur))
            cur = None
    if cur:
        runs.append(tuple(cur))
    for a_, b_ in runs:
        print(f"run x={a_}..{b_}  center={(a_ + b_) // 2}  width={b_ - a_ + 1}")


if __name__ == "__main__":
    main()
