// 同步环：把「本地与云端差多少」压成一个可读的图形。
//
// 环的弧长 = 差异程度（落后+待推送越多环越满），环色 = 主状态：
// 落后=琥珀、待推送=青、已同步=绿、失联=灰。中心是大字数字，不用读句子。
// 纯展示，不发事件——点击行为由父组件决定。
import { IconDiff, IconRepo } from "../icons";

interface Props {
  /** 落后云端的提交数 */
  behind: number;
  /** 待推送的提交数 */
  ahead: number;
  /** 目录已经不是 git 仓库（失联） */
  broken: boolean;
  /** 没有配置上游分支 */
  noUpstream: boolean;
  size?: number;
}

const R = 46;
const C = 2 * Math.PI * R;

export default function SyncRing({ behind, ahead, broken, noUpstream, size = 104 }: Props) {
  // 差异越大弧越长；10 个提交为满环，再多也不画满，避免视觉夸张
  const load = Math.min(1, (behind + ahead) / 10);
  const offset = C * (1 - load);

  let color = "var(--ok)";
  if (broken) color = "var(--fg3)";
  else if (behind > 0) color = "var(--warn)";
  else if (ahead > 0) color = "var(--accent)";

  let center: React.ReactNode;
  let label: string;
  if (broken) {
    center = <IconRepo size={22} />;
    label = "仓库失联";
  } else if (behind > 0) {
    center = <div className="ring-v" style={{ color }}>↓{behind}</div>;
    label = "待拉取";
  } else if (ahead > 0) {
    center = <div className="ring-v" style={{ color }}>↑{ahead}</div>;
    label = "待推送";
  } else if (noUpstream) {
    center = <IconDiff size={20} />;
    label = "无上游";
  } else {
    center = <div className="ring-v" style={{ color }}>✓</div>;
    label = "已同步";
  }

  return (
    <div className="ring" style={{ width: size, height: size }}>
      <svg width={size} height={size} viewBox="0 0 104 104" style={{ transform: "rotate(-90deg)" }}>
        <circle cx="52" cy="52" r={R} fill="none" stroke="var(--line-soft)" strokeWidth="7" />
        <circle
          cx="52"
          cy="52"
          r={R}
          fill="none"
          stroke={color}
          strokeWidth="7"
          strokeLinecap="round"
          strokeDasharray={C}
          strokeDashoffset={offset}
          style={{ transition: "stroke-dashoffset 0.4s var(--ease), stroke 0.3s" }}
        />
      </svg>
      <div className="ring-c">
        <div>{center}</div>
        <div className="ring-l">{label}</div>
      </div>
    </div>
  );
}
