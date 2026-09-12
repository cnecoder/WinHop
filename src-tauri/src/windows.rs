use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, OnceLock};

use windows_sys::core::GUID;
use windows_sys::Win32::System::Com::{
    CoInitializeEx, COINIT_APARTMENTTHREADED, StructuredStorage::PROPVARIANT,
};
use windows_sys::Win32::System::Variant::VT_LPWSTR;
use windows_sys::Win32::UI::Shell::PropertiesSystem::{PROPERTYKEY, SHGetPropertyStoreForWindow};
use windows_sys::Win32::Foundation::{
    CloseHandle, BOOL, GENERIC_WRITE, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, POINT,
    RECT, WPARAM, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, GetLastError,
};
use windows_sys::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DwmRegisterThumbnail, DwmUnregisterThumbnail,
    DwmUpdateThumbnailProperties, DWM_THUMBNAIL_PROPERTIES, DWM_TNP_RECTDESTINATION,
    DWM_TNP_RECTSOURCE, DWM_TNP_VISIBLE, DWMWA_CLOAKED,
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
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegEnumKeyExW, RegOpenKeyExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
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
    GetForegroundWindow, GetWindowLongW, GetWindowPlacement, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, MSLLHOOKSTRUCT,
    SetForegroundWindow, SetWindowsHookExW, WS_EX_TOOLWINDOW, GWL_EXSTYLE,
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

// PWA 虚拟进程名前缀。Chromium PWA（网站安装成应用）窗口与普通浏览窗口同属
// chrome.exe/msedge.exe，但承载 PWA 窗口的进程命令行带 "--app-id=<32 位 id>"，
// 普通窗口进程没有。用 "pwa#<app-id>" 作为独立分组键。
pub const PWA_PROC_PREFIX: &str = "pwa#";

// PROCESS_BASIC_INFORMATION（仅取 PebBaseAddress，x64 偏移 8）
#[repr(C)]
struct ProcessBasicInformation {
    reserved1: *mut c_void,
    peb_base_address: *mut c_void,
    reserved2: [*mut c_void; 2],
    unique_process_id: *mut c_void,
    reserved3: *mut c_void,
}

// RTL_UNICODE_STRING：Length/MaximumLength 为字节数，x64 下 Buffer 偏移 8
#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationProcess(
        process: HANDLE,
        info_class: u32,
        info: *mut c_void,
        info_length: u32,
        return_length: *mut u32,
    ) -> i32;
}

// PKEY_AppUserModel_ID = {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}, pid 5
const PKEY_APPUSERMODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID {
        data1: 0x9f4c2855,
        data2: 0x9f79,
        data3: 0x4b39,
        data4: [0xa8, 0xd0, 0xe1, 0xd4, 0x2d, 0xe1, 0xd5, 0xf3],
    },
    pid: 5,
};

// IPropertyStore 的 COM vtable（windows-sys 是 raw 绑定，方法需自行经 vtable 调用）
#[repr(C)]
struct PropertyStoreVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32,
    get_at: unsafe extern "system" fn(*mut c_void, u32, *mut PROPERTYKEY) -> i32,
    get_value:
        unsafe extern "system" fn(*mut c_void, *const PROPERTYKEY, *mut PROPVARIANT) -> i32,
    set_value:
        unsafe extern "system" fn(*mut c_void, *const PROPERTYKEY, *const PROPVARIANT) -> i32,
    commit: unsafe extern "system" fn(*mut c_void) -> i32,
}

#[repr(C)]
struct PropertyStore {
    vtbl: *const PropertyStoreVtbl,
}

