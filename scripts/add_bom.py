#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""给 .iss 补 UTF-8 BOM。

Inno Setup 读脚本时：**有 BOM 才当 UTF-8**，没有就按系统 ANSI 解释——
本机是 GBK，于是脚本里每一处中文都会变成乱码，编出来的向导也跟着花。
这是「写进去的东西没人读」那一类的坑：文件在、ISCC 也成功、只是内容坏了。
"""
import sys
from pathlib import Path

# PowerShell 5.1 的控制台是 GBK，**编不出中文**，于是这两行 print 出来全是
# `����`——而脚本本身干得对不对看不出来。同 `qbguard.py` 的处理：把 stdout/stderr
# 切到 UTF-8 + 替换（编不出的字符降级成 '?'，而不是把脚本炸掉）。
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

BOM = b"\xef\xbb\xbf"

for arg in sys.argv[1:]:
    p = Path(arg)
    raw = p.read_bytes()
    if raw.startswith(BOM):
        print(f"{p.name}: 已有 BOM，跳过")
        continue
    p.write_bytes(BOM + raw)
    print(f"{p.name}: 已补 BOM（{len(raw)} -> {len(raw) + 3} 字节）")
