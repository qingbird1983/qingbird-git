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


def client_origin(hwnd):
    pt = wt.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    return pt.x, pt.y


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


def build_sandbox(inspector_open):
    """settings 抄真实那份（主题、字号这些影响版面的不许我猜），repos 只留夹具一颗"""
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
    json.dump(real, open(CFG, "w", encoding="utf-8"), ensure_ascii=False, indent=1)


def cfg_inspector_open():
    """**不靠像素的生效判据**：那颗键点下去会把 `inspector_open` 写回 config
    （`shell.rs:965` → `store::set_inspector_layout`）。读沙箱那份，翻转了才叫点中了。"""
    try:
        return json.load(open(CFG, encoding="utf-8-sig"))["settings"]["inspector_open"]
    except Exception:  # noqa: BLE001
        return None


# ─────────────── 像素扫：只给"不进 a11y 树的那一列"用 ───────────────
#
# 设置对话框左栏那四分类是裸 `div().on_click(...)`（`settings.rs:740-758`）——
# **既没有 id 也没有 role**，按基座规则不生成 a11y 节点，所以名字驱动在它们身上
# 拿不到东西。这两条函数是从 2026-10-11 那次取证（`D86`）里搬的，两个坑都写在里面。

def dark_bands(png, x0, x1, y0, y1, thresh=170, min_px=2, gap=6):
    """竖着扫 [x0,x1) 这一条，返回每一簇暗像素的 (中心 y, 高度)。"""
    im = Image.open(png).convert("RGB")
    px = im.load()
    runs = []
    for y in range(y0, min(y1, im.size[1])):
        n = sum(1 for x in range(x0, x1) if sum(px[x, y][:3]) / 3.0 < thresh)
        if n >= min_px:
            if runs and y - runs[-1][1] <= gap:
                runs[-1][1] = y
            else:
                runs.append([y, y])
    return [((a + b) // 2, b - a + 1) for a, b in runs]


def dialog_box(png):
    """对话框那张外卡的横向范围：**多行取并集**，不是单行最长亮段。

    ⚠️ 两个坑都踩过（2026-10-11）：① 按"左栏大概在 x=90..240"去点，结果点到**遮罩**
    上把对话框关了——症状是"点完与上一帧差 214412 处"，与刚开起来那次一模一样；
    ② 按单行最长亮段量，量到的是里面那段卡的两个空洞，把左栏扫到了内容区里去。
    ⇒ 改成"每一行数亮像素个数，够多的那些行的亮区并起来"。
    """
    im = Image.open(png).convert("RGB")
    px = im.load()
    w, h = im.size
    lo, hi = w, 0
    for y in range(150, min(h - 60, 780), 7):
        xs = [x for x in range(w)
              if px[x, y][0] >= 228 and px[x, y][1] >= 228 and px[x, y][2] >= 228]
        if len(xs) > 250:
            lo, hi = min(lo, xs[0]), max(hi, xs[-1])
    return (lo if hi > lo else 0, hi)


# ───────────────────────── 走查 ─────────────────────────

def run_walk(inspector_open, tag):
    """一跑 = 一个进程（抽屉开 / 关两种起始态各一次：那颗键的两种 label、两个落点
    都要在 a11y 树真建起来之后被走过一遍，才算 §12.7 那条"条件出现的入口一帧只挂一次"）"""
    build_sandbox(inspector_open)
    # 沙箱路径由脚本自己知道，就别指望跑之前 shell 里记得设（AGENTS：隔离靠人记得迟早会漏）。
    # 但**覆盖仍由 qbguard.launch_env 做**，这里只把它的入口填上——API 不改，三个脚本共用一份规则。
    os.environ["QB_APPDATA"] = SANDBOX
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

        # ④-b 左栏那四分类**逐个点开**——它们不在 a11y 树里（裸 div + on_click，
        #     没 id 没 role ⇒ 基座不给生成节点），所以这一列只能回到像素扫。
        #     "零节点"这件事本身是这一跑抓出来的缺陷，登记进路线表。
        f_dlg = shot("dlg-open")
        dx0, dx1 = dialog_box(f_dlg)
        assert dx1 - dx0 > 400, f"没扫到设置那张外卡（宽 {dx1 - dx0}）⇒ 对话框没开起来"
        rows = dark_bands(f_dlg, dx0 + 20, dx0 + 150, 120, 700)
        titles = [y for y, hh in rows if hh >= 16]      # 副标题与版本号那行都只有 10px 高
        print(f"[{tag}] 设置左栏扫到 {len(rows)} 行，按高度筛完的类别标题：{titles}")
        assert len(titles) >= 3, f"类别标题至少该有三行，只扫到 {len(titles)} 行"
        for ty in titles:
            def go(ty=ty):
                click(hwnd, dx0 + 60 - offx, ty - offy, settle=1.6)
                a0, a1 = dialog_box(shot("dlg-at-%d" % ty))
                if a1 - a0 < 400:
                    raise AssertionError(f"点 y={ty} 之后外卡没了 ⇒ 这一击落在遮罩上，对话框被关掉")
            step(f"设置分类 y={ty}", go)
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
