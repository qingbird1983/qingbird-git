"""31.4 补二十五那条回归判据：**抽屉开与关，左栏那一列不许被抢宽**。

判据全部落在帧上量的两个数（窗口客户区宽、左栏白面宽），不是"跑起来没报错"。

五跑十条判据（每跑各写一份 config 再启动，**状态走配置、不走鼠标**）：
  A 抽屉开            → 记下窗口宽 wA、左栏白面 sA
  B 抽屉关            → **sB 必须等于 sA**（这条就是补二十五的原话）
  C 抽屉开（再跑一次）→ wC == wA 且 sC == sA（"再撑一次要撑到同一个数"，
                        不幂等的症状是窗口越开越宽）
  D 抽屉开 + 侧栏 +40 → 窗口长的部分**必须全部进了侧栏**：
                        `(wD − wA) − (sD − sA) ≈ 0`
                        这一档挡的就是那一轮修掉的病——拿中间那一列去垫侧栏。
                        ⚠️ 判据写成**两个增量的差**，不写成"中间列 == 950"：
                        后者要把轨宽、右缘宽这些壳层常数抄进脚本，
                        它们一改判据就悄悄失效（本项目为写死的数交过三次学费）
  E 抽屉关 + 侧栏 +40 → 与 D 只差抽屉那一列：`wD − wE ≈ inspector_w`（**读配置**，
                        2026-10-11 实测那一列是 560，不是从前抄的 320），
                        证"撑宽"这件事真的由抽屉参与，而不是窗口一直那么宽

与上一版的三处不同（2026-10-11 按路线表 32-15 的尾巴重写）：
  ① **夹具自己造**（%TEMP 下一颗合成仓库 + 沙箱 config），不再吃 10-08 那轮
     留在 `%TEMP%\\qb_v4` 里的旧沙箱——上一版红就红在它读到的是别人的状态；
  ② **删掉拖把手那一步**：`sidebar_w` 是配置项，直接写进 config 就是"拖完之后"
     那一态，比模拟拖拽稳（拖拽要猜把手坐标，而把手位置随窗口漂）；
  ③ **删掉找收起键那一步**：收起 = `inspector_open=false` 那一跑（B），
     不需要点任何键。⇒ 全程只剩**一次**点击（导航轨「我的仓库」），
     而它的 y 是**现场扫出来的**（D93 那条：v4 改版之后没有一个写死坐标复核过）。

⚠️ **这次重写换来的稳，代价要说清**：它验的是**宽度规则**（谁要更多宽、由哪一列垫），
状态由配置给，所以**「拖把手」与「点收起键」这两条交互路径本身不再由这条脚本负责**——
把手能不能拖动、那颗键点得点不动，归别的判据（侧栏/抽屉的单测 + 他双击实机）。
**别把它当"这两条也回归过了"**：上一版那点得到，但它的坐标随改版过期，
红的时候分不清是规则坏了还是坐标坏了（D93 的原话正是这件事）。

用法：`python scripts/verify_grow.py`
"""
import ctypes
import datetime
import io
import json
import os
import re
import shutil
import subprocess
import sys
import time

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
import qbguard, shoot, steps  # noqa: E402

from PIL import Image  # noqa: E402

u = shoot.user32
wt = shoot.wt
TEMP = os.environ["TEMP"]
FIX_ROOT = os.path.join(TEMP, "qb_grow_ui")
REPO = os.path.join(FIX_ROOT, "repo")
SANDBOX = os.path.join(TEMP, "qb_grow_app")
CFG = os.path.join(SANDBOX, "com.qingbird.git", "config.json")
EXE = os.path.join(ROOT, "target", "debug", "qingbird-git.exe")
DRAG_DELTA = 40.0   # "拖了 40" 的等价物：sidebar_w 直接 +40

# 两列的宽度**都从配置里读**，脚本里一个都不钉（2026-10-11 的更正）：
# 第一版把侧栏写成 263、注释还写着"与真实那份一致"——真实那份是 **360**，
# 那句是假话；而抽屉那一列我照旧抄了个 320，实测是 **560**，两条判据当场红。
# ⇒ 两列同源同法，别再"一个读配置、一个抄常数"（同一件事两个出处就是两个会漂的数）
BASE: dict = {}

