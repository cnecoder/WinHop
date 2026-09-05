# WinHop 视觉设计与 UI 规则

本文档固化前端（`src/*.css/html/js`）的视觉设计系统与组件规则。**改 UI 必须先读本文件，并严格沿用既有令牌与组件样式**；令牌值、组件外观变更时同步更新本文。

所有颜色走 CSS 变量（`src/styles.css` 顶部）。深色全屏覆盖层，主题只换强调色（accent），底色/文字/字体/尺度全部主题无关。

---

## 1. 设计令牌（CSS 变量，定义于 `styles.css` `:root` / `[data-theme]`）

### 主题无关（` :root`，所有主题共享）

| 变量 | 值 | 用途 |
|---|---|---|
| `--bg` | `#0b0c0f` | 覆盖层底色 |
| `--surface` | `#12141a` | 面板/卡片表面（窗口列表、预览、弹窗） |
| `--surface-2` | `#0d0f14` | 输入框底、黑名单行底（比 surface 略深） |
| `--fg` | `#e8ebf1` | 正文文字（亮） |
| `--dim` | `#9aa3b5` | 次要文字、说明、hint、未运行 |
| `--off` | `#5b6475` | 更弱文字（空黑名单提示、置灰代号） |
| `--line-rgb` | `255,255,255` | 中性描边/分隔线用白色透明度 `rgba(var(--line-rgb), α)` |
| `--error` | `#ff6b6b` | 错误态（输入校验失败） |

**圆角阶梯**：`--r-sm:4px`（徽章、缩略图）、`--r-control:6px`（按钮、输入框、代号徽章、设置项）、`--r-row:6px`（列表行）、`--r-panel:8px`（面板、大预览、弹窗）。

**动效**：`--dur:140ms`，缓动 `--ease: cubic-bezier(0.16,1,0.3,1)`；只动 `transform/opacity/background/border-color` 等短时长属性。

**字体**（系统原生栈，不打包 webfont，中西文自动配对）：
- 正文 `--font-sans: "Segoe UI","Microsoft YaHei",system-ui,sans-serif`
- 等宽 `--font-mono: Consolas,"Cascadia Mono","Microsoft YaHei",ui-monospace,monospace`——用于代号徽章、进程名、版本号、数字（配 `font-variant-numeric: tabular-nums`）

### 主题色（accent，每个 `[data-theme]` 块只覆盖这 4 个）

| 变量 | 黑绿 `black-green`（默认） | 黑黄 `black-yellow` | 用途 |
|---|---|---|---|
| `--accent` | `#35d07a` | `#fbbf24` | 强调主色 |
| `--accent-rgb` | `53,208,122` | `251,191,36` | accent 的 RGB，用于 `rgba(var(--accent-rgb), α)` 透明度 |
| `--accent-strong` | `#48e08c` | `#fcd34d` | hover 加深 |
| `--on-accent` | `#04240f` | `#241a02` | 放在 accent 实心填充上的文字色（深底对比） |

**新增主题 = 加一个 `[data-theme="xxx"]` 块**（仅覆盖上述 4 个 accent 变量），并在 Rust 侧 `config::THEMES` 登记 id、前端 `i18n.js` 加 `themeXxx` 中英文名 + `main.js` 的 `themeName` id→key 映射。Rust 只下发主题 id，显示名完全由前端 i18n 负责（不在 Rust 硬编码中文名）。中性底色/文字不要随主题变。

---

## 2. 颜色使用规则

- **accent 只表达「交互/选中/已配置运行」**：主按钮、输入框边框、选中高亮、代号徽章（已配置）、设置分组标题、模式徽章（多字母）。
- **中性灰（`--dim`/`--off` + 白透明描边）表达「次要/不可达/未运行」**：说明文字、未运行程序、未配置代号占位、分隔线。
- **红 `--error` 仅用于错误**：输入校验失败的红框。不再用红色做 hover（历史上「退出/屏蔽」hover 变红，已统一为 accent 实心）。
- 选中/高亮用 **tint 填充 + `inset` 内描边**（`rgba(accent,0.12)` 底 + `inset 0 0 0 1px rgba(accent,0.45)`），不用外发光、不用硬 `outline`。
- 分隔线/弱描边统一 `rgba(var(--line-rgb), 0.08~0.25)`。

---

## 3. 组件规则

### 3.1 按钮（核心规则，全站统一）

**可点击按钮 = accent 实心填充 + `--on-accent` 文字 + 加粗**；hover 用 `--accent-strong`。**唯一的例外是「不可点」= 置灰。**

```css
/* 标准可点按钮（实心） */
background: var(--accent);
border: none;
color: var(--on-accent);
font-weight: 700;
border-radius: var(--r-control);
cursor: pointer;
transition: background var(--dur) var(--ease);
/* hover */ background: var(--accent-strong);
```

**禁用态**（仅保存按钮有）：`background: rgba(var(--line-rgb),0.12); color: var(--dim); cursor: default;`——灰底灰字，明确不可点。

覆盖范围（全部已按此实现，新增按钮必须沿用）：
- 主按钮 `.primary-btn`（设置页保存、弹窗保存）——禁用置灰
- 头部「设置」`#settings-btn`、行内编辑 `✎` `.edit-btn`
- 设置页「返回」`#settings-back`、「录制」`#hotkey-btn`、「退出」`#quit-btn`、黑名单「解除」`.blocked-unblock`
- 编辑面板「保存」`.confirm-btn`、「屏蔽」`.block-btn`
- 弹窗「保存/不保存/取消」`.confirm-buttons button`

