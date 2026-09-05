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
    /// 单字母模式代号（空表示仅多字母配置）
    key: String,
    /// 多字母模式代号（空表示未配置多字母）
    multi_key: String,
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
    letter_buf: String,
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
            letter_buf: String::new(),
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
    multi_key: String,
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
    multi_letter: bool,
    theme: String,
    win_digit_mode: String,
    filter: String,
    programs: Vec<ProgramUi>,
    windows: Vec<WindowUi>,
    page: usize,
    page_count: usize,
    /// 多字母模式窗口层：已输入的窗口索引（数字串），用于提示；Enter 确认
    win_digit: String,
}

// 计算单个条目对输入串的匹配得分：多字母快捷键优先（精确>前缀>子串），
// 其次名称（精确>前缀>子串），再次进程名（前缀>子串）。0 表示不匹配。
fn match_score(e: &ProgEntry, q: &str) -> u32 {
    let mk = e.multi_key.to_lowercase();
    let name = e.name.to_lowercase();
    let proc = e.process.to_lowercase();
    let mut s = 0u32;
    if !mk.is_empty() {
        if mk == q {
            s = s.max(1000);
        } else if mk.starts_with(q) {
            s = s.max(800);
        } else if mk.contains(q) {
            s = s.max(600);
        }
    }
    if name == q {
        s = s.max(500);
    } else if name.starts_with(q) {
        s = s.max(400);
    } else if name.contains(q) {
        s = s.max(300);
    }
    if proc.starts_with(q) {
        s = s.max(200);
    } else if proc.contains(q) {
        s = s.max(100);
    }
    s
}

// 当前视图索引（按显示顺序）。
// 单字母模式：全量（保持 build_prog_list 的运行中在前顺序）。
// 多字母模式：无输入时全量；有输入时按匹配得分降序，0 分排除。
fn view_indices(ov: &OverlayState, multi: bool) -> Vec<usize> {
    if multi {
        if ov.letter_buf.is_empty() {
            (0..ov.prog_list.len()).collect()
        } else {
            let q = ov.letter_buf.to_lowercase();
            let mut scored: Vec<(usize, u32)> = ov
                .prog_list
                .iter()
                .enumerate()
                .map(|(i, e)| (i, match_score(e, &q)))
                .filter(|(_, s)| *s > 0)
                .collect();
            scored.sort_by(|a, b| b.1.cmp(&a.1));
            scored.into_iter().map(|(i, _)| i).collect()
        }
    } else {
        (0..ov.prog_list.len()).collect()
    }
}

