# ADR-001: 技术栈选型 — Rust + Tauri（Windows-only）

- 状态: 已接受，2026-08-02 修订为 Windows-only（原跨平台目标取消：macOS/Linux 已有更好的窗口切换方案）
- 关联: [[glossary]]

## 背景

个人自用 + GitHub 开源。早期评估过跨平台（Win/macOS/Linux），后经调研确认 macOS/Linux 平台已有更成熟的窗口切换工具，**本项目只面向 Windows 10/11 x64**。

软件核心难点不在 UI，而在三块底层 Win32 API：全局热键（`RegisterHotKey`）、窗口枚举（`EnumWindows`）、窗口激活（`SetForegroundWindow` + 前台锁绕过）。

## 决策

采用 **Rust + Tauri 2.x**（保留，不因 Windows-only 重写）：

- 前端覆盖层 UI 用 Web 技术（HTML/CSS/JS）——WebView2 是 Windows 原生组件，开发效率高
- Windows API 用 `windows-sys`（直接调用，无跨平台抽象负担）
- 全局热键用 `tauri-plugin-global-shortcut`（RegisterHotKey，系统级）
- 覆盖层按键由选择页 webview 的 JS keydown 接收（见 ADR-005）

## 备选与拒绝理由

- **C#/WPF**：Windows 原生体验好，但现有代码已基于 Tauri 工作正常，重写无收益
- **C++/Win32**：控制力最强，但 UI 开发效率低，覆盖层渲染成本高
- **AutoHotkey**：脚本即配置、热键天然支持，但复杂 UI 与工程化差

## 后果

- Windows-only 后可用 Windows 专属能力：虚拟桌面 COM（`IVirtualDesktopManager`）、UIAccess 免 UAC、注册表自启、DWM 缩略图等（见 TODO）
- 无需维护跨平台抽象；后续新增能力全部走 Win32 直调
