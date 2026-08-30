use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, OnceLock};
use windows_sys::Win32::Foundation::{
    CloseHandle, BOOL, GENERIC_WRITE, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, POINT,
    RECT, WPARAM, ERROR_ALREADY_EXISTS, GetLastError,
};
use windows_sys::Win32::Graphics::Dwm::{
    DwmRegisterThumbnail, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_RECTDESTINATION, DWM_TNP_RECTSOURCE, DWM_TNP_VISIBLE,
};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_QUERY};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, SetFilePointer, FILE_END, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_ALWAYS,
};
use windows_sys::Win32::System::Console::{SetStdHandle, STD_ERROR_HANDLE};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, OpenProcess,
    OpenProcessToken, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::Globalization::{
    GetUserDefaultLCID, GetUserDefaultLocaleName, GetUserDefaultUILanguage,
    GetSystemDefaultUILanguage,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CallNextHookEx, EnumWindows, GetClassNameW, GetClientRect,
    GetForegroundWindow,
    GetWindowPlacement, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindow, IsWindowVisible, MSLLHOOKSTRUCT, SetForegroundWindow, SetWindowsHookExW,
    ShowWindow, SM_CYCAPTION, SM_CXPADDEDBORDER, SM_CXSIZEFRAME, SM_CYSIZEFRAME,
    SPIF_SENDCHANGE, SPI_GETFOREGROUNDLOCKTIMEOUT, SPI_SETFOREGROUNDLOCKTIMEOUT,
    SystemParametersInfoW, SW_RESTORE, WINDOWPLACEMENT, WM_LBUTTONDOWN, WM_MBUTTONDOWN,
    WM_RBUTTONDOWN, WH_MOUSE_LL, WPF_RESTORETOMAXIMIZED,
};

#[derive(Clone)]
pub struct WinInfo {
    pub hwnd: isize,
    pub title: String,
    pub process: String,
    pub path: String,
    pub monitor: u32,
}

#[derive(Debug)]
pub enum HookMsg {
    Letter(char),
    Digit(char),
    Esc,
    Hotkey,
    Up,
    Down,
    PageUp,
    PageDown,
    Backspace,
    Space,
    Enter,
    Jump(usize),
    ClickOutside,
}

struct MouseCtx {
    visible: Arc<AtomicBool>,
    overlay_hwnd: AtomicIsize,
    handler: Box<dyn Fn(HookMsg) + Send + Sync>,
}

static MOUSE_CTX: OnceLock<MouseCtx> = OnceLock::new();

