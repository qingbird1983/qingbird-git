# 青鸟 Git · 界面设计规范 **v4**（视觉契约）

> **这份文档管什么**：任何看得见的东西——颜色、圆角、间距、字号、阴影、动效、图标、控件形态。
> 它是一份**契约**，不是施工记录。判据只有一条：**新东西进来时，能不能在这份文档里找到它该用的那一档。**
> 找不到就先补文档，不许先在代码里写一个数。

## 0. 取代关系与来源

**本文档取代** [`UI-DESIGN.md`](./UI-DESIGN.md) 的 **第 4 章（视觉语言）、第 5 章（交互状态）**，以及其中所有颜色/圆角/间距的具体数值。

**仍然有效、不在本文档重复**的部分（去旧文档看，别在这里另写一份）：旧文第 1–3 章版面结构与各页布局、第 6 章功能分层与唯一入口对照、第 7 章文案、第 8 章键盘、第 9 章官方组件对照、第 10 章实施顺序。

> ⚠️ **这是刻意的分工，不是两份规范并存。** 旧文档管"哪里放什么"，本文档管"它长什么样"。
> 一条视觉规则只允许有一个家。写进旧文档的视觉值，实施时按本文档改，改完旧文档那处必须删掉或指向本文档。

**数值来源**（不是凭印象）：

| 来源 | 取了什么 |
|---|---|
| `prototype/redesign-v4.html`、`prototype/settings-v4.html` | 全部色值、圆角、间距、阴影、组件形态。渲染图在 `prototype/_shots/` |
| `F:\Warehouse\AAA\555\A (1..6).png` | 设计语言方向（Minari 那套）：冷灰底 + 白浮岛卡、深墨绿主色、大圆角、胶囊、全大写标签、粗图标 |
| cargo registry `gpui-component-0.7.0`、`gpui-pre-0.3.7` | **能力边界**。所有"做得到/做不到"的结论只认这两版锁定的源码，不认上游 clone |

---

## 1. 四条铁律

1. **一个属性一个出处。** 同一个语义的量（选中面、卡外壳、行高、时长……）全站只允许有一个函数或常量。发现两处各写一份，**先收敛再改样式**——历史上这类分叉长出过 9 种圆角、6 种按钮高度。
2. **禁写裸数字。** 颜色、圆角、间距、字号、时长、阴影，一律走本文档的档；没有档就先加档，加档要同时给深色值和单测。
3. **不演拿不到的效果。** 目标框架没有的能力，不许出现在设计稿里冒充"能落地"。字距、`text-transform`、粗笔画图标、负字距**都因此被删过**（见第 9 节）。
4. **深色不是浅色的取反。** 每新增一档颜色，深色值必须一起定，且要满足第 8 节那五条成立条件。只给浅色值的档位 = 一处"在深色下不成立"的地方。

---

## 2. 色板

### 2.1 浅色（`qingbird-light`）

| 令牌 | 值 | 用途 | 落点 |
|---|---|---|---|
| `canvas` | `#E8EDF0` | 画布。**冷灰蓝**，与白卡差 23/255，卡片靠这个色差浮起来 | theme.json `background` |
| `canvas-deep` | `#DDE5E9` | 窗口外的桌面底色（仅原型用，应用内无此层） | — |
| `card` | `#FFFFFF` | 卡、侧栏、弹层 | theme.json `secondary.background` |
| `card-sunken` | `#F4F7F8` | 卡内下沉区：输入框底、分组盒、表头 | theme.json `muted.background` |
| `ink` | `#0C1B1F` | 一级文字、图标默认色 | theme.json `foreground` |
| `body` | `#33474C` | 正文 | tokens.rs `body()` |
| `muted` | `#6B8085` | 次要文字、表头、段说明 | tokens.rs `tier3()` |
| `faint` | `#9AABAF` | **只许用于占位符与装饰**，不许承载信息（对比度 2.4:1） | tokens.rs `tier4()` |
| `line` | `#D7E0E4` | 主描边、分隔线 | theme.json `border` |
| `line-soft` | `#E8EEF0` | 卡内分隔、卡描边 | theme.json `table.row.border` |
| `brand` | `#0E4F55` | **深石油绿。品牌主色**：主按钮、选中图标、折线、链接 | theme.json `accent/primary/link/chart.1` |
| `brand-hover` | `#166A71` | 主按钮悬停 | `primary.hover.background` |
| `brand-press` | `#0A3D42` | 主按钮按下 | `primary.active.background` |
| `brand-ink` | `#FFFFFF` | 主按钮上的字 | `primary.foreground` |
| `aqua` | `#57C3CB` | 焦点环、输入框悬停描边、图表次色 | `ring` / `drag.border` |
| `wash` | `#DCEFF0` | **选中面、图标底块**——本规范最重要的手法，见 5.3 | `sidebar.accent.background` |
| `wash-soft` | `#EEF6F7` | 悬停面、分组底 | `secondary.hover.background` |
| `stream` | `#0C1B1F` | 日志输出流的墨底 | 新增 |
| `stream-fg` | `#C3D4D7` | 输出流正文 | 新增 |
| `stream-dim` | `#6E858A` | 输出流的 git 噪音行 | 新增 |
| `ok` / `ok-wash` | `#1E8A4C` / `#D6F0DD` | 成功 | `success.*` |
| `warn` / `warn-wash` | `#B85C12` / `#FBE9D6` | 落后、可清理 | `warning.*` / `chart.3` |
| `bad` / `bad-wash` | `#B23A3E` / `#F8E2E3` | 失败、分叉 | `danger.*` |
| `info` / `info-wash` | `#0E4F55` / `#DCEFF0` | 待推送、进行中 | `info.*` |

**卡内的盒只有两档，判据是「里面那一样东西点不点得动」**：

* **装可点的行**（列表、表格、勾选项）→ `r_lg` + 1px `line-soft` 边，**不给底**（继承 `card` 白）。
  给底就把行与底块糊在一起；而 `card-sunken` 那档（#F4F7F8）比悬停面 `wash-soft`（#EEF6F7）
  还浅一档，铺在它下面 hover 直接看不见。
* **装一段只读内容**（变更清单、模块勾选栏、输入框）→ `r_lg` + `card-sunken` 底 + 1px `line-soft` 边。