# 下面四个数是**壳层常数**，各有一个出处；改版之后它们会漂，
# 而这条脚本必须红到让人回来复核（D93 那条：写死的数不是错，**没人复核它过期**才是错）
TRACK_W = 41.0      # 导航轨宽：`gui/src/tokens.rs` 的 NAV_W + 1px 分隔
EDGE_W = 8.0        # 主区右缘气口
MAIN_MIN = 950.0    # 中间那一列的最小需求：`gui/src/tokens.rs` 的 MAIN_MIN_W
INITIAL_W = 1280.0  # 窗口初始客户区宽（规则只"撑宽"，**从不主动缩小**）


def read_range(name):
    """侧栏的合法夹取范围**从 core 现读**，不抄进脚本（抄一次就多一处会漂的真源）。

    ⚠️ 这一条不是洁癖，是本轮红出来的：他的 `sidebar_w` 正好是上限 **360**，
    而 D/E 两跑要"侧栏 +40"，脚本往配置里写 400 ⇒ 应用按 `clamp_sidebar_w`
    **夹回 360** ⇒ 那两跑与 A 是**同一个状态**，三条判据红而宽度规则一个字没错。
    **红的是前提，不是实现**（AGENTS「测试的前提不能外包给环境」的同族：
    这一回前提不是环境给的，是**我写进配置里的那个数被应用改掉了**）。
    ⇒ 落法：基线取"离上限还留着 delta 余量"的那个数，余量与上限都从 core 读。"""
    src = io.open(os.path.join(ROOT, "crates", "core", "src", "store.rs"),
                  encoding="utf-8-sig").read()
    m = re.search(r"pub const %s\s*:\s*\(f32, f32\)\s*=\s*\(\s*([\d.]+)\s*,\s*([\d.]+)\s*\)\s*;"
                  % re.escape(name), src)
    assert m, f"store.rs 里读不到 {name}——夹范围的形状变了，这条脚本的前提没了，先去看它"
    return float(m.group(1)), float(m.group(2))


def unlock(func, path, exc):
    os.chmod(path, 0o777)
    func(path)


def build_fixture():
    """一颗最小的合成仓库。⚠️ 不许拿用户那 17 颗当素材（AGENTS 明写）"""
    if os.path.isdir(FIX_ROOT):
        shutil.rmtree(FIX_ROOT, onerror=unlock)
    assert not os.path.exists(FIX_ROOT), "夹具没清干净，读到的可能是上一跑的旧状态"
    os.makedirs(FIX_ROOT)
    subprocess.run(["git", "init", "-q", "--initial-branch=main", REPO], check=True)
    for k, v in (("user.email", "t@t"), ("user.name", "t"), ("core.autocrlf", "false")):
        subprocess.run(["git", "-C", REPO, "config", k, v], check=True)
    with open(os.path.join(REPO, "base.txt"), "w", encoding="utf-8") as f:
        f.write("base\n")
    subprocess.run(["git", "-C", REPO, "add", "-A"], check=True)
    subprocess.run(["git", "-C", REPO, "commit", "-q", "-m", "base"], check=True)
    print(f"夹具就绪：{REPO}")


