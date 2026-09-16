// 分叉卡片的"该显示什么"是纯逻辑，放在 src/lib/forkGuide.ts 里单独测，
// 不用起 React 也能覆盖所有分支（跑法：pnpm test:web）。
import { test } from "node:test";
import assert from "node:assert/strict";

import { forkView, commitSectionTitle } from "./lib/forkGuide.ts";

/** 造一个分叉信息，只覆盖关心的字段 */
function fork(over = {}) {
  return {
    ahead: 2,
    behind: 3,
    dirty: false,
    branch: "main",
    upstream: "origin/main",
    local_commits: ["aaa 本地提交"],
    remote_commits: ["bbb 云端提交"],
    blocked_reason: null,
    ...over,
  };
}

test("没有分叉也没有备份时不出卡片", () => {
  assert.equal(forkView(null, null), null);
  assert.equal(forkView(undefined, undefined), null);
  assert.equal(forkView(null, ""), null, "空串当作没有备份");
});

test("干净的分叉：给一个不带危险标记的变基动作", () => {
  const v = forkView(fork(), null);
  assert.equal(v.title, "本地与云端已分叉");
  assert.match(v.summary, /本地有 2 个提交未推送/);
  assert.match(v.summary, /云端有 3 个新提交/);
  assert.equal(v.needsStash, false);
  assert.equal(v.blocked, null);
  assert.deepEqual(
    v.actions.map((a) => [a.id, a.label, a.danger]),
    [["rebase", "备份并变基到云端", false]],
    "变基有备份兜底，不该当成危险动作再要一次确认",
  );
});

test("脏工作区：动作改成先暂存，并把这一点写进摘要", () => {
  const v = forkView(fork({ dirty: true }), null);
  assert.equal(v.needsStash, true);
  assert.match(v.summary, /工作区还有未提交变更/);
  assert.deepEqual(
    v.actions.map((a) => a.id),
    ["rebase-stash"],
  );
});

test("仓库卡在自己的变基/合并里：只解释原因，一个动作都不给", () => {
  const v = forkView(fork({ blocked_reason: "仓库正处于一次未完成的变基中" }), "backup/qingbird-git");
  assert.equal(v.blocked, "仓库正处于一次未完成的变基中");
  assert.deepEqual(v.actions, [], "半成品状态不许向导插手");
  assert.equal(forkView(fork({ blocked_reason: "x" }), null).actions.length, 0);
});

test("刚变基完（已不是分叉）：只剩撤销入口，且标记为危险动作", () => {
  const v = forkView(null, "backup/qingbird-git");
  assert.equal(v.title, "上次变基的备份还在");
  assert.equal(v.needsStash, false);
  assert.deepEqual(
    v.actions.map((a) => [a.id, a.danger]),
    [["undo", true]],
    "撤销会丢弃变基后的提交，必须二次确认",
  );
  assert.match(v.actions[0].hint, /backup\/qingbird-git/);
});

test("既分叉又有备份：两个动作都在，撤销排在后", () => {
  const v = forkView(fork(), "backup/qingbird-git");
  assert.deepEqual(
    v.actions.map((a) => a.id),
    ["rebase", "undo"],
  );
});

test("提交列表标题：空列表要点明没有，不能留白", () => {
  assert.equal(commitSectionTitle("local", 2), "本地未推送的提交（2）");
  assert.equal(commitSectionTitle("remote", 1), "云端新增的提交（1）");
  assert.equal(commitSectionTitle("local", 0), "本地没有未推送的提交");
  assert.equal(commitSectionTitle("remote", 0), "云端没有新提交");
});
