// WGC（Windows.Graphics.Capture）窗口快照：window_thumbnail 命令的取图实现。
// 直接向 DWM 合成器取窗口纹理，与遮挡无关——全屏选择页盖住目标窗口时，
// Chromium 系程序暂停客户区渲染，PrintWindow 只能拿到标题栏一条，WGC 不受影响。
// 窗口层缩略图/大预览已改走 DWM 缩略图（windows.rs thumb_set），本模块仅作备用路径。
// Win10 1903+ 可用，不可用时所有入口返回 None，调用方回退 PrintWindow 路径。

use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::core::{factory, Interface};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
    D3D11_BOX, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ,
    D3D11_SDK_VERSION, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_TYPELESS, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

// ===== D3D11 / WinRT 设备（进程一份） =====

struct DeviceCtx {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    rt: IDirect3DDevice,
}

// D3D11 对象本身线程安全，WinRT IDirect3DDevice 是 agile 的；跨线程使用安全
unsafe impl Send for DeviceCtx {}
unsafe impl Sync for DeviceCtx {}

static CTX: OnceLock<Option<DeviceCtx>> = OnceLock::new();

fn device() -> Option<&'static DeviceCtx> {
    CTX.get_or_init(|| unsafe { init_device() }).as_ref()
}

unsafe fn init_device() -> Option<DeviceCtx> {
    let mut dev: Option<ID3D11Device> = None;
    let mut ctx: Option<ID3D11DeviceContext> = None;
    D3D11CreateDevice(
        None,
        D3D_DRIVER_TYPE_HARDWARE,
        HMODULE::default(),
        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        None,
        D3D11_SDK_VERSION,
        Some(&mut dev),
        Some(&mut D3D_FEATURE_LEVEL::default()),
        Some(&mut ctx),
    )
    .ok()?;
    let dev = dev?;
    let ctx = ctx?;
    let dxgi = dev.cast::<IDXGIDevice>().ok()?;
    let rt: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)
        .ok()?
        .cast()
        .ok()?;
    Some(DeviceCtx { device: dev, ctx, rt })
}

fn create_item(hwnd: isize) -> Option<GraphicsCaptureItem> {
    let interop: IGraphicsCaptureItemInterop =
        factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().ok()?;
    unsafe { interop.CreateForWindow(HWND(hwnd as *mut _)).ok() }
}

// ===== 帧拷贝 =====

struct Frame {
    seq: u64,
    w: i32,
    h: i32,
    data: Vec<u8>, // 紧排 BGRA，自顶向下
}

// immediate context 非线程安全：快照/流的 FrameArrived 回调共用此锁序列化 D3D 操作
static D3D_LOCK: Mutex<()> = Mutex::new(());
static FRAME_SEQ: AtomicU64 = AtomicU64::new(0);

// 拷出最新一帧（staging 纹理 → 紧排 BGRA）。只在 FrameArrived 回调里调用。
unsafe fn copy_latest_frame(pool: &Direct3D11CaptureFramePool) -> Option<Frame> {
    let dev = device()?;
    let frame = pool.TryGetNextFrame().ok()?;
    let size = frame.ContentSize().ok()?;
    if size.Width <= 0 || size.Height <= 0 {
        return None;
    }
    let surface = frame.Surface().ok()?;
    let tex: ID3D11Texture2D = surface
        .cast::<IDirect3DDxgiInterfaceAccess>()
        .ok()?
        .GetInterface()
        .ok()?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    tex.GetDesc(&mut desc);
    // swapchain 纹理可能大于内容尺寸（滞后于窗口变化），取两者较小值
    let w = size.Width.min(desc.Width as i32);
    let h = size.Height.min(desc.Height as i32);
    if w <= 0 || h <= 0 {
        return None;
    }
    // TYPELESS 纹理不能 Map，配成对应 UNORM（CopySubresourceRegion 兼容同族格式）
    let fmt = if desc.Format == DXGI_FORMAT_B8G8R8A8_TYPELESS {
        DXGI_FORMAT_B8G8R8A8_UNORM
    } else {
        desc.Format
    };
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Width: w as u32,
        Height: h as u32,
        MipLevels: 1,
        ArraySize: 1,
        Format: fmt,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut staging: Option<ID3D11Texture2D> = None;
    if let Err(e) = dev.device.CreateTexture2D(&staging_desc, None, Some(&mut staging)) {
        eprintln!("[t={}] wgc CreateTexture2D 失败: {}", crate::windows::now_ms(), e);
        return None;
    }
    let staging = staging?;
    // CopySubresourceRegion 容忍源/目标尺寸差异（CopyResource 要求完全一致，不符时静默无效 → 全零）
    let staging_res: ID3D11Resource = staging.cast().ok()?;
    let tex_res: ID3D11Resource = tex.cast().ok()?;
    dev.ctx.CopySubresourceRegion(
        &staging_res,
        0,
        0,
        0,
        0,
        &tex_res,
        0,
        Some(&D3D11_BOX {
            left: 0,
            top: 0,
            front: 0,
            right: w as u32,
            bottom: h as u32,
            back: 1,
        }),
    );
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    if dev
        .ctx
        .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        .is_err()
    {
        eprintln!("[t={}] wgc Map 失败", crate::windows::now_ms());
        return None;
    }
    let pitch = mapped.RowPitch as usize;
    let row = w as usize * 4;
    let mut data = vec![0u8; row * h as usize];
    for y in 0..h as usize {
        let src = (mapped.pData as *const u8).add(y * pitch);
        let dst = data.as_mut_ptr().add(y * row);
        std::ptr::copy_nonoverlapping(src, dst, row);
    }
    dev.ctx.Unmap(&staging, 0);
    Some(Frame {
        seq: FRAME_SEQ.fetch_add(1, Ordering::Relaxed) + 1,
        w,
        h,
        data,
    })
}

