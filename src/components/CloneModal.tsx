import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useApp } from "../store";

interface Props {
  onClose: () => void;
}

export default function CloneModal({ onClose }: Props) {
  const { busy, progress, cloneRepo } = useApp();
  const [url, setUrl] = useState("");
  const [dir, setDir] = useState("");
  const cloning = !!busy && busy.includes("克隆");

  // 默认目标目录：上次克隆位置
  useEffect(() => {
    setDir(localStorage.getItem("qb.clone-dir") || "");
  }, []);

  const pickDir = async () => {
    const picked = await open({ directory: true, multiple: false, title: "选择克隆保存位置" });
    if (typeof picked === "string") {
      setDir(picked);
      localStorage.setItem("qb.clone-dir", picked);
    }
  };

  const handleClone = async () => {
    if (!url.trim() || !dir.trim()) return;
    const ok = await cloneRepo(url.trim(), dir.trim());
    if (ok) onClose();
  };

  return (
    <div className="modal-mask" onClick={cloning ? undefined : onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>克隆新仓库</h3>
        <p className="modal-desc">
          粘贴仓库地址（支持 GitHub / Gitee 等），克隆完成后自动加入管理列表。
          <br />
          例如 <code>https://gitee.com/user/repo.git</code>
        </p>
        <input
          className="modal-input modal-input-full"
          placeholder="仓库地址"
          value={url}
          disabled={cloning}
          onChange={(e) => setUrl(e.target.value)}
        />
        <div className="modal-row" style={{ marginTop: 10 }}>
          <input
            className="modal-input"
            placeholder="克隆保存位置（点击选择）"
            value={dir}
            disabled={cloning}
            readOnly
            onClick={() => void pickDir()}
          />
          <button className="btn-sm btn-ghost" disabled={cloning} onClick={() => void pickDir()}>
            选择…
          </button>
        </div>

        {cloning && (
          <div className="progress-box">
            <div className="progress-stage">
              {busy}
              {progress ? ` · ${progress.stage}` : ""}
              {progress?.percent != null ? ` ${Math.round(progress.percent)}%` : ""}
            </div>
            <div className="progress-track">
              <div
                className={`progress-fill ${progress?.percent == null ? "indeterminate" : ""}`}
                style={progress?.percent != null ? { width: `${Math.min(100, progress.percent)}%` } : undefined}
              />
            </div>
          </div>
        )}

        <div className="modal-footer">
          <button className="btn-sm btn-ghost" disabled={cloning} onClick={onClose}>
            取消
          </button>
          <button className="btn-sm" disabled={cloning || !url.trim() || !dir.trim()} onClick={() => void handleClone()}>
            开始克隆
          </button>
        </div>
      </div>
    </div>
  );
}
