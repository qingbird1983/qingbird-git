// ⌘K 命令面板的模糊匹配规则测试（跑法：pnpm test:web）。
import { test } from "node:test";
import assert from "node:assert/strict";

import { PALETTE_LIMIT, matchCommands, moveSelection } from "./lib/commands.ts";

// fixture 刻意照着真实命令表的形状写：中文标题 + 英文关键词 + 分组
const CMDS = [
  { id: "pull", title: "拉取", group: "仓库操作", keywords: "pull fetch download" },
  { id: "push", title: "推送", group: "仓库操作", keywords: "push upload" },
  { id: "check", title: "检测云端更新", group: "仓库操作", keywords: "check fetch remote" },
  { id: "terminal", title: "在此打开终端", group: "仓库操作", keywords: "open terminal shell cmd" },
  { id: "folder", title: "打开文件夹", group: "仓库操作", keywords: "open folder explorer" },
  { id: "readme", title: "查看仓库说明", group: "仓库操作", keywords: "readme doc" },
  { id: "website", title: "打开仓库主页", group: "仓库操作", keywords: "open website github gitee" },
  { id: "add", title: "添加本地仓库", group: "仓库管理", keywords: "add folder local" },
  { id: "clone", title: "从地址克隆仓库", group: "仓库管理", keywords: "clone download url" },
  { id: "newrepo", title: "在云端新建仓库", group: "仓库管理", keywords: "create new github gitee" },
  { id: "scan", title: "扫描全部仓库", group: "仓库管理", keywords: "scan refresh all" },
  { id: "settings", title: "设置", group: "应用", keywords: "settings preference config" },
];

const ids = (q, limit) => matchCommands(CMDS, q, limit).map((c) => c.id);

test("空查询：按注册表原顺序取前 N 条", () => {
  assert.deepEqual(ids("", 3), ["pull", "push", "check"]);
  assert.equal(matchCommands(CMDS, "   ").length, PALETTE_LIMIT, "纯空格等于没输入");
  assert.equal(matchCommands(CMDS, "", 2).length, 2, "limit 要真的截断");
});

test("完全相等 > 前缀 > 子序列", () => {
  assert.equal(ids("推送")[0], "push", "完全相等排最前");
  assert.equal(ids("检测")[0], "check", "前缀命中");
  assert.ok(ids("检测").includes("check"));
});

test("中文按子序列匹配：单个字也能命中长标题", () => {
  assert.equal(ids("终")[0], "terminal", "「终」→ 在此打开终端");
  assert.ok(ids("文").includes("folder"), "「文」→ 打开文件夹");
  assert.ok(ids("明").includes("readme"), "「明」→ 查看仓库说明");
  assert.ok(ids("拉").includes("pull"), "「拉」→ 拉取");
});

test("命中关键词：搜英文也能找到中文标题", () => {
  assert.equal(ids("terminal")[0], "terminal");
  assert.ok(ids("github").includes("website"));
  assert.ok(ids("github").includes("newrepo"), "两个条目都带 github 关键词");
});

test("多词是 AND：每个词都要命中", () => {
  const r = ids("open terminal");
  assert.deepEqual(r, ["terminal"], "只有终端同时命中 open + terminal");
  assert.deepEqual(ids("open nonsense"), [], "有一个词不命中就整条淘汰");
  assert.ok(ids("open folder").includes("folder"));
});

test("命中标题的排在命中关键词的前面", () => {
  // 「clone」既是某条命令的关键词，也可能是别的标题的一部分，排序必须稳定可预期
  const r = ids("push");
  assert.equal(r[0], "push", "标题含 push 的那条排最前");
});

test("全不命中返回空数组，不是回退成全量列表", () => {
  assert.deepEqual(ids("zzzz不存在"), []);
});

test("同分保持注册表原顺序", () => {
  // github 同时出现在 website / newrepo 的关键词里，分数相同，必须按注册顺序
  assert.deepEqual(ids("github"), ["website", "newrepo"]);
});

test("大小写不敏感", () => {
  assert.deepEqual(ids("TERMINAL"), ids("terminal"));
});

test("moveSelection：边界不环绕，空列表安全", () => {
  assert.equal(moveSelection(0, -1, 5), 0, "已在顶部再按上，停在顶部");
  assert.equal(moveSelection(4, 1, 5), 4, "已在底部再按下，停在底部");
  assert.equal(moveSelection(1, 1, 5), 2);
  assert.equal(moveSelection(0, -1, 0), -1, "空列表");
  assert.equal(moveSelection(-1, 1, 3), 0, "还没选中时向下 → 第一项");
});
