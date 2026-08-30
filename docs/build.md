# 构建与验证指南

覆盖两种场景：本地开发编译验证、正式发布安装包（exe/MSI）构建。
调试与输入路径见 [debug.md](debug.md)；版本号、双语 release note、发布流程见 [release.md](release.md)。

## 环境前置

- **Rust toolchain**（rustup 安装，stable 即可）
- **Node.js + npm**（Tauri CLI 入口：`npm run tauri`，依赖在 `package.json` 的 devDependencies，首次先 `npm install`）
- **WebView2**：Win10 自带，无需安装
- 编译器：MSVC toolchain + VS Build Tools（Rust MSVC target 默认即可）

## 本地编译（dev）

```bash
npm run tauri dev
```

特性：
- debug 构建**跳过提权**（`cfg!(debug_assertions)` 判断，见 `src-tauri/src/lib.rs`），不弹 UAC，迭代快
- 日志直接打印到终端
- 配置：统一存 `%APPDATA%\WinHop\config.json`（debug 与 release 一致）

## 单测

```bash
cd src-tauri
cargo test --release
```

现有回归测试（`src-tauri/src/windows.rs`）：
- `full_name_not_truncated`：版本资源名称不截断（Chrome → "Google Chrome"）
- `system_exe_not_generic_name`：系统二进制不返回通用串

## 手动验证清单（每次改动后）

1. Ctrl+Space 呼出/退出覆盖层（**连续多轮**，确认热键不失效）
2. 字母键选程序、数字键选窗口、方向键移动、Enter 确认、Esc 逐层退出
3. 鼠标点击行选择；点击覆盖层外部关闭
4. 自动补全程序显示名称正确（与任务管理器「文件说明」一致），点 ✎ 可改名/配代号并写入配置
5. 多字母模式：设置页（F2）切换后，连续字母筛选、Enter 确认、Backspace 删除、代号（multi_key）匹配
6. 空格快速跳转（最近两个窗口来回切）；程序超过 20 个时 PageUp/PageDown 翻页
7. 设置页（F2）：窗口排序切换、多字母开关、版本与更新记录显示；未保存返回弹确认（热键录制功能仍暂停，见 debug-hotkey-hang.md）
8. 缩略图：小图/大预览颜色、比例、多窗口切换不闪烁
9. 托盘：左键唤出、右键退出
10. 日志确认：`%APPDATA%\WinHop\winhop.log` 正常写入

## 正式发布构建（安装包）

### 1. 版本号

两处同步（不一致会导致安装包文件名与程序版本不符）：

- `src-tauri/tauri.conf.json` → `"version": "X.Y.Z"`
- `src-tauri/Cargo.toml` → `version = "X.Y.Z"`

### 2. 前置清理

**必须杀掉运行中的 winhop 实例**：exe 被占用时构建报 `拒绝访问 (os error 5)`。
实例是提权运行，普通 taskkill 无权限，用提权 taskkill（弹一次 UAC）：

```powershell
Start-Process taskkill -ArgumentList '/F','/IM','winhop.exe' -Verb RunAs -Wait
```

确认无残留：

```bash
tasklist //FI "IMAGENAME eq winhop.exe"
```

### 3. 构建

```bash
npm run tauri build
```

产物（`src-tauri/target/release/bundle/`）：

```
bundle/msi/WinHop_X.Y.Z_x64_en-US.msi     ← MSI 安装包
bundle/nsis/WinHop_X.Y.Z_x64-setup.exe    ← NSIS 安装包（推荐）
```

说明：
- release 构建的 exe 启动会**弹 UAC**（`elevate: true`，切换管理员程序必需）
- NSIS 需要本机装有 makensis（Tauri CLI 自动下载）；MSI 走 WiX/light（首次构建自动准备）

### 4. 安装包验证

1. 全新安装（本机先卸载旧版）→ 首次运行自动生成 `%APPDATA%\WinHop\config.json`
2. 升级安装（覆盖旧版）→ **配置不丢**（用户目录安装器不触碰）
3. 卸载重装 → 配置仍在
4. 旧版升级（曾用 WinTab 名）→ `%APPDATA%\WinTab` 首次运行自动迁移到 `%APPDATA%\WinHop`
5. 日志在 `%APPDATA%\WinHop\winhop.log`

### 5. 发布到 GitHub

打 tag、写双语 release note、建 Release（含 gh CLI 与网页两种方式）的完整流程见 [release.md](release.md)。

## 常见问题

| 问题 | 原因 | 解决 |
|---|---|---|
| 构建报 `拒绝访问 (os error 5)` | winhop.exe 在运行（单实例，文件被锁） | 提权 taskkill 后重试 |
| 改图标后 exe 图标仍是旧的 | tauri-build 的 Windows 资源（winres）缓存，icon 文件变化不触发 build script 重跑 | `touch src-tauri/build.rs src-tauri/tauri.conf.json` 后重新构建 |
| 安装后桌面快捷方式图标仍旧 | Windows shell 图标缓存（exe/t 托盘已正确） | 重启 Explorer，或删 `%localappdata%\IconCache.db` 与 `%localappdata%\Microsoft\Windows\Explorer\iconcache*` 后重启 Explorer |
| `no such command: tauri` | cargo-tauri 未装（用 npm 入口即可） | 项目根 `npm run tauri ...` |
| 热键无响应 | 冲突检测功能在 `hotkey-conflict-wip` 分支未合入；或另一实例占热键 | 确认单实例；杀残留进程 |
| 调试需要看日志 | release 无控制台 | 看 `%APPDATA%\WinHop\winhop.log` |
