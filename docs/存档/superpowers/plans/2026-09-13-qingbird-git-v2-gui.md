# 青鸟 Git v2 GUI 重设 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按设计文档 `docs/superpowers/specs/2026-09-13-qingbird-git-v2-design.md` 完成纯前端 GUI 重设：token 化视觉系统、侧栏/主面板重构、三键 Hero、低频功能收入设置弹窗。

**Architecture:** 全部改动限于 `src/` 前端。新建 `src/styles/tokens.css` 承载全部设计 token（dark/light 双主题块）；`App.css` 分任务逐区块重写为 token 取值；`RepoList.tsx`、`RepoPanel.tsx`、`SettingsModal.tsx` 做结构性 JSX 改动；其余组件只被动接受共享类名的 CSS 重绘。Rust 后端、`store.ts` 行为、`api.ts`、`icons.tsx` 现有图标、桌面图标均不动（新增 1 个 kebab 图标除外）。

**Tech Stack:** React 19 + TypeScript + 原生 CSS variables（不引入 UI 框架 / 测试框架）。

## Global Constraints

以下为设计文档（spec）全项目要求，每个任务隐式包含：

- **不改 Rust 后端、Tauri commands、git 子进程逻辑**（spec §2）。
- **不动 `icons.tsx` 现有图标**，仅允许新增 `IconMore`（kebab 菜单需要，spec §2 允许"组件级图标先沿用"）。
- **不改桌面图标** `scripts/gen-icons.mjs`、Tauri `icons/`（spec §2）。
- **不引入 UI 框架**，继续原生 React + CSS variables（spec §2）；因此**本计划不新增任何 npm 依赖**。
- **不改窗口尺寸/标题栏/托盘菜单**（spec §2）。
- **不改 `store.ts` 行为与 `api.ts` 接口**（spec 头部范围声明）；字号 `--font-scale` 仅会话级生效、不持久化（spec §3.2 "留给未来字号偏好"）。
- 所有颜色/字号/圆角/阴影/动效**必须取自 `src/styles/tokens.css` 的 token**，组件样式里不写裸色值（图标语言色 `LANG_COLORS` 与品牌蓝除外）。
- **dark + light 双主题都必须可用**，主题由 `html[data-theme]` 切换（现有 `store.ts: applyThemeClass` 已实现，不改）。
- 每个任务完成后的验证命令：`pnpm build`（tsc 类型检查 + vite 构建，在仓库根目录运行）。
- 提交信息用中文 conventional commits（沿用仓库现有风格，如 `feat: …`）。

---

## 文件结构总览

| 文件 | 动作 | 负责内容 |
|------|------|----------|
| `src/styles/tokens.css` | 新建（Task 1） | 全部设计 token：调色板、类型、圆角、阴影、动效，dark/light 双主题块 |
| `src/main.tsx` | 修改（Task 1） | 在 `App` 之前 import tokens.css |
| `src/components/RepoList.tsx` | 修改（Task 2） | `repo-item`→`repo-row`、host 分组、状态徽章 |
| `src/App.css` | 重写（Task 2-8 分区块进行） | 全部组件样式，全部取 token |
| `src/components/icons.tsx` | 修改（Task 3，仅新增） | 新增 `IconMore`（kebab 三点） |
| `src/App.tsx` | 修改（Task 3） | 给 `RepoPanel` 传 `onOpenSettings` |
| `src/components/RepoPanel.tsx` | 大改（Task 3、4） | topbar + status chips + kebab；三键 Hero；日志区 raised；语言统计默认隐藏 |
| `src/components/SpaceModal.tsx` | 重构（Task 6） | 拆出可内嵌的 `SpaceContent`，弹窗壳保留 |
| `src/components/SettingsModal.tsx` | 重写（Task 6） | 分节设置：外观 / 网络 / 命令预设 / Git 身份与令牌 / 维护 / 关于 |
| `src/components/CloneModal.tsx` `NewRepoModal.tsx` `InitRepoModal.tsx` `HistoryModal.tsx` `CmdPanel.tsx` | 不改 JSX（Task 7 仅 CSS） | 共享类名 token 化重绘 |

设计文档中的 token 值以 spec §3 为准；spec 未给出 light 值的条目（如状态色、tile 底色），本计划按"中性调、α 层"原则在 Task 1 定死具体值，后续任务直接引用。

---

### Task 1: 设计 token 文件与入口

**Files:**
- Create: `src/styles/tokens.css`
- Modify: `src/main.tsx:1-9`

**Interfaces:**
- Consumes: 无。
- Produces: 全部 `--ds-*` / `--text-*` / `--radius-*` / `--motion-*` / `--font-*` CSS 变量，后续所有任务的 CSS 都从这里取值；`--font-scale`（默认 1）由 Task 6 的设置滑杆写入 `document.documentElement.style`。

- [ ] **Step 1: 创建 `src/styles/tokens.css`**

```css
/* ============================================================
   设计 token — 青鸟 Git v2 GUI
   所有视觉（颜色/类型/圆角/阴影/动效）从这里取值；
   主题调整只改本文件，不改组件样式。
   ============================================================ */

:root {
  /* 类型 */
  --font-scale: 1;
  --font-sans: -apple-system, "Segoe UI", "Microsoft YaHei", system-ui, sans-serif;
  --font-mono: "Cascadia Mono", "Consolas", "SF Mono", monospace;

  --text-2xs: calc(11px * var(--font-scale));
  --text-xs: calc(11.5px * var(--font-scale));
  --text-sm: calc(12px * var(--font-scale));
  --text-base: calc(13px * var(--font-scale));
  --text-md: calc(13.5px * var(--font-scale));
  --text-lg: calc(15px * var(--font-scale));
  --text-xl: calc(17px * var(--font-scale));
  --text-2xl: calc(20px * var(--font-scale));

  --leading-tight: 1.25;
  --leading-body: 1.45;
  --leading-relaxed: 1.55;
  --tracking-tight: -0.01em;
  --tracking-normal: 0;

  /* 圆角 */
  --radius-2xs: 4px;
  --radius-xs: 6px;
  --radius-sm: 8px;
  --radius-md: 10px;
  --radius-lg: 12px;
  --radius-xl: 16px;
  --radius-2xl: 20px;
  --radius-full: 9999px;

  /* 阴影 / 动效（与主题无关的部分；随主题的 shadow/stroke 在下方主题块覆盖） */
  --ds-shadow-composer: 0 3px 7.5px rgba(0, 0, 0, 0.04), 0 0 20px rgba(0, 0, 0, 0.05);
  --motion-duration-fast: 150ms;
  --motion-duration-normal: 200ms;
  --motion-duration-slow: 300ms;
  --motion-ease-out: cubic-bezier(0.22, 1, 0.36, 1);
  --motion-ease-in: cubic-bezier(0.4, 0, 1, 1);
  --motion-ease-standard: cubic-bezier(0.2, 0, 0, 1);
}

/* ---------- dark ---------- */
:root,
:root[data-theme="dark"] {
  color-scheme: dark;

  /* 灰阶（α 文字层的基底） */
  --gray-0: #ffffff;
  --gray-300: #c4c7cc;

  /* 背景分频：主面板 / 侧栏深一档 */
  --ds-bg-primary: #181818;
  --ds-bg-sidebar: #000000;
  --ds-bg-secondary: #212121;
  --ds-bg-tertiary: #282828;
  --ds-bg-inset: #0d0d0d;
  --ds-bg-elevated: color-mix(in srgb, var(--ds-bg-secondary) 96%, transparent);
  --ds-bg-composer: color-mix(in srgb, var(--ds-bg-secondary) 96%, transparent);
  --ds-bg-chip: color-mix(in srgb, #fff 6%, transparent);
  --ds-tile: color-mix(in srgb, #fff 3.5%, transparent);
  --ds-tile-hover: color-mix(in srgb, #fff 6%, transparent);
  --ds-tile-deep: color-mix(in srgb, #fff 8%, transparent);
  --ds-raised: #282828;
  --ds-raised-shadow: 0 1px 2px rgba(0, 0, 0, 0.35);

  /* 品牌蓝（青鸟） */
  --ds-accent: #4a9eff;
  --ds-accent-strong: #2f7fe8;
  --ds-accent-soft: color-mix(in srgb, #4a9eff 14%, transparent);

  /* 状态色 */
  --ds-success: #40c977;
  --ds-warning: #ff8549;
  --ds-error: #ff6764;
  --ds-info: var(--gray-300);

  /* α 文字层 */
  --ds-text-primary: var(--gray-0);
  --ds-text-secondary: color-mix(in srgb, var(--gray-0) 70%, transparent);
  --ds-text-muted: color-mix(in srgb, var(--gray-0) 52%, transparent);
  --ds-text-faint: color-mix(in srgb, var(--gray-0) 38%, transparent);

  /* 阴影 / 描边 */
  --ds-shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.25);
  --ds-shadow-dialog: 0 16px 48px rgba(0, 0, 0, 0.55);
  --ds-elevation-stroke: 0 0 0 0.5px color-mix(in srgb, #fff 12%, transparent);
}

/* ---------- light ---------- */
:root[data-theme="light"] {
  color-scheme: light;

  --gray-0: #1a1c1f;
  --gray-300: #5c6066;

  --ds-bg-primary: #ffffff;
  --ds-bg-sidebar: #f3f3f3;
  --ds-bg-secondary: #f9f9f9;
  --ds-bg-tertiary: #f3f3f3;
  --ds-bg-inset: #ededed;
  --ds-bg-elevated: color-mix(in srgb, var(--ds-bg-secondary) 96%, transparent);
  --ds-bg-composer: color-mix(in srgb, var(--ds-bg-secondary) 96%, transparent);
  --ds-bg-chip: color-mix(in srgb, #1a1c1f 6%, transparent);
  --ds-tile: color-mix(in srgb, #1a1c1f 3.5%, transparent);
  --ds-tile-hover: color-mix(in srgb, #1a1c1f 6%, transparent);
  --ds-tile-deep: color-mix(in srgb, #1a1c1f 8%, transparent);
  --ds-raised: #ffffff;
  --ds-raised-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);

  --ds-accent: #2f7fe8;
  --ds-accent-strong: #1f66c4;
  --ds-accent-soft: color-mix(in srgb, #2f7fe8 10%, transparent);

  --ds-success: #1a9c42;
  --ds-warning: #c25e14;
  --ds-error: #d33a33;
  --ds-info: var(--gray-300);

  --ds-text-primary: var(--gray-0);
  --ds-text-secondary: color-mix(in srgb, var(--gray-0) 70%, transparent);
  --ds-text-muted: color-mix(in srgb, var(--gray-0) 52%, transparent);
  --ds-text-faint: color-mix(in srgb, var(--gray-0) 38%, transparent);

  --ds-shadow-sm: 0 1px 2px rgba(30, 50, 90, 0.1);
  --ds-shadow-dialog: 0 16px 48px rgba(30, 50, 90, 0.25);
  --ds-elevation-stroke: 0 0 0 0.5px color-mix(in srgb, #1a1c1f 10%, transparent);
}
```

