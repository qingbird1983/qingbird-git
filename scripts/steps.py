"""跑一串点击并逐步抓帧——用来验"看得见但没点过"的那些路径。

每步一个 token：<动作>:<参数，逗号分隔>
  nav:<y>          点导航轨（x=28，y 自带：概览 68 / 我的 105 / 第三方 142）
  row:<x>,<y>      点侧栏某一行
  tab:<x>          点详情页页签（y=150）
  click:<x>,<y>[,<settle秒>]
  shot:<名字>      抓一帧到 %TEMP%\<名字>.png
  wait:<秒>        只等

用法：python scripts/steps.py "nav:105" "row:121,120" "wait:2" "tab:474" "wait:2" "shot:qb_a"

**先设 QB_APPDATA 指向 $TEMP 下的配置副本**，否则跑的是用户真实配置、会把
用户的设置写掉（脚本收尾会核对真实配置的指纹并在不一致时报警）：
  $env:QB_APPDATA = "$env:TEMP\\qbapp"
"""

import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import qbguard  # noqa: E402
import shoot  # noqa: E402

user32 = ctypes.windll.user32

REPO = r"F:\AIwork\qingbird-git"
EXE = os.path.join(REPO, "target", "debug", "qingbird-git.exe")


def find_window(pid, timeout=25.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        found = []

        @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
        def cb(hwnd, _):
            p = wt.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
            if p.value != pid or not user32.IsWindowVisible(hwnd):
                return True
            r = wt.RECT()
            user32.GetWindowRect(hwnd, ctypes.byref(r))
            if (r.right - r.left) * (r.bottom - r.top) > 20000:
                found.append(hwnd)
            return True

        user32.EnumWindows(cb, 0)
        if found:
            return found[0]
        time.sleep(0.4)
    return None


def ensure_focus(hwnd):
    """把窗口弄到前台并**确认**它真的在前台。

    AGENTS.md 那条纪律：窗口未获焦时第一击只用于激活窗口。`SetForegroundWindow`
    不保证立刻生效，所以这里轮询确认；确认不了就空点一下再确认。
    """
    for _ in range(3):
        user32.SetForegroundWindow(hwnd)
        time.sleep(0.25)
        if user32.GetForegroundWindow() == hwnd:
            return True
    # 退一步：用一次空点击把激活吃掉
    pt = wt.POINT(5, 400)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    user32.SetCursorPos(pt.x, pt.y)
    time.sleep(0.1)
    for _ in range(2):
        user32.mouse_event(0x0002, 0, 0, 0, 0)
        time.sleep(0.05)
        user32.mouse_event(0x0004, 0, 0, 0, 0)
        time.sleep(0.15)
    time.sleep(0.4)
    return user32.GetForegroundWindow() == hwnd


def click(hwnd, x, y, settle=0.7):
    """单击。**不要连点两下**——连着两次 down/up 会被当成双击，
    而且第一次的抬起常常落在窗口还没激活的时段里，白点。"""
    if user32.GetForegroundWindow() != hwnd:
        ensure_focus(hwnd)
    pt = wt.POINT(int(x), int(y))
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    user32.SetCursorPos(pt.x, pt.y)
    time.sleep(0.2)
    if user32.GetForegroundWindow() != hwnd:
        ensure_focus(hwnd)
        user32.SetCursorPos(pt.x, pt.y)
        time.sleep(0.2)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.08)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(settle)


def run_step(hwnd, step):
    verb, _, rest = step.partition(":")
    args = rest.split(",") if rest else []
    if verb == "nav":
        click(hwnd, 28, int(args[0]))
    elif verb == "row":
        click(hwnd, int(args[0]), int(args[1]))
    elif verb == "tab":
        click(hwnd, int(args[0]), 150, settle=1.6)
    elif verb == "click":
        settle = float(args[2]) if len(args) > 2 else 0.9
        click(hwnd, int(args[0]), int(args[1]), settle=settle)
    elif verb == "wait":
        time.sleep(float(args[0]))
    elif verb == "wheel":
        # 滚轮：<x>,<y>,<档数>（正数向下滚，WHEEL_DELTA=120）
        pt = wt.POINT(int(args[0]), int(args[1]))
        user32.ClientToScreen(hwnd, ctypes.byref(pt))
        user32.SetCursorPos(pt.x, pt.y)
        time.sleep(0.15)
        for _ in range(abs(int(args[2]))):
            user32.mouse_event(0x0800, 0, 0, -120 if int(args[2]) > 0 else 120, 0)
            time.sleep(0.06)
        time.sleep(0.5)
    elif verb == "shot":
        path = os.path.join(os.environ["TEMP"], args[0] + ".png")
        shoot.capture(hwnd, path)
        print("  shot ->", path)
    else:
        print("  ?? 不会的步骤:", step)


def main():
    steps = sys.argv[1:]
    err = os.path.join(os.environ["TEMP"], "qb_steps_err.txt")

    before = qbguard.real_cfg_fp()
    env = qbguard.launch_env()
    subprocess.run(["taskkill", "/F", "/IM", "qingbird-git.exe"],
                   capture_output=True)
    time.sleep(0.8)
    fh = open(err, "wb")
    proc = subprocess.Popen([EXE], cwd=REPO, env=env,
                            stdout=subprocess.DEVNULL, stderr=fh)
    hwnd = find_window(proc.pid)
    if not hwnd:
        print("NO WINDOW")
        proc.kill()
        return 1
    try:
        for s in steps:
            print(">", s)
            run_step(hwnd, s)
            if proc.poll() is not None:
                print("!! 进程死了，停在:", s)
                break
    finally:
        alive = proc.poll() is None
        fh.close()
        print("alive =", alive, "| stderr", os.path.getsize(err), "字节")
        if os.path.getsize(err):
            with open(err, encoding="utf-8", errors="replace") as f:
                print("--- stderr ---")
                print(f.read()[:3000])
        proc.kill()
        time.sleep(0.5)
        qbguard.check_real_cfg(before)
    return 0


if __name__ == "__main__":
    sys.exit(main())
