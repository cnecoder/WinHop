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

- **状态机全在 Rust**（`src-tauri/src/lib.rs`）：`OverlayState`（阶段 `Closed/Programs/Windows`、程序列表、窗口列表、筛选缓冲、选中项、待激活 `pending`、MRU 等）。核心是**纯函数** `OverlayState::transition(msg, &cfg, &mut mru, now, overlay_hwnd, is_visible) -> Effect`——只改自身状态、返回 `Effect{None,Emit,Close,ActivateWindow}`，不碰 Tauri/锁，可直接单测。薄驱动 `handle_key`/`pick_program` 拿锁、clone 一份 cfg 快照调 transition，再由 `apply_effect` 落地（emit/close/轮询激活）。前端只渲染 `Render` 结构、回传按键/点击。
- **Win32 层**（`src-tauri/src/windows.rs`）：窗口枚举、激活、鼠标钩子、DWM 缩略图、语言/提权/单实例/自启注册表/日志/`open_url`（默认浏览器打开链接，限 `https://`；设置页关于区 GitHub 仓库引导）。
- **配置层**（`src-tauri/src/config.rs`）：`config.json` 加载、迁移、校验、原子保存。
- **设置页**（`src-tauri/src/settings.rs`）：`get_settings`/`save_settings` 命令、设置 DTO、changelog、热键注册与自启副作用；**热键录制**（`src-tauri/src/hotkey_capture.rs`）：`GetAsyncKeyState` 轮询检测组合键（绕开 IME 吞事件），自包含不依赖状态机。
- 状态查询用原子量（`visible`、`prev_fg`），共享状态用 `Mutex`；transition 不持锁（驱动层一次性 clone cfg 快照、在锁内取 `&mut mru`），调 `close()` 等会重入取 overlay 锁的路径前先 `drop(ov)`。

## 4. 两层键位模型与交互

### 阶段

- **程序层**（Programs）：列出运行中的程序（+ 已配置未运行的灰色项）。
- **窗口层**（Windows）：进入某多窗口程序后列出其窗口，带序号、标题、屏幕标签、DWM 缩略图、右侧大预览。

### 程序选择：两种模式（设置页切换）

- **单字母模式**（`multi_letter:false`）：每个程序绑一个字母。**字母只由用户显式配置（✎ 面板），不自动分配**；未配置的运行中程序仍列出但键位留空（显示 `·`，鼠标可点选、键盘字母够不到）。按字母：单窗口程序直切，多窗口程序进窗口层。✎ 面板把字母删空保存 = 清除字母绑定。两种整条移除：「删除」（`delete_program`）只移除配置条目，未运行即不再灰色显示、运行中仍按未配置（·）出现可重配；「屏蔽」（`block_program`）移除配置并加入黑名单，运行时也彻底隐藏（设置页可解除）。
- **多字母模式**（`multi_letter:true`）：连续输入字母进 `letter_buf`，按 `multi_key`（精确>前缀>子串）→ 名称 → 进程名打分实时筛选排序，`Enter` 确认最高分；`Backspace` 删字符；无匹配显示空状态。代号 `multi_key` 可多字母（如 `ch`、`vs`），无 26 上限。未配置程序无代号、只按名称匹配。

### 窗口选择：数字

- 程序层每页 **N 个**（偏好配置 `prog_page_size`，范围 8–64，默认 20；`PageUp/PageDown` 翻页）。N 是用户偏好，**生效值由前端按屏幕高度反推钳制**（见 §5「覆盖层缩放」），保证卡片铺满屏幕且不过大/过小。
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
- `F2` 进设置页，`F11` 切换覆盖层全屏，`F1`（或头部「?」）打开内置帮助页。帮助页是覆盖层内的**纯前端视图**（不经过 Rust 状态机，状态机仍停在程序层），Esc/F1/返回关闭。

## 5. 窗口匹配与切换

### 枚举与过滤（`enum_windows`）

