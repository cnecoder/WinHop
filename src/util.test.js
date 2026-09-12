// 前端纯函数单测：node --test（无需 DOM / Tauri）。
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  escapeHtml,
  prettyHotkey,
  settingsFormEquals,
  rectPhys,
  clipRectPhys,
  winHintKind,
  cardScale,
  pageSizeBounds,
  pageSizeSettingBounds,
  clampPageSize,
  optimalPageSize,
  CARD_SCALE_MIN,
  CARD_SCALE_MAX,
} from "./util.js";

test("escapeHtml 转义五个特殊字符", () => {
  assert.equal(
    escapeHtml(`<a href="x">&'`),
    "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
  );
});

test("escapeHtml 普通文本不变、非字符串安全转字符串", () => {
  assert.equal(escapeHtml("winhop 123"), "winhop 123");
  assert.equal(escapeHtml(123), "123");
  assert.equal(escapeHtml(""), "");
});

test("prettyHotkey 修饰键映射为可读名", () => {
  assert.equal(prettyHotkey("ctrl+space"), "Ctrl + Space");
  assert.equal(prettyHotkey("ctrl+alt+shift+super+f"), "Ctrl + Alt + Shift + Win + F");
});

test("prettyHotkey 主键字母大写、其余首字母大写", () => {
  assert.equal(prettyHotkey("alt+a"), "Alt + A");
  assert.equal(prettyHotkey("ctrl+f5"), "Ctrl + F5");
});

// 伪 getBoundingClientRect 结果
const R = (left, top, width, height) => ({
  left,
  top,
  right: left + width,
  bottom: top + height,
  width,
  height,
});

test("settingsFormEquals 同值（含 blocked 乱序）相等，字段差异不等", () => {
  const a = {
    hotkey: "ctrl+space",
    autostart: false,
    window_order: "zorder",
    multi_letter: false,
    theme: "black-green",
    win_digit_mode: "jump",
    lang: "system",
    blocked: ["a.exe", "b.exe"],
  };
  const b = { ...a, blocked: ["b.exe", "a.exe"] }; // blocked 顺序不同
  assert.equal(settingsFormEquals(a, b), true);
  assert.equal(settingsFormEquals(a, { ...a, theme: "black-yellow" }), false);
  assert.equal(settingsFormEquals(a, { ...a, blocked: ["a.exe"] }), false);
  assert.equal(settingsFormEquals(null, null), true);
});

test("rectPhys 按 dpr 缩放并四舍五入", () => {
  assert.deepEqual(rectPhys(R(10, 20, 100, 50), 2), { x: 20, y: 40, w: 200, h: 100 });
  assert.deepEqual(rectPhys(R(0.5, 0.5, 10, 10), 1), { x: 1, y: 1, w: 10, h: 10 });
});

test("clipRectPhys 无容器返回全 0（不裁剪）", () => {
  assert.deepEqual(clipRectPhys(R(0, 0, 100, 100), null, 1), {
    ax: 0,
    ay: 0,
    aw: 0,
    ah: 0,
  });
});

test("clipRectPhys 求交：元素被容器裁剪时返回交集物理像素", () => {
  // 容器 clip 覆盖元素右下半：元素 (0,0,200,200)，容器 (100,100,200,200) → 交 (100,100)-(200,200)
  const clip = clipRectPhys(R(0, 0, 200, 200), R(100, 100, 200, 200), 1);
  assert.deepEqual(clip, { ax: 100, ay: 100, aw: 100, ah: 100 });
  // dpr=2 缩放
  const clip2 = clipRectPhys(R(0, 0, 200, 200), R(100, 100, 200, 200), 2);
  assert.deepEqual(clip2, { ax: 200, ay: 200, aw: 200, ah: 200 });
});

test("winHintKind 非多字母或非 preview 一律 plain", () => {
  assert.equal(winHintKind(false, true, ""), "plain");
  assert.equal(winHintKind(true, false, "1"), "plain");
  assert.equal(winHintKind(false, false, ""), "plain");
});

test("winHintKind 多字母 preview：未输入 enter、已输入 typed", () => {
  assert.equal(winHintKind(true, true, ""), "enter");
  assert.equal(winHintKind(true, true, "12"), "typed");
});

