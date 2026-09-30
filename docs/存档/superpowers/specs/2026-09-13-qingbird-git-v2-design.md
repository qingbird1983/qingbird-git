# 青鸟 Git v2 — GUI 重设设计

- **日期**: 2026-09-13
- **范围**: 纯前端 GUI 视觉与交互重设。后端 (Rust/Tauri)、数据模型、`store.ts` 行为、`api.ts` 接口、桌面图标 (`scripts/gen-icons.mjs`) 均不在本次范围。
- **基线**: 沿用 Codex/PI-Desktop 的设计语言(中性调、α 文字层、分频无边框、raised 激活态),保留青鸟品牌蓝色作为强调色。

## 1. 目标

1. 把"按钮太乱"的主面板重组成 3 个核心动作 + 1 个日志区 + 少量工具入口,符合用户对"工具"的认知。
2. 把低频功能(命令预设、磁盘清理、语言统计)统一收进「设置」页面,主面板回到"扫描/拉/推/看日志"的本份。
3. 用无边框 + 三层调色板重写视觉,避免到处 1px 描边。
4. 引入 token 化的类型、动效、圆角、阴影系统,后续主题/字号/动效调整只改 token。
5. 保持 dark + light 双主题、跟随系统切换。

## 2. 非目标

- 不改 Rust 后端、Tauri commands、git 子进程逻辑。
- 不动 `icons.tsx`(组件级图标先沿用),后续图标重做不在本期。
- 不改桌面图标(`scripts/gen-icons.mjs`、Tauri `icons/` 目录),后期跟随新视觉做。
- 不引入 UI 框架(继续用原生 React + CSS variables)。
- 不改窗口尺寸/标题栏/托盘菜单。

## 3. 设计语言

### 3.1 调色板

`src/styles/tokens.css`(新建,被 `main.tsx` 引入)定义:

```
dark:
  --ds-bg-primary:    #181818     // 主面板
  --ds-bg-sidebar:    #000000     // 侧栏(深一档)
  --ds-bg-secondary:  #212121
  --ds-bg-tertiary:   #282828
  --ds-bg-inset:      #0d0d0d     // 输入框底/日志区
  --ds-bg-elevated:   color-mix(--ds-bg-secondary 96%, transparent)
  --ds-bg-composer:   color-mix(--ds-bg-secondary 96%, transparent)
  --ds-bg-chip:       color-mix(#fff 6%, transparent)
  --ds-tile:          color-mix(#fff 3.5%, transparent)
  --ds-tile-hover:    color-mix(#fff 6%, transparent)
  --ds-tile-deep:     color-mix(#fff 8%, transparent)
  --ds-raised:        #282828
  --ds-raised-shadow: 0 1px 2px rgba(0,0,0,.35)

light:
  --ds-bg-primary:    #ffffff
  --ds-bg-sidebar:    #f3f3f3
  --ds-bg-secondary:  #f9f9f9
  --ds-bg-tertiary:   #f3f3f3
  --ds-bg-inset:      #ededed
  ...
```

**品牌蓝(青鸟)**:

```
--ds-accent:        #4a9eff   // dark 主题下与当前一致
--ds-accent-strong: #2f7fe8
--ds-accent-soft:   color-mix(#4a9eff 14%, transparent)
light 主题下: --ds-accent: #2f7fe8, accent-soft 透明度更低
```

**状态色**:

```
--ds-success: #40c977 (ahead 推送提示 — 用绿表示可推送,见 §6.2)
--ds-warning: #ff8549 (dirty 未提交)
--ds-error:   #ff6764 (missing 失联、网络错误)
--ds-info:    var(--gray-300)
```

**α 文字层**:

```
--ds-text-primary:   var(--gray-0)          /* #fff in dark */
--ds-text-secondary: color-mix(#fff 70%, transparent)
--ds-text-muted:     color-mix(#fff 52%, transparent)
--ds-text-faint:     color-mix(#fff 38%, transparent)
light 主题用 #1a1c1f 替代 #fff。
```

### 3.2 类型

```
--font-sans: -apple-system, "Segoe UI", "Microsoft YaHei", system-ui, sans-serif
--font-mono: "Cascadia Mono", "Consolas", "SF Mono", monospace

--text-2xs: 11px    --text-xs: 11.5px   --text-sm: 12px    --text-base: 13px
--text-md:  13.5px  --text-lg: 15px     --text-xl: 17px    --text-2xl: 20px
字号可被 --font-scale 整体缩放(默认 1,留给未来字号偏好)。

--leading-tight:    1.25
--leading-body:     1.45
--leading-relaxed:  1.55

--tracking-tight:   -0.01em
--tracking-normal:  0
```