// 读窗口显式 AppUserModelID（PKEY_AppUserModel_ID）。失败/为空返回 None。
// 必须在 STA 线程调用（见 enum_windows 的 CoInitializeEx），否则跨进程读回空。
// 注意：跨进程读 Chromium 窗口时该串可能被系统截断（实测稳定缺固定若干字符），
// 故不能直接当 app-id；本处只用其中的 "._crx_" 标记做「是否 PWA 窗口」的布尔判定，
// 完整 32 位 id 改由进程命令行 --app-id 取得（见 process_command_line）。
fn window_aumid(hwnd: HWND) -> Option<String> {
    unsafe {
        let iid = GUID {
            data1: 0x886d8eeb,
            data2: 0x8cf2,
            data3: 0x4446,
            data4: [0x8d, 0x02, 0xcd, 0xba, 0x1d, 0xbd, 0xcf, 0x99],
        };
        let mut store: *mut PropertyStore = std::ptr::null_mut();
        let hr = SHGetPropertyStoreForWindow(
            hwnd,
            &iid,
            &mut store as *mut _ as *mut *mut c_void,
        );
        if hr != 0 || store.is_null() {
            return None;
        }
        let value = {
            let mut pv: PROPVARIANT = std::mem::zeroed();
            let gv = ((*store).vtbl.as_ref().unwrap().get_value)(
                store as *mut c_void,
                &PKEY_APPUSERMODEL_ID,
                &mut pv,
            );
            if gv == 0 && pv.Anonymous.Anonymous.vt == VT_LPWSTR {
                let p = pv.Anonymous.Anonymous.Anonymous.pwszVal;
                if p.is_null() {
                    None
                } else {
                    // 指向目标进程属性存储内部缓冲：不得 CoTaskMemFree/PropVariantClear，
                    // Release 前直接拷贝即可。
                    let mut len = 0usize;
                    while *p.add(len) != 0 {
                        len += 1;
                    }
                    Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)))
                }
            } else {
                None
            }
        };
        ((*store).vtbl.as_ref().unwrap().release)(store as *mut c_void);
        value
    }
}

// 从浏览器进程命令行提取 PWA 的 32 位 app-id（"--app-id=<32 位小写>"）。
// 普通浏览窗口进程命令行无此标记 → None。纯函数便于单测。
fn pwa_app_id(cmdline: &str) -> Option<&str> {
    let marker = "--app-id=";
    let rest = cmdline.split(marker).nth(1)?;
    let id = rest.split(['"', ' ']).next().unwrap_or("");
    if id.len() == 32 && id.bytes().all(|b| b.is_ascii_lowercase()) {
        Some(id)
    } else {
        None
    }
}

// PWA 两种安装形态（同一窗口 AUMID 信号统一分类）：
// - crx：Chrome（及旧版 Edge、Brave/Vivaldi/Opera/Arc 等全系 Chromium）。窗口 AUMID
//   形如 "<宿主>._crx_<32 位 id>"；同进程可同时承载普通浏览窗口，须按窗口判定。
// - AppX：新版 Edge「安装为应用」装成 hosted MSIX 包（manifest 声明
//   HostRuntimeDependency=Microsoft.MicrosoftEdge.Stable）。窗口 AUMID =
//   "<PackageFamilyName>!App"，宿主 msedge 进程命令行裸空（PEB 取 id 无效），
//   只能靠 AUMID；同进程可承载多个不同 PWA 包，仍按窗口判定。
// 入参为窗口 AUMID 与小写 exe 名；返回分组键（PWA_PROC_PREFIX 之后的部分），非 PWA 返回 None。
fn classify_pwa(aumid: &str, exe: &str) -> Option<String> {
    // 分组键统一小写：配置读取会把 process 归一化为小写、普通 exe 名本就小写；
    // AppX 的 PackageFamilyName 含大写十六进制（如 …-2DE81424_…），不归一化会导致
    // 配置的 PWA 代号/名称匹配不上枚举窗口。
    if let Some(id) = aumid.split("._crx_").nth(1) {
        if !id.is_empty() {
            return Some(id.to_lowercase());
        }
    }
    if exe == "msedge.exe" {
        let pfn = aumid.strip_suffix("!App")?;
        // 排除基础宿主 AUMID（…!MSEDGE 不以 !App 结尾，本就不命中）；PFN 必须含
        // 发布者哈希段（name_hash），避免把普通短串误判成包
        if pfn.contains('_') && pfn != "MSEdge" {
            return Some(pfn.to_lowercase());
        }
    }
    None
}