- [ ] **Step 2: `src/main.tsx` 在 `App` 之前引入 tokens**

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import "./styles/tokens.css";
import App from "./App";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

- [ ] **Step 3: 验证类型检查与构建通过**

Run: `pnpm build`
Expected: tsc 无错误，vite 构建成功。

- [ ] **Step 4: dev 目检无回归**

Run: `pnpm tauri dev`
Expected: 界面与改动前完全一致（本任务只加变量、不改任何现有规则）。

- [ ] **Step 5: Commit**

```bash
git add src/styles/tokens.css src/main.tsx
git commit -m "feat: 新增 v2 设计 token 体系（tokens.css）"
```

---

### Task 2: 侧栏重构（repo-row、分组、raised 激活态）

**Files:**
- Modify: `src/components/RepoList.tsx`（整个列表渲染部分重排）
- Modify: `src/App.css`（`.sidebar` 到 `.pill-btn:disabled` 区块整体替换，约 1-405 行中的侧栏部分）

**Interfaces:**
- Consumes: Task 1 的全部 `--ds-*` token；现有 `store.ts` 的 `repos/selected/status/select/renameRepo/removeRepo`。
- Produces: 侧栏类名 `.repo-row` / `.repo-group` / `.repo-group-title`（Task 7 动效与 Task 8 清理会引用 `.repo-row`）。

- [ ] **Step 1: 改 `RepoList.tsx` — 导入 `useMemo` 与 `RepoEntry` 类型**

文件顶部：

```tsx
import { useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, remoteHost, type RepoEntry } from "../api";
import { useApp } from "../store";
```

- [ ] **Step 2: 在组件外加分组工具**

```tsx
type GroupKey = "github" | "gitee" | "local" | "missing";
const GROUP_ORDER: GroupKey[] = ["github", "gitee", "local", "missing"];
const GROUP_LABELS: Record<GroupKey, string> = {
  github: "GitHub",
  gitee: "Gitee",
  local: "本地",
  missing: "失联",
};

function groupOf(host: "gitee" | "github" | "other" | null, missing: boolean): GroupKey {
  if (missing) return "missing";
  if (host === "github") return "github";
  if (host === "gitee") return "gitee";
  return "local";
}
```

- [ ] **Step 3: 组件内计算分组，把 `.map` 渲染换成 `renderRepo` + 分组容器**

在 `RepoList` 组件体内（`const dragging = ...` 之后）加入：

```tsx
const groups = useMemo(() => {
  const m = new Map<GroupKey, RepoEntry[]>();
  for (const r of repos) {
    const st = status[r.path];
    const k = groupOf(st ? remoteHost(st.remote_url) : null, !!st && !st.is_repo);
    if (!m.has(k)) m.set(k, []);
    m.get(k)!.push(r);
  }
  return m;
}, [repos, status]);
const showGroups = groups.size > 1;
```

`renderRepo`（放在 `commitRename` 之后；行结构与现版本一致，只改类名与徽章逻辑）：

```tsx
const renderRepo = (r: RepoEntry) => {
  const st = status[r.path];
  const missing = !!st && !st.is_repo;
  const host = st ? remoteHost(st.remote_url) : null;
  return (
    <div
      key={r.path}
      className={`repo-row ${selected === r.path ? "active" : ""} ${missing ? "is-missing" : ""}`}
      onClick={() => select(r.path)}
      onDoubleClick={() => startRename(r.path, r.alias)}
      title={`${r.path}\n双击可设置自定义显示名`}
    >
      <span className="repo-lead">
        <span className="repo-icon">
          <IconRepo size={15} />
        </span>
        <span className="repo-tools">
          <button
            className="repo-tool"
            title="重命名（仅改本地显示名）"
            onClick={(e) => {
              e.stopPropagation();
              startRename(r.path, r.alias);
            }}
          >
            <IconPencil size={12} />
          </button>
          <button
            className="repo-tool danger"
            title="移除"
            onClick={(e) => {
              e.stopPropagation();
              handleRemove(r.path, r.name);
            }}
          >
            <IconX size={12} />
          </button>
        </span>
      </span>
      <div className="repo-info">
        <div className="repo-row-main">
          {editing === r.path ? (
            <input
              className="rename-input"
              autoFocus
              value={editValue}
              placeholder="自定义显示名（如中文名）"
              onClick={(e) => e.stopPropagation()}
              onChange={(e) => setEditValue(e.target.value)}
              onBlur={() => void commitRename()}
              onKeyDown={(e) => {
                if (e.key === "Enter") void commitRename();
                if (e.key === "Escape") setEditing(null);
              }}
            />
          ) : (
            <span className="repo-name">
              {r.name}
              {r.alias && <span className="alias-flag" title="已设置自定义显示名">名</span>}
            </span>
          )}
          <span className="repo-badges">
            {host && host !== "other" && !missing && (
              <span className={`badge badge-${host}`}>{host === "gitee" ? "Gitee" : "GitHub"}</span>
            )}
            {missing && <span className="badge badge-missing">失联</span>}
            {st && st.ahead > 0 && <span className="badge badge-ahead">↑{st.ahead}</span>}
            {st && st.behind > 0 && <span className="badge badge-behind">↓{st.behind}</span>}
            {st?.dirty && <span className="badge badge-dirty">●</span>}
          </span>
        </div>
        <div className="repo-row-sub">
          {r.alias && (
            <span className="repo-alias" title={`自定义显示名（原名：${r.name}）`}>
              {r.alias}
            </span>
          )}
          <span className="repo-branch">{st?.is_repo ? st.branch || "—" : ""}</span>
        </div>
      </div>
    </div>
  );
};
```

`.repo-list` 的 JSX 改为：

```tsx
<div className="repo-list">
  {repos.length === 0 && (
    <div className="empty-tip">
      还没有仓库
      <br />
      点击右上角 ＋ 添加，或把仓库文件夹
      <br />
      直接拖进窗口
    </div>
  )}
  {!showGroups && repos.map(renderRepo)}
  {showGroups &&
    GROUP_ORDER.filter((k) => groups.has(k)).map((k) => (
      <div key={k} className="repo-group">
        <div className="repo-group-title">{GROUP_LABELS[k]}</div>
        {groups.get(k)!.map(renderRepo)}
      </div>
    ))}
</div>
```

侧栏 header / footer 的 JSX 不动（品牌、三入口、扫描全部、主题、设置按钮保持原样，样式在 Step 4 重绘）。

- [ ] **Step 4: 重写 App.css 侧栏区块**

在 `App.css` 中删除以下旧选择器的规则块（保留其它区块不动）：
`.sidebar`、`.sidebar-header`、`.brand`、`.brand-icon`、`.header-actions`、`.repo-list`、`.empty-tip`、`.repo-item`、`.repo-item:last-child`、`.repo-item:hover`、`.repo-item.active`、`.repo-lead`、`.repo-icon`、`.repo-item.active .repo-icon`、`.repo-tools`、`.repo-item:hover .repo-tools`、`.repo-item:hover .repo-icon`、`.repo-tool`、`.repo-tool:hover`、`.repo-tool.danger:hover`、`.repo-info`、`.repo-item-main`、`.repo-name`、`.repo-badges`、`.badge`、`.badge-dirty`、`.badge-behind`、`.badge-ahead`、`.badge-missing`、`.badge-host-github`、`.badge-host-gitee`、`.repo-item-sub`、`.repo-alias`、`.repo-branch`、`.sidebar-footer`、`.footer-icons`、`.pill-btn`、`.pill-btn:hover:not(:disabled)`、`.pill-btn:disabled`、`.drop-overlay`、`.drop-hint`、`.alias-flag`

在同一位置写入：

