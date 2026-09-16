import { useEffect, useState } from "react";
import { api, type CommitDetail, type CommitInfo, type CommitPatch } from "../api";
import { useApp } from "../store";
import { changeLabel, patchLineKind, visiblePatchLines } from "../lib/commitDiff";

interface Props {
  onClose: () => void;
}

/**
 * 提交历史：左侧列提交，点一条看它改了哪些文件，再点文件看 patch。
 *
 * 合并提交按第一个父提交算变更（后端做这件事，见 `commands/history.rs`）：
 * `git show <merge>` 默认什么都不输出，不处理的话点开合并提交会是空白。
 */
export default function HistoryModal({ onClose }: Props) {
  const selected = useApp((s) => s.selected);
  const log = useApp((s) => s.log);
  const [commits, setCommits] = useState<CommitInfo[] | null>(null);
  const [hash, setHash] = useState<string | null>(null);
  const [detail, setDetail] = useState<CommitDetail | null>(null);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [file, setFile] = useState<string | null>(null);
  const [patch, setPatch] = useState<CommitPatch | null>(null);
  const [loadingPatch, setLoadingPatch] = useState(false);

  useEffect(() => {
    setCommits(null);
    setHash(null);
    if (selected) {
      void api.recentCommits(selected).then(setCommits).catch(() => setCommits([]));
    }
  }, [selected]);

  // 选中提交 → 拉文件级摘要；切提交时清掉上一个文件与 patch
  useEffect(() => {
    if (!selected || !hash) {
      setDetail(null);
      return;
    }
    let stale = false;
    setLoadingDetail(true);
    setFile(null);
    setPatch(null);
    void api
      .commitDetail(selected, hash)
      .then((d) => {
        if (!stale) setDetail(d);
      })
      .catch((e) => {
        if (!stale) {
          setDetail(null);
          log(String(e), "error");
        }
      })
      .finally(() => {
        if (!stale) setLoadingDetail(false);
      });
    return () => {
      stale = true;
    };
  }, [selected, hash, log]);

  // 选中文件 → 拉 patch
  useEffect(() => {
    if (!selected || !hash || !file) {
      setPatch(null);
      return;
    }
    let stale = false;
    setLoadingPatch(true);
    void api
      .commitPatch(selected, hash, file)
      .then((p) => {
        if (!stale) setPatch(p);
      })
      .catch((e) => {
        if (!stale) {
          setPatch(null);
          log(String(e), "error");
        }
      })
      .finally(() => {
        if (!stale) setLoadingPatch(false);
      });
    return () => {
      stale = true;
    };
  }, [selected, hash, file, log]);

  const shown = patch ? visiblePatchLines(patch.text) : null;
  // 后端截断了正文、或界面只渲染了前若干行，都得给"去终端看"的出口
  const clipped = !!patch && (patch.truncated || (shown?.hidden ?? 0) > 0);

  const openFullDiff = () => {
    if (!selected || !hash || !file) return;
    void api.openCommitDiffInTerminal(selected, hash, file).catch((e) => log(String(e), "error"));
  };

  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal modal-wide" onClick={(e) => e.stopPropagation()}>
        <h3>提交历史</h3>
        <div className="history-body">
          <div className="history-list">
            {commits === null && <div className="log-empty">加载中…</div>}
            {commits?.length === 0 && <div className="log-empty">还没有任何提交</div>}
            {commits?.map((c) => (
              <button
                key={c.hash}
                className={`history-item ${hash === c.hash ? "history-item-active" : ""}`}
                onClick={() => setHash(c.hash)}
              >
                <div className="history-top">
                  <span className="commit-hash">{c.hash}</span>
                  <span className="commit-author">{c.author}</span>
                  <span className="commit-date">{c.date}</span>
                </div>
                <div className="commit-summary">{c.summary}</div>
              </button>
            ))}
          </div>

          <div className="history-detail">
            {!hash && <div className="log-empty">点左边一条提交，看它改了什么</div>}
            {hash && loadingDetail && <div className="log-empty">正在读取变更…</div>}
            {detail && (
              <>
                <div className="history-detail-head">
                  <span className="commit-hash">{detail.hash}</span>
                  <span className="history-detail-count">{detail.files.length} 个文件</span>
                  {detail.truncated && <span className="history-detail-warn">文件过多，未全部列出</span>}
                </div>
                <div className="history-files">
                  {detail.files.length === 0 && (
                    <div className="log-empty">这条提交没有文件变更（空提交或只改了合并关系）</div>
                  )}
                  {detail.files.map((f) => (
                    <button
                      key={f.path}
                      className={`history-file ${file === f.path ? "history-file-active" : ""}`}
                      title={f.path}
                      onClick={() => setFile(f.path)}
                    >
                      <span className="history-file-name">{f.path}</span>
                      <span className="history-file-count">{changeLabel(f)}</span>
                    </button>
                  ))}
                </div>

                {file && (
                  <div className="history-patch">
                    <div className="history-patch-head">
                      <span className="history-patch-title" title={file}>
                        {file}
                      </span>
                      {clipped && (
                        <button
                          className="btn-sm btn-ghost"
                          title="在仓库目录打开终端并执行同样的查看命令"
                          onClick={openFullDiff}
                        >
                          在终端看完整 diff
                        </button>
                      )}
                    </div>
                    {loadingPatch && <div className="log-empty">正在读取 patch…</div>}
                    {shown && (
                      <>
                        <pre className="history-patch-body">
                          {shown.lines.map((l, i) => (
                            <div key={i} className={`patch-line patch-${patchLineKind(l)}`}>
                              {l || " "}
                            </div>
                          ))}
                        </pre>
                        {shown.hidden > 0 && (
                          <div className="history-patch-note">
                            还有 {shown.hidden} 行没显示，用上面的按钮去终端看完整版
                          </div>
                        )}
                      </>
                    )}
                  </div>
                )}
              </>
            )}
          </div>
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