test("cardScale 基准：行区恰好容纳 n 个槽位时为 1", () => {
  // 每槽位 44（前导/行间距 4 + 行高 40），n 个槽位总高 44n
  assert.equal(cardScale(44 * 20, 20), 1);
  assert.equal(cardScale(44 * 8, 8), 1);
});

test("cardScale 连续缩放并钳制上下限", () => {
  // 空间不足 → 等比缩小（不触底时连续）
  assert.ok(Math.abs(cardScale(700, 20) - 700 / 880) < 1e-9);
  // 极小空间触底
  assert.equal(cardScale(400, 24), CARD_SCALE_MIN);
  // 极大空间触顶
  assert.equal(cardScale(2000, 8), CARD_SCALE_MAX);
  // 同样高度下每页越多缩放越小
  assert.ok(cardScale(900, 24) < cardScale(900, 8));
});

test("cardScale 非法输入回退 1", () => {
  assert.equal(cardScale(0, 20), 1);
  assert.equal(cardScale(900, 0), 1);
  assert.equal(cardScale(-5, 20), 1);
});

test("pageSizeBounds 由缩放上下限反推卡片数区间", () => {
  // avail=1000：n_min=ceil(1000/59.4)=17，n_max=floor(1000/33)=30
  assert.deepEqual(pageSizeBounds(1000), { min: 17, max: 30 });
});

test("pageSizeBounds 区间内任意 n 都铺满且缩放不越界", () => {
  const avail = 900;
  const b = pageSizeBounds(avail);
  for (const n of [b.min, b.min + 1, b.max - 1, b.max]) {
    const s = cardScale(avail, n);
    assert.ok(s >= CARD_SCALE_MIN - 1e-9 && s <= CARD_SCALE_MAX + 1e-9);
    // 总高 44n*s ≈ avail（边界 n 因 ceil/floor 有微小余量，误差不足一个槽位）
    assert.ok(Math.abs(44 * n * s - avail) < 44);
  }
  // 区间外必然越界：n<min → 触顶 1.35；n>max → 触底 0.75
  assert.equal(cardScale(avail, b.min - 1), CARD_SCALE_MAX);
  assert.equal(cardScale(avail, b.max + 1), CARD_SCALE_MIN);
});

test("pageSizeBounds 非法高度返回 null", () => {
  assert.equal(pageSizeBounds(0), null);
  assert.equal(pageSizeBounds(-1), null);
});

test("pageSizeSettingBounds 屏幕区间与绝对 sanity 区间取交", () => {
  const avail = 1000; // 屏幕区间 [17,30]，与 [8,64] 取交不变
  assert.deepEqual(pageSizeSettingBounds(avail), { min: 17, max: 30 });
  // 非法高度退回绝对区间
  assert.deepEqual(pageSizeSettingBounds(0), { min: 8, max: 64 });
});

test("clampPageSize 钳进屏幕可行区间与绝对 sanity 区间", () => {
  const avail = 1000; // 屏幕区间 [17,30]
  assert.equal(clampPageSize(8, avail), 17); // 低于屏幕下限抬上来
  assert.equal(clampPageSize(20, avail), 20); // 区间内保持
  assert.equal(clampPageSize(64, avail), 30); // 高于屏幕上限压下去
  // 非法高度：原样返回
  assert.equal(clampPageSize(20, 0), 20);
});

test("optimalPageSize 返回 scale≈1 的基准行数并落在可行区间内", () => {
  const avail = 904; // 1080p 典型行区
  const n = optimalPageSize(avail); // round(904/44)=21
  assert.equal(n, 21);
  const b = pageSizeBounds(avail);
  assert.ok(n >= b.min && n <= b.max);
  // scale 最接近 1：与相邻行数相比偏差最小
  const s = cardScale(avail, n);
  assert.ok(Math.abs(s - 1) <= Math.abs(cardScale(avail, n - 1) - 1));
  assert.ok(Math.abs(s - 1) <= Math.abs(cardScale(avail, n + 1) - 1));
  // 非法高度回退默认 20
  assert.equal(optimalPageSize(0), 20);
});
