#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""问系统"托盘里那枚图标到底在不在、在哪" —— `Shell_NotifyIconGetRect`。

    python scripts/tray_icon_query.py <exe> <out.png> [uID ...]

为什么不能靠截图找：托盘图标默认在**「隐藏的图标」**折叠面板里，通知区那一截
启动前后求差分只能差出输入法「中/英」在闪的 15×15（栽过）；把面板点开又要在
一堆陌生图标里用眼睛认哪个是新的——认错就得出"图标没出来"的结论，而事实相反。

`Shell_NotifyIconGetRect` 是**机器判据**：给定 `(hWnd, uID)`，成功就返回
它在屏幕上的矩形。托盘图标 crate 用的是**没有 GUID** 的老式身份
（`platform_impl/windows/mod.rs:626-640` 的 `NOTIFYICONDATAW` 只填
`hWnd` + `uID`），而那个 `uID` 是**进程内自增计数器**
（同文件 `:97` 的 `internal_id = COUNTER.next()`），所以扫一小圈 uID 就能撞上。
"""
import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import qbguard  # noqa: E402

USER = ctypes.WinDLL("user32", use_last_error=True)
SHELL = ctypes.WinDLL("shell32", use_last_error=True)

USER.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
USER.GetWindowThreadProcessId.restype = wt.DWORD
USER.GetClassNameW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
USER.GetClassNameW.restype = ctypes.c_int
USER.IsWindowVisible.argtypes = [wt.HWND]
USER.IsWindowVisible.restype = wt.BOOL


class NOTIFYICONIDENTIFIER(ctypes.Structure):
    # cbSize, hWnd, uID, guidItem
    _fields_ = [("cbSize", wt.DWORD), ("hWnd", wt.HWND), ("uID", wt.UINT),
                ("guidItem", ctypes.c_byte * 16)]


SHELL.Shell_NotifyIconGetRect.argtypes = [ctypes.POINTER(NOTIFYICONIDENTIFIER),
                                          ctypes.POINTER(wt.RECT)]
SHELL.Shell_NotifyIconGetRect.restype = ctypes.c_long

TRAY_CLASS = "tray_icon_app"          # tray-icon 0.26 建托盘窗口用的类名


def enum_proc():
    return ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)


def tray_windows(pid):
    """这个进程名下、类名是 `tray_icon_app` 的窗口（托盘 crate 建的隐藏窗口）。"""
    found = []
    P = enum_proc()

    def cb(h, _l):
        wpid = wt.DWORD()
        USER.GetWindowThreadProcessId(h, ctypes.byref(wpid))
        if wpid.value == pid:
            buf = ctypes.create_unicode_buffer(256)
            USER.GetClassNameW(h, buf, 256)
            if buf.value == TRAY_CLASS:
                found.append(h)
        return True

    USER.EnumWindows(P(cb), 0)
    return found


def probe(hwnd, uid):
    nid = NOTIFYICONIDENTIFIER()
    nid.cbSize = ctypes.sizeof(NOTIFYICONIDENTIFIER)
    nid.hWnd = hwnd
    nid.uID = uid
    r = wt.RECT()
    hr = SHELL.Shell_NotifyIconGetRect(ctypes.byref(nid), ctypes.byref(r))
    return (hr, (r.left, r.top, r.right, r.bottom)) if hr == 0 else None


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    exe = Path(sys.argv[1]).resolve()
    out = Path(sys.argv[2]).resolve()
    uids = [int(x) for x in sys.argv[3:]] or list(range(0, 8))

    before_fp = qbguard.real_cfg_fp()
    print(f"启动 {exe.name} …")
    proc = subprocess.Popen([str(exe)], env=qbguard.launch_env())
    try:
        time.sleep(8)
        hwnds = tray_windows(proc.pid)
        print(f"进程 {proc.pid} 下类名 {TRAY_CLASS} 的窗口: {hwnds or '**一个都没有**'}")
        if not hwnds:
            print("→ 托盘 crate 连隐藏窗口都没建起来，图标必然不在托盘里")
            return

        hits = []
        for h in hwnds:
            for uid in uids:
                got = probe(h, uid)
                if got:
                    hits.append((h, uid, got[1]))
                    print(f"  hwnd={h} uID={uid}  →  矩形 {got[1]}")
        if not hits:
            print(f"→ 窗口在，但这几个 uID {uids} 都问不到图标（NIM_ADD 可能没成功）")
            return

        from PIL import Image, ImageGrab
        for i, (_h, _u, (l, t, r, b)) in enumerate(hits):
            pad = 10
            box = (max(0, l - pad), max(0, t - pad), r + pad, b + pad)
            img = ImageGrab.grab(bbox=box, all_screens=True).convert("RGB")
            scale = max(1, min(12, 360 // max(img.width, 1)))
            big = img.resize((img.width * scale, img.height * scale), Image.NEAREST)
            p = out.with_name(f"{out.stem}_{i}.png")
            big.save(p)
            print(f"{p}  系统指定位置 {box}  {scale} 倍最近邻")
    finally:
        time.sleep(0.3)
        proc.terminate()
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
        qbguard.check_real_cfg(before_fp)


if __name__ == "__main__":
    main()
