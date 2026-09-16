import { AUTO_SYNC_DAILY, AUTO_SYNC_OPTIONS } from "../../lib/autoSync";
import { IconX } from "../icons";

interface Props {
  /** 已保存的间隔（分钟）；配合 daily 判断当前模式 */
  mins: number;
  /** "每天定时"的执行时刻 "HH:MM"，非空即每天定时模式 */
  daily: string;
  /** 检测到更新后是否自动拉取 */
  autoPull: boolean;
  /** 上次自动检查距今的文案；从未检查过为 null */
  lastCheckText: string | null;
  /** 上次自动同步结果摘要（已去掉 "ok:"/"err:" 前缀） */
  lastResult: string;
  /** 档位 / 时间点 / 自动拉取任一变化都回传完整三元组，由父组件落盘 */
  onChange: (mins: number, autoPull: boolean, daily: string) => void;
  onClose: () => void;
}

/** 定时自动同步弹层：间隔档位 / 每天定时时间点 / 检测到更新后自动拉取。
只负责交互，落盘由父组件转交 setAutoSync。 */
export default function AutoSyncPopover({ mins, daily, autoPull, lastCheckText, lastResult, onChange, onClose }: Props) {
  const on = mins > 0 || !!daily;
  // 切换档位：每天定时首次点入默认 09:00，切回间隔模式清空时间点
  const applyMins = (next: number) => {
    if (next === AUTO_SYNC_DAILY) onChange(0, autoPull, daily || "09:00");
    else onChange(next, autoPull, "");
  };
  return (
                <div className="commit-popover auto-sync-pop">
                  <div className="update-header">
                    <span className="commit-title">定时自动同步</span>
                    <button className="popover-close" title="关闭" onClick={() => onClose()}>
                      <IconX size={13} />
                    </button>
                  </div>
                  <p className="auto-sync-desc">
                    在后台按间隔检测云端更新（自动走直连/代理回退），仅对此仓库生效；
                    只做快进合并（--ff-only），本地分叉或有未提交变更时不会动你的代码。
                  </p>
                  <div className="segmented auto-sync-seg">
                    {AUTO_SYNC_OPTIONS.map((o) => (
                      <button
                        key={o.mins}
                        className={`segment ${
                          (o.mins === AUTO_SYNC_DAILY ? !!daily : mins === o.mins)
                            ? "segment-active"
                            : ""
                        }`}
                        onClick={() => applyMins(o.mins)}
                      >
                        {o.label}
                      </button>
                    ))}
                  </div>
                  {daily && (
                    <div className="auto-sync-daily-row">
                      <span>每天执行时刻</span>
                      <input
                        type="time"
                        className="auto-sync-time"
                        value={daily}
                        onChange={(e) => void onChange(0, autoPull, e.target.value)}
                      />
                    </div>
                  )}
                  <label className="switch-row auto-sync-switch">
                    <span>检测到更新后自动拉取</span>
                    <button
                      className={`switch ${autoPull ? "on" : ""}`}
                      role="switch"
                      aria-checked={autoPull}
                      title={on ? undefined : "请先选择间隔或每天定时开启"}
                      onClick={() => {
                        if (on) void onChange(mins, !autoPull, daily);
                      }}
                    >
                      <span className="switch-knob" />
                    </button>
                  </label>
                  <div className="auto-sync-meta">
                    {lastCheckText
                      ? `上次自动检查：${lastCheckText}${lastResult ? ` · ${lastResult}` : ""}`
                      : on
                        ? "已开启，稍后自动进行首次检查"
                        : "尚未开启，选择间隔或每天定时即可启用"}
                  </div>
                </div>
  );
}
