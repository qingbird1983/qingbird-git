#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""盯系统托盘里那枚图标——**折叠面板开前后各拍一张，求差**。

    python scripts/tray_icon_check.py <exe> <out.png> <before.png> <after.png>

为什么非要用差分：
  ① 通知区那一截：应用一启动，任务栏中间的按钮区就多出一个窗口按钮，差出来
     525px 宽一长条，托盘那枚小图标淹在里面
  ② 只看通知区：图标默认在**「隐藏的图标」**里，那一截启动前后**完全一样**
     （实测差出来的是输入法「中/英」在闪的 15×15）
  ③ 点开折叠面板肉眼看：一屏 8 个陌生图标，认错一个就得出了"图标没出来"

三张拍法合起来是唯一无歧义的：**面板开着**这个状态，缺了应用就没有、
有了应用才多出来的那一格，就是它。

⚠️ 会真的启动应用 → 必须设 `QB_APPDATA` 指向 $TEMP 副本（qbguard 负责核账）。
脚本自己启动、自己收尾，不留进程。
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
USER.FindWindowW.argtypes = [wt.LPCWSTR, wt.LPCWSTR]
USER.FindWindowW.restype = wt.HWND
USER.FindWindowExW.argtypes = [wt.HWND, wt.HWND, wt.LPCWSTR, wt.LPCWSTR]
USER.FindWindowExW.restype = wt.HWND
USER.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
USER.SetCursorPos.argtypes = [ctypes.c_int, ctypes.c_int]
USER.mouse_event.argtypes = [wt.DWORD, ctypes.c_uint, ctypes.c_uint,
                             wt.DWORD, ctypes.c_size_t, ctypes.c_long]

MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP = 0x0002, 0x0004
FLYOUT_W, FLYOUT_H = 360, 320


def tray_rect():
    """通知区真实矩形。`TrayNotifyWnd` 是 `Shell_TrayWnd` 的**子窗口**，
    `FindWindowW` 只找顶层，用它会返回空——得用 `FindWindowExW(parent, ...)`。"""
    shell = USER.FindWindowW("Shell_TrayWnd", None)
    notify = USER.FindWindowExW(shell, None, "TrayNotifyWnd", None) if shell else None
    if not notify:
        raise SystemExit("找不到 TrayNotifyWnd")
    r = wt.RECT()
    USER.GetWindowRect(notify, ctypes.byref(r))
    return r.left, r.top, r.right, r.bottom


def grab(box):
    from PIL import ImageGrab
    return ImageGrab.grab(bbox=box, all_screens=True).convert("RGB")


def click(x, y):
    USER.SetCursorPos(x, y)
    time.sleep(0.4)
    USER.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0, 0)
    USER.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0, 0)


def flyout_box(L, T):
    """折叠面板**浮在通知区正上方、贴着底边**。所以判开合要看这块区域的
    **下半截**——早先判的是上半截（那儿本来就是桌面），于是永远判成"没展开"。"""
    return (L - 40, T - FLYOUT_H, L - 40 + FLYOUT_W, T + 4)


def is_flyout_open(img):
    """面板是浅色圆角浮层，背景是桌面。取下半截数"比桌面亮"的像素占比。"""
    from PIL import ImageStat
    bottom = img.crop((0, int(img.height * 0.55), img.width, img.height))
    mean = sum(ImageStat.Stat(bottom).mean[:3]) / 3
    return mean > 150, mean


def arrow_pos(L, T, R, B):
    """折叠箭头的位置：通知区**最左边**那簇深色像素的中点。**不靠目测。**"""
    strip = grab((L, T, R, B))
    w, h = strip.size
    px = strip.load()
    limit = max(8, w // 6)
    cols = [x for x in range(limit)
            if any(sum(px[x, y]) < 300 for y in range(h))]
    if not cols:
        return None
    runs, cur = [], [cols[0]]
    for a, b in zip(cols, cols[1:]):
        if b - a <= 4:
            cur.append(b)
        else:
            runs.append(cur)
            cur = [b]
    runs.append(cur)
    best = max(runs, key=lambda r: r[-1] - r[0])
    return L + (best[0] + best[-1]) // 2, T + h // 2


def open_flyout(tries=4):
    """把「隐藏的图标」面板点开，并**校验它真的开了**。返回 (箭头坐标, 区域, 图)。"""
    L, T, R, B = tray_rect()
    box = flyout_box(L, T)
    for attempt in range(tries):
        arrow = arrow_pos(L, T, R, B)
        if arrow is None:
            return None, None, None
        if not is_flyout_open(grab(box))[0]:
            click(*arrow)
            time.sleep(1.6)
        img = grab(box)
        if is_flyout_open(img)[0]:
            return arrow, box, img
        print(f"   （第 {attempt + 1} 次点没点开，再来）")
    return None, None, None


def main():
    if len(sys.argv) < 5:
        raise SystemExit(__doc__)
    exe = Path(sys.argv[1]).resolve()
    out, shot_a, shot_b = (Path(sys.argv[i]).resolve() for i in (2, 3, 4))

    L, T, R, B = tray_rect()
    print(f"托盘通知区: ({L},{T})-({R},{B})")

    print("① 启动前，点开折叠面板…")
    arrow, box, img_a = open_flyout()
    if img_a is None:
        raise SystemExit("点不开折叠面板（这台机器可能没有隐藏图标）")
    img_a.save(shot_a)
    print(f"   已展开 → {shot_a}  折叠箭头 {arrow}")
    click(*arrow)                      # 收回去
    time.sleep(0.8)

    before_fp = qbguard.real_cfg_fp()
    print(f"② 启动 {exe.name} …")
    proc = subprocess.Popen([str(exe)], env=qbguard.launch_env())
    try:
        time.sleep(8)                  # 托盘是启动时建的
        print("③ 再点开折叠面板…")
        _arrow, _box, img_b = open_flyout()
        if img_b is None:
            raise SystemExit("第二次点不开折叠面板")
        img_b.save(shot_b)
        print(f"   已展开 → {shot_b}")

        from PIL import Image, ImageChops
        bbox = ImageChops.difference(img_a, img_b).getbbox()
        print("④ 两张面板图求差，包围盒:", bbox)
        if not bbox:
            print("   → 完全一样：托盘里没多出任何图标")
            return
        x0 = max(0, bbox[0] - 12)
        y0 = max(0, bbox[1] - 12)
        x1 = min(img_b.width, bbox[2] + 12)
        y1 = min(img_b.height, bbox[3] + 12)
        crop = img_b.crop((x0, y0, x1, y1))
        scale = max(1, min(8, 900 // max(crop.width, 1)))
        big = crop.resize((crop.width * scale, crop.height * scale), Image.NEAREST)
        big.save(out)
        print(f"{out}  新出现的那一格 {crop.size[0]}×{crop.size[1]}  {scale} 倍最近邻")
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
