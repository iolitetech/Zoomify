use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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
        if let Some(path) = Self::config_path()
            && let Ok(data) = fs::read_to_string(&path)
            && let Ok(cfg) = serde_json::from_str::<Self>(&data)
        {
            return cfg;
        }
        Self::default()
    }

    pub fn save(&self) {
        if let Some(path) = Self::config_path()
            && let Ok(json) = serde_json::to_string_pretty(self)
        {
            let _ = fs::write(&path, json);
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

        let json = serde_json::to_string(&cfg).expect("serialize");
        let restored: AppConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.default_zoom_level, 2.0);
        assert_eq!(restored.spotlight_radius, 180.0);
    }
}
