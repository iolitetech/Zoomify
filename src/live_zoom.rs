#![allow(dead_code)]

use windows::core::{s, BOOL};
use windows::Win32::Foundation::{HMODULE, POINT};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

type MagInitFn = unsafe extern "system" fn() -> BOOL;
type MagUninitFn = unsafe extern "system" fn() -> BOOL;
type MagSetFullscreenTransformFn = unsafe extern "system" fn(f32, i32, i32) -> BOOL;
type MagGetFullscreenTransformFn = unsafe extern "system" fn(*mut f32, *mut i32, *mut i32) -> BOOL;
type MagShowSystemCursorFn = unsafe extern "system" fn(BOOL) -> BOOL;

pub struct LiveZoomEngine {
    module: Option<HMODULE>,
    mag_init: Option<MagInitFn>,
    mag_uninit: Option<MagUninitFn>,
    mag_set_transform: Option<MagSetFullscreenTransformFn>,
    mag_get_transform: Option<MagGetFullscreenTransformFn>,
    mag_show_cursor: Option<MagShowSystemCursorFn>,
    is_active: bool,
    zoom_level: f32,
    current_x_offset: f32,
    current_y_offset: f32,
    target_x_offset: f32,
    target_y_offset: f32,
}

impl LiveZoomEngine {
    pub fn new() -> Self {
        let mut engine = Self {
            module: None,
            mag_init: None,
            mag_uninit: None,
            mag_set_transform: None,
            mag_get_transform: None,
            mag_show_cursor: None,
            is_active: false,
            zoom_level: 2.0,
            current_x_offset: 0.0,
            current_y_offset: 0.0,
            target_x_offset: 0.0,
            target_y_offset: 0.0,
        };

        engine.load_dll();
        engine
    }

    fn load_dll(&mut self) {
        unsafe {
            if let Ok(hmodule) = LoadLibraryA(s!("magnification.dll")) {
                if !hmodule.is_invalid() {
                    self.module = Some(hmodule);

                    if let Some(f) = GetProcAddress(hmodule, s!("MagInitialize")) {
                        self.mag_init = Some(std::mem::transmute(f));
                    }
                    if let Some(f) = GetProcAddress(hmodule, s!("MagUninitialize")) {
                        self.mag_uninit = Some(std::mem::transmute(f));
                    }
                    if let Some(f) = GetProcAddress(hmodule, s!("MagSetFullscreenTransform")) {
                        self.mag_set_transform = Some(std::mem::transmute(f));
                    }
                    if let Some(f) = GetProcAddress(hmodule, s!("MagGetFullscreenTransform")) {
                        self.mag_get_transform = Some(std::mem::transmute(f));
                    }
                    if let Some(f) = GetProcAddress(hmodule, s!("MagShowSystemCursor")) {
                        self.mag_show_cursor = Some(std::mem::transmute(f));
                    }
                }
            }
        }
    }

    pub fn is_supported(&self) -> bool {
        self.mag_init.is_some() && self.mag_set_transform.is_some()
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }

    pub fn zoom_level(&self) -> f32 {
        self.zoom_level
    }

    pub fn start(&mut self, initial_level: f32) -> bool {
        if !self.is_supported() {
            return false;
        }

        if self.is_active {
            return true;
        }

        unsafe {
            if let Some(init_fn) = self.mag_init {
                if init_fn().as_bool() {
                    self.is_active = true;
                    self.zoom_level = initial_level.clamp(1.25, 10.0);

                    self.update_target_from_cursor();
                    self.current_x_offset = self.target_x_offset;
                    self.current_y_offset = self.target_y_offset;

                    if let Some(set_fn) = self.mag_set_transform {
                        let _ = set_fn(
                            self.zoom_level,
                            self.current_x_offset.round() as i32,
                            self.current_y_offset.round() as i32,
                        );
                    }
                    return true;
                }
            }
        }
        false
    }

    pub fn stop(&mut self) {
        if !self.is_active {
            return;
        }

        unsafe {
            if let Some(set_fn) = self.mag_set_transform {
                let _ = set_fn(1.0, 0, 0);
            }
            if let Some(uninit_fn) = self.mag_uninit {
                let _ = uninit_fn();
            }
        }
        self.is_active = false;
        self.current_x_offset = 0.0;
        self.current_y_offset = 0.0;
    }

    pub fn set_zoom_level(&mut self, level: f32) {
        if !self.is_active {
            return;
        }
        self.zoom_level = level.clamp(1.25, 10.0);
        self.update_target_from_cursor();
        self.apply_transform();
    }

    pub fn adjust_zoom(&mut self, delta: f32) {
        let new_level = (self.zoom_level + delta).clamp(1.25, 10.0);
        self.set_zoom_level(new_level);
    }

    pub fn update_target_from_cursor(&mut self) {
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                let screen_w = GetSystemMetrics(SM_CXSCREEN) as f32;
                let screen_h = GetSystemMetrics(SM_CYSCREEN) as f32;

                let view_w = screen_w / self.zoom_level;
                let view_h = screen_h / self.zoom_level;

                let target_x = (pt.x as f32) - (view_w / 2.0);
                let target_y = (pt.y as f32) - (view_h / 2.0);

                let max_x = (screen_w - view_w).max(0.0);
                let max_y = (screen_h - view_h).max(0.0);

                self.target_x_offset = target_x.clamp(0.0, max_x);
                self.target_y_offset = target_y.clamp(0.0, max_y);
            }
        }
    }

    pub fn tick_smooth_pan(&mut self, lerp_factor: f32) {
        if !self.is_active {
            return;
        }

        self.update_target_from_cursor();

        let factor = lerp_factor.clamp(0.05, 1.0);
        self.current_x_offset += (self.target_x_offset - self.current_x_offset) * factor;
        self.current_y_offset += (self.target_y_offset - self.current_y_offset) * factor;

        self.apply_transform();
    }

    fn apply_transform(&self) {
        if self.is_active {
            if let Some(set_fn) = self.mag_set_transform {
                unsafe {
                    let _ = set_fn(
                        self.zoom_level,
                        self.current_x_offset.round() as i32,
                        self.current_y_offset.round() as i32,
                    );
                }
            }
        }
    }
}

impl Drop for LiveZoomEngine {
    fn drop(&mut self) {
        self.stop();
    }
}
