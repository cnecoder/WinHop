# Release 发布流程

版本号、发布步骤、release note 规范。构建细节见 [build.md](build.md)，排障见 [debug.md](debug.md)。

## 1. 版本号三处同步

发版前统一改到 `X.Y.Z`：

- `src-tauri/tauri.conf.json` → `"version"`
- `src-tauri/Cargo.toml` → `version`
- `src-tauri/src/lib.rs` → `CURRENT_CHANGELOG.version`（设置页「更新记录」显示的版本）

## 2. 写双语更新记录（两处）

release note 有两个落点，**内容保持一致**：

### a. 设置页内（app 内显示）

`src-tauri/src/lib.rs` 的 `CURRENT_CHANGELOG`：

```rust
const CURRENT_CHANGELOG: ChangelogEntry = ChangelogEntry {
    version: "0.3.0",
    date: "2026-08",
    notes_zh: &[ /* 中文条目，每条一个字符串 */ ],
    notes_en: &[ /* 英文条目，逐条对应中文 */ ],
};
```

- 后端通过 `get_settings` 把 `notes_zh` / `notes_en` 都发给前端，前端按当前界面语言选取（`src/main.js` openSettings 内 `isEn() ? notes_en : notes_zh`），切语言即时切换。
- 面向用户，一句话说清「改了什么、对我有什么影响」，不写内部实现/commit hash。

### b. GitHub Release 正文

归档于 `docs/release-vX.Y.Z.md`，**中英双语同一文件**：结构为 `## 中文` 段 + `## English` 段。

## 3. Release note 规范

- **首条为重点**：用户最关心的修复/特性放第一条，并加粗一句概括。
- **前缀 emoji 分类**：
  - 🐛 修复（bug fix）
  - ✨ / 🌐 / 🚫 / 🔢 / 🖼️ 等按内容选：新功能、多语言、屏蔽、数字、缩略图等
- **每条一个可感知变化**：从用户视角写（「切换窗口后键盘不再错乱」），不写代码细节（不说「改用 SPI_SETFOREGROUNDLOCKTIMEOUT」）。
- **中英文逐条对应**：同一条目中文第 N 条 = 英文第 N 条，顺序一致。
- 不罗列内部重构（删死代码、依赖瘦身等）——这些进 commit message，不进用户 release note。

模板见 `docs/release-v0.3.0.md`。

## 4. 发布步骤

```bash
# 0. 改完三处版本号 + 双语 changelog，编译验证
cd src-tauri && cargo check && cargo test

# 1. 杀掉运行实例（release 提权运行占用 exe，需提权 taskkill）
powershell -Command "Start-Process taskkill -ArgumentList '/F','/IM','winhop.exe' -Verb RunAs -Wait"

# 2. 打安装包（产物见 build.md）
npm run tauri build

# 3. 提交版本改动
git add -A && git commit -m "vX.Y.Z：..." && git push

# 4. 打 tag 并推送
git tag -a vX.Y.Z -m "WinHop vX.Y.Z"
git push origin vX.Y.Z
```

产物：

```
src-tauri/target/release/bundle/nsis/WinHop_X.Y.Z_x64-setup.exe   ← NSIS（推荐）
src-tauri/target/release/bundle/msi/WinHop_X.Y.Z_x64_en-US.msi    ← MSI
```

## 5. 创建 GitHub Release

**首选 `gh` CLI**（需 `gh auth login`；若 api.github.com 走代理，命令前加
`HTTPS_PROXY=http://127.0.0.1:10809`，端口按本机代理改）：

```bash
gh release create vX.Y.Z \
  "src-tauri/target/release/bundle/nsis/WinHop_X.Y.Z_x64-setup.exe" \
  "src-tauri/target/release/bundle/msi/WinHop_X.Y.Z_x64_en-US.msi" \
  --title "WinHop vX.Y.Z" \
  --notes-file docs/release-vX.Y.Z.md
```

**gh 认证不可用时（网络受限，如本机仅 SSH 能通）走网页**：

1. 打开 `https://github.com/<owner>/<repo>/releases/new?tag=vX.Y.Z`（tag 已推，自动选中）
2. Title 填 `WinHop vX.Y.Z`
3. 正文粘贴 `docs/release-vX.Y.Z.md` 全文
4. Attach binaries 拖入上面两个安装包
5. 勾 "Set as the latest release" → Publish

发布后用 API 核对（带代理）：

```bash
curl -s -x http://127.0.0.1:10809 \
  "https://api.github.com/repos/<owner>/<repo>/releases/tags/vX.Y.Z" \
  | grep -E '"tag_name"|"name"|"browser_download_url"'
```
