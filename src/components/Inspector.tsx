// 检视抽屉：主面板右侧的常驻页签容器，承接全部「查看类」功能。
//
// 原来的做法是 8 个只靠 tooltip 认的图标按钮各挂一个浮层，
// 现在收成「1 个开关 + 4 个页签」：变更 / 历史 / 定时 / 空间。
// 页签内容全部是内嵌的（不是浮层），所以不需要选锚点、不怕互相遮挡。
import { useCallback, useEffect, useState } from "react";
import { api, type CommitInfo, type FileChange, type SpaceReport } from "../api";
import { AUTO_SYNC_DAILY, AUTO_SYNC_OPTIONS } from "../lib/autoSync";
import HistoryModal from "./HistoryModal";
import SpacePanel from "./SpacePanel";
import { IconClock, IconDiff, IconDisk, IconHistory, IconX } from "./icons";

export type InspTab = "changes" | "history" | "auto" | "space";

interface Props {
  selected: string;
  tab: InspTab;
  onTab: (t: InspTab) => void;
  onClose: () => void;
  /** 未提交变更数量，用于页签上的角标 */
  changeCount: number;
  /** 变更页签需要的受控值（提交说明由父组件持有，成功后清空） */
  files: FileChange[];
  commitMsg: string;
  netBusy: boolean;
  onCommitMsg: (v: string) => void;
  onCommitSubmit: () => void;
  /** 定时同步设置 */
  autoMins: number;
  autoDaily: string;
  autoPull: boolean;
  lastCheckText: string | null;
  lastResult: string;
  onAutoChange: (mins: number, autoPull: boolean, daily: string) => void;
  /** 最近一次拉取带回的提交摘要（历史页顶部单独高亮） */
  syncCommits: string[];
  /** 最近一次拉取确实带回过东西（哪怕只有文件变更、没有新提交） */
  hasSync: boolean;
  /** 远端托管平台，用于「本次同步内容」的来源标注 */
  syncHost: "gitee" | "github" | "other" | null;
}

