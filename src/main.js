const { listen } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;

const appEl = document.getElementById("app");
const titleEl = document.getElementById("title");
const listEl = document.getElementById("list");
const settingsEl = document.getElementById("settings");

let settingsOpen = false;

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

function render(state) {
  if (!state.visible) {
    appEl.style.display = "none";
    settingsOpen = false;
    settingsEl.hidden = true;
    return;
  }
  appEl.style.display = "block";
  renderSettings(state);
  if (state.phase === "windows") {
    titleEl.textContent = state.title;
    listEl.innerHTML = state.windows
      .map(
        (w) =>
          `<div class="row${w.active ? " active" : ""}">` +
          `<span class="key">${w.index}</span>` +
          `<span class="name">${escapeHtml(w.title)}</span>` +
          `<span class="screen">屏${w.screen + 1}</span>` +
          `</div>`
      )
      .join("");
  } else {
    titleEl.textContent = "WinTab";
    listEl.innerHTML = state.programs
      .map(
        (p) =>
          `<div class="row${p.active ? " active" : ""}${p.running ? "" : " off"}">` +
          `<span class="key">${p.key}</span>` +
          `<span class="name">${escapeHtml(p.name)}</span>` +
          `<span class="screen">${p.running ? "×" + p.count : "未运行"}</span>` +
          `</div>`
      )
      .join("");
  }
}

listen("overlay", (e) => render(e.payload));
