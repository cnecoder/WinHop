<div align="center">

![WinTab](src-tauri/icons/128x128.png)

# WinTab

Windows 程序快速切换器

全局热键呼出全屏选择页，字母键选程序、数字键选窗口，双屏显示窗口所在屏幕。

Rust + Tauri 2（WebView2）· Windows 10/11 x64

</div>

## 功能特性

- **两层键位模型**：`Ctrl+Space` 呼出 → 字母键选程序（单窗口直切 / 多窗口进窗口层）→ 数字键选窗口，最常用路径两次按键
- **窗口层**：编号 + 缩略图 + 大预览（占屏宽 3/4），方向键/悬停联动、自动滚动跟随选中
- **排序模式可选**：固定序号（创建顺序）/ 最近使用优先，设置界面即时切换并落盘
- **双屏支持**：条目显示窗口所在屏幕标签；切换后窗口留在原屏幕
- **自动补全**：未配置的运行中程序自动列出（显示软件名称），一键 + 号添加入配置
- **管理员程序支持**：提权运行，可切换 taskmgr 等 UIPI 保护的窗口
- **托盘常驻**：左键唤出、右键菜单退出、单实例保护
- **配置便携**：config.json 放 exe 旁，复制即迁移；缺失时自动生成默认配置

## 安装

从 [GitHub Releases](https://github.com/cnecoder/WinTab/releases) 下载：

- **WinTab_0.1.0_x64-setup.exe**（NSIS 安装包，推荐）
- 或 **WinTab_0.1.0_x64_en-US.msi**

要求：Windows 10/11 x64（WebView2 自带）。安装后运行，首次启动弹 UAC 提权确认（切换管理员程序必需）。

## 使用方式

### 呼出与选择

```
Ctrl+Space（默认热键，可配置）
  → 全屏选择页列出所有程序（每个程序一个字母键）
    → 按字母：
        单窗口程序 → 直接切换
        多窗口程序 → 进入窗口层
          → 按数字 → 切换对应窗口（支持多位数字）
          → 再按同字母 → 轮询切下一个窗口
          → ↑↓ 移动选中 / 悬停预览 / 点击选择
    → Esc 逐层回退
  → 再按热键 / 点击选择页外部 → 关闭
```

- 未运行的程序灰色显示在列表末尾，不可选（只做切换，不做启动器）
- 未配置的程序自动补全：按进程名排序分配空闲字母，显示「软件名称 (exe文件名)」
- 窗口层每行：序号 + 标题 + 屏幕标签 + 小缩略图；右侧大预览跟随选中/悬停

### 添加程序到配置

自动补全的程序行右侧有 **+** 号：点击 → 输入字母（下方显示可用字母，可点击填入）→ 修改名称 → 点「确认」→ 立即写入 `config.json` 并生效。

### 设置

选择页内按 **F2** 或点头部「设置」：

- **窗口排序方式**：固定序号（按创建顺序，不受使用影响）/ 最近使用优先（上次用的排 1）
- **退出 WinTab**

### 退出方式

选择页只在以下情况关闭：再按热键、选中窗口/程序、Esc、点击选择页外部、失焦（Alt+Tab 切走）。无定时自动退出。

### 托盘

- **左键**点击托盘图标：呼出/关闭选择页
- **右键**：菜单「退出」

## 配置

配置文件 `config.json`，查找顺序：exe 目录 → 当前目录 → 项目根目录。缺失时在 exe 目录（或 `%APPDATA%\WinTab`）自动生成默认配置。

```json
{
  "hotkey": "ctrl+space",
  "elevate": true,
  "window_order": "zorder",
  "programs": [
    { "key": "c", "name": "Chrome", "process": "chrome.exe" },
    { "key": "v", "name": "VS Code", "process": "Code.exe" }
  ]
}
```

| 字段 | 说明 |
|---|---|
| `hotkey` | 全局热键，格式 `修饰键+按键`：ctrl/alt/shift/win + space/esc/enter/tab/字母/数字 |
| `elevate` | release 构建是否提权运行（切换管理员程序必需，debug 构建忽略） |
| `window_order` | 窗口层排序：`zorder` 固定序号（创建顺序）/ `mru` 最近使用优先。可在设置界面改，运行时落盘 |
| `programs[]` | 程序条目：`key` 单字母代号（不重复）、`name` 显示名、`process` exe 文件名 |

## 构建

依赖：Rust（MSVC 工具链）、Node.js。

```bash
npm install
npm run tauri dev     # 开发模式（debug 构建，跳过提权）
npm run tauri build   # 打包发布版（生成 MSI + NSIS 安装包）
```

## 架构

### 输入架构（为什么不用键盘钩子）

Windows 上 Chromium（Edge/Chrome/Electron/WebView2）前台时用 **raw input** 接收键盘，`WH_KEYBOARD_LL` 钩子完全看不见按键（实测）。因此：

- **热键**：RegisterHotKey（系统级，与前台无关）
- **覆盖层按键**：选择页夺焦后，按键落在自身 webview，由 JS keydown 接收 → 状态机
- **点外部关闭**：鼠标 LL 钩子 + 窗口失焦事件
- **缩略图**：PrintWindow 直捕（每窗口独立、遮挡无关）→ 24bpp BMP → base64 data URL；进入窗口层时捕获一次（静态）

### 代码结构

```
src-tauri/src/
  lib.rs      状态机（程序层/窗口层、排序、MRU、设置命令）
  windows.rs  窗口枚举/激活/缩略图捕获/鼠标钩子/提权
  config.rs   配置加载/校验/保存/默认配置生成
src/          覆盖层 UI（HTML/CSS/JS，纯事件驱动渲染）
```

## 已知限制

- **虚拟桌面**：目标窗口在另一个任务视图桌面时无法激活（Windows 限制，日志记录 `激活失败`）
- **taskmgr 前台**：taskmgr 自带键盘钩子吞掉热键，需用托盘图标唤出
- **缩略图静态**：进入窗口层时捕获一次，不实时刷新；少数硬件加速窗口（全屏游戏等）PrintWindow 可能得到黑图
- **IME**：中文输入法开着时选择页按键不受影响（选择页无输入框，不参与 IME 组合输入）

## 日志

运行日志写入 `%TEMP%\wintab.log`（release 无控制台，日志落文件便于排查）。

## 文档

- [TODO（未完成需求）](docs/TODO.md)
- [术语表](docs/glossary.md)
- [ADR-001 技术栈](docs/ADR-001-tech-stack.md)
- [ADR-002 两层键位模型](docs/ADR-002-keyboard-model.md)
- [ADR-003 窗口匹配与切换](docs/ADR-003-window-matching.md)
- [ADR-004 配置文件](docs/ADR-004-config.md)
- [ADR-005 热键与覆盖层交互](docs/ADR-005-hotkey-overlay.md)（已废弃：原 LL 钩子方案被 raw input 问题推翻，现方案见上「输入架构」）
