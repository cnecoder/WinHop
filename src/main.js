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
  const ml = document.getElementById("multi-letter");
  if (ml) ml.checked = !!state.multi_letter;
}

// 覆盖层夺焦后按键落在本 webview 内部（raw input 内部可达），路由给 Rust 状态机。
// LL 键盘钩子对 Chromium 前台无效（raw input 绕过钩子链），这是唯一的按键路径。
window.addEventListener("keydown", (e) => {
  // 输入框（+ 号配置字母）按键不参与快捷键路由
  if (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA") return;
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
  else if (e.key === "PageUp") k = "pageup";
  else if (e.key === "PageDown") k = "pagedown";
  else if (e.key === "Backspace") k = "back";
  else if (e.key === "Enter") k = "enter";
  else if (e.ctrlKey && e.code === "Space") k = "hotkey";
  else if (/^[a-zA-Z]$/.test(e.key)) k = "letter:" + e.key.toLowerCase();
  else if (/^[0-9]$/.test(e.key)) k = "digit:" + e.key;
  if (k) invoke("key", { k });
});

document.getElementById("settings-btn").addEventListener("click", () => {
  settingsOpen = !settingsOpen;
  settingsEl.hidden = !settingsOpen;
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
  } else if (state && state.phase === "programs" && row.dataset.key) {
    invoke("key", { k: "letter:" + row.dataset.key });
  }
});

// 统一编辑面板：行下方弹字母 + 名称框，Enter 保存（已配置改键/名，未配置入配置）
function startEditProgram(btn) {
  const row = btn.closest(".row");
  const prog = state.programs.find((p) => p.key === row.dataset.key) || {};
  if (!prog.process) return;

  const panel = document.createElement("div");
  panel.className = "add-panel";

  const fields = document.createElement("div");
  fields.className = "add-fields";

  const keyInput = document.createElement("input");
  keyInput.className = "key-input";
  keyInput.maxLength = 1;
  keyInput.placeholder = "字母";
  keyInput.value = prog.key || "";

  const nameInput = document.createElement("input");
  nameInput.className = "name-input";
  nameInput.placeholder = "名称";
  nameInput.value = prog.name || "";

  const confirmBtn = document.createElement("button");
  confirmBtn.className = "confirm-btn";
  confirmBtn.textContent = "保存";

  // 可用字母：a-z 减去其它已配置程序占用的字母（自动补全的临时字母不算占用）
  const used = new Set(state.programs.filter((p) => p.configured).map((p) => p.key));
  const free = "abcdefghijklmnopqrstuvwxyz"
    .split("")
    .filter((c) => !used.has(c) || c === prog.key);
  const hint = document.createElement("span");
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

  const save = () => {
    const k = keyInput.value.toLowerCase();
    const v = nameInput.value.trim();
    if (!k) {
      keyInput.classList.add("err");
      setTimeout(() => keyInput.classList.remove("err"), 2000);
      return;
    }
    if (!v) {
      nameInput.classList.add("err");
      setTimeout(() => nameInput.classList.remove("err"), 2000);
      return;
    }
    invoke("edit_program", { process: prog.process, key: k, name: v }).catch((err) => {
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

document.querySelectorAll('input[name="order"]').forEach((r) => {
  r.addEventListener("change", () => {
    if (r.checked) {
      invoke("set_window_order", { order: r.value }).catch((e) =>
        console.error("设置保存失败:", e)
      );
    }
  });
});

document.getElementById("multi-letter").addEventListener("change", (e) => {
  invoke("set_multi_letter", { on: e.target.checked }).catch((e) =>
    console.error("设置保存失败:", e)
  );
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
    titleEl.textContent = "WinTab";
    const pager = s.filter
      ? `<div class="pager">筛选 “${escapeHtml(s.filter)}” · Enter 确认 · Esc 清除 · Backspace 删字母</div>`
      : s.page_count > 1
        ? `<div class="pager">第 ${s.page} / ${s.page_count} 页 · PageUp/PageDown 翻页</div>`
        : "";
    listEl.innerHTML =
      pager +
      s.programs
        .map(
          (p) => {
            const keyCls = p.configured
              ? p.running
                ? "key key-cfg"
                : "key key-off"
              : "key key-auto";
            return (
              `<div class="row${p.active ? " active" : ""}${p.running ? "" : " off"}" data-key="${p.key}">` +
              `<span class="${keyCls}">${p.key}</span>` +
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