```css
/* ---------- 侧栏 ---------- */
.sidebar {
  width: 260px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  background: var(--ds-bg-sidebar);
  position: relative;
}

.sidebar-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 14px 12px;
}

.brand {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: var(--text-lg);
  font-weight: 700;
  letter-spacing: var(--tracking-tight);
  color: var(--ds-text-primary);
}

.brand-icon {
  width: 20px;
  height: 20px;
}

.header-actions {
  display: flex;
  gap: 2px;
}

.repo-list {
  flex: 1;
  overflow-y: auto;
  padding: 2px 0 8px;
}

.empty-tip {
  padding: 32px 20px;
  text-align: center;
  font-size: var(--text-sm);
  line-height: var(--leading-body);
  color: var(--ds-text-faint);
}

.repo-group-title {
  padding: 8px 20px 4px;
  font-size: var(--text-2xs);
  letter-spacing: 0.05em;
  color: var(--ds-text-faint);
}

.repo-row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 1px 6px;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  background: transparent;
  transition: background var(--motion-duration-fast) var(--motion-ease-out),
    box-shadow var(--motion-duration-fast) var(--motion-ease-out);
}

.repo-row:hover {
  background: var(--ds-tile-hover);
}

.repo-row:hover .repo-icon {
  display: none;
}

.repo-row:hover .repo-tools {
  display: flex;
}

.repo-row.active {
  background: var(--ds-raised);
  box-shadow: var(--ds-raised-shadow);
}

.repo-row.active .repo-icon {
  color: var(--ds-accent);
}

.repo-row.is-missing {
  opacity: 0.55;
}

.repo-lead {
  width: 26px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}

.repo-icon {
  display: flex;
  color: var(--ds-text-faint);
  transition: color var(--motion-duration-fast) var(--motion-ease-out);
}

.repo-tools {
  display: none;
  align-items: center;
  gap: 2px;
}

.repo-tool {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border: none;
  border-radius: var(--radius-2xs);
  background: transparent;
  color: var(--ds-text-muted);
  cursor: pointer;
}

.repo-tool:hover {
  background: var(--ds-tile-deep);
  color: var(--ds-text-primary);
}

.repo-tool.danger:hover {
  color: var(--ds-error);
}

.repo-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.repo-row-main {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.repo-name {
  font-size: var(--text-base);
  font-weight: 500;
  color: var(--ds-text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.alias-flag {
  margin-left: 4px;
  padding: 0 3px;
  font-size: var(--text-2xs);
  border-radius: var(--radius-2xs);
  background: var(--ds-bg-chip);
  color: var(--ds-text-muted);
}

.repo-badges {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.badge {
  font-size: var(--text-2xs);
  line-height: 1;
  padding: 2px 5px;
  border-radius: var(--radius-2xs);
  background: var(--ds-bg-chip);
  color: var(--ds-text-muted);
}

.badge-ahead {
  background: transparent;
  color: var(--ds-success);
}

.badge-behind {
  background: var(--ds-accent-soft);
  color: var(--ds-accent);
}

.badge-dirty {
  background: transparent;
  color: var(--ds-warning);
}

.badge-missing {
  background: color-mix(in srgb, var(--ds-error) 14%, transparent);
  color: var(--ds-error);
}

.badge-github,
.badge-gitee {
  background: transparent;
  color: var(--ds-text-faint);
  border: none;
}

.repo-row-sub {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.repo-alias {
  font-size: var(--text-2xs);
  color: var(--ds-text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.repo-branch {
  font-family: var(--font-mono);
  font-size: var(--text-2xs);
  color: var(--ds-text-faint);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.rename-input {
  width: 100%;
  padding: 2px 6px;
  font-size: var(--text-base);
  color: var(--ds-text-primary);
  background: var(--ds-bg-inset);
  border: none;
  border-radius: var(--radius-2xs);
  outline: 1px solid var(--ds-accent);
}

.sidebar-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
}

.footer-icons {
  display: flex;
  gap: 2px;
}

.pill-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 14px;
  font-size: var(--text-sm);
  color: var(--ds-text-secondary);
  background: var(--ds-tile);
  border: none;
  border-radius: var(--radius-full);
  cursor: pointer;
  transition: background var(--motion-duration-fast) var(--motion-ease-out);
}

.pill-btn:hover:not(:disabled) {
  background: var(--ds-tile-hover);
  color: var(--ds-text-primary);
}

.pill-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.drop-overlay {
  position: absolute;
  inset: 0;
  z-index: 10;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--ds-accent) 8%, transparent);
}

.drop-hint {
  padding: 10px 18px;
  font-size: var(--text-sm);
  color: var(--ds-accent);
  background: var(--ds-raised);
  border-radius: var(--radius-md);
  box-shadow: var(--ds-shadow-sm);
}
```

- [ ] **Step 5: 验证**

Run: `pnpm build`
Expected: tsc 无错误（`repo-item-main` 等旧类名已全部改名，若仍有引用会编译报错）。

Run: `pnpm tauri dev` 目检：
Expected: 侧栏深一档、无 1px 描边；active 行抬起（raised + shadow）；hover 行出现重命名/移除图标；badge 颜色符合 §6.2（↑ 绿、↓ 蓝、● 橙、失联红）；添加 ≥2 个不同平台的仓库时出现分组标题。

- [ ] **Step 6: Commit**

```bash
git add src/components/RepoList.tsx src/App.css
git commit -m "feat: 侧栏重构——repo-row、host 分组、raised 激活态"
```

---

### Task 3: 主面板 topbar + status chips + kebab 溢出菜单

**Files:**
- Modify: `src/components/icons.tsx`（文件末尾新增 `IconMore`）
- Modify: `src/App.tsx:77`（给 RepoPanel 传 prop）
- Modify: `src/components/RepoPanel.tsx`（头部区域重写）
- Modify: `src/App.css`（`.panel-header`/`.quick-actions`/`.chip*` 区块替换为 topbar/chips 样式）

**Interfaces:**
- Consumes: Task 1 token；`store.ts` 现有 `theme/setTheme/openReadme/runRepoCmd/stopRepoCmd/cancelOp`；`RepoEntry.test_cmd/build_cmd`。
- Produces: `RepoPanel` 组件签名变为 `function RepoPanel({ onOpenSettings }: { onOpenSettings: () => void })`；kebab 菜单内部状态 `kebabOpen`、`showLangs`（Task 4 的语言统计渲染会引用 `showLangs`）；类名 `.topbar`、`.topbar-left`、`.topbar-right`、`.topbar-branch`、`.chips-row`、`.kebab-anchor`、`.kebab-menu`、`.kebab-item`、`.menu-mask`。

- [ ] **Step 1: `icons.tsx` 末尾新增 kebab 三点图标**

```tsx
/** 溢出菜单（竖排三点） */
export const IconMore = (p: { size?: number; className?: string }) => (
  <Icon {...p}>
    <circle cx="12" cy="5" r="1.6" fill="currentColor" stroke="none" />
    <circle cx="12" cy="12" r="1.6" fill="currentColor" stroke="none" />
    <circle cx="12" cy="19" r="1.6" fill="currentColor" stroke="none" />
  </Icon>
);
```

- [ ] **Step 2: `App.tsx` 给 RepoPanel 传 `onOpenSettings`**

```tsx
<RepoPanel onOpenSettings={() => setShowSettings(true)} />
```

- [ ] **Step 3: `RepoPanel.tsx` 重写头部区域**

3a. 组件签名与 import 修改：

```tsx
import {
  IconCloud, IconCloudDown, IconCloudUp,
  IconFolder, IconTerminal, IconHistory, IconDiff, IconX,
  IconFlask, IconHammer, IconPlay, IconDisk,
  IconSync, IconMore, IconGear, IconMoon, IconSun, IconMonitor,
} from "./icons";

export default function RepoPanel({ onOpenSettings }: { onOpenSettings: () => void }) {
```

（删除 `SpaceModal` 的 import；`IconFile` 不再使用。）

3b. 从 store 解构中加入 `theme, setTheme`；删除 `showSpace` state；新增：

```tsx
const [kebabOpen, setKebabOpen] = useState(false);
const [showLangs, setShowLangs] = useState(false);
```

3c. 在 `handleCommitPush` 之后加入主题循环与 kebab 收起逻辑：

```tsx
const cycleTheme = () => {
  const next = theme === "dark" ? "light" : theme === "light" ? "system" : "dark";
  void setTheme(next);
};
const ThemeIcon = theme === "dark" ? IconMoon : theme === "light" ? IconSun : IconMonitor;
```

`useEffect(... [selected])` 重置块中追加 `setKebabOpen(false); setShowLangs(false);`。

3d. 把 `<div className="panel-header card"> … </div>` 整块替换为：

```tsx
<div className="topbar">
  <div className="topbar-left">
    <h2 className="topbar-title">
      {displayName}
      {repoEntry?.alias && (
        <span className="alias-sub" title="本地自定义显示名">
          {selected.split(/[\\/]/).pop()}
        </span>
      )}
    </h2>
    <span className="topbar-branch" title={selected}>
      {st?.is_repo ? st.branch || "—" : "不可用"}
    </span>
  </div>
  <div className="topbar-right">
    {st?.is_repo && st.dirty && (
      <span className="changes-anchor">
        <button
          className={`icon-btn has-badge ${showChanges ? "icon-btn-active" : ""}`}
          title={`待提交变更（${fileChanges.length} 个文件）`}
          onClick={() => setShowChanges((v) => !v)}
        >
          <IconDiff size={15} />
          {fileChanges.length > 0 && <span className="btn-badge">{fileChanges.length}</span>}
        </button>
      </span>
    )}
    <button className="icon-btn" title={themeTitle} onClick={cycleTheme}>
      <ThemeIcon size={15} />
    </button>
    <button className="icon-btn" title="设置" onClick={onOpenSettings}>
      <IconGear size={15} />
    </button>
    <span className="kebab-anchor">
      <button
        className={`icon-btn ${kebabOpen ? "icon-btn-active" : ""}`}
        title="更多工具"
        onClick={() => setKebabOpen((v) => !v)}
      >
        <IconMore size={15} />
      </button>
    </span>
  </div>
</div>

<div className="chips-row">
  {host && host !== "other" && (
    <span className={`chip chip-host-${host}`}>{host === "gitee" ? "Gitee" : "GitHub"}</span>
  )}
  {st?.is_repo ? (
    <>
      <span className="chip">分支：{st.branch || "未知"}</span>
      {st.behind > 0 && <span className="chip chip-behind">↓ 落后云端 {st.behind} 个提交</span>}
      {st.ahead > 0 && <span className="chip chip-ahead">↑ 待推送 {st.ahead} 个提交</span>}
      {st.dirty && <span className="chip chip-dirty">有未提交变更</span>}
      {st.behind === 0 && st.ahead === 0 && !st.has_upstream && (
        <span className="chip">未设置上游分支</span>
      )}
    </>
  ) : (
    <span className="chip chip-warn">仓库不可用（可能已被移动或删除）</span>
  )}
</div>
```

