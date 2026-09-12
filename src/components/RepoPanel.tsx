import { useEffect, useRef, useState } from "react";
import { api, remoteHost, type FileChange, type LangStat } from "../api";
import { useApp } from "../store";
import HistoryModal from "./HistoryModal";
import {
  IconCloud, IconCloudDown, IconCloudUp,
  IconFolder, IconTerminal, IconHistory, IconDiff, IconX,
} from "./icons";

/** 常见语言的显示颜色（对齐 GitHub Linguist 配色） */
const LANG_COLORS: Record<string, string> = {
  Rust: "#dea584",
  TypeScript: "#3178c6",
  JavaScript: "#f1e05a",
  Python: "#3572A5",
  Go: "#00ADD8",
  Java: "#b07219",
  Kotlin: "#A97BFF",
  Swift: "#F05138",
  Ruby: "#701516",
  PHP: "#4F5D95",
  C: "#555555",
  "C++": "#f34b7d",
  "C#": "#178600",
  "Objective-C": "#438eff",
  Scala: "#c22d40",
  Dart: "#00B4AB",
  Lua: "#000080",
  HTML: "#e34c26",
  Vue: "#41b883",
  Svelte: "#ff3e00",
  CSS: "#663399",
  SCSS: "#c6538c",
  Less: "#1d365d",
  Shell: "#89e051",
  PowerShell: "#012456",
  SQL: "#e38c00",
  Zig: "#ec915c",
  Solidity: "#AA6746",
};

function langColor(name: string): string {
  return LANG_COLORS[name] ?? (name === "其他" ? "#8b949e" : "#6e7681");
}

