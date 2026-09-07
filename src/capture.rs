#![allow(dead_code)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::Graphics::Direct2D::Common::{D2D_SIZE_U, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_PROPERTIES, ID2D1Bitmap, ID2D1RenderTarget};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};
use windows::core::Result;

#[derive(Clone)]
pub struct ScreenCapture {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // 32-bit BGRA top-down
}

/// Whether to try Windows.Graphics.Capture before falling back to BitBlt.
/// Set once from config at startup; captures happen too often to re-read a file.
static USE_WGC: AtomicBool = AtomicBool::new(true);

pub fn set_use_graphics_capture(enabled: bool) {
    USE_WGC.store(enabled, Ordering::Relaxed);
}

impl ScreenCapture {
    /// Grab a screen rectangle, preferring Windows.Graphics.Capture.
    ///
    /// WGC sees what the compositor composed — hardware-overlay video and
    /// accelerated surfaces that come back black through GDI — so it is tried
    /// first. It only works a whole monitor at a time, so a rectangle that
    /// spans displays (or any failure at all) drops through to BitBlt.
    pub fn capture_rect(x: i32, y: i32, width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        if USE_WGC.load(Ordering::Relaxed)
            && let Some(cap) = Self::capture_rect_wgc(x, y, width, height)
        {
            return Some(cap);
        }
        Self::capture_rect_bitblt(x, y, width, height)
    }

    fn capture_rect_wgc(x: i32, y: i32, width: u32, height: u32) -> Option<Self> {
        let (mon_w, mon_h, pixels, origin_x, origin_y) =
            crate::capture_wgc::capture_monitor_at(x, y)?;

        let src_x = x - origin_x;
        let src_y = y - origin_y;
        // Anything reaching outside this monitor is a job for BitBlt, which
        // reads the whole virtual desktop.
        if src_x < 0
            || src_y < 0
            || src_x as u32 + width > mon_w
            || src_y as u32 + height > mon_h
        {
            return None;
        }

        if src_x == 0 && src_y == 0 && width == mon_w && height == mon_h {
            return Some(Self {
                x,
                y,
                width,
                height,
                pixels,
            });
        }

        let mut out = vec![0u8; (width as usize) * (height as usize) * 4];
        let src_stride = mon_w as usize * 4;
        let dst_stride = width as usize * 4;
        for row in 0..height as usize {
            let s = (src_y as usize + row) * src_stride + src_x as usize * 4;
            let dbeg = row * dst_stride;
            out[dbeg..dbeg + dst_stride].copy_from_slice(&pixels[s..s + dst_stride]);
        }
        Some(Self {
            x,
            y,
            width,
            height,
            pixels: out,
        })
    }

    fn capture_rect_bitblt(x: i32, y: i32, width: u32, height: u32) -> Option<Self> {
        unsafe {
            if width == 0 || height == 0 {
                return None;
            }

            let screen_dc = GetDC(None);
            if screen_dc.is_invalid() {
                return None;
            }

            let mem_dc = CreateCompatibleDC(Some(screen_dc));
            if mem_dc.is_invalid() {
                let _ = ReleaseDC(None, screen_dc);
                return None;
            }

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    biHeight: -(height as i32), // Negative height = top-down DIB
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    biSizeImage: width * height * 4,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD {
                    rgbBlue: 0,
                    rgbGreen: 0,
                    rgbRed: 0,
                    rgbReserved: 0,
                }],
            };

            let mut bits_ptr: *mut c_void = std::ptr::null_mut();
            let hbitmap =
                CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0);

            if let Ok(hbitmap) = hbitmap
                && !hbitmap.is_invalid()
                && !bits_ptr.is_null()
            {
                let old_bitmap = SelectObject(mem_dc, hbitmap.into());

                // Fast hardware BitBlt without DWM pipeline stalls
                let _ = BitBlt(
                    mem_dc,
                    0,
                    0,
                    width as i32,
                    height as i32,
                    Some(screen_dc),
                    x,
                    y,
                    SRCCOPY,
                );

                let total_bytes = (width * height * 4) as usize;
                let mut pixels = vec![0u8; total_bytes];
                std::ptr::copy_nonoverlapping(
                    bits_ptr as *const u8,
                    pixels.as_mut_ptr(),
                    total_bytes,
                );

                // Ensure opaque alpha
                for chunk in pixels.chunks_exact_mut(4) {
                    chunk[3] = 255;
                }

                let _ = SelectObject(mem_dc, old_bitmap);
                let _ = DeleteObject(hbitmap.into());
                let _ = DeleteDC(mem_dc);
                let _ = ReleaseDC(None, screen_dc);

                return Some(Self {
                    x,
                    y,
                    width,
                    height,
                    pixels,
                });
            }

            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            None
        }
    }

    pub fn capture_screen() -> Option<Self> {
        unsafe {
            let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let width = GetSystemMetrics(SM_CXVIRTUALSCREEN) as u32;
            let height = GetSystemMetrics(SM_CYVIRTUALSCREEN) as u32;
            Self::capture_rect(x, y, width, height)
        }
    }

    pub fn create_d2d_bitmap(&self, render_target: &ID2D1RenderTarget) -> Result<ID2D1Bitmap> {
        let size = D2D_SIZE_U {
            width: self.width,
            height: self.height,
        };

        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };

        unsafe {
            let pitch = self.width * 4;
            render_target.CreateBitmap(
                size,
                Some(self.pixels.as_ptr() as *const c_void),
                pitch,
                &props,
            )
        }
    }

    pub fn crop(
        &self,
        crop_x: i32,
        crop_y: i32,
        crop_w: u32,
        crop_h: u32,
    ) -> Option<ScreenCapture> {
        if crop_w == 0 || crop_h == 0 {
            return None;
        }

        let rel_x = crop_x - self.x;
        let rel_y = crop_y - self.y;

        if rel_x < 0 || rel_y < 0 {
            return None;
        }

        let start_x = (rel_x as u32).min(self.width);
        let start_y = (rel_y as u32).min(self.height);
        let actual_w = crop_w.min(self.width.saturating_sub(start_x));
        let actual_h = crop_h.min(self.height.saturating_sub(start_y));

        if actual_w == 0 || actual_h == 0 {
            return None;
        }

        let mut cropped_pixels = vec![0u8; (actual_w * actual_h * 4) as usize];
        let src_pitch = (self.width * 4) as usize;
        let dst_pitch = (actual_w * 4) as usize;

        for row in 0..actual_h {
            let src_offset = ((start_y + row) as usize * src_pitch) + (start_x as usize * 4);
            let dst_offset = row as usize * dst_pitch;
            cropped_pixels[dst_offset..dst_offset + dst_pitch]
                .copy_from_slice(&self.pixels[src_offset..src_offset + dst_pitch]);
        }

        Some(ScreenCapture {
            x: crop_x,
            y: crop_y,
            width: actual_w,
            height: actual_h,
            pixels: cropped_pixels,
        })
    }

    pub fn save_png(&self, path: &str) -> std::io::Result<()> {
        let mut rgba_pixels = self.pixels.clone();
        for chunk in rgba_pixels.chunks_exact_mut(4) {
            let b = chunk[0];
            let r = chunk[2];
            chunk[0] = r;
            chunk[2] = b;
        }

        image::save_buffer(
            path,
            &rgba_pixels,
            self.width,
            self.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| std::io::Error::other(e.to_string()))
    }
}
