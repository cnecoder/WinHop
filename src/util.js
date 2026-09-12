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

// 覆盖层卡片缩放：行区可用高度（CSS px，已扣除 #list padding-top 与工具条）÷ n 个卡片槽位。
// 每个槽位基准 44px = 行高 40 + 一个间距 4（flex 列里工具条与首行之间有前导 gap，
// 行间 n-1 个 gap，合计恰好 n 个 gap，末行无尾 gap），总高 = 44n。
// 结果钳制到 [min,max] 防止卡片过大/过小；非法输入回退 1（不缩放）。
export const CARD_SCALE_MIN = 0.75;
export const CARD_SCALE_MAX = 1.35;
export function cardScale(availH, n, min = CARD_SCALE_MIN, max = CARD_SCALE_MAX) {
  if (!(availH > 0) || !(n > 0)) return 1;
  const s = availH / (44 * n);
  return Math.min(max, Math.max(min, s));
}

// 配置值的绝对 sanity 区间（防止写入/推送离谱值）；真正的可选区间由屏幕高度反推
export const PAGE_SIZE_ABS_MIN = 8;
export const PAGE_SIZE_ABS_MAX = 64;

// 由卡片缩放下限/上限反推当前屏幕下每页卡片数的可行区间：
// n 个槽位总高 44n；要让 scale=avail/(44n) 落在 [0.75,1.35]，
// n_min=ceil(avail/(44·1.35))（再少卡片即使放到最大也铺不满），
// n_max=floor(avail/(44·0.75))（再多卡片即使缩到最小也放不下）。
// 区间内任意 n 都恰好铺满行区且卡片不过大/过小。非法高度返回 null。
export function pageSizeBounds(
  availH,
  min = CARD_SCALE_MIN,
  max = CARD_SCALE_MAX
) {
  if (!(availH > 0)) return null;
  return {
    min: Math.ceil(availH / (44 * max)),
    max: Math.floor(availH / (44 * min)),
  };
}

// 屏幕可行区间与绝对 sanity 区间取交；极端高度冲突时退回屏幕区间
export function pageSizeSettingBounds(availH) {
  const b = pageSizeBounds(availH);
  if (!b) return { min: PAGE_SIZE_ABS_MIN, max: PAGE_SIZE_ABS_MAX };
  const lo = Math.max(PAGE_SIZE_ABS_MIN, b.min);
  const hi = Math.min(PAGE_SIZE_ABS_MAX, b.max);
  return lo > hi ? { min: b.min, max: b.max } : { min: lo, max: hi };
}

// 把用户设置的每页卡片数 n 钳制到当前屏幕可行区间（再交绝对区间 sanity 约束；
// 若两者冲突——极罕见的极端高度——以铺满屏幕的屏幕区间为准）
export function clampPageSize(n, availH) {
  if (!(availH > 0)) return n;
  const b = pageSizeSettingBounds(availH);
  return Math.min(b.max, Math.max(b.min, n));
}

// 本屏推荐卡片数：scale 恰为 1（基准卡片大小）的行数，再钳进可行区间
export function optimalPageSize(availH) {
  if (!(availH > 0)) return 20;
  return clampPageSize(Math.round(availH / 44), availH);
}

// 窗口层标题提示决策：多字母 + preview 模式下，已输入数字→'typed'，未输入→'enter'；其余 'plain'
// （返回决策码，文案 i18n 由调用方拼装，保持纯函数不依赖 t()）
export function winHintKind(multiLetter, previewMode, winDigit) {
  if (!multiLetter || !previewMode) return "plain";
  return winDigit ? "typed" : "enter";
}
