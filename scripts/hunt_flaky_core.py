"""抓 core 那条偶发红：连跑 N 轮，把每一轮的 FAILED 名字与 panic 消息留下来。

「重跑就绿」对 flaky 零证明力（AGENTS 明写），所以这里要的不是"最后一次绿"，
而是**红的那一条到底叫什么**。
"""

import re
import subprocess
import sys

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

ROUNDS = int(sys.argv[1]) if len(sys.argv) > 1 else 6
# 只盯一条时给它 filter：偶发概率低，全量跑一轮 20 秒，聚焦跑一轮 2 秒
FILTER = sys.argv[2] if len(sys.argv) > 2 else None
# 偶发只在**并行**下出现，所以要跑的是整个 workspace 的测试数（第 3 个参数）
PKG = sys.argv[3] if len(sys.argv) > 3 else "qingbird-core"
hits = {}

for i in range(ROUNDS):
    cmd = ["cargo", "test", "-p", PKG] + (["--lib"] if PKG == "qingbird-core" else ["--bins"])
    if FILTER:
        cmd += ["--", FILTER, "--nocapture"]
    r = subprocess.run(
        cmd,
        capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    out = r.stdout + r.stderr
    failed = re.findall(r"^test (\S+) \.\.\. FAILED", out, re.M)
    print(f"第 {i + 1} 轮：退出码 {r.returncode} / 红 {failed}")
    for name in failed:
        if name not in hits:
            # 抓它自己的 panic 段：failures 块里那条以名字开头的输出
            block = re.search(rf"---- {re.escape(name)} stdout ----\n(.*?)\n\n", out, re.S)
            hits[name] = block.group(1).strip()[:600] if block else "（没抓到 stdout 段）"

print("\n==== 偶发红的名单 ====")
if not hits:
    print(f"  {ROUNDS} 轮全绿——注意：这只说明**这 {ROUNDS} 轮没撞上**，不说明它不存在")
for name, detail in hits.items():
    print(f"\n--- {name} ---\n{detail}")
