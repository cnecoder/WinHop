# 构建与验证指南

覆盖两种场景：本地开发编译验证、正式发布安装包（exe/MSI）构建。

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
- 配置：新版统一存 `%APPDATA%\WinTab\config.json`（debug 与 release 一致）

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
4. 自动补全程序显示名称正确（与任务管理器「文件说明」一致），点 **+** 号入配置
5. 设置面板（F2）：窗口排序切换、热键输入（注意：热键设置功能已暂停，见 debug-hotkey-hang.md）
6. 缩略图：小图/大预览颜色、比例、多窗口切换不闪烁
7. 托盘：左键唤出、右键退出
8. 日志确认：`%APPDATA%\WinTab\wintab.log` 正常写入

## 正式发布构建（安装包）

### 1. 版本号

两处同步（不一致会导致安装包文件名与程序版本不符）：

- `src-tauri/tauri.conf.json` → `"version": "X.Y.Z"`
- `src-tauri/Cargo.toml` → `version = "X.Y.Z"`

### 2. 前置清理

**必须杀掉运行中的 wintab 实例**：exe 被占用时构建报 `拒绝访问 (os error 5)`。
实例是提权运行，普通 taskkill 无权限，用提权 taskkill（弹一次 UAC）：

```powershell
Start-Process taskkill -ArgumentList '/F','/IM','wintab.exe' -Verb RunAs -Wait
```

确认无残留：

```bash
tasklist //FI "IMAGENAME eq wintab.exe"
```

### 3. 构建

```bash
npm run tauri build
```

产物（`src-tauri/target/release/bundle/`）：

```
bundle/msi/WinTab_X.Y.Z_x64_en-US.msi     ← MSI 安装包
bundle/nsis/WinTab_X.Y.Z_x64-setup.exe    ← NSIS 安装包（推荐）
```

说明：
- release 构建的 exe 启动会**弹 UAC**（`elevate: true`，切换管理员程序必需）
- NSIS 需要本机装有 makensis（Tauri CLI 自动下载）；MSI 走 WiX/light（首次构建自动准备）

### 4. 安装包验证

1. 全新安装（本机先卸载旧版）→ 首次运行自动生成 `%APPDATA%\WinTab\config.json`
2. 升级安装（覆盖旧版）→ **配置不丢**（用户目录安装器不触碰）
3. 卸载重装 → 配置仍在
4. 日志在 `%APPDATA%\WinTab\wintab.log`

### 5. 发布到 GitHub

```bash
# 认证（首次）
gh auth login

# 打 tag + 建 release（名称/说明按需改）
git tag v0.1.1
git push origin v0.1.1
gh release create v0.1.1 \
  "src-tauri/target/release/bundle/nsis/WinTab_0.1.1_x64-setup.exe" \
  "src-tauri/target/release/bundle/msi/WinTab_0.1.1_x64_en-US.msi" \
  --title "WinTab 0.1.1" --notes "见仓库 docs/CHANGELOG 或历史 release"
```

## 常见问题

| 问题 | 原因 | 解决 |
|---|---|---|
| 构建报 `拒绝访问 (os error 5)` | wintab.exe 在运行（单实例，文件被锁） | 提权 taskkill 后重试 |
| `no such command: tauri` | cargo-tauri 未装（用 npm 入口即可） | 项目根 `npm run tauri ...` |
| 热键无响应 | 冲突检测功能在 `hotkey-conflict-wip` 分支未合入；或另一实例占热键 | 确认单实例；杀残留进程 |
| 调试需要看日志 | release 无控制台 | 看 `%APPDATA%\WinTab\wintab.log` |
