import { IconX } from "../icons";

interface Props {
  /** 本次拉取下来的提交摘要行 */
  commits: string[];
  /** 远端托管平台，用于标题后缀 */
  host: "gitee" | "github" | "other" | null;
  onClose: () => void;
}

/** 本次同步内容浮层：列出刚拉下来的提交（来自 Gitee / GitHub）。 */
export default function SyncContentPopover({ commits, host, onClose }: Props) {
  return (
                  <div className="commit-popover">
                    <div className="update-header">
                      <span className="commit-title">
                        本次同步内容
                        {host === "gitee" ? "（来自 Gitee）" : host === "github" ? "（来自 GitHub）" : ""}
                      </span>
                      <span className="update-count">{commits.length} 个新提交</span>
                      <button className="popover-close" title="关闭" onClick={() => onClose()}>
                        <IconX size={13} />
                      </button>
                    </div>
                    {commits.length === 0 ? (
                      <div className="commit-more">本次没有新提交（可能只是文件变更同步）</div>
                    ) : (
                      <div className="sync-commit-list">
                        {commits.map((c, i) => (
                          <div key={i} className="sync-commit-line">
                            {c}
                          </div>
                        ))}
                      </div>
                    )}
                    <div className="commit-more">文件变更明细见下方操作日志</div>
                  </div>
  );
}
