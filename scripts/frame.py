"""看界面截图的三件事：放大读小字、几帧竖着比对、两帧差在哪。

这个 GUI 只能由人双击 exe 定版，但**画面是脚本能拿的**：抓帧 + 量坐标 +
做差分，能把"看起来没问题"和"真的没问题"分开。2026-10-05 那轮说明页改造
就是这样查出来的——点「中文」后画面停在骨架屏、目录锚点落点差 43px。

三个子命令（坐标都是**客户区**坐标，原点在窗口左上）：

  crop  <in.png> <x0> <y0> <x1> <y1> [放大倍数] [输出名]
  stack <x0> <y0> <x1> <y1> <帧1.png> [帧2.png ...]   竖着拼成一张对照图
  diff  <a.png> <b.png> [阈值]                        改动像素数 + 包围盒

输出都在 %TEMP%\\qb_*.png。坐标从哪来的：先 crop 放大目标区域，
按 (放大倍数) 反算回原图——**别目测**，10-05 有一次目测偏了 43px，
点了个空处还以为功能坏了。
"""

import os
import sys

from PIL import Image, ImageChops

TEMP = os.environ["TEMP"]


def out_path(name):
    return os.path.join(TEMP, name + ".png")


def cmd_crop(argv):
    src = argv[0]
    x0, y0, x1, y1 = (int(v) for v in argv[1:5])
    k = int(argv[5]) if len(argv) > 5 else 2
    out = argv[6] if len(argv) > 6 else "qb_crop"
    im = Image.open(src).convert("RGB")
    print("原图", im.size, "-> 区域", (x0, y0, x1, y1), "x", k)
    crop = im.crop((x0, y0, x1, y1))
    crop = crop.resize((crop.width * k, crop.height * k), Image.NEAREST)
    path = out_path(out)
    crop.save(path)
    print("->", path)


def cmd_stack(argv):
    x0, y0, x1, y1 = (int(v) for v in argv[:4])
    box = (x0, y0, x1, y1)
    crops = [
        (os.path.basename(p), Image.open(p).convert("RGB").crop(box)) for p in argv[4:]
    ]
    if not crops:
        print("至少给一帧")
        return 1
    w = max(c.width for _, c in crops)
    gap = 6
    h = sum(c.height + gap for _, c in crops) + gap
    canvas = Image.new("RGB", (w + 2 * gap, h), (30, 30, 30))
    y = gap
    for _, c in crops:
        canvas.paste(c, (gap, y))
        y += c.height + gap
    path = out_path("qb_stack")
    canvas.save(path)
    print("帧：", " / ".join(n for n, _ in crops))
    print("->", path)
    return 0


def cmd_diff(argv):
    a = Image.open(argv[0]).convert("RGB")
    b = Image.open(argv[1]).convert("RGB")
    thr = int(argv[2]) if len(argv) > 2 else 12
    if a.size != b.size:
        print("尺寸不同:", a.size, b.size)
        return 1
    d = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > thr else 0)
    box = d.getbbox()
    # 用直方图数非零像素：Pillow 13 起 `getdata()` 弃用了
    total = a.size[0] * a.size[1]
    n = total - d.histogram()[0]
    print(f"改动像素 {n} / {total} = {n * 100.0 / total:.2f}%")
    print("改动包围盒:", box)
    if box:
        # 半透明叠两张：变了的地方会显出重影，一眼看出是"挪了"还是"变了"
        merged = Image.blend(a, b, 0.5)
        crop = merged.crop(
            (
                max(0, box[0] - 20),
                max(0, box[1] - 20),
                min(a.size[0], box[2] + 20),
                min(a.size[1], box[3] + 20),
            )
        )
        k = max(1, min(4, 1200 // max(1, crop.width)))
        crop = crop.resize((crop.width * k, crop.height * k), Image.NEAREST)
        path = out_path("qb_diff")
        crop.save(path)
        print("叠加图 ->", path, "x", k)
    return 0


def cmd_sheet(argv):
    """几帧 2×2 拼一张，用来一眼比四页的版式（再对可疑的那张单独 crop 看）"""
    cells = []
    for p in argv:
        im = Image.open(p).convert("RGB")
        k = 0.5
        cells.append((os.path.basename(p), im.resize((int(im.width * k), int(im.height * k)))))
    if not cells:
        print("至少给一帧")
        return 1
    w = max(c.width for _, c in cells)
    h = max(c.height for _, c in cells)
    gap = 4
    cols = 2
    rows = (len(cells) + cols - 1) // cols
    canvas = Image.new("RGB", (cols * w + (cols + 1) * gap, rows * h + (rows + 1) * gap), (40, 40, 40))
    for i, (_, c) in enumerate(cells):
        x = gap + (i % cols) * (w + gap)
        y = gap + (i // cols) * (h + gap)
        canvas.paste(c, (x, y))
    path = out_path("qb_sheet")
    canvas.save(path)
    print("顺序：", " / ".join(n for n, _ in cells))
    print("->", path)
    return 0


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return 1
    verb, rest = sys.argv[1], sys.argv[2:]
    if verb == "crop":
        cmd_crop(rest)
        return 0
    if verb == "stack":
        return cmd_stack(rest)
    if verb == "diff":
        return cmd_diff(rest)
    if verb == "sheet":
        return cmd_sheet(rest)
    print("只会 crop / stack / diff / sheet")
    return 1


if __name__ == "__main__":
    sys.exit(main())