（`themeTitle` 定义在 `cycleTheme` 旁：）

```tsx
const themeTitle = `主题：${theme === "dark" ? "暗色" : theme === "light" ? "亮色" : "跟随系统"}（点击切换）`;
```

注意：原 `panel-status` 里的 commit-popover（待提交变更浮窗 JSX 主体）**保留**，本任务只把它的触发按钮挪到 topbar（上面的 `changes-anchor` 块只包含按钮；popover 部分原样跟在按钮后面，与现有代码相同——即把现有 `{showChanges && (<div className="commit-popover">…</div>)}` 一并放进这个 `changes-anchor` 内）。

3e. 删除的 JSX：原 7 个 icon-btn 中的「磁盘空间与缓存清理」「查看仓库说明」「运行命令」三个按钮及 README/disk 图标 import；`showSpace` state 与 `{showSpace && <SpaceModal …>}` 行。

3f. kebab 下拉菜单 + 菜单遮罩，放在 `<main className="panel">` 的 `chips-row` 之后、`.actions` 之前：

```tsx
{kebabOpen && <div className="menu-mask" onClick={() => setKebabOpen(false)} />}
{kebabOpen && (
  <div className="kebab-menu">
    <button
      className="kebab-item"
      disabled={!!busy || !!cmdRunning}
      onClick={() => {
        setKebabOpen(false);
        void runCmd("test");
      }}
    >
      <IconFlask size={14} /> 跑测试
    </button>
    <button
      className="kebab-item"
      disabled={!!busy || !!cmdRunning}
      onClick={() => {
        setKebabOpen(false);
        void runCmd("build");
      }}
    >
      <IconHammer size={14} /> 构建
    </button>
    <button
      className="kebab-item"
      disabled={!!busy || !!cmdRunning}
      onClick={() => {
        setKebabOpen(false);
        setShowCmd(true);
      }}
    >
      <IconPlay size={14} /> 命令面板
    </button>
    <button
      className="kebab-item"
      disabled={!!busy}
      onClick={() => {
        setKebabOpen(false);
        onOpenSettings();
      }}
    >
      <IconDisk size={14} /> 磁盘清理
    </button>
    <button
      className="kebab-item"
      disabled={langs.length === 0}
      onClick={() => {
        setKebabOpen(false);
        setShowLangs((v) => !v);
      }}
    >
      <IconSync size={14} /> 语言统计
    </button>
  </div>
)}
```

3g. 语言统计 footer 的显示条件由 `langs.length > 0` 改为 `showLangs && langs.length > 0`（语言统计默认隐藏，spec §5）。

- [ ] **Step 4: App.css — topbar / chips / kebab 样式**

删除旧选择器：`.panel-header`、`.panel-title`、`.panel-title h2`、`.panel-path`、`.panel-status`、`.quick-actions`、`.chip-host-gitee`、`.chip-host-github`、`.chip-behind`、`.chip-ahead`。新增：

```css
/* ---------- 主面板 topbar ---------- */
.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 16px 4px;
}

.topbar-left {
  display: flex;
  align-items: baseline;
  gap: 10px;
  min-width: 0;
}

.topbar-title {
  font-size: var(--text-xl);
  font-weight: 700;
  letter-spacing: var(--tracking-tight);
  color: var(--ds-text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.topbar-branch {
  font-family: var(--font-mono);
  font-size: var(--text-sm);
  color: var(--ds-text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.topbar-right {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}

/* ---------- status chips 行 ---------- */
.chips-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 16px 10px;
  overflow-x: auto;
}

.chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 9px;
  font-size: var(--text-xs);
  white-space: nowrap;
  border-radius: var(--radius-full);
  background: var(--ds-bg-chip);
  color: var(--ds-text-secondary);
}

.chip-behind {
  background: var(--ds-accent-soft);
  color: var(--ds-accent);
}

.chip-ahead {
  background: color-mix(in srgb, var(--ds-success) 14%, transparent);
  color: var(--ds-success);
}

.chip-dirty {
  background: color-mix(in srgb, var(--ds-warning) 14%, transparent);
  color: var(--ds-warning);
}

.chip-warn {
  background: color-mix(in srgb, var(--ds-error) 14%, transparent);
  color: var(--ds-error);
}

.chip-host-github,
.chip-host-gitee {
  background: transparent;
  color: var(--ds-text-muted);
  box-shadow: var(--ds-elevation-stroke);
}

/* ---------- kebab 溢出菜单 ---------- */
.menu-mask {
  position: fixed;
  inset: 0;
  z-index: 40;
}

.kebab-anchor {
  position: relative;
  display: inline-flex;
}

.kebab-menu {
  position: absolute;
  top: 34px;
  right: 12px;
  z-index: 41;
  min-width: 150px;
  padding: 5px;
  display: flex;
  flex-direction: column;
  background: var(--ds-bg-elevated);
  border-radius: var(--radius-lg);
  box-shadow: var(--ds-shadow-dialog), var(--ds-elevation-stroke);
  animation: menu-in var(--motion-duration-fast) var(--motion-ease-out);
}

@keyframes menu-in {
  from {
    opacity: 0;
    transform: translateY(-4px);
  }
}

.kebab-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  font-size: var(--text-sm);
  text-align: left;
  color: var(--ds-text-secondary);
  background: transparent;
  border: none;
  border-radius: var(--radius-xs);
  cursor: pointer;
  transition: background var(--motion-duration-fast) var(--motion-ease-out);
}

.kebab-item:hover:not(:disabled) {
  background: var(--ds-tile-hover);
  color: var(--ds-text-primary);
}

.kebab-item:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
```

- [ ] **Step 5: 验证**

Run: `pnpm build`
Expected: 无 tsc 错误（`IconFile`、`SpaceModal` 若仍被引用但已删除 import 会报错）。

Run: `pnpm tauri dev` 目检：
Expected: 顶栏只有 文件夹/终端/历史 3 个 icon-btn + dirty 时的 diff 按钮 + 主题/设置/kebab；dirty 时 diff 触发的变更浮窗仍正常；kebab 5 项均可用；磁盘清理点击后打开的是设置弹窗（Task 6 之前仍是旧版设置弹窗，属预期）；语言统计默认不显示、kebab 点「语言统计」后显示。

- [ ] **Step 6: Commit**

```bash
git add src/components/icons.tsx src/App.tsx src/components/RepoPanel.tsx src/App.css
git commit -m "feat: 主面板 topbar + status chips + kebab 溢出菜单"
```

---

### Task 4: 三键 Hero + 日志区 raised + busy/stash 条 token 化

**Files:**
- Modify: `src/components/RepoPanel.tsx`（`.actions` 区块替换为 Hero；log 区 JSX 微调）
- Modify: `src/App.css`（`.actions`/`.action-btn`/`.log-area`/`.busy-*`/`.stash-bar`/`.progress-*` 区块替换）

**Interfaces:**
- Consumes: Task 3 的 `netBusy`、`runCmd`、`showLangs`；Task 1 token；`store.ts` 的 `busy/cloning/progress`。
- Produces: 类名 `.hero`、`.hero-btn`、`.hero-main`、`.hero-label`、`.hero-detail`（`is-accent` / `is-warning` / `is-faint` 修饰类）、`.hero-progress`；日志区保持 `.log-area` 类名但样式变 raised。

- [ ] **Step 1: `RepoPanel.tsx` — 用 Hero 替换 `.actions` 区块**

删除整个 `<div className="actions"> … </div>`（5 个 action-btn），替换为：

```tsx
<div className="hero">
  <button
    className={`hero-btn ${busy?.startsWith("正在检查") ? "is-busy" : ""}`}
    disabled={netBusy}
    onClick={() => void checkUpdate()}
  >
    <span className="hero-main">
      <IconCloud size={17} />
      <span className="hero-label">检测云端更新</span>
    </span>
    <span className={`hero-detail ${st && st.ahead > 0 ? "is-accent" : ""}`}>
      {st && st.ahead > 0 ? `↑${st.ahead} 待推送` : "点击检测云端状态"}
    </span>
    {busy?.startsWith("正在检查") && <HeroProgress />}
  </button>
  <button
    className={`hero-btn ${busy?.startsWith("正在拉取") ? "is-busy" : ""}`}
    disabled={netBusy}
    onClick={() => void handlePull()}
  >
    <span className="hero-main">
      <IconCloudDown size={17} />
      <span className="hero-label">一键拉取</span>
    </span>
    <span className={`hero-detail ${st && st.behind > 0 ? "is-warning" : ""}`}>
      {st && st.behind > 0 ? `↓${st.behind} 落后云端` : "已最新"}
      {st?.dirty && <span className="hero-detail-hint">（会先 stash）</span>}
    </span>
    {busy?.startsWith("正在拉取") && <HeroProgress />}
  </button>
  <button
    className={`hero-btn ${busy?.startsWith("正在推送") ? "is-busy" : ""}`}
    disabled={netBusy}
    onClick={() => void push()}
  >
    <span className="hero-main">
      <IconCloudUp size={17} />
      <span className="hero-label">一键推送</span>
    </span>
    <span className={`hero-detail ${st && st.ahead > 0 ? "is-accent" : ""}`}>
      {st && st.ahead > 0 ? `↑${st.ahead} 待推送` : st?.is_repo ? "已推送" : "—"}
    </span>
    {busy?.startsWith("正在推送") && <HeroProgress />}
  </button>
</div>
```