`EnumWindows` 遍历，仅计入：`IsWindowVisible` 可见、**非 DWM cloaked**（`DwmGetWindowAttribute(DWMWA_CLOAKED)`——挂起 UWP/后台、其它虚拟桌面的幽灵窗口，可见标记为真但激活不了）、**非 `WS_EX_TOOLWINDOW`**（不进 Alt-Tab 的工具/弹窗，与系统切换器口径一致）、非自身进程、非桌面（`Progman`）/任务栏（`Shell_TrayWnd`）、有标题。每个窗口取进程名（`QueryFullProcessImageNameW`，小写 exe 名）、完整路径、文件说明（版本资源 `FileDescription`，作为显示名）、所在屏幕。
**黑名单**（`blocked`）命中的进程不计入：系统预置项来自 `system-blocklist.txt`（首次播种一次，`blocked_seeded` 标记后完全交给用户），用户可经 ✎ 编辑面板屏蔽或设置页解除。

#### 浏览器 PWA（网站安装成应用）独立分组

Chrome/Edge 把「安装成应用」的网站（PWA）窗口与普通标签页窗口放在**同一进程**（`chrome.exe`/`msedge.exe`）——同一进程还可能同时承载普通浏览窗口、甚至多个不同 PWA，所以**进程级标记不足以逐窗口区分，必须逐窗口读 AUMID**。PWA 有两种安装形态，检测统一在窗口 AUMID 上分类（`classify_pwa`），取名因安装模型不同而分流：

1. **crx 形态**（Chrome、旧版 Edge，以及 Brave/Vivaldi/Opera/Arc 等全系 Chromium）：窗口 AppUserModelID（`SHGetPropertyStoreForWindow` → `IPropertyStore::GetValue(PKEY_AppUserModel_ID)`，fmtid=`9F4C2855-…`、pid=5）形如 `Chrome._crx_<32 位 id>`/`MSEdge._crx_<id>`，普通窗口是 `Chrome`/`MSEdge`。跨进程读该 AUMID 实测可能得到**缺固定若干字符的截断串**，故只取 `._crx_` 标记与后缀；完整 32 位 id 改取**承载窗口的进程命令行** `--app-id=<32 位>`——经 `NtQueryInformationProcess(ProcessBasicInformation)` 取 PEB → `ProcessParameters->CommandLine`，`ReadProcessMemory` 读出（按 pid 缓存，每进程读一次）；命令行缺失再回退 AUMID 后缀（可能截断，但仍能独立成组）。
2. **AppX/MSIX 托管形态**（新版 Edge「安装为应用」）：站点被装成 hosted AppX 包（manifest 声明 `uap10:HostRuntimeDependency=Microsoft.MicrosoftEdge.Stable`、`HostId=PWA`），窗口 AUMID = `<PackageFamilyName>!App`（如 `www.volcengine.com-2DE81424_m5663p5smhvk4!App`），普通窗口仍是 `MSEdge`。此形态跨进程读 AUMID **完整不截断**，直接以 PFN 为键；但宿主 msedge 进程命令行**裸空**（无 `--app-id`），PEB 路径无效。仅对 `msedge.exe` 认 `!App` 后缀（排除宿主自身 `…!MSEDGE`），避免误伤。

实现要点：

- 枚举固定跑在**新建的 STA 线程**（`CoInitializeEx(COINIT_APARTMENTTHREADED)`）：调用方线程（Tauri 运行时线程可能已是 MTA）套间不可控，MTA 下跨进程 AUMID 读回空。
- **不得释放** AUMID 返回的 `pwszVal`（不 `CoTaskMemFree`/`PropVariantClear`）——它指向目标进程属性存储内部缓冲；Release store 前直接拷贝。

两种形态的窗口分组键都改写为虚拟进程 `pwa#<key>`（而非 exe 名），每个 PWA 独立成一个程序，同进程的普通窗口仍归 `chrome.exe`/`msedge.exe`；key 为 32 位小写即 crx，否则是 AppX 的 PFN。显示名分流解析：

