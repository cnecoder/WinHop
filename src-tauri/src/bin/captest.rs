// 临时诊断工具 v2：captest <hwnd>
// PrintWindow 直捕 vs 虚拟屏幕坐标直捕，多点采样对比，验证 PrintWindow 颜色是否真的反转
use std::ffi::c_void;
use std::fs;

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetDC,
    ReleaseDC, SelectObject, SRCCOPY, BITMAPINFO, BITMAPINFOHEADER,
};
use windows_sys::Win32::Storage::Xps::PrintWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, PW_RENDERFULLCONTENT};

fn read_bits(mem: isize, bmp: isize, tw: i32, th: i32) -> Vec<u8> {
    unsafe {
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: tw,
                biHeight: th,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: std::mem::zeroed(),
        };
        let mut bits = vec![0u8; (tw * th * 4) as usize];
        GetDIBits(
            mem as *mut c_void,
            bmp as *mut c_void,
            0,
            th as u32,
            bits.as_mut_ptr() as *mut c_void,
            &mut bi,
            0,
        );
        bits
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--dwm") {
        return dwm_test();
    }
    let hwnd = args[1].parse::<isize>().expect("hwnd");

    unsafe {
        let h = hwnd as HWND;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        GetWindowRect(h, &mut rect);
        let w = rect.right - rect.left;
        let hh = rect.bottom - rect.top;
        println!("window rect: {}x{} at ({},{})", w, hh, rect.left, rect.top);

        let (tw, th) = (w, hh);
        let screen = GetDC(std::ptr::null_mut());

        // PrintWindow 直捕
        let pw_mem = CreateCompatibleDC(screen);
        let pw_bmp = CreateCompatibleBitmap(screen, tw, th);
        SelectObject(pw_mem, pw_bmp);
        let pw_ok = PrintWindow(h, pw_mem, PW_RENDERFULLCONTENT);
        let pw_bits = read_bits(pw_mem as isize, pw_bmp as isize, tw, th);

        // 屏幕直捕（虚拟坐标，多显示器正确）
        let scr_mem = CreateCompatibleDC(screen);
        let scr_bmp = CreateCompatibleBitmap(screen, tw, th);
        SelectObject(scr_mem, scr_bmp);
        let bb = BitBlt(scr_mem, 0, 0, tw, th, screen, rect.left, rect.top, SRCCOPY);
        let scr_bits = read_bits(scr_mem as isize, scr_bmp as isize, tw, th);
        ReleaseDC(std::ptr::null_mut(), screen);

        println!("PrintWindow={} screenBitBlt={}", pw_ok, bb);

        // 网格采样：3x3 点
        for gy in 0..3 {
            for gx in 0..3 {
                let x = (tw as i64 * gx / 2).min(tw as i64 - 1) as i32;
                let y = (hh as i64 * gy / 2).min(hh as i64 - 1) as i32;
                let i = (y * tw + x) as usize * 4;
                println!(
                    "({},{})  pw=BGR {},{},{}  scr=BGR {},{},{}",
                    x,
                    y,
                    pw_bits[i],
                    pw_bits[i + 1],
                    pw_bits[i + 2],
                    scr_bits[i],
                    scr_bits[i + 1],
                    scr_bits[i + 2]
                );
            }
        }

        // 平均差异
        let mut diff = 0i64;
        for i in (0..pw_bits.len()).step_by(4) {
            diff += (pw_bits[i] as i64 - scr_bits[i] as i64).abs()
                + (pw_bits[i + 1] as i64 - scr_bits[i + 1] as i64).abs()
                + (pw_bits[i + 2] as i64 - scr_bits[i + 2] as i64).abs();
        }
        let n = (pw_bits.len() / 4) as i64;
        println!("avg abs diff per channel: {:.1}", diff as f64 / (n * 3) as f64);

        // 落盘 PrintWindow 的 BMP 供人工查看
        let row_size = ((tw * 4 + 3) / 4) * 4;
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
        data.extend_from_slice(&((row_size * th) as u32).to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        for row in 0..th as usize {
            let start = row * (tw as usize * 4);
            for px in 0..tw as usize {
                data.extend_from_slice(&pw_bits[start + px * 4..start + px * 4 + 3]);
            }
            data.extend(std::iter::repeat(0u8).take(row_size as usize - tw as usize * 3));
        }
        fs::write("C:/Windows/Temp/pw_dump.bmp", &data).expect("write");
        println!("pw_dump.bmp written ({}x{})", tw, th);

        DeleteObject(pw_bmp);
        DeleteDC(pw_mem);
        DeleteObject(scr_bmp);
        DeleteDC(scr_mem);
    }
}

