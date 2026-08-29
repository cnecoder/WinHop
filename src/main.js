const { listen } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;

const appEl = document.getElementById("app");
const titleEl = document.getElementById("title");
const listEl = document.getElementById("list");
const overlayView = document.getElementById("overlay-view");
const settingsView = document.getElementById("settings-view");
const confirmMask = document.getElementById("confirm-mask");

let settingsOpen = false;
let state = null;
let settingsLoaded = null; // 已保存的设置快照，用于判断是否改动

function escapeHtml(s) {
  return s.replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])
  );
}

// 应用主题：写到 <html data-theme>，CSS 变量块据此切换
function applyTheme(id) {
  document.documentElement.dataset.theme = id || "black-green";
}

// 读取设置页表单当前值
function readSettingsForm() {
  const order = document.querySelector('input[name="order"]:checked');
  const theme = document.querySelector('input[name="theme"]:checked');
  return {
    window_order: order ? order.value : "zorder",
    multi_letter: document.getElementById("multi-letter").checked,
    theme: theme ? theme.value : "black-green",
  };
}

// 设置是否有未保存改动
function settingsDirty() {
  if (!settingsLoaded) return false;
  const cur = readSettingsForm();
  return (
    cur.window_order !== settingsLoaded.window_order ||
    cur.multi_letter !== settingsLoaded.multi_letter ||
    cur.theme !== settingsLoaded.theme
  );
}

// 更新保存按钮可用状态与状态文案
function updateSettingsState() {
  const dirty = settingsDirty();
  const saveBtn = document.getElementById("settings-save");
  saveBtn.disabled = !dirty;
  document.getElementById("settings-status").textContent = dirty
    ? "有未保存的修改"
    : "";
}

