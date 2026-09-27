import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import TitleBar from "./components/TitleBar";
import RepoList from "./components/RepoList";
import RepoPanel from "./components/RepoPanel";
import SettingsModal from "./components/SettingsModal";
import CloneModal from "./components/CloneModal";
import NewRepoModal from "./components/NewRepoModal";
import InitRepoModal from "./components/InitRepoModal";
import { useApp } from "./store";
import { listenAutoSync, listenTrayScan } from "./lib/appEvents";
import { IconCloudDown, IconSync, IconX } from "./components/icons";
import "./App.css";

const SIDEBAR_DEFAULT_W = 300;

function App() {
  const { loadRepos, initTheme, addRepoByPath, log } = useApp();
  const proxyToast = useApp((s) => s.proxyToast);
  const dismissProxy = useApp((s) => s.dismissProxy);
  const autoToast = useApp((s) => s.autoToast);
  const dismissAuto = useApp((s) => s.dismissAuto);
  // 侧栏常挂载 + 宽度/透明度过渡做平滑收展（--panel-anim）；
  // 内容包在 .panel-clip 里保持固定宽，收缩时只被裁切不被挤压。
  // 开关入口在自建标题栏的品牌印章钮上（TitleBar.tsx）。
  // 拖宽条可调宽度（SIDEBAR_MIN_W..SIDEBAR_MAX_W）；拖拽中关掉过渡避免跟手延迟。
  const sidebarOpen = useApp((s) => s.sidebarOpen);
  const sidebarWidth = useApp((s) => s.sidebarWidth);
  const showSettings = useApp((s) => s.settingsOpen);
  const [resizing, setResizing] = useState(false);

  const startResize = (e: React.MouseEvent) => {
    e.preventDefault();
    setResizing(true);
    const onMove = (ev: MouseEvent) => useApp.getState().setSidebarWidth(ev.clientX);
    const onUp = () => {
      setResizing(false);
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };
  const [showClone, setShowClone] = useState(false);
  const [showNew, setShowNew] = useState(false);
  const [initPath, setInitPath] = useState<string | null>(null);

  useEffect(() => {
    void loadRepos();
    void initTheme();
    const unlisteners: Promise<() => void>[] = [listenTrayScan(), listenAutoSync()];

    // 拖拽文件夹到窗口任意位置即可添加仓库
    unlisteners.push(
      getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === "drop") {
          useApp.setState({ dragging: false });
          const paths = event.payload.paths;
          if (paths.length === 0) return;
          void (async () => {
            let ok = 0;
            let firstInvalid: string | null = null;
            for (const p of paths) {
              if (await addRepoByPath(p)) {
                ok++;
              } else if (!firstInvalid) {
                firstInvalid = p;
              }
            }
            if (ok < paths.length) {
              const n = paths.length - ok;
              log(
                n === 1
                  ? "拖入的文件夹不是 git 仓库，可在此初始化"
                  : `有 ${n} 个拖入项不是 git 仓库（未找到 .git）`,
                "error",
              );
              // 单个非仓库文件夹 → 打开初始化面板
              if (n === 1 && firstInvalid) setInitPath(firstInvalid);
            }
          })();
        } else if (event.payload.type === "enter" || event.payload.type === "over") {
          useApp.setState({ dragging: true });
        } else if (event.payload.type === "leave") {
          useApp.setState({ dragging: false });
        }
      }),
    );
    return () => {
      for (const p of unlisteners) void p.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="app">
      <TitleBar />
      <div className="app-body">
        <aside
          className={`sidebar${resizing ? " resizing" : ""}`}
          style={{
            width: sidebarOpen ? sidebarWidth : 0,
            opacity: sidebarOpen ? 1 : 0,
          }}
        >
          <div className="panel-clip" style={{ width: sidebarWidth }}>
            <RepoList
              onOpenClone={() => setShowClone(true)}
              onOpenNew={() => setShowNew(true)}
              onNeedInit={(p) => setInitPath(p)}
            />
          </div>
        </aside>
        {/* 拖宽条：骑在侧栏右缝上，按住左右拖调宽，双击恢复默认宽度 */}
        {sidebarOpen && (
          <div
            className={`app-resizer${resizing ? " active" : ""}`}
            title="拖动调整仓库栏宽度（双击恢复默认）"
            onMouseDown={startResize}
            onDoubleClick={() => useApp.getState().setSidebarWidth(SIDEBAR_DEFAULT_W)}
          />
        )}
        <RepoPanel />
      </div>
      {showSettings && <SettingsModal onClose={useApp.getState().closeSettings} />}
      {showClone && <CloneModal onClose={() => setShowClone(false)} />}
      {showNew && <NewRepoModal onClose={() => setShowNew(false)} />}
      {initPath && <InitRepoModal path={initPath} onClose={() => setInitPath(null)} />}
      {proxyToast && (
        <div className="proxy-toast" key={proxyToast.at}>
          <IconCloudDown size={15} className="proxy-toast-icon" />
          <span className="proxy-toast-text">{proxyToast.text}</span>
          <button className="proxy-toast-close" title="关闭" onClick={dismissProxy}>
            <IconX size={11} />
          </button>
        </div>
      )}
      {autoToast && (
        <div className={`proxy-toast auto-toast-${autoToast.kind}`} key={autoToast.at}>
          <IconSync size={15} className="proxy-toast-icon" />
          <span className="proxy-toast-text">{autoToast.text}</span>
          <button className="proxy-toast-close" title="关闭" onClick={dismissAuto}>
            <IconX size={11} />
          </button>
        </div>
      )}
    </div>
  );
}

export default App;
