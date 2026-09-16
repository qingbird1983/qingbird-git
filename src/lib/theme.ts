/** 界面主题：dark / light / system（system 跟随系统深色偏好） */
export type ThemeMode = "dark" | "light" | "system";

/** 把主题写到 <html data-theme>；theme.css 的两套「纸墨」令牌据此切换 */
export function applyThemeClass(mode: ThemeMode) {
  const dark =
    mode === "dark" ||
    (mode === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

/** 后端存的主题字符串归一成合法值（非法或缺失都回退 dark） */
export function normalizeTheme(theme: string): ThemeMode {
  return (["dark", "light", "system"].includes(theme) ? theme : "dark") as ThemeMode;
}
