// 临时诊断工具：captest <hwnd> <out.bmp> [--full] [--noalpha] [--screen <x> <y>]
// 输出捕获的 BMP 供像素级分析（通道顺序、行方向、alpha）；
// --screen 模式同时 BitBlt 屏幕同区域作对照
use std::ffi::c_void;
use std::fs;

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
    GetWindowDC, ReleaseDC, SelectObject, SRCCOPY, StretchBlt, BITMAPINFO, BITMAPINFOHEADER,
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
    let hwnd = args[1].parse::<isize>().expect("hwnd");
    let out = &args[2];
    let full = args.iter().any(|a| a == "--full");
    let screen_cmp = args.iter().position(|a| a == "--screen");

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
        println!("window rect: {}x{}", w, hh);

        let (tw, th) = if full { (w, hh) } else { (160, 90) };
        let screen = GetWindowDC(std::ptr::null_mut());
        let full_mem = CreateCompatibleDC(screen);
        let full_bmp = CreateCompatibleBitmap(screen, w, hh);
        let thumb_mem = CreateCompatibleDC(screen);
        let thumb_bmp = CreateCompatibleBitmap(screen, tw, th);
        ReleaseDC(std::ptr::null_mut(), screen);
        SelectObject(full_mem, full_bmp);
        SelectObject(thumb_mem, thumb_bmp);

        let pw_ok = PrintWindow(h, full_mem, PW_RENDERFULLCONTENT);
        let sb_ok = StretchBlt(thumb_mem, 0, 0, tw, th, full_mem, 0, 0, w, hh, SRCCOPY);
        println!("PrintWindow={} StretchBlt={}", pw_ok, sb_ok);

        // 屏幕同区域对照：BitBlt 从屏幕 DC 直捕窗口所在区域，与 PrintWindow 输出对比
        if let Some(pos) = screen_cmp {
            let _x = args[pos + 1].parse::<i32>().unwrap();
            let _y = args[pos + 2].parse::<i32>().unwrap();
            let scr = GetWindowDC(std::ptr::null_mut());
            let scr_mem = CreateCompatibleDC(scr);
            let scr_bmp = CreateCompatibleBitmap(scr, tw, th);
            SelectObject(scr_mem, scr_bmp);
            let bb = BitBlt(
                scr_mem,
                0,
                0,
                tw,
                th,
                scr,
                rect.left,
                rect.top,
                SRCCOPY,
            );
            ReleaseDC(std::ptr::null_mut(), scr);
            println!("screen BitBlt={}", bb);
            let screen_bits = read_bits(scr_mem as isize, scr_bmp as isize, tw, th);
            let pw_bits = read_bits(thumb_mem as isize, thumb_bmp as isize, tw, th);
            let c = (th / 2 * tw + tw / 2) as usize * 4;
            println!(
                "pw center:     BGR={} {} {}",
                pw_bits[c], pw_bits[c + 1], pw_bits[c + 2]
            );
            println!(
                "screen center: BGR={} {} {}",
                screen_bits[c], screen_bits[c + 1], screen_bits[c + 2]
            );
            let mut diff = 0i64;
            for i in (0..screen_bits.len()).step_by(4) {
                diff += (screen_bits[i] as i64 - pw_bits[i] as i64).abs()
                    + (screen_bits[i + 1] as i64 - pw_bits[i + 1] as i64).abs()
                    + (screen_bits[i + 2] as i64 - pw_bits[i + 2] as i64).abs();
            }
            let n = (screen_bits.len() / 4) as i64;
            println!("avg abs diff per channel: {:.1}", diff as f64 / (n * 3) as f64);
            let _ = (scr_mem, scr_bmp);
        }

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
        let dib_ok = GetDIBits(
            thumb_mem,
            thumb_bmp,
            0,
            th as u32,
            bits.as_mut_ptr() as *mut c_void,
            &mut bi,
            0,
        );
        println!("GetDIBits={}", dib_ok);

        // 采样几个点
        let idx = |x: i32, y: i32| -> [u8; 4] {
            let i = (y * tw + x) as usize * 4;
            [bits[i], bits[i + 1], bits[i + 2], bits[i + 3]]
        };
        println!("sample top-left(0,0):      {:?}", idx(0, 0));
        println!("sample top-right(w-1,0):   {:?}", idx(tw - 1, 0));
        println!("sample center:             {:?}", idx(tw / 2, th / 2));
        println!("sample bottom-left(0,h-1): {:?}", idx(0, th - 1));
        println!("sample bottom-right:       {:?}", idx(tw - 1, th - 1));

        DeleteObject(full_bmp);
        DeleteObject(thumb_bmp);
        DeleteDC(full_mem);
        DeleteDC(thumb_mem);

        // 组装 BMP 落盘
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
        data.extend_from_slice(&32u16.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&((row_size * th) as u32).to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        for row in 0..th as usize {
            let start = row * (tw as usize * 4);
            data.extend_from_slice(&bits[start..start + tw as usize * 4]);
            data.extend(std::iter::repeat(0u8).take(row_size as usize - tw as usize * 4));
        }
        fs::write(out, &data).expect("write bmp");
        println!("written {} bytes -> {}", data.len(), out);
    }
}
