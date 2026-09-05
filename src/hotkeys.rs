#![allow(dead_code)]

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_DOWN, VK_OEM_COMMA,
    VK_OEM_MINUS, VK_OEM_PLUS, VK_UP,
};

pub const HOTKEY_STATIC_ZOOM: i32 = 101; // Ctrl+1
pub const HOTKEY_DRAW: i32 = 102; // Ctrl+2
pub const HOTKEY_SPOTLIGHT: i32 = 103; // Ctrl+3
pub const HOTKEY_LIVE_ZOOM: i32 = 104; // Ctrl+4
pub const HOTKEY_TIMER: i32 = 105; // Ctrl+5
pub const HOTKEY_LOUPE: i32 = 106; // Ctrl+6
pub const HOTKEY_LIVE_ZOOM_IN: i32 = 107; // Ctrl+Up
pub const HOTKEY_LIVE_ZOOM_OUT: i32 = 108; // Ctrl+Down
pub const HOTKEY_LIVE_ZOOM_IN_PLUS: i32 = 109; // Ctrl+= / Ctrl++
pub const HOTKEY_LIVE_ZOOM_OUT_MINUS: i32 = 110; // Ctrl+-
pub const HOTKEY_SETTINGS: i32 = 111; // Ctrl+,

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
        let cfg = crate::config::AppConfig::load();
        self.register_from_config(&cfg);
    }

    pub fn register_from_config(&mut self, cfg: &crate::config::AppConfig) {
        unsafe {
            let register = |hwnd: HWND, id: i32, hk: &crate::config::HotkeyBinding| -> bool {
                let flags = windows::Win32::UI::Input::KeyboardAndMouse::HOT_KEY_MODIFIERS(
                    hk.modifiers | MOD_NOREPEAT.0,
                );
                RegisterHotKey(Some(hwnd), id, flags, hk.vk_code).is_ok()
            };

            // 1. Static Zoom
            if register(self.hwnd, HOTKEY_STATIC_ZOOM, &cfg.hotkey_static_zoom) {
                self.registered.push(HOTKEY_STATIC_ZOOM);
            }

            // 2. Draw Mode
            if register(self.hwnd, HOTKEY_DRAW, &cfg.hotkey_draw) {
                self.registered.push(HOTKEY_DRAW);
            }

            // 3. Spotlight
            if register(self.hwnd, HOTKEY_SPOTLIGHT, &cfg.hotkey_spotlight) {
                self.registered.push(HOTKEY_SPOTLIGHT);
            }

            // 4. Live Zoom
            if register(self.hwnd, HOTKEY_LIVE_ZOOM, &cfg.hotkey_live_zoom) {
                self.registered.push(HOTKEY_LIVE_ZOOM);
            }

            // 5. Timer
            if register(self.hwnd, HOTKEY_TIMER, &cfg.hotkey_timer) {
                self.registered.push(HOTKEY_TIMER);
            }

            // 6. Loupe Magnifier
            if register(self.hwnd, HOTKEY_LOUPE, &cfg.hotkey_loupe) {
                self.registered.push(HOTKEY_LOUPE);
            }

            // Live Zoom secondary keys
            let ctrl_norepeat = MOD_CONTROL | MOD_NOREPEAT;
            if RegisterHotKey(
                Some(self.hwnd),
                HOTKEY_LIVE_ZOOM_IN,
                ctrl_norepeat,
                VK_UP.0 as u32,
            )
            .is_ok()
            {
                self.registered.push(HOTKEY_LIVE_ZOOM_IN);
            }

            if RegisterHotKey(
                Some(self.hwnd),
                HOTKEY_LIVE_ZOOM_OUT,
                ctrl_norepeat,
                VK_DOWN.0 as u32,
            )
            .is_ok()
            {
                self.registered.push(HOTKEY_LIVE_ZOOM_OUT);
            }

            if RegisterHotKey(
                Some(self.hwnd),
                HOTKEY_LIVE_ZOOM_IN_PLUS,
                ctrl_norepeat,
                VK_OEM_PLUS.0 as u32,
            )
            .is_ok()
            {
                self.registered.push(HOTKEY_LIVE_ZOOM_IN_PLUS);
            }

            if RegisterHotKey(
                Some(self.hwnd),
                HOTKEY_LIVE_ZOOM_OUT_MINUS,
                ctrl_norepeat,
                VK_OEM_MINUS.0 as u32,
            )
            .is_ok()
            {
                self.registered.push(HOTKEY_LIVE_ZOOM_OUT_MINUS);
            }

            // 7. Settings Window (Ctrl+,)
            if RegisterHotKey(
                Some(self.hwnd),
                HOTKEY_SETTINGS,
                ctrl_norepeat,
                VK_OEM_COMMA.0 as u32,
            )
            .is_ok()
            {
                self.registered.push(HOTKEY_SETTINGS);
            }
        }
    }

    pub fn reload_from_config(&mut self, cfg: &crate::config::AppConfig) {
        self.unregister_all();
        self.register_from_config(cfg);
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
