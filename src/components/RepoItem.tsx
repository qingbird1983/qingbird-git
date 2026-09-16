import type { RepoEntry, RepoStatus } from "../api";
import { autoSyncLabel } from "../lib/autoSync";
import { IconRepo, IconPencil, IconX, IconPin } from "./icons";

interface Props {
  repo: RepoEntry;
  /** 该仓库的 git 状态；还没扫描到时为 undefined */
  status?: RepoStatus;
  /** 是否为当前选中仓库 */
  active: boolean;
  /** 是否处于重命名编辑态（父组件保证同一时刻只有一条） */
  editing: boolean;
  /** 重命名输入框的值（受控） */
  editValue: string;
  onSelect: () => void;
  onStartRename: () => void;
  onEditValue: (value: string) => void;
  onCommitRename: () => void;
  onCancelRename: () => void;
  onRemove: () => void;
  onPin: () => void;
  onOpenWebsite: () => void;
}

/** 侧栏里的单个仓库条目：图标 / 名称 / 悬停操作钮 / 状态徽标，双击进入重命名 */
export default function RepoItem({
  repo,
  status,
  active,
  editing,
  editValue,
  onSelect,
  onStartRename,
  onEditValue,
  onCommitRename,
  onCancelRename,
  onRemove,
  onPin,
  onOpenWebsite,
}: Props) {
  const missing = !!status && !status.is_repo;
  return (
    <div
      className={`repo-item ${active ? "active" : ""}`}
              onClick={onSelect}
              onDoubleClick={onStartRename}
              title={`${repo.path}\n双击可设置自定义显示名`}
            >
              <span className="repo-lead">
                {/* 有云端远端时小图标可点击，用默认浏览器打开仓库主页 */}
                {status?.remote_url ? (
                  <button
                    className="repo-icon repo-icon-btn"
                    title="打开云端仓库主页（默认浏览器）"
                    onClick={(e) => {
                      e.stopPropagation();
                      onOpenWebsite();
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
                  {editing ? (
                    <input
                      className="rename-input"
                      autoFocus
                      value={editValue}
                      placeholder="自定义显示名（如中文名）"
                      onClick={(e) => e.stopPropagation()}
                      onChange={(e) => onEditValue(e.target.value)}
                      onBlur={() => void onCommitRename()}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void onCommitRename();
                        if (e.key === "Escape") onCancelRename();
                      }}
                    />
                  ) : (
                    <span className="repo-name">
                      {repo.name}
                      {repo.alias && <span className="alias-flag" title="已设置自定义显示名">名</span>}
                    </span>
                  )}
                  {/* 悬停显示在仓库名右侧的操作钮 */}
                  <span className="repo-tools">
                    <button
                      className={`repo-tool ${repo.pinned ? "pinned" : ""}`}
                      title={repo.pinned ? "取消置顶" : "置顶（常驻列表最前）"}
                      onClick={(e) => {
                        e.stopPropagation();
                        void onPin();
                      }}
                    >
                      <IconPin size={12} />
                    </button>
                    <button
                      className="repo-tool"
                      title="重命名（仅改本地显示名）"
                      onClick={(e) => {
                        e.stopPropagation();
                        onStartRename();
                      }}
                    >
                      <IconPencil size={12} />
                    </button>
                    <button
                      className="repo-tool danger"
                      title="移除"
                      onClick={(e) => {
                        e.stopPropagation();
                        onRemove();
                      }}
                    >
                      <IconX size={12} />
                    </button>
                  </span>
                  <span className="repo-badges">
                    {repo.pinned && (
                      <span className="badge badge-pin" title="已置顶">
                        顶
                      </span>
                    )}
                    {(repo.auto_sync_mins ?? 0) > 0 || repo.daily_time ? (
                      <span
                        className="badge badge-auto"
                        title={`定时自动同步：${autoSyncLabel(repo.auto_sync_mins ?? 0, repo.daily_time)}`}
                      >
                        ⟳
                      </span>
                    ) : null}
                    {missing && <span className="badge badge-missing">失联</span>}
                    {status?.dirty && <span className="badge badge-dirty">●</span>}
                    {status && status.behind > 0 && <span className="badge badge-behind">↓{status.behind}</span>}
                    {status && status.ahead > 0 && <span className="badge badge-ahead">↑{status.ahead}</span>}
                  </span>
                </div>
                {repo.alias && (
                  <div className="repo-item-sub">
                    <span className="repo-alias" title={`自定义显示名（原名：${repo.name}）`}>
                      {repo.alias}
                    </span>
                  </div>
                )}
              </div>
            </div>
  );
}
