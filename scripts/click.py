"""按**客户区坐标**点击 gpui 窗口。

为什么不用屏幕坐标直接点：gpui 自绘标题栏，整窗是 `WS_POPUP | WS_THICKFRAME`，
客户区原点不在窗口左上角（有一圈非客户区边框），拿窗口左上角当原点会点歪。

`--first` 是「窗口还没获焦」时的保险：Windows 上第一击只用来激活窗口，
事件被吃掉，不补第二击就会把「点了没反应」误判成功能坏了。
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import os
import sys
import time

user32 = ctypes.windll.user32

MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004
MOUSEEVENTF_RIGHTDOWN = 0x0008
MOUSEEVENTF_RIGHTUP = 0x0010
MOUSEEVENTF_MOVE = 0x0001
MOUSEEVENTF_ABSOLUTE = 0x8000

SW_RESTORE = 9
MOUSEEVENTF_WHEEL = 0x0800


def client_to_screen(hwnd, x, y):
    pt = wt.POINT(int(round(x)), int(round(y)))
    if not user32.ClientToScreen(hwnd, ctypes.byref(pt)):
        raise SystemExit("ClientToScreen 失败")
    return pt.x, pt.y


def move(hwnd, x, y):
    sx, sy = client_to_screen(hwnd, x, y)
    user32.SetCursorPos(sx, sy)
    time.sleep(0.05)


def click_at(hwnd, x, y, button="left", settle=0.45):
    move(hwnd, x, y)
    if button == "left":
        down, up = MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP
    else:
        down, up = MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP
    user32.mouse_event(down, 0, 0, 0, 0)
    time.sleep(0.04)
    user32.mouse_event(up, 0, 0, 0, 0)
    time.sleep(settle)


def wheel_at(hwnd, x, y, notches):
    move(hwnd, x, y)
    user32.mouse_event(MOUSEEVENTF_WHEEL, 0, 0, notches * 120, 0)
    time.sleep(0.4)


def ensure_focus(hwnd):
    """把窗口拉到前台。失败不致命——真拿不到焦点时靠 --first 补第二击。"""
    user32.ShowWindow(hwnd, SW_RESTORE)
    # AttachThreadInput 那套太脆，直接 SetForegroundWindow + 两次空点击更省事
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.3)
    click_at(hwnd, 4, 4, settle=0.2)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--hwnd", type=int, required=True)
    ap.add_argument("--at", required=True, help="客户区坐标 x,y")
    ap.add_argument("--button", default="left", choices=["left", "right"])
    ap.add_argument("--wheel", type=int, default=0, help="滚轮格数，正数向上")
    ap.add_argument("--hold", action="store_true", help="按下后不松开（拖拽起点）")
    ap.add_argument("--first", action="store_true", help="窗口刚失焦，补一次激活点击")
    ap.add_argument("--no-focus", action="store_true", help="不尝试抢前台")
    ap.add_argument(
        "--probe", default=None, help="移过去等一帧后立刻抓到这张图（悬停类验证用）"
    )
    ap.add_argument(
        "--click", action="store_true", help="配合 --probe：先点一下再抓（点下去那一下的样子）"
    )
    args = ap.parse_args()

    x, y = (float(v) for v in args.at.split(","))
    hwnd = args.hwnd

    if not args.no_focus:
        ensure_focus(hwnd)
    if args.probe:
        sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
        w, h = probe(hwnd, x, y, args.probe, click=args.click)
        print(f"PROBED{'(+click)' if args.click else ''} at {x},{y} -> {args.probe} {w}x{h}")
        return
    if args.first:
        click_at(hwnd, x, y, args.button, settle=0.5)
    if args.wheel:
        wheel_at(hwnd, x, y, args.wheel)
        return
    if args.hold:
        move(hwnd, x, y)
        down = MOUSEEVENTF_LEFTDOWN if args.button == "left" else MOUSEEVENTF_RIGHTDOWN
        user32.mouse_event(down, 0, 0, 0, 0)
        time.sleep(0.1)
        print(f"HELD at {x},{y}")
        return
    click_at(hwnd, x, y, args.button)
    print(f"CLICKED {args.button} at {x},{y} hwnd={hwnd}")


def probe(hwnd, x, y, out, click=False):
    """移过去 → 等一帧 → （可选）点一下 → 立刻抓帧，**全在一个进程里连着做**。

    为什么不能拆成两次 `bash` 调用：机器上有人用真鼠标时，
    `SetCursorPos` 移过去的位置会被物理光标抢回去——2026-10-05
    抓「悬停才出现的复制键」时踩到过，移到位、等 0.9 秒、读回来
    已经偏了 15px。悬停与点击类验证都只能"移 + 点 + 抓"原子执行。

    `click=True` 用于**按下去之后的样子**（复制键换对号、按钮进 active 态）。
    注意点完光标不能挪走：悬停态一丢，悬停才出现的元素当场消失，
    截出来的是"点之前"那张图，白跑一轮（同样 2026-10-05 踩到）。
    """
    move(hwnd, x, y)
    time.sleep(0.7)
    if click:
        down, up = MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP
        user32.mouse_event(down, 0, 0, 0, 0)
        time.sleep(0.04)
        user32.mouse_event(up, 0, 0, 0, 0)
        time.sleep(0.35)
    import shoot

    return shoot.capture(hwnd, out)


if __name__ == "__main__":
    sys.exit(main())
