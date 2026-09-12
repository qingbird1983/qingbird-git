import { create } from "zustand";
import { Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { api, remoteHost, type ProgressEvent, type RepoEntry, type RepoStatus, type UpdateChanges } from "./api";

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

let proxyToastTimer: ReturnType<typeof setTimeout> | null = null;

interface AppState {
  repos: RepoEntry[];
  selected: string | null;
  status: Record<string, RepoStatus>;
  busy: string | null;
  logs: LogLine[];
  lastProxy: string | null;
  /** 当前显示的代理回退通知（null 表示不显示） */
  proxyToast: ProxyToast | null;
  /** 当前拉取/克隆进度（busy 时有值） */
  progress: ProgressEvent | null;
  /** 每个仓库最近一次拉取的更新内容 */
  changes: Record<string, UpdateChanges>;
  theme: ThemeMode;
  /** 有文件夹拖入窗口时为 true（显示拖放提示层） */
  dragging: boolean;
  loadRepos: () => Promise<void>;
  select: (path: string | null) => void;
  refreshStatus: (path: string) => Promise<void>;
  addRepoByPath: (path: string) => Promise<boolean>;
  removeRepo: (path: string) => Promise<void>;
  renameRepo: (path: string, alias: string) => Promise<void>;
  setRepoCmds: (path: string, testCmd: string, buildCmd: string) => Promise<void>;
  log: (text: string, kind?: LogLine["kind"]) => void;
  /** 弹出代理回退通知（约 6 秒后自动消失） */
  notifyProxy: (proxy: string) => void;
  dismissProxy: () => void;
  setBusy: (op: string | null) => void;
  setProgress: (p: ProgressEvent | null) => void;
  checkUpdate: () => Promise<void>;
  pull: (stash: boolean) => Promise<"ok" | "dirty" | "fail">;
  push: () => Promise<void>;
  commitPush: (message: string) => Promise<boolean>;
  cloneRepo: (url: string, parentDir: string) => Promise<boolean>;
  openInFolder: (path: string) => void;
  openInTerminal: (path: string) => void;
  scanAll: () => Promise<void>;
  setTheme: (t: ThemeMode) => Promise<void>;
  initTheme: () => Promise<void>;
}

function nowStr() {
  return new Date().toLocaleTimeString("zh-CN", { hour12: false });
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
  logs: [],
  lastProxy: null,
  proxyToast: null,
  progress: null,
  changes: {},
  theme: "dark",
  dragging: false,

  loadRepos: async () => {
    const repos = await api.listRepos();
    // 按最后使用时间倒序，最近使用的排前面
    repos.sort((a, b) => b.last_used - a.last_used);
    set({ repos });
    const selected = get().selected;
    if (!selected && repos.length > 0) {
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
      repos.sort((a, b) => b.last_used - a.last_used);
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
    repos.sort((a, b) => b.last_used - a.last_used);
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
    repos.sort((a, b) => b.last_used - a.last_used);
    set({ repos });
  },

  setRepoCmds: async (path, testCmd, buildCmd) => {
    const repos = await api.setRepoCmds(path, testCmd, buildCmd);
    repos.sort((a, b) => b.last_used - a.last_used);
    set({ repos });
  },

  log: (text, kind = "info") =>
    set((s) => ({
      logs: [...s.logs.slice(-500), { time: nowStr(), text, kind }],
    })),

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

  setBusy: (op) => set({ busy: op, progress: op ? get().progress : null }),

  setProgress: (p) => set({ progress: p }),

  checkUpdate: async () => {
    const path = get().selected;
    if (!path || get().busy) return;
    get().setBusy("正在检查云端更新…");
    try {
      const r = await api.checkUpdate(path);
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
    }
  },

  pull: async (stash) => {
    const path = get().selected;
    if (!path || get().busy) return "fail";
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
    if (!path || get().busy) return;
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

  cloneRepo: async (url, parentDir) => {
    if (get().busy) return false;
    get().setBusy("正在克隆仓库…");
    get().log(`开始克隆：${url}`);
    const channel = new Channel<ProgressEvent>();
    channel.onmessage = (p) => get().setProgress(p);
    try {
      const r = await api.clone(url, parentDir, channel);
      r.repos.sort((a, b) => b.last_used - a.last_used);
      set({ repos: r.repos });
      get().select(r.path);
      get().log(`克隆完成，已自动添加仓库：${r.path}`, "success");
      return true;
    } catch (e) {
      get().log(String(e), "error");
      return false;
    } finally {
      get().setBusy(null);
      get().setProgress(null);
    }
  },

  commitPush: async (message) => {
    const path = get().selected;
    if (!path || get().busy) return false;
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
