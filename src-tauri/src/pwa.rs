//! 浏览器 PWA 子系统：窗口级 PWA 检测（AUMID/命令行）、分组键分类、显示名解析。
//! 自 windows.rs 拆出（纯组织性搬移，行为不变）——该子系统自成一体约 450 行，
//! 与窗口枚举/激活等 Win32 通用功能边界清晰。
//!
//! 两种安装形态（同一窗口 AUMID 信号统一分类，见 classify_pwa）：
//! - crx：Chrome（及旧版 Edge、Brave/Vivaldi/Opera/Arc 等全系 Chromium）。窗口 AUMID
//!   形如 "<宿主>._crx_<32 位 id>"；同进程可同时承载普通浏览窗口，须按窗口判定。
//! - AppX：新版 Edge「安装为应用」装成 hosted MSIX 包。窗口 AUMID =
//!   "<PackageFamilyName>!App"，宿主 msedge 进程命令行裸空（PEB 取 id 无效），
//!   只能靠 AUMID；同进程可承载多个不同 PWA 包，仍按窗口判定。

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::OnceLock;

use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows_sys::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::System::Variant::VT_LPWSTR;
use windows_sys::Win32::UI::Shell::PropertiesSystem::{PROPERTYKEY, SHGetPropertyStoreForWindow};

use crate::windows::to_wide;

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
pub(crate) fn window_aumid(hwnd: HWND) -> Option<String> {
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
pub(crate) fn pwa_app_id(cmdline: &str) -> Option<&str> {
    let marker = "--app-id=";
    let rest = cmdline.split(marker).nth(1)?;
    let id = rest.split(['"', ' ']).next().unwrap_or("");
    if id.len() == 32 && id.bytes().all(|b| b.is_ascii_lowercase()) {
        Some(id)
    } else {
        None
    }
}

// 入参为窗口 AUMID 与小写 exe 名；返回分组键（PWA_PROC_PREFIX 之后的部分），非 PWA 返回 None。
pub(crate) fn classify_pwa(aumid: &str, exe: &str) -> Option<String> {
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
pub(crate) fn process_command_line(pid: u32) -> Option<String> {
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

// 逐窗口做 PWA 判定的宿主浏览器名单。crx 形态的 AUMID 结构 "<宿主>._crx_<id>" 全系
// Chromium 同源，classify_pwa 靠 ._crx_ 标记识别、不依赖具体浏览器名；AppX !App 形态
// 仅新版 Edge 有，已在 classify_pwa 内限定 exe。未列出的浏览器窗口按普通程序分组，
// 不列举就不会误伤。取名只扫 Chrome/Edge 的 User Data，其余浏览器 PWA 回退窗口标题。
pub(crate) fn is_chromium_host(exe: &str) -> bool {
    matches!(
        exe,
        "chrome.exe"
            | "msedge.exe"
            | "brave.exe"
            | "vivaldi.exe"
            | "opera.exe"
            | "chromium.exe"
            | "arc.exe"
    )
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn chromium_host_whitelist_covers_browsers_only() {
        for exe in ["chrome.exe", "msedge.exe", "brave.exe", "vivaldi.exe", "opera.exe", "chromium.exe", "arc.exe"] {
            assert!(is_chromium_host(exe), "{} 应在宿主名单", exe);
        }
        // 非浏览器 / 相近名不误伤
        for exe in ["firefox.exe", "notepad.exe", "chrome.exe.old", "mychrome.exe", ""] {
            assert!(!is_chromium_host(exe), "{} 不应在宿主名单", exe);
        }
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
