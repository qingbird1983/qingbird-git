import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import RepoList from "./components/RepoList";
import RepoPanel from "./components/RepoPanel";
import SettingsModal from "./components/SettingsModal";
import CloneModal from "./components/CloneModal";
import NewRepoModal from "./components/NewRepoModal";
import { useApp, listenTrayScan } from "./store";
import "./App.css";

function App() {
  const { loadRepos, initTheme, addRepoByPath, log } = useApp();
  const [showSettings, setShowSettings] = useState(false);
  const [showClone, setShowClone] = useState(false);
  const [showNew, setShowNew] = useState(false);

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
            for (const p of paths) {
              if (await addRepoByPath(p)) ok++;
            }
            if (ok < paths.length) {
              log(`有 ${paths.length - ok} 个拖入项不是 git 仓库（未找到 .git）`, "error");
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
      />
      <RepoPanel />
      {showSettings && <SettingsModal onClose={() => setShowSettings(false)} />}
      {showClone && <CloneModal onClose={() => setShowClone(false)} />}
      {showNew && <NewRepoModal onClose={() => setShowNew(false)} />}
    </div>
  );
}

export default App;
