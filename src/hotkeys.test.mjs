// 全局快捷键的判定规则测试（跑法：pnpm test:web）。
import { test } from "node:test";
import assert from "node:assert/strict";

import { resolveHotkey, stepRepoIndex } from "./lib/hotkeys.ts";

/** 造一个 keydown 事件；只写关心的字段，其余按"裸键"处理 */
const ev = (key, mods = {}) => ({
  key,
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  altKey: false,
  ...mods,
});
const typing = { inInput: true, hasRepo: true };
const idle = { inInput: false, hasRepo: true };
const noRepo = { inInput: false, hasRepo: false };

test("Ctrl K 任何时候都开面板，连输入框里也生效", () => {
  assert.equal(resolveHotkey(ev("k", { ctrlKey: true }), typing), "palette");
  assert.equal(resolveHotkey(ev("k", { ctrlKey: true }), idle), "palette");
  assert.equal(resolveHotkey(ev("K", { ctrlKey: true }), idle), "palette", "大写 K 一样");
  assert.equal(resolveHotkey(ev("k", { metaKey: true }), idle), "palette", "Cmd K 等价");
});

test("其余键位在输入框里全部让位给打字", () => {
  assert.equal(resolveHotkey(ev("1"), typing), null, "按 1 是要打 1");
  assert.equal(resolveHotkey(ev("t", { ctrlKey: true }), typing), null);
  assert.equal(resolveHotkey(ev("ArrowDown"), typing), null);
});

test("1/2/3 对应同步卡上从左到右：拉取 / 推送 / 检测", () => {
  assert.equal(resolveHotkey(ev("1"), idle), "pull");
  assert.equal(resolveHotkey(ev("2"), idle), "push");
  assert.equal(resolveHotkey(ev("3"), idle), "check");
});

test("带修饰键的 1 不是三键（Shift+1 是感叹号）", () => {
  assert.equal(resolveHotkey(ev("1", { shiftKey: true }), idle), null);
  assert.equal(resolveHotkey(ev("1", { altKey: true }), idle), null);
  assert.equal(resolveHotkey(ev("4"), idle), null, "只绑 1/2/3");
});

test("Ctrl T 开终端，但得有选中仓库", () => {
  assert.equal(resolveHotkey(ev("t", { ctrlKey: true }), idle), "terminal");
  assert.equal(resolveHotkey(ev("t", { ctrlKey: true }), noRepo), null);
});

test("方向键上下切仓库", () => {
  assert.equal(resolveHotkey(ev("ArrowDown"), idle), "nextRepo");
  assert.equal(resolveHotkey(ev("ArrowUp"), idle), "prevRepo");
  assert.equal(resolveHotkey(ev("ArrowDown"), noRepo), null, "没仓库就没得切");
});

test("没有选中仓库时，三键与方向键都不触发", () => {
  for (const k of ["1", "2", "3", "ArrowUp", "ArrowDown"]) {
    assert.equal(resolveHotkey(ev(k), noRepo), null, `${k} 应被忽略`);
  }
});

test("没绑定的键返回 null，不误伤正常输入", () => {
  for (const k of ["a", "5", "Enter", "Tab", "Escape", "F5", " "]) {
    assert.equal(resolveHotkey(ev(k), idle), null, `${k} 不该有动作`);
  }
});

test("stepRepoIndex：到头停在头，尚未选中时从两端起算", () => {
  assert.equal(stepRepoIndex(0, -1, 3), 0, "第一项再按上还是第一项");
  assert.equal(stepRepoIndex(2, 1, 3), 2, "最后一项再按下还是最后一项");
  assert.equal(stepRepoIndex(0, 1, 3), 1);
  assert.equal(stepRepoIndex(-1, 1, 3), 0, "没选中时向下 → 第一项");
  assert.equal(stepRepoIndex(-1, -1, 3), 0, "没选中时向上 → 也落到第一项，不越界");
  assert.equal(stepRepoIndex(0, 1, 0), -1, "空列表");
});
