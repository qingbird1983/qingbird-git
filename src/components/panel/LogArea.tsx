import type { RefObject } from "react";
import type { LogLine } from "../../store";

interface Props {
  /** 当前选中仓库的日志（父组件用稳定引用传入） */
  logs: LogLine[];
  /** 正在运行的后台命令；null 表示空闲 */
  cmdRunning: string | null;
  /** 日志容器，父组件用它自动滚到底部 */
  logRef: RefObject<HTMLDivElement | null>;
  /** 终止后台命令 */
  onStop: () => void;
}

/** 操作日志区：日志按仓库分开显示，后台命令运行时提供终止按钮。 */
export default function LogArea({ logs, cmdRunning, logRef, onStop }: Props) {
  return (
      <div className="log-area card">
        <div className="log-header">
          <span>操作日志</span>
          {cmdRunning ? (
            <button className="btn-sm btn-danger" title={`终止后台命令：${cmdRunning}`} onClick={() => void onStop()}>
              终止
            </button>
          ) : (
            <span className="log-hint">后台命令运行时可在此终止</span>
          )}
        </div>
        <div className="log-body" ref={logRef}>
          {logs.length === 0 && <div className="log-empty">暂无日志</div>}
          {logs.map((l, i) => (
            <div key={i} className={`log-line log-${l.kind}`}>
              <span className="log-time">{l.time}</span>
              <span className="log-text">{l.text}</span>
            </div>
          ))}
        </div>
      </div>
  );
}
