import { useEffect, useRef, useState } from "react";
import { api, remoteHost, type FileChange } from "../api";
import { useApp, type LogLine } from "../store";
import { autoSyncLabel } from "../lib/autoSync";
import type { PinnedCmd } from "../lib/pinnedCmds";
import HistoryModal from "./HistoryModal";
import CmdPanel from "./CmdPanel";
import AutoSyncPopover from "./panel/AutoSyncPopover";
import BusyPop from "./panel/BusyPop";
import ChangesPopover from "./panel/ChangesPopover";
import ForkCard from "./panel/ForkCard";
import LogArea from "./panel/LogArea";
import StashBar from "./panel/StashBar";
import SyncContentPopover from "./panel/SyncContentPopover";
import {
  IconCloud, IconCloudDown, IconCloudUp,
  IconFolder, IconTerminal, IconHistory, IconDiff, IconFile,
  IconFlask, IconHammer, IconSync, IconPlay, IconClock,
} from "./icons";

// zustand v5 要求选择器返回稳定引用：仓库还没有日志时返回这个共享常量，
// 不能在 selector 里新建 []，否则 getSnapshot 每次都是新引用导致 React 白屏
const EMPTY_LOGS: LogLine[] = [];
/** 同上：仓库没钉自定义按钮时也必须是稳定引用 */
const EMPTY_PINNED: PinnedCmd[] = [];

export default function RepoPanel() {
  const {
    selected, status, busy, cloning, cmdRunning, progress, changes,
    checkUpdate, pull, push, commitPush, runRepoCmd, stopRepoCmd,
    openInFolder, openInTerminal, openReadme, cancelOp, setAutoSync,
    rebaseFork, undoRebase,
  } = useApp();
  // 日志按仓库分开：只显示当前选中仓库自己的操作日志
  const logs = useApp((s) => (s.selected ? s.logs[s.selected] : undefined) ?? EMPTY_LOGS);
  const fork = useApp((s) => (s.selected ? s.forks[s.selected] : undefined));
  const backup = useApp((s) => (s.selected ? s.backups[s.selected] : undefined));
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
  /** 钉在操作区的自定义命令按钮（旧配置没有这个字段，缺省为空） */
  const pinned = repoEntry?.pinned_cmds ?? EMPTY_PINNED;

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
<AutoSyncPopover
                  mins={autoMins}
                  daily={autoDaily}
                  autoPull={autoPullOn}
                  lastCheckText={lastCheckText}
                  lastResult={lastAutoResult}
                  onChange={(mins, autoPull, daily) => void setAutoSync(selected, mins, autoPull, daily)}
                  onClose={() => setShowAutoSync(false)}
                />
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
<SyncContentPopover commits={update.commits} host={host} onClose={() => setShowSync(false)} />
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
<ChangesPopover
                      files={fileChanges}
                      commitMsg={commitMsg}
                      netBusy={netBusy}
                      onCommitMsg={setCommitMsg}
                      onSubmit={() => void handleCommitPush()}
                      onClose={() => setShowChanges(false)}
                    />
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
        {pinned.map((p, i) => (
          <button
            key={`${i}-${p.cmd}`}
            className="action-btn pill"
            title={`自定义按钮：${p.cmd}`}
            disabled={!!busy || !!cmdRunning}
            onClick={() => selected && void runRepoCmd(selected, p.cmd)}
          >
            <IconPlay size={17} />
            {p.label}
          </button>
        ))}
      </div>

      {confirmStash && (
<StashBar onConfirm={() => void handleStashPull()} onCancel={() => setConfirmStash(false)} />
      )}

      {(busy || cloning) && (
<BusyPop
          label={(busy ?? cloning ?? "").replace(/…$/, "")}
          progress={progress}
          percent={progressPct}
          onCancel={() => void cancelOp()}
        />
      )}

      <ForkCard
        fork={fork}
        backup={backup}
        busy={!!busy}
        onRebase={(stash) => void rebaseFork(stash)}
        onUndo={() => void undoRebase()}
      />

<LogArea logs={logs} cmdRunning={cmdRunning} logRef={logRef} onStop={() => void stopRepoCmd()} />

      {showHistory && <HistoryModal onClose={() => setShowHistory(false)} />}
      {showCmd && <CmdPanel onClose={() => setShowCmd(false)} />}
    </main>
  );
}
