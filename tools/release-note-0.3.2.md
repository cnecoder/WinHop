# WinHop v0.3.2

## 中文

**修复「幽灵窗口」与保存设置的错乱；键位交互纳入自动化测试防线**

- 🐛 修复幽灵窗口：单窗口程序（cc-switch、MobaXterm 等）不再多出打不开的窗口，窗口列表与 Alt+Tab 一致（跳过 DWM 隐藏的后台/挂起窗口与工具窗口）
- 🐛 保存设置更可靠：保存失败（如开机自启被系统拒绝）时，已注册的新热键会一并还原，不会出现「设置页还开着、新热键已生效」的错乱
- 🛡️ 回归防线：键位交互（数字跳转/组合编号/筛选/翻页/空格互切）抽成纯函数并纳入自动化单元测试（Rust 34 项 + 前端 10 项），每次推送 CI 自动运行

## English

**Fixed \"ghost windows\" and a half-applied-hotkey glitch; key interactions are now guarded by automated tests**

- 🐛 Fixed ghost windows: single-window apps (cc-switch, MobaXterm, etc.) no longer show extra unopenable windows — the window list now matches Alt+Tab (DWM-cloaked background/suspended windows and tool windows are skipped)
- 🐛 More reliable settings save: on failure (e.g. autostart rejected by the system) the newly registered hotkey is rolled back too — no more \"settings page open but the new hotkey already active\"
- 🛡️ Regression guard: key interactions (digit jump / combo index / filter / paging / Space toggle) are now pure functions covered by automated unit tests (34 Rust + 10 frontend), run by CI on every push
