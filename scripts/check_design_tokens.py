#!/usr/bin/env python
"""设计令牌机器闸：扫 `gui/src/`，把"同一属性两处各写一份"抓出来。

为什么要有它（路线表 D87 / v4 §6.1）：**没有闸的时候，这类规则三周就烂**。
本项目"禁写裸数字"那条立过一次，阶段 11 收尾时还是扫出 13 处裸 `gap`
（其中 8 处该是 4px 却写了 1–2px）；而"两处用了同一套写法，改一处忘另一处"
更是反复出现（D35 的通用形态）。⇒ 靠人记得住是假的，靠闸才是真的。

跑法：
    python scripts/check_design_tokens.py            # 全扫，有问题退出码 1
    python scripts/check_design_tokens.py -v         # 连"白名单命中"一起打

七条规则（编号会出现在报错里）：
    R1  字面色值（`rgb(0x…)` / `"#RRGGBB"`）—— 只有档位定义处允许
    R2  裸间距 / 圆角 / 字号：`.gap(3)` `.rounded(px(7.0))` `.text_size(px(13.0))` `.p(5)`
    R3  裸时长 `Duration::from_millis(数字)`（轮询节奏除外，见 POLL_OK）
    R4  `.bg(` 后面跟字面色
    R5  文字里混进 emoji / 假图标字符（`✓ ● ↑ ↓ ⓘ`）
    R6  同一个 `ElementId` 字符串在一个文件里出现两次（debug 构建会 panic）
    R7  直写 `t.colors.border` —— v4 的 `line` 档只许从 `tokens::edge(t)` 取

R7 为什么单独一条：它抓的**不是错值**，是"同一个属性两个名字"。`edge(t)` 就是
`t.colors.border`，值完全一样，所以显示不会漂、`cargo check` 也不会报——
但读代码的人看不出这两拨东西是不是一个用途，于是"把这一档调深一点"时
改了一处、漏了一处（2026-10-08 台账第三条）。**档位名与 v4 一字对齐、
每档只有一个取值入口**才是这条闸要的形态（`line-soft` 那一档早就是 24:0，
`line` 这一档以前是 5:6，现在归一）。

白名单只放**定义处**：`tokens.rs`（档位）、`theme.rs`（应用主题）、
`tray.rs` / `build.rs`（图标像素判据里必须写字面色）。
"""

import argparse
import io
import os
import re
import sys

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "gui", "src")

# 定义处：这些文件天生要写死值，不报 R1 / R2 / R4
DEFINITIONS = {"tokens.rs", "theme.rs"}
# 图像判据：把像素渲成文件看一眼（D39）时必须写字面色与阈值
IMAGING = {"tray.rs", "build.rs"}

# 轮询节奏不是"时长档"（ops.rs 每 120ms 读一次日志缓冲、tray.rs 扫托盘），
# 把它们塞进 motion_tokens 才是错。⇒ 这两处放行，但**要写明理由**。
POLL_OK = {"ops.rs", "tray.rs", "self_update.rs"}

# R5：拿文字冒充图标。历史上翻过车的字符都在这里。
# R5：拿文字冒充图标。历史上翻过车的字符都在这里。
# ⚠️ **箭头不在此列**（`→ ← ↑ ↓`）——它们出现在中文文案里是**标点**
# （"到「设置 → 显示哪些模块」里打开"那一类说明），不是图标。
# 把它们一起报，闸就会被人当噪音关掉，那比没有闸更糟。
FAKE_ICON = "✓✔●•○✖ⓘ⚠★☆"

RULES = [
    ("R1", re.compile(r"rgb\(0x[0-9A-Fa-f]{6}\)|#[0-9A-Fa-f]{6}\b")),
    ("R2", re.compile(r"\.(gap|p|px|py|rounded|text_size)\(\s*(?:px\()?\s*-?[0-9]+(?:\.[0-9]+)?\s*\)?\s*\)")),
    ("R3", re.compile(r"Duration::from_millis\(\s*[0-9]")),
    ("R4", re.compile(r"\.bg\(\s*(?:rgb|hsla?|rgba?)\s*\(")),
    # R7：v4 的 `line` 档只许从 `tokens::edge(t)` 取。直写 `t.colors.border`
    # 不是错值（两者同一个色），是**第二个出处**——调这一档时会漏改它
    ("R7", re.compile(r"\bcolors\.border\b")),
]

# R6 单独走：它判的是"**同文件内**同一个 id 出现两次"，逐行判看不出来
ID_PAT = re.compile(r"(?:Button|div|Kbd|Skeleton)::new\(\s*\"([^\"]+)\"")

# R2 的合法例外：`.p(0)` 之类"显式清零"不是档位，是"这里不要内边距"
R2_ZERO_OK = re.compile(r"\.(p|px|py|gap)\(\s*(?:px\()?\s*0(?:\.0+)?\s*\)?\s*\)")


def scan():
    hits = []
    for dirpath, _dirs, files in os.walk(SRC):
        for fn in sorted(files):
            if not fn.endswith(".rs"):
                continue
            path = os.path.join(dirpath, fn)
            rel = os.path.relpath(path, ROOT).replace("\\", "/")
            lines = io.open(path, encoding="utf-8").read().split("\n")
            is_def = fn in DEFINITIONS or fn in IMAGING
            ids = {}
            for no, line in enumerate(lines, 1):
                stripped = line.strip()
                if stripped.startswith("//") or stripped.startswith("///"):
                    continue  # 注释里提到旧值是**记录**，不是违规
                for code, pat in RULES:
                    if code == "R2" and R2_ZERO_OK.search(line):
                        continue
                    if code in ("R1", "R2", "R4") and is_def:
                        continue
                    # R7 的白名单**只有档位定义处**：IMAGING（tray/build 画像素）
                    # 不该出现在这里，把它们一起放进来等于给这条闸开后门
                    if code == "R7" and fn in DEFINITIONS:
                        continue
                    if code == "R3" and fn in POLL_OK:
                        continue
                    if pat.search(line):
                        hits.append((code, rel, no, stripped[:100]))
                if not is_def:
                    for ch in FAKE_ICON:
                        if ch in stripped and ".child(" in stripped:
                            hits.append(("R5", rel, no, stripped[:100]))
                            break
                for m in ID_PAT.finditer(line):
                    ids.setdefault(m.group(1), []).append(no)
            for eid, nos in ids.items():
                if len(nos) > 1:
                    hits.append(("R6", rel, nos[0], "id \"%s\" 另出现在 %s" % (eid, nos[1:])))
    return hits


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()
    hits = scan()
    by_rule = {}
    for code, rel, no, text in hits:
        by_rule.setdefault(code, []).append((rel, no, text))
    for code in sorted(by_rule):
        print("\n[%s] %d 处" % (code, len(by_rule[code])))
        for rel, no, text in by_rule[code]:
            print("  %s:%d  %s" % (rel, no, text))
    if not hits:
        print("设计令牌闸：gui/src 干净")
        return 0
    print("\n合计 %d 处违规" % len(hits))
    return 1


if __name__ == "__main__":
    sys.exit(main())
