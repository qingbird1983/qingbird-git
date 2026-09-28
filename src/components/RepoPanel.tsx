import { useEffect, useRef, useState } from "react";
import appIcon from "../assets/icon.png";
import { api, remoteHost, type ActivityDay, type FileChange, type LangStat } from "../api";
import { useApp, type LogLine } from "../store";
import { autoSyncLabel } from "../lib/autoSync";
import type { PinnedCmd } from "../lib/pinnedCmds";
import Inspector, { type InspTab } from "./Inspector";
import CmdPanel from "./CmdPanel";
import ActivityChart from "./panel/ActivityChart";
import BusyPop from "./panel/BusyPop";
import ForkCard from "./panel/ForkCard";
import LogArea from "./panel/LogArea";
import StashBar from "./panel/StashBar";
import SyncRing from "./panel/SyncRing";
import {
  IconCloud, IconCloudDown, IconCloudUp, IconDownload, IconFile, IconFlask, IconFolder,
  IconHammer, IconPanelRight, IconPlay, IconSync, IconTerminal,
} from "./icons";

// zustand v5 要求选择器返回稳定引用：仓库还没有日志时返回这个共享常量，
// 不能在 selector 里新建 []，否则 getSnapshot 每次都是新引用导致 React 白屏
const EMPTY_LOGS: LogLine[] = [];
/** 同上：仓库没钉自定义按钮时也必须是稳定引用 */
const EMPTY_PINNED: PinnedCmd[] = [];
/** 同上：提交活动为空时（没仓库 / 不是仓库 / 还没加载完）用这个常量占位 */
const EMPTY_ACTIVITY: ActivityDay[] = [];

