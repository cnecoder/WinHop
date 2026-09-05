mod config;
mod hotkey_capture;
mod settings;
mod windows;

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use config::{Config, Program, WinDigitMode, WindowOrder};
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
    /// 本次呼出已明确选定、待 close 后激活的窗口（0=无）
    pending: isize,
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
            pending: 0,
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

impl Render {
    // 覆盖层关闭时下发的空渲染（visible=false，列表清空）
    fn closed(cfg: &Config) -> Render {
        Render {
            visible: false,
            phase: "programs".into(),
            title: String::new(),
            window_order: cfg.window_order.as_str().into(),
            multi_letter: cfg.multi_letter,
            theme: cfg.theme.clone(),
            win_digit_mode: cfg.win_digit_mode.as_str().into(),
            filter: String::new(),
            programs: Vec::new(),
            windows: Vec::new(),
            page: 1,
            page_count: 1,
            win_digit: String::new(),
        }
    }
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

/// 状态机一次按键的副作用：纯逻辑只产出效果，由驱动层落地
/// （emit 渲染 / close 收尾 / 激活轮询目标；轮询切换的实时激活本就绕过状态机）。
#[derive(Debug, PartialEq, Clone, Copy)]
enum Effect {
    /// 状态未变
    None,
    /// 状态已改，需要重新 emit 渲染
    Emit,
    /// 用户选定窗口（pending 已置），驱动层应 close
    Close,
    /// 轮询切换：驱动层立即在独立线程激活（不关闭覆盖层）
    ActivateWindow(isize),
}

impl OverlayState {
    /// 纯状态转换：依据 msg/配置/MRU 修改自身并返回副作用。
    /// - `cfg`：当前配置快照（调用方一次性 clone，转换中不持锁）
    /// - `mru`：最近使用窗口时间戳表（可读可写）
    /// - `now`：当前时间戳（驱动层传入 windows::now_ms，测试可固定）
    /// - `overlay_hwnd`：覆盖层自身句柄（Space 快速跳转时排除）
    /// - `is_visible`：句柄是否为可见窗口（Space 过滤用，测试可注入）
    /// select_entry 的轮询激活返回 ActivateWindow（驱动层 deferred_activate），
    /// 实时激活本就不经状态机/close，故不进 mru 之外的共享状态。
    fn transition(
        &mut self,
        msg: &HookMsg,
        cfg: &Config,
        mru: &mut HashMap<isize, u64>,
        now: u64,
        overlay_hwnd: isize,
        is_visible: &dyn Fn(isize) -> bool,
    ) -> Effect {
        match msg {
            HookMsg::Esc => match self.phase {
                Phase::Windows => {
                    self.phase = Phase::Programs;
                    self.sel_proc = None;
                    self.digit_buf.clear();
                    self.letter_buf.clear();
                    Effect::Emit
                }
                Phase::Programs => {
                    if !self.letter_buf.is_empty() {
                        self.letter_buf.clear();
                        self.prog_page = 0;
                        Effect::Emit
                    } else {
                        Effect::Close
                    }
                }
                Phase::Closed => Effect::None,
            },
            HookMsg::ClickOutside => Effect::Close,
            HookMsg::Letter(c) => {
                if cfg.multi_letter {
                    if self.phase != Phase::Programs {
                        return Effect::None;
                    }
                    self.letter_buf.push(*c);
                    let v = view_indices(self, true);
                    self.prog_sel = v.first().copied().unwrap_or(self.prog_sel);
                    self.prog_page = 0;
                    Effect::Emit
                } else {
                    self.select_by_letter(*c, cfg, mru, now)
                }
            }
            HookMsg::Backspace => {
                if self.phase == Phase::Programs && !self.letter_buf.is_empty() {
                    self.letter_buf.pop();
                    let v = view_indices(self, true);
                    if !v.contains(&self.prog_sel) {
                        self.prog_sel = v.first().copied().unwrap_or(self.prog_sel);
                    }
                    self.prog_page = 0;
                    Effect::Emit
                } else if self.phase == Phase::Windows
                    && cfg.multi_letter
                    && self.wins.len() > 9
                    && !self.digit_buf.is_empty()
                {
                    self.digit_buf.pop();
                    if let Ok(n) = self.digit_buf.parse::<usize>() {
                        if n >= 1 && n <= self.wins.len() {
                            self.active = n - 1;
                        }
                    }
                    Effect::Emit
                } else {
                    Effect::None
                }
            }
            HookMsg::Digit(d) => self.handle_digit(*d, cfg, mru, now),
            HookMsg::Up | HookMsg::Down => {
                let delta: isize = if matches!(msg, HookMsg::Up) { -1 } else { 1 };
                match self.phase {
                    Phase::Programs if !self.prog_list.is_empty() => {
                        let view = view_indices(self, cfg.multi_letter);
                        if view.is_empty() {
                            return Effect::None;
                        }
                        let pos = view.iter().position(|&i| i == self.prog_sel).unwrap_or(0);
                        let len = view.len() as isize;
                        let new_pos = ((pos as isize + delta + len) % len) as usize;
                        self.prog_sel = view[new_pos];
                        self.sync_page(cfg.multi_letter);
                        Effect::Emit
                    }
                    Phase::Windows if !self.wins.is_empty() => {
                        let len = self.wins.len() as isize;
                        self.active = ((self.active as isize + delta + len) % len) as usize;
                        Effect::Emit
                    }
                    _ => Effect::None,
                }
            }
            HookMsg::PageUp | HookMsg::PageDown => {
                if self.phase != Phase::Programs || self.prog_list.is_empty() {
                    return Effect::None;
                }
                let view = view_indices(self, cfg.multi_letter);
                if view.is_empty() {
                    return Effect::None;
                }
                let page_count = view.len().div_ceil(PROG_PAGE_SIZE);
                let cur = self.prog_sel / PROG_PAGE_SIZE;
                let new_page = if matches!(msg, HookMsg::PageDown) {
                    (cur + 1).min(page_count - 1)
                } else if cur == 0 {
                    0
                } else {
                    cur - 1
                };
                self.prog_page = new_page;
                let start = new_page * PROG_PAGE_SIZE;
                let end = (start + PROG_PAGE_SIZE).min(view.len());
                let page_items = &view[start..end];
                if !page_items.contains(&self.prog_sel) {
                    self.prog_sel = if matches!(msg, HookMsg::PageDown) {
                        page_items[0]
                    } else {
                        *page_items.last().unwrap()
                    };
                }
                Effect::Emit
            }
            HookMsg::Space => {
                if self.phase != Phase::Programs {
                    return Effect::None;
                }
                // MRU 中可见、非覆盖层的最新两个窗口，切到第二个（两窗口互切）
                let mut recent: Vec<(u64, isize)> = mru
                    .iter()
                    .filter(|(&h, _)| h != 0 && h != overlay_hwnd)
                    .map(|(&h, &ts)| (ts, h))
                    .collect();
                recent.sort_by(|a, b| b.0.cmp(&a.0));
                let target = recent
                    .iter()
                    .filter(|(_, h)| is_visible(*h))
                    .nth(1)
                    .map(|&(_, h)| h);
                if let Some(hwnd) = target {
                    self.switched = true;
                    self.last_activated = hwnd;
                    mru.insert(hwnd, now);
                    self.pending = hwnd;
                    Effect::Close
                } else {
                    Effect::None
                }
            }
            HookMsg::Jump(n) => {
                if self.phase == Phase::Windows && self.resolve_window(*n + 1, mru, now) {
                    Effect::Close
                } else {
                    Effect::None
                }
            }
            HookMsg::Enter => match self.phase {
                Phase::Programs => {
                    if self.prog_list.is_empty() {
                        Effect::None
                    } else {
                        let view = view_indices(self, cfg.multi_letter);
                        if view.is_empty() {
                            Effect::None // 无匹配，Enter 无动作
                        } else {
                            // 多字母筛选后 prog_sel 已是匹配最高项；不在视图内（边界）取首项
                            let idx = if view.contains(&self.prog_sel) {
                                self.prog_sel
                            } else {
                                view[0]
                            };
                            self.select_indexed(idx, cfg, mru, now)
                        }
                    }
                }
                Phase::Windows => {
                    if self.resolve_window(self.active + 1, mru, now) {
                        Effect::Close
                    } else {
                        Effect::None
                    }
                }
                Phase::Closed => Effect::None,
            },
            // Hotkey 由驱动层处理（toggle 开/关），不进转换
            HookMsg::Hotkey => Effect::None,
        }
    }