在 `RepoPanel` 组件函数之外加进度条子组件（复用现有 `.progress-fill` 类，细线 3px）：

```tsx
/** Hero 键内嵌的 3px 进度线（indeterminate / determinate 跟随全局 progress） */
function HeroProgress() {
  const progress = useApp((s) => s.progress);
  const pct = progress?.percent != null ? Math.min(100, progress.percent) : null;
  return (
    <span className="hero-progress">
      <span
        className={`progress-fill ${pct == null ? "indeterminate" : ""}`}
        style={pct != null ? { width: `${pct}%` } : undefined}
      />
    </span>
  );
}
```

- [ ] **Step 2: log 区 JSX 微调**

`.log-area` 的 `card` 类去掉：`<div className="log-area card">` → `<div className="log-area">`。

- [ ] **Step 3: App.css — Hero / 日志区 / busy / stash 样式**

删除旧选择器：`.actions`、`.action-btn`、`.action-btn:hover:not(:disabled)`、`.action-btn:disabled`、`.stash-bar`、`.busy-bar`、`.busy-row`、`.busy-pct`、`.busy-cancel`、`.busy-cancel:hover`、`.progress-track`、`.progress-fill`、`.progress-fill.indeterminate`、`.progress-box`、`.progress-stage`、`.progress-detail`。新增：

```css
/* ---------- 三键 Hero ---------- */
.hero {
  display: flex;
  flex-direction: column;
  margin: 4px 16px 0;
  background: var(--ds-tile);
  border-radius: var(--radius-lg);
  overflow: hidden;
}

.hero-btn {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 14px 16px;
  background: transparent;
  border: none;
  color: var(--ds-text-primary);
  text-align: left;
  cursor: pointer;
  transition: background var(--motion-duration-fast) var(--motion-ease-out);
}

.hero-btn + .hero-btn {
  border-top: 1px solid color-mix(in srgb, var(--gray-0) 6%, transparent);
}

.hero-btn:hover:not(:disabled) {
  background: var(--ds-tile-hover);
}

.hero-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.hero-btn.is-busy {
  background: var(--ds-tile-deep);
}

.hero-main {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: var(--text-lg);
  font-weight: 600;
  line-height: var(--leading-tight);
}

.hero-detail {
  font-size: var(--text-sm);
  color: var(--ds-text-muted);
  white-space: nowrap;
}

.hero-detail.is-accent {
  color: var(--ds-accent);
}

.hero-detail.is-warning {
  color: var(--ds-warning);
}

.hero-detail-hint {
  color: var(--ds-text-faint);
}

.hero-progress {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 3px;
  display: block;
  overflow: hidden;
  border-radius: 0 0 var(--radius-lg) var(--radius-lg);
}

.hero-progress .progress-fill {
  height: 100%;
}

/* ---------- 日志区（raised） ---------- */
.log-area {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 120px;
  margin: 12px 16px 0;
  background: var(--ds-raised);
  border-radius: var(--radius-lg);
  box-shadow: var(--ds-raised-shadow);
  overflow: hidden;
}

.log-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 9px 14px;
  font-size: var(--text-sm);
  color: var(--ds-text-muted);
}

.log-hint {
  font-size: var(--text-2xs);
  color: var(--ds-text-faint);
}

.log-body {
  flex: 1;
  overflow-y: auto;
  padding: 4px 14px 10px;
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  line-height: var(--leading-relaxed);
}

.log-empty {
  color: var(--ds-text-faint);
}

.log-line {
  display: flex;
  gap: 8px;
}

.log-time {
  flex-shrink: 0;
  color: var(--ds-text-faint);
}

.log-text {
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--ds-text-secondary);
}

.log-success .log-text {
  color: var(--ds-success);
}

.log-error .log-text {
  color: var(--ds-error);
}

.log-proxy .log-text {
  color: var(--ds-info);
}

/* ---------- busy / stash / 进度 ---------- */
.stash-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin: 8px 16px 0;
  padding: 10px 14px;
  font-size: var(--text-sm);
  color: var(--ds-warning);
  background: color-mix(in srgb, var(--ds-warning) 10%, transparent);
  border-radius: var(--radius-md);
}

.busy-bar {
  margin: 8px 16px 0;
  padding: 10px 14px;
  background: var(--ds-bg-secondary);
  border-radius: var(--radius-md);
}

.busy-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: var(--text-sm);
  color: var(--ds-text-secondary);
}

.busy-pct {
  margin-left: auto;
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  color: var(--ds-text-muted);
}

.busy-cancel {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-left: 8px;
  padding: 3px 10px;
  font-size: var(--text-xs);
  color: var(--ds-text-muted);
  background: var(--ds-tile);
  border: none;
  border-radius: var(--radius-full);
  cursor: pointer;
}

.busy-cancel:hover {
  background: var(--ds-tile-hover);
  color: var(--ds-error);
}

.spinner {
  width: 13px;
  height: 13px;
  flex-shrink: 0;
  border: 2px solid var(--ds-accent-soft);
  border-top-color: var(--ds-accent);
  border-radius: var(--radius-full);
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.progress-track {
  height: 3px;
  margin-top: 8px;
  overflow: hidden;
  border-radius: var(--radius-full);
  background: var(--ds-tile);
}

.progress-fill {
  display: block;
  height: 100%;
  background: var(--ds-accent);
  transition: width var(--motion-duration-normal) var(--motion-ease-standard);
}

.progress-fill.indeterminate {
  width: 30%;
  animation: indeterminate 1.2s ease-in-out infinite;
}

@keyframes indeterminate {
  0% {
    margin-left: -30%;
  }
  100% {
    margin-left: 100%;
  }
}
```

- [ ] **Step 4: 验证**

Run: `pnpm build`
Expected: 无错误。

Run: `pnpm tauri dev` 目检：
Expected: 主面板只有 检测/拉取/推送 三键（测试/构建按钮消失）；hover 整块过渡；无仓库状态时右侧 detail 灰色；ahead/behind 时 detail 变蓝/橙；busy 时键底加深 + 底部 3px 进度线；日志区 raised 无描边；跑测试（kebab）后日志头部出现「终止」。

- [ ] **Step 5: Commit**

```bash
git add src/components/RepoPanel.tsx src/App.css
git commit -m "feat: 三键 Hero 与 raised 日志区"
```

---

### Task 5: 浮窗 token 化（commit 变更 / 同步内容 popover）

**Files:**
- Modify: `src/App.css`（`.commit-popover`、`.commit-row`、`.file-list`/`.file-line`、`.sync-commit-list`、`.update-header` 区块替换；`.changes-anchor`、`.btn-badge`、`.icon-btn-active` 微调）

**Interfaces:**
- Consumes: Task 1 token；Task 3 的 `changes-anchor` 触发按钮。
- Produces: 无新接口，纯样式。

- [ ] **Step 1: App.css — 浮窗样式替换**

删除旧选择器：`.changes-anchor`、`.icon-btn-active`、`.btn-badge`、`.commit-popover`、`.popover-close`、`.popover-close:hover`、`.commit-card`、`.commit-title`、`.file-list`、`.file-line`、`.file-code`、`.file-M`/`.file-R`/`.file-A`/`.file-D`/`.file-U` 系列、`.commit-more`、`.update-header`、`.update-count`、`.sync-commit-list`、`.sync-commit-line`、`.commit-row`。新增：

```css
/* ---------- 触发按钮与角标 ---------- */
.changes-anchor {
  position: relative;
  display: inline-flex;
}

.icon-btn-active {
  background: var(--ds-tile-deep);
  color: var(--ds-accent);
}

.btn-badge {
  position: absolute;
  top: -3px;
  right: -3px;
  min-width: 14px;
  height: 14px;
  padding: 0 3px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 9px;
  font-weight: 600;
  color: var(--ds-text-primary);
  background: var(--ds-accent);
  border-radius: var(--radius-full);
}

/* ---------- 浮窗（commit 变更 / 同步内容共用） ---------- */
.commit-popover {
  position: absolute;
  top: 34px;
  right: 0;
  z-index: 30;
  width: 340px;
  padding: 14px 16px;
  background: var(--ds-bg-elevated);
  border-radius: var(--radius-xl);
  box-shadow: var(--ds-shadow-dialog), var(--ds-elevation-stroke);
  animation: menu-in var(--motion-duration-fast) var(--motion-ease-out);
}

.update-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.commit-title {
  flex: 1;
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--ds-text-primary);
}

.update-count {
  font-size: var(--text-2xs);
  color: var(--ds-text-muted);
}

.popover-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  color: var(--ds-text-faint);
  background: transparent;
  border: none;
  border-radius: var(--radius-2xs);
  cursor: pointer;
}

.popover-close:hover {
  background: var(--ds-tile-hover);
  color: var(--ds-text-primary);
}

.file-list {
  display: flex;
  flex-direction: column;
  gap: 1px;
  max-height: 220px;
  overflow-y: auto;
}

.file-line {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border-radius: var(--radius-xs);
  transition: background var(--motion-duration-fast) var(--motion-ease-out);
}

.file-line:hover {
  background: var(--ds-tile-hover);
}

.file-code {
  flex-shrink: 0;
  width: 14px;
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  font-weight: 700;
  text-align: center;
  color: var(--ds-text-muted);
}

.file-M, .file-R { color: var(--ds-warning); }
.file-A { color: var(--ds-success); }
.file-D { color: var(--ds-error); }
.file-u { color: var(--ds-info); }

.file-path {
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  color: var(--ds-text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.commit-more {
  padding: 6px 8px;
  font-size: var(--text-2xs);
  color: var(--ds-text-faint);
}

.sync-commit-list {
  display: flex;
  flex-direction: column;
  gap: 1px;
  max-height: 240px;
  overflow-y: auto;
}

.sync-commit-line {
  padding: 6px 8px;
  font-size: var(--text-xs);
  line-height: var(--leading-body);
  color: var(--ds-text-secondary);
  border-radius: var(--radius-xs);
  transition: background var(--motion-duration-fast) var(--motion-ease-out);
}

.sync-commit-line:hover {
  background: var(--ds-tile-hover);
}

/* 提交输入行：无边框输入 + 底部 alpha 线，focus 变 accent */
.commit-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
}

.commit-row .modal-input {
  background: transparent;
  border: none;
  border-radius: 0;
  border-bottom: 1px solid color-mix(in srgb, var(--gray-0) 20%, transparent);
}

.commit-row .modal-input:focus {
  outline: none;
  border-bottom-color: var(--ds-accent);
}
```

