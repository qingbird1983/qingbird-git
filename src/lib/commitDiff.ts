/**
 * 提交历史看 diff 的展示规则（纯逻辑，配 `src/commitDiff.test.mjs`）。
 *
 * 这里只放"怎么显示"，不放"怎么取"——取数在 history.rs 与 store/api 里。
 */
import type { CommitFile } from "../api";

/** 一个 patch 最多渲染多少行：几千个 div 足够把 webview 卡住 */
export const MAX_SHOWN_LINES = 1500;

/** 变更行的分类，用于给 patch 上色 */
export type PatchLineKind = "add" | "del" | "hunk" | "meta" | "ctx";

/**
 * 按行首特征判断这一行是什么。
 * 顺序要紧：`+++` / `---` 是文件头，必须先于增删行判断，否则会被染成增删色。
 */
export function patchLineKind(line: string): PatchLineKind {
  if (line.startsWith("@@")) return "hunk";
  if (line.startsWith("+++") || line.startsWith("---")) return "meta";
  if (line.startsWith("+")) return "add";
  if (line.startsWith("-")) return "del";
  if (
    /^(diff |index |new file|deleted file|similarity index|rename |old mode|new mode|Binary files)/.test(
      line,
    )
  ) {
    return "meta";
  }
  return "ctx";
}

/** 行数增减的展示。二进制文件没有行数，要说清楚，而不是显示成 +0 −0 */
export function changeLabel(f: CommitFile): string {
  if (f.additions === null || f.deletions === null) return "二进制";
  return `+${f.additions} −${f.deletions}`;
}

/**
 * 只取前 MAX_SHOWN_LINES 行渲染，并告诉调用方还剩多少行没显示。
 * 剩了就得给出「在终端查看完整 diff」，不能默默少显示。
 */
export function visiblePatchLines(text: string): { lines: string[]; hidden: number } {
  const all = text.split("\n");
  if (all.length <= MAX_SHOWN_LINES) return { lines: all, hidden: 0 };
  return { lines: all.slice(0, MAX_SHOWN_LINES), hidden: all.length - MAX_SHOWN_LINES };
}
