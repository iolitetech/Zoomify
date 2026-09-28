#![allow(dead_code)]

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::Graphics::Gdi::{
    BI_BITFIELDS, BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, GetDC, GetDIBits,
    GetObjectW, HBITMAP, ReleaseDC,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::core::w;

const CF_BITMAP: u32 = 2;
const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;

/// `OpenClipboard` routinely fails with ERROR_ACCESS_DENIED while another
/// process holds the clipboard open - clipboard managers and Office do this
/// constantly. A single attempt makes copy fail at random, so retry briefly.
fn open_clipboard_retrying(hwnd: Option<HWND>) -> bool {
    const ATTEMPTS: u32 = 10;
    for attempt in 0..ATTEMPTS {
        if unsafe { OpenClipboard(hwnd) }.is_ok() {
            return true;
        }
        if attempt + 1 < ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    false
}

/// Build the CF_DIB payload (header + bottom-up 32bpp rows) into a fresh
/// `GMEM_MOVEABLE` block, ready to hand to `SetClipboardData`.
///
/// Frees the block itself on the one failure path that can happen after
/// allocating it (the lock), so a caller that gets `None` never has to.
unsafe fn build_dib_global(width: u32, height: u32, top_down_bgra: &[u8]) -> Option<HGLOBAL> {
    unsafe {
        let header_size = std::mem::size_of::<BITMAPINFOHEADER>();
        let image_size = (width as usize) * (height as usize) * 4;
        let total_size = header_size + image_size;

        let h_global = match GlobalAlloc(GMEM_MOVEABLE, total_size) {
            Ok(h) if !h.is_invalid() => h,
            _ => return None,
        };

        let ptr = GlobalLock(h_global);
        if ptr.is_null() {
            let _ = GlobalFree(Some(h_global));
            return None;
        }

        let header = BITMAPINFOHEADER {
            biSize: header_size as u32,
            biWidth: width as i32,
            biHeight: height as i32, // Positive height = bottom-up DIB (standard for clipboard)
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: image_size as u32,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };

        // Copy header
        std::ptr::copy_nonoverlapping(
            &header as *const _ as *const u8,
            ptr as *mut u8,
            header_size,
        );

        // Copy pixel rows in bottom-up order
        let row_pitch = width as usize * 4;
        let dest_pixel_ptr = (ptr as *mut u8).add(header_size);

        for row in 0..height {
            let src_row = (height - 1 - row) as usize;
            let src_offset = src_row * row_pitch;
            let dst_offset = row as usize * row_pitch;

            std::ptr::copy_nonoverlapping(
                top_down_bgra.as_ptr().add(src_offset),
                dest_pixel_ptr.add(dst_offset),
                row_pitch,
            );
        }

        let _ = GlobalUnlock(h_global);
        Some(h_global)
    }
}

/// Encode a PNG copy of the same pixels (for apps - Discord, Slack, Telegram,
/// browsers - that read CF_PNG rather than CF_DIB, preserving alpha without
/// the black-border look a DIB gives them) into a fresh `GMEM_MOVEABLE`
/// block. `None` on any failure (encoding, allocation, or lock); frees the
/// block itself if it was allocated but the lock failed.
unsafe fn build_png_global(
    width: u32,
    height: u32,
    top_down_bgra: &[u8],
) -> Option<(u32, HGLOBAL)> {
    let mut rgba = top_down_bgra.to_vec();
    for chunk in rgba.as_chunks_mut::<4>().0 {
        chunk.swap(0, 2);
    }

    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    image::ImageEncoder::write_image(
        encoder,
        &rgba,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )
    .ok()?;
    if png_bytes.is_empty() {
        return None;
    }

    let cf_png = unsafe { RegisterClipboardFormatW(w!("PNG")) };
    if cf_png == 0 {
        return None;
    }

    unsafe {
        let h_png = GlobalAlloc(GMEM_MOVEABLE, png_bytes.len()).ok()?;
        if h_png.is_invalid() {
            return None;
        }
        let png_ptr = GlobalLock(h_png);
        if png_ptr.is_null() {
            let _ = GlobalFree(Some(h_png));
            return None;
        }
        std::ptr::copy_nonoverlapping(png_bytes.as_ptr(), png_ptr as *mut u8, png_bytes.len());
        let _ = GlobalUnlock(h_png);
        Some((cf_png, h_png))
    }
}

pub fn copy_bgra_to_clipboard(hwnd: HWND, width: u32, height: u32, top_down_bgra: &[u8]) -> bool {
    if width == 0 || height == 0 || top_down_bgra.len() != (width as usize) * (height as usize) * 4
    {
        return false;
    }

    // Build both payloads *before* ever touching the clipboard.
    // EmptyClipboard discards whatever was there before; building these
    // first means a failure here (allocation, encoding) leaves the user's
    // existing clipboard content untouched instead of wiping it for nothing.
    let Some(h_dib) = (unsafe { build_dib_global(width, height, top_down_bgra) }) else {
        return false;
    };
    let png = unsafe { build_png_global(width, height, top_down_bgra) };

    if !open_clipboard_retrying(Some(hwnd)) {
        unsafe {
            let _ = GlobalFree(Some(h_dib));
        }
        if let Some((_, h_png)) = png {
            unsafe {
                let _ = GlobalFree(Some(h_png));
            }
        }
        return false;
    }

    unsafe {
        let _ = EmptyClipboard();

        // The system takes ownership of the handle only once SetClipboardData
        // succeeds - on failure it is still ours to free.
        let dib_success = SetClipboardData(CF_DIB, Some(HANDLE(h_dib.0))).is_ok();
        if !dib_success {
            let _ = GlobalFree(Some(h_dib));
        }

        if let Some((cf_png, h_png)) = png
            && SetClipboardData(cf_png, Some(HANDLE(h_png.0))).is_err()
        {
            let _ = GlobalFree(Some(h_png));
        }

        let _ = CloseClipboard();
        dib_success
    }
}

pub fn get_clipboard_text() -> Option<String> {
    if !open_clipboard_retrying(None) {
        return None;
    }

    unsafe {
        let text = if let Ok(handle) = GetClipboardData(CF_UNICODETEXT) {
            if !handle.is_invalid() {
                let hglobal = HGLOBAL(handle.0);
                let ptr = GlobalLock(hglobal);
                if !ptr.is_null() {
                    let u16_ptr = ptr as *const u16;
                    let max_chars = GlobalSize(hglobal) / 2;
                    let mut len = 0;
                    while len < max_chars && *u16_ptr.add(len) != 0 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(u16_ptr, len);
                    let result = String::from_utf16_lossy(slice);
                    let _ = GlobalUnlock(hglobal);
                    Some(result)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let _ = CloseClipboard();
        text
    }
}

/// An image lifted off the clipboard, as top-down BGRA.
pub struct ClipboardImage {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Read a bitmap from the clipboard, if there is one.
///
/// CF_DIB is what every screenshot tool and browser puts there, and is tried
/// first. Some apps — anything going through .NET's `Clipboard.SetImage`
/// among them — offer only CF_BITMAP, a GDI handle with no pixel bytes of its
/// own, so that is the fallback.
pub fn get_clipboard_image() -> Option<ClipboardImage> {
    if !open_clipboard_retrying(None) {
        return None;
    }
    let out = unsafe { read_dib().or_else(|| read_cf_bitmap()) };
    unsafe {
        let _ = CloseClipboard();
    }
    out
}

unsafe fn read_dib() -> Option<ClipboardImage> {
    unsafe {
        let handle = GetClipboardData(CF_DIB).ok()?;
        if handle.is_invalid() {
            return None;
        }
        let hglobal = HGLOBAL(handle.0);
        // Check the block is at least big enough to hold a BITMAPINFOHEADER
        // *before* reinterpreting its contents as one - GlobalSize needs no
        // lock, so this is safe to do first. A clipboard owner (a
        // deliberately hostile one, or just a buggy one) could otherwise hand
        // over a block smaller than the header, and `&*(ptr as *const
        // BITMAPINFOHEADER)` below would read past the end of it.
        let block_size = GlobalSize(hglobal);
        if block_size < std::mem::size_of::<BITMAPINFOHEADER>() {
            return None;
        }

        let ptr = GlobalLock(hglobal);
        if ptr.is_null() {
            return None;
        }

        let header = &*(ptr as *const BITMAPINFOHEADER);
        let width = header.biWidth;
        // A negative height means the rows are already top-down.
        let top_down = header.biHeight < 0;
        let height = header.biHeight.abs();
        let bpp = header.biBitCount as u32;

        // Only the two layouts that actually turn up on the clipboard.
        if width <= 0 || height <= 0 || (bpp != 24 && bpp != 32) {
            let _ = GlobalUnlock(hglobal);
            return None;
        }
        // Guard against something absurd before allocating for it.
        if width > 32768 || height > 32768 {
            let _ = GlobalUnlock(hglobal);
            return None;
        }
        // biSize is trusted below as the offset pixels start at; a header
        // claiming to be smaller than what we already validated the block
        // holds (an OS/2-style BITMAPCOREHEADER, or just a corrupt value)
        // is not a layout this function understands.
        if (header.biSize as usize) < std::mem::size_of::<BITMAPINFOHEADER>() {
            let _ = GlobalUnlock(hglobal);
            return None;
        }

        // Pixels start after the header and any colour masks the DIB declares.
        let mask_bytes = if header.biCompression == BI_BITFIELDS.0 {
            12
        } else {
            0
        };

        let w = width as usize;
        let h = height as usize;
        let src_stride = (w * bpp as usize).div_ceil(32) * 4; // rows pad to 4 bytes

        let needed = header.biSize as usize + mask_bytes + src_stride * h;
        if block_size < needed {
            let _ = GlobalUnlock(hglobal);
            return None;
        }
        // Computed only now that `needed` has been checked against the
        // block's actual size, so every row this function goes on to read
        // through `pixels` is inside the allocation.
        let pixels = (ptr as *const u8).add(header.biSize as usize + mask_bytes);
        let mut bgra = vec![0u8; w * h * 4];

        for row in 0..h {
            let src_row = if top_down { row } else { h - 1 - row };
            let src = pixels.add(src_row * src_stride);
            let dst = row * w * 4;
            for col in 0..w {
                let s = src.add(col * (bpp as usize / 8));
                let d = dst + col * 4;
                bgra[d] = *s;
                bgra[d + 1] = *s.add(1);
                bgra[d + 2] = *s.add(2);
                // A 32-bit DIB's fourth byte is usually zero rather than a
                // real alpha, and honouring it would paste an invisible image.
                bgra[d + 3] = 255;
            }
        }

        let _ = GlobalUnlock(hglobal);
        Some(ClipboardImage {
            width: width as u32,
            height: h as u32,
            bgra,
        })
    }
}

/// Fall back for a clipboard that only offers CF_BITMAP: a GDI handle with no
/// pixel bytes of its own until `GetDIBits` renders it into a buffer.
///
/// This is the common case for anything going through .NET's managed
/// clipboard API, which registers CF_BITMAP without ever writing CF_DIB.
unsafe fn read_cf_bitmap() -> Option<ClipboardImage> {
    unsafe {
        let handle = GetClipboardData(CF_BITMAP).ok()?;
        if handle.is_invalid() {
            return None;
        }
        let hbitmap = HBITMAP(handle.0);

        let mut bmp = BITMAP {
            bmType: 0,
            bmWidth: 0,
            bmHeight: 0,
            bmWidthBytes: 0,
            bmPlanes: 0,
            bmBitsPixel: 0,
            bmBits: std::ptr::null_mut(),
        };
        let got = GetObjectW(
            hbitmap.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bmp as *mut _ as *mut std::ffi::c_void),
        );
        if got == 0 || bmp.bmWidth <= 0 || bmp.bmHeight <= 0 {
            return None;
        }
        if bmp.bmWidth > 32768 || bmp.bmHeight > 32768 {
            return None;
        }

        let width = bmp.bmWidth as u32;
        let height = bmp.bmHeight as u32;

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                // Negative asks GDI to hand rows back top-down, so no flip
                // pass is needed afterwards.
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let hdc = GetDC(None);
        if hdc.is_invalid() {
            return None;
        }
        let mut bgra = vec![0u8; width as usize * height as usize * 4];
        let rows = GetDIBits(
            hdc,
            hbitmap,
            0,
            height,
            Some(bgra.as_mut_ptr() as *mut std::ffi::c_void),
            &mut info,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, hdc);

        if rows == 0 {
            return None;
        }
        // A 32-bit source's fourth byte is not a reliable alpha channel;
        // honouring it as-is would often paste an invisible image.
        for px in bgra.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }

        Some(ClipboardImage {
            width,
            height,
            bgra,
        })
    }
}