    // 数字键（窗口层）：单字母累积 n*10>total 即跳；多字母 ≤9 直切、>9 组合编号；
    // preview 模式只聚焦不跳转。
    fn handle_digit(
        &mut self,
        d: char,
        cfg: &Config,
        mru: &mut HashMap<isize, u64>,
        now: u64,
    ) -> Effect {
        if self.phase != Phase::Windows || self.wins.is_empty() {
            return Effect::None;
        }
        let total = self.wins.len();
        let preview = cfg.win_digit_mode == WinDigitMode::Preview;
        if !cfg.multi_letter {
            // 单字母模式：数字累积，n*10 > total（再加一位必超总数）立即跳转
            self.digit_buf.push(d);
            if let Ok(n) = self.digit_buf.parse::<usize>() {
                if n >= 1 && n * 10 > total && self.resolve_window(n, mru, now) {
                    return Effect::Close;
                }
            }
            Effect::None
        } else if total <= 9 {
            // 窗口 ≤9：每个数字独立，按到即定
            let n = d.to_digit(10).unwrap_or(0) as usize;
            if n < 1 || n > total {
                return Effect::None;
            }
            self.digit_buf = n.to_string();
            if preview {
                self.active = n - 1;
                Effect::Emit
            } else if self.resolve_window(n, mru, now) {
                Effect::Close
            } else {
                Effect::None
            }
        } else {
            // 窗口 >9：组合编号（1 然后 2 = 12），Backspace 退格
            self.digit_buf.push(d);
            let n = match self.digit_buf.parse::<usize>() {
                Ok(n) => n,
                Err(_) => {
                    self.digit_buf.pop();
                    return Effect::None;
                }
            };
            if n < 1 || n > total {
                self.digit_buf.pop(); // 超界，忽略本次输入
                return Effect::None;
            }
            self.active = n - 1;
            if preview {
                Effect::Emit
            } else if n * 10 > total && self.resolve_window(n, mru, now) {
                // 再加一位必超总数 → 立即跳转；否则等下一位或 Enter 确认
                Effect::Close
            } else {
                Effect::None
            }
        }
    }

