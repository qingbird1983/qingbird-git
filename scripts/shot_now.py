"""沙箱起 exe、抓总览页一帧，量 2026-10-08 那四条缩进落没落。

三道闸（AGENTS / 记忆都记着踩过）：
  ① `QB_APPDATA` 必须在 import qbguard 之前设好，且 `launch_env()` 出来的
     `APPDATA` 要 **assert** 等于沙箱——上一轮这里是 `print`，打印了 False 还照跑，
     结果拿用户真实配置起了进程。
  ② 出图前打 exe 的 mtime + md5，与刚编完的那份一致才算数。
  ③ 收尾 `check_real_cfg` 核对真实 config 的指纹。
"""
import ctypes
import hashlib
import json
import os
import subprocess
import sys
import time

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

TMP = r"C:\Users\42527\AppData\Local\Temp\qb_v4"
SANDBOX = os.path.join(TMP, "appdata")
EXE = r"F:\AIwork\qingbird-git\target\debug\qingbird-git.exe"
REAL_CFG = r"C:\Users\42527\AppData\Roaming\com.qingbird.git\config.json"

os.environ["QB_APPDATA"] = SANDBOX
sys.path.insert(0, r"F:\AIwork\qingbird-git\scripts")
from qbguard import launch_env, real_cfg_fp, check_real_cfg  # noqa: E402
import shoot  # noqa: E402

# 沙箱 config：把选中项清掉，保证起来就停在总览页
# ⚠️ 抽屉（inspector）按**用户真实态**关掉（他那份 config 是 false）：
# 开着抽屉量的"右缩进"里混着抽屉宽，跟他屏幕上那四条数不是同一个东西
dst = os.path.join(SANDBOX, "com.qingbird.git", "config.json")
with open(dst, "r", encoding="utf-8-sig") as f:
    cfg = json.load(f)
cfg["selected_path"] = None
cfg["sidebar_open"] = True
cfg["settings"]["inspector_open"] = False
with open(dst, "w", encoding="utf-8") as f:
    json.dump(cfg, f, ensure_ascii=False)

h = hashlib.md5(open(EXE, "rb").read()).hexdigest()
print("exe md5 =", h, " mtime =", time.strftime("%H:%M:%S", time.localtime(os.path.getmtime(EXE))))

before = real_cfg_fp()
env = launch_env()
assert env["APPDATA"] == SANDBOX, "APPDATA 没指向沙箱，拒绝启动：%s" % env["APPDATA"]
p = subprocess.Popen([EXE], env=env)
print("pid", p.pid)

hwnd = None
for _ in range(40):
    time.sleep(0.4)
    r = shoot.find("", pid=p.pid)
    hwnd = r[0] if isinstance(r, (tuple, list)) else r
    if hwnd:
        break
assert hwnd, "40 次没找到窗口"
print("hwnd", hwnd)

user32 = shoot.user32
user32.SetForegroundWindow(hwnd)
time.sleep(2.0)
# **把光标移到「这个窗口矩形之外」**，不是一个写死的屏幕坐标。
# 原来写的是 `SetCursorPos(1900, 1240)`——上一轮 `steps.py` 点在导航轨上的位置
# 会留下 tooltip，而"移到某个绝对坐标"在窗口位置变了之后可能**仍落在窗口里**
# （本轮实测：帧上页头标题被导航键的 tooltip「我的仓库」盖住，看着像标题没了）。
# 判据与 D93 同族：**写死的坐标会过期，要算**。
rect = shoot.wt.RECT()
user32.GetWindowRect(hwnd, ctypes.byref(rect))
out_x = rect.left - 60 if rect.left - 60 > 0 else rect.right + 60
out_y = rect.top - 60 if rect.top - 60 > 0 else rect.bottom + 60
user32.SetCursorPos(out_x, out_y)
time.sleep(0.4)
user32.SetCursorPos(out_x - 8, out_y - 8)  # 再动一次：让"离开"真的变成一个 move 事件
time.sleep(1.2)
out = os.path.join(TMP, "v_now_overview.png")
shoot.capture(hwnd, out)
print("抓到", out, os.path.getsize(out), "｜光标已移到窗口外", (out_x, out_y),
      "｜窗口矩形", (rect.left, rect.top, rect.right, rect.bottom))

p.terminate()
time.sleep(0.6)
check_real_cfg(before)
