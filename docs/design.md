# WinHop 设计文档

本文合并原 ADR-001～005，描述**当前实现**（与代码一致）。待办与已知限制见 [TODO.md](TODO.md)，构建/调试/发布见 [build.md](build.md) / [debug.md](debug.md) / [release.md](release.md)。

## 1. 定位

Windows 窗口**快速切换器**，不是启动器：

- 全局热键呼出全屏覆盖层，字母选程序、数字选窗口，纯键盘两次按键到任意窗口。
- 只切换**运行中**的窗口；未运行程序灰色不可选，不负责启动。
- 仅面向 Windows 10/11 x64（macOS/Linux 已有更成熟方案，不做跨平台）。

## 2. 技术栈

- **Rust + Tauri 2.x**（WebView2 前端）。Windows API 全部用 `windows-sys` 直调，无跨平台抽象。
- 依赖：`tauri`、`tauri-plugin-global-shortcut`（系统热键）、`serde`/`serde_json`（配置）、`windows-sys`。
- 前端：原生 HTML/CSS/JS（无框架），`withGlobalTauri` 下经 `window.__TAURI__` 调 `invoke`/`listen`。
- 曾用 `windows` crate 做 WGC 窗口截图，缩略图改 DWM 方案后已整条移除（无截图/编码代码）。

选型理由：UI 用 Web 技术开发效率高，底层三块难点（全局热键、窗口枚举、窗口激活）直接走 Win32；拒绝 C#/WPF（重写无收益）、C++（UI 效率低）、AutoHotkey（工程化差）。

## 3. 架构总览

```
RegisterHotKey ──toggle──┐
                         ├──► Rust 状态机 handle_key() ──► emit("overlay") ──► 前端 render()
WebView JS keydown ─invoke("key")─┘        (lib.rs)                            (main.js)
鼠标 WH_MOUSE_LL ─ClickOutside─┘
```

- **状态机全在 Rust**（`src-tauri/src/lib.rs`）：`OverlayState`（阶段 `Closed/Programs/Windows`、程序列表、窗口列表、筛选缓冲、选中项、MRU 等），由 `handle_key(HookMsg)` 驱动。前端只渲染 `Render` 结构、回传按键/点击。
- **Win32 层**（`src-tauri/src/windows.rs`）：窗口枚举、激活、鼠标钩子、DWM 缩略图、语言/提权/单实例/日志。
- **配置层**（`src-tauri/src/config.rs`）：`config.json` 加载、迁移、校验、原子保存。
- 状态查询用原子量（`visible`、`prev_fg`、`pending_activate`），共享状态用 `Mutex`；锁顺序固定（cfg 与 overlay 不嵌套长持），调 `close()` 等会重入取锁的路径前先 `drop(ov)`。

## 4. 两层键位模型与交互

### 阶段

- **程序层**（Programs）：列出运行中的程序（+ 已配置未运行的灰色项）。
- **窗口层**（Windows）：进入某多窗口程序后列出其窗口，带序号、标题、屏幕标签、DWM 缩略图、右侧大预览。

### 程序选择：两种模式（设置页切换）

- **单字母模式**（`multi_letter:false`）：每个程序绑一个字母。已配置程序用配置字母；未配置的运行中程序按进程名排序自动补空闲字母，**上限 26（a–z）**，耗尽即止。按字母：单窗口程序直切，多窗口程序进窗口层。
- **多字母模式**（`multi_letter:true`）：连续输入字母进 `letter_buf`，按 `multi_key`（精确>前缀>子串）→ 名称 → 进程名打分实时筛选排序，`Enter` 确认最高分；`Backspace` 删字符；无匹配显示空状态。代号 `multi_key` 可多字母（如 `ch`、`vs`），无 26 上限。未配置程序无代号、只按名称匹配。

### 窗口选择：数字

- 程序层每页 **20** 个，`PageUp/PageDown` 翻页。
- 窗口层每个窗口有数字编号，支持多位：
  - **单字母模式**：数字累积，`n*10 > 总数`（再加一位必超）时立即跳转。
  - **多字母模式 + 窗口 ≤9**：每个数字独立，按到即定。
  - **多字母模式 + 窗口 >9**：组合编号（`1` 后 `2` = 12），`Backspace` 退格，`Enter` 确认。
- 数字行为由 `win_digit_mode` 决定：
  - `jump`（默认）：按数字直接切换。
  - `preview`：按数字先高亮/预览，`Enter` 才切换。
- **轮询切换**：窗口层内重复触发同一程序，切到该程序下一个窗口（序号递增循环），并即时激活。
- 鼠标：点击程序行等同选中；点击窗口行直接跳转；↑↓ 移动选中；悬停联动大预览。

### 其它键

