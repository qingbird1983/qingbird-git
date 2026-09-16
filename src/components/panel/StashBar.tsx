

interface Props {
  /** 暂存并拉取 */
  onConfirm: () => void;
  onCancel: () => void;
}

/** 拉取前的暂存确认条：本地有未提交变更时询问是否 stash 后拉取。 */
export default function StashBar({ onConfirm, onCancel }: Props) {
  return (
        <div className="stash-bar">
          <span>本地有未提交变更，是否先暂存（stash）再拉取，完成后自动恢复？</span>
          <div>
            <button className="btn-sm" onClick={() => void onConfirm()}>
              暂存并拉取
            </button>
            <button className="btn-sm btn-ghost" onClick={() => onCancel()}>
              取消
            </button>
          </div>
        </div>
  );
}
