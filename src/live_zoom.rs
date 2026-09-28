#![allow(dead_code)]

use windows::Win32::Foundation::{FreeLibrary, HMODULE, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetCursorPos, GetSystemMetrics, HHOOK, MSLLHOOKSTRUCT, SM_CXSCREEN,
    SM_CYSCREEN, SetWindowsHookExW, UnhookWindowsHookEx, WH_MOUSE_LL, WM_MOUSEWHEEL,
};
use windows::core::{BOOL, s};

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};

/// Zoom change per wheel notch.
const ZOOM_STEP: f32 = 0.25;

/// Installed hook handle. Only ever touched from the thread that owns the
/// engine, but kept atomic so no `static mut` reference is ever formed.
static LIVE_ZOOM_HOOK: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Whether a Live Zoom session is running, published for the hook to read.
static LIVE_ZOOM_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Zoom change requested by the hook but not yet applied, in thousandths of a
/// zoom step.
///
/// A low-level mouse hook runs on the thread that installed it, re-entering
/// while that thread may already hold a `&mut` to the engine (or to the overlay
/// that owns it). The hook therefore only *posts* an integer here; the owning
/// thread drains it from its normal tick, where holding `&mut self` is sound.
/// Reaching into the engine through a raw pointer, as this used to, aliased a
/// mutable reference.
static PENDING_ZOOM_MILLI: AtomicI32 = AtomicI32::new(0);

