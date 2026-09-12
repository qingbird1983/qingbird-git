import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { useApp } from "../store";

interface Props {
  onClose: () => void;
}

type Platform = "none" | "github" | "gitee";

export default function NewRepoModal({ onClose }: Props) {
  const { loadRepos } = useApp();
  const [name, setName] = useState("");
  const [dir, setDir] = useState("");
  const [platform, setPlatform] = useState<Platform>("none");
  const [isPrivate, setIsPrivate] = useState(false);
  const [description, setDescription] = useState("");
  const [error, setError] = useState<string | null>(null);
  const creating = useApp((s) => !!s.busy && s.busy.includes("创建"));

  useEffect(() => {
    setDir(localStorage.getItem("qb.create-dir") || localStorage.getItem("qb.clone-dir") || "");
  }, []);

  const pickDir = async () => {
    const picked = await open({ directory: true, multiple: false, title: "选择新仓库保存位置" });
    if (typeof picked === "string") {
      setDir(picked);
      localStorage.setItem("qb.create-dir", picked);
    }
  };

  const handleCreate = async () => {
    if (!name.trim() || !dir.trim()) return;
    setError(null);
    useApp.getState().setBusy("正在创建仓库…");
    try {
      const r = await api.createRepo(name.trim(), dir.trim(), platform, isPrivate, description.trim());
      await loadRepos();
      useApp.getState().select(r.path);
      useApp.getState().log(`新仓库已创建：${r.path}`, "success");
      useApp.getState().log(r.message, "success");
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      useApp.getState().setBusy(null);
    }
  };

  const nameOk = /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(name.trim());

  return (
    <div className="modal-mask" onClick={creating ? undefined : onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>新建仓库</h3>
        <p className="modal-desc">
          填个名字就能创建：自动初始化本地仓库并生成首次提交；也可以同时在 GitHub / Gitee
          创建远端仓库并完成首次推送（需要先在设置里配置对应令牌）。
        </p>

        <div className="modal-row">
          <input
            className="modal-input"
            placeholder="仓库名，如 my-project"
            value={name}
            disabled={creating}
            onChange={(e) => setName(e.target.value)}
          />
          {name && !nameOk && <span className="name-warn">名称含不支持的字符</span>}
        </div>
        <div className="modal-row" style={{ marginTop: 10 }}>
          <input
            className="modal-input"
            placeholder="保存位置（点击选择）"
            value={dir}
            disabled={creating}
            readOnly
            onClick={() => void pickDir()}
          />
          <button className="btn-sm btn-ghost" disabled={creating} onClick={() => void pickDir()}>
            选择…
          </button>
        </div>

        <p className="modal-subtitle">同时创建远端仓库</p>
        <div className="segmented">
          <button className={`segment ${platform === "none" ? "segment-active" : ""}`} onClick={() => setPlatform("none")} disabled={creating}>
            仅本地
          </button>
          <button className={`segment ${platform === "github" ? "segment-active" : ""}`} onClick={() => setPlatform("github")} disabled={creating}>
            GitHub
          </button>
          <button className={`segment ${platform === "gitee" ? "segment-active" : ""}`} onClick={() => setPlatform("gitee")} disabled={creating}>
            Gitee
          </button>
        </div>
        {platform !== "none" && (
          <>
            <label className="switch-row" style={{ marginTop: 10 }}>
              <span>私有仓库</span>
              <button
                className={`switch ${isPrivate ? "on" : ""}`}
                role="switch"
                aria-checked={isPrivate}
                onClick={() => setIsPrivate(!isPrivate)}
              >
                <span className="switch-knob" />
              </button>
            </label>
            <input
              className="modal-input modal-input-full"
              style={{ marginTop: 10 }}
              placeholder="仓库描述（可选）"
              value={description}
              disabled={creating}
              onChange={(e) => setDescription(e.target.value)}
            />
          </>
        )}

        {creating && <div className="modal-result">正在创建，请稍候…</div>}
        {error && <div className="modal-result error-text">{error}</div>}

        <div className="modal-footer">
          <button className="btn-sm btn-ghost" disabled={creating} onClick={onClose}>
            取消
          </button>
          <button
            className="btn-sm"
            disabled={creating || !name.trim() || !dir.trim() || !nameOk}
            onClick={() => void handleCreate()}
          >
            创建仓库
          </button>
        </div>
      </div>
    </div>
  );
}