// 读远程进程命令行：NtQueryInformationProcess 取 PEB → ProcessParameters(x64 0x20)
// → CommandLine UNICODE_STRING(x64 0x70) → ReadProcessMemory。任何失败返回 None
// （退回普通按 exe 分组）。不走窗口 AUMID：实测跨进程 SHGetPropertyStoreForWindow
// 对 Chromium 读到的 AUMID 会被截断（缺固定若干字符），不可靠；命令行是进程自身
// 启动参数，确定且逐进程精确。
fn process_command_line(pid: u32) -> Option<String> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::PROCESS_VM_READ;

    unsafe fn read_mem(h: HANDLE, addr: usize, buf: *mut c_void, len: usize) -> bool {
        unsafe {
            let (mut off, mut remaining) = (0usize, len);
            while remaining > 0 {
                let mut got = 0usize;
                if ReadProcessMemory(
                    h,
                    addr.wrapping_add(off) as *const c_void,
                    (buf as usize).wrapping_add(off) as *mut c_void,
                    remaining,
                    &mut got,
                ) == 0
                    || got == 0
                {
                    return false;
                }
                off += got;
                remaining -= got;
            }
            true
        }
    }

    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
        if h.is_null() {
            return None;
        }
        let result = (|| -> Option<String> {
            let mut pbi: ProcessBasicInformation = std::mem::zeroed();
            let status = NtQueryInformationProcess(
                h,
                0, // ProcessBasicInformation
                &mut pbi as *mut _ as *mut c_void,
                std::mem::size_of::<ProcessBasicInformation>() as u32,
                std::ptr::null_mut(),
            );
            if status != 0 || pbi.peb_base_address.is_null() {
                return None;
            }
            // PEB->ProcessParameters
            let mut params = 0usize;
            if !read_mem(
                h,
                pbi.peb_base_address as usize + 0x20,
                &mut params as *mut _ as *mut c_void,
                std::mem::size_of::<usize>(),
            ) || params == 0
            {
                return None;
            }
            // RTL_USER_PROCESS_PARAMETERS->CommandLine
            let mut cmd = UnicodeString {
                length: 0,
                maximum_length: 0,
                buffer: std::ptr::null(),
            };
            if !read_mem(
                h,
                params + 0x70,
                &mut cmd as *mut _ as *mut c_void,
                std::mem::size_of::<UnicodeString>(),
            ) || cmd.buffer.is_null() || cmd.length == 0
            {
                return None;
            }
            let n = (cmd.length as usize) / 2;
            let mut wide = vec![0u16; n];
            if !read_mem(
                h,
                cmd.buffer as usize,
                wide.as_mut_ptr() as *mut c_void,
                cmd.length as usize,
            ) {
                return None;
            }
            Some(String::from_utf16_lossy(&wide))
        })();
        CloseHandle(h);
        result
    }
}

// 浏览器 PWA 名表：app-id → 名称。来源：Chrome/Edge profile 的
// "User Data\<Profile>\Web Applications\_crx_<app-id>\<名>.lnk"（.lnk 主文件名即 PWA 名）。
// 首次使用时扫描全部本地 profile 一次。
fn pwa_names() -> &'static HashMap<String, String> {
    static NAMES: OnceLock<HashMap<String, String>> = OnceLock::new();
    NAMES.get_or_init(scan_pwa_names)
}

fn scan_pwa_names() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(local) = std::env::var("LOCALAPPDATA") else {
        return map;
    };
    // Chrome 与 Edge 的 User Data 根；下一层的每个 profile 目录里找 Web Applications
    for rel in [
        "Google\\Chrome\\User Data",
        "Microsoft\\Edge\\User Data",
    ] {
        scan_user_data(&std::path::Path::new(&local).join(rel), &mut map);
    }
    map
}

fn scan_user_data(base: &std::path::Path, map: &mut HashMap<String, String>) {
    let Ok(profiles) = std::fs::read_dir(base) else {
        return;
    };
    for prof in profiles.flatten() {
        let webapps = prof.path().join("Web Applications");
        let Ok(dirs) = std::fs::read_dir(&webapps) else {
            continue;
        };
        for d in dirs.flatten() {
            let dir_name = d.file_name();
            let Some(name) = dir_name.to_str() else {
                continue;
            };
            let Some(app_id) = name.strip_prefix("_crx_") else {
                continue;
            };
            if map.contains_key(app_id) {
                continue;
            }
            // 目录内有同名 <PWA名>.lnk 与 <PWA名>.ico，取 .lnk 主文件名
            let Ok(files) = std::fs::read_dir(d.path()) else {
                continue;
            };
            for f in files.flatten() {
                let p = f.path();
                if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        if !stem.is_empty() {
                            map.insert(app_id.to_string(), stem.to_string());
                        }
                    }
                }
            }
        }
    }
}

