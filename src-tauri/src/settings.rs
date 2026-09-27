//! 设置页：版本/更新记录、当前设置读取、批量保存（含热键注册与开机自启副作用）。

use std::collections::HashSet;
use std::str::FromStr;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::windows;
use crate::{
    config::{
        self, OverlayBg, WinDigitMode, WindowOrder, PROG_PAGE_SIZE_MAX, PROG_PAGE_SIZE_MIN,
    },
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
    version: "0.4.0",
    date: "2026-09",
    notes_zh: &[
        "程序列表现在显示应用图标：运行中的程序直接显示，未运行的程序也会自动探测（运行过一次即记住路径，另支持注册表与开始菜单快捷方式解析）；PWA 显示宿主浏览器图标",
        "背景效果可选：不透明暗色（默认）或 Acrylic 系统级毛玻璃模糊，设置页一键切换、即时预览",
        "界面动效全面升级：呼出时程序行逐行入场、进入窗口层柔和淡入、退出类关闭短促淡出（可关闭；切换窗口类操作始终瞬时，不牺牲速度）",
        "窗口层重新设计：16:9 缩略图恰好铺满 6 行、右侧实时预览更大、不再显示滚动条，选中窗口以「高亮行 + 实心数字徽章」清晰标示",
        "修复若干显示问题：退出/切换时屏幕顶端不再出现白色横条，预览与缩略图边缘不再有亮线残留，列表选中高亮不再随代号长度错位",
    ],
    notes_en: &[
        "Program lists now show application icons: running programs directly, and not-running ones are auto-discovered too (the install path is remembered after the first run, with registry and Start Menu shortcut resolution as fallbacks); PWAs show their host browser icon",
        "Background effects: choose between opaque dark (default) and Acrylic system-level blur, switchable in Settings with live preview",
        "Interface animations upgraded: program rows cascade in on launch, the window layer fades in softly, and dismissals fade out briefly (optional; window switching is always instant, never slowed down)",
        "Redesigned window layer: 16:9 thumbnails filling exactly 6 rows, a larger live preview, no scrollbars, and the selected window is clearly marked with a highlighted row plus a solid number badge",
        "Fixed several display issues: no more white bar at the top of the screen when exiting or switching, no more bright line residue around the preview and thumbnails, and list selection no longer misaligns with variable-length codes",
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
    /// 程序层每页卡片数（驱动卡片自适应缩放）
    prog_page_size: usize,
    /// 覆盖层背景效果 id：solid/translucent/acrylic（显示名由前端 i18n 负责）
    overlay_bg: String,
    /// 退出类关闭是否淡出（切换类关闭始终瞬时）
    close_anim: bool,
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
        prog_page_size: cfg.prog_page_size,
        overlay_bg: cfg.overlay_bg.as_str().into(),
        close_anim: cfg.close_anim,
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
    /// 程序层每页卡片数（范围 MIN..=MAX）
    #[serde(default)]
    pub(crate) prog_page_size: usize,
    /// 覆盖层背景效果：solid/translucent/acrylic
    #[serde(default)]
    pub(crate) overlay_bg: String,
    /// 退出类关闭淡出开关
    #[serde(default)]
    pub(crate) close_anim: bool,
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
    // 背景效果：字符串须能往返解析（非法值拒绝）
    let overlay_bg = OverlayBg::parse(&input.overlay_bg);
    if overlay_bg.as_str() != input.overlay_bg {
        return Err(format!("无效的背景效果「{}」", input.overlay_bg));
    }
    // 每页卡片数：0（前端缺省/旧版）按默认 20，其余必须在 [MIN, MAX]
    let prog_page_size = if input.prog_page_size == 0 {
        20
    } else if !(PROG_PAGE_SIZE_MIN..=PROG_PAGE_SIZE_MAX).contains(&input.prog_page_size) {
        return Err(format!(
            "每页卡片数须在 {}..={} 之间",
            PROG_PAGE_SIZE_MIN, PROG_PAGE_SIZE_MAX
        ));
    } else {
        input.prog_page_size
    };
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
        cfg.overlay_bg = overlay_bg;
        cfg.close_anim = input.close_anim;
        cfg.prog_page_size = prog_page_size;
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
        "[t={}] 保存设置 order={} multi_letter={} theme={} bg={} close_anim={} hotkey={}",
        windows::now_ms(),
        input.window_order,
        input.multi_letter,
        input.theme,
        overlay_bg.as_str(),
        input.close_anim,
        new_hotkey
    );
    // 背景特效随保存落地（预览已应用时为幂等重设；防止预览漏发的兜底）
    crate::apply_overlay_bg(&app, overlay_bg);
    Ok(())
}
