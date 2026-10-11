"""32-17：每屏**明暗两档各抓一帧**，并把 v4 §8 那"深色档的五条成立条件"**逐条量出来**。

为什么这一条要机器量而不是"看一眼"：阶段 31 出口那条写的是
「反差 ≥12/255、卡间距一致、深色五条成立条件——**三条都是实测，不是目测**」。
而 v4 §8.3 自己就写着"沿用浅色主色会整屏失去主色，**而 check 零错、单测全绿**"——
那种东西只有帧能判。

## 深色那一档怎么进——⚠️ 一个会静默失效的坑

`settings.theme` 存的**不是** `"dark"` / `"light"`，而是**主题名字**：判据是
`s.theme == theme::DARK`（`theme.rs:106`）而 `DARK = "qingbird-dark"`（`:41`）。
写错**不报错、不落深色**，静默按浅色画（`store.rs:88` 那句注释说的就是这一坑）。
⇒ 名字从 `gui/theme.json` 现读，不写死字符串；并且**先验"深色那一跑真的暗"**
（两跑各取 canvas 档比亮度），验不过就当场抛错——否则五条全是在浅色帧上量深色判据，
**每一条都会假绿**。

## 逐屏定位走 a11y 名字，不走像素扫（理由见 `a11y_stress.py` 头注释）

只有设置左栏那四分类例外——它们**有 id 但没有 role**，不进 a11y 树，只能扫像素。

## 八档色从 theme.json 现读，一帧一帧数"完全相等的采样点"

判"应用真按那份画了"要的是**末端有没有出现这一档**。⚠️ 但"某档 0 点"有两种可能：
没画 / 这一屏本来就没有那个东西（没扫过的仓库不会有 ok-wash）。
⇒ 只有 **canvas / card / line / ink 四档**判"每帧都必须出现"（它们是版面本身），
其余四档（sunken / line-soft / brand / wash）**报数不判**，交给五条里的具体那一条。

## 每一格都要有"这一击发生了"的证据（2026-10-11 实锤之后加的）

清单是 13 格：启动即总览 / 我的仓库（概览页签）/ 说明·历史·空间·变更四页签 /
对话框 6 格（空间清理·提交·定时同步·常用命令·添加仓库·克隆）/ 设置四分类 /
第三方仓库 / 关窗询问。⚠️ **两格故意不抓**：`01-我的仓库` 与 `03-页签-概览`
在上一版里是**同一屏**（启动即总览、进"我的仓库"即停在概览页签），留着它们
"每屏一帧"这句话就是假的。另外三格拍不到，原因写在路线表 32-19 里
（批量日志只有 ⌘K 一个入口、变基要分叉夹具、初始化与编辑重命名走右键菜单）。

`grab()` 每格抓之前做三件事：dump 树 → 判"该有『取消』/该没有『取消』" →
判"签名与上一格不能逐字相同"。撞车当场抛，**不等到末端**。

用法：python scripts/verify_dark_five.py            # 抓 + 量（两跑约 10 分钟）
      python scripts/verify_dark_five.py --measure-only   # 只重量判据（帧留在盘上）
"""

import hashlib
import json
import os
import subprocess
import sys
import time

sys.stdout.reconfigure(encoding="utf-8", errors="replace")   # GBK 控制台会编不出 ✓ / ⇒

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import qbguard  # noqa: E402
import steps as _steps  # noqa: E402
import shoot as _shoot  # noqa: E402
import a11y_stress as A  # noqa: E402  ← 沙箱、夹具、dump、名字定位、像素扫全部复用它
from PIL import Image  # noqa: E402

WORK = os.path.join(os.environ["TEMP"], "qb_dark")
TABS = A.TABS
# v4 那八档 → theme.json 里的点号键（**色值不写死**，从文件现读）
TIERS = [
    ("canvas", "background"),
    ("card", "secondary.background"),
    ("sunken", "muted.background"),
    ("line", "border"),
    ("line-soft", "table.row.border"),
    ("brand", "primary.background"),
    ("ink", "foreground"),
    ("wash", "secondary.active.background"),
]
# 每一帧都必须出现的档：版面本身，缺了就是"这一屏没按那份画"
MUST_APPEAR = ("canvas", "card", "line", "ink")

