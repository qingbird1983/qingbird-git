import { useState } from "react";
import type { ForkInfo } from "../../api";
import { commitSectionTitle, forkView, type ForkAction } from "../../lib/forkGuide";

interface Props {
  /** 后端算好的分叉信息；undefined 表示当前没有分叉 */
  fork: ForkInfo | undefined;
  /** 刚做过变基时的备份分支名；有它才给"撤销变基" */
  backup: string | undefined;
  busy: boolean;
  onRebase: (stash: boolean) => void;
  onUndo: () => void;
}

/**
 * 分叉恢复向导卡片：坐在操作日志上方，把"ff 拉取失败"这种死胡同变成有出口的选择。
 * 有未提交变更时会先暂存，冲突时后端会自动中止并复原，因此只有"撤销变基"需要二次确认。
 */
export default function ForkCard({ fork, backup, busy, onRebase, onUndo }: Props) {
  const [showDiff, setShowDiff] = useState(false);
  const [confirming, setConfirming] = useState<string | null>(null);

  const view = forkView(fork, backup);
  if (!view) return null;

  const run = (a: ForkAction) => {
    if (a.danger && confirming !== a.id) {
      setConfirming(a.id);
      return;
    }
    setConfirming(null);
    if (a.id === "undo") onUndo();
    else onRebase(a.id === "rebase-stash");
  };

  const commitBlock = (kind: "local" | "remote", commits: string[]) => (
    <div className="fork-commits">
      <div className="fork-commits-title">{commitSectionTitle(kind, commits.length)}</div>
      {commits.length > 0 && (
        <div className="fork-commit-list">
          {commits.map((c, i) => (
            <div key={i} className="fork-commit-line">
              {c}
            </div>
          ))}
        </div>
      )}
    </div>
  );

  return (
    <div className="fork-card card">
      <div className="fork-head">
        <span className="fork-title">{view.title}</span>
        <span className="fork-summary">{view.summary}</span>
      </div>

      {view.blocked ? (
        <div className="fork-blocked">{view.blocked}</div>
      ) : (
        <div className="fork-actions">
          {view.actions.map((a) => (
            <span key={a.id} className="fork-action">
              <button
                className={`btn-sm ${a.danger ? "btn-danger" : ""}`}
                title={a.hint}
                disabled={busy}
                onClick={() => run(a)}
              >
                {confirming === a.id ? `确认${a.label}？` : a.label}
              </button>
              {confirming === a.id && (
                <button className="btn-sm btn-ghost" disabled={busy} onClick={() => setConfirming(null)}>
                  取消
                </button>
              )}
            </span>
          ))}
          {fork && (
            <button className="btn-sm btn-ghost" onClick={() => setShowDiff((v) => !v)}>
              {showDiff ? "收起差异" : "只看差异"}
            </button>
          )}
        </div>
      )}

      {fork && showDiff && (
        <div className="fork-diff">
          {commitBlock("local", fork.local_commits)}
          {commitBlock("remote", fork.remote_commits)}
          <div className="fork-hint">
            变基会把本地提交重放到 {fork.upstream || "云端"} 之上，提交内容不变但历史会改写；
            动手前后端会先把"变基前"的位置固定在备份分支上。
          </div>
        </div>
      )}
    </div>
  );
}