// 打开设置页：拉取设置、填表单、显示版本与更新记录
async function openSettings() {
  const info = await invoke("get_settings");
  settingsLoaded = {
    window_order: info.window_order,
    multi_letter: info.multi_letter,
    theme: info.theme,
  };
  document.querySelectorAll('input[name="order"]').forEach((r) => {
    r.checked = r.value === info.window_order;
  });
  document.getElementById("multi-letter").checked = !!info.multi_letter;
  // 主题单选项由后端主题表动态生成
  const themeBox = document.getElementById("theme-options");
  themeBox.innerHTML = (info.themes || [])
    .map(
      (t) =>
        `<label class="setting-row"><input type="radio" name="theme" value="${escapeHtml(
          t.id
        )}"${t.id === info.theme ? " checked" : ""} /><span>${escapeHtml(
          t.name
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
  document.getElementById("version-info").textContent = "版本 " + info.version;
  const e = info.changelog;
  document.getElementById("changelog").innerHTML =
    `<div class="changelog-entry">
      <div class="changelog-ver">${escapeHtml(e.version)} <span class="changelog-date">${escapeHtml(e.date)}</span></div>
      <ul>${e.notes.map((n) => `<li>${escapeHtml(n)}</li>`).join("")}</ul>
    </div>`;
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

// 返回/ESC 时若有未保存改动则弹确认
function requestCloseSettings() {
  if (settingsDirty()) {
    confirmMask.hidden = false;
  } else {
    closeSettings();
  }
}

async function saveSettingsAndClose() {
  const input = readSettingsForm();
  await invoke("save_settings", { input });
  settingsLoaded = input;
  updateSettingsState();
  closeSettings();
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
    badge.textContent = "多字母";
  } else {
    badge.className = "mode-badge mode-single";
    badge.textContent = "单字母";
  }
  legend.innerHTML =
    `<span class="legend-item"><span class="key key-cfg demo"></span>已配置·运行</span>` +
    `<span class="legend-item"><span class="key key-auto demo"></span>动态检测</span>` +
    `<span class="legend-item"><span class="key key-off demo"></span>已配置·未运行</span>`;
}

// 覆盖层夺焦后按键落在本 webview 内部（raw input 内部可达），路由给 Rust 状态机。
// LL 键盘钩子对 Chromium 前台无效（raw input 绕过钩子链），这是唯一的按键路径。
window.addEventListener("keydown", (e) => {
  // 设置页按键独立处理
  if (settingsOpen) {
    if (e.key === "Escape") {
      e.preventDefault();
      if (confirmMask.hidden) requestCloseSettings();
      return;
    }
    // 表单内的按键（radio/checkbox）正常处理，不路由
    return;
  }
  // 覆盖层中编辑程序的输入框：按键不参与快捷键路由
  if (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA") return;
  e.preventDefault();
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
  else if (e.ctrlKey && e.code === "Space") k = "hotkey";
  else if (/^[a-zA-Z]$/.test(e.key)) k = "letter:" + e.key.toLowerCase();
  else if (/^[0-9]$/.test(e.key)) k = "digit:" + e.key;
  if (k) invoke("key", { k });
});

document.getElementById("settings-btn").addEventListener("click", () => {
  openSettings();
});

document.getElementById("settings-back").addEventListener("click", () => {
  requestCloseSettings();
});

document.querySelectorAll('input[name="order"], #multi-letter').forEach((el) => {
  el.addEventListener("change", updateSettingsState);
});

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
  closeSettings();
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
    invoke("key", { k: "digit:" + row.dataset.idx });
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
    keyInput.placeholder = "多字母代号";
    keyInput.value = prog.multi_key || "";
  } else {
    keyInput.maxLength = 1;
    keyInput.placeholder = "字母";
    keyInput.value = prog.key || "";
  }

  const nameInput = document.createElement("input");
  nameInput.className = "name-input";
  nameInput.placeholder = "名称";
  nameInput.value = prog.name || "";

  const confirmBtn = document.createElement("button");
  confirmBtn.className = "confirm-btn";
  confirmBtn.textContent = "保存";

  // 单字母模式显示可用字母提示；多字母模式提示代号规则
  let hint;
  if (!multi) {
    const used = new Set(state.programs.filter((p) => p.configured).map((p) => p.key));
    const free = "abcdefghijklmnopqrstuvwxyz"
      .split("")
      .filter((c) => !used.has(c) || c === prog.key);
    hint = document.createElement("span");
    hint.className = "free-hint";
    hint.appendChild(document.createTextNode("可用字母: "));
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
  } else {
    hint = document.createElement("span");
    hint.className = "free-hint";
    hint.textContent = "多字母代号（如 ch、vs），匹配时优先于软件名；留空则只按名称匹配";
  }

  const save = () => {
    const rawKey = keyInput.value.toLowerCase().replace(/[^a-z]/g, "");
    const v = nameInput.value.trim();
    // 单字母模式必须 1 字符；多字母模式可留空（只按名称匹配），非空则 1+ 字母
    if (!multi && rawKey.length !== 1) {
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
  panel.appendChild(fields);
  panel.appendChild(hint);
  btn.style.display = "none";
  row.insertAdjacentElement("afterend", panel);
  nameInput.focus();
  nameInput.select();
}

let hoverIdx = null;
let capturing = false;
let lastWinKey = null;

// 缩略图捕获：DWM Thumbnail 管道（win+tab 同款），每个窗口独立捕获，
// 颜色正确、重叠窗口互不影响、无闪烁。进入窗口层时捕获一次（静态）。
async function captureAll() {
  if (capturing) return;
  capturing = true;
  try {
    const rows = [...document.querySelectorAll(".wrow[data-hwnd]")];
    const results = await Promise.all(
      rows.map(async (row) => {
        try {
          const url = await invoke("window_thumbnail", {
            hwnd: Number(row.dataset.hwnd),
            maxW: 960,
            maxH: 540,
          });
          return { row, url };
        } catch (err) {
          return { row, url: "" };
        }
      })
    );
    for (const { row, url } of results) {
      const img = row.querySelector("img.wthumb");
      if (img) img.src = url;
    }
    updatePreview();
  } finally {
    capturing = false;
  }
}

// 选中行自动滚动进可视区（窗口多时滚动条跟随选中）
function scrollActiveIntoView() {
  const active = listEl.querySelector(".wrow.active");
  if (active) active.scrollIntoView({ block: "nearest" });
}

// 右侧大预览：独立按 1920px 高清捕获（目标 = 悬停行优先，否则选中行）。
// 行缩略图只有 960px，直接拉伸到大预览会模糊。
let previewTarget = null;
async function updatePreview() {
  const img = document.getElementById("preview-img");
  if (!img || !state || state.phase !== "windows") return;
  const idx = hoverIdx !== null ? hoverIdx : state.windows.findIndex((w) => w.active);
  const target = state.windows[Math.max(0, idx)];
  if (!target || previewTarget === target.hwnd) return;
  previewTarget = target.hwnd;
  img.dataset.hwnd = String(target.hwnd);
  try {
    const url = await invoke("window_thumbnail", {
      hwnd: target.hwnd,
      maxW: 1920,
      maxH: 1080,
    });
    if (img && img.dataset.hwnd === String(target.hwnd)) img.src = url;
  } catch (err) {
    // 捕获失败：保留上一张
  }
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

function render(s) {
  state = s;
  // 主题以后端配置为准（保存后/启动时同步）
  if (s.theme) applyTheme(s.theme);
  if (!s.visible) {
    appEl.style.display = "none";
    settingsOpen = false;
    settingsView.hidden = true;
    overlayView.hidden = false;
    confirmMask.hidden = true;
    return;
  }
  appEl.style.display = "block";
  // 设置页打开时不刷新覆盖层（它被隐藏）；关闭设置后由新事件覆盖
  if (settingsOpen) return;
  renderHeader(s);
  if (s.phase === "windows") {
    titleEl.textContent = s.title;
    const winKey = s.windows.map((w) => w.hwnd).join(",");
    if (lastWinKey === winKey && listEl.querySelector(".wrow")) {
      // 同一批窗口：只更新选中态，避免整列表重建导致预览闪烁
      listEl.querySelectorAll(".wrow").forEach((row, i) => {
        row.classList.toggle("active", !!(s.windows[i] && s.windows[i].active));
      });
      scrollActiveIntoView();
      updatePreview();
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
              `<span class="screen">屏${w.screen + 1}</span>` +
              `</div>` +
              `<img class="wthumb" alt="" />` +
              `</div>`
          )
          .join("") +
        `</div>` +
        `<div class="wpreview"><img id="preview-img" alt="" /></div>`;
      captureAll();
      updatePreview();
    }
  } else {
    lastWinKey = null;
    listEl.className = "";
    titleEl.textContent = "WinHop";
    // 多字母筛选无匹配：显示空状态提示，且不显示翻页
    const noMatch = s.multi_letter && s.filter && s.programs.length === 0;
    // 工具条：左侧筛选（多字母模式），右侧分页/翻页提示，同一行两端对齐
    const filterLeft = s.multi_letter
      ? `<div class="filter-bar">
           <span class="filter-label">筛选</span>
           <span class="filter-box${s.filter ? " active" : ""}">${escapeHtml(s.filter)}<span class="caret">▏</span></span>
           <span class="filter-hint">Enter 确认，Esc 清除，Backspace 删除</span>
         </div>`
      : `<span></span>`;
    const pageRight = noMatch
      ? ""
      : s.page_count > 1
        ? `第 ${s.page} / ${s.page_count} 页 · PageUp/PageDown 翻页`
        : `PageUp/PageDown 翻页`;
    const emptyHint = noMatch
      ? `<div class="empty-hint"><span class="key key-empty">·</span><span>无匹配「${escapeHtml(s.filter)}」，Esc 清除筛选</span></div>`
      : "";
    listEl.innerHTML =
      `<div class="toolbar">${filterLeft}<span class="pager">${pageRight}</span></div>` +
      emptyHint +
      s.programs
        .map(
          (p) => {
            const hasKey = p.key && p.key.length > 0;
            const keyCls = !hasKey
              ? "key key-empty"
              : p.configured
                ? p.running
                  ? "key key-cfg"
                  : "key key-off"
                : "key key-auto";
            const wide = p.key && p.key.length > 1 ? " key-wide" : "";
            return (
              `<div class="row${p.active ? " active" : ""}${p.running ? "" : " off"}" data-key="${escapeHtml(p.key)}" data-process="${escapeHtml(p.process)}">` +
              `<span class="${keyCls}${wide}">${hasKey ? escapeHtml(p.key) : "·"}</span>` +
              `<span class="name">${escapeHtml(p.name)} (${escapeHtml(p.process)})</span>` +
              `<span class="screen">${p.running ? "×" + p.count : "未运行"}</span>` +
              `<button class="edit-btn" title="编辑">✎</button>` +
              `</div>`
            );
          }
        )
        .join("");
  }
}

listen("overlay", (e) => render(e.payload));
