import { setLang, t, isEn, applyStaticI18n } from "./i18n.js";
import {
  escapeHtml,
  prettyHotkey,
  settingsFormEquals,
  rectPhys,
  clipRectPhys,
  winHintKind,
  cardScale,
  clampPageSize,
  pageSizeSettingBounds,
  optimalPageSize,
} from "./util.js";

const { listen } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;

// 与后端 windows::PWA_PROC_PREFIX 一致：Chromium PWA 的虚拟进程键前缀
const PWA_PROC_PREFIX = "pwa#";

// 每页卡片数默认偏好（绝对 sanity 区间 8–64 在 Rust 与 util pageSizeSettingBounds 中）。
// 注意：设置值是用户偏好，设置页步进范围与实际生效值都按屏幕可行区间钳制
const PROG_PAGE_SIZE_DEFAULT = 20;

const appEl = document.getElementById("app");
const titleEl = document.getElementById("title");
const listEl = document.getElementById("list");
const overlayView = document.getElementById("overlay-view");
const settingsView = document.getElementById("settings-view");
const helpView = document.getElementById("help-view");
const confirmMask = document.getElementById("confirm-mask");

let settingsOpen = false;
let helpOpen = false; // 帮助页：覆盖层内纯前端视图，不经过 Rust 状态机
let currentHotkey = "ctrl+space"; // 当前已保存热键（启动/打开设置时同步），帮助页展示
let state = null;
let settingsLoaded = null; // 已保存的设置快照，用于判断是否改动
let blockedState = null; // 设置页黑名单本地暂存（解除不立即生效，保存后统一写入）
let langOverride = null; // 语言手动覆盖（null=跟随系统），保存后持久化
let langChoice = "system"; // 设置页语言单选当前值（system/zh-CN/en）
let sysLang = "zh-CN"; // 后端检测到的系统语言

// 应用语言到 UI（不重开设置页，避免与关闭时序竞态）：
// choice 为 system/zh-CN/en；setLang + 静态文案 + 覆盖层重绘（settingsOpen 时 render 会被挡，调用方保证顺序）
function applyLanguage(choice) {
  langChoice = choice;
  langOverride = choice === "system" ? null : choice;
  setLang(langOverride || sysLang);
  document.documentElement.lang = isEn() ? "en" : "zh-CN";
  applyStaticI18n();
  if (typeof state === "object" && state) {
    render(state); // 覆盖层动态文案（settingsOpen=true 时 render 内部早退，需先关设置页）
  }
}

// 应用主题：写到 <html data-theme>，CSS 变量块据此切换
function applyTheme(id) {
  document.documentElement.dataset.theme = id || "black-green";
}

// 读取设置页表单当前值
function readSettingsForm() {
  const order = document.querySelector('input[name="order"]:checked');
  const theme = document.querySelector('input[name="theme"]:checked');
  const mode = document.querySelector('input[name="mode"]:checked');
  const wdm = document.querySelector('input[name="win-digit-mode"]:checked');
  return {
    hotkey: formHotkey,
    autostart: document.getElementById("autostart-check").checked,
    window_order: order ? order.value : "zorder",
    multi_letter: (mode ? mode.value : "single") === "multi",
    theme: theme ? theme.value : "black-green",
    win_digit_mode: wdm ? wdm.value : "jump",
    prog_page_size:
      Number(document.getElementById("pagesize-value").textContent) ||
      PROG_PAGE_SIZE_DEFAULT,
    lang: langChoice, // system 或具体语言
    blocked: (blockedState || []).map((b) => b.process),
  };
}

// 设置是否有未保存改动（表单当前值 vs 已保存快照；blocked 顺序无关）
function settingsDirty() {
  if (!settingsLoaded) return false;
  return !settingsFormEquals(readSettingsForm(), settingsLoaded);
}

// 更新保存按钮可用状态与状态文案
function updateSettingsState() {
  const dirty = settingsDirty();
  const saveBtn = document.getElementById("settings-save");
  saveBtn.disabled = !dirty;
  document.getElementById("settings-status").textContent = dirty
    ? t("setUnsaved")
    : "";
}

