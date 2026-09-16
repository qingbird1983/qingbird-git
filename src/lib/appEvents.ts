import { listen } from "@tauri-apps/api/event";
import type { AutoSyncEvent } from "../api";
import { useApp } from "../store";
import { sortRepos } from "./repos";

/** 监听托盘菜单的"一键扫描全部仓库"事件 */
export function listenTrayScan() {
  return listen("scan-all", () => {
    void useApp.getState().scanAll();
  });
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
