// 全局快捷键的判定（纯逻辑，不碰 DOM，便于 node --test 回归）。
//
// 由 App 挂一个 window keydown 调 resolveHotkey，拿到动作 id 再去 store 执行；
// DOM 相关的东西（焦点在不在输入框）由调用方查好后当参数传进来。
//
// 键位约定：
//   Ctrl K  命令面板（输入框里也生效——面板本身就是"从任何地方跳出来"的东西）
//   Ctrl T  在当前仓库打开终端
//   1 / 2 / 3  对应同步卡上从左到右的三个键：拉取 / 推送 / 检测
//   ↑ / ↓    上下切仓库
// 1/2/3 与 ↑↓ **按屏幕位置**对位而不是按操作顺序：用户是先看到按钮再按对应位置的键，
// 而不是先背一套编号规则。

export type HotkeyAction = "palette" | "terminal" | "pull" | "push" | "check" | "prevRepo" | "nextRepo";

export interface HotkeyEvent {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

export interface HotkeyCtx {
  /** 焦点在 input / textarea / contenteditable 里：除了 Ctrl K 都让给用户打字 */
  inInput: boolean;
  /** 当前有选中仓库：仓库相关的键位才有意义 */
  hasRepo: boolean;
  /**
   * 当前是不是仓库主面板。总览页上没有那三个键，
   * 凭空按 1 就在另一个仓库上拉取会让人以为按错了——所以这里让位。
   * Ctrl K 与 ↑↓ 不受限制：前者是全局入口，后者切仓库在两屏都自然。
   */
  inReposView: boolean;
}

/** 三键的数字键 → 动作，顺序必须和同步卡上按钮从左到右一致（拉取 / 推送 / 检测） */
const DIGIT_ACTIONS: Record<string, HotkeyAction> = {
  "1": "pull",
  "2": "push",
  "3": "check",
};

export function resolveHotkey(e: HotkeyEvent, ctx: HotkeyCtx): HotkeyAction | null {
  // Ctrl K 无论焦点在哪都生效：面板是全局入口，输入框里按它也该跳出来
  const mod = e.ctrlKey || e.metaKey;
  if (mod && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "k") return "palette";

  // 其余键位一律不进：正在打字时按 1 就是要打 1，按 ↓ 就是要移动光标
  if (ctx.inInput) return null;

  if (mod && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "t") {
    return ctx.hasRepo && ctx.inReposView ? "terminal" : null;
  }

  // 下面这些都要求"光秃秃"的一个键：Shift+1 是 !，Alt+1 在某些布局下另有含义
  if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return null;
  if (!ctx.hasRepo) return null;

  const digit = DIGIT_ACTIONS[e.key];
  if (digit) return ctx.inReposView ? digit : null;

  if (e.key === "ArrowUp") return "prevRepo";
  if (e.key === "ArrowDown") return "nextRepo";
  return null;
}

/** 在排好序的仓库列表里上下移动，到头就停在头（不环绕） */
export function stepRepoIndex(current: number, delta: number, length: number): number {
  if (length <= 0) return -1;
  // current 是"当前选中项在数组里的下标"；-1 表示还没有选中，从头开始
  const from = current < 0 ? (delta > 0 ? -1 : 0) : current;
  const next = from + delta;
  if (next < 0) return 0;
  if (next >= length) return length - 1;
  return next;
}
