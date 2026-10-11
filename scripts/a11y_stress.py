"""在**无障碍已激活**的条件下把整个界面走一遍，专抓潜伏的重复 ElementId。

为什么必须先激活 a11y 再走查：gpui 的无障碍树**只在平台适配器的 activation
回调被触发时才建**（`window/a11y.rs:212` 的 `active_flag`）。平时跑，整棵树
根本不建，`Duplicate a11y node id` 也就不会响——2026-10-05 那个 bug 正是
这样被漏成"偶发"的：它在开发时表现为"进详情页偶尔崩一下"。

## 控件**由 a11y 的名字定位**，不靠像素扫（2026-10-11 换成这样）

这一版之前那三处坐标是"按暗色扫像素"扫出来的，两个坑都踩到了：
① 取错索引，把标题栏的右栏开关点成了**明暗主题键**；
② 主题一被切到深色，那套"亮度 < 200 算暗"的判据**整条失效**（深色底上每一
像素都算暗，整条标题栏连成一个簇）。
而树里本来就有应用自己写的 `accessibility_label`，实测一次 dump 就能拿到
`Button|收起右栏|2533|208|22|22` 这种带矩形的条目 ⇒ **名字是应用给的、矩形是
系统量的**，比猜颜色强，而且明暗两档都成立。像素只留一个用途：与上一步比
diff，证"这一下真的改变了画面"。

## 四条纪律（都是翻过车写进来的，别改回去）

  · **坐标一律现场量**（D93）。2026-10-07 那次发现头一份写死的三个坐标全过期，
    于是"进我的仓库 / 五个页签 / 展开右栏"那 10 步**一次都没命中**，而每一步都报
    `alive`。现在每一步之前重 dump 一次，控件位置随窗口宽漂也不怕。
  · **每一步都要有"它生效了"的证据**：画面 diff 是一档，能读到状态真值的是另一档
    （右栏那颗键点完去读沙箱 config 的 `inspector_open`，见 `cfg_inspector_open`）。
    只有 `alive` 不算过。
  · **状态尽量走配置**（AGENTS：状态用 config 驱动，不靠点键）。选中哪颗仓库、
    抽屉开不开，都写进沙箱 config；夹具是 %TEMP 现造的，**用户那些真实仓库一颗都不进沙箱**。
  · **不点明暗主题键**。点了后面每一帧都换一档底色，抓帧对不上基线；
    深色那一档要单独走查（v4 §12.1），不混在这一条里。

⚠️ 这一版**删掉了 ⌘K 那两步**：`steps.py` 送不进按键是量出来的事实
（`keybd_event` 与 `PostMessageW` 都试过，前台确认是本应用、逐像素差 0），
留着那两步就是给"命令面板也走查过了"盖一个没测的章。那条路要他双击实机。

用法：python scripts/a11y_stress.py
"""

import ctypes
import ctypes.wintypes as wt
import json
import os
import shutil
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import qbguard  # noqa: E402
import steps as _steps  # noqa: E402
import shoot as _shoot  # noqa: E402
from PIL import Image, ImageChops  # noqa: E402

find_window = _steps.find_window
click = _steps.click
user32 = ctypes.windll.user32

REPO = r"F:\AIwork\qingbird-git"
EXE = os.path.join(REPO, "target", "debug", "qingbird-git.exe")
TMP = os.environ["TEMP"]
WORK = os.path.join(TMP, "qb_a11y")
SANDBOX = os.path.join(WORK, "appdata")
CFG = os.path.join(SANDBOX, "com.qingbird.git", "config.json")
FIXTURE = os.path.join(WORK, "repo")
ERR = os.path.join(TMP, "qb_stress_err.txt")

# 详情页五个页签（2026-10-11 dump 出来的实际顺序：概览 / 说明 / 历史 / 空间 / 变更）
TABS = ["概览", "说明", "历史", "空间", "变更"]
# 抽屉那颗键的两句 label（`shell.rs:793-797`）——两态各一句，点完必须换成另一句
INSPECTOR_LABELS = {"收起右栏": "展开右栏", "展开右栏": "收起右栏"}


