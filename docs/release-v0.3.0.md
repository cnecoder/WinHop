# WinHop v0.3.0

## 中文

**重点修复：切换窗口后键盘输入混乱**

- 🐛 彻底修复切换窗口后键盘失灵/错乱：原激活逻辑会注入模拟 Alt 键，在某些情况下导致前台程序的 Alt 键永久卡住，之后所有按键都变成 Alt 组合。改为不注入任何按键的前台锁超时方案，根治该问题。
- 🌐 界面中英双语：默认跟随系统语言，设置页可手动切换并持久化。
- 🚫 新增黑名单：可屏蔽不常用程序，不再出现在选择列表（✎ 编辑面板或设置页管理）。
- 🔢 窗口层数字键新增「先聚焦预览」模式：按数字高亮并预览窗口、回车确认切换（设置页可选；默认仍为按数字直接切换）。
- 🖼️ 窗口缩略图改用 DWM 实时合成：被选择页遮挡、窗口最小化时也能清晰预览，且实时更新。
- 🐛 修复空格快速跳转偶尔切错窗口。
- 🐛 修复覆盖层打开时热键 / Alt+Tab 的焦点竞态；鼠标钩子不再干扰其它程序（AutoHotkey、鼠标手势等）。

## English

**Key fix: garbled keyboard input after switching windows**

- 🐛 Fully fixed keyboard input becoming garbled after a switch: the old activation logic injected a synthetic Alt key, which in some cases left Alt stuck in the foreground app so every subsequent key became an Alt combo. Activation now uses a keystroke-free foreground-lock timeout approach, fixing this at the root.
- 🌐 Bilingual UI (Chinese / English): follows the system language by default, manually switchable and persisted in settings.
- 🚫 Blocklist: hide programs you don't need from the picker (via the ✎ edit panel or the settings page).
- 🔢 New window-layer digit mode: press a digit to highlight and preview a window, Enter to switch (configurable in settings; default still switches directly on digit).
- 🖼️ Window thumbnails now use live DWM composition: clear, real-time previews even when covered by the picker or minimized.
- 🐛 Fix Space quick-jump sometimes targeting the wrong window.
- 🐛 Fix focus races with the hotkey / Alt+Tab while the picker is open; the mouse hook no longer interferes with other apps (AutoHotkey, mouse gestures, etc.).