export default function RepoPanel() {
  const {
    selected, status, busy, progress, logs, changes,
    checkUpdate, pull, push, commitPush, refreshStatus, openInFolder, openInTerminal,
  } = useApp();
  const [confirmStash, setConfirmStash] = useState(false);
  const [fileChanges, setFileChanges] = useState<FileChange[]>([]);
  const [commitMsg, setCommitMsg] = useState("");
  const [showHistory, setShowHistory] = useState(false);
  const [showChanges, setShowChanges] = useState(false);
  const [langs, setLangs] = useState<LangStat[]>([]);
  const repos = useApp((s) => s.repos);
  const logRef = useRef<HTMLDivElement>(null);

  const st = selected ? status[selected] : undefined;
  const update = selected ? changes[selected] : undefined;
  const repoEntry = selected ? repos.find((r) => r.path === selected) : undefined;
  const displayName = repoEntry?.alias || selected?.split(/[\\/]/).pop() || "";

  useEffect(() => {
    setConfirmStash(false);
    setCommitMsg("");
    setFileChanges([]);
    setLangs([]);
    setShowChanges(false);
  }, [selected]);

  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [logs]);

  // 选中仓库后拉取语言构成
  useEffect(() => {
    if (selected && st?.is_repo) {
      void api.languages(selected).then(setLangs).catch(() => setLangs([]));
    } else {
      setLangs([]);
    }
  }, [selected, st?.is_repo]);

  // 有未提交变更时拉取变更文件列表
  useEffect(() => {
    if (selected && st?.dirty) {
      void api.listChanges(selected).then(setFileChanges).catch(() => setFileChanges([]));
    } else {
      setFileChanges([]);
      setShowChanges(false);
    }
  }, [selected, st?.dirty]);

  const handlePull = async () => {
    const result = await pull(false);
    if (result === "dirty") setConfirmStash(true);
  };

  const handleStashPull = async () => {
    setConfirmStash(false);
    await pull(true);
  };

  const handleCommitPush = async () => {
    if (!commitMsg.trim()) return;
    const ok = await commitPush(commitMsg.trim());
    if (ok) {
      setCommitMsg("");
      setShowChanges(false);
    }
  };

  if (!selected) {
    return (
      <main className="panel panel-empty">
        <div className="empty-big">
          <div className="empty-logo">git</div>
          选择或添加一个仓库开始使用
          <div className="empty-sub">支持本地文件夹、拖拽添加、粘贴地址克隆、一键新建仓库（GitHub / Gitee）</div>
        </div>
      </main>
    );
  }

  const host = st ? remoteHost(st.remote_url) : null;
  const progressPct = progress?.percent != null ? Math.min(100, progress.percent) : null;

  return (
    <main className="panel">
      <div className="panel-header card">
        <div className="panel-title">
          <h2>
            {displayName}
            {repoEntry?.alias && (
              <span className="alias-sub" title="本地自定义显示名">
                {selected.split(/[\\/]/).pop()}
              </span>
            )}
          </h2>
          <span className="panel-path" title={selected}>
            {selected}
          </span>
        </div>
        <div className="panel-status">
          <div className="quick-actions">
            <button className="icon-btn" title="打开文件夹" onClick={() => void openInFolder(selected)}>
              <IconFolder size={15} />
            </button>
            <button className="icon-btn" title="在此文件夹打开终端" onClick={() => void openInTerminal(selected)}>
              <IconTerminal size={15} />
            </button>
            <button className="icon-btn" title="提交历史" onClick={() => setShowHistory(true)}>
              <IconHistory size={15} />
            </button>
            {st?.is_repo && st.dirty && (
              <span className="changes-anchor">
                <button
                  className={`icon-btn has-badge ${showChanges ? "icon-btn-active" : ""}`}
                  title={`待提交变更（${fileChanges.length} 个文件）`}
                  onClick={() => setShowChanges((v) => !v)}
                >
                  <IconDiff size={15} />
                  {fileChanges.length > 0 && <span className="btn-badge">{fileChanges.length}</span>}
                </button>
                {showChanges && (
                  <div className="commit-popover">
                    <div className="update-header">
                      <span className="commit-title">待提交变更（{fileChanges.length} 个文件）</span>
                      <button className="popover-close" title="关闭" onClick={() => setShowChanges(false)}>
                        <IconX size={13} />
                      </button>
                    </div>
                    {fileChanges.length === 0 ? (
                      <div className="commit-more">正在读取变更列表…</div>
                    ) : (
                      <div className="file-list">
                        {fileChanges.slice(0, 8).map((f, i) => (
                          <div key={i} className="file-line">
                            <span className={`file-code file-${f.code.replace("?", "u")}`}>{f.code}</span>
                            <span className="file-path">{f.path}</span>
                          </div>
                        ))}
                        {fileChanges.length > 8 && (
                          <div className="commit-more">…还有 {fileChanges.length - 8} 个文件</div>
                        )}
                      </div>
                    )}
                    <div className="commit-row">
                      <input
                        className="modal-input"
                        placeholder="提交说明，如：修复登录问题"
                        value={commitMsg}
                        disabled={!!busy}
                        onChange={(e) => setCommitMsg(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" && commitMsg.trim()) void handleCommitPush();
                        }}
                      />
                      <button className="btn-sm" disabled={!!busy || !commitMsg.trim()} onClick={() => void handleCommitPush()}>
                        提交并推送
                      </button>
                    </div>
                  </div>
                )}
              </span>
            )}
          </div>
          {host && host !== "other" && (
            <span className={`chip chip-host-${host}`}>{host === "gitee" ? "Gitee" : "GitHub"}</span>
          )}
          {st?.is_repo ? (
            <>
              <span className="chip">分支：{st.branch || "未知"}</span>
              {st.behind > 0 && <span className="chip chip-behind">落后云端 {st.behind} 个提交</span>}
              {st.ahead > 0 && <span className="chip chip-ahead">待推送 {st.ahead} 个提交</span>}
              {st.behind === 0 && st.ahead === 0 && !st.has_upstream && <span className="chip">未设置上游分支</span>}
            </>
          ) : (
            <span className="chip chip-warn">仓库不可用（可能已被移动或删除）</span>
          )}
        </div>
      </div>

      <div className="actions">
        <button className="action-btn pill" disabled={!!busy} onClick={() => void checkUpdate()}>
          <IconCloud size={17} />
          检测云端更新
        </button>
        <button className="action-btn pill" disabled={!!busy} onClick={() => void handlePull()}>
          <IconCloudDown size={17} />
          一键拉取
        </button>
        <button className="action-btn pill" disabled={!!busy} onClick={() => void push()}>
          <IconCloudUp size={17} />
          一键推送
        </button>
      </div>

      {confirmStash && (
        <div className="stash-bar">
          <span>本地有未提交变更，是否先暂存（stash）再拉取，完成后自动恢复？</span>
          <div>
            <button className="btn-sm" onClick={() => void handleStashPull()}>
              暂存并拉取
            </button>
            <button className="btn-sm btn-ghost" onClick={() => setConfirmStash(false)}>
              取消
            </button>
          </div>
        </div>
      )}

      {busy && (
        <div className="busy-bar">
          <div className="busy-row">
            <span className="spinner" />
            <span>{progress ? `${busy.replace(/…$/, "")} · ${progress.stage}` : busy}</span>
            {progressPct != null && <span className="busy-pct">{Math.round(progressPct)}%</span>}
          </div>
          <div className="progress-track">
            <div
              className={`progress-fill ${progressPct == null ? "indeterminate" : ""}`}
              style={progressPct != null ? { width: `${progressPct}%` } : undefined}
            />
          </div>
        </div>
      )}

      {update && !busy && (
        <div className="update-card">
          <div className="update-header">
            <span className="update-title">
              本次同步内容
              {host === "gitee" ? "（来自 Gitee）" : host === "github" ? "（来自 GitHub）" : ""}
            </span>
            <span className="update-count">{update.commits.length} 个新提交</span>
          </div>
          {update.commits.length > 0 && (
            <ul className="commit-list">
              {update.commits.slice(0, 10).map((c, i) => (
                <li key={i} className="commit-line">
                  {c}
                </li>
              ))}
              {update.commits.length > 10 && <li className="commit-more">…等共 {update.commits.length} 个提交</li>}
            </ul>
          )}
          {update.diff_stat && <pre className="diff-stat">{update.diff_stat}</pre>}
        </div>
      )}

      <div className="log-area card">
        <div className="log-header">
          <span>操作日志</span>
          <button className="btn-sm btn-ghost" onClick={() => selected && void refreshStatus(selected)}>
            刷新状态
          </button>
        </div>
        <div className="log-body" ref={logRef}>
          {logs.length === 0 && <div className="log-empty">暂无日志</div>}
          {logs.map((l, i) => (
            <div key={i} className={`log-line log-${l.kind}`}>
              <span className="log-time">{l.time}</span>
              <span className="log-text">{l.text}</span>
            </div>
          ))}
        </div>
      </div>

      {showHistory && <HistoryModal onClose={() => setShowHistory(false)} />}

      {langs.length > 0 && (
        <div className="lang-footer">
          <div className="lang-bar">
            {langs.map((l) => (
              <span
                key={l.language}
                className="lang-seg"
                style={{ width: `${l.percent}%`, background: langColor(l.language) }}
                title={`${l.language} ${l.percent.toFixed(1)}%`}
              />
            ))}
          </div>
          <div className="lang-legend">
            {langs.map((l) => (
              <span key={l.language} className="lang-item">
                <span className="lang-dot" style={{ background: langColor(l.language) }} />
                {l.language} <b>{l.percent.toFixed(1)}%</b>
              </span>
            ))}
          </div>
        </div>
      )}
    </main>
  );
}
