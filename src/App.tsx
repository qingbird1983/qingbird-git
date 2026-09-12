import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import RepoList from "./components/RepoList";
import RepoPanel from "./components/RepoPanel";
import SettingsModal from "./components/SettingsModal";
import CloneModal from "./components/CloneModal";
import NewRepoModal from "./components/NewRepoModal";
import InitRepoModal from "./components/InitRepoModal";
import { useApp, listenTrayScan } from "./store";
import { IconCloudDown, IconX } from "./components/icons";
import "./App.css";

function App() {
  const { loadRepos, initTheme, addRepoByPath, log } = useApp();
  const proxyToast = useApp((s) => s.proxyToast);
  const dismissProxy = useApp((s) => s.dismissProxy);
  const [showSettings, setShowSettings] = useState(false);
  const [showClone, setShowClone] = useState(false);
  const [showNew, setShowNew] = useState(false);
  const [initPath, setInitPath] = useState<string | null>(null);

  useEffect(() => {
    void loadRepos();
    void initTheme();
    const unlisteners: Promise<() => void>[] = [listenTrayScan()];

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
      <RepoList
        onOpenClone={() => setShowClone(true)}
        onOpenNew={() => setShowNew(true)}
        onOpenSettings={() => setShowSettings(true)}
        onNeedInit={(p) => setInitPath(p)}
      />
      <RepoPanel />
      {showSettings && <SettingsModal onClose={() => setShowSettings(false)} />}
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
    </div>
  );
}

export default App;
