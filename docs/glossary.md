# 术语表

| 术语 | 英文 | 定义 |
|---|---|---|
| 覆盖层 | Overlay | 热键呼出的 borderless 顶层浮层 UI，程序/窗口列表展示与选择界面 |
| 程序条目 | Program Entry | 配置中的一行：字母代号 + 显示名 + 进程名 |
| 字母代号 | Key | 程序绑定的字母键（第一层选择键），配置手动指定 |
| 窗口编号 | Window Index | 第二层中窗口的数字编号，Z 序，多位数字无上限 |
| 程序层 | Program Layer | 第一层，字母选程序 |
| 窗口层 | Window Layer | 第二层，数字选窗口 |
| 单窗口直切 | Direct Switch | 目标程序仅 1 个窗口时，按字母直接切换、跳过窗口层 |
| 轮询切换 | Cycling | 窗口层内连按同字母，依次切下一个窗口（编号递增循环） |
| Z 序 | Z-order | 窗口堆叠顺序，`EnumWindows` 返回序；编号 1 = 最上层 |
| 屏幕标签 | Screen Tag | 条目上的 `[屏N]` 标记，表示窗口所在显示器 |
| 前台锁定 | Foreground Lock | Windows 限制非前台进程调用 `SetForegroundWindow`，需 `AttachThreadInput` 技巧绕过 |
| 便携配置 | Portable Config | 配置文件放程序目录，复制目录即迁移 |
| 捕获命令 | Capture Command | `wintab capture`，打印当前激活窗口的进程名/标题供配置使用 |
| 低级键盘钩子 | Low-Level Keyboard Hook | `WH_KEYBOARD_LL`，覆盖层打开时截取按键、绕过 IME 路由 |
| 逐层回退 | Layer Back | Esc 在窗口层回程序层、程序层关闭覆盖层的回退行为 |
| 覆盖层主屏 | Primary Monitor | 覆盖层固定显示的主显示器 |

关联文档: [[ADR-001-tech-stack]] [[ADR-002-keyboard-model]] [[ADR-003-window-matching]] [[ADR-004-config]] [[ADR-005-hotkey-overlay]]