注意：`.file-u`（原 `file-?` 替换类）必须保留，`RepoPanel.tsx` 的 `file-${f.code.replace("?", "u")}` 依赖它。

- [ ] **Step 2: 验证**

Run: `pnpm build`
Expected: 无错误。

Run: `pnpm tauri dev` 目检（dirty 仓库 + 拉取一次产生 update）：
Expected: 变更浮窗 / 同步内容浮窗为 16px 圆角、raised 底、带 elevation-stroke 细描边、无 1px 实线边框；列表项 hover 有 tile 反馈；提交输入框只有底部一条 alpha 线且 focus 变蓝。

- [ ] **Step 3: Commit**

```bash
git add src/App.css
git commit -m "feat: commit/同步内容浮窗 token 化重绘"
```

---

### Task 6: 设置弹窗大改（分节 + 命令预设 + 维护 + 关于）

**Files:**
- Modify: `src/components/SpaceModal.tsx`（拆出可内嵌的 `SpaceContent`）
- Rewrite: `src/components/SettingsModal.tsx`
- Modify: `src/App.css`（`.modal` 系列 + `.segmented`/`.switch` + 新增 `.settings-*` 类；与 Task 7 有交叉，本任务只加 settings 相关）

**Interfaces:**
- Consumes: Task 1 token；`store.ts` 的 `theme/setTheme/repos/selected/setRepoCmds/openReadme`；`api.getSettings/setSettings/probe/autostart*`、`api.getCredentials/setCredentials`。
- Produces: `SpaceModal.tsx` 导出命名组件 `SpaceContent`（无 props，内部读 store 的 `selected`）+ 默认导出的 `SpaceModal` 壳（`{ onClose }`）；`SettingsModal` 签名不变（`{ onClose }`）。`.settings-modal`、`.settings-section`、`.settings-subtitle`、`.settings-row`、`.settings-desc`、`.settings-footer` 类名。

- [ ] **Step 1: `SpaceModal.tsx` 拆出 `SpaceContent`**

保留 `fmtSize` 等工具与全部状态逻辑，把 JSX 里 `<div className="modal-mask">…<div className="modal modal-wide">…</div></div>` 这层壳抽成默认导出，内层内容抽成命名导出：

```tsx
/** 可内嵌在设置弹窗「维护」区块的磁盘空间面板（不含弹窗壳） */
export function SpaceContent() {
  // …原 SpaceModal 的全部 hook 与 handleDelete 逻辑原样搬入…
  return (
    <div className="space-panel">
      <div className="modal-subtitle">{selected}</div>
      {report && (
        <div className="space-summary">
          总占用 <b>{fmtSize(report.total)}</b>
          {cacheTotal > 0 && (
            <>
              ，其中可清理缓存 <b className="space-cache-size">{fmtSize(cacheTotal)}</b>
            </>
          )}
        </div>
      )}
      {error && <div className="log-empty space-error">{error}</div>}
      <div className="space-list">
        {!report && !error && <div className="log-empty">正在扫描，大仓库可能需要几秒…</div>}
        {report?.dirs.map((d) => (
          /* …原 space-item JSX 原样保留… */
        ))}
      </div>
      <div className="space-rescan">
        <button className="btn-sm btn-ghost" onClick={() => void scan()}>
          <IconSync size={13} /> 重新扫描
        </button>
      </div>
    </div>
  );
}

/** 独立弹窗壳（保留：兼容外部直接打开） */
export default function SpaceModal({ onClose }: Props) {
  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal modal-wide" onClick={(e) => e.stopPropagation()}>
        <h3>
          磁盘空间
          <button className="popover-close modal-close-right" title="关闭" onClick={onClose}>
            <IconX size={13} />
          </button>
        </h3>
        <SpaceContent />
      </div>
    </div>
  );
}
```

`Props` 接口与 `IconX`/`IconSync` import 保留。

- [ ] **Step 2: 重写 `SettingsModal.tsx`**