def build_sandbox():
    """settings 抄真实那份（主题、缩放、两列的宽度这些不许跟着变），repos 只留夹具这一条"""
    real = json.load(open(os.path.join(os.environ["APPDATA"], "com.qingbird.git",
                                       "config.json"), encoding="utf-8-sig"))
    if os.path.isdir(SANDBOX):
        shutil.rmtree(SANDBOX, onerror=unlock)
    os.makedirs(os.path.dirname(CFG))
    json.dump(real, open(CFG, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
    rs = real.get("settings", {})
    for key, fallback in (("sidebar_w", 263.0), ("inspector_w", 320.0)):
        raw = rs.get(key)
        # 那两个兜底值只在"配置里根本没有这一格"时用得上（老配置可能没有）。
        # **用了哪一个当场打印**，不许静默把抄的数当成他配的数
        BASE[key] = float(raw) if raw else fallback
        print(f"   从真实 config 读 {key}："
              f"{'读到 ' + format(float(raw), '.0f') if raw else '没有这一格，用兜底 ' + format(fallback, '.0f')}"
              f"（这两个数脚本里不钉）")


def write_case(inspector_open: bool, sidebar_w: float):
    cfg = json.load(open(CFG, encoding="utf-8"))
    cfg["settings"]["inspector_open"] = inspector_open
    cfg["settings"]["sidebar_collapsed"] = False
    cfg["settings"]["sidebar_w"] = sidebar_w
    cfg["selected_path"] = REPO
    # ⚠️ 沙箱那份是从真实 config **整份拷**来的（为了 settings 一致），
    # 所以这里必须把 repos 换成只有夹具这一颗——留着用户那 17 颗的话，
    # 量的时候侧栏画的是他的仓库列表，而这一屏本来就不该碰它们
    cfg["repos"] = [
        {
            "path": REPO, "name": "grow-fixture", "alias": "",
            "added_at": 1791600000, "last_used": 1791600000,
            "test_cmd": "", "build_cmd": "", "dev_cmd": "", "install_cmd": "",
            "pinned_cmds": [], "cmd_runs": {}, "auto_sync_mins": 0,
            "daily_time": "", "auto_pull": False, "last_auto_check": 0,
            "last_auto_result": "", "is_third_party": False,
        }
    ]
    json.dump(cfg, open(CFG, "w", encoding="utf-8"), ensure_ascii=False, indent=1)


def white_runs(png, y, x0, x1):
    """帧上 [x0,x1) 里连续的白面段（三通道都 >=248），返回 (起, 宽) 列表"""
    im = Image.open(png).convert("RGB")
    px = im.load()
    x1 = min(x1, im.size[0])
    out, start = [], None
    for x in range(x0, x1):
        c = px[x, y]
        white = c[0] >= 248 and c[1] >= 248 and c[2] >= 248
        if white and start is None:
            start = x
        elif not white and start is not None:
            if x - start >= 8:
                out.append((start, x - start))
            start = None
    if start is not None and x1 - start >= 8:
        out.append((start, x1 - start))
    return out


def win_w(hwnd):
    r = wt.RECT()
    u.GetClientRect(hwnd, ctypes.byref(r))
    return r.right - r.left


def park_cursor(hwnd):
    """把鼠标挪出窗口：hover 淡底会留在帧上，把"两帧应当一样"这条判据污染掉"""
    r = wt.RECT()
    u.GetWindowRect(hwnd, ctypes.byref(r))
    x = r.left - 60 if r.left - 60 > 0 else r.right + 60
    y = r.top - 60 if r.top - 60 > 0 else r.bottom + 60
    u.SetCursorPos(x, y)
    time.sleep(0.5)


def nav_bands(png):
    """导航轨上每颗图标的 y——**现场扫，不写死**（D93 那条）"""
    im = Image.open(png).convert("RGB")
    px = im.load()
    runs = []
    for y in range(30, im.size[1] - 90):
        n = sum(1 for x in range(8, 46) if sum(px[x, y][:3]) / 3 < 150)
        if n >= 3:
            if runs and y - runs[-1][1] <= 6:
                runs[-1][1] = y
            else:
                runs.append([y, y])
    return [(a + b) // 2 for a, b in runs]


def run_case(tag, inspector_open, sidebar_w):
    write_case(inspector_open, sidebar_w)
    env = qbguard.launch_env()
    assert env["APPDATA"] == SANDBOX, f"沙箱没生效：APPDATA={env.get('APPDATA')}"
    subprocess.run(["taskkill", "/F", "/IM", "qingbird-git.exe"], capture_output=True)
    time.sleep(0.7)
    p = subprocess.Popen([EXE], cwd=ROOT, env=env,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        hwnd = steps.find_window(p.pid)
        assert hwnd, f"{tag}：窗口没出来"
        assert steps.ensure_focus(hwnd), f"{tag}：拿不到前台"
        f0 = os.path.join(FIX_ROOT, f"{tag}-0.png")
        shoot.capture(hwnd, f0)
        bands = []
        for i in range(12):
            bands = nav_bands(f0)
            if len(bands) >= 2:
                break
            time.sleep(0.5)
            shoot.capture(hwnd, f0)
        ox, oy = frame_offset(hwnd)
        # 第二颗 =「我的仓库」。x/y **各减各的偏移**：从前记成"偏的是 y"点过 8px 的亏
        #（AGENTS：帧 = 客户 + (8, 0)），而这一档每次现场量，不抄那个数
        steps.click(hwnd, 28 - ox, bands[1] - oy, settle=2.2)
        prev = f0
        for i in range(10):
            time.sleep(0.8)
            cur = os.path.join(FIX_ROOT, f"{tag}-{i+1}.png")
            shoot.capture(hwnd, cur)
            from PIL import ImageChops
            d = ImageChops.difference(Image.open(prev).convert("RGB"),
                                      Image.open(cur).convert("RGB"))
            n = sum(1 for y in range(0, d.size[1], 3) for x in range(0, d.size[0], 3)
                    if d.getpixel((x, y)) != (0, 0, 0))
            if n < 30:
                break
            prev = cur
        park_cursor(hwnd)
        f = os.path.join(FIX_ROOT, f"{tag}-final.png")
        shoot.capture(hwnd, f)
        w = win_w(hwnd)
        # 左栏白面：从导航轨右边一点起扫，取第一段（第一段就是侧栏那张卡的白面）
        runs = white_runs(f, 300, 40, 1200)
        s = runs[0][1] if runs else 0
        print(f"  {tag}：客户区宽 {w}，左栏白面宽 {s}（起于 {runs[0][0] if runs else '-'}）")
        return {"w": w, "s": s, "frame": f}
    finally:
        p.terminate()
        try:
            p.wait(timeout=5)
        except Exception:
            pass


def frame_offset(hwnd):
    r = wt.RECT()
    u.GetWindowRect(hwnd, ctypes.byref(r))
    pt = wt.POINT(0, 0)
    u.ClientToScreen(hwnd, ctypes.byref(pt))
    return (pt.x - r.left, pt.y - r.top)


def main():
    os.environ["QB_APPDATA"] = SANDBOX
    st = os.stat(EXE)
    print(f"产物：{EXE}  mtime={datetime.datetime.fromtimestamp(st.st_mtime)}  size={st.st_size}")
    build_fixture()
    build_sandbox()
    before = qbguard.real_cfg_fp()
    try:
        lo, hi = read_range("SIDEBAR_W_RANGE")
        # 基线要留出 DRAG_DELTA 的余量，否则 D/E 那一跑会被 clamp 夹回同一个宽，
        # 等于两跑测的是同一个状态而判据以为它们差 40
        sw = max(lo, min(BASE["sidebar_w"], hi - DRAG_DELTA))
        print(f"   侧栏合法区间（从 core 现读）{lo}–{hi}；"
              f"他配的是 {BASE['sidebar_w']} → 基线取 {sw}，D/E 取 {sw + DRAG_DELTA}")
        if sw != BASE["sidebar_w"]:
            print(f"   ⚠️ 他那个数离上限只剩 {hi - BASE['sidebar_w']}，"
                  f"不够放 {DRAG_DELTA} 的增量 ⇒ 基线往下挪，五跑仍测同一条规则")

        a = run_case("A-open", True, sw)
        b = run_case("B-closed", False, sw)
        c = run_case("C-open-again", True, sw)
        d = run_case("D-open-wider", True, sw + DRAG_DELTA)
        e = run_case("E-closed-wider", False, sw + DRAG_DELTA)

        insp = BASE["inspector_w"]

        def need(open_, sidebar):
            return TRACK_W + sidebar + MAIN_MIN + (insp if open_ else 0.0) + EDGE_W

        def target(open_, sidebar):
            """规则的完整形状：**宽 = max(初始宽, 需求)**。
            那条规则只会"向右撑"，需求低于初始宽时它一个像素都不收——
            所以只写 `== need` 会在"需求小于初始"那一跑上假红（上一版就红在这里，
            而我当时把它写成 `>=`，等于把这条规则的另一半从判据里漏掉了）。
            `max` 让两种情形都由同一条判据负责，换谁的 sidebar_w 都不用改脚本"""
            return max(INITIAL_W, need(open_, sidebar))

        print(f"   算式：轨 {TRACK_W} + 侧栏 + 中间下限 {MAIN_MIN} + 抽屉 {insp}"
              f" + 右缘 {EDGE_W}，再与初始 {INITIAL_W} 取大；"
              f"侧栏基线 {sw}（读配置）→ A 该是 {target(True, sw)}、B 该是 {target(False, sw)}")
        ok = [
            # ── 两条**前提**（放在判据之前，并把名字写成"前提"）：
            # 状态走配置不等于配置里的值真的进了界面——应用会夹它（`clamp_sidebar_w`），
            # 而"两跑其实是同一个状态"若由后面的规则判据去发现，报出来的就是"规则坏了"。
            # 上一轮那三条红就是这么来的：红的是前提，不是实现
            ("前提：D 的侧栏比 A 宽 ≈ 40（没被 clamp 夹掉）",
             abs((d["s"] - a["s"]) - DRAG_DELTA) <= 3),
            ("前提：A 与 B 只差抽屉那一列（宽差 ≈ inspector_w，收起真的生效）",
             abs((a["w"] - b["w"]) - insp) <= 2),
            # ①② 就是补二十五那条原话：抽屉开与关，左栏那一列不许被抢
            ("B 与 A 左栏同宽（抽屉不抢左栏）", abs(b["s"] - a["s"]) <= 1),
            ("C 与 A 窗口同宽（再撑一次撑到同一个数）", abs(c["w"] - a["w"]) <= 2),
            ("C 与 A 左栏同宽", abs(c["s"] - a["s"]) <= 1),
            # ④ 撑宽这件事要能算出来，不是"看起来变宽了"
            ("A 的宽 == 算式给的数", abs(a["w"] - target(True, sw)) <= 2),
            ("B 的宽 == 算式给的数（含『需求不到初始就不收』那一半）",
             abs(b["w"] - target(False, sw)) <= 2),
            # ⑤ 侧栏变宽的 40 必须由窗口承担。判据取**两个增量的差**，
            # 不去反算中间列的绝对宽（那要把轨宽与右缘宽抄成脚本里的数）
            ("D 侧栏 +40 之后窗口确实长了", (d["w"] - a["w"]) >= 20),
            ("D 长的部分全部进了侧栏（没拿中间列垫）",
             abs((d["w"] - a["w"]) - (d["s"] - a["s"])) <= 3),
            ("D 的宽 == 算式给的数", abs(d["w"] - target(True, sw + DRAG_DELTA)) <= 2),
            ("E 的宽 == 算式给的数", abs(e["w"] - target(False, sw + DRAG_DELTA)) <= 2),
            # 头注释答应过的一条：D 与 E 只差抽屉那一列，证撑宽真的由抽屉参与
            # （宽差要读配置里那一列，不许抄一个 320 进来——32-15 就红在抄的数上）
            ("D 与 E 只差抽屉那一列（宽差 ≈ inspector_w）",
             abs((d["w"] - e["w"]) - insp) <= 2),
        ]
        print("\n判读：")
        for name, passed in ok:
            print(f"   {'通过' if passed else '不通过'}  {name}")
        print(f"   参考：A {a['w']}/{a['s']}  B {b['w']}/{b['s']}  C {c['w']}/{c['s']}"
              f"  D {d['w']}/{d['s']}  E {e['w']}/{e['s']}")
        return 0 if all(p for _, p in ok) else 1
    finally:
        after = qbguard.real_cfg_fp()
        print(f"真实 config 指纹 before={before} after={after}｜"
              f"{'一致' if before == after else '**不一致，查是谁写的**'}")


if __name__ == "__main__":
    sys.exit(main())
