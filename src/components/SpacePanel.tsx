import { useEffect, useState } from "react";
import { api, type SpaceReport } from "../api";
import { useApp } from "../store";
import { IconSync } from "./icons";

const UNITS = ["B", "KB", "MB", "GB", "TB"];

/** 字节数 → 人类可读容量（B/KB/MB/GB/TB），仓库栏胶囊与容量面板共用 */
export function fmtSize(n: number): string {
  let v = n;
  let u = 0;
  while (v >= 1024 && u < UNITS.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v >= 100 || u === 0 ? Math.round(v) : v.toFixed(1)} ${UNITS[u]}`;
}

interface Props {
  selected: string;
  report: SpaceReport | null;
  error: string;
  loading: boolean;
  onRescan: () => void;
}

/** 容量面板：嵌入仓库栏的磁盘占用扫描与缓存清理（原弹窗内容内联到列表位） */
export default function SpacePanel({ selected, report, error, loading, onRescan }: Props) {
  const log = useApp((s) => s.log);
  const [confirmName, setConfirmName] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);

  useEffect(() => {
    setConfirmName(null);
    setDeleting(null);
  }, [selected]);

  const cacheTotal = report?.dirs.filter((d) => d.is_cache).reduce((a, d) => a + d.size, 0) ?? 0;
  const maxSize = report?.dirs[0]?.size ?? 1;

  const handleDelete = async (name: string) => {
    // 两步确认：第一次点击变为“确认删除”，再次点击才真正执行
    if (confirmName !== name) {
      setConfirmName(name);
      return;
    }
    setConfirmName(null);
    setDeleting(name);
    try {
      const freed = await api.deleteCacheDir(selected, name);
      log(`已清理 ${name}，释放 ${fmtSize(freed)}`, "success");
      onRescan();
    } catch (e) {
      log(String(e), "error");
    } finally {
      setDeleting(null);
    }
  };

  return (
    <div className="space-panel">
      <div className="space-panel-path" title={selected}>
        {selected}
      </div>
      {report && (
        <div className="space-summary">
          总占用 <b>{fmtSize(report.total)}</b>
          {cacheTotal > 0 && (
            <>
              ，其中可清理缓存 <b className="space-cache-size">{fmtSize(cacheTotal)}</b>
            </>
          )}
        </div>
      )}
      {error && <div className="log-empty space-error">{error}</div>}
      <div className="space-list">
        {!report && !error && loading && <div className="log-empty">正在扫描，大仓库可能需要几秒…</div>}
        {report?.dirs.map((d) => (
          <div key={d.name} className={`space-item ${d.size === 0 ? "space-zero" : ""}`}>
            <div className="space-info">
              <div className="space-name">
                {d.name}
                {d.name === ".git" && <span className="space-tag">Git 数据，勿删</span>}
                {d.is_cache && <span className="space-tag space-tag-cache">可清理</span>}
              </div>
              <div className="space-bar-track">
                <div
                  className={`space-bar-fill ${d.is_cache ? "is-cache" : ""}`}
                  style={{ width: `${Math.max((d.size / maxSize) * 100, d.size > 0 ? 2 : 0)}%` }}
                />
              </div>
            </div>
            <span className="space-size">{fmtSize(d.size)}</span>
            {d.is_cache && d.size > 0 && (
              <button
                className={`btn-sm ${confirmName === d.name ? "btn-danger" : "btn-ghost"}`}
                disabled={deleting === d.name}
                onClick={() => void handleDelete(d.name)}
              >
                {deleting === d.name ? "删除中…" : confirmName === d.name ? "确认删除" : "删除"}
              </button>
            )}
          </div>
        ))}
      </div>
      <button className="btn-sm btn-ghost space-panel-rescan" onClick={onRescan} disabled={loading}>
        <IconSync size={13} /> 重新扫描
      </button>
    </div>
  );
}
