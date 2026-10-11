"""阶段 21 出口最后一格（路线表 2841）：三条待量的东西一次量完。

要量的三条（原文的出口）：
- **21-8 三级明暗**：一级 `foreground` / 二级 `body(t)` / 三级 `tier3(t)`——
  出口要的是"帧上三档都画出来了、且两两分得出"，不是"代码里选了这三颗函数"
- **21-3 胶囊高度**：出口写「抓帧核胶囊高度」，而 `pill_h()` 是三档字号的函数
  ⇒ 光量中档一次不算"核过"，**三档各量一次**才证得了"字号一改胶囊跟着长"
  （D77 那条：行高与胶囊高必须是字号的函数，不许写死）
- **21-7 头部到页签的间距**：⚠️ 这一条的**判据对象已经换过位置**——原文量的是
  「云端地址字带下沿 → 页签字带上沿」那段卡**外**的 gap，而 31.4 把页签收进了头卡
  底部（路线表 4376 行：「判断没错，位置错了」）。⇒ 这里量的是现行形态：
  头卡内「最后一行文字下沿 → `card_split` 那条线 → 页签文字上沿」两段气口

## 四处判据的地基（都是量出来的，不是拍的）

① **文字色按"精确等于该档"数点**。2026-10-12 探针实测浅色总览一帧：
   `foreground` 896 颗、`body` 177 颗、`tier3` 1746 颗、`tier4` 426 颗 ⇒ 抗锯齿没把
   笔画中心色打散，"完全相等"这一档在这里**能用**（与卡间距那条相反，那里画布被
   阴影压暗过，"完全等于 canvas"一条都认不上）。
② **档值一律现读**：`foreground` / `muted.foreground` 从 `gui/theme.json`，
   `body` / `tier4` 从 `gui/src/tokens.rs` 的函数体，`pill_h` / `meta` 从 `Metrics` 字面量。
   写死任何一个数，脚本就退化成"迎合实现"。
③ **胶囊不许"全帧自动找浅色矩形"，要锚在 a11y 给的 `DataItem` 行矩形里量**。
   这是连错四轮才落对的一条，四轮全是**判据错、界面从头到尾是对的**
   （四轮返工记在 `pill_in_rows` 的头注释）。最关键的一点：官方 `Button`（树里 79x22）
   与侧栏选中行铺的是**同一个 `wash` 档**，光靠颜色分不开它们和胶囊；
   而表格里最高的东西就是那颗胶囊（字 12–16、图标 14–16 都比 `pill_h` 矮）——
   把量测锚到"系统给的那一行"里，这些反例就结构上不存在了。
④ **基准色必须由调用方说清"这一帧是哪套主题"**，不能用 `a11y_stress.card_face_of`
   （它读沙箱 config 此刻的主题名）。本轮一次跑抓了四套主题×字号再统一量，
   最后写进 config 的是深色 ⇒ 拿它解释浅色帧，浅色帧里一个卡面像素都匹配不上。
   详见 `theme_colors` 的头注释。

用法：python scripts/verify_type_tiers.py            # 抓 + 量（四次起进程）
      python scripts/verify_type_tiers.py --measure-only   # 只重量（帧与矩形表留在盘上）

## 变异验证做过（2026-10-12，全走 --measure-only：帧留盘、只动真源或过滤条件）
- **M1** `tokens.rs` 里 `pill_h` 中档 24→26 ⇒ **精确红在 medium 那一屏**（"期望值真的从源码现读"）
- **M2** `theme.json` 浅色 `muted.foreground` 改掉 ⇒ 红在「tier3 在帧上 0 颗」，
  而且**只红浅色那三屏、深色一屏不红**（"按档取色"没串档）
- **M3** 行内量测的 `DataItem` 过滤名写错 ⇒ 红在「有 1 条表格行却一颗都没量到」
  （"量不到不许当通过"那一条真的有牙）
- **M4【预期不红】** 把气口阈值 `16 + 4` 改成 `100` ⇒ **一条都不红**。
  ⛔ 这不是脚本坏了，是**③ 那一档根本没有独立判据**——它只报数、不判阈值
  （阈值该是多少归他双击看）。⇒ **别把 ③ 读成"撑住了下限"**，它撑的是"量得到量不到"。
还原：三条被改文件（`tokens.rs` / `theme.json` / 本脚本）每条之间**一起**还原，
末端按 sha 逐字节核对相同，还原后复跑一次全绿。
"""