- **空格**（程序层）：快速跳转到**上一个最近使用**的可见窗口（MRU 前两个里的第二个，两窗互切，类 Alt-Tab 瞬切）。
- **Esc**：窗口层 → 程序层；程序层若有多字母筛选缓冲先清空，再按才关闭。
- **热键再按 / 点击覆盖层外部 / 焦点丢失**：关闭（焦点丢失不抢回焦点，见 §7）。
- `F2` 进设置页，`F11` 切换覆盖层全屏。

## 5. 窗口匹配与切换

### 枚举与过滤（`enum_windows`）

`EnumWindows` 遍历，仅计入：`IsWindowVisible` 可见、非自身进程、非桌面（`Progman`）/任务栏（`Shell_TrayWnd`）、有标题。每个窗口取进程名（`QueryFullProcessImageNameW`，小写 exe 名）、完整路径、文件说明（版本资源 `FileDescription`，作为显示名）、所在屏幕。
**黑名单**（`blocked`）命中的进程不计入：系统预置项来自 `system-blocklist.txt`（首次播种一次，`blocked_seeded` 标记后完全交给用户），用户可经 ✎ 编辑面板屏蔽或设置页解除。

### 匹配

按**进程名**匹配配置条目（`process`，统一小写）。不做标题正则。限制：无法区分同进程的不同 profile（如 Chrome 多 profile），接受。

### 窗口排序（窗口层）

- `zorder`（默认）：按窗口句柄 `hwnd` 排序（≈创建顺序，稳定）。注意真正的 Z 序不能当固定序号——任何激活都会把窗口提到 Z 顶，等于隐式 MRU，故不用。
- `mru`：按 MRU 时间戳倒序（1 = 上次用的），时间戳相同回退句柄序。

MRU 在经 WinHop 切换、呼出时记录前台、以及看门狗线程检测前台变化时补录。

### 激活（`activate`，高风险区）

- 最小化窗口先 `ShowWindow(SW_RESTORE)` 还原。
- **前台锁破解用 `SystemParametersInfoW(SPI_SETFOREGROUNDLOCKTIMEOUT, 0)` 临时把锁定超时置 0**，再 `SetForegroundWindow` + `BringWindowToTop`，然后恢复原超时。
- **绝不注入键盘事件**（曾用 `keybd_event` 假 Alt 破解前台锁，down/up 落不同线程会在前台程序队列留下卡死的 Alt，导致整个键盘错乱——已废弃）。
- **不用 `AttachThreadInput`**（会把调用线程输入队列与目标线程共享，目标慢时连带挂死前台/钩子/热键）。
- 激活在**独立线程**执行，且在覆盖层 `emit(visible=false)` 收尾之后启动（顺序反了会在 WebView2 hide+IPC 期间抢焦点，阻塞主线程）。失败 50ms 后重试一次；目标在另一虚拟桌面时无法激活（任务栏闪烁），记日志。

### 双屏

屏幕归属 = 窗口 rect 与各显示器（`EnumDisplayMonitors`）rect 相交面积最大者，条目显示屏幕标签。切换只激活，**窗口留在原屏幕**，不改布局。

### 缩略图（DWM）

窗口层行缩略图与右侧大预览都用 **DWM 缩略图**（`DwmRegisterThumbnail` / `DwmUpdateThumbnailProperties`，Win+Tab 同款）：DWM 直接把目标窗口纹理实时合成到覆盖层区域，零拷贝、抗遮挡、抗最小化（最小化用 `rcNormalPosition` 还原尺寸 + CLIENTONLY 路径）。按 `slot`（`"pane"` 大预览 / `"row:<hwnd>"` 行）注册，换源先注销；回程序层/关闭时 `thumb_clear` 全部注销。

## 6. 配置

### 位置与迁移

`%APPDATA%\WinHop\config.json`（用户目录，安装器不触碰，升级/重装不丢）。日志同目录 `winhop.log`（超 1MB 轮转为 `winhop.log.1`）。
首次运行自动迁移旧位置：`%APPDATA%\WinTab`（改名前整目录迁移）、exe 目录/项目根目录的 `config.json`（复制到 APPDATA，旧文件保留）。APPDATA 不可用时退回 exe 目录。缺失则生成默认配置（常用软件预置代号）。

### 字段

```json
{
  "hotkey": "ctrl+space",
  "elevate": true,
  "autostart": false,
  "window_order": "zorder",
  "multi_letter": false,
  "theme": "black-green",
  "win_digit_mode": "jump",
  "lang": "",
  "programs": [
    { "key": "c", "multi_key": "ch", "name": "Chrome", "process": "chrome.exe" }
  ],
  "blocked": [ { "process": "xxx.exe", "note": "..." } ],
  "blocked_seeded": true
}
```

