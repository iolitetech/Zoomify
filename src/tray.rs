#![allow(dead_code)]

use windows::core::{s, w, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, GetDC, ReleaseDC,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, RGBQUAD,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos,
    LoadIconW, SetForegroundWindow, TrackPopupMenuEx, HICON, ICONINFO, IDI_APPLICATION,
    MF_POPUP, MF_SEPARATOR, MF_STRING, TPM_LEFTALIGN, TPM_RIGHTBUTTON,
};

pub const WM_TRAY_ICON: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 200;

pub const ID_TRAY_STATIC_ZOOM: usize = 2001;
pub const ID_TRAY_DRAW: usize = 2002;
pub const ID_TRAY_SPOTLIGHT: usize = 2003;
pub const ID_TRAY_LIVE_ZOOM: usize = 2004;
pub const ID_TRAY_SNIP: usize = 2005;
pub const ID_TRAY_TIMER: usize = 2006;
pub const ID_TRAY_CHEATSHEET: usize = 2007;
pub const ID_TRAY_ABOUT: usize = 2008;
pub const ID_TRAY_EXIT: usize = 2009;
pub const ID_TRAY_LASER: usize = 2010;
pub const ID_TRAY_ERASER: usize = 2011;
pub const ID_TRAY_WHITEBOARD: usize = 2012;
pub const ID_TRAY_BLACKBOARD: usize = 2013;
pub const ID_TRAY_OPEN_CONFIG: usize = 2014;
pub const ID_TRAY_RESET_TOOLBAR: usize = 2015;

/// Enable authentic Windows 11 / Windows 10 Dark Mode for Win32 popup menus
pub fn enable_windows_dark_mode_for_menus() {
    unsafe {
        if let Ok(hmodule) = LoadLibraryA(s!("uxtheme.dll")) {
            if !hmodule.is_invalid() {
                // Ordinal 135 is SetPreferredAppMode
                let proc = GetProcAddress(hmodule, windows::core::PCSTR(135 as *const u8));
                if let Some(f) = proc {
                    type SetPreferredAppModeFn = unsafe extern "system" fn(i32) -> i32;
                    let set_preferred_app_mode: SetPreferredAppModeFn = std::mem::transmute(f);
                    let _ = set_preferred_app_mode(2); // 2 = ForceDark
                }
            }
        }
    }
}

pub struct TrayIcon {
    hwnd: HWND,
    hicon: HICON,
    nid: NOTIFYICONDATAW,
}

