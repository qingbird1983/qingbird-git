#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""核对「三处图标真的是同一张图」——把三处的产物摆一起比。

    python scripts/verify_icon_sources.py

比三样：
  ① 标题栏读的 `gui/assets/app-icon.png`（原图，128×128）
  ② 托盘读的 `$OUT_DIR/app-tray.rgba`（build.rs Lanczos3 缩到 32）
  ③ exe 资源里的 32×32 那档（`.ico` 里的 32 档）

②③ 都是同一张 128 原图用**同一套参数**缩出来的，所以它们**必须逐像素相同**。
不一样就是"同一份源图派生出了两套结果"，那正是这轮要消灭的毛病。
"""
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "gui" / "assets" / "app-icon.png"
PNG_SIG = b"\x89PNG\r\n\x1a\n"


def find_out_dir() -> Path:
    base = ROOT / "target" / "debug" / "build"
    cands = [d / "out" for d in base.glob("qingbird-gui-*")
             if (d / "out" / "app-tray.rgba").exists()]
    if not cands:
        raise SystemExit("找不到 app-tray.rgba，先 cargo build")
    return max(cands, key=lambda p: p.stat().st_mtime)


def icon_32(exe: Path):
    sys.path.insert(0, str(ROOT / "scripts"))
    from extract_exe_icon import read_exe_resources
    res = read_exe_resources(exe)
    for g in res["groups"]:
        if g["w"] == 32:
            return res["icons"][g["nid"]]
    raise SystemExit("exe 里没有 32×32 那档")


def main():
    from PIL import Image, ImageChops
    import io

    out = find_out_dir()
    tray_raw = (out / "app-tray.rgba").read_bytes()
    tray = Image.frombytes("RGBA", (32, 32), tray_raw)

    exe = ROOT / "target" / "debug" / "qingbird-git.exe"
    blob = icon_32(exe)
    ico32 = Image.open(io.BytesIO(blob)).convert("RGBA")

    src = Image.open(SRC).convert("RGBA")
    print(f"① 源图   {SRC.relative_to(ROOT)}  {src.size[0]}×{src.size[1]}")
    print(f"② 托盘   app-tray.rgba          {tray.size[0]}×{tray.size[1]}  {len(tray_raw)}B")
    print(f"③ exe    RT_ICON id=3            {ico32.size[0]}×{ico32.size[1]}  {len(blob)}B"
          f"  ({'PNG' if blob[:8] == PNG_SIG else 'DIB'} 压缩)")

    diff = ImageChops.difference(tray, ico32)
    bbox = diff.getbbox()
    worst = max(diff.getextrema()[i][1] for i in range(4))
    print(f"\n②③ 逐像素差：不同像素包围盒 = {bbox}，单通道最大差 = {worst}")
    print("   → " + ("完全一致" if bbox is None else "**不一致**（同一张源图不该派生两套）"))

    # 源图四角的 alpha —— 决定托盘/桌面在深色任务栏上的观感
    a = src.getchannel("A")
    corners = [a.getpixel(p) for p in [(0, 0), (src.width - 1, 0),
                                       (0, src.height - 1), (src.width - 1, src.height - 1)]]
    lo, hi = a.getextrema()
    print(f"\n源图 alpha：四角 = {corners}，全图范围 = {lo}..{hi}")
    print("   → " + ("四角近不透明（圆角是画在图里的，底色会跟着走到托盘/桌面）"
                  if min(corners) > 200 else
                  "四角透明（真正的透明圆角，深色任务栏上不显方块）"))

    strip = Image.new("RGBA", (128 + 32 + 32 + 24, 128), (245, 245, 244, 255))
    strip.paste(src, (0, 0), src)
    strip.paste(tray.resize((128, 128), Image.NEAREST), (136, 0), tray.resize((128, 128), Image.NEAREST))
    strip.paste(ico32.resize((128, 128), Image.NEAREST), (176, 0), ico32.resize((128, 128), Image.NEAREST))
    p = Path(sys.argv[1]) if len(sys.argv) > 1 else (Path.home() / "qb_icon_three.png")
    strip.save(p)
    print(f"\n{p}  左=源图  中=托盘32(最近邻放大)  右=exe 32(最近邻放大)")


if __name__ == "__main__":
    main()
