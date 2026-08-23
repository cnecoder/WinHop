mod config;
mod windows;

use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use config::{Config, Program};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use windows::{HookMsg, WinInfo};

#[derive(PartialEq, Clone, Copy, Debug)]
enum Phase {
    Closed,
    Programs,
    Windows,
}

// 程序层每页显示数量（超过则 PageUp/PageDown 翻页）
const PROG_PAGE_SIZE: usize = 20;

#[derive(Clone)]
struct ProgEntry {
    key: String,
    name: String,
    process: String,
    configured: bool,
}

struct OverlayState {
    phase: Phase,
    prog_list: Vec<ProgEntry>,
    wins_by_proc: HashMap<String, Vec<WinInfo>>,
    prog_sel: usize,
    prog_page: usize,
    sel_proc: Option<String>,
    wins: Vec<WinInfo>,
    active: usize,
    last_activated: isize,
    digit_buf: String,
    switched: bool,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            phase: Phase::Closed,
            prog_list: Vec::new(),
            wins_by_proc: HashMap::new(),
            prog_sel: 0,
            prog_page: 0,
            sel_proc: None,
            wins: Vec::new(),
            active: 0,
            last_activated: 0,
            digit_buf: String::new(),
            switched: false,
        }
    }
}

struct Inner {
    cfg: Mutex<Config>,
    cfg_path: std::path::PathBuf,
    mru: Mutex<HashMap<isize, u64>>,
    visible: Arc<AtomicBool>,
    overlay: Mutex<OverlayState>,
    prev_fg: AtomicIsize,
    pending_activate: AtomicIsize,
}

#[derive(Serialize, Clone)]
struct ProgramUi {
    key: String,
    name: String,
    process: String,
    running: bool,
    count: usize,
    active: bool,
    configured: bool,
}

#[derive(Serialize, Clone)]
struct WindowUi {
    index: usize,
    title: String,
    screen: u32,
    active: bool,
    hwnd: isize,
}

#[derive(Serialize, Clone)]
struct Render {
    visible: bool,
    phase: String,
    title: String,
    window_order: String,
    programs: Vec<ProgramUi>,
    windows: Vec<WindowUi>,
    page: usize,
    page_count: usize,
}

fn emit(app: &AppHandle, inner: &Inner, ov: &OverlayState) {
    let cfg = inner.cfg.lock().unwrap();
    let visible = inner.visible.load(Ordering::Relaxed);
    let total = ov.prog_list.len();
    let page_count = total.div_ceil(PROG_PAGE_SIZE).max(1);
    let page = ov.prog_page.min(page_count - 1);
    let start = page * PROG_PAGE_SIZE;
    let end = (start + PROG_PAGE_SIZE).min(total);
    let mut render = Render {
        visible,
        phase: "programs".into(),
        title: String::new(),
        window_order: cfg.window_order.clone(),
        programs: Vec::new(),
        windows: Vec::new(),
        page: page + 1,
        page_count,
    };
    if ov.phase == Phase::Windows {
        render.phase = "windows".into();
        if let Some(proc) = &ov.sel_proc {
            let entry = ov
                .prog_list
                .iter()
                .find(|e| &e.process == proc)
                .or_else(|| ov.prog_list.first());
            if let Some(e) = entry {
                render.title = e.name.clone();
            }
            for (i, w) in ov.wins.iter().enumerate() {
                render.windows.push(WindowUi {
                    index: i + 1,
                    title: w.title.clone(),
                    screen: w.monitor,
                    active: i == ov.active,
                    hwnd: w.hwnd,
                });
            }
        }
    }
    for (i, p) in ov.prog_list[start..end].iter().enumerate() {
        let abs = start + i;
        let count = ov
            .wins_by_proc
            .get(&p.process)
            .map(|w| w.len())
            .unwrap_or(0);
        render.programs.push(ProgramUi {
            key: p.key.clone(),
            name: p.name.clone(),
            process: p.process.clone(),
            running: count > 0,
            count,
            active: abs == ov.prog_sel,
            configured: p.configured,
        });
    }
    drop(cfg);
    let _ = app.emit("overlay", &render);
}