    fn sync_page(&mut self, multi: bool) {
        let view = view_indices(self, multi);
        if let Some(pos) = view.iter().position(|&i| i == self.prog_sel) {
            self.prog_page = pos / PROG_PAGE_SIZE;
        }
    }

    // 单字母模式按代号选中
    fn select_by_letter(
        &mut self,
        c: char,
        cfg: &Config,
        mru: &mut HashMap<isize, u64>,
        now: u64,
    ) -> Effect {
        let Some((idx, entry)) = self
            .prog_list
            .iter()
            .enumerate()
            .find(|(_, e)| e.key == c.to_string())
            .map(|(i, e)| (i, e.clone()))
        else {
            return Effect::None;
        };
        self.prog_sel = idx;
        self.sync_page(cfg.multi_letter);
        self.select_entry(&entry, cfg, mru, now)
    }

    // Enter/点击：按视图绝对索引选中
    fn select_indexed(
        &mut self,
        idx: usize,
        cfg: &Config,
        mru: &mut HashMap<isize, u64>,
        now: u64,
    ) -> Effect {
        let entry = self.prog_list[idx].clone();
        self.select_entry(&entry, cfg, mru, now)
    }

    // 选中程序层条目：
    // - 窗口层重复选中同一程序 → 轮询切下一窗口，返回 ActivateWindow（驱动层实时激活，不关闭）
    // - 单窗口 → 置 pending，返回 Close
    // - 多窗口 → 进窗口层，返回 Emit
    fn select_entry(
        &mut self,
        entry: &ProgEntry,
        cfg: &Config,
        mru: &mut HashMap<isize, u64>,
        now: u64,
    ) -> Effect {
        let wins = match self.wins_by_proc.get(&entry.process) {
            Some(w) if !w.is_empty() => w,
            _ => return Effect::None,
        };
        if self.phase == Phase::Windows
            && self.sel_proc.as_deref() == Some(entry.process.as_str())
        {
            self.active = (self.active + 1) % wins.len();
            let hwnd = wins[self.active].hwnd;
            self.last_activated = hwnd;
            mru.insert(hwnd, now);
            return Effect::ActivateWindow(hwnd);
        }
        if wins.len() == 1 {
            let hwnd = wins[0].hwnd;
            self.switched = true;
            self.last_activated = hwnd;
            mru.insert(hwnd, now);
            self.pending = hwnd;
            Effect::Close
        } else {
            self.phase = Phase::Windows;
            self.sel_proc = Some(entry.process.clone());
            self.letter_buf.clear();
            // 排序：mru 按最近使用倒序（1=上次用的）；zorder 按句柄序（创建序，稳定）
            let mut wins = wins.clone();
            if cfg.window_order == WindowOrder::Mru {
                wins.sort_by_key(|w| std::cmp::Reverse(mru.get(&w.hwnd).copied().unwrap_or(0)));
            } else {
                wins.sort_by_key(|w| w.hwnd);
            }
            self.wins = wins;
            self.active = 0;
            self.digit_buf.clear();
            Effect::Emit
        }
    }