pub fn set_overlay_hwnd(hwnd: isize) {
    if let Some(ctx) = MOUSE_CTX.get() {
        ctx.overlay_hwnd.store(hwnd, Ordering::Relaxed);
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn thread_id() -> u32 {
    unsafe { GetCurrentThreadId() }
}

// 只装鼠标 LL 钩子（点击覆盖层外部关闭）。
// 键盘不走 LL 钩子：Chromium 前台用 raw input 收键盘，LL 键盘钩子完全看不见按键
//（实测：Edge/Chrome 前台钩子静默，notepad 前台正常）。热键走 RegisterHotKey
//（系统级，与前台无关），覆盖层按键走 webview JS keydown（覆盖层自己夺焦）。
pub fn install_mouse_hook(visible: Arc<AtomicBool>, handler: Box<dyn Fn(HookMsg) + Send + Sync>) {
    let _ = MOUSE_CTX.set(MouseCtx {
        visible,
        overlay_hwnd: AtomicIsize::new(0),
        handler,
    });
    unsafe {
        let mh = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), std::ptr::null_mut(), 0);
        eprintln!("[t={}] 鼠标钩子安装 {:?}", now_ms(), mh);
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // 不拦截的事件必须 CallNextHookEx 传给钩子链上的其它程序（AHK、鼠标手势等），
    // 否则直接 return 会截断整条 LL 钩子链；仅「点击覆盖层外」才吞掉（return 1）
    let pass = |code: i32, wparam: WPARAM, lparam: LPARAM| unsafe {
        CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
    };
    let ctx = match MOUSE_CTX.get() {
        Some(c) => c,
        None => return pass(code, wparam, lparam),
    };
    if code < 0 {
        return pass(code, wparam, lparam);
    }
    let wm = wparam as u32;
    if wm != WM_LBUTTONDOWN && wm != WM_RBUTTONDOWN && wm != WM_MBUTTONDOWN {
        return pass(code, wparam, lparam);
    }
    eprintln!(
        "[t={}] mouse {} thread={} vis={}",
        now_ms(),
        wm,
        thread_id(),
        ctx.visible.load(Ordering::Relaxed)
    );
    if !ctx.visible.load(Ordering::Relaxed) {
        return pass(code, wparam, lparam);
    }
    let ms = &*(lparam as *const MSLLHOOKSTRUCT);
    let hwnd = ctx.overlay_hwnd.load(Ordering::Relaxed);
    if hwnd != 0 {
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd as HWND, &mut r) != 0 {
            let inside =
                ms.pt.x >= r.left && ms.pt.x <= r.right && ms.pt.y >= r.top && ms.pt.y <= r.bottom;
            if !inside {
                eprintln!("[t={}] 鼠标点击外部，关闭", now_ms());
                (ctx.handler)(HookMsg::ClickOutside);
                return 1; // 吞掉点击，避免误操作原应用（不 CallNextHookEx）
            }
        }
    }
    pass(code, wparam, lparam)
}

struct EnumCtx {
    out: Vec<WinInfo>,
    monitors: Vec<Monitor>,
}

pub fn enum_windows() -> Vec<WinInfo> {
    let monitors = enum_monitors();
    let mut ctx = EnumCtx {
        out: Vec::new(),
        monitors,
    };
    unsafe {
        EnumWindows(Some(enum_proc), &mut ctx as *mut EnumCtx as isize);
    }
    ctx.out
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    let mut own_pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, &mut own_pid);
    if own_pid != 0 && own_pid == GetCurrentProcessId() {
        return 1; // 排除自身（覆盖层）
    }
    let mut class = [0u16; 256];
    let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
    if len > 0 {
        let cls = String::from_utf16_lossy(&class[..len as usize]);
        if cls == "Shell_TrayWnd" || cls == "Progman" {
            return 1;
        }
    }
    let title = window_title(hwnd);
    if title.is_empty() {
        return 1;
    }
    let ctx = &mut *(lparam as *mut EnumCtx);
    let (process, path) = process_name(hwnd);
    ctx.out.push(WinInfo {
        hwnd: hwnd as isize,
        title,
        process,
        path,
        monitor: monitor_index(hwnd, &ctx.monitors),
    });
    1
}

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

// 返回 (小写 exe 文件名, 完整 exe 路径)
fn process_name(hwnd: HWND) -> (String, String) {
    unsafe {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return (String::new(), String::new());
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return (String::new(), String::new());
        }
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(handle);
        if ok == 0 {
            return (String::new(), String::new());
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        let stem = path.rsplit('\\').next().unwrap_or("").to_lowercase();
        (stem, path)
    }
}

