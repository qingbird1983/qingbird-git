"""阶段 22 专用探针：发 `WM_CLOSE` 看关窗三档真的怎么走。

**为什么不用 `steps.py` 点 X**：X 在非客户区，鼠标点击走 `HTCLOSE`；
而 `steps.py` 送不进按键（`keybd_event` / `PostMessageW` 对键盘都试过不行，
见 `steps.py` 文件头）。**但 `WM_CLOSE` 是窗口消息不是键盘输入**——
`PostMessageW(hwnd, WM_CLOSE, 0, 0)` 直接进窗口过程，不经系统输入队列，
与那条键盘路是两码事。

⚠️ **`env=` 不能省**：`store::config_path()` 读 `APPDATA`，而 `Popen` 默认继承
调用方 shell 的环境 = 用户真实的 `%APPDATA%`。漏掉时脚本照样跑、弹窗照样弹，
抓帧里**用户的 17 个真实仓库赫然在列**。

用法（先设 `QB_APPDATA`，**不设就别跑**）：
    $env:QB_APPDATA = "$env:TEMP\\qb_close"
    python scripts/close_probe.py remember-tray   # 勾记住 + 关闭到托盘
    python scripts/close_probe.py remember-exit   # 勾记住 + 完全退出
    python scripts/close_probe.py plain-tray      # 不勾 + 关闭到托盘（不该落盘）
    python scripts/close_probe.py cancel          # 取消：窗口应当还在
    python scripts/close_probe.py tray            # 已记住 tray：直接关、不弹
    python scripts/close_probe.py exit            # 已记住 exit：直接退出

**坐标是量出来的，不是目测的**（`scripts/scancols.py` 逐列扫暗像素）：
复选框 x455..467、取消 x463..487、完全退出 x701..750、关闭到托盘 x760..840，
都在 1296×828 的窗口内坐标里，y 分别 210 / 248。改弹窗版式要重量。
"""
import ctypes
import ctypes.wintypes as wt
import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import qbguard  # noqa: E402
import shoot  # noqa: E402

user32 = ctypes.windll.user32
WM_CLOSE = 0x0010

REPO = r"F:\AIwork\qingbird-git"
EXE = os.path.join(REPO, "target", "debug", "qingbird-git.exe")
CFG = os.path.join(os.environ.get("QB_APPDATA", ""), "com.qingbird.git", "config.json")

# 窗口内相对坐标（1296×828 时量出来的）
REMEMBER = (461, 210)
CANCEL = (475, 248)
QUIT = (725, 248)
TRAY = (800, 248)


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
        time.sleep(0.2)
    return None


def origin(hwnd):
    r = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    return r.left, r.top


def activate(hwnd):
    """把窗口拉到前台。**少了这一步，第一次点击会被系统吃掉只用来激活窗口**——
    表现是「点了没反应」，而那一档恰好是本项目 AGENTS.md 记的纪律③。

    `remember-tray` 那一轮点过两次（先复选框再按钮），第二次落在窗口已激活之后
    就生效了，于是**只有不点复选框的 `plain-tray` 露了馅**——
    一次点击被吃掉，看起来像「关闭到托盘那个键是坏的」。
    """
    user32.ShowWindow(hwnd, 9)  # SW_RESTORE
    user32.SetForegroundWindow(hwnd)
    user32.BringWindowToTop(hwnd)
    time.sleep(0.4)
    # 焦点可能还在别的进程上，SetForegroundWindow 常常直接失败，
    # 那就再用一次前台返回空点击把它顶过来
    if user32.GetForegroundWindow() != hwnd:
        click_abs((origin(hwnd)[0] + 20, origin(hwnd)[1] + 400))
        time.sleep(0.3)


def click_abs(x, y):
    user32.SetCursorPos(int(x), int(y))
    time.sleep(0.15)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.06)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(0.4)


def click_rel(hwnd, rel):
    ox, oy = origin(hwnd)
    click_abs(ox + rel[0], oy + rel[1])


