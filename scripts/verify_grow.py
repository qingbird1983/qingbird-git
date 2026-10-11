"""验「向右撑窗口」这一条规则，判据都是**帧上的窗口宽**，不是"跑起来没报错"。

① 抽屉开 → 窗口向右撑到 `required_shell_w` 那个数（客户区 1502 = 轨 41 + 左栏 263
   + 中间 870 + 右栏 320 + 右缘 8）；
② 切到总览（那一屏两栏都没有）→ 收回到 1280；
③ 切回仓库页（`inspector_open` 还开着、选中也还在）→ **再撑一次**到 1502，
   且左栏宽与①一模一样（"右栏一开左栏就抢宽"那一族缺陷的判据）；
④ 点抽屉自己那颗收起键 → 收回到 1280（`shell_grown_w` 那条还原路径。这条最容易被
   "看起来没事"放过：不还原的症状是"窗口越开越宽"，得连着开关两次才看得出来）；
⑤ 拖侧栏把手 +40 → 侧栏变宽、**窗口跟着向右长那 40**，中间那一列仍是 950
   （拖把手也是"要更多宽"，同一条规则的另一种触发；拿中间列去垫就又是这一轮修的那个病）。

三条纪律（AGENTS 都记着踩过）：
  · 状态用 config 驱动，不靠点键（上一轮"抽屉开着"的两帧其实都是关着的）；
  · 坐标**量出来**再用：收起键按逐列扫暗像素定在帧上 x=1472..1483，
    从放大图反算必错（`crop` 的倍数在显示时又被缩过一次）；
  · 每一步抓一帧并与上一步比，零变化就点名（`alive` 只说明没崩）。
  · 沙箱：`QB_APPDATA` 指副本，收尾按 mtime+size+sha256 核对真实 config。

用法：python target/verify_grow.py
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
ROW_DIRTY = 222          # 沙箱里 demo-dirty 那一行（shot_widths.py 同一份实测表）
HANDLE = (300, 300)      # 侧栏与中间列那条把手（帧上缝 x=306..311 ⇒ 客户区 298..303）


def park_cursor(hwnd):
    r = wt.RECT()
    u.GetWindowRect(hwnd, ctypes.byref(r))
    ox = r.left - 60 if r.left - 60 > 0 else r.right + 60
    oy = r.top - 60 if r.top - 60 > 0 else r.bottom + 60
    u.SetCursorPos(ox, oy)
    time.sleep(0.4)
    u.SetCursorPos(ox - 8, oy - 8)
    time.sleep(0.8)


def to_client(hwnd, x, y):
    pt = wt.POINT(int(x), int(y))
    u.ClientToScreen(hwnd, ctypes.byref(pt))
    return pt.x, pt.y


def drag(hwnd, cx, cy, dx, dy=0):
    """按住 -> 分几步移动 -> 松手。一步到位的移动会被当成一次跳跃，
    gpui 的把手靠 `MouseMoveEvent` 增量算宽，所以要拆成几段"""
    if u.GetForegroundWindow() != hwnd:
        steps.ensure_focus(hwnd)
    sx, sy = to_client(hwnd, cx, cy)
    u.SetCursorPos(sx, sy)
    time.sleep(0.25)
    u.mouse_event(0x0002, 0, 0, 0, 0)      # down
    time.sleep(0.12)
    for i in range(1, 5):
        u.SetCursorPos(sx + dx * i // 4, sy + dy * i // 4)
        time.sleep(0.08)
    u.SetCursorPos(sx + dx, sy + dy)
    time.sleep(0.25)
    u.mouse_event(0x0004, 0, 0, 0, 0)      # up
    time.sleep(0.9)


def win_w(hwnd):
    """客户区宽（帧上判据换算成客户区，好跟算式对数）"""
    r = wt.RECT()
    u.GetClientRect(hwnd, ctypes.byref(r))
    return r.right - r.left


def white_runs(png, y, x0, x1):
    """帧上 [x0,x1) 里连续的白面段（>=248 三通道），返回 (起, 宽) 列表"""
    im = Image.open(png).convert("RGB")
    px = im.load()
    x1 = min(x1, im.size[0])       # 帧宽随窗口变，扫过头会 IndexError
    out, start = [], None
    for x in range(x0, x1):
        c = px[x, y]
        white = c[0] >= 248 and c[1] >= 248 and c[2] >= 248
        if white and start is None:
            start = x
        elif not white and start is not None:
            if x - start >= 8:
                out.append((start, x - start))
            start = None
    if start is not None and x1 - start >= 8:
        out.append((start, x1 - start))
    return out


def find_collapse(png, drawer_left):
    """在抽屉表头那一条里找**最右边那颗图标**的中心（帧坐标 x,y）。
    ⚠️ 收起键的坐标**不许写死**：窗口一撑宽它就跟着右移，写死的数下一轮就过期
    （路线表 D93 那条「alive 全绿而一步都没生效」的同款坑）。
    判据：抽屉左缘右侧 20px 起，到帧右缘留 12px 气口止，y=48..80 里最右一簇暗像素"""
    im = Image.open(png).convert("RGB")
    px = im.load()
    xs = [
        x
        for x in range(drawer_left + 20, im.size[0] - 12)
        if any(sum(px[x, y]) < 460 for y in range(48, 80))
    ]
    assert xs, "抽屉表头里没找到图标（x 从 %d 起）" % (drawer_left + 20)
    # 最右一簇：从右往左连续（气口 <= 4px）的都算同一颗键
    hi = xs[-1]
    lo = hi
    for x in reversed(xs):
        if hi - x <= 20:
            lo = x
        else:
            break
    return (lo + hi) // 2, 62


def main():
    real = json.load(open(REAL, encoding="utf-8-sig"))
    rs = real.get("settings", {})
    os.environ["QB_APPDATA"] = SANDBOX
    cfg = json.load(open(CFG, encoding="utf-8-sig"))
    for k in ("ui_scale", "theme", "sidebar_w", "sidebar_collapsed"):
        if k in rs:
            cfg["settings"][k] = rs[k]
    cfg["settings"]["inspector_open"] = True
    cfg["settings"]["sidebar_collapsed"] = False
    paths = [e["path"] for e in cfg["repos"] if e["name"] == "demo-dirty"]
    assert paths, "沙箱里没有 demo-dirty"
    cfg["selected_path"] = paths[0]
    json.dump(cfg, open(CFG, "w", encoding="utf-8"), ensure_ascii=False)

    before = real_cfg_fp()
    env = launch_env()
    assert env["APPDATA"] == SANDBOX, "APPDATA 没指向沙箱：%s" % env["APPDATA"]
    p = subprocess.Popen([EXE], env=env)
    try:
        hwnd = steps.find_window(p.pid)
        assert hwnd, "找不到窗口"
        assert steps.ensure_focus(hwnd), "窗口拿不到前台"
        park_cursor(hwnd)
        steps.click(hwnd, 700, 300)          # 空点激活
        steps.ensure_focus(hwnd)
        steps.click(hwnd, 28, 94)            # 导航轨「我的仓库」
        time.sleep(1.2)
        steps.click(hwnd, 122, ROW_DIRTY)    # 选中 demo-dirty
        time.sleep(3.0)
        park_cursor(hwnd)
        f1 = os.path.join(TMP, "g1_open.png")
        shoot.capture(hwnd, f1)
        w1 = win_w(hwnd)
        print("① 抽屉开：客户区宽 %d（算式给的数 = %d）" % (w1, 40 + 1 + 8 + 950 + 263 + 320))
        print("   帧上白面 y=300:", white_runs(f1, 300, 8, 1600))

        # ② 切到总览（那一屏两栏都没有）-> 该收回 1280
        steps.ensure_focus(hwnd)
        steps.click(hwnd, 28, 50)
        time.sleep(2.2)
        park_cursor(hwnd)
        f2 = os.path.join(TMP, "g2_overview.png")
        shoot.capture(hwnd, f2)
        w2 = win_w(hwnd)
        print("② 切到总览：客户区宽 %d（期望收回 1280）" % w2)

        # ③ 切回仓库页（config 里 inspector_open 还开着、选中也还在）-> 该再撑到 1502
        steps.ensure_focus(hwnd)
        steps.click(hwnd, 28, 94)
        time.sleep(2.6)
        park_cursor(hwnd)
        f3 = os.path.join(TMP, "g3_back.png")
        shoot.capture(hwnd, f3)
        w3 = win_w(hwnd)
        print("③ 切回仓库页：客户区宽 %d（期望再撑回 1582，且左栏仍是 257）" % w3)
        print("   帧上白面 y=300:", white_runs(f3, 300, 8, 1600))

        # ④ 点抽屉自己那颗收起键 -> 收回 1280（`shell_grown_w` 那条还原路径）。
        #    键的位置从③那一帧现量，不写死
        runs3 = white_runs(f3, 300, 8, 1700)
        drawer_left = runs3[-1][0]
        fx, fy = find_collapse(f3, drawer_left)
        print("   收起键量到：帧 (%d,%d) → 客户区 (%d,%d)" % (fx, fy, fx - 8, fy - 8))
        steps.ensure_focus(hwnd)
        steps.click(hwnd, fx - 8, fy - 8)
        time.sleep(2.5)
        park_cursor(hwnd)
        f4 = os.path.join(TMP, "g4_closed.png")
        shoot.capture(hwnd, f4)
        w4 = win_w(hwnd)
        print("④ 点收起键之后：客户区宽 %d（期望回到没撑之前的 1280）" % w4)
        print("   帧上白面 y=300:", white_runs(f4, 300, 8, 1600))

        # ⑤ 拖侧栏把手 +40 -> 侧栏变宽、窗口跟着向右撑那 40，**中间那一列仍是 950**
        #    （同一条规则的另一种触发：拖把手也是"要更多宽"，不该拿中间列去垫）
        steps.ensure_focus(hwnd)
        drag(hwnd, HANDLE[0], HANDLE[1], 40)
        park_cursor(hwnd)
        f5 = os.path.join(TMP, "g5_dragged.png")
        shoot.capture(hwnd, f5)
        w5 = win_w(hwnd)
        print("⑤ 拖侧栏把手 +40 之后：客户区宽 %d（④是 %d，期望长出接近 40）" % (w5, w4))
        print("   帧上白面 y=300:", white_runs(f5, 300, 8, 1600))

        ok = [
            ("①抽屉开撑到算式给的数", abs(w1 - 1582) <= 2),
            ("②切总览收回去", abs(w2 - 1280) <= 2),
            ("③切回来再撑一次", abs(w3 - 1582) <= 2),
            ("④点收起键收回去", abs(w4 - 1280) <= 2),
            ("⑤拖把手窗口跟着长", w5 - w4 >= 10),
        ]
        # 侧栏那一格确实跟着把手变宽了（否则"窗口跟着长"只是因为拖拽整个没生效）
        sw4 = white_runs(f4, 300, 40, 340)
        sw5 = white_runs(f5, 300, 40, 400)
        a = sw4[0][1] if sw4 else 0
        b = sw5[0][1] if sw5 else 0
        ok.append(("⑤侧栏真的被拖宽了", b - a >= 30))
        print("   侧栏白面：拖前 %d -> 拖后 %d" % (a, b))
        # ⑤ 真正的不变量不是"窗口长 40"，是**中间那一列还是 950**：
        # 拖之前窗口本来就比需求宽 18（1280 vs 1262），那 18 先垫掉，
        # 所以窗口只长了 40 − 18 = 22 上下。用帧上的白面宽反算中间列
        panel5 = b + 6                       # 侧栏面板宽 = 卡白面 + 它自己那 6px 内缩
        main5 = w5 - 41 - panel5 - 8         # 客户区 − 轨 41 − 侧栏 − 右缘 8
        ok.append(("⑤拖完中间列仍是 950", abs(main5 - 950) <= 3))
        print("   ⑤反算中间列 = %d − 41 − %d − 8 = %d（目标 950）" % (w5, panel5, main5))
        # 左栏宽在①③两次里必须是同一个数（抢宽那一族缺陷的判据）
        s1 = white_runs(f1, 300, 40, 340)
        s3 = white_runs(f3, 300, 40, 340)
        v1 = s1[0][1] if s1 else 0
        v3 = s3[0][1] if s3 else 0
        ok.append(("①③左栏宽一致（没被右栏抢）", abs(v1 - v3) <= 1))
        print("   左栏白面：①%d ③%d" % (v1, v3))
        print("\n判读：")
        for name, passed in ok:
            print("   %s  %s" % ("✓" if passed else "✗", name))
        if not all(p for _, p in ok):
            print("   ⇒ 有没过的，别当过了")
    finally:
        p.terminate()
        time.sleep(0.6)
        check_real_cfg(before)


if __name__ == "__main__":
    main()
