#!/usr/bin/env python
"""抓 qingbird-git 窗口截图，落盘 PNG。

为什么需要它：GUI 的问题（间距、字号、层级、留白）只有看渲染结果才能发现，
读代码里的 px(16.0) 是找不到"粗糙"的。这个脚本让 agent 自己拿到像素。

用法：
    python scripts/shoot.py                       # 抓「青鸟 Git」窗口
    python scripts/shoot.py --title xxx --out a.png
    python scripts/shoot.py --screen              # 抓整屏（窗口找不到时的兜底）
    python scripts/shoot.py --list                # 列出所有可见窗口标题

原理：PrintWindow 直接问窗口要自己的位图，不需要窗口在前台、不抢焦点，
也不会被别的窗口盖住。PW_RENDERFULLCONTENT(2) 是必须的——GPU 加速的窗口
用旧标志位只会拿到一张黑图。
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import os
import sys
import time

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32
user32.SetProcessDPIAware()

PW_RENDERFULLCONTENT = 0x00000002
SRCCOPY = 0x00CC0020
DIB_RGB_COLORS = 0


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wt.DWORD),
        ("biWidth", ctypes.c_long),
        ("biHeight", ctypes.c_long),
        ("biPlanes", wt.WORD),
        ("biBitCount", wt.WORD),
        ("biCompression", wt.DWORD),
        ("biSizeImage", wt.DWORD),
        ("biXPelsPerMeter", ctypes.c_long),
        ("biYPelsPerMeter", ctypes.c_long),
        ("biClrUsed", wt.DWORD),
        ("biClrImportant", wt.DWORD),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wt.DWORD * 3)]


def list_windows():
    out = []
    EnumProc = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)

    def cb(hwnd, _):
        if not user32.IsWindowVisible(hwnd):
            return True
        n = user32.GetWindowTextLengthW(hwnd)
        buf = ctypes.create_unicode_buffer(n + 1)
        if n:
            user32.GetWindowTextW(hwnd, buf, n + 1)
        r = wt.RECT()
        user32.GetWindowRect(hwnd, ctypes.byref(r))
        pid = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        out.append((hwnd, buf.value, r.right - r.left, r.bottom - r.top, pid.value))
        return True

    user32.EnumWindows(EnumProc(cb), 0)
    return out


def find(title, pid=None, largest=True):
    """挑目标窗口。同一进程可能开多个窗（有个黑屏副窗），所以要能按 pid + 面积筛。"""
    cands = []
    for hwnd, name, w, h, p in list_windows():
        if pid and p != pid:
            continue
        if title and title not in name:
            continue
        cands.append((hwnd, w, h, p))
    if not cands:
        return (None, 0, 0, 0)
    if largest:
        cands.sort(key=lambda c: c[1] * c[2], reverse=True)
    return cands[0]


def capture_screen(hwnd, path):
    """从屏幕 DC 抓——GPU 加速的窗口 PrintWindow 只会给黑图。

    代价是窗口必须真的在屏幕上可见，所以要先置前。
    """
    SW_RESTORE = 9
    user32.ShowWindow(hwnd, SW_RESTORE)
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.6)  # 等 compositor 把帧送上屏

    r = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    x, y = r.left, r.top
    w, h = r.right - r.left, r.bottom - r.top
    if w <= 0 or h <= 0:
        raise SystemExit(f"窗口尺寸异常 {w}x{h}")

    hdc = user32.GetDC(0)
    mem = gdi32.CreateCompatibleDC(hdc)
    bmp = gdi32.CreateCompatibleBitmap(hdc, w, h)
    gdi32.SelectObject(mem, bmp)
    gdi32.BitBlt(mem, 0, 0, w, h, hdc, x, y, SRCCOPY)

    info = BITMAPINFO()
    info.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    info.bmiHeader.biWidth = w
    info.bmiHeader.biHeight = -h
    info.bmiHeader.biPlanes = 1
    info.bmiHeader.biBitCount = 32
    info.bmiHeader.biCompression = 0

    buf = ctypes.create_string_buffer(w * h * 4)
    if not gdi32.GetDIBits(mem, bmp, 0, h, buf, ctypes.byref(info), DIB_RGB_COLORS):
        raise SystemExit("GetDIBits 失败")

    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(0, hdc)

    from PIL import Image

    img = Image.frombuffer("RGBA", (w, h), buf, "raw", "BGRA", 0, 1).convert("RGB")
    os.makedirs(os.path.dirname(os.path.abspath(path)) or ".", exist_ok=True)
    img.save(path)
    return w, h


def capture(hwnd, path):
    r = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    w, h = r.right - r.left, r.bottom - r.top
    if w <= 0 or h <= 0:
        raise SystemExit(f"窗口尺寸异常 {w}x{h}")

    hdc = user32.GetWindowDC(hwnd)
    mem = gdi32.CreateCompatibleDC(hdc)
    bmp = gdi32.CreateCompatibleBitmap(hdc, w, h)
    gdi32.SelectObject(mem, bmp)

    ok = user32.PrintWindow(hwnd, mem, PW_RENDERFULLCONTENT)
    if not ok:
        raise SystemExit("PrintWindow 失败（窗口可能被最小化或已关闭）")

    info = BITMAPINFO()
    info.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    info.bmiHeader.biWidth = w
    info.bmiHeader.biHeight = -h  # 负数 = 自上而下，省一次翻转
    info.bmiHeader.biPlanes = 1
    info.bmiHeader.biBitCount = 32
    info.bmiHeader.biCompression = 0

    buf = ctypes.create_string_buffer(w * h * 4)
    if not gdi32.GetDIBits(mem, bmp, 0, h, buf, ctypes.byref(info), DIB_RGB_COLORS):
        raise SystemExit("GetDIBits 失败")

    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(hwnd, hdc)

    from PIL import Image

    img = Image.frombuffer("RGBA", (w, h), buf, "raw", "BGRA", 0, 1).convert("RGB")
    os.makedirs(os.path.dirname(os.path.abspath(path)) or ".", exist_ok=True)
    img.save(path)
    return w, h


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--title",
        default="",
        help="按标题子串筛；留空=只要 --pid 匹配就行（GPUI 窗口标题常为空）",
    )
    ap.add_argument("--pid", type=int, default=None, help="只挑这个进程的窗")
    ap.add_argument("--out", default="docs/shots/current.png")
    ap.add_argument("--screen", action="store_true", help="抓整屏")
    ap.add_argument("--list", action="store_true", help="列出可见窗口")
    ap.add_argument(
        "--grab",
        default="screen",
        choices=["screen", "print"],
        help="screen=从屏幕抓（GPU 窗口唯一可用）；print=问窗口要位图",
    )
    a = ap.parse_args()

    if a.list:
        for hwnd, name, w, h, pid in list_windows():
            print(f"{hwnd:>10} pid={pid:<6} {w:>5}x{h:<5} {name[:50]}")
        return

    if a.screen:
        from PIL import ImageGrab

        os.makedirs(os.path.dirname(os.path.abspath(a.out)) or ".", exist_ok=True)
        ImageGrab.grab(all_screens=True).save(a.out)
        print(f"整屏-> {a.out}")
        return

    hwnd, _, _, _ = find(a.title, a.pid)
    if not hwnd:
        print(f"没找到标题含「{a.title}」的窗口。先跑 --list 看有哪些。", file=sys.stderr)
        raise SystemExit(2)
    w, h = capture_screen(hwnd, a.out) if a.grab == "screen" else capture(hwnd, a.out)
    print(f"pid/window {hwnd}  {w}x{h} -> {a.out}")


if __name__ == "__main__":
    main()
