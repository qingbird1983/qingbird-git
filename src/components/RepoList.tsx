import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type SpaceReport } from "../api";
import { useApp, autoSyncLabel } from "../store";
import { fmtSize } from "./SpacePanel";
import SpacePanel from "./SpacePanel";
import {
  IconRepo, IconFilePlus, IconDownload, IconPlus,
  IconPencil, IconX, IconSync, IconPin, IconSearch,
} from "./icons";

export default function RepoList({
  onOpenClone,
  onOpenNew,
  onNeedInit,
}: {
  onOpenClone: () => void;
  onOpenNew: () => void;
  onNeedInit: (path: string) => void;
}) {
  const { repos, selected, status, select, addRepoByPath, removeRepo, renameRepo, busy, scanAll, openRepoWebsite, pinRepo } =
    useApp();
  const dragging = useApp((s) => s.dragging);
  const [editing, setEditing] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  // 搜索筛选：匹配仓库名或自定义别名（不区分大小写），只影响显示不影响数据
  const [query, setQuery] = useState("");
  // 头部双胶囊切换：仓库列表 / 所选仓库的容量面板
  const [tab, setTab] = useState<"repos" | "space">("repos");
  // 容量扫描：胶囊上常显所选仓库的总占用；按路径缓存，切换仓库时后台重扫保持新鲜
  const sizeCache = useRef(new Map<string, number>());
  const [spaceReport, setSpaceReport] = useState<SpaceReport | null>(null);
  const [spaceError, setSpaceError] = useState("");
  const [spaceLoading, setSpaceLoading] = useState(false);
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

  const scanSpace = useCallback(async (path: string) => {
    setSpaceLoading(true);
    setSpaceError("");
    try {
      const rep = await api.scanRepoSpace(path);
      sizeCache.current.set(path, rep.total);
      setSpaceReport(rep);
    } catch (e) {
      setSpaceError(String(e));
    } finally {
      setSpaceLoading(false);
    }
  }, []);

  useEffect(() => {
    setSpaceReport(null);
    setSpaceError("");
    if (selected) void scanSpace(selected);
  }, [selected, scanSpace]);

  const cachedTotal = selected ? sizeCache.current.get(selected) : undefined;
  const capText = selected
    ? spaceReport
      ? fmtSize(spaceReport.total)
      : cachedTotal != null
        ? fmtSize(cachedTotal)
        : "…"
    : "";

  return (
    <>
      <div className="sidebar-header">
        <div className="segmented sidebar-tabs" role="tablist">
          <button
            className={`segment seg-repos ${tab === "repos" ? "segment-active" : ""}`}
            role="tab"
            aria-selected={tab === "repos"}
            title="仓库列表"
            onClick={() => setTab("repos")}
          >
            仓库
            <span className="seg-num">{repos.length}</span>
          </button>
          <button
            className={`segment seg-space ${tab === "space" ? "segment-active" : ""}`}
            role="tab"
            aria-selected={tab === "space"}
            disabled={!selected}
            title={selected ? "所选仓库的磁盘占用与缓存清理" : "先选择一个仓库"}
            onClick={() => setTab("space")}
          >
            容量
            <span className="seg-num">{capText}</span>
          </button>
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

      {tab === "repos" && (
        <>
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
        {visible.map((r) => {
          const st = status[r.path];
          const missing = st && !st.is_repo;
          return (
            <div
              key={r.path}
              className={`repo-item ${selected === r.path ? "active" : ""}`}
              onClick={() => select(r.path)}
              onDoubleClick={() => startRename(r.path, r.alias)}
              title={`${r.path}\n双击可设置自定义显示名`}
            >
              <span className="repo-lead">
                {/* 有云端远端时小图标可点击，用默认浏览器打开仓库主页 */}
                {st?.remote_url ? (
                  <button
                    className="repo-icon repo-icon-btn"
                    title="打开云端仓库主页（默认浏览器）"
                    onClick={(e) => {
                      e.stopPropagation();
                      openRepoWebsite(r.path);
                    }}
                  >
                    <IconRepo size={15} />
                  </button>
                ) : (
                  <span className="repo-icon">
                    <IconRepo size={15} />
                  </span>
                )}
              </span>
              <div className="repo-info">
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
                      {r.name}
                      {r.alias && <span className="alias-flag" title="已设置自定义显示名">名</span>}
                    </span>
                  )}
                  {/* 悬停显示在仓库名右侧的操作钮 */}
                  <span className="repo-tools">
                    <button
                      className={`repo-tool ${r.pinned ? "pinned" : ""}`}
                      title={r.pinned ? "取消置顶" : "置顶（常驻列表最前）"}
                      onClick={(e) => {
                        e.stopPropagation();
                        void pinRepo(r.path, !r.pinned);
                      }}
                    >
                      <IconPin size={12} />
                    </button>
                    <button
                      className="repo-tool"
                      title="重命名（仅改本地显示名）"
                      onClick={(e) => {
                        e.stopPropagation();
                        startRename(r.path, r.alias);
                      }}
                    >
                      <IconPencil size={12} />
                    </button>
                    <button
                      className="repo-tool danger"
                      title="移除"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleRemove(r.path, r.name);
                      }}
                    >
                      <IconX size={12} />
                    </button>
                  </span>
                  <span className="repo-badges">
                    {r.pinned && (
                      <span className="badge badge-pin" title="已置顶">
                        顶
                      </span>
                    )}
                    {(r.auto_sync_mins ?? 0) > 0 || r.daily_time ? (
                      <span
                        className="badge badge-auto"
                        title={`定时自动同步：${autoSyncLabel(r.auto_sync_mins ?? 0, r.daily_time)}`}
                      >
                        ⟳
                      </span>
                    ) : null}
                    {missing && <span className="badge badge-missing">失联</span>}
                    {st?.dirty && <span className="badge badge-dirty">●</span>}
                    {st && st.behind > 0 && <span className="badge badge-behind">↓{st.behind}</span>}
                    {st && st.ahead > 0 && <span className="badge badge-ahead">↑{st.ahead}</span>}
                  </span>
                </div>
                {r.alias && (
                  <div className="repo-item-sub">
                    <span className="repo-alias" title={`自定义显示名（原名：${r.name}）`}>
                      {r.alias}
                    </span>
                  </div>
                )}
              </div>
            </div>
          );
        })}
      </div>
        </>
      )}

      {tab === "space" && (
        selected ? (
          <SpacePanel
            selected={selected}
            report={spaceReport}
            error={spaceError}
            loading={spaceLoading}
            onRescan={() => selected && void scanSpace(selected)}
          />
        ) : (
          <div className="empty-tip">
            先选择一个仓库
            <br />
            即可查看它的磁盘占用与可清理缓存
          </div>
        )
      )}

      {dragging && (
        <div className="drop-overlay">
          <div className="drop-hint">松开即可添加仓库文件夹</div>
        </div>
      )}
    </>
  );
}
