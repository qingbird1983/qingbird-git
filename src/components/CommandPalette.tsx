// ⌘K 命令面板：低频功能的统一入口。
//
// 定位见 docs/design/2026-09-27-ui-redesign-proposal.html —— 主面板只留执行层与命令层，
// 「打开文件夹 / 终端 / 说明」这类低频动作在这里按名字找，不再往头部堆图标按钮。
//
// 匹配与选中移动的规则都在 src/lib/commands.ts（纯逻辑，有回归测试）；
// 这个组件只负责把 store 拼成命令表、渲染、处理键盘。
//
// 组件由 App 在 paletteOpen 时条件挂载，所以每次打开 state 都是全新的，不用手动重置。

import { useEffect, useMemo, useRef, useState } from "react";
import { useApp } from "../store";
import { matchCommands, moveSelection, type PaletteCommand } from "../lib/commands";
import { IconSearch } from "./icons";

interface Item extends PaletteCommand {
  run: () => void;
  /** 仓库条目前面要带一颗状态点 */
  kind?: "repo";
}

/** 仓库条目的状态点颜色：按 store 里的状态缓存取，跟侧栏列表口径一致 */
function repoDot(path: string): string {
  const st = useApp.getState().status[path];
  if (!st || !st.is_repo) return "idle";
  if (st.behind > 0) return "behind";
  if (st.ahead > 0) return "ahead";
  return "ok";
}

/** 仓库条目右侧的状态提示 */
function repoHint(path: string): string | undefined {
  const st = useApp.getState().status[path];
  if (!st) return undefined;
  if (!st.is_repo) return "仓库不可用";
  if (st.behind > 0) return `落后 ${st.behind}`;
  if (st.ahead > 0) return `待推送 ${st.ahead}`;
  return "已同步";
}

