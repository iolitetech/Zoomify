#![allow(dead_code)]

use std::ffi::c_void;
use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap, ID2D1RenderTarget, D2D1_BITMAP_PROPERTIES,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

#[derive(Clone)]
pub struct ScreenCapture {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // 32-bit BGRA top-down
}

impl ScreenCapture {
    pub fn capture_screen() -> Option<Self> {
        unsafe {
            let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let width = GetSystemMetrics(SM_CXVIRTUALSCREEN) as u32;
            let height = GetSystemMetrics(SM_CYVIRTUALSCREEN) as u32;

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
            let hbitmap = CreateDIBSection(
                Some(mem_dc),
                &bmi,
                DIB_RGB_COLORS,
                &mut bits_ptr,
                None,
                0,
            );

            if let Ok(hbitmap) = hbitmap {
                if !hbitmap.is_invalid() && !bits_ptr.is_null() {
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
                    std::ptr::copy_nonoverlapping(bits_ptr as *const u8, pixels.as_mut_ptr(), total_bytes);

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
            }

            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            None
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
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };

        unsafe {
            let pitch = self.width * 4;
            render_target.CreateBitmap(size, Some(self.pixels.as_ptr() as *const c_void), pitch, &props)
        }
    }

    pub fn crop(&self, crop_x: i32, crop_y: i32, crop_w: u32, crop_h: u32) -> Option<ScreenCapture> {
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
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }
}