// 打开设置页：拉取设置、填表单、显示版本与更新记录
async function openSettings() {
  const info = await invoke("get_settings");
  currentHotkey = info.hotkey || "ctrl+space";
  settingsLoaded = {
    hotkey: info.hotkey || "ctrl+space",
    autostart: !!info.autostart,
    window_order: info.window_order,
    multi_letter: info.multi_letter,
    theme: info.theme,
    win_digit_mode: info.win_digit_mode || "jump",
    prog_page_size: info.prog_page_size || PROG_PAGE_SIZE_DEFAULT,
    // 保存的语言选择：跟随系统=system，否则具体语言
    lang: info.lang_cfg || "system",
    blocked: (info.blocked || []).map((b) => b.process).sort(),
  };
  document.getElementById("autostart-check").checked = !!info.autostart;
  document.querySelectorAll('input[name="order"]').forEach((r) => {
    r.checked = r.value === info.window_order;
  });
  const modeVal = info.multi_letter ? "multi" : "single";
  document.querySelectorAll('input[name="mode"]').forEach((r) => {
    r.checked = r.value === modeVal;
    r.onchange = () => {
      syncMultiOpts();
      updateSettingsState();
    };
  });
  syncMultiOpts();
  document.querySelectorAll('input[name="win-digit-mode"]').forEach((r) => {
    r.checked = r.value === (info.win_digit_mode || "jump");
  });
  // 每页卡片数步进器：上下限为当前屏幕反推的可行区间，点改只暂存，保存后生效。
  // 已存偏好若超出本屏区间（如在大屏设的小值换到小屏），打开即钳到边界并同步快照
  const n = clampPageSize(
    info.prog_page_size || PROG_PAGE_SIZE_DEFAULT,
    rowsAvailHeight()
  );
  document.getElementById("pagesize-value").textContent = String(n);
  settingsLoaded.prog_page_size = n;
  updatePageSizeUi();
  // 语言单选项：system（跟随系统）/ zh-CN / en。点选只暂存，保存后生效。
  // "跟随系统"标签显示系统实际检测语言（lang_sys，与用户设置无关）
  sysLang = info.lang_sys || "zh-CN";
  const langBox = document.getElementById("lang-options");
  const langCur = langChoice; // 当前选择（system/zh-CN/en），由保存的设置决定
  const sysName = sysLang === "en" ? "English" : "简体中文";
  langBox.innerHTML =
    `<label class="setting-row"><input type="radio" name="lang" value="system"${langCur === "system" ? " checked" : ""} /><span>${escapeHtml(t("langSystem", { lang: sysName }))}</span></label>` +
    `<label class="setting-row"><input type="radio" name="lang" value="zh-CN"${langCur === "zh-CN" ? " checked" : ""} /><span>${escapeHtml(t("langZh"))}</span></label>` +
    `<label class="setting-row"><input type="radio" name="lang" value="en"${langCur === "en" ? " checked" : ""} /><span>${escapeHtml(t("langEn"))}</span></label>`;
  langBox.querySelectorAll('input[name="lang"]').forEach((r) => {
    r.addEventListener("change", () => {
      langChoice = r.value; // 仅暂存，保存后 apply（见 saveSettingsAndClose）
      updateSettingsState();
    });
  });
  // 主题单选项由后端下发 id 列表；显示名由前端 i18n 唯一负责（后端不硬编码中文名）
  const themeBox = document.getElementById("theme-options");
  const themeName = (id) => {
    const key = { "black-green": "themeBlackGreen", "black-yellow": "themeBlackYellow" }[id];
    return key && t(key) !== key ? t(key) : id;
  };
  themeBox.innerHTML = (info.themes || [])
    .map(
      (th) =>
        `<label class="setting-row"><input type="radio" name="theme" value="${escapeHtml(
          th.id
        )}"${th.id === info.theme ? " checked" : ""} /><span>${escapeHtml(
          themeName(th.id)
        )}</span></label>`
    )
    .join("");
  themeBox.querySelectorAll('input[name="theme"]').forEach((r) => {
    // 点选即预览（未保存返回/Esc 会回退）
    r.addEventListener("change", () => {
      applyTheme(r.value);
      updateSettingsState();
    });
  });
  applyTheme(info.theme);
  renderBlocked(info.blocked || []);
  document.getElementById("version-info").textContent = t("version", { v: info.version });
  const e = info.changelog;
  const notes = isEn() ? e.notes_en : e.notes_zh;
  document.getElementById("changelog").innerHTML =
    `<div class="changelog-entry">
      <div class="changelog-ver">${escapeHtml(e.version)} <span class="changelog-date">${escapeHtml(e.date)}</span></div>
      <ul>${notes.map((n) => `<li>${escapeHtml(n)}</li>`).join("")}</ul>
    </div>`;
  // 热键：打开设置即临时注销（录制能收到组合键、且不误触 toggle）；保存时注册新键，放弃时 resume
  formHotkey = info.hotkey || "ctrl+space";
  hkListening = false;
  renderHotkey();
  invoke("hotkey_suspend").catch(() => {});
  updateSettingsState();
  settingsOpen = true;
  overlayView.hidden = true;
  settingsView.hidden = false;
}

function closeSettings() {
  settingsOpen = false;
  settingsView.hidden = true;
  overlayView.hidden = false;
}

// 帮助页：覆盖层内纯前端视图（Rust 状态机仍停在程序层），Esc/F1/返回回到覆盖层
function openHelp() {
  helpOpen = true;
  document.getElementById("help-hotkey").textContent = prettyHotkey(currentHotkey);
  overlayView.hidden = true;
  helpView.hidden = false;
}

function closeHelp() {
  helpOpen = false;
  helpView.hidden = true;
  overlayView.hidden = false;
}

// 放弃修改（返回/不保存）：热键仍是 suspend 状态，恢复注册旧键
function discardSettings() {
  resumeHotkey();
  closeSettings();
}

// 黑名单列表（设置页本地暂存：解除不立即生效，保存后统一写入）
function renderBlocked(blocked) {
  blockedState = blocked.map((b) => ({ ...b }));
  renderBlockedUi();
}

