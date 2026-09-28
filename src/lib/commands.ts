// ⌘K 命令面板的命令表与模糊匹配（纯逻辑，不碰 store / DOM，便于 node --test 回归）。
//
// 设计取舍：
// - 查询串按空格切成多个词，**所有词都要命中**（AND）。这样 "open terminal"
//   这种带空格的输入才有意义，否则整串去匹配单条命令几乎必然失败。
// - 匹配分三档：完全相等 > 前缀 > 子序列。中文按子序列匹配很有效
//   （"终" 能命中「在此打开终端」），所以不需要额外维护拼音表。
// - keywords 是空格分隔的多个独立词，命中 keywords 比命中标题低一档，
//   这样搜 "term" 时「终端」不会盖过标题里真写着 Terminal 的条目。
// - 同分保持注册表原顺序：排序必须稳定，否则每次打开面板条目会乱跳。

export interface PaletteCommand {
  id: string;
  title: string;
  /** 分组标题，面板里渲染成分节小标题 */
  group: string;
  /** 右侧灰字提示，通常是快捷键或当前仓库的状态 */
  hint?: string;
  /** 右侧 kbd 角标，如 "Ctrl T" */
  shortcut?: string;
  /** 额外的可搜索词（空格分隔）：英文名、同义词 */
  keywords?: string;
}

/** 面板最多列出多少条：太多要滚动，太少会让"我知道功能但想不起名字"的情况直接落空 */
export const PALETTE_LIMIT = 8;

const SCORE_EXACT = 100;
const SCORE_PREFIX = 60;
const SCORE_SUB = 20;
/** 命中关键词比命中标题低这么多分 */
const KEYWORD_PENALTY = 15;

/** query 是否是 target 的子序列（顺序要对，但可以不连续） */
function isSubsequence(query: string, target: string): boolean {
  let qi = 0;
  for (let ti = 0; ti < target.length && qi < query.length; ti++) {
    if (target[ti] === query[qi]) qi++;
  }
  return qi === query.length;
}

/** 单个词对单个目标的匹配分，0 = 不匹配 */
function scoreOne(target: string, word: string): number {
  if (target === word) return SCORE_EXACT;
  if (target.startsWith(word)) return SCORE_PREFIX;
  // 单字符也要走子序列：中文按单字搜（「终」找终端、「拉」找拉取）是最自然的用法，
  // 为此禁掉单字只会把中文搜索整块废掉；多命中由下面的稳定排序去压。
  if (isSubsequence(word, target)) return SCORE_SUB;
  return 0;
}

/** 一条命令对一个词的得分：标题与各关键词里取最高 */
function scoreTerm(cmd: PaletteCommand, word: string): number {
  let best = scoreOne(cmd.title.toLowerCase(), word);
  if (cmd.keywords) {
    for (const kw of cmd.keywords.toLowerCase().split(/\s+/)) {
      if (kw) best = Math.max(best, scoreOne(kw, word) - KEYWORD_PENALTY);
    }
  }
  return best;
}

/**
 * 模糊搜索命令表。
 *
 * 空查询 = 按注册表原顺序取前 limit 条（面板打开时的默认列表）。
 * 全部词都命中才算命中（AND）；同分按原顺序稳定排序。
 *
 * 泛型保留调用方自己挂在命令上的字段（面板那边挂了 `run`），免得匹配完类型被抹平。
 */
export function matchCommands<T extends PaletteCommand>(
  cmds: readonly T[],
  query: string,
  limit: number = PALETTE_LIMIT,
): T[] {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return cmds.slice(0, limit);

  const scored: { cmd: T; score: number; order: number }[] = [];
  for (let i = 0; i < cmds.length; i++) {
    const cmd = cmds[i];
    let total = 0;
    let ok = true;
    for (const word of words) {
      const s = scoreTerm(cmd, word);
      if (s <= 0) {
        ok = false;
        break;
      }
      total += s;
    }
    if (ok) scored.push({ cmd, score: total, order: i });
  }
  scored.sort((a, b) => b.score - a.score || a.order - b.order);
  return scored.slice(0, limit).map((s) => s.cmd);
}

/**
 * 在一维列表里移动选中下标，边界处不环绕（连按 ↑ 到顶就停住）。
 * 列表为空返回 -1。
 */
export function moveSelection(current: number, delta: number, length: number): number {
  if (length <= 0) return -1;
  const next = current + delta;
  if (next < 0) return 0;
  if (next >= length) return length - 1;
  return next;
}
