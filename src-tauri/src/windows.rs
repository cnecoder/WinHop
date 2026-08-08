use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, OnceLock};
use windows_sys::Win32::Foundation::{
    CloseHandle, BOOL, GENERIC_WRITE, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, RECT,
    WPARAM, ERROR_ALREADY_EXISTS, GetLastError,
};
use windows_sys::Win32::Graphics::Gdi::EnumDisplayMonitors;
use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_QUERY};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, SetFilePointer, FILE_END, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_ALWAYS,
};
use windows_sys::Win32::System::Console::{SetStdHandle, STD_ERROR_HANDLE};
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, CreateMutexW, GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId,
    OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_MENU, keybd_event, KEYEVENTF_KEYUP,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, MSLLHOOKSTRUCT,
    SetForegroundWindow, SetWindowsHookExW, ShowWindow, SW_RESTORE, WM_LBUTTONDOWN, WM_MBUTTONDOWN,
    WM_RBUTTONDOWN, WH_MOUSE_LL,
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
    Enter,
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
    let ctx = match MOUSE_CTX.get() {
        Some(c) => c,
        None => return 0,
    };
    if code < 0 {
        return 0;
    }
    let wm = wparam as u32;
    if wm != WM_LBUTTONDOWN && wm != WM_RBUTTONDOWN && wm != WM_MBUTTONDOWN {
        return 0;
    }
    eprintln!(
        "[t={}] mouse {} thread={} vis={}",
        now_ms(),
        wm,
        thread_id(),
        ctx.visible.load(Ordering::Relaxed)
    );
    if !ctx.visible.load(Ordering::Relaxed) {
        return 0;
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
                return 1; // 吞掉点击，避免误操作原应用
            }
        }
    }
    0
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
        // 从 Translation 确定语言/代码页
        let mut lang = 0x0409u16;
        let mut cp = 0x04B0u16;
        let trans_key = to_wide("\\VarFileInfo\\Translation");
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
            let t = std::slice::from_raw_parts(trans as *const u8, 4);
            lang = (t[0] as u16) | ((t[1] as u16) << 8);
            cp = (t[2] as u16) | ((t[3] as u16) << 8);
        }
        // 取 FileDescription 与 ProductName 中较长者：部分软件（如 MobaXterm）
        // FileDescription 是短名（"MobaX"），完整名在 ProductName
        let mut best: Option<String> = None;
        for key in ["FileDescription", "ProductName"] {
            let path_str = format!("\\StringFileInfo\\{:04X}{:04X}\\{}", lang, cp, key);
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
                let s = std::slice::from_raw_parts(val as *const u16, (val_len as usize) / 2);
                let s = String::from_utf16_lossy(s);
                let s = s.trim_end_matches('\0').trim().to_string();
                if !s.is_empty()
                    && best.as_ref().map(|b| s.len() > b.len()).unwrap_or(true)
                {
                    best = Some(s);
                }
            }
        }
        best
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

pub fn activate(hwnd: isize) -> bool {
    unsafe {
        let h = hwnd as HWND;
        if IsIconic(h) != 0 {
            ShowWindow(h, SW_RESTORE);
        }
        let fg = GetForegroundWindow();
        if fg == h {
            return true;
        }
        let mut pid: u32 = 0;
        let tid = GetWindowThreadProcessId(h, &mut pid);
        let fg_tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let our_tid = GetCurrentThreadId();
        // 前台锁定绕过：输入线程挂到目标线程再激活
        AttachThreadInput(our_tid, fg_tid, 1);
        AttachThreadInput(our_tid, tid, 1);
        SetForegroundWindow(h);
        BringWindowToTop(h);
        AttachThreadInput(our_tid, fg_tid, 0);
        AttachThreadInput(our_tid, tid, 0);
        GetForegroundWindow() == h
    }
}

