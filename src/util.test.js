// 前端纯函数单测：node --test（无需 DOM / Tauri）。
import { test } from "node:test";
import assert from "node:assert/strict";
import { escapeHtml, prettyHotkey } from "./util.js";

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