### 3.3 圆角

```
--radius-2xs: 4    --radius-xs: 6    --radius-sm: 8
--radius-md:  10   --radius-lg: 12   --radius-xl: 16
--radius-2xl: 20   --radius-full: 9999
```

### 3.4 阴影 / 描边

```
--ds-shadow-sm:        0 1px 2px rgba(0,0,0,.25)
--ds-shadow-dialog:    0 16px 48px rgba(0,0,0,.55)
--ds-shadow-composer:  0 3px 7.5px #0000000a, 0 0 20px #0000000d
--ds-elevation-stroke: 0 0 0 0.5px color-mix(#fff 12%, transparent)
```

### 3.5 动效

```
--motion-duration-fast:   150ms
--motion-duration-normal: 200ms
--motion-duration-slow:   300ms
--motion-ease-out:        cubic-bezier(.22, 1, .36, 1)
--motion-ease-in:         cubic-bezier(.4, 0, 1, 1)
--motion-ease-standard:   cubic-bezier(.2, 0, 0, 1)
```

## 4. 顶层结构

```
┌─ 侧栏 260px ─────────┬─ 主面板 bg-primary ────────────────────────┐
│ header               │ topbar                                       │
│  · brand + 三入口   │  · 左:仓库显示名 + 分支(mono)             │
│  · (新建/克隆/添加)  │  · 右:[主题] [设置]                          │
├──────────────────────┼──────────────────────────────────────────────┤
│ repo 列表            │ status chips 行                              │
│  · active 行 =       │  · branch · ahead N · behind N · dirty       │
│    --ds-raised +     │  · host badge(GitHub/Gitee)                  │
│    shadow            │                                              │
│  · hover =           ├──────────────────────────────────────────────┤
│    --ds-tile-hover   │ 三键 Hero(3 个等宽大块)                      │
│  · 普通 = 透明       │  [检测] [拉取] [推送]                        │
│                      │  各自右侧 detail 行(↑N / ↓N / 已最新)        │
│ footer               ├──────────────────────────────────────────────┤
│  · 扫描全部          │ log stream 区(raised,内部滚动)              │
│  · 主题 · 设置       │  · 头部:[操作日志]  · 终止按钮(运行时)     │
│                      │  · 内容:monospace 时间戳 + 文本             │
└──────────────────────┴──────────────────────────────────────────────┘
```

**弹层**:代理 toast 右下角;commit 变更 / 同步内容 popover 仍以小浮窗形式从对应按钮下方展开,沿用 raised + elevation-stroke 描边。

## 5. 组件重新组织

| 区域 | 当前 | 新版 | 说明 |
|------|------|------|------|
| 主面板顶部工具栏 | 7 个 icon-btn(文件夹/终端/历史/磁盘/README/命令/diff) | **3 个**:打开文件夹 / 提交历史 / 终端 | diff 移到 §5.2 折叠卡 |
| 主面板 actions 行 | 5 个均分胶囊(检测/拉/推/测试/构建) | **3 键 Hero**(检测/拉/推) | 测试/构建收进设置 |
| 状态 chips | 顶部右侧,4-5 个 | status chips 行,3-4 个 | host badge 保留 |
| 日志区 | 底部 card | 仍底部,改为 raised | 头部加终止按钮(已有) |
| 跑测试/构建 按钮 | actions 行 | **设置 → 命令预设** | 保存的命令下拉触发,主面板不显示 |
| 磁盘空间清理 按钮 | 顶部 icon | **设置 → 维护** | 单独设置页 |
| README 打开 按钮 | 顶部 icon | **设置 → 文件关联**(默认系统关联) | 删除入口,设置里改 |
| 语言统计 | 主面板底部 footer | **设置 → 仓库 → 语言统计**(默认折叠) | 不再出现在主面板 |
| 主题切换 | 侧栏底部 icon-btn | 侧栏底部 + topbar 右侧 两处可点 | 行为不变 |
| 工具栏溢出菜单(kebab) | 新增 | topbar 右上 ⋯ 下拉菜单 | 收纳「跑测试 / 构建 / 命令面板 / 磁盘清理 / 语言统计」,按设置中保存的命令触发 |

