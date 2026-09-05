//! 设置页：版本/更新记录、当前设置读取、批量保存（含热键注册与开机自启副作用）。

use std::collections::HashSet;
use std::str::FromStr;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::windows;
use crate::{
    config::{self, WinDigitMode, WindowOrder},
    Inner,
};

// 当前版本与更新记录（显示在设置页）
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

struct ChangelogEntry {
    version: &'static str,
    date: &'static str,
    notes_zh: &'static [&'static str],
    notes_en: &'static [&'static str],
}

// 当前版本的更新记录（设置页只显示当前版本，按界面语言取中/英文）
const CURRENT_CHANGELOG: ChangelogEntry = ChangelogEntry {
    version: "0.3.2",
    date: "2026-09",
    notes_zh: &[
        "修复「幽灵窗口」：单窗口程序（cc-switch、MobaXterm 等）不再多出打不开的窗口，窗口列表与 Alt+Tab 一致（跳过 DWM 隐藏的后台/挂起窗口与工具窗口）",
        "保存设置更可靠：保存失败（如开机自启被系统拒绝）时，已注册的新热键会一并还原，不会出现「设置页还开着、新热键已生效」的错乱",
        "回归防线：键位交互（数字跳转/组合编号/筛选/翻页/空格互切）抽成纯函数并纳入自动化单元测试（Rust 34 项 + 前端 10 项），每次推送 CI 自动运行",
    ],
    notes_en: &[
        "Fixed \"ghost windows\": single-window apps (cc-switch, MobaXterm, etc.) no longer show extra unopenable windows — the window list now matches Alt+Tab (DWM-cloaked background/suspended windows and tool windows are skipped)",
        "More reliable settings save: on failure (e.g. autostart rejected by the system) the newly registered hotkey is rolled back too — no more \"settings page open but the new hotkey already active\"",
        "Regression guard: key interactions (digit jump / combo index / filter / paging / Space toggle) are now pure functions covered by automated unit tests (34 Rust + 10 frontend), run by CI on every push",
    ],
};

#[derive(Serialize, Clone)]
pub(crate) struct SettingsInfo {
    version: String,
    hotkey: String,
    autostart: bool,
    window_order: String,
    multi_letter: bool,
    theme: String,
    win_digit_mode: String,
    /// 当前生效语言（cfg.lang 为空则取系统检测值）
    lang: String,
    /// 配置里保存的语言（空=跟随系统；用于区分"明确选了 zh-CN"与"跟随系统恰好是中文"）
    lang_cfg: String,
    /// 系统检测语言（与用户设置无关，始终是 GetSystemDefault 结果）
    lang_sys: String,
    themes: Vec<ThemeUi>,
    blocked: Vec<BlockedUi>,
    changelog: ChangelogUi,
}

#[derive(Serialize, Clone)]
struct BlockedUi {
    process: String,
    note: String,
}

#[derive(Serialize, Clone)]
struct ThemeUi {
    id: String,
}

#[derive(Serialize, Clone)]
struct ChangelogUi {
    version: String,
    date: String,
    notes_zh: Vec<String>,
    notes_en: Vec<String>,
}