import hashlib
import json
import os
import re
import subprocess
import sys
import time

sys.stdout.reconfigure(encoding="utf-8", errors="replace")   # GBK 控制台编不出 ⇒
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
from PIL import Image

import qbguard
import steps as _steps
import shoot as _shoot
import a11y_stress as A
import verify_dark_five as V

REPO = A.REPO
WORK = os.path.join(os.environ["TEMP"], "qb_tiers")
LIGHT = "qingbird-light"


# ───────────── 真源现读 ─────────────

def theme_colors(name):
    """一帧主题的三档文字色 + 卡面档：一级与三级是 token，二级是 `tokens.rs` 里的常量。

    ⛔ **基准色必须由调用方把"这一帧是哪套主题"传进来**，不能用 `a11y_stress.card_face_of`
    ——那颗函数读的是**沙箱 config 此刻**的主题名（`_saved_theme()`）。本轮一次跑里
    抓了四套主题×字号组合、之后才统一量，而最后写好那份 config 是 `qingbird-dark`
    ⇒ 拿它去解释浅色帧，卡面档就成了 (22,38,43)、浅色帧里一个像素都匹配不上。
    症状实测有两种：`card_area` 报"0 行 0 列有卡面"（抛出来还算好的），
    以及**整帧都被当成"非卡面"**、游程横跨整屏 ⇒ 判据量到的是屏幕底色而不是胶囊。
    （AGENTS「色档永远按当下这张帧量，别拿别的场景的常数」的第二种触发：
    这次不是别的场景，是**别的批次**。）
    """
    d = json.load(open(os.path.join(REPO, "gui", "theme.json"), encoding="utf-8-sig"))
    for t in d["themes"]:
        if t["name"] == name:
            c = t["colors"]
            return {
                "t1 foreground": _hex(c["foreground"]),
                "t3 tier3": _hex(c["muted.foreground"]),
                "face": _hex(c["secondary.background"]),
            }
    raise AssertionError(f"theme.json 没有 {name}")


def _hex(s):
    return (int(s[1:3], 16), int(s[3:5], 16), int(s[5:7], 16))


def src_consts():
    """从 `tokens.rs` 现读：`body()` 的两档文字色 + 三档 `pill_h` + 三档 `meta` 字号"""
    src = open(os.path.join(REPO, "gui", "src", "tokens.rs"), encoding="utf-8").read()
    m = re.search(r"fn body\(t: &Theme\)[\s\S]{0,400}?rgb\(0x([0-9A-Fa-f]{6})\)"
                  r"[\s\S]{0,200}?rgb\(0x([0-9A-Fa-f]{6})\)", src)
    if not m:
        raise AssertionError("tokens.rs 里 body() 的形状变了，正则没命中 ⇒ 去看一眼再改脚本")
    body = {"dark": _hex("#" + m.group(1)), "light": _hex("#" + m.group(2))}
    # ⚠️ 别用 `\npill_h` 锚行首：`.rs` 在工作树里是 **CRLF**（AGENTS 记着那条），
    # `\n` 前面夹着 `\r` 就一条都不匹配——症状是"没读全"，看着像代码变了形
    pills = [float(x) for x in re.findall(r"pill_h: (\d+\.\d)", src)]
    metas = [float(x) for x in re.findall(r"meta: (\d+\.\d)", src)]
    if len(pills) != 3 or len(metas) != 3:
        raise AssertionError(f"Metrics 字面量没读全：pill_h {pills} / meta {metas}")
    return body, dict(zip(("small", "medium", "large"), pills)), \
        dict(zip(("small", "medium", "large"), metas))


def lum(c):
    """粗亮度（通道均值）。只用来比"两档分不分得出"，不当色值用。"""
    return sum(c) / 3.0


# ───────────── 抓帧 ─────────────