// exe 版本资源里的显示名（FileDescription，回退 ProductName）
pub fn file_description(path: &str) -> Option<String> {
    unsafe {
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
        };
        let wide = to_wide(path);
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, buf.as_mut_ptr() as *mut c_void) == 0 {
            return None;
        }
        // 收集所有 Translation 条目 + 常用回退组合：
        // 部分软件（如 MobaXterm）Translation 首项是 0009/00E4 但字符串块不存在，
        // 实际内容在 0409/04B0 等组合下——只试首项会拿到空，回退成 exe 文件名
        let trans_key = to_wide("\\VarFileInfo\\Translation");
        let mut combos: Vec<(u16, u16)> = Vec::new();
        let mut trans: *mut c_void = std::ptr::null_mut();
        let mut trans_len: u32 = 0;
        if VerQueryValueW(
            buf.as_mut_ptr() as *mut c_void,
            trans_key.as_ptr(),
            &mut trans,
            &mut trans_len,
        ) != 0
            && !trans.is_null()
            && trans_len >= 4
        {
            let n = (trans_len as usize) / 4;
            let t = std::slice::from_raw_parts(trans as *const u8, n * 4);
            for i in 0..n {
                combos.push((
                    (t[i * 4] as u16) | ((t[i * 4 + 1] as u16) << 8),
                    (t[i * 4 + 2] as u16) | ((t[i * 4 + 3] as u16) << 8),
                ));
            }
        }
        for fallback in [(0x0409u16, 0x04B0u16), (0x0409, 0x0000), (0x0804, 0x04B0), (0x0804, 0x0000), (0x0000, 0x04B0), (0x0000, 0x0000)] {
            if !combos.contains(&fallback) {
                combos.push(fallback);
            }
        }
        // 只取 FileDescription（与任务管理器「文件说明」列同源同规则），
        // 不读 ProductName：系统二进制的 ProductName 是「Microsoft® Windows®
        // Operating System」通用串，曾导致多个系统窗口被识别成同名
        for (lang, cp) in &combos {
            let path_str = format!("\\StringFileInfo\\{:04X}{:04X}\\FileDescription", lang, cp);
            let key_wide = to_wide(&path_str);
            let mut val: *mut c_void = std::ptr::null_mut();
            let mut val_len: u32 = 0;
            if VerQueryValueW(
                buf.as_mut_ptr() as *mut c_void,
                key_wide.as_ptr(),
                &mut val,
                &mut val_len,
            ) != 0
                && !val.is_null()
                && val_len > 0
            {
                // 注意：val_len 不可靠——部分软件（Chrome/ASUS/系统二进制等）的
                // 版本资源长度字段不规范，实测返回值仅为真实长度的一半，
                // 按 val_len 截断会把 "Google Chrome" 读成 "Google"。
                // 改为按 null 终止符读取：在版本资源 buffer 范围内从 val 起找 \0
                let buf_units = buf.len() / 2;
                let val_off = (val as usize - buf.as_ptr() as usize) / 2;
                let avail = buf_units.saturating_sub(val_off).min(512);
                let slice = std::slice::from_raw_parts(val as *const u16, avail);
                let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
                let s = String::from_utf16_lossy(&slice[..end]);
                let s = s.trim().to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        None
    }
}

#[derive(Clone, Copy)]
struct Monitor {
    rect: RECT,
}

fn enum_monitors() -> Vec<Monitor> {
    let mut out: Vec<Monitor> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(monitor_proc),
            &mut out as *mut Vec<Monitor> as isize,
        );
    }
    unsafe extern "system" fn monitor_proc(
        _hmon: *mut c_void,
        _hdc: *mut c_void,
        rect: *mut RECT,
        data: isize,
    ) -> BOOL {
        let v = &mut *(data as *mut Vec<Monitor>);
        v.push(Monitor { rect: *rect });
        1
    }
    out
}

fn monitor_index(hwnd: HWND, monitors: &[Monitor]) -> u32 {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe { GetWindowRect(hwnd, &mut rect) };
    let mut best = 0u32;
    let mut best_area = 0i64;
    for (i, m) in monitors.iter().enumerate() {
        let w = (rect.right.min(m.rect.right) - rect.left.max(m.rect.left)).max(0) as i64;
        let h = (rect.bottom.min(m.rect.bottom) - rect.top.max(m.rect.top)).max(0) as i64;
        let area = w * h;
        if area > best_area {
            best_area = area;
            best = i as u32;
        }
    }
    best
}

