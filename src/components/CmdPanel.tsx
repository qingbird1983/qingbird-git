import { useEffect, useState } from "react";
import { api } from "../api";
import { useApp } from "../store";
import {
  CMD_KINDS,
  CMD_META,
  cmdsFromDetect,
  cmdsFromEntry,
  cmdsToSave,
  emptyCmds,
  prefillCmds,
  withCmd,
  type CmdKind,
  type RepoCmds,
} from "../repoCmds";
import {
  MAX_PINNED,
  addPinned,
  canAddPinned,
  normalizePinned,
  pinnedEquals,
  removePinned,
  updatePinned,
  type PinnedCmd,
} from "../lib/pinnedCmds";
import { IconX } from "./icons";

interface Props {
  onClose: () => void;
}

// 仓库没有操作区按钮时返回这个共享常量：selector / deps 里新建 [] 每次都是新引用，
// 会让下面的 effect 每次都重跑（与 RepoPanel 的 EMPTY_LOGS 同一个坑）
const EMPTY_PINNED: PinnedCmd[] = [];

/**
 * 运行命令面板：测试 / 构建 / 开发 / 安装依赖四条命令都在这里改，
 * 点「启动」= 保存并后台运行，输出实时写入日志窗口（可随时在日志头部终止）；
 * 「自动」= 填入该行的自动识别结果并保存（不运行），识别不准时可直接改输入框。
 *
 * 四条命令是四个互相独立的字段，保存时按名字取值（见 repoCmds.ts），
 * 改一条不会连带改掉其他三条。
 *
 * 下半部分可以另外钉最多 3 个「操作区按钮」——内容任意（lint / clippy / 部署脚本都行），
 * 钉好后在主面板操作区一键运行，跑的是同一条后台管道。
 */