# ⛔ 脚本**拍不到**的格，连同原因写在这里，末端一起打印。
# 为什么要有这个常量：没有它，"每屏两档各一帧"这句话会悄悄变成"抓了 N 帧"——
# 而 N 帧里根本没有那几屏（与 AGENTS「留着那一步就是给'走查过了'盖一个没测的章」同一条）。
NOT_COVERED = [
    ("命令面板（⌘K）", "它是第 12 个 `open_dialog` 调用点（`palette.rs:116`），而入口只有 ⌘K 一条路，"
     "`steps.py` 送不进按键（AGENTS 记着的第 3、4 次确认）⇒ 只能他双击验"),
    ("批量日志对话框", "同样只有 ⌘K 一条入口（`batch_log.rs:52`），原因同上"),
    ("同一颗仓库 Dialog 的另外三档：编辑 / 重命名 / 初始化",
     "`repo.rs` 一颗 `open_dialog`（`:333`）服务四种模式（`open_add_dlg` / `open_edit_dlg` / "
     "`open_rename_dlg` / `init_body`），本轮只拍了 add 那一档；编辑与重命名的入口是侧栏那一行"
     "的右键，而**那一行不进 a11y 树**（实测：树里只有 TreeItem「我的仓库 · 1」，没有仓库行）"),
    ("变基那一屏", "⚠️ 32-19 登记原文把它列进「各对话框」，**查实它不是对话框**：`fork.rs` 里只有 "
     "`fork_commit_block`，那颗键挂在详情页的分叉卡上（`panels.rs:18`），要仓库处于分叉态才画 "
     "⇒ 归「未覆盖的屏」，32-6 那轮造过分叉夹具、本轮没搬"),
    ("stash 清单对话框", "那颗三角同样不进 a11y 树；32-7 那轮拍过浅色一版，**深色没拍**"),
]


def hexes():
    """{主题名: {档: (r,g,b)}}，两套"""
    d = json.load(open(os.path.join(A.REPO, "gui", "theme.json"), encoding="utf-8-sig"))
    out = {}
    for t in d["themes"]:
        vals = {}
        for tier, key in TIERS:
            raw = t["colors"].get(key)
            if isinstance(raw, str) and raw.startswith("#") and len(raw) == 7:
                vals[tier] = (int(raw[1:3], 16), int(raw[3:5], 16), int(raw[5:7], 16))
        out[t["name"]] = vals
    return out


def lum(c):
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]


def delta(a, b):
    return max(abs(a[0] - b[0]), abs(a[1] - b[1]), abs(a[2] - b[2]))


def arr(png):
    return np.asarray(Image.open(png).convert("RGB"), dtype=np.int16)


def mask_of(a, rgb):
    return (a == np.array(rgb, dtype=np.int16)).all(axis=2)


def count_tiers(png, want):
    """一帧里每档**完全相等的像素数**（numpy 向量化，隔点采样会漏掉细描边那种 1px 档）"""
    a = arr(png)
    hits = {k: int(mask_of(a, v).sum()) for k, v in want.items()}
    return hits, (a.shape[1], a.shape[0])


def edge_between(png, card, canvas, soft):
    """条②的测量：**卡与画布之间那一道是不是 `line-soft`**。

    ⚠️ 第一版写成"卡外 1..4px 应等于 canvas（有阴影才会更暗）"，实测只有 0.10~0.42，
    看着像"深色档真画了阴影"——**那是判据被我编错了两处**：
    ① v4 §8.2 的原话是"深色下阴影只做近身层次，**卡与画布的分界由 `line_soft` 承担**"，
       它要的是**描边在场**，不是"外面那一圈干净"；
    ② 取"card 段两端外侧"时，段边界自己那一像素也是 card 档（k=0 全废），
       而两张相邻卡（侧栏与内容区）会把对方的卡面当成"外侧"。
    ⇒ 换成一个**没有歧义的形式**：同一行里找 `card → line-soft → canvas`（或反向）
       这样紧挨着的三像素。它出现一次，就说明"卡与画布之间那一道确实是 line-soft"。
    返回：这种三元组出现的次数。
    """
    a = arr(png)
    mc, ms, mv = mask_of(a, card), mask_of(a, soft), mask_of(a, canvas)

    def trip(x, y, z):
        return int((x[:, :-2] & y[:, 1:-1] & z[:, 2:]).sum())

    return trip(mc, ms, mv) + trip(mv, ms, mc)


