import type { FileChange } from "../../api";
import { IconX } from "../icons";

interface Props {
  /** 未提交的文件变更（最多展示 8 条） */
  files: FileChange[];
  /** 提交说明（受控，由父组件持有，提交成功后清空） */
  commitMsg: string;
  /** 网络操作进行中：禁用输入与提交 */
  netBusy: boolean;
  onCommitMsg: (value: string) => void;
  onSubmit: () => void;
  onClose: () => void;
}

/** 待提交变更浮层：文件列表 + 提交说明输入 + 提交并推送。 */
export default function ChangesPopover({ files, commitMsg, netBusy, onCommitMsg, onSubmit, onClose }: Props) {
  return (
                  <div className="commit-popover">
                    <div className="update-header">
                      <span className="commit-title">待提交变更（{files.length} 个文件）</span>
                      <button className="popover-close" title="关闭" onClick={() => onClose()}>
                        <IconX size={13} />
                      </button>
                    </div>
                    {files.length === 0 ? (
                      <div className="commit-more">正在读取变更列表…</div>
                    ) : (
                      <div className="file-list">
                        {files.slice(0, 8).map((f, i) => (
                          <div key={i} className="file-line">
                            <span className={`file-code file-${f.code.replace("?", "u")}`}>{f.code}</span>
                            <span className="file-path">{f.path}</span>
                          </div>
                        ))}
                        {files.length > 8 && (
                          <div className="commit-more">…还有 {files.length - 8} 个文件</div>
                        )}
                      </div>
                    )}
                    <div className="commit-row">
                      <input
                        className="modal-input"
                        placeholder="提交说明，如：修复登录问题"
                        value={commitMsg}
                        disabled={netBusy}
                        onChange={(e) => onCommitMsg(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" && commitMsg.trim()) void onSubmit();
                        }}
                      />
                      <button className="btn-sm" disabled={netBusy || !commitMsg.trim()} onClick={() => void onSubmit()}>
                        提交并推送
                      </button>
                    </div>
                  </div>
  );
}
