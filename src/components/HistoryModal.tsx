import { useEffect, useState } from "react";
import { api, type CommitInfo } from "../api";
import { useApp } from "../store";

interface Props {
  onClose: () => void;
}

export default function HistoryModal({ onClose }: Props) {
  const selected = useApp((s) => s.selected);
  const [commits, setCommits] = useState<CommitInfo[] | null>(null);

  useEffect(() => {
    if (selected) {
      void api.recentCommits(selected).then(setCommits).catch(() => setCommits([]));
    }
  }, [selected]);

  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal modal-wide" onClick={(e) => e.stopPropagation()}>
        <h3>提交历史</h3>
        <div className="history-list">
          {commits === null && <div className="log-empty">加载中…</div>}
          {commits?.length === 0 && <div className="log-empty">还没有任何提交</div>}
          {commits?.map((c) => (
            <div key={c.hash} className="history-item">
              <div className="history-top">
                <span className="commit-hash">{c.hash}</span>
                <span className="commit-author">{c.author}</span>
                <span className="commit-date">{c.date}</span>
              </div>
              <div className="commit-summary">{c.summary}</div>
            </div>
          ))}
        </div>
        <div className="modal-footer">
          <button className="btn-sm btn-ghost" onClick={onClose}>
            关闭
          </button>
        </div>
      </div>
    </div>
  );
}
