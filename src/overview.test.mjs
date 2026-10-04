// 总览页的数据规则测试（跑法：pnpm test:web）。
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  BATCH_LABEL,
  BUCKET_LABEL,
  bucketOf,
  buildRows,
  buildTodo,
  batchPlan,
  batchSummary,
  countBuckets,
  overviewSummary,
  ringGradient,
} from "./lib/overview.ts";

/** 造一个仓库条目：只填总览用得到的字段 */
const repo = (path, alias) => ({ path, name: path.split(/[\\/]/).pop(), alias: alias ?? "" });

/** 造一份状态；behind / ahead 不给就是 0 */
const st = (o = {}) => ({
  is_repo: true,
  branch: "main",
  ahead: 0,
  behind: 0,
  dirty: false,
  has_upstream: true,
  remote_url: "git@github.com:a/b.git",
  ...o,
});

test("一个仓库只归一档，分叉优先于落后与待推", () => {
  assert.equal(bucketOf(undefined), "unknown", "没扫过就是待扫描，不能当成失联");
  assert.equal(bucketOf(st({ is_repo: false })), "missing");
  assert.equal(bucketOf(st({ behind: 3 })), "behind");
  assert.equal(bucketOf(st({ ahead: 2 })), "ahead");
  assert.equal(bucketOf(st({ behind: 3, ahead: 2 })), "fork", "两边都有就是分叉，不再拆成落后+待推");
  assert.equal(bucketOf(st()), "synced");
});

test("行文案与动作跟着档走，分叉不给一键动作", () => {
  const rows = buildRows(
    [repo("F:/a/one"), repo("F:/a/two"), repo("F:/a/three"), repo("F:/a/four")],
    {
      "F:/a/one": st({ behind: 3 }),
      "F:/a/two": st({ ahead: 5 }),
      "F:/a/three": st({ behind: 2, ahead: 1 }),
      "F:/a/four": st({ is_repo: false }),
    },
  );
  assert.deepEqual(
    rows.map((r) => r.action),
    ["pull", "push", "goto", "goto"],
  );
  assert.equal(rows[0].note, "落后 3 个提交");
  assert.equal(rows[2].note, "已分叉 · 本地 1 / 云端 2");
  // 分叉不给"一键变基"：那会改写本地历史，只能进仓库里的向导点
  assert.equal(rows[2].actionLabel, "查看");
  assert.equal(rows[3].note, "目录不存在或已不是 git 仓库");
});

test("已同步的行没有动作，也进不了待处理", () => {
  const rows = buildRows([repo("F:/a/ok")], { "F:/a/ok": st() });
  assert.equal(rows[0].action, null);
  assert.deepEqual(buildTodo(rows), []);
  assert.equal(overviewSummary(rows).synced, 1);
});

test("待扫描的仓库不算待办：它只是还没扫，不是有事要做", () => {
  const rows = buildRows([repo("F:/a/x"), repo("F:/a/y", "别名仓库")], { "F:/a/y": st({ behind: 1 }) });
  assert.equal(rows[0].bucket, "unknown");
  assert.equal(rows[1].name, "别名仓库", "有别名就用别名，没有就用目录名");
  assert.deepEqual(
    buildTodo(rows).map((r) => r.path),
    ["F:/a/y"],
  );
});

test("待处理排序：分叉 → 落后 → 待推送 → 失联，同档按提交数多的在前", () => {
  const rows = buildRows(
    [repo("F:/a/missing"), repo("F:/a/ahead1"), repo("F:/a/behind1"), repo("F:/a/fork"), repo("F:/a/behind9")],
    {
      "F:/a/missing": st({ is_repo: false }),
      "F:/a/ahead1": st({ ahead: 1 }),
      "F:/a/behind1": st({ behind: 1 }),
      "F:/a/fork": st({ behind: 2, ahead: 2 }),
      "F:/a/behind9": st({ behind: 9 }),
    },
  );
  assert.deepEqual(
    buildTodo(rows).map((r) => r.path),
    ["F:/a/fork", "F:/a/behind9", "F:/a/behind1", "F:/a/ahead1", "F:/a/missing"],
  );
});

test("状态分布：每一档各计一次，总数等于仓库数", () => {
  const rows = buildRows(
    [repo("F:/a/1"), repo("F:/a/2"), repo("F:/a/3"), repo("F:/a/4"), repo("F:/a/5")],
    {
      "F:/a/1": st(),
      "F:/a/2": st(),
      "F:/a/3": st({ behind: 1 }),
      "F:/a/4": st({ ahead: 1 }),
      "F:/a/5": undefined,
    },
  );
  const counts = countBuckets(rows);
  assert.equal(counts.synced, 2);
  assert.equal(counts.behind, 1);
  assert.equal(counts.ahead, 1);
  assert.equal(counts.unknown, 1);
  assert.equal(Object.values(counts).reduce((a, b) => a + b, 0), 5);

  const sum = overviewSummary(rows);
  assert.deepEqual(sum, { total: 5, todo: 2, synced: 2, commits: 2 });
});

test("环的渐变按 RING_ORDER 顺时针排，零计数的档不占扇区", () => {
  const counts = { unknown: 0, missing: 0, fork: 0, behind: 2, ahead: 1, synced: 5 };
  assert.equal(
    ringGradient(counts, 8),
    "conic-gradient(var(--ok) 0% 62.5%, var(--warn) 62.5% 87.5%, var(--accent) 87.5% 100%)",
  );
});

