/**
 * 操作区自定义命令按钮（把任意命令钉住、一键跑）的纯逻辑。
 *
 * 单独抽出来是因为这里是"列表编辑 + 落盘前整理"的一套规则：上限、空行、文案兜底。
 * 写在 JSX 里没法测，而后端只做校验、不做兜底，两边规则必须一致。
 */

/** 最多钉几个，与后端 `MAX_PINNED_CMDS` 保持一致 */
export const MAX_PINNED = 3;

/** 按钮文案最长几个字符，与后端 `PINNED_LABEL_CHARS` 一致 */
const LABEL_CHARS = 12;

export interface PinnedCmd {
  label: string;
  cmd: string;
}

/**
 * 后端字段可能整段缺省（旧配置没有这个字段）。
 * 读取时请写成 `entry?.pinned_cmds ?? EMPTY_PINNED`，其中 EMPTY_PINNED 是调用方模块级的共享常量——
 * 不要在这里包一层返回 `?? []` 的函数：每次渲染都新建数组，
 * 拿它当 useEffect 依赖会让 effect 每轮都重跑。
 */

/** 还能不能再加一个 */
export function canAddPinned(list: readonly PinnedCmd[]): boolean {
  return list.length < MAX_PINNED;
}

/**
 * 命令 → 按钮文案：取前两个词（"pnpm run lint" → "pnpm run"），
 * 没有空格可切的长命令按字符截断。按字符而不是 UTF-16 单元切，中文与 emoji 都不会被切坏。
 */
export function deriveLabel(cmd: string): string {
  const words = cmd.trim().split(/\s+/).slice(0, 2).join(" ");
  return [...words].slice(0, LABEL_CHARS).join("");
}

/** 加一行空按钮；已到上限就原样返回，不做静默替换 */
export function addPinned(list: readonly PinnedCmd[]): PinnedCmd[] {
  const next = [...list];
  if (canAddPinned(list)) next.push({ label: "", cmd: "" });
  return next;
}

/** 删掉第 index 行 */
export function removePinned(list: readonly PinnedCmd[], index: number): PinnedCmd[] {
  return list.filter((_, i) => i !== index);
}

/** 改第 index 行的字段，其余行原样保留 */
export function updatePinned(
  list: readonly PinnedCmd[],
  index: number,
  patch: Partial<PinnedCmd>,
): PinnedCmd[] {
  return list.map((p, i) => (i === index ? { ...p, ...patch } : p));
}

/**
 * 落盘前整理：去掉首尾空白、丢掉没填命令的空行、文案留空则用命令兜底、最多前 MAX_PINNED 个。
 * 编辑过程中留一行空的是正常中间态，既不该报错也不该落盘。
 */
export function normalizePinned(list: readonly PinnedCmd[]): PinnedCmd[] {
  return list
    .map((p) => ({ label: p.label.trim(), cmd: p.cmd.trim() }))
    .filter((p) => p.cmd !== "")
    .map((p) => ({ label: p.label || deriveLabel(p.cmd), cmd: p.cmd }))
    .slice(0, MAX_PINNED);
}

/** 与已保存的是否一致（用来提示「未保存」） */
export function pinnedEquals(a: readonly PinnedCmd[], b: readonly PinnedCmd[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((p, i) => p.label === b[i].label && p.cmd === b[i].cmd);
}
