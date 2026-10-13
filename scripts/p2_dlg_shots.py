"""P2 合并项（`dialogs::footer` / `dialogs::shell` / `widgets::ghost_btn`）的**对话框抓帧**。

## 为什么要有这个脚本

台账 §3.3 的原话：合并项都在**实机看得见的地方**（底栏、按钮、wash 底），
改完要抓帧复验。`a11y_stress.py` 的走查只覆盖**设置**一个对话框
（`dlg-open.png`），而 P2-1/P2-2 动了 **12 个对话框的底栏 + 10 处开壳**。
这个脚本把剩下 10 个开得起来的对话框逐个抓一帧，改前改后各跑一次逐帧 diff。

## 覆盖矩阵（10 个抓得到 + 2 个如实记录抓不到）

| 对话框 | 入口（a11y 名字） | 主键（"真的开了"的判据） |
| --- | --- | --- |
| 提交 | 变更页「提交并推送…」 | 提交并推送 |
| 常用命令 | 仓库头「常用命令」 | 保存 |
| 定时同步 | 同步卡「定时同步设置」 | 保存 |
| 收起来的改动 | 仓库头感叹号（气泡名前 60 字） | 关闭 |
| 清理 | 空间页「一键清理」 | 删除这 N 个目录 |
| 设置 | 导航轨「设置」 | 保存 |
| 克隆 | 侧栏「克隆仓库」 | 开始克隆 |
| 添加仓库 | 侧栏「添加仓库」 | 添加 |
| 清除 | 第三方变更页「清掉多出来的文件…」 | 按勾选清理 |
| 关窗询问 | `PostMessageW(WM_CLOSE)` | 完全退出 |

**抓不到的两个，理由是量出来的、不是懒得抓**：
- **上次批量结果**：唯一入口是 ⌘K（`palette.rs:265`），而 `steps.py` 文件头
  记着实测结论——`keybd_event` 与 `PostMessageW` 都送不进按键。它的等价性
  走代码级论证（底栏左槽复制 + 右排单键，合并前后逐字相同）。
- **初始化向导**：要先在「添加仓库」的路径输入框里打字（InputState 没有
  预置内容的路），同样送不进按键。同理走代码级论证。

## 纪律（与 a11y_stress / shot_wb 同一套）

- 沙箱 `QB_APPDATA` + 启动后 assert + 收尾核对真实 config 指纹（qbguard）；
- 控件一律 **a11y 名字定位**（`dump` + 全等/前缀匹配），坐标不写死；
- 每一步都要有"它生效了"的证据：主键出现在树里才算对话框开了；
- **抓帧前把光标移出窗口矩形**（shot_wb ②：悬停高亮与气泡会污染 diff）；
- 不点明暗主题键（底色漂了整批帧对不上）。

用法：
  python scripts/p2_dlg_shots.py before     # 抓一套到 target/p2_frames_before/
  python scripts/p2_dlg_shots.py after      # 抓一套到 target/p2_frames_after/
  python scripts/p2_dlg_shots.py diff       # 两套逐帧对比（阈值同 frame.py）

## 判读口径（2026-10-13 实测，P2 出口时踩的坑，比脚本本身更重要）

**同一份代码的两次运行，主区可能整体水平漂 1px**——左栏/检视列的宽度由弹簧动画
驱动（`shell.rs` `spring("inspector-width-…")`，ζ=0.85 有 0.63% 过冲），两次运行
停的相位不同 ⇒ 分割线在 x=1360/1361 之间随机游走。它产生的差异量级：

- 只检视列漂 1px ≈ **43k px**；只侧栏漂 ≈ **82k px**；两者都漂 ≈ **127k px**。
- 差异形态：主区文字**字形逐像素互换**（相邻像素 RGB 互换 = 平移 1px 签名），
  **对话框区域完全不受影响**（对话框按窗口居中，不随列原点动）。
- 平移一帧再 diff 只会更差（±1px 平移差 5-8 倍）⇒ 是光栅相位不是布局偏移。
- **同一二进制相隔几分钟也能跨相位**（实测 md5 相同的 exe 相隔 45 分钟差 83k），
  所以「同一 exe 抓两套当噪声底」**不保证**同相位；相邻构建的同相位对
  （expC/expE 隔 8 分钟）也可能跨相位（127k）。

⇒ **正确的判读法**：先量两套帧的**分割线 x**（`dlg-add.png` 中线 y 上
x∈[1200,1500] 最暗像素），**只有两套同相位，diff 才可归因于代码**；
不同相位就重抓到同相位为止。同相位下，真正的代码差异是**逐帧局部、可点名**
的（本仓库 P2 出口的实测：10 帧里 7 帧逐字节相同，剩 3 帧各有明确出处——
动态数字两帧、有意修复一帧），而漂移是**全帧一致的整体位移**。
探针脚本 `probe_window_geometry.py` 留档：窗口/客户区几何逐运行恒定
（(752,202) 1919×820），漂移**不在几何层**，在 app 内部分数布局的舍入里。
"""

