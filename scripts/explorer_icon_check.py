#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""让 Explorer 亲自把 exe 的图标画出来，抓下来看 —— 桌面/任务栏就是这么画的。

    python scripts/explorer_icon_check.py <exe> <out.png>

为什么非要过一遍 Explorer：`extract_exe_icon.py` 验的是 **PE 里存了什么**，
Explorer 验的是 **Windows 实际会画什么**。两者之间隔着系统图标缓存——
资源对了而缓存没刷新，看到的仍是旧图。这一步是唯一能同时覆盖两者的。

只在一处有代价：图标缓存。Windows 拿图标缓存做哈希，**改 exe 会让它失效**，
下次打开该目录会重画——这是我们想要的，但确实会动到用户机器的状态。
所以本脚本**只在一个临时目录里**建一个 .lnk 指向 exe，不碰用户桌面，
结束时不删缓存（那要 `SHChangeNotify(SHCNE_ASSOCCHANGED)`，反而更扰人）。
"""
import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import tempfile
import time
from pathlib import Path

USER = ctypes.WinDLL("user32", use_last_error=True)
SHELL = ctypes.WinDLL("shell32", use_last_error=True)
OLE = ctypes.WinDLL("ole32", use_last_error=True)

# IShellLink + IPersistFile 的最小 COM 声明
CLSID_ShellLink = "{00021401-0000-0000-C000-000000000046}"
IID_IShellLinkW = "{000214F9-0000-0000-C000-000000000046}"
IID_IPersistFile = "{0000010B-0000-0000-C000-000000000046}"


def make_shortcut(target: Path, link: Path):
    """建一个 .lnk 指向 target。用 PowerShell 的 WScript.Shell，不跟 COM 死磕。"""
    ps = (
        "$s = New-Object -ComObject WScript.Shell; "
        f"$l = $s.CreateShortcut('{link}'); "
        f"$l.TargetPath = '{target}'; "
        "$l.Save()"
    )
    subprocess.run(["powershell", "-NoProfile", "-Command", ps], check=True)


def find_explorer(folder: Path):
    """等 Explorer 把那个目录的窗口开出来，返回 (hwnd, 标题)。"""
    EnumWindowsProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    found = {}

    def cb(hwnd, _l):
        if not USER.IsWindowVisible(hwnd):
            return True
        buf = ctypes.create_unicode_buffer(512)
        USER.GetWindowTextW(hwnd, buf, 512)
        title = buf.value
        if title and title.startswith(folder.name):
            found["hwnd"] = hwnd
            found["title"] = title
            return False
        return True

    USER.EnumWindows(EnumWindowsProc(cb), 0)
    return found.get("hwnd"), found.get("title", "")


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    exe = Path(sys.argv[1]).resolve()
    out = Path(sys.argv[2]).resolve()

    tmp = Path(tempfile.gettempdir()) / "qb_icon_check"
    tmp.mkdir(exist_ok=True)
    for p in tmp.glob("*"):
        p.unlink()
    link = tmp / f"{exe.stem}.lnk"
    make_shortcut(exe, link)
    print(f"临时目录：{tmp}\n快捷方式：{link.name} → {exe}")

    subprocess.Popen(["explorer.exe", str(tmp)])
    hwnd = title = None
    for _ in range(40):
        time.sleep(0.5)
        hwnd, title = find_explorer(tmp)
        if hwnd:
            break
    if not hwnd:
        raise SystemExit("没等到 Explorer 窗口")
    print(f"Explorer 窗口 hwnd={hwnd} 标题={title!r}")

    # 摆到确定的位置与大小：左上角那一格就是中等大小的 exe 图标
    r = wt.RECT(60, 60, 1100, 620)
    USER.SetWindowPos(hwnd, 0, r.left, r.top, r.right - r.left, r.bottom - r.top, 0x0040)
    USER.SetForegroundWindow(hwnd)
    time.sleep(2.5)          # 等图标真的画完

    from PIL import Image, ImageGrab
    # 抓整个窗口。**别只猜一块**——Win11 资源管理器上面是标签栏 + 功能区（能占
    # 150px 上下），按"窗口左上角"裁裁到的会是一排工具按钮，不是文件图标
    full_box = (r.left, r.top, r.right, r.bottom)
    full = ImageGrab.grab(bbox=full_box, all_screens=True)
    full.save(out)
    print(f"{out}  整窗 {full_box}  {full.size[0]}×{full.size[1]}")

    # 文件列表第一行那张图标。坐标是按 **Win11 资源管理器「详细信息」视图 + 左侧
    # 导航窗格** 量出来的：导航窗格约 230 宽，列头在窗口顶部往下约 150。
    # 少了这两个偏移，裁到的会分别是导航窗格和功能区——都还"像模像样"，
    # 很容易当成"图标不对"（栽过两次）
    grid = full.crop((240, 150, 440, 220))
    small = grid.resize((grid.width * 5, grid.height * 5), Image.NEAREST)
    sp = out.with_name(f"{out.stem}_4x.png")
    small.save(sp)
    print(f"{sp}  文件格 5 倍最近邻")
    print(f"\n临时目录留着不删（里面有那个 .lnk）：{tmp}")


if __name__ == "__main__":
    main()