function renderBlockedUi() {
  const box = document.getElementById("blocked-list");
  const blocked = blockedState || [];
  if (!blocked.length) {
    box.innerHTML = `<span class="blocked-empty">${t("blockedEmpty")}</span>`;
    updateSettingsState();
    return;
  }
  box.innerHTML = "";
  for (const item of blocked) {
    const row = document.createElement("div");
    row.className = "blocked-row";
    const left = document.createElement("span");
    left.className = "blocked-name";
    const procName = document.createElement("b");
    procName.textContent = item.process;
    left.appendChild(procName);
    if (item.note) {
      const note = document.createElement("span");
      note.className = "blocked-note";
      note.textContent = item.note;
      left.appendChild(note);
    }
    const btn = document.createElement("button");
    btn.className = "blocked-unblock";
    btn.textContent = t("unblock");
    btn.addEventListener("click", () => {
      // 仅本地移除，保存后才真正生效
      blockedState = blockedState.filter((b) => b.process !== item.process);
      renderBlockedUi();
    });
    row.appendChild(left);
    row.appendChild(btn);
    box.appendChild(row);
  }
  updateSettingsState();
}

// 设置页步进器上下限 = 当前屏幕反推的可行区间（卡片缩放保证在 0.75–1.35 内且铺满）
function settingPageSizeBounds() {
  return pageSizeSettingBounds(rowsAvailHeight());
}

// 每页卡片数步进器：刷新数值显示与上下限禁用态
function updatePageSizeUi() {
  const n =
    Number(document.getElementById("pagesize-value").textContent) ||
    PROG_PAGE_SIZE_DEFAULT;
  const b = settingPageSizeBounds();
  document.getElementById("pagesize-dec").disabled = n <= b.min;
  document.getElementById("pagesize-inc").disabled = n >= b.max;
}

document.getElementById("pagesize-dec").addEventListener("click", () => {
  const el = document.getElementById("pagesize-value");
  const b = settingPageSizeBounds();
  const n = Number(el.textContent) || PROG_PAGE_SIZE_DEFAULT;
  if (n > b.min) {
    el.textContent = String(n - 1);
    updatePageSizeUi();
    updateSettingsState();
  }
});
document.getElementById("pagesize-inc").addEventListener("click", () => {
  const el = document.getElementById("pagesize-value");
  const b = settingPageSizeBounds();
  const n = Number(el.textContent) || PROG_PAGE_SIZE_DEFAULT;
  if (n < b.max) {
    el.textContent = String(n + 1);
    updatePageSizeUi();
    updateSettingsState();
  }
});
// 重置：一键回到当前屏幕的推荐卡片数（基准卡片大小 scale≈1，铺满行区）
document.getElementById("pagesize-reset").addEventListener("click", () => {
  const el = document.getElementById("pagesize-value");
  el.textContent = String(optimalPageSize(rowsAvailHeight()));
  updatePageSizeUi();
  updateSettingsState();
});

// 多字母专属选项：仅选中多字母模式时显示
function syncMultiOpts() {
  const mode = document.querySelector('input[name="mode"]:checked');
  document.getElementById("multi-opts").style.display =
    mode && mode.value === "multi" ? "" : "none";
}

// ===== 全局热键录制：设置页表单字段（保存才生效）。打开设置时 suspend 热键，
// 放弃/返回时 resume 旧键，保存时由后端注册新键。
// 检测在 Rust 侧轮询 GetAsyncKeyState（webview 事件会被中文输入法吞掉，
// 物理键状态不受影响）；前端每 100ms 轮询取结果 =====
const hotkeyDisplay = document.getElementById("hotkey-display");
const hotkeyBtn = document.getElementById("hotkey-btn");
let hkListening = false;
let hkPollTimer = null;
let formHotkey = ""; // 设置页内暂存的热键（未保存）

function renderHotkey() {
  hotkeyDisplay.classList.toggle("listening", hkListening);
  hotkeyDisplay.classList.remove("err");
  hotkeyDisplay.textContent = formHotkey ? prettyHotkey(formHotkey) : "…";
}

function startHotkeyCapture() {
  hkListening = true;
  hotkeyDisplay.classList.add("listening");
  hotkeyDisplay.classList.remove("err");
  hotkeyDisplay.textContent = t("hotkeyListening");
  hotkeyBtn.blur(); // 避免 Space/Enter 再触发按钮
  invoke("hotkey_capture_start").catch(() => {});
  hkPollTimer = setInterval(async () => {
    try {
      const combo = await invoke("hotkey_capture_poll");
      if (combo) {
        formHotkey = combo;
        endHotkeyCapture();
        updateSettingsState();
      }
    } catch {}
  }, 100);
}

function endHotkeyCapture() {
  hkListening = false;
  if (hkPollTimer) {
    clearInterval(hkPollTimer);
    hkPollTimer = null;
  }
  invoke("hotkey_capture_stop").catch(() => {});
  renderHotkey();
}

hotkeyBtn.addEventListener("click", startHotkeyCapture);

// 放弃修改 / 直接返回：恢复注册旧热键
async function resumeHotkey() {
  try {
    await invoke("hotkey_resume");
  } catch {
    /* 恢复失败不阻塞关闭 */
  }
}