⚠️ 两档的边**都是 `line-soft`**，不许写 `line`：那一格比卡自己的描边还重，坐在卡里就是
"里圈比外圈粗"。⚠️ 也别拿 `popover.background` 顶 `card-sunken`——它在两条主题里
都等于 `card` 白（浅 #FFFFFF / 深 #16262B），拿它当下沉就是白里画了个白框。
落点：`widgets::list_box` / `widgets::sunken_box`（**全站只这两个出处**）。

**语义色的用法**：**浅 wash 底 + 深字**，不是实底白字。徽章、告警段、状态胶囊全走这一式。

**语义色的 hover / active 是推出来的，不是现编的**：`hover = 本体明度 ×0.87`、`active = ×0.75`。 系数取自旧版三档的实测比例（success 0.828 / 0.691，warning 0.878 / 0.738，danger 0.884 / 0.767 的中值）， 换色相不换手感。落定的六个值：

| 本体 | hover | active |
|---|---|---|
| `ok` `#1E8A4C` | `#1A7842` | `#176839` |
| `warn` `#B85C12` | `#A05010` | `#8A450E` |
| `bad` `#B23A3E` | `#9B3236` | `#862B2E` |

⚠️ **`brand` 的 hover / press 不走这条规则**（`#166A71` / `#0A3D42` 是设计值，见上表）。 主色那一族压到 ×0.87 会闷成一块看不出颜色的黑绿——**主色要的是"亮一点"，语义色要的才是"暗一点"**。

### 2.2 深色（`qingbird-dark`）

| 令牌 | 值 | 与浅色的关系 |
|---|---|---|
| `canvas` | `#0B1417` | 不是 `#E8EDF0` 的反色，是另定的深墨蓝 |
| `card` | `#16262B` | **比浅色版差更大**：与 canvas 差 20/255，见 8.2 |
| `card-sunken` | `#101E22` | 比卡更暗（浅色是比卡更暗，方向一致） |
| `ink` / `body` / `muted` / `faint` | `#E6EFF0` / `#BACACE` / `#82979B` / `#5D7074` | 文字**变亮**不是变暗 |
| `line` / `line-soft` | `#2E454C` / `#213539` | 深色下描边是主力，见 8.2 |
| `brand` | **`#3FB9C4`** | ⚠️ **必须提亮**。`#0E4F55` 压在深底上等于一块黑，整屏失去主色 |
| `brand-hover` / `brand-press` | `#58CCD5` / `#2F9CA6` | 悬停更亮、按下更暗（浅色相反） |
| `brand-ink` | `#062125` | 亮底配暗字 |
| `wash` / `wash-soft` | `#17383F` / `#112A30` | 选中/悬停面 |
| `ok-wash` / `warn-wash` / `bad-wash` | `#10281A` / `#1E1610` / `#331A1C` | ⚠️ **深底 + 亮字**。浅色 wash 铺在深卡上是一块刺眼的白斑 |

### 2.3 反差判据（这条是硬指标）

**卡与画布的最大通道差必须 ≥ 12/255。** 低于它，卡片边界就得靠描边硬撑，"浮起来"这个感觉就没了。

| | 计算 | 结果 |
|---|---|---|
| 浅色 | `#FFFFFF` − `#E8EDF0` = (23, 18, 15) | **23/255** ✅ |
| 深色 | `#16262B` − `#0B1417` = (11, 18, 20) | **20/255** ✅ |
| 改造前 | `#FFFFFF` − `#FAFAF9` = (5, 5, 6) | **5/255** ❌ 这就是"普通"的头号根因 |

### 2.4 对比度实测（白底，WCAG 口径）

| 色 | 对白底 | 判定 |
|---|---|---|
| `ink` | 17.6:1 | 正文、标题，无限制 |
| `brand` | 9.3:1 | 可作文字色、可作按钮底（白字 9.3:1） |
| `body` | 9.8:1 | 正文 |
| `muted` | 4.1:1 | **只用于 ≥ 12px** 的次要文字。11px 用它就掉出 AA |
| `warn` | 4.6:1 | 徽章文字下限 **12px** |
| `bad` | 5.9:1 | 无限制 |
| `ok` | **4.4:1** | ⚠️ **已知不足**：低于 AA 的 4.5。徽章文字最小 12px + 半粗；不许拿它当正文色 |
| `faint` | 2.4:1 | 装饰/占位符专用，不许承载信息 |

> `ok` 这一条是量出来的缺陷，不是选择。要修就得把 `#1E8A4C` 压深到约 `#17753E`（5.2:1），代价是成功绿变闷。**当前决定：保留现值 + 限制用途**，深色档另算。

---

## 3. 档位表

### 3.1 圆角

| 档 | 值 | 用在哪 |
|---|---|---|
| `R_SM` | 4 | 小色块、进度条段、图例点 |
| `R_MD` | 6 | 键帽、图标小底 |
| `R_LG` | **14** | 按钮、输入框、列表行、卡内盒、导航项 |
| `R_XL` | **20** | 大卡、弹窗、抽屉 |
| `R_PILL` | 999 | **一切按钮、徽章、胶囊、开关、页签组容器** |

⚠️ **圆角只有一个出处，这条以前漏过。** `gui/theme.json` 的 `radius` / `radius.lg` 会被一大片官方件直接读（`button.rs:625`、`alert.rs:180`、`combobox.rs:993`、`group_box.rs:176`、`dock/tab_panel.rs:70`、`command/state.rs:715`）。所以：

- `R_LG` 必须**等于** `cx.theme().radius`，`R_XL` 必须**等于** `radius.lg`——**写成读主题的函数，不写常量**，否则"按钮是 14 而我们的卡是 12"这种分叉必然发生（同 D56 的形状）。
- `R_SM` / `R_MD` 保持常量：小控件不跟主题走。
- 提档有**联动**，改完必须逐个抓帧看：`checkbox.rs:245` 是 `radius.min(px(4))`、`kbd.rs:253` 是 `radius.half()`、`button.rs:624-626` 是 `×0.5 / ×1 / ×2`。
- 上限安全：`gpui-pre/style.rs:706` 绘制前 `clamp_radii_for_quad_size` 会把每角夹到 `min(w,h)/2`，所以胶囊填 999 不会画爆。

### 3.2 间距

| 档 | 值 | 语义（**这两条不许混**） |
|---|---|---|
| `GAP_DENSE` | 2 | **一串同类元素相邻两项**的呼吸量（列表行、色块分段、同块多行元信息） |
| `GAP_TIGHT` | 4 | **两个不同东西**之间的关系（图标与文字、标题与说明、柱间） |
| `GAP` | 8 | 常规：行内元素、按钮内图标与字 |
| `GAP_LG` | 12 | 分组：**卡片之间**、卡内区块之间 |
| `GAP_XL` | **16** | **卡内边距**（v4 新增档） |