```tsx
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getVersion } from "@tauri-apps/api/app";
import { api } from "../api";
import { useApp, type ThemeMode } from "../store";
import { IconMoon, IconSun, IconMonitor } from "./icons";
import { SpaceContent } from "./SpaceModal";

/** 项目发布页（检测更新跳转目标） */
const RELEASE_URL = "https://github.com/sansan1983/qingbird-git/releases";

interface Props {
  onClose: () => void;
}

export default function SettingsModal({ onClose }: Props) {
  const { theme, setTheme } = useApp();
  const repos = useApp((s) => s.repos);
  const selected = useApp((s) => s.selected);
  const setRepoCmds = useApp((s) => s.setRepoCmds);
  const openReadme = useApp((s) => s.openReadme);

  // —— 外观 ——
  const [fontScale, setFontScale] = useState(1);
  useEffect(() => {
    document.documentElement.style.setProperty("--font-scale", String(fontScale));
  }, [fontScale]);

  // —— 网络 ——
  const [addr, setAddr] = useState("");
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [autostart, setAutostart] = useState(false);

  // —— Git 身份与令牌 ——
  const [gitName, setGitName] = useState("");
  const [gitEmail, setGitEmail] = useState("");
  const [githubToken, setGithubToken] = useState("");
  const [giteeToken, setGiteeToken] = useState("");
  const [sshKeyPath, setSshKeyPath] = useState("");
  const [credSaved, setCredSaved] = useState(false);

  // —— 命令预设 ——
  const [cmdRepo, setCmdRepo] = useState("");
  const effectivePath = cmdRepo || selected || "";
  const cmdEntry = repos.find((r) => r.path === effectivePath);
  const [cmdTest, setCmdTest] = useState("");
  const [cmdBuild, setCmdBuild] = useState("");
  const [cmdSaved, setCmdSaved] = useState(false);

  // —— 关于 ——
  const [version, setVersion] = useState("");

  useEffect(() => {
    void (async () => {
      try {
        setAddr((await api.getSettings()).manual_proxy);
        setAutostart(await api.autostartIsEnabled());
        const c = await api.getCredentials();
        setGitName(c.git_name);
        setGitEmail(c.git_email);
        setGithubToken(c.github_token);
        setGiteeToken(c.gitee_token);
        setSshKeyPath(c.ssh_key_path);
      } catch {
        /* 忽略 */
      }
      try {
        setVersion(await getVersion());
      } catch {
        setVersion("");
      }
    })();
  }, []);

  // 切换命令预设编辑的仓库时，回填该仓库已保存的命令
  useEffect(() => {
    setCmdTest(cmdEntry?.test_cmd ?? "");
    setCmdBuild(cmdEntry?.build_cmd ?? "");
    setCmdSaved(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [effectivePath]);

  const saveCmds = async () => {
    if (!effectivePath) return;
    await setRepoCmds(effectivePath, cmdTest.trim(), cmdBuild.trim());
    setCmdSaved(true);
    setTimeout(() => setCmdSaved(false), 2000);
  };

  const handleSaveCreds = async () => {
    await api.setCredentials({ gitName, gitEmail, githubToken, giteeToken, sshKeyPath });
    setCredSaved(true);
    setTimeout(() => setCredSaved(false), 2000);
  };

  const pickKey = async () => {
    const picked = await open({ multiple: false, title: "选择 SSH 私钥文件（如 id_ed25519 / id_rsa）" });
    if (typeof picked === "string") setSshKeyPath(picked);
  };

  const handleSaveProxy = async () => {
    await api.setSettings(addr.trim());
  };

  const toggleAutostart = async () => {
    try {
      if (autostart) {
        await api.autostartDisable();
        setAutostart(false);
      } else {
        await api.autostartEnable();
        setAutostart(true);
      }
    } catch (e) {
      setResult(`❌ 开机自启设置失败：${e}`);
    }
  };

  const handleTest = async () => {
    if (!addr.trim()) return;
    setTesting(true);
    setResult(null);
    try {
      const ok = await api.probe(addr.trim());
      setResult(ok ? "✅ 该代理可用，可以通过它访问 github.com" : "❌ 该代理不可用（连不上或无法访问 github.com）");
    } catch (e) {
      setResult(`❌ 测试失败：${e}`);
    } finally {
      setTesting(false);
    }
  };

  const themeOptions: { value: ThemeMode; label: string; icon: typeof IconMoon }[] = [
    { value: "dark", label: "暗色", icon: IconMoon },
    { value: "light", label: "亮色", icon: IconSun },
    { value: "system", label: "跟随系统", icon: IconMonitor },
  ];

  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal settings-modal" onClick={(e) => e.stopPropagation()}>
        <h3>
          设置
          <button className="popover-close modal-close-right" title="关闭" onClick={onClose}>
            <IconX size={13} />
          </button>
        </h3>

        <div className="settings-body">
          <p className="settings-subtitle">外观</p>
          <div className="settings-section">
            <div className="segmented">
              {themeOptions.map((o) => (
                <button
                  key={o.value}
                  className={`segment ${theme === o.value ? "segment-active" : ""}`}
                  onClick={() => void setTheme(o.value)}
                >
                  <o.icon size={13} />
                  {o.label}
                </button>
              ))}
            </div>
            <label className="settings-row">
              <span>界面字号</span>
              <input
                type="range"
                min={0.85}
                max={1.25}
                step={0.05}
                value={fontScale}
                onChange={(e) => setFontScale(Number(e.target.value))}
              />
              <span className="settings-value">{Math.round(fontScale * 100)}%</span>
            </label>
          </div>

          <p className="settings-subtitle">网络</p>
          <div className="settings-section">
            <label className="switch-row">
              <span>开机自动启动</span>
              <button
                className={`switch ${autostart ? "on" : ""}`}
                role="switch"
                aria-checked={autostart}
                onClick={() => void toggleAutostart()}
              >
                <span className="switch-knob" />
              </button>
            </label>
            <p className="settings-desc">
              通常无需设置：网络不通时会自动探测本机代理（系统代理、7897 / 7890 / 10808 等常见端口），
              仅对当次 git 操作使用。也可手动指定，如 <code>http://127.0.0.1:7890</code>，留空恢复全自动。
            </p>
            <div className="settings-row">
              <input
                className="modal-input"
                placeholder="http://127.0.0.1:7890（留空为自动探测）"
                value={addr}
                onChange={(e) => setAddr(e.target.value)}
              />
              <button className="btn-sm" disabled={testing || !addr.trim()} onClick={() => void handleTest()}>
                {testing ? "测试中…" : "测试"}
              </button>
              <button className="btn-sm btn-ghost" onClick={() => void handleSaveProxy()}>
                保存
              </button>
            </div>
            {result && <div className="modal-result">{result}</div>}
          </div>

          <p className="settings-subtitle">命令预设</p>
          <div className="settings-section">
            <p className="settings-desc">
              为每个仓库保存一键运行的测试 / 构建命令，保存后可从主面板右上 ⋯ 菜单直接运行。
            </p>
            <select
              className="modal-input"
              value={effectivePath}
              onChange={(e) => setCmdRepo(e.target.value)}
            >
              {repos.map((r) => (
                <option key={r.path} value={r.path}>
                  {r.alias || r.name}
                </option>
              ))}
            </select>
            <div className="settings-row">
              <input
                className="modal-input"
                placeholder="测试命令，如：cargo test / pnpm test / pytest"
                value={cmdTest}
                onChange={(e) => setCmdTest(e.target.value)}
              />
            </div>
            <div className="settings-row">
              <input
                className="modal-input"
                placeholder="构建命令，如：cargo build --release / pnpm run build"
                value={cmdBuild}
                onChange={(e) => setCmdBuild(e.target.value)}
              />
            </div>
            <div className="settings-row">
              <button
                className="btn-sm"
                disabled={!effectivePath || (!cmdTest.trim() && !cmdBuild.trim())}
                onClick={() => void saveCmds()}
              >
                {cmdSaved ? "✓ 已保存" : "保存命令"}
              </button>
            </div>
          </div>

          <p className="settings-subtitle">Git 身份与令牌</p>
          <div className="settings-section">
            <div className="settings-row">
              <input
                className="modal-input"
                placeholder="姓名，如 Zhang San"
                value={gitName}
                onChange={(e) => setGitName(e.target.value)}
              />
              <input
                className="modal-input"
                placeholder="邮箱，如 me@example.com"
                value={gitEmail}
                onChange={(e) => setGitEmail(e.target.value)}
              />
            </div>
            <p className="settings-desc">
              推送私有仓库或云端建仓需要令牌：GitHub 填
              <a
                className="link"
                href="https://github.com/settings/tokens"
                onClick={(e) => {
                  e.preventDefault();
                  void openUrl("https://github.com/settings/tokens");
                }}
              >
                Personal Access Token
              </a>
              ，Gitee 填
              <a
                className="link"
                href="https://gitee.com/profile/personal_access_tokens"
                onClick={(e) => {
                  e.preventDefault();
                  void openUrl("https://gitee.com/profile/personal_access_tokens");
                }}
              >
                私人令牌
              </a>
              。留空则使用本机 Git 凭证管理器。
            </p>
            <div className="settings-col">
              <input
                className="modal-input"
                type="password"
                placeholder="GitHub 访问令牌（PAT）"
                value={githubToken}
                onChange={(e) => setGithubToken(e.target.value)}
              />
              <input
                className="modal-input"
                type="password"
                placeholder="Gitee 私人令牌"
                value={giteeToken}
                onChange={(e) => setGiteeToken(e.target.value)}
              />
              <div className="settings-row">
                <input
                  className="modal-input"
                  placeholder="SSH 私钥路径（远端为 git@ 地址时使用，可留空）"
                  value={sshKeyPath}
                  readOnly
                  onClick={() => void pickKey()}
                />
                <button className="btn-sm btn-ghost" onClick={() => void pickKey()}>
                  选择…
                </button>
              </div>
              <div className="settings-row">
                <button className="btn-sm" onClick={() => void handleSaveCreds()}>
                  {credSaved ? "✓ 已保存" : "保存身份与令牌"}
                </button>
              </div>
            </div>
          </div>

          <p className="settings-subtitle">维护</p>
          <div className="settings-section">
            <SpaceContent />
            <p className="settings-desc">README 使用系统默认程序打开。</p>
            <div className="settings-row">
              <button className="btn-sm btn-ghost" disabled={!selected} onClick={() => openReadme(selected!)}>
                打开 README
              </button>
            </div>
          </div>

          <p className="settings-subtitle">关于</p>
          <div className="settings-section">
            <div className="settings-row">
              <span className="settings-value">青鸟 Git{version ? ` v${version}` : ""}</span>
              <span className="settings-copyright">© 2026 青鸟 Git</span>
              <button className="btn-sm btn-ghost" onClick={() => void openUrl(RELEASE_URL)}>
                检测更新
              </button>
            </div>
          </div>
        </div>

        <div className="settings-footer">
          <button className="btn-sm btn-ghost" onClick={onClose}>
            关闭
          </button>
        </div>
      </div>
    </div>
  );
}
```

（`IconX` 需要 import：`import { IconMoon, IconSun, IconMonitor, IconX } from "./icons";`）

- [ ] **Step 3: App.css — 设置弹窗与 space-panel 样式**

修改 `.modal`（原规则保留布局，去掉边框、改 token、限宽 640px 由 `.settings-modal` 承担），并新增：

```css
.settings-modal {
  width: 640px;
  max-width: calc(100vw - 48px);
  padding: 0;
}

.settings-modal h3 {
  padding: 16px 20px 12px;
  border-bottom: 1px solid color-mix(in srgb, var(--gray-0) 8%, transparent);
}

.settings-body {
  max-height: min(70vh, 640px);
  overflow-y: auto;
  padding: 4px 20px 12px;
}

.settings-subtitle {
  margin: 14px 0 6px;
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--ds-text-secondary);
}

.settings-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px 14px;
  background: var(--ds-tile);
  border-radius: var(--radius-md);
}

.settings-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.settings-row > span:first-child {
  flex-shrink: 0;
  font-size: var(--text-sm);
  color: var(--ds-text-secondary);
}

.settings-col {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.settings-desc {
  font-size: var(--text-xs);
  line-height: var(--leading-body);
  color: var(--ds-text-muted);
}

.settings-value {
  font-size: var(--text-sm);
  color: var(--ds-text-secondary);
}

.settings-copyright {
  flex: 1;
  font-size: var(--text-2xs);
  color: var(--ds-text-faint);
}

.settings-footer {
  display: flex;
  justify-content: flex-end;
  padding: 10px 20px 14px;
}

/* 设置内嵌的磁盘空间面板 */
.space-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.space-rescan {
  display: flex;
}
```

同时把 `.modal-subtitle`、`.modal-desc`、`.modal-result`、`.modal-footer`、`.switch-row`、`.switch`、`.switch-knob`、`.segmented`、`.segment`、`.segment-active`、`.space-*` 系列的旧取色（`--bg`/`--card`/`--border`/`--text` 等）换成对应 token（`--ds-bg-secondary` / `--ds-bg-inset` / `--gray-0` α 混色 / `--ds-text-*`），本步只改取值不改布局。

- [ ] **Step 4: 验证**

Run: `pnpm build`
Expected: 无错误。

Run: `pnpm tauri dev` 目检：
Expected: 设置弹窗 640px、分 6 节；主题 segmented 正常；字号滑杆拖动全局字号实时缩放；命令预设下拉可切仓库、保存后 kebab「跑测试/构建」能用上；维护区显示当前选中仓库的磁盘扫描并可删除缓存；关于显示版本号。设置内不再有「保存代理并关闭」整弹窗保存（代理单独保存）。

- [ ] **Step 5: Commit**

```bash
git add src/components/SpaceModal.tsx src/components/SettingsModal.tsx src/App.css
git commit -m "feat: 设置弹窗分节重构（外观/网络/命令预设/身份令牌/维护/关于）"
```

---

### Task 7: 其余弹窗 token 化 + 全局动效

