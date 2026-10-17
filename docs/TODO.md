# TODO（未完成需求与改进）

## 功能缺口（已确认需求但未实现，Windows-only 后均为可用专属能力）

- [ ] **UIAccess 免 UAC**（高优先级）：代码签名 + 可信位置部署后可免提权弹窗（需签名证书）
- [ ] **虚拟桌面支持**（低优先级，用户场景少用）：目标窗口在另一任务视图桌面时无法激活——`IVirtualDesktopManager` COM API（先做识别+标记，再考虑自动切桌面）
- [ ] **`winhop capture` 辅助命令**：CLI 打印当前激活窗口的进程名/标题，方便手写配置（原设计项，现已被「✎ 统一编辑入配置」部分替代，纯手写场景仍有用）

## 体验优化

- [ ] **热键冲突提示**：`RegisterHotKey` 注册失败时已记日志 + 托盘兜底（不再静默/崩溃）；进一步可提示具体占用方
- [ ] **主题跟随系统**：深色/浅色切换
- [~] **DPI 感知**（部分完成）：覆盖层卡片/窗口层已按屏幕高度与每页卡片数连续缩放（`--ui-scale`，含 DPR 变化重算，见 design.md §5「覆盖层缩放」）；**未做**：覆盖层只开在主显示器，多显示器不同缩放比例时的跨屏跟随
- [ ] **窗口「拉到当前屏」选项**：每程序可配置切换时是否把窗口移到当前屏（原需求讨论过，默认留在原屏）
- [ ] **Acrylic 在多显示器/特殊 DPI 场景验证**：acrylic 背景已在 Win10 22H2 实机验证可用（SWCA 路径）；未覆盖多屏不同缩放、远程会话等场景，异常时用户可回退 solid（设置页二选一）
- [ ] **程序图标持久缓存**：图标提取现仅在内存缓存（每次启动重提取一次，后台线程不阻塞呼出）；后续可落盘缓存（进程 exe 路径 + mtime 失效）减少启动后台开销

## 工程

- [ ] **集成测试**：窗口枚举/激活依赖 Win32 桌面会话，需集成测试环境（现有单测覆盖配置校验/序列化、状态机筛选排序、版本资源，见 `cargo test`）
- [ ] **windows.rs 模块拆分**：2119 行承载枚举/激活/钩子/DWM/PWA/图标/注册表/日志等职责；PWA 子系统（AUMID/PEB/AppX/lnk 约 700 行）边界清晰可拆 `pwa.rs`，`HookMsg` 可上移。纯组织性改动、无行为收益，待下次功能迭代时一并做

## 代码 review 发现（2026-10-17）

- [x] **`file_description` 加缓存**：`build_prog_list` 每次呼出对未配置运行中进程同步读 exe 版本资源（磁盘 IO），显示名不随运行变化——加 path → 显示名 进程内缓存（含负缓存，与 ICONS 同款模式）
- [x] **PWA 检测扩展到全部 Chromium 系浏览器**：`enum_proc` 原只匹配 `chrome.exe | msedge.exe`，与 design.md「Brave/Vivaldi/Opera/Arc crx 同源可用」不符——扩展进程名单（classify_pwa 靠 `._crx_` 标记，本就不依赖具体浏览器名）
- [x] **注释滞后修正**：lib.rs `Render.anim_close_ms` 引用已删除的 `CLOSE_ANIM_MS_SWITCH`；main.js 关闭淡出注释「切换类 100ms」（已改瞬时）；main.js `applyBg(info.overlay_bg || "translucent")` 回退值（translucent 已移除）；settings.rs / lib.rs `overlay_bg` 注释仍含 translucent
- [x] **`hotkey_capture_start` 防重复线程**：连续调用会 spawn 多个轮询线程（30ms 轮询窗口内旧线程未退出即重复）——加代际号（generation），旧线程按代际退出；前端 `startHotkeyCapture` 加 `hkListening` 防重入（原实现重复点击还会叠加 poll 定时器）
- [x] **`close_impl` 合并两次 cfg 锁**：算 `anim_ms` 与构造 `Render::closed` 各取一次锁，合并为一次（消除两锁之间配置变化的理论窗口）
- [ ] **`config::validate` 非法代号 panic 的权衡**（待决策）：release 下用户表现为无提示闪退；可改「跳过条目 + 日志/托盘提示」，但会削弱「配置错误启动即暴露」的设计意图，需用户拍板
- [ ] **`activate()` 的 `SPIF_SENDCHANGE`**（待实测）：前后两次 `SPI_SETFOREGROUNDLOCKTIMEOUT` 均带该标志，每次切换全局广播两次 `WM_SETTINGCHANGE`；多数实现不带也能生效，需实机验证激活行为后决定是否去掉
- [~] **`match_score`/`view_indices` 每键重复计算**（评估后不做）：每次按键对每条目 to_lowercase ×3 + 全量排序；程序数 <100 时开销微秒级，预计算增加 `ProgEntry` 字段与维护成本，不值——真出现性能问题再按呼出时缓存小写副本处理