impl TrayIcon {
    pub fn new(hwnd: HWND) -> Self {
        enable_windows_dark_mode_for_menus();

        let hicon = Self::create_app_icon();

        let mut tip_chars = [0u16; 128];
        let tip_str = "Zoomify — Screen Zoom, Spotlight & Annotation (Windows 11 Fluent)";
        for (i, c) in tip_str.encode_utf16().enumerate().take(127) {
            tip_chars[i] = c;
        }

        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY_ICON,
            hIcon: hicon,
            szTip: tip_chars,
            ..Default::default()
        };

        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        }

        Self { hwnd, hicon, nid }
    }

    pub fn show_balloon(&mut self, title: &str, message: &str) {
        let mut info_title = [0u16; 64];
        for (i, c) in title.encode_utf16().enumerate().take(63) {
            info_title[i] = c;
        }

        let mut info_text = [0u16; 256];
        for (i, c) in message.encode_utf16().enumerate().take(255) {
            info_text[i] = c;
        }

        self.nid.uFlags |= NIF_INFO;
        self.nid.szInfoTitle = info_title;
        self.nid.szInfo = info_text;

        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.nid);
        }
    }

    pub fn show_menu(&self) {
        unsafe {
            let menu = match CreatePopupMenu() {
                Ok(m) if !m.is_invalid() => m,
                _ => return,
            };

            // 1. Primary Modes
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_STATIC_ZOOM, w!("Static Zoom\tCtrl+1"));
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_LIVE_ZOOM, w!("Live Zoom\tCtrl+4"));
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_DRAW, w!("Draw Mode\tCtrl+2"));
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_SPOTLIGHT, w!("Spotlight Mode\tCtrl+3"));
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_SNIP, w!("Snip Selection\tCtrl+Shift+S"));
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_TIMER, w!("Countdown Timer\tCtrl+5"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

            // 2. Presenter Tools Submenu
            if let Ok(tools_menu) = CreatePopupMenu() {
                let _ = AppendMenuW(tools_menu, MF_STRING, ID_TRAY_LASER, w!("Laser Pointer\tK"));
                let _ = AppendMenuW(tools_menu, MF_STRING, ID_TRAY_ERASER, w!("Stroke Eraser\tX"));
                let _ = AppendMenuW(tools_menu, MF_STRING, ID_TRAY_WHITEBOARD, w!("Whiteboard Slate\tW"));
                let _ = AppendMenuW(tools_menu, MF_STRING, ID_TRAY_BLACKBOARD, w!("Blackboard Slate\tShift+K"));
                let _ = AppendMenuW(menu, MF_POPUP, tools_menu.0 as usize, w!("Presenter Tools"));
            }

            // 3. Settings & Help Submenu
            if let Ok(opts_menu) = CreatePopupMenu() {
                let _ = AppendMenuW(opts_menu, MF_STRING, ID_TRAY_CHEATSHEET, w!("Keyboard Shortcuts\tF1"));
                let _ = AppendMenuW(opts_menu, MF_STRING, ID_TRAY_RESET_TOOLBAR, w!("Reset Toolbar Position\tF2"));
                let _ = AppendMenuW(opts_menu, MF_STRING, ID_TRAY_OPEN_CONFIG, w!("Open Settings (config.json)"));
                let _ = AppendMenuW(opts_menu, MF_SEPARATOR, 0, PCWSTR::null());
                let _ = AppendMenuW(opts_menu, MF_STRING, ID_TRAY_ABOUT, w!("About Zoomify"));
                let _ = AppendMenuW(menu, MF_POPUP, opts_menu.0 as usize, w!("Settings & Help"));
            }

            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_EXIT, w!("Exit Zoomify\tAlt+F4"));

            let mut pt = windows::Win32::Foundation::POINT::default();
            let _ = GetCursorPos(&mut pt);

            let _ = SetForegroundWindow(self.hwnd);
            let _ = TrackPopupMenuEx(
                menu,
                (TPM_LEFTALIGN | TPM_RIGHTBUTTON).0,
                pt.x,
                pt.y,
                self.hwnd,
                None,
            );
            let _ = DestroyMenu(menu);
        }
    }

    fn create_app_icon() -> HICON {
        unsafe {
            let width: i32 = 32;
            let height: i32 = 32;

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // top-down DIB
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [RGBQUAD::default()],
            };

            let screen_dc = GetDC(None);
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let hbm_color = CreateDIBSection(
                Some(screen_dc),
                &bmi,
                DIB_RGB_COLORS,
                &mut bits,
                None,
                0,
            );
            ReleaseDC(None, screen_dc);

            let mut mask_bytes = [0xFFu8; (32 * 32 / 8)]; // Start with fully transparent 1-mask

            if let Ok(color_bmp) = hbm_color {
                if !color_bmp.is_invalid() && !bits.is_null() {
                    let pixel_slice = std::slice::from_raw_parts_mut(bits as *mut u32, (width * height) as usize);

                    // Draw 32-bit ARGB Windows 11 Fluent Magnifier Icon with crisp antialiasing
                    let center_x = 12.0f32;
                    let center_y = 12.0f32;
                    let outer_radius = 9.8f32;
                    let inner_radius = 6.6f32;

                    for y in 0..height {
                        for x in 0..width {
                            let fx = x as f32 + 0.5;
                            let fy = y as f32 + 0.5;

                            let dist_lens = ((fx - center_x).powi(2) + (fy - center_y).powi(2)).sqrt();

                            // 45-degree grip handle projection
                            // From (16, 16) down to (27, 27)
                            let h_dx = fx - 16.0;
                            let h_dy = fy - 16.0;
                            let handle_len = (h_dx + h_dy) / std::f32::consts::SQRT_2;
                            let handle_dist = (h_dx - h_dy).abs() / std::f32::consts::SQRT_2;
                            let in_handle = handle_len >= 0.0 && handle_len <= 15.0 && handle_dist <= 2.2;
                            let in_handle_tip = ((fx - 26.5).powi(2) + (fy - 26.5).powi(2)).sqrt() <= 2.2;

                            let (r, g, b, a): (u8, u8, u8, u8) = if in_handle || in_handle_tip {
                                // Fluent Metallic Dark Grip Handle with silver highlight
                                if handle_dist < 0.8 {
                                    (140, 155, 175, 255) // Silver top highlight
                                } else {
                                    (55, 65, 80, 255) // Deep slate metal body
                                }
                            } else if dist_lens >= inner_radius && dist_lens <= outer_radius {
                                // Antialiased Fluent Accent Blue lens ring
                                let edge_fade = if dist_lens > outer_radius - 0.7 {
                                    ((outer_radius - dist_lens) / 0.7).clamp(0.0, 1.0)
                                } else if dist_lens < inner_radius + 0.7 {
                                    ((dist_lens - inner_radius) / 0.7).clamp(0.0, 1.0)
                                } else {
                                    1.0
                                };
                                let alpha = (255.0 * edge_fade) as u8;
                                (0, 120, 212, alpha) // Windows 11 Accent Blue #0078D4
                            } else if dist_lens < inner_radius {
                                // Glass interior
                                let is_specular = fx < 12.0 && fy < 12.0 && dist_lens >= 3.5 && dist_lens <= 5.8;
                                let is_cross_h = (fy - center_y).abs() <= 0.8 && (fx - center_x).abs() <= 3.2;
                                let is_cross_v = (fx - center_x).abs() <= 0.8 && (fy - center_y).abs() <= 3.2;

                                if is_cross_h || is_cross_v {
                                    (255, 255, 255, 240) // Crisp white center plus target
                                } else if is_specular {
                                    (210, 240, 255, 180) // Curved glass reflection shine
                                } else {
                                    (15, 65, 125, 85) // Translucent deep sky glass
                                }
                            } else {
                                (0, 0, 0, 0)
                            };

                            // Premultiply alpha for high-fidelity Windows taskbar compositing
                            let pr = ((r as u32 * a as u32) + 127) / 255;
                            let pg = ((g as u32 * a as u32) + 127) / 255;
                            let pb = ((b as u32 * a as u32) + 127) / 255;
                            pixel_slice[(y * width + x) as usize] = ((a as u32) << 24) | (pr << 16) | (pg << 8) | pb;

                            // Update 1-bpp AND mask bit (0 = opaque, 1 = transparent)
                            if a >= 32 {
                                let byte_idx = (y * 4 + x / 8) as usize;
                                let bit_idx = 7 - (x % 8);
                                mask_bytes[byte_idx] &= !(1 << bit_idx);
                            }
                        }
                    }

                    let hbm_mask = CreateBitmap(32, 32, 1, 1, Some(mask_bytes.as_ptr() as *const _));

                    if !hbm_mask.is_invalid() {
                        let mut icon_info = ICONINFO {
                            fIcon: true.into(),
                            xHotspot: 0,
                            yHotspot: 0,
                            hbmMask: hbm_mask,
                            hbmColor: color_bmp,
                        };

                        let created_icon = CreateIconIndirect(&mut icon_info);
                        let _ = DeleteObject(color_bmp.into());
                        let _ = DeleteObject(hbm_mask.into());

                        if let Ok(icon) = created_icon {
                            if !icon.is_invalid() {
                                return icon;
                            }
                        }
                    } else {
                        let _ = DeleteObject(color_bmp.into());
                    }
                }
            }

            LoadIconW(None, IDI_APPLICATION).unwrap_or_default()
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
            if !self.hicon.is_invalid() {
                let _ = DestroyIcon(self.hicon);
            }
        }
    }
}