**卡间 = `GAP_LG` 12，卡内 = `GAP_XL` 16。**

> ⚠️ 原型用的是 10 和 16。**落地时卡间用 12，不新增 10 这一档**——为 2px 开一档，下一处就会开出 11 和 13。真机比原型略松，这是有意的偏差，不是照搬像素。
>
> ⚠️ **卡间距只允许由父容器的 `gap` 给一次。** 阶段 28 踩过：给滚动容器又加了一层 `padding-top`，叠在父 gap 上 ⇒ 头卡下 20、卡间 10，看着"就是不齐"而根因不是数字。**判据：数一数这个间距被谁写了几次。**

### 3.3 字号

沿用阶段 28 定的三档运行期可切（`UiScale::metrics()`），**本文档不重复其值与不变式**——见 `gui/src/tokens.rs` 头注释与 `the_bottom_three_tiers_are_far_enough_apart_to_read_as_levels`。

v4 只补一条用法：**层级靠"字号档差 + 字重 + 颜色退档"三样叠**，因为字距拿不到（9.1）。

| 层级 | 字号 | 字重 | 颜色 |
|---|---|---|---|
| 页面主标题 | `fs_head` | 800 | `ink` |
| 卡标题 / 弹窗标题 | `fs_title` | 700 | `ink` |
| 正文 | `fs_body` | 400–500 | `body` |
| 表头 / 段标签 | `fs_sec` | **700** | `muted` |
| 元信息 / 时间戳 | `fs_meta` | 400–600 | `muted` |
| 占位 / 装饰 | `fs_meta` | 400 | `faint` |

**行高与胶囊高必须是字号的函数**：`row_h(text_px)`、`pill_h()`。写死会在大档切字，而那种缺陷 check 零错、单测全绿、抓帧只是"有点挤"。

### 3.4 阴影四档

**v4 把阴影压薄了**——他明确说"卡片太厚了"。模糊半径从 24px 砍到 6px，黑色不透明度砍一半。

| 档 | 浅色值 | 深色值 | 用在哪 |
|---|---|---|---|
| `SH_1` | `0 1px 2px rgba(12,27,31,.04)` | `0 1px 2px rgba(0,0,0,.24)` | 内嵌小件：键帽、图标底块、选中页签 |
| `SH_2` | `SH_1 + 0 2px 6px -2px rgba(12,27,31,.09)` | `SH_1 + 0 2px 6px -2px rgba(0,0,0,.34)` | **卡片静置（默认）** |
| `SH_2B` | `SH_1 + 0 4px 12px -4px rgba(12,27,31,.13)` | `SH_1 + 0 4px 12px -4px rgba(0,0,0,.42)` | **卡片悬停** |
| `SH_3` | `0 2px 4px rgba(12,27,31,.06) + 0 18px 44px -12px rgba(12,27,31,.22)` | `0 2px 4px rgba(0,0,0,.34) + 0 18px 44px -12px rgba(0,0,0,.62)` | **浮层**：弹窗、菜单、气泡 |

**判据**：`SH_2 → SH_2B` 是"重一点就够"，跳到大档会像换了一颗控件。**只有浮层能跨到 `SH_3`**——它要回答的是"我不在这一层"，不是"我被选中了"。

⚠️ **官方件的阴影压不了**：`button.rs:643` 只允许 `shadow_xs()` 这一个预设，且被 `cx.theme().shadow` 那个**布尔总开关**门控。`theme.json` 里没有阴影形状键（`ThemeConfigColors` 123 个键一个都没有），别往里填 `BoxShadow`。⇒ **卡能压薄，官方按钮那圈仍以基座给的为准。**

**落地进度（2026-10-07）**：只有 `SH_2` 落了（`tokens::sh_2(t)` → `widgets::card`）。
`SH_1` / `SH_2B` / `SH_3` **没有调用点所以没写**——留一份没人用的档，`cargo check` 每次报
unused，而"零错零警"是本项目的出口条件。⚠️ 其中 `SH_2B`（卡悬停）卡在一处基座事实上：
**gpui 的裸 `Div` 没有 `.hover()` 样式入口**（全 crate 只有 `inspector.rs:94` 那个内部用的），
悬停必须走 `on_hover` 记状态 ⇒ 那是版面改造，跟着阶段 31.4 一起进来。

### 3.5 图标尺寸与 wash 底块

| 档 | 值 | 用在哪 |
|---|---|---|
| `ICON_SM` | 14 | 行内小图标、胶囊内 |
| `ICON` | 16 | 按钮内 |
| `ICON_LG` | **20** | wash 底块内、导航轨 |
| `GLYPH` | **40×40，圆角 12，内图标 20** | 唯一的"图标底块"档 |

⚠️ **`GLYPH` 只有一档，这条是我在写这份文档时才发现的自纠。** 原型里我写了 42（统计卡）、40（导航轨）、46（空态）三种——**正是本铁律 1 要治的病**。统一成 40，圆角 12，内图标 20。

---

## 4. 动效规范

### 4.1 只准用官方 `MotionTokens` 的档，不许自己定数字

**基座已经给了一组语义动效档**（`gpui-component-0.7.0/src/theme/motion.rs:8-42`），读法 `cx.theme().motion_tokens()`：

| 档 | 值 |
|---|---|
| `duration_instant` | 0 |
| `duration_fast` | **120ms** |
| `duration_normal` | **180ms** |
| `duration_slow` | **280ms** |
| `easing_enter` | `cubic-bezier(.16, 1, .3, 1)` — 进场，末段极缓，有"落位"感 |
| `easing_exit` | `cubic-bezier(.4, 0, 1, 1)` — 退场，加速离开 |
| `easing_move` | `cubic-bezier(.2, 0, 0, 1)` — 位移 |
| `distance_short` | `rems(0.25)` = **4px** |
| `distance_medium` | `rems(0.5)` = **8px** |

⇒ **时长只准这四档、缓动只准这三条。** 我在原型里写了 `.14/.15/.16/.18/.3/.35s` 六种时长和两种缓动混用——那是野数字，本规范把它收进上表。

