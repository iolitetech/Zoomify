#![allow(dead_code)]

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, VK_DOWN, VK_OEM_MINUS,
    VK_OEM_PLUS, VK_UP,
};

pub const HOTKEY_STATIC_ZOOM: i32 = 101;     // Ctrl+1
pub const HOTKEY_DRAW: i32 = 102;            // Ctrl+2
pub const HOTKEY_SPOTLIGHT: i32 = 103;       // Ctrl+3
pub const HOTKEY_LIVE_ZOOM: i32 = 104;       // Ctrl+4
pub const HOTKEY_TIMER: i32 = 105;           // Ctrl+5
pub const HOTKEY_SNIP: i32 = 106;            // Ctrl+Shift+S
pub const HOTKEY_LIVE_ZOOM_IN: i32 = 107;    // Ctrl+Up
pub const HOTKEY_LIVE_ZOOM_OUT: i32 = 108;   // Ctrl+Down
pub const HOTKEY_LIVE_ZOOM_IN_PLUS: i32 = 109;  // Ctrl+= / Ctrl++
pub const HOTKEY_LIVE_ZOOM_OUT_MINUS: i32 = 110; // Ctrl+-

pub struct HotkeyManager {
    hwnd: HWND,
    registered: Vec<i32>,
}

impl HotkeyManager {
    pub fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            registered: Vec::new(),
        }
    }

    pub fn register_all(&mut self) {
        unsafe {
            let ctrl_norepeat = MOD_CONTROL | MOD_NOREPEAT;
            let ctrl_shift_norepeat = MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT;

            // Ctrl+1: Static Zoom
            if RegisterHotKey(Some(self.hwnd), HOTKEY_STATIC_ZOOM, ctrl_norepeat, '1' as u32).is_ok() {
                self.registered.push(HOTKEY_STATIC_ZOOM);
            }

            // Ctrl+2: Draw Mode
            if RegisterHotKey(Some(self.hwnd), HOTKEY_DRAW, ctrl_norepeat, '2' as u32).is_ok() {
                self.registered.push(HOTKEY_DRAW);
            }

            // Ctrl+3: Spotlight
            if RegisterHotKey(Some(self.hwnd), HOTKEY_SPOTLIGHT, ctrl_norepeat, '3' as u32).is_ok() {
                self.registered.push(HOTKEY_SPOTLIGHT);
            }

            // Ctrl+4: Live Zoom
            if RegisterHotKey(Some(self.hwnd), HOTKEY_LIVE_ZOOM, ctrl_norepeat, '4' as u32).is_ok() {
                self.registered.push(HOTKEY_LIVE_ZOOM);
            }

            // Ctrl+5: Timer
            if RegisterHotKey(Some(self.hwnd), HOTKEY_TIMER, ctrl_norepeat, '5' as u32).is_ok() {
                self.registered.push(HOTKEY_TIMER);
            }

            // Ctrl+Shift+S: Snip
            if RegisterHotKey(Some(self.hwnd), HOTKEY_SNIP, ctrl_shift_norepeat, 'S' as u32).is_ok() {
                self.registered.push(HOTKEY_SNIP);
            }

            // Ctrl+Up: Live Zoom In
            if RegisterHotKey(Some(self.hwnd), HOTKEY_LIVE_ZOOM_IN, ctrl_norepeat, VK_UP.0 as u32).is_ok() {
                self.registered.push(HOTKEY_LIVE_ZOOM_IN);
            }

            // Ctrl+Down: Live Zoom Out
            if RegisterHotKey(Some(self.hwnd), HOTKEY_LIVE_ZOOM_OUT, ctrl_norepeat, VK_DOWN.0 as u32).is_ok() {
                self.registered.push(HOTKEY_LIVE_ZOOM_OUT);
            }

            // Ctrl++: Live Zoom In
            if RegisterHotKey(Some(self.hwnd), HOTKEY_LIVE_ZOOM_IN_PLUS, ctrl_norepeat, VK_OEM_PLUS.0 as u32).is_ok() {
                self.registered.push(HOTKEY_LIVE_ZOOM_IN_PLUS);
            }

            // Ctrl+-: Live Zoom Out
            if RegisterHotKey(Some(self.hwnd), HOTKEY_LIVE_ZOOM_OUT_MINUS, ctrl_norepeat, VK_OEM_MINUS.0 as u32).is_ok() {
                self.registered.push(HOTKEY_LIVE_ZOOM_OUT_MINUS);
            }
        }
    }

    pub fn unregister_all(&mut self) {
        unsafe {
            for &id in &self.registered {
                let _ = UnregisterHotKey(Some(self.hwnd), id);
            }
        }
        self.registered.clear();
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.unregister_all();
    }
}
