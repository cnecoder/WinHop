const { listen } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;

const appEl = document.getElementById("app");
const titleEl = document.getElementById("title");
const listEl = document.getElementById("list");
const settingsEl = document.getElementById("settings");

let settingsOpen = false;
let state = null;

function escapeHtml(s) {
  return s.replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])
  );
}

function renderSettings(state) {
  const order = state.window_order;
  document.querySelectorAll('input[name="order"]').forEach((r) => {
    r.checked = r.value === order;
  });
}

// 覆盖层夺焦后按键落在本 webview 内部（raw input 内部可达），路由给 Rust 状态机。
// LL 键盘钩子对 Chromium 前台无效（raw input 绕过钩子链），这是唯一的按键路径。
window.addEventListener("keydown", (e) => {
  e.preventDefault();
  if (e.key === "F2") {
    settingsOpen = !settingsOpen;
    settingsEl.hidden = !settingsOpen;
    return;
  }
  if (e.key === "F11") {
    invoke("toggle_fullscreen");
    return;
  }
  if (settingsOpen) {
    if (e.key === "Escape") {
      settingsOpen = false;
      settingsEl.hidden = true;
    }
    return;
  }
  let k = null;
  if (e.key === "Escape") k = "esc";
  else if (e.key === "ArrowUp") k = "up";
  else if (e.key === "ArrowDown") k = "down";
  else if (e.key === "Enter") k = "enter";
  else if (e.ctrlKey && e.code === "Space") k = "hotkey";
  else if (/^[a-z]$/.test(e.key)) k = "letter:" + e.key;
  else if (/^[0-9]$/.test(e.key)) k = "digit:" + e.key;
  if (k) invoke("key", { k });
});

document.getElementById("settings-btn").addEventListener("click", () => {
  settingsOpen = !settingsOpen;
  settingsEl.hidden = !settingsOpen;
});

// 鼠标点击选择：复用键盘路径——程序层按字母、窗口层按编号
listEl.addEventListener("click", (e) => {
  const row = e.target.closest(".row, .wrow");
  if (!row) return;
  if (state && state.phase === "windows" && row.dataset.idx) {
    invoke("key", { k: "digit:" + row.dataset.idx });
  } else if (state && state.phase === "programs" && row.dataset.key) {
    invoke("key", { k: "letter:" + row.dataset.key });
  }
});

let thumbTimer = null;
let hoverIdx = null;

// 右侧大缩略图预览：目标 = 悬停行（优先）或当前选中行，每 2 秒刷新
async function loadPreview() {
  if (!state || state.phase !== "windows") return;
  const idx = hoverIdx !== null ? hoverIdx : state.windows.findIndex((w) => w.active);
  const target = state.windows[Math.max(0, idx)];
  if (!target) return;
  const img = document.getElementById("preview-img");
  if (!img) return;
  if (img.dataset.hwnd !== String(target.hwnd)) {
    img.dataset.hwnd = String(target.hwnd);
    img.src = "";
  }
  try {
    const url = await invoke("window_thumbnail", {
      hwnd: target.hwnd,
      maxW: 640,
      maxH: 420,
    });
    if (img.dataset.hwnd === String(target.hwnd)) img.src = url;
  } catch (err) {
    img.src = "";
  }
}

function startThumbs() {
  stopThumbs();
  loadPreview();
  thumbTimer = setInterval(loadPreview, 2000);
}

function stopThumbs() {
  if (thumbTimer) {
    clearInterval(thumbTimer);
    thumbTimer = null;
  }
}

// 悬停联动：鼠标移入某行预览该窗口，移出列表回到选中行
listEl.addEventListener("mouseover", (e) => {
  const row = e.target.closest(".wrow");
  if (!row || !state || state.phase !== "windows") return;
  hoverIdx = Number(row.dataset.idx) - 1;
  loadPreview();
});

listEl.addEventListener("mouseleave", () => {
  if (hoverIdx !== null) {
    hoverIdx = null;
    loadPreview();
  }
});

document.getElementById("fullscreen-btn").addEventListener("click", () => {
  invoke("toggle_fullscreen");
});

document.querySelectorAll('input[name="order"]').forEach((r) => {
  r.addEventListener("change", () => {
    if (r.checked) {
      invoke("set_window_order", { order: r.value }).catch((e) =>
        console.error("设置保存失败:", e)
      );
    }
  });
});

document.getElementById("quit-btn").addEventListener("click", () => {
  invoke("quit_app");
});

function render(s) {
  state = s;
  if (!s.visible) {
    appEl.style.display = "none";
    settingsOpen = false;
    settingsEl.hidden = true;
    stopThumbs();
    return;
  }
  appEl.style.display = "block";
  renderSettings(s);
  if (s.phase === "windows") {
    titleEl.textContent = s.title;
    listEl.className = "window-layer";
    listEl.innerHTML =
      `<div class="wlist">` +
      s.windows
        .map(
          (w) =>
            `<div class="wrow${w.active ? " active" : ""}" data-idx="${w.index}" data-hwnd="${w.hwnd}">` +
            `<span class="key">${w.index}</span>` +
            `<span class="name">${escapeHtml(w.title)}</span>` +
            `<span class="screen">屏${w.screen + 1}</span>` +
            `</div>`
        )
        .join("") +
      `</div>` +
      `<div class="wpreview"><img id="preview-img" alt="" /></div>`;
    startThumbs();
  } else {
    listEl.className = "";
    stopThumbs();
    titleEl.textContent = "WinTab";
    titleEl.textContent = "WinTab";
    listEl.innerHTML = s.programs
      .map(
        (p) =>
          `<div class="row${p.active ? " active" : ""}${p.running ? "" : " off"}" data-key="${p.key}">` +
          `<span class="key">${p.key}</span>` +
          `<span class="name">${escapeHtml(p.name)}</span>` +
          `<span class="screen">${p.running ? "×" + p.count : "未运行"}</span>` +
          `</div>`
      )
      .join("");
  }
}

listen("overlay", (e) => render(e.payload));