> ⚠️ **一个纠正**：`AGENTS.md` 说「theme.json 没有 motion 段，动效只能落在代码里」。前半句对（`ThemeConfig` 确实没有 motion 键，`SemanticThemeConfig` 也没有），但结论容易被读成"所以得自己定数字"。**实际是：档已经存在且可 `cx.theme()` 读，日常用档就够；只有要改档值本身时才需要在代码里覆写 Theme。**

### 4.2 场景 → 档

| 场景 | 时长 | 缓动 | 备注 |
|---|---|---|---|
| 纯颜色/不透明度变化（hover 变色、选中面切换、描边） | `duration_fast` | `easing_move` | 颜色没有"进出"，别用 enter/exit |
| 小位移（按钮上浮 1px、卡片上浮 2px） | `duration_fast` | `easing_enter` | 位移值见 4.3 |
| 浮层**出现**（气泡、菜单、对话框、⌘K） | `duration_normal` | `easing_enter` | 配 `distance_short` 的 4px 进场位移 |
| 浮层**消失** | `duration_fast` | `easing_exit` | 出场必须比进场快——这是那两条曲线存在的理由 |
| 主题整屏切换 | `duration_slow` | `easing_move` | 唯一允许"慢"的非面板场合 |
| 面板开合（侧栏、检视抽屉，大位移） | — | **弹簧** | 见 4.4 |
| 骨架屏呼吸 | 官方 `Skeleton` 自带 | — | 别自写 |
| 进度动画 | `with_animation` | — | 官方件优先 |

### 4.3 位移档

只准 **0 / 1 / 2 / 4 / 8 px**：

- `1` 按钮 hover 上浮
- `2` 卡片 hover 上浮
- `4` = `distance_short`，浮层进场
- `8` = `distance_medium`，较大位移

**不许写 3px、5px、6px。** 上浮类一旦超过 2px，在网格里就会读成"跳了一下"。

### 4.4 弹簧：官方两档都看不见回弹，大位移必须用自订档

这是 D 台账里已经踩实的结论，作为动效契约的一部分收在这里：

| 档 | ζ | 过冲 | 220px 行程上 |
|---|---|---|---|
| `spring_control` | **1.00**（`Spring::new` 默认，没被覆写） | 0% | 0px，压根不过冲 |
| `spring_move` | 0.85 | 0.63% | **1.4px——算得出，看不见** |
| `tokens::spring_bounce()` | 0.60 | 9.5% | **21px** ✅ |

⇒ **侧栏/抽屉这类大位移只用 `spring_bounce()`。** 判据：**位移越大越要低阻尼**；小位移反过来必须高阻尼（0.6 阻尼下 4px 过冲会变成"抖了一下"）。`tokens.rs` 有单测把过冲钉在 5%–15%。

### 4.5 拖拽改出来的值，必须换 keyed state id 重新锚定

`spring()` 的内部 position 在 `window.use_keyed_state` 里，外部改不了，且静止在目标上时直接返回不推进。⇒ **凡由拖拽把手改出来的值，要换一份 id 把弹簧就地锚定**（本项目用 `sidebar_gen` / `insp_gen` 代次，只在 `commit_*_width` 里自增）。不换的后果实测是**面板被拽回旧宽度再弹过来，每次拖拽必现**。

⚠️ **侧栏与检视抽屉是同一套写法的两份拷贝，改一处必须回头改另一处**，且那两个代次字段**不能合并**——它们是两个独立的 `ResizableState`，共用 id 会让两个弹簧互相拽。

### 4.6 循环指示只有一个合法用途

呼吸动画（`opacity .55 ↔ 1`，1100ms，ease-in-out）**只许用于"正在等你按键"**（快捷键录制态）。不许用于装饰、不许用于"这个卡片比较重要"。循环动画一多，界面就从"工具"变成"广告位"。

---

## 5. SVG 与图标

### 5.1 唯一来源

```rust
use gpui_kit::assets::IconName as AssetIcon;   // 完整 lucide 1830 个 + 品牌 Bird
```

⛔ **不许用 `component::IconName`**——那是另一套，线宽与视觉重量不同，且没有 `GitBranch`。两套混用一眼就能看出不一致（历史上混过）。

### 5.2 笔画粗细**不是**可调设计变量（实测）

- `gpui-kit-assets-0.7.0`：1830 个 SVG 里 **1825 个硬编码 `stroke-width="2"`**，`fill="none"` 1828 个——纯描线图标集。
- `gpui-pre/src/elements/svg.rs` 全文只有 `path / external_path / data / with_transformation`，渲染走 `window.paint_svg(..., color, ...)`——**单一 Hsla 着色，无 stroke 参数**。
- `gpui-component-0.7.0/src/icon.rs:105-201` 的 `Icon` 全部 builder：`new/path/data/view/transform/rotate/empty` + `Styled` + `Sizable`——**没有任何 stroke 入口**。

⇒ 参考图那种灌满的 duotone 粗图标，**这套基座给不出**。唯一硬绕是 `Icon::data(&[u8])` 传运行时改写过的 SVG 字节（把 `stroke-width="2"` 换成 2.5）——**本规范禁止这条路**：它把图标变成运行期字符串处理，代价与收益完全不成比例。

### 5.3 视觉重量靠**底块面积**（本规范的核心手法）

既然笔画不能加粗，重量就从别处来：**把图标放进一个 `wash` 色块里**。

```
GLYPH = 40×40 方块 · 圆角 R_MD(6)→实际用 12 · 底色 wash · 图标 ICON_LG(20) · 色 brand
```

这一式覆盖：统计卡、导航轨选中态、空态、设置页分类导航。**它在 gpui 里 100% 可得**（`bg(wash)` + `rounded` + 居中一个 `Icon`），而观感上直接对上参考图那种"图标有分量"。

⚠️ **深色下 `wash` 是 `#17383F`、`brand` 是 `#3FB9C4`**——底块与图标都换值，不是把浅色的块调暗。

### 5.4 颜色档

| 场合 | 色 | 说明 |
|---|---|---|
| 正文行内图标 | 继承 `ink` | 默认 |
| **幽灵图标按钮** | **必须显式 `.text_color(tier3(t))`** | 默认继承墨色，观感"太深"（旧文档已记） |
| 选中态 | `brand` | 导航轨、侧栏、分类导航 |
| 未选中/装饰 | `faint` | |
| 告警 | `warn` | 感叹号点 |

### 5.5 位图（PNG）