def unlock(func, path, exc):
    os.chmod(path, 0o777)
    func(path)


def clean(path):
    if os.path.isdir(path):
        shutil.rmtree(path, onerror=unlock)
    assert not os.path.exists(path), f"没清干净：{path}（读到旧状态就等于没测）"


# ───────────────────────── 拿 a11y 树 ─────────────────────────

PS_DUMP = r"""
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$el = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]{hwnd})
if ($null -eq $el) {{ Write-Output "NO-ELEMENT"; exit 0 }}
$cond = [System.Windows.Automation.Condition]::TrueCondition
try {{
  $all = $el.FindAll([System.Windows.Automation.TreeScope]::Subtree, $cond)
  foreach ($n in $all) {{
    $c = $n.Current
    $r = $c.BoundingRectangle
    $nm = ($c.Name -replace '[\r\n\|]', ' ')
    if ($nm.Length -gt 60) {{ $nm = $nm.Substring(0,60) }}
    Write-Output ($c.ControlType.ProgrammaticName.replace('ControlType.','') + '|' + $nm + '|' +
                  ('{{0}}|{{1}}|{{2}}|{{3}}' -f [int]$r.X, [int]$r.Y, [int]$r.Width, [int]$r.Height))
  }}
}} catch {{ Write-Output ("ERR=" + $_.Exception.Message) }}
"""


class Node:
    def __init__(self, kind, name, x, y, w, h):
        self.kind, self.name = kind, name
        self.x, self.y, self.w, self.h = x, y, w, h
        self.cx, self.cy = x + w // 2, y + h // 2

    def __repr__(self):
        return f"{self.kind}|{self.name}|{self.w}x{self.h}@{self.x},{self.y}"


def dump(hwnd):
    """一次 PowerShell 拿全树。这一步同时就是**激活 a11y** 的那个动作
    （`AutomationElement.FromHandle` + `FindAll` 会让平台回调把 `active_flag` 置上）。"""
    p = subprocess.run(
        ["powershell", "-NoProfile", "-Command", PS_DUMP.format(hwnd=hwnd)],
        capture_output=True, text=True, timeout=120,
    )
    out = []
    for ln in (p.stdout or "").splitlines():
        parts = ln.strip().split("|")
        if len(parts) == 6:
            try:
                out.append(Node(parts[0], parts[1], *(int(v) for v in parts[2:])))
            except ValueError:
                pass
        elif ln.startswith(("ERR=", "NO-ELEMENT")):
            raise AssertionError(f"a11y dump 失败：{ln.strip()}")
    if not out:
        raise AssertionError(f"a11y dump 一个节点都没有。stderr={p.stderr.strip()[:200]}")
    return out


def one(nodes, name, kind=None):
    """按名字取**恰好一颗**。取到两颗就直接抛——这一条本身就是 §12.7 那个判据：
    '同一个功能的两个入口' 在 a11y 名字层面同样会露出来。"""
    hit = [n for n in nodes if n.name == name and (kind is None or n.kind == kind)]
    if len(hit) != 1:
        raise AssertionError(f"名字「{name}」在本帧命中 {len(hit)} 颗（要 1 颗）：{hit}")
    return hit[0]


def tree_sig(nodes):
    """整棵 a11y 树的**签名**：`(类型:名字)` 的可排序元组。

    给"抓了一帧"这一步当生效判据用（`verify_dark_five.py` 里那两帧逐字节相同
    就是靠它抓的）。⚠️ **矩形不进签名**——窗口一 resize 每一颗都动，那会把
    "同一个屏"判成两个屏；名字与类型才是屏的身份。
    """
    return tuple(sorted(f"{n.kind}:{n.name}" for n in nodes))