// 激活目标窗口。不使用 AttachThreadInput——它会把调用线程输入队列与目标线程
// 同步共享，目标线程处理慢时会把调用方（甚至前台线程）一起挂死，表现为光标卡顿、
// 热键无响应。
// 也不注入按键（曾用 keybd_event Alt 按下/抬起破解前台锁）：注入的 down/up 若
// 因焦点变化落到不同线程，会在受害应用线程的输入队列留下「卡住的 Alt」——之后
// 该应用里所有按键都变成 Alt 组合（实测 12 次激活失败后前台应用键盘全乱）。
// 改用临时关闭前台锁定超时（SPI_SETFOREGROUNDLOCKTIMEOUT=0）再 SetForegroundWindow，
// 不产生任何按键事件，失败也无副作用。
//
// 注意：调用方须在独立线程执行本函数，且须在覆盖层 emit(visible=false) 收尾之后再启动
// （见 close() 顺序说明）——若在 WebView2 处理 hide+IPC 期间从外部抢走焦点，会阻塞
// 主线程，连带挂住鼠标钩子与热键派发。
pub fn activate(hwnd: isize) -> bool {
    unsafe {
        let h = hwnd as HWND;
        if IsIconic(h) != 0 {
            ShowWindow(h, SW_RESTORE);
        }
        if GetForegroundWindow() == h {
            return true;
        }
        let mut timeout: u32 = 0;
        SystemParametersInfoW(
            SPI_GETFOREGROUNDLOCKTIMEOUT,
            0,
            &mut timeout as *mut u32 as *mut c_void,
            0,
        );
        SystemParametersInfoW(
            SPI_SETFOREGROUNDLOCKTIMEOUT,
            0,
            &0u32 as *const u32 as *mut c_void,
            SPIF_SENDCHANGE,
        );
        let ok = SetForegroundWindow(h);
        BringWindowToTop(h);
        // 恢复原锁定超时
        SystemParametersInfoW(
            SPI_SETFOREGROUNDLOCKTIMEOUT,
            0,
            &timeout as *const u32 as *mut c_void,
            SPIF_SENDCHANGE,
        );
        let success = ok != 0 && GetForegroundWindow() == h;
        if !success {
            eprintln!(
                "[t={}] 激活 SetForegroundWindow 失败 hwnd={:#x} ok={} err={}",
                now_ms(),
                hwnd,
                ok,
                GetLastError()
            );
        }
        success
    }
}

