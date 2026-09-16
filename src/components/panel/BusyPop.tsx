import type { ProgressEvent } from "../../api";
import { IconX } from "../icons";

interface Props {
  /** 当前操作文案（已去掉尾部省略号） */
  label: string;
  progress: ProgressEvent | null;
  /** 进度百分比；null 表示不确定进度（走马灯样式） */
  percent: number | null;
  onCancel: () => void;
}

/** 操作进行中的浮条：当前步骤、进度条与取消按钮。 */
export default function BusyPop({ label, progress, percent, onCancel }: Props) {
  return (
        <div className="busy-pop">
          <div className="busy-row">
            <span className="spinner" />
            <span>
              {label}
              {progress ? ` · ${progress.stage}` : ""}
              {progress?.detail ? ` · ${progress.detail}` : ""}
            </span>
            {percent != null && <span className="busy-pct">{Math.round(percent)}%</span>}
          </div>
          <div className="progress-track">
            <div
              className={`progress-fill ${percent == null ? "indeterminate" : ""}`}
              style={percent != null ? { width: `${percent}%` } : undefined}
            />
          </div>
          <button className="busy-cancel" title="取消当前操作" onClick={() => void onCancel()}>
            <IconX size={13} />
            取消
          </button>
        </div>
  );
}
