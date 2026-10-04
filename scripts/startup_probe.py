r"""量「启动到内容上屏」花了多久——阶段 16 的出口判据（秒开）。

为什么不能只抓最后一张帧：最后那张只证明「扫完了能显示」，
**证不了「打开就显示」**。要证的是**头几百毫秒那一帧就已经有内容**。
而"有没有内容"不能靠肉眼看——同一屏的骨架屏与真实数据肉眼可分，
机器分不出来。所以这里用逐像素差分：**每一帧都跟"最后一帧（已稳定）"比**，
差得少的说明内容已经到位，差得多的说明那一刻还在骨架屏。

用法：
    # 第一次跑（库里没东西）——预期前面几帧差很多。
    # 想从零开始就把 $QB_APPDATA\com.qingbird.git\cache.db 先删掉，
    # **删除单独一条命令做**，别塞进脚本参数里
    python scripts/startup_probe.py --tag cold
    # 第二次跑（库里有东西）——预期第一帧就已经接近稳定
    python scripts/startup_probe.py --tag warm

⚠️ **必须先设 `QB_APPDATA` 指向 $TEMP 下的配置副本**（走 qbguard），
否则跑的是用户真实配置——2026-10-05 那次就是这么把用户的
`inspector_open` 写成了 true 的。
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import qbguard  # noqa: E402

user32 = ctypes.windll.user32

REPO = r"F:\AIwork\qingbird-git"
EXE = os.path.join(REPO, "target", "debug", "qingbird-git.exe")

# 采样点（毫秒，从**窗口出现**算起，不是从进程启动）。
#
# ⚠️ 基准必须是窗口出现而不是进程启动：GPUI 初始化 + 建窗要 ~1.7 秒，
# 那段是「还没画」，**秒开与否跟它无关**。真正要量的是「窗口已经在了，
# 用户还要等多久才看到内容」。
# 头几个必须密——「打开就有」与「等一秒才有」的差别全在这前几百毫秒上。
SAMPLES_MS = [0, 100, 250, 450, 800, 1500, 3000, 6000, 12000, 25000]


def find_window(pid, timeout=25.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        found = []

        @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
        def cb(hwnd, _):
            p = wt.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
            if p.value != pid or not user32.IsWindowVisible(hwnd):
                return True
            r = wt.RECT()
            user32.GetWindowRect(hwnd, ctypes.byref(r))
            if (r.right - r.left) * (r.bottom - r.top) > 20000:
                found.append(hwnd)
            return True

        user32.EnumWindows(cb, 0)
        if found:
            return found[0]
        time.sleep(0.05)
    return None


def grab(hwnd, path):
    """走 shoot 那套 PrintWindow。GPU 加速的窗口必须带
    PW_RENDERFULLCONTENT(2)，否则拿到的是黑图。"""
    import shoot

    try:
        shoot.capture(hwnd, path)
        return True
    except SystemExit:
        return False


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tag", required=True, help="这一轮的标记，帧落在 %TEMP%\\qb16_<tag>")
    ap.add_argument("--extra-wait", type=float, default=0.0, help="最后一帧之后再等这么久")
    a = ap.parse_args()

    outdir = os.path.join(os.environ["TEMP"], "qb16_" + a.tag)
    os.makedirs(outdir, exist_ok=True)

    before = qbguard.real_cfg_fp()
    env = qbguard.launch_env()

    subprocess.run(["taskkill", "/F", "/IM", "qingbird-git.exe"], capture_output=True)
    time.sleep(0.8)

    err = os.path.join(outdir, "stderr.txt")
    t0 = time.time()
    fh = open(err, "wb")
    proc = subprocess.Popen([EXE], cwd=REPO, env=env,
                            stdout=subprocess.DEVNULL, stderr=fh)
    hwnd = None
    shots = []
    try:
        # 窗口还没出现之前先等它出现；出现那一刻单独记一笔
        while time.time() - t0 < 25.0 and hwnd is None:
            hwnd = find_window(proc.pid, timeout=0.2)
        if not hwnd:
            print("!! 25 秒内没出现窗口")
            return 2
        # 窗口出现那一刻另起一个计时基准：下面所有采样点都从它算起
        tw = time.time()
        print("进程启动 -> 窗口出现: %.0f ms" % ((tw - t0) * 1000))

        for ms in SAMPLES_MS:
            # 睡到点再抓，别把抓帧本身的耗时算进去
            while (time.time() - tw) * 1000 < ms:
                time.sleep(0.005)
            path = os.path.join(outdir, "t%04d.png" % ms)
            ok = grab(hwnd, path)
            real = (time.time() - tw) * 1000
            shots.append((ms, path, ok))
            print("  窗口出现后 %4d ms（实际 %4d）%s"
                  % (ms, real, "->" + path if ok else "抓不到"))

        if a.extra_wait:
            time.sleep(a.extra_wait)
            path = os.path.join(outdir, "final.png")
            grab(hwnd, path)
            print("  终帧 ->", path)
    finally:
        alive = proc.poll() is None
        fh.close()
        proc.kill()
        time.sleep(0.5)
        print("进程结束前还活着 =", alive, "| stderr",
              os.path.getsize(err), "字节")
        if os.path.getsize(err):
            with open(err, encoding="utf-8", errors="replace") as f:
                print(f.read()[:2000])
        qbguard.check_real_cfg(before)

    # ---- 逐帧与最后一帧比 ----
    from PIL import Image, ImageChops

    good = [(ms, p) for ms, p, ok in shots if ok]
    if len(good) < 2:
        print("帧太少，没法比")
        return 1
    last = Image.open(good[-1][1]).convert("RGB")
    total = last.size[0] * last.size[1]
    print("\n每帧与最后一帧的差（0 = 内容已与稳定态一致）")
    for ms, p in good:
        cur = Image.open(p).convert("RGB")
        if cur.size != last.size:
            print("  t=%4d ms  尺寸不同，跳过" % ms)
            continue
        diff = ImageChops.difference(cur, last)
        n = sum(1 for px in diff.getdata() if px != (0, 0, 0))
        print("  t=%4d ms  %7d px (%5.2f%%)" % (ms, n, 100.0 * n / total))
    print("\n帧目录:", outdir)
    return 0


if __name__ == "__main__":
    sys.exit(main())
