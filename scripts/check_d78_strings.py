#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""核 release exe 里**确实含有**本轮（D78）的新文案。

## 为什么不为这个写单测

判据是**产物自己**，不是构建命令的退出码：release 编成功只证明"没崩"，
而「文案没编进去」「改的是另一份二进制」这两类都能在退出码为 0 的情况下发生
（阶段 12 的 Windows 嵌图标就是这么漏的：`rc.exe` 见到二进制当脚本处理、
**不报错**，产出一个零条目的空 `.res`，exe 照样出来、图标一个没有）。

字符串在 exe 里的搜索比图标那套更直接：改过的文案一定会进 .rdata，
搜不到就说明**这份产物不是你以为的那一份**。

用法：python scripts/check_d78_strings.py target/release/qingbird-git.exe
"""
import sys

EXPECT = [
    # 本轮新增/改写的三句
    ("松开即加入", True),
    ("总览页不能添加仓库", True),
    ("请到「我的仓库」或「第三方仓库」页拖入", True),
    # 旧文案必须**不再出现**——它就是"提示没写组别"那个状态
    ("松开即可添加仓库", False),
]


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: check_d78_strings.py <exe>")
        return 2
    path = sys.argv[1]
    with open(path, "rb") as f:
        raw = f.read()
    text = raw.decode("utf-8", errors="ignore")
    print(f"{path}  {len(raw)} bytes")
    bad = 0
    for s, should in EXPECT:
        found = s in text
        ok = found == should
        if not ok:
            bad += 1
        want = "要出现" if should else "要消失"
        print(f"  [{'OK  ' if ok else 'FAIL'}] {s!r}  {want}  -> {'有' if found else '无'}")
    if bad:
        print(f"{bad} 条不符：这份 exe 不是本轮编出来的那一份（或改动没生效）")
        return 1
    print("全部符合：产物确实是本轮的")
    return 0


if __name__ == "__main__":
    sys.exit(main())