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
    live_zoom_keys_active: bool,
}

impl HotkeyManager {
    pub fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            registered: Vec::new(),
            live_zoom_keys_active: false,
        }
    }

    pub fn register_all(&mut self) -> Vec<&'static str> {
        let cfg = crate::config::AppConfig::load();
        self.register_from_config(&cfg)
    }

    /// Returns the names of any actions whose hotkey could not be claimed -
    /// almost always because another running app already owns that combo.
    /// Silently dropping these leaves a shortcut that simply does nothing.
    pub fn register_from_config(&mut self, cfg: &crate::config::AppConfig) -> Vec<&'static str> {
        let mut failed = Vec::new();
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
            } else {
                failed.push("Static Zoom");
            }

            // 2. Draw Mode
            if register(self.hwnd, HOTKEY_DRAW, &cfg.hotkey_draw) {
                self.registered.push(HOTKEY_DRAW);
            } else {
                failed.push("Draw Mode");
            }

            // 3. Spotlight
            if register(self.hwnd, HOTKEY_SPOTLIGHT, &cfg.hotkey_spotlight) {
                self.registered.push(HOTKEY_SPOTLIGHT);
            } else {
                failed.push("Spotlight");
            }

            // 4. Live Zoom
            if register(self.hwnd, HOTKEY_LIVE_ZOOM, &cfg.hotkey_live_zoom) {
                self.registered.push(HOTKEY_LIVE_ZOOM);
            } else {
                failed.push("Live Zoom");
            }

            // 5. Timer
            if register(self.hwnd, HOTKEY_TIMER, &cfg.hotkey_timer) {
                self.registered.push(HOTKEY_TIMER);
            } else {
                failed.push("Timer");
            }

            // 6. Loupe Magnifier
            if register(self.hwnd, HOTKEY_LOUPE, &cfg.hotkey_loupe) {
                self.registered.push(HOTKEY_LOUPE);
            } else {
                failed.push("Loupe");
            }

            // 7. Settings Window (Ctrl+,)
            if RegisterHotKey(
                Some(self.hwnd),
                HOTKEY_SETTINGS,
                MOD_CONTROL | MOD_NOREPEAT,
                VK_OEM_COMMA.0 as u32,
            )
            .is_ok()
            {
                self.registered.push(HOTKEY_SETTINGS);
            } else {
                failed.push("Settings");
            }
        }
        failed
    }

    /// Zoom-adjust keys (Ctrl+Up/Down/+/-) are only claimed while Live Zoom is
    /// running. Registering them globally would steal browser zoom and
    /// paragraph navigation from every other app for the life of the process.
    pub fn set_live_zoom_hotkeys(&mut self, active: bool) {
        if active == self.live_zoom_keys_active {
            return;
        }
        self.live_zoom_keys_active = active;

        const LIVE_ZOOM_KEYS: [(i32, u16); 4] = [
            (HOTKEY_LIVE_ZOOM_IN, VK_UP.0),
            (HOTKEY_LIVE_ZOOM_OUT, VK_DOWN.0),
            (HOTKEY_LIVE_ZOOM_IN_PLUS, VK_OEM_PLUS.0),
            (HOTKEY_LIVE_ZOOM_OUT_MINUS, VK_OEM_MINUS.0),
        ];

        unsafe {
            for (id, vk) in LIVE_ZOOM_KEYS {
                if active {
                    if RegisterHotKey(Some(self.hwnd), id, MOD_CONTROL | MOD_NOREPEAT, vk as u32)
                        .is_ok()
                    {
                        self.registered.push(id);
                    }
                } else {
                    let _ = UnregisterHotKey(Some(self.hwnd), id);
                    self.registered.retain(|&r| r != id);
                }
            }
        }
    }

    pub fn reload_from_config(&mut self, cfg: &crate::config::AppConfig) -> Vec<&'static str> {
        let live_zoom_was_active = self.live_zoom_keys_active;
        self.unregister_all();
        let failed = self.register_from_config(cfg);
        // Preserve the Live Zoom keys across a settings reload if a session is running.
        self.set_live_zoom_hotkeys(live_zoom_was_active);
        failed
    }

    pub fn unregister_all(&mut self) {
        unsafe {
            for &id in &self.registered {
                let _ = UnregisterHotKey(Some(self.hwnd), id);
            }
        }
        self.registered.clear();
        self.live_zoom_keys_active = false;
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.unregister_all();
    }
}
