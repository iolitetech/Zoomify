#![allow(dead_code)]

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush, DeleteDC,
    DeleteObject, GetDC, ReleaseDC, SelectObject,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos,
    LoadIconW, SetForegroundWindow, TrackPopupMenuEx, HICON, ICONINFO, IDI_APPLICATION,
    MF_SEPARATOR, MF_STRING, TPM_LEFTALIGN, TPM_RIGHTBUTTON,
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

pub struct TrayIcon {
    hwnd: HWND,
    hicon: HICON,
    nid: NOTIFYICONDATAW,
}

impl TrayIcon {
    pub fn new(hwnd: HWND) -> Self {
        let hicon = Self::create_app_icon();

        let mut tip_chars = [0u16; 128];
        let tip_str = "Zoomify — Screen Zoom, Spotlight & Annotation (Sysinternals-Class)";
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

            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_STATIC_ZOOM, w!("🔎  Static Zoom\tCtrl+1"));
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_DRAW, w!("✏️  Draw Mode\tCtrl+2"));
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_SPOTLIGHT, w!("🔦  Spotlight Mode\tCtrl+3"));
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_LIVE_ZOOM, w!("🔍  Live Zoom\tCtrl+4"));
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_SNIP, w!("✂️  Snip Selection\tCtrl+Shift+S"));
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_TIMER, w!("⏱️  Countdown Timer\tCtrl+5"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_CHEATSHEET, w!("⌨️  Keybindings Cheat Sheet\tF1"));
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_ABOUT, w!("ℹ️  About Zoomify"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_EXIT, w!("❌  Exit Zoomify"));

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
            let screen_dc = GetDC(None);
            let mem_dc = CreateCompatibleDC(Some(screen_dc));

            let width = 32;
            let height = 32;

            let hbm_color = CreateCompatibleBitmap(screen_dc, width, height);
            let mask_bytes = [0xFFu8; (32 * 32 / 8)]; // All 1s = transparent background initially
            let hbm_mask = CreateBitmap(32, 32, 1, 1, Some(mask_bytes.as_ptr() as *const _));

            if !hbm_color.is_invalid() && !hbm_mask.is_invalid() {
                let old_bmp = SelectObject(mem_dc, hbm_color.into());

                let blue_brush = CreateSolidBrush(COLORREF(0x00D06020)); // BGR: vibrant cyan-blue
                let rect = windows::Win32::Foundation::RECT {
                    left: 2,
                    top: 2,
                    right: 30,
                    bottom: 30,
                };
                let _ = windows::Win32::Graphics::Gdi::FillRect(mem_dc, &rect, blue_brush);
                let _ = DeleteObject(blue_brush.into());

                let white_brush = CreateSolidBrush(COLORREF(0x00FFFFFF));
                let inner_rect = windows::Win32::Foundation::RECT {
                    left: 7,
                    top: 7,
                    right: 25,
                    bottom: 25,
                };
                let _ = windows::Win32::Graphics::Gdi::FillRect(mem_dc, &inner_rect, white_brush);
                let _ = DeleteObject(white_brush.into());

                let _ = SelectObject(mem_dc, old_bmp);
                let _ = DeleteDC(mem_dc);
                let _ = ReleaseDC(None, screen_dc);

                let mut icon_info = ICONINFO {
                    fIcon: true.into(),
                    xHotspot: 0,
                    yHotspot: 0,
                    hbmMask: hbm_mask,
                    hbmColor: hbm_color,
                };

                let created_icon = CreateIconIndirect(&mut icon_info);
                let _ = DeleteObject(hbm_color.into());
                let _ = DeleteObject(hbm_mask.into());

                if let Ok(icon) = created_icon {
                    if !icon.is_invalid() {
                        return icon;
                    }
                }
            } else {
                let _ = DeleteDC(mem_dc);
                let _ = ReleaseDC(None, screen_dc);
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