    // 选定窗口 n（1-based）：置 switched/last_activated/pending、记 mru。返回是否有效。
    fn resolve_window(&mut self, n: usize, mru: &mut HashMap<isize, u64>, now: u64) -> bool {
        let total = self.wins.len();
        if total == 0 || n < 1 {
            return false;
        }
        let idx = (n - 1).min(total - 1);
        let hwnd = self.wins[idx].hwnd;
        self.switched = true;
        self.last_activated = hwnd;
        mru.insert(hwnd, now);
        self.pending = hwnd;
        true
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
        window_order: cfg.window_order.as_str().into(),
        multi_letter: multi,
        theme: cfg.theme.clone(),
        win_digit_mode: cfg.win_digit_mode.as_str().into(),
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

// 枚举当前窗口并按进程分组，黑名单（系统预置 + 用户屏蔽）不进入
fn group_windows(cfg: &Config) -> HashMap<String, Vec<WinInfo>> {
    let mut wins_by_proc: HashMap<String, Vec<WinInfo>> = HashMap::new();
    for w in windows::enum_windows() {
        if cfg.blocked.iter().any(|b| b.process() == w.process) {
            continue;
        }
        wins_by_proc.entry(w.process.clone()).or_default().push(w);
    }
    wins_by_proc
}

fn open(app: &AppHandle) {
    let inner = app.state::<Inner>();
    if inner.visible.load(Ordering::Relaxed) {
        return;
    }
    let cfg = inner.cfg.lock().unwrap().clone();
    let wins_by_proc = group_windows(&cfg);
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
    ov.pending = 0;
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
    let pending = ov.pending;
    drop(ov);
    windows::set_overlay_hwnd(0);
    windows::thumb_clear();
    // 先隐藏再处理尺寸：若先退全屏，面板会在屏幕上可见地缩小跳动（闪烁）
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    // 决定要激活的目标窗口（先记下来，spawn 放到 emit 之后）。
    // pending（用户明确选了窗口）总是优先；否则仅 restore_prev 路径回退 prev_fg
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
    let render = Render::closed(&cfg);
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
    ov.wins_by_proc = group_windows(&cfg);
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
// 鼠标点击程序行：多字母代号可能多字母，统一按 process 选中（不能复用 letter 路径）。
// 点击已高亮项等同 Enter 确认；否则只移动高亮。
#[tauri::command]
fn pick_program(app: AppHandle, process: String) {
    let inner = app.state::<Inner>();
    if !inner.visible.load(Ordering::Relaxed) {
        return;
    }
    let cfg = inner.cfg.lock().unwrap().clone();
    let mut ov = inner.overlay.lock().unwrap();
    if ov.phase != Phase::Programs || ov.prog_list.is_empty() {
        return;
    }
    let view = view_indices(&ov, cfg.multi_letter);
    let Some(pos) = view
        .iter()
        .position(|&i| ov.prog_list[i].process == process)
    else {
        return;
    };
    let target = view[pos];
    // 点击已高亮项 = 确认；否则只移动高亮
    let eff = if target == ov.prog_sel {
        let now = windows::now_ms();
        let mut mru = inner.mru.lock().unwrap();
        ov.select_indexed(target, &cfg, &mut mru, now)
    } else {
        ov.prog_sel = target;
        ov.sync_page(cfg.multi_letter);
        Effect::Emit
    };
    apply_effect(&app, &*inner, ov, eff);
}

// 设置页保存关闭后：重新枚举并 emit，让程序列表立即按新设置（模式/黑名单等）刷新
#[tauri::command]
fn refresh_overlay(app: AppHandle) {
    let inner = app.state::<Inner>();
    if inner.visible.load(Ordering::Relaxed) {
        rebuild_and_emit(&app, &inner);
    }
}

// 落地状态机产出的 Effect：Emit 重渲染、Close 收尾激活、ActivateWindow 轮询实时激活。
// 调用方持有 ov 锁；Close 路径先 drop 再 close（close 会再取 overlay 锁）。
fn apply_effect(app: &AppHandle, inner: &Inner, ov: std::sync::MutexGuard<'_, OverlayState>, eff: Effect) {
    match eff {
        Effect::None => {}
        Effect::Emit => emit(app, inner, &ov),
        Effect::ActivateWindow(hwnd) => {
            eprintln!("[winhop] 轮询激活 hwnd={:#x}", hwnd);
            deferred_activate(app, hwnd);
            emit(app, inner, &ov);
        }
        Effect::Close => {
            drop(ov);
            close(app);
        }
    }
}

// 薄驱动：Hotkey 单独 toggle；其余按键交给纯状态机 transition，再落地 Effect。
fn handle_key(app: &AppHandle, msg: HookMsg) {
    let inner = app.state::<Inner>();
    if matches!(msg, HookMsg::Hotkey) {
        if inner.visible.load(Ordering::Relaxed) {
            close(app);
        } else {
            open(app);
        }
        return;
    }
    if !inner.visible.load(Ordering::Relaxed) {
        return;
    }
    let cfg = inner.cfg.lock().unwrap().clone();
    let mut ov = inner.overlay.lock().unwrap();
    eprintln!("[t={}] key {:?} phase={:?}", windows::now_ms(), msg, ov.phase);
    let now = windows::now_ms();
    let overlay_hwnd = windows::get_overlay_hwnd();
    let eff = {
        let mut mru = inner.mru.lock().unwrap();
        ov.transition(&msg, &cfg, &mut mru, now, overlay_hwnd, &windows::overlay_visible)
    };
    if eff == Effect::None {
        // 空格无目标等场景保留诊断日志
        if matches!(msg, HookMsg::Space) {
            eprintln!("[winhop] 空格快速跳转：无可切换的上一个窗口");
        }
        return;
    }
    apply_effect(app, &*inner, ov, eff);
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
            settings::get_settings,
            settings::save_settings,
            hotkey_suspend,
            hotkey_resume,
            hotkey_capture::hotkey_capture_start,
            hotkey_capture::hotkey_capture_poll,
            hotkey_capture::hotkey_capture_stop,
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

    // 构造仅含给定程序的最小 Config（其余字段走 Config::default）
    fn cfg_with(programs: Vec<Program>) -> Config {
        Config {
            programs,
            ..Config::default()
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
        let cfg = cfg_with(vec![prog("c", "", "Chrome", "chrome.exe")]);
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

    // ===== transition 纯状态机：构造窗口层状态 =====
    fn wwin(hwnd: isize) -> WinInfo {
        WinInfo {
            hwnd,
            title: format!("w{}", hwnd),
            process: "chrome.exe".into(),
            path: String::new(),
            monitor: 0,
        }
    }

    // 窗口层状态：sel_proc=chrome，wins 与 wins_by_proc 已填
    fn windows_state(wins: Vec<WinInfo>) -> OverlayState {
        let mut ov = OverlayState::default();
        ov.phase = Phase::Windows;
        ov.sel_proc = Some("chrome.exe".into());
        ov.prog_list = vec![ProgEntry {
            key: "c".into(),
            multi_key: "ch".into(),
            name: "Chrome".into(),
            process: "chrome.exe".into(),
            configured: true,
        }];
        ov.wins = wins.clone();
        ov.wins_by_proc.insert("chrome.exe".to_string(), wins);
        ov
    }

    // 程序层状态：chrome 单窗口、code 三窗口
    fn programs_state() -> (OverlayState, Config) {
        let cfg = cfg_with(vec![
            prog("c", "ch", "Chrome", "chrome.exe"),
            prog("v", "vs", "Code", "code.exe"),
        ]);
        let mut wins: HashMap<String, Vec<WinInfo>> = HashMap::new();
        wins.insert("chrome.exe".into(), vec![wwin(100)]);
        wins.insert(
            "code.exe".into(),
            vec![wwin(200), wwin(201), wwin(202)],
        );
        let mut ov = OverlayState::default();
        ov.phase = Phase::Programs;
        ov.prog_list = build_prog_list(&cfg, &wins);
        ov.wins_by_proc = wins;
        (ov, cfg)
    }

    const VIS: &dyn Fn(isize) -> bool = &|_| true;

    #[test]
    fn transition_single_letter_single_window_closes_with_pending() {
        let (mut ov, cfg) = programs_state();
        let mut mru = HashMap::new();
        let eff = ov.transition(&HookMsg::Letter('c'), &cfg, &mut mru, 1, 0, VIS);
        assert_eq!(eff, Effect::Close);
        assert_eq!(ov.pending, 100); // chrome 单窗口直切
        assert!(ov.switched);
    }

    #[test]
    fn transition_single_letter_multi_window_enters_windows_phase() {
        let (mut ov, cfg) = programs_state();
        let mut mru = HashMap::new();
        let eff = ov.transition(&HookMsg::Letter('v'), &cfg, &mut mru, 1, 0, VIS);
        assert_eq!(eff, Effect::Emit);
        assert_eq!(ov.phase, Phase::Windows);
        assert_eq!(ov.wins.len(), 3);
        assert_eq!(ov.pending, 0); // 进窗口层，尚未选定
    }

    #[test]
    fn transition_windows_phase_poll_cycles_window() {
        // 窗口层重复按同一程序代号 → 轮询下一窗口，实时激活但不关闭
        let mut ov = windows_state(vec![wwin(200), wwin(201), wwin(202)]);
        let cfg = cfg_with(vec![prog("c", "ch", "Chrome", "chrome.exe")]);
        let mut mru = HashMap::new();
        let eff = ov.transition(&HookMsg::Letter('c'), &cfg, &mut mru, 1, 0, VIS);
        assert_eq!(eff, Effect::ActivateWindow(201)); // active 0→1
        assert_eq!(ov.active, 1);
        assert_eq!(ov.pending, 0); // 轮询不置 pending
    }

    #[test]
    fn transition_digit_single_mode_accumulates_until_overflow() {
        // 15 个窗口：按 1 不跳（1*10≤15），按 2 → 12 立即跳转第 12 个
        let wins: Vec<WinInfo> = (100..115).map(wwin).collect();
        let mut ov = windows_state(wins);
        let cfg = cfg_with(vec![]); // 单字母 + jump
        let mut mru = HashMap::new();
        let e1 = ov.transition(&HookMsg::Digit('1'), &cfg, &mut mru, 1, 0, VIS);
        assert_eq!(e1, Effect::None);
        assert_eq!(ov.pending, 0);
        let e2 = ov.transition(&HookMsg::Digit('2'), &cfg, &mut mru, 2, 0, VIS);
        assert_eq!(e2, Effect::Close);
        assert_eq!(ov.pending, 111); // 第 12 个 idx=11 → hwnd 111
    }

    #[test]
    fn transition_digit_multi_le9_jump_direct() {
        let mut ov = windows_state(vec![wwin(200), wwin(201), wwin(202)]);
        let mut cfg = cfg_with(vec![]);
        cfg.multi_letter = true;
        let mut mru = HashMap::new();
        let eff = ov.transition(&HookMsg::Digit('2'), &cfg, &mut mru, 1, 0, VIS);
        assert_eq!(eff, Effect::Close);
        assert_eq!(ov.pending, 201); // 第 2 个 idx=1
    }

    #[test]
    fn transition_digit_multi_le9_preview_only_focuses() {
        let mut ov = windows_state(vec![wwin(200), wwin(201), wwin(202)]);
        let mut cfg = cfg_with(vec![]);
        cfg.multi_letter = true;
        cfg.win_digit_mode = WinDigitMode::Preview;
        let mut mru = HashMap::new();
        let eff = ov.transition(&HookMsg::Digit('2'), &cfg, &mut mru, 1, 0, VIS);
        assert_eq!(eff, Effect::Emit);
        assert_eq!(ov.active, 1); // 只聚焦
        assert_eq!(ov.pending, 0); // 不跳转
    }

    #[test]
    fn transition_digit_multi_gt9_combo_index() {
        // 12 个窗口：按 1 不跳，按 2 → 组合 12 跳转；按 3（13 超界）忽略
        let wins: Vec<WinInfo> = (100..112).map(wwin).collect();
        let mut ov = windows_state(wins);
        let mut cfg = cfg_with(vec![]);
        cfg.multi_letter = true;
        let mut mru = HashMap::new();
        assert_eq!(ov.transition(&HookMsg::Digit('1'), &cfg, &mut mru, 1, 0, VIS), Effect::None);
        assert_eq!(ov.transition(&HookMsg::Digit('3'), &cfg, &mut mru, 2, 0, VIS), Effect::None); // 13>12 弹回
        assert_eq!(ov.digit_buf, "1");
        assert_eq!(ov.transition(&HookMsg::Digit('2'), &cfg, &mut mru, 3, 0, VIS), Effect::Close); // 12
        assert_eq!(ov.pending, 111);
    }

    #[test]
    fn transition_esc_windows_back_to_programs_then_close() {
        let mut ov = windows_state(vec![wwin(200), wwin(201)]);
        let cfg = cfg_with(vec![]);
        let mut mru = HashMap::new();
        assert_eq!(ov.transition(&HookMsg::Esc, &cfg, &mut mru, 1, 0, VIS), Effect::Emit);
        assert_eq!(ov.phase, Phase::Programs);
        assert_eq!(ov.sel_proc, None);
        // 程序层无筛选：Esc 关闭
        assert_eq!(ov.transition(&HookMsg::Esc, &cfg, &mut mru, 2, 0, VIS), Effect::Close);
    }

    #[test]
    fn transition_esc_programs_clears_filter_first() {
        let (mut ov, mut cfg) = programs_state();
        cfg.multi_letter = true;
        let mut mru = HashMap::new();
        ov.transition(&HookMsg::Letter('v'), &cfg, &mut mru, 1, 0, VIS);
        assert!(!ov.letter_buf.is_empty());
        // 第一次 Esc：清筛选不关
        assert_eq!(ov.transition(&HookMsg::Esc, &cfg, &mut mru, 2, 0, VIS), Effect::Emit);
        assert!(ov.letter_buf.is_empty());
        assert_eq!(ov.phase, Phase::Programs);
        // 第二次 Esc：关闭
        assert_eq!(ov.transition(&HookMsg::Esc, &cfg, &mut mru, 3, 0, VIS), Effect::Close);
    }

    #[test]
    fn transition_enter_programs_single_window_closes() {
        let (mut ov, cfg) = programs_state();
        let mut mru = HashMap::new();
        ov.prog_sel = 0; // 高亮首个（chrome 单窗口）
        let eff = ov.transition(&HookMsg::Enter, &cfg, &mut mru, 1, 0, VIS);
        assert_eq!(eff, Effect::Close);
        assert_eq!(ov.pending, 100);
    }

    #[test]
    fn transition_space_jumps_to_second_mru() {
        let (mut ov, cfg) = programs_state();
        let mut mru = HashMap::new();
        mru.insert(200, 20); // 最新（当前窗口）
        mru.insert(100, 10); // 上一个
        let eff = ov.transition(&HookMsg::Space, &cfg, &mut mru, 30, 5, VIS); // overlay_hwnd=5
        assert_eq!(eff, Effect::Close);
        assert_eq!(ov.pending, 100); // 跳过最新 200，切到 100
        assert_eq!(mru.get(&100), Some(&30)); // MRU 时间戳刷新
    }

    #[test]
    fn transition_space_no_target_is_none() {
        let (mut ov, cfg) = programs_state();
        let mut mru = HashMap::new(); // 空 MRU
        let eff = ov.transition(&HookMsg::Space, &cfg, &mut mru, 1, 0, VIS);
        assert_eq!(eff, Effect::None);
        assert_eq!(ov.pending, 0);
    }
}
