"""在**无障碍已激活**的条件下把整个界面走一遍，专抓潜伏的重复 ElementId。

为什么必须先激活 a11y 再走查：gpui 的无障碍树**只在平台适配器的 activation
回调被触发时才建**（`window/a11y.rs:212` 的 `active_flag`）。平时跑，整棵树
根本不建，`Duplicate a11y node id` 也就不会响——2026-10-05 那个 bug 正是
这样被漏成"偶发"的：它在开发时表现为"进详情页偶尔崩一下"。

做法：先进详情页 → 用 .NET UIAutomation 当一次无障碍客户端把 flag 置上 →
之后 flag 一直有效，**每一次重绘都会重建整棵树**，于是任何一处重复 id
都会当场 panic。走查覆盖：五个页签、右栏收/展、⌘K、总览页、第三方仓库。
"""

import ctypes
import ctypes.wintypes as wt
import glob
import os
import subprocess
import sys
import time

user32 = ctypes.windll.user32

REPO = r"F:\AIwork\qingbird-git"
EXE = os.path.join(REPO, "target", "debug", "qingbird-git.exe")
TMP = os.environ["TEMP"]
ERR = os.path.join(TMP, "qb_stress_err.txt")

NAV_OVERVIEW = (28, 68)
NAV_MINE = (28, 105)
NAV_THIRD = (28, 142)
ROW1 = (121, 144)

# 详情页页签（抓帧校准：概览/说明/历史/空间/变更 在 y=150 一行）
TAB_Y = 150
TAB_XS = [289, 330, 370, 410, 446]


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
            area = (r.right - r.left) * (r.bottom - r.top)
            if area > 20000:
                found.append((area, hwnd))
            return True

        user32.EnumWindows(cb, 0)
        if found:
            found.sort(reverse=True)
            return found[0][1]
        time.sleep(0.4)
    return None


def click(hwnd, x, y, settle=0.7):
    pt = wt.POINT(int(x), int(y))
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    user32.SetCursorPos(pt.x, pt.y)
    time.sleep(0.15)
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.25)
    for _ in range(2):
        user32.mouse_event(0x0002, 0, 0, 0, 0)
        time.sleep(0.05)
        user32.mouse_event(0x0004, 0, 0, 0, 0)
        time.sleep(settle)


def key_ctrl_k():
    user32.keybd_event(0x11, 0, 0, 0)
    user32.keybd_event(0x4B, 0, 0, 0)
    time.sleep(0.06)
    user32.keybd_event(0x4B, 0, 2, 0)
    user32.keybd_event(0x11, 0, 2, 0)
    time.sleep(1.0)


PS_ACTIVATE = r"""
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$el = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]{hwnd})
if ($null -eq $el) {{ Write-Output "NO-ELEMENT"; exit 0 }}
$cond = [System.Windows.Automation.Condition]::TrueCondition
try {{ $all = $el.FindAll([System.Windows.Automation.TreeScope]::Subtree, $cond)
      Write-Output ("NODES=" + $all.Count) }}
catch {{ Write-Output ("ERR=" + $_.Exception.Message) }}
"""


def activate(hwnd):
    p = subprocess.run(
        ["powershell", "-NoProfile", "-Command", PS_ACTIVATE.format(hwnd=hwnd)],
        capture_output=True,
        text=True,
        timeout=90,
    )
    return p.stdout.strip().replace("\n", " ")


def main():
    subprocess.run(["taskkill", "/F", "/IM", "qingbird-git.exe"], capture_output=True)
    time.sleep(0.8)
    if os.path.exists(ERR):
        os.remove(ERR)

    env = dict(os.environ)
    env["RUST_BACKTRACE"] = "1"
    err = open(ERR, "wb")
    proc = subprocess.Popen(
        [EXE], cwd=REPO, env=env, stdout=subprocess.DEVNULL, stderr=err
    )
    hwnd = find_window(proc.pid)
    if not hwnd:
        print("NO WINDOW")
        proc.kill()
        return 1
    print(f"hwnd={hwnd}")

    steps = []

    def step(label, fn):
        if proc.poll() is not None:
            steps.append((label, "ALREADY DEAD"))
            return
        try:
            fn()
        except Exception as e:  # noqa: BLE001
            steps.append((label, f"EXC {e}"))
            return
        steps.append((label, "alive" if proc.poll() is None else "DIED"))

    # ---- 1. 仓库详情页，先把 a11y 激活 ----
    step("进我的仓库", lambda: click(hwnd, *NAV_MINE))
    time.sleep(0.8)
    step("点第一个仓库", lambda: click(hwnd, *ROW1))
    time.sleep(2.0)
    print("activate ->", activate(hwnd))
    time.sleep(1.2)
    steps.append(("激活后", "alive" if proc.poll() is None else "DIED"))

    # ---- 2. 五个页签来回切 ----
    for i, x in enumerate(TAB_XS):
        step(f"页签{i}", lambda x=x: click(hwnd, x, TAB_Y, settle=0.8))
    step("回概览", lambda: click(hwnd, TAB_XS[0], TAB_Y, settle=0.8))

    # ---- 3. 右栏展开 / 收起（仓库头右上角那颗）----
    step("展开右栏", lambda: click(hwnd, 1264, 65, settle=1.0))
    step("收起右栏", lambda: click(hwnd, 1264, 65, settle=1.0))

    # ---- 4. ⌘K 开 / Esc 关 ----
    step("开命令面板", key_ctrl_k)
    step("关命令面板", lambda: user32.keybd_event(0x1B, 0, 0, 0) or time.sleep(0.1)
         or user32.keybd_event(0x1B, 0, 2, 0) or time.sleep(0.9))

    # ---- 5. 总览页 ----
    step("去总览", lambda: click(hwnd, *NAV_OVERVIEW, settle=1.4))
    time.sleep(0.8)
    steps.append(("总览稳定后", "alive" if proc.poll() is None else "DIED"))

    # ---- 6. 第三方仓库（profile_body，另一套版面）----
    step("去第三方", lambda: click(hwnd, *NAV_THIRD, settle=1.2))
    time.sleep(0.8)
    step("点第一个第三方", lambda: click(hwnd, *ROW1, settle=1.6))
    time.sleep(1.2)
    steps.append(("第三方稳定后", "alive" if proc.poll() is None else "DIED"))

    time.sleep(1.0)
    err.close()

    print("---- 走查 ----")
    for label, st in steps:
        flag = "  <<<" if st in ("DIED",) or st.startswith("EXC") or st.startswith("ALREADY") else ""
        print(f"  {label:16s} {st}{flag}")

    size = os.path.getsize(ERR) if os.path.exists(ERR) else 0
    print(f"stderr_bytes={size}")
    if size:
        for ln in open(ERR, encoding="utf-8", errors="replace"):
            if "A11Y-DUP" in ln or "panicked" in ln or "Duplicate" in ln:
                print("  >>", ln.strip())

    if proc.poll() is None:
        proc.kill()
    return 0


if __name__ == "__main__":
    sys.exit(main())
