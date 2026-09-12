import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, remoteHost } from "../api";
import { useApp } from "../store";

export default function RepoList({
  onOpenClone,
  onOpenNew,
  onOpenSettings,
  onNeedInit,
}: {
  onOpenClone: () => void;
  onOpenNew: () => void;
  onOpenSettings: () => void;
  onNeedInit: (path: string) => void;
}) {
  const { repos, selected, status, select, addRepoByPath, removeRepo, renameRepo, busy, scanAll, theme, setTheme } =
    useApp();
  const dragging = useApp((s) => s.dragging);
  const [editing, setEditing] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");

  const handleAdd = async () => {
    const picked = await open({ directory: true, multiple: false, title: "选择 git 仓库文件夹" });
    if (typeof picked !== "string") return;
    // 非 git 仓库文件夹 → 引导一键初始化
    const st = await api.status(picked).catch(() => null);
    if (st && !st.is_repo) {
      onNeedInit(picked);
      return;
    }
    const added = await addRepoByPath(picked);
    if (!added) onNeedInit(picked);
  };

  const handleRemove = (path: string, name: string) => {
    if (window.confirm(`从列表移除仓库「${name}」？\n（只移除记录，不会删除磁盘上的文件）`)) {
      void removeRepo(path);
    }
  };

  const startRename = (path: string, current: string) => {
    setEditing(path);
    setEditValue(current);
  };

  const commitRename = async () => {
    if (editing) {
      await renameRepo(editing, editValue.trim());
    }
    setEditing(null);
  };

  const cycleTheme = () => {
    const next = theme === "dark" ? "light" : theme === "light" ? "system" : "dark";
    void setTheme(next);
  };

  const themeIcon = theme === "dark" ? "🌙" : theme === "light" ? "☀️" : "🌗";
  const themeTitle = `主题：${theme === "dark" ? "暗色" : theme === "light" ? "亮色" : "跟随系统"}（点击切换）`;

  return (
    <aside className={`sidebar ${dragging ? "sidebar-dragging" : ""}`}>
      <div className="sidebar-header">
        <span className="brand">
          <img src="/appicon.png" alt="" className="brand-icon" />
          青鸟 Git
        </span>
        <div className="header-actions">
          <button
            className="icon-btn"
            title="新建仓库（本地初始化，可同步创建到 GitHub / Gitee）"
            onClick={onOpenNew}
            disabled={!!busy}
          >
            ✦
          </button>
          <button className="icon-btn" title="克隆新仓库（粘贴地址下载）" onClick={onOpenClone} disabled={!!busy}>
            ⤓
          </button>
          <button className="icon-btn" title="添加已有仓库文件夹" onClick={() => void handleAdd()} disabled={!!busy}>
            ＋
          </button>
        </div>
      </div>

      <div className="repo-list">
        {repos.length === 0 && (
          <div className="empty-tip">
            还没有仓库
            <br />
            点击右上角 ＋ 添加，或把仓库文件夹
            <br />
            直接拖进窗口
          </div>
        )}
        {repos.map((r) => {
          const st = status[r.path];
          const missing = st && !st.is_repo;
          const host = st ? remoteHost(st.remote_url) : null;
          const display = r.alias || r.name;
          return (
            <div
              key={r.path}
              className={`repo-item ${selected === r.path ? "active" : ""}`}
              onClick={() => select(r.path)}
              onDoubleClick={() => startRename(r.path, r.alias)}
              title={`${r.path}\n双击可设置自定义显示名`}
            >
              <div className="repo-item-main">
                {editing === r.path ? (
                  <input
                    className="rename-input"
                    autoFocus
                    value={editValue}
                    placeholder="自定义显示名（如中文名）"
                    onClick={(e) => e.stopPropagation()}
                    onChange={(e) => setEditValue(e.target.value)}
                    onBlur={() => void commitRename()}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void commitRename();
                      if (e.key === "Escape") setEditing(null);
                    }}
                  />
                ) : (
                  <span className="repo-name">
                    {display}
                    {r.alias && <span className="alias-flag" title="自定义显示名">名</span>}
                  </span>
                )}
                <span className="repo-badges">
                  {host && host !== "other" && (
                    <span className={`badge badge-${host}`}>{host === "gitee" ? "Gitee" : "GitHub"}</span>
                  )}
                  {missing && <span className="badge badge-missing">失联</span>}
                  {st?.dirty && <span className="badge badge-dirty">●</span>}
                  {st && st.behind > 0 && <span className="badge badge-behind">↓{st.behind}</span>}
                  {st && st.ahead > 0 && <span className="badge badge-ahead">↑{st.ahead}</span>}
                </span>
              </div>
              <span className="repo-branch">
                {r.alias ? `${r.name}${st?.is_repo && st.branch ? ` · ${st.branch}` : ""}` : st?.is_repo ? st.branch || "—" : ""}
              </span>
              <span className="repo-tools">
                <button
                  className="repo-tool"
                  title="重命名（仅改本地显示名）"
                  onClick={(e) => {
                    e.stopPropagation();
                    startRename(r.path, r.alias);
                  }}
                >
                  ✎
                </button>
                <button
                  className="repo-tool"
                  title="移除"
                  onClick={(e) => {
                    e.stopPropagation();
                    handleRemove(r.path, r.name);
                  }}
                >
                  ×
                </button>
              </span>
            </div>
          );
        })}
      </div>

      <div className="sidebar-footer">
        <button className="pill-btn" title="扫描全部仓库的云端/本地状态" onClick={() => void scanAll()} disabled={!!busy}>
          ⟳ 扫描全部
        </button>
        <div className="footer-icons">
          <button className="icon-btn" title={themeTitle} onClick={cycleTheme}>
            {themeIcon}
          </button>
          <button className="icon-btn" title="设置" onClick={onOpenSettings}>
            ⚙
          </button>
        </div>
      </div>

      {dragging && (
        <div className="drop-overlay">
          <div className="drop-hint">松开即可添加仓库文件夹</div>
        </div>
      )}
    </aside>
  );
}
