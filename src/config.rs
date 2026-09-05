use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::w;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyBinding {
    pub modifiers: u32,
    pub vk_code: u32,
}

impl HotkeyBinding {
    pub const fn new(modifiers: u32, vk_code: u32) -> Self {
        Self { modifiers, vk_code }
    }

    pub fn format_display(&self) -> String {
        let mut parts = Vec::new();
        if (self.modifiers & 0x0002) != 0 {
            // MOD_CONTROL
            parts.push("Ctrl");
        }
        if (self.modifiers & 0x0001) != 0 {
            // MOD_ALT
            parts.push("Alt");
        }
        if (self.modifiers & 0x0004) != 0 {
            // MOD_SHIFT
            parts.push("Shift");
        }
        if (self.modifiers & 0x0008) != 0 {
            // MOD_WIN
            parts.push("Win");
        }

        let key_str = match self.vk_code {
            0x30..=0x39 => ((self.vk_code as u8) as char).to_string(),
            0x41..=0x5A => ((self.vk_code as u8) as char).to_string(),
            0x70..=0x87 => format!("F{}", self.vk_code - 0x70 + 1),
            0x20 => "Space".to_string(),
            0x09 => "Tab".to_string(),
            0x0D => "Enter".to_string(),
            0x1B => "Esc".to_string(),
            0x25 => "Left".to_string(),
            0x26 => "Up".to_string(),
            0x27 => "Right".to_string(),
            0x28 => "Down".to_string(),
            0xBC => ",".to_string(),
            0xBE => ".".to_string(),
            0xBD => "-".to_string(),
            0xBB => "+".to_string(),
            0xBA => ";".to_string(),
            0xBF => "/".to_string(),
            0xC0 => "`".to_string(),
            0xDB => "[".to_string(),
            0xDC => "\\".to_string(),
            0xDD => "]".to_string(),
            0xDE => "'".to_string(),
            _ => format!("0x{:X}", self.vk_code),
        };
        parts.push(&key_str);
        parts.join(" + ")
    }
}

fn default_timer_sound() -> bool {
    true
}

fn default_hk_static_zoom() -> HotkeyBinding {
    HotkeyBinding::new(0x0002, '1' as u32)
}

fn default_hk_draw() -> HotkeyBinding {
    HotkeyBinding::new(0x0002, '2' as u32)
}

fn default_hk_spotlight() -> HotkeyBinding {
    HotkeyBinding::new(0x0002, '3' as u32)
}

fn default_hk_live_zoom() -> HotkeyBinding {
    HotkeyBinding::new(0x0002, '4' as u32)
}

fn default_hk_timer() -> HotkeyBinding {
    HotkeyBinding::new(0x0002, '5' as u32)
}

fn default_hk_loupe() -> HotkeyBinding {
    HotkeyBinding::new(0x0002, '6' as u32)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub default_zoom_level: f32,
    pub spotlight_radius: f32,
    pub default_stroke_width: f32,
    pub default_color: String,
    pub timer_duration_mins: u32,
    pub toolbar_collapsed: bool,
    pub toolbar_custom_position: Option<(f32, f32)>,
    pub monitor_target: String,
    pub allow_monitor_cycling: bool,
    pub default_fill_mode: String,
    pub default_stroke_pattern: String,
    pub default_badge_size: String,
    #[serde(default)]
    pub start_with_windows: bool,
    #[serde(default = "default_timer_sound")]
    pub timer_sound_enabled: bool,
    #[serde(default = "default_hk_static_zoom")]
    pub hotkey_static_zoom: HotkeyBinding,
    #[serde(default = "default_hk_draw")]
    pub hotkey_draw: HotkeyBinding,
    #[serde(default = "default_hk_spotlight")]
    pub hotkey_spotlight: HotkeyBinding,
    #[serde(default = "default_hk_live_zoom")]
    pub hotkey_live_zoom: HotkeyBinding,
    #[serde(default = "default_hk_timer")]
    pub hotkey_timer: HotkeyBinding,
    #[serde(default = "default_hk_loupe")]
    pub hotkey_loupe: HotkeyBinding,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_zoom_level: 2.0,
            spotlight_radius: 180.0,
            default_stroke_width: 4.0,
            default_color: "Red".to_string(),
            timer_duration_mins: 10,
            toolbar_collapsed: false,
            toolbar_custom_position: None,
            monitor_target: "cursor".to_string(),
            allow_monitor_cycling: true,
            default_fill_mode: "None".to_string(),
            default_stroke_pattern: "Solid".to_string(),
            default_badge_size: "Medium".to_string(),
            start_with_windows: false,
            timer_sound_enabled: true,
            hotkey_static_zoom: default_hk_static_zoom(),
            hotkey_draw: default_hk_draw(),
            hotkey_spotlight: default_hk_spotlight(),
            hotkey_live_zoom: default_hk_live_zoom(),
            hotkey_timer: default_hk_timer(),
            hotkey_loupe: default_hk_loupe(),
        }
    }
}