// 已配置程序在前（保持配置字母），未配置的运行中程序按进程名排序补全（取空闲字母）
fn build_prog_list(cfg: &Config, wins_by_proc: &HashMap<String, Vec<WinInfo>>) -> Vec<ProgEntry> {
    let mut list: Vec<ProgEntry> = Vec::new();
    let mut used: HashSet<String> = HashSet::new();
    for p in &cfg.programs {
        used.insert(p.key.clone());
        list.push(ProgEntry {
            key: p.key.clone(),
            name: p.name.clone(),
            process: p.process.clone(),
            configured: true,
        });
    }
    let mut autos: Vec<&String> = wins_by_proc
        .keys()
        .filter(|proc| !cfg.programs.iter().any(|p| &p.process == *proc))
        .collect();
    autos.sort();
    for proc in autos {
        let letter = ('a'..='z').find(|c| !used.contains(&c.to_string()));
        if let Some(l) = letter {
            used.insert(l.to_string());
            // 显示名 = 版本资源 FileDescription（与任务管理器「文件说明」同源），
            // 无则用 exe 文件名（保留大小写）
            let path = wins_by_proc.get(proc).and_then(|wins| wins.first()).map(|w| w.path.clone());
            let stem = path
                .as_deref()
                .and_then(|p| p.rsplit('\\').next())
                .unwrap_or(proc)
                .trim_end_matches(".exe")
                .to_string();
            let name = path
                .as_deref()
                .and_then(windows::file_description)
                .unwrap_or(stem);
            list.push(ProgEntry {
                key: l.to_string(),
                name,
                process: proc.clone(),
                configured: false,
            });
        } else {
            break; // 字母耗尽，不再补全
        }
    }
    // 字母 a-z 共 26 个上限；超过的程序不再补全（前端 PageUp/PageDown 分页，每页 20 个）
    list.truncate(26);
    // 未运行的配置程序排到最后（运行中在前：配置序、自动补全次之；稳定排序保持组内顺序）
    list.sort_by_key(|e| {
        let running = wins_by_proc
            .get(&e.process)
            .map(|w| !w.is_empty())
            .unwrap_or(false);
        if running {
            0
        } else {
            1
        }
    });
    list
}

fn open(app: &AppHandle) {
    let inner = app.state::<Inner>();
    if inner.visible.load(Ordering::Relaxed) {
        return;
    }
    let mut all: Vec<WinInfo> = windows::enum_windows();
    let mut wins_by_proc: HashMap<String, Vec<WinInfo>> = HashMap::new();
    for w in all.drain(..) {
        wins_by_proc.entry(w.process.clone()).or_default().push(w);
    }
    let mut ov = inner.overlay.lock().unwrap();
    ov.phase = Phase::Programs;
    let cfg = inner.cfg.lock().unwrap().clone();
    ov.prog_list = build_prog_list(&cfg, &wins_by_proc);
    ov.wins_by_proc = wins_by_proc;
    ov.prog_sel = 0;
    ov.prog_page = 0;
    ov.sel_proc = None;
    ov.wins.clear();
    ov.digit_buf.clear();
    ov.switched = false;
    ov.last_activated = 0;
    inner.prev_fg.store(windows::foreground(), Ordering::Relaxed);
    inner.visible.store(true, Ordering::Relaxed);
    eprintln!(
        "[t={}] overlay open ({} programs)",
        windows::now_ms(),
        ov.prog_list.len()
    );
    if let Some(win) = app.get_webview_window("main") {
        if let Ok(Some(mon)) = win.primary_monitor() {
            let pos = mon.position();
            let size = mon.size();
            let ws = win.outer_size().unwrap_or(tauri::PhysicalSize::new(720, 520));
            let x = (pos.x + (size.width as i32 - ws.width as i32) / 2).max(0);
            let y = (pos.y + (size.height as i32 - ws.height as i32) / 2).max(0);
            let _ = win.set_position(PhysicalPosition::new(x, y));
        }
        let _ = win.show();
        // 整个覆盖层默认全屏（Win+Tab 风格选择页）
        set_overlay_fullscreen(app);
        // 覆盖层必须夺焦：否则前台是 Chromium 时按键走 raw input，谁也收不到。
        // 夺焦后按键落在覆盖层自己的 webview，由 JS keydown 接收（本窗口内部 raw input 可达）。
        let _ = win.set_focus();
        if let Ok(hwnd) = win.hwnd() {
            windows::set_overlay_hwnd(hwnd.0 as isize);
        }
    }
    emit(app, &inner, &ov);
}

