# ADR-004: 配置文件

- 状态: 已接受 (2026-08-02)
- 关联: [[glossary]]

## 背景

定位为个人自用但代码开源，配置必须简单可读、可直接进版本库。用户不想做交互式配置 UI。

## 决策

**便携式 JSON 配置文件** `config.json`，位于程序目录或用户指定路径，复制目录即迁移。

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

字段：
- `hotkey`: 全局热键，默认 `ctrl+space`
- `elevate`: release 构建是否提权运行（切换管理员程序必需，debug 构建忽略）
- `window_order`: 窗口层排序 `zorder`/`mru`（见 ADR-003）
- `programs[]`: 程序条目数组
  - `key`: 字母代号，手动指定；启动时校验重复与非法字符，冲突则报错退出
  - `name`: 显示名
  - `process`: 进程名（exe 文件名），用于窗口匹配（ADR-003）

**缺失自动生成**：找不到配置文件时，在 exe 目录（或 `%APPDATA%\WinTab`）生成默认配置，开箱即用。

**运行中修改**：设置界面与「+ 号添加程序」会改写配置并立即落盘（`config.json` 保存路径 = 加载路径）。

**校验规则**：`key` 不重复、唯一小写字母；`window_order` 非法值回退 `zorder`。加载失败时输出路径与错误，不静默忽略。

**未实现（见 TODO）**：`wintab capture` 辅助捕获命令、`autostart` 开机自启。

## 后果

- 配置即代码，GitHub 开源友好；不同机器手动同步或通过 dotfiles 管理
- 不支持热重载（改动需重启），接受此限制
- 字母冲突在启动时暴露而非运行时