def wash_has_ink(png, wash, ink, pad=2, row_step=2):
    """条④的测量：**落在 wash 底上的字是不是 ink 那一档**。

    v4 那条写的是"深底 + 亮字"，光看 wash 自己的亮度判不了（深色档 wash 本来就比
    card 亮一档，那是"选中面要看得见"，不是白斑）。真正要判的是**紧挨着 wash 底的
    文字是 ink**（浅色档的 ink 是接近黑的深墨，沿用就会糊在深底上）。
    返回（有这种字的行数，字数）。
    """
    a = arr(png)
    w = mask_of(a, wash)
    i = mask_of(a, ink)
    near = np.zeros_like(w)
    for k in range(1, pad + 1):
        near[:, k:] |= w[:, :-k]
        near[:, :-k] |= w[:, k:]
    hit = i & near
    rows = int((hit.any(axis=1)).sum())
    return rows, int(hit.sum())


def shot_dir(theme_name):
    return os.path.join(WORK, "frames-" + ("dark" if "dark" in theme_name else "light"))


def walk_shots(theme_name, vals):
    """一跑 = 一个主题：逐屏抓帧，返回 [(屏名, 帧路径, 那一刻的树签名)]"""
    A.build_sandbox(False, theme=theme_name)
    # 夹具要补两样，否则有两格拍不到（都是"那颗键只在有内容时才画"）：
    # ① `target/` —— 空间页的「一键清理」挂在"构建缓存"那一段的表头，而整段
    #    只在 `caches` 非空时才出现（`space_tab.rs:168-187`）。没有它，那一屏只有
    #    「重新扫描磁盘占用」与「收起全部」两颗（帧上实测，报出来的全名单里没有清理）。
    # ② 一颗未提交的文件 —— 「提交并推送」对话框要工作区有变更才开得出内容。
    # 都在 %TEMP 下的合成仓库里，一个真实仓库都不碰。
    cache = os.path.join(A.FIXTURE, "target")
    os.makedirs(cache, exist_ok=True)
    for i in range(3):
        with open(os.path.join(cache, "blob%d.bin" % i), "wb") as fh:
            fh.write(b"\0" * (1 << 20))
    with open(os.path.join(A.FIXTURE, "dirty.txt"), "w", encoding="utf-8") as fh:
        fh.write("未提交的一行\n")
    subprocess.run(["taskkill", "/F", "/IM", "qingbird-git.exe"], capture_output=True)
    time.sleep(0.8)
    env = qbguard.launch_env()
    assert env["APPDATA"] == A.SANDBOX, f"沙箱没生效：{env['APPDATA']}"
    proc = subprocess.Popen([A.EXE], cwd=A.REPO, env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    d = shot_dir(theme_name)
    # 上一版的帧**必须清掉**：留着它们，"这一跑有几帧"就变成"目录里有几个文件"，
    # 而清单换过之后那两类屏根本不再对应任何东西（32-10 那条"产物是新的、夹具是旧的"同族）。
    if os.path.isdir(d):
        A.clean(d)
    os.makedirs(d, exist_ok=True)
    frames = []
    missing = []      # 这一跑**没拍到的格**，末端如实报，别让它变成"几帧 = 几屏"
    try:
        hwnd = A.find_window(proc.pid)
        assert hwnd, "找不到窗口"
        assert _steps.ensure_focus(hwnd), "拿不到前台"
        clx, cly = A.client_origin(hwnd)
        offx, offy = clx - A.window_rect(hwnd)[0], cly - A.window_rect(hwnd)[1]

        prev = [None, None]      # [上一格的屏名, 上一格的树签名]

        def grab(name, png=None, dlg=None):
            """抓一帧，并**拿树签名当这一格的生效判据**。

            ⚠️ 为什么必须有（2026-10-11 实锤）：上一版那 15 帧里有**三对逐字节相同**
            （`00-启动`==`01-总览`、`02-我的仓库`==`03-页签-概览`、
            `05-添加仓库`==`06-克隆`），也就是"每屏一帧"实际只覆盖 12 个屏，
            而脚本里没有任何一条判据负责"这一击换屏了没有"。这是 D93
            「`alive` 不是做到了」的**抓帧版**：抓一帧只证明"拍到了一张画"，
            **证不了"拍的是那一屏"**。前两对是清单本身写重了（启动即在总览页、
            进"我的仓库"即停在概览页签），第三对才是那一步点击没生效。
            `dlg`：True = 这一刻必须有对话框开着；False = 必须没有。
            『取消』那颗键是对话框的通用标志——主界面 30 个节点里没有这个名字（实测）。
            """
            nodes = A.dump(hwnd)
            sig = A.tree_sig(nodes)
            has_dlg = "取消" in {n.name for n in nodes}
            if dlg is True and not has_dlg:
                raise AssertionError(
                    f"抓「{name}」：树里没有『取消』⇒ 对话框没开着。这一屏按钮："
                    f"{sorted(n.name for n in nodes if n.kind == 'Button')}")
            if dlg is False and has_dlg:
                raise AssertionError(
                    f"抓「{name}」：树里还有『取消』⇒ 上一个对话框没关掉，"
                    f"这一格拍到的是它盖在下一屏上")
            if prev[1] == sig:
                raise AssertionError(
                    f"抓「{name}」：树签名与上一格「{prev[0]}」逐字相同 ⇒ 中间那一步没换屏。"
                    f"这一屏按钮：{sorted(n.name for n in nodes if n.kind == 'Button')}")
            prev[0], prev[1] = name, sig
            p = png or os.path.join(d, name + ".png")
            _shoot.capture(hwnd, p)
            frames.append((name, p, sig))

        def node(name, kind=None):
            return A.one(A.dump(hwnd), name, kind)

        def tap(n, settle=1.5):
            _steps.click(hwnd, n.cx - clx, n.cy - cly, settle=settle)

        def tap_any(*cands, settle=1.5):
            """按候选名点**第一个在树里出现的**；一个都没有就把这一屏按钮全打出来。

            为什么不写死一个名字：这些入口的名字我没逐个验过，写死了就是
            "找不到 ⇒ 整跑抛在中间 ⇒ 前面那些格的帧白抓"。报出全名单一次就能补准。
            """
            ns = A.dump(hwnd)
            for c in cands:
                hit = [n for n in ns if n.name == c and n.kind == "Button"]
                if len(hit) == 1:
                    tap(hit[0], settle)
                    return c
            raise AssertionError(
                f"候选入口 {list(cands)} 一个都不在树里；这一屏的按钮："
                f"{sorted(n.name for n in ns if n.kind == 'Button')}")

        _steps.click(hwnd, 700 - offx, 300 - offy, settle=1.0)   # 空点激活
        assert _steps.ensure_focus(hwnd)
        grab("00-启动即总览", dlg=False)
        tap(node("我的仓库")); grab("01-我的仓库-概览页签", dlg=False)
        for tab in TABS[1:]:                      # 概览就是 01，别再抓一遍重复屏
            tap(node(tab, "TabItem")); grab("02-页签-" + tab, dlg=False)

        # 空间页那颗清理键要趁还在这一页点掉（关掉对话框会退回原页签）。
        # ⚠️ 先点一次「重新扫描磁盘占用」：夹具里的 `target/` 是这一跑现造的，
        # 缓存库里那份还没有它——不重扫就等于拿旧数据判"没有可清理项"（前提没造出来）。
        tap(node("空间", "TabItem"), 1.2)
        tap_any("重新扫描磁盘占用")
        time.sleep(3.5)
        tap(node("空间", "TabItem"), 1.0)
        tap_any("一键清理", "清理构建产物", "清理构建缓存")
        grab("03-对话框-空间清理", dlg=True)
        tap(node("取消"), 1.2)

        tap(node("概览", "TabItem"), 1.2)     # 清理对话框关掉后还停在空间页签，那排卡不在那一页
        tap(node("提交并推送")); grab("04-对话框-提交", dlg=True)
        tap(node("取消"), 1.2)
        tap_any("定时同步设置", "同步设置"); grab("05-对话框-定时同步", dlg=True)
        tap(node("取消"), 1.2)
        tap_any("常用命令", "命令设置"); grab("06-对话框-常用命令", dlg=True)
        tap(node("取消"), 1.2)
        tap(node("添加仓库")); grab("07-对话框-添加仓库", dlg=True)
        tap(node("取消"), 1.2)
        tap(node("克隆仓库")); grab("08-对话框-克隆", dlg=True)
        tap(node("取消"), 1.2)

        # 设置四分类：左栏那四类不进 a11y 树（有 id 没 role）⇒ 只能像素扫。
        # 左栏位置由 wash 块量（两档同一份判据），"对话框还在"由『保存』那颗键判
        tap(node("设置"))
        f = os.path.join(d, "09-设置-1.png")
        _shoot.capture(hwnd, f)

        def dialog_open(when):
            if "保存" not in {n.name for n in A.dump(hwnd) if n.kind == "Button"}:
                raise AssertionError(f"{when}：设置对话框已经不在（树里没有『保存』）")

        dialog_open("刚点开设置")
        wx0, wx1, _ = A.wash_box(f, vals["wash"])
        titles = [y for y, hh in A.dark_bands(f, wx0, wx1, 120, 700, face=vals["card"])
                  if hh >= 16]
        assert len(titles) >= 3, f"设置左栏只扫到 {len(titles)} 个类别标题：{titles}"
        print(f"  [{theme_name}] 左栏 x={wx0}..{wx1}，类别标题：{titles}")
        for i, ty in enumerate(titles):
            _steps.click(hwnd, (wx0 + wx1) // 2 - offx, ty - offy, settle=1.5)
            p = os.path.join(d, "09-设置-%d.png" % (i + 1))
            dialog_open(f"点第 {i + 1} 类之后")
            grab("10-设置分类#" + str(i + 1), png=p, dlg=True)
        tap(node("取消"), 1.2)

        tap(node("第三方仓库")); grab("11-第三方仓库", dlg=False)

        # 关窗询问面板：标题栏那三颗系统键是自绘的、不进 a11y 树 ⇒ 按窗口右缘算 x，
        # y 借同一行的主题键（它两档都在，一句"切到深色主题"一句"切到浅色主题"
        # ⇒ **按后缀认，别写死整句**，深色那一跑写死了就命中 0 颗）。
        # **放在最后一格**：万一它没弹面板而直接把窗口关了，前面那些帧都已经拍完了。
        # ⚠️ 包在 try 里：这一格拍不到**不该毁掉整跑**——一抛，main 就拿不到 frames，
        # 前面 25 帧白抓。拍不到就记进 `missing`，末端与 `NOT_COVERED` 一起如实报。
        try:
            wr = A.window_rect(hwnd)
            th = next(n for n in A.dump(hwnd)
                      if n.kind == "Button" and n.name.endswith("主题"))
            _steps.click(hwnd, wr[2] - 16 - clx, th.cy - cly, settle=1.4)
            grab("12-关窗询问")
        except Exception as e:
            missing.append(("12-关窗询问", f"{type(e).__name__}: {e}"))
            print(f"  ⚠️ 12-关窗询问 这一格没拍到：{e}")
    finally:
        proc.kill()
        time.sleep(0.5)
    return frames, missing


def dup_check(shots):
    """**每跑内部两两查重**：同一跑里任何两帧不许逐字节相同。

    撞车意味着"这两格里至少有一格没拍到它声称的那个屏"——抓帧脚本把两个屏
    报成一屏，比少抓一屏更坏：它会让人以为那一屏已经看过了（D93 同族）。
    返回硬撞车的描述列表。树签名相同而像素不同**只报不判**（同一屏的两个状态，
    例如页签切回来又动了一根柱），要逐条看一眼再决定。
    """
    bad, same_sig = [], []
    for n, fr in shots.items():
        by_hash, by_sig = {}, {}
        for name, p, sig in fr:
            h = hashlib.sha256(open(p, "rb").read()).hexdigest()[:16]
            if h in by_hash:
                bad.append(f"{n}：两帧逐字节相同（{h}）：{by_hash[h]} == {name}")
            else:
                by_hash[h] = name
            if sig:
                if sig in by_sig:
                    same_sig.append(f"{n}：树签名相同而像素不同：{by_sig[sig]} / {name}")
                else:
                    by_sig[sig] = name
    if same_sig:
        print("\n⚠️ 树签名相同（同一屏的两个状态，不算撞车，逐条看一眼）：")
        for s in same_sig:
            print("   " + s)
    return bad


def main():
    all_hex = hexes()
    dk = [n for n in all_hex if "dark" in n]
    lt = [n for n in all_hex if "light" in n]
    assert len(dk) == 1 and len(lt) == 1, f"theme.json 里明暗两套没各找到一个：{list(all_hex)}"
    os.makedirs(WORK, exist_ok=True)
    before = qbguard.real_cfg_fp()
    shots = {}
    miss_all = []
    if "--measure-only" in sys.argv:
        # 帧已经在盘上（抓一次要三分多钟），只重算判据 ⇒ 判据本身可以反复改而不重跑应用
        shots = {n: [(os.path.basename(p)[:-4], os.path.join(shot_dir(n), p), None)
                     for p in sorted(os.listdir(shot_dir(n))) if p.endswith(".png")]
                 for n in (lt[0], dk[0])}
        print(f"只测量已有帧：{lt[0]} {len(shots[lt[0]])} 帧、{dk[0]} {len(shots[dk[0]])} 帧")
    else:
        try:
            for n in (lt[0], dk[0]):
                print(f"\n== 抓帧：{n} ==")
                shots[n], missing = walk_shots(n, all_hex[n])
                miss_all += [(n, a, b) for a, b in missing]
        finally:
            qbguard.check_real_cfg(before)

    print("\n================ 每帧八档采样点 ================")
    rows = {}
    for n, fr in shots.items():
        vals = all_hex[n]
        print(f"\n-- {n}（{len(fr)} 帧）--")
        rows[n] = []
        for name, p, sig in fr:
            hits, size = count_tiers(p, vals)
            miss = [k for k in MUST_APPEAR if hits[k] == 0]
            rows[n].append((name, hits, p))
            # 对话框那一屏"没有 card 档"是**结构性的**，不是漏画：官方 Dialog 的面板底
            # 写死读 `tokens.background`（`dialog.rs:669`）= v4 的 **canvas 档**，而 v4 §2
            # 从没给"浮层面板"定过档。⇒ 这一档缺席单独说明，不混进"缺档"（已登记 32-21）。
            is_dlg = bool(sig) and "Button:取消" in sig
            if is_dlg and miss == ["card"]:
                tail = "   （对话框格：面板底走 canvas 档，card 缺席是结构性的 ⇒ 32-21）"
            else:
                tail = "   <<< 缺档：" + "、".join(miss) if miss else ""
            print(f"  {name:22s} {size[0]}x{size[1]}  " +
                  "  ".join(f"{k}={hits[k]}" for k, _ in TIERS) + tail)
            if sig:
                print(f"  {'':22s} 树：节点 {len(sig)}、按钮 "
                      f"{sum(1 for s in sig if s.startswith('Button:'))}")

    # 前提：深色那一跑**真的暗**（不成立的话五条全是假绿）
    dkv, ltv = all_hex[dk[0]], all_hex[lt[0]]
    assert lum(dkv["canvas"]) < lum(ltv["canvas"]), "深色那跑的 canvas 不比浅色暗 ⇒ 主题没切上"
    print(f"\n前提成立：canvas 亮度 浅色 {lum(ltv['canvas']):.0f} > 深色 {lum(dkv['canvas']):.0f}")

    dups = dup_check(shots)   # 抓帧自己的判据，先于那五条报

    print("\n================ v4 §8 深色五条（逐条实测）================")
    fails = []

    # ⓪ **抓帧自己的前提**：15 帧得是 15 个不同的屏。撞车的话后面每一条都可能是
    # 在拿"少一格"当"少一格"读，而登记里那句"每屏两档"就成了假话。
    def verdict(label, ok, why):
        print(f"  {label:22s} {'PASS' if ok else 'FAIL'}   {why}")
        if not ok:
            fails.append(label)

    verdict("⓪ 每帧是不同的一屏", not dups,
            f"两跑共 {sum(len(v) for v in shots.values())} 帧，逐字节撞车 "
            f"{len(dups)} 对" + ("：" + "；".join(dups) if dups else ""))

    # ① 反差 ≥ 12/255。⚠️ "两档都出现"只统计**该屏本来就该有卡**的那些帧——
    # 第一版把"某帧 card=0"也算进这条的失败里，混成了"反差不够"。
    # 而 `card=0` 本身是一条**独立的发现**（见下面单独打印那一行），要查的是
    # "对话框那一屏的底色是哪一档"，不是反差。
    d = delta(dkv["card"], dkv["canvas"])
    nocard = [r[0] for r in rows[dk[0]] if r[1]["card"] == 0]
    has = [r for r in rows[dk[0]] if r[1]["card"] > 0 and r[1]["canvas"] > 0]
    verdict("① 反差 ≥12/255", d >= 12 and len(has) >= 10,
            f"深色 card {dkv['card']} − canvas {dkv['canvas']} = 最大通道差 {d}/255；"
            f"两档同时出现的帧 {len(has)}/{len(rows[dk[0]])}"
            f"（浅色同算：{delta(ltv['card'], ltv['canvas'])}/255）")
    if nocard:
        print(f"       ⚠️ 这几帧上 card 档一个像素都没有：{nocard}"
              f" ⇒ 不是反差不够：官方 Dialog 的面板底写死读 `tokens.background`"
              f"（`dialog.rs:669`）= v4 的 canvas 档，而 v4 §2 从没给浮层面板定过档"
              f" ⇒ 已查因、登记为 32-21")

    # ② 卡与画布之间那一道是 `line-soft`（v4 §8.2 那句"分界由 line_soft 承担"的形式）
    trips = [(name, edge_between(p, dkv["card"], dkv["canvas"], dkv["line-soft"]))
             for name, _, p in rows[dk[0]]]
    dk_total = sum(t for _, t in trips)
    lt_total = sum(edge_between(p, ltv["card"], ltv["canvas"], ltv["line-soft"])
                   for _, _, p in rows[lt[0]])
    verdict("② 分界靠 line-soft", dk_total > 0,
            f"深色帧上 card→line-soft→canvas 相邻三元组共 {dk_total} 处，"
            f"{sum(1 for _, t in trips if t)}/{len(trips)} 帧里有；浅色同算 {lt_total} 处"
            "（逐帧明细：" + "、".join(f"{n.split('-')[-1]}={t}" for n, t in trips) + "）")

    # ③ 品牌色必须提亮（旧主色不许在深色帧上出现）
    brand_pts = sum(r[1]["brand"] for r in rows[dk[0]])
    stale = 0
    for _, _, p in rows[dk[0]]:
        stale += count_tiers(p, {"old": ltv["brand"]})[0]["old"]
    verdict("③ 品牌色提亮", brand_pts > 0 and stale == 0,
            f"深色帧上 brand {dkv['brand']} 共 {brand_pts} 个像素；"
            f"浅色主色 {ltv['brand']} 在深色帧上 {stale} 个（要 0）")

    # ④ 语义 wash 是"深底 + 亮字"
    rows_with_ink = letters = 0
    for _, _, p in rows[dk[0]]:
        a, b = wash_has_ink(p, dkv["wash"], dkv["ink"])
        rows_with_ink += a
        letters += b
    verdict("④ wash 深底 + 亮字", rows_with_ink > 0 and letters > 0,
            f"深色帧里紧挨着 wash {dkv['wash']} 底的 ink {dkv['ink']} 文字："
            f"{rows_with_ink} 行、{letters} 个像素（浅色 ink 亮度 {lum(ltv['ink']):.0f} "
            f"若沿用会糊在深底上：深色 wash 亮度 {lum(dkv['wash']):.0f}）")

    # ⑤ 文字变亮不是变暗
    ink_pts = sum(r[1]["ink"] for r in rows[dk[0]])
    verdict("⑤ 文字比底亮", lum(dkv["ink"]) > lum(dkv["canvas"]) and ink_pts > 0,
            f"ink 亮度 {lum(dkv['ink']):.0f} > canvas 亮度 {lum(dkv['canvas']):.0f}；"
            f"帧上 ink 采样 {ink_pts} 点")

    print("\n================ 覆盖边界（这一轮没拍到的屏）================")
    for name, why in NOT_COVERED:
        print(f"  ⛔ {name}：{why}")
    for theme, name, why in miss_all:
        print(f"  ⚠️ {theme} 这一跑没拍到「{name}」：{why}")

    print("\n判读：", "⓪+五条全过" if not fails else f"没过：{fails}")
    print(f"帧都在 {WORK} 下，两档各 {len(rows[lt[0]])} 帧")
    return 0 if not fails else 1


if __name__ == "__main__":
    sys.exit(main())
