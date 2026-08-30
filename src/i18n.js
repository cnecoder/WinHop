// 界面文案：中文 / 英文。默认语言由后端按系统语言检测并经 get_settings 返回。
// 静态文案用 [data-i18n="key"]，动态文案用 t("key")。
export const I18N = {
  "zh-CN": {
    // header
    settings: "设置",
    escExit: "Esc 退出",
    badgeSingle: "单字母",
    badgeMulti: "多字母",
    legendCfg: "已配置·运行",
    legendAuto: "动态检测",
    legendOff: "已配置·未运行",
    // 程序层
    filterLabel: "筛选",
    filterHint: "Enter 确认，Esc 清除，Backspace 删除",
    pager: "PageUp/PageDown 翻页",
    pageOf: "第 {a} / {b} 页 · PageUp/PageDown 翻页",
    notRunning: "未运行",
    noMatch: "无匹配「{q}」，Esc 清除筛选",
    edit: "编辑",
    // 窗口层
    screen: "屏{n}",
    winEnter: "{title} · 输入数字聚焦窗口，Enter 跳转",
    winTyped: "{title} · 已输入 {n}（Enter 跳转，Backspace 删除）",
    // 设置页
    setBack: "← 返回",
    setTitle: "设置",
    setUnsaved: "有未保存的修改",
    setSave: "保存",
    setHotkey: "全局热键",
    hotkeyRecord: "录制",
    hotkeyListening: "按下组合键…",
    hotkeyHint: "点「录制」后按下组合键（须含 Ctrl/Alt/Shift/Win），实时显示在框内；Enter 结束录制、Esc 取消。修改后点右上「保存」生效。",
    hotkeyNeedModifier: "需含修饰键 Ctrl/Alt/Shift/Win",
    setOrder: "窗口排序方式",
    orderZ: "固定序号（按创建顺序，不受使用影响）",
    orderMru: "最近使用优先（上次用的排 1）",
    setMode: "模式",
    modeSingle: "单字母模式：每个程序一个字母键，一按直达",
    modeMulti:
      "多字母模式：连续输入字母按代号/名称筛选，回车确认，代号可多字母（突破 26 个上限）",
    setWinDigit: "窗口层数字键",
    digitJump: "按数字直接跳转（窗口 ≤9 直切，&gt;9 快速连按组合编号）",
    digitPreview: "按数字先聚焦预览，回车确认（窗口 &gt;9 时 Backspace 退格）",
    setTheme: "主题",
    setBlocked: "黑名单（已屏蔽的程序）",
    blockedEmpty: "无；在程序行 ✎ 编辑面板点「屏蔽」可添加",
    unblock: "解除",
    quit: "退出 WinHop",
    about: "关于",
    version: "版本 {v}",
    changelog: "更新记录",
    setLanguage: "语言 / Language",
    langSystem: "跟随系统（{lang}）",
    langZh: "简体中文",
    langEn: "English",
    // 弹窗
    confirmText: "有未保存的修改，是否保存？",
    confirmSave: "保存",
    confirmDiscard: "不保存",
    confirmCancel: "取消",
  },
  en: {
    settings: "Settings",
    escExit: "Esc to exit",
    badgeSingle: "Single",
    badgeMulti: "Multi",
    legendCfg: "Configured·running",
    legendAuto: "Auto-detected",
    legendOff: "Configured·stopped",
    filterLabel: "Filter",
    filterHint: "Enter to confirm, Esc to clear, Backspace to delete",
    pager: "PageUp/PageDown to page",
    pageOf: "Page {a} / {b} · PageUp/PageDown",
    notRunning: "stopped",
    noMatch: 'No match for "{q}", Esc to clear filter',
    edit: "Edit",
    screen: "Screen {n}",
    winEnter: "{title} · Type a number to focus, Enter to switch",
    winTyped: "{title} · Typed {n} (Enter to switch, Backspace to delete)",
    setBack: "← Back",
    setTitle: "Settings",
    setUnsaved: "Unsaved changes",
    setSave: "Save",
    setHotkey: "Global hotkey",
    hotkeyRecord: "Record",
    hotkeyListening: "Press a combo…",
    hotkeyHint: "Click “Record”, then press a combination (must include Ctrl/Alt/Shift/Win); it shows in the box live. Enter finishes, Esc cancels. Click “Save” (top right) to apply.",
    hotkeyNeedModifier: "Must include a modifier Ctrl/Alt/Shift/Win",
    setOrder: "Window order",
    orderZ: "Fixed (creation order, unaffected by use)",
    orderMru: "Most recently used first (last used is #1)",
    setMode: "Mode",
    modeSingle: "Single-letter mode: one letter per program, press to jump",
    modeMulti:
      "Multi-letter mode: type letters to filter by code/name, Enter to confirm; codes can be multi-letter (beyond the 26 limit)",
    setWinDigit: "Window-layer number keys",
    digitJump: "Press a number to switch directly (≤9 direct, &gt;9 type the index quickly)",
    digitPreview: "Press a number to focus/preview, Enter to confirm (Backspace deletes when &gt;9)",
    setTheme: "Theme",
    setBlocked: "Blocklist (hidden programs)",
    blockedEmpty: "None; use the ✎ panel on a program row to block it",
    unblock: "Unblock",
    quit: "Quit WinHop",
    about: "About",
    version: "Version {v}",
    changelog: "Changelog",
    setLanguage: "语言 / Language",
    langSystem: "System ({lang})",
    langZh: "简体中文",
    langEn: "English",
    confirmText: "You have unsaved changes. Save?",
    confirmSave: "Save",
    confirmDiscard: "Discard",
    confirmCancel: "Cancel",
  },
};

let lang = "zh-CN";

export function setLang(l) {
  lang = I18N[l] ? l : "zh-CN";
}
export function getLang() {
  return lang;
}
export function isEn() {
  return lang === "en";
}

export function t(key, vars) {
  let s = (I18N[lang] && I18N[lang][key]) || I18N["zh-CN"][key] || key;
  if (vars) {
    for (const k in vars) s = s.replaceAll(`{${k}}`, vars[k]);
  }
  return s;
}

// 把所有 [data-i18n] 元素的文本替换为当前语言
export function applyStaticI18n(root) {
  (root || document).querySelectorAll("[data-i18n]").forEach((el) => {
    el.textContent = t(el.getAttribute("data-i18n"));
  });
  // 含 HTML 实体（如 &gt;）的文案
  (root || document).querySelectorAll("[data-i18n-html]").forEach((el) => {
    el.innerHTML = t(el.getAttribute("data-i18n-html"));
  });
  // title 属性
  (root || document).querySelectorAll("[data-i18n-title]").forEach((el) => {
    el.setAttribute("title", t(el.getAttribute("data-i18n-title")));
  });
}
