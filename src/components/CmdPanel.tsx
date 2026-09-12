import { useEffect, useState } from "react";
import { api } from "../api";
import { useApp } from "../store";
import { IconX } from "./icons";

interface Props {
  onClose: () => void;
}

/**
 * 运行命令面板：测试 / 构建命令在这里随时可改，点启动即保存并后台运行，
 * 输出实时写入日志窗口（可随时在日志头部终止），命令填错也不会被卡死；
 * 另附按项目类型识别的快捷命令（开发 / 安装依赖）。
 */
export default function CmdPanel({ onClose }: Props) {
  const selected = useApp((s) => s.selected);
  const repos = useApp((s) => s.repos);
  const setRepoCmds = useApp((s) => s.setRepoCmds);
  const runRepoCmd = useApp((s) => s.runRepoCmd);
  const cmdRunning = useApp((s) => s.cmdRunning);
  const log = useApp((s) => s.log);
  const entry = selected ? repos.find((r) => r.path === selected) : undefined;
  const [testCmd, setTestCmd] = useState(entry?.test_cmd ?? "");
  const [buildCmd, setBuildCmd] = useState(entry?.build_cmd ?? "");
  const [presets, setPresets] = useState<{ name: string; cmd: string }[]>([]);
  const [running, setRunning] = useState(false);

  // 已保存的命令优先展示；没保存过才用自动识别结果预填
  useEffect(() => {
    if (!selected) return;
    void api
      .detectCmds(selected)
      .then((d) => {
        const list: { name: string; cmd: string }[] = [];
        if (d.dev_cmd) list.push({ name: "开发", cmd: d.dev_cmd });
        if (d.install_cmd) list.push({ name: "安装依赖", cmd: d.install_cmd });
        setPresets(list);
        setTestCmd((v) => v || d.test_cmd);
        setBuildCmd((v) => v || d.build_cmd);
      })
      .catch(() => {});
  }, [selected]);

  const run = async (cmd: string) => {
    if (!selected || !cmd.trim()) return;
    setRunning(true);
    try {
      await runRepoCmd(selected, cmd.trim());
      onClose();
    } catch (e) {
      log(String(e), "error");
    } finally {
      setRunning(false);
    }
  };

  // 一键命令：先保存再运行，保证下次主面板按钮直接用上最新命令
  const saveAndRun = async (kind: "test" | "build", cmd: string) => {
    if (!selected || !cmd.trim()) return;
    setRunning(true);
    try {
      await setRepoCmds(
        selected,
        kind === "test" ? cmd.trim() : (buildCmd.trim() || entry?.build_cmd || ""),
        kind === "build" ? cmd.trim() : (testCmd.trim() || entry?.test_cmd || ""),
      );
      log("已保存一键运行命令", "success");
      await runRepoCmd(selected, cmd.trim());
      onClose();
    } catch (e) {
      log(String(e), "error");
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>
          运行命令
          <button className="popover-close modal-close-right" title="关闭" onClick={onClose}>
            <IconX size={13} />
          </button>
        </h3>
        <div className="modal-desc">
          命令在后台运行，输出实时写入下方日志窗口，可随时在日志头部点「终止」；
          测试 / 构建命令点「启动」自动保存，出错时改好再点一次即可。
        </div>
        <div className="modal-col">
          <label className="modal-label">测试</label>
          <div className="cmd-row">
            <input
              className="modal-input"
              placeholder="如：cargo test / pnpm test / pytest"
              value={testCmd}
              onChange={(e) => setTestCmd(e.target.value)}
            />
            <button
              className="btn-sm"
              disabled={running || !!cmdRunning || !testCmd.trim()}
              onClick={() => void saveAndRun("test", testCmd)}
            >
              启动
            </button>
          </div>
          <label className="modal-label">构建</label>
          <div className="cmd-row">
            <input
              className="modal-input"
              placeholder="如：cargo build --release / pnpm run build"
              value={buildCmd}
              onChange={(e) => setBuildCmd(e.target.value)}
            />
            <button
              className="btn-sm"
              disabled={running || !!cmdRunning || !buildCmd.trim()}
              onClick={() => void saveAndRun("build", buildCmd)}
            >
              启动
            </button>
          </div>
        </div>
        {presets.length > 0 && (
          <div className="modal-col" style={{ marginTop: 12 }}>
            <label className="modal-label">快捷命令（按项目类型自动识别，直接运行不保存）</label>
            {presets.map((p) => (
              <div className="cmd-row" key={p.name}>
                <span className="cmd-name">{p.name}</span>
                <code className="cmd-text">{p.cmd}</code>
                <button
                  className="btn-sm btn-ghost"
                  disabled={running || !!cmdRunning}
                  onClick={() => void run(p.cmd)}
                >
                  启动
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