// 返回/ESC 时若有未保存改动则弹确认
function requestCloseSettings() {
  if (settingsDirty()) {
    confirmMask.hidden = false;
  } else {
    discardSettings(); // 未改也返回：恢复 suspend 的热键
  }
}

async function saveSettingsAndClose() {
  const input = readSettingsForm();
  try {
    await invoke("save_settings", { input });
  } catch (err) {
    // 保存失败（多为新热键被占用）：后端已回退旧键，设置页保持打开供重试
    document.getElementById("settings-status").textContent = String(err);
    return;
  }
  settingsLoaded = input;
  updateSettingsState();
  closeSettings(); // 先关设置页，再应用语言 → 覆盖层 render 不被 settingsOpen 早退挡住
  applyLanguage(langChoice);
  // 保存后重新枚举并刷新列表（模式切换/黑名单解除/语言切换立即生效）
  invoke("refresh_overlay");
}

// 窗口层标题：多字母 + preview 模式提示数字聚焦/组合；其它模式只显示程序名
function winHint(s) {
  switch (winHintKind(s.multi_letter, s.win_digit_mode === "preview", s.win_digit)) {
    case "typed":
      return t("winTyped", { title: s.title, n: s.win_digit });
    case "enter":
      return t("winEnter", { title: s.title });
    default:
      return s.title;
  }
}

function renderHeader(s) {
  const badge = document.getElementById("mode-badge");
  const legend = document.getElementById("legend");
  const sep = document.querySelector(".header-sep");
  // 窗口层（数字选窗口）不显示模式徽章与图例
  if (s.phase === "windows") {
    badge.style.display = "none";
    legend.style.display = "none";
    if (sep) sep.style.display = "none";
    return;
  }
  badge.style.display = "";
  legend.style.display = "";
  if (sep) sep.style.display = "";
  if (s.multi_letter) {
    badge.className = "mode-badge mode-multi";
    badge.textContent = t("badgeMulti");
  } else {
    badge.className = "mode-badge mode-single";
    badge.textContent = t("badgeSingle");
  }
  legend.innerHTML =
    `<span class="legend-item"><span class="key key-cfg demo"></span>${t("legendCfg")}</span>` +
    `<span class="legend-item"><span class="key key-empty demo">·</span>${t("legendNone")}</span>` +
    `<span class="legend-item"><span class="key key-off demo"></span>${t("legendOff")}</span>`;
}

