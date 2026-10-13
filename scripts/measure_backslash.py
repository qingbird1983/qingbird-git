r"""D92 那条「改完报三个数」的测量语义核对（2026-10-10）。

Git Bash 里 `grep -o '\\\\' file` 给的其实是**反斜杠总数**而不是"连续两处"的成对数。
本轮一度把它叫成「成对反斜杠」写进登记，名实不符（要挡的那类损坏仍挡得住——
翻倍会让总数翻倍——但名字要改准）。这里两种测法各算一遍，用真值定名。
"""
import subprocess
import sys

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

B = chr(92)          # 一个反斜杠
BB = chr(92) * 2     # 连续两个


def stat(name, raw):
    s = raw.decode("utf-8", errors="replace")
    print(
        "%-26s 行数=%5d 块引用行=%3d 反斜杠总数=%4d 连续两处=%3d CRLF=%d"
        % (
            name,
            s.count("\n"),
            sum(1 for ln in s.split("\n") if ln.startswith(">")),
            s.count(B),
            s.count(BB),
            s.count("\r\n"),
        )
    )


def git_show(p):
    return subprocess.run(["git", "show", "HEAD:" + p], capture_output=True).stdout


DOCS = [
    "README.md",
    "AGENTS.md",
    "docs/UI-ROADMAP.md",
    "docs/CHANGELOG.md",
    "docs/roadmap/archive.md",
    "docs/roadmap/31-v4-redesign.md",
    "docs/roadmap/32-gaps.md",
    "docs/roadmap/cli.md",
]

for p in DOCS:
    stat(p + " 工作树", open(p, "rb").read())
    head = git_show(p)
    if head:
        stat(p + " HEAD 版", head)
    else:
        print("%-26s HEAD 里还没有这一份（本轮新加的，没有可比的一版）" % (p + " HEAD 版"))