| 字段 | 说明 |
|---|---|
| `hotkey` | 全局热键（`修饰键+按键`），默认 `ctrl+space`。注册失败不退出（日志 + 托盘兜底）；配置值无效回退默认 |
| `elevate` | release 是否提权运行（切管理员程序必需）；debug 构建忽略，不弹 UAC |
| `autostart` | 开机自启。落地于注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 的 `WinHop` 值（REG_SZ，加引号的 exe 路径），非仅配置项；保存设置时先写注册表（失败整体不保存），启动时以配置为准幂等对齐 |
| `window_order` | 窗口层排序：`zorder` / `mru` |
| `multi_letter` | 多字母模式开关 |
| `theme` | 主题 id：`black-green`（默认）/ `black-yellow`；配色全走 CSS 变量，`<html data-theme>` 切换 |
| `win_digit_mode` | 窗口层数字行为：`jump` 直切 / `preview` 先预览 |
| `lang` | 界面语言：空=跟随系统，`zh-CN` / `en` |
| `programs[]` | `key` 单字母代号（单小写字母，可空）、`multi_key` 多字母代号（全小写，可空）、`name` 显示名、`process` 小写 exe 名；`key`/`multi_key` 各自唯一 |
| `blocked[]` | 黑名单，兼容裸字符串或 `{process,note}`；进程名小写 |
| `blocked_seeded` | 系统黑名单是否已播种（仅一次） |

### 校验与保存

- 加载时归一化（进程名/代号小写）、去重黑名单；`window_order`/`theme`/`win_digit_mode`/`lang` 非法值回退默认；`key` 非单小写字母或重复、`multi_key` 非法或重复 → **panic**（配置错误启动即暴露，不静默）。
- **原子保存**：写 `config.json.tmp` 再 `rename`，防写坏导致下次起不来。
- UI 内修改（✎ 编辑、屏蔽、设置页保存）即时落盘并 `rebuild_and_emit`/`refresh_overlay` 刷新覆盖层；直接改文件需重启。

## 7. 热键与覆盖层输入架构

**键盘不走低级钩子**：Chromium 前台用 raw input 收键盘，`WH_KEYBOARD_LL` 完全看不见按键（早期 LL 键盘钩子方案因此废弃）。三条输入路径：

1. **呼出/关闭**：`RegisterHotKey`（系统级，与前台无关）→ 插件 handler → toggle open/close。
2. **覆盖层内按键**：覆盖层 `set_focus` 夺焦后，WebView JS `keydown` → `invoke("key",{k})` → Rust `key()` → `handle_key()`。前端忽略按住的 repeat 键（字母/数字）。
3. **鼠标**：`WH_MOUSE_LL` 钩子只处理「点击覆盖层外部 → 关闭」（并吞掉该次点击）；**不拦截的事件必须 `CallNextHookEx` 透传**，否则截断其它程序（AHK、鼠标手势）的钩子链。

### 焦点与关闭

- 覆盖层全屏、`transparent`、`alwaysOnTop`、`skipTaskbar`、无装饰。
- `open()` 夺焦后校验前台是否为覆盖层；拿不到键盘焦点（如非提权 + 管理员窗口前台）则直接关闭还原，避免按键落入后台程序。
- `Focused(false)`（Alt+Tab / Win 键离开）→ 关闭但**不还原旧前台**（用户已主动切走，不抢回）；选择窗口后的关闭才激活目标/还原。
- 看门狗线程（2s）：检测「`visible=true` 但窗口不可见」的分叉状态强制关闭；顺带补录 MRU。

### 热键录制（设置页）

- **不走 webview 按键事件**：中文输入法会吞掉 `Ctrl+Space` 的 keydown（IME 用于切中英），前端只能收到 keyup；改由 Rust 后台线程每 30ms 轮询 `GetAsyncKeyState` 物理键状态（IME 不影响）。
- 检测两个方向：① 主键（A-Z / 0-9 / F1-F24 / Space）按下沿且修饰键（Ctrl/Alt/Shift/Win，左右 Ctrl 归并）已按住；② 修饰键按下沿且主键已按住（覆盖先按主键/同时按）。命中即组合成 `ctrl+alt+...+主键` 存入槽位并停线程；前端 100ms 轮询 `hotkey_capture_poll` 取结果。
- 打开设置页时 `hotkey_suspend`（`unregister_all`）临时注销全局热键——否则按下当前热键会被系统当作 `WM_HOTKEY` 吞掉并触发 toggle；放弃修改/返回时 `hotkey_resume` 恢复旧键。
- 录制结果只暂存在表单（`formHotkey`），与其他设置一致：点「保存」才生效。保存时先注册新热键（失败则回滚注册旧键并报错，不写配置），成功后才原子落盘；Esc/Enter 结束录制。

### 提权与单实例

- 管理员程序（taskmgr 等）受 UIPI 保护：非提权进程热键/激活被拒。release 且 `elevate` 时检测未提权则 `ShellExecute("runas")` 自提升重启（debug 跳过）。
- 单实例：命名互斥量 `WinHop_SingleInstance`，第二实例直接退出，避免两套钩子。
- 托盘常驻：左键 toggle 覆盖层（键盘路径全失效时的兜底），右键菜单退出。