PNG **不能**喂给 `gpui_kit::component::Icon`——它只会 `svg().data(..)`，喂 PNG 是**空白方块**。位图走 `img()` + 自建 `AssetSource`（`gui/src/app_assets.rs`），不额外引入图像解码依赖（gpui 自带的 `ImageAssetLoader` 就会解 PNG）。

### 5.6 应用图标：一份源图派三份

改图标**只改 `gui/assets/app-icon.png`（必须 128×128）**，`gui/build.rs` 派生其余两份：

| 位置 | 要什么 | 怎么来 |
|---|---|---|
| 标题栏左上角 | PNG | `include_bytes!` 原始 PNG |
| 系统托盘 | 原始 RGBA 字节 | Lanczos3 缩到 32 → `app-tray.rgba` |
| 桌面/任务栏/Alt-Tab | exe 里的 `.ico` | 七档 PNG 条目 → `app.ico` → `embed-resource` |

⚠️ `assets/icon-source.png`（1024 那张）**没有任何代码读它**，是纯存档——照着它重画并存回去，**重新构建什么都不会变**。

### 5.7 禁止文字冒充图标

不许用 emoji、`↑`、`✓`、`●` 之类字符当图标。要图形就用 `Icon`；`Icon` 里没有的，先查 1830 个全集，确实没有再谈自绘。

---

## 6. 同属性模块同规范（登记表）

**这张表是这份文档的执行面。** 左列是"属性"，中列是**唯一出处**，右列是**禁止的做法**。新增界面元素时先在左列找到它属于哪一类。

| 属性 | 唯一出处 | 禁止 | 怎么验 |
|---|---|---|---|
| 卡外壳 | `widgets::card(t)` | 手搓 `bg + border + rounded + p` | 闸：扫 `.rounded(px(R_XL))` 附近必须同现 `card(` |
| 卡内分段标题 | `widgets::mini_head` | 各页自己 `h_flex + text_size` | 闸：`fs_sec` + 700 的组合只允许出现在此构件 |
| 卡内分隔线 | `widgets::card_split(t)` | 自拼 `border_t_1 + 负 margin` | 同上 |
| 两档灰（`line` / `line-soft`） | `tokens::edge(t)` 与 `tokens::track(t)`——**一档一个入口** | 直写 `t.colors.border` / `t.colors.table_row_border` | 闸 **R7**：`colors.border` 只许出现在 `tokens.rs` 定义处。⚠️ **给"档"起名字时别把用途写死**：`edge()` 的注释以前写着"次按钮描边"，于是分隔线、中性状态点以为不归它管、各自直写，同一个值长了两个名字（补十五实测：改前 5 别名 / 6 直写，改后 11 : 0，零视觉变化） |
| 选中面 | `tokens::wash(t)` | 各页自挑一个灰 | 闸：`bg(` 后跟字面色值的，白名单外一律报 |
| 悬停面 | `tokens::hover_face(t)`（**待落地**：gpui 裸 `Div` 没有 `.hover()` 入口，要靠 `on_hover` 记状态 ⇒ 阶段 31.4） | 有的用 `secondary_hover` 有的自己挑灰 | 收敛后闸：全站 `wash_soft` 只出现在此函数 |
| 图标底块 | `widgets::glyph(...)`，一档 40/12/20 | 42 / 46 / 圆角混用 | 闸：只允许一组常量 |
| 徽章 / 胶囊 | `widgets::pill(kind, text)` | 各页自拼 `bg + rounded + text_size` | 闸：`pill_h()` 只在此构件 |
| 空态 | `widgets::empty_state(icon, title, sub)` | 各页自写居中 `div` | 抓帧：每屏空态必须逐像素同构 |
| 表单行 | `widgets::FormRows`（`.row(label, control)` → `.build(t)`） | 标签在上、输入框在下；**两种对齐混在一页** | 控件列 `justify_end` 是**唯一**对齐口径：按内容宽的控件（开关、单选组、按钮）自动贴行末，**要占满的必须自己写 `w_full()`**（不给就塌成 0 宽，见 D92 那轮抓帧）。标签列宽 `tokens::FORM_LABEL_W`，别在调用方再写一个 `w(px(..))` |
| 互斥单选 | 官方 `RadioGroup`（勾选态由一个 `Option<usize>` 算出来） | 三颗 `Checkbox` 当单选；**`ToggleGroup::segmented()`**（只改外观，仍允许零选/多选） | 判据见 D88：让"非法态"没有选项，而不是补一段兜底 |
| 说明气泡 | `widgets::info_dot(id, text, t, gv)`（一颗 ghost 图标键 + `.tooltip()`） | 裸 `Div` 挂 tooltip（**基座没这条路**）；气泡正文写 `**加粗**` / 反引号（纯文本，会连符号画出来） | 判据：**这句话不看会不会让用户做错事**——会就不许进气泡，留在界面上（"即时生效"因此是一枚可见胶囊） |
| 设置页一段 | `dialogs/settings.rs::SetCard`（= `card` + `CardHeader` + 卡身） | 手搓"标题 + 两三行灰字"浮在 dialog 底上 | 抓帧：四张分类页**外框逐像素同尺寸** |
| 告警 | 行内 `warn_dot` + 气泡 | **整块 wash 横贯全宽** | 判据：提示不许压过操作 |
| 状态条 / 结论落点 | `log_of(path)` 或 `app_log` | 写进只活在单页的 `note` | 写之前先问"谁会读它" |
| 卡间距 | 父容器 `gap(GAP_LG)` **一次** | 再叠一层 `padding-top` | 数这个间距被谁写了几次 |
| 圆角 | `theme.radius` / `R_SM` `R_MD` | 裸 `px(8)` | 闸 |
| 间距 | `GAP_*` 五档 | 裸 `.gap(数字)` | 闸（旧规已有） |
| 字号 | `fs_*()` 六函数 | 裸 `text_size(px(数字))` | 闸 |
| 行高 / 胶囊高 | `row_h(text_px)` / `pill_h()` | 写死 28 / 20 | 闸 |
| 时长 / 缓动 | `motion_tokens()` 四档三条 | 写 `150ms` | 闸：扫 `Duration::from_millis(<字面量>)` |
| 阴影 | `SH_1/2/2B/3` | 裸 `BoxShadow{..}` | 闸 |
| tooltip / 键位提示 | 官方 `tooltip_with_action(.., &Action, None)` 或 `Kbd::global_binding_for_action` | **手写「（Ctrl T）」进文案** | 那是会漂的假提示 |
| 进度读数 | `widgets::progress_readout` | 与 `clone.rs` 的 `progress_block` 混用 | 两者是**包含关系不是重复**，名字分开、都保留 |

