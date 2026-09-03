use std::sync::Once;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateFontW, CreatePen, CreateSolidBrush, DIB_RGB_COLORS, DT_CENTER,
    DT_LEFT, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, EndPaint, FONT_CHARSET,
    FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FONT_QUALITY, FW_BOLD, FW_NORMAL, HALFTONE, HBRUSH,
    PAINTSTRUCT, PS_SOLID, RoundRect, SRCCOPY, SelectObject, SetBkMode, SetStretchBltMode,
    SetTextColor, StretchDIBits, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
    GetWindowLongPtrW, KillTimer, RegisterClassExW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_DESTROY,
    WM_ERASEBKGND, WM_LBUTTONUP, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

static REGISTER_CLASS: Once = Once::new();
static CURRENT_NOTIFY_HWND: AtomicIsize = AtomicIsize::new(0);

struct SnipNotificationData {
    orig_width: u32,
    orig_height: u32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

pub fn show_snip_notification(width: u32, height: u32, pixels: &[u8]) {
    if width == 0 || height == 0 || pixels.is_empty() {
        return;
    }

    unsafe {
        let prev = HWND(CURRENT_NOTIFY_HWND.swap(0, Ordering::SeqCst) as *mut _);
        if !prev.is_invalid() {
            let _ = DestroyWindow(prev);
        }

        REGISTER_CLASS.call_once(|| {
            let hinst: HINSTANCE =
                windows::Win32::System::LibraryLoader::GetModuleHandleW(PCWSTR::null())
                    .unwrap_or_default()
                    .into();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(snip_notify_wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst,
                hIcon: windows::Win32::UI::WindowsAndMessaging::HICON::default(),
                hCursor: windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                    None,
                    windows::Win32::UI::WindowsAndMessaging::IDC_ARROW,
                )
                .unwrap_or_default(),
                hbrBackground: HBRUSH::default(),
                lpszMenuName: PCWSTR::null(),
                lpszClassName: w!("ZoomifySnipNotify"),
                hIconSm: windows::Win32::UI::WindowsAndMessaging::HICON::default(),
            };
            let _ = RegisterClassExW(&wc);
        });

        // Query active monitor where the cursor is located to properly position on multi-monitor setups
        let mon = crate::monitor::MonitorManager::get_monitor_from_cursor();
        let card_w = 300;
        let card_h = 220;
        let right_bound = mon.work_x + mon.work_width as i32;
        let bottom_bound = mon.work_y + mon.work_height as i32;
        let pos_x = (right_bound - card_w - 24).max(mon.work_x);
        let pos_y = (bottom_bound - card_h - 24).max(mon.work_y);

        let hinst: HINSTANCE =
            windows::Win32::System::LibraryLoader::GetModuleHandleW(PCWSTR::null())
                .unwrap_or_default()
                .into();

        let hwnd = match CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("ZoomifySnipNotify"),
            w!("Zoomify Snip"),
            WS_POPUP,
            pos_x,
            pos_y,
            card_w,
            card_h,
            None,
            None,
            Some(hinst),
            None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        };

        // Downscale thumbnail to max container dimensions (268x152) to avoid cloning tens of megabytes on UI thread
        let max_w = 268u32;
        let max_h = 152u32;
        let scale = (max_w as f32 / width as f32)
            .min(max_h as f32 / height as f32)
            .min(1.0);
        let thumb_w = ((width as f32 * scale).round() as u32).max(1);
        let thumb_h = ((height as f32 * scale).round() as u32).max(1);

        let mut thumb_pixels = vec![0u8; (thumb_w * thumb_h * 4) as usize];
        for ty in 0..thumb_h {
            let sy = (((ty as f32 / thumb_h as f32) * height as f32) as u32).min(height - 1);
            let src_row_offset = (sy * width * 4) as usize;
            let dst_row_offset = (ty * thumb_w * 4) as usize;
            for tx in 0..thumb_w {
                let sx = (((tx as f32 / thumb_w as f32) * width as f32) as u32).min(width - 1);
                let src_pixel_offset = src_row_offset + (sx * 4) as usize;
                let dst_pixel_offset = dst_row_offset + (tx * 4) as usize;
                thumb_pixels[dst_pixel_offset..dst_pixel_offset + 4]
                    .copy_from_slice(&pixels[src_pixel_offset..src_pixel_offset + 4]);
            }
        }

