// 总览：一屏回答「所有仓库里有什么要我做」。
//
// 两张卡，不多不少：
//   状态分布环 —— 全库各档的占比，圆心是仓库总数
//   待处理列表 —— 只列需要动手的，排序规则在 src/lib/overview.ts
// 跨仓库磁盘条这一轮没做：扫盘要把每个仓库的目录树走一遍，是全应用最慢的操作，
// 而它并不回答"有什么要做"。同一份数据在检视抽屉的空间页里还有清理动作。
//
// 本屏只读本地 git 状态（scan_all 走 read_status，不联网），所以进页面可以自动补扫一次。

import { useEffect, useMemo, useState } from "react";
import { useApp } from "../store";
import {
  BUCKET_LABEL,
  RING_ORDER,
  bucketColor,
  buildRows,
  buildTodo,
  countBuckets,
  overviewSummary,
  ringGradient,
  type OverviewRow,
} from "../lib/overview";
import { IconCloudDown, IconCloudUp, IconSync } from "./icons";

const RING = 96;

function clockOf(ts: number): string {
  return new Date(ts).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false });
}

export default function Overview() {
  const repos = useApp((s) => s.repos);
  const status = useApp((s) => s.status);
  const selected = useApp((s) => s.selected);
  const busy = useApp((s) => s.busy);
  const cloning = useApp((s) => s.cloning);
  const scanAll = useApp((s) => s.scanAll);
  const pullFor = useApp((s) => s.pullFor);
  const pushFor = useApp((s) => s.pushFor);
  const select = useApp((s) => s.select);
  const setView = useApp((s) => s.setView);
  const log = useApp((s) => s.log);
  /** 正在跑动作的那一行：全局 busy 不带仓库名，总览上得自己知道是哪一行 */
  const [running, setRunning] = useState<string | null>(null);
  const [lastScan, setLastScan] = useState<number | null>(null);

  const rows = useMemo(() => buildRows(repos, status), [repos, status]);
  const todo = useMemo(() => buildTodo(rows), [rows]);
  const counts = useMemo(() => countBuckets(rows), [rows]);
  const sum = useMemo(() => overviewSummary(rows), [rows]);

  // 状态缓存不全（第一次进 / 刚加了仓库）就补扫一次。scan_all 是纯本地读，不联网。
  useEffect(() => {
    if (repos.length === 0 || busy) return;
    if (repos.every((r) => status[r.path])) return;
    void scanAll().then(() => setLastScan(Date.now()));
  }, [repos, status, busy, scanAll]);

  const rescan = async () => {
    await scanAll();
    setLastScan(Date.now());
  };

  const openRepo = (path: string) => {
    select(path);
    setView("repos");
  };

  // 行尾动作。分叉与失联不给一键操作——变基会改写本地历史，只能进仓库里点向导
  const run = async (row: OverviewRow) => {
    if (row.action === "goto") {
      openRepo(row.path);
      return;
    }
    setRunning(row.path);
    try {
      if (row.action === "pull") {
        // 不替用户决定要不要暂存：脏工作区就停下，让他切过去自己确认
        if ((await pullFor(row.path, false)) === "dirty") {
          log(`「${row.name}」有未提交改动，拉取已停下；切到该仓库确认暂存后再拉`, "error", row.path);
        }
      } else if (row.action === "push") {
        await pushFor(row.path);
      }
    } finally {
      setRunning(null);
    }
  };

  // 网络操作在后端是进程级互斥（OP_LOCK），同一时刻只可能有一个在跑
  const locked = !!busy || !!cloning || !!running;
  const legend = RING_ORDER.filter((b) => counts[b] > 0);

  return (
    <main className="panel ov-panel">
      <div className="panel-header card">
        <div className="panel-title">
          <h2>总览</h2>
          <span className="panel-path">
            {sum.total === 0
              ? "还没有添加仓库"
              : `${sum.total} 个仓库 · ${sum.todo > 0 ? `${sum.todo} 项待处理` : "全部已同步"}` +
                (sum.commits > 0 ? ` · 相差 ${sum.commits} 个提交` : "")}
          </span>
        </div>
      </div>

      <div className="ov-body">
        {/* ---------- 状态分布：一个环说完全库构成 ---------- */}
        <section className="ov-card card ov-dist">
          <div className="ov-card-hd">
            <span className="ov-card-t">状态分布</span>
            {lastScan && <span className="ov-card-x">扫描于 {clockOf(lastScan)}</span>}
          </div>
          <div className="ov-ring-box">
            <div className="ov-ring" style={{ width: RING, height: RING, background: ringGradient(counts, sum.total) }}>
              <div className="ov-ring-in">
                <div className="ov-ring-n">{sum.total}</div>
                <div className="ov-ring-l">仓库</div>
              </div>
            </div>
          </div>
          {legend.length === 0 ? (
            <div className="ov-empty">添加仓库后这里会显示各状态占比</div>
          ) : (
            <ul className="ov-legend">
              {legend.map((b) => (
                <li key={b}>
                  <span className="ov-dot" style={{ background: bucketColor(b) }} aria-hidden />
                  <span className="ov-legend-l">{BUCKET_LABEL[b]}</span>
                  <b className="ov-legend-n">{counts[b]}</b>
                </li>
              ))}
            </ul>
          )}
        </section>

        {/* ---------- 待处理：只列要动手的，点一下就做 ---------- */}
        <section className="ov-card card ov-todo">
          <div className="ov-card-hd">
            <span className="ov-card-t">待处理</span>
            <span className="ov-card-x">{todo.length} 项</span>
          </div>
          {todo.length === 0 ? (
            <div className="ov-done">
              {/* 空列表时别挂那个绿勾：还没有仓库不是"全部搞定" */}
              {repos.length > 0 && <span className="ov-done-tick">✓</span>}
              {repos.length === 0 ? "还没有仓库" : "所有仓库都是最新状态"}
            </div>
          ) : (
            <div className="ov-rows">
              {todo.map((row) => (
                <div
                  key={row.path}
                  className={`ov-row${selected === row.path ? " on" : ""}`}
                  onClick={() => openRepo(row.path)}
                  title={row.path}
                >
                  <span className="ov-dot" style={{ background: row.color }} aria-hidden />
                  <span className="ov-name">{row.name}</span>
                  <span className="ov-note">{row.note}</span>
                  {row.action && (
                    <button
                      className={`ov-act${row.action === "pull" ? " acc" : ""}`}
                      disabled={locked}
                      onClick={(e) => {
                        e.stopPropagation();
                        void run(row);
                      }}
                    >
                      {row.action === "pull" && <IconCloudDown size={12} />}
                      {row.action === "push" && <IconCloudUp size={12} />}
                      {running === row.path ? "处理中" : row.actionLabel}
                    </button>
                  )}
                </div>
              ))}
            </div>
          )}
        </section>
      </div>

      <div className="status-bar">
        <span className={`sb-dot${busy ? " on" : ""}`} />
        <span className="sb-text">
          {busy ? busy.replace(/…$/, "") : lastScan ? `全量扫描于 ${clockOf(lastScan)}` : "尚未扫描"}
        </span>
        <button className="sb-log" onClick={() => void rescan()} disabled={locked}>
          <IconSync size={12} />
          重新扫描
        </button>
      </div>
    </main>
  );
}