// 覆盖层夺焦后按键落在本 webview 内部（raw input 内部可达），路由给 Rust 状态机。
// LL 键盘钩子对 Chromium 前台无效（raw input 绕过钩子链），这是唯一的按键路径。
window.addEventListener("keydown", (e) => {
  // 设置页按键独立处理
  if (settingsOpen) {
    if (hkListening) {
      // 录制中：Esc 结束录制、Enter 确认当前录制值（检测结果由轮询写入 formHotkey）
      if (e.key === "Escape") {
        e.preventDefault();
        endHotkeyCapture();
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        endHotkeyCapture();
        return;
      }
      // 其余按键不拦截，交给 Rust 轮询检测
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      if (confirmMask.hidden) requestCloseSettings();
      return;
    }
    // 表单内的按键（radio/checkbox）正常处理，不路由
    return;
  }
  // 帮助页：纯前端视图，Esc / F1 / 返回按钮关闭，其余键一律不路由给状态机
  if (helpOpen) {
    if (e.key === "Escape" || e.key === "F1") {
      e.preventDefault();
      closeHelp();
    }
    return;
  }
  // 覆盖层中编辑程序的输入框：按键不参与快捷键路由
  if (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA") return;
  e.preventDefault();
  if (e.key === "F1") {
    openHelp();
    return;
  }
  if (e.key === "F2") {
    openSettings();
    return;
  }
  if (e.key === "F11") {
    invoke("toggle_fullscreen");
    return;
  }
  let k = null;
  if (e.key === "Escape") k = "esc";
  else if (e.key === "ArrowUp") k = "up";
  else if (e.key === "ArrowDown") k = "down";
  else if (e.key === "PageUp") k = "pageup";
  else if (e.key === "PageDown") k = "pagedown";
  else if (e.key === "Backspace") k = "back";
  else if (e.key === " " || e.code === "Space") k = "space";
  else if (e.key === "Enter") k = "enter";
  else if (/^[a-zA-Z]$/.test(e.key)) k = "letter:" + e.key.toLowerCase();
  else if (/^[0-9]$/.test(e.key)) k = "digit:" + e.key;
  if (k) {
    // 按住不放的重复按键：字母/数字会重复路由（多字母串累积、窗口轮询连转），忽略
    if (e.repeat && (k.startsWith("letter:") || k.startsWith("digit:"))) return;
    invoke("key", { k });
  }
});

document.getElementById("settings-btn").addEventListener("click", () => {
  openSettings();
});

document.getElementById("help-btn").addEventListener("click", openHelp);
document.getElementById("help-back").addEventListener("click", closeHelp);

document.getElementById("settings-back").addEventListener("click", () => {
  requestCloseSettings();
});

document
  .querySelectorAll('input[name="order"], input[name="mode"], input[name="win-digit-mode"]')
  .forEach((el) => {
    el.addEventListener("change", updateSettingsState);
  });

// 开机自启复用原生 radio 元素（外观与排序/模式单选项完全一致），但语义是布尔开关：
// radio 选中后默认点不掉，故在激活前（mousedown/keydown）记下旧状态，
// 若点击时它本就已选中 → 拦截默认并手动取消；未选中 → 走默认选中（change 正常触发）
const autostartCheck = document.getElementById("autostart-check");
let autostartWasChecked = false;
const captureAutostartWas = () => {
  autostartWasChecked = autostartCheck.checked;
};
// mousedown 挂到包裹 label（点文字/圆点都会冒泡到这里），键盘走 input 的 keydown
autostartCheck.closest("label").addEventListener("mousedown", captureAutostartWas);
autostartCheck.addEventListener("keydown", (e) => {
  if (e.key === " " || e.key === "Enter") captureAutostartWas();
});
autostartCheck.addEventListener("click", (e) => {
  if (autostartWasChecked) {
    e.preventDefault();
    autostartCheck.checked = false;
    updateSettingsState();
  }
});
autostartCheck.addEventListener("change", updateSettingsState);

document.getElementById("settings-save").addEventListener("click", () => {
  saveSettingsAndClose();
});

document.getElementById("confirm-save").addEventListener("click", async () => {
  confirmMask.hidden = true;
  await saveSettingsAndClose();
});
document.getElementById("confirm-discard").addEventListener("click", () => {
  confirmMask.hidden = true;
  // 丢弃未保存的主题预览，回退到已保存主题
  if (settingsLoaded) applyTheme(settingsLoaded.theme);
  discardSettings(); // 不保存：恢复旧热键
});
document.getElementById("confirm-cancel").addEventListener("click", () => {
  confirmMask.hidden = true;
});

// 鼠标点击选择：复用键盘路径——程序层按字母、窗口层按编号
listEl.addEventListener("click", (e) => {
  const editBtn = e.target.closest(".edit-btn");
  if (editBtn) {
    e.stopPropagation();
    startEditProgram(editBtn);
    return;
  }
  const row = e.target.closest(".row, .wrow");
  if (!row) return;
  if (state && state.phase === "windows" && row.dataset.idx) {
    // 点击=明确选择，直接跳转（键盘数字在多字母模式只聚焦）
    invoke("key", { k: "jump:" + row.dataset.idx });
  } else if (state && state.phase === "programs" && row.dataset.process) {
    // 多字母模式代号可能多字母，统一按 process 选中；单字母也适用
    invoke("pick_program", { process: row.dataset.process });
  }
});

// 统一编辑面板：行下方弹字母 + 名称框，Enter 保存（已配置改键/名，未配置入配置）
function startEditProgram(btn) {
  const row = btn.closest(".row");
  // 多字母模式下未配置程序的 key 均为空串，按 key 查找会命中排名最前的空 key 程序；
  // process 在列表中唯一，统一按它定位
  const prog = state.programs.find((p) => p.process === row.dataset.process) || {};
  if (!prog.process) return;

  const panel = document.createElement("div");
  panel.className = "add-panel";

  const fields = document.createElement("div");
  fields.className = "add-fields";

  const multi = !!state.multi_letter;
  const keyInput = document.createElement("input");
  keyInput.className = "key-input";
  if (multi) {
    // 多字母模式：1+ 字母，不限长度
    keyInput.placeholder = isEn() ? "multi-letter code" : "多字母代号";
    keyInput.value = prog.multi_key || "";
  } else {
    keyInput.maxLength = 1;
    keyInput.placeholder = isEn() ? "letter" : "字母";
    keyInput.value = prog.key || "";
  }

  const nameInput = document.createElement("input");
  nameInput.className = "name-input";
  nameInput.placeholder = isEn() ? "Name" : "名称";
  nameInput.value = prog.name || "";

  const confirmBtn = document.createElement("button");
  confirmBtn.className = "confirm-btn";
  confirmBtn.textContent = t("setSave");

  const blockBtn = document.createElement("button");
  blockBtn.className = "block-btn";
  blockBtn.textContent = isEn() ? "Block" : "屏蔽";
  blockBtn.title = isEn()
    ? "Hide this program from the list (unblock in settings)"
    : "从此列表中隐藏该程序（设置页可解除）";
  blockBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    invoke("block_program", { process: prog.process, note: nameInput.value.trim() || prog.name || "" })
      .then(() => {
        panel.remove();
        btn.style.display = "";
      });
  });

  // 删除按钮：仅已配置程序显示。移除已存名称/代号（不加黑名单）——未运行即从列表消失，
  // 运行中仍作为未配置项（·）出现可重新配；与「屏蔽」（彻底隐藏）区分
  let delBtn = null;
  if (prog.configured) {
    delBtn = document.createElement("button");
    delBtn.className = "block-btn";
    delBtn.textContent = isEn() ? "Delete" : "删除";
    delBtn.title = isEn()
      ? "Remove its saved name/code: hidden when not running, still shows (unconfigured) while running. Use Block to hide it entirely."
      : "移除已存名称/代号：未运行时从列表消失，运行中仍以未配置（·）显示可重配；想彻底隐藏用「屏蔽」";
    delBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      invoke("delete_program", { process: prog.process }).then(() => {
        panel.remove();
        btn.style.display = "";
      });
    });
  }

  // 单字母模式显示可用字母提示；多字母模式提示代号规则
  let hint;
  if (!multi) {
    const used = new Set(state.programs.filter((p) => p.configured).map((p) => p.key));
    const free = "abcdefghijklmnopqrstuvwxyz"
      .split("")
      .filter((c) => !used.has(c) || c === prog.key);
    hint = document.createElement("span");
    hint.className = "free-hint";
    hint.appendChild(document.createTextNode(isEn() ? "Free letters: " : "可用字母: "));
    for (const c of free) {
      const b = document.createElement("b");
      b.textContent = c;
      b.addEventListener("click", (e) => {
        e.stopPropagation();
        keyInput.value = c;
        keyInput.focus();
      });
      hint.appendChild(b);
      hint.appendChild(document.createTextNode(" "));
    }
    const clearTip = document.createElement("span");
    clearTip.className = "clear-hint";
    clearTip.textContent = isEn() ? "(empty = clear letter)" : "（留空=清除字母）";
    hint.appendChild(clearTip);
  } else {
    hint = document.createElement("span");
    hint.className = "free-hint";
    hint.textContent = isEn()
      ? "Multi-letter code (e.g. ch, vs); matched before the name. Leave empty to match by name only."
      : "多字母代号（如 ch、vs），匹配时优先于软件名；留空则只按名称匹配";
  }

  const save = () => {
    const rawKey = keyInput.value.toLowerCase().replace(/[^a-z]/g, "");
    const v = nameInput.value.trim();
    // 单字母模式：非空必须恰 1 个字母，留空=清除字母绑定；多字母模式可留空（只按名称匹配）
    if (!multi && rawKey.length > 1) {
      keyInput.classList.add("err");
      setTimeout(() => keyInput.classList.remove("err"), 2000);
      return;
    }
    if (!v) {
      nameInput.classList.add("err");
      setTimeout(() => nameInput.classList.remove("err"), 2000);
      return;
    }
    invoke("edit_program", {
      process: prog.process,
      key: rawKey,
      multi,
      name: v,
    }).catch((err) => {
      keyInput.classList.add("err");
      nameInput.classList.add("err");
      keyInput.placeholder = String(err);
      setTimeout(() => {
        keyInput.classList.remove("err");
        nameInput.classList.remove("err");
      }, 2000);
    });
  };

  const onKey = (e) => {
    e.stopPropagation();
    if (e.key === "Enter") save();
    else if (e.key === "Escape") {
      panel.remove();
      btn.style.display = "";
    }
  };
  keyInput.addEventListener("keydown", onKey);
  nameInput.addEventListener("keydown", onKey);
  confirmBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    save();
  });

  fields.appendChild(keyInput);
  fields.appendChild(nameInput);
  fields.appendChild(confirmBtn);
  fields.appendChild(blockBtn);
  if (delBtn) fields.appendChild(delBtn);
  panel.appendChild(fields);
  panel.appendChild(hint);
  btn.style.display = "none";
  row.insertAdjacentElement("afterend", panel);
  nameInput.focus();
  nameInput.select();
}

