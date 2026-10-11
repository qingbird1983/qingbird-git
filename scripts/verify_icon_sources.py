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

## 2026-10-12 补的两件事：这一条从前**不会红**

`main()` 原来走到末尾 `return None` ⇒ **退出码永远是 0**，"不一致"只是打印出来的一句
`**不一致**（同一张源图不该派生两套）`。也就是说它是一份**报告**，不是一道闸：
谁把它接进 CI 或者用 `&&` 串在提交前面，都拦不住任何东西。
⇒ 现在三件事一起判，红就 `exit 1` 并点名是哪一条：
  · 托盘 RGBA 的**字节数**（尺寸假设漂了要在解图之前就说，不要让 `Image.frombytes`
    抛一个看不懂的异常当"失败"）；
  · 两处**尺寸必须同一个**（`TRAY_EDGE` 改了而 `.ico` 那档没改就是两套）；
  · ②③ **逐像素相同**。
源图 alpha 那一段**保持只报数不判**——钉它的是 `gui` 里那两条单测
（`tray_icon_is_a_transparent_sticker` / `tray_icon_is_not_blank`），
这里再判一遍就是同一个规则两个出处。

## 变异账（2026-10-12 实测，驱动 `target/mutate_icons.py`，一次性不入仓库）

  · **P0 应当放行**：未变异跑一次 ⇒ 退出码 0，`判读：三处同源、两处派生逐像素相同`。
  · **I1 末端侧**：`gui/build.rs` 托盘那一路的缩放从共用的 `resize()` 换成
    `FilterType::Triangle`，`.ico` 那一路不动 ⇒ 重编之后
    **红在「②③ 逐像素不同（包围盒 (0,0,32,32)，单通道最大差 50）」**。
    这就是这条判据要挡的真实形状：今天两条共用一颗 `resize()`，结构上保证；
    将来谁给其中一路单独加个开关，就得由这条判据负责。
  · **I2 源码侧**（新加那一臂的证据）：`build.rs` 的 `include_bytes!` 改指向仓库里
    **真实存在的另一张图** `assets/icon-source.png`（1024 那张，今天没人读它），
    **故意不重编** ⇒ 盘上产物仍是干净那份，②③ 照样逐像素相同，
    红的只有新加那两条：「标题栏与托盘/桌面读的不是同一张源图」+
    「源图 1024×1024 不是 build.rs 断言的 128×128」。
    补的是这份脚本原本的洞：它标题写着「三处图标真的是同一张图」，
    而像素那一路只比 ②③——两处都由同一份 `build.rs` 派生，**永远相同**，
    标题栏那一路一个字都没判过。
  · **P9 还原后复跑**：退出码 0，`build.rs` 与基线 sha 逐字节相同 ✓
  · ⚠️ **第一轮这两条都读不到结果**，成因值得留在这里：这份脚本原来没有
    `sys.stdout.reconfigure`，而绿色那句里没有 `⛔`，失败那句正好以 `⛔` 开头 ⇒
    `UnicodeEncodeError: 'gbk' codec can't encode character '\u26d4'`——
    **判据一红它自己先崩**，退出码 1 是异常给的、不是判据给的。
    这是 AGENTS 那条「GBK 控制台编不出 ✓ / →」的**第四种触发**：
    以前三次都炸在收尾的报喜句上，这次炸在**报错路径**上——
    这种坏了平时完全看不出来，因为它只在"真的抓到漂移"的那一刻才坏。
    ⇒ 凡是会打印 `⛔` 的判据脚本，第一行就要 reconfigure，驱动那边还要把
      "Traceback / 找不到『判读』那一行"单独判成一种状态，不许当"没红"。