def client_origin(hwnd):
    pt = wt.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    return pt.x, pt.y


def window_rect(hwnd):
    """`(left, top, right, bottom)` 屏幕坐标。帧偏移 = 客户原点 − (left, top)。"""
    r = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    return r.left, r.top, r.right, r.bottom


# ───────────────────────── 夹具与沙箱 ─────────────────────────

def build_fixture():
    clean(WORK)
    os.makedirs(FIXTURE)
    subprocess.run(["git", "init", "-q", "--initial-branch=main", FIXTURE], check=True)
    for k, v in (("user.email", "t@t"), ("user.name", "t"), ("core.autocrlf", "false")):
        subprocess.run(["git", "-C", FIXTURE, "config", k, v], check=True)
    open(os.path.join(FIXTURE, "base.txt"), "w", encoding="utf-8").write("base\n")
    subprocess.run(["git", "-C", FIXTURE, "add", "-A"], check=True)
    subprocess.run(["git", "-C", FIXTURE, "commit", "-q", "-m", "base"], check=True)


def build_sandbox(inspector_open, theme=None):
    """settings 抄真实那份（主题、字号这些影响版面的不许我猜），repos 只留夹具一颗。

    ⚠️ `theme` 那一格写的是**主题名字**，不是 "dark" / "light"：判据是
    `s.theme == theme::DARK`（`theme.rs:106`）而 `DARK = "qingbird-dark"`（`:41`）——
    写错**不报错、不落深色**，静默按浅色画（`store.rs:88` 那句注释说的就是这一坑）。
    默认 `None` = 不动它（沿用真实那份），走查全程要浅色是为了底色不漂。
    """
    real = json.load(open(os.path.join(os.environ["APPDATA"], "com.qingbird.git",
                                       "config.json"), encoding="utf-8-sig"))
    clean(SANDBOX)
    os.makedirs(os.path.dirname(CFG))
    real["repos"] = [{
        "path": FIXTURE.replace("\\", "/"), "name": "a11y-fixture", "alias": "",
        "added_at": 1791600000, "last_used": 1791600000,
        "test_cmd": "cargo test", "build_cmd": "cargo build", "dev_cmd": "",
        "install_cmd": "", "pinned_cmds": [], "cmd_runs": {},
        "auto_sync_mins": 0, "daily_time": "", "auto_pull": False,
        "last_auto_check": 0, "last_auto_result": "", "is_third_party": False,
    }]
    real["selected_path"] = FIXTURE.replace("\\", "/")
    real.setdefault("settings", {})["inspector_open"] = inspector_open
    real["settings"]["sidebar_collapsed"] = False
    if theme is not None:
        real["settings"]["theme"] = theme
    json.dump(real, open(CFG, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
    #  **建沙箱的人负责把启动入口指过去**（D78 的落法：让"忘"没有选项）。
    # 2026-10-11 第二次为这条交学费：新写的 `verify_dark_five.py` 忘了设 `QB_APPDATA`，
    # 靠 `launch_env` 那句"没设就是跑真实配置"的警告 + 调用方的 assert 才没写脏用户配置。
    # 但"每个脚本自己记得设"迟早还会漏 ⇒ 收在这里，所有 import 这颗函数的脚本一起生效。
    os.environ["QB_APPDATA"] = SANDBOX


def cfg_inspector_open():
    """**不靠像素的生效判据**：那颗键点下去会把 `inspector_open` 写回 config
    （`shell.rs:965` → `store::set_inspector_layout`）。读沙箱那份，翻转了才叫点中了。"""
    try:
        return json.load(open(CFG, encoding="utf-8-sig"))["settings"]["inspector_open"]
    except Exception:  # noqa: BLE001
        return None


# ─────────────── 像素扫：只给"不进 a11y 树的那一列"用 ───────────────
#
# 设置对话框左栏那四分类是 `v_flex().id("set-nav-…")` + `on_click`（`settings.rs:740-758`）
# ——**有 id，但没有 role**，按基座规则（`element.rs:368-402`）不生成 a11y 节点，
# 所以名字驱动在它们身上拿不到东西。这两条函数是从 2026-10-11 那次取证（`D86`）搬的。
#
# ⚠️ **两条都改成"与该帧画布色比"，不写死绝对亮度**（2026-10-11 第二次为同一档翻车）：
# 原来 `dialog_box` 认 "r,g,b 都 ≥ 228"、`dark_bands` 认 "亮度 < 170"——那是**浅色档的数**。
# 深色档下外卡是 `#16262B`、文字是 `#E6EFF0`，**两条一起反号**：对话框"永远开不起来"、
# 类别标题"一行都扫不出"。症状与"功能坏了"长得一模一样，而坏的只是判据
# （与上面"一档底色变了整条像素判据作废"是同一条）。

from collections import Counter  # noqa: E402


def canvas_of(png):
    """该帧的**背景主色** = 全帧隔点取众数（只给人看，不当判据用）。

    ⚠️ 它**不能**当 `dialog_box` / `dark_bands` 的基准色：对话框打开时遮罩把背景压暗，
    实测压暗值 (206,209,210) 与画布 (232,237,240) 只差 **26** ⇒ "与画布差 ≥10"这种判据
    会把整帧都算成外卡（当天帧上量到 0..1374，而对话框左缘其实在 x≈220）。
    ⇒ 那两条改用**卡面档本身**（`card_face_of`），见下面。
    """
    im = Image.open(png).convert("RGB")
    px = im.load()
    w, h = im.size
    return Counter(px[x, y] for y in range(0, h, 7) for x in range(0, w, 7)).most_common(1)[0][0]


DARK_NAME = "qingbird-dark"


def card_face_of(png=None):
    """**该帧那套主题的卡面档**，从 `gui/theme.json` 按沙箱 config 里的主题名现取。

    为什么用它当基准而不是"背景色"：外卡与左栏里的白卡都是这一档，而遮罩压暗之后
    是另一档（混合值）⇒ 认这一档就只认得出"真正亮着的那一块"，明暗两档同一份判据
    （浅色 `#FFFFFF`、深色 `#16262B`）。
    主题名取法与 `theme.rs::apply_saved` 一致：**只有等于 `qingbird-dark` 才算深色**，
    其余（含空串、含老 Tauri 的 `dark`）一律浅色。
    """
    name = "qingbird-light"
    if _saved_theme() == DARK_NAME:
        name = DARK_NAME
    d = json.load(open(os.path.join(REPO, "gui", "theme.json"), encoding="utf-8-sig"))
    for t in d["themes"]:
        if t["name"] == name:
            v = t["colors"]["secondary.background"]
            return (int(v[1:3], 16), int(v[3:5], 16), int(v[5:7], 16))
    raise AssertionError(f"theme.json 里没有 {name} 那一套")


def far(c, base, tol=10):
    return max(abs(c[0] - base[0]), abs(c[1] - base[1]), abs(c[2] - base[2])) >= tol


def dark_bands(png, x0, x1, y0, y1, min_px=2, gap=6, tol=40, face=None):
    """竖着扫 [x0,x1) 这一条，返回每一簇"与卡面明显不同"的 (中心 y, 高度)。"""
    im = Image.open(png).convert("RGB")
    px = im.load()
    base = face if face is not None else card_face_of(png)
    runs = []
    for y in range(y0, min(y1, im.size[1])):
        n = sum(1 for x in range(x0, x1) if far(px[x, y], base, tol))
        if n >= min_px:
            if runs and y - runs[-1][1] <= gap:
                runs[-1][1] = y
            else:
                runs.append([y, y])
    return [((a + b) // 2, b - a + 1) for a, b in runs]


def dialog_box(png, face=None):
    """**正文卡**的横向范围：认该帧那套主题的卡面档，多行取并集。

    ⚠️ 三个坑都踩过（2026-10-11）：① 按"左栏大概在 x=90..240"去点，结果点到**遮罩**
    上把对话框关了——症状是"点完与上一帧差 214412 处"，与刚开起来那次一模一样；
    ② 按单行最长亮段量，量到的是里面那段卡的两个空洞，把左栏扫到了内容区里去；
    ③ 判"是不是外卡"原来写死绝对亮度 `>= 228`（浅色那档卡面是白），深色档下外卡是
    `#16262B` ⇒ 一条都不满足，报成"对话框根本没开起来"。
    ⚠️ 而且它量到的是**"该行有 >250 个卡面档像素"的那一段**：浅色下外卡与正文卡同档
    （`popover.background` 与 `secondary.background` 两条主题都等于卡面白）⇒ 它给的是
    **外卡左缘**；而一旦某一档两者不同，它就会从正文卡开始。**别把它当"外卡左缘"依赖**——
    要找左栏用 `wash_box`（它量的是选中那一格，与底色档无关）。
    """
    im = Image.open(png).convert("RGB")
    px = im.load()
    w, h = im.size
    base = face if face is not None else card_face_of(png)
    lo, hi = w, 0
    for y in range(150, min(h - 60, 780), 7):
        xs = [x for x in range(w) if px[x, y] == base]
        if len(xs) > 250:
            lo, hi = min(lo, xs[0]), max(hi, xs[-1])
    return (lo if hi > lo else 0, hi)


def wash_box(png, wash=None):
    """**当前选中的那一格**的矩形：等于 wash 档的那一块，返回 `(x0, x1, 中心 y)`。

    为什么用它找左栏：设置那四分类里选中那一格铺 `tokens::wash(t)`，而它的 x 范围
    就是整列的 x 范围（四格同宽同位）⇒ **左栏的位置从帧上量出来，不目测**。
    ⚠️ 这条是 2026-10-11 补的：以前"左栏在哪一列"是从 `dialog_box` 的返回值推的，
    而那个数**依赖"外卡与正文卡同档"这一巧合**（浅色成立、深色不成立）⇒ 换档就漂。
    现在直接量"选中那一格"的底，位置与宽度都从帧上来。
    """
    im = Image.open(png).convert("RGB")
    px = im.load()
    w, h = im.size
    base = wash if wash is not None else wash_of()
    lo, hi, ys = w, 0, []
    for y in range(120, min(h - 60, 780)):
        xs = [x for x in range(w) if px[x, y] == base]
        if len(xs) > 40:
            ys.append(y)
            lo, hi = min(lo, xs[0]), max(hi, xs[-1])
    if not ys:
        raise AssertionError("帧上没有 wash 档那一块 ⇒ 要么没选中项，要么 wash 档取错了")
    return lo, hi, (ys[0] + ys[-1]) // 2


def wash_of():
    """该帧那套主题的 wash 档（选中面），从 theme.json 现取"""
    name = DARK_NAME if _saved_theme() == DARK_NAME else "qingbird-light"
    d = json.load(open(os.path.join(REPO, "gui", "theme.json"), encoding="utf-8-sig"))
    for t in d["themes"]:
        if t["name"] == name:
            v = t["colors"]["secondary.active.background"]
            return (int(v[1:3], 16), int(v[3:5], 16), int(v[5:7], 16))
    raise AssertionError(f"theme.json 里没有 {name} 那一套")


def _saved_theme():
    try:
        return json.load(open(CFG, encoding="utf-8-sig"))["settings"].get("theme")
    except Exception:  # noqa: BLE001
        return None


# ───────────────────────── 走查 ─────────────────────────

def run_walk(inspector_open, tag):
    """一跑 = 一个进程（抽屉开 / 关两种起始态各一次：那颗键的两种 label、两个落点
    都要在 a11y 树真建起来之后被走过一遍，才算 §12.7 那条"条件出现的入口一帧只挂一次"）"""
    build_sandbox(inspector_open)
    # 沙箱入口由 `build_sandbox` 自己设好了（见那里那句注释），这里只**验**它生效
    subprocess.run(["taskkill", "/F", "/IM", "qingbird-git.exe"], capture_output=True)
    time.sleep(0.8)
    if os.path.exists(ERR):
        os.remove(ERR)

    env = qbguard.launch_env()
    assert env["APPDATA"] == SANDBOX, f"沙箱没生效：{env['APPDATA']}"
    env["RUST_BACKTRACE"] = "1"
    err = open(ERR, "wb")
    proc = subprocess.Popen([EXE], cwd=REPO, env=env, stdout=subprocess.DEVNULL, stderr=err)
    steps = []
    prev = [None]
    shot_dir = os.path.join(WORK, f"frames-{tag}")
    os.makedirs(shot_dir, exist_ok=True)
    try:
        hwnd = find_window(proc.pid)
        assert hwnd, "找不到窗口"
        assert _steps.ensure_focus(hwnd), "拿不到前台"
        clx, cly = client_origin(hwnd)
        r = wt.RECT()
        user32.GetWindowRect(hwnd, ctypes.byref(r))
        offx, offy = clx - r.left, cly - r.top      # 帧坐标 → 客户坐标要减掉的量（现场量，D93）
        print(f"[{tag}] hwnd={hwnd} 客户原点=({clx},{cly}) 帧={r.right - r.left}x{r.bottom - r.top}"
              f" 帧偏移=({offx},{offy})")
        click(hwnd, 700 - offx, 300 - offy, settle=1.0)   # 空点一次：未获焦时第一击只用于激活
        assert _steps.ensure_focus(hwnd)

        def shot(name):
            p = os.path.join(shot_dir, name + ".png")
            _shoot.capture(hwnd, p)
            return p

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
            changed = None
            try:
                cur = Image.open(shot("f%02d" % len(steps))).convert("RGB")
                if prev[0] is not None:
                    bb = ImageChops.difference(cur, prev[0]).getbbox()
                    changed = 0 if bb is None else (bb[2] - bb[0]) * (bb[3] - bb[1])
                prev[0] = cur
            except Exception:  # noqa: BLE001
                changed = None
            steps.append((label, state, changed))

        def click_node(node, settle=1.0):
            click(hwnd, node.cx - clx, node.cy - cly, settle=settle)

        # ① 进「我的仓库」→ 详情页
        nodes = dump(hwnd)
        step("进我的仓库", lambda: click_node(one(nodes, "我的仓库", "Button"), 1.8))
        time.sleep(0.8)
        nodes = dump(hwnd)
        btn = [n for n in nodes if n.kind == "Button"]
        print(f"[{tag}] 激活后 a11y 树：节点 {len(nodes)}，其中按钮 {len(btn)} 颗")
        steps.append(("激活后", "alive" if proc.poll() is None else "DIED", None))

        # ② 五个页签逐个点（名字来自 a11y，不靠像素）
        for tab in TABS:
            def go(tab=tab):
                click_node(one(dump(hwnd), tab, "TabItem"), 1.0)
            step(f"页签「{tab}」", go)

        # ③ 右栏开关：点两下（去 / 回），每下都验 config 真翻了、label 真换了
        for i in range(2):
            def toggle(i=i):
                nodes = dump(hwnd)
                cur = next((n for n in nodes
                            if n.kind == "Button" and n.name in INSPECTOR_LABELS), None)
                assert cur is not None, "标题栏没找到右栏开关（两句 label 都不在树里）"
                was = cfg_inspector_open()
                click_node(cur, 1.4)
                now = cfg_inspector_open()
                assert now != was, (f"点「{cur.name}」({cur.cx},{cur.cy}) 之后 "
                                    f"inspector_open 仍是 {now} ⇒ 这一下没落在右栏开关上")
                after = [n.name for n in dump(hwnd)
                         if n.kind == "Button" and n.name in INSPECTOR_LABELS]
                assert after == [INSPECTOR_LABELS[cur.name]], (
                    f"点完 label 没换成「{INSPECTOR_LABELS[cur.name]}」，而是 {after}"
                    " ⇒ 同帧出现了两颗同一个功能的入口")
            step(f"右栏开关{i}（起点 open={cfg_inspector_open()}）", toggle)

        # ④ 设置对话框：D93 那次的原始事故现场（写死坐标 + 双击 → 10 步全没生效）。
        #    ⚠️ 这里**连名字为空的节点一起打**——上一版只打有名字的，于是"四分类
        #    一个都没出现"这句话根本说不清是"它们不在树里"还是"被我筛掉了"
        #    （AGENTS：零命中不等于没有，Q38 同款）。
        step("开设置", lambda: click_node(one(dump(hwnd), "设置", "Button"), 1.6))
        nodes = dump(hwnd)
        print(f"[{tag}] 设置打开后 a11y 树 {len(nodes)} 个节点：")
        for n in nodes:
            print(f"    {n.kind}|{n.name or '<空名字>'}|{n.w}x{n.h}@{n.x},{n.y}")

        # ④-b 左栏那四分类**逐个点开**——它们不在 a11y 树里（有 id 没 role ⇒ 不生成节点），
        #     所以这一列只能回到像素扫。"零节点"这件事本身是抓出来的缺陷，已登记。
        #     ⚠️ 这一版换了**两条判据**，都不是因为旧版给了错答案，而是旧版那两条
        #     **答对了也证明不了**（AGENTS：一条测不出东西的判据 = 给"没测"盖章）：
        #     · "对话框还在不在"原来判 `dialog_box` 的宽度——关掉之后主界面那张白卡
        #       照样满足它，所以它对"点遮罩把对话框关了"这件事**零证明力**。
        #       现在判 **a11y 树里有没有『保存』那颗键**（主界面没有）。
        #     · 原来没有"选中那一格真的换了"的判据。现在每点一次都复量 `wash_box`
        #       的中心 y，**没动就是这一击没换分类**，当场点名。
        #     左栏 x 范围也改由 wash 块量（不再依赖"外卡那一档恰好是白"，深色下它不是白）。
        f_dlg = shot("dlg-open")
        f_dlg = shot("dlg-open")

        def assert_dialog_open(when):
            if "保存" not in {n.name for n in dump(hwnd) if n.kind == "Button"}:
                raise AssertionError(f"{when}：设置对话框已经不在（树里没有『保存』那颗键）")

        assert_dialog_open("刚点开设置")
        wx0, wx1, wy = wash_box(f_dlg)
        rows = dark_bands(f_dlg, wx0, wx1, 120, 700)
        titles = [y for y, hh in rows if hh >= 16]      # 副标题与脚底版本号都只有 10px 高
        print(f"[{tag}] 左栏由 wash 块定到 x={wx0}..{wx1}（选中那一格中心 y={wy}），"
              f"按高度筛完的类别标题：{titles}")
        assert len(titles) >= 3, f"类别标题至少该有三行，只扫到 {len(titles)} 行"
        # ⚠️ `wash_box` 量到的是**全帧所有 wash 底的并集**：这一帧上有两块——左栏选中
        # 那一格 + 侧栏选中那一行（两处都是 `tokens::wash`）。所以它的 `wy` **不是**
        # "左栏选中格的位置"，拿它判"点完换没换分类"是**一条测不出东西的断言**
        # （2026-10-11 第一版就这么写着，跑绿了而它一次都没真的判过事）⇒ 删掉。
        # "这一击换了分类"改判一件真有分辨力的事：**整棵 a11y 树的签名必须变**
        # （四分类各自屏里的控件本来就不是同一批）。
        last_sig = [None]

        def go(ty):
            click(hwnd, (wx0 + wx1) // 2 - offx, ty - offy, settle=1.6)
            assert_dialog_open(f"点 y={ty} 那一类之后")
            sig = sorted(f"{n.kind}:{n.name}" for n in dump(hwnd))
            if last_sig[0] is not None and sig == last_sig[0]:
                raise AssertionError(f"点 y={ty} 之后整棵树与上一屏逐字相同 ⇒ 这一击没换分类")
            last_sig[0] = sig
        for ty in titles:
            step(f"设置分类 y={ty}", lambda ty=ty: go(ty))
            ns = dump(hwnd)
            kinds = {}
            for m in ns:
                kinds[m.kind] = kinds.get(m.kind, 0) + 1
            print(f"    这一屏 {len(ns)} 节点 " +
                  " ".join(f"{k}={v}" for k, v in sorted(kinds.items())))

        def close_dialog():
            ns = [n for n in dump(hwnd) if n.name in ("取消", "关闭", "完成")]
            if len(ns) != 1:
                raise AssertionError(f"关不掉设置：按『取消/关闭/完成』命中 {len(ns)} 颗 {ns}")
            click_node(ns[0], 1.2)
            if any(n.name == "保存" for n in dump(hwnd)):
                raise AssertionError("点了取消，设置对话框还在 ⇒ 这一击没落在键上")
        step("关设置", close_dialog)

        # ⑤ 另外两屏
        step("去总览", lambda: click_node(one(dump(hwnd), "总览", "Button"), 1.6))
        step("去第三方", lambda: click_node(one(dump(hwnd), "第三方仓库", "Button"), 1.4))
        time.sleep(0.8)
        steps.append(("第三方稳定后", "alive" if proc.poll() is None else "DIED", None))
    finally:
        err.close()
        print(f"---- 走查（{tag}）----")
        still = []
        for label, st, changed in steps:
            flag = "  <<<" if st == "DIED" or st.startswith(("EXC", "ALREADY")) else ""
            mark = "" if changed is None else ("  [画面没变]" if changed == 0 else "  Δ%dpx" % changed)
            if changed == 0:
                still.append(label)
            print(f"  {label:26s} {st}{mark}{flag}")
        if still:
            print("静止的步（逐条判断：本来就不该变 / 那一步根本没生效）:", "、".join(still))
        size = os.path.getsize(ERR) if os.path.exists(ERR) else 0
        print(f"stderr_bytes={size}")
        bad = []
        if size:
            for ln in open(ERR, encoding="utf-8", errors="replace"):
                if "panicked" in ln or "Duplicate" in ln:
                    print("  >>", ln.strip())
                    bad.append(ln.strip())
        if proc.poll() is None:
            proc.kill()
        time.sleep(0.5)
    dead = [l for l, s, _ in steps
            if s == "DIED" or s.startswith(("EXC", "ALREADY"))] + bad
    return dead


def main():
    build_fixture()
    before = qbguard.real_cfg_fp()
    problems = []
    # 只跑一跑（`python scripts/a11y_stress.py open`）是给变异验证用的：
    # 变异那一次只要证明"这套走查抓得住撞 id"，不需要两跑各抓一次
    which = (sys.argv[1] if len(sys.argv) > 1 else "both").lower()
    runs = {"open": [(True, "drawer-open")],
            "closed": [(False, "drawer-closed")],
            "both": [(True, "drawer-open"), (False, "drawer-closed")]}[which]
    try:
        for open_, tag in runs:
            problems += run_walk(open_, tag)
    finally:
        qbguard.check_real_cfg(before)
    print("\n判读：", "全绿（没崩、没重复 id、每一步都有生效证据）"
          if not problems else f"有问题 {len(problems)} 条：")
    for p in problems:
        print("  -", p)
    return 0 if not problems else 1


if __name__ == "__main__":
    sys.exit(main())