// AppX（Edge 托管 PWA）显示名缓存：PackageFamilyName → 显示名。包安装后名不变，
// 进程内缓存即可；只缓存命中项（未安装包罕见，重复读 manifest 代价可接受）。
fn appx_name_cache() -> &'static std::sync::Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

// 由 PackageFamilyName 经包仓库注册表解析 PackageFullName（含版本/架构/发布者哈希）。
// HKCU\…\AppModel\Repository\Families\<PFN> 下每个子键是一个已安装版本的完整包名。
fn appx_package_full_name(pfn: &str) -> Option<String> {
    unsafe {
        let sub = to_wide(&format!(
            "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Families\\{}",
            pfn
        ));
        let mut fam: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            KEY_READ,
            &mut fam,
        ) != 0
            || fam.is_null()
        {
            return None;
        }
        let mut full = None;
        let mut buf = [0u16; 260];
        for i in 0.. {
            let mut len = buf.len() as u32;
            let rc = RegEnumKeyExW(
                fam,
                i,
                buf.as_mut_ptr(),
                &mut len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if rc != 0 {
                break; // 259=ERROR_NO_MORE_ITEMS
            }
            full = Some(String::from_utf16_lossy(&buf[..len as usize]));
            break; // 通常只装一个版本，取首个即可
        }
        RegCloseKey(fam);
        full
    }
}

// 从 AppxManifest.xml 取包显示名。托管 Web 应用包的 <Properties><DisplayName> 为内联
// 字面量（非 ms-resource），且位于 <Applications> 之前——取首个 <DisplayName> 即包名。
// 纯函数，便于单测。
fn extract_manifest_display_name(xml: &str) -> Option<String> {
    let start_tag = xml.find("<DisplayName")?;
    let open_end = start_tag + xml[start_tag..].find('>')? + 1;
    let close = xml[open_end..].find("</DisplayName>")? + open_end;
    let raw = xml[open_end..close].trim();
    if raw.is_empty() || raw.starts_with("ms-resource:") {
        return None;
    }
    Some(decode_xml_entities(raw))
}