// 读取当前设置与版本/更新记录（设置页打开时调用，不立即写盘）
// 主题只下发 id 列表；显示名由前端 i18n 唯一负责（避免 Rust 硬编码中文名在英文环境漏出）
#[tauri::command]
pub(crate) fn get_settings(app: AppHandle) -> SettingsInfo {
    let inner = app.state::<Inner>();
    let cfg = inner.cfg.lock().unwrap();
    SettingsInfo {
        version: APP_VERSION.into(),
        hotkey: cfg.hotkey.clone(),
        autostart: cfg.autostart,
        window_order: cfg.window_order.as_str().into(),
        multi_letter: cfg.multi_letter,
        theme: cfg.theme.clone(),
        win_digit_mode: cfg.win_digit_mode.as_str().into(),
        // 当前生效语言：配置指定优先，空则跟随系统
        lang: if cfg.lang.is_empty() {
            windows::system_lang().to_string()
        } else {
            cfg.lang.clone()
        },
        lang_cfg: cfg.lang.clone(),
        lang_sys: windows::system_lang().to_string(),
        themes: config::THEMES.iter().map(|id| ThemeUi { id: (*id).into() }).collect(),
        blocked: cfg
            .blocked
            .iter()
            .map(|b| BlockedUi {
                process: b.process().to_string(),
                note: b.note().to_string(),
            })
            .collect(),
        changelog: ChangelogUi {
            version: CURRENT_CHANGELOG.version.into(),
            date: CURRENT_CHANGELOG.date.into(),
            notes_zh: CURRENT_CHANGELOG.notes_zh.iter().map(|s| s.to_string()).collect(),
            notes_en: CURRENT_CHANGELOG.notes_en.iter().map(|s| s.to_string()).collect(),
        },
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct SettingsInput {
    /// 新全局热键（global-shortcut 串）；设置页打开期间热键已 suspend
    #[serde(default)]
    pub(crate) hotkey: String,
    #[serde(default)]
    pub(crate) autostart: bool,
    pub(crate) window_order: String,
    pub(crate) multi_letter: bool,
    pub(crate) theme: String,
    pub(crate) win_digit_mode: String,
    /// 界面语言（"zh-CN"/"en"；空串=跟随系统，由前端传 system 表达）
    #[serde(default)]
    pub(crate) lang: String,
    /// 设置页保存时黑名单保留的进程名（被解除的不在其中）
    #[serde(default)]
    pub(crate) blocked: Vec<String>,
}

// 批量保存设置（设置页点保存时调用）
#[tauri::command]
pub(crate) fn save_settings(app: AppHandle, input: SettingsInput) -> Result<(), String> {
    // 排序/数字键行为：字符串须能往返解析为对应枚举（非法值拒绝）
    let window_order = WindowOrder::parse(&input.window_order);
    if window_order.as_str() != input.window_order {
        return Err(format!("无效的排序方式「{}」", input.window_order));
    }
    if !config::THEMES.contains(&input.theme.as_str()) {
        return Err(format!("无效的主题「{}」", input.theme));
    }
    let win_digit_mode = WinDigitMode::parse(&input.win_digit_mode);
    if win_digit_mode.as_str() != input.win_digit_mode {
        return Err(format!("无效的数字键行为「{}」", input.win_digit_mode));
    }
    // lang：空=跟随系统（前端传 "system" 时归一为空），zh-CN/en 直接存
    let lang: String = if input.lang == "system" {
        String::new()
    } else {
        input.lang.clone()
    };
    if lang != "" && lang != "zh-CN" && lang != "en" {
        return Err(format!("无效的语言「{}」", input.lang));
    }
    // 新热键解析校验（设置页期间热键已 suspend 注销）
    let new_hotkey = input.hotkey.trim();
    let new_sc = if new_hotkey.is_empty() {
        None
    } else {
        Some(
            Shortcut::from_str(new_hotkey)
                .map_err(|e| format!("热键「{}」无效: {}", new_hotkey, e))?,
        )
    };
    let inner = app.state::<Inner>();
    let (old_hotkey, old_autostart) = {
        let cfg = inner.cfg.lock().unwrap();
        (cfg.hotkey.clone(), cfg.autostart)
    };
    // 统一回滚：任一副作用/写盘失败后，把热键注册与自启注册表都还原到保存前状态
    let rollback = |reason: &str| {
        eprintln!("[winhop] 保存失败，回滚副作用: {}", reason);
        if let Ok(old) = Shortcut::from_str(&old_hotkey) {
            let _ = app.global_shortcut().unregister_all();
            let _ = app.global_shortcut().register(old);
        }
        if input.autostart != old_autostart {
            let _ = windows::set_autostart(old_autostart);
        }
    };
    // 1) 热键：注册新键（未改则恢复注册旧键——suspend 期间被注销）。新键注册失败时尚无其它副作用，还原旧键即可
    if let Some(sc) = new_sc {
        if let Err(e) = app.global_shortcut().register(sc) {
            eprintln!("[winhop] 新热键注册失败 {:?}: {}，回退旧键", new_hotkey, e);
            if let Ok(old) = Shortcut::from_str(&old_hotkey) {
                let _ = app.global_shortcut().register(old);
            }
            return Err(format!("热键「{}」注册失败（可能被其它程序占用）", new_hotkey));
        }
    } else if let Ok(old) = Shortcut::from_str(&old_hotkey) {
        let _ = app.global_shortcut().register(old);
    }
    // 2) 开机自启：落地注册表（HKCU\...\Run）。失败 → 回滚已注册的热键
    if input.autostart != old_autostart {
        if let Err(e) = windows::set_autostart(input.autostart) {
            rollback(&format!("自启注册表: {}", e));
            return Err(format!("设置开机自启失败: {}", e));
        }
    }
    // 3) 写盘。失败 → 回滚热键 + 自启注册表
    let save_res = {
        let mut cfg = inner.cfg.lock().unwrap();
        cfg.window_order = window_order;
        cfg.multi_letter = input.multi_letter;
        cfg.autostart = input.autostart;
        cfg.theme = input.theme.clone();
        cfg.win_digit_mode = win_digit_mode;
        cfg.lang = lang;
        if new_sc.is_some() {
            cfg.hotkey = new_hotkey.to_string();
        }
        // 黑名单：设置页保存保留列表之外的（被解除的）才移除
        let keep: HashSet<String> = input.blocked.iter().map(|b| b.to_lowercase()).collect();
        cfg.blocked.retain(|b| keep.contains(b.process()));
        config::save(&cfg, &inner.cfg_path)
    };
    if let Err(e) = save_res {
        rollback(&format!("写盘: {}", e));
        return Err(format!("保存配置失败: {}", e));
    }
    eprintln!(
        "[t={}] 保存设置 order={} multi_letter={} theme={} hotkey={}",
        windows::now_ms(),
        input.window_order,
        input.multi_letter,
        input.theme,
        new_hotkey
    );
    Ok(())
}