- **crx**：浏览器 profile 的 `User Data\<Profile>\Web Applications\_crx_<id>\<名>.lnk` 主文件名（首次枚举扫描 Chrome/Edge 全部 profile，缓存）。
- **AppX**：由 PFN 查注册表 `HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Families\<PFN>` 的子键得 PackageFullName，再非提权直读 `C:\Program Files\WindowsApps\<PackageFullName>\AppxManifest.xml` 的首个 `<Properties><DisplayName>`（内联字面量，做最小 XML 实体解码；遇 `ms-resource:` 占位则放弃），按 PFN 缓存。

取名都失败时回退该 PWA 窗口标题（PWA 标题即应用名），再退 `PWA <key>`。前端程序行副标题对 `pwa#` 键只显示 `PWA`，不泄露内部 id/PFN。

**覆盖面**：Chrome、新 Edge(AppX)、旧 Edge(crx) 全覆盖并自动取名；Brave/Vivaldi/Opera/Arc 等 Chromium 浏览器 crx 检测/分组/激活同源可用，但取名只扫 Chrome/Edge 的 User Data，其余回退窗口标题。Firefox 无窗口化 PWA 支持（SSB 已移除），普通 firefox.exe 窗口无独立 AUMID，无法区分，不在覆盖内。

### 匹配

按**分组键**匹配配置条目（`process`，统一小写）：普通程序是 exe 名，浏览器 PWA 是 `pwa#<key>`（crx 为 32 位 app-id，AppX 为 PackageFamilyName，见上）。不做标题正则。限制：无法区分同进程的不同 profile（如 Chrome 多 profile），接受。

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

### 覆盖层缩放（前端自适应）

- 目标：任何分辨率/缩放下，程序层卡片**铺满行区**且不过大/过小；纯自动无手动档位。
- 设置项 `prog_page_size`（步进器 8–64，默认 20）只是**偏好**；真正生效的 N 由屏幕反推：
  - 行区可用高度 avail = `#list.clientHeight` − 工具条高（窗口层复用程序层缓存值，两层同 scale 不跳动）− `#list` padding-top 10px。
  - 可行区间 `pageSizeBounds(avail)`：`n_min=ceil(avail/(44·1.35))`、`n_max=floor(avail/(44·0.75))`（44 = 行高 40 + 一个 gap 4；flex 列工具条带前导 gap，n 行恰有 n 个 gap）。1080p≈[16,27]，1440p≈[22,38]，720p≈[10,16]。
  - `clampPageSize(偏好, avail)` 把偏好钳进屏幕可行区间（再交绝对 sanity 区间 8–64）；区间内任意 N 都使 `cardScale(avail,n)=avail/(44n)` 落在 0.75–1.35 且总高恰好铺满。
  - **设置页步进器上下限就是该屏幕区间**（`pageSizeSettingBounds`，−/+ 到边界置灰；设置页打开时覆盖层隐藏测不到 DOM，avail 按 `innerHeight-124-工具条(实测缓存/兜底42)-10` 推算）；打开时已存偏好超界即钳到边界并同步快照。「重置」按钮 = `optimalPageSize` = `round(avail/44)` 再钳区间，即 scale≈1 的本屏推荐行数。
- 每次 `render` 后前端 `applyUiScale()`：钳出生效 N，写 `--ui-scale`；**生效 N 与后端下发值不同则 invoke `set_page_size(n)`**——后端存入 `OverlayState.eff_page_size`（仅运行时、不持久化，呼出时为 0 回退配置），分页/sync_page/emit 全部改用 `effective_page_size`，并立即重发一帧渲染，前端第二次 render 即收敛，不产生循环。尺寸令牌在 `#overlay-view` 上全部 `calc(基准 * var(--ui-scale))`；设置页/帮助页在其外不受影响。
- 写变量后下一帧（rAF）再测 `getBoundingClientRect() × devicePixelRatio` 并 `thumb_set`，故 DWM 缩略图坐标始终跟随缩放，无需改公式。
- 重算触发：每次 render、`window resize`（rAF 合并）、`ResizeObserver(#list)`、DPR 变化（`matchMedia((resolution:Ndppx))` 一次性监听重建，覆盖主屏缩放调整）；语言切换经 render 也顺带修复了缩略图陈旧。