fn close(app: &AppHandle) {
    let inner = app.state::<Inner>();
    if !inner.visible.load(Ordering::Relaxed) {
        eprintln!("[wintab] close 被跳过（已关闭）");
        return;
    }
    let ov = inner.overlay.lock().unwrap();
    inner.visible.store(false, Ordering::Relaxed);
    eprintln!("[t={}] overlay close (switched={})", windows::now_ms(), ov.switched);
    let switched = ov.switched;
    drop(ov);
    windows::set_overlay_hwnd(0);
    // 先隐藏再处理尺寸：若先退全屏，面板会在屏幕上可见地缩小跳动（闪烁）
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    // 激活延迟到事件循环处理（run_on_main_thread）：
    // 1) 覆盖层隐藏、钩子放行后 Alt 注入才不被吞
    // 2) SendInput 绝不能在钩子回调上下文执行（会自我死锁）
    let pending = inner.pending_activate.swap(0, Ordering::Relaxed);
    if pending != 0 {
        let app2 = app.clone();
        let _ = app2.run_on_main_thread(move || windows::activate_with_retry(pending));
    } else if !switched {
        let prev = inner.prev_fg.load(Ordering::Relaxed);
        if prev != 0 {
            let app2 = app.clone();
            let _ = app2.run_on_main_thread(move || windows::activate_with_retry(prev));
        }
    }
    let render = Render {
        visible: false,
        phase: "programs".into(),
        title: String::new(),
        window_order: inner.cfg.lock().unwrap().window_order.clone(),
        programs: Vec::new(),
        windows: Vec::new(),
        page: 1,
        page_count: 1,
    };
    let _ = app.emit("overlay", &render);
}

// 覆盖层整体全屏（Win+Tab 风格选择页）
fn set_overlay_fullscreen(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        if !win.is_fullscreen().unwrap_or(false) {
            let _ = win.set_fullscreen(true);
        }
    }
}

// 激活操作延迟到钩子回调之外执行：回调内严禁阻塞（超时会被 Windows 静默卸载钩子）
fn deferred_activate(app: &AppHandle, hwnd: isize) {
    let app = app.clone();
    let _ = app.run_on_main_thread(move || {
        let _ = windows::activate(hwnd);
    });
}

