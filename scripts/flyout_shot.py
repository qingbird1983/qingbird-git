#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""点开「隐藏的图标」折叠面板，**按窗口句柄把那块屏幕抓下来**。

    python scripts/flyout_shot.py <out.png> [exe]

`exe` 给了就先启动它（隔离由 `QB_APPDATA` 负责），再点开面板抓一张。

为什么不按固定坐标裁：折叠面板是 XAML 弹层，**高度随图标数量变**，
位置也是贴着任务栏算出来的。按坐标裁裁到的可能是桌面壁纸——
看起来"面板没开"，其实是裁错地方（栽过）。
改成**点开之后枚举顶层窗口，找新冒出来的那一个**（可看见、有面积、
且贴着任务栏顶部），直接按它的矩形抓。

定位用三个条件筛：点之前不存在、点之后可见、底边贴近任务栏上沿。
"""
import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

USER = ctypes.WinDLL("user32", use_last_error=True)
P = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
USER.FindWindowW.argtypes = [wt.LPCWSTR, wt.LPCWSTR]
USER.FindWindowW.restype = wt.HWND
USER.FindWindowExW.argtypes = [wt.HWND, wt.HWND, wt.LPCWSTR, wt.LPCWSTR]
USER.FindWindowExW.restype = wt.HWND
USER.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
USER.SetCursorPos.argtypes = [ctypes.c_int, ctypes.c_int]
USER.mouse_event.argtypes = [wt.DWORD, ctypes.c_uint, ctypes.c_uint,
                             wt.DWORD, ctypes.c_size_t, ctypes.c_long]


def top_windows():
    out = {}

    def cb(h, _l):
        if not USER.IsWindowVisible(h):
            return True
        r = wt.RECT()
        USER.GetWindowRect(h, ctypes.byref(r))
        if r.right - r.left > 0 and r.bottom - r.top > 0:
            c = ctypes.create_unicode_buffer(256)
            USER.GetClassNameW(h, c, 256)
            out[h] = (c.value, r.left, r.top, r.right, r.bottom)
        return True

    USER.EnumWindows(P(cb), 0)
    return out


def tray_arrow():
    """折叠箭头的屏幕坐标：通知区最左边那簇深色像素的中点。**不靠目测。**"""
    from PIL import ImageGrab
    shell = USER.FindWindowW("Shell_TrayWnd", None)
    notify = USER.FindWindowExW(shell, None, "TrayNotifyWnd", None) if shell else None
    if not notify:
        raise SystemExit("找不到 TrayNotifyWnd")
    r = wt.RECT()
    USER.GetWindowRect(notify, ctypes.byref(r))
    img = ImageGrab.grab(bbox=(r.left, r.top, r.right, r.bottom)).convert("RGB")
    w, h = img.size
    px = img.load()
    limit = max(8, w // 6)
    cols = [x for x in range(limit) if any(sum(px[x, y]) < 300 for y in range(h))]
    if not cols:
        return None, (r.left, r.top, r.right, r.bottom)
    runs, cur = [], [cols[0]]
    for a, b in zip(cols, cols[1:]):
        if b - a <= 4:
            cur.append(b)
        else:
            runs.append(cur)
            cur = [b]
    runs.append(cur)
    best = max(runs, key=lambda x: x[-1] - x[0])
    return (r.left + (best[0] + best[-1]) // 2, r.top + h // 2), (r.left, r.top, r.right, r.bottom)


def find_flyout(before, tray_top):
    """点开之后新出现的、底边贴着任务栏上沿的窗口 = 折叠面板。"""
    after = top_windows()
    cands = []
    for h, (cls, l, t, r, b) in after.items():
        if h in before:
            continue
        if r - l < 60 or b - t < 60:
            continue
        if abs(b - tray_top) > 12:            # 底边必须贴着任务栏
            continue
        cands.append((h, cls, l, t, r, b))
    return cands


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    out = Path(sys.argv[1]).resolve()
    exe = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else None

    proc = None
    fp = None
    if exe:
        import qbguard
        fp = qbguard.real_cfg_fp()
        print(f"启动 {exe.name} …")
        proc = subprocess.Popen([str(exe)], env=qbguard.launch_env())
        time.sleep(8)

    try:
        arrow, tray = tray_arrow()
        print(f"托盘通知区 {tray}  折叠箭头 {arrow}")
        before = top_windows()
        USER.SetCursorPos(*arrow)
        time.sleep(0.4)
        USER.mouse_event(0x0002, 0, 0, 0, 0, 0)
        USER.mouse_event(0x0004, 0, 0, 0, 0, 0)
        time.sleep(2.0)

        cands = find_flyout(before, tray[1])
        if not cands:
            print("没找到新出现的面板窗口。点开后的顶层窗口：")
            for h, (cls, l, t, r, b) in top_windows().items():
                if h not in before:
                    print(f"  {h} [{cls}] ({l},{t})-({r},{b})")
            return
        from PIL import Image, ImageGrab
        for i, (h, cls, l, t, r, b) in enumerate(cands):
            pad = 8
            box = (l - pad, t - pad, r + pad, b + pad)
            img = ImageGrab.grab(bbox=box, all_screens=True).convert("RGB")
            scale = max(1, min(8, 1100 // max(img.width, 1)))
            big = img.resize((img.width * scale, img.height * scale), Image.NEAREST)
            p = out.with_name(f"{out.stem}_{i}.png")
            big.save(p)
            print(f"{p}  [{cls}] {box}  {scale} 倍最近邻")
    finally:
        if proc is not None:
            time.sleep(0.3)
            proc.terminate()
            try:
                proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                proc.kill()
            import qbguard
            qbguard.check_real_cfg(fp)


if __name__ == "__main__":
    main()
