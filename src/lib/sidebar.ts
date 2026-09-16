/** 侧栏宽度上下限（App.tsx 的拖宽条与这里双重 clamp） */
export const SIDEBAR_MIN_W = 300;
export const SIDEBAR_MAX_W = 440;
const SIDEBAR_DEFAULT_W = 300;
const SIDEBAR_W_KEY = "qingbird.sidebarWidth";

function clampWidth(w: number): number {
  return Math.min(SIDEBAR_MAX_W, Math.max(SIDEBAR_MIN_W, Math.round(w)));
}

/** 读取持久化的侧栏宽度：非法值回退默认，越界值夹到上下限之间 */
export function loadSidebarWidth(): number {
  const raw = Number(localStorage.getItem(SIDEBAR_W_KEY));
  if (!Number.isFinite(raw) || raw <= 0) return SIDEBAR_DEFAULT_W;
  return clampWidth(raw);
}

/** 夹取并持久化侧栏宽度，返回最终生效值 */
export function saveSidebarWidth(w: number): number {
  const clamped = clampWidth(w);
  localStorage.setItem(SIDEBAR_W_KEY, String(clamped));
  return clamped;
}
