// 提交活跃度柱状图的数据规则测试（跑法：pnpm test:web）。
import { test } from "node:test";
import assert from "node:assert/strict";

import { ACTIVITY_WINDOW, barRatio, barTitle, dateKey, fillActivityDays } from "./lib/activity.ts";

test("本地时区的日期键：不能滑到 UTC 那一天", () => {
  assert.equal(dateKey(new Date(2026, 8, 28)), "2026-09-28");
  assert.equal(dateKey(new Date(2026, 0, 5)), "2026-01-05", "月份补零");
  assert.equal(dateKey(new Date(2026, 11, 31)), "2026-12-31");
  // 早上 8 点前的提交：用 toISOString 会算成前一天
  const early = new Date(2026, 8, 28, 3, 30);
  assert.equal(dateKey(early), "2026-09-28");
});

test("稀疏数据补成连续窗口：旧的在前、今天在最后", () => {
  const now = new Date(2026, 8, 28, 15, 0);
  const days = fillActivityDays(
    [
      { date: "2026-09-28", count: 4 },
      { date: "2026-09-26", count: 1 },
    ],
    ACTIVITY_WINDOW,
    now,
  );
  assert.equal(days.length, ACTIVITY_WINDOW, "窗口长度固定");
  assert.equal(days[0].date, "2026-09-15", "第一天是 14 天前");
  assert.equal(days.at(-1)?.date, "2026-09-28", "最后一天是今天");
  // 索引 11 = 09-26，索引 13 = 09-28
  assert.equal(days[11].count, 1);
  assert.equal(days[13].count, 4);
  assert.equal(days[12].count, 0, "没提交的日子补 0，不能缺位");
  assert.equal(days.reduce((s, d) => s + d.count, 0), 5);
});

test("跨月跨年往回数", () => {
  const now = new Date(2026, 0, 3);
  const days = fillActivityDays([{ date: "2026-01-01", count: 2 }], 5, now);
  assert.deepEqual(
    days.map((d) => d.date),
    ["2025-12-30", "2025-12-31", "2026-01-01", "2026-01-02", "2026-01-03"],
  );
  assert.equal(days[2].count, 2);
});

test("空仓库 / 还没加载完：一排 0，不报错", () => {
  const days = fillActivityDays([], ACTIVITY_WINDOW, new Date(2026, 8, 28));
  assert.equal(days.length, ACTIVITY_WINDOW);
  assert.ok(days.every((d) => d.count === 0));
});

test("窗口外的日期不进来", () => {
  const now = new Date(2026, 8, 28);
  const days = fillActivityDays([{ date: "2025-01-01", count: 99 }], ACTIVITY_WINDOW, now);
  assert.ok(days.every((d) => d.count === 0), "后端只给窗口内的数据，越界的也要挡住");
});

test("柱高比例：最大值满高，全 0 时不除零", () => {
  assert.equal(barRatio(0, 10), 0);
  assert.equal(barRatio(10, 10), 1);
  assert.equal(barRatio(5, 10), 0.5);
  assert.equal(barRatio(0, 0), 0, "全空仓库不能出现 NaN");
  assert.equal(barRatio(3, 0), 0);
  assert.equal(barRatio(20, 10), 1, "异常数据也不该溢出卡外");
});

test("hover 提示：有几次说几次，没提交照实说", () => {
  assert.equal(barTitle("2026-09-28", 3), "9月28日 · 3 次提交");
  assert.equal(barTitle("2026-09-01", 1), "9月1日 · 1 次提交");
  assert.equal(barTitle("2026-09-28", 0), "9月28日 · 无提交");
});
