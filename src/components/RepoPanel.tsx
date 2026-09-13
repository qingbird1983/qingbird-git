import { useEffect, useRef, useState } from "react";
import { api, remoteHost, type FileChange } from "../api";
import { useApp, autoSyncLabel, AUTO_SYNC_OPTIONS, AUTO_SYNC_DAILY, type LogLine } from "../store";
import HistoryModal from "./HistoryModal";
import CmdPanel from "./CmdPanel";
import {
  IconCloud, IconCloudDown, IconCloudUp,
  IconFolder, IconTerminal, IconHistory, IconDiff, IconX, IconFile,
  IconFlask, IconHammer, IconSync, IconPlay, IconClock,
} from "./icons";

// zustand v5 要求选择器返回稳定引用：仓库还没有日志时返回这个共享常量，
// 不能在 selector 里新建 []，否则 getSnapshot 每次都是新引用导致 React 白屏
const EMPTY_LOGS: LogLine[] = [];

export default function RepoPanel() {
  const {
    selected, status, busy, cloning, cmdRunning, progress, changes,
    checkUpdate, pull, push, commitPush, runRepoCmd, stopRepoCmd,
    openInFolder, openInTerminal, openReadme, cancelOp, setAutoSync,
  } = useApp();
  // 日志按仓库分开：只显示当前选中仓库自己的操作日志
  const logs = useApp((s) => (s.selected ? s.logs[s.selected] : undefined) ?? EMPTY_LOGS);
  const [confirmStash, setConfirmStash] = useState(false);
  const [fileChanges, setFileChanges] = useState<FileChange[]>([]);
  const [commitMsg, setCommitMsg] = useState("");
  const [showHistory, setShowHistory] = useState(false);
  const [showChanges, setShowChanges] = useState(false);
  const [showSync, setShowSync] = useState(false);
  const [showCmd, setShowCmd] = useState(false);
  const [showAutoSync, setShowAutoSync] = useState(false);
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
    setShowChanges(false);
    setShowSync(false);
    setShowAutoSync(false);
  }, [selected]);

  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [logs]);

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

  // 一键运行：已保存命令后台运行并流式写入日志，否则打开运行命令面板（可修改后保存重跑）
  const runCmd = async (kind: "test" | "build") => {
    if (!selected) return;
    const saved = kind === "test" ? repoEntry?.test_cmd : repoEntry?.build_cmd;
    if (saved && saved.trim()) {
      await runRepoCmd(selected, saved.trim());
    } else {
      setShowCmd(true);
    }
  };

  if (!selected) {
    return (
      <main className="panel panel-empty">
        <div className="empty-big">
          <div className="empty-logo" aria-hidden>
            青
          </div>
          选择或添加一个仓库开始使用
          <div className="empty-sub">支持本地文件夹、拖拽添加、粘贴地址克隆、一键新建仓库（GitHub / Gitee）</div>
        </div>
      </main>
    );
  }

  const host = st ? remoteHost(st.remote_url) : null;
  const progressPct = progress?.percent != null ? Math.min(100, progress.percent) : null;
  // 网络操作（检测/拉取/推送/提交推送）与克隆互斥：取消标记与代理缓存是全局的
  const netBusy = !!busy || !!cloning;
  // 定时自动同步（仅对此仓库生效）：间隔模式或每天定时模式
  const autoMins = repoEntry?.auto_sync_mins ?? 0;
  const autoDaily = repoEntry?.daily_time ?? "";
  const autoOn = autoMins > 0 || !!autoDaily;
  const autoPullOn = repoEntry?.auto_pull ?? false;
  const lastAutoCheck = repoEntry?.last_auto_check ?? 0;
  const lastAutoResult = (repoEntry?.last_auto_result ?? "").replace(/^[a-z]+:/, "");
  const lastCheckText =
    lastAutoCheck > 0
      ? (() => {
          const mins = Math.max(1, Math.round(Date.now() / 1000 - lastAutoCheck));
          return mins < 60 ? `${mins} 分钟前` : `${Math.round(mins / 60)} 小时前`;
        })()
      : null;
  // 切换档位：每天定时首次点入默认 09:00，切回间隔模式清空时间点
  const applyAutoSync = (mins: number) => {
    if (mins === AUTO_SYNC_DAILY) {
      void setAutoSync(selected, 0, autoPullOn, autoDaily || "09:00");
    } else {
      void setAutoSync(selected, mins, autoPullOn, "");
    }
  };

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
            <button className="icon-btn" title="查看仓库说明（用系统默认程序打开 README）" onClick={() => void openReadme(selected)}>
              <IconFile size={15} />
            </button>
            <button className="icon-btn" title="运行命令（测试 / 构建 / 开发，可修改）" onClick={() => setShowCmd(true)}>
              <IconPlay size={15} />
            </button>
            <span className="changes-anchor">
              <button
                className={`icon-btn ${autoOn ? "auto-on" : ""} ${showAutoSync ? "icon-btn-active" : ""}`}
                title={
                  autoOn
                    ? `定时自动同步：${autoSyncLabel(autoMins, autoDaily)}（仅此仓库，点击设置）`
                    : "定时自动同步（仅此仓库，点击设置）"
                }
                onClick={() => setShowAutoSync((v) => !v)}
              >
                <IconClock size={15} />
              </button>
              {showAutoSync && (
                <div className="commit-popover auto-sync-pop">
                  <div className="update-header">
                    <span className="commit-title">定时自动同步</span>
                    <button className="popover-close" title="关闭" onClick={() => setShowAutoSync(false)}>
                      <IconX size={13} />
                    </button>
                  </div>
                  <p className="auto-sync-desc">
                    在后台按间隔检测云端更新（自动走直连/代理回退），仅对此仓库生效；
                    只做快进合并（--ff-only），本地分叉或有未提交变更时不会动你的代码。
                  </p>
                  <div className="segmented auto-sync-seg">
                    {AUTO_SYNC_OPTIONS.map((o) => (
                      <button
                        key={o.mins}
                        className={`segment ${
                          (o.mins === AUTO_SYNC_DAILY ? !!autoDaily : autoMins === o.mins)
                            ? "segment-active"
                            : ""
                        }`}
                        onClick={() => applyAutoSync(o.mins)}
                      >
                        {o.label}
                      </button>
                    ))}
                  </div>
                  {autoDaily && (
                    <div className="auto-sync-daily-row">
                      <span>每天执行时刻</span>
                      <input
                        type="time"
                        className="auto-sync-time"
                        value={autoDaily}
                        onChange={(e) => void setAutoSync(selected, 0, autoPullOn, e.target.value)}
                      />
                    </div>
                  )}
                  <label className="switch-row auto-sync-switch">
                    <span>检测到更新后自动拉取</span>
                    <button
                      className={`switch ${autoPullOn ? "on" : ""}`}
                      role="switch"
                      aria-checked={autoPullOn}
                      title={autoOn ? undefined : "请先选择间隔或每天定时开启"}
                      onClick={() => {
                        if (autoOn) void setAutoSync(selected, autoMins, !autoPullOn, autoDaily);
                      }}
                    >
                      <span className="switch-knob" />
                    </button>
                  </label>
                  <div className="auto-sync-meta">
                    {lastCheckText
                      ? `上次自动检查：${lastCheckText}${lastAutoResult ? ` · ${lastAutoResult}` : ""}`
                      : autoOn
                        ? "已开启，稍后自动进行首次检查"
                        : "尚未开启，选择间隔或每天定时即可启用"}
                  </div>
                </div>
              )}
            </span>
            {update && !busy && (
              <span className="changes-anchor">
                <button
                  className={`icon-btn has-badge ${showSync ? "icon-btn-active" : ""}`}
                  title={`本次同步内容（${update.commits.length} 个新提交）`}
                  onClick={() => setShowSync((v) => !v)}
                >
                  <IconSync size={15} />
                  {update.commits.length > 0 && <span className="btn-badge">{update.commits.length}</span>}
                </button>
                {showSync && (
                  <div className="commit-popover">
                    <div className="update-header">
                      <span className="commit-title">
                        本次同步内容
                        {host === "gitee" ? "（来自 Gitee）" : host === "github" ? "（来自 GitHub）" : ""}
                      </span>
                      <span className="update-count">{update.commits.length} 个新提交</span>
                      <button className="popover-close" title="关闭" onClick={() => setShowSync(false)}>
                        <IconX size={13} />
                      </button>
                    </div>
                    {update.commits.length === 0 ? (
                      <div className="commit-more">本次没有新提交（可能只是文件变更同步）</div>
                    ) : (
                      <div className="sync-commit-list">
                        {update.commits.map((c, i) => (
                          <div key={i} className="sync-commit-line">
                            {c}
                          </div>
                        ))}
                      </div>
                    )}
                    <div className="commit-more">文件变更明细见下方操作日志</div>
                  </div>
                )}
              </span>
            )}
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
                        disabled={netBusy}
                        onChange={(e) => setCommitMsg(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" && commitMsg.trim()) void handleCommitPush();
                        }}
                      />
                      <button className="btn-sm" disabled={netBusy || !commitMsg.trim()} onClick={() => void handleCommitPush()}>
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
        <button className="action-btn pill" disabled={netBusy} onClick={() => void checkUpdate()}>
          <IconCloud size={17} />
          检测云端更新
        </button>
        <button className="action-btn pill" disabled={netBusy} onClick={() => void handlePull()}>
          <IconCloudDown size={17} />
          一键拉取
        </button>
        <button className="action-btn pill" disabled={netBusy} onClick={() => void push()}>
          <IconCloudUp size={17} />
          一键推送
        </button>
        <button className="action-btn pill" disabled={!!busy || !!cmdRunning} onClick={() => void runCmd("test")}>
          <IconFlask size={17} />
          跑测试
        </button>
        <button className="action-btn pill" disabled={!!busy || !!cmdRunning} onClick={() => void runCmd("build")}>
          <IconHammer size={17} />
          构建
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

      {(busy || cloning) && (
        <div className="busy-pop">
          <div className="busy-row">
            <span className="spinner" />
            <span>
              {(busy ?? cloning ?? "").replace(/…$/, "")}
              {progress ? ` · ${progress.stage}` : ""}
              {progress?.detail ? ` · ${progress.detail}` : ""}
            </span>
            {progressPct != null && <span className="busy-pct">{Math.round(progressPct)}%</span>}
          </div>
          <div className="progress-track">
            <div
              className={`progress-fill ${progressPct == null ? "indeterminate" : ""}`}
              style={progressPct != null ? { width: `${progressPct}%` } : undefined}
            />
          </div>
          <button className="busy-cancel" title="取消当前操作" onClick={() => void cancelOp()}>
            <IconX size={13} />
            取消
          </button>
        </div>
      )}

      <div className="log-area card">
        <div className="log-header">
          <span>操作日志</span>
          {cmdRunning ? (
            <button className="btn-sm btn-danger" title={`终止后台命令：${cmdRunning}`} onClick={() => void stopRepoCmd()}>
              终止
            </button>
          ) : (
            <span className="log-hint">后台命令运行时可在此终止</span>
          )}
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
      {showCmd && <CmdPanel onClose={() => setShowCmd(false)} />}
    </main>
  );
}
