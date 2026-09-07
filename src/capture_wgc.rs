//! Screen capture via Windows.Graphics.Capture.
//!
//! BitBlt reads the desktop through GDI, which misses anything the compositor
//! draws on the GPU: hardware-overlay video, protected content, and some
//! accelerated apps come back black. WGC asks DWM for the composed frame
//! instead, so what lands in the buffer is what the user actually sees.
//!
//! Everything here is fallible by design — the API is missing on older
//! Windows, unavailable in some session types, and can simply refuse. Every
//! entry point returns `Option`, and `capture.rs` falls back to BitBlt.

use std::cell::RefCell;

use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HANDLE, HMODULE};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_FLAG, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_RESOURCE_MISC_FLAG, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{HMONITOR, MONITOR_DEFAULTTONEAREST, MonitorFromPoint};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::Foundation::POINT;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::core::{Interface, Result};

// Cached D3D11 device. Building one costs milliseconds and every capture in a
// session wants the same one; it is only ever touched from the UI thread.
thread_local! {
    static DEVICE: RefCell<Option<Devices>> = const { RefCell::new(None) };
}

struct Devices {
    d3d: ID3D11Device,
    context: ID3D11DeviceContext,
    winrt: windows::Graphics::DirectX::Direct3D11::IDirect3DDevice,
}

/// Whether this machine can capture at all. Cheap after the first call.
pub fn is_supported() -> bool {
    GraphicsCaptureSession::IsSupported().unwrap_or(false)
}

fn create_devices() -> Result<Devices> {
    unsafe {
        let mut d3d: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            // WGC surfaces are BGRA; without this flag the device cannot bind them.
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d),
            None,
            Some(&mut context),
        )?;
        let d3d = d3d.ok_or_else(windows::core::Error::from_thread)?;
        let context = context.ok_or_else(windows::core::Error::from_thread)?;
        let dxgi: IDXGIDevice = d3d.cast()?;
        let inspectable = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?;
        let winrt = inspectable.cast()?;
        Ok(Devices {
            d3d,
            context,
            winrt,
        })
    }
}

fn with_devices<T>(f: impl FnOnce(&Devices) -> Option<T>) -> Option<T> {
    DEVICE.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = create_devices().ok();
        }
        let devices = slot.as_ref()?;
        f(devices)
    })
}

/// Drop the cached device, so the next capture rebuilds it. Called after a
/// capture fails, which is the symptom of a GPU device that was reset or
/// removed underneath us.
fn drop_devices() {
    DEVICE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// Grab the monitor containing `(x, y)` and return it as top-down BGRA along
/// with the monitor's own pixel size.
///
/// The caller crops; WGC works per-monitor, not per-rectangle.
pub fn capture_monitor_at(x: i32, y: i32) -> Option<(u32, u32, Vec<u8>, i32, i32)> {
    if !is_supported() {
        return None;
    }
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return None;
    }
    let result = with_devices(|devices| capture_monitor(devices, monitor).ok());
    if result.is_none() {
        drop_devices();
    }
    result
}

