// 全窗通铺的语言统计页脚（同 qingbird-md 状态条）：钉在窗口最底部一行，
// 从仓库栏下方一直通到右缘，底色与仓库栏相同（--bg2）。
// 自己读 store 拉取当前选中仓库的语言构成，无选中/非仓库时整条隐藏。
import { useEffect, useState } from "react";
import { api, type LangStat } from "../api";
import { useApp } from "../store";

/** 常见语言的显示颜色（对齐 GitHub Linguist 配色） */
const LANG_COLORS: Record<string, string> = {
  Rust: "#dea584",
  TypeScript: "#3178c6",
  JavaScript: "#f1e05a",
  Python: "#3572A5",
  Go: "#00ADD8",
  Java: "#b07219",
  Kotlin: "#A97BFF",
  Swift: "#F05138",
  Ruby: "#701516",
  PHP: "#4F5D95",
  C: "#555555",
  "C++": "#f34b7d",
  "C#": "#178600",
  "Objective-C": "#438eff",
  Scala: "#c22d40",
  Dart: "#00B4AB",
  Lua: "#000080",
  HTML: "#e34c26",
  Vue: "#41b883",
  Svelte: "#ff3e00",
  CSS: "#663399",
  SCSS: "#c6538c",
  Less: "#1d365d",
  Shell: "#89e051",
  PowerShell: "#012456",
  SQL: "#e38c00",
  Zig: "#ec915c",
  Solidity: "#AA6746",
};

function langColor(name: string): string {
  return LANG_COLORS[name] ?? (name === "其他" ? "#8b949e" : "#6e7681");
}

export default function LangFooter() {
  const selected = useApp((s) => s.selected);
  const isRepo = useApp((s) => (s.selected ? s.status[s.selected]?.is_repo : false) ?? false);
  const [langs, setLangs] = useState<LangStat[]>([]);

  // 选中仓库后拉取语言构成；切换时先清空避免闪旧数据
  useEffect(() => {
    setLangs([]);
    if (selected && isRepo) {
      void api.languages(selected).then(setLangs).catch(() => setLangs([]));
    }
  }, [selected, isRepo]);

  if (langs.length === 0) return null;
  return (
    <footer className="lang-footer">
      <div className="lang-bar">
        {langs.map((l) => (
          <span
            key={l.language}
            className="lang-seg"
            style={{ width: `${l.percent}%`, background: langColor(l.language) }}
            title={`${l.language} ${l.percent.toFixed(1)}%`}
          />
        ))}
      </div>
      <div className="lang-legend">
        {langs.map((l) => (
          <span key={l.language} className="lang-item">
            <span className="lang-dot" style={{ background: langColor(l.language) }} />
            {l.language} <b>{l.percent.toFixed(1)}%</b>
          </span>
        ))}
      </div>
    </footer>
  );
}
