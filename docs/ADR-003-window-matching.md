# ADR-003: 窗口匹配与切换策略

- 状态: 已接受 (2026-08-02)
- 关联: [[glossary]]

## 背景

需要确定配置条目如何对应运行中的窗口，以及切换的语义。核心权衡：匹配粒度（进程 vs 标题）、多窗口顺序、未运行程序行为、双屏归属与切换行为。

## 决策

**匹配规则：按进程名**。配置条目声明 `process`（如 `chrome.exe`），运行时枚举所有可见窗口，按 `GetWindowThreadProcessId` 取进程名匹配。不做标题正则过滤（保持配置简单）。

**窗口过滤**：仅计入可见窗口（`IsWindowVisible`），排除自身进程窗口、桌面（`Progman`）、任务栏（`Shell_TrayWnd`）及无标题窗口。

**多窗口编号顺序：可配置**（设置界面切换，运行时落盘）：
- `zorder` 固定序号：按窗口句柄序（≈创建顺序），编号不受使用影响
- `mru` 最近使用优先：按最后激活时间倒序，1 = 上次用的；时间戳相同保持句柄序（稳定排序）
- 注：原始 Z 序不能做「固定序号」——任何激活都会把窗口提到 Z 顶，等于隐式 MRU，故弃用

**未运行程序：只切已运行，不启动**。覆盖层显示灰色不可选。

**最小化窗口**：切换时若窗口最小化，先 `ShowWindow(SW_RESTORE)` 还原再激活。

**前台锁定**：Windows 禁止非前台进程直接调用 `SetForegroundWindow`，使用 `AttachThreadInput` 到目标线程后再 `SetForegroundWindow` / `BringWindowToTop` 的兼容技巧。此问题 macOS 侧对应 Accessibility 授权，Linux 侧对应 `_NET_ACTIVE_WINDOW`。

**双屏**：
- 屏幕归属判定：窗口 rect 与各 monitor（`EnumDisplayMonitors`）rect 求相交面积，相交面积最大者为其所在屏，覆盖层显示 `[屏N]` 标签
- 切换行为：仅激活，窗口留在原屏幕，不改变用户布局
- 覆盖层默认全屏（Win+Tab 风格）

## 后果

- 配置只需进程名，简单直接；无法区分同进程的不同 profile（如 Chrome 多 profile 窗口），接受此限制
- 固定序号稳定、最近使用直观，两种模式按需选择
- 不启动未运行程序：切换器不承担启动器职责，行为范围清晰
