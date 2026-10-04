"""抓帧脚本共用的隔离闸门——**每次验收都不许碰用户真实配置**。

为什么要有这个模块（2026-10-05 实锤）：
  `steps.py` 用 `Popen([EXE])` 启动 exe，继承的是调用方 shell 的环境变量，
  脚本自身没有任何隔离入口。那一轮我拷了配置副本却忘了在 shell 里设
  `$env:APPDATA`，进程直接读写真实配置——把 `inspector_open` 从 false 写成了
  true（仓库一个没碰，配置被改了）。
  根因不是"忘了"，是**隔离靠人记得**。所以这里做成闸门：不设 `QB_APPDATA`
  就打醒目警告，收尾再按指纹核对一次，改了就一定报出来。

三个脚本（steps / uia_probe / a11y_stress）都从这里取，别各自再写一遍——
三处各写一遍的规则迟早有一处会漂。

用法：
    env = qbguard.launch_env()      # 启动前
    ...
    qbguard.check_real_cfg(before)  # 收尾
"""

import hashlib
import os
import sys

# PowerShell 5.1 的控制台是 GBK，**编不出 `✓` / `→` 这类字符**，# 而收尾那句「真实 config 未被触碰 ✓」正好带一个。于是整个验收脚本
# 在**跑完全部步骤之后**死在最后一句 print 上——前面抓的帧白跑，
# 而报错信息（`UnicodeEncodeError`）还指向一个看起来无关的地方。
# 统一把 stdout/stderr 切到 UTF-8 + 替换：编不出的字符降级成 '?'，
# 而不是把脚本炸掉。
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass  # 老解释器没有 reconfigure，认了

REAL_CFG = os.path.join(
    os.environ["APPDATA"], "com.qingbird.git", "config.json")


def real_cfg_fp():
    """真实 config 的指纹 (mtime, size, sha256 前 16 位)；读不到返回 None。"""
    try:
        with open(REAL_CFG, "rb") as fh:
            data = fh.read()
        return (os.path.getmtime(REAL_CFG), len(data),
                hashlib.sha256(data).hexdigest()[:16])
    except OSError:
        return None


def launch_env():
    """给 `Popen(env=...)` 用的环境：QB_APPDATA 存在就覆盖 APPDATA。"""
    env = os.environ.copy()
    appdata = env.get("QB_APPDATA")
    if appdata:
        env["APPDATA"] = appdata
        print("APPDATA ->", appdata)
    else:
        print("!! 没设 QB_APPDATA：这次跑的是【真实】%s，配置会被写"
              % os.environ["APPDATA"])
    return env


def check_real_cfg(before):
    """收尾核对：真实 config 的指纹变了就报出来。返回是否被碰过。"""
    after = real_cfg_fp()
    print("真实 config 前:", before)
    print("真实 config 后:", after)
    if before != after:
        print("!! 真实 config 被改了 —— 本次抓帧隔离没生效。"
              "设 QB_APPDATA 指向 $TEMP 下的副本再跑；"
              "已经改了的字段要手工改回去。")
        return True
    print("真实 config 未被触碰 ✓")
    return False