### 6.1 机器闸（必须做，否则这张表三周后就烂了）

新增 `scripts/check_design_tokens.py`，扫 `gui/src/`，白名单只放 `tokens.rs`、`theme.rs`、`widgets.rs` 的**定义处**，其余位置报：

1. 字面色值 `rgb(0x` / `#RRGGBB`
2. 裸 `.gap(<数字>)`、`.rounded(px(<数字>))`、`.text_size(px(<数字>))`、`.p(<数字>)`
3. 裸 `Duration::from_millis(<数字>)`
4. `.bg(` 后面跟字面色
5. emoji / `↑↓✓●` 出现在 `.child("` 里
6. 同一个 `ElementId` 字符串在一帧里出现两次（这条已有前科，见 D 台账）
   ⚠️ **实现时降级成了"同一个文件里出现两次"**：脚本是纯文本扫，拿不到"哪一帧"。
   这不是妥协而是换了判据——**同文件两次是"同帧两次"的必要条件**，报出来让人确认；
   跨文件的同名 id 反而安全（两条 `inspector-open` 那次就是分在 `repo_head.rs` 与
   `sync_card.rs` 两个文件里，**文本闸抓不到它**，只有"条件出现的入口一帧只挂一次"
   那条人工判据能抓）。⇒ **别把这条闸跑绿当成它不存在**

**新闸必须变异验证**：故意弄坏一处 → 见红 → 精确还原 → grep 查残留。变异要在运行时真生效，多塞几个参数等于没变异。
⚠️ **而且要连"应当放行"的一起验**：落地那次探针里塞了 6 条违规 + 3 条合法（注释里的色值、`.p(0)`、中文文案里的 `→`），确认 6 报 3 不报。
R5 第一版把 `↑ ↓ → ←` 也算"文字冒充图标"，于是"到「设置 → 显示哪些模块」里打开"这种**中文标点**被误报——**误报的闸会被人当噪音关掉，那比没有闸更糟**。

---

## 7. 组件形态契约

| 构件 | 形态 | 关键值 |
|---|---|---|
| **卡** | 白底 + `line_soft` 描边 + `R_XL` + `SH_2` + `p(GAP_XL)` | 内边距 16，**不许各页手搓** |
| **主按钮** | `brand` 实心 + `brand-ink` 字 + `R_PILL` + `SH_1` | ⚠️ **必须 `ButtonVariant::Custom`**：调用方 `.bg()` 只在 idle 层压得过官方状态层，`hover/active` 两层不重放 `instance_style`（`button.rs:701-712`） |
| **次按钮** | 白底 + `line` 描边 + `R_PILL` | 悬停只改描边为 `aqua`，**不改底色** |
| **幽灵键** | 透明底 | 悬停 `wash_soft`；图标必须显式 `tier3` |
| **按钮四态** | idle / hover 上浮 1px / press 回 `brand_press` / disabled 降透明 | ⛔ **终止手段不许用 `.loading()`**（它完全惰性，D64） |
| **胶囊 / 徽章** | `pill_h()` 高 + `R_PILL` + 浅 wash 底 + 深字 | 字 `fs_meta`，**`ok` 色那档最小 12px** |
| **页签** | **下划线式**，收进头卡底部 | 选中 = `brand` 字 + 2px `brand` 下划线，行高 36。⚠️ 不用胶囊组：胶囊自带一层容器底，卡在卡里显重且高 8px |
| **单选（三档）** | `segmented` 胶囊组 | ⛔ **禁止用三颗复选框做单选**（旧设置页就这么干，理由是怕"一颗都不勾"——那是治标，胶囊组从根上不存在这状态） |
| **开关** | `R_PILL` 42×24，开 = `brand` | 官方 `Checkbox`/`Switch` 恒 24px，**不随字号档**，那类行高可以留 |
| **表单行** | 标签左 104px + 控件**右对齐到同一条线** | ⚠️ 一页里两种对齐（开关贴右、胶囊组紧跟标签）就是"乱"的实体 |
| **表格** | 表头 `fs_sec` + 700 + `muted`；行高 `row_h(fs_body)`；hover = `wash_soft` + **左侧 3px `brand` 竖条** | 数字列一律 `tabular-nums`（`TextStyle.font_features` 支持，拿得到） |
| **图标底块** | `GLYPH` 一档：40×40 / r12 / `wash` / 内图标 20 / `brand` | 见 5.3 |
| **气泡** | `ink` 底 + `canvas` 字 + `R_LG` + `SH_3` + 4px 进场位移 | 出现 `duration_normal`+`enter`，消失 `fast`+`exit` |
| **告警** | **行内 `warn_dot`（18px 圆 + `warn` 色）+ 悬停气泡** | ⛔ 整块 wash 横贯全宽 = 一条提示的优先级抬到操作之上 |
| **空态** | `GLYPH` + 主句 + 副句，居中 | 副句用 `line` 色（比 `faint` 更退），**不许用等宽字体容器装中文** |
| **日志输出流** | `stream` 墨底 + `stream-fg` 正文 + `stream-dim` 噪音 | 空态提示语**不放**这个容器（会掉字重） |

---

## 8. 深色档的五条成立条件

新增或改动深色值时逐条对：

1. **反差 ≥ 12/255**（实测当前 20）。
2. **边界靠描边，不靠阴影。** 黑底上加黑阴影等于没加——深色下阴影只做"近身层次"，卡与画布的分界由 `line_soft` 承担。
3. **品牌色必须提亮**（`#0E4F55` → `#3FB9C4`）。沿用浅色主色会"整屏失去主色"而 check 零错、单测全绿。
4. **语义 wash 必须是"深底 + 亮字"**。浅色 wash 铺在深卡上是一块刺眼的白斑。
5. **文字变亮不是变暗**，且每档都要单独定值，不许机械取反。

---

## 9. GPUI 能力边界：做不到的，不许演