// 激活校验：失败时再注入 Alt 重试一次。
// 目标窗口在另一个虚拟桌面时无效（taskbar 闪烁但无法显示）。
pub fn activate_with_retry(hwnd: isize) {
    if activate(hwnd) {
        return;
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
    if !activate(hwnd) {
        eprintln!(
            "[t={}] 激活失败 hwnd={:#x}（可能在另一个虚拟桌面）",
            now_ms(),
            hwnd
        );
    }
}

pub fn overlay_visible(hwnd: isize) -> bool {
    unsafe { hwnd != 0 && IsWindowVisible(hwnd as HWND) != 0 }
}

pub fn is_window(hwnd: isize) -> bool {
    unsafe { hwnd != 0 && IsWindow(hwnd as HWND) != 0 }
}

// ===== DWM 缩略图（大预览 + 窗口层行缩略图，Win+Tab 同款）：DWM 直接把目标窗口纹理
// 合成到覆盖层指定区域，零拷贝、实时、与遮挡/空闲无关——被全屏选择页盖住且空闲的
// 窗口 WGC 也拿不到帧，DWM 缩略图不受限。
// slot 区分注册位（"pane" 大预览 / "row:<hwnd>" 行），同 slot 换源先注销旧注册。

static THUMBS: std::sync::Mutex<Option<std::collections::HashMap<String, (isize, isize)>>> =
    std::sync::Mutex::new(None);

// 缩略图源尺寸/偏移：最小化窗口的 GetClientRect 是极小的最小化尺寸，
// 会导致 DWM 只截到一条小切片被放大——尺寸改用 rcNormalPosition（还原后尺寸），
// 且最小化窗口的 DWM 源坐标系与普通窗口不同，需走"空源 + CLIENTONLY"路径
fn effective_source(hwnd: HWND) -> (i32, i32, i32, i32, bool) {
    unsafe {
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetClientRect(hwnd, &mut r) == 0 {
            return (0, 0, 0, 0, false);
        }
        let (cw, ch) = (r.right - r.left, r.bottom - r.top);
        if IsIconic(hwnd) != 0 {
            let mut wp = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                flags: 0,
                showCmd: 0,
                ptMinPosition: POINT { x: 0, y: 0 },
                ptMaxPosition: POINT { x: 0, y: 0 },
                rcNormalPosition: RECT { left: 0, top: 0, right: 0, bottom: 0 },
            };
            if GetWindowPlacement(hwnd, &mut wp) != 0 {
                // 最大化后最小化（WPF_RESTORETOMAXIMIZED）：rcNormalPosition 是"还原尺寸"
                // （如默认 1024x768），与当前最大化内容无关——内容尺寸取显示器工作区
                if wp.flags & WPF_RESTORETOMAXIMIZED != 0 {
                    let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                    let mut mi = MONITORINFO {
                        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                        rcMonitor: RECT { left: 0, top: 0, right: 0, bottom: 0 },
                        rcWork: RECT { left: 0, top: 0, right: 0, bottom: 0 },
                        dwFlags: 0,
                    };
                    if GetMonitorInfoW(mon, &mut mi) != 0 {
                        let (mw, mh) = (
                            mi.rcWork.right - mi.rcWork.left,
                            mi.rcWork.bottom - mi.rcWork.top,
                        );
                        eprintln!(
                            "[t={}] 缩略图最小化源(最大化) hwnd={:#x} workarea={}x{}",
                            now_ms(),
                            hwnd as isize,
                            mw,
                            mh
                        );
                        if mw > 0 && mh > 0 {
                            return (mw, mh, 0, 0, true);
                        }
                    }
                }
                let n = wp.rcNormalPosition;
                // 浮动窗口：还原尺寸即内容尺寸（rcNormalPosition 为 96 基准虚拟坐标，换回物理）
                let dpi = GetDpiForWindow(hwnd).max(96);
                let s = dpi as f64 / 96.0;
                let (nw, nh) = (
                    ((n.right - n.left) as f64 * s) as i32,
                    ((n.bottom - n.top) as f64 * s) as i32,
                );
                if nw > 0 && nh > 0 {
                    // 扣除物理边框对齐客户区
                    let fx =
                        GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                    let fy =
                        GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                    let cap = GetSystemMetricsForDpi(SM_CYCAPTION, dpi);
                    let (ew, eh) = (nw - 2 * fx, nh - 2 * fy - cap);
                    eprintln!(
                        "[t={}] 缩略图最小化源(浮动) hwnd={:#x} dpi={} normal={}x{} client={}x{}",
                        now_ms(),
                        hwnd as isize,
                        dpi,
                        nw,
                        nh,
                        ew,
                        eh
                    );
                    if ew > 0 && eh > 0 {
                        return (ew, eh, 0, 0, true);
                    }
                    return (nw, nh, 0, 0, true);
                }
            }
        }
        let (ox, oy) = client_origin_in_window(hwnd);
        (cw, ch, ox, oy, false)
    }
}

// 客户区原点在窗口坐标系中的偏移（rcSource 用窗口坐标，需精确剔除边框）
fn client_origin_in_window(hwnd: HWND) -> (i32, i32) {
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        if ClientToScreen(hwnd, &mut pt) == 0 {
            return (0, 0);
        }
        let mut wr = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut wr) == 0 {
            return (0, 0);
        }
        (pt.x - wr.left, pt.y - wr.top)
    }
}