import ctypes
import ctypes.wintypes as wt
import json
import os
import shutil
import subprocess
import sys
import time

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import qbguard  # noqa: E402
import steps as _steps  # noqa: E402
import shoot as _shoot  # noqa: E402
from PIL import Image, ImageChops  # noqa: E402

# a11y_stress 那套 dump / Node 直接复用（同一条 PowerShell UIA 通道）
import a11y_stress as _a11y  # noqa: E402

dump = _a11y.dump
Node = _a11y.Node

REPO = r"F:\AIwork\qingbird-git"
EXE = os.path.join(REPO, "target", "debug", "qingbird-git.exe")
TMP = os.environ["TEMP"]
WORK = os.path.join(TMP, "qb_p2dlg")
SANDBOX = os.path.join(WORK, "appdata")
CFG = os.path.join(SANDBOX, "com.qingbird.git", "config.json")
FIXTURE = os.path.join(WORK, "repo")        # 我的仓库：dirty + 1 份 stash + 构建缓存
FIXTURE2 = os.path.join(WORK, "repo2")      # 第三方仓库：dirty
ERR = os.path.join(TMP, "qb_p2dlg_err.txt")
OUT = os.path.join(REPO, "target", "p2_frames_" + (sys.argv[1] if len(sys.argv) > 1 else "run"))

WM_CLOSE = 0x0010
user32 = ctypes.windll.user32


def unlock(func, path, exc):
    os.chmod(path, 0o777)
    func(path)


def clean(path):
    if os.path.isdir(path):
        shutil.rmtree(path, onerror=unlock)
    assert not os.path.exists(path), f"没清干净：{path}"


