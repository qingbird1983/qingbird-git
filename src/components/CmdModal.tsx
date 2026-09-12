import { useEffect, useState } from "react";
import { api } from "../api";
import { useApp } from "../store";
import { IconX } from "./icons";

interface Props {
  /** 首次要运行的是测试还是构建 */
  kind: "test" | "build";
  onClose: () => void;
}

/** 首次点击“跑测试 / 构建”时的命令设置：自动识别预填，保存后记住，下次一键直达 */
export default function CmdModal({ kind, onClose }: Props) {
  const selected = useApp((s) => s.selected);
  const setRepoCmds = useApp((s) => s.setRepoCmds);
  const log = useApp((s) => s.log);
  const [testCmd, setTestCmd] = useState("");
  const [buildCmd, setBuildCmd] = useState("");
  const [detected, setDetected] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!selected) return;
    void api
      .detectCmds(selected)
      .then((d) => {
        setTestCmd(d.test_cmd);
        setBuildCmd(d.build_cmd);
        setDetected(!!(d.test_cmd || d.build_cmd));
      })
      .catch(() => setDetected(false));
  }, [selected]);

  const save = async (run: boolean) => {
    if (!selected) return;
    setSaving(true);
    try {
      await setRepoCmds(selected, testCmd.trim(), buildCmd.trim());
      log(`已保存一键运行命令`, "success");
      if (run) {
        const cmd = (kind === "test" ? testCmd : buildCmd).trim();
        if (!cmd) {
          log(kind === "test" ? "测试命令为空，请先填写" : "构建命令为空，请先填写", "error");
        } else {
          await api.runRepoCmd(selected, cmd);
          log(`已在终端窗口运行：${cmd}`, "info");
        }
      }
      onClose();
    } catch (e) {
      log(String(e), "error");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>
          设置一键运行命令
          <button className="popover-close modal-close-right" title="关闭" onClick={onClose}>
            <IconX size={13} />
          </button>
        </h3>
        <div className="modal-desc">
          {detected
            ? "已根据项目类型自动识别命令，可修改；保存后下次点击按钮直接在终端运行。"
            : "未能自动识别项目类型，请手动填写命令（在仓库目录下执行）。"}
        </div>
        <div className="modal-col">
          <label className="modal-label">测试命令</label>
          <input
            className="modal-input"
            placeholder="如：cargo test / npm test / pytest"
            value={testCmd}
            onChange={(e) => setTestCmd(e.target.value)}
          />
          <label className="modal-label">构建命令</label>
          <input
            className="modal-input"
            placeholder="如：cargo build --release / npm run build"
            value={buildCmd}
            onChange={(e) => setBuildCmd(e.target.value)}
          />
        </div>
        <div className="modal-footer">
          <button className="btn-sm btn-ghost" disabled={saving} onClick={() => void save(false)}>
            仅保存
          </button>
          <button className="btn-sm" disabled={saving} onClick={() => void save(true)}>
            保存并运行
          </button>
        </div>
      </div>
    </div>
  );
}
