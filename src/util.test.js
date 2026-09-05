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