| 想要的效果 | 结论 | 依据 | 本规范的替代做法 |
|---|---|---|---|
| **字距 letter-spacing** | ⛔ 没有 | `TextStyle` 15 个字段无此项，三层 grep 零命中 | 层级用"字号档差 + 字重 + 颜色退档"三样叠（3.3） |
| **负字距**（标题收紧） | ⛔ 没有 | 同上 | 放弃。原型里那 `-0.4px` 已删 |
| **`text-transform: uppercase`** | ⛔ 没有 | 同上 | 自己 `to_uppercase()`，只对拉丁文案有意义 |
| **duotone / 粗笔画图标** | ⛔ 没有 | 1825/1830 硬编码 `stroke-width="2"`；`paint_svg` 只传单色 | 图标放进 `wash` 底块（5.3） |
| **官方件阴影形状** | ⛔ 改不了 | `button.rs:643` 只允许 `shadow_xs` 预设 | 卡自己 `.shadow(SH_*)`；按钮接受基座给的 |
| **theme.json 配阴影** | ⛔ 那个键是布尔 | `ThemeConfig.shadow: Option<bool>` 是总开关 | 阴影档落在 `tokens.rs` |
| **theme.json 配 motion** | ⛔ 没有 motion 键 | `ThemeConfig` / `SemanticThemeConfig` 都没有 | 用 `motion_tokens()` 现成档；要改值在代码覆写 Theme |
| **Button 的 hover 用 `.bg()` 覆盖** | ⛔ 盖不掉，且 debug 直接 panic | `div.rs:843` `debug_assert!(hover_style already set)` | 换 `ButtonVariant::Custom`（那是重新设计这颗按钮，不是改色） |
| **`.loading(true)` 的按钮当终止键** | ⛔ 完全惰性 | `button.rs:1679`，有单测锁 | "正在跑"由"这颗键只在 running 时出现"来表达 |
| **官方 `Checkbox` 随字号档** | ⛔ 恒 24px | 基座写死 | 那一类行高不跟档 |
| **`Column::text_right()` 在 DataTable 上** | ⛔ 空操作 | 只有 `table::Table` 那条路读 `align` | 自己排 |
| **`Button::icon()` 传自定义尺寸** | ⛔ 尺寸被丢弃 | `button.rs:756-761` 无条件 `icon.with_size(icon_size)` | 用按钮档位推图标，或自排 child |
| **外部文件拖放用 `PathBuf`** | ⛔ 永不触发 | 外部拖入是 `ExternalPaths` | `on_drop::<ExternalPaths>` + `.paths()` |

> 这张表的用法：**设计稿画之前先查它。** 查不到的能力就不该出现在稿子里——那会让验收的人以为某个方向被验证过。

---

## 10. 令牌 → 代码落点

| 本文档令牌 | 落点 | 备注 |
|---|---|---|
| `canvas` | theme.json `background` | 两套主题各一份 |
| `card` | `secondary.background` | |
| `card-sunken` | `muted.background` | |
| `ink` | `foreground` | |
| `body` / `muted` / `faint` | `tokens.rs` `body()` / `tier3()` / `tier4()` | theme.json 覆盖不了这几级 |
| `line` / `line-soft` | `border` / `table.row.border` | |
| `brand` 一族 | `accent.background` / `primary.*` / `link*` / `chart.1` | **约 20 个键**，一次改齐 |
| `aqua` | `ring` / `drag.border` / `chart.2`? | 待定，别撞 `success` |
| `wash` / `wash-soft` | `sidebar.accent.background` / `secondary.hover.background` | |
| `ok/warn/bad/info` + wash | 对应 `success/warning/danger/info.*` + `tokens.rs` 的 `*_bg()` | wash 那半在 tokens.rs |
| `stream` 三件 | **未落地** | theme.json 无对应键。⚠️ 这一档要不要存在取决于 **Q15**（日志区留不留深色底），没拍板之前不落码 |
| `R_LG` / `R_XL` | theme.json `radius`=14 / `radius.lg`=20 | ✅ 已改成 `tokens::r_lg(t)` / `r_xl(t)` 读主题，常量已删（10 处调用点跟着换） |
| `GAP_XL` | `tokens.rs` = 16 | ✅ 已落，用在 `widgets::card` 的 `.p()` |
| `SH_1/2/2B/3` | `tokens.rs`，类型 `Vec<BoxShadow>`，按 `is_dark()` 分支 | ⚠️ **只落了 `SH_2`**（卡）。其余三档**等有调用点再写**，理由见 3.4 末尾 |
| `GLYPH` | `tokens.rs`：40 / 12 / 20 | ✅ 已落（31.3 `widgets::glyph`，用在总览四张统计卡与设置页两栏盒的图标位） |
| `FORM_LABEL_W` | `tokens.rs` = **112** | ✅ 已落（31.5 `widgets::FormRows` 的标签列）。**只有 `FormRows` 读它**；推导写在常量注释里：最长那条标签「GitHub 令牌」约 95px，折一行整页行高就齐不了 |
| `ok-wash` | `tokens::green_bg(t)`（浅 `#D6F0DD` / 深 `#10281A`） | ✅ 已落（31.5「即时生效」胶囊与「已改 N 项」那类状态字）。四色 wash 到此补齐：`blue_bg` / `orange_bg` / `red_bg` / `green_bg` |
| markdown **行内代码胶囊**（README 正文里的 `code`） | `card-sunken` 底 + `ink` 字，**在调用侧 `TextView::style()` 覆盖**（`readme_tab.rs::readme_text_style`） | ⛔ 基座默认把这一格画成 `theme.accent`（shadcn 语义里那是浅灰），而我们的 `accent` 是 `brand` 本体（18 处、10 文件在读）⇒ **深底埋字**。动 token 影响半个界面，所以覆盖在 README 这一处。实测见路线表 31.4 补二十六⑤ |
| 时长 / 缓动 / 位移 | **直接用 `cx.theme().motion_tokens()`**（`theme/mod.rs:496`，已核实） | ✅ 不落新常量。⚠️ 现在 `gui/src` 里那 3 处 `Duration::from_millis` **是轮询节奏不是时长档**，别当"待清理" |
| `R_SM` / `R_MD` | `tokens.rs` 常量 4 / 6 | ✅ 小控件不跟主题（保持常量） |
| `hover_face()` | **待新增** `tokens.rs` | ⏳ 未落地：gpui 裸 `Div` 没有 `.hover()` 入口，要靠 `on_hover` 记状态 ⇒ 阶段 31.4 |

### 10.1 `theme.json` 里色板没专门给档的那些键——归并表

`theme.json` 现在两套主题各写 **100 个键**（`ThemeConfigColors` 的字段面比这更大， 我们只填了用到的那些），而第 2 节的色板只有 20 多个令牌。**剩下的键不是"随便填"**：每一个都必须归到某个令牌上， 否则改完一轮，画布是冷的、滚动条还是暖灰的，**一屏两种色温**（这正是"普通"的第二根来源）。 归并原则：**能归的就归，归不了的就沿用旧值并在这里写清楚**，**一个都不许现编**。

