# 调试指南

日常调试、日志、输入路径与常见故障排查。构建/发布见 [build.md](build.md)、[release.md](release.md)。

## 日志

release 无控制台，stderr 重定向到文件，启动时自动轮转（超 1MB 改名 `winhop.log.1`）：

```
%APPDATA%\WinHop\winhop.log
```

关键事件都有日志：`=== winhop start ===`、配置加载/迁移、`鼠标钩子安装`、`overlay open/close (switched=..)`、每次按键 `key <msg> phase=..`、激活失败 `激活 SetForegroundWindow 失败 .. err=..`。

实时跟踪：

```bash
tail -f "$APPDATA/WinHop/winhop.log"
```

## 三条输入路径（改键盘行为前必读）

键盘**不走**低级钩子（Chromium 前台用 raw input，`WH_KEYBOARD_LL` 看不见按键，详见 [design.md](design.md) 第 7 节）：

1. **呼出/关闭热键**：`RegisterHotKey`（系统级，与前台无关）→ 插件 handler → `handle_key(Hotkey)`。
2. **覆盖层内按键**：覆盖层 `set_focus` 夺焦后，WebView JS `keydown`（`src/main.js`）→ `invoke("key", {k})` → Rust `key()` → `handle_key()`。
3. **鼠标**：`WH_MOUSE_LL` 钩子（`windows.rs::mouse_proc`）只处理「点击覆盖层外部关闭」；不拦截的事件必须 `CallNextHookEx` 透传，否则截断其它程序（AHK、鼠标手势）的钩子链。

状态机全在 `src-tauri/src/lib.rs::handle_key`；窗口枚举/激活在 `src-tauri/src/windows.rs`。

## 改代码后的验证循环

每次修改后（用户要求自动执行）：

```bash
# 1. 编译（杀占用实例，见 build.md）
cd src-tauri && cargo build
# 2. 跑 dev（debug 跳过提权，不弹 UAC）
./target/debug/winhop.exe &
# 3. 查启动日志正常（配置加载 + 钩子安装，无 panic）
tail -5 "$APPDATA/WinHop/winhop.log"
```

dev 版不主动杀，交给用户实测。手动验证清单见 [build.md](build.md)「手动验证清单」。

注入按键做自动化验证（PowerShell + `keybd_event`），例：

```powershell
# Ctrl+Space 呼出；Enter 确认；再查 Alt 全局状态
# 参考会话内 /tmp/keytest.ps1 模式：keybd_event(VK,0,flags,0)，flags 2=KEYUP
```

## 常见故障

| 现象 | 排查方向 |
|---|---|
| 热键唤不出 | 看日志有无 `热键注册失败`（被占用，托盘左键兜底）；确认单实例（`tasklist \| findstr winhop`）；taskmgr 前台需提权 |
| 覆盖层开了但按键没反应 | `open()` 夺焦失败会自动关闭并记 `覆盖层夺焦失败`；非提权 + 管理员前台窗口时发生 |
| **切窗口后整个键盘错乱/像 Alt 卡住** | **历史重大事故**：旧 `activate()` 用 `keybd_event` 注入假 Alt 破解前台锁，down/up 落到不同线程会在前台程序队列留下卡死的 Alt。已改 `SPI_SETFOREGROUNDLOCKTIMEOUT=0`，零按键注入。若复现：查日志是否还有 `keybd_event` 路径；受害程序线程内 Alt 卡死，重启该程序即恢复 |
| 鼠标卡顿/热键派发挂 | 主线程被阻塞（曾因 WebView2 hide+IPC 期间外部抢焦点）。`close()` 内激活必须 `spawn` 独立线程且在 emit 之后，详见 lib.rs `close_impl` 注释 |
| 切到别的虚拟桌面的窗口无效 | 已知限制（`activate_with_retry` 日志 `可能在另一个虚拟桌面`），见 TODO |
| 配置改坏启动崩 | `read_cfg` 解析失败会 panic；日志看 `解析/读取 config.json 失败`。配置写入是原子的（tmp+rename） |

## 排障记录归档

- 「热键录制导致热键一轮失效 + 主线程卡死」事故复盘：旧实现每次按键都走插件注销/注册热键，反复 churn 打乱分发表，且主线程转发路径易卡死，已回退（分支 `hotkey-settings-wip` 留档）。**v0.3.x 已重做**：录制改走 Rust 侧 `GetAsyncKeyState` 轮询线程（绕开 IME 吞键，见 design.md §热键录制），热键仅在打开设置时 `unregister_all` 一次、保存/返回时注册一次，不再随按键 churn。

## 提交/发布

- 用户说「提交」= commit + `git push -u origin main`；没说不推。
- 发版流程与 release note 规范见 [release.md](release.md)。
