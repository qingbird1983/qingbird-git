/** 自动同步间隔档位；mins = -1 表示"每天定时"模式（时间点由 daily_time 指定） */
export const AUTO_SYNC_OPTIONS = [
  { mins: 0, label: "关闭" },
  { mins: 60, label: "1小时" },
  { mins: 360, label: "6小时" },
  { mins: 720, label: "12小时" },
  { mins: -1, label: "每天定时" },
];

/** 每天定时模式的档位哨兵值 */
export const AUTO_SYNC_DAILY = -1;

/** 档位展示文案；每天定时模式显示时间点，如 "每天 09:30" */
export function autoSyncLabel(mins: number, dailyTime?: string): string {
  if (dailyTime) return `每天 ${dailyTime}`;
  return AUTO_SYNC_OPTIONS.find((o) => o.mins === mins)?.label ?? `${mins}分钟`;
}