## 6. 配置

### 位置与迁移

`%APPDATA%\WinHop\config.json`（用户目录，安装器不触碰，升级/重装不丢）。日志同目录 `winhop.log`（超 1MB 轮转为 `winhop.log.1`）。
首次运行自动迁移旧位置：`%APPDATA%\WinTab`（改名前整目录迁移）、exe 目录/项目根目录的 `config.json`（复制到 APPDATA，旧文件保留）。APPDATA 不可用时退回 exe 目录。缺失则生成默认配置（预置常用软件条目：仅名称/进程用于识别与友好命名，代号留空，字母一律由用户显式配置）。

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
  "prog_page_size": 20,
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
| `autostart` | 开机自启。落地于注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 的 `WinHop` 值（REG_SZ，加引号的 exe 路径），非仅配置项；保存设置时先写注册表（失败整体不保存），启动时以配置为准幂等对齐。**debug 构建（console 子系统）跳过注册表写入**，避免 dev 验证污染自启路径（见 debug.md） |
| `window_order` | 窗口层排序：`zorder` / `mru` |
| `multi_letter` | 多字母模式开关 |
| `theme` | 主题 id：`black-green`（默认）/ `black-yellow`；配色全走 CSS 变量，`<html data-theme>` 切换 |
| `win_digit_mode` | 窗口层数字行为：`jump` 直切 / `preview` 先预览 |
| `prog_page_size` | 每页卡片数**偏好**，8–64，默认 20；实际生效值由前端按屏幕钳制后经 `set_page_size` 下发（仅运行时，不持久化），Rust 分页与前端缩放共用生效值 |
| `lang` | 界面语言：空=跟随系统，`zh-CN` / `en` |
| `programs[]` | `key` 单字母代号（单小写字母，可空）、`multi_key` 多字母代号（全小写，可空）、`name` 显示名、`process` 小写 exe 名；`key`/`multi_key` 各自唯一 |
| `blocked[]` | 黑名单，兼容裸字符串或 `{process,note}`；进程名小写 |
| `blocked_seeded` | 系统黑名单是否已播种（仅一次） |

### 校验与保存

- 加载时归一化（进程名/代号小写）、去重黑名单；`window_order`/`theme`/`win_digit_mode`/`lang` 非法值回退默认；`prog_page_size` 越界（非 8–64）记 eprintln 并钳制到边界，不 panic（该值只是偏好，屏幕可行性运行时再钳）；`key` 非单小写字母或重复、`multi_key` 非法或重复 → **panic**（配置错误启动即暴露，不静默）。
- **原子保存**：写 `config.json.tmp` 再 `rename`，防写坏导致下次起不来。
- UI 内修改（✎ 编辑/删除、屏蔽、设置页保存）即时落盘并 `rebuild_and_emit`/`refresh_overlay` 刷新覆盖层；直接改文件需重启。

## 7. 热键与覆盖层输入架构

**键盘不走低级钩子**：Chromium 前台用 raw input 收键盘，`WH_KEYBOARD_LL` 完全看不见按键（早期 LL 键盘钩子方案因此废弃）。三条输入路径：

1. **呼出/关闭**：`RegisterHotKey`（系统级，与前台无关）→ 插件 handler → toggle open/close。
2. **覆盖层内按键**：覆盖层 `set_focus` 夺焦后，WebView JS `keydown` → `invoke("key",{k})` → Rust `key()` → `handle_key()`。前端忽略按住的 repeat 键（字母/数字）。`F1` 帮助页打开期间前端截获全部按键（仅 Esc/F1 关闭），不 invoke 状态机；`F2` 设置页同理。
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