let hoverIdx = null;
let lastWinKey = null;

// ===== 覆盖层卡片自适应缩放 + DWM 缩略图布局（行缩略图与大预览均由后端 DWM 合成） =====
function dpr() {
  return window.devicePixelRatio || 1;
}

// 程序层工具条高度缓存：窗口层无工具条，但两层共用同一 --ui-scale，
// 行区可用高度统一按「#list 高 - 工具条高」计算，避免进出窗口层缩放跳动
let toolbarH = 0;

// 行区固定开销：#list 的 padding-top 10px（内容盒之外的唯一固定项；
// 工具条与首行、各行之间的 n 个 gap 已包含在 cardScale 的 44n 槽位内）
const ROWS_FIXED_OVERHEAD = 10;

// 行区可用高度：#list 高固定为 calc(100vh - 124px)，内容区再减 padding-top 10 与工具条。
// 设置页打开时覆盖层被隐藏（测不到 DOM），故一律按视口推算而非读 clientHeight；
// 工具条高沿用最近一次覆盖层渲染的实测缓存（窗口层/设置页都复用，保证同屏同区间）
function rowsAvailHeight() {
  const toolbar = listEl.querySelector(".toolbar");
  if (toolbar) toolbarH = toolbar.offsetHeight;
  if (!toolbarH) toolbarH = 42; // 首次尚未渲染过覆盖层时的兜底
  return window.innerHeight - 124 - toolbarH - ROWS_FIXED_OVERHEAD;
}

// 计算 --ui-scale 并写入 #overlay-view（仅覆盖层两层消费）。
// 关键：用户设置的页长只是偏好，先按当前屏幕可行区间（clampPageSize）钳出生效页长，
// 生效页长不同则下发 set_page_size 让后端分页对齐（触发一次重发渲染后即收敛），
// 因此 scale 永不触顶/封底，N 行总高恰好铺满行区——不会出现卡片放到最大仍空半屏。
function applyUiScale() {
  if (!state || !state.visible || settingsOpen || helpOpen) return;
  const preferred = state.prog_page_size || PROG_PAGE_SIZE_DEFAULT;
  const avail = rowsAvailHeight();
  const n = clampPageSize(preferred, avail);
  overlayView.style.setProperty("--ui-scale", String(cardScale(avail, n)));
  if (n !== preferred) {
    state.prog_page_size = n; // 先本地收敛，避免重发渲染前的重复 invoke
    invoke("set_page_size", { n }).catch(() => {});
  }
}

