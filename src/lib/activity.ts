// 提交活跃度柱状图的数据规则（跑法：pnpm test:web）。
//
// 后端只返回「有提交的日子」，补齐窗口、按比例定柱高都在这里，
// 组件只负责画。
import type { ActivityDay } from "../api";

/** 柱状图窗口天数，与 Rust 侧 ACTIVITY_DAYS 对应 */
export const ACTIVITY_WINDOW = 14;

/**
 * 本地时区的 YYYY-MM-DD。
 * 不能用 toISOString()：那是 UTC，本地早上 8 点前的提交会被算到前一天，
 * 柱子会整体错位一天。
 */
export function dateKey(d: Date): string {
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

/**
 * 把稀疏数据补成连续 window 天，旧的在前、今天在最后，缺的天补 0。
 * now 固定成当天零点再往前数，避免跨月/跨年时 setDate 受当前时分秒影响。
 */
export function fillActivityDays(
  days: readonly ActivityDay[],
  window: number = ACTIVITY_WINDOW,
  now: Date = new Date(),
): ActivityDay[] {
  const byDate = new Map(days.map((d) => [d.date, d.count]));
  const out: ActivityDay[] = [];
  for (let i = window - 1; i >= 0; i--) {
    const d = new Date(now);
    d.setHours(0, 0, 0, 0);
    d.setDate(d.getDate() - i);
    const date = dateKey(d);
    out.push({ date, count: byDate.get(date) ?? 0 });
  }
  return out;
}

/** 柱高比例 0..1；窗口内全是 0 时返回 0，画出来是一排空槽而不是除零 */
export function barRatio(count: number, max: number): number {
  if (count <= 0 || max <= 0) return 0;
  return Math.min(1, count / max);
}

/** hover 提示文案：`9月28日 · 3 次提交`；0 次的那天也照实说 */
export function barTitle(date: string, count: number): string {
  const [, m, d] = date.split("-");
  const day = `${Number(m)}月${Number(d)}日`;
  return count > 0 ? `${day} · ${count} 次提交` : `${day} · 无提交`;
}
