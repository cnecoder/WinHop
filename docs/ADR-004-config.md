# ADR-004: 配置文件与捕获命令

- 状态: 已接受 (2026-08-02)
- 关联: [[glossary]]

## 背景

定位为个人自用但代码开源，配置必须简单可读、可直接进版本库。用户不想做交互式配置 UI。

## 决策

**便携式 JSON 配置文件** `config.json`，位于程序目录或用户指定路径，复制目录即迁移。

```json
{
  "hotkey": "Ctrl+Space",
  "autostart": false,
  "programs": [
    { "key": "c", "name": "Chrome", "process": "chrome.exe" },
    { "key": "v", "name": "VS Code", "process": "Code.exe" }
  ]
}
```

字段：
- `hotkey`: 全局热键，默认 `Ctrl+Space`
- `autostart`: 开机自启，写入注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
- `programs[]`: 程序条目数组
  - `key`: 字母代号，手动指定；启动时校验重复与非法字符，冲突则报错退出
  - `name`: 显示名
  - `process`: 进程名（exe 文件名），用于窗口匹配（ADR-003）

**辅助捕获命令** `wintab capture`：CLI 子命令，打印当前激活窗口的进程名与窗口标题，方便复制进配置，免去手动查 exe 路径。

**校验规则**：`key` 不重复、唯一；`process` 非空。加载失败时输出错误行号，不静默忽略。

## 后果

- 配置即代码，GitHub 开源友好；不同机器手动同步或通过 dotfiles 管理
- 不支持热重载（改动需重启），接受此限制
- 字母冲突在启动时暴露而非运行时
