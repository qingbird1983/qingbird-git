#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""读 PE 头的 Subsystem 字段，判 exe 是 GUI 子系统还是 console 子系统。

为什么要有这个脚本
------------------
`#![windows_subsystem = "windows"]` 写在源码里，**不报错就等于写进去了吗？**
不是。属性写错位置、profile 没走到、构建的不是那条产物路径——这三种情况
`cargo check` 零错、构建全绿，而 exe 仍然是 console 子系统，用户双击就是黑框。

**判据落在产物自己身上**：PE Optional Header 偏移 68 的那个 WORD。
- 2 = IMAGE_SUBSYSTEM_WINDOWS_GUI   （无控制台，双击不弹黑框）
- 3 = IMAGE_SUBSYSTEM_WINDOWS_CUI   （会分配控制台 = 黑框）

用法：
    python scripts/pe_subsystem.py <exe> [更多 exe...]
    python scripts/pe_subsystem.py target/release/qingbird-git.exe

退出码 0 = 全部是 GUI 子系统；1 = 有 console 子系统的（黑框还在）。
"""

import struct
import sys

SUBSYSTEMS = {
    0: "UNKNOWN",
    1: "NATIVE",
    2: "WINDOWS_GUI",   # 要的
    3: "WINDOWS_CUI",   # 黑框
    5: "OS2_CUI",
    7: "POSIX_CUI",
    9: "WINDOWS_CE_GUI",
    10: "EFI_APPLICATION",
    11: "EFI_BOOT_SERVICE_DRIVER",
    12: "EFI_RUNTIME_DRIVER",
    13: "EFI_ROM",
    14: "XBOX",
    16: "WINDOWS_BOOT_APPLICATION",
}

# Optional Header 里 Subsystem 的字节偏移（PE32 与 PE32+ 同位置）
SUBSYSTEM_OFFSET = 68
# Optional Header 的起始 = PE 签名(4) + COFF 头(20)
OPTIONAL_HEADER_START = 24


def read_subsystem(path):
    """返回 (subsystem 值, 名称)；不是合法 PE 就抛 ValueError。"""
    with open(path, "rb") as f:
        data = f.read()

    if len(data) < 0x40 or data[:2] != b"MZ":
        raise ValueError("不是 PE 文件（缺 MZ 头）")

    # e_lfanew 在 DOS 头偏移 0x3C
    e_lfanew = struct.unpack_from("<I", data, 0x3C)[0]
    if e_lfanew + OPTIONAL_HEADER_START + 2 > len(data):
        raise ValueError("e_lfanew 越界")
    if data[e_lfanew : e_lfanew + 4] != b"PE\0\0":
        raise ValueError("缺 PE 签名")

    off = e_lfanew + OPTIONAL_HEADER_START + SUBSYSTEM_OFFSET
    (value,) = struct.unpack_from("<H", data, off)
    return value, SUBSYSTEMS.get(value, "未知")


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2

    bad = 0
    for path in argv[1:]:
        try:
            value, name = read_subsystem(path)
        except (OSError, ValueError) as e:
            print(f"[FAIL] {path}: {e}")
            bad += 1
            continue

        mark = "OK  " if value == 2 else "BAD "
        if value != 2:
            bad += 1
        extra = "" if value == 2 else "   <- console 子系统，双击会弹黑终端窗口"
        print(f"[{mark}] Subsystem={value} ({name})  {path}{extra}")

    return 1 if bad else 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.exit(main(sys.argv))