// 选中程序层条目：单窗口直切返回 true（调用方负责解锁并 close），多窗口进窗口层，
// 窗口层中重复选中同一程序则轮询切下一窗口
fn select_entry(app: &AppHandle, inner: &Inner, ov: &mut OverlayState, entry: &ProgEntry) -> bool {
    let wins = match ov.wins_by_proc.get(&entry.process) {
        Some(w) if !w.is_empty() => w,
        _ => {
            eprintln!("[wintab] '{}' 无窗口", entry.name);
            return false;
        }
    };
    let now = windows::now_ms();
    if ov.phase == Phase::Windows && ov.sel_proc.as_deref() == Some(entry.process.as_str()) {
        ov.active = (ov.active + 1) % wins.len();
        ov.last_activated = wins[ov.active].hwnd;
        inner.mru.lock().unwrap().insert(wins[ov.active].hwnd, now);
        deferred_activate(app, wins[ov.active].hwnd);
        eprintln!("[wintab] 轮询 {} 窗口 {}", entry.name, ov.active + 1);
        emit(app, inner, ov);
        return false;
    }
    if wins.len() == 1 {
        ov.switched = true;
        ov.last_activated = wins[0].hwnd;
        inner.mru.lock().unwrap().insert(wins[0].hwnd, now);
        inner.pending_activate.store(wins[0].hwnd, Ordering::Relaxed);
        true
    } else {
        ov.phase = Phase::Windows;
        ov.sel_proc = Some(entry.process.clone());
        // 排序：mru 按最近使用倒序（1 = 上次用的）；zorder 按窗口句柄序（创建序，稳定）。
        // Z 序不能做「固定序号」——任何激活都会把窗口提到 Z 顶，等于隐式 MRU
        let mut wins = wins.clone();
        if inner.cfg.lock().unwrap().window_order == "mru" {
            let mru = inner.mru.lock().unwrap();
            wins.sort_by_key(|w| std::cmp::Reverse(mru.get(&w.hwnd).copied().unwrap_or(0)));
        } else {
            wins.sort_by_key(|w| w.hwnd);
        }
        ov.wins = wins;
        ov.active = 0;
        ov.digit_buf.clear();
        eprintln!("[wintab] '{}' -> {} 窗口数 {}", entry.key, entry.name, ov.wins.len());
        emit(app, inner, ov);
        false
    }
}

// 确保 prog_page 是 prog_sel 所在页
fn sync_page(ov: &mut OverlayState) {
    ov.prog_page = ov.prog_sel / PROG_PAGE_SIZE;
}

fn select_by_letter(app: &AppHandle, inner: &Inner, ov: &mut OverlayState, c: char) -> bool {
    let Some((idx, entry)) = ov
        .prog_list
        .iter()
        .enumerate()
        .find(|(_, e)| e.key == c.to_string())
        .map(|(i, e)| (i, e.clone()))
    else {
        eprintln!("[wintab] letter '{}' 无对应程序", c);
        return false;
    };
    ov.prog_sel = idx;
    sync_page(ov);
    select_entry(app, inner, ov, &entry)
}

fn resolve_window(inner: &Inner, ov: &mut OverlayState, n: usize) -> bool {
    let total = ov.wins.len();
    if total == 0 || n < 1 {
        return false;
    }
    let idx = (n - 1).min(total - 1);
    ov.switched = true;
    ov.last_activated = ov.wins[idx].hwnd;
    inner.mru.lock().unwrap().insert(ov.wins[idx].hwnd, windows::now_ms());
    inner.pending_activate.store(ov.wins[idx].hwnd, Ordering::Relaxed);
    eprintln!("[wintab] 切换到窗口 {}", idx + 1);
    true
}

// webview JS keydown → 状态机。覆盖层夺焦后按键落在覆盖层内部，走这条路径
#[tauri::command]
fn key(app: AppHandle, k: String) {
    let msg = match k.as_str() {
        "esc" => HookMsg::Esc,
        "up" => HookMsg::Up,
        "down" => HookMsg::Down,
        "pageup" => HookMsg::PageUp,
        "pagedown" => HookMsg::PageDown,
        "enter" => HookMsg::Enter,
        "hotkey" => HookMsg::Hotkey,
        s if s.starts_with("letter:") => {
            HookMsg::Letter(s[7..].chars().next().unwrap_or(' '))
        }
        s if s.starts_with("digit:") => HookMsg::Digit(s[6..].chars().next().unwrap_or('0')),
        _ => return,
    };
    handle_key(&app, msg);
}

