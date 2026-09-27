import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { useApp } from "../store";
import RepoItem from "./RepoItem";
import { IconFilePlus, IconDownload, IconPlus, IconX, IconSync, IconSearch } from "./icons";

export default function RepoList({
  onOpenClone,
  onOpenNew,
  onNeedInit,
}: {
  onOpenClone: () => void;
  onOpenNew: () => void;
  onNeedInit: (path: string) => void;
}) {
  const {
    repos, selected, status, select, addRepoByPath, removeRepo, renameRepo,
    busy, scanAll, openRepoWebsite, pinRepo,
  } = useApp();
  const dragging = useApp((s) => s.dragging);
  const [editing, setEditing] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  // 搜索筛选：匹配仓库名或自定义别名（不区分大小写），只影响显示不影响数据
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const visible = q
    ? repos.filter(
        (r) =>
          r.name.toLowerCase().includes(q) ||
          (r.alias ?? "").toLowerCase().includes(q),
      )
    : repos;

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

  // 切换仓库时退出重命名/编辑态，避免输入框挂着上一条仓库的名字
  useEffect(() => {
    setEditing(null);
  }, [selected]);

  return (
    <>
      <div className="sidebar-header">
        <div className="sidebar-title-row">
          <span className="sidebar-title">仓库</span>
          <span className="sidebar-count">{repos.length}</span>
        </div>
        <div className="header-actions">
          <button
            className="icon-btn"
            title="新建仓库（本地初始化，可同步创建到 GitHub / Gitee）"
            onClick={onOpenNew}
            disabled={!!busy}
          >
            <IconFilePlus size={15} />
          </button>
          <button className="icon-btn" title="克隆新仓库（粘贴地址下载）" onClick={onOpenClone} disabled={!!busy}>
            <IconDownload size={15} />
          </button>
          <button
            className="icon-btn"
            title="扫描全部仓库的云端/本地状态"
            onClick={() => void scanAll()}
            disabled={!!busy}
          >
            <IconSync size={15} />
          </button>
          <button className="icon-btn" title="添加已有仓库文件夹" onClick={() => void handleAdd()} disabled={!!busy}>
            <IconPlus size={15} />
          </button>
        </div>
      </div>

      <div className="repo-search">
        <IconSearch size={13} className="repo-search-icon" />
        <input
          className="repo-search-input"
          placeholder="搜索仓库名 / 别名"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") setQuery("");
          }}
        />
        {query && (
          <button className="repo-search-clear" title="清空搜索" onClick={() => setQuery("")}>
            <IconX size={11} />
          </button>
        )}
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
        {repos.length > 0 && visible.length === 0 && (
          <div className="empty-tip">没有匹配「{query.trim()}」的仓库</div>
        )}
        {visible.map((r) => (
          <RepoItem
            key={r.path}
            repo={r}
            status={status[r.path]}
            active={selected === r.path}
            editing={editing === r.path}
            editValue={editValue}
            onSelect={() => select(r.path)}
            onStartRename={() => startRename(r.path, r.alias)}
            onEditValue={setEditValue}
            onCommitRename={() => void commitRename()}
            onCancelRename={() => setEditing(null)}
            onRemove={() => handleRemove(r.path, r.name)}
            onPin={() => void pinRepo(r.path, !r.pinned)}
            onOpenWebsite={() => openRepoWebsite(r.path)}
          />
        ))}
      </div>

      {dragging && (
        <div className="drop-overlay">
          <div className="drop-hint">松开即可添加仓库文件夹</div>
        </div>
      )}
    </>
  );
}
