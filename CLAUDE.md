# WinHop — Claude/Agent 工作指南

Windows 窗口快速切换器（Rust + Tauri 2 / WebView2，Windows-only）。全局热键呼出全屏覆盖层，字母选程序、数字选窗口。

## 必读文档（动手前）

| 文档 | 内容 |
|---|---|
| [docs/build.md](docs/build.md) | 环境、dev 编译、单测、手动验证清单、正式打包 |
| [docs/debug.md](docs/debug.md) | **日志位置、三条输入路径、调试循环、键盘事故复盘**——改键盘/焦点/激活前必读 |
| [docs/release.md](docs/release.md) | 版本号三处同步、双语 release note 规范、发布流程 |
| [docs/TODO.md](docs/TODO.md) | 未完成需求与已知限制（先查这里，避免重复造） |
| [docs/design.md](docs/design.md) | 设计文档（与当前实现一致）：技术栈、架构、两层键位模型、窗口匹配/激活、配置、输入架构 |
| [docs/ui-design.md](docs/ui-design.md) | **视觉设计系统与 UI 规则**：颜色/字体/圆角令牌、按钮/输入框/单选/代号徽章/列表组件规则——改前端样式前必读 |
| [docs/glossary.md](docs/glossary.md) | 术语（覆盖层、前台锁定等） |

## 代码结构

- `src-tauri/src/lib.rs` — 覆盖层状态机（纯函数 `OverlayState::transition` + 薄驱动 `handle_key`/`apply_effect`）、overlay/程序编辑等 Tauri 命令、配置读写编排、builder/托盘装配
- `src-tauri/src/settings.rs` — 设置页：`get_settings`/`save_settings` 命令、设置 DTO、版本 changelog、热键注册与自启副作用
- `src-tauri/src/hotkey_capture.rs` — 全局热键录制（`GetAsyncKeyState` 轮询绕开 IME）：start/poll/stop 命令与检测，自包含不依赖状态机
- `src-tauri/src/windows.rs` — Win32：窗口枚举、激活、鼠标 LL 钩子、DWM 缩略图、语言/提权/单实例/自启注册表
- `src-tauri/src/config.rs` — `config.json` 结构、加载迁移、校验、**原子保存**
- `src/main.js` / `i18n.js` / `index.html` / `styles.css` — 覆盖层与设置页前端
- 配置与日志：`%APPDATA%\WinHop\`（config.json、winhop.log）

## 工作流（必须遵守）

1. **先读后写**：理解意图再改；有多种理解时列出，不擅自选；不确定先问。
2. **简洁优先、精准修改**：只动必须改的，不顺手重构/格式化相邻代码；注意到无关死代码先提出不擅自删。
3. **改完自动验证**：`cd src-tauri && cargo test`（34 项：配置校验/序列化、纯状态机 `transition` 键位模型——数字累积/组合编号/preview/Esc 两级/字母筛选/翻页/空格 MRU、筛选排序/程序列表、代号冲突、热键 vk、枚举冒烟、版本资源）+ 前端 `node --test`（`src/util.js` 纯函数）与 `node --check` 通过 → `cargo build` → 启动 `./target/debug/winhop.exe`（debug 不弹 UAC）→ 查 `winhop.log` 启动正常 → 交给用户实测。**不主动杀用户在跑的实例**（release 提权需提权 taskkill，见 build.md）。CI（`.github/workflows/ci.yml`）在 PR/主分支跑 `cargo test` + 前端 `node --test`/`node --check`，推 `v*` tag 自动打包发布。
4. **用户说「提交」** = `git commit` + `git push -u origin main`；没说不推。
5. **书面内容主要用中文**；代码、命令、技术术语、API 名保持原文不汉化。
6. **文档与代码始终一致**：改动行为/架构/配置/命令/流程时，同一提交内同步更新受影响文档（主要是 `docs/design.md`，以及 build/debug/release/TODO/glossary 和 README）。文档描述以当前代码为准；发现文档与实现不符时先对齐再继续，不允许文档滞后。
7. **改前端样式必须遵守视觉设计系统**（见 [docs/ui-design.md](docs/ui-design.md)，动手前必读）：颜色只用 CSS 变量、不硬编码色值；按钮统一「accent 实心填充 + `--on-accent` 文字，hover `--accent-strong`，不可点才置灰」，不自造描边/多色按钮；输入框 accent 半透明边框、聚焦实心、错误红框；单选/布尔用原生 radio（`accent-color`）不手绘；新主题只加 accent 变量块，在 Rust `config::THEMES` 登记 id、显示名归前端 i18n（Rust 不硬编码中文名）；动效须被 `prefers-reduced-motion` 覆盖。

## 高风险红线（踩过的坑）

- **绝不注入键盘事件来激活窗口**。旧代码用 `keybd_event` 假 Alt 破解前台锁，down/up 落不同线程会让前台程序 Alt 永久卡死、整个键盘错乱。激活一律用 `SPI_SETFOREGROUNDLOCKTIMEOUT=0`（见 `windows.rs::activate` 与 debug.md）。
- **键盘不走 LL 钩子**（Chromium raw input 看不见）。呼出靠 `RegisterHotKey`，覆盖层内按键靠 webview JS keydown；不要重新引入键盘钩子。
- **鼠标 LL 钩子不拦截时必须 `CallNextHookEx`**，否则截断其它程序的钩子链。
- **锁顺序**：状态机持 `overlay` 锁时，调 `close()` 等会再取 `overlay` 锁的路径前必须先 `drop(ov)`，否则死锁。
- **激活目标窗口放独立线程**，且在覆盖层 `emit(visible=false)` 收尾之后启动；顺序反了会阻塞主线程、挂住鼠标钩子与热键。
- **配置写入必须原子**（tmp + rename）；改坏 config 会让 `read_cfg` panic 起不来。

## Release 要点

发版：改三处版本号（tauri.conf.json / Cargo.toml / lib.rs 的 `CURRENT_CHANGELOG`）→ 更新 `CURRENT_CHANGELOG` 的 `notes_zh`/`notes_en`（设置页双语更新记录，也是 GitHub Release note 的来源）→ `npm run tauri build` → 打 tag 推送 → 建 GitHub Release（note 不单独归档进仓库，流程与模板见 release.md）。
