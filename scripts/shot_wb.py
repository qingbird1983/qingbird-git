"""抓「我的仓库」工作台页（或第三方页）一帧，环境一律对齐用户真实 config。

与 `shot_now.py` 同一套三道闸：`QB_APPDATA` 沙箱 + `assert env["APPDATA"]==沙箱`
+ 收尾核对真实 config 指纹。三条纪律（AGENTS / 路线表 D93 / D97 都记着踩过）：
  ① **沙箱的 `ui_scale` / `theme` / `inspector_open` / `sidebar_open` 必须抄真实 config**
     ——上一轮就是因为沙箱停在 `large` 而用户是 `medium`，一整批 px 数全废；
  ② **光标移到「这个窗口矩形之外」**，不是写死坐标（写死会随窗口位置过期，
     而且 tooltip 盖住页头会被误判成"标题没了"）；
  ③ 点击一律复用 `steps.click`（单击 + 前台确认），不用 `a11y_stress` 那份双击。

用法：python target/shot_wb.py [仓库名，默认 demo-dirty] [输出名，默认 wb]
"""
import json
import os
import subprocess
import sys
import time

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

TMP = r"C:\Users\42527\AppData\Local\Temp\qb_v4"
SANDBOX = os.path.join(TMP, "appdata")
REAL = r"C:\Users\42527\AppData\Roaming\com.qingbird.git\config.json"
CFG = os.path.join(SANDBOX, "com.qingbird.git", "config.json")
EXE = r"F:\AIwork\qingbird-git\target\debug\qingbird-git.exe"

want_repo = sys.argv[1] if len(sys.argv) > 1 else "demo-dirty"
out_name = sys.argv[2] if len(sys.argv) > 2 else "wb"
view = sys.argv[3] if len(sys.argv) > 3 else "mine"

os.environ["QB_APPDATA"] = SANDBOX
sys.path.insert(0, r"F:\AIwork\qingbird-git\scripts")
from qbguard import launch_env, real_cfg_fp, check_real_cfg  # noqa: E402
import shoot  # noqa: E402
import steps  # noqa: E402

# ---- ① 环境对齐：把真实 config 里那四个"会影响版面"的字段抄进沙箱 ----
real = json.load(open(REAL, encoding="utf-8-sig"))
rs = real.get("settings", {})
cfg = json.load(open(CFG, encoding="utf-8-sig"))
for k in ("ui_scale", "theme", "inspector_open", "sidebar_w", "sidebar_collapsed"):
    if k in rs:
        cfg["settings"][k] = rs[k]
cfg["settings"]["inspector_open"] = False   # 他真实那份就是关着的
cfg["settings"]["sidebar_collapsed"] = False
paths = [e["path"] for e in cfg["repos"] if e["name"] == want_repo]
if want_repo != "-":
    assert paths, "沙箱里没有仓库叫 %s，只有：%s" % (
        want_repo, [e["name"] for e in cfg["repos"]])
    cfg["selected_path"] = paths[0]
json.dump(cfg, open(CFG, "w", encoding="utf-8"), ensure_ascii=False)
print("沙箱已对齐：ui_scale=%s theme=%s selected=%s"
      % (cfg["settings"]["ui_scale"], cfg["settings"]["theme"], want_repo))

before = real_cfg_fp()
env = launch_env()
assert env["APPDATA"] == SANDBOX, "APPDATA 没指向沙箱，拒绝启动：%s" % env["APPDATA"]
p = subprocess.Popen([EXE], env=env)
hwnd = steps.find_window(p.pid)
assert hwnd, "找不到窗口"
assert steps.ensure_focus(hwnd), "窗口拿不到前台"

# ---- ② 光标移到窗口矩形之外，移两次让"离开"真的产出一个 move ----
import ctypes
u = shoot.user32
r = shoot.wt.RECT()
u.GetWindowRect(hwnd, ctypes.byref(r))
ox = r.left - 60 if r.left - 60 > 0 else r.right + 60
oy = r.top - 60 if r.top - 60 > 0 else r.bottom + 60
u.SetCursorPos(ox, oy)
time.sleep(0.4)
u.SetCursorPos(ox - 8, oy - 8)
time.sleep(1.2)
# ⚠️ **先空点一次把窗口激活**（D93 那一族的第二次实锤，见 `shot_box.py`）：
# 未获焦窗口的第一击只用于激活、不派发给元素。2026-10-08 这一版漏了它，
# 结果"点三方导航"整步没生效，抓回来的还是「我的仓库」那一屏
steps.click(hwnd, 700, 300)
time.sleep(0.8)
steps.ensure_focus(hwnd)

# ⚠️ **光设 `selected_path` 不会切屏**——view 是运行期状态、不在 config 里，
# 启动永远停在总览（本轮第一次抓帧与总览那张**字节数完全相同**才发现）。
# 要进工作台必须真的点导航轨那颗键。导航键的中心 y 是**实测**的：
# 概览 50 / 我的 94 / 三方 138 / 设置 755（客户区坐标，`NAV_W`40 + `NAV_PAD`4 +
# 32 高的键 + `GAP_LG`12 一档，改轨的任何一档都要回来重量这些数——D93）
steps.click(hwnd, 28, {"mine": 94, "third": 138}.get(view, 94))
time.sleep(1.2)

# ⚠️ **`selected_path` 在启动时不切屏**（view 是运行期状态），所以"看哪个仓库"
# 必须真的去点侧栏那一行。
# 下面五个 y 是**逐行暗像素量出来的**（帧中心 − 8 = 客户区）。
# 2026-10-08 换成 36 高自绘行之后重测一遍：笔画带 113..120 / 151..158 /
# （第三行图标是浅色的没抓到，按等距补）/ 224..237 / 265..272，
# 中心 116.5 / 154.5 / 192.5 / 230.5 / 268.5，**行距实测 38 = 36 高 + 2 gap** ✓
# 上一版这张表是我目测填的（89/129/169/209/249），**五行全部点偏**——
# 结果把分组标题那颗折叠键按下去了，侧栏一度看起来"行没了"，差点当成渲染 bug 去查。
# ⇒ 路线表 D93 记过两次，这是第三次：**坐标一律量出来再生成，不许手填**
ROW_Y = {
    "demo-clean": 108, "demo-behind": 146, "demo-ahead": 184,
    "demo-dirty": 222, "no-upstream": 260,
}
if want_repo in ROW_Y:
    steps.click(hwnd, 122, ROW_Y[want_repo])
    time.sleep(1.4)
time.sleep(2.6)   # 等档案扫描与 README 读完，画面进稳定态

out = os.path.join(TMP, out_name + ".png")
shoot.capture(hwnd, out)
print("抓到", out, os.path.getsize(out), "｜窗口矩形", (r.left, r.top, r.right, r.bottom))

p.terminate()
time.sleep(0.6)
check_real_cfg(before)
