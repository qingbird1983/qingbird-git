// 提交活跃度柱状图：近 14 天每天的提交数。
//
// 数据整形（补齐窗口、按比例定高）在 lib/activity.ts，这里只负责画。
// 14 根柱子挤在右侧统计条的窄列里，所以不标日期：靠 hover 的原生提示
// 说明每一天是几号、几次提交，两端只给「14 天前 / 今天」定向。
import { useMemo } from "react";
import type { ActivityDay } from "../../api";
import { ACTIVITY_WINDOW, barRatio, barTitle, fillActivityDays } from "../../lib/activity";

interface Props {
  /** 后端给的稀疏数据：只有有提交的日子，缺的天由 lib/activity 补 0 */
  days: ActivityDay[];
}

export default function ActivityChart({ days }: Props) {
  const bars = useMemo(() => fillActivityDays(days), [days]);
  const max = useMemo(() => bars.reduce((m, b) => Math.max(m, b.count), 0), [bars]);
  const total = useMemo(() => bars.reduce((s, b) => s + b.count, 0), [bars]);

  return (
    <div className="activity card">
      <div className="stats-head">
        近 {ACTIVITY_WINDOW} 天提交
        <span className="activity-total">{total}</span>
      </div>
      <div className="activity-bars">
        {bars.map((b, i) => (
          <div
            key={b.date}
            className={`activity-bar${b.count > 0 ? " has" : ""}${i === bars.length - 1 ? " today" : ""}`}
            title={barTitle(b.date, b.count)}
          >
            <div className="activity-fill" style={{ height: `${barRatio(b.count, max) * 100}%` }} />
          </div>
        ))}
      </div>
      <div className="activity-axis">
        <span>{ACTIVITY_WINDOW} 天前</span>
        <span>今天</span>
      </div>
    </div>
  );
}
