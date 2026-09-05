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