export default function CmdPanel({ onClose }: Props) {
  const selected = useApp((s) => s.selected);
  const repos = useApp((s) => s.repos);
  const setRepoCmds = useApp((s) => s.setRepoCmds);
  const setPinnedCmds = useApp((s) => s.setPinnedCmds);
  const runRepoCmd = useApp((s) => s.runRepoCmd);
  const cmdRunning = useApp((s) => s.cmdRunning);
  const log = useApp((s) => s.log);
  const entry = selected ? repos.find((r) => r.path === selected) : undefined;

  // 已保存的命令（旧配置里没有 dev/install，缺省读成空串）
  const saved = cmdsFromEntry(entry);
  const { test: savedTest, build: savedBuild, dev: savedDev, install: savedInstall } = saved;

  // 自动识别结果：只用来预填空行 + 供「自动」按钮回填，不直接落盘
  const [detected, setDetected] = useState<RepoCmds>(emptyCmds);
  const [cmds, setCmds] = useState<RepoCmds>(emptyCmds);
  const [running, setRunning] = useState(false);

  // 操作区按钮：钉住的命令 + 编辑中的草稿
  const savedPinnedRaw = entry?.pinned_cmds;
  const savedPinned = savedPinnedRaw ?? EMPTY_PINNED;
  const [pinned, setPinned] = useState<PinnedCmd[]>(savedPinned);
  const [pinnedSaved, setPinnedSaved] = useState(false);

  // 已保存的命令优先展示；没保存过（含旧配置缺字段）才用自动识别结果预填
  useEffect(() => {
    if (!selected) return;
    const path = selected;
    let stale = false;
    setCmds({ test: savedTest, build: savedBuild, dev: savedDev, install: savedInstall });
    setPinned(savedPinned);
    setPinnedSaved(false);
    void api
      .detectCmds(path)
      .then((d) => {
        // 识别期间用户切换了仓库：这条结果已经不属于当前面板，丢弃
        if (stale) return;
        const det = cmdsFromDetect(d);
        setDetected(det);
        // 只补空：用户已经看到 / 改过的值不被识别结果顶掉
        setCmds((cur) => prefillCmds(cur, det));
      })
      .catch(() => {});
    return () => {
      stale = true;
    };
  }, [selected, savedTest, savedBuild, savedDev, savedInstall, savedPinnedRaw]);

  const busy = running || !!cmdRunning;
  const pinnedDirty = !pinnedEquals(normalizePinned(pinned), savedPinned);

  /** 保存四条命令（统一去首尾空白；空串=未设置，下次打开重新用识别结果预填） */
  const saveCmds = async (next: RepoCmds) => {
    if (!selected) return;
    await setRepoCmds(selected, next);
  };

  /** 保存操作区按钮：空行丢掉、文案兜底都由 normalizePinned 统一处理 */
  const savePinned = async () => {
    if (!selected) return;
    setRunning(true);
    try {
      const next = normalizePinned(pinned);
      await setPinnedCmds(selected, next);
      setPinned(next);
      setPinnedSaved(true);
      setTimeout(() => setPinnedSaved(false), 2000);
    } catch (e) {
      log(String(e), "error");
    } finally {
      setRunning(false);
    }
  };

  /** 点某行「启动」：先保存再运行，只有该行用输入框里的最新值，其余三行原样 */
  const saveAndRun = async (kind: CmdKind, cmd: string) => {
    if (!selected || !cmd.trim()) return;
    setRunning(true);
    try {
      await saveCmds(cmdsToSave(kind, cmd, cmds));
      await runRepoCmd(selected, cmd.trim());
      onClose();
    } catch (e) {
      log(String(e), "error");
    } finally {
      setRunning(false);
    }
  };

  /** 点某行「自动」：该行恢复成自动识别结果并保存，不影响其他三行 */
  const restoreDetected = async (kind: CmdKind) => {
    if (!selected || !detected[kind]) return;
    const next = cmdsToSave(kind, detected[kind], cmds);
    setCmds(next);
    setRunning(true);
    try {
      await saveCmds(next);
      log(`已恢复「${CMD_META[kind].label}」的自动识别命令：${detected[kind]}`, "success");
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
          点「启动」即保存并运行，出错时改好再点一次即可。四条命令各自独立保存，
          识别不准时直接改输入框，点「自动」可恢复自动识别结果。
        </div>
        <div className="modal-col">
          {CMD_KINDS.map((kind) => (
            <div className="cmd-field" key={kind}>
              <label className="modal-label">
                {CMD_META[kind].label}
                <span className="cmd-state">
                  {cmds[kind].trim() !== saved[kind]
                    ? "未保存"
                    : saved[kind]
                      ? "已自定义"
                      : detected[kind]
                        ? `自动识别：${detected[kind]}`
                        : "未识别到命令"}
                </span>
              </label>
              <div className="cmd-row">
                <input
                  className="modal-input"
                  placeholder={CMD_META[kind].placeholder}
                  value={cmds[kind]}
                  onChange={(e) => setCmds((cur) => withCmd(cur, kind, e.target.value))}
                />
                <button
                  className="btn-sm btn-ghost"
                  title="填入自动识别结果并保存（不运行）"
                  disabled={busy || !detected[kind]}
                  onClick={() => void restoreDetected(kind)}
                >
                  自动
                </button>
                <button
                  className="btn-sm"
                  disabled={busy || !cmds[kind].trim()}
                  onClick={() => void saveAndRun(kind, cmds[kind])}
                >
                  启动
                </button>
              </div>
            </div>
          ))}
        </div>

        <p className="modal-subtitle">操作区按钮（最多 {MAX_PINNED} 个）</p>
        <div className="modal-desc">
          把常用命令钉到主面板操作区，点一下就跑、输出同样进日志窗口。
          文案留空会自动取命令的前两个词；这里写什么就跑什么，不经过上面的识别结果。
        </div>
        <div className="modal-col">
          {pinned.length === 0 && <div className="log-empty">还没有钉住任何命令</div>}
          {pinned.map((p, i) => (
            <div className="cmd-row" key={i}>
              <input
                className="modal-input cmd-pin-label"
                placeholder="按钮文案，如 检查"
                value={p.label}
                onChange={(e) => setPinned((cur) => updatePinned(cur, i, { label: e.target.value }))}
              />
              <input
                className="modal-input"
                placeholder="命令，如 pnpm run lint"
                value={p.cmd}
                onChange={(e) => setPinned((cur) => updatePinned(cur, i, { cmd: e.target.value }))}
              />
              <button
                className="btn-sm btn-ghost"
                title="移除这个按钮"
                disabled={busy}
                onClick={() => setPinned((cur) => removePinned(cur, i))}
              >
                移除
              </button>
            </div>
          ))}
          <div className="cmd-row cmd-pin-actions">
            <button
              className="btn-sm btn-ghost"
              disabled={busy || !canAddPinned(pinned)}
              title={canAddPinned(pinned) ? "再加一个按钮" : `最多只能钉 ${MAX_PINNED} 个`}
              onClick={() => setPinned((cur) => addPinned(cur))}
            >
              + 添加按钮
            </button>
            <span className="cmd-state">
              {pinnedDirty ? "未保存" : savedPinned.length > 0 ? "已保存" : "未设置"}
            </span>
            <button className="btn-sm" disabled={busy || !pinnedDirty} onClick={() => void savePinned()}>
              {pinnedSaved ? "✓ 已保存" : "保存按钮"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
