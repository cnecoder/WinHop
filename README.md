<div align="center">

<img src="src-tauri/icons/icon.png" alt="WinHop" width="128">

# WinHop

Windows 窗口快速切换器

全局热键呼出全屏选择页，字母键选程序、数字键选窗口；支持单/多字母两种模式、空格在最近两个窗口间来回跳。

Rust + Tauri 2（WebView2）· Windows 10/11 x64

</div>

## 功能特性

- **两层键位模型**：`Ctrl+Space` 呼出 → 字母键选程序（单窗口直切 / 多窗口进窗口层）→ 数字键选窗口，最常用路径两次按键
- **单字母 / 多字母两种模式**（设置页切换）：
  - 单字母：每个程序一个字母代号，一键直达
  - 多字母：连续输入字母按代号/名称实时筛选，`Enter` 确认最佳匹配，突破 26 个字母上限；代号可配成多字母（如 `ch`、`vs`）
- **空格快速跳转**：呼出后直接按空格，切到上一个使用的窗口（两个最近窗口间来回切换）
- **程序层分页**：每页 20 个，超过时 `PageUp/PageDown` 翻页
- **窗口层**：编号 + 缩略图 + 大预览，方向键/悬停联动、自动滚动跟随选中
- **统一编辑**：所有程序（无论是否已配置）都能改名、配代号；已配置可改键；写入 `config.json` 即时生效
- **代号状态一眼区分**：实心亮边 = 已配置且运行，虚线亮边 = 动态检测到的未配置程序，灰色实心 = 已配置但未运行
- **排序模式可选**：固定序号（创建顺序）/ 最近使用优先
- **双屏支持**：条目显示窗口所在屏幕标签；切换后窗口留在原屏幕
- **管理员程序支持**：提权运行，可切换 taskmgr 等 UIPI 保护的窗口
- **皮肤系统**：所有配色走 CSS 变量主题，设置页切换；当前内置黑绿主题，新增主题只需加一个变量块并登记
- **设置页**：`F2` 或点头部设置按钮进入独立页面，改动保存后生效（主题点选即预览，放弃则回退）；显示当前版本与该版本更新记录
- **托盘常驻**：左键唤出、右键菜单退出、单实例保护
- **配置持久化**：config.json 存 `%APPDATA%\WinHop`，升级/重装不丢失；旧版（WinTab 及 exe 旁配置）首次运行自动迁移

## 安装

