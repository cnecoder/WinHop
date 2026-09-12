// 界面文案：中文 / 英文。默认语言由后端按系统语言检测并经 get_settings 返回。
// 静态文案用 [data-i18n="key"]，动态文案用 t("key")。
export const I18N = {
  "zh-CN": {
    // header
    settings: "设置",
    help: "帮助",
    escExit: "Esc 退出",
    badgeSingle: "单字母",
    badgeMulti: "多字母",
    legendCfg: "已配置·运行",
    legendNone: "未配置字母",
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
    setLaunch: "启动",
    setAutostart: "开机自启",
    setOrder: "窗口排序方式",
    orderZ: "固定序号（按创建顺序，不受使用影响）",
    orderMru: "最近使用优先（上次用的排 1）",
    setPageSize: "每页卡片数",
    pageSizeHint:
      "偏好数量；步进范围随当前屏幕自动限定（卡片等比缩放、铺满屏幕且不过大/过小）",
    pageSizeReset: "重置（本屏推荐）",
    setMode: "模式",
    modeSingle: "单字母模式：每个程序一个字母键，一按直达",
    modeMulti:
      "多字母模式：连续输入字母按代号/名称筛选，回车确认，代号可多字母（突破 26 个上限）",
    setWinDigit: "窗口层数字键",
    digitJump: "按数字直接跳转（窗口 ≤9 直切，&gt;9 快速连按组合编号）",
    digitPreview: "按数字先聚焦预览，回车确认（窗口 &gt;9 时 Backspace 退格）",
    setTheme: "主题",
    themeBlackGreen: "黑绿",
    themeBlackYellow: "黑黄",
    setBlocked: "黑名单（已屏蔽的程序）",
    blockedEmpty: "无；在程序行 ✎ 编辑面板点「屏蔽」可添加",
    unblock: "解除",
    quit: "退出 WinHop",
    about: "关于",
    version: "版本 {v}",
    githubRepo: "GitHub 仓库（问题反馈 / 下载新版）",
    changelog: "更新记录",
    setLanguage: "语言 / Language",
    langSystem: "跟随系统（{lang}）",
    langZh: "简体中文",
    langEn: "English",
    // 帮助页
    helpBack: "← 返回",
    helpOpen: "呼出与关闭",
    helpOpenText: "随时按下全局热键呼出覆盖层；再次按下或按 Esc 关闭，不切换任何窗口。点击覆盖层外部也会关闭。",
    helpPrograms: "第一步：选程序",
    helpSingle: "单字母模式：按程序对应的字母键直接进入该程序的窗口列表。字母需自己绑定（出厂不带预设）。",
    helpMulti: "多字母模式：连续输入字母，按代号或软件名实时筛选；↑↓ 选择，Enter 进入；Backspace 删除，Esc 清除。",
    helpWindows: "第二步：选窗口",
    helpWinDigit: "进入程序后，按窗口前的数字切换：窗口 ≤9 直切；&gt;9 时快速连按组成编号（如 1、2 = 12），Enter 确认，Backspace 退格。也可直接鼠标点击。Esc 返回程序层。",
    helpKeys: "其它按键",
    helpSpace: "在最近使用的两个窗口间快速互切。",
    helpArrows: "上下移动选择条（程序层 / 窗口层均可）。",
    helpPager: "程序一页放不下时翻页（每页卡片数可在设置中调整）。",
    helpF2: "打开设置页（也可点右上角「设置」）。",
    helpF11: "覆盖层全屏 / 窗口模式切换。",
    helpEdit: "绑定代号 / 改名 / 屏蔽",
    helpEditText: "点程序行右侧 ✎：为程序绑定字母（或多字母代号）、改显示名，也可「屏蔽」不想看到的程序（设置页可解除）；「删除」仅移除已保存的代号与名称。",
    helpMore: "更多",
    helpMoreText: "关闭覆盖层。修改热键、开机自启、排序、模式、主题、语言或黑名单，按 F2 进设置页。",
    // 弹窗
    confirmText: "有未保存的修改，是否保存？",
    confirmSave: "保存",
    confirmDiscard: "不保存",
    confirmCancel: "取消",
  },
  en: {
    settings: "Settings",
    help: "Help",
    escExit: "Esc to exit",
    badgeSingle: "Single",
    badgeMulti: "Multi",
    legendCfg: "Configured·running",
    legendNone: "No letter",
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
    setLaunch: "Startup",
    setAutostart: "Launch at startup",
    setOrder: "Window order",
    orderZ: "Fixed (creation order, unaffected by use)",
    orderMru: "Most recently used first (last used is #1)",
    setPageSize: "Cards per page",
    pageSizeHint:
      "Preferred count; the stepper range is auto-limited to this screen (cards scale to fill the screen, never too large/small)",
    pageSizeReset: "Reset (best for this screen)",
    setMode: "Mode",
    modeSingle: "Single-letter mode: one letter per program, press to jump",
    modeMulti:
      "Multi-letter mode: type letters to filter by code/name, Enter to confirm; codes can be multi-letter (beyond the 26 limit)",
    setWinDigit: "Window-layer number keys",
    digitJump: "Press a number to switch directly (≤9 direct, &gt;9 type the index quickly)",
    digitPreview: "Press a number to focus/preview, Enter to confirm (Backspace deletes when &gt;9)",
    setTheme: "Theme",
    themeBlackGreen: "Black-Green",
    themeBlackYellow: "Black-Yellow",
    setBlocked: "Blocklist (hidden programs)",
    blockedEmpty: "None; use the ✎ panel on a program row to block it",
    unblock: "Unblock",
    quit: "Quit WinHop",
    about: "About",
    version: "Version {v}",
    githubRepo: "GitHub repository (issues / releases)",
    changelog: "Changelog",
    setLanguage: "语言 / Language",
    langSystem: "System ({lang})",
    langZh: "简体中文",
    langEn: "English",
    // Help page
    helpBack: "← Back",
    helpOpen: "Open & close",
    helpOpenText: "Press the global hotkey any time to summon the overlay; press it again or Esc to close without switching. Clicking outside also closes it.",
    helpPrograms: "Step 1: pick a program",
    helpSingle: "Single-letter mode: press a program’s letter to open its window list. You bind the letters yourself (none are preset).",
    helpMulti: "Multi-letter mode: type letters to filter live by code or name; ↑↓ to move, Enter to open; Backspace deletes, Esc clears.",
    helpWindows: "Step 2: pick a window",
    helpWinDigit: "Inside a program, press the number before a window: direct when ≤9 windows; when &gt;9, type the index quickly (1 then 2 = 12), Enter confirms, Backspace deletes. You can also click a row. Esc goes back.",
    helpKeys: "Other keys",
    helpSpace: "Quick-switch between your two most recently used windows.",
    helpArrows: "Move the selection bar (both program and window layers).",
    helpPager: "Page through programs when one page isn't enough (cards per page is adjustable in Settings).",
    helpF2: "Open Settings (or click “Settings” at the top right).",
    helpF11: "Toggle the overlay between fullscreen and windowed.",
    helpEdit: "Bind a code / rename / block",
    helpEditText: "Click ✎ on a row to bind a letter (or multi-letter code), rename it, or “Block” programs you never want to see (unblock in Settings). “Delete” only removes its saved code and name.",
    helpMore: "More",
    helpMoreText: "Closes the overlay. To change the hotkey, autostart, ordering, mode, theme, language, or blocklist, press F2 for Settings.",
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
