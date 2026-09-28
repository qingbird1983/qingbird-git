// 生成 qingbird-git 应用图标：把源图 assets/icon-source.png 展开成
// src-tauri/icons/ 下的全套平台图标（Windows 打包用的 ICO/PNG、托盘图标、
// 前端标题栏用的 src/assets/icon.png）。
//
// 用法: node scripts/gen-icons.mjs   （在项目根目录执行）
//
// 实现走 Tauri 官方 `tauri icon`（@tauri-apps/cli 已带，无需额外依赖）：
// 它负责缩放 + 多尺寸导出 + ICO/ICNS 打包。改图标只改 assets/icon-source.png，
// 然后重跑本脚本；不要手改 src-tauri/icons/ 下的产物。
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { createRequire } from "node:module";

const root = process.cwd();
const source = path.resolve(root, "assets", "icon-source.png");
const iconsDir = path.resolve(root, "src-tauri", "icons");

if (!fs.existsSync(source)) {
  console.error(`找不到源图：${source}`);
  process.exit(1);
}

// 直接 spawn 本地 @tauri-apps/cli 的入口（不经 shell，避免 Windows 上
// pnpm.cmd 的 spawnSync EINVAL，也保证用的是项目锁定的 CLI 版本）。
const tauriCli = createRequire(import.meta.url).resolve("@tauri-apps/cli/tauri.js");
console.log(`tauri icon ${source}`);
execFileSync(process.execPath, [tauriCli, "icon", source], { stdio: "inherit", cwd: root });

// 本项目只发 Windows NSIS（tauri.conf.json bundle.targets = ["nsis"]），
// macOS / Android 的产物用不上，删掉省仓库体积。
for (const junk of ["ios", "android", "icon.icns"]) {
  const p = path.join(iconsDir, junk);
  if (fs.existsSync(p)) {
    fs.rmSync(p, { recursive: true, force: true });
    console.log(`清理 ${junk}`);
  }
}

// gen-icons.mjs 早期版本留下的托盘图，现在托盘直接用 icons/32x32.png（lib.rs 引用）。
const tray = path.join(iconsDir, "tray.png");
if (fs.existsSync(tray)) {
  fs.rmSync(tray, { force: true });
  console.log("清理 tray.png");
}

// 前端标题栏用的小尺寸版本（128px，21px 逻辑像素在高分屏上够清晰）。
const frontIcon = path.resolve(root, "src", "assets", "icon.png");
fs.mkdirSync(path.dirname(frontIcon), { recursive: true });
fs.copyFileSync(path.join(iconsDir, "128x128.png"), frontIcon);
console.log(`前端图标 ${path.relative(root, frontIcon)}`);

console.log("icons generated:", path.relative(root, iconsDir));