// 激活校验 + 前台锁破解：失败时注入一次 Alt 刷新「最近输入」状态再试。
// 必须在覆盖层关闭（visible=false）后调用，否则注入的 Alt 会被自己的钩子吞掉。
// 注意：目标窗口在另一个虚拟桌面时此方法无效（taskbar 会闪烁但无法显示）。
// 必须从 run_on_main_thread 的闭包里调用，绝不能在钩子回调内直接执行：
// SendInput 注入的键需要当前线程的消息泵派发，回调内执行会自我死锁。
pub fn activate_with_retry(hwnd: isize) {
    eprintln!("[t={}] 激活尝试 hwnd={:#x}", now_ms(), hwnd);
    if activate(hwnd) {
        return;
    }
    eprintln!("[t={}] 激活失败，注入 Alt 重试", now_ms());
    unsafe {
        keybd_event(VK_MENU as u8, 0, 0, 0);
        keybd_event(VK_MENU as u8, 0, KEYEVENTF_KEYUP, 0);
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
    if !activate(hwnd) {
        eprintln!("[t={}] 激活失败 hwnd={:#x}（可能在另一个虚拟桌面）", now_ms(), hwnd);
    }
}

pub fn overlay_visible(hwnd: isize) -> bool {
    unsafe { hwnd != 0 && IsWindowVisible(hwnd as HWND) != 0 }
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

// PrintWindow 直捕窗口内容（每窗口独立、遮挡无关）→ StretchBlt 等比缩小 → 24bpp BMP。
// 经实测验证：PrintWindow 与屏幕像素一致（平均差 <1）；
// 之前「色彩反转/失真」为误诊——对照时屏幕位置显示的是另一覆盖窗口。
// 24bpp 无 alpha 字节，消除 BMP 解码歧义（32bpp 的 alpha/方向问题曾导致显示异常）。
pub fn capture_window(hwnd: isize, max_w: u32, max_h: u32) -> Option<Vec<u8>> {
    unsafe {
        use windows_sys::Win32::Graphics::Gdi::{
            CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetDC,
            ReleaseDC, SelectObject, SRCCOPY, StretchBlt, BITMAPINFO, BITMAPINFOHEADER,
        };
        use windows_sys::Win32::Storage::Xps::PrintWindow;
        use windows_sys::Win32::UI::WindowsAndMessaging::PW_RENDERFULLCONTENT;

        let h = hwnd as HWND;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(h, &mut rect) == 0 {
            return None;
        }
        let w = rect.right - rect.left;
        let hh = rect.bottom - rect.top;
        if w <= 0 || hh <= 0 {
            return None;
        }
        let scale = ((max_w as f64) / w as f64)
            .min((max_h as f64) / hh as f64)
            .min(1.0);
        let tw = (w as f64 * scale).max(1.0) as i32;
        let th = (hh as f64 * scale).max(1.0) as i32;

        let screen = GetDC(std::ptr::null_mut());
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, tw, th);
        let tmp_mem = CreateCompatibleDC(screen);
        let tmp_bmp = CreateCompatibleBitmap(screen, w, hh);
        ReleaseDC(std::ptr::null_mut(), screen);
        if mem.is_null() || bmp.is_null() || tmp_mem.is_null() || tmp_bmp.is_null() {
            return None;
        }
        let _old = SelectObject(mem, bmp);
        let _old_tmp = SelectObject(tmp_mem, tmp_bmp);

        PrintWindow(h, tmp_mem, PW_RENDERFULLCONTENT);
        StretchBlt(mem, 0, 0, tw, th, tmp_mem, 0, 0, w, hh, SRCCOPY);
        DeleteObject(tmp_bmp);
        DeleteDC(tmp_mem);

        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: tw,
                biHeight: th, // 正高度 = 自底向上
                biPlanes: 1,
                biBitCount: 24, // 24bpp 无 alpha，最通用格式
                biCompression: 0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: std::mem::zeroed(),
        };
        let row_size = ((tw * 3 + 3) / 4) * 4; // 24bpp 行按 4 字节对齐
        let mut bits = vec![0u8; (row_size * th) as usize];
        GetDIBits(
            mem,
            bmp,
            0,
            th as u32,
            bits.as_mut_ptr() as *mut c_void,
            &mut bi,
            0,
        );

        DeleteObject(bmp);
        DeleteDC(mem);

        // 组装 24bpp BMP：54 字节头 + 对齐行像素
        let file_size = 54u32 + row_size as u32 * th as u32;
        let mut data = Vec::with_capacity(file_size as usize);
        data.extend_from_slice(b"BM");
        data.extend_from_slice(&file_size.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&54u32.to_le_bytes());
        data.extend_from_slice(&40u32.to_le_bytes());
        data.extend_from_slice(&(tw as i32).to_le_bytes());
        data.extend_from_slice(&(th as i32).to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&24u16.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&(row_size as u32 * th as u32).to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&bits[..(row_size * th) as usize]);
        Some(data)
    }
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len() / 3 * 4 + 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(T[(b0 >> 2) as usize] as char);
        out.push(T[(((b0 & 3) << 4) | (b1 >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(((b1 & 15) << 2) | (b2 >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 { T[(b2 & 63) as usize] as char } else { '=' });
    }
    out
}

// taskmgr 等管理员程序受 UIPI 保护：非提权进程的钩子吞键被无视、SetForegroundWindow 被拒。
// 检测当前进程是否提权，未提权则按配置自提升重启。
// release 无控制台，stderr 无处可去；重定向到 %TEMP%\wintab.log 保证日志可查。
// 必须在任何 eprintln 之前调用（Rust 首次取 stderr 句柄时生效）。
pub fn redirect_stderr_to_file() {
    unsafe {
        let path = std::env::temp_dir().join("wintab.log");
        let wide = to_wide(path.to_str().unwrap_or("wintab.log"));
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
        let name = to_wide("WinTab_SingleInstance");
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
            eprintln!("[wintab] 提权重启失败 {:?}", h);
        }
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