| `theme.json` 键 | 归到 | 理由 |
|---|---|---|
| `title_bar.background` / `status_bar.background` | **`canvas`** | 原型 `.titlebar` / `.statusbar` 都是画布色，**卡才是白的**。这两条跟着 `background` 同值，让头尾与画布一起把白卡**夹**在中间 |
| `title_bar.border` / `status_bar.border` / `window.border` / `input.border` / `sidebar.border` | `line` | 主描边一档 |
| `list.even.background` / `table.even.background` | **`card`（= 关掉斑马纹）** | 原型里没有 `nth-child(even)`——v4 的行区分靠 **hover 面 + 左侧 3px `brand` 竖条**（第 7 节），斑马纹和它两套机制叠在一起会互相打架。**斑马纹与竖条只能留一个** |
| `list.head.background` / `table.head.background` / `tab_bar.segmented.background` / `skeleton.background` | `card-sunken` | 下沉区一档 |
| `sidebar.background` / `list.background` / `table.background` / `popover.background` / `tab.active.background` | `card` | 侧栏在原型里就是**一张卡**（`.sidebar { background: var(--card); border-radius: var(--r-card) }`） |
| `scrollbar.thumb.background` | `line` | 原型 `::-webkit-scrollbar-thumb { background: var(--line) }` |
| `scrollbar.thumb.hover.background` | **`faint`** | 原型 hover 用的就是 `--faint`。`faint` 那条"不许承载信息"的禁令管的是**文字**；滚动条是装饰/反馈，合规 |
| `selection.background` / `overlay` | `brand` + 现成 alpha | 不新配色相，只挪透明度 |
| `switch.background` / `slider.background` | `line` | 关态轨道 = 一根描边 |
| `switch.thumb.background` / `accent.foreground` / `primary.foreground` | `card`（浅）/ `brand-ink`（深） | — |
| **`success/warning/danger` 的深色前景** | **沿用现值** `#47B356` / `#F08A3C` / `#E06063` | ⚠️ **刻意不换**：这三个深色值本来就是浅色版提亮来的、在白字卡上对比度已够； 而 v4 只重画了**画布/卡/描边/主色**四层。**为了"成套"去重算三个提亮值 = 现编**——先量出不足再改， 别先改再找理由（第 2.4 节那条 `ok` 的 4.4:1 就是"量出来的"， 那种才值得动） |
| `chart.2` | **`aqua`**（浅 `#57C3CB` / 深 `#58CCD5`） | 已定：**不撞** `success`（浅色版 `success` 是 `#1E8A4C` 绿，`aqua` 是青）。第 10 节原来写"待定"，量过色相后落定 |
| `base.cyan` / `base.magenta` | `aqua` / **沿用现值** | 六个 `base.*` 是终端与语法高亮用色，**只映射有对应档的**：blue→brand、green→ok、yellow→warn、red→bad、cyan→aqua； `magenta` 色板里**没有对应档**，沿用并在下一轮真要动图表时再议 |
| `base.blue/green/yellow/red` | `brand` / `ok` / `warn` / `bad` | 与 chart 同值，**不许再走一套独立的** |

⚠️ **改 `theme.json` 的正确顺序**：先在这张表里给要动的键找到归属（**找不到就不动那个键**）， 再动文件。理由与第 12 节第 2 条同形——**"每个新值都能在表里找到档"是这条规范的执行面**， 而色值最容易在"顺手调一个"里漏网。


---

## 11. 与原型的关系（哪些不照搬）

方向稿在 `prototype/redesign-v4.html` 与 `prototype/settings-v4.html`，渲染图 `prototype/_shots/`。**`prototype/` 已进 `.gitignore`，不提交。**

落地时**有意不等**的五处：

1. **卡间距**：原型 10 → 落地 12（`GAP_LG`）。不为 2px 开一档。
2. **字体**：这台机器没装 Noto Sans SC，原型是微软雅黑回退渲染的。落到 GPUI 用 Noto 后字会略细略疏，**观感与截图有差是素材差，不是不一致**。
3. **`GLYPH` 底块**：原型里 42 / 40 / 46 三种混用，落地统一 40。
4. **互斥单选是圆点，不是分段胶囊**（31.5）。原型画的是胶囊组，而基座**没有"外观是胶囊、语义是互斥"的那个件**： `ToggleGroup::segmented()` 只连边框、不改多选语义。⇒ **外观可以漂，能力不能造假**： 这一条的立论（D88）就是"让非法态没有选项"，为了像原型去选一个允许零选的组件， 等于把这条决策作废。
5. **告警那颗是橙色三角图标，不是 wash 圆底**（31.4）。`.tooltip()` 只挂在 `Button` / `Toggle` 上， 裸 `Div` 没有这条路 ⇒ 要"鼠标悬停出气泡"就得让它是按钮，而按钮画不出契约里那个 wash 圆底。 ⇒ 偏离写在 `widgets::warn_dot` 的注释里，**没有偷偷当成做到了**。

---

## 12. 收尾自查（每屏逐条过）

> ⚠️ **分工**：本节只列 **v4 新增的维度**（反差、登记表、动效档、能力边界、深色五条…）。
> 全量的 22+ 条清单在 [`UI-DESIGN.md`](./UI-DESIGN.md) 第 11 节，**每阶段仍要过一遍**。
> 两份不是重复：那一份管"这个项目一贯的规矩"，这一份管"换 v4 之后新长出来的判据"。
> 同一条判据如果两边都有（例如"有没有裸 hex"），**以本节为准**。

1. 明暗两档都抓帧了吗？深色那五条成立条件逐条对了吗？
2. 每个新值都能在第 2/3 节的表里找到档吗？找到的话是不是**先补了文档**？
3. 第 6 节登记表里，有没有哪一类被两处各写一份？
4. 动效是不是只用了四档三条？有没有写裸毫秒数？
5. 稿子里有没有演第 9 节那张"做不到"表中的效果？
6. 空态、未扫过、无数据那一档显示什么？（**"没扫过"不许写成"已同步"**）
7. 条件出现的入口（收起时才露的键、悬浮才现的键）一帧里只挂了一次吗？
8. 机器闸跑了吗？跑绿之后做过一次变异验证吗？
