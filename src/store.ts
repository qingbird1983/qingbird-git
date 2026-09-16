import { create } from "zustand";
import { Channel } from "@tauri-apps/api/core";
import { api, remoteHost, type ForkInfo, type OpResult, type ProgressEvent, type RepoEntry, type RepoStatus, type UpdateChanges } from "./api";
import { autoSyncLabel } from "./lib/autoSync";
import { sortRepos } from "./lib/repos";
import { loadSidebarWidth, saveSidebarWidth } from "./lib/sidebar";
import { applyThemeClass, normalizeTheme, type ThemeMode } from "./lib/theme";
import type { RepoCmds } from "./repoCmds";

export interface LogLine {
  time: string;
  text: string;
  kind: "info" | "success" | "error" | "proxy";
}

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

/**
 * 把后端返回的分叉信息落进 store。
 *
 * `forks` 每次都以后端为准（一次成功的变基就该让卡片消失）；
 * `backups` 只在后端确实返回时才写入——它是"刚做过变基"的凭据，
 * 不能被一次普通的检测/拉取顺手抹掉（那样用户就找不到撤销入口了），
 * 只在推送成功或撤销成功时显式清掉。
 */
function forkPatch(path: string, r: OpResult) {
  return (s: AppState) => {
    const forks = { ...s.forks };
    if (r.fork) forks[path] = r.fork;
    else delete forks[path];
    const backups = r.backup_branch ? { ...s.backups, [path]: r.backup_branch } : s.backups;
    return { forks, backups };
  };
}

/** 变基结果已经推上去了（或已撤销）：再"回退到备份"只会制造分叉，收起入口 */
function clearBackup(path: string) {
  return (s: AppState) => {
    const backups = { ...s.backups };
    delete backups[path];
    return { backups };
  };
}

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
  /** 每个仓库最近一次检测/拉取/推送发现的分叉（后端算好，无分叉即删除） */
  forks: Record<string, ForkInfo>;
  /** 每个仓库"刚做过变基"的备份分支名：有它才给撤销入口（会话级，不落盘） */
  backups: Record<string, string>;
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
  setRepoCmds: (path: string, cmds: RepoCmds) => Promise<void>;
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
  /** 备份并变基到云端（会改写本地历史，动手前后端强制先建备份分支） */
  rebaseFork: (stash: boolean) => Promise<void>;
  /** 撤销上次变基：硬回退到备份分支（界面上需二次确认） */
  undoRebase: () => Promise<void>;
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
  forks: {},
  backups: {},
  theme: "dark",
  dragging: false,
  sidebarOpen: true,
  toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
  settingsOpen: false,
  openSettings: () => set({ settingsOpen: true }),
  closeSettings: () => set({ settingsOpen: false }),
  sidebarWidth: loadSidebarWidth(),
  setSidebarWidth: (w) => set({ sidebarWidth: saveSidebarWidth(w) }),

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

  setRepoCmds: async (path, cmds) => {
    const repos = await api.setRepoCmds(path, cmds);
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
      set(forkPatch(path, r));
    } catch (e) {
      get().log(String(e), "error");
    } finally {
      get().setBusy(null);
      get().setProgress(null);
    }
  },

  rebaseFork: async (stash) => {
    const path = get().selected;
    if (!path || get().busy || get().cloning) return;
    get().setBusy("正在变基到云端…");
    const channel = new Channel<ProgressEvent>();
    channel.onmessage = (p) => get().setProgress(p);
    try {
      const r = await api.rebaseFork(path, stash, channel);
      if (r.output.trim()) get().log(r.output.trim());
      get().log(r.message, r.ok ? "success" : "error");
      if (r.used_proxy) {
        set({ lastProxy: r.used_proxy });
        get().notifyProxy(r.used_proxy);
        get().log(`本次操作已自动通过代理 ${r.used_proxy} 完成`, "proxy");
      }
      if (r.status) set((s) => ({ status: { ...s.status, [path]: r.status! } }));
      set(forkPatch(path, r));
      if (r.ok && r.backup_branch) {
        get().log(`备份分支 ${r.backup_branch} 停在变基前的位置，随时可以撤销`, "info");
      }
    } catch (e) {
      get().log(String(e), "error");
    } finally {
      get().setBusy(null);
      get().setProgress(null);
    }
  },

  undoRebase: async () => {
    const path = get().selected;
    if (!path || get().busy || get().cloning) return;
    get().setBusy("正在撤销变基…");
    try {
      const r = await api.undoRebase(path);
      if (r.output.trim()) get().log(r.output.trim());
      get().log(r.message, r.ok ? "success" : "error");
      if (r.status) set((s) => ({ status: { ...s.status, [path]: r.status! } }));
      set(forkPatch(path, r));
      // 撤销完成后收起入口，避免"撤销的撤销"这种绕圈操作
      if (r.ok) set(clearBackup(path));
    } catch (e) {
      get().log(String(e), "error");
    } finally {
      get().setBusy(null);
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
      set(forkPatch(path, r));
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
      set(forkPatch(path, r));
      // 变基结果一旦推上去就发布了，再回退只会制造新的分叉
      if (r.ok) set(clearBackup(path));
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
      set(forkPatch(path, r));
      // 提交并推送成功后同样是"已发布"，收起变基的回退入口
      if (r.ok) set(clearBackup(path));
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
      const t = normalizeTheme(s.theme);
      set({ theme: t, lastProxy: s.last_good_proxy });
      applyThemeClass(t);
    } catch {
      applyThemeClass("dark");
    }
  },
}));
