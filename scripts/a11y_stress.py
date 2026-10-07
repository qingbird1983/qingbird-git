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

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import qbguard  # noqa: E402
import steps as _steps  # noqa: E402
import shoot as _shoot  # noqa: E402
from PIL import Image, ImageChops  # noqa: E402

# ⚠️ **点击与找窗一律复用 `steps.py`，本文件不再自己写一份**（路线表 D93）。
# 这里原来有一份自己的 `click()`，内部是 `for _ in range(2): down/up`——
# **那是双击**。对"打开一个浮层"的目标，效果是"开完立刻被遮罩那一下关掉"，
# 而本脚本的判据只有 `alive`（= 没崩），于是**整轮走查什么都没点开也算通过**。
# 2026-10-07 设置页那轮就是这么被骗过去的：16 步全 alive，证据帧显示对话框没开。
# `steps.click` 是单击 + 前台确认 + 未激活时补一次空点，那份纪律写在它自己的注释里。
find_window = _steps.find_window
click = _steps.click

user32 = ctypes.windll.user32

REPO = r"F:\AIwork\qingbird-git"
EXE = os.path.join(REPO, "target", "debug", "qingbird-git.exe")
TMP = os.environ["TEMP"]
ERR = os.path.join(TMP, "qb_stress_err.txt")

# ⚠️ **2026-10-07 全部重新校准**（v4 阶段 31.4 之后）。原来那一份是 2026-10-05 量的：
# `NAV_MINE=(28,105)`、`TAB_Y=150`、右栏那颗 `(1264,65)`——**三个坐标全部过期**，
# 于是走查里"进我的仓库 / 五个页签 / 展开右栏"那 10 步**一次都没有命中过**，
# 而它每一步都报 `alive`。是这一步新加的"画面零变化要点名"把它们抓出来的
# （路线表 **D93**）。⇒ **改版之后必须重量坐标，别信脚本里写死的数**
NAV_OVERVIEW = (28, 71)
NAV_MINE = (28, 113)
NAV_THIRD = (28, 155)
ROW1 = (121, 92)

# 详情页页签（v4 之后**收进头卡底部**，不再是浮在卡外的一行）
TAB_Y = 198
TAB_XS = [452, 497, 543, 589, 635]

# 头卡右上角那颗"收起 / 展开检视抽屉"
INSPECTOR_TOGGLE = (1232, 92)


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

    before = qbguard.real_cfg_fp()
    env = qbguard.launch_env()
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
    prev = [None]
    SHOT_DIR = os.path.join(TMP, "qb_stress_frames")
    os.makedirs(SHOT_DIR, exist_ok=True)

    def step(label, fn):
        if proc.poll() is not None:
            steps.append((label, "ALREADY DEAD", None))
            return
        try:
            fn()
        except Exception as e:  # noqa: BLE001
            steps.append((label, f"EXC {e}", None))
            return
        state = "alive" if proc.poll() is None else "DIED"
        # ⚠️ **只判 `alive` 是不够的**（D93）：它的语义是"没崩"，不是"做到了"。
        # 上一版这里就是因为把 `alive` 当成了"点开了"，整轮走查其实什么都没点开。
        # ⇒ 每步抓一帧，与上一步比：**画面零变化的步要点名**，
        # 让人去判断"这一步本来就不该变"还是"它根本没生效"
        changed = None
        try:
            path = os.path.join(SHOT_DIR, "f%02d.png" % len(steps))
            _shoot.capture(hwnd, path)
            cur = Image.open(path).convert("RGB")
            if prev[0] is not None:
                bb = ImageChops.difference(cur, prev[0]).getbbox()
                changed = 0 if bb is None else (bb[2] - bb[0]) * (bb[3] - bb[1])
            prev[0] = cur
        except Exception:  # noqa: BLE001
            changed = None
        steps.append((label, state, changed))

    # ---- 1. 仓库详情页，先把 a11y 激活 ----
    step("进我的仓库", lambda: click(hwnd, *NAV_MINE))
    time.sleep(0.8)
    step("点第一个仓库", lambda: click(hwnd, *ROW1))
    time.sleep(2.0)
    print("activate ->", activate(hwnd))
    time.sleep(1.2)
    steps.append(("激活后", "alive" if proc.poll() is None else "DIED", None))

    # ---- 2. 五个页签来回切 ----
    for i, x in enumerate(TAB_XS):
        step(f"页签{i}", lambda x=x: click(hwnd, x, TAB_Y, settle=0.8))
    step("回概览", lambda: click(hwnd, TAB_XS[0], TAB_Y, settle=0.8))

    # ---- 3. 右栏展开 / 收起（仓库头右上角那颗）----
    step("展开右栏", lambda: click(hwnd, *INSPECTOR_TOGGLE, settle=1.0))
    step("收起右栏", lambda: click(hwnd, *INSPECTOR_TOGGLE, settle=1.0))

    # ---- 4. ⌘K 开 / Esc 关 ----
    step("开命令面板", key_ctrl_k)
    step("关命令面板", lambda: user32.keybd_event(0x1B, 0, 0, 0) or time.sleep(0.1)
         or user32.keybd_event(0x1B, 0, 2, 0) or time.sleep(0.9))

    # ---- 5. 总览页 ----
    step("去总览", lambda: click(hwnd, *NAV_OVERVIEW, settle=1.4))
    time.sleep(0.8)
    steps.append(("总览稳定后", "alive" if proc.poll() is None else "DIED", None))

    # ---- 6. 第三方仓库（profile_body，另一套版面）----
    step("去第三方", lambda: click(hwnd, *NAV_THIRD, settle=1.2))
    time.sleep(0.8)
    step("点第一个第三方", lambda: click(hwnd, *ROW1, settle=1.6))
    time.sleep(1.2)
    steps.append(("第三方稳定后", "alive" if proc.poll() is None else "DIED", None))

    time.sleep(1.0)
    err.close()

    print("---- 走查 ----")
    still = []
    for label, st, changed in steps:
        flag = "  <<<" if st in ("DIED",) or st.startswith("EXC") or st.startswith("ALREADY") else ""
        mark = "" if changed is None else ("  [画面没变]" if changed == 0 else "  Δ%dpx" % changed)
        if changed == 0:
            still.append(label)
        print(f"  {label:16s} {st}{mark}{flag}")
    if still:
        # 不自动判失败：有些步**本来就不该改变画面**（比如关掉一个已经关着的东西）。
        # 但**它必须被看见**——"画面没变"正是 D93 那一轮唯一的线索
        print("静止的步（逐条判断：本来就不该变 / 那一步根本没生效）:", "、".join(still))

    size = os.path.getsize(ERR) if os.path.exists(ERR) else 0
    print(f"stderr_bytes={size}")
    if size:
        for ln in open(ERR, encoding="utf-8", errors="replace"):
            if "A11Y-DUP" in ln or "panicked" in ln or "Duplicate" in ln:
                print("  >>", ln.strip())

    if proc.poll() is None:
        proc.kill()
    time.sleep(0.5)
    qbguard.check_real_cfg(before)
    return 0


if __name__ == "__main__":
    sys.exit(main())