// DWM 缩略图方案验证：可见离屏目标窗口 + PrintWindow 读取
fn dwm_test() {
    unsafe {
        use windows_sys::Win32::Graphics::Dwm::{
            DwmFlush, DwmRegisterThumbnail, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
            DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_VISIBLE,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, RegisterClassExW, CS_HREDRAW, CS_VREDRAW,
            WNDCLASSEXW, WS_POPUP, WS_VISIBLE,
        };
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

        let args: Vec<String> = std::env::args().collect();
        let src = args
            .iter()
            .find(|a| a.parse::<isize>().is_ok())
            .and_then(|a| a.parse::<isize>().ok())
            .expect("hwnd");

        let class = "WinTabThumbTest".encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
        let hinst = GetModuleHandleW(std::ptr::null());
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.hInstance = hinst;
        wc.lpfnWndProc = Some(DefWindowProcW);
        wc.lpszClassName = class.as_ptr();
        wc.style = CS_HREDRAW | CS_VREDRAW;
        RegisterClassExW(&wc);
        let target = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP | WS_VISIBLE,
            -32000,
            -32000,
            640,
            360,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null_mut(),
        );
        println!("target window: {:?}", target);

        let mut hthumb: isize = 0;
        let hr = DwmRegisterThumbnail(target, src as HWND, &mut hthumb);
        println!("DwmRegisterThumbnail hr={} thumb={}", hr, hthumb);
        if hr >= 0 && hthumb != 0 {
            let props = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY,
                rcDestination: RECT {
                    left: 0,
                    top: 0,
                    right: 640,
                    bottom: 360,
                },
                rcSource: RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                },
                opacity: 255,
                fVisible: 1,
                fSourceClientAreaOnly: 0,
            };
            let up = DwmUpdateThumbnailProperties(hthumb, &props);
            println!("DwmUpdateThumbnailProperties hr={}", up);
            DwmFlush();
            std::thread::sleep(std::time::Duration::from_millis(150));

            // PrintWindow 读目标窗口
            let screen = GetDC(std::ptr::null_mut());
            let mem = CreateCompatibleDC(screen);
            let bmp = CreateCompatibleBitmap(screen, 640, 360);
            ReleaseDC(std::ptr::null_mut(), screen);
            SelectObject(mem, bmp);
            let pw = PrintWindow(target, mem, PW_RENDERFULLCONTENT);
            let bits = read_bits(mem as isize, bmp as isize, 640, 360);
            println!("PrintWindow(target)={}", pw);
            for (x, y) in [(0, 0), (320, 40), (320, 180), (320, 340), (639, 359)] {
                let i = (y * 640 + x) as usize * 4;
                println!(
                    "({},{}) BGR={},{},{}",
                    x,
                    y,
                    bits[i],
                    bits[i + 1],
                    bits[i + 2]
                );
            }
            // 落盘
            let mut data = Vec::new();
            data.extend_from_slice(b"BM");
            data.extend_from_slice(&(54u32 + 640 * 360 * 3).to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes());
            data.extend_from_slice(&54u32.to_le_bytes());
            data.extend_from_slice(&40u32.to_le_bytes());
            data.extend_from_slice(&640i32.to_le_bytes());
            data.extend_from_slice(&360i32.to_le_bytes());
            data.extend_from_slice(&1u16.to_le_bytes());
            data.extend_from_slice(&24u16.to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes());
            data.extend_from_slice(&0i32.to_le_bytes());
            data.extend_from_slice(&0i32.to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes());
            for row in 0..360 {
                let start = row * 640 * 4;
                for px in 0..640 {
                    data.extend_from_slice(&bits[start + px * 4..start + px * 4 + 3]);
                }
            }
            fs::write("C:/Windows/Temp/dwm_dump.bmp", &data).expect("write");
            println!("dwm_dump.bmp written");

            DwmUnregisterThumbnail(hthumb);
        }
    }
}