## 已完成（历史记录）

- [x] **覆盖层顺畅化 + 视觉升级**：背景效果两档（不透明默认/Acrylic 系统模糊，设置页可选；曾有三档半透明因桌面透出杂乱移除）；退出类关闭柔和淡出（设置开关，200ms；切换类关闭一律瞬时——曾试全档 200ms/切换 100ms 淡出，均被反馈切换迟钝后回退；淡出期提前激活会让覆盖层「可见但失焦」析出顶端横条伪影，实测回退，激活统一在动画结束回执后执行；`overlay_hidden` 回执收尾 + 看门狗兜底）；程序行应用图标（SHGetFileInfoW 提取，后台线程 + 独立 icons 事件，PWA 用宿主浏览器图标）；选中/悬停高亮维持名称段（整行高亮因代号槽变宽对不齐回退）；呼出行级联入场与窗口层淡入（reduced-motion 全覆盖）；阴影 token 化（v0.3.x）
- [x] **开机自启**：设置页勾选 + `autostart` 配置项，落地注册表 `HKCU\...\Run` 的 `WinHop` 值；保存先写注册表（失败整体不保存），启动时以配置为准幂等对齐（v0.3.x）
- [x] **删除程序配置**：✎ 编辑面板「删除」按钮移除已存名称/代号（`delete_program` 命令 + 纯函数 `apply_program_delete`），未运行即不再灰色显示，运行中仍作为未配置项（·）可重配；与「屏蔽」（加黑名单彻底隐藏）区分（v0.3.x）
- [x] **自动化单元测试**：配置校验/原子保存/序列化（config.rs）、纯状态机 `OverlayState::transition` 键位模型（数字累积/组合编号/preview/Esc 两级/字母筛选/翻页/空格 MRU）、匹配筛选/程序列表排序与名称回退、程序编辑代号校验与冲突检测（`validate_program_key`/`apply_program_edit`/`apply_program_delete`）、热键 vk 映射、`enum_windows` 冒烟、版本资源（windows.rs，缺目标文件时显式 SKIP 标注）、配置 `prog_page_size` 默认/钳制、分页随配置页长、运行时生效页长覆盖（lib.rs/config.rs）、PWA 分类与清单名解析（windows.rs），`cargo test` 共 42 项（v0.3.x）
- [x] **前端纯函数单测**：`src/util.js`（escapeHtml/prettyHotkey/settingsFormEquals/rectPhys/clipRectPhys/winHintKind/cardScale 等不依赖 DOM 的逻辑）用 `node --test` 覆盖（19 项），新增纯逻辑抽到此文件并加 `*.test.js`
- [x] **CI**：GitHub Actions（`.github/workflows/ci.yml`）——PR/主分支跑 `cargo test` + 前端 `node --test`/`node --check`；推 `v*` tag 自动构建 NSIS/MSI 并幂等发布 Release（流程见 [release.md](release.md)）

- [x] **日志轮转**：`winhop.log` 超 1MB 轮转 `winhop.log.1`（v0.3.0）
- [x] **实时缩略图**：窗口层改用 DWM 缩略图实时合成，被遮挡/最小化也清晰、随窗口内容实时更新（替代 PrintWindow/WGC 静态截图，v0.3.0）
- [x] **黑名单 + 窗口数字浏览模式**：屏蔽程序、数字键先聚焦预览模式（v0.2.x）
- [x] **键盘输入混乱根治**：移除 Alt 按键注入，改前台锁超时激活，杜绝 Alt 卡死（v0.3.0，详见 [debug.md](debug.md)）
- [x] **配置原子写**：临时文件 + rename，防写坏崩溃（v0.3.0）