test("环只有一个档时铺满整圈，不留缺口", () => {
  assert.equal(
    ringGradient({ unknown: 0, missing: 0, fork: 3, behind: 0, ahead: 0, synced: 0 }, 3),
    "conic-gradient(var(--fork) 0% 100%)",
  );
});

test("百分比不吐一长串小数（三个均分要写得出 33.33%）", () => {
  const g = ringGradient({ unknown: 0, missing: 0, fork: 1, behind: 1, ahead: 1, synced: 0 }, 3);
  assert.equal(g, "conic-gradient(var(--warn) 0% 33.33%, var(--accent) 33.33% 66.67%, var(--fork) 66.67% 100%)");
});

test("一个仓库都没有时环退回底色线，不画成空心圆", () => {
  assert.equal(ringGradient(countBuckets([]), 0), "var(--line-soft)");
});

test("每一档都有中文标签（图例直接用它，别在组件里再写一遍）", () => {
  for (const k of ["unknown", "missing", "fork", "behind", "ahead", "synced"]) {
    assert.ok(BUCKET_LABEL[k]?.length, `${k} 缺标签`);
  }
});

/* ---------- 一键批量 ---------- */

/** 造一份待处理：落后两个、待推一个、分叉一个、失联一个 */
function todoFixture() {
  return buildTodo(
    buildRows(
      [repo("F:/a/behind9"), repo("F:/a/behind1"), repo("F:/a/ahead"), repo("F:/a/fork"), repo("F:/a/gone")],
      {
        "F:/a/behind9": st({ behind: 9 }),
        "F:/a/behind1": st({ behind: 1 }),
        "F:/a/ahead": st({ ahead: 4 }),
        "F:/a/fork": st({ behind: 2, ahead: 1 }),
        "F:/a/gone": st({ is_repo: false }),
      },
    ),
  );
}

test("批量只收同类动作的行：分叉与失联两边都不进队列", () => {
  const todo = todoFixture();
  assert.deepEqual(
    batchPlan(todo, "pull").rows.map((r) => r.path),
    ["F:/a/behind9", "F:/a/behind1"],
  );
  assert.deepEqual(
    batchPlan(todo, "push").rows.map((r) => r.path),
    ["F:/a/ahead"],
  );
  for (const p of batchPlan(todo, "pull").rows) {
    assert.notEqual(p.bucket, "fork", "分叉不能被一键改写历史");
    assert.notEqual(p.bucket, "missing", "失联仓库目录都不在了");
  }
});

test("队列顺序沿用待处理排序，不再排一套", () => {
  const todo = todoFixture();
  const pulls = batchPlan(todo, "pull").rows.map((r) => r.path);
  assert.deepEqual(pulls, todo.filter((r) => r.action === "pull").map((r) => r.path));
  assert.deepEqual(pulls, ["F:/a/behind9", "F:/a/behind1"], "落后多的先来（待处理里就这个序）");
});

test("没有同类行时不出计划：按钮不出现，而不是摆一个点了没反应的钮", () => {
  const onlyAhead = buildTodo(buildRows([repo("F:/a/ahead")], { "F:/a/ahead": st({ ahead: 1 }) }));
  assert.equal(batchPlan(onlyAhead, "pull"), null);
  assert.equal(batchPlan(onlyAhead, "push").rows.length, 1);
  assert.equal(batchPlan([], "pull"), null, "待办为空也不给计划");
  assert.equal(batchPlan([], "push"), null);
});

test("按钮文案带上数量，用户点之前就知道会动几个仓库", () => {
  assert.equal(batchPlan(todoFixture(), "pull").label, `全部${BATCH_LABEL.pull} 2`);
  assert.equal(batchPlan(todoFixture(), "push").label, `全部${BATCH_LABEL.push} 1`);
});

test("汇总：一切正常就只报成功数，不写「跳过 0 个」这种废话", () => {
  const r = { action: "pull", total: 3, done: 3, ok: 3, skipped: 0, failed: 0, stopped: false };
  assert.equal(batchSummary(r), "批量拉取完成：3 个成功");
});

test("汇总：跳过与失败才出现，且顺序固定在成功之后", () => {
  const r = { action: "pull", total: 4, done: 4, ok: 2, skipped: 1, failed: 1, stopped: false };
  assert.equal(
    batchSummary(r),
    "批量拉取完成：2 个成功 · 1 个跳过（工作区有改动，需切到该仓库确认后再拉） · 1 个失败，原因见各自日志",
  );
});

test("汇总：跳过的原因跟着动作走——推送没有暂存这回事", () => {
  const base = { total: 1, done: 1, ok: 0, skipped: 1, failed: 0, stopped: false };
  assert.match(batchSummary({ ...base, action: "pull" }), /确认后再拉/);
  assert.equal(batchSummary({ ...base, action: "push" }), "批量推送完成：0 个成功 · 1 个跳过（已跳过）");
});

test("汇总：中途停止时说清跑到哪、还剩几个没动", () => {
  const r = { action: "pull", total: 5, done: 2, ok: 2, skipped: 0, failed: 1, stopped: true };
  assert.equal(batchSummary(r), "批量拉取已停止：2/5 个已处理，剩下 3 个没动");
});

test("汇总：停在最后一个上算跑完了，别报「剩下 0 个没动」", () => {
  const r = { action: "push", total: 2, done: 2, ok: 2, skipped: 0, failed: 0, stopped: true };
  assert.equal(batchSummary(r), "批量推送完成：2 个成功");
});