def proc_alive(pid):
    out = subprocess.run(["tasklist", "/FI", f"PID eq {pid}", "/NH"],
                         capture_output=True, text=True, errors="replace").stdout
    return str(pid) in out


def read_cfg():
    try:
        with open(CFG, encoding="utf-8") as f:
            return json.load(f)["settings"].get("close_action", "<无此字段>")
    except Exception as e:  # noqa: BLE001
        return f"<读不到: {e}>"


def write_cfg(raw):
    with open(CFG, encoding="utf-8") as f:
        cfg = json.load(f)
    cfg["settings"]["close_action"] = raw
    with open(CFG, "w", encoding="utf-8", newline="") as f:
        json.dump(cfg, f, ensure_ascii=False, indent=2)


def shot(hwnd, name):
    p = os.path.join(os.environ["TEMP"], f"qb_close_{name}.png")
    # ⚠️ 要捕 `BaseException` 不是 `Exception`：窗口已销毁时 `shoot` 抛的是
    # `SystemExit`（脚本里的 `sys.exit`），`Exception` 捕不住，
    # 会把"抓不到帧"升级成"整个探针退出"，后面几步的结果全丢
    if not hwnd or not user32.IsWindow(hwnd):
        print(f"    （{name}: 窗口已销毁，跳过抓帧）")
        return p
    try:
        shoot.capture(hwnd, p)
    except BaseException:  # noqa: BLE001
        print(f"    （{name}: 抓帧失败，不影响结论）")
    return p


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "ask"
    if not os.environ.get("QB_APPDATA"):
        print("!! 没设 QB_APPDATA —— 会碰到真实配置，先停下")
        return 2
    before = qbguard.real_cfg_fp()
    env = qbguard.launch_env()
    print(f"模式 = {mode}")

    if mode in ("tray", "exit"):
        write_cfg("tray" if mode == "tray" else "exit")
        print(f"已把副本配置写成 close_action={mode}")

    proc = subprocess.Popen([EXE], cwd=REPO, env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    hwnd = find_window(proc.pid)
    if not hwnd:
        print("!! 没找到窗口")
        proc.kill()
        return 1
    ox, oy = origin(hwnd)
    print(f"窗口 hwnd={hwnd} 原点=({ox},{oy})")
    time.sleep(1.5)
    print(f"启动后：config.close_action = {read_cfg()}")

    # ① 发 WM_CLOSE
    user32.PostMessageW(hwnd, WM_CLOSE, 0, 0)
    time.sleep(2.0)
    alive = bool(user32.IsWindow(hwnd))
    print(f"[1] WM_CLOSE 之后：窗口在={alive} 进程在={proc_alive(proc.pid)}")
    shot(hwnd, f"{mode}_1")

    if mode.startswith("remember") or mode == "plain-tray" or mode == "cancel":
        if not alive:
            print("!! 应当弹面板、窗口不关，却直接没了")
        else:
            activate(hwnd)
            if mode.startswith("remember"):
                click_rel(hwnd, REMEMBER)
                shot(hwnd, f"{mode}_2_remember")
                print("[2] 已点「记住我的选择」")
            target = CANCEL if mode == "cancel" else (
                TRAY if "tray" in mode else QUIT)
            click_rel(hwnd, target)
            time.sleep(2.0)
            print(f"[3] 已点第 {target} 那个键：窗口在={bool(user32.IsWindow(hwnd))} "
                  f"进程在={proc_alive(proc.pid)}")
            shot(hwnd, f"{mode}_3")
    else:
        print("[2] 该档不该弹面板；若上面窗口在=True 就是没按档走")

    print(f"[4] config.close_action = {read_cfg()}")
    if proc_alive(proc.pid):
        proc.terminate()
        time.sleep(0.6)
    if proc_alive(proc.pid):
        subprocess.run(["taskkill", "/F", "/PID", str(proc.pid)], capture_output=True)
    err = proc.stderr.read() if proc.stderr else b""
    print(f"stderr {len(err)} 字节")
    if err:
        print(err.decode("utf-8", "replace")[:2000])
    qbguard.check_real_cfg(before)
    return 0


if __name__ == "__main__":
    sys.exit(main())