### 5.1 三键 Hero(核心)

布局:

```
┌────────────────────────────────────────────────────────────────┐
│ 检测云端更新                       ↑3 待推送                    │
│ [icon-cloud]                                                      │
├────────────────────────────────────────────────────────────────┤
│ 一键拉取  --ff-only             ↓2 落后云端 / 已最新            │
│ [icon-cloud-down]                                                 │
├────────────────────────────────────────────────────────────────┤
│ 一键推送                         ↑5 待推送 / 已推送              │
│ [icon-cloud-up]                                                   │
└────────────────────────────────────────────────────────────────┘
```

样式:
- 容器:`--ds-tile` 底色,`--radius-lg`(12px) 圆角,无边框。
- 每个键:`padding: 14px 16px`,两列(左 icon+标签,右 detail 行),文字两行,标签 `--text-lg`,detail `--text-sm`。
- hover:`--ds-tile-hover`(整块过渡 `--motion-duration-fast`)。
- disabled(网络操作互斥):opacity 0.5 + cursor not-allowed。
- busy(正在拉/推):`--ds-tile-deep` 底色 + 内嵌进度条(细线 3px)。
- ahead>0 时,「推送」键右侧 detail `--ds-accent` 色 + ↑N。
- behind>0 时,「拉取」键右侧 detail `--ds-warning` 色 + ↓N。
- dirty 时,「拉取」键 detail 追加「(会先 stash)」灰色提示。

### 5.2 commit / 同步 popover(保留浮窗)

- 触发:变更指示器按钮(主面板右上,仅 dirty 时显示)、同步内容按钮(已拉取后显示)。
- 浮窗:`--ds-shadow-dialog` + `--ds-elevation-stroke`,`--radius-xl`(16px),`padding: 14px 16px`。
- 内部列表:每条 commit/file `padding: 6px 8px`,`--radius-xs`,hover `--ds-tile-hover`。
- commit 提交行:输入框无边框、底部 alpha 线 1px、focus 变 `--ds-accent`。

## 6. 侧栏

### 6.1 结构

```
.sidebar                          bg = --ds-bg-sidebar(深一档)
├── .sidebar-header               padding 16px 14px 12px
│   ├── .brand                    品牌 logo + "青鸟 Git"
│   └── .header-actions           3 个 icon-btn:新建 / 克隆 / 添加
├── .sidebar-section "仓库"       12px 小标题,muted
│   └── .repo-list                flex:1, overflow:auto
│       ├── .repo-group(host)     仅多于 1 个 host 时显示分组
│       └── .repo-row             无边框行
└── .sidebar-footer               padding 10px 14px
    ├── .scan-btn                 「扫描全部」pill 按钮
    └── .footer-actions           [主题] [设置]
```

### 6.2 repo-row 状态视觉

| 状态 | 样式 |
|------|------|
| 默认 | 透明底,左 26px lead 区(显示 repo icon),右 repo 名 + 路径/分支 |
| hover | `--ds-tile-hover` 整行,出现「重命名 / 移除」inline icon(覆盖 repo icon 位) |
| active(选中) | `--ds-raised` 底 + `--ds-raised-shadow` 抬起来,repo icon 变 `--ds-accent` |
| missing | 整行 opacity 0.55 + 红色「失联」badge |
| dirty | 右侧 detail 行前缀橙色 `·` |
| ahead | 右上 small badge `↑N`(`--ds-success` 色,在 dark 下偏亮) |
| behind | 右上 small badge `↓N`(`--ds-accent` 色,表示有更新可拉) |
| host | 右上 badge `GitHub` / `Gitee`(单色,与 active 不冲突) |

分组(GitHub / Gitee / 本地 / 失联)用 12px 分组标题行 + 上下 4px padding,只在超过 1 个分组时渲染,无 stroke。

## 7. 设置(新页面)

**入口**:topbar 右侧齿轮 icon、侧栏 footer 齿轮 icon。

**结构**(简单单页,无侧边导航):