def grab(theme, scale):
    """起一次进程：指定主题与字号档，抓「总览」与「我的仓库-概览」两屏。
    返回 [(tag, png, 帧坐标矩形表)]"""
    # 夹具自己造 + 自己验：`build_sandbox` 只把 `A.FIXTURE` 这个路径写进 repos，
    # 那颗仓库是不是真 git 仓库它不管。上一轮的临时目录被清理之后，这里就会拿到
    # 一个"没有 .git 的空文件夹"，而三档字色照样量得出数 ⇒ **测的是另一屏而报告不知情**。
    # （2026-10-12 做变异验证时在同一族脚本上炸出来的，三份脚本一起补。）
    A.build_fixture()
    assert os.path.isdir(os.path.join(A.FIXTURE, ".git")), \
        f"夹具 {A.FIXTURE} 没有 .git ⇒ 概览页画的是『不是仓库』那一屏"
    A.build_sandbox(False, theme=theme)
    cfg = json.load(open(A.CFG, encoding="utf-8-sig"))
    cfg["settings"]["ui_scale"] = scale
    json.dump(cfg, open(A.CFG, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
    subprocess.run(["taskkill", "/F", "/IM", "qingbird-git.exe"], capture_output=True)
    time.sleep(0.8)
    env = qbguard.launch_env()
    assert env["APPDATA"] == A.SANDBOX, f"沙箱没生效：{env['APPDATA']}"
    proc = subprocess.Popen([A.EXE], cwd=REPO, env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    d = os.path.join(WORK, f"{theme}-{scale}")
    A.clean(d) if os.path.isdir(d) else None
    os.makedirs(d, exist_ok=True)
    out = []
    try:
        hwnd = A.find_window(proc.pid)
        assert hwnd and _steps.ensure_focus(hwnd)
        clx, cly = A.client_origin(hwnd)

        def tap(name, kind=None, settle=1.6):
            n = A.one(A.dump(hwnd), name, kind)
            _steps.click(hwnd, n.cx - clx, n.cy - cly, settle=settle)

        def grab_shot(tag):
            """⚠️ 窗口原点在两屏之间**会变**：进「我的仓库」时 31.4 补二十五那条
            「主区不足就向右撑窗口」会把窗口从 1296 撑到 1375（实测两屏帧宽不同）。
            ⇒ 换算必须在每一帧抓的这一刻重取，不能在进程启动时取一次。
            （本轮一度把"量到一快 87x28 的 wash 块"归因于这里没重取——**那个推断是错的**，
            真凶是 `pill_blocks` 里三维 mask 忘了 `.all(axis=2)`、列索引被放大 3 倍。
            重取仍然要留：它挡的是"第二屏所有矩形整体右移 79px"这一类，与那个 bug 无关。）"""
            wl, wt = A.window_rect(hwnd)[0], A.window_rect(hwnd)[1]
            nodes = A.dump(hwnd)
            p = os.path.join(d, tag + ".png")
            _shoot.capture(hwnd, p)
            # **帧坐标**（= 屏幕 − 窗口原点）的矩形表：页签用 `TabItem`、
            # 胶囊所在那一行用 `DataItem`——两条都不目测坐标
            rects = [(n.x - wl, n.y - wt, n.w, n.h, f"{n.kind}:{n.name}")
                     for n in nodes]
            # 矩形表落盘：`--measure-only` 那一档要能不带进程复跑判据（一次起进程 25 秒，
            # 判据要改好几轮 ⇒ 别逼自己每轮都重抓一遍）
            json.dump(rects, open(os.path.join(d, tag + ".rects.json"), "w",
                                  encoding="utf-8"), ensure_ascii=False)
            out.append((tag, p, rects))

        _steps.click(hwnd, 700, 300, settle=1.0)
        assert _steps.ensure_focus(hwnd)
        grab_shot("00-总览")
        tap("我的仓库")
        grab_shot("01-我的仓库-概览")
    finally:
        proc.kill()
        time.sleep(0.5)
    return out


# ───────────── 判据一：三档文字色 ─────────────

def color_hits(png, want):
    """返回 {档名: (精确点数, 出现位置包围盒)}；包围盒用来防"一档全命中在一枚图标上" """
    a = np.asarray(Image.open(png).convert("RGB"), dtype=np.int16)
    res = {}
    for name, c in want.items():
        m = (a == np.array(c, dtype=np.int16)).all(axis=2)
        ys, xs = np.nonzero(m)
        res[name] = (int(m.sum()),
                     None if len(ys) == 0 else
                     (int(xs.min()), int(ys.min()), int(xs.max()), int(ys.max())))
    return res


# ───────────── 判据二：胶囊高度 ─────────────

def card_area(a, face, row_min=0.5, col_min=0.25):
    """**主区那张卡**的矩形 `(x0, x1, y0, y1)`：按"这一行/这一列有多少卡面"划出来。

    为什么必须先划区域（2026-10-12 实测）：标题栏那 40 行整行都是画布色，
    "卡面白底上的一块浅色矩形"这个描述在它身上同样成立 ⇒ 不划区域就会量到
    **标题栏的高度**（35/40/44，正好是三档 `title_bar_h`），报成"胶囊比 pill_h 高一档"。

    ⚠️ 取的是**最长那一连续段**，不是"首列到末列"。浅色档下
    `sidebar.background` 与卡面**同一个值**（两条主题都写 `#FFFFFF`）⇒
    "有卡面的列"会把侧栏一起算进来，而侧栏选中行铺的正是 `wash` 档那块浅色矩形
    （实测 x 79..252、高 15/16/17）——它不是胶囊，是三档行高里侧栏那一行。
    侧栏与主区卡之间隔着画布列，所以"最长连续段"给的就是主区卡那一段。
    """
    f = np.array(face, dtype=np.int16)
    isf = (a == f).all(axis=2)

    def longest(mask):
        best, cur = (0, 0), None
        for i, v in enumerate(mask):
            if v:
                if cur is None:
                    cur = [i, i]
                else:
                    cur[1] = i
            elif cur is not None:
                if cur[1] - cur[0] > best[1] - best[0]:
                    best = tuple(cur)
                cur = None
        if cur is not None and cur[1] - cur[0] > best[1] - best[0]:
            best = tuple(cur)
        return best

    mask_rows = isf.mean(axis=1) >= row_min
    mask_cols = isf.mean(axis=0) >= col_min
    # ⚠️ **两个轴用两种取法，不是随手写的**：
    # · x 取**最长连续段** ⇒ 把侧栏那一列隔掉（它和卡同值，见上面）
    # · y 取**首尾跨度** ⇒ 因为纵向必然被打断：表头那一行整行是 `sunken` 档、
    #   白占比 0，于是"最长段"只给出一截 67 行（实测 y 95..162），
    #   而状态列那颗胶囊在 y 443 ⇒ 又被区域挡在门外。
    #   第一版两轴都取 min/max、第二版两轴都取 longest，**各错一半**。
    ys = np.flatnonzero(mask_rows)
    if len(ys) < 40:
        raise AssertionError(f"卡区划不出来：只有 {len(ys)} 行有卡面")
    rows = (int(ys.min()), int(ys.max()))
    cols = longest(mask_cols)
    if cols[1] - cols[0] < 40:
        raise AssertionError(f"卡区列段太窄：{cols}")
    return int(cols[0]), int(cols[1]), rows[0], rows[1]


def pill_in_rows(png, face, rects, pad=2):
    """在**系统给的那一行**里量胶囊高：取行内"竖直连续非卡面"最长的那一列。

    ## 为什么放弃"全帧自动找浅色矩形"（那一版连错四轮，每轮都是判据错、界面没错）
    · 按"非卡面游程 + 段内众数"扫 ⇒ 抓到的是标题栏（整行画布）与统计卡里 40×40 的
      图标底块，而图标把底块拦腰打断成两个 18 高的碎块
    · 换成"按底色 mask + 覆盖率" ⇒ 圆角首末行的跨度掉到 `wmin` 以下被筛掉，
      量出来的高度**系统性地少 2px**（22 档量到 20）⇒ "等于 `pill_h`"必然假红
    · 而一帧里"浅色矩形"本来就比胶囊多：`wash` 这一档同时被官方 `Button`、
      侧栏选中行、告警 wash 在用，**光靠色分不开**（上一版还因此多写了一条
      "帽形"和一张交互件排除表，两条都依赖像素坐标换算，越补越脆）

    换成"坐在已知那一行里量"就一步砍掉全部这些反例：表格行里最高的东西就是
    那颗胶囊（字 12–16、图标 14–16 都比 `pill_h` 的 22/24/28 矮），
    而**行在哪一格由 a11y 的 `DataItem` 矩形给**（名字应用写、矩形系统量，D93）。

    返回 [(高, 宽, x, y0, y1)]，一行一颗。`pad` 用来避开行的上下边框——
    ⚠️ 不避开的话每一列都是"整行高"，判据会量到 40 而不是 24。
    """
    a = np.asarray(Image.open(png).convert("RGB"), dtype=np.int16)
    h, w = a.shape[:2]
    f = np.array(face, dtype=np.int16)
    out = []
    for (rx, ry, rw, rh, lbl) in rects:
        if not lbl.startswith("DataItem"):
            continue
        rx, ry, rw, rh = int(rx), int(ry), int(rw), int(rh)
        if rx < 0 or ry < 0 or rx + rw > w or ry + rh > h:
            raise AssertionError(f"{lbl} 的矩形越出帧外：{rx},{ry} {rw}x{rh} 帧 {w}x{h}")
        sub = (a[ry + pad: ry + rh - pad, rx: rx + rw] != f).any(axis=2)
        # 逐列的最长连续非卡面段（向量化累加游程，再取全行最大那一列）
        runs = np.zeros(sub.shape[1], dtype=np.int32)
        best = np.zeros(sub.shape[1], dtype=np.int32)
        for j in range(sub.shape[0]):
            runs = np.where(sub[j, :], runs + 1, 0)
            best = np.maximum(best, runs)
        cx = int(np.argmax(best))
        bh = int(best[cx])
        if bh < 8:
            continue
        # 该列最长连续段的起止（要它的中位行来水平扩宽）
        col, bs, be, s = sub[:, cx], 0, 0, None
        for j in range(len(col) + 1):
            v = bool(col[j]) if j < len(col) else False
            if v and s is None:
                s = j
            elif not v and s is not None:
                if j - s > be - bs:
                    bs, be = s, j
                s = None
        # 在中位行上向两侧扩到卡面 ⇒ 得到这一块有多宽
        ym = ry + pad + (bs + be) // 2
        x0 = x1 = cx
        while x0 > 0 and not (a[ym, rx + x0 - 1] == f).all():
            x0 -= 1
        while x1 < rw - 1 and not (a[ym, rx + x1 + 1] == f).all():
            x1 += 1
        out.append((bh, x1 - x0 + 1, rx + cx, ym - bh // 2, ym + bh // 2, lbl))
    return out


# ───────────── 判据三：头卡内「文字 → 线 → 页签」 ─────────────

def head_gap(png, vals, rects, face):
    """量头卡里「最后一行文字 → 那条分隔线 → 页签」的两段气口。

    ⚠️ 页签的位置**不目测、不从像素猜**，用 a11y 树里 `TabItem` 的矩形——
    名字由应用写、矩形由系统量（这是 D93 那条「改版之后脚本里的坐标要重量一遍」
    的正解：干脆不留坐标）。
    线取「页签上沿往上的第一条横贯 `line-soft` 线」。⚠️ 第一版按"从上往下第二条
    横贯线"取，量到的是 y=100 那条**统计卡之间**的线，气口报成 0——
    **线要按特征选，不能按序号选**（序号依赖"这一屏上面正好有几条线"，版面一改就漂）。
    文字行 = 与卡面色差 > 60 的行。⚠️ 不能用"非白"当有文字：卡边、分隔线、阴影带
    的色差只有 20–40，全都算"非白"，第一版因此上下的气口都量成 0。
    """
    a = np.asarray(Image.open(png).convert("RGB"), dtype=np.int16)
    h, w = a.shape[:2]
    fv = np.array(face, dtype=np.int16)
    ax, bx, _ay, _by = card_area(a, face)
    soft = np.array(vals["line-soft"], dtype=np.int16)
    tabs = [r for r in rects if r[4].startswith("TabItem")]
    if not tabs:
        return None, "这一屏的 a11y 树里没有 TabItem ⇒ 页签不在头卡里，或它没进树"
    tx, ty, tw, th, lbl = min(tabs, key=lambda r: r[1])
    width = bx - ax + 1
    lines = [y for y in range(h)
             if y < ty and int((a[y, ax:bx + 1] == soft).all(axis=1).sum()) >= 0.55 * width]
    if not lines:
        return None, f"页签上沿 y={ty} 之上没有横贯卡内的 line-soft 线"
    y_line = max(lines)

    def text_row(y):
        # ⚠️ 只在**卡内 x 范围**里判有没有文字：整行扫会被右缘滚动条打穿
        # （`scrollbar.thumb.background` (215,224,228) 与卡面差 40 不算，
        #  但 hover 那一档 (154,171,175) 差 101 ⇒ 每一行都"有文字"、
        #  上下气口全量成 0）。实测：浅档那一次上气口报 0px，查因就是它。
        return 0 <= y < h and int((np.abs(a[y, ax:bx + 1] - fv).max(axis=1) > 60).sum()) >= 3

    up = next((y for y in range(y_line - 1, max(y_line - 120, -1), -1) if text_row(y)), None)
    dn = next((y for y in range(y_line + 1, min(y_line + 120, h)) if text_row(y)), None)
    return {"line": y_line, "tab_top": ty, "tab_h": th, "tab": lbl,
            "x": (ax, bx), "width": width,
            "above": up, "below": dn,
            "gap_a": None if up is None else y_line - up - 1,
            "gap_b": None if dn is None else dn - y_line - 1,
            "gap_tab": ty - y_line - 1}, None


# ───────────── 主流程 ─────────────

def main():
    all_hex = V.hexes()
    dk = [n for n in all_hex if "dark" in n][0]
    body, pills, metas = src_consts()
    only = "--measure-only" in sys.argv
    before = qbguard.real_cfg_fp()
    shots = {}
    try:
        runs = [(LIGHT, "small"), (LIGHT, "medium"), (LIGHT, "large"), (dk, "medium")]
        for theme, scale in runs:
            if only:
                d = os.path.join(WORK, f"{theme}-{scale}")
                shots[(theme, scale)] = [
                    (f[:-4], os.path.join(d, f),
                     json.load(open(os.path.join(d, f[:-4] + ".rects.json"), encoding="utf-8")))
                    for f in sorted(os.listdir(d)) if f.endswith(".png")]
                continue
            print(f"\n== 抓帧：{theme} / 字号 {scale} ==")
            shots[(theme, scale)] = grab(theme, scale)
    finally:
        qbguard.check_real_cfg(before)

    bad = []

    print("============ ⓪ 前置自证：帧是新的、深色那一跑真的是深色 ============")
    # 两条都是本轮当场要用的地基，缺一条后面三段的结论都不成立：
    # ① 四次起进程抓八帧，若某次没换屏（或抓到上一次的窗口），"三档字号各量一次"
    #    就变成"一档量了三次"；判据取逐字节 sha 两两不同。
    # ② 深色那一跑要是**静默落回浅色**（`settings.theme` 存的是主题名字，写错不报错），
    #    ①②③ 三段全是在浅色帧上量深色判据 ⇒ 每一条都会假绿。
    seen, allp = {}, []
    for (theme, scale), fr in shots.items():
        for tag, p, _r in fr:
            allp.append((theme, tag, p))
            h = hashlib.sha256(open(p, "rb").read()).hexdigest()[:16]
            if h in seen:
                bad.append(f"两帧逐字节相同：{seen[h]} == {theme}/{tag}（{h}）")
            seen[h] = f"{theme}/{tag}"
    print(f"  共 {len(allp)} 帧，sha 两两不同：{'✓' if len(seen) == len(allp) else '⛔'}")
    for theme in {t for t, _ in shots}:
        n = sum(1 for p in allp if p[0] == theme)
        print(f"   {theme} 抓到 {n} 帧")
    lt_canvas = V.hexes()[LIGHT]["canvas"]
    dk_canvas = V.hexes()[dk]["canvas"]
    assert lum(dk_canvas) < lum(lt_canvas), "深色档的 canvas 不比浅色暗 ⇒ theme.json 读错了"
    for theme in {t for t, _ in shots}:
        want = V.hexes()[theme]["canvas"]
        for tag, p, _r in shots[(theme, "medium")]:
            n = color_hits(p, {"canvas": want})["canvas"][0]
            ok = n > 5000
            print(f"   {theme} / {tag}：画布档 {want} 精确 {n} 颗 "
                  f"{'✓ 这一跑真的是这一档' if ok else '⛔ 底色没换'}")
            if not ok:
                bad.append(f"{theme}/{tag}：画布档只精确出现 {n} 颗 ⇒ 主题没生效")

    print("\n============ ① 21-8：三档文字色在帧上是否都画出来 ============")
    print(f"  真源现读：body 浅 {body['light']} / 深 {body['dark']}，"
          f"pill_h {pills}，meta 字号 {metas}")
    for (theme, scale), fr in shots.items():
        tcol = theme_colors(theme)
        want = {"t1 foreground": tcol["t1 foreground"],
                "t2 body(t)": body["dark" if "dark" in theme else "light"],
                "t3 tier3": tcol["t3 tier3"]}
        order = [want["t1 foreground"], want["t2 body(t)"], want["t3 tier3"]]
        if "dark" in theme:
            order = list(reversed(order))          # 深底下一级要**最亮**
        lums = [lum(c) for c in order]
        print(f"\n-- {theme} / {scale} -- 亮度序 "
              f"{' < '.join(f'{v:.1f}' for v in sorted(lums))}")
        for i in range(2):
            d = abs(lums[i + 1] - lums[i]) if "light" in theme else abs(lums[i] - lums[i + 1])
            print(f"   相邻两档亮度差：{d:.1f}/255", "⛔ 分不出" if d < 12 else "可辨")
            if d < 12:
                bad.append(f"{theme}：一级/二级/三级里有两档只差 {d:.1f}/255 ⇒ 肉眼分不出层级")
        for tag, p, _r in fr:
            hits = color_hits(p, want)
            for name in ("t1 foreground", "t2 body(t)", "t3 tier3"):
                n, box = hits[name]
                mark = "" if n else "  ⛔ 这一帧上一颗都没有"
                print(f"   {tag:16s} {name:14s} {n:6d} 颗  包围盒 {box}{mark}")
                if not n:
                    bad.append(f"{theme}/{scale}/{tag}：{name} 在帧上 0 颗")

    print("\n============ ② 21-3：胶囊高度（三档字号各量一次） ============")
    for (theme, scale), fr in shots.items():
        want_h = pills[scale]
        face = theme_colors(theme)["face"]
        for tag, p, rects in fr:
            nrow = sum(1 for r in rects if r[4].startswith("DataItem"))
            got = pill_in_rows(p, face, rects)
            print(f"\n   {theme}/{scale} → {tag}：期望 pill_h {want_h}px  "
                  f"表格行 {nrow} 条，量到 {len(got)} 颗")
            for (hgt, wid, cx, y0, y1, lbl) in got:
                ok = "✓" if hgt == want_h else "⛔"
                print(f"      {ok} 高{hgt:3d} 宽{wid:4d} 中心 x {cx} y {y0}..{y1}  在 {lbl}")
            if nrow == 0:
                print("      不适用：这一屏没有表格行（详情五页签里不放状态胶囊）")
                continue
            wrong = [g for g in got if g[0] != want_h]
            if not got:
                bad.append(f"{theme}/{scale}/{tag}：有 {nrow} 条表格行却一颗都没量到 "
                           "⇒ 判据没测到东西，不能当通过")
            elif wrong:
                bad.append(f"{theme}/{scale}/{tag}：{len(wrong)} 颗高度不是 {want_h}"
                           f"（实测 {[g[0] for g in got]}）")

    print("\n============ ③ 21-7：头卡内「文字 → 分隔线 → 页签」的气口 ============")
    print("   现行形态：页签收在头卡底部，中间是 `card_split` 那条 1px 线。"
          "代码推得的期望气口 = GAP_TIGHT(4，卡内两 child 的 gap) + GAP_LG(12，线自己的 mt) = 16")
    for (theme, scale), fr in shots.items():
        if scale != "medium":
            continue
        vals = all_hex[theme]
        for tag, p, rects in fr:
            if not tag.startswith("01"):
                print(f"\n   {theme} → {tag}：不适用（总览页没有仓库头卡）")
                continue
            res, why = head_gap(p, vals, rects, theme_colors(theme)["face"])
            print(f"\n   {theme} → {tag}")
            if res is None:
                bad.append(f"{theme}/{tag}：21-7 那条量不出来——{why}")
                print(f"      ⛔ {why}")
                continue
            print(f"      页签「{res['tab']}」上沿 y={res['tab_top']} 矩形高 {res['tab_h']}"
                  f"   分隔线 y={res['line']}   卡内 x {res['x'][0]}..{res['x'][1]}"
                  f"（横贯判据用的宽度 {res['width']}）")
            print(f"      上方最近文字 y={res['above']} → 气口 {res['gap_a']}px"
                  f"（期望 ≤16）   线下方最近文字 y={res['below']} → 气口 {res['gap_b']}px"
                  f"   线到页签矩形上沿 {res['gap_tab']}px")
            if res["gap_a"] is not None and res["gap_a"] > 16 + 4:
                print("      ⚠️ 比期望大 4px 以上：这一档登记数值，不判红（观感归他定）")

    print("\n判读：", "三条都过" if not bad else f"⛔ {len(bad)} 条不成立")
    for b in bad:
        print("   ⛔ " + b)
    print(f"帧在 {WORK} 下")
    return 0 if not bad else 1


if __name__ == "__main__":
    sys.exit(main())