// 窗口缩略图（win+tab 风格）：PrintWindow 捕获 → BMP base64。async 跑在工作线程，不阻塞主线程
#[tauri::command]
async fn window_thumbnail(hwnd: isize, max_w: u32, max_h: u32) -> Result<String, String> {
    let bmp = windows::capture_window(hwnd, max_w, max_h).ok_or("窗口捕获失败")?;
    eprintln!(
        "[t={}] 缩略图 hwnd={:#x} bytes={}",
        windows::now_ms(),
        hwnd,
        bmp.len()
    );
    // 诊断转储：设置 WINTAB_DUMP_THUMB=1 时把 BMP 落盘
    if std::env::var("WINTAB_DUMP_THUMB").is_ok() {
        let path = std::env::temp_dir().join(format!("wt_thumb_{:#x}.bmp", hwnd));
        let _ = std::fs::write(&path, &bmp);
        eprintln!("[t={}] 转储 {}", windows::now_ms(), path.display());
    }
    Ok(format!("data:image/bmp;base64,{}", windows::base64(&bmp)))
}

#[tauri::command]
async fn toggle_fullscreen(app: AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let is_full = win.is_fullscreen().unwrap_or(false);
        if win.set_fullscreen(!is_full).is_err() {
            eprintln!("[t={}] 切换全屏失败", windows::now_ms());
        }
    }
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    eprintln!("[t={}] 设置面板退出", windows::now_ms());
    app.exit(0);
}

// 自动补全程序一键入配置：选字母 + 落盘 + 刷新覆盖层
#[tauri::command]
fn add_program(app: AppHandle, key: String, name: String, process: String) -> Result<(), String> {
    let inner = app.state::<Inner>();
    {
        let mut cfg = inner.cfg.lock().unwrap();
        if key.len() != 1 || !key.as_bytes()[0].is_ascii_lowercase() {
            return Err("字母必须是单个小写字母".into());
        }
        if cfg.programs.iter().any(|p| p.key == key) {
            return Err(format!("字母「{}」已被占用", key));
        }
        if cfg.programs.iter().any(|p| p.process == process) {
            return Err("该程序已在配置中".into());
        }
        cfg.programs.push(Program {
            key: key.clone(),
            name,
            process: process.clone(),
        });
        config::save(&cfg, &inner.cfg_path).map_err(|e| format!("保存配置失败: {}", e))?;
    }
    eprintln!("[t={}] 添加程序 {} ({})", windows::now_ms(), key, process);
    if inner.visible.load(Ordering::Relaxed) {
        let mut ov = inner.overlay.lock().unwrap();
        let cfg = inner.cfg.lock().unwrap().clone();
        ov.prog_list = build_prog_list(&cfg, &ov.wins_by_proc);
        ov.prog_sel = 0;
        ov.prog_page = 0;
        emit(&app, &inner, &ov);
    }
    Ok(())
}

// 设置界面：修改窗口排序方式，立即落盘
#[tauri::command]
fn set_window_order(app: AppHandle, order: String) -> Result<(), String> {
    if order != "zorder" && order != "mru" {
        return Err(format!("无效的排序方式「{}」", order));
    }
    let inner = app.state::<Inner>();
    {
        let mut cfg = inner.cfg.lock().unwrap();
        cfg.window_order = order.clone();
        config::save(&cfg, &inner.cfg_path).map_err(|e| format!("保存配置失败: {}", e))?;
    }
    eprintln!("[t={}] 窗口排序方式改为 {}", windows::now_ms(), order);
    if inner.visible.load(Ordering::Relaxed) {
        let ov = inner.overlay.lock().unwrap();
        emit(&app, &inner, &ov);
    }
    Ok(())
}