> 不要新做描边（outline）风格按钮；不要给不同按钮配不同 hover 色。需要区分主次时用尺寸/位置，不用颜色体系。

### 3.2 输入框

- 底色 `--surface-2`，边框 **accent 半透明** `1px solid rgba(var(--accent-rgb),0.5)`，文字 `--fg`，圆角 `--r-control`。
- 聚焦/激活：边框变实心 `var(--accent)` + `inset 0 0 0 1px rgba(accent,0.3)`；去掉默认 `outline`。
- 错误态 `.err`：边框/内描边变 `--error`（红）。
- 代号输入框 `.key-input` 默认即实心 accent 边框；名称框 `.name-input`、热键显示框 `.hotkey-display`、筛选框 `.filter-box` 默认半透明 accent 边框、聚焦变实心。
- 只读显示框（如筛选框无输入时）也用 accent 半透明边框，保持「这是可交互区」的视觉。

### 3.3 单选 / 复选

- 一律用**原生 radio**（`<input type="radio">`），样式 `accent-color: var(--accent); width/height:16px`，交给系统绘制，不要手绘圆形（手绘无法与原生完全一致）。
- 布尔开关（开机自启）也复用 radio 元素 + JS 处理「已选中再点=取消」，保证外观与单选项 100% 一致；不要用 checkbox 仿 radio。

### 3.4 代号徽章（程序行键位）

- 等宽字体、accent 文字、圆角 `--r-sm`。
- 三种状态 class：
  - `.key-cfg` 已配置且运行：accent 实心描边 + `rgba(accent,0.18)` 底。
  - `.key-off` 已配置但未运行：白透明灰描边/灰底/灰字。
  - `.key-empty` 未配置代号：无边框无底色，显示占位 `·`（`--dim`）。
- **键位槽 `.key-slot` 固定宽 52px、右对齐**：徽章右缘对齐，软件名始终从同一列开始（左对齐）；短代号居槽右，超长代号（>5 字符，如 `settings`）徽章比槽宽时**向左溢出**、不截断（列表容器不设 `overflow:hidden`，徽章可探进 app 左 padding）。

### 3.5 列表行 / 卡片

- 程序行 `.row`：flex、`align-items:center`、圆角 `--r-row`、字号 16px、行高固定 40px、行间距 4px。
- 程序层列表占满视口剩余高度 `calc(100vh - 124px)`；**每页最多 20 个（`PROG_PAGE_SIZE`），卡片固定高、从上到下排列**；不足 20 个时顶部对齐，不居中、不拉伸均分。
- 选中/悬停高亮**只套软件名卡片 `.name`**（不框整行、不框代号）：选中 = accent tint + inset 描边；悬停 = 白透明 `rgba(line,0.07)`。
- 未运行行 `.row.off`：代号/名称/屏数全部降为 `--off` 灰。
- 窗口层：左侧 `.wlist`（420px 宽、surface 底、accent 描边面板）+ 右侧 `.wpreview` 大预览；窗口行 `.wrow` 选中同 accent tint 规则。

### 3.6 分组与文字层级

- 设置分组标题 `.setting-group-title`：13px、加粗、accent、大写、字距 0.5px。
- 页面/设置标题 18px/600；正文列表 15–16px；说明/hint 12–13px、`--dim`。
- 弹窗 `.confirm-dialog`：surface 底、accent 半透明描边、圆角 `--r-panel`、阴影；遮罩 `rgba(bg,0.6)`。

### 3.7 动效

- 进场 `overlay-in`（淡入 + 4px 上移）、弹窗 `pop-in`（淡入 + 0.97 缩放）。
- 所有 transition 用 `--dur`/`--ease`；筛选光标 `.caret` 用 1s steps(2) 闪烁。
- `@media (prefers-reduced-motion: reduce)` 下全部动画/过渡降为 0.01ms、光标停闪——新增动效必须被该媒体查询覆盖。

---

## 4. 布局约定

- 覆盖层全屏 `100vw/100vh`，`#app` 内边距 `24px 36px`；内容列 `max-width:1200px` 居中（窗口层突破到全宽）。
- 设置页 `max-width:720px` 居中，分区 `.setting-section` 间距 22px。
- 头部两端对齐：左标题+模式徽章，右图例+设置按钮+提示；底部 1px 白透明分隔线。

## 5. UI 改动检查清单

- [ ] 颜色只用 CSS 变量，没有硬编码色值（accent/灰/红均走令牌）。
- [ ] 新按钮沿用实心 accent 规则；不可点才置灰；无自造描边/多色按钮。
- [ ] 新输入框 accent 半透明边框、聚焦实心 accent、错误红框。
- [ ] 单选/布尔用原生 radio（`accent-color`），不手绘。
- [ ] 新主题只加 accent 变量块 + `config::THEMES` 登记 id + 前端 i18n 名，不动中性令牌。
- [ ] 动效被 `prefers-reduced-motion` 覆盖。
- [ ] 中文文案走 i18n（`data-i18n` / `t()`），中英双语齐全。
