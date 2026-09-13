import { create } from "zustand";
import { Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { api, remoteHost, type AutoSyncEvent, type ProgressEvent, type RepoEntry, type RepoStatus, type UpdateChanges } from "./api";

export interface LogLine {
  time: string;
  text: string;
  kind: "info" | "success" | "error" | "proxy";
}

export type ThemeMode = "dark" | "light" | "system";

/** 代理回退弹窗通知（用时弹出，自动消失） */
export interface ProxyToast {
  text: string;
  at: number;
}

/** 定时自动同步结果通知（后台推送，自动消失） */
export interface AutoToast {
  text: string;
  kind: "info" | "warn" | "error";
  at: number;
}

let proxyToastTimer: ReturnType<typeof setTimeout> | null = null;
let autoToastTimer: ReturnType<typeof setTimeout> | null = null;

interface AppState {
  repos: RepoEntry[];
  selected: string | null;
  status: Record<string, RepoStatus>;
  busy: string | null;
  /** 克隆进行中的提示文案；克隆不锁主面板（可同时看日志），仅与其他网络操作互斥 */
  cloning: string | null;
  /** 后台正在运行的本地命令（测试/构建等），输出流式写入日志窗口 */
  cmdRunning: string | null;
  /** 每个仓库独立的操作日志；key 为仓库路径，"_app" 为无选中仓库时的全局日志 */
  logs: Record<string, LogLine[]>;
  lastProxy: string | null;
  /** 当前显示的代理回退通知（null 表示不显示） */
  proxyToast: ProxyToast | null;
  /** 当前定时自动同步结果通知（null 表示不显示） */
  autoToast: AutoToast | null;
  /** 当前拉取/克隆进度（busy 时有值） */
  progress: ProgressEvent | null;
  /** 每个仓库最近一次拉取的更新内容 */
  changes: Record<string, UpdateChanges>;
  theme: ThemeMode;
  /** 有文件夹拖入窗口时为 true（显示拖放提示层） */
  dragging: boolean;
  /** 侧栏展开/收起（会话级，不落盘） */
  sidebarOpen: boolean;
  toggleSidebar: () => void;
  /** 设置弹窗开关（标题栏设置按钮与 App 弹窗层共用） */
  settingsOpen: boolean;
  openSettings: () => void;
  closeSettings: () => void;
  /** 侧栏宽度（拖宽条可调，clamp 到 SIDEBAR_MIN_W..SIDEBAR_MAX_W；localStorage 记忆） */
  sidebarWidth: number;
  setSidebarWidth: (w: number) => void;
  loadRepos: () => Promise<void>;
  select: (path: string | null) => void;
  refreshStatus: (path: string) => Promise<void>;
  addRepoByPath: (path: string) => Promise<boolean>;
  removeRepo: (path: string) => Promise<void>;
  renameRepo: (path: string, alias: string) => Promise<void>;
  setRepoCmds: (path: string, testCmd: string, buildCmd: string) => Promise<void>;
  pinRepo: (path: string, pinned: boolean) => Promise<void>;
  setAutoSync: (path: string, mins: number, autoPull: boolean, dailyTime?: string) => Promise<void>;
  log: (text: string, kind?: LogLine["kind"], key?: string) => void;
  /** 弹出代理回退通知（约 6 秒后自动消失） */
  notifyProxy: (proxy: string) => void;
  dismissProxy: () => void;
  /** 弹出定时自动同步结果通知（约 8 秒后自动消失） */
  notifyAuto: (text: string, kind?: AutoToast["kind"]) => void;
  dismissAuto: () => void;
  setBusy: (op: string | null) => void;
  setProgress: (p: ProgressEvent | null) => void;
  checkUpdate: () => Promise<void>;
  pull: (stash: boolean) => Promise<"ok" | "dirty" | "fail">;
  push: () => Promise<void>;
  commitPush: (message: string) => Promise<boolean>;
  cloneRepo: (url: string, parentDir: string, netMode?: string) => boolean;
  /** 在仓库目录下后台运行本地命令，输出逐行写入该仓库的日志窗口 */
  runRepoCmd: (path: string, cmd: string) => Promise<void>;
  stopRepoCmd: () => Promise<void>;
  cancelOp: () => Promise<void>;
  openInFolder: (path: string) => void;
  openInTerminal: (path: string) => void;
  openReadme: (path: string) => void;
  /** 用系统默认浏览器打开仓库的云端主页（GitHub / Gitee） */
  openRepoWebsite: (path: string) => void;
  scanAll: () => Promise<void>;
  setTheme: (t: ThemeMode) => Promise<void>;
  initTheme: () => Promise<void>;
}

function nowStr() {
  return new Date().toLocaleTimeString("zh-CN", { hour12: false });
}

/** 仓库列表统一排序（原地）：置顶优先，其余按最近使用自动靠前 */
export function sortRepos(repos: RepoEntry[]): RepoEntry[] {
  return repos.sort(
    (a, b) =>
      Number(b.pinned ?? false) - Number(a.pinned ?? false) || b.last_used - a.last_used,
  );
}

/** 侧栏宽度上下限（App.tsx 的拖宽条与 store 双重 clamp） */
export const SIDEBAR_MIN_W = 300;
export const SIDEBAR_MAX_W = 440;
const SIDEBAR_DEFAULT_W = 300;
const SIDEBAR_W_KEY = "qingbird.sidebarWidth";

function loadSidebarWidth(): number {
  const raw = Number(localStorage.getItem(SIDEBAR_W_KEY));
  if (!Number.isFinite(raw) || raw <= 0) return SIDEBAR_DEFAULT_W;
  return Math.min(SIDEBAR_MAX_W, Math.max(SIDEBAR_MIN_W, raw));
}

export function applyThemeClass(mode: ThemeMode) {
  const dark =
    mode === "dark" ||
    (mode === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

export const useApp = create<AppState>((set, get) => ({
  repos: [],
  selected: null,
  status: {},
  busy: null,
  cloning: null,
  cmdRunning: null,
  logs: {},
  lastProxy: null,
  proxyToast: null,
  autoToast: null,
  progress: null,
  changes: {},
  theme: "dark",
  dragging: false,
  sidebarOpen: true,
  toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
  settingsOpen: false,
  openSettings: () => set({ settingsOpen: true }),
  closeSettings: () => set({ settingsOpen: false }),
  sidebarWidth: loadSidebarWidth(),
  setSidebarWidth: (w) => {
    const clamped = Math.min(SIDEBAR_MAX_W, Math.max(SIDEBAR_MIN_W, Math.round(w)));
    localStorage.setItem(SIDEBAR_W_KEY, String(clamped));
    set({ sidebarWidth: clamped });
  },

  loadRepos: async () => {
    const repos = await api.listRepos();
    // 按最后使用时间倒序，最近使用的排前面
    sortRepos(repos);
    set({ repos });
    const selected = get().selected;    if (!selected && repos.length > 0) {
      get().select(repos[0].path);
    }
  },

  select: (path) => {
    set({ selected: path });
    if (path) {
      get().refreshStatus(path);
    }
  },

  refreshStatus: async (path) => {
    try {
      const st = await api.status(path);
      set((s) => ({ status: { ...s.status, [path]: st } }));
    } catch {
      /* 忽略，界面按无状态处理 */
    }
  },

  addRepoByPath: async (path) => {
    try {
      const repos = await api.addRepo(path);
      sortRepos(repos);
      set({ repos });
      get().select(path);
      get().log(`已添加仓库：${path}`, "success");
      return true;
    } catch (e) {
      get().log(String(e), "error");
      return false;
    }
  },

  removeRepo: async (path) => {
    const repos = await api.removeRepo(path);
    sortRepos(repos);
    const status = { ...get().status };
    delete status[path];
    const changes = { ...get().changes };
    delete changes[path];
    const selected = get().selected === path ? null : get().selected;
    set({ repos, status, changes, selected });
    if (!selected && repos.length > 0) {
      get().select(repos[0].path);
    }
  },

  renameRepo: async (path, alias) => {
    const repos = await api.renameRepo(path, alias);
    sortRepos(repos);
    set({ repos });
  },

  setRepoCmds: async (path, testCmd, buildCmd) => {
    const repos = await api.setRepoCmds(path, testCmd, buildCmd);
    sortRepos(repos);
    set({ repos });
  },

  setAutoSync: async (path, mins, autoPull, dailyTime = "") => {
    const repos = await api.setAutoSync(path, mins, autoPull, dailyTime);
    sortRepos(repos);
    set({ repos });
    const on = mins > 0 || !!dailyTime;
    if (on) {
      const label = dailyTime ? `每天 ${dailyTime}` : `每 ${autoSyncLabel(mins)}`;
      get().log(
        `已开启定时自动同步（${label}），${autoPull ? "检测到更新后自动拉取" : "仅检测提醒，不自动拉取"}`,
        "success",
        path,
      );
    } else {
      get().log("已关闭定时自动同步", "info", path);
    }
  },

  pinRepo: async (path, pinned) => {
    const repos = await api.pinRepo(path, pinned);
    sortRepos(repos);
    set({ repos });
  },

  log: (text, kind = "info", key) =>
    set((s) => {
      const k = key ?? s.selected ?? "_app";
      // git 的进度信息用 \r 在终端里原地刷新；整段 stderr 进日志时会把一堆
      // 进度片段挤在同一行。折叠规则：每个 \n 行只保留它最后一个 \r 段
      //（即该行的终态，如 "Counting objects: 100% (33/33), done."），
      // 与终端最终显示一致。
      const collapsed = text
        .split("\n")
        .map((ln) => {
          const i = ln.lastIndexOf("\r");
          return (i >= 0 ? ln.slice(i + 1) : ln).trimEnd();
        })
        .filter((ln) => ln.trim() !== "")
        .join("\n");
      if (!collapsed) return {};
      const arr = [...(s.logs[k] ?? []).slice(-500), { time: nowStr(), text: collapsed, kind }];
      return { logs: { ...s.logs, [k]: arr } };
    }),

  notifyProxy: (proxy) => {
    if (proxyToastTimer) clearTimeout(proxyToastTimer);
    set({ proxyToast: { text: `本次 git 操作已自动通过代理 ${proxy} 完成，不影响系统网络`, at: Date.now() } });
    proxyToastTimer = setTimeout(() => set({ proxyToast: null }), 6000);
  },

  dismissProxy: () => {
    if (proxyToastTimer) clearTimeout(proxyToastTimer);
    proxyToastTimer = null;
    set({ proxyToast: null });
  },

  notifyAuto: (text, kind = "info") => {
    if (autoToastTimer) clearTimeout(autoToastTimer);
    set({ autoToast: { text, kind, at: Date.now() } });
    autoToastTimer = setTimeout(() => set({ autoToast: null }), 8000);
  },

  dismissAuto: () => {
    if (autoToastTimer) clearTimeout(autoToastTimer);
    autoToastTimer = null;
    set({ autoToast: null });
  },

  setBusy: (op) => set({ busy: op, progress: op ? get().progress : null }),

  setProgress: (p) => set({ progress: p }),

  checkUpdate: async () => {
    const path = get().selected;
    if (!path || get().busy || get().cloning) return;
    get().setBusy("正在检查云端更新…");
    const channel = new Channel<ProgressEvent>();
    channel.onmessage = (p) => get().setProgress(p);
    try {
      const r = await api.checkUpdate(path, channel);
      get().log(r.output.trim() || "(无输出)");
      get().log(r.message, r.ok ? "success" : "error");
      if (r.used_proxy) {
        set({ lastProxy: r.used_proxy });
        get().notifyProxy(r.used_proxy);
        get().log(`本次操作已自动通过代理 ${r.used_proxy} 完成`, "proxy");
      }
      if (r.status) set((s) => ({ status: { ...s.status, [path]: r.status! } }));
    } catch (e) {
      get().log(String(e), "error");
    } finally {
      get().setBusy(null);
      get().setProgress(null);
    }
  },

  pull: async (stash) => {
    const path = get().selected;
    if (!path || get().busy || get().cloning) return "fail";
    get().setBusy("正在拉取…");
    const channel = new Channel<ProgressEvent>();
    channel.onmessage = (p) => get().setProgress(p);
    try {
      const r = await api.pull(path, stash, channel);
      if (r.output.trim()) get().log(r.output.trim());
      get().log(r.message, r.ok ? "success" : "error");
      if (r.used_proxy) {
        set({ lastProxy: r.used_proxy });
        get().notifyProxy(r.used_proxy);
        get().log(`本次操作已自动通过代理 ${r.used_proxy} 完成`, "proxy");
      }
      if (r.status) set((s) => ({ status: { ...s.status, [path]: r.status! } }));
      if (r.ok && r.changes) {
        set((s) => ({ changes: { ...s.changes, [path]: r.changes! } }));
        const host = remoteHost(r.status?.remote_url ?? "");
        const src = host === "gitee" ? "Gitee" : host === "github" ? "GitHub" : "云端";
        get().log(`已从 ${src} 同步 ${r.changes.commits.length} 个新提交`, "success");
      } else if (r.ok) {
        set((s) => {
          const c = { ...s.changes };
          delete c[path];
          return { changes: c };
        });
      }
      if (r.need_stash) return "dirty";
      return r.ok ? "ok" : "fail";
    } catch (e) {
      get().log(String(e), "error");
      return "fail";
    } finally {
      get().setBusy(null);
      get().setProgress(null);
    }
  },

  push: async () => {
    const path = get().selected;
    if (!path || get().busy || get().cloning) return;
    get().setBusy("正在推送…");
    try {
      const r = await api.push(path);
      if (r.output.trim()) get().log(r.output.trim());
      get().log(r.message, r.ok ? "success" : "error");
      if (r.used_proxy) {
        set({ lastProxy: r.used_proxy });
        get().notifyProxy(r.used_proxy);
        get().log(`本次操作已自动通过代理 ${r.used_proxy} 完成`, "proxy");
      }
      if (r.status) set((s) => ({ status: { ...s.status, [path]: r.status! } }));
    } catch (e) {
      get().log(String(e), "error");
    } finally {
      get().setBusy(null);
    }
  },

  cloneRepo: (url, parentDir, netMode) => {
    if (get().busy || get().cloning) return false;
    if (!url.trim() || !parentDir.trim()) return false;
    const target = get().selected ?? "_app";
    set({ cloning: "正在克隆仓库…" });
    get().log(`开始克隆：${url}`, "info", target);
    const channel = new Channel<ProgressEvent>();
    // 克隆不走弹窗：进度写入主面板忙碌条，同时逐行打进日志区（终端式），克隆期间可自由查看
    channel.onmessage = (p) => {
      get().setProgress(p);
      const pct = p.percent != null ? ` ${Math.round(p.percent)}%` : "";
      const detail = p.detail ? ` · ${p.detail}` : "";
      get().log(`${p.stage}${pct}${detail}`, "info", target);
    };
    void (async () => {
      try {
        const r = await api.clone(url, parentDir, channel, netMode);
        sortRepos(r.repos);
        set({ repos: r.repos });
        get().select(r.path);
        get().log(`克隆完成，已自动添加仓库：${r.path}`, "success", target);
      } catch (e) {
        get().log(String(e), "error", target);
      } finally {
        set({ cloning: null });
        get().setProgress(null);
      }
    })();
    return true;
  },

  cancelOp: async () => {
    await api.cancelBusyOp().catch(() => {});
  },

  runRepoCmd: async (path, cmd) => {
    // 日志固定写入发起命令的仓库，中途切换选中仓库也不会串行
    const key = path;
    const channel = new Channel<string>();
    channel.onmessage = (line) => get().log(line, "info", key);
    set({ cmdRunning: cmd });
    try {
      await api.runRepoCmd(path, cmd, channel);
      get().log(`命令执行完成：${cmd}`, "success", key);
    } catch (e) {
      get().log(String(e), "error", key);
    } finally {
      // 只有仍然是自己时才清空，避免误清新启动命令的状态
      if (get().cmdRunning === cmd) set({ cmdRunning: null });
    }
  },

  stopRepoCmd: async () => {
    await api.stopRepoCmd().catch((e) => get().log(String(e), "error"));
  },

  commitPush: async (message) => {
    const path = get().selected;
    if (!path || get().busy || get().cloning) return false;
    get().setBusy("正在提交并推送…");
    try {
      const r = await api.commitPush(path, message);
      if (r.output.trim()) get().log(r.output.trim());
      get().log(r.message, r.ok ? "success" : "error");
      if (r.used_proxy) {
        set({ lastProxy: r.used_proxy });
        get().notifyProxy(r.used_proxy);
        get().log(`本次操作已自动通过代理 ${r.used_proxy} 完成`, "proxy");
      }
      if (r.status) set((s) => ({ status: { ...s.status, [path]: r.status! } }));
      return r.ok;
    } catch (e) {
      get().log(String(e), "error");
      return false;
    } finally {
      get().setBusy(null);
    }
  },

  openInFolder: (path) => {
    void api.openInFolder(path).catch((e) => get().log(String(e), "error"));
  },

  openInTerminal: (path) => {
    void api.openInTerminal(path).catch((e) => get().log(String(e), "error"));
  },

  openReadme: (path) => {
    void api.openReadme(path).catch((e) => get().log(String(e), "error"));
  },

  openRepoWebsite: (path) => {
    void api.openRepoWebsite(path).catch((e) => get().log(String(e), "error"));
  },

  scanAll: async () => {
    if (get().busy) return;
    const n = get().repos.length;
    if (n === 0) return;
    get().setBusy(`正在扫描 ${n} 个仓库状态…`);
    try {
      const items = await api.scanAll();
      const status = { ...get().status };
      for (const it of items) status[it.path] = it.status;
      set({ status });
      const missing = items.filter((i) => !i.status.is_repo).length;
      const totalBehind = items.reduce((a, i) => a + (i.status.is_repo ? i.status.behind : 0), 0);
      const totalAhead = items.reduce((a, i) => a + (i.status.is_repo ? i.status.ahead : 0), 0);
      get().log(
        `扫描完成：${items.length - missing} 个可用` +
          (missing ? `，${missing} 个失联` : "") +
          (totalBehind ? `，共落后云端 ${totalBehind} 个提交` : "") +
          (totalAhead ? `，共 ${totalAhead} 个提交待推送` : ""),
        "success",
      );
    } catch (e) {
      get().log(String(e), "error");
    } finally {
      get().setBusy(null);
    }
  },

  setTheme: async (t) => {
    set({ theme: t });
    applyThemeClass(t);
    try {
      await api.setTheme(t);
    } catch (e) {
      get().log(`主题切换失败：${e}`, "error");
    }
  },

  initTheme: async () => {
    try {
      const s = await api.getSettings();
      const t = (["dark", "light", "system"].includes(s.theme) ? s.theme : "dark") as ThemeMode;
      set({ theme: t, lastProxy: s.last_good_proxy });
      applyThemeClass(t);
    } catch {
      applyThemeClass("dark");
    }
  },
}));

/** 监听托盘菜单的"一键扫描全部仓库"事件 */
export function listenTrayScan() {
  return listen("scan-all", () => {
    void useApp.getState().scanAll();
  });
}

/** 自动同步间隔档位；mins = -1 表示"每天定时"模式（时间点由 daily_time 指定） */
export const AUTO_SYNC_OPTIONS = [
  { mins: 0, label: "关闭" },
  { mins: 15, label: "15分钟" },
  { mins: 30, label: "30分钟" },
  { mins: 60, label: "1小时" },
  { mins: 360, label: "6小时" },
  { mins: 720, label: "12小时" },
  { mins: -1, label: "每天定时" },
];

/** 每天定时模式的档位哨兵值 */
export const AUTO_SYNC_DAILY = -1;

/** 档位展示文案；每天定时模式显示时间点，如 "每天 09:30" */
export function autoSyncLabel(mins: number, dailyTime?: string): string {
  if (dailyTime) return `每天 ${dailyTime}`;
  return AUTO_SYNC_OPTIONS.find((o) => o.mins === mins)?.label ?? `${mins}分钟`;
}

/** 监听后台定时自动同步事件：更新仓库列表 / 更新卡片 / 日志与通知 */
export function listenAutoSync() {
  return listen<AutoSyncEvent>("auto-sync-event", (e) => {
    const ev = e.payload;
    const { repos } = ev;
    sortRepos(repos);
    useApp.setState({ repos });
    if (ev.kind === "pulled") {
      useApp.setState((s) => ({
        changes: {
          ...s.changes,
          [ev.path]: { commits: ev.commits, diff_stat: ev.diff_stat },
        },
      }));
    }
    const { log, notifyAuto, refreshStatus } = useApp.getState();
    log(`[自动同步] ${ev.message}`, ev.kind === "error" ? "error" : ev.kind === "ok" ? "info" : "success", ev.path);
    if (ev.kind !== "ok") {
      notifyAuto(`「${ev.name}」${ev.message}`, ev.kind === "error" ? "error" : "info");
    }
    void refreshStatus(ev.path);
  });
}
