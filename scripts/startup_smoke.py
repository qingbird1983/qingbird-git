#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""启动冒烟：进程活着吗 / 窗口句柄非零吗 / Responding 吗 / stderr 空吗 / 有 panic 吗。

为什么要有这个
--------------
「编译通过」和「能跑起来」是**两道关**。带 `debug_assert!` 的框架尤其如此：
同一个错在 debug 里是立刻 panic，在 release 里变成「静默地少活一帧」。
本项目的血泪史就是 release 全绿、用户双击 exe 闪退（11 处 `cx.on_action` 挂在构造期）。

⚠️ **为什么必须用 debug 那份 exe**
`release` 现在是 GUI 子系统（没有 stderr，`#![windows_subsystem]`），
panic 消息在它那里无处可去——**恰好是这份冒烟最需要的东西**。
debug 保留控制台，stderr 与 `app.log` 都读得到。

⚠️ **必须设 `QB_APPDATA` 指向 $TEMP 下的副本**
否则跑的是用户真实配置（2026-10-05 那次把用户的 `inspector_open` 写成了 true）。
脚本收尾按 `mtime+size+sha256` 核对真实文件，不一致直接报警。

用法：
    python scripts/startup_smoke.py [秒数，默认 15]
退出码 0 = 四项全过且无 panic；1 = 有问题。
"""

import ctypes
import ctypes.wintypes as wt
import os
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import qbguard  # noqa: E402

user32 = ctypes.windll.user32

REPO = r"F:\AIwork\qingbird-git"
EXE = os.path.join(REPO, "target", "debug", "qingbird-git.exe")


def enum_windows_for_pid(pid):
    """列出属于该 pid 的可见顶层窗口，返回 [(hwnd, title)]。"""
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(hwnd, _):
        wpid = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(wpid))
        if wpid.value == pid and user32.IsWindowVisible(hwnd):
            n = user32.GetWindowTextLengthW(hwnd)
            buf = ctypes.create_unicode_buffer(n + 1)
            user32.GetWindowTextW(hwnd, buf, n + 1)
            found.append((hwnd, buf.value))
        return True

    user32.EnumWindows(cb, 0)
    return found


def main(argv):
    seconds = float(argv[1]) if len(argv) > 1 else 15.0

    if not os.path.exists(EXE):
        print(f"[FAIL] 找不到 {EXE}，先 cargo build --workspace")
        return 1

    # ---- 隔离：一律走 qbguard 这个共用闸门，别自己拼 env ----
    #
    # ⚠️⚠️ **第一版就是死在这里**：我另起炉灶写了
    #   `env = dict(os.environ); env["QB_APPDATA"] = sandbox`
    # 而 `QB_APPDATA` **只有 qbguard 认识**——它是给闸门用的输入，
    # **应用读的是 `APPDATA`**（`store::config_path()` 直接
    # `std::env::var("APPDATA")`）。于是子进程照样写真实配置，
    # 收尾指纹核对把这件事报了出来——**闸门是有效的，坏的是我绕过它**。
    # 判据：`QB_APPDATA` 是**脚本之间的约定**，不是环境契约。
    # 新写任何起子进程的脚本，`env` 一律 `qbguard.launch_env()`。
    sandbox = tempfile.mkdtemp(prefix="qb-smoke-")
    os.environ["QB_APPDATA"] = sandbox
    env = qbguard.launch_env()
    assert env["APPDATA"] == sandbox, "launch_env 没把 APPDATA 指到沙箱"

    # 沙箱里放一份真实配置的副本，让应用有东西可读（否则走的是"库是空的"那条路）
    real_dir = os.path.join(qbguard.REAL_CFG, "..")
    real_dir = os.path.normpath(real_dir)
    if os.path.isdir(real_dir):
        shutil.copytree(real_dir, os.path.join(sandbox, "com.qingbird.git"),
                        dirs_exist_ok=True)

    before = qbguard.real_cfg_fp()
    proc = subprocess.Popen([EXE], env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE)
    print(f"已起 pid={proc.pid}，等 {seconds:.0f}s …")
    time.sleep(seconds)

    alive = proc.poll() is None
    hwnds = enum_windows_for_pid(proc.pid) if alive else []
    responding = bool(hwnds) and user32.IsHungAppWindow is not None
    # IsHungAppWindow 只在目标挂起时为 True；这里反过来问"没挂"
    hung = bool(hwnds) and bool(user32.IsHungAppWindow(hwnds[0][0]))

    if alive:
        proc.terminate()
        try:
            out, err = proc.communicate(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
            out, err = proc.communicate()
    else:
        out, err = proc.communicate()

    log = os.path.join(sandbox, "com.qingbird.git", "app.log")
    log_text = ""
    if os.path.exists(log):
        with open(log, "rb") as f:
            log_text = f.read().decode("utf-8", "replace")
    panicked = "PANIC at" in log_text

    print(f"进程存活        : {alive}")
    print(f"可见窗口        : {[(hex(h), t) for h, t in hwnds]}")
    print(f"窗口未挂起      : {not hung}")
    print(f"stderr 字节数   : {len(err)}")
    if err:
        print("  stderr 前 500 字：")
        print("  " + err.decode("utf-8", "replace")[:500].replace("\n", "\n  "))
    print(f"app.log 行数    : {len(log_text.splitlines())}")
    # ⚠️ **有内容就一定要打出来**（D39：产物必须亲眼看）。
    # 一份没人读过的日志等于没有日志；"7 行"这种数字本身解释不了任何事。
    if log_text.strip():
        print("  app.log 内容：")
        for line in log_text.splitlines()[:20]:
            print("    " + line)
        if len(log_text.splitlines()) > 20:
            print(f"    …（还有 {len(log_text.splitlines()) - 20} 行）")
    print(f"有 panic        : {panicked}")
    if panicked:
        print("  app.log 内容：")
        print("  " + log_text[:1000].replace("\n", "\n  "))

    after = qbguard.real_cfg_fp()
    touched = before != after
    print(f"真实 config 未动: {not touched}")
    if touched:
        print(f"  before={before}\n  after ={after}")

    shutil.rmtree(sandbox, ignore_errors=True)

    ok = (alive and bool(hwnds) and not hung and len(err) == 0
          and not panicked and not touched)
    print("\n" + ("✅ 全过" if ok else "❌ 有问题，见上"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.exit(main(sys.argv))
