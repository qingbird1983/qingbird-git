// 提交历史看 diff 的展示规则测试（跑法：pnpm test:web）。
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  MAX_SHOWN_LINES,
  changeLabel,
  patchLineKind,
  visiblePatchLines,
} from "./lib/commitDiff.ts";

test("patch 行分类：文件头不能被当成增删行", () => {
  assert.equal(patchLineKind("+++ b/src/main.rs"), "meta", "文件头优先于 +");
  assert.equal(patchLineKind("--- a/src/main.rs"), "meta", "文件头优先于 -");
  assert.equal(patchLineKind("+let x = 1;"), "add");
  assert.equal(patchLineKind("-let x = 0;"), "del");
  assert.equal(patchLineKind("@@ -1,3 +1,4 @@"), "hunk");
  assert.equal(patchLineKind("diff --git a/x b/x"), "meta");
  assert.equal(patchLineKind("new file mode 100644"), "meta");
  assert.equal(patchLineKind("Binary files a/x and b/x differ"), "meta");
  assert.equal(patchLineKind(" context line"), "ctx");
  assert.equal(patchLineKind(""), "ctx");
});

test("行数展示：二进制文件不显示成 +0 −0", () => {
  assert.equal(changeLabel({ path: "a.png", additions: null, deletions: null }), "二进制");
  assert.equal(changeLabel({ path: "a.rs", additions: 12, deletions: 3 }), "+12 −3");
  assert.equal(changeLabel({ path: "a.rs", additions: 0, deletions: 7 }), "+0 −7");
  // 只有一边是 null（理论上不该出现）时也要当二进制，不能拼出 +null
  assert.equal(changeLabel({ path: "a.png", additions: null, deletions: 3 }), "二进制");
});

test("patch 行数超过上限时只渲染前若干行，并报出隐藏了多少", () => {
  const small = visiblePatchLines("a\nb\nc");
  assert.deepEqual(small, { lines: ["a", "b", "c"], hidden: 0 });

  const many = Array.from({ length: MAX_SHOWN_LINES + 25 }, (_, i) => `line${i}`).join("\n");
  const got = visiblePatchLines(many);
  assert.equal(got.lines.length, MAX_SHOWN_LINES);
  assert.equal(got.hidden, 25, "隐藏行数要如实报出来，不能默默少显示");
  assert.equal(got.lines[0], "line0");
  assert.equal(got.lines[MAX_SHOWN_LINES - 1], `line${MAX_SHOWN_LINES - 1}`);

  // 刚好到上限：不截断
  const exact = Array.from({ length: MAX_SHOWN_LINES }, (_, i) => `l${i}`).join("\n");
  assert.equal(visiblePatchLines(exact).hidden, 0);
});
