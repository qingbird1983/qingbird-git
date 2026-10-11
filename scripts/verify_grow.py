"""31.4 补二十五那条回归判据：**抽屉开与关，左栏那一列不许被抢宽**。

判据全部落在帧上量的两个数（窗口客户区宽、左栏白面宽），不是"跑起来没报错"。

五跑两腿十七条判据（每跑各写一份 config 再启动，**状态走配置**；两腿走鼠标）：
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

  F 在 A 那一跑里**当场点**「收起右栏」→ 不另起进程，量点完的窗口宽与左栏白面
  G 再点回「展开右栏」      → 再量一次，与 A 比。
                        为什么要补这两腿：A–E 的状态全由配置给，**进程起来就是
                        那一态**，容器宽度从头到尾没变过；而补二十五那条病
                        （每一格都留着宽度偏好时基座按百分比重分整组）
                        **只在容器变化那一下发生**——点一次开合 = 外层容器少了
                        `inspector_w` 那一列、窗口又跟着还宽一次。
                        ⇒ 五跑一条都判不到它。2026-10-12 的变异实测就是证据：
                        把 `keep_main_flexible` 里"每帧把中间那一格交回 flex"
                        改成恒不执行，**十二判据一条都不红**。
                        两腿的判据：F 三条（前提「config 那一格翻了」+ 左栏白面
                        == A + 宽 == 收起态算式），G 两条（同样的前提 +
                        往返回到 A 的左栏与宽）。
                        坐标按 a11y 名字现场量（那颗键带 `accessibility_label`），
                        生效判据读沙箱 config，**都不靠像素**

与上一版的三处不同（2026-10-11 按路线表 32-15 的尾巴重写）：
  ① **夹具自己造**（%TEMP 下一颗合成仓库 + 沙箱 config），不再吃 10-08 那轮
     留在 `%TEMP%\\qb_v4` 里的旧沙箱——上一版红就红在它读到的是别人的状态；
  ② **删掉拖把手那一步**：`sidebar_w` 是配置项，直接写进 config 就是"拖完之后"
     那一态，比模拟拖拽稳（拖拽要猜把手坐标，而把手位置随窗口漂）；
  ③ **收起态那一跑不必点键**：B / E 走 `inspector_open=false` 的配置就是收起态。
     但**会话中开合**那一档不在这条配置路径上，由 F / G 两腿点那颗键负责
     ⇒ 全程三次点击：导航轨「我的仓库」（y 现场扫）+ 两腿各一下，
     后两下的坐标按 a11y 名字现场量（D93 那条：写死的坐标改版之后全过期过）。

⚠️ **这次重写换来的稳，代价要说清**：它验的是**宽度规则**（谁要更多宽、由哪一列垫）。
2026-10-12 之前这一段写的是"点收起键那条路径不由这条脚本负责"——**那句已经不准了**：
F / G 两腿走的就是那颗键（点下去 → 量 → 点回来 → 量）。
现在仍然不负责的是**「拖把手」**：把手坐标随窗口宽漂（D93 那条），模拟拖拽红的
时候分不清是规则坏了还是坐标坏了，所以它归侧栏 / 抽屉的单测 + 他双击实机。
**别把它当"拖把手也回归过了"**。

## 变异账（2026-10-12，驱动 `target/mutate_grow2.py` / `mutate_grow3.py`，一次性不入仓库）

三条，各自单变量、跑完按 sha 逐字节还原：

  · **V2 实现侧 `MAIN_MIN_W` 950→100** ⇒ 红 8 条（算式四条 + 前提"只差抽屉那一列" +
    D 那两条 + D/E 差值那条）。**承重成立**。
  · **V3 只改脚本里那颗抄来的常数 950→100，二进制保持正确** ⇒ 红 5 条，
    **正好是"宽 == 算式给的数"那五条**（A / B / D / E / **F**），一条不多一条不少。
    这条证的是头注释答应过的那件事：四个壳层常数改版会红**并点名是哪一条**；
    没有它，V2 的红就分不清是"实现坏了"还是"算式本来不敏感"。
    ⚠️ V3 第一轮跑错了：驱动的还原**只在 `need_build` 分支里重编**，而 V3 标的是
    `False` ⇒ 二进制里还躺着 V2 的 100，两个变量一起动、两侧口径一致地错，
    那条结果作废（第二轮把"每条之前无条件还原 + 重编 + 回读确认变异串在盘上"写死）。
  · **V1 把 `shell.rs::keep_main_flexible` 的每帧交回改成恒不执行 ⇒ 不红。**
    第二轮它红过一条「B 与 A 左栏同宽」，第三轮单独重跑（全文留在
    `target/grow_V1_full.log`）**17 条全绿**，而两跑的白面数是这样换着的：
    基线 A 313 / B 312，V1 跑 A 312 / B 313 ⇒ 那条"红"落在**±1 容差的抖动带上**，
    不能归因给 V1。⇒ 三条结论一起登记：
    ① **这一档今天没有判据负责**：这条脚本（含 F / G 两腿）测不出
       "每帧交回偏好"有没有在做工——保护整个关掉，量出来的宽一个都不变。
    ② 由此有两种解释，**我没有查清是哪一种**：
       要么补二十五那条病在现在的代码形态下已经不会发生（那颗保护成了冗余保险，
       与 32-9 那颗 `busy_timeout` 同族），要么触发它的事件序列不在本脚本覆盖的
       范围内（历史上量到 630 那一次是**会话中反复开合 + 拖把手**）。
       查法要往 `ResizableState::sizes()` 里插桩打数，登记成新未决。
       ⛔ **不许因为"V1 不红"就把 `keep_main_flexible` 删掉**——那是拿"我的判据
       看不见"当"它不存在"，正是本项目最恨的那种反向错误。
    ③ 判据的**分辨率下限**要说清：白面宽在跑与跑之间自然抖 **1px**，而容差就是 1px
       ⇒ 这条脚本对 1px 级的漂移没有区分度，只抓得住 ≥2px 的（历史上那条病是 100px 级）。
       新增判据时别把容差调到 0，那不是"更严"，那是把抖动判成缺陷。

F / G 两腿是这一轮新加的，**承重由 V3 证过**（「F 的宽 == 收起态算式给的数」跟着红）；
它们**没能**证到的是"会话中开合那一下会不会抢左栏"（V1 不红，同上②）。

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
import a11y_stress as A  # noqa: E402  只用它的 a11y dump / 按名字取矩形 / 客户原点

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


def wait_stable(hwnd, prev, tag, rounds=10, budget=30):
    """抓到"画面不再变"为止，返回最后一帧路径。

    抽成一颗函数是给 A–E 五跑与 F/G 两腿**共用**的：两腿之后也要等窗口把
    还宽 / 再撑宽那两段跑完才能量，各写一遍就是两个会漂的数（本项目为
    "同一件事两个出处"交过四次学费）。
    """
    from PIL import ImageChops
    cur = prev
    for i in range(rounds):
        time.sleep(0.8)
        cur = os.path.join(FIX_ROOT, f"{tag}-{i + 1}.png")
        shoot.capture(hwnd, cur)
        d = ImageChops.difference(Image.open(prev).convert("RGB"),
                                  Image.open(cur).convert("RGB"))
        n = sum(1 for y in range(0, d.size[1], 3) for x in range(0, d.size[0], 3)
                if d.getpixel((x, y)) != (0, 0, 0))
        if n < budget:
            break
        prev = cur
    return cur


def measure(png):
    """一帧量两个数：窗口客户区宽与左栏白面宽。A–E 与 F/G 同一颗函数。"""
    runs = white_runs(png, 300, 40, 1200)
    return runs[0][1] if runs else 0, (runs[0][0] if runs else "-")


def toggle_leg(label, hwnd, seq):
    """会话中点标题栏那颗「收起右栏 / 展开右栏」，量点完之后的两个数。

    三条纪律：
    · **坐标按名字现场量**——那颗键带 `accessibility_label`（`shell.rs:794/796`），
      矩形由系统给，不目测、不写死（D93；a11y 走查 2026-10-11 就是靠这一条
      把三处过期坐标换掉的）。
    · **生效判据不靠像素**：点下去会把 `inspector_open` 写回沙箱 config
      （`store::set_inspector_layout`），翻了才叫点中了——"alive / 画面动过"
      都不算（AGENTS：`alive` 的语义只是没崩）。
    · **点完要等稳**（`wait_stable`）：还宽与再撑宽是两次 `Window::resize`，
      抢量会量到中间态。
    """
    clx, cly = A.client_origin(hwnd)
    n = A.one(A.dump(hwnd), label, "Button")
    before = os.path.join(FIX_ROOT, f"live-{seq}-before.png")
    shoot.capture(hwnd, before)
    steps.click(hwnd, n.cx - clx, n.cy - cly, settle=1.2)
    open_after = json.load(open(CFG, encoding="utf-8-sig"))["settings"]["inspector_open"]
    wait_stable(hwnd, before, f"live-{seq}")
    park_cursor(hwnd)
    p = os.path.join(FIX_ROOT, f"live-{seq}-{label}.png")
    shoot.capture(hwnd, p)
    s, x0 = measure(p)
    w = win_w(hwnd)
    print(f"  腿 {seq}「{label}」：客户区宽 {w}，左栏白面宽 {s}（起于 {x0}），"
          f"config 的 inspector_open = {open_after}")
    return {"w": w, "s": s, "open_after": open_after, "frame": p}


def run_case(tag, inspector_open, sidebar_w, toggles=()):
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
        wait_stable(hwnd, f0, tag)
        park_cursor(hwnd)
        f = os.path.join(FIX_ROOT, f"{tag}-final.png")
        shoot.capture(hwnd, f)
        w = win_w(hwnd)
        # 左栏白面：从导航轨右边一点起扫，取第一段（第一段就是侧栏那张卡的白面）
        s, x0 = measure(f)
        print(f"  {tag}：客户区宽 {w}，左栏白面宽 {s}（起于 {x0}）")
        res = {"w": w, "s": s, "frame": f, "live": []}
        # 两腿就在这一跑的会话里点，不另起进程：要的就是"容器变过"那个事件序列
        for ix, label in enumerate(toggles, 1):
            res["live"].append(toggle_leg(label, hwnd, f"{tag}-{ix}"))
        return res
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

        a = run_case("A-open", True, sw, toggles=("收起右栏", "展开右栏"))
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
        # 两腿的数：少了腿要**当场点名"前提没造出来"**，不许留到判据里 IndexError
        assert len(a["live"]) == 2, f"F / G 两腿只跑出来 {len(a['live'])} 条 ⇒ 判据不能判"
        f_leg, g_leg = a["live"]
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
            # ── 会话中开合抽屉的两腿：A–E 判不到的那一档（容器宽度变化那一下）
            ("前提：F 那一下点到了（config 的 inspector_open 翻成 false）",
             f_leg["open_after"] is False),
            ("F 会话中收起抽屉后左栏没被抢宽（F 的白面 == A 的）",
             abs(f_leg["s"] - a["s"]) <= 1),
            ("F 的宽 == 收起态算式给的数（还宽只还我们自己撑的那一段）",
             abs(f_leg["w"] - target(False, sw)) <= 2),
            ("前提：G 那一下点回来了（config 的 inspector_open 翻回 true）",
             g_leg["open_after"] is True),
            ("G 往返之后左栏与窗口宽都回到 A 的数（每开合一次漂一次就是补二十五）",
             abs(g_leg["s"] - a["s"]) <= 1 and abs(g_leg["w"] - a["w"]) <= 2),
        ]
        print("\n判读：")
        for name, passed in ok:
            print(f"   {'通过' if passed else '不通过'}  {name}")
        print(f"   参考：A {a['w']}/{a['s']}  B {b['w']}/{b['s']}  C {c['w']}/{c['s']}"
              f"  D {d['w']}/{d['s']}  E {e['w']}/{e['s']}"
              f"  F(会话收起) {f_leg['w']}/{f_leg['s']}  G(点回来) {g_leg['w']}/{g_leg['s']}")
        return 0 if all(p for _, p in ok) else 1
    finally:
        after = qbguard.real_cfg_fp()
        print(f"真实 config 指纹 before={before} after={after}｜"
              f"{'一致' if before == after else '**不一致，查是谁写的**'}")


if __name__ == "__main__":
    sys.exit(main())
