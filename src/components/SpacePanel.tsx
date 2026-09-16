import { useEffect, useState } from "react";
import { api, type SpaceReport } from "../api";
import { useApp } from "../store";
import { IconFolder, IconSync } from "./icons";

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

/** 容量面板：嵌入仓库栏的磁盘占用扫描、缓存清理、大文件排查与 .gitignore 建议 */
export default function SpacePanel({ selected, report, error, loading, onRescan }: Props) {
  const log = useApp((s) => s.log);
  const openInFolder = useApp((s) => s.openInFolder);
  const [confirmName, setConfirmName] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);
  // .gitignore 会改动用户仓库里的文件，所以同样要两步确认
  const [confirmIgnore, setConfirmIgnore] = useState(false);
  const [writing, setWriting] = useState(false);

  useEffect(() => {
    setConfirmName(null);
    setDeleting(null);
    setConfirmIgnore(false);
  }, [selected]);

  const cacheTotal = report?.dirs.filter((d) => d.is_cache).reduce((a, d) => a + d.size, 0) ?? 0;
  const maxSize = report?.dirs[0]?.size ?? 1;
  const suggestions = report?.ignorable ?? [];
  // 已被跟踪的目录写进 .gitignore 是无效的：按钮只处理真正能生效的那些
  const writable = suggestions.filter((s) => !s.tracked);

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

  const handleWriteGitignore = async () => {
    if (!confirmIgnore) {
      setConfirmIgnore(true);
      return;
    }
    setConfirmIgnore(false);
    setWriting(true);
    try {
      const written = await api.addGitignoreEntries(selected, writable.map((s) => s.name));
      log(
        written.length > 0
          ? `已把 ${written.join("、")} 写入 .gitignore`
          : "这些目录已经被忽略规则覆盖，无需再写",
        "success",
      );
      onRescan();
    } catch (e) {
      log(String(e), "error");
    } finally {
      setWriting(false);
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

      {suggestions.length > 0 && (
        <div className="space-block">
          <div className="space-block-head">
            <span>建议加入 .gitignore</span>
            {writable.length > 0 && (
              <button
                className={`btn-sm ${confirmIgnore ? "btn-danger" : "btn-ghost"}`}
                disabled={writing}
                onClick={() => void handleWriteGitignore()}
              >
                {writing ? "写入中…" : confirmIgnore ? "确认写入" : "加入 .gitignore"}
              </button>
            )}
          </div>
          {suggestions.map((s) => (
            <div key={s.name} className="space-suggest">
              <span className="space-suggest-name">{s.name}</span>
              {s.tracked ? (
                <span
                  className="space-tag space-tag-warn"
                  title="目录里已经有被 git 跟踪的文件，写进 .gitignore 不会取消跟踪；需要时在终端执行 git rm -r --cached 该目录"
                >
                  已被跟踪
                </span>
              ) : (
                <span className="space-tag">未忽略</span>
              )}
            </div>
          ))}
          <div className="space-block-hint">
            这些目录是构建产物 / 依赖缓存，通常不该进版本库。
            {writable.length === 0
              ? "当前列出的目录都已被跟踪，写 .gitignore 无效，需要时请在终端执行 git rm -r --cached。"
              : "写入只追加内容，不改动你原有的规则。"}
          </div>
        </div>
      )}

      {(report?.largest.length ?? 0) > 0 && report && (
        <div className="space-block">
          <div className="space-block-head">
            <span>最大的文件</span>
            <span className="space-block-note">不含 .git 与缓存目录</span>
          </div>
          {report.largest.map((f) => {
            // 文件名是重点、目录是次要信息，拆成两行显示，省得靠 CSS 去截长路径
            const cut = f.name.lastIndexOf("/");
            const base = cut >= 0 ? f.name.slice(cut + 1) : f.name;
            const parent = cut >= 0 ? f.name.slice(0, cut) : "";
            return (
              <button
                key={f.name}
                className="space-item space-file"
                title={`${f.name}\n点击打开所在文件夹：${f.dir}`}
                onClick={() => openInFolder(f.dir)}
              >
                <IconFolder size={13} className="space-file-icon" />
                <span className="space-file-text">
                  <span className="space-file-name">{base}</span>
                  {parent && <span className="space-file-dir">{parent}</span>}
                </span>
                <span className="space-size">{fmtSize(f.size)}</span>
              </button>
            );
          })}
        </div>
      )}

      <button className="btn-sm btn-ghost space-panel-rescan" onClick={onRescan} disabled={loading}>
        <IconSync size={13} /> 重新扫描
      </button>
    </div>
  );
}