**Files:**
- Modify: `src/App.css`（`.modal` 系列通用类、`.history-*`、`.cmd-*`、`.exclude-*`、`.proxy-toast`、`.progress-*` 取值替换；新增模态/仓库切换动画）
- Modify: `src/App.css`（`.panel` 加切换动画类）

**Interfaces:**
- Consumes: Task 1 token。
- Produces: `@keyframes modal-in`、`@keyframes mask-in`、`@keyframes panel-in`（Task 3 的 `.kebab-menu` 已引用 `menu-in`，此处保证其存在——`menu-in` 定义在 Task 3，若执行顺序不同需保留）。

- [ ] **Step 1: 模态与仓库切换动画**

在 `.modal-mask` 与 `.modal` 规则上追加动画（并替换其取色为 token）：

```css
.modal-mask {
  position: fixed;
  inset: 0;
  z-index: 50;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, #000 45%, transparent);
  animation: mask-in var(--motion-duration-fast) var(--motion-ease-out);
}

@keyframes mask-in {
  from {
    opacity: 0;
  }
}

.modal {
  background: var(--ds-bg-secondary);
  border: none;
  border-radius: var(--radius-xl);
  box-shadow: var(--ds-shadow-dialog), var(--ds-elevation-stroke);
  padding: 20px 22px;
  animation: modal-in var(--motion-duration-normal) var(--motion-ease-out);
}

@keyframes modal-in {
  from {
    opacity: 0;
    transform: scale(0.96);
  }
}
```

`.panel` 追加（仓库切换 fade + 6px 上移，spec §9）：

```css
.panel {
  /* …保留原有 flex 布局取值，仅追加动画… */
  animation: panel-in var(--motion-duration-normal) var(--motion-ease-out);
}

@keyframes panel-in {
  from {
    opacity: 0;
    transform: translateY(6px);
  }
}
```

并在 `RepoPanel.tsx` 的 `<main className="panel">` 上加 `key={selected}` 使切换时重新触发动画（空态 `<main className="panel panel-empty">` 同样加 `key`）：

```tsx
<main className="panel" key={selected}>
```

- [ ] **Step 2: 其余弹窗类 token 取值替换**

将以下选择器中的旧变量换成 token（布局不动）：
- `.modal h3`、`.modal-subtitle`、`.modal-desc`、`.modal-input`、`.modal-row`、`.modal-col`、`.modal-result`、`.modal-footer`、`.modal-label`、`.cmd-row`、`.cmd-name`、`.cmd-text`、`.link`、`.btn-sm`、`.btn-sm:hover`、`.btn-ghost`、`.btn-danger`、`.icon-btn`、`.spinner`
  - 通用规则：`background: var(--card)` → `var(--ds-bg-secondary)`；输入框 → `background: var(--ds-bg-inset); border: none;`，focus 用 `outline: 1px solid var(--ds-accent)`；文字 → `var(--ds-text-*)`；`--accent` → `var(--ds-accent)`；`--red` → `var(--ds-error)`；`--green` → `var(--ds-success)`。
- `.history-item`（history tile）：`background: var(--ds-raised); border-radius: var(--radius-md); box-shadow: var(--ds-raised-shadow); padding: 10px 12px;`，去掉边框；`.commit-hash` 用 `var(--font-mono)` + `var(--ds-accent)`。
- `.proxy-toast`：`background: var(--ds-bg-elevated); border-radius: var(--radius-lg); box-shadow: var(--ds-shadow-dialog), var(--ds-elevation-stroke);` 追加 `animation: toast-in var(--motion-duration-normal) var(--motion-ease-out);`：

```css
@keyframes toast-in {
  from {
    opacity: 0;
    transform: translateY(6px);
  }
}
```

（spec §9：右下角 fade + translateY(-6px→0)，视觉上从下方浮入。）

- [ ] **Step 3: 验证**

Run: `pnpm build`
Expected: 无错误。

Run: `pnpm tauri dev` 目检：
Expected: 克隆 / 新建 / 初始化 / 提交历史 / 命令面板弹窗为深色 raised 卡片、出现时有 scale 动画；切换仓库时主面板有 fade+上移动画；代理 toast 从右下浮入；light 主题下所有弹窗同样正常。

- [ ] **Step 4: Commit**

```bash
git add src/App.css src/components/RepoPanel.tsx
git commit -m "feat: 弹窗 token 化与全局动效"
```

---

### Task 8: App.css 收尾清理 + 验收

**Files:**
- Modify: `src/App.css`（删除不再被引用的旧变量与规则）
- Modify: `src/main.tsx`（无改动，确认即可）

**Interfaces:**
- Consumes: 前 7 个任务的全部成果。
- Produces: 无。

- [ ] **Step 1: 删除死代码**

1. `App.css` 顶部 `:root`/`:root[data-theme=dark]`/`:root[data-theme=light]` 的旧变量块（`--bg`、`--bg-gradient`、`--card`、`--card-hover`、`--bg-inset`、`--border`、`--border-strong`、`--text`、`--text-dim`、`--accent`、`--accent-strong`、`--accent-soft`、`--green*`、`--red*`、`--yellow*`、`--shadow`、`--shadow-sm`、`--gitee*`）整体删除。
2. `body` 规则改为：

```css
body {
  font-family: var(--font-sans);
  font-size: var(--text-base);
  background: var(--ds-bg-primary);
  color: var(--ds-text-primary);
  overflow: hidden;
  user-select: none;
  transition: background-color var(--motion-duration-normal), color var(--motion-duration-normal);
}
```

3. 删除 `--bg-gradient` 背景的使用（新视觉是纯色分频，无径向渐变）。
4. 全文搜索旧变量名（`--bg`、`--card`、`--border`、`--text`、`--accent`（不带 `-soft` 前缀的旧名）、`--red`、`--green`、`--yellow`、`--shadow`），替换为对应新 token：`--card`→`--ds-bg-secondary`、`--bg-inset`→`--ds-bg-inset`、`--border`→删除边框或 `color-mix(in srgb, var(--gray-0) 8%, transparent)`、`--text`→`--ds-text-primary`、`--text-dim`→`--ds-text-muted`、`--accent`→`--ds-accent`、`--red`→`--ds-error`、`--green`→`--ds-success`、`--yellow`→`--ds-warning`、`--shadow`→`--ds-shadow-dialog`、`--shadow-sm`→`--ds-shadow-sm`。替换完成后这些旧变量应零引用。
5. 搜索已无组件引用的类名（如 `.card`、`.panel-header`、`.actions`、`.action-btn`、`.repo-item`、`.progress-box`、`.progress-stage`、`.progress-detail`），确认 `src/` 中无引用后删除其规则。`.card` 若仍被 `.panel-empty` 等引用则就地展开为 token 值后删除 `.card` 类。

- [ ] **Step 2: 全量验证**

Run: `pnpm build`
Expected: 无错误，产物正常产出。

Run: `pnpm tauri dev`，按 spec §11 验收清单逐条目检：
- 主面板呈现 §4 布局（topbar / chips / 三键 Hero / raised 日志区），无明显 1px 边框。
- 三键 Hero 默认/hover/disabled（克隆时互斥）/busy 四态可见、过渡平滑。
- 侧栏 active 行 raised + shadow 抬起，无边框。
- 切 light 主题：侧栏浅灰 `#f3f3f3`、主面板纯白、蓝色 accent 清晰。
- 主题切换、字号变化、自启开关、代理开关、命令预设保存均正常。
- 跑测试 / 构建 / 磁盘清理 / 语言统计不出现在主面板（仅 kebab 与设置内可触发）。
- 现有功能行为不变：检测/拉/推/克隆/新建/初始化/扫描全部/拖拽添加/代理回退 toast。

- [ ] **Step 3: 产物构建**

Run: `pnpm tauri build`
Expected: 成功产出 exe（只验证可构建，不发布）。

- [ ] **Step 4: Commit**

```bash
git add src/App.css
git commit -m "chore: 清理旧主题变量与死样式，完成 v2 视觉收尾"
```

---

## Self-Review 记录

- **Spec 覆盖**：§3 全部 token → Task 1；§4 顶层布局 → Task 2/3/4；§5 组件重组表逐行 → Task 3（topbar 3 按钮、kebab）、Task 4（Hero、日志区）、Task 6（测试/构建/磁盘/README/语言统计/命令预设入设置）、Task 3（主题双入口）；§5.1 Hero 细节（含 dirty stash 提示、ahead/behind 着色）→ Task 4；§5.2 浮窗 → Task 5；§6 侧栏 → Task 2；§7 设置 → Task 6（spec 未列出的「Git 身份与令牌」为现有功能，保留为独立节，不删除）；§8 主题反相 → Task 1 light 块；§9 动效 → Task 3（菜单）、Task 7（模态/仓库切换/toast/进度）、Task 4（tile hover 过渡）；§10 受影响文件逐一对得上；§11 验收 → Task 8。
- **占位符扫描**：无 TBD/TODO；唯一外部未知值是 `RELEASE_URL`（仓库当前无 git remote、README 亦未给出发布页），已在 Task 6 中以常量写入并注释说明；执行时如与实际发布地址不符，改这一行常量即可。
- **类型一致性**：`RepoPanel` 新签名 `{ onOpenSettings: () => void }` 在 Task 3 定义、App.tsx 同步传参；`SpaceContent`（无 props）Task 6 定义并在同任务被 SettingsModal 引用；`IconMore` Task 3 定义并在同任务使用；`showLangs`/`kebabOpen` 状态与消费均在 Task 3 内闭合（语言统计 footer 渲染条件在本任务内改完）；`--font-scale` 写入（Task 6）与消费（Task 1 的 `calc()` 字号）命名一致。
