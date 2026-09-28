// 导航轨：工作区最左边一列，40px，只有"切屏"和两个全局动作。
//
// 为什么这么窄这么空：头部与侧栏已经堆过一轮图标按钮了（阶段 2 把它们收进了
// ⌘K 和检视抽屉）。这里不再塞功能入口，只回答一个问题——我现在在看哪一屏。
// 需要但少用的功能去 Ctrl K，常用的「添加 / 扫描」两颗放在底部常驻。

import { useApp } from "../store";
import { IconGrid, IconPlus, IconRepo, IconSync } from "./icons";

export default function NavRail() {
  const view = useApp((s) => s.view);
  const setView = useApp((s) => s.setView);
  const busy = useApp((s) => s.busy);
  const addRepoDialog = useApp((s) => s.addRepoDialog);
  const scanAll = useApp((s) => s.scanAll);

  return (
    <nav className="rail" aria-label="工作区视图">
      <button
        className={`rail-i${view === "repos" ? " on" : ""}`}
        title="仓库"
        aria-current={view === "repos"}
        onClick={() => setView("repos")}
      >
        <IconRepo size={17} />
      </button>
      <button
        className={`rail-i${view === "overview" ? " on" : ""}`}
        title="总览"
        aria-current={view === "overview"}
        onClick={() => setView("overview")}
      >
        <IconGrid size={17} />
      </button>

      <div className="rail-gap" />

      <button
        className="rail-i"
        title="添加已有仓库文件夹"
        disabled={!!busy}
        onClick={() => void addRepoDialog()}
      >
        <IconPlus size={17} />
      </button>
      <button
        className="rail-i"
        title="扫描全部仓库状态"
        disabled={!!busy}
        onClick={() => void scanAll()}
      >
        <IconSync size={17} />
      </button>
    </nav>
  );
}
