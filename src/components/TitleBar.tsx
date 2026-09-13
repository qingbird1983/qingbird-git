// 自定义标题栏（替代 Tauri 原生标题栏，同兄弟项目 qingbird-md-rust）。
//
// 布局：左侧 朱砂「git」印章钮（hover 显侧栏开关图标，点击 toggleSidebar——
// logo 与侧栏开关二合一，同 qingbird-md 的 seal-btn）+ 衬线品牌名 |
// 中部空白归入拖拽区 | 右侧窗控按钮组。
// 关闭按钮走 appWindow.close() → Rust 端 CloseRequested 拦截隐藏到托盘（行为不变）。
import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useApp, type ThemeMode } from "../store";
import { IconGear, IconGitMark, IconMinus, IconMaximize, IconMonitor, IconMoon, IconPanelLeft, IconRestore, IconSun, IconX } from "./icons";

const appWindow = getCurrentWindow();

export default function TitleBar() {
  const [maximized, setMaximized] = useState(false);
  const sidebarOpen = useApp((s) => s.sidebarOpen);
  const toggleSidebar = useApp((s) => s.toggleSidebar);
  // 明暗主题循环切换（暗→亮→跟随系统），与原侧栏底部的按钮同逻辑
  const theme = useApp((s) => s.theme);
  const ThemeIcon = theme === "dark" ? IconMoon : theme === "light" ? IconSun : IconMonitor;
  const themeTitle = `主题：${theme === "dark" ? "暗色" : theme === "light" ? "亮色" : "跟随系统"}（点击切换）`;
  const cycleTheme = () => {
    const next: ThemeMode = theme === "dark" ? "light" : theme === "light" ? "system" : "dark";
    void useApp.getState().setTheme(next);
  };

  useEffect(() => {
    void appWindow.isMaximized().then(setMaximized);
    const unlisten = appWindow.onResized(async () => {
      setMaximized(await appWindow.isMaximized());
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);

  const toggleMaximize = () => {
    if (maximized) void appWindow.unmaximize();
    else void appWindow.maximize();
  };

  return (
    <header className="titlebar" data-tauri-drag-region="deep">
      {/* deep：整个标题栏子树可拖；BUTTON（印章/窗控）自动豁免走自身点击。
          空白处双击 = 最大化/还原（原生标题栏习惯）。 */}
      <div className="titlebar-left" onDoubleClick={toggleMaximize}>
        <button
          type="button"
          className="seal-btn"
          aria-label={sidebarOpen ? "收起仓库列表" : "展开仓库列表"}
          title={sidebarOpen ? "收起仓库列表" : "展开仓库列表"}
          onClick={toggleSidebar}
        >
          {/* 双层：默认朱砂「git」印，hover 淡出、原位浮现侧栏开关图标（CSS .seal-btn） */}
          <span className="seal-face" aria-hidden="true">
            <IconGitMark size={13} />
          </span>
          <span className="seal-ico" aria-hidden="true">
            <IconPanelLeft size={14} />
          </span>
        </button>
        <span className="titlebar-brand">青鸟 Git</span>
      </div>
      <span className="titlebar-divider" aria-hidden="true" />

      <div className="window-controls" onDoubleClick={toggleMaximize}>
        <button type="button" className="win-btn" title={themeTitle} onClick={cycleTheme}>
          <ThemeIcon size={14} />
        </button>
        <button
          type="button"
          className="win-btn"
          title="设置"
          aria-label="设置"
          onClick={useApp.getState().openSettings}
        >
          <IconGear size={14} />
        </button>
        <button type="button" className="win-btn" title="最小化" onClick={() => void appWindow.minimize()}>
          <IconMinus size={14} />
        </button>
        <button
          type="button"
          className="win-btn"
          title={maximized ? "还原" : "最大化"}
          onClick={toggleMaximize}
        >
          {maximized ? <IconRestore size={13} /> : <IconMaximize size={13} />}
        </button>
        <button
          type="button"
          className="win-btn win-close"
          title="关闭（隐藏到托盘）"
          onClick={() => void appWindow.close()}
        >
          <IconX size={14} />
        </button>
      </div>
    </header>
  );
}