// 注册/更新一个缩略图位。x/y/w/h = 元素完整区域，ax/ay/aw/ah = 可视裁剪区域
// （滚动容器相交部分，均为覆盖层客户区物理像素）。源按窗口客户区等比 contain 居中。
pub fn thumb_set(
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
    let dest = get_overlay_hwnd();
    if dest == 0 || !is_window(hwnd) {
        return;
    }
    let shwnd = hwnd as HWND;
    let (cw, ch, ox, oy, minimized) = effective_source(shwnd);
    if w <= 0 || h <= 0 || cw <= 0 || ch <= 0 {
        thumb_set_invisible(&slot);
        return;
    }
    // 等比 contain 进元素区域（大预览/行缩略图都保持源窗口比例）
    let scale = (w as f64 / cw as f64).min(h as f64 / ch as f64);
    let fw = (cw as f64 * scale) as i32;
    let fh = (ch as f64 * scale) as i32;
    let fx = x + (w - fw) / 2;
    let fy = y + (h - fh) / 2;
    let clipped = ax > 0 || ay > 0 || aw > 0 || ah > 0;
    // 目标矩形：行缩略图与可视裁剪框求交（滚出容器只显示可见部分），大预览用完整 contain 区
    let (vx0, vy0, vx1, vy1) = if clipped {
        (
            fx.max(ax),
            fy.max(ay),
            (fx + fw).min(ax + aw),
            (fy + fh).min(ay + ah),
        )
    } else {
        (fx, fy, fx + fw, fy + fh)
    };
    if vx0 >= vx1 || vy0 >= vy1 {
        // 完全滚出：保持注册但不可见
        thumb_set_invisible(&slot);
        return;
    }
    let rcd = RECT { left: vx0, top: vy0, right: vx1, bottom: vy1 };
    let (flags, rcs) = if minimized {
        // 最小化窗口：DWM 源坐标按客户区解释，显式给客户区尺寸（rcNormalPosition 扣边框）
        (
            DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_RECTSOURCE,
            RECT { left: 0, top: 0, right: cw, bottom: ch },
        )
    } else if clipped {
        // 行缩略图切片：rcSource 为窗口坐标下对应可视区域的部分
        let src = RECT {
            left: ox + ((vx0 - fx) as f64 / fw as f64 * cw as f64) as i32,
            top: oy + ((vy0 - fy) as f64 / fh as f64 * ch as f64) as i32,
            right: ox + ((vx1 - fx) as f64 / fw as f64 * cw as f64) as i32,
            bottom: oy + ((vy1 - fy) as f64 / fh as f64 * ch as f64) as i32,
        };
        (DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_RECTSOURCE, src)
    } else {
        // 大预览：完整客户区
        (
            DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_RECTSOURCE,
            RECT { left: ox, top: oy, right: ox + cw, bottom: oy + ch },
        )
    };
    let visible = 1;
    let props = DWM_THUMBNAIL_PROPERTIES {
        dwFlags: flags,
        rcDestination: rcd,
        rcSource: rcs,
        opacity: 255,
        fVisible: visible,
        fSourceClientAreaOnly: 0,
    };
    let mut map = THUMBS.lock().unwrap();
    let map = map.get_or_insert_with(Default::default);
    let id = match map.get(&slot) {
        Some(&(sh, id)) if sh == hwnd && id != 0 => id,
        Some(&(_, id)) => {
            // 换源：注销旧注册
            if id != 0 {
                unsafe {
                    DwmUnregisterThumbnail(id);
                }
            }
            let mut nid: isize = 0;
            let hr = unsafe { DwmRegisterThumbnail(dest as HWND, shwnd, &mut nid) };
            if hr != 0 || nid == 0 {
                eprintln!(
                    "[t={}] DwmRegisterThumbnail 失败 slot={} hwnd={:#x} hr={}",
                    now_ms(),
                    slot,
                    hwnd,
                    hr
                );
                map.remove(&slot);
                return;
            }
            map.insert(slot.clone(), (hwnd, nid));
            nid
        }
        None => {
            let mut nid: isize = 0;
            let hr = unsafe { DwmRegisterThumbnail(dest as HWND, shwnd, &mut nid) };
            if hr != 0 || nid == 0 {
                eprintln!(
                    "[t={}] DwmRegisterThumbnail 失败 slot={} hwnd={:#x} hr={}",
                    now_ms(),
                    slot,
                    hwnd,
                    hr
                );
                return;
            }
            map.insert(slot.clone(), (hwnd, nid));
            nid
        }
    };
    let hr = unsafe { DwmUpdateThumbnailProperties(id, &props) };
    if hr != 0 {
        eprintln!("[t={}] DwmUpdateThumbnailProperties 失败 hr={}", now_ms(), hr);
        unsafe {
            DwmUnregisterThumbnail(id);
        }
        map.remove(&slot);
    }
}

