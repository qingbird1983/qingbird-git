import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import TitleBar from "./components/TitleBar";
import NavRail from "./components/NavRail";
import RepoList from "./components/RepoList";
import RepoPanel from "./components/RepoPanel";
import Overview from "./components/Overview";
import SettingsModal from "./components/SettingsModal";
import CloneModal from "./components/CloneModal";
import NewRepoModal from "./components/NewRepoModal";
import InitRepoModal from "./components/InitRepoModal";
import CommandPalette from "./components/CommandPalette";
import { useApp } from "./store";
import { listenAutoSync, listenTrayScan } from "./lib/appEvents";
import { resolveHotkey, stepRepoIndex } from "./lib/hotkeys";
import { IconCloudDown, IconSync, IconX } from "./components/icons";
import "./App.css";

const SIDEBAR_DEFAULT_W = 300;

/** 焦点是不是在能打字的控件里：决定 1/2/3、方向键这些键位让不让 */
function focusInInput(): boolean {
  const el = document.activeElement;
  if (!el) return false;
  if (el instanceof HTMLTextAreaElement) return true;
  if (el instanceof HTMLInputElement) return true;
  return el instanceof HTMLElement && el.isContentEditable;
}

function App() {
  const { loadRepos, initTheme, addRepoByPath, log } = useApp();
  const proxyToast = useApp((s) => s.proxyToast);
  const dismissProxy = useApp((s) => s.dismissProxy);
  const autoToast = useApp((s) => s.autoToast);
  const dismissAuto = useApp((s) => s.dismissAuto);
  // 侧栏常挂载 + 宽度/透明度过渡做平滑收展（--panel-anim）；
  // 内容包在 .panel-clip 里保持固定宽，收缩时只被裁切不被挤压。
  // 开关入口在自建标题栏的品牌 logo 钮上（TitleBar.tsx）。
  // 拖宽条可调宽度（SIDEBAR_MIN_W..SIDEBAR_MAX_W）；拖拽中关掉过渡避免跟手延迟。
  const sidebarOpen = useApp((s) => s.sidebarOpen);
  const sidebarWidth = useApp((s) => s.sidebarWidth);
  const view = useApp((s) => s.view);
  const showSettings = useApp((s) => s.settingsOpen);
  const paletteOpen = useApp((s) => s.paletteOpen);
  const cloneOpen = useApp((s) => s.cloneOpen);
  const newRepoOpen = useApp((s) => s.newRepoOpen);
  const initPath = useApp((s) => s.initPath);
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
  // 全局快捷键：Ctrl K 面板 / Ctrl T 终端 / 1·2·3 三键 / ↑↓ 切仓库。
  // 规则本身在 src/lib/hotkeys.ts（纯逻辑，有回归测试），这里只负责挂监听与执行。
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const s = useApp.getState();
      // 任何浮层开着时都不响应：设置/克隆/新建/初始化弹窗里焦点未必在输入框上，
      // 用户随手按 1 不该在弹窗背后偷偷拉取一个仓库。Ctrl K 除外（它就是来开面板的）。
      const overlay =
        s.paletteOpen || s.settingsOpen || s.cloneOpen || s.newRepoOpen || !!s.initPath;
      const action = overlay
        ? e.ctrlKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "k"
          ? "palette"
          : null
        : resolveHotkey(e, {
            inInput: focusInInput(),
            hasRepo: !!s.selected,
            inReposView: s.view === "repos",
          });
      if (!action) return;
      // 这些键浏览器/WebView 自带行为（Ctrl T 开新标签页、方向键滚动），
      // 不拦住会先触发默认动作再执行我们的
      e.preventDefault();

      switch (action) {
        case "palette":
          s.togglePalette();
          break;
        case "terminal":
          s.openInTerminal(s.selected!);
          break;
        case "pull":
          void s.pullSmart();
          break;
        case "push":
          void s.push();
          break;
        case "check":
          void s.checkUpdate();
          break;
        case "prevRepo":
        case "nextRepo": {
          const i = s.repos.findIndex((r) => r.path === s.selected);
          const next = s.repos[stepRepoIndex(i, action === "nextRepo" ? 1 : -1, s.repos.length)];
          if (next) s.select(next.path);
          break;
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

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
              if (n === 1 && firstInvalid) useApp.getState().openInit(firstInvalid);
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
        <NavRail />
        <aside
          className={`sidebar${resizing ? " resizing" : ""}`}
          style={{
            width: sidebarOpen ? sidebarWidth : 0,
            opacity: sidebarOpen ? 1 : 0,
          }}
        >
          <div className="panel-clip" style={{ width: sidebarWidth }}>
            <RepoList />
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
        {view === "overview" ? <Overview /> : <RepoPanel />}
      </div>
      {showSettings && <SettingsModal onClose={useApp.getState().closeSettings} />}
      {cloneOpen && <CloneModal onClose={useApp.getState().closeClone} />}
      {newRepoOpen && <NewRepoModal onClose={useApp.getState().closeNewRepo} />}
      {initPath && <InitRepoModal path={initPath} onClose={useApp.getState().closeInit} />}
      {paletteOpen && <CommandPalette />}
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