fn capture_monitor(
    devices: &Devices,
    monitor: HMONITOR,
) -> Result<(u32, u32, Vec<u8>, i32, i32)> {
    unsafe {
        let interop: IGraphicsCaptureItemInterop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = interop.CreateForMonitor(monitor)?;
        let size: SizeInt32 = item.Size()?;
        if size.Width <= 0 || size.Height <= 0 {
            return Err(windows::core::Error::from_thread());
        }

        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &devices.winrt,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            // One buffer: this is a one-shot grab, not a running stream.
            1,
            size,
        )?;
        let session = pool.CreateCaptureSession(&item)?;

        // The cursor is drawn by the overlay itself, and the capture border is
        // a recording affordance that would end up baked into the snapshot.
        // Both are best-effort: older builds do not have the setters.
        let _ = session.SetIsCursorCaptureEnabled(false);
        let _ = session.SetIsBorderRequired(false);

        // FrameArrived is what wakes us; a bare poll loop can spin for the
        // whole timeout on a desktop that is not changing.
        let signal = CaptureSignal::new()?;
        let waiter = signal.clone_handle();
        pool.FrameArrived(&TypedEventHandler::<
            Direct3D11CaptureFramePool,
            windows::core::IInspectable,
        >::new(move |_, _| {
            waiter.set();
            Ok(())
        }))?;

        session.StartCapture()?;
        // A desktop that is not changing still produces one frame on start, but
        // cap the wait so a refusing capture cannot hang the overlay.
        signal.wait(600);

        let frame = pool.TryGetNextFrame();
        // Stop capturing before touching the texture: the session holds the
        // frame pool alive and we want it torn down on every exit path.
        let _ = session.Close();

        let frame = match frame {
            Ok(f) => f,
            Err(e) => {
                let _ = pool.Close();
                return Err(e);
            }
        };

        let surface = frame.Surface()?;
        let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
        let texture: ID3D11Texture2D = access.GetInterface()?;

        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);

        // The captured texture lives on the GPU with no CPU access; copy it to
        // a staging texture that can be mapped.
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: D3D11_BIND_FLAG(0).0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
            ..desc
        };
        let mut staging: Option<ID3D11Texture2D> = None;
        devices
            .d3d
            .CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
        let staging = staging.ok_or_else(windows::core::Error::from_thread)?;
        devices.context.CopyResource(&staging, &texture);

        let mut mapped = Default::default();
        devices
            .context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;

        let width = desc.Width;
        let height = desc.Height;
        let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
        let row_bytes = (width as usize) * 4;
        let src = mapped.pData as *const u8;
        for row in 0..height as usize {
            std::ptr::copy_nonoverlapping(
                src.add(row * mapped.RowPitch as usize),
                pixels.as_mut_ptr().add(row * row_bytes),
                row_bytes,
            );
        }
        devices.context.Unmap(&staging, 0);
        let _ = pool.Close();

        // WGC frames are already premultiplied-opaque for the desktop, but the
        // alpha channel comes back as whatever the compositor left there; the
        // rest of the app treats captures as opaque.
        for px in pixels.chunks_exact_mut(4) {
            px[3] = 255;
        }

        // Where this monitor sits on the virtual desktop, so the caller can
        // crop with the same coordinates it would use for BitBlt.
        let (origin_x, origin_y) = monitor_origin(monitor);
        Ok((width, height, pixels, origin_x, origin_y))
    }
}

fn monitor_origin(monitor: HMONITOR) -> (i32, i32) {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
    unsafe {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            (info.rcMonitor.left, info.rcMonitor.top)
        } else {
            (0, 0)
        }
    }
}

/// A one-shot event the FrameArrived handler can signal from the thread pool.
struct CaptureSignal {
    handle: HANDLE,
}

impl CaptureSignal {
    fn new() -> Result<Self> {
        use windows::Win32::System::Threading::CreateEventW;
        let handle = unsafe { CreateEventW(None, true, false, None)? };
        Ok(Self { handle })
    }

    fn clone_handle(&self) -> SignalHandle {
        SignalHandle(self.handle)
    }

    /// Returns true if a frame arrived within `timeout_ms`.
    fn wait(&self, timeout_ms: u32) -> bool {
        use windows::Win32::Foundation::WAIT_OBJECT_0;
        use windows::Win32::System::Threading::WaitForSingleObject;
        unsafe { WaitForSingleObject(self.handle, timeout_ms) == WAIT_OBJECT_0 }
    }
}

impl Drop for CaptureSignal {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// The handle alone, so the event handler closure can be `Send` without owning
/// the lifetime. The event outlives the closure because `capture_monitor` does
/// not return until the session is closed.
#[derive(Clone, Copy)]
struct SignalHandle(HANDLE);

unsafe impl Send for SignalHandle {}
unsafe impl Sync for SignalHandle {}

impl SignalHandle {
    fn set(&self) {
        use windows::Win32::System::Threading::SetEvent;
        unsafe {
            let _ = SetEvent(self.0);
        }
    }
}

// No unit test lives here on purpose. Activating WinRT and running a capture
// inside a bare `cargo test` process hangs: the harness gives no apartment the
// capture broker is happy with, and the call never returns. The app itself
// initialises COM through OleInitialize and this path is exercised there, so
// it is verified by driving the real overlay rather than from the test binary.