unsafe extern "system" fn live_zoom_mouse_hook(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if code >= 0 && wparam.0 as u32 == WM_MOUSEWHEEL && LIVE_ZOOM_ACTIVE.load(Ordering::Acquire)
        {
            let is_ctrl = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
            if is_ctrl {
                let hook_struct = *(lparam.0 as *const MSLLHOOKSTRUCT);
                let notches = ((hook_struct.mouseData >> 16) as i16 as f32) / 120.0;
                let milli = (notches * ZOOM_STEP * 1000.0).round() as i32;
                PENDING_ZOOM_MILLI.fetch_add(milli, Ordering::AcqRel);
                // Swallow the wheel so the app underneath does not also zoom.
                return LRESULT(1);
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }
}

type MagInitFn = unsafe extern "system" fn() -> BOOL;
type MagUninitFn = unsafe extern "system" fn() -> BOOL;
type MagSetFullscreenTransformFn = unsafe extern "system" fn(f32, i32, i32) -> BOOL;
type MagGetFullscreenTransformFn = unsafe extern "system" fn(*mut f32, *mut i32, *mut i32) -> BOOL;
type MagShowSystemCursorFn = unsafe extern "system" fn(BOOL) -> BOOL;
type FarProc = unsafe extern "system" fn() -> isize;

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
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub monitor_w: u32,
    pub monitor_h: u32,
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
            monitor_x: 0,
            monitor_y: 0,
            monitor_w: 0,
            monitor_h: 0,
        };

        engine.load_dll();
        engine
    }

    pub fn set_monitor_bounds(&mut self, x: i32, y: i32, w: u32, h: u32) {
        self.monitor_x = x;
        self.monitor_y = y;
        self.monitor_w = w;
        self.monitor_h = h;
    }

    fn load_dll(&mut self) {
        unsafe {
            if let Ok(hmodule) = LoadLibraryA(s!("magnification.dll"))
                && !hmodule.is_invalid()
            {
                self.module = Some(hmodule);

                if let Some(f) = GetProcAddress(hmodule, s!("MagInitialize")) {
                    self.mag_init = Some(std::mem::transmute::<FarProc, MagInitFn>(f));
                }
                if let Some(f) = GetProcAddress(hmodule, s!("MagUninitialize")) {
                    self.mag_uninit = Some(std::mem::transmute::<FarProc, MagUninitFn>(f));
                }
                if let Some(f) = GetProcAddress(hmodule, s!("MagSetFullscreenTransform")) {
                    self.mag_set_transform = Some(std::mem::transmute::<
                        FarProc,
                        MagSetFullscreenTransformFn,
                    >(f));
                }
                if let Some(f) = GetProcAddress(hmodule, s!("MagGetFullscreenTransform")) {
                    self.mag_get_transform = Some(std::mem::transmute::<
                        FarProc,
                        MagGetFullscreenTransformFn,
                    >(f));
                }
                if let Some(f) = GetProcAddress(hmodule, s!("MagShowSystemCursor")) {
                    self.mag_show_cursor =
                        Some(std::mem::transmute::<FarProc, MagShowSystemCursorFn>(f));
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

    /// Current pan offset, relative to the active monitor's top-left corner
    /// (i.e. in the same coordinate space as `monitor_x`/`monitor_y`, not the
    /// virtual desktop `MagSetFullscreenTransform` itself takes).
    pub fn offsets(&self) -> (f32, f32) {
        (self.current_x_offset, self.current_y_offset)
    }

    pub fn start(&mut self, initial_level: f32) -> bool {
        if !self.is_supported() {
            return false;
        }

        if self.is_active {
            return true;
        }

        unsafe {
            if let Some(init_fn) = self.mag_init
                && init_fn().as_bool()
            {
                self.is_active = true;
                self.zoom_level = initial_level.clamp(1.25, 10.0);

                self.update_target_from_cursor();
                self.current_x_offset = self.target_x_offset;
                self.current_y_offset = self.target_y_offset;
                self.apply_transform();

                // Install the low-level wheel hook for Ctrl+Wheel zooming. The
                // hook only posts into PENDING_ZOOM_MILLI; it never touches self.
                PENDING_ZOOM_MILLI.store(0, Ordering::Release);
                LIVE_ZOOM_ACTIVE.store(true, Ordering::Release);
                if LIVE_ZOOM_HOOK.load(Ordering::Acquire).is_null()
                    && let Ok(hook) =
                        SetWindowsHookExW(WH_MOUSE_LL, Some(live_zoom_mouse_hook), None, 0)
                {
                    LIVE_ZOOM_HOOK.store(hook.0, Ordering::Release);
                }

                return true;
            }
        }
        false
    }

    pub fn stop(&mut self) {
        if !self.is_active {
            return;
        }

        // Stop the hook acting before tearing anything down.
        LIVE_ZOOM_ACTIVE.store(false, Ordering::Release);
        PENDING_ZOOM_MILLI.store(0, Ordering::Release);

        unsafe {
            let hook = LIVE_ZOOM_HOOK.swap(std::ptr::null_mut(), Ordering::AcqRel);
            if !hook.is_null() {
                let _ = UnhookWindowsHookEx(HHOOK(hook));
            }

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

        // update_target_from_cursor() only recomputed target_*_offset;
        // tick_smooth_pan() eases current_*_offset toward it over several
        // ticks. Applying the transform right now with the *old* zoom's
        // current_*_offset - which can be out of range for the *new* zoom,
        // since the valid range shrinks when zooming out (e.g. 4x -> 2x) -
        // briefly showed a view past the monitor's edge until smoothing
        // caught up a few ticks later. Clamp current_*_offset into the new
        // zoom's valid range first so every frame stays in bounds.
        let mon_w = if self.monitor_w > 0 {
            self.monitor_w as f32
        } else {
            unsafe { GetSystemMetrics(SM_CXSCREEN) as f32 }
        };
        let mon_h = if self.monitor_h > 0 {
            self.monitor_h as f32
        } else {
            unsafe { GetSystemMetrics(SM_CYSCREEN) as f32 }
        };
        let max_x = (mon_w - mon_w / self.zoom_level).max(0.0);
        let max_y = (mon_h - mon_h / self.zoom_level).max(0.0);
        self.current_x_offset = self.current_x_offset.clamp(0.0, max_x);
        self.current_y_offset = self.current_y_offset.clamp(0.0, max_y);

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
                let mon_x = self.monitor_x as f32;
                let mon_y = self.monitor_y as f32;
                let mon_w = if self.monitor_w > 0 {
                    self.monitor_w as f32
                } else {
                    GetSystemMetrics(SM_CXSCREEN) as f32
                };
                let mon_h = if self.monitor_h > 0 {
                    self.monitor_h as f32
                } else {
                    GetSystemMetrics(SM_CYSCREEN) as f32
                };

                let view_w = mon_w / self.zoom_level;
                let view_h = mon_h / self.zoom_level;

                let cur_rel_x = (pt.x as f32 - mon_x).clamp(0.0, mon_w);
                let cur_rel_y = (pt.y as f32 - mon_y).clamp(0.0, mon_h);

                let target_x = cur_rel_x - (view_w / 2.0);
                let target_y = cur_rel_y - (view_h / 2.0);

                let max_x = (mon_w - view_w).max(0.0);
                let max_y = (mon_h - view_h).max(0.0);

                // Relative to this monitor's own top-left, not the virtual
                // desktop - apply_transform() does that conversion in one
                // place. Keeping it monitor-relative here means a monitor
                // placed left of or above the primary (mon_x/mon_y negative)
                // cannot silently leak into these bounds.
                self.target_x_offset = target_x.clamp(0.0, max_x);
                self.target_y_offset = target_y.clamp(0.0, max_y);
            }
        }
    }

    /// Apply any zoom the wheel hook posted since the last tick. Runs on the
    /// owning thread, so mutating the engine here is sound.
    fn drain_pending_zoom(&mut self) {
        let milli = PENDING_ZOOM_MILLI.swap(0, Ordering::AcqRel);
        if milli != 0 {
            self.adjust_zoom(milli as f32 / 1000.0);
        }
    }

    pub fn tick_smooth_pan(&mut self, lerp_factor: f32) {
        if !self.is_active {
            return;
        }

        self.drain_pending_zoom();
        self.update_target_from_cursor();

        let dx = self.target_x_offset - self.current_x_offset;
        let dy = self.target_y_offset - self.current_y_offset;

        // Ease toward the cursor instead of snapping to it. This previously
        // assigned target straight to current and ignored `lerp_factor`
        // entirely, which is what made the magnified view judder.
        if dx.abs() > 0.5 || dy.abs() > 0.5 {
            let f = lerp_factor.clamp(0.05, 1.0);
            self.current_x_offset += dx * f;
            self.current_y_offset += dy * f;
            self.apply_transform();
        } else if dx != 0.0 || dy != 0.0 {
            // Settle exactly so we stop issuing transforms once we arrive.
            self.current_x_offset = self.target_x_offset;
            self.current_y_offset = self.target_y_offset;
            self.apply_transform();
        }
    }

    fn apply_transform(&self) {
        if self.is_active
            && let Some(set_fn) = self.mag_set_transform
        {
            let z = self.zoom_level;
            let mx = self.monitor_x as f32;
            let my = self.monitor_y as f32;
            // MagSetFullscreenTransform maps a screen point P - relative to
            // the *primary* monitor's top-left, per the API docs - to source
            // pixel `offset + P/z`. For this monitor's own top-left (P = mx,
            // my) to show source (mx + current_x_offset, my +
            // current_y_offset), the offset must be `mx + current_x_offset -
            // mx/z` (and the same for y). The missing `- mx/z` term used to
            // shift the whole magnified view by `|mx|/z` on any monitor
            // placed left of or above the primary (mx or my negative),
            // panning it off the left/top edge of the desktop - invisible on
            // the primary monitor itself, where mx == my == 0.
            let off_x = mx + self.current_x_offset - mx / z;
            let off_y = my + self.current_y_offset - my / z;
            unsafe {
                let _ = set_fn(z, off_x.round() as i32, off_y.round() as i32);
            }
        }
    }
}

impl Drop for LiveZoomEngine {
    fn drop(&mut self) {
        self.stop();
        if let Some(h) = self.module {
            unsafe {
                let _ = FreeLibrary(h);
            }
        }
    }
}