// 已注册 slot 设为不可见（元素滚出/无内容时保持注册避免反复注册）
fn thumb_set_invisible(slot: &str) {
    let mut map = THUMBS.lock().unwrap();
    let map = map.get_or_insert_with(Default::default);
    if let Some(&(_, id)) = map.get(slot) {
        if id != 0 {
            let props = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_VISIBLE,
                rcDestination: RECT { left: 0, top: 0, right: 0, bottom: 0 },
                rcSource: RECT { left: 0, top: 0, right: 0, bottom: 0 },
                opacity: 255,
                fVisible: 0,
                fSourceClientAreaOnly: 0,
            };
            unsafe {
                DwmUpdateThumbnailProperties(id, &props);
            }
        }
    }
}

// 注销全部缩略图（幂等；回程序层/关闭覆盖层时调用）
pub fn thumb_clear() {
    let ids: Vec<isize> = {
        let mut map = THUMBS.lock().unwrap();
        match std::mem::take(&mut *map) {
            Some(m) => m.into_values().map(|(_, id)| id).collect(),
            None => Vec::new(),
        }
    };
    for id in ids {
        unsafe {
            DwmUnregisterThumbnail(id);
        }
    }
}

pub fn get_overlay_hwnd() -> isize {
    MOUSE_CTX
        .get()
        .map(|c| c.overlay_hwnd.load(Ordering::Relaxed))
        .unwrap_or(0)
}

pub fn foreground() -> isize {
    unsafe { GetForegroundWindow() as isize }
}

// 检测系统语言：返回界面语言 id（"zh-CN" / "en"），默认中文。
// 以系统 UI 语言的 LANGID 主语言为准（Windows 显示语言），用户区域设置可不同于 UI 语言。
// 主语言 ID 0x04 = LANG_CHINESE 归中文；LocaleName 前缀 zh 兜底；其余英文。
pub fn system_lang() -> &'static str {
    unsafe {
        // 1) UI 语言：用户 UI 语言 > 系统 UI 语言
        let mut langid = GetUserDefaultUILanguage();
        if langid == 0 {
            langid = GetSystemDefaultUILanguage();
        }
        // PRIMARYLANGID(lgid) = lgid & 0x3ff（0x0804 zh-CN → 0x04 = LANG_CHINESE）
        if langid & 0x3ff == 0x04 {
            return "zh-CN";
        }
        // 2) 兜底：用户区域设置名
        let mut buf = [0u16; 32];
        if GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) > 0 {
            let s = String::from_utf16_lossy(
                &buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())],
            );
            if s.to_lowercase().starts_with("zh") {
                return "zh-CN";
            }
        }
        // 3) LCID 兜底（上述 API 均失败时）
        let lcid = GetUserDefaultLCID();
        if lcid & 0x3ff == 0x04 {
            return "zh-CN";
        }
        "en"
    }
}