从 [GitHub Releases](https://github.com/cnecoder/WinHop/releases) 下载：

- **WinHop_x.y.z_x64-setup.exe**（NSIS 安装包，推荐）
- 或 **WinHop_x.y.z_x64_en-US.msi**

要求：Windows 10/11 x64（WebView2 自带）。安装后运行，首次启动弹 UAC 提权确认（切换管理员程序必需）。

## 使用方式

### 呼出与选择

```
Ctrl+Space（默认热键，可配置）
  → 全屏选择页列出所有程序
    → 单字母模式：按字母代号
    → 多字母模式：连续输入字母筛选 → Enter 确认
        单窗口程序 → 直接切换
        多窗口程序 → 进入窗口层
          → 按数字 → 切换对应窗口（支持多位数字）
          → 再按同字母 → 轮询切下一个窗口
          → ↑↓ 移动选中 / 悬停预览 / 点击选择
    → 空格 → 直接切到上一个窗口（最近两个窗口来回切）
    → PageUp/PageDown 翻页（程序超过 20 个时）
    → Esc 逐层回退
  → 再按热键 / 点击选择页外部 → 关闭
```

- 未运行的程序灰色显示在列表末尾，不可选（只做切换，不做启动器）
- 未配置的运行中程序自动补全：单字母模式按进程名分配空闲字母，多字母模式只按名称/代号匹配
- 窗口层每行：序号 + 标题 + 屏幕标签 + 小缩略图；右侧大预览跟随选中/悬停

### 编辑程序

每个程序行右侧有编辑按钮（✎）：点击 → 输入代号（单字母模式为单个字母，下方显示可用字母；多字母模式为多字母代号，可留空只按名称匹配）→ 修改名称 → 保存，立即写入 `config.json`。

### 设置

选择页内按 **F2** 或点头部「设置」进入独立设置页：

- **窗口排序方式**：固定序号（按创建顺序）/ 最近使用优先
- **多字母模式**开关
- **主题**切换（皮肤）
- **退出 WinHop**
- 当前版本号与该版本更新记录

改动在点「保存」后才生效；未保存就返回/按 Esc 会提示保存。

### 托盘

- **左键**点击托盘图标：呼出/关闭选择页
- **右键**：菜单「退出」

## 配置

配置文件 `config.json` 位于 `%APPDATA%\WinHop\`（升级/重装不丢失）。旧版本配置（`%APPDATA%\WinTab`、exe 目录、项目根目录）首次运行自动迁移复制过来，旧文件保留。缺失时自动生成默认配置。

```json
{
  "hotkey": "ctrl+space",
  "elevate": true,
  "window_order": "zorder",
  "multi_letter": false,
  "theme": "black-green",
  "programs": [
    { "key": "c", "multi_key": "ch", "name": "Chrome", "process": "chrome.exe" },
    { "key": "v", "multi_key": "vs", "name": "VS Code", "process": "code.exe" }
  ]
}
```

| 字段 | 说明 |
|---|---|
| `hotkey` | 全局热键，格式 `修饰键+按键`：ctrl/alt/shift/win + space/esc/enter/tab/字母/数字 |
| `elevate` | release 构建是否提权运行（切换管理员程序必需，debug 构建忽略） |
| `window_order` | 窗口层排序：`zorder` 固定序号（创建顺序）/ `mru` 最近使用优先。设置页可改 |
| `multi_letter` | 是否启用多字母模式（设置页可改） |
| `theme` | 主题 id（皮肤，设置页可改）；内置 `black-green`（黑绿）、`black-yellow`（黑黄，亮姜黄 accent），缺省 `black-green` |
| `programs[]` | 程序条目：`key` 单字母代号（单个小写字母，可空）、`multi_key` 多字母代号（全小写字母，可空）、`name` 显示名、`process` exe 文件名（小写） |

`key` 与 `multi_key` 各自唯一、均可为空（只用另一种模式时）。默认配置预置了常用软件的两套代号。

## 构建

依赖：Rust（MSVC 工具链）、Node.js。

```bash
npm install
npm run tauri dev     # 开发模式（debug 构建，跳过提权）
npm run tauri build   # 打包发布版（生成 MSI + NSIS 安装包）
```

> 改图标后若 exe 图标没更新，是 tauri-build 的 Windows 资源缓存：`touch src-tauri/build.rs` 强制 build script 重跑再构建。

## 架构

### 输入架构（为什么不用键盘钩子）

Windows 上 Chromium（Edge/Chrome/Electron/WebView2）前台时用 **raw input** 接收键盘，`WH_KEYBOARD_LL` 钩子完全看不见按键（实测）。因此：

- **热键**：RegisterHotKey（系统级，与前台无关）
- **覆盖层按键**：选择页夺焦后，按键落在自身 webview，由 JS keydown 接收 → 状态机
- **点外部关闭**：鼠标 LL 钩子 + 窗口失焦事件
- **缩略图**：DWM Thumbnail 管道捕获（每窗口独立、遮挡无关），进入窗口层时捕获一次（静态）

### 代码结构

```
src-tauri/src/
  lib.rs      状态机（程序层/窗口层、排序、MRU、单/多字母匹配、分页、设置命令）
  windows.rs  窗口枚举/激活/缩略图捕获/鼠标钩子/提权/单实例
  config.rs   配置加载/校验/保存/默认配置/旧版迁移
src/          覆盖层 UI（HTML/CSS/JS，纯事件驱动渲染）
```

## 已知限制

- **虚拟桌面**：目标窗口在另一个任务视图桌面时无法激活（Windows 限制，日志记录 `激活失败`）
- **taskmgr 前台**：taskmgr 自带键盘钩子吞掉热键，需用托盘图标唤出
- **缩略图静态**：进入窗口层时捕获一次，不实时刷新；少数硬件加速窗口（全屏游戏等）可能捕获异常
- **IME**：中文输入法开着时选择页按键不受影响（选择页无输入框，不参与 IME 组合输入）

## 日志

运行日志写入 `%APPDATA%\WinHop\winhop.log`（release 无控制台，日志落文件便于排查）。

## 文档

- [构建与验证指南](docs/build.md)（本地编译、单测、安装包构建、发布）
- [TODO（未完成需求）](docs/TODO.md)
- [术语表](docs/glossary.md)
- [ADR-001 技术栈](docs/ADR-001-tech-stack.md)
- [ADR-002 两层键位模型](docs/ADR-002-keyboard-model.md)
- [ADR-003 窗口匹配与切换](docs/ADR-003-window-matching.md)
- [ADR-004 配置文件](docs/ADR-004-config.md)
- [ADR-005 热键与覆盖层交互](docs/ADR-005-hotkey-overlay.md)（已废弃：原 LL 钩子方案被 raw input 问题推翻，现方案见上「输入架构」）
- [热键挂起排查记录](docs/debug-hotkey-hang.md)
