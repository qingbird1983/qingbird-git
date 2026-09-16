// 运行命令面板的纯逻辑回归测试（零依赖，node 自带测试运行器 + 原生 TS 类型擦除）。
//   pnpm test:web
//
// 覆盖用户报告的 bug：保存「测试」命令会把「构建」也改成同样的值，反之亦然。
// 根因是 saveAndRun 里把四个 string 参数按位置传给后端时写反了变量，
// 现在四条命令统一走 repoCmds.ts 的按名字取值接口，这里锁住该不变式。
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  CMD_KINDS,
  cmdsFromDetect,
  cmdsFromEntry,
  cmdsToSave,
  emptyCmds,
  prefillCmds,
  withCmd,
} from "./repoCmds.ts";

/** 面板打开时四个输入框的典型内容：四条命令各不相同 */
const boxes = {
  test: "cargo test",
  build: "cargo build --release",
  dev: "cargo run",
  install: "cargo fetch",
};

test("只改一行时，另外三行的命令必须原样保留", () => {
  // 回归：过去保存「测试」会把「构建」一起改掉，反向同理
  assert.deepEqual(cmdsToSave("test", "pnpm test", boxes), {
    test: "pnpm test",
    build: "cargo build --release",
    dev: "cargo run",
    install: "cargo fetch",
  });
  assert.deepEqual(cmdsToSave("build", "pnpm run build", boxes), {
    test: "cargo test",
    build: "pnpm run build",
    dev: "cargo run",
    install: "cargo fetch",
  });
});

test("四条命令逐行改一遍，互不串位", () => {
  let cur = { ...boxes };
  for (const kind of CMD_KINDS) {
    const cmd = `cmd-${kind}`;
    const next = cmdsToSave(kind, cmd, cur);
    assert.deepEqual(next, { ...cur, [kind]: cmd }, `保存「${kind}」后应只有该行变化`);
    cur = next;
  }
  // 四行都改过之后，每行都是自己最后一次的值
  assert.deepEqual(cur, {
    test: "cmd-test",
    build: "cmd-build",
    dev: "cmd-dev",
    install: "cmd-install",
  });
});

test("落盘前去掉首尾空白", () => {
  assert.equal(cmdsToSave("dev", "  pnpm run dev  ", emptyCmds()).dev, "pnpm run dev");
});

test("已保存的命令优先，自动识别只补空行", () => {
  const saved = cmdsFromEntry({ test_cmd: "pnpm test", build_cmd: "" });
  const detected = cmdsFromDetect({
    test_cmd: "npm test",
    build_cmd: "npm run build",
    dev_cmd: "npm run dev",
    install_cmd: "npm install",
  });
  assert.deepEqual(prefillCmds(saved, detected), {
    test: "pnpm test", // 用户保存过 → 不被识别结果顶掉
    build: "npm run build", // 没保存过 → 用识别结果预填
    dev: "npm run dev",
    install: "npm install",
  });
});

test("旧配置缺 dev/install 字段时读成空串（不报错、不串到别的字段）", () => {
  assert.deepEqual(cmdsFromEntry({ test_cmd: "cargo test" }), {
    test: "cargo test",
    build: "",
    dev: "",
    install: "",
  });
  assert.deepEqual(cmdsFromEntry(undefined), emptyCmds());
});

test("withCmd 只改指定的一行", () => {
  assert.deepEqual(withCmd(boxes, "install", "pnpm install"), {
    test: "cargo test",
    build: "cargo build --release",
    dev: "cargo run",
    install: "pnpm install",
  });
});
