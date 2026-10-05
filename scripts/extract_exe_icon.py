#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""从 exe 里把内嵌图标**逐档取出来**渲成 PNG —— 用来亲眼核对「桌面图标真的是那张图」。

    python scripts/extract_exe_icon.py <exe> <out.png> [边长 ...]

不带边长时默认取 16/24/32/48/64/128/256，每档写成 `<out>_<n>.png`，
并另出 `<out>_strip.png` 把各档横排一次看全。

两件事分开验，别混：

1. **资源里到底有什么** —— 直接读 PE 的资源树，解出 RT_ICON 条目。
   build.rs 把每档都编成 PNG（Vista+ 全支持），所以这里解出来就是原图，
   不经过 Windows 的缩放，"看到的"就是"存进去的"。
2. **系统认不认这个图标** —— 另跑一次 `ExtractIconExW`，拿到非空句柄才算数。
   资源在、句柄也拿得到，才说明桌面/任务栏/Alt-Tab 都会用它。

⚠️ 不能用「看着任务栏上那个像不像」当判据：任务栏读的是**系统图标缓存**，
换完 exe 头几分钟它可能还端着旧图。直接读 PE 才是真的。
"""
import ctypes
import struct
import sys
from ctypes import wintypes
from pathlib import Path

SHELL = ctypes.WinDLL("shell32", use_last_error=True)

PNG_MAGIC = b"\x89PNG\r\n\x1a\n"
RT_ICON, RT_GROUP_ICON = 3, 14


def read_exe_resources(exe: Path):
    """把 exe 的 RT_ICON / RT_GROUP_ICON 资源读出来。

    资源树固定三层：类型 → 名字(ID) → 语言；语言层下面是数据项（存数据的 RVA）。
    三条踩过的坑都写在下面注释里。
    """
    data = exe.read_bytes()
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    assert data[pe:pe + 4] == b"PE\0\0", f"{exe} 不是 PE"
    nsec = struct.unpack_from("<H", data, pe + 6)[0]
    opt_size = struct.unpack_from("<H", data, pe + 20)[0]
    opt = pe + 24
    magic = struct.unpack_from("<H", data, opt)[0]
    dd = opt + (112 if magic == 0x20B else 96)
    rva_rsrc = struct.unpack_from("<I", data, dd + 2 * 8)[0]
    if not rva_rsrc:
        raise SystemExit("exe 没有资源节（.rsrc）—— 资源一个字都没进去")

    secs = []
    base = opt + opt_size
    for i in range(nsec):
        o = base + i * 40
        # 节头：0=Name[8] 8=VirtualSize 12=VirtualAddress 16=SizeOfRawData 20=PointerToRawData
        sva, sraw, sptr = struct.unpack_from("<III", data, o + 12)
        secs.append((sva, sraw, sptr))

    def to_off(rva):
        for sva, sraw, sptr in secs:
            if sva <= rva < sva + sraw:
                return sptr + (rva - sva)
        raise ValueError(f"RVA {rva:#x} 不在任何节里")

    def entries(off):
        """列一层，返回 [(名字或 ID, 子节点文件偏移)]。

        - 名字与 ID 挤在同一个 32 位字段：最高位为 1 是字符串偏移，否则整条就是 ID
          （把两种都归成 None 会把整个类型层抹平）
        - `OffsetToData` **一律相对资源节根**（RVA 空间），不相对父目录
        """
        named, ids = struct.unpack_from("<HH", data, off + 12)
        out = []
        for i in range(named + ids):
            name, child = struct.unpack_from("<II", data, off + 16 + i * 8)
            out.append((None if name & 0x80000000 else name,
                        to_off(rva_rsrc + (child & 0x7FFFFFFF))))
        return out

    def data_at(node):
        """语言层下面是 IMAGE_RESOURCE_DATA_ENTRY，它存的是数据的 RVA（不是偏移）。"""
        rva, size = struct.unpack_from("<II", data, node)
        return to_off(rva), size

    by_type = dict(entries(to_off(rva_rsrc)))
    out = {"groups": [], "icons": {}}
    if RT_GROUP_ICON in by_type:
        for gid, name_node in entries(by_type[RT_GROUP_ICON]):
            for _lang, node in entries(name_node):
                p, _sz = data_at(node)
                _res, _typ, n = struct.unpack_from("<HHH", data, p)
                for i in range(n):
                    w, h, _c, _r, _p, _b, bsz, nid = struct.unpack_from("<BBBBHHIH", data, p + 6 + i * 14)
                    out["groups"].append({"gid": gid, "w": w or 256, "h": h or 256, "nid": nid})
                break                            # 多语言取第一个即可
    if RT_ICON in by_type:
        for nid, name_node in entries(by_type[RT_ICON]):
            for _lang, node in entries(name_node):
                p, size = data_at(node)
                out["icons"][nid] = data[p:p + size]
                break
    return out


def system_icon_count(exe: Path) -> int:
    """`ExtractIconExW` 报图标组数。**这是"系统认不认"的判据**，与 PE 解析互相独立。"""
    SHELL.ExtractIconExW.argtypes = [wintypes.LPCWSTR, ctypes.c_int, ctypes.c_void_p,
                                     ctypes.c_void_p, wintypes.UINT]
    SHELL.ExtractIconExW.restype = wintypes.UINT
    return SHELL.ExtractIconExW(str(exe), -1, None, None, 0)


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    exe = Path(sys.argv[1]).resolve()
    out = Path(sys.argv[2])
    want = [int(x) for x in sys.argv[3:]] or [16, 24, 32, 48, 64, 128, 256]

    res = read_exe_resources(exe)
    if not res["groups"]:
        raise SystemExit("exe 里没有 RT_GROUP_ICON —— 图标没嵌进去（看到的是 Windows 通用那个）")
    print(f"图标组 {res['groups'] and len(res['groups'])} 档："
          + ", ".join(f"{g['w']}x{g['h']}" for g in res["groups"]))

    by_size = {}
    for g in res["groups"]:
        blob = res["icons"].get(g["nid"])
        if not blob:
            print(f"  !! 组里指向 id={g['nid']} 的 RT_ICON 不存在")
            continue
        by_size[g["w"]] = blob
        kind = "PNG" if blob[:8] == PNG_MAGIC else "DIB"
        print(f"  {g['w']:>3}x{g['h']:<3} id={g['nid']:<3} {len(blob):>6}B  {kind}")

    n = system_icon_count(exe)
    print(f"ExtractIconExW 报图标组数 = {n}"
          + ("" if n else "   ← 系统认不出，资源没生效"))

    from PIL import Image
    import io
    tiles = []
    for size in want:
        blob = by_size.get(size)
        if not blob:
            print(f"  (跳过 {size}px：exe 里没有这一档)")
            continue
        img = Image.open(io.BytesIO(blob)).convert("RGBA")
        p = out.with_name(f"{out.stem}_{size}.png")
        img.save(p)
        print(f"{p}  {img.width}x{img.height}")
        tiles.append(img)

    if not tiles:
        return
    scale = 4
    strip = Image.new("RGBA", (sum(t.width for t in tiles) * scale + 4 * (len(tiles) - 1),
                              max(t.height for t in tiles) * scale), (245, 245, 244, 255))
    x = 0
    for t in tiles:
        big = t.resize((t.width * scale, t.height * scale), Image.NEAREST)
        strip.paste(big, (x, 0), big)
        x += big.width + 4
    sp = out.with_name(f"{out.stem}_strip.png")
    strip.save(sp)
    print(f"{sp}  各档横排（{scale} 倍最近邻）")


if __name__ == "__main__":
    main()
