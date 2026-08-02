# ADR-001: 技术栈选型 — Rust + Tauri

- 状态: 已接受 (2026-08-02)
- 关联: [[glossary]]

## 背景

个人自用 + GitHub 开源，跨平台（Win/macOS/Linux）是确认的扩展方向。软件核心难点不在 UI，而在三块底层 API：

| 能力 | Win32 | macOS | Linux |
|---|---|---|---|
| 全局热键 | `RegisterHotKey` | Carbon `RegisterEventHotKey` | X11 grabs / Wayland 受限 |
| 窗口枚举 | `EnumWindows` | Accessibility API | EWMH |
| 激活窗口 | `SetForegroundWindow` | Accessibility 授权后 activate | `_NET_ACTIVE_WINDOW` |

跨平台成本几乎全在这三块，UI 是次要矛盾。

## 决策

采用 **Rust + Tauri 2.x**：

- 前端覆盖层 UI 用 Web 技术（HTML/CSS/JS），一套 UI 三平台复用
- 三块底层 API 在 Rust 侧抽象成 `WindowManager` trait，每平台一个 adapter（Windows 先行）
- 全局热键用 `tauri-plugin-global-shortcut`（RegisterHotKey，系统级）
- Windows API 用 `windows-sys`
- 覆盖层按键由选择页 webview 的 JS keydown 接收（见 ADR-005）

## 备选与拒绝理由

- **C#/WPF**：Windows 体验最佳、出活最快，但跨平台需 Avalonia 半路重写 UI 层，等于双份维护。用户确认跨平台是确定方向后否决。
- **C++/Qt**：跨平台成熟，但三平台开发成本最高。
- **AutoHotkey**：Windows only，不满足跨平台。

## 后果

- 承担 Rust 生态学习成本；初期开发速度慢于 WPF
- UI 层获得免费跨平台；`WindowManager` adapter 每平台约 1 个文件
- 后续可给 `WindowManager` 补 macOS/Linux 实现而不动 UI 与交互逻辑