        let data = Box::new(SnipNotificationData {
            orig_width: width,
            orig_height: height,
            width: thumb_w,
            height: thumb_h,
            pixels: thumb_pixels,
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(data) as isize);

        CURRENT_NOTIFY_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

        // Auto-dismiss after 3.5 seconds
        let _ = SetTimer(Some(hwnd), 1, 3500, None);

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            hwnd,
            Some(windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST),
            pos_x,
            pos_y,
            card_w,
            card_h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

unsafe extern "system" fn snip_notify_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            WM_ERASEBKGND => LRESULT(1),

            WM_PAINT => {
                let raw_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const SnipNotificationData;
                if raw_ptr.is_null() {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                }
                let data = &*raw_ptr;

                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);

                let card_w = 300;
                let card_h = 220;

                // Double buffer
                let mem_dc = CreateCompatibleDC(Some(hdc));
                let mem_bmp = CreateCompatibleBitmap(hdc, card_w, card_h);
                let old_bmp = SelectObject(mem_dc, mem_bmp.into());

                // 1. Card background (Dark Acrylic)
                let bg_brush = CreateSolidBrush(rgb(26, 28, 34));
                let border_pen = CreatePen(PS_SOLID, 1, rgb(58, 64, 78));
                let old_brush = SelectObject(mem_dc, bg_brush.into());
                let old_pen = SelectObject(mem_dc, border_pen.into());

                let _ = RoundRect(mem_dc, 0, 0, card_w, card_h, 16, 16);

                let _ = SelectObject(mem_dc, old_brush);
                let _ = SelectObject(mem_dc, old_pen);
                let _ = DeleteObject(bg_brush.into());
                let _ = DeleteObject(border_pen.into());

                let _ = SetBkMode(mem_dc, TRANSPARENT);

                // 2. Title Header
                let title_font = CreateFontW(
                    -14,
                    0,
                    0,
                    0,
                    FW_BOLD.0 as i32,
                    0,
                    0,
                    0,
                    FONT_CHARSET(0),
                    FONT_OUTPUT_PRECISION(0),
                    FONT_CLIP_PRECISION(0),
                    FONT_QUALITY(0),
                    0,
                    w!("Segoe UI"),
                );
                let old_font = SelectObject(mem_dc, title_font.into());
                let _ = SetTextColor(mem_dc, rgb(245, 246, 250));

                let title_str = "Snip Copied to Clipboard";
                let mut title_utf16: Vec<u16> = title_str.encode_utf16().collect();
                let mut title_rect = RECT {
                    left: 16,
                    top: 10,
                    right: 260,
                    bottom: 28,
                };
                let _ = DrawTextW(
                    mem_dc,
                    &mut title_utf16,
                    &mut title_rect,
                    DT_LEFT | DT_SINGLELINE | DT_VCENTER,
                );

                // 3. Subtitle (Dimensions & Status)
                let sub_font = CreateFontW(
                    -12,
                    0,
                    0,
                    0,
                    FW_NORMAL.0 as i32,
                    0,
                    0,
                    0,
                    FONT_CHARSET(0),
                    FONT_OUTPUT_PRECISION(0),
                    FONT_CLIP_PRECISION(0),
                    FONT_QUALITY(0),
                    0,
                    w!("Segoe UI"),
                );
                let _ = SelectObject(mem_dc, sub_font.into());
                let _ = SetTextColor(mem_dc, rgb(150, 156, 170));

                let sub_str = format!(
                    "{} × {} px • Ready to paste",
                    data.orig_width, data.orig_height
                );
                let mut sub_utf16: Vec<u16> = sub_str.encode_utf16().collect();
                let mut sub_rect = RECT {
                    left: 16,
                    top: 29,
                    right: 260,
                    bottom: 47,
                };
                let _ = DrawTextW(
                    mem_dc,
                    &mut sub_utf16,
                    &mut sub_rect,
                    DT_LEFT | DT_SINGLELINE | DT_VCENTER,
                );

                // 4. Close '✕' Icon in top-right
                let mut close_rect = RECT {
                    left: 270,
                    top: 10,
                    right: 290,
                    bottom: 30,
                };
                let mut close_utf16: Vec<u16> = "✕".encode_utf16().collect();
                let _ = DrawTextW(
                    mem_dc,
                    &mut close_utf16,
                    &mut close_rect,
                    DT_CENTER | DT_SINGLELINE | DT_VCENTER,
                );

                let _ = SelectObject(mem_dc, old_font);
                let _ = DeleteObject(title_font.into());
                let _ = DeleteObject(sub_font.into());

                // 5. Image Thumbnail Box
                let box_x = 16;
                let box_y = 52;
                let box_w = 268;
                let box_h = 152;

                let box_bg_brush = CreateSolidBrush(rgb(16, 18, 22));
                let box_border_pen = CreatePen(PS_SOLID, 1, rgb(44, 48, 58));
                let old_b_brush = SelectObject(mem_dc, box_bg_brush.into());
                let old_b_pen = SelectObject(mem_dc, box_border_pen.into());

                let _ = RoundRect(mem_dc, box_x, box_y, box_x + box_w, box_y + box_h, 8, 8);

                let _ = SelectObject(mem_dc, old_b_brush);
                let _ = SelectObject(mem_dc, old_b_pen);
                let _ = DeleteObject(box_bg_brush.into());
                let _ = DeleteObject(box_border_pen.into());

                // Fitted thumbnail coordinates
                let inner_w = (box_w - 8) as f32;
                let inner_h = (box_h - 8) as f32;
                let img_w = data.width as f32;
                let img_h = data.height as f32;
                let scale = (inner_w / img_w).min(inner_h / img_h);

                let thumb_w = (img_w * scale).round().max(1.0) as i32;
                let thumb_h = (img_h * scale).round().max(1.0) as i32;
                let thumb_x = box_x + 4 + ((inner_w as i32 - thumb_w) / 2);
                let thumb_y = box_y + 4 + ((inner_h as i32 - thumb_h) / 2);

                let bmi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: data.width as i32,
                        biHeight: -(data.height as i32), // Top-down DIB
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        biSizeImage: 0,
                        biXPelsPerMeter: 0,
                        biYPelsPerMeter: 0,
                        biClrUsed: 0,
                        biClrImportant: 0,
                    },
                    bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default()],
                };

                let _ = SetStretchBltMode(mem_dc, HALFTONE);
                let _ = StretchDIBits(
                    mem_dc,
                    thumb_x,
                    thumb_y,
                    thumb_w,
                    thumb_h,
                    0,
                    0,
                    data.width as i32,
                    data.height as i32,
                    Some(data.pixels.as_ptr() as *const _),
                    &bmi,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );

                // Blit buffer to screen
                let _ = BitBlt(hdc, 0, 0, card_w, card_h, Some(mem_dc), 0, 0, SRCCOPY);

                let _ = SelectObject(mem_dc, old_bmp);
                let _ = DeleteObject(mem_bmp.into());
                let _ = DeleteDC(mem_dc);

                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }

            WM_TIMER => {
                let _ = KillTimer(Some(hwnd), wparam.0);
                let _ = CURRENT_NOTIFY_HWND.compare_exchange(
                    hwnd.0 as isize,
                    0,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }

            WM_LBUTTONUP => {
                let _ = CURRENT_NOTIFY_HWND.compare_exchange(
                    hwnd.0 as isize,
                    0,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }

            WM_DESTROY => {
                let raw_ptr =
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut SnipNotificationData;
                if !raw_ptr.is_null() {
                    let _ = Box::from_raw(raw_ptr);
                }
                LRESULT(0)
            }

            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
