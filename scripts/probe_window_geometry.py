"""窗口几何探针：连续启动 app 两次，记录窗口矩形、客户区、DPI、监视器信息。

用途：排查抓帧 diff 的「文字抗锯齿相位漂移」是否由窗口几何（位置/尺寸/DPI）
逐运行变化引起。两次输出逐字段对比即可判定。
"""
import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import p2_dlg_shots as P  # noqa: E402

user32 = ctypes.windll.user32
MONITOR_DEFAULTTONEAREST = 2


class MONITORINFOEXW(ctypes.Structure):
    _fields_ = [
        ("cbSize", wt.DWORD),
        ("rcMonitor", wt.RECT),
        ("rcWork", wt.RECT),
        ("dwFlags", wt.DWORD),
        ("szDevice", wt.CHAR * 32),
    ]


def probe(tag):
    P.build_sandbox()
    subprocess.run(["taskkill", "/F", "/IM", "qingbird-git.exe"], capture_output=True)
    time.sleep(0.8)
    env = P.qbguard.launch_env()
    env["RUST_BACKTRACE"] = "1"
    err = open(P.ERR, "wb")
    proc = subprocess.Popen([P.EXE], cwd=P.REPO, env=env,
                            stdout=subprocess.DEVNULL, stderr=err)
    try:
        hwnd = P._steps.find_window(proc.pid)
        assert hwnd, "找不到窗口"
        print(f"[{tag}] hwnd={hwnd}")
        for i in range(12):
            time.sleep(1.0)
            r = wt.RECT()
            user32.GetWindowRect(hwnd, ctypes.byref(r))
            w, h = r.right - r.left, r.bottom - r.top
            if w <= 0:
                continue
            pt = wt.POINT(0, 0)
            user32.ClientToScreen(hwnd, ctypes.byref(pt))
            cr = wt.RECT()
            user32.GetClientRect(hwnd, ctypes.byref(cr))
            dpi = user32.GetDpiForWindow(hwnd)
            hmon = user32.MonitorFromPoint(wt.POINT(r.left, r.top), MONITOR_DEFAULTTONEAREST)
            mi = MONITORINFOEXW()
            mi.cbSize = ctypes.sizeof(MONITORINFOEXW)
            user32.GetMonitorInfoW(hmon, ctypes.byref(mi))
            if w > 1000 or i == 11:
                print(f"[{tag}] t+{i+1}s rect=({r.left},{r.top})-({r.right},{r.bottom}) "
                      f"{w}x{h} client_origin=({pt.x},{pt.y}) client={cr.right-cr.left}x{cr.bottom-cr.top} "
                      f"dpi={dpi} mon={mi.rcMonitor.left},{mi.rcMonitor.top}-"
                      f"{mi.rcMonitor.right},{mi.rcMonitor.bottom} "
                      f"flags={mi.dwFlags}")
                break
    finally:
        subprocess.run(["taskkill", "/F", "/IM", "qingbird-git.exe"], capture_output=True)


if __name__ == "__main__":
    for t in sys.argv[1:] or ["p1", "p2"]:
        probe(t)