def build_fixture():
    """两颗夹具。主夹具刻意造齐三个前置：未提交改动、一份 stash、一个构建缓存目录。"""
    clean(WORK)
    subprocess.run(["git", "init", "-q", "--initial-branch=main", FIXTURE], check=True)
    for k, v in (("user.email", "t@t"), ("user.name", "t"), ("core.autocrlf", "false")):
        subprocess.run(["git", "-C", FIXTURE, "config", k, v], check=True)
    open(os.path.join(FIXTURE, "base.txt"), "w", encoding="utf-8").write("base\n")
    subprocess.run(["git", "-C", FIXTURE, "add", "-A"], check=True)
    subprocess.run(["git", "-C", FIXTURE, "commit", "-q", "-m", "base"], check=True)
    # ① 一份 stash（「收起来的改动」入口要 stash_count > 0）
    open(os.path.join(FIXTURE, "base.txt"), "a", encoding="utf-8").write("stash me\n")
    subprocess.run(["git", "-C", FIXTURE, "stash", "-q"], check=True)
    # ② 未提交改动（提交对话框要 changes 非空）
    open(os.path.join(FIXTURE, "base.txt"), "a", encoding="utf-8").write("dirty\n")
    open(os.path.join(FIXTURE, "new.txt"), "w", encoding="utf-8").write("untracked\n")
    # ③ 构建缓存（「一键清理」要有条目）
    os.makedirs(os.path.join(FIXTURE, "dist", "assets"), exist_ok=True)
    open(os.path.join(FIXTURE, "dist", "bundle.js"), "w").write("x" * 2048)
    open(os.path.join(FIXTURE, "dist", "assets", "app.css"), "w").write("y" * 1024)
    # 第三方夹具：只要 dirty
    subprocess.run(["git", "init", "-q", "--initial-branch=main", FIXTURE2], check=True)
    for k, v in (("user.email", "t@t"), ("user.name", "t"), ("core.autocrlf", "false")):
        subprocess.run(["git", "-C", FIXTURE2, "config", k, v], check=True)
    open(os.path.join(FIXTURE2, "a.txt"), "w", encoding="utf-8").write("a\n")
    subprocess.run(["git", "-C", FIXTURE2, "add", "-A"], check=True)
    subprocess.run(["git", "-C", FIXTURE2, "commit", "-q", "-m", "base"], check=True)
    open(os.path.join(FIXTURE2, "a.txt"), "a", encoding="utf-8").write("dirty\n")


def entry(path, third=False):
    return {
        "path": path.replace("\\", "/"), "name": os.path.basename(path), "alias": "",
        "added_at": 1791600000, "last_used": 1791600000,
        "test_cmd": "cargo test", "build_cmd": "cargo build", "dev_cmd": "",
        "install_cmd": "", "pinned_cmds": [], "cmd_runs": {},
        "auto_sync_mins": 0, "daily_time": "", "auto_pull": False,
        "last_auto_check": 0, "last_auto_result": "", "is_third_party": third,
    }


