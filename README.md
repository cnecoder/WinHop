# WinTab

Windows 程序快速切换器。一个全局热键呼出覆盖层，字母键选程序，数字键选窗口，双屏显示窗口所在屏幕。

技术栈：Rust + Tauri 2（跨平台为目标，Windows 先行）。

## 核心交互

```
Ctrl+Space（默认热键，可配置）
  → 覆盖层列出所有程序（每个程序一个字母键）
    → 按字母：
        单窗口程序 → 直接切换
        多窗口程序 → 显示窗口层（数字编号，Z 序）
          → 按数字 → 切换对应窗口（支持多位数字）
          → 再按同字母 → 轮询切下一个窗口
    → ↑↓ 移动高亮，Enter 确认
    → Esc 逐层回退
  → 再按热键 / 点击覆盖层外部 → 关闭
```

未运行的程序显示灰色不可选（只做切换，不做启动器）。未配置的程序自动补全（按进程名排序分配空闲字母，配置字母优先）。

## 特性

- **两层键位模型**：字母=程序，数字=窗口，最常用路径两次按键
- **窗口层**：Z 序编号、多位数字、连按字母轮询
- **双屏**：条目显示 `[屏N]` 标签（窗口与显示器相交面积判定）；切换后窗口留在原屏幕
- **方向键导航**：↑↓ 移动高亮 + Enter 确认
- **自动补全**：未配置的运行中程序自动分配字母
- **托盘常驻**：托盘图标点击唤出，菜单可退出
- **管理员程序支持**（release 构建）：以提权运行，可切换 taskmgr 等 UIPI 保护的窗口
- **单实例**：重复启动自动退出

## 构建与运行

依赖：Rust（MSVC 工具链）、Node.js、WebView2（Win10 自带）。

```bash
npm install
npm run tauri dev     # 开发模式（debug 构建，跳过提权）
npm run tauri build   # 打包发布版
```

release 版运行 `src-tauri/target/release/wintab.exe`（首次启动弹 UAC 提权确认）。

## 配置

配置文件 `config.json`，查找顺序：exe 目录 → 当前目录 → 项目根目录。复制目录即可迁移。

```json
{
  "hotkey": "ctrl+space",
  "elevate": true,
  "programs": [
    { "key": "c", "name": "Chrome", "process": "chrome.exe" },
    { "key": "v", "name": "VS Code", "process": "Code.exe" },
    { "key": "t", "name": "Terminal", "process": "WindowsTerminal.exe" }
  ]
}
```

| 字段 | 说明 |
|---|---|
| `hotkey` | 全局热键，格式 `修饰键+按键`：ctrl/alt/shift/win + space/esc/enter/tab/字母/数字 |
| `elevate` | release 构建是否提权运行（切换管理员程序必需，debug 构建忽略） |
| `programs[]` | 程序条目：`key` 单字母代号（不重复）、`name` 显示名、`process` exe 文件名 |

## 输入架构（为什么这样设计）

Windows 上 Chromium（Edge/Chrome/Electron/WebView2）前台时用 raw input 接收键盘，**LL 键盘钩子完全看不见按键**。因此：

- **热键**：RegisterHotKey（系统级，与前台无关）
- **覆盖层按键**：覆盖层夺焦后，按键落在自身 webview，由 JS keydown 接收 → 状态机
- **点外部关闭**：鼠标 LL 钩子 + 窗口失焦事件

## 已知限制

- **虚拟桌面**：目标窗口在另一个任务视图桌面时无法激活（Windows 限制，日志会记录 `激活失败`）
- **taskmgr 前台**：taskmgr 自带键盘钩子吞掉热键，需用托盘图标唤出
- **IME**：中文输入法开着时覆盖层按键表现待充分验证（覆盖层无输入框，理论不受 IME 组合输入影响）

## 日志

运行日志写入 `%TEMP%\wintab.log`（release 无控制台，日志落文件便于排查）。

## 文档

- [术语表](docs/glossary.md)
- [ADR-001 技术栈](docs/ADR-001-tech-stack.md)
- [ADR-002 两层键位模型](docs/ADR-002-keyboard-model.md)
- [ADR-003 窗口匹配与切换](docs/ADR-003-window-matching.md)
- [ADR-004 配置文件与捕获命令](docs/ADR-004-config.md)
- [ADR-005 热键与覆盖层交互](docs/ADR-005-hotkey-overlay.md)（已废弃：原 LL 钩子方案被 raw input 问题推翻，现方案见 README「输入架构」）