fn emit(app: &AppHandle, inner: &Inner, ov: &OverlayState) {
    // 回到程序层时注销全部缩略图（覆盖层关闭由 close() 收尾）
    if ov.phase != Phase::Windows {
        windows::thumb_clear();
    }
    let cfg = inner.cfg.lock().unwrap();
    let visible = inner.visible.load(Ordering::Relaxed);
    let multi = cfg.multi_letter;
    let view = view_indices(&ov, multi);
    let page_count = view.len().div_ceil(PROG_PAGE_SIZE).max(1);
    let page = ov.prog_page.min(page_count - 1);
    let start = page * PROG_PAGE_SIZE;
    let end = (start + PROG_PAGE_SIZE).min(view.len());
    let mut render = Render {
        visible,
        phase: "programs".into(),
        title: String::new(),
        window_order: cfg.window_order.clone(),
        multi_letter: multi,
        theme: cfg.theme.clone(),
        win_digit_mode: cfg.win_digit_mode.clone(),
        filter: if multi { ov.letter_buf.clone() } else { String::new() },
        programs: Vec::new(),
        windows: Vec::new(),
        page: page + 1,
        page_count,
        win_digit: String::new(),
    };
    if ov.phase == Phase::Windows {
        render.phase = "windows".into();
        if multi {
            render.win_digit = ov.digit_buf.clone();
        }
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
    for &abs in &view[start..end] {
        let p = &ov.prog_list[abs];
        let count = ov
            .wins_by_proc
            .get(&p.process)
            .map(|w| w.len())
            .unwrap_or(0);
        // 多字母模式显示 multi_key，单字母模式显示 key
        let display_key = if multi {
            p.multi_key.clone()
        } else {
            p.key.clone()
        };
        render.programs.push(ProgramUi {
            key: display_key,
            multi_key: p.multi_key.clone(),
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

// 构造程序列表。
// 已配置程序（cfg.programs，含未运行的）在前；未配置的运行中程序按进程名排序追加，
//   一律不自动分配字母（键位留空显示 ·，鼠标可点选、✎ 手动配键）。字母只能由用户显式配置。
// 未运行的配置程序排到最后。
fn build_prog_list(
    cfg: &Config,
    wins_by_proc: &HashMap<String, Vec<WinInfo>>,
) -> Vec<ProgEntry> {
    let mut list: Vec<ProgEntry> = Vec::new();
    for p in &cfg.programs {
        list.push(ProgEntry {
            key: p.key.clone(),
            multi_key: p.multi_key.clone(),
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
        let path = wins_by_proc.get(proc).and_then(|wins| wins.first()).map(|w| w.path.clone());
        // 取 exe 文件名去 .exe 作为回退名；路径缺失/为空时回退到进程名
        let stem = path
            .as_deref()
            .filter(|p| !p.is_empty())
            .and_then(|p| p.rsplit('\\').next())
            .map(|s| s.trim_end_matches(".exe"))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| proc.trim_end_matches(".exe"))
            .to_string();
        let name = path
            .as_deref()
            .filter(|p| !p.is_empty())
            .and_then(windows::file_description)
            .unwrap_or(stem);
        list.push(ProgEntry {
            key: String::new(),
            multi_key: String::new(),
            name,
            process: proc.clone(),
            configured: false,
        });
    }
    // 未运行的配置程序排到最后（运行中在前：配置序、未配置运行中次之；稳定排序保持组内顺序）
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
    let cfg = inner.cfg.lock().unwrap().clone();
    let mut wins_by_proc: HashMap<String, Vec<WinInfo>> = HashMap::new();
    for w in all.drain(..) {
        if cfg.blocked.iter().any(|b| b.process() == w.process) {
            continue; // 黑名单（系统预置 + 用户屏蔽）
        }
        wins_by_proc.entry(w.process.clone()).or_default().push(w);
    }
    let mut ov = inner.overlay.lock().unwrap();
    ov.phase = Phase::Programs;
    ov.prog_list = build_prog_list(&cfg, &wins_by_proc);
    ov.wins_by_proc = wins_by_proc;
    ov.prog_sel = 0;
    ov.prog_page = 0;
    ov.letter_buf.clear();
    ov.sel_proc = None;
    ov.wins.clear();
    ov.digit_buf.clear();
    ov.switched = false;
    ov.last_activated = 0;
    let fg = windows::foreground();
    inner.prev_fg.store(fg, Ordering::Relaxed);
    // 把呼出前的前台窗口记入 MRU：MRU 原本只在经 WinHop 切换时更新，
    // 用户用鼠标/任务栏切走后，Space 快速跳转就会拿错目标（跳过的"当前窗口"不是当前窗口）
    if fg != 0 {
        inner
            .mru
            .lock()
            .unwrap()
            .insert(fg, windows::now_ms());
    }
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
            // 夺焦校验：覆盖层必须拿到键盘焦点，否则按键仍进底层程序（用户看着
            // 选择页打字打到后台）。失败即关闭，还原呼出前窗口。
            if windows::foreground() != hwnd.0 as isize {
                eprintln!(
                    "[t={}] 覆盖层夺焦失败，关闭（前台被其它窗口锁定）",
                    windows::now_ms()
                );
                drop(ov); // close 会锁 overlay，必须先释放
                close(app);
                return;
            }
        }
    }
    emit(app, &inner, &ov);
}

fn close(app: &AppHandle) {
    close_impl(app, true);
}

// 焦点丢失触发的关闭（Alt+Tab / Win 键离开覆盖层）：用户已主动切到别的窗口，
// 不还原 prev_fg，否则会把用户刚选的目标窗口强夺回旧窗口
fn close_no_restore(app: &AppHandle) {
    close_impl(app, false);
}

fn close_impl(app: &AppHandle, restore_prev: bool) {
    let inner = app.state::<Inner>();
    if !inner.visible.load(Ordering::Relaxed) {
        eprintln!("[winhop] close 被跳过（已关闭）");
        return;
    }
    let ov = inner.overlay.lock().unwrap();
    inner.visible.store(false, Ordering::Relaxed);
    eprintln!(
        "[t={}] overlay close (switched={})",
        windows::now_ms(),
        ov.switched
    );
    let switched = ov.switched;
    drop(ov);
    windows::set_overlay_hwnd(0);
    windows::thumb_clear();
    // 先隐藏再处理尺寸：若先退全屏，面板会在屏幕上可见地缩小跳动（闪烁）
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    // 决定要激活的目标窗口（先记下来，spawn 放到 emit 之后）。
    // pending（用户明确选了窗口）总是优先；否则仅 restore_prev 路径回退 prev_fg
    let pending = inner.pending_activate.swap(0, Ordering::Relaxed);
    let target = if pending != 0 {
        pending
    } else if restore_prev && !switched {
        inner.prev_fg.load(Ordering::Relaxed)
    } else {
        0
    };
    // 先完成覆盖层收尾（emit visible=false），再启动激活线程。
    // 关键顺序：emit 会向刚 hide 的 WebView2 发 IPC，必须在外部 SetForegroundWindow
    // 抢焦点之前完成。若后台线程先抢走焦点，WebView2 在处理 hide+IPC 时会阻塞主线程，
    // 连带挂住鼠标 LL 钩子（光标卡顿）和 WM_HOTKEY 派发（热键唤不起）——这是竞态，
    // 不能靠 sleep/日志延迟掩盖，必须用顺序保证。
    let cfg = inner.cfg.lock().unwrap();
    let render = Render {
        visible: false,
        phase: "programs".into(),
        title: String::new(),
        window_order: cfg.window_order.clone(),
        multi_letter: cfg.multi_letter,
        theme: cfg.theme.clone(),
        win_digit_mode: cfg.win_digit_mode.clone(),
        filter: String::new(),
        programs: Vec::new(),
        windows: Vec::new(),
        page: 1,
        page_count: 1,
        win_digit: String::new(),
    };
    drop(cfg);
    let _ = app.emit("overlay", &render);
    // 收尾完成，再在独立线程激活目标（AttachThreadInput 已移除，激活不阻塞主线程；
    // 独立线程保险，任何目标窗口的慢响应都不影响钩子/热键）。
    if target != 0 {
        std::thread::spawn(move || {
            windows::activate_with_retry(target);
        });
    }
}

// 覆盖层整体全屏（Win+Tab 风格选择页）
fn set_overlay_fullscreen(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        if !win.is_fullscreen().unwrap_or(false) {
            let _ = win.set_fullscreen(true);
        }
    }
}

// 激活操作放独立线程执行：AttachThreadInput 可能被慢目标线程阻塞，不能占主线程
fn deferred_activate(_app: &AppHandle, hwnd: isize) {
    std::thread::spawn(move || {
        let _ = windows::activate(hwnd);
    });
}

// 选中程序层条目：单窗口直切返回 true（调用方负责解锁并 close），多窗口进窗口层，
// 窗口层中重复选中同一程序则轮询切下一窗口
fn select_entry(app: &AppHandle, inner: &Inner, ov: &mut OverlayState, entry: &ProgEntry) -> bool {
    let wins = match ov.wins_by_proc.get(&entry.process) {
        Some(w) if !w.is_empty() => w,
        _ => {
            eprintln!("[winhop] '{}' 无窗口", entry.name);
            return false;
        }
    };
    let now = windows::now_ms();
    if ov.phase == Phase::Windows && ov.sel_proc.as_deref() == Some(entry.process.as_str()) {
        ov.active = (ov.active + 1) % wins.len();
        ov.last_activated = wins[ov.active].hwnd;
        inner.mru.lock().unwrap().insert(wins[ov.active].hwnd, now);
        deferred_activate(app, wins[ov.active].hwnd);
        eprintln!("[winhop] 轮询 {} 窗口 {}", entry.name, ov.active + 1);
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
        ov.letter_buf.clear();
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
        eprintln!("[winhop] '{}' -> {} 窗口数 {}", entry.key, entry.name, ov.wins.len());
        emit(app, inner, ov);
        false
    }
}

// 确保 prog_page 是 prog_sel 在当前视图中所在页（视图在多字母模式下会被筛选/重排）
fn sync_page(ov: &mut OverlayState, multi: bool) {
    let view = view_indices(ov, multi);
    if let Some(pos) = view.iter().position(|&i| i == ov.prog_sel) {
        ov.prog_page = pos / PROG_PAGE_SIZE;
    }
}

fn select_by_letter(app: &AppHandle, inner: &Inner, ov: &mut OverlayState, c: char) -> bool {
    let Some((idx, entry)) = ov
        .prog_list
        .iter()
        .enumerate()
        .find(|(_, e)| e.key == c.to_string())
        .map(|(i, e)| (i, e.clone()))
    else {
        eprintln!("[winhop] letter '{}' 无对应程序", c);
        return false;
    };
    ov.prog_sel = idx;
    let multi = inner.cfg.lock().unwrap().multi_letter;
    sync_page(&mut *ov, multi);
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
    eprintln!("[winhop] 切换到窗口 {}", idx + 1);
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
        "back" => HookMsg::Backspace,
        "space" => HookMsg::Space,
        "enter" => HookMsg::Enter,
        "hotkey" => HookMsg::Hotkey,
        s if s.starts_with("letter:") => {
            HookMsg::Letter(s[7..].chars().next().unwrap_or(' '))
        }
        s if s.starts_with("digit:") => HookMsg::Digit(s[6..].chars().next().unwrap_or('0')),
        s if s.starts_with("jump:") => HookMsg::Jump(
            s[5..]
                .parse::<usize>()
                .map(|n| n.saturating_sub(1))
                .unwrap_or(0),
        ),
        _ => return,
    };
    handle_key(&app, msg);
}

// DWM 缩略图注册/更新：大预览用 slot "pane"，行缩略图用 "row:<hwnd>"。
// x/y/w/h = 元素区域，ax/ay/aw/ah = 可视裁剪（0 表示不裁剪），覆盖层客户区物理像素
#[tauri::command]
fn thumb_set(
    slot: String,
    hwnd: isize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    ax: i32,
    ay: i32,
    aw: i32,
    ah: i32,
) {
    windows::thumb_set(slot, hwnd, x, y, w, h, ax, ay, aw, ah);
}

#[tauri::command]
fn thumb_clear() {
    windows::thumb_clear();
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

// 设置页打开时临时注销全局热键：否则录制中按下当前热键会被系统作为 WM_HOTKEY
// 吞掉（webview 收不到 keydown，且触发 toggle）。保存时注册新键，放弃时 hotkey_resume 恢复。
#[tauri::command]
fn hotkey_suspend(app: AppHandle) {
    let r = app.global_shortcut().unregister_all();
    eprintln!("[winhop] hotkey_suspend unregister_all={:?}", r);
}

// 放弃修改 / 直接返回：恢复注册配置中的（旧）热键
#[tauri::command]
fn hotkey_resume(app: AppHandle) -> Result<(), String> {
    let inner = app.state::<Inner>();
    let hk = inner.cfg.lock().unwrap().hotkey.clone();
    let sc = Shortcut::from_str(&hk).map_err(|e| format!("热键「{}」无效: {}", hk, e))?;
    let r = app.global_shortcut().register(sc);
    eprintln!("[winhop] hotkey_resume {} register={:?}", hk, r);
    r.map_err(|e| format!("恢复热键失败: {}", e))?;
    Ok(())
}

// ===== 热键录制：Rust 侧轮询 GetAsyncKeyState 检测组合 =====
// webview 事件会被中文输入法吞掉（Ctrl+Space 的 keydown 被 IME 用于切中英），
// 物理键状态 GetAsyncKeyState 不受影响——录制改走轮询，绕开事件系统。
static CAPTURE: std::sync::OnceLock<std::sync::Mutex<Option<String>>> =
    std::sync::OnceLock::new();
static CAPTURE_ON: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

const VK_MODS: [(i32, &'static str); 6] = [
    (0x11, "ctrl"),
    (0xA2, "ctrl"),
    (0xA3, "ctrl"),
    (0x12, "alt"),
    (0x10, "shift"),
    (0x5B, "super"),
];

fn mods_down() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for (vk, name) in VK_MODS {
        if unsafe { (windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk) as i32 & 0x8000) != 0 }
            && !out.iter().any(|n| *n == name)
        {
            out.push(name);
        }
    }
    // 固定顺序：ctrl alt shift super
    let order = ["ctrl", "alt", "shift", "super"];
    out.sort_by_key(|n| order.iter().position(|o| o == n).unwrap_or(99));
    out
}

// 主键 vk → 组合串里的键名；None 表示该 vk 是修饰键
fn vk_key_name(vk: i32) -> Option<String> {
    match vk {
        0x20 => Some("space".into()),
        0x41..=0x5A => Some(((b'a' + (vk - 0x41) as u8) as char).to_string()),
        0x30..=0x39 => Some(((b'0' + (vk - 0x30) as u8) as char).to_string()),
        0x70..=0x87 => Some(format!("f{}", vk - 0x70 + 1)),
        _ => None,
    }
}

// 开始检测：后台线程轮询，主键「按下沿」+ 修饰键按住 → 记录组合，一次后停止
#[tauri::command]
fn hotkey_capture_start() {
    use std::sync::atomic::Ordering;
    use std::collections::HashSet;
    CAPTURE.get_or_init(|| std::sync::Mutex::new(None));
    CAPTURE_ON.store(true, Ordering::Relaxed);
    std::thread::spawn(|| {
        let mut prev: HashSet<i32> = HashSet::new();
        let mut prev_mods: Vec<&'static str> = Vec::new();
        while CAPTURE_ON.load(Ordering::Relaxed) {
            let mods = mods_down();
            let mut now: HashSet<i32> = HashSet::new();
            for vk in 0x41..=0x5A { if key_down(vk) { now.insert(vk); } } // A-Z
            for vk in 0x30..=0x39 { if key_down(vk) { now.insert(vk); } } // 0-9
            for vk in 0x70..=0x87 { if key_down(vk) { now.insert(vk); } } // F1-F24
            if key_down(0x20) { now.insert(0x20); } // Space
            // 方向 1：主键按下沿（上一轮未按、本轮按下）且修饰键已按住
            for &vk in now.difference(&prev) {
                if mods.is_empty() { break; }
                if capture_hit(&mods, vk) { return; }
            }
            // 方向 2：修饰键刚按下（按下沿）且已有主键按住（先按主键/同时按）
            if !mods.is_empty()
                && mods.iter().any(|m| !prev_mods.contains(m))
                && !now.is_empty()
            {
                let vk = *now.iter().next().unwrap();
                if capture_hit(&mods, vk) { return; }
            }
            prev = now;
            prev_mods = mods;
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    });
}

// 命中组合：mods + 主键 vk → 记录并停线程；返回 true 表示已命中
fn capture_hit(mods: &[&'static str], vk: i32) -> bool {
    if let Some(name) = vk_key_name(vk) {
        let mut combo = mods.to_vec();
        combo.push(name.as_str());
        if let Some(slot) = CAPTURE.get() {
            *slot.lock().unwrap() = Some(combo.join("+"));
        }
        CAPTURE_ON.store(false, std::sync::atomic::Ordering::Relaxed);
        return true;
    }
    false
}

fn key_down(vk: i32) -> bool {
    unsafe {
        (windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk) as i32 & 0x8000) != 0
    }
}

#[tauri::command]
fn hotkey_capture_poll() -> Option<String> {
    CAPTURE.get().and_then(|m| m.lock().unwrap().take())
}

#[tauri::command]
fn hotkey_capture_stop() {
    use std::sync::atomic::Ordering;
    CAPTURE_ON.store(false, Ordering::Relaxed);
    if let Some(m) = CAPTURE.get() {
        *m.lock().unwrap() = None;
    }
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    eprintln!("[t={}] 设置面板退出", windows::now_ms());
    app.exit(0);
}

// 编辑程序（已配置改代号/名称；未配置添加进配置）。按 process 匹配。
// multi=true 时写 multi_key（多字母代号，1+ 小写字母），否则写 key（单字母）。
// 代号格式校验：空=清除字母绑定（单字母模式允许，表示解绑）；非空必须全小写，
// 单字母模式非空时必须恰 1 个字母。
fn validate_program_key(key: &str, multi: bool) -> Result<(), String> {
    if !key.is_empty() && !key.bytes().all(|b| b.is_ascii_lowercase()) {
        return Err("代号必须全为小写字母".into());
    }
    if !multi && !key.is_empty() && key.len() != 1 {
        return Err("单字母模式代号必须是单个字母".into());
    }
    Ok(())
}

// 纯逻辑：把一次程序编辑应用到 cfg（已存在则更新、否则新增），含代号占用冲突检测。
// 调用方负责 name 非空、代号格式校验与落盘。空 key 不查重（空键不占字母）。
fn apply_program_edit(
    cfg: &mut Config,
    process: &str,
    key: &str,
    multi: bool,
    name: &str,
) -> Result<(), String> {
    if let Some(idx) = cfg.programs.iter().position(|p| p.process == process) {
        if multi {
            if !key.is_empty() {
                let conflict = cfg
                    .programs
                    .iter()
                    .enumerate()
                    .any(|(j, p)| j != idx && p.multi_key == key);
                if conflict {
                    return Err(format!("多字母代号「{}」已被占用", key));
                }
            }
            cfg.programs[idx].multi_key = key.to_string();
        } else {
            if !key.is_empty() {
                let conflict = cfg
                    .programs
                    .iter()
                    .enumerate()
                    .any(|(j, p)| j != idx && p.key == key);
                if conflict {
                    return Err(format!("字母「{}」已被占用", key));
                }
            }
            cfg.programs[idx].key = key.to_string();
        }
        cfg.programs[idx].name = name.to_string();
    } else {
        // 新增：另一模式的代号留空
        let conflict = if multi {
            !key.is_empty() && cfg.programs.iter().any(|p| p.multi_key == key)
        } else {
            !key.is_empty() && cfg.programs.iter().any(|p| p.key == key)
        };
        if conflict {
            return Err(format!("代号「{}」已被占用", key));
        }
        cfg.programs.push(Program {
            key: if multi { String::new() } else { key.to_string() },
            multi_key: if multi { key.to_string() } else { String::new() },
            name: name.to_string(),
            process: process.to_string(),
        });
    }
    Ok(())
}

#[tauri::command]
fn edit_program(
    app: AppHandle,
    process: String,
    key: String,
    multi: bool,
    name: String,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("名称不能为空".into());
    }
    validate_program_key(&key, multi)?;
    let inner = app.state::<Inner>();
    {
        let mut cfg = inner.cfg.lock().unwrap();
        apply_program_edit(&mut cfg, &process, &key, multi, name)?;
        config::save(&cfg, &inner.cfg_path).map_err(|e| format!("保存配置失败: {}", e))?;
    }
    eprintln!(
        "[t={}] 编辑程序 {} ({}) {}={}",
        windows::now_ms(),
        name,
        process,
        if multi { "multi_key" } else { "key" },
        key
    );
    if inner.visible.load(Ordering::Relaxed) {
        rebuild_and_emit(&app, &inner);
    }
    Ok(())
}

// 从当前可见窗口重建程序列表并 emit（编辑/屏蔽后刷新覆盖层）
fn rebuild_and_emit(app: &AppHandle, inner: &Inner) {
    let mut ov = inner.overlay.lock().unwrap();
    let cfg = inner.cfg.lock().unwrap().clone();
    // 重新枚举，让新屏蔽的程序从 wins_by_proc 中消失
    let mut wins: HashMap<String, Vec<WinInfo>> = HashMap::new();
    for w in windows::enum_windows() {
        if cfg.blocked.iter().any(|b| b.process() == w.process) {
            continue;
        }
        wins.entry(w.process.clone()).or_default().push(w);
    }
    ov.wins_by_proc = wins;
    ov.prog_list = build_prog_list(&cfg, &ov.wins_by_proc);
    ov.prog_sel = 0;
    ov.prog_page = 0;
    ov.letter_buf.clear();
    emit(app, inner, &ov);
}

// 屏蔽程序：加入黑名单（note 取程序显示名，方便设置页对应；同时移除已配置代号，避免占键位）
#[tauri::command]
fn block_program(app: AppHandle, process: String, note: String) -> Result<(), String> {
    let process = process.to_lowercase();
    let inner = app.state::<Inner>();
    {
        let mut cfg = inner.cfg.lock().unwrap();
        cfg.blocked.retain(|b| b.process() != process);
        cfg.blocked.push(config::Blocked::Entry {
            process: process.clone(),
            note: note.trim().to_string(),
        });
        cfg.blocked.sort_by(|a, b| a.process().cmp(b.process()));
        // 屏蔽即移除其配置条目（代号/名称），不再出现在列表也不占键
        cfg.programs.retain(|p| p.process != process);
        config::save(&cfg, &inner.cfg_path).map_err(|e| format!("保存配置失败: {}", e))?;
    }
    eprintln!("[t={}] 屏蔽程序 {} ({})", windows::now_ms(), process, note);
    if inner.visible.load(Ordering::Relaxed) {
        rebuild_and_emit(&app, &inner);
    }
    Ok(())
}

// 解除屏蔽
#[tauri::command]
fn unblock_program(app: AppHandle, process: String) -> Result<(), String> {
    let process = process.to_lowercase();
    let inner = app.state::<Inner>();
    {
        let mut cfg = inner.cfg.lock().unwrap();
        cfg.blocked.retain(|b| b.process() != process);
        config::save(&cfg, &inner.cfg_path).map_err(|e| format!("保存配置失败: {}", e))?;
    }
    eprintln!("[t={}] 解除屏蔽 {}", windows::now_ms(), process);
    if inner.visible.load(Ordering::Relaxed) {
        rebuild_and_emit(&app, &inner);
    }
    Ok(())
}
// 不能复用 letter 路径）。若该行已高亮则等同 Enter 确认。
#[tauri::command]
fn pick_program(app: AppHandle, process: String) {
    let inner = app.state::<Inner>();
    if !inner.visible.load(Ordering::Relaxed) {
        return;
    }
    let mut ov = inner.overlay.lock().unwrap();
    if ov.phase != Phase::Programs || ov.prog_list.is_empty() {
        return;
    }
    let multi = inner.cfg.lock().unwrap().multi_letter;
    let view = view_indices(&ov, multi);
    let Some(pos) = view
        .iter()
        .position(|&i| ov.prog_list[i].process == process)
    else {
        return;
    };
    let target = view[pos];
    let entry = ov.prog_list[target].clone();
    // 点击已高亮项 = 确认；否则只移动高亮
    if target == ov.prog_sel {
        if select_entry(&app, &inner, &mut ov, &entry) {
            drop(ov);
            close(&app);
        }
    } else {
        ov.prog_sel = target;
        sync_page(&mut ov, multi);
        emit(&app, &inner, &ov);
    }
}

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
    version: "0.3.1",
    date: "2026-09",
    notes_zh: &[
        "新增开机自启：设置页勾选即可登录 Windows 时自动启动（写入系统启动项）",
        "新增全局热键录制：设置页点「录制」按下组合键即可，支持 Ctrl+Space（中文输入法下也能录），保存后生效",
        "字母改为手动配置：不再自动给程序分配字母，未配置的程序显示「·」；✎ 面板里把字母删空保存即可清除绑定",
        "界面统一：所有按钮改为主题色实心样式、可点与否一目了然，输入框/筛选框统一主题色边框",
        "程序列表固定卡片高度、每页最多 20 个；长代号（如 settings）完整显示不截断",
    ],
    notes_en: &[
        "Launch at startup: tick it in settings to start WinHop automatically when you sign in to Windows",
        "Global-hotkey recording: click \"Record\" in settings and press a combo — works even for Ctrl+Space under a Chinese IME; applies on Save",
        "Letters are now manual only: programs are no longer auto-assigned a letter (unconfigured ones show \"·\"); clear the letter in the ✎ panel and save to unbind",
        "Unified look: all buttons use a solid theme-color style so clickable vs disabled is obvious; inputs/filter boxes share themed borders",
        "Fixed-height program cards, up to 20 per page; long codes (e.g. \"settings\") are shown in full without truncation",
    ],
};

#[derive(Serialize, Clone)]
struct SettingsInfo {
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
    name: String,
}

#[derive(Serialize, Clone)]
struct ChangelogUi {
    version: String,
    date: String,
    notes_zh: Vec<String>,
    notes_en: Vec<String>,
}

// 主题 id → 显示名（与前端 styles.css 的 [data-theme=...] 对应）
fn theme_name(id: &str) -> &'static str {
    // 调用方只遍历 config::THEMES，未知 id 不会出现；兜底给静态串
    match id {
        "black-green" => "黑绿",
        "black-yellow" => "黑黄",
        _ => "未知主题",
    }
}

