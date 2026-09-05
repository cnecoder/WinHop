// 纯函数工具集：不依赖 DOM / Tauri，可被 main.js 引用，也可被 node --test 直接单测。

// HTML 转义（渲染用户/进程名等不可信字符串前使用）
export function escapeHtml(s) {
  return String(s).replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])
  );
}

// global-shortcut 串 → 展示文本（ctrl+space → "Ctrl + Space"）
export function prettyHotkey(hk) {
  const name = { ctrl: "Ctrl", alt: "Alt", shift: "Shift", super: "Win", space: "Space" };
  return String(hk)
    .split("+")
    .map((k) => name[k] || (k.length === 1 ? k.toUpperCase() : k[0].toUpperCase() + k.slice(1)))
    .join(" + ");
}

// 设置表单两份快照是否相等：blocked 排序后比较（顺序无关），其余字段直接比较
export function settingsFormEquals(a, b) {
  const norm = (f) => ({ ...f, blocked: (f.blocked || []).slice().sort().join(",") });
  const x = norm(a || {});
  const y = norm(b || {});
  for (const k of new Set([...Object.keys(x), ...Object.keys(y)])) {
    if (x[k] !== y[k]) return false;
  }
  return true;
}

// getBoundingClientRect 结果 → 物理像素矩形（dpr 缩放）
export function rectPhys(r, dpr) {
  return {
    x: Math.round(r.left * dpr),
    y: Math.round(r.top * dpr),
    w: Math.round(r.width * dpr),
    h: Math.round(r.height * dpr),
  };
}

// 元素矩形与滚动容器矩形求交 → 物理像素可视裁剪区（无容器返回全 0，表示不裁剪）
export function clipRectPhys(elRect, clipRect, dpr) {
  if (!clipRect) return { ax: 0, ay: 0, aw: 0, ah: 0 };
  const x0 = Math.max(elRect.left, clipRect.left);
  const y0 = Math.max(elRect.top, clipRect.top);
  const x1 = Math.min(elRect.right, clipRect.right);
  const y1 = Math.min(elRect.bottom, clipRect.bottom);
  return {
    ax: Math.round(x0 * dpr),
    ay: Math.round(y0 * dpr),
    aw: Math.round((x1 - x0) * dpr),
    ah: Math.round((y1 - y0) * dpr),
  };
}

// 窗口层标题提示决策：多字母 + preview 模式下，已输入数字→'typed'，未输入→'enter'；其余 'plain'
// （返回决策码，文案 i18n 由调用方拼装，保持纯函数不依赖 t()）
export function winHintKind(multiLetter, previewMode, winDigit) {
  if (!multiLetter || !previewMode) return "plain";
  return winDigit ? "typed" : "enter";
}
