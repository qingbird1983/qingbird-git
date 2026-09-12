import { useEffect, useState } from "react";
import { api, type DirItem } from "../api";
import { useApp } from "../store";

interface Props {
  path: string;
  onClose: () => void;
}

/** 常见建议排除的目录 */
const SUGGESTED = new Set([
  "node_modules", "dist", "build", "out", "target", ".idea", ".vs", "vendor",
  "__pycache__", ".gradle", "bin", "obj", "coverage", ".next", ".nuxt", "venv",
]);

export default function InitRepoModal({ path, onClose }: Props) {
  const { loadRepos } = useApp();
  const [items, setItems] = useState<DirItem[]>([]);
  const [excludes, setExcludes] = useState<Set<string>>(new Set());
  const [initializing, setInitializing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dirName = path.split(/[\\/]/).filter(Boolean).pop() || path;

  useEffect(() => {
    void api.listDirItems(path).then((list) => {
      setItems(list);
      // 自动勾选常见的构建产物 / IDE 目录
      const auto = new Set<string>(list.filter((i) => i.is_dir && SUGGESTED.has(i.name)).map((i) => i.name));
      setExcludes(auto);
    });
  }, [path]);

  const toggle = (name: string) => {
    setExcludes((s) => {
      const n = new Set(s);
      if (n.has(name)) n.delete(name);
      else n.add(name);
      return n;
    });
  };

  const handleInit = async () => {
    setInitializing(true);
    setError(null);
    useApp.getState().setBusy("正在初始化仓库…");
    try {
      const entry = await api.initRepo(path, [...excludes]);
      await loadRepos();
      useApp.getState().select(entry.path);
      useApp.getState().log(`仓库初始化完成：${entry.path}`, "success");
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setInitializing(false);
      useApp.getState().setBusy(null);
    }
  };

  return (
    <div className="modal-mask" onClick={initializing ? undefined : onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>初始化为 Git 仓库</h3>
        <p className="modal-desc">
          「{dirName}」还不是 git 仓库。初始化后自动生成首次提交。
          勾选不想纳入版本管理的文件或文件夹（会写入 <code>.gitignore</code>）：
        </p>
        <p className="panel-path" style={{ marginBottom: 10 }} title={path}>
          {path}
        </p>

        <div className="exclude-list">
          {items.length === 0 && <div className="log-empty">（空文件夹）</div>}
          {items.map((it) => (
            <label key={it.name} className="exclude-row" title={it.is_dir ? "文件夹" : "文件"}>
              <input
                type="checkbox"
                checked={excludes.has(it.name)}
                onChange={() => toggle(it.name)}
              />
              <span className={`exclude-name ${excludes.has(it.name) ? "excluded" : ""}`}>
                {it.is_dir ? "📁" : "📄"} {it.name}
              </span>
              {SUGGESTED.has(it.name) && it.is_dir && <span className="badge badge-missing">建议排除</span>}
            </label>
          ))}
        </div>

        {initializing && <div className="modal-result">正在初始化，请稍候…</div>}
        {error && <div className="modal-result error-text">{error}</div>}

        <div className="modal-footer">
          <button className="btn-sm btn-ghost" disabled={initializing} onClick={onClose}>
            取消
          </button>
          <button className="btn-sm" disabled={initializing} onClick={() => void handleInit()}>
            初始化仓库
          </button>
        </div>
      </div>
    </div>
  );
}
