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

let hoverIdx = null;
let capturing = false;

// 缩略图捕获：覆盖层窗口透明度置 0（露出桌面）→ 并行抓所有窗口 → 恢复。
// 屏幕直捕是唯一对 Chromium 窗口颜色正确的路径（PrintWindow 输出反转，实测）。
// 静态缩略图（进入窗口层时捕获一次）。
async function captureAll() {
  if (capturing) return;
  capturing = true;
  try {
    await invoke("set_overlay_opacity", { opacity: 0 });
    await new Promise((r) => setTimeout(r, 80));
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
    await invoke("set_overlay_opacity", { opacity: 1 });
    capturing = false;
  }
}

// 右侧大预览：复用行捕获图（目标 = 悬停行优先，否则选中行），不重复捕获
function updatePreview() {
  const img = document.getElementById("preview-img");
  if (!img || !state || state.phase !== "windows") return;
  const idx = hoverIdx !== null ? hoverIdx : state.windows.findIndex((w) => w.active);
  const target = state.windows[Math.max(0, idx)];
  if (!target) return;
  const row = document.querySelector(`.wrow[data-idx="${target.index}"]`);
  const src = row ? row.querySelector("img.wthumb").src : "";
  if (img.src !== src) img.src = src;
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
  } else {
    listEl.className = "";
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