// ===== 单帧快照（窗口层行缩略图） =====

// 快照间串行：前端可能并发 invoke，而 immediate context 不可并发
static SNAP_LOCK: Mutex<()> = Mutex::new(());
static SNAP_SLOT: Mutex<Option<Frame>> = Mutex::new(None);
static SNAP_CV: Condvar = Condvar::new();

// 一次性 session 取首帧 → 24bpp BMP。失败/超时（如最小化窗口无帧）返回 None。
pub fn snapshot_bmp(hwnd: isize, max_w: u32, max_h: u32) -> Option<Vec<u8>> {
    let _serial = SNAP_LOCK.lock().ok()?;
    let dev = device()?;
    let item = create_item(hwnd)?;
    let size = item.Size().ok()?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &dev.rt,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        2,
        SizeInt32 { Width: size.Width, Height: size.Height },
    )
    .ok()?;
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let handler_done = done.clone();
    *SNAP_SLOT.lock().unwrap() = None;
    pool.FrameArrived(&TypedEventHandler::new(
        move |pool: windows::core::Ref<'_, Direct3D11CaptureFramePool>, _| {
            if handler_done.load(Ordering::Relaxed) {
                return Ok(());
            }
            let Ok(pool) = pool.ok() else { return Ok(()) };
        let _g = D3D_LOCK.lock().unwrap();
        if let Some(f) = unsafe { copy_latest_frame(pool) } {
            *SNAP_SLOT.lock().unwrap() = Some(f);
            SNAP_CV.notify_all();
        }
        Ok(())
    }))
    .ok()?;
    let session = pool.CreateCaptureSession(&item).ok()?;
    session.StartCapture().ok()?;
    let f = wait_stable_frame(hwnd, Duration::from_millis(80), Duration::from_millis(450))?;
    done.store(true, Ordering::Relaxed);
    let _ = session.Close();
    let _ = pool.Close();
    Some(bgra_to_bmp_scaled(&f.data, f.w, f.h, max_w, max_h))
}

// 等帧稳定再取最新：WGC 首帧可能撞上 DWM 初始竞态（内容陈旧/不全），
// quiet 时间内无新帧视为稳定；150ms 仍无帧则 nudge 逼 DWM 重新合成
// （被遮挡且空闲的窗口可能不出初始帧）；总耗时不超过 cap
fn wait_stable_frame(hwnd: isize, quiet: Duration, cap: Duration) -> Option<Frame> {
    let start = Instant::now();
    let mut latest: Option<Frame> = None;
    let mut last_change = Instant::now();
    let mut nudged = false;
    loop {
        if latest.is_none() && !nudged && start.elapsed() >= Duration::from_millis(150) {
            nudged = true;
            crate::windows::nudge_compose(hwnd);
        }
        let mut g = SNAP_SLOT.lock().unwrap();
        if let Some(f) = g.take() {
            if latest.as_ref().map_or(true, |l| f.seq > l.seq) {
                latest = Some(f);
                last_change = Instant::now();
            }
        }
        if latest.is_some() && last_change.elapsed() >= quiet {
            return latest;
        }
        let now = Instant::now();
        if now >= start + cap {
            return latest;
        }
        let to = if latest.is_some() {
            quiet.saturating_sub(last_change.elapsed())
        } else {
            cap - (now - start)
        };
        let (g2, _) = SNAP_CV.wait_timeout(g, to.max(Duration::from_millis(1))).unwrap();
        drop(g2);
    }
}

// 紧排 BGRA → 等比最近邻缩放 → 24bpp BMP（自底向上，与 windows.rs 的 PrintWindow 输出同格式）
fn bgra_to_bmp_scaled(bgra: &[u8], w: i32, h: i32, max_w: u32, max_h: u32) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let scale = (max_w as f64 / w as f64).min(max_h as f64 / h as f64).min(1.0);
    let tw = ((w as f64 * scale) as usize).max(1);
    let th = ((h as f64 * scale) as usize).max(1);
    let trow = ((tw * 3 + 3) / 4) * 4;
    let mut out = vec![0u8; trow * th];
    for y in 0..th {
        // BMP 自底向上：文件第 y 行 = 图像自底向上第 y 行
        let srow = h - 1 - (((y as f64 + 0.5) * h as f64 / th as f64) as usize).min(h - 1);
        for x in 0..tw {
            let sx = (((x as f64 + 0.5) * w as f64 / tw as f64) as usize).min(w - 1);
            let si = (srow * w + sx) * 4;
            let di = y * trow + x * 3;
            out[di] = bgra[si + 2];
            out[di + 1] = bgra[si + 1];
            out[di + 2] = bgra[si];
        }
    }
    let file_size = 54u32 + trow as u32 * th as u32;
    let mut bmp = Vec::with_capacity(file_size as usize);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&file_size.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&54u32.to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&(tw as i32).to_le_bytes());
    bmp.extend_from_slice(&(th as i32).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&24u16.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&(trow as u32 * th as u32).to_le_bytes());
    bmp.extend_from_slice(&0i32.to_le_bytes());
    bmp.extend_from_slice(&0i32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&out);
    bmp
}