// 读取当前设置与版本/更新记录（设置页打开时调用，不立即写盘）
#[tauri::command]
fn get_settings(app: AppHandle) -> SettingsInfo {
    let inner = app.state::<Inner>();
    let cfg = inner.cfg.lock().unwrap();
    SettingsInfo {
        version: APP_VERSION.into(),
        hotkey: cfg.hotkey.clone(),
        autostart: cfg.autostart,
        window_order: cfg.window_order.clone(),
        multi_letter: cfg.multi_letter,
        theme: cfg.theme.clone(),
        win_digit_mode: cfg.win_digit_mode.clone(),
        // 当前生效语言：配置指定优先，空则跟随系统
        lang: if cfg.lang.is_empty() {
            windows::system_lang().to_string()
        } else {
            cfg.lang.clone()
        },
        lang_cfg: cfg.lang.clone(),
        lang_sys: windows::system_lang().to_string(),
        themes: config::THEMES
            .iter()
            .map(|id| ThemeUi {
                id: (*id).into(),
                name: theme_name(id).into(),
            })
            .collect(),
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
struct SettingsInput {
    /// 新全局热键（global-shortcut 串）；设置页打开期间热键已 suspend
    #[serde(default)]
    hotkey: String,
    #[serde(default)]
    autostart: bool,
    window_order: String,
    multi_letter: bool,
    theme: String,
    win_digit_mode: String,
    /// 界面语言（"zh-CN"/"en"；空串=跟随系统，由前端传 system 表达）
    #[serde(default)]
    lang: String,
    /// 设置页保存时黑名单保留的进程名（被解除的不在其中）
    #[serde(default)]
    blocked: Vec<String>,
}

// 批量保存设置（设置页点保存时调用）
#[tauri::command]
fn save_settings(app: AppHandle, input: SettingsInput) -> Result<(), String> {
    if input.window_order != "zorder" && input.window_order != "mru" {
        return Err(format!("无效的排序方式「{}」", input.window_order));
    }
    if !config::THEMES.contains(&input.theme.as_str()) {
        return Err(format!("无效的主题「{}」", input.theme));
    }
    if input.win_digit_mode != "jump" && input.win_digit_mode != "preview" {
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
    let old_hotkey = inner.cfg.lock().unwrap().hotkey.clone();
    // 先注册新键：失败则回退注册旧键，配置不变更
    if let Some(sc) = new_sc {
        if let Err(e) = app.global_shortcut().register(sc) {
            eprintln!("[winhop] 新热键注册失败 {:?}: {}，回退旧键", new_hotkey, e);
            if let Ok(old) = Shortcut::from_str(&old_hotkey) {
                let _ = app.global_shortcut().register(old);
            }
            return Err(format!("热键「{}」注册失败（可能被其它程序占用）", new_hotkey));
        }
    } else if let Ok(old) = Shortcut::from_str(&old_hotkey) {
        // 未改热键：恢复注册旧键（suspend 期间被注销）
        let _ = app.global_shortcut().register(old);
    }
    // 开机自启：先落地注册表（HKCU\...\Run），失败则整体不保存（与热键注册同一模式）
    let old_autostart = inner.cfg.lock().unwrap().autostart;
    if input.autostart != old_autostart {
        if let Err(e) = windows::set_autostart(input.autostart) {
            eprintln!("[winhop] 自启注册表写入失败: {}", e);
            return Err(format!("设置开机自启失败: {}", e));
        }
    }
    let save_res = {
        let mut cfg = inner.cfg.lock().unwrap();
        cfg.window_order = input.window_order.clone();
        cfg.multi_letter = input.multi_letter;
        cfg.autostart = input.autostart;
        cfg.theme = input.theme.clone();
        cfg.win_digit_mode = input.win_digit_mode.clone();
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
        // 写盘失败（罕见）：回滚热键到旧值
        if let Ok(old) = Shortcut::from_str(&old_hotkey) {
            let _ = app.global_shortcut().unregister_all();
            let _ = app.global_shortcut().register(old);
        }
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

// 设置页保存关闭后：重新枚举并 emit，让程序列表立即按新设置（模式/黑名单等）刷新
#[tauri::command]
fn refresh_overlay(app: AppHandle) {
    let inner = app.state::<Inner>();
    if inner.visible.load(Ordering::Relaxed) {
        rebuild_and_emit(&app, &inner);
    }
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
                ov.letter_buf.clear();
                emit(app, &inner, &ov);
            }
            Phase::Programs => {
                if !ov.letter_buf.is_empty() {
                    // 多字母模式：先清筛选，再 Esc 才关闭
                    ov.letter_buf.clear();
                    ov.prog_page = 0;
                    emit(app, &inner, &ov);
                } else {
                    drop(ov);
                    close(app);
                }
            }
            Phase::Closed => {}
        },
        HookMsg::Letter(c) => {
            let multi = inner.cfg.lock().unwrap().multi_letter;
            if multi {
                if ov.phase != Phase::Programs {
                    return;
                }
                ov.letter_buf.push(c);
                // 选中匹配度最高的（列表第一个）
                let v = view_indices(&ov, true);
                ov.prog_sel = v.first().copied().unwrap_or(ov.prog_sel);
                ov.prog_page = 0;
                emit(app, &inner, &ov);
            } else if select_by_letter(app, &inner, &mut ov, c) {
                drop(ov);
                close(app);
            }
        }
        HookMsg::Backspace => {
            if ov.phase == Phase::Programs && !ov.letter_buf.is_empty() {
                ov.letter_buf.pop();
                let v = view_indices(&ov, true);
                if v.contains(&ov.prog_sel) {
                    // 当前选中仍在结果内则保留
                } else {
                    ov.prog_sel = v.first().copied().unwrap_or(ov.prog_sel);
                }
                ov.prog_page = 0;
                emit(app, &inner, &ov);
            } else if ov.phase == Phase::Windows
                && inner.cfg.lock().unwrap().multi_letter
                && ov.wins.len() > 9
                && !ov.digit_buf.is_empty()
            {
                // 多字母模式且窗口 >9（组合编号）：删一位，剩余索引有效则回退聚焦
                ov.digit_buf.pop();
                if let Ok(n) = ov.digit_buf.parse::<usize>() {
                    if n >= 1 && n <= ov.wins.len() {
                        ov.active = n - 1;
                    }
                }
                emit(app, &inner, &ov);
            }
        }
        HookMsg::Digit(d) => {
            if ov.phase != Phase::Windows || ov.wins.is_empty() {
                return;
            }
            let (multi, preview_mode) = {
                let cfg = inner.cfg.lock().unwrap();
                (cfg.multi_letter, cfg.win_digit_mode == "preview")
            };
            let total = ov.wins.len();
            if !multi {
                // 单字母模式：数字累积，n*10 > total（再加一位必超总数）立即跳转
                ov.digit_buf.push(d);
                if let Ok(n) = ov.digit_buf.parse::<usize>() {
                    if n >= 1 && n * 10 > total && resolve_window(&inner, &mut ov, n) {
                        drop(ov);
                        close(app);
                    }
                }
            } else if total <= 9 {
                // 窗口 ≤9：每个数字独立，按到即定（无需退格/组合）
                let n = d.to_digit(10).unwrap_or(0) as usize;
                if n < 1 || n > total {
                    return;
                }
                ov.digit_buf = n.to_string();
                if preview_mode {
                    ov.active = n - 1;
                    emit(app, &inner, &ov);
                } else if resolve_window(&inner, &mut ov, n) {
                    drop(ov);
                    close(app);
                }
            } else {
                // 窗口 >9：组合编号（1 然后 2 = 12），Backspace 退格
                ov.digit_buf.push(d);
                let n = match ov.digit_buf.parse::<usize>() {
                    Ok(n) => n,
                    Err(_) => {
                        ov.digit_buf.pop();
                        return;
                    }
                };
                if n < 1 || n > total {
                    ov.digit_buf.pop(); // 超界，忽略本次输入
                    return;
                }
                ov.active = n - 1; // Enter 也可确认当前输入
                if preview_mode {
                    emit(app, &inner, &ov);
                } else if n * 10 > total && resolve_window(&inner, &mut ov, n) {
                    // 再加一位必超总数 → 立即跳转；否则等下一位或 Enter 确认
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
                    let multi = inner.cfg.lock().unwrap().multi_letter;
                    let view = view_indices(&ov, multi);
                    if view.is_empty() {
                        return;
                    }
                    let pos = view.iter().position(|&i| i == ov.prog_sel).unwrap_or(0);
                    let len = view.len() as isize;
                    let new_pos = ((pos as isize + delta + len) % len) as usize;
                    ov.prog_sel = view[new_pos];
                    sync_page(&mut ov, multi);
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
            let multi = inner.cfg.lock().unwrap().multi_letter;
            let view = view_indices(&ov, multi);
            if view.is_empty() {
                return;
            }
            let page_count = view.len().div_ceil(PROG_PAGE_SIZE);
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
            let end = (start + PROG_PAGE_SIZE).min(view.len());
            let page_items = &view[start..end];
            // 选中保持在当前页内：超出则夹到页内首/末
            if !page_items.contains(&ov.prog_sel) {
                ov.prog_sel = if matches!(msg, HookMsg::PageDown) {
                    page_items[0]
                } else {
                    *page_items.last().unwrap()
                };
            }
            emit(app, &inner, &ov);
        }
        HookMsg::Space => {
            // 快速跳转：呼出后按空格，切到上一个最近使用窗口（两窗口互切，类似 Alt-Tab 瞬切）。
            // 取 MRU 中可见、非覆盖层的两个最新窗口，切到第二个。
            if ov.phase != Phase::Programs {
                return;
            }
            let overlay_hwnd = windows::get_overlay_hwnd();
            let mut recent: Vec<(u64, isize)> = {
                let mru = inner.mru.lock().unwrap();
                mru.iter()
                    .filter(|(&h, _)| h != 0 && h != overlay_hwnd)
                    .map(|(&h, &ts)| (ts, h))
                    .collect()
            };
            recent.sort_by(|a, b| b.0.cmp(&a.0)); // 时间戳降序
            // 跳过第一个（当前/最新），取第二个可见的
            let target = recent
                .iter()
                .filter(|(_, h)| windows::overlay_visible(*h))
                .nth(1)
                .map(|&(_, h)| h);
            if let Some(hwnd) = target {
                ov.switched = true;
                ov.last_activated = hwnd;
                inner
                    .mru
                    .lock()
                    .unwrap()
                    .insert(hwnd, windows::now_ms());
                inner.pending_activate.store(hwnd, Ordering::Relaxed);
                drop(ov);
                close(app);
            } else {
                eprintln!("[winhop] 空格快速跳转：无可切换的上一个窗口");
            }
        }
        HookMsg::Jump(n) => {
            // 点击窗口行：直接跳转（0-based → resolve 用 1-based）
            if ov.phase == Phase::Windows && resolve_window(&inner, &mut ov, n + 1) {
                drop(ov);
                close(app);
            }
        }
        HookMsg::Enter => {
            let done = match ov.phase {
                Phase::Programs => {
                    if ov.prog_list.is_empty() {
                        false
                    } else {
                        let multi = inner.cfg.lock().unwrap().multi_letter;
                        let view = view_indices(&ov, multi);
                        if view.is_empty() {
                            false // 无匹配，Enter 无动作
                        } else {
                            // 多字母筛选后 prog_sel 已是匹配最高项；若不在视图内（边界情况）取首项
                            let idx = if view.contains(&ov.prog_sel) {
                                ov.prog_sel
                            } else {
                                view[0]
                            };
                            let entry = ov.prog_list[idx].clone();
                            select_entry(app, &inner, &mut ov, &entry)
                        }
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
    eprintln!("=== winhop start ===");
    let (cfg, cfg_path) = config::load();
    // taskmgr 等管理员程序需要提权才能钩子生效/激活；debug 构建跳过（dev 迭代不弹 UAC）
    if cfg.elevate && !cfg!(debug_assertions) && !windows::is_elevated() {
        windows::relaunch_elevated();
        return;
    }
    if !windows::acquire_single_instance() {
        eprintln!("[winhop] 已有实例在运行，退出");
        return;
    }
    // 开机自启：以配置为准对齐 HKCU\...\Run（幂等；外部删过值/升级场景下自愈）
    if let Err(e) = windows::set_autostart(cfg.autostart) {
        eprintln!("[winhop] 对齐自启注册表失败: {}", e);
    }
    // 配置热键无效时回退默认，不让 app 崩溃
    let shortcut = Shortcut::from_str(&cfg.hotkey).unwrap_or_else(|e| {
        eprintln!(
            "配置热键「{}」无效: {}，回退默认 ctrl+space",
            cfg.hotkey, e
        );
        Shortcut::from_str("ctrl+space").expect("默认热键无效")
    });
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
            get_settings,
            save_settings,
            hotkey_suspend,
            hotkey_resume,
            hotkey_capture_start,
            hotkey_capture_poll,
            hotkey_capture_stop,
            quit_app,
            thumb_set,
            thumb_clear,
            toggle_fullscreen,
            edit_program,
            block_program,
            unblock_program,
            refresh_overlay,
            pick_program
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
            // 热键被其它程序占用时注册失败：不退出，托盘/鼠标钩子仍可用，只记日志
            if let Err(e) = app.global_shortcut().register(shortcut) {
                eprintln!("[winhop] 热键注册失败: {}（可用托盘图标唤出覆盖层）", e);
            }
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
                    close_no_restore(app);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod state_machine_tests {
    use super::*;

    fn entry(key: &str, mk: &str, name: &str, proc_: &str) -> ProgEntry {
        ProgEntry {
            key: key.into(),
            multi_key: mk.into(),
            name: name.into(),
            process: proc_.into(),
            configured: true,
        }
    }

    fn state_with(entries: Vec<ProgEntry>) -> OverlayState {
        let mut ov = OverlayState::default();
        ov.prog_list = entries;
        ov
    }

    // 构造仅含给定程序的最小 Config（其余字段默认）
    fn cfg_with(programs: Vec<Program>) -> Config {
        Config {
            hotkey: "ctrl+space".into(),
            elevate: true,
            autostart: false,
            window_order: "zorder".into(),
            multi_letter: false,
            theme: "black-green".into(),
            win_digit_mode: "jump".into(),
            lang: String::new(),
            programs,
            blocked: vec![],
            blocked_seeded: true,
        }
    }

    fn prog(key: &str, mk: &str, name: &str, proc_: &str) -> Program {
        Program {
            key: key.into(),
            multi_key: mk.into(),
            name: name.into(),
            process: proc_.into(),
        }
    }

    fn win(proc_: &str, path: &str) -> WinInfo {
        WinInfo { hwnd: 1, title: String::new(), process: proc_.into(), path: path.into(), monitor: 0 }
    }

    #[test]
    fn match_score_prefers_multi_key_then_name() {
        let e = entry("v", "vs", "VS Code", "code.exe");
        // 多字母代号精确 > 前缀 > 名称精确
        assert!(match_score(&e, "vs") >= 1000);
        assert!(match_score(&e, "v") >= 800); // 代号前缀
        assert_eq!(match_score(&e, "zzz"), 0); // 不匹配
        // 名称匹配
        let e2 = entry("", "", "记事本", "notepad.exe");
        assert!(match_score(&e2, "记事本") > 0);
        assert!(match_score(&e2, "记事") > 0);
    }

    #[test]
    fn view_indices_single_letter_is_full_order() {
        let ov = state_with(vec![entry("c", "", "Chrome", "chrome.exe"), entry("v", "", "Code", "code.exe")]);
        let v = view_indices(&ov, false);
        assert_eq!(v, vec![0, 1]); // 单字母模式不筛选，保持顺序
    }

    #[test]
    fn view_indices_multi_filters_and_ranks() {
        let ov = state_with(vec![
            entry("c", "ch", "Chrome", "chrome.exe"),
            entry("v", "vs", "VS Code", "code.exe"),
            entry("f", "ff", "Firefox", "firefox.exe"),
        ]);
        // 无输入 = 全量
        assert_eq!(view_indices(&ov, true).len(), 3);
        let mut ov = ov;
        // "vs" 精确命中 code，排第一且排除其它
        ov.letter_buf = "vs".into();
        let v = view_indices(&ov, true);
        assert_eq!(v, vec![1]);
        // "f" 命中 ff 代号前缀 + firefox 名
        ov.letter_buf = "f".into();
        let v = view_indices(&ov, true);
        assert!(v.contains(&2));
        assert!(!v.contains(&0));
    }

    #[test]
    fn build_prog_list_does_not_auto_assign_letters() {
        use config::Config;
        let cfg = Config {
            hotkey: "ctrl+space".into(),
            elevate: true,
            autostart: false,
            window_order: "zorder".into(),
            multi_letter: false,
            theme: "black-green".into(),
            win_digit_mode: "jump".into(),
            lang: String::new(),
            programs: vec![Program {
                key: "c".into(),
                multi_key: String::new(),
                name: "Chrome".into(),
                process: "chrome.exe".into(),
            }],
            blocked: vec![],
            blocked_seeded: true,
        };
        fn win(proc_: &str) -> WinInfo {
            WinInfo { hwnd: 1, title: String::new(), process: proc_.into(), path: String::new(), monitor: 0 }
        }
        let mut wins: HashMap<String, Vec<WinInfo>> = HashMap::new();
        wins.insert("chrome.exe".into(), vec![win("chrome.exe")]);
        wins.insert("notepad.exe".into(), vec![win("notepad.exe")]);
        wins.insert("spotify.exe".into(), vec![win("spotify.exe")]);

        let list = build_prog_list(&cfg, &wins);
        // 3 个进程都在（已配置 + 2 个未配置运行中）
        assert_eq!(list.len(), 3);
        let chrome = list.iter().find(|e| e.process == "chrome.exe").unwrap();
        assert_eq!(chrome.key, "c"); // 已配置保留字母
        // 未配置的运行中程序：字母留空（不再自动分配）
        for proc_ in ["notepad.exe", "spotify.exe"] {
            let e = list.iter().find(|x| x.process == proc_).unwrap();
            assert!(e.key.is_empty(), "{} 不应被自动分配字母", proc_);
            assert!(!e.configured);
        }
    }

    #[test]
    fn pagination_uses_page_size() {
        // 超过一页时页数 = ceil(n / PROG_PAGE_SIZE)
        let n = PROG_PAGE_SIZE * 2 + 3;
        let page_count = n.div_ceil(PROG_PAGE_SIZE);
        assert_eq!(page_count, 3);
        assert_eq!(PROG_PAGE_SIZE, 20);
    }

    #[test]
    fn build_prog_list_sorts_not_running_last_and_appends_unconfigured() {
        // 配置项：chrome 运行、notepad 未运行 → notepad 排最后；未配置运行中 mspaint 追加。
        //（blocked 由调用方在枚举时过滤，不进 wins_by_proc，故 build_prog_list 不负责）
        let cfg = cfg_with(vec![
            prog("c", "", "Chrome", "chrome.exe"),
            prog("n", "", "记事本", "notepad.exe"),
        ]);
        let mut wins: HashMap<String, Vec<WinInfo>> = HashMap::new();
        wins.insert("chrome.exe".into(), vec![win("chrome.exe", "C:\\chrome.exe")]);
        wins.insert("mspaint.exe".into(), vec![win("mspaint.exe", "C:\\mspaint.exe")]);
        let list = build_prog_list(&cfg, &wins);
        let procs: Vec<&str> = list.iter().map(|e| e.process.as_str()).collect();
        // notepad（未运行配置项）排最后；运行中的 chrome、mspaint 在前
        assert_eq!(*procs.last().unwrap(), "notepad.exe");
        assert!(procs.contains(&"mspaint.exe"));
        assert!(procs.contains(&"chrome.exe"));
        // 未配置运行中项
        let paint = list.iter().find(|e| e.process == "mspaint.exe").unwrap();
        assert!(!paint.configured);
        assert!(paint.key.is_empty());
    }

    #[test]
    fn build_prog_list_unconfigured_uses_fallback_name_without_path() {
        // 无路径（枚举拿不到 exe）时名称回退进程名去 .exe，不 panic
        let cfg = cfg_with(vec![]);
        let mut wins: HashMap<String, Vec<WinInfo>> = HashMap::new();
        wins.insert("wechat.exe".into(), vec![win("wechat.exe", "")]);
        let list = build_prog_list(&cfg, &wins);
        let w = &list[0];
        assert_eq!(w.process, "wechat.exe");
        assert_eq!(w.name, "wechat"); // 空路径 → 进程名去 .exe
    }

    #[test]
    fn vk_key_name_maps_keys() {
        // 空格 / 字母 / 数字 / 功能键 → 组合串名；修饰键等返回 None
        assert_eq!(vk_key_name(0x20).as_deref(), Some("space"));
        assert_eq!(vk_key_name(0x41).as_deref(), Some("a")); // A
        assert_eq!(vk_key_name(0x5A).as_deref(), Some("z")); // Z
        assert_eq!(vk_key_name(0x30).as_deref(), Some("0"));
        assert_eq!(vk_key_name(0x39).as_deref(), Some("9"));
        assert_eq!(vk_key_name(0x70).as_deref(), Some("f1"));
        assert_eq!(vk_key_name(0x87).as_deref(), Some("f24"));
        // 修饰键（Ctrl 0x11 / Alt 0x12 / Shift 0x10）不作主键
        assert!(vk_key_name(0x11).is_none());
        assert!(vk_key_name(0x12).is_none());
    }

    #[test]
    fn validate_program_key_rules() {
        // 空键合法（清除字母绑定），单字母/多字母模式都允许
        assert!(validate_program_key("", false).is_ok());
        assert!(validate_program_key("", true).is_ok());
        // 单字母模式：非空必须恰 1 个小写字母
        assert!(validate_program_key("c", false).is_ok());
        assert!(validate_program_key("ch", false).is_err());
        assert!(validate_program_key("C", false).is_err());
        // 多字母模式：1+ 小写字母
        assert!(validate_program_key("ch", true).is_ok());
        assert!(validate_program_key("CH", true).is_err());
        assert!(validate_program_key("c1", true).is_err());
    }

    #[test]
    fn apply_program_edit_detects_conflict_and_clears() {
        // 初始：chrome=c, code=v
        let mut cfg = cfg_with(vec![
            prog("c", "ch", "Chrome", "chrome.exe"),
            prog("v", "vs", "Code", "code.exe"),
        ]);
        // 单字母改键：占用 v → 报错
        assert!(apply_program_edit(&mut cfg, "chrome.exe", "v", false, "Chrome").is_err());
        // 改成空闲字母 a → 成功
        assert!(apply_program_edit(&mut cfg, "chrome.exe", "a", false, "Chrome").is_ok());
        assert_eq!(cfg.programs[0].key, "a");
        // 空键 = 清除字母（不查重），条目保留
        assert!(apply_program_edit(&mut cfg, "chrome.exe", "", false, "Chrome").is_ok());
        assert_eq!(cfg.programs[0].key, "");
        assert_eq!(cfg.programs.len(), 2);
        // 多字母代号冲突（vs 被 code 占）
        assert!(apply_program_edit(&mut cfg, "chrome.exe", "vs", true, "Chrome").is_err());
        // 新增程序：v 被 code 占 → 报错；f 空闲 → 成功
        assert!(apply_program_edit(&mut cfg, "firefox.exe", "v", false, "Firefox").is_err());
        assert!(apply_program_edit(&mut cfg, "firefox.exe", "f", false, "Firefox").is_ok());
        assert_eq!(cfg.programs.len(), 3);
        assert_eq!(cfg.programs[2].process, "firefox.exe");
        assert_eq!(cfg.programs[2].key, "f");
        assert_eq!(cfg.programs[2].multi_key, ""); // 单字母新增，多字母留空
    }

    #[test]
    fn default_config_roundtrips_and_invalid_hotkey_falls_back() {
        // autostart 等新字段序列化往返不丢
        let mut cfg = cfg_with(vec![prog("c", "ch", "Chrome", "chrome.exe")]);
        cfg.autostart = true;
        let dir = std::env::temp_dir().join(format!("winhop_cfg_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        config::save(&cfg, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let back: Config = serde_json::from_str(&text).unwrap();
        assert!(back.autostart);
        // 无效热键字符串不影响解析（回退逻辑在 run()，这里仅确认 Shortcut::from_str 报错而非 panic）
        assert!(Shortcut::from_str("not-a-valid-combo!!!").is_err());
        assert!(Shortcut::from_str("ctrl+space").is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn enum_windows_does_not_panic() {
        // 冒烟：窗口枚举在测试环境（可能无交互桌面/为空）也不得 panic
        let wins = windows::enum_windows();
        // 每条结果都应有进程名或标题；仅断言不 panic 与基本不变量
        for w in &wins {
            assert!(w.hwnd != 0);
        }
    }
}