```
.modal-mask
└── .modal .settings-modal       width: 640px, --ds-shadow-dialog
    ├── .settings-header         标题「设置」 + 关闭按钮
    ├── .settings-section
    │   ├── 「外观」
    │   │   · 主题 segmented(dark / light / 跟随系统)
    │   │   · 字号 slider(改 --font-scale)
    │   ├── 「网络」
    │   │   · 代理设置(代理探测、强制启用开关)
    │   │   · 自启开关(已存在,移到这)
    │   ├── 「命令预设」
    │   │   · 仓库下拉框选择要编辑的仓库
    │   │   · 测试命令 input + 保存按钮
    │   │   · 构建命令 input + 保存按钮
    │   │   · (本期不做全局命令预设,沿用每个仓库单独存;命令执行入口见 §10)
    │   ├── 「维护」
    │   │   · 扫描仓库磁盘空间 + 清理缓存按钮(搬自 SpaceModal)
    │   │   · README 关联程序(默认系统关联)
    │   └── 「关于」
    │       · 版本号、版权、检测更新按钮
    └── .settings-footer          关闭按钮
```

设置打开时仍是 modal 浮层(不真的换页面),沿用 `--ds-shadow-dialog` + 描边。

## 8. 主题

- `html[data-theme="dark"]` / `html[data-theme="light"]` 两个根块。
- 主色在 light 主题下反相(`--ds-accent` 变深蓝 `--ds-accent-strong`)。
- 侧栏颜色保持深一档(light 主题下侧栏是浅灰 `#f3f3f3`,主面板纯白 `#fff`,仍对比明显)。

## 9. 动效

- repo 切换:主面板内容 `fade + 6px translateY`(200ms,`--motion-ease-out`)。
- 模态出现:`opacity 0→1 + scale 0.96→1`(200ms,`--motion-ease-out`)。
- hover 状态:所有 `--ds-tile*` 过渡 `--motion-duration-fast`(150ms)。
- 进度条:indeterminate 1.2s 循环,determinate 用 `--motion-ease-standard`。
- 代理 toast:右下角 fade + translateY(-6px→0),200ms,自动 5s 消失(沿用现有逻辑)。

## 10. 受影响文件

| 文件 | 改动 |
|------|------|
| `src/styles/tokens.css` | **新建**。定义 §3 全部 token,被 `main.tsx` import。 |
| `src/App.css` | **重写**。只保留 base + 通用组件样式,所有具体视觉改用 token + 局部类。 |
| `src/main.tsx` | 加 `import "./styles/tokens.css"`;字体栈换 token。 |
| `src/App.tsx` | 微调:新增 settings modal 的 show 状态(由 topbar/侧栏入口触发)。 |
| `src/components/RepoList.tsx` | 类名调整:`repo-item` → `repo-row`,active 用 `--ds-raised`;footer 加分组容器;host badge 重排。 |
| `src/components/RepoPanel.tsx` | 大改:① 新增 topbar 块(仓库名 + 分支 + 主题 + 设置 + kebab 溢出);② 删除"打开 README/磁盘"两个 icon-btn;③ 5 按钮行 → 3 键 Hero;④ status chips 单独一行;⑤ log 区头部加终止按钮(已有);⑥ 语言统计改为可选折叠(默认隐藏)。 |
| `src/components/SettingsModal.tsx` | 大改:分节、分组;命令预设、维护、文件关联三个新区块。 |
| `src/components/CloneModal.tsx` / `NewRepoModal.tsx` / `InitRepoModal.tsx` | 仅类名调整,视觉用 token 化。 |
| `src/components/HistoryModal.tsx` | 类名调整,history-item 改 raised tile。 |
| `src/components/CmdPanel.tsx` | 删除主面板入口调用点(命令面板暂留,后续从设置触发时仍可用)。 |
| `src/components/SpaceModal.tsx` | 整窗逻辑搬到设置 → 维护区块,文件可保留供设置引用。 |

## 11. 验收

- [ ] `pnpm tauri dev` 启动后,主面板呈现 §4 布局,无明显 1px 边框。
- [ ] 三键 Hero 各自状态(默认/hover/disabled/busy)可见且过渡平滑。
- [ ] 侧栏 active 行有"抬起"视觉(raised + shadow),无边框。
- [ ] 切到 light 主题:侧栏浅灰、主面板纯白,蓝色 accent 仍清晰。
- [ ] 主题切换、字号变化、自启开关、代理开关、命令预设保存 均正常(沿用现有 store)。
- [ ] 跑测试 / 构建 / 磁盘清理 / 语言统计 不再出现在主面板,可在设置中触发。
- [ ] 现有功能(检测/拉/推/克隆/新建/初始化/扫描全部/拖拽添加/代理回退)行为不变。
- [ ] `pnpm build` 通过,`pnpm tauri build` 可产出 exe。
