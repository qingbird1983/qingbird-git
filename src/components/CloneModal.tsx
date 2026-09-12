import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useApp } from "../store";

interface Props {
  onClose: () => void;
}

export default function CloneModal({ onClose }: Props) {
  const { cloning, cloneRepo } = useApp();
  const cloningActive = !!cloning;
  const [url, setUrl] = useState("");
  const [dir, setDir] = useState("");
  const [netMode, setNetMode] = useState<"auto" | "direct" | "proxy">(
    () => (localStorage.getItem("qb.net-mode") as "direct" | "proxy") || "auto",
  );

  // 默认目标目录：上次克隆位置
  useEffect(() => {
    setDir(localStorage.getItem("qb.clone-dir") || "");
  }, []);

  const pickRoute = (v: "auto" | "direct" | "proxy") => {
    setNetMode(v);
    localStorage.setItem("qb.net-mode", v);
  };

  const pickDir = async () => {
    const picked = await open({ directory: true, multiple: false, title: "选择克隆保存位置" });
    if (typeof picked === "string") {
      setDir(picked);
      localStorage.setItem("qb.clone-dir", picked);
    }
  };

  const handleClone = () => {
    if (!url.trim() || !dir.trim()) return;
    // 立即关闭弹窗：克隆在后台进行，进度看主面板忙碌条与日志区，不阻塞主面板
    const ok = cloneRepo(url.trim(), dir.trim(), netMode);
    if (ok) onClose();
  };

  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>克隆新仓库</h3>
        <p className="modal-desc">
          粘贴仓库地址（支持 GitHub / Gitee 等），克隆完成后自动加入管理列表。
          <br />
          点击开始后弹窗立即关闭，克隆在后台进行，可在主面板日志区实时查看。
          <br />
          例如 <code>https://gitee.com/user/repo.git</code>
        </p>
        <input
          className="modal-input modal-input-full"
          placeholder="仓库地址"
          value={url}
          disabled={cloningActive}
          onChange={(e) => setUrl(e.target.value)}
        />
        <div className="modal-row" style={{ marginTop: 10 }}>
          <input
            className="modal-input"
            placeholder="克隆保存位置（点击选择）"
            value={dir}
            disabled={cloningActive}
            readOnly
            onClick={() => void pickDir()}
          />
          <button className="btn-sm btn-ghost" disabled={cloningActive} onClick={() => void pickDir()}>
            选择…
          </button>
        </div>

        <div className="modal-row" style={{ marginTop: 10, alignItems: "center", gap: 8 }}>
          <span className="modal-label" style={{ margin: 0 }}>
            网络路线
          </span>
          <div className="segmented" style={{ flexShrink: 0 }}>
            {(
              [
                ["auto", "自动"],
                ["direct", "直连"],
                ["proxy", "代理"],
              ] as const
            ).map(([v, label]) => (
              <button
                key={v}
                disabled={cloningActive}
                className={`segment ${netMode === v ? "segment-active" : ""}`}
                onClick={() => pickRoute(v)}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        {cloningActive && (
          <p className="modal-desc" style={{ marginTop: 10 }}>
            已有克隆正在进行（进度见主面板忙碌条与日志区），完成后再开始下一个。
          </p>
        )}

        <div className="modal-footer">
          <button className="btn-sm btn-ghost" onClick={onClose}>
            关闭
          </button>
          <button className="btn-sm" disabled={cloningActive || !url.trim() || !dir.trim()} onClick={handleClone}>
            开始克隆
          </button>
        </div>
      </div>
    </div>
  );
}
