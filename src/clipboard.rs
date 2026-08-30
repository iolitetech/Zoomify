#![allow(dead_code)]

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::Graphics::Gdi::{BITMAPINFOHEADER, BI_RGB};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;

pub fn copy_bgra_to_clipboard(width: u32, height: u32, top_down_bgra: &[u8]) -> bool {
    if width == 0 || height == 0 || top_down_bgra.len() != (width * height * 4) as usize {
        return false;
    }

    unsafe {
        if OpenClipboard(None).is_err() {
            return false;
        }

        let _ = EmptyClipboard();

        let header_size = std::mem::size_of::<BITMAPINFOHEADER>();
        let image_size = (width * height * 4) as usize;
        let total_size = header_size + image_size;

        let h_global = match GlobalAlloc(GMEM_MOVEABLE, total_size) {
            Ok(h) if !h.is_invalid() => h,
            _ => {
                let _ = CloseClipboard();
                return false;
            }
        };

        let ptr = GlobalLock(h_global);
        if ptr.is_null() {
            let _ = CloseClipboard();
            return false;
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
        let row_pitch = (width * 4) as usize;
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

        let success = SetClipboardData(CF_DIB, Some(HANDLE(h_global.0))).is_ok();
        let _ = CloseClipboard();

        success
    }
}

pub fn get_clipboard_text() -> Option<String> {
    unsafe {
        if OpenClipboard(None).is_err() {
            return None;
        }

        let text = if let Ok(handle) = GetClipboardData(CF_UNICODETEXT) {
            if !handle.is_invalid() {
                let hglobal = HGLOBAL(handle.0);
                let ptr = GlobalLock(hglobal);
                if !ptr.is_null() {
                    let u16_ptr = ptr as *const u16;
                    let mut len = 0;
                    while *u16_ptr.add(len) != 0 {
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
