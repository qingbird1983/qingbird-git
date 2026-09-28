// 总览页的数据整形：从仓库列表 + 状态缓存算出「状态分布」与「待处理清单」。
//
// 纯逻辑，不碰 DOM、不发事件——和 src/lib/activity.ts 一个路子，回归测试在
// src/overview.test.mjs。组件只负责把这些数据画成环和列表。
//
// 分叉不再单独探测：ahead > 0 && behind > 0 就是已分叉，口径与后端 ForkInfo
// 的定义一致（src-tauri/src/commands/types.rs）。这样侧栏、⌘K、总览三处
// 共用同一个判定，不会各写一份各说各话。

import type { RepoEntry, RepoStatus } from "../api";

/** 一个仓库在总览里归到哪一档。一个仓库只归一档，优先级从上到下。 */
export type Bucket = "unknown" | "missing" | "fork" | "behind" | "ahead" | "synced";

export const BUCKET_LABEL: Record<Bucket, string> = {
  unknown: "待扫描",
  missing: "目录失联",
  fork: "已分叉",
  behind: "落后云端",
  ahead: "待推送",
  synced: "已同步",
};

/** 环的扇区顺序（从 12 点方向顺时针）。只有计数非零的档才画出来。 */
export const RING_ORDER: Bucket[] = ["synced", "behind", "ahead", "fork", "missing", "unknown"];

const RING_COLOR: Record<Bucket, string> = {
  synced: "var(--ok)",
  behind: "var(--warn)",
  ahead: "var(--accent)",
  fork: "var(--fork)",
  missing: "var(--fg3)",
  unknown: "var(--line-soft)",
};

/** 行尾那颗状态点 / 环扇区用的颜色，跟同步环、侧栏列表同一套语义色 */
export function bucketColor(bucket: Bucket): string {
  return RING_COLOR[bucket];
}

export interface OverviewRow {
  path: string;
  /** 显示名：有自定义别名用别名，否则用目录名 */
  name: string;
  bucket: Bucket;
  behind: number;
  ahead: number;
  color: string;
  /** 行右侧的说明文案 */
  note: string;
  /** 行尾动作；null 表示这一档没有能直接点一下就做完的事 */
  action: "pull" | "push" | "goto" | null;
  actionLabel: string;
}

export function bucketOf(st: RepoStatus | undefined): Bucket {
  if (!st) return "unknown";
  if (!st.is_repo) return "missing";
  if (st.behind > 0 && st.ahead > 0) return "fork";
  if (st.behind > 0) return "behind";
  if (st.ahead > 0) return "ahead";
  return "synced";
}

function rowOf(repo: RepoEntry, st: RepoStatus | undefined): OverviewRow {
  const bucket = bucketOf(st);
  const behind = st?.behind ?? 0;
  const ahead = st?.ahead ?? 0;
  const base = { path: repo.path, name: repo.alias || repo.name, bucket, behind, ahead, color: RING_COLOR[bucket] };

  switch (bucket) {
    case "behind":
      return { ...base, note: `落后 ${behind} 个提交`, action: "pull", actionLabel: "拉取" };
    case "ahead":
      return { ...base, note: `待推送 ${ahead} 个提交`, action: "push", actionLabel: "推送" };
    case "fork":
      // 分叉不给"一键"动作：备份变基会改写本地历史，只能进那个仓库的向导里点
      return { ...base, note: `已分叉 · 本地 ${ahead} / 云端 ${behind}`, action: "goto", actionLabel: "查看" };
    case "missing":
      return { ...base, note: "目录不存在或已不是 git 仓库", action: "goto", actionLabel: "查看" };
    case "unknown":
      return { ...base, note: "尚未扫描", action: null, actionLabel: "" };
    default:
      return { ...base, note: "本地与云端一致", action: null, actionLabel: "" };
  }
}

export function buildRows(repos: RepoEntry[], status: Record<string, RepoStatus>): OverviewRow[] {
  return repos.map((r) => rowOf(r, status[r.path]));
}

/** 待处理清单的排序权重：分叉最急，其次是云端有新提交，最后才是目录失联 */
const TODO_RANK: Record<Bucket, number> = {
  fork: 0,
  behind: 1,
  ahead: 2,
  missing: 3,
  synced: 9,
  unknown: 9,
};

/** 需要用户动手的档。「待扫描」和「已同步」不算待办 */
const TODO_BUCKETS: Bucket[] = ["fork", "behind", "ahead", "missing"];

export function buildTodo(rows: OverviewRow[]): OverviewRow[] {
  return rows
    .filter((r) => TODO_BUCKETS.includes(r.bucket))
    .sort((a, b) => TODO_RANK[a.bucket] - TODO_RANK[b.bucket] || b.behind + b.ahead - (a.behind + a.ahead));
}

export function countBuckets(rows: OverviewRow[]): Record<Bucket, number> {
  const counts: Record<Bucket, number> = {
    unknown: 0,
    missing: 0,
    fork: 0,
    behind: 0,
    ahead: 0,
    synced: 0,
  };
  for (const r of rows) counts[r.bucket] += 1;
  return counts;
}

const pct = (n: number) => `${Math.round(n * 100) / 100}%`;

/**
 * 状态分布环的 conic-gradient。零计数的档不占扇区（画出来是一条看不见的缝），
 * 一个仓库都没有时退回一条底色线，不要画成空心圆误导人。
 */
export function ringGradient(counts: Record<Bucket, number>, total: number): string {
  if (total <= 0) return "var(--line-soft)";
  const segs: string[] = [];
  let acc = 0;
  for (const b of RING_ORDER) {
    const n = counts[b] ?? 0;
    if (n <= 0) continue;
    const from = pct((acc / total) * 100);
    acc += n;
    segs.push(`${RING_COLOR[b]} ${from} ${pct((acc / total) * 100)}`);
  }
  return `conic-gradient(${segs.join(", ")})`;
}

export interface OverviewSummary {
  total: number;
  /** 需要动手的仓库数 */
  todo: number;
  synced: number;
  /** 落后 + 待推送的提交总数，标题行里当一句话结论 */
  commits: number;
}

export function overviewSummary(rows: OverviewRow[]): OverviewSummary {
  const counts = countBuckets(rows);
  return {
    total: rows.length,
    todo: TODO_BUCKETS.reduce((a, b) => a + counts[b], 0),
    synced: counts.synced,
    commits: rows.reduce((a, r) => a + r.behind + r.ahead, 0),
  };
}