export default function CommandPalette() {
  const repos = useApp((s) => s.repos);
  const selected = useApp((s) => s.selected);
  const status = useApp((s) => s.status);
  const closePalette = useApp((s) => s.closePalette);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // 命令表只依赖仓库列表 / 当前选中 / 状态缓存；执行时才 getState()，
  // 所以 store 里那些 action 函数不会成为这份 memo 的依赖。
  const items = useMemo<Item[]>(() => {
    const s = useApp.getState();
    const list: Item[] = [];

    if (selected) {
      const repo = selected;
      list.push(
        {
          id: "pull",
          title: "拉取",
          group: "仓库操作",
          keywords: "pull fetch download",
          run: () => void s.pullSmart(),
        },
        { id: "push", title: "推送", group: "仓库操作", keywords: "push upload", run: () => void s.push() },
        {
          id: "check",
          title: "检测云端更新",
          group: "仓库操作",
          keywords: "check fetch remote",
          run: () => void s.checkUpdate(),
        },
        {
          id: "terminal",
          title: "在此打开终端",
          group: "仓库操作",
          keywords: "open terminal shell cmd",
          shortcut: "Ctrl T",
          run: () => void s.openInTerminal(repo),
        },
        {
          id: "folder",
          title: "打开文件夹",
          group: "仓库操作",
          keywords: "open folder explorer",
          run: () => void s.openInFolder(repo),
        },
        {
          id: "readme",
          title: "查看仓库说明",
          group: "仓库操作",
          keywords: "readme doc 说明",
          run: () => void s.openReadme(repo),
        },
        {
          id: "website",
          title: "打开仓库主页",
          group: "仓库操作",
          keywords: "open website github gitee",
          run: () => void s.openRepoWebsite(repo),
        },
      );
    }

    list.push(
      {
        id: "add",
        title: "添加本地仓库",
        group: "仓库管理",
        keywords: "add folder local 添加",
        run: () => void s.addRepoDialog(),
      },
      {
        id: "clone",
        title: "从地址克隆仓库",
        group: "仓库管理",
        keywords: "clone download url 克隆",
        run: () => s.openClone(),
      },
      {
        id: "newrepo",
        title: "在云端新建仓库",
        group: "仓库管理",
        keywords: "create new github gitee 新建",
        run: () => s.openNewRepo(),
      },
      {
        id: "scan",
        title: "扫描全部仓库",
        group: "仓库管理",
        keywords: "scan refresh all 扫描",
        run: () => void s.scanAll(),
      },
      {
        id: "settings",
        title: "设置",
        group: "应用",
        keywords: "settings preference config 设置",
        run: () => s.openSettings(),
      },
      {
        id: "theme",
        title: "切换主题",
        group: "应用",
        keywords: "theme dark light system 主题 暗色 亮色",
        run: () => s.cycleTheme(),
      },
    );

    // 切仓库排在最后；关键词里带上真名与全路径，搜名字照样能捞上来
    for (const r of repos) {
      list.push({
        id: `repo:${r.path}`,
        title: r.alias || r.name,
        group: "切换到仓库",
        keywords: `${r.name} ${r.alias ?? ""} ${r.path}`,
        hint: repoHint(r.path),
        kind: "repo",
        run: () => s.select(r.path),
      });
    }
    return list;
  }, [repos, selected, status]);

  const results = useMemo(() => matchCommands(items, query), [items, query]);

  // 按分组切成小节；同一组的连续条目共用一个标题
  const sections = useMemo(() => {
    const out: { group: string; rows: { item: Item; idx: number }[] }[] = [];
    results.forEach((item, idx) => {
      const last = out[out.length - 1];
      if (last && last.group === item.group) last.rows.push({ item, idx });
      else out.push({ group: item.group, rows: [{ item, idx }] });
    });
    return out;
  }, [results]);

  // 查询一变，选中项必须回到第一条：否则第 3 条被选中时输入框一改，Enter 会误触发别的命令
  useEffect(() => {
    setActive(0);
  }, [query]);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // 键盘往下走时把选中项滚进可视区，否则长列表会"越走越看不见"
  useEffect(() => {
    listRef.current?.querySelector(`[data-idx="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const execute = (item: Item) => {
    // 先关面板再执行：要开弹窗的命令（克隆 / 新建 / 设置）不能被面板盖住
    closePalette();
    item.run();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => moveSelection(i, 1, results.length));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => moveSelection(i, -1, results.length));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = results[active];
      if (item) execute(item);
    } else if (e.key === "Escape") {
      e.preventDefault();
      closePalette();
    }
  };

  return (
    <div className="palette-mask" onMouseDown={closePalette}>
      <div
        className="palette"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
        role="dialog"
        aria-label="命令面板"
      >
        <div className="palette-head">
          <IconSearch size={14} className="palette-head-icon" />
          <input
            ref={inputRef}
            className="palette-input"
            value={query}
            placeholder="搜索命令或仓库名…"
            onChange={(e) => setQuery(e.target.value)}
            aria-label="搜索命令"
          />
          <span className="kbd">Esc</span>
        </div>

        <div className="palette-list" ref={listRef}>
          {results.length === 0 && <div className="palette-empty">没有匹配的命令</div>}
          {sections.map((sec) => (
            <div key={sec.group + sec.rows[0].item.id}>
              <div className="palette-group">{sec.group}</div>
              {sec.rows.map(({ item, idx }) => (
                <button
                  key={item.id}
                  data-idx={idx}
                  className={`palette-item${idx === active ? " active" : ""}`}
                  onMouseEnter={() => setActive(idx)}
                  onClick={() => execute(item)}
                >
                  {item.kind === "repo" && (
                    <span className={`palette-dot dot-${repoDot(item.id.slice(5))}`} aria-hidden />
                  )}
                  <span className="palette-item-text">{item.title}</span>
                  {item.hint && <span className="palette-hint">{item.hint}</span>}
                  {item.shortcut && <span className="kbd">{item.shortcut}</span>}
                </button>
              ))}
            </div>
          ))}
        </div>

        <div className="palette-foot">
          <span>↑↓ 选择</span>
          <span>Enter 执行</span>
          <span>Esc 关闭</span>
        </div>
      </div>
    </div>
  );
}