"""
import re
import subprocess
import sys
from pathlib import Path

# ⚠️ **GBK 控制台会在"红的那一句"上崩**（2026-10-12 变异实测）：这一份脚本原来没有
# reconfigure，绿色那条话里没有 `⛔` 所以一直打印得出来，而失败那条以 `⛔` 开头 ⇒
# `UnicodeEncodeError: 'gbk' codec can't encode character '\u26d4'`——
# **判据红的时候它自己先崩**，退出码 1 是异常给的、不是判据给的，
# 上面那条"应当红在哪一条"当场读不出来。与 AGENTS 记的「GBK 编不出 ✓ / ⇒」同一条坑，
# 这是它的第四种触发：**炸点正好在报错路径上**。
sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

ROOT = Path(__file__).resolve().parent.parent
BUILD_RS = ROOT / "gui" / "build.rs"
APP_ASSETS = ROOT / "gui" / "src" / "app_assets.rs"
PNG_SIG = b"\x89PNG\r\n\x1a\n"


def embedded_png_path(file: Path, relative_to: Path) -> Path:
    """从源码里现读那颗 `include_bytes!("…")` 指的是哪张图。

    ⚠️ 这一条是 2026-10-12 补的，补的是这份脚本自己的一个洞：
    原来 `SRC` 是脚本里写死的 `gui/assets/app-icon.png`，而 `build.rs` 读的是它自己那行
    `include_bytes!`。于是"build.rs 改去读另一张图"这一类漂移**像素那一路判不到**
    （②③ 都由同一份 build.rs 派生，它们俩永远相同），而脚本的标题写的却是
    「三处图标真的是同一张图」——**那句话以前没人判**。
    ⇒ 现在两处各现读一次并要求**同一个文件**；`SRC` 也由现读的结果给，
      脚本里不再写死图路径（抄一次就多一处会漂的出处）。
    """
    txt = file.read_text(encoding="utf-8-sig")
    hits = re.findall(r'include_bytes!\(\s*"([^"]+\.png)"\s*\)', txt)
    if len(hits) != 1:
        raise SystemExit(f"{file.name} 里的 `include_bytes!(\"….png\")` 不是恰好一条："
                         f"{hits} ⇒ 现读失败，这条判据的前提没了")
    return (relative_to / hits[0]).resolve()


def const_int(file: Path, name: str) -> int:
    """现读 build.rs 里那一档尺寸常数（`TRAY_EDGE` / `SRC_EDGE`），不抄进脚本"""
    m = re.search(r"const %s:\s*u32\s*=\s*(\d+);" % re.escape(name),
                  file.read_text(encoding="utf-8-sig"))
    if not m:
        raise SystemExit(f"{file.name} 里读不到 `const {name}: u32` ⇒ 形状变了，先去看它")
    return int(m.group(1))


def find_out_dir() -> Path:
    base = ROOT / "target" / "debug" / "build"
    cands = [d / "out" for d in base.glob("qingbird-gui-*")
             if (d / "out" / "app-tray.rgba").exists()]
    if not cands:
        raise SystemExit("找不到 app-tray.rgba，先 cargo build")
    return max(cands, key=lambda p: p.stat().st_mtime)


def icon_of(exe: Path, edge: int):
    """从 exe 资源里取 `.ico` 的 `edge×edge` 那一档（档号由 `TRAY_EDGE` 现读给，
    原来这里写死 32，`TRAY_EDGE` 一改这份脚本就自己去对一档不相干的东西）"""
    sys.path.insert(0, str(ROOT / "scripts"))
    from extract_exe_icon import read_exe_resources
    res = read_exe_resources(exe)
    for g in res["groups"]:
        if g["w"] == edge:
            return res["icons"][g["nid"]]
    raise SystemExit(f"exe 里没有 {edge}×{edge} 那档（ICO_EDGES 与 TRAY_EDGE 对不上）")


def main():
    from PIL import Image, ImageChops
    import io

    bad = []

    # ⓪ 两处"读哪张图"现读一次，必须同一个文件（脚本标题那句"三处同一张图"由这一条负责）
    p_build = embedded_png_path(BUILD_RS, BUILD_RS.parent)
    p_assets = embedded_png_path(APP_ASSETS, APP_ASSETS.parent)
    print(f"build.rs 读   {p_build}")
    print(f"app_assets 读 {p_assets}")
    if p_build != p_assets:
        bad.append(f"标题栏与托盘/桌面读的不是同一张源图：{p_build.name} vs {p_assets.name}")
    tray_edge = const_int(BUILD_RS, "TRAY_EDGE")
    src_edge = const_int(BUILD_RS, "SRC_EDGE")
    print(f"现读 build.rs：SRC_EDGE={src_edge}  TRAY_EDGE={tray_edge}（脚本里不抄这两个数）")

    out = find_out_dir()
    tray_raw = (out / "app-tray.rgba").read_bytes()
    if len(tray_raw) != tray_edge * tray_edge * 4:
        bad.append(f"托盘 RGBA {len(tray_raw)}B，与 `TRAY_EDGE={tray_edge}` 要的 "
                   f"{tray_edge}×{tray_edge}×4={tray_edge * tray_edge * 4}B 对不上"
                   "⇒ 产物不是按这一档编出来的（多半是没重编）")
        print("\n判读：⛔ " + "；".join(bad))
        return 1
    tray = Image.frombytes("RGBA", (tray_edge, tray_edge), tray_raw)

    exe = ROOT / "target" / "debug" / "qingbird-git.exe"
    blob = icon_of(exe, tray_edge)
    ico32 = Image.open(io.BytesIO(blob)).convert("RGBA")

    src = Image.open(p_build).convert("RGBA")
    print(f"① 源图   {p_build.relative_to(ROOT)}  {src.size[0]}×{src.size[1]}")
    print(f"② 托盘   app-tray.rgba          {tray.size[0]}×{tray.size[1]}  {len(tray_raw)}B")
    print(f"③ exe    RT_ICON               {ico32.size[0]}×{ico32.size[1]}  {len(blob)}B"
          f"  ({'PNG' if blob[:8] == PNG_SIG else 'DIB'} 压缩)")
    if src.size != (src_edge, src_edge):
        bad.append(f"源图 {src.size[0]}×{src.size[1]} 不是 build.rs 断言的 "
                   f"{src_edge}×{src_edge}")

    if tray.size != ico32.size:
        bad.append(f"两处尺寸就不是同一个：托盘 {tray.size} vs exe 那档 {ico32.size}"
                   "⇒ 同一个东西两个出处")
    else:
        diff = ImageChops.difference(tray, ico32)
        bbox = diff.getbbox()
        worst = max(diff.getextrema()[i][1] for i in range(4))
        print(f"\n②③ 逐像素差：不同像素包围盒 = {bbox}，单通道最大差 = {worst}")
        print("   → " + ("完全一致" if bbox is None else "**不一致**（同一张源图不该派生两套）"))
        if bbox is not None:
            bad.append(f"②③ 逐像素不同（包围盒 {bbox}，单通道最大差 {worst}）"
                       "⇒ 同一张源图派生出了两套结果：两条路径的缩放参数分叉了")

    # 源图四角的 alpha —— **只报数不判**：钉它的是 `gui` 里那两条单测
    # （`tray_icon_is_a_transparent_sticker` / `tray_icon_is_not_blank`），
    # 这里再判一遍就是同一条规则两个出处
    a = src.getchannel("A")
    corners = [a.getpixel(p) for p in [(0, 0), (src.width - 1, 0),
                                       (0, src.height - 1), (src.width - 1, src.height - 1)]]
    lo, hi = a.getextrema()
    print(f"\n源图 alpha（只报数，判据在 gui 那两条单测里）：四角 = {corners}，全图范围 = {lo}..{hi}")

    import os
    strip = Image.new("RGBA", (src_edge + tray_edge + tray_edge + 24, src_edge),
                      (245, 245, 244, 255))
    strip.paste(src, (0, 0), src)
    strip.paste(tray.resize((src_edge, src_edge), Image.NEAREST),
                (src_edge + 8, 0), tray.resize((src_edge, src_edge), Image.NEAREST))
    strip.paste(ico32.resize((src_edge, src_edge), Image.NEAREST),
                (src_edge + tray_edge + 16, 0),
                ico32.resize((src_edge, src_edge), Image.NEAREST))
    # 默认落 %TEMP，不往用户家目录写东西（沙箱纪律同一条：临时产物自己有明确的家）
    p = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(os.environ["TEMP"]) / "qb_icon_three.png"
    strip.save(p)
    print(f"\n{p}  左=源图  中=托盘(最近邻放大)  右=exe 同档(最近邻放大)")

    print("\n判读：", "三处同源、两处派生逐像素相同" if not bad else "⛔ " + "；".join(bad))
    return 0 if not bad else 1


if __name__ == "__main__":
    sys.exit(main())