fn handle_key(app: &AppHandle, msg: HookMsg) {
    let inner = app.state::<Inner>();
    match msg {
        HookMsg::Hotkey => {
            if inner.visible.load(Ordering::Relaxed) {
                close(app);
            } else {
                open(app);
            }
            return;
        }
        _ => {}
    }
    if !inner.visible.load(Ordering::Relaxed) {
        return;
    }
    let mut ov = inner.overlay.lock().unwrap();
    eprintln!("[t={}] key {:?} phase={:?}", windows::now_ms(), msg, ov.phase);
    match msg {
        HookMsg::ClickOutside => {
            drop(ov);
            close(app);
        }
        HookMsg::Esc => match ov.phase {
            Phase::Windows => {
                ov.phase = Phase::Programs;
                ov.sel_proc = None;
                ov.digit_buf.clear();
                emit(app, &inner, &ov);
            }
            Phase::Programs => {
                drop(ov);
                close(app);
            }
            Phase::Closed => {}
        },
        HookMsg::Letter(c) => {
            if select_by_letter(app, &inner, &mut ov, c) {
                drop(ov);
                close(app);
            }
        }
        HookMsg::Digit(d) => {
            if ov.phase != Phase::Windows || ov.wins.is_empty() {
                return;
            }
            ov.digit_buf.push(d);
            if let Ok(n) = ov.digit_buf.parse::<usize>() {
                let total = ov.wins.len();
                // n*10 > total：再加任何一位都会超过总数，此刻立即兑现
                if n >= 1 && n * 10 > total && resolve_window(&inner, &mut ov, n) {
                    drop(ov);
                    close(app);
                }
            }
        }
        HookMsg::Hotkey => {}
        HookMsg::Up | HookMsg::Down => {
            let delta: isize = if matches!(msg, HookMsg::Up) { -1 } else { 1 };
            match ov.phase {
                Phase::Programs if !ov.prog_list.is_empty() => {
                    let len = ov.prog_list.len() as isize;
                    ov.prog_sel = ((ov.prog_sel as isize + delta + len) % len) as usize;
                    sync_page(&mut ov);
                    emit(app, &inner, &ov);
                }
                Phase::Windows if !ov.wins.is_empty() => {
                    let len = ov.wins.len() as isize;
                    ov.active = ((ov.active as isize + delta + len) % len) as usize;
                    emit(app, &inner, &ov);
                }
                _ => {}
            }
        }
        HookMsg::PageUp | HookMsg::PageDown => {
            if ov.phase != Phase::Programs || ov.prog_list.is_empty() {
                return;
            }
            let total = ov.prog_list.len();
            let page_count = total.div_ceil(PROG_PAGE_SIZE);
            let cur = ov.prog_sel / PROG_PAGE_SIZE;
            let new_page = if matches!(msg, HookMsg::PageDown) {
                (cur + 1).min(page_count - 1)
            } else if cur == 0 {
                0
            } else {
                cur - 1
            };
            ov.prog_page = new_page;
            let start = new_page * PROG_PAGE_SIZE;
            let end = (start + PROG_PAGE_SIZE).min(total);
            // 选中保持在当前页内：超出则夹到页内首/末
            if ov.prog_sel < start || ov.prog_sel >= end {
                ov.prog_sel = if matches!(msg, HookMsg::PageDown) { start } else { end - 1 };
            }
            emit(app, &inner, &ov);
        }
        HookMsg::Enter => {
            let done = match ov.phase {
                Phase::Programs => {
                    if ov.prog_list.is_empty() {
                        false
                    } else {
                        let idx = ov.prog_sel.min(ov.prog_list.len() - 1);
                        let entry = ov.prog_list[idx].clone();
                        select_entry(app, &inner, &mut ov, &entry)
                    }
                }
                Phase::Windows => {
                    let n = ov.active + 1;
                    resolve_window(&inner, &mut ov, n)
                }
                Phase::Closed => false,
            };
            if done {
                drop(ov);
                close(app);
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    windows::redirect_stderr_to_file();
    eprintln!("=== wintab start ===");
    let (cfg, cfg_path) = config::load();
    // taskmgr 等管理员程序需要提权才能钩子生效/激活；debug 构建跳过（dev 迭代不弹 UAC）
    if cfg.elevate && !cfg!(debug_assertions) && !windows::is_elevated() {
        windows::relaunch_elevated();
        return;
    }
    if !windows::acquire_single_instance() {
        eprintln!("[wintab] 已有实例在运行，退出");
        return;
    }
    let shortcut = Shortcut::from_str(&cfg.hotkey)
        .unwrap_or_else(|e| panic!("配置热键「{}」无效: {}", cfg.hotkey, e));
    tauri::Builder::default()
        .plugin(
            // RegisterHotKey 系统级热键：Chromium 前台（raw input）时 LL 键盘钩子失效，
            // 但 RegisterHotKey 与前台无关，始终生效
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let inner = app.state::<Inner>();
                        if inner.visible.load(Ordering::Relaxed) {
                            close(app);
                        } else {
                            open(app);
                        }
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            key,
            set_window_order,
            quit_app,
            window_thumbnail,
            toggle_fullscreen,
            add_program
        ])
        .setup(move |app| {
            let visible = Arc::new(AtomicBool::new(false));
            app.manage(Inner {
                cfg: Mutex::new(cfg),
                cfg_path,
                mru: Mutex::new(HashMap::new()),
                visible: visible.clone(),
                overlay: Mutex::new(OverlayState::default()),
                prev_fg: AtomicIsize::new(0),
                pending_activate: AtomicIsize::new(0),
            });
            let handle = app.handle().clone();
            // 鼠标钩子（点击外部关闭）；键盘走 RegisterHotKey + webview JS keydown
            windows::install_mouse_hook(visible, Box::new(move |msg| handle_key(&handle, msg)));
            app.global_shortcut().register(shortcut)?;
            // 健康看门狗：仅检测「隐形覆盖层」异常（visible=true 但窗口不可见）。
            // 不做空闲自动退出：配置面板打字不经过状态机，定时退出会打断用户操作。
            // 覆盖层只通过：热键 toggle / 选中窗口 / Esc / 点外部 关闭。
            let health_handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut last_fg: isize = 0;
                loop {
                    std::thread::sleep(Duration::from_secs(2));
                    let inner = health_handle.state::<Inner>();
                    // MRU 补录：用户手动点击/切换前台窗口也计入最近使用
                    let fg = windows::foreground();
                    if fg != 0 && fg != last_fg {
                        inner
                            .mru
                            .lock()
                            .unwrap()
                            .insert(fg, windows::now_ms());
                        last_fg = fg;
                    }
                    if !inner.visible.load(Ordering::Relaxed) {
                        continue;
                    }
                    let hwnd = windows::get_overlay_hwnd();
                    if hwnd != 0 && !windows::overlay_visible(hwnd) {
                        eprintln!(
                            "[t={}] 分叉检测：visible=true 但窗口不可见，强制关闭",
                            windows::now_ms()
                        );
                        close(&health_handle);
                    }
                }
            });
            // 托盘：鼠标路径不受任何键盘钩子影响，保证 taskmgr 等场景下仍可唤出
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit])?;
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("缺省图标").clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "quit" {
                        app.exit(0);
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    // 仅左键抬起触发覆盖层；右键留给系统原生菜单
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        let inner = app.state::<Inner>();
                        if inner.visible.load(Ordering::Relaxed) {
                            close(app);
                        } else {
                            open(app);
                        }
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::Focused(false) = event {
                let app = window.app_handle();
                let inner = app.state::<Inner>();
                if !inner.visible.load(Ordering::Relaxed) {
                    return;
                }
                let ov = inner.overlay.lock().unwrap();
                // 轮询切换时焦点落在目标窗口，属于预期，不关闭
                let keep = ov.last_activated != 0 && windows::foreground() == ov.last_activated;
                drop(ov);
                if !keep {
                    close(app);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