// render 后统一重排：先写缩放变量，下一帧（新尺寸生效后）再测量缩略图/滚动选中行进区
let layoutRaf = 0;
function scheduleOverlayLayout() {
  applyUiScale();
  if (layoutRaf) return;
  layoutRaf = requestAnimationFrame(() => {
    layoutRaf = 0;
    scrollActiveIntoView();
    layoutThumbs();
    updatePreview();
  });
}

// 元素物理 rect + 与滚动容器的可视裁剪 rect（ax/ay/aw/ah 为 0 表示不裁剪）
function thumbRects(el, clipEl) {
  const d = dpr();
  const r = el.getBoundingClientRect();
  const full = rectPhys(r, d);
  if (!clipEl) return { ...full, ax: 0, ay: 0, aw: 0, ah: 0 };
  return { ...full, ...clipRectPhys(r, clipEl.getBoundingClientRect(), d) };
}

// 按当前 DOM 布局注册/更新全部行缩略图（列表重建、滚动后调用）
function layoutThumbs() {
  if (!state || state.phase !== "windows") return;
  const scrollEl = listEl.querySelector(".wlist");
  for (const row of listEl.querySelectorAll(".wrow[data-hwnd]")) {
    const img = row.querySelector(".wthumb");
    if (!img) continue;
    const t = thumbRects(img, scrollEl);
    invoke("thumb_set", {
      slot: "row:" + row.dataset.hwnd,
      hwnd: Number(row.dataset.hwnd),
      x: t.x,
      y: t.y,
      w: t.w,
      h: t.h,
      ax: t.ax,
      ay: t.ay,
      aw: t.aw,
      ah: t.ah,
    }).catch(() => {});
  }
}

// 选中行自动滚动进可视区（窗口多时滚动条跟随选中）
function scrollActiveIntoView() {
  const active = listEl.querySelector(".wrow.active");
  if (active) active.scrollIntoView({ block: "nearest" });
}

// 右侧大预览：DWM 实时缩略图（目标 = 悬停行优先，否则选中行），后端等比 contain 到预览框
let previewTarget = null;
async function updatePreview() {
  const img = document.getElementById("preview-img");
  if (!img || !state || state.phase !== "windows") return;
  const idx = hoverIdx !== null ? hoverIdx : state.windows.findIndex((w) => w.active);
  const target = state.windows[Math.max(0, idx)];
  if (!target) return;
  previewTarget = target.hwnd;
  const r = thumbRects(img, null);
  invoke("thumb_set", {
    slot: "pane",
    hwnd: target.hwnd,
    x: r.x,
    y: r.y,
    w: r.w,
    h: r.h,
    ax: 0,
    ay: 0,
    aw: 0,
    ah: 0,
  }).catch(() => {});
}

// 悬停联动：预览跟随悬停行
listEl.addEventListener("mouseover", (e) => {
  const row = e.target.closest(".wrow");
  if (!row || !state || state.phase !== "windows") return;
  hoverIdx = Number(row.dataset.idx) - 1;
  updatePreview();
});

listEl.addEventListener("mouseleave", () => {
  if (hoverIdx !== null) {
    hoverIdx = null;
    updatePreview();
  }
});

document.getElementById("quit-btn").addEventListener("click", () => {
  invoke("quit_app");
});

document.getElementById("github-btn").addEventListener("click", () => {
  invoke("open_url", { url: "https://github.com/cnecoder/WinHop" });
});