// taskmgr 等管理员程序受 UIPI 保护：非提权进程的钩子吞键被无视、SetForegroundWindow 被拒。
// 检测当前进程是否提权，未提权则按配置自提升重启。
// release 无控制台，stderr 无处可去；重定向到配置目录 %APPDATA%\WinHop\winhop.log
// 保证日志可查（与 config.json 同目录，升级/重装不丢）。
// 必须在任何 eprintln 之前调用（Rust 首次取 stderr 句柄时生效）。
pub fn redirect_stderr_to_file() {
    unsafe {
        let dir = std::env::var("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        let dir = dir.join("WinHop");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("winhop.log");
        // 日志轮转：超过 1MB 时把旧日志改名为 winhop.log.1（覆盖旧备份），本次启动重开新日志
        const MAX_LOG: u64 = 1024 * 1024;
        if let Ok(md) = std::fs::metadata(&path) {
            if md.len() > MAX_LOG {
                let _ = std::fs::remove_file(dir.join("winhop.log.1"));
                let _ = std::fs::rename(&path, dir.join("winhop.log.1"));
            }
        }
        let wide = to_wide(path.to_str().unwrap_or("winhop.log"));
        let file = CreateFileW(
            wide.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            OPEN_ALWAYS,
            0,
            std::ptr::null_mut(),
        );
        if file != INVALID_HANDLE_VALUE {
            SetFilePointer(file, 0, std::ptr::null_mut(), FILE_END);
            SetStdHandle(STD_ERROR_HANDLE, file);
        }
    }
}

// 单实例保护：第二个实例直接退出，避免两套钩子同时吞输入
pub fn acquire_single_instance() -> bool {
    unsafe {
        static MUTEX: OnceLock<isize> = OnceLock::new();
        let name = to_wide("WinHop_SingleInstance");
        let h = CreateMutexW(std::ptr::null_mut(), 1, name.as_ptr());
        if h.is_null() {
            return true; // 创建失败保守放行
        }
        let _ = MUTEX.set(h as isize); // 句柄保持到进程退出，OS 自动释放
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

pub fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elev: u32 = 0;
        let mut size: u32 = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elev as *mut u32 as *mut c_void,
            std::mem::size_of::<u32>() as u32,
            &mut size,
        );
        CloseHandle(token);
        ok != 0 && elev != 0
    }
}

pub fn relaunch_elevated() {
    unsafe {
        let exe = std::env::current_exe().expect("获取自身路径失败");
        let dir = std::env::current_dir().expect("获取当前目录失败");
        let file = to_wide(exe.to_str().expect("exe 路径非 UTF-8"));
        let op = to_wide("runas");
        let wd = to_wide(dir.to_str().expect("目录非 UTF-8"));
        let empty = to_wide("");
        let h = ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), file.as_ptr(), empty.as_ptr(), wd.as_ptr(), 1);
        if h as isize <= 32 {
            eprintln!("[winhop] 提权重启失败 {:?}", h);
        }
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // 回归：系统二进制须返回各自 FileDescription（如 "Application Frame Host"），
    // 不得是通用串「Microsoft® Windows® Operating System」
    //（曾因读 ProductName 导致多个系统窗口全部识别成 "Microsoft Windows"）
    #[test]
    fn system_exe_not_generic_name() {
        for p in [
            "C:\\Windows\\System32\\ApplicationFrameHost.exe",
            "C:\\Windows\\System32\\RuntimeBroker.exe",
        ] {
            if std::path::Path::new(p).exists() {
                let n = file_description(p);
                assert!(n.is_some(), "{} 应有版本资源", p);
                assert!(
                    !n.unwrap().contains("Operating System"),
                    "{} 返回了通用串",
                    p
                );
            }
        }
    }

    // 回归：版本资源长度字段不规范导致名称截断（val_len 不可靠，按 null 截断）。
    // Chrome 的 FileDescription 是 "Google Chrome"，若截断只剩 "Google"
    #[test]
    fn full_name_not_truncated() {
        let p = std::env::var("LOCALAPPDATA")
            .map(|d| {
                std::path::PathBuf::from(d)
                    .join("Google\\Chrome\\Application\\chrome.exe")
            })
            .unwrap_or_default();
        if p.exists() {
            assert_eq!(
                file_description(p.to_str().unwrap()).as_deref(),
                Some("Google Chrome")
            );
        }
    }
}