export default function Inspector(p: Props) {
  const [space, setSpace] = useState<SpaceReport | null>(null);
  const [spaceErr, setSpaceErr] = useState("");
  const [spaceLoading, setSpaceLoading] = useState(false);
  const [commits, setCommits] = useState<CommitInfo[] | null>(null);
  const [showHistory, setShowHistory] = useState(false);

  const scanSpace = useCallback(async (path: string) => {
    setSpaceLoading(true);
    setSpaceErr("");
    try {
      setSpace(await api.scanRepoSpace(path));
    } catch (e) {
      setSpaceErr(String(e));
      setSpace(null);
    } finally {
      setSpaceLoading(false);
    }
  }, []);

  // 切仓库清空所有按仓库缓存的页签内容，避免看到上一个仓库的残留
  useEffect(() => {
    setSpace(null);
    setCommits(null);
    void scanSpace(p.selected);
  }, [p.selected, scanSpace]);

  // 历史页按需拉取（第一次进这个页签才请求）
  useEffect(() => {
    if (p.tab !== "history" || commits) return;
    void api.recentCommits(p.selected).then(setCommits).catch(() => setCommits([]));
  }, [p.tab, p.selected, commits]);

  const autoOn = p.autoMins > 0 || !!p.autoDaily;
  const applyMins = (next: number) => {
    if (next === AUTO_SYNC_DAILY) p.onAutoChange(0, p.autoPull, p.autoDaily || "09:00");
    else p.onAutoChange(next, p.autoPull, "");
  };

  return (
    <aside className="inspector">
      <div className="insp-tabs">
        <button
          className={`insp-tab${p.tab === "changes" ? " on" : ""}`}
          onClick={() => p.onTab("changes")}
          title="待提交变更"
        >
          <IconDiff size={13} />变更
          {p.changeCount > 0 && <span className="insp-badge">{p.changeCount}</span>}
        </button>
        <button
          className={`insp-tab${p.tab === "history" ? " on" : ""}`}
          onClick={() => p.onTab("history")}
          title="提交历史"
        >
          <IconHistory size={13} />历史
        </button>
        <button
          className={`insp-tab${p.tab === "auto" ? " on" : ""}${autoOn ? " has-on" : ""}`}
          onClick={() => p.onTab("auto")}
          title="定时自动同步"
        >
          <IconClock size={13} />定时
        </button>
        <button
          className={`insp-tab${p.tab === "space" ? " on" : ""}`}
          onClick={() => p.onTab("space")}
          title="磁盘占用与缓存清理"
        >
          <IconDisk size={13} />空间
        </button>
        <button className="insp-close" title="收起检视栏" onClick={p.onClose}>
          <IconX size={13} />
        </button>
      </div>

      <div className="insp-body">
        {/* ---------- 变更 ---------- */}
        {p.tab === "changes" && (
          <>
            {p.files.length === 0 ? (
              <div className="insp-empty">没有待提交的变更</div>
            ) : (
              <div className="file-list">
                {p.files.slice(0, 20).map((f, i) => (
                  <div key={i} className="file-line">
                    <span className={`file-code file-${f.code.replace("?", "u")}`}>{f.code}</span>
                    <span className="file-path">{f.path}</span>
                  </div>
                ))}
                {p.files.length > 20 && (
                  <div className="commit-more">…还有 {p.files.length - 20} 个文件</div>
                )}
              </div>
            )}
            <input
              className="modal-input insp-input"
              placeholder="提交说明，如：修复登录问题"
              value={p.commitMsg}
              disabled={p.netBusy}
              onChange={(e) => p.onCommitMsg(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && p.commitMsg.trim()) void p.onCommitSubmit();
              }}
            />
            <button
              className="btn-sm insp-submit"
              disabled={p.netBusy || !p.commitMsg.trim()}
              onClick={() => void p.onCommitSubmit()}
            >
              提交并推送
            </button>
          </>
        )}

        {/* ---------- 历史 ---------- */}
        {p.tab === "history" && (
          <>
            {p.hasSync && (
              <div className="insp-sync-block">
                <div className="insp-block-hd">
                  <span>
                    本次同步内容
                    {p.syncHost === "gitee"
                      ? "（来自 Gitee）"
                      : p.syncHost === "github"
                        ? "（来自 GitHub）"
                        : ""}
                  </span>
                  <span className="insp-block-n">{p.syncCommits.length} 个新提交</span>
                </div>
                {p.syncCommits.length === 0 ? (
                  <div className="insp-block-hint">本次没有新提交（可能只是文件变更同步）</div>
                ) : (
                  p.syncCommits.map((c, i) => (
                    <div key={i} className="sync-commit-line">
                      {c}
                    </div>
                  ))
                )}
                <div className="insp-block-hint">文件变更明细见操作日志</div>
              </div>
            )}
            {commits === null && <div className="insp-empty">正在读取提交…</div>}
            {commits?.length === 0 && <div className="insp-empty">还没有提交</div>}
            {commits?.map((c) => (
              <div key={c.hash} className="insp-commit">
                <span className="insp-hash mono">{c.hash.slice(0, 7)}</span>
                <div>
                  <div className="insp-sum">{c.summary}</div>
                  <div className="insp-meta">
                    {c.author} · {c.date}
                  </div>
                </div>
              </div>
            ))}
            {commits !== null && commits.length > 0 && (
              <button className="btn-sm insp-more" onClick={() => setShowHistory(true)}>
                查看完整历史与 diff
              </button>
            )}
          </>
        )}

        {/* ---------- 定时 ---------- */}
        {p.tab === "auto" && (
          <>
            <p className="auto-sync-desc">
              在后台按间隔检测云端更新（自动走直连/代理回退），仅对此仓库生效；
              只做快进合并（<span className="mono">--ff-only</span>），
              本地分叉或有未提交变更时不会动你的代码。
            </p>
            <div className="segmented auto-sync-seg">
              {AUTO_SYNC_OPTIONS.map((o) => (
                <button
                  key={o.mins}
                  className={`segment ${
                    (o.mins === AUTO_SYNC_DAILY ? !!p.autoDaily : p.autoMins === o.mins)
                      ? "segment-active"
                      : ""
                  }`}
                  onClick={() => applyMins(o.mins)}
                >
                  {o.label}
                </button>
              ))}
            </div>
            {p.autoDaily && (
              <div className="auto-sync-daily-row">
                <span>每天执行时刻</span>
                <input
                  type="time"
                  className="auto-sync-time"
                  value={p.autoDaily}
                  onChange={(e) => void p.onAutoChange(0, p.autoPull, e.target.value)}
                />
              </div>
            )}
            <label className="switch-row auto-sync-switch">
              <span>检测到更新后自动拉取</span>
              <button
                className={`switch ${p.autoPull ? "on" : ""}`}
                role="switch"
                aria-checked={p.autoPull}
                title={autoOn ? undefined : "请先选择间隔或每天定时开启"}
                onClick={() => {
                  if (autoOn) void p.onAutoChange(p.autoMins, !p.autoPull, p.autoDaily);
                }}
              >
                <span className="switch-knob" />
              </button>
            </label>
            <div className="auto-sync-meta">
              {p.lastCheckText
                ? `上次自动检查：${p.lastCheckText}${p.lastResult ? ` · ${p.lastResult}` : ""}`
                : autoOn
                  ? "已开启，稍后自动进行首次检查"
                  : "尚未开启，选择间隔或每天定时即可启用"}
            </div>
          </>
        )}

        {/* ---------- 空间 ---------- */}
        {p.tab === "space" && (
          <SpacePanel
            selected={p.selected}
            report={space}
            error={spaceErr}
            loading={spaceLoading}
            onRescan={() => void scanSpace(p.selected)}
          />
        )}
      </div>

      {showHistory && <HistoryModal onClose={() => setShowHistory(false)} />}
    </aside>
  );
}