// 最小 XML 实体解码（托管包 DisplayName 可能用 &#xXXXX; 数字字符引用与 &amp; 等）。
// 按 UTF-8 字符切片处理，不能逐字节拷贝（显示名常含中文）。
fn decode_xml_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let semi_rel = rest[amp + 1..].find(';');
        match semi_rel {
            Some(rel) => {
                let semi = amp + 1 + rel;
                let ent = &rest[amp + 1..semi];
                let ch = if let Some(hex) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X"))
                {
                    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                } else if let Some(dec) = ent.strip_prefix('#') {
                    dec.parse::<u32>().ok().and_then(char::from_u32)
                } else {
                    match ent {
                        "amp" => Some('&'),
                        "lt" => Some('<'),
                        "gt" => Some('>'),
                        "quot" => Some('"'),
                        "apos" => Some('\''),
                        _ => None,
                    }
                };
                match ch {
                    Some(c) => out.push(c),
                    None => out.push_str(&rest[amp..=semi]), // 识别不了原样保留
                }
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[amp + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn appx_app_name(pfn: &str) -> Option<String> {
    if let Some(n) = appx_name_cache().lock().unwrap().get(pfn) {
        return Some(n.clone());
    }
    let full = appx_package_full_name(pfn)?;
    let programfiles = std::env::var("ProgramW6432")
        .or_else(|_| std::env::var("ProgramFiles"))
        .ok()?;
    let manifest = std::path::Path::new(&programfiles)
        .join("WindowsApps")
        .join(&full)
        .join("AppxManifest.xml");
    let xml = std::fs::read_to_string(&manifest).ok()?;
    let name = extract_manifest_display_name(&xml)?;
    appx_name_cache()
        .lock()
        .unwrap()
        .insert(pfn.to_string(), name.clone());
    Some(name)
}

// 供 lib.rs 取 PWA 显示名。key=32 位小写 crx id 走浏览器快捷方式名；其余（含下划线与
// 发布者哈希的 PackageFamilyName）走 AppX 包清单名。未找到时由调用方回退窗口标题/id。
pub fn pwa_app_name(key: &str) -> Option<String> {
    if key.len() == 32 && key.bytes().all(|b| b.is_ascii_lowercase()) {
        pwa_names().get(key).cloned()
    } else {
        appx_app_name(key)
    }
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
    // pid → 该进程命令行 --app-id（无则空串），避免同进程多窗口重复读 PEB
    app_ids: HashMap<u32, String>,
}

pub fn enum_windows() -> Vec<WinInfo> {
    // 读窗口 AUMID 要求调用线程为 STA（MTA 下跨进程读回空）。调用方线程的套间
    // 类型不可控（Tauri 运行时线程可能已是 MTA），故固定在新建的 STA 线程枚举。
    std::thread::scope(|s| {
        s.spawn(|| {
            unsafe {
                CoInitializeEx(
                    std::ptr::null::<c_void>(),
                    COINIT_APARTMENTTHREADED as u32,
                );
            }
            let monitors = enum_monitors();
            let mut ctx = EnumCtx {
                out: Vec::new(),
                monitors,
                app_ids: HashMap::new(),
            };
            unsafe {
                EnumWindows(Some(enum_proc), &mut ctx as *mut EnumCtx as isize);
            }
            ctx.out
        })
        .join()
        .unwrap_or_default()
    })
}

// DWM cloaked 判定：挂起的 UWP/后台应用、其它虚拟桌面的窗口，DWM 会标记为 cloaked。
// 这类窗口 IsWindowVisible 仍为真、也有标题，但实际上不可见、无法激活（cc-switch、
// MobaXterm 等会多出一个「打不开的窗口」多属此类）。非 0 即 cloaked。
fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            &mut cloaked as *mut u32 as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        ) == 0
            && cloaked != 0
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    if is_cloaked(hwnd) {
        return 1; // 挂起/后台/其它虚拟桌面的幽灵窗口：激活不了，排除
    }
    let mut own_pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, &mut own_pid);
    if own_pid != 0 && own_pid == GetCurrentProcessId() {
        return 1; // 排除自身（覆盖层）
    }
    // 工具窗口（WS_EX_TOOLWINDOW）不进 Alt-Tab/Win+Tab，多为辅助弹窗，排除（与系统切换器一致）
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if ex_style & WS_EX_TOOLWINDOW != 0 {
        return 1;
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
    let (mut process, path) = process_name(hwnd);
    // PWA 判定（窗口级，统一两种形态）：浏览器进程会同时承载普通窗口/多个 PWA，不能按进程
    // 分组，逐窗口读 AUMID 分类（classify_pwa）：
    // - crx（Chrome 等）：AUMID "<宿主>._crx_<id>"，跨进程读可能截断 → 优先进程命令行
    //   --app-id 补全完整 32 位 id，缺失再回退 AUMID 后缀（仍独立成组）。
    // - AppX（新版 Edge）：AUMID "<PFN>!App"，宿主进程命令行裸空，直接以 PFN 为键。
    if matches!(process.as_str(), "chrome.exe" | "msedge.exe") {
        if let Some(aumid) = window_aumid(hwnd) {
            let is_crx = aumid.contains("._crx_");
            if let Some(mut key) = classify_pwa(&aumid, &process) {
                if is_crx {
                    if !ctx.app_ids.contains_key(&own_pid) {
                        let id = process_command_line(own_pid)
                            .and_then(|cmd| pwa_app_id(&cmd).map(str::to_string))
                            .unwrap_or_default();
                        ctx.app_ids.insert(own_pid, id);
                    }
                    let cmd_id = ctx.app_ids.get(&own_pid).map(String::as_str).unwrap_or("");
                    if !cmd_id.is_empty() {
                        key = cmd_id.to_string();
                    }
                }
                process = format!("{}{}", PWA_PROC_PREFIX, key);
            }
        }
    }
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

// 用默认浏览器打开 URL（设置页 GitHub 仓库引导）。只允许 https://，防注入其它 ShellExecute 动词。
pub fn open_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("仅允许 https URL".into());
    }
    unsafe {
        let file = to_wide(url);
        let open = to_wide("open");
        let empty = to_wide("");
        let h = ShellExecuteW(
            std::ptr::null_mut(),
            open.as_ptr(),
            file.as_ptr(),
            empty.as_ptr(),
            empty.as_ptr(),
            1,
        );
        if h as isize <= 32 {
            return Err(format!("打开链接失败 {:?}", h));
        }
    }
    Ok(())
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

// 开机自启：HKCU\Software\Microsoft\Windows\CurrentVersion\Run 下写 WinHop 值（REG_SZ）。
// HKCU 无需提权；enable=false 删除该值（不存在返回 ERROR_FILE_NOT_FOUND=2 视为成功）。
// 命令行为加引号的 exe 路径，防路径含空格（Program Files）时资源管理器解析错误。
const AUTOSTART_RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const AUTOSTART_VALUE: &str = "WinHop";

pub fn set_autostart(enable: bool) -> Result<(), String> {
    // debug 构建（target\debug\winhop.exe）是 console 子系统，开机自启会弹黑终端；
    // 且 dev 验证与正式版共享 %APPDATA%\WinHop\config.json，若写 Run 键会把自启指向
    // debug 路径，污染正式用户环境（启动对齐/设置保存两处都走这里）。debug 一律跳过，
    // 正式 release（windows_subsystem="windows"）才真正读写注册表。
    if cfg!(debug_assertions) {
        eprintln!("[winhop] debug 构建跳过自启注册表写入（enable={}）", enable);
        return Ok(());
    }
    unsafe {
        let mut hkey: HKEY = std::mem::zeroed();
        let subkey = to_wide(AUTOSTART_RUN_KEY);
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            std::ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut hkey,
            std::ptr::null_mut(),
        );
        if status != 0 {
            return Err(format!("打开 Run 注册表项失败 code={}", status));
        }
        let name = to_wide(AUTOSTART_VALUE);
        let result = if enable {
            let exe = std::env::current_exe().map_err(|e| format!("获取 exe 路径失败: {}", e))?;
            let cmd = format!("\"{}\"", exe.display());
            let data = to_wide(&cmd);
            RegSetValueExW(
                hkey,
                name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr() as *const u8,
                (data.len() * std::mem::size_of::<u16>()) as u32,
            )
        } else {
            RegDeleteValueW(hkey, name.as_ptr())
        };
        RegCloseKey(hkey);
        if result != 0 && !( !enable && result == ERROR_FILE_NOT_FOUND ) {
            return Err(format!("写入自启注册表失败 code={}", result));
        }
        Ok(())
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
        // 依赖系统二进制存在；缺失时显式标注 SKIP（不空转绿，暴露「断言未执行」）
        let mut checked = 0;
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
                checked += 1;
            } else {
                eprintln!("SKIP: {} 不存在，跳过该断言", p);
            }
        }
        if checked == 0 {
            eprintln!("SKIP: system_exe_not_generic_name 无目标文件，全部断言未执行");
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
        } else {
            // 本机/CI 未装 Chrome：断言无法执行，显式标注而非空转绿
            eprintln!("SKIP: Chrome 未安装（{} 不存在），full_name_not_truncated 未执行", p.display());
        }
    }

    #[test]
    fn pwa_app_id_parses() {
        // 承载 PWA 的浏览器进程命令行带 --app-id=<32 位小写>
        assert_eq!(
            pwa_app_id(r#""C:\Program Files\Google\Chrome\Application\chrome_proxy.exe" --profile-directory=Default --app-id=mjoklplbddabcmpepnokjaffbmgbkkgg"#),
            Some("mjoklplbddabcmpepnokjaffbmgbkkgg")
        );
        assert_eq!(
            pwa_app_id("msedge.exe --app-id=agimnkijcaahngcdmfeangaknmldooml --other"),
            Some("agimnkijcaahngcdmfeangaknmldooml")
        );
        // 引号结尾也能截到
        assert_eq!(
            pwa_app_id(r#"chrome.exe --app-id=mjoklplbddabcmpepnokjaffbmgbkkgg""#),
            Some("mjoklplbddabcmpepnokjaffbmgbkkgg")
        );
        // 普通浏览器窗口进程命令行无 --app-id → None
        assert_eq!(pwa_app_id(r#"chrome.exe --type=browser"#), None);
        // app-id 长度/字符非法 → None，防把其它参数误判成 PWA
        assert_eq!(pwa_app_id("chrome.exe --app-id=short"), None);
        assert_eq!(pwa_app_id("chrome.exe --app-id=MJOKLPLBDDABCMPEPNOKJAFFBMGKKGG"), None);
        assert_eq!(pwa_app_id("anything"), None);
    }

    #[test]
    fn pwa_classify_distinguishes_forms() {
        // crx 形态：Chrome / 旧 Edge，取 ._crx_ 后缀（可能截断，命令行补全在枚举侧）
        assert_eq!(
            classify_pwa("Chrome._crx_mjoklplbddabcmpepnokjaffbmgbkkgg", "chrome.exe"),
            Some("mjoklplbddabcmpepnokjaffbmgbkkgg".to_string())
        );
        assert_eq!(
            classify_pwa("MSEdge._crx_agimnkijcaahngcdmfeangaknmldooml", "msedge.exe"),
            Some("agimnkijcaahngcdmfeangaknmldooml".to_string())
        );
        // AppX 形态：新版 Edge，AUMID=<PFN>!App，取完整 PFN 并归一为小写
        //（PFN 含大写十六进制；注册表/文件路径大小写不敏感，但配置匹配要求小写）
        assert_eq!(
            classify_pwa("www.volcengine.com-2DE81424_m5663p5smhvk4!App", "msedge.exe"),
            Some("www.volcengine.com-2de81424_m5663p5smhvk4".to_string())
        );
        // 普通浏览窗口不是 PWA
        assert_eq!(classify_pwa("Chrome", "chrome.exe"), None);
        assert_eq!(classify_pwa("MSEdge", "msedge.exe"), None);
        // Edge 宿主自身 AUMID 以 !MSEDGE 结尾（非 !App），不命中
        assert_eq!(
            classify_pwa(
                "Microsoft.MicrosoftEdge.Stable_8wekyb3d8bbwe!MSEDGE",
                "msedge.exe"
            ),
            None
        );
        // 非 Edge 进程不得仅凭 !App 后缀判成 PWA
        assert_eq!(classify_pwa("whatever!App", "chrome.exe"), None);
        // crx 后缀为空不命中
        assert_eq!(classify_pwa("Chrome._crx_", "chrome.exe"), None);
    }

    #[test]
    fn manifest_display_name_extracts_first_prop() {
        let xml = r#"<?xml version="1.0"?>
<Package xmlns="x"><Properties>
 <DisplayName>&#x65B9;&#x821F; Agent Plan</DisplayName>
 <PublisherDisplayName>www.volcengine.com</PublisherDisplayName>
</Properties>
<Applications><Application Id="App">
 <uap:VisualElements DisplayName="&#x65B9;&#x821F; Agent Plan &amp; co"/>
</Application></Applications></Package>"#;
        // 首个 <DisplayName> 在 <Properties>，内联字面量 + 数字字符引用解码
        assert_eq!(
            extract_manifest_display_name(xml),
            Some("方舟 Agent Plan".to_string())
        );
        // ms-resource 占位名不可直接用 → None（交由回退）
        assert_eq!(
            extract_manifest_display_name("<Properties><DisplayName>ms-resource:AppName</DisplayName></Properties>"),
            None
        );
        assert_eq!(extract_manifest_display_name("<nope/>"), None);
        // 命名实体
        assert_eq!(
            decode_xml_entities("a&amp;b&lt;c&quot;d"),
            "a&b<c\"d".to_string()
        );
    }

    // 集成：本机装有 Edge 托管 PWA（AppX）时，PFN 经注册表 Families→包清单应取到非空名。
    // CI/未装环境显式 SKIP，不空转绿。
    #[test]
    fn appx_name_resolves_when_edge_pwa_installed() {
        let pfn = "www.volcengine.com-2DE81424_m5663p5smhvk4";
        if appx_package_full_name(pfn).is_none() {
            eprintln!("SKIP: 未安装 Edge 托管 PWA 包 {}，appx 取名链未验证", pfn);
            return;
        }
        let name = appx_app_name(pfn).expect("已装包应取到清单显示名");
        assert!(!name.is_empty());
        assert!(!name.starts_with("ms-resource:"));
        eprintln!("appx 名: {}", name);
    }
}