impl AppConfig {
    pub fn config_path() -> Option<PathBuf> {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let mut dir = PathBuf::from(appdata);
            dir.push("Zoomify");
            let _ = fs::create_dir_all(&dir);
            dir.push("config.json");
            Some(dir)
        } else {
            Some(PathBuf::from("zoomify_config.json"))
        }
    }

    pub fn load() -> Self {
        let mut cfg = if let Some(path) = Self::config_path()
            && let Ok(data) = fs::read_to_string(&path)
            && let Ok(loaded) = serde_json::from_str::<Self>(&data)
        {
            loaded
        } else {
            Self::default()
        };

        // Sync start_with_windows from actual registry state
        cfg.start_with_windows = Self::is_registered_for_startup();
        cfg
    }

    pub fn save(&self) {
        let _ = Self::set_startup_registration(self.start_with_windows);
        if let Some(path) = Self::config_path()
            && let Ok(json) = serde_json::to_string_pretty(self)
        {
            let _ = fs::write(&path, json);
        }
    }

    pub fn is_registered_for_startup() -> bool {
        unsafe {
            let mut hkey = HKEY::default();
            let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
            if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, Some(0), KEY_READ, &mut hkey).is_err() {
                return false;
            }
            let val_name = w!("Zoomify");
            let mut buf = [0u8; 1024];
            let mut size = buf.len() as u32;
            let res = RegQueryValueExW(
                hkey,
                val_name,
                None,
                None,
                Some(buf.as_mut_ptr()),
                Some(&mut size),
            );
            let _ = RegCloseKey(hkey);
            res.is_ok()
        }
    }

    pub fn set_startup_registration(enable: bool) -> Result<(), String> {
        unsafe {
            let mut hkey = HKEY::default();
            let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
            RegOpenKeyExW(HKEY_CURRENT_USER, subkey, Some(0), KEY_SET_VALUE, &mut hkey)
                .ok()
                .map_err(|e| format!("Failed to open Run key: {}", e))?;

            let val_name = w!("Zoomify");
            let res = if enable {
                if let Ok(exe_path) = std::env::current_exe() {
                    let exe_str = format!("\"{}\"", exe_path.to_string_lossy());
                    let utf16: Vec<u16> =
                        exe_str.encode_utf16().chain(std::iter::once(0)).collect();
                    let byte_slice = std::slice::from_raw_parts(
                        utf16.as_ptr() as *const u8,
                        utf16.len() * std::mem::size_of::<u16>(),
                    );
                    RegSetValueExW(hkey, val_name, Some(0), REG_SZ, Some(byte_slice))
                        .ok()
                        .map_err(|e| format!("Failed to set Run value: {}", e))
                } else {
                    Err("Failed to determine current exe path".to_string())
                }
            } else {
                let _ = RegDeleteValueW(hkey, val_name);
                Ok(())
            };

            let _ = RegCloseKey(hkey);
            res
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults_and_serialization() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.default_zoom_level, 2.0);
        assert_eq!(cfg.spotlight_radius, 180.0);
        assert_eq!(cfg.default_stroke_width, 4.0);
        assert_eq!(cfg.default_color, "Red");
        assert_eq!(cfg.timer_duration_mins, 10);
        assert!(!cfg.toolbar_collapsed);
        assert_eq!(cfg.toolbar_custom_position, None);
        assert!(!cfg.start_with_windows);
        assert!(cfg.timer_sound_enabled);
        assert_eq!(cfg.hotkey_static_zoom.format_display(), "Ctrl + 1");
        assert_eq!(cfg.hotkey_draw.format_display(), "Ctrl + 2");
        assert_eq!(cfg.hotkey_spotlight.format_display(), "Ctrl + 3");
        assert_eq!(cfg.hotkey_live_zoom.format_display(), "Ctrl + 4");
        assert_eq!(cfg.hotkey_timer.format_display(), "Ctrl + 5");
        assert_eq!(cfg.hotkey_loupe.format_display(), "Ctrl + 6");

        let json = serde_json::to_string(&cfg).expect("serialize");
        let restored: AppConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.default_zoom_level, 2.0);
        assert_eq!(restored.spotlight_radius, 180.0);
        assert_eq!(restored.hotkey_static_zoom, cfg.hotkey_static_zoom);
    }

    #[test]
    fn test_hotkey_binding_format() {
        // Ctrl + Alt + Shift + Z
        let hk = HotkeyBinding::new(0x0002 | 0x0001 | 0x0004, 'Z' as u32);
        assert_eq!(hk.format_display(), "Ctrl + Alt + Shift + Z");

        // Win + F1
        let hk2 = HotkeyBinding::new(0x0008, 0x70);
        assert_eq!(hk2.format_display(), "Win + F1");
    }
}