function render(s) {
  state = s;
  // 主题以后端配置为准（保存后/启动时同步）
  if (s.theme) applyTheme(s.theme);
  if (!s.visible) {
    appEl.style.display = "none";
    if (hkListening) endHotkeyCapture(); // 录制中：停检测
    if (settingsOpen) resumeHotkey(); // 覆盖层关闭带走设置页：恢复 suspend 的热键
    settingsOpen = false;
    settingsView.hidden = true;
    helpOpen = false; // 覆盖层关闭带走帮助页
    helpView.hidden = true;
    overlayView.hidden = false;
    confirmMask.hidden = true;
    return;
  }
  appEl.style.display = "block";
  // 设置页/帮助页打开时不刷新覆盖层（它们被隐藏）；关闭后由新事件覆盖
  if (settingsOpen || helpOpen) return;
  renderHeader(s);
  if (s.phase === "windows") {
    titleEl.textContent = winHint(s);
    const winKey = s.windows.map((w) => w.hwnd).join(",");
    if (lastWinKey === winKey && listEl.querySelector(".wrow")) {
      // 同一批窗口：只更新选中态，避免整列表重建导致预览闪烁
      listEl.querySelectorAll(".wrow").forEach((row, i) => {
        row.classList.toggle("active", !!(s.windows[i] && s.windows[i].active));
      });
      titleEl.textContent = winHint(s);
      scheduleOverlayLayout();
    } else {
      lastWinKey = winKey;
      listEl.className = "window-layer";
      listEl.innerHTML =
        `<div class="wlist">` +
        s.windows
          .map(
            (w) =>
              `<div class="wrow${w.active ? " active" : ""}" data-idx="${w.index}" data-hwnd="${w.hwnd}">` +
              `<div class="wtop">` +
              `<span class="key">${w.index}</span>` +
              `<span class="name">${escapeHtml(w.title)}</span>` +
              `<span class="screen">${t("screen", { n: w.screen + 1 })}</span>` +
              `</div>` +
              `<img class="wthumb" alt="" />` +
              `</div>`
          )
          .join("") +
        `</div>` +
        `<div class="wpreview"><img id="preview-img" alt="" /></div>`;
      scheduleOverlayLayout();
    }
  } else {
    lastWinKey = null;
    previewTarget = null;
    invoke("thumb_clear");
    listEl.className = "";
    titleEl.textContent = "WinHop";
    // 多字母筛选无匹配：显示空状态提示，且不显示翻页
    const noMatch = s.multi_letter && s.filter && s.programs.length === 0;
    // 工具条：左侧筛选（多字母模式），右侧分页/翻页提示，同一行两端对齐
    const filterLeft = s.multi_letter
      ? `<div class="filter-bar">
           <span class="filter-label">${t("filterLabel")}</span>
           <span class="filter-box${s.filter ? " active" : ""}">${escapeHtml(s.filter)}<span class="caret">▏</span></span>
           <span class="filter-hint">${t("filterHint")}</span>
         </div>`
      : `<span></span>`;
    const pageRight = noMatch
      ? ""
      : s.page_count > 1
        ? t("pageOf", { a: s.page, b: s.page_count })
        : t("pager");
    const emptyHint = noMatch
      ? `<div class="empty-hint"><span class="key key-empty">·</span><span>${t("noMatch", { q: escapeHtml(s.filter) })}</span></div>`
      : "";
    listEl.innerHTML =
      `<div class="toolbar">${filterLeft}<span class="pager">${pageRight}</span></div>` +
      emptyHint +
      s.programs
        .map(
          (p) => {
            const hasKey = p.key && p.key.length > 0;
            // 有字母：运行中=高亮 key-cfg，未运行=置灰 key-off；无字母=占位 key-empty
            const keyCls = !hasKey
              ? "key key-empty"
              : p.running
                ? "key key-cfg"
                : "key key-off";
            const wide = p.key && p.key.length > 1 ? " key-wide" : "";
            // PWA 虚拟进程（pwa#<app-id>，与浏览器同 exe）不显示内部键，只标 PWA
            const procLabel = p.process.startsWith(PWA_PROC_PREFIX)
              ? "PWA"
              : p.process;
            return (
              `<div class="row${p.active ? " active" : ""}${p.running ? "" : " off"}" data-key="${escapeHtml(p.key)}" data-process="${escapeHtml(p.process)}">` +
              `<span class="key-slot"><span class="${keyCls}${wide}">${hasKey ? escapeHtml(p.key) : "·"}</span></span>` +
              `<span class="name">${escapeHtml(p.name)} (${escapeHtml(procLabel)})</span>` +
              `<span class="screen">${p.running ? "×" + p.count : t("notRunning")}</span>` +
              `<button class="edit-btn" title="${t("edit")}">✎</button>` +
              `</div>`
            );
          }
        )
        .join("");
    scheduleOverlayLayout();
  }
}

// 启动：按系统/已保存的语言设定界面语言
invoke("get_settings")
  .then((info) => {
    // lang_cfg：配置保存的语言（空=跟随系统）；lang_sys：系统检测（与用户设置无关）
    sysLang = info.lang_sys || "zh-CN";
    currentHotkey = info.hotkey || "ctrl+space";
    const saved = info.lang_cfg || ""; // 空 = 跟随系统
    applyLanguage(saved || "system");
  })
  .catch(() => applyLanguage("system"));

listen("overlay", (e) => render(e.payload));

// 行缩略图随滚动重排（capture 捕获 .wlist 自身滚动，列表重建后无需重绑）
listEl.addEventListener("scroll", () => requestAnimationFrame(layoutThumbs), true);

// 视口变化（分辨率切换 / F11 窗口模式 / 主屏 DPI 调整）后重算缩放与缩略图，rAF 合并
let relayoutRaf = 0;
function requestOverlayRelayout() {
  if (relayoutRaf) return;
  relayoutRaf = requestAnimationFrame(() => {
    relayoutRaf = 0;
    scheduleOverlayLayout();
  });
}
window.addEventListener("resize", requestOverlayRelayout);
new ResizeObserver(requestOverlayRelayout).observe(listEl);

// DPR 变化监听：matchMedia 当前 dppx 的一次性查询，触发后按新 dpr 重建
function watchDpr() {
  const mq = window.matchMedia(`(resolution: ${dpr()}dppx)`);
  mq.addEventListener(
    "change",
    () => {
      watchDpr();
      requestOverlayRelayout();
    },
    { once: true }
  );
}
watchDpr();
