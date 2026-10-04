"""用 .NET UIAutomation 当一次"无障碍客户端"，强制 gpui 把 a11y 树建起来。

为什么需要这一步：gpui 的 a11y 树**只在平台适配器的 activation 回调被系统/
读屏软件触发时才建**（`window/a11y.rs:212` 的 `active_flag`，由
`window.rs:1626-1647` 的 `a11y_init` 回调置位）。没有环境变量、没有公开 API
能强开——`Application::new_inaccessible` 恰恰是把它**关**掉。

所以正常跑的时候整棵树是不建的，`Duplicate a11y node id` 那个 debug 断言
也就不会响。这解释了它为什么偶发：某一帧有个 UIA 客户端探了这个窗口。

Windows 上制造那个客户端最省事的办法就是 .NET 的 UIAutomation：
`AutomationElement.FromHandle(hwnd)` 之后碰一个属性，provider 就会被查询，
accesskit 随之激活。激活后 `Shell::render` 里那个临时探针就会把节点树
写到 `%TEMP%\\qb_a11y_*.json`，同时真撞上重复 id 就会带出回溯栈。
"""

import ctypes
import ctypes.wintypes as wt
import glob
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import qbguard  # noqa: E402

user32 = ctypes.windll.user32

REPO = r"F:\AIwork\qingbird-git"
EXE = os.path.join(REPO, "target", "debug", "qingbird-git.exe")
TMP = os.environ["TEMP"]
ERR = os.path.join(TMP, "qb_uia_err.txt")

NAV_MINE = (28, 105)
ROW1 = (121, 144)


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


def click(hwnd, x, y, settle=0.6):
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


PS_PROBE = r"""
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$el = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]{hwnd})
if ($null -eq $el) {{ Write-Output "NO-ELEMENT"; exit 0 }}
Write-Output ("NAME=" + $el.Current.Name)
$cond = [System.Windows.Automation.Condition]::TrueCondition
$all = $el.FindAll([System.Windows.Automation.TreeScope]::Subtree, $cond)
Write-Output ("DESCENDANTS=" + $all.Count)
"""


def main():
    subprocess.run(["taskkill", "/F", "/IM", "qingbird-git.exe"], capture_output=True)
    time.sleep(0.8)
    for f in glob.glob(os.path.join(TMP, "qb_a11y_*.json")):
        os.remove(f)
    if os.path.exists(ERR):
        os.remove(ERR)

    before = qbguard.real_cfg_fp()
    env = qbguard.launch_env()
    env["RUST_BACKTRACE"] = "full"
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

    click(hwnd, *NAV_MINE)
    time.sleep(1.0)
    click(hwnd, *ROW1)
    time.sleep(2.5)
    print("alive@detail =", proc.poll() is None)

    # 当一次 UIA 客户端
    script = PS_PROBE.format(hwnd=hwnd)
    p = subprocess.run(
        ["powershell", "-NoProfile", "-Command", script],
        capture_output=True,
        text=True,
        timeout=90,
    )
    print("---- UIA probe ----")
    print(p.stdout.strip()[:1500])
    if p.stderr.strip():
        print("stderr:", p.stderr.strip()[:600])

    time.sleep(2.0)
    print("alive_after_uia =", proc.poll() is None)
    err.close()
    size = os.path.getsize(ERR) if os.path.exists(ERR) else 0
    print(f"stderr_bytes={size}")
    if size:
        txt = open(ERR, encoding="utf-8", errors="replace").read()
        keep = [
            ln
            for ln in txt.splitlines()
            if "qingbird" in ln or "Duplicate" in ln or "panicked" in ln
        ]
        print("---- 相关栈帧 ----")
        for ln in keep[:50]:
            print(ln)

    print("---- a11y dumps ----")
    for f in sorted(glob.glob(os.path.join(TMP, "qb_a11y_*.json"))):
        print(f"  {os.path.basename(f)}  {os.path.getsize(f)}")

    if proc.poll() is None:
        proc.kill()
    time.sleep(0.5)
    qbguard.check_real_cfg(before)
    return 0


if __name__ == "__main__":
    sys.exit(main())