def build_sandbox():
    """settings 抄真实那份（主题/字号影响版面），repos 换两颗夹具，关窗档显式 Ask。"""
    real = json.load(open(os.path.join(os.environ["APPDATA"], "com.qingbird.git",
                                       "config.json"), encoding="utf-8-sig"))
    clean(SANDBOX)
    os.makedirs(os.path.dirname(CFG))
    real["repos"] = [entry(FIXTURE), entry(FIXTURE2, third=True)]
    real["selected_path"] = FIXTURE.replace("\\", "/")
    real.setdefault("settings", {})["inspector_open"] = True
    real["settings"]["sidebar_collapsed"] = False
    # ⚠️ **两个 hidden_modules 显式清空**：抄真实 settings 时会把用户勾掉的
    # 页签一起抄过来——他若把「变更」勾掉了，第三方仓库连变更页签都没有，
    # 「清掉多出来的文件…」（purge）这颗键就不在树里，那一帧永远抓不到。
    # 基线只藏空间+分叉（`THIRD_BASELINE_HIDDEN`），清空用户那份才是"默认全套"。
    real["settings"]["hidden_modules_mine"] = []
    real["settings"]["hidden_modules_third"] = []
    # 关窗询问那一档：不显式钉 Ask 就会沿用用户真实设置（他若是 Tray 档，
    # 点 X 直接关窗，那一帧永远抓不到）——ask 是默认档，钉它不算改行为
    real["settings"]["close_action"] = "ask"
    json.dump(real, open(CFG, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
    os.environ["QB_APPDATA"] = SANDBOX


# ───────────────────────── 定位 helpers ─────────────────────────

def btn(nodes, name):
    """全等取一颗 Button；不是一颗就抛（a11y_stress::one 的同口径）。"""
    hit = [n for n in nodes if n.kind == "Button" and n.name == name]
    if len(hit) != 1:
        raise AssertionError(f"按钮「{name}」命中 {len(hit)} 颗（要 1 颗）：{hit}")
    return hit[0]


def btn_pre(nodes, prefix, contains=None):
    """前缀 + 包含匹配——给 warn_dot 那颗用：它的 a11y 名字是整句气泡，
    dump 时会按 60 字截断，全等匹配永远命不中。"""
    hit = [n for n in nodes if n.kind == "Button" and n.name.startswith(prefix)
           and (contains is None or contains in n.name)]
    if len(hit) != 1:
        raise AssertionError(
            f"按钮（前缀「{prefix}」含「{contains}」）命中 {len(hit)} 颗：{hit}")
    return hit[0]


def tab(nodes, name):
    hit = [n for n in nodes if n.kind == "TabItem" and n.name == name]
    if len(hit) != 1:
        raise AssertionError(f"页签「{name}」命中 {len(hit)} 颗（要 1 颗）：{hit}")
    return hit[0]


def has_btn(nodes, name):
    return any(n.kind == "Button" and n.name == name for n in nodes)


def client_origin(hwnd):
    pt = wt.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    return pt.x, pt.y


def window_rect(hwnd):
    r = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    return r.left, r.top, r.right, r.bottom


# ───────────────────────── 走查 ─────────────────────────

def run(tag, before):
    build_sandbox()
    subprocess.run(["taskkill", "/F", "/IM", "qingbird-git.exe"], capture_output=True)
    time.sleep(0.8)
    if os.path.exists(ERR):
        os.remove(ERR)
    env = qbguard.launch_env()
    assert env["APPDATA"] == SANDBOX, f"沙箱没生效：{env['APPDATA']}"
    env["RUST_BACKTRACE"] = "1"
    err = open(ERR, "wb")
    proc = subprocess.Popen([EXE], cwd=REPO, env=env,
                            stdout=subprocess.DEVNULL, stderr=err)
    os.makedirs(OUT, exist_ok=True)
    log = []

    def note(label, ok, extra=""):
        log.append((label, ok, extra))
        print(f"  [{'OK' if ok else 'FAIL'}] {label} {extra}")

    try:
        hwnd = _steps.find_window(proc.pid)
        assert hwnd, "找不到窗口"
        assert _steps.ensure_focus(hwnd), "拿不到前台"
        clx, cly = client_origin(hwnd)
        r = wt.RECT()
        user32.GetWindowRect(hwnd, ctypes.byref(r))
        offx, offy = clx - r.left, cly - r.top
        print(f"[{tag}] hwnd={hwnd} 帧={r.right - r.left}x{r.bottom - r.top}")

        def click_node(node, settle=1.2):
            _steps.click(hwnd, node.cx - clx, node.cy - cly, settle=settle)

        def park_cursor():
            """光标移到窗口矩形之外两次（shot_wb ②）：悬停高亮/气泡不进帧。"""
            ox = r.left - 80 if r.left - 80 > 0 else r.right + 80
            oy = r.top - 80 if r.top - 80 > 0 else r.bottom + 80
            user32.SetCursorPos(ox, oy)
            time.sleep(0.3)
            user32.SetCursorPos(ox - 8, oy - 8)
            time.sleep(0.6)

        def shot(name):
            park_cursor()
            p = os.path.join(OUT, name + ".png")
            _rr = wt.RECT()
            user32.GetWindowRect(hwnd, ctypes.byref(_rr))
            _cr = wt.RECT()
            user32.GetClientRect(hwnd, ctypes.byref(_cr))
            print(f"    [shot {name}] 窗口 {_rr.right - _rr.left}x{_rr.bottom - _rr.top} "
                  f"客户区 {_cr.right - _cr.left}x{_cr.bottom - _cr.top}")
            _shoot.capture(hwnd, p)
            return p

        # 空点一次激活（D93：未获焦窗口的第一击只用于激活）
        _steps.click(hwnd, 700 - offx, 300 - offy, settle=1.0)
        assert _steps.ensure_focus(hwnd)

        def do_dialog(label, entry_name, primary, close_name, finder=None, settle=1.6):
            """一个对话框的完整一轮：点入口 → 主键在树里（开了）→ 抓帧 → 点关闭 → 主键没了。

            `primary` 支持前缀匹配（`删除这 N 个目录` 的 N 由扫描出的缓存数
            决定，猜数字就是给脚本埋一个随机失败）——传 `"前缀:"` 开头即按
            startswith 判。
            """
            if proc.poll() is not None:
                note(label, False, "进程已死")
                return
            finder = finder or (lambda ns: btn(ns, entry_name))
            pre = primary.startswith(":")
            want = primary[1:] if pre else primary

            def opened(ns):
                if pre:
                    return any(n.kind == "Button" and n.name.startswith(want) for n in ns)
                return has_btn(ns, want)

            try:
                click_node(finder(dump(hwnd)), settle)
                ns = dump(hwnd)
                if not opened(ns):
                    raise AssertionError(f"点了「{entry_name}」但树里没有主键「{primary}」"
                                         f" ⇒ 对话框没开（树：{[n.name for n in ns if n.kind == 'Button']}）")
                shot(f"dlg-{label}")
                click_node(btn(dump(hwnd), close_name), 1.4)
                ns2 = dump(hwnd)
                if opened(ns2):
                    raise AssertionError(f"点了「{close_name}」但主键「{primary}」还在 ⇒ 没关掉")
                note(label, True)
            except Exception as e:  # noqa: BLE001
                note(label, False, str(e)[:160])

        # ① 进「我的仓库」详情页（selected_path 已在沙箱指好，点导航即切屏）
        click_node(btn(dump(hwnd), "我的仓库"), 2.0)
        time.sleep(1.0)
        # ② 同步卡在**概览**页（`detail.rs::overview_body`）⇒ 先切过去抓定时同步
        click_node(tab(dump(hwnd), "概览"), 1.6)
        do_dialog("sync", "定时同步设置", "保存", "取消")
        # ③ 变更页：提交对话框（changes 非空才在树里）
        click_node(tab(dump(hwnd), "变更"), 1.6)
        do_dialog("commit", "提交并推送…", "提交并推送", "取消")
        # ④ 仓库头两颗（任何页签都在）：「常用命令」编辑窗 + 感叹号开的暂存清单
        do_dialog("cmds", "常用命令", "保存", "取消")
        # 收起来的改动：入口是仓库头那颗感叹号，a11y 名字 = 气泡整句（截断到 60 字）
        do_dialog("stash", "有 ", "关闭", "关闭",
                  finder=lambda ns: btn_pre(ns, "有 ", "收起来"))
        # ⑤ 侧栏两颗（克隆 / 添加仓库）
        do_dialog("clone", "克隆仓库", "开始克隆", "取消")
        do_dialog("add", "添加仓库", "添加", "取消")
        # ⑥ 空间页 → 一键清理（主键的目录数由扫描定，前缀匹配）
        click_node(tab(dump(hwnd), "空间"), 1.6)
        do_dialog("clean", "一键清理", ":删除这", "取消")
        # ⑦ 设置（导航轨那颗；主界面没有「保存」，判据分辨力够）
        # ⚠️ 这里**不要**先点一次再进 do_dialog——do_dialog 自己会点入口，
        # 先点等于把对话框开好，第二击落在遮罩上又把它关了（2026-10-12 实测：
        # 表现是"点了设置但树里没有保存"，而单独点每次都正常）
        do_dialog("settings", "设置", "保存", "取消")
        # ⑧ 第三方：点导航键**直接进第三方仓库详情**（侧栏不用点——
        #    2026-10-12 实测那颗键就等价于选中唯一的第三方仓库）
        click_node(btn(dump(hwnd), "我的仓库"), 1.6)
        click_node(btn(dump(hwnd), "第三方仓库"), 2.0)
        time.sleep(0.8)
        click_node(tab(dump(hwnd), "变更"), 1.6)
        do_dialog("purge", "清掉多出来的文件…", "按勾选清理", "取消")
        # ⑨ 关窗询问：WM_CLOSE（点 X 的编程等价；Ask 档下窗口留着、下一帧弹面板）
        if proc.poll() is None:
            try:
                user32.PostMessageW(hwnd, WM_CLOSE, 0, 0)
                time.sleep(1.6)
                ns = dump(hwnd)
                if not has_btn(ns, "完全退出"):
                    raise AssertionError("WM_CLOSE 之后树里没有「完全退出」⇒ 关窗询问没弹")
                shot("dlg-close-prompt")
                click_node(btn(dump(hwnd), "取消"), 1.4)
                ns2 = dump(hwnd)
                if has_btn(ns2, "完全退出"):
                    raise AssertionError("点了取消但「完全退出」还在 ⇒ 没关掉")
                note("close-prompt", True)
            except Exception as e:  # noqa: BLE001
                note("close-prompt", False, str(e)[:160])
    finally:
        err.close()
        size = os.path.getsize(ERR) if os.path.exists(ERR) else 0
        print(f"---- 抓帧（{tag}）----")
        for label, ok, extra in log:
            print(f"  {label:14s} {'OK' if ok else 'FAIL'} {extra}")
        bad = []
        if size:
            for ln in open(ERR, encoding="utf-8", errors="replace"):
                if "panicked" in ln or "Duplicate" in ln:
                    print("  >>", ln.strip())
                    bad.append(ln.strip())
        print(f"stderr_bytes={size}")
        if proc.poll() is None:
            proc.kill()
        time.sleep(0.5)
        qbguard.check_real_cfg(before)
    return [l for l, ok, _ in log if not ok] + bad


def diff_all():
    """两套帧逐对 diff（阈值与 frame.py 的 12 同口径）。"""
    a_dir = os.path.join(REPO, "target", "p2_frames_before")
    b_dir = os.path.join(REPO, "target", "p2_frames_after")
    names = sorted(set(os.listdir(a_dir)) & set(os.listdir(b_dir)))
    only_a = sorted(set(os.listdir(a_dir)) - set(os.listdir(b_dir)))
    only_b = sorted(set(os.listdir(b_dir)) - set(os.listdir(a_dir)))
    print(f"两套都有 {len(names)} 帧；仅改前有 {only_a}；仅改后有 {only_b}")
    worst = []
    for n in names:
        a = Image.open(os.path.join(a_dir, n)).convert("RGB")
        b = Image.open(os.path.join(b_dir, n)).convert("RGB")
        if a.size != b.size:
            print(f"{n}: 尺寸不同 {a.size} vs {b.size}")
            worst.append((n, -1))
            continue
        d = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 12 else 0)
        total = a.size[0] * a.size[1]
        cnt = total - d.histogram()[0]
        pct = cnt * 100.0 / total
        flag = "" if pct == 0 else ("  <<<" if pct > 0.5 else "")
        print(f"  {n:24s} 改动 {cnt:7d} px = {pct:6.3f}%{flag}")
        if pct > 0:
            worst.append((n, pct))
    print("\n判读：", "全部逐字节一致 ✓" if not worst else f"{len(worst)} 帧有差异，逐对看叠加图")
    return 0 if not worst else 1


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "diff":
        return diff_all()
    tag = sys.argv[1] if len(sys.argv) > 1 else "run"
    # 指纹在**动任何东西之前**取（build_fixture 虽不碰真实 config，但顺序上
    # 先取才说得上是"开跑前"）；run 的 finally 拿它核对，不拿当前值——
    # 拿当前值是自己跟自己比，那是一道永远绿的假闸
    before = qbguard.real_cfg_fp()
    build_fixture()
    problems = []
    try:
        problems = run(tag, before)
    finally:
        qbguard.check_real_cfg(before)
    print("\n判读：", "全绿" if not problems else f"有问题 {len(problems)} 条")
    for p in problems:
        print("  -", p)
    return 0 if not problems else 1


if __name__ == "__main__":
    sys.exit(main())