export default function RepoPanel() {
  const {
    selected, status, busy, cloning, cmdRunning, progress, changes,
    checkUpdate, pullSmart, stashPull, cancelStash, stashConfirm, push, commitPush,
    runRepoCmd, stopRepoCmd,
    openInFolder, openInTerminal, openReadme, cancelOp, setAutoSync,
    rebaseFork, undoRebase,
  } = useApp();
  // 日志按仓库分开：只显示当前选中仓库自己的操作日志
  const logs = useApp((s) => (s.selected ? s.logs[s.selected] : undefined) ?? EMPTY_LOGS);
  const fork = useApp((s) => (s.selected ? s.forks[s.selected] : undefined));
  const backup = useApp((s) => (s.selected ? s.backups[s.selected] : undefined));
  const lastProxy = useApp((s) => s.lastProxy);
  const [fileChanges, setFileChanges] = useState<FileChange[]>([]);
  const [commitMsg, setCommitMsg] = useState("");
  const [showCmd, setShowCmd] = useState(false);
  const [inspOpen, setInspOpen] = useState(true);
  const [inspTab, setInspTab] = useState<InspTab>("changes");
  const [logOpen, setLogOpen] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const [langs, setLangs] = useState<LangStat[]>([]);
  const [activity, setActivity] = useState<ActivityDay[]>(EMPTY_ACTIVITY);
  const repos = useApp((s) => s.repos);
  const logRef = useRef<HTMLDivElement>(null);

  const st = selected ? status[selected] : undefined;
  const update = selected ? changes[selected] : undefined;
  const repoEntry = selected ? repos.find((r) => r.path === selected) : undefined;
  const displayName = repoEntry?.alias || selected?.split(/[\\/]/).pop() || "";
  /** 钉在命令区的自定义命令按钮（旧配置没有这个字段，缺省为空） */
  const pinned = repoEntry?.pinned_cmds ?? EMPTY_PINNED;

  useEffect(() => {
    setCommitMsg("");
    setFileChanges([]);
    setMoreOpen(false);
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
    }
  }, [selected, st?.dirty]);

  // 语言构成：从原来的通铺页脚收进工作区统计卡
  useEffect(() => {
    setLangs([]);
    if (selected && st?.is_repo) {
      void api.languages(selected).then(setLangs).catch(() => setLangs([]));
    }
  }, [selected, st?.is_repo]);

  // 提交活跃度：近 14 天柱状图。与语言构成同一档数据，都是切仓库时重取
  useEffect(() => {
    setActivity(EMPTY_ACTIVITY);
    if (selected && st?.is_repo) {
      void api.commitActivity(selected).then(setActivity).catch(() => setActivity(EMPTY_ACTIVITY));
    }
  }, [selected, st?.is_repo]);

  const handlePull = async () => {
    await pullSmart();
  };

  const handleStashPull = async () => {
    await stashPull();
  };

  const handleCommitPush = async () => {
    if (!commitMsg.trim()) return;
    const ok = await commitPush(commitMsg.trim());
    if (ok) setCommitMsg("");
  };

  // 一键运行：已保存命令后台运行并流式写入日志，否则打开运行命令面板
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
          <img className="empty-logo" src={appIcon} alt="" aria-hidden draggable={false} />
          选择或添加一个仓库开始使用
          <div className="empty-sub">支持本地文件夹、拖拽添加、粘贴地址克隆、一键新建仓库（GitHub / Gitee）</div>
        </div>
      </main>
    );
  }

  const host = st ? remoteHost(st.remote_url) : null;
  // 状态还没扫出来时（st 为 undefined）一律按 0 处理：未知的差异不能当成有差异
  const behindN = st?.behind ?? 0;
  const aheadN = st?.ahead ?? 0;
  const progressPct = progress?.percent != null ? Math.min(100, progress.percent) : null;
  // 网络操作（检测/拉取/推送/克隆）互斥：取消标记与代理缓存是全局的
  const netBusy = !!busy || !!cloning;
  // 定时自动同步（仅对此仓库生效）：间隔模式或每天定时模式
  const autoMins = repoEntry?.auto_sync_mins ?? 0;
  const autoDaily = repoEntry?.daily_time ?? "";
  const autoOn = autoMins > 0 || !!autoDaily;
  const lastAutoCheck = repoEntry?.last_auto_check ?? 0;
  const lastAutoResult = (repoEntry?.last_auto_result ?? "").replace(/^[a-z]+:/, "");
  const lastCheckText =
    lastAutoCheck > 0
      ? (() => {
          const mins = Math.max(1, Math.round(Date.now() / 1000 - lastAutoCheck));
          return mins < 60 ? `${mins} 分钟前` : `${Math.round(mins / 60)} 小时前`;
        })()
      : null;

  // 状态栏：优先报有意义的结论，否则退到最后一条日志
  const lastLog = logs[logs.length - 1];
  const statusText = busy
    ? (busy ?? "").replace(/…$/, "")
    : lastLog
      ? lastLog.text.split("\n").filter(Boolean).slice(-1)[0]
      : "就绪";

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
          {host && host !== "other" && (
            <span className={`chip chip-host-${host}`}>{host === "gitee" ? "Gitee" : "GitHub"}</span>
          )}
          {st?.is_repo ? (
            <>
              <span className="chip">分支：{st.branch || "未知"}</span>
              {st.behind > 0 && <span className="chip chip-behind">落后云端 {st.behind} 个提交</span>}
              {st.ahead > 0 && <span className="chip chip-ahead">待推送 {st.ahead} 个提交</span>}
              {st.behind === 0 && st.ahead === 0 && !st.has_upstream && (
                <span className="chip">未设置上游分支</span>
              )}
            </>
          ) : (
            <span className="chip chip-warn">仓库不可用（可能已被移动或删除）</span>
          )}
          <div className="head-tools">
            <button
              className={`icon-btn${inspOpen ? " icon-btn-active" : ""}`}
              title="检视（变更 / 历史 / 定时 / 空间）"
              onClick={() => setInspOpen((v) => !v)}
            >
              <IconPanelRight size={15} />
            </button>
            <span className="changes-anchor">
              <button
                className="icon-btn"
                title="更多"
                onClick={() => setMoreOpen((v) => !v)}
              >
                ⋯
              </button>
              {moreOpen && (
                <div className="more-menu" onMouseLeave={() => setMoreOpen(false)}>
                  <button onClick={() => void openInFolder(selected)}>
                    <IconFolder size={13} />打开文件夹
                  </button>
                  <button onClick={() => void openInTerminal(selected)}>
                    <IconTerminal size={13} />在此打开终端
                  </button>
                  <button onClick={() => void openReadme(selected)}>
                    <IconFile size={13} />查看仓库说明
                  </button>
                </div>
              )}
            </span>
          </div>
        </div>
      </div>

      <div className="work-row">
        <div className="work-main">
          {/* ---------- 同步：执行层的唯一中枢 ---------- */}
          <div className="sync-card card">
            <SyncRing
              behind={behindN}
              ahead={aheadN}
              broken={!st?.is_repo}
              noUpstream={!!st?.is_repo && !st.has_upstream}
            />
            <div className="sync-text">
              <div className="sync-title">
                {!st?.is_repo
                  ? "仓库不可用"
                  : st.behind > 0
                    ? `云端有 ${st.behind} 个新提交`
                    : st.ahead > 0
                      ? `本地有 ${st.ahead} 个提交待推送`
                      : st.has_upstream
                        ? "本地与云端一致"
                        : "还没有设置上游分支"}
              </div>
              <div className="sync-desc">
                {behindN > 0 || aheadN > 0 ? (
                  <>
                    拉取固定带 <span className="mono">--ff-only</span>，遇到分叉只会停下报错，不会自动合并。
                    {autoOn && ` 定时同步：${autoSyncLabel(autoMins, autoDaily)}。`}
                  </>
                ) : (
                  <>
                    {update && update.commits.length > 0
                      ? `上次同步带回 ${update.commits.length} 个新提交。`
                      : "随时可以检测云端是否有更新。"}
                    {autoOn && ` 定时同步：${autoSyncLabel(autoMins, autoDaily)}。`}
                  </>
                )}
              </div>
            </div>
            <div className="sync-btns">
              <button className="sbtn acc" disabled={netBusy} onClick={() => void handlePull()}>
                <IconCloudDown size={15} />拉取
              </button>
              <button className="sbtn" disabled={netBusy} onClick={() => void push()}>
                <IconCloudUp size={15} />推送
              </button>
              <button className="sbtn ghost" disabled={netBusy} onClick={() => void checkUpdate()}>
                <IconCloud size={15} />检测
              </button>
            </div>
          </div>

          {/* ---------- 命令：测试 / 构建 / 开发 / 安装依赖 + 自定义 ---------- */}
          <div className="cmd-card card">
            <div className="cmd-head">
              <span>命令</span>
              <button className="btn-sm btn-ghost" onClick={() => setShowCmd(true)}>
                配置命令
              </button>
            </div>
            <div className="cmd-list">
              <div className="cmd-row">
                <span className="cmd-ic">
                  <IconFlask size={14} />
                </span>
                <button className="cmd-name" onClick={() => setShowCmd(true)}>
                  跑测试
                </button>
                <span className="cmd-text" title={repoEntry?.test_cmd}>
                  {repoEntry?.test_cmd || "未设置"}
                </span>
                <button
                  className="cmd-run"
                  title="运行"
                  disabled={!!busy || !!cmdRunning}
                  onClick={() => void runCmd("test")}
                >
                  <IconPlay size={13} />
                </button>
              </div>
              <div className="cmd-row">
                <span className="cmd-ic">
                  <IconHammer size={14} />
                </span>
                <button className="cmd-name" onClick={() => setShowCmd(true)}>
                  构建
                </button>
                <span className="cmd-text" title={repoEntry?.build_cmd}>
                  {repoEntry?.build_cmd || "未设置"}
                </span>
                <button
                  className="cmd-run"
                  title="运行"
                  disabled={!!busy || !!cmdRunning}
                  onClick={() => void runCmd("build")}
                >
                  <IconPlay size={13} />
                </button>
              </div>
              <div className="cmd-row">
                <span className="cmd-ic">
                  <IconPlay size={14} />
                </span>
                <button className="cmd-name" onClick={() => setShowCmd(true)}>
                  开发
                </button>
                <span className="cmd-text" title={repoEntry?.dev_cmd}>
                  {repoEntry?.dev_cmd || "未设置"}
                </span>
                <button
                  className="cmd-run"
                  title="运行"
                  disabled={!!busy || !!cmdRunning}
                  onClick={() => {
                    const c = repoEntry?.dev_cmd;
                    if (c && c.trim()) void runRepoCmd(selected, c.trim());
                    else setShowCmd(true);
                  }}
                >
                  <IconPlay size={13} />
                </button>
              </div>
              <div className="cmd-row">
                <span className="cmd-ic">
                  <IconDownload size={14} />
                </span>
                <button className="cmd-name" onClick={() => setShowCmd(true)}>
                  安装依赖
                </button>
                <span className="cmd-text" title={repoEntry?.install_cmd}>
                  {repoEntry?.install_cmd || "未设置"}
                </span>
                <button
                  className="cmd-run"
                  title="运行"
                  disabled={!!busy || !!cmdRunning}
                  onClick={() => {
                    const c = repoEntry?.install_cmd;
                    if (c && c.trim()) void runRepoCmd(selected, c.trim());
                    else setShowCmd(true);
                  }}
                >
                  <IconPlay size={13} />
                </button>
              </div>
              {pinned.map((p, i) => (
                <div key={`${i}-${p.cmd}`} className="cmd-row">
                  <span className="cmd-ic">
                    <IconPlay size={14} />
                  </span>
                  <span className="cmd-name plain" title={`自定义命令：${p.cmd}`}>
                    {p.label}
                  </span>
                  <span className="cmd-text" title={p.cmd}>
                    {p.cmd}
                  </span>
                  <button
                    className="cmd-run"
                    title="运行"
                    disabled={!!busy || !!cmdRunning}
                    onClick={() => void runRepoCmd(selected, p.cmd)}
                  >
                    <IconPlay size={13} />
                  </button>
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* ---------- 统计条：提交活跃度 + 语言构成（窄列竖排，不占主区宽度） ---------- */}
        <div className="stats-col">
          <ActivityChart days={activity} />
          <div className="stats-card card">
            <div className="stats-head">语言构成</div>
            {langs.length === 0 ? (
              <div className="insp-empty">暂无数据</div>
            ) : (
              <>
                <div className="lang-bar">
                  {langs.map((l) => (
                    <span
                      key={l.language}
                      className="lang-seg"
                      style={{ width: `${l.percent}%`, background: LANG_COLORS[l.language] ?? "#a1a1aa" }}
                      title={`${l.language} ${l.percent.toFixed(1)}%`}
                    />
                  ))}
                </div>
                <div className="lang-legend">
                  {langs.slice(0, 6).map((l) => (
                    <span key={l.language} className="lang-item">
                      <span className="lang-dot" style={{ background: LANG_COLORS[l.language] ?? "#a1a1aa" }} />
                      {l.language}
                      <b>{l.percent.toFixed(1)}%</b>
                    </span>
                  ))}
                </div>
              </>
            )}
          </div>
        </div>

        {inspOpen && (
          <Inspector
            selected={selected}
            tab={inspTab}
            onTab={setInspTab}
            onClose={() => setInspOpen(false)}
            changeCount={fileChanges.length}
            files={fileChanges}
            commitMsg={commitMsg}
            netBusy={netBusy}
            onCommitMsg={setCommitMsg}
            onCommitSubmit={() => void handleCommitPush()}
            autoMins={autoMins}
            autoDaily={autoDaily}
            autoPull={repoEntry?.auto_pull ?? false}
            lastCheckText={lastCheckText}
            lastResult={lastAutoResult}
            onAutoChange={(m, ap, d) => void setAutoSync(selected, m, ap, d)}
            syncCommits={update?.commits ?? []}
            hasSync={!!update}
            syncHost={host}
          />
        )}
      </div>

      {stashConfirm && <StashBar onConfirm={() => void handleStashPull()} onCancel={cancelStash} />}

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

      {/* ---------- 底部状态栏：平时只占一行，需要排查时升起日志 ---------- */}
      <div className="status-bar">
        <span className={`sb-dot${netBusy ? " on" : ""}`} />
        <span className="sb-text" title={statusText}>
          {statusText}
        </span>
        {lastProxy && <span className="sb-proxy" title="本次操作走过的代理">代理 {lastProxy}</span>}
        {cmdRunning && (
          <button className="btn-sm btn-danger" onClick={() => void stopRepoCmd()}>
            终止 {cmdRunning}
          </button>
        )}
        <button className="sb-log" onClick={() => setLogOpen((v) => !v)}>
          <IconSync size={12} />
          日志{logs.length > 0 ? ` ${logs.length}` : ""}
        </button>
      </div>

      {logOpen && <LogArea logs={logs} cmdRunning={cmdRunning} logRef={logRef} onStop={() => void stopRepoCmd()} />}

      {showCmd && <CmdPanel onClose={() => setShowCmd(false)} />}
    </main>
  );
}

/** 常见语言的显示颜色（对齐 GitHub Linguist 配色，从 LangFooter 搬来） */
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
