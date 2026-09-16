// 操作区自定义命令按钮的纯逻辑测试（跑法：pnpm test:web）。
// 这些规则必须与后端 run_cmd.rs 的 sanitize_pinned / derive_pinned_label 一致：
// 上限 3 个、空命令的行丢掉、文案留空时用命令前两个词兜底。
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  MAX_PINNED,
  addPinned,
  canAddPinned,
  deriveLabel,
  normalizePinned,
  pinnedEquals,
  removePinned,
  updatePinned,
} from "./lib/pinnedCmds.ts";

const cmd = (label, c) => ({ label, cmd: c });

test("最多钉 3 个，到上限后加不出来", () => {
  let list = [];
  assert.equal(canAddPinned(list), true);
  for (let i = 0; i < MAX_PINNED; i++) list = addPinned(list);
  assert.equal(list.length, MAX_PINNED);
  assert.equal(canAddPinned(list), false);
  assert.equal(addPinned(list).length, MAX_PINNED, "到上限再加不该有任何变化");
});

test("改名 / 改命令只动那一行，其余原样", () => {
  const list = [cmd("检查", "pnpm lint"), cmd("格式化", "pnpm fmt"), cmd("测试", "pnpm test")];
  const one = updatePinned(list, 1, { cmd: "pnpm prettier -w ." });
  assert.deepEqual(one[1], cmd("格式化", "pnpm prettier -w ."));
  assert.deepEqual(one[0], list[0]);
  assert.deepEqual(one[2], list[2]);

  const removed = removePinned(list, 0);
  assert.deepEqual(removed, [list[1], list[2]], "删掉的是指定那一行");
  assert.equal(list.length, 3, "原数组不该被改动");
});

test("落盘前丢空行、去空白、文案兜底、截到上限", () => {
  const got = normalizePinned([
    cmd("  检查  ", "  pnpm lint  "),
    cmd("空的", "   "),
    cmd("", "cargo clippy --all"),
  ]);
  assert.deepEqual(got, [cmd("检查", "pnpm lint"), cmd("cargo clippy", "cargo clippy --all")]);

  const many = normalizePinned([cmd("a", "c1"), cmd("b", "c2"), cmd("c", "c3"), cmd("d", "c4")]);
  assert.equal(many.length, MAX_PINNED, "超上限按前几个截断");
});

test("文案兜底取命令前两个词，长命令按字符截断", () => {
  assert.equal(deriveLabel("pnpm run lint"), "pnpm run");
  assert.equal(deriveLabel("  cargo   clippy  "), "cargo clippy");
  assert.equal(deriveLabel("make"), "make");
  // 中文按字符切，不切半个字
  assert.equal(deriveLabel("一个非常非常长的中文命令名占位"), "一个非常非常长的中文命令");
  assert.equal([...deriveLabel("abcdefghijklmnop")].length, 12);
});

test("未保存判定：顺序与内容都要一致", () => {
  const saved = [cmd("检查", "pnpm lint")];
  assert.equal(pinnedEquals(saved, [cmd("检查", "pnpm lint")]), true);
  assert.equal(pinnedEquals(saved, [cmd("检查", "pnpm lint2")]), false);
  assert.equal(pinnedEquals(saved, []), false);
  assert.equal(pinnedEquals([], []), true);
  assert.equal(
    pinnedEquals(
      [cmd("检查", "a"), cmd("测试", "b")],
      [cmd("测试", "b"), cmd("检查", "a")],
    ),
    false,
    "按钮顺序变了就是变了，不能当成一样",
  );
});
