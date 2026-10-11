"""量「中间那一列压到多窄开始切内容」——`MAIN_MIN_W` 这个数的真值。

为什么要量它：用户给的规则是「中间不足 870 就把窗口向右撑」，而 870 那一格
**是按"不挂右栏时主区卡的宽"来的**，不是按"内容压到多窄会切"来的。
抽屉开、窗口撑到 1502 之后中间正好 870，帧上却看见头卡那排右边的键被切掉
⇒ 说明这条算式的输入用错了数：**内容的 min-content 比 870 大**。

判据不目测：每一档宽度下，取主区卡**右边界那条 1px 边框**的 x，再取这一屏里
最右一格内容的 x。**内容伸进右边内边距（16px）以内 = 没切；越过边框 = 切了**。

用法：python target/measure_mincontent.py
"""
import ctypes
import json
import os
import subprocess
import sys
import time

sys.path.insert(0, r"F:\AIwork\qingbird-git\scripts")
from PIL import Image  # noqa: E402

TMP = r"C:\Users\42527\AppData\Local\Temp\qb_v4"
SANDBOX = os.path.join(TMP, "appdata")
REAL = r"C:\Users\42527\AppData\Roaming\com.qingbird.git\config.json"
CFG = os.path.join(SANDBOX, "com.qingbird.git", "config.json")
EXE = r"F:\AIwork\qingbird-git\target\debug\qingbird-git.exe"

from qbguard import launch_env, real_cfg_fp, check_real_cfg  # noqa: E402
import shoot  # noqa: E402
import steps  # noqa: E402

u = shoot.user32
wt = shoot.wt
ROW_DIRTY = 222
# 三行都读：头卡那排右边的键 / 路径行（最长，多半是它定 min-content）/ 同步卡按钮行
BANDS = {"头卡行": (78, 104), "路径行": (122, 140), "同步卡行": (218, 246), "日志表头": (372, 394)}


def park_cursor(hwnd):
    r = wt.RECT()
    u.GetWindowRect(hwnd, ctypes.byref(r))
    ox = r.left - 60 if r.left - 60 > 0 else r.right + 60
    oy = r.top - 60 if r.top - 60 > 0 else r.bottom + 60
    u.SetCursorPos(ox, oy)
    time.sleep(0.4)
    u.SetCursorPos(ox - 8, oy - 8)
    time.sleep(0.8)


def drag_right_border(hwnd, dx):
    """按住窗口**右边界外那 8px 的缩放手柄**往左拖 dx 像素。
    这是操作系统自己在处理，不经过 gpui ⇒ 与用户手动拖窗口同一条路"""
    r = wt.RECT()
    u.GetWindowRect(hwnd, ctypes.byref(r))
    x, y = r.right - 3, r.top + 400
    u.SetCursorPos(x, y)
    time.sleep(0.25)
    u.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.12)
    for i in range(1, 6):
        u.SetCursorPos(x - dx * i // 5, y)
        time.sleep(0.06)
    u.SetCursorPos(x - dx, y)
    time.sleep(0.2)
    u.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(1.2)


def client_w(hwnd):
    r = wt.RECT()
    u.GetClientRect(hwnd, ctypes.byref(r))
    return r.right - r.left


def line_color(c):
    """卡的那条 1px 边框：`edge(t)` = (215,224,228)。画布灰是 (232,237,240)，
    差 17 ⇒ 容差收到 6 才不会被画布灰与侧栏选中淡底混进来"""
    return abs(c[0] - 215) <= 6 and abs(c[1] - 224) <= 6 and abs(c[2] - 228) <= 6


def ink(c):
    """一格"内容"：暗的（字/线）或饱和的（绿按钮、色块）"""
    return sum(c) < 520 or (max(c) - min(c)) > 40


def read_clip(png):
    """每行：主区卡白面的右缘、卡内最右一格内容、两者之间的气口。
    卡有 **16px 右内边距** ⇒ 没切的话最右内容离白面右缘至少 16；
    气口掉到 14 以下 = 内容伸进了内边距 = 这一屏压过头了"""
    im = Image.open(png).convert("RGB")
    px = im.load()
    W = im.size[0]
    out = []
    for tag, (y0, y1) in BANDS.items():
        h = y1 - y0
        white_right = None
        for x in range(W - 12, 900, -1):
            if sum(1 for y in range(y0, y1) if min(px[x, y]) >= 248) >= h * 0.6:
                white_right = x
                break
        if white_right is None:
            out.append((tag, None, None, None))
            continue
        ink_x = None
        for x in range(white_right - 1, 330, -1):
            if sum(1 for y in range(y0, y1) if ink(px[x, y])) >= 2:
                ink_x = x
                break
        out.append((tag, white_right, ink_x,
                    None if ink_x is None else white_right - ink_x))
    return out


def main():
    real = json.load(open(REAL, encoding="utf-8-sig"))
    rs = real.get("settings", {})
    os.environ["QB_APPDATA"] = SANDBOX
    cfg = json.load(open(CFG, encoding="utf-8-sig"))
    for k in ("ui_scale", "theme", "sidebar_w", "sidebar_collapsed"):
        if k in rs:
            cfg["settings"][k] = rs[k]
    # 抽屉**关着**量：这一档只问"中间那一列能压到多窄"，与右栏无关
    cfg["settings"]["inspector_open"] = False
    cfg["settings"]["sidebar_collapsed"] = False
    paths = [e["path"] for e in cfg["repos"] if e["name"] == "demo-dirty"]
    cfg["selected_path"] = paths[0]
    json.dump(cfg, open(CFG, "w", encoding="utf-8"), ensure_ascii=False)

    before = real_cfg_fp()
    env = launch_env()
    assert env["APPDATA"] == SANDBOX
    p = subprocess.Popen([EXE], env=env)
    try:
        hwnd = steps.find_window(p.pid)
        assert steps.ensure_focus(hwnd)
        steps.click(hwnd, 700, 300)
        steps.ensure_focus(hwnd)
        steps.click(hwnd, 28, 94)
        time.sleep(1.2)
        steps.click(hwnd, 122, ROW_DIRTY)
        time.sleep(3.0)
        for step in range(6):
            park_cursor(hwnd)
            w = client_w(hwnd)
            png = os.path.join(TMP, "mc_%d.png" % step)
            shoot.capture(hwnd, png)
            main_w = w - 41 - int(rs.get("sidebar_w", 263)) - 8
            print("\n客户区 %d ⇒ 中间那一列约 %d" % (w, main_w))
            for tag, border, content, over in read_clip(png):
                verdict = "?" if over is None else ("切了 +%d" % over if over > 0 else "没切")
                print("   %-6s 边框 x=%s 最右内容 x=%s ⇒ %s" % (tag, border, content, verdict))
            if step == 5:
                break
            steps.ensure_focus(hwnd)
            drag_right_border(hwnd, 20)
    finally:
        p.terminate()
        time.sleep(0.6)
        check_real_cfg(before)


if __name__ == "__main__":
    main()
