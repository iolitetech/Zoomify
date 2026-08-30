#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use windows::core::{w, PCWSTR, Result};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_ESCAPE, VK_F1, VK_MENU,
    VK_RETURN, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, GetSystemMetrics,
    GetWindowLongPtrW, LoadCursorW, RegisterClassExW, SetCursor, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_USERDATA, HWND_TOPMOST, IDC_CROSS,
    IDC_IBEAM, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WM_CHAR, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_TIMER, WNDCLASSEXW, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::capture::ScreenCapture;
use crate::clipboard::{copy_bgra_to_clipboard, get_clipboard_text};
use crate::live_zoom::LiveZoomEngine;
use crate::renderer::D2DRenderer;
use crate::shapes::{snap_to_angle, snap_to_square};
use crate::types::{
    AppMode, CanvasBackground, ColorPreset, DrawTool, HistoryAction, Point2D, Shape, SnipSelection,
    SpotlightState, ToastNotification, ZoomState,
};

const OVERLAY_CLASS_NAME: PCWSTR = w!("ZoomifyFullscreenOverlay");
const TIMER_ID_ANIMATION: usize = 1001;

pub struct OverlayWindow {
    pub hwnd: HWND,
    pub renderer: D2DRenderer,
    pub mode: AppMode,
    pub background_capture: Option<ScreenCapture>,
    pub background_bitmap: Option<ID2D1Bitmap>,
    pub background_type: CanvasBackground,
    pub zoom: ZoomState,
    pub spotlight: SpotlightState,
    pub snip: SnipSelection,
    pub shapes: Vec<Shape>,
    pub undo_history: Vec<HistoryAction>,
    pub redo_history: Vec<HistoryAction>,
    pub active_shape: Option<Shape>,
    pub current_tool: DrawTool,
    pub current_color: ColorPreset,
    pub stroke_width: f32,
    pub live_zoom: LiveZoomEngine,
    pub is_drawing: bool,
    pub draw_start_pt: Point2D,
    pub step_counter: u32,
    pub text_editor: Option<(Point2D, String, ColorPreset, f32)>,
    pub toast: Option<ToastNotification>,
    pub show_cheat_sheet: bool,
    pub timer_seconds: u32,
    pub timer_remaining: f64,
    pub timer_paused: bool,
    pub timer_last_tick: Instant,
    pub screen_x: i32,
    pub screen_y: i32,
    pub screen_width: u32,
    pub screen_height: u32,
}

impl OverlayWindow {
    pub fn create() -> Result<Rc<RefCell<Self>>> {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let hinstance: HINSTANCE = GetModuleHandleW(None)?.into();

            let wnd_class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(Self::wnd_proc),
                hInstance: hinstance,
                lpszClassName: OVERLAY_CLASS_NAME,
                hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
                ..Default::default()
            };

            RegisterClassExW(&wnd_class);

            let screen_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let screen_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let screen_width = GetSystemMetrics(SM_CXVIRTUALSCREEN) as u32;
            let screen_height = GetSystemMetrics(SM_CYVIRTUALSCREEN) as u32;

            let ex_style = WS_EX_TOPMOST | WS_EX_TOOLWINDOW;

            let hwnd = CreateWindowExW(
                ex_style,
                OVERLAY_CLASS_NAME,
                w!("Zoomify Overlay"),
                WS_POPUP,
                screen_x,
                screen_y,
                screen_width as i32,
                screen_height as i32,
                None,
                None,
                Some(hinstance),
                None,
            )?;

            let mut renderer = D2DRenderer::new()?;
            renderer.init_hwnd(hwnd, screen_width, screen_height)?;

            let state = Rc::new(RefCell::new(Self {
                hwnd,
                renderer,
                mode: AppMode::Idle,
                background_capture: None,
                background_bitmap: None,
                background_type: CanvasBackground::Transparent,
                zoom: ZoomState::default(),
                spotlight: SpotlightState::default(),
                snip: SnipSelection {
                    active: false,
                    start: Point2D::default(),
                    current: Point2D::default(),
                },
                shapes: Vec::new(),
                undo_history: Vec::new(),
                redo_history: Vec::new(),
                active_shape: None,
                current_tool: DrawTool::Pen,
                current_color: ColorPreset::Red,
                stroke_width: 4.0,
                live_zoom: LiveZoomEngine::new(),
                is_drawing: false,
                draw_start_pt: Point2D::default(),
                step_counter: 1,
                text_editor: None,
                toast: None,
                show_cheat_sheet: false,
                timer_seconds: 600,
                timer_remaining: 600.0,
                timer_paused: false,
                timer_last_tick: Instant::now(),
                screen_x,
                screen_y,
                screen_width,
                screen_height,
            }));

            let raw_ptr = Rc::into_raw(Rc::clone(&state));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw_ptr as isize);

            windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(hwnd), TIMER_ID_ANIMATION, 16, None);

            Ok(state)
        }
    }

    pub fn enter_live_zoom(&mut self) {
        self.mode = AppMode::LiveZoom;
        self.live_zoom.start(2.0);
        self.set_toast("🔍", "Live Zoom (Wheel: Zoom | Ctrl+3: Spotlight | Esc: Exit)");
    }

    pub fn enter_static_zoom(&mut self) {
        if self.live_zoom.is_active() {
            self.live_zoom.stop();
        }
        self.capture_current_screen();
        let mut cursor_screen = Point2D::new(self.screen_width as f32 / 2.0, self.screen_height as f32 / 2.0);
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                cursor_screen = Point2D::new((pt.x - self.screen_x) as f32, (pt.y - self.screen_y) as f32);
            }
        }
        self.zoom.set_zoom_centered(2.0, cursor_screen, self.screen_width as f32, self.screen_height as f32);
        self.mode = AppMode::StaticZoom;
        self.show_window();
        self.set_toast("🔎", "Static Zoom (Wheel: Zoom | Move: Pan | Click: Draw)");
    }

    pub fn enter_draw_mode(&mut self) {
        if self.live_zoom.is_active() {
            self.live_zoom.stop();
        }
        if self.mode == AppMode::Idle {
            self.capture_current_screen();
            self.zoom = ZoomState::default();
        }
        self.mode = AppMode::Draw;
        self.show_window();
        self.set_toast("✏️", "Draw Mode (P/H/L/A/R/U/E/T/N • r/g/b/y/o/p/c • Shift: Snip)");
    }

    pub fn enter_spotlight_mode(&mut self) {
        if self.live_zoom.is_active() {
            self.live_zoom.stop();
        }
        if self.mode == AppMode::Idle {
            self.capture_current_screen();
            self.zoom = ZoomState::default();
        }
        self.mode = AppMode::Spotlight;
        self.spotlight.active = true;
        self.spotlight.pinned = false;

        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                let screen_pt = Point2D::new((pt.x - self.screen_x) as f32, (pt.y - self.screen_y) as f32);
                let canvas_pt = self.zoom.screen_to_canvas(screen_pt);
                self.spotlight.x = canvas_pt.x;
                self.spotlight.y = canvas_pt.y;
            }
        }
        self.show_window();
        self.set_toast("🔦", "Spotlight Active (Wheel: Resize | Space: Pin)");
    }

    pub fn enter_timer_mode(&mut self, minutes: u32) {
        if self.live_zoom.is_active() {
            self.live_zoom.stop();
        }
        self.mode = AppMode::Timer;
        self.timer_seconds = minutes * 60;
        self.timer_remaining = self.timer_seconds as f64;
        self.timer_paused = false;
        self.timer_last_tick = Instant::now();
        self.show_window();
    }

    pub fn enter_snip_mode(&mut self) {
        if self.live_zoom.is_active() {
            self.live_zoom.stop();
        }
        self.mode = AppMode::Snip;
        self.capture_current_screen();
        self.current_tool = DrawTool::Snip;
        self.show_window();
        self.set_toast("✂️", "Snip Selection (Drag box to copy to Clipboard)");
    }

    pub fn toggle_spotlight(&mut self) {
        self.spotlight.active = !self.spotlight.active;
        if self.spotlight.active {
            unsafe {
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    self.spotlight.x = (pt.x - self.screen_x) as f32;
                    self.spotlight.y = (pt.y - self.screen_y) as f32;
                }
            }
            self.set_toast("🔦", "Spotlight Enabled (Wheel: Resize | Space: Pin)");
        } else {
            self.set_toast("🔦", "Spotlight Disabled");
        }
        self.request_repaint();
    }

    pub fn exit_overlay(&mut self) {
        if self.live_zoom.is_active() {
            self.live_zoom.stop();
        }
        self.mode = AppMode::Idle;
        self.spotlight.active = false;
        self.snip.active = false;
        self.is_drawing = false;
        self.active_shape = None;
        self.text_editor = None;
        self.show_cheat_sheet = false;
        self.background_bitmap = None;
        self.background_capture = None;
        self.zoom = ZoomState::default();
        self.hide_window();
    }

    pub fn show_window(&mut self) {
        unsafe {
            let sx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let sy = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let sw = GetSystemMetrics(SM_CXVIRTUALSCREEN) as u32;
            let sh = GetSystemMetrics(SM_CYVIRTUALSCREEN) as u32;

            if sw != self.screen_width || sh != self.screen_height {
                self.screen_x = sx;
                self.screen_y = sy;
                self.screen_width = sw;
                self.screen_height = sh;
                self.renderer.resize(sw, sh);
            }

            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                self.screen_x,
                self.screen_y,
                self.screen_width as i32,
                self.screen_height as i32,
                SWP_SHOWWINDOW,
            );

            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));
            self.request_repaint();
        }
    }

    pub fn hide_window(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    pub fn capture_current_screen(&mut self) {
        if let Some(cap) = ScreenCapture::capture_screen() {
            if let Some(rt) = &self.renderer.render_target {
                if let Ok(bmp) = cap.create_d2d_bitmap(rt) {
                    self.background_bitmap = Some(bmp);
                }
            }
            self.background_capture = Some(cap);
        }
    }

    pub fn request_repaint(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    pub fn set_toast(&mut self, icon: &'static str, message: impl Into<String>) {
        self.toast = Some(ToastNotification::new(icon, message));
        self.request_repaint();
    }

    pub fn push_shape(&mut self, shape: Shape) {
        self.shapes.push(shape.clone());
        self.undo_history.push(HistoryAction::AddShape(shape));
        self.redo_history.clear();
    }

    pub fn undo(&mut self) {
        if let Some(action) = self.undo_history.pop() {
            match action {
                HistoryAction::AddShape(_) => {
                    if let Some(shape) = self.shapes.pop() {
                        self.redo_history.push(HistoryAction::AddShape(shape));
                        self.set_toast("↩️", "Undo Shape");
                    }
                }
                HistoryAction::Clear(prev_shapes) => {
                    let current_shapes = std::mem::replace(&mut self.shapes, prev_shapes);
                    self.redo_history.push(HistoryAction::Clear(current_shapes));
                    self.set_toast("↩️", "Restored Cleared Canvas");
                }
            }
            self.request_repaint();
        }
    }

    pub fn redo(&mut self) {
        if let Some(action) = self.redo_history.pop() {
            match action {
                HistoryAction::AddShape(shape) => {
                    self.shapes.push(shape.clone());
                    self.undo_history.push(HistoryAction::AddShape(shape));
                    self.set_toast("↪️", "Redo Shape");
                }
                HistoryAction::Clear(_prev_shapes) => {
                    let current_shapes = std::mem::replace(&mut self.shapes, Vec::new());
                    self.undo_history.push(HistoryAction::Clear(current_shapes));
                    self.set_toast("↪️", "Re-cleared Canvas");
                }
            }
            self.request_repaint();
        }
    }

    pub fn clear_all(&mut self) {
        if !self.shapes.is_empty() {
            let old = std::mem::take(&mut self.shapes);
            self.undo_history.push(HistoryAction::Clear(old));
            self.redo_history.clear();
            self.set_toast("🧹", "Canvas Cleared (Ctrl+Z to Undo)");
            self.request_repaint();
        }
    }

    pub fn commit_text_editor(&mut self) {
        if let Some((pos, text, color, font_size)) = self.text_editor.take() {
            if !text.trim().is_empty() {
                self.push_shape(Shape::Text {
                    origin: pos,
                    text,
                    font_size,
                    color,
                });
            }
            self.request_repaint();
        }
    }

    pub fn get_composite_capture(&self, include_spotlight: bool) -> Option<ScreenCapture> {
        let text_input = self.text_editor.as_ref().map(|(p, t, c, s)| (p, t.as_str(), c, *s));
        self.renderer.render_to_capture(
            self.screen_x,
            self.screen_y,
            self.screen_width,
            self.screen_height,
            self.background_bitmap.as_ref(),
            self.background_type,
            &self.spotlight,
            &self.shapes,
            self.active_shape.as_ref(),
            text_input,
            include_spotlight,
        )
    }

    pub fn copy_screen_to_clipboard(&mut self) {
        if let Some(composite) = self.get_composite_capture(self.spotlight.active) {
            if copy_bgra_to_clipboard(composite.width, composite.height, &composite.pixels) {
                self.set_toast("📋", "Copied Screen + Drawings to Clipboard!");
            }
        }
    }

    pub fn save_snapshot(&mut self) {
        if let Some(composite) = self.get_composite_capture(self.spotlight.active) {
            let pictures_dir = std::env::var("USERPROFILE")
                .map(|p| format!("{}\\Pictures", p))
                .unwrap_or_else(|_| ".".to_string());

            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let filename = format!("{}\\Zoomify_{}.png", pictures_dir, timestamp);
            if composite.save_png(&filename).is_ok() {
                self.set_toast("💾", format!("Saved to Pictures\\Zoomify_{}.png", timestamp));
            } else {
                self.set_toast("❌", "Failed to save screenshot!");
            }
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            let raw_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<Self>;
            if raw_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }

            let mut this = match (*raw_ptr).try_borrow_mut() {
                Ok(b) => b,
                Err(_) => return DefWindowProcW(hwnd, msg, wparam, lparam),
            };

            match msg {
                WM_PAINT => {
                    let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
                    let _ = windows::Win32::Graphics::Gdi::BeginPaint(hwnd, &mut ps);

                    let mut timer_info = None;
                    if this.mode == AppMode::Timer {
                        let total = this.timer_seconds.max(1) as f32;
                        let rem = this.timer_remaining.max(0.0) as f32;
                        let mins = (rem / 60.0).floor() as u32;
                        let secs = (rem % 60.0).floor() as u32;
                        let progress = 1.0 - (rem / total);
                        timer_info = Some((mins, secs, progress, this.timer_paused));
                    }

                    let text_input = this.text_editor.as_ref().map(|(p, t, c, s)| (p, t.as_str(), c, *s));
                    let snip_ref = if this.snip.active { Some(&this.snip) } else { None };

                    this.renderer.render_frame(
                        this.mode,
                        this.screen_width as f32,
                        this.screen_height as f32,
                        this.background_bitmap.as_ref(),
                        this.background_type,
                        &this.zoom,
                        &this.spotlight,
                        &this.shapes,
                        this.active_shape.as_ref(),
                        text_input,
                        snip_ref,
                        this.current_tool,
                        this.current_color,
                        this.stroke_width,
                        this.toast.as_ref(),
                        this.show_cheat_sheet,
                        timer_info,
                    );

                    let _ = windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
                    LRESULT(0)
                }

                windows::Win32::UI::WindowsAndMessaging::WM_ERASEBKGND => LRESULT(1),

                WM_TIMER => {
                    if wparam.0 == TIMER_ID_ANIMATION {
                        let mut needs_paint = false;
                        let sw = this.screen_width as f32;
                        let sh = this.screen_height as f32;

                        if this.mode == AppMode::StaticZoom {
                            if this.zoom.tick_smooth_pan(0.25, sw, sh) {
                                needs_paint = true;
                            }
                        }

                        if this.mode == AppMode::LiveZoom {
                            this.live_zoom.tick_smooth_pan(0.25);
                        }

                        if this.mode == AppMode::Timer && !this.timer_paused {
                            let dt = this.timer_last_tick.elapsed().as_secs_f64();
                            this.timer_last_tick = Instant::now();
                            this.timer_remaining = (this.timer_remaining - dt).max(0.0);
                            needs_paint = true;
                        } else {
                            this.timer_last_tick = Instant::now();
                        }

                        if let Some(t) = &this.toast {
                            if !t.is_expired() {
                                needs_paint = true;
                            }
                        }

                        if needs_paint {
                            this.request_repaint();
                        }
                    }
                    LRESULT(0)
                }

                WM_SETCURSOR => {
                    if this.current_tool == DrawTool::Text {
                        let _ = SetCursor(Some(LoadCursorW(None, IDC_IBEAM).unwrap_or_default()));
                        return LRESULT(1);
                    }
                    let _ = SetCursor(Some(LoadCursorW(None, IDC_CROSS).unwrap_or_default()));
                    LRESULT(1)
                }

                WM_MOUSEMOVE => {
                    let x = (lparam.0 & 0xFFFF) as i16 as f32;
                    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                    let screen_pt = Point2D::new(x, y);
                    let canvas_pt = this.zoom.screen_to_canvas(screen_pt);
                    let sw = this.screen_width as f32;
                    let sh = this.screen_height as f32;

                    if this.spotlight.active && !this.spotlight.pinned {
                        this.spotlight.x = canvas_pt.x;
                        this.spotlight.y = canvas_pt.y;
                        this.request_repaint();
                    }

                    if this.snip.active {
                        this.snip.current = screen_pt;
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.mode == AppMode::StaticZoom && !this.is_drawing && !this.zoom.is_dragging {
                        this.zoom.update_target_from_cursor(x, y, sw, sh);
                        this.zoom.view_x = this.zoom.target_view_x;
                        this.zoom.view_y = this.zoom.target_view_y;
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.zoom.is_dragging {
                        let z = this.zoom.level.max(1.0);
                        let dx = (screen_pt.x - this.zoom.drag_start_mouse.x) / z;
                        let dy = (screen_pt.y - this.zoom.drag_start_mouse.y) / z;
                        this.zoom.target_view_x = this.zoom.drag_start_view.x - dx;
                        this.zoom.target_view_y = this.zoom.drag_start_view.y - dy;
                        this.zoom.clamp_viewport(sw, sh);
                        this.zoom.view_x = this.zoom.target_view_x;
                        this.zoom.view_y = this.zoom.target_view_y;
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.is_drawing {
                        let is_shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                        let start_pt = this.draw_start_pt;

                        match &mut this.active_shape {
                            Some(Shape::Stroke { points, .. }) => {
                                let should_push = match points.last() {
                                    Some(last) => last.distance(&canvas_pt) >= 1.5,
                                    None => true,
                                };
                                if should_push {
                                    points.push(canvas_pt);
                                    this.request_repaint();
                                }
                            }
                            Some(Shape::Line { end, .. }) => {
                                *end = if is_shift {
                                    snap_to_angle(start_pt, canvas_pt)
                                } else {
                                    canvas_pt
                                };
                                this.request_repaint();
                            }
                            Some(Shape::Arrow { end, .. }) => {
                                *end = if is_shift {
                                    snap_to_angle(start_pt, canvas_pt)
                                } else {
                                    canvas_pt
                                };
                                this.request_repaint();
                            }
                            Some(Shape::Rectangle { end, .. }) => {
                                *end = if is_shift {
                                    snap_to_square(start_pt, canvas_pt)
                                } else {
                                    canvas_pt
                                };
                                this.request_repaint();
                            }
                            Some(Shape::Ellipse { end, .. }) => {
                                *end = if is_shift {
                                    snap_to_square(start_pt, canvas_pt)
                                } else {
                                    canvas_pt
                                };
                                this.request_repaint();
                            }
                            _ => {}
                        }
                    }

                    LRESULT(0)
                }

                WM_LBUTTONDOWN => {
                    let x = (lparam.0 & 0xFFFF) as i16 as f32;
                    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                    let screen_pt = Point2D::new(x, y);
                    let canvas_pt = this.zoom.screen_to_canvas(screen_pt);

                    let is_shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                    let is_ctrl = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
                    let is_tab = (GetKeyState(VK_TAB.0 as i32) as u16 & 0x8000) != 0;

                    // If in Static Zoom mode, clicking locks the viewport and transitions into Draw mode while starting the stroke immediately
                    if this.mode == AppMode::StaticZoom {
                        this.mode = AppMode::Draw;
                        this.set_toast("✏️", "Draw Mode");
                        this.is_drawing = true;
                        this.draw_start_pt = canvas_pt;
                        this.active_shape = Some(Shape::Stroke {
                            points: vec![canvas_pt],
                            color: this.current_color,
                            width: this.stroke_width,
                            is_highlighter: false,
                        });
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.current_tool == DrawTool::Snip || (is_shift && is_ctrl && !is_tab) {
                        this.snip.active = true;
                        this.snip.start = screen_pt;
                        this.snip.current = screen_pt;
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.current_tool == DrawTool::Text {
                        this.commit_text_editor();
                        let cur_col = this.current_color;
                        let cur_sz = this.stroke_width * 3.5 + 16.0;
                        this.text_editor = Some((canvas_pt, String::new(), cur_col, cur_sz));
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.current_tool == DrawTool::StepBadge {
                        let num = this.step_counter;
                        let rad = this.stroke_width * 1.5 + 14.0;
                        let col = this.current_color;
                        this.push_shape(Shape::StepBadge {
                            center: canvas_pt,
                            number: num,
                            radius: rad,
                            color: col,
                        });
                        this.step_counter += 1;
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    this.is_drawing = true;
                    this.draw_start_pt = canvas_pt;

                    // ZoomIt Standard Modifiers:
                    // Shift+Ctrl = Arrow
                    // Shift = Line
                    // Ctrl = Rectangle
                    // Tab = Ellipse
                    let shape_to_create = if is_shift && is_ctrl {
                        Shape::Arrow {
                            start: canvas_pt,
                            end: canvas_pt,
                            color: this.current_color,
                            width: this.stroke_width,
                        }
                    } else if is_shift && !is_ctrl {
                        Shape::Line {
                            start: canvas_pt,
                            end: canvas_pt,
                            color: this.current_color,
                            width: this.stroke_width,
                        }
                    } else if is_ctrl && !is_shift {
                        Shape::Rectangle {
                            start: canvas_pt,
                            end: canvas_pt,
                            color: this.current_color,
                            width: this.stroke_width,
                            rounded: false,
                        }
                    } else if is_tab {
                        Shape::Ellipse {
                            start: canvas_pt,
                            end: canvas_pt,
                            color: this.current_color,
                            width: this.stroke_width,
                        }
                    } else {
                        match this.current_tool {
                            DrawTool::Pen => Shape::Stroke {
                                points: vec![canvas_pt],
                                color: this.current_color,
                                width: this.stroke_width,
                                is_highlighter: false,
                            },
                            DrawTool::Highlighter => Shape::Stroke {
                                points: vec![canvas_pt],
                                color: this.current_color,
                                width: this.stroke_width,
                                is_highlighter: true,
                            },
                            DrawTool::Line => Shape::Line {
                                start: canvas_pt,
                                end: canvas_pt,
                                color: this.current_color,
                                width: this.stroke_width,
                            },
                            DrawTool::Arrow => Shape::Arrow {
                                start: canvas_pt,
                                end: canvas_pt,
                                color: this.current_color,
                                width: this.stroke_width,
                            },
                            DrawTool::Rectangle => Shape::Rectangle {
                                start: canvas_pt,
                                end: canvas_pt,
                                color: this.current_color,
                                width: this.stroke_width,
                                rounded: false,
                            },
                            DrawTool::RoundedRectangle => Shape::Rectangle {
                                start: canvas_pt,
                                end: canvas_pt,
                                color: this.current_color,
                                width: this.stroke_width,
                                rounded: true,
                            },
                            DrawTool::Ellipse => Shape::Ellipse {
                                start: canvas_pt,
                                end: canvas_pt,
                                color: this.current_color,
                                width: this.stroke_width,
                            },
                            _ => Shape::Stroke {
                                points: vec![canvas_pt],
                                color: this.current_color,
                                width: this.stroke_width,
                                is_highlighter: false,
                            },
                        }
                    };

                    this.active_shape = Some(shape_to_create);
                    this.request_repaint();
                    LRESULT(0)
                }

                WM_LBUTTONUP => {
                    if this.snip.active {
                        this.snip.active = false;
                        let (l, t, r, b) = this.snip.rect();
                        let w = (r - l).round() as u32;
                        let h = (b - t).round() as u32;

                        if w > 4 && h > 4 {
                            if let Some(composite) = this.get_composite_capture(false) {
                                if let Some(cropped) = composite.crop(l as i32 + this.screen_x, t as i32 + this.screen_y, w, h) {
                                    if copy_bgra_to_clipboard(cropped.width, cropped.height, &cropped.pixels) {
                                        this.set_toast("✂️", format!("Snipped {}×{} px to Clipboard!", w, h));
                                        this.exit_overlay();
                                        return LRESULT(0);
                                    }
                                }
                            }
                        }
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.is_drawing {
                        this.is_drawing = false;
                        if let Some(shape) = this.active_shape.take() {
                            this.push_shape(shape);
                        }
                        this.request_repaint();
                    }
                    LRESULT(0)
                }

                WM_RBUTTONDOWN | WM_RBUTTONUP => {
                    // Right-click exits / dismisses immediately in ZoomIt
                    this.exit_overlay();
                    LRESULT(0)
                }

                WM_MBUTTONDOWN => {
                    let x = (lparam.0 & 0xFFFF) as i16 as f32;
                    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                    this.zoom.is_dragging = true;
                    this.zoom.drag_start_mouse = Point2D::new(x, y);
                    this.zoom.drag_start_view = Point2D::new(this.zoom.view_x, this.zoom.view_y);
                    LRESULT(0)
                }

                windows::Win32::UI::WindowsAndMessaging::WM_MBUTTONUP => {
                    this.zoom.is_dragging = false;
                    LRESULT(0)
                }

                WM_MOUSEWHEEL => {
                    let delta = ((wparam.0 >> 16) as i16 as f32) / 120.0;
                    let is_ctrl = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
                    let is_alt = (GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0;

                    if this.spotlight.active || is_alt {
                        let old_r = this.spotlight.radius;
                        let new_r = (old_r + delta * 20.0).clamp(30.0, 700.0);
                        this.spotlight.radius = new_r;
                        let diam = (new_r * 2.0).round() as u32;
                        this.set_toast("🔦", format!("Spotlight ⌀{} px", diam));
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.mode == AppMode::LiveZoom {
                        this.live_zoom.adjust_zoom(delta * 0.25);
                        let z_val = this.live_zoom.zoom_level();
                        this.set_toast("🔍", format!("Live Zoom {:.2}x", z_val));
                        return LRESULT(0);
                    }

                    if this.mode == AppMode::StaticZoom || (this.mode == AppMode::Draw && is_ctrl) {
                        let mut pt = POINT::default();
                        let sx = this.screen_x;
                        let sy = this.screen_y;
                        let sw = this.screen_width as f32;
                        let sh = this.screen_height as f32;
                        let (cursor_x, cursor_y) = if GetCursorPos(&mut pt).is_ok() {
                            ((pt.x - sx) as f32, (pt.y - sy) as f32)
                        } else {
                            (sw / 2.0, sh / 2.0)
                        };
                        let new_lvl = (this.zoom.level + delta * 0.25).clamp(1.0, 10.0);
                        this.zoom.set_zoom_centered(new_lvl, Point2D::new(cursor_x, cursor_y), sw, sh);
                        this.set_toast("🔎", format!("Zoom {:.2}x", new_lvl));
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.mode == AppMode::Draw || this.mode == AppMode::Snip {
                        let new_width = (this.stroke_width + delta * 1.5).clamp(1.0, 40.0);
                        this.stroke_width = new_width;
                        let w_val = new_width.round() as u32;
                        this.set_toast("🖌️", format!("Stroke Width {} px", w_val));
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    LRESULT(0)
                }

                WM_CHAR => {
                    let ch = char::from_u32(wparam.0 as u32).unwrap_or('\0');
                    if let Some((_, text, _, _)) = &mut this.text_editor {
                        if ch == '\x08' {
                            // Backspace handled in WM_KEYDOWN
                        } else if ch == '\r' || ch == '\n' {
                            this.commit_text_editor();
                        } else if !ch.is_control() {
                            text.push(ch);
                            this.request_repaint();
                        }
                        return LRESULT(0);
                    }
                    LRESULT(0)
                }

                WM_KEYDOWN => {
                    let key = wparam.0 as i32;
                    let is_ctrl = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
                    let is_shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;

                    // ── Text editor intercepts all keys first ──
                    if let Some((_, text, _, font_size)) = &mut this.text_editor {
                        if key == VK_ESCAPE.0 as i32 {
                            this.text_editor = None;
                            this.request_repaint();
                            return LRESULT(0);
                        } else if key == VK_RETURN.0 as i32 {
                            this.commit_text_editor();
                            return LRESULT(0);
                        } else if key == VK_BACK.0 as i32 {
                            text.pop();
                            this.request_repaint();
                            return LRESULT(0);
                        } else if is_ctrl && key == 'V' as i32 {
                            if let Some(clip_text) = get_clipboard_text() {
                                text.push_str(&clip_text);
                                this.request_repaint();
                            }
                            return LRESULT(0);
                        } else if key == VK_UP.0 as i32 {
                            *font_size = (*font_size + 4.0).min(96.0);
                            this.request_repaint();
                            return LRESULT(0);
                        } else if key == VK_DOWN.0 as i32 {
                            *font_size = (*font_size - 4.0).max(12.0);
                            this.request_repaint();
                            return LRESULT(0);
                        }
                        // Let WM_CHAR handle typed characters
                        return LRESULT(0);
                    }

                    // ── Ctrl combos (always take priority) ──
                    if is_ctrl {
                        match key {
                            k if k == 'Z' as i32 && !is_shift => { this.undo(); }
                            k if k == 'Z' as i32 && is_shift => { this.redo(); }
                            k if k == 'Y' as i32 => { this.redo(); }
                            k if k == 'C' as i32 => { this.copy_screen_to_clipboard(); }
                            k if k == 'S' as i32 => { this.save_snapshot(); }
                            _ => {}
                        }
                        return LRESULT(0);
                    }

                    // ── Non-Ctrl keys ──
                    match key {
                        // ─── Navigation / System ───
                        k if k == VK_ESCAPE.0 as i32 => {
                            this.exit_overlay();
                        }

                        k if k == VK_F1.0 as i32 => {
                            this.show_cheat_sheet = !this.show_cheat_sheet;
                            this.request_repaint();
                        }

                        k if k == VK_SPACE.0 as i32 => {
                            if this.mode == AppMode::Timer {
                                let paused = !this.timer_paused;
                                this.timer_paused = paused;
                                this.set_toast("⏱️", if paused { "Timer Paused" } else { "Timer Resumed" });
                                this.request_repaint();
                            } else if this.spotlight.active {
                                let pinned = !this.spotlight.pinned;
                                this.spotlight.pinned = pinned;
                                this.set_toast("🔦", if pinned { "Spotlight Pinned" } else { "Spotlight Following" });
                                this.request_repaint();
                            }
                        }

                        k if k == VK_TAB.0 as i32 => {
                            this.toggle_spotlight();
                        }

                        // ─── Colors (single key, no modifier = color) ───
                        k if k == 'R' as i32 && !is_shift => {
                            this.current_color = ColorPreset::Red;
                            this.set_toast("🔴", "Red");
                            this.request_repaint();
                        }
                        k if k == 'G' as i32 => {
                            this.current_color = ColorPreset::Green;
                            this.set_toast("🟢", "Green");
                            this.request_repaint();
                        }
                        k if k == 'B' as i32 => {
                            this.current_color = ColorPreset::Blue;
                            this.set_toast("🔵", "Blue");
                            this.request_repaint();
                        }
                        k if k == 'Y' as i32 => {
                            this.current_color = ColorPreset::Yellow;
                            this.set_toast("🟡", "Yellow");
                            this.request_repaint();
                        }
                        k if k == 'O' as i32 => {
                            this.current_color = ColorPreset::Orange;
                            this.set_toast("🟠", "Orange");
                            this.request_repaint();
                        }
                        k if k == 'P' as i32 => {
                            this.current_color = ColorPreset::Pink;
                            this.set_toast("🌸", "Pink");
                            this.request_repaint();
                        }
                        k if k == 'C' as i32 || k == 'I' as i32 => {
                            this.current_color = ColorPreset::Cyan;
                            this.set_toast("🩵", "Cyan");
                            this.request_repaint();
                        }

                        // ─── Canvas Slate Modes ───
                        k if k == 'W' as i32 => {
                            this.background_type = if this.background_type == CanvasBackground::Whiteboard {
                                CanvasBackground::Transparent
                            } else {
                                CanvasBackground::Whiteboard
                            };
                            this.current_color = ColorPreset::Red;
                            this.set_toast("⚪", "Whiteboard");
                            this.request_repaint();
                        }
                        k if k == 'K' as i32 => {
                            this.background_type = if this.background_type == CanvasBackground::Blackboard {
                                CanvasBackground::Transparent
                            } else {
                                CanvasBackground::Blackboard
                            };
                            this.current_color = ColorPreset::White;
                            this.set_toast("⚫", "Blackboard");
                            this.request_repaint();
                        }

                        // ─── Drawing Tools ───
                        k if k == 'T' as i32 => {
                            this.current_tool = DrawTool::Text;
                            this.set_toast("🔤", "Text — click to type");
                            this.request_repaint();
                        }
                        k if k == 'H' as i32 => {
                            this.current_tool = DrawTool::Highlighter;
                            this.set_toast("🖍️", "Highlighter");
                            this.request_repaint();
                        }
                        k if k == 'L' as i32 => {
                            this.current_tool = DrawTool::Line;
                            this.set_toast("📏", "Line");
                            this.request_repaint();
                        }
                        k if k == 'A' as i32 => {
                            this.current_tool = DrawTool::Arrow;
                            this.set_toast("➜", "Arrow");
                            this.request_repaint();
                        }
                        k if k == 'R' as i32 && is_shift => {
                            this.current_tool = DrawTool::Rectangle;
                            this.set_toast("▭", "Rectangle");
                            this.request_repaint();
                        }
                        k if k == 'U' as i32 => {
                            this.current_tool = DrawTool::RoundedRectangle;
                            this.set_toast("▢", "Rounded Rectangle");
                            this.request_repaint();
                        }
                        k if (k == 'N' as i32 && is_shift) || k == '0' as i32 => {
                            this.step_counter = 1;
                            this.set_toast("🔢", "Step badge counter reset to 1");
                            this.request_repaint();
                        }
                        k if k == 'N' as i32 => {
                            let next_num = this.step_counter;
                            this.current_tool = DrawTool::StepBadge;
                            this.set_toast("🔢", format!("Step Badge (next: #{})", next_num));
                            this.request_repaint();
                        }
                        k if k == 'D' as i32 || k == 'P' as i32 => {
                            this.current_tool = DrawTool::Pen;
                            this.set_toast("✏️", "Pen");
                            this.request_repaint();
                        }
                        k if k == 'S' as i32 || k == 'X' as i32 => {
                            this.current_tool = DrawTool::Snip;
                            this.set_toast("✂️", "Snip — drag to copy");
                            this.request_repaint();
                        }

                        // ─── Erase / Clear ───
                        k if k == 'E' as i32 || k == VK_DELETE.0 as i32 => {
                            this.clear_all();
                        }

                        // ─── Stroke Width Presets (1..9) ───
                        k if k >= '1' as i32 && k <= '9' as i32 => {
                            let idx = (k - '1' as i32) as usize;
                            let widths = [2.0, 4.0, 6.0, 8.0, 12.0, 16.0, 22.0, 28.0, 36.0];
                            let w = widths[idx.min(8)];
                            this.stroke_width = w;
                            this.set_toast("🖌️", format!("{} px", w as u32));
                            this.request_repaint();
                        }

                        // ─── [ ] Bracket Sizing ───
                        k if k == 219 => { // [
                            if this.spotlight.active {
                                let new_rad = (this.spotlight.radius - 20.0).max(30.0);
                                this.spotlight.radius = new_rad;
                                this.set_toast("🔦", format!("⌀{} px", (new_rad * 2.0).round() as u32));
                            } else {
                                let new_w = (this.stroke_width - 2.0).max(1.0);
                                this.stroke_width = new_w;
                                this.set_toast("🖌️", format!("{} px", new_w.round() as u32));
                            }
                            this.request_repaint();
                        }
                        k if k == 221 => { // ]
                            if this.spotlight.active {
                                let new_rad = (this.spotlight.radius + 20.0).min(700.0);
                                this.spotlight.radius = new_rad;
                                this.set_toast("🔦", format!("⌀{} px", (new_rad * 2.0).round() as u32));
                            } else {
                                let new_w = (this.stroke_width + 2.0).min(40.0);
                                this.stroke_width = new_w;
                                this.set_toast("🖌️", format!("{} px", new_w.round() as u32));
                            }
                            this.request_repaint();
                        }

                        // ─── Arrow Keys & Zoom/Timer Controls ───
                        k if k == VK_UP.0 as i32 || k == 187 => { // Up or '+'
                            if this.mode == AppMode::Timer {
                                this.timer_remaining += 60.0;
                                this.timer_seconds = this.timer_remaining.round() as u32;
                                let m = (this.timer_remaining / 60.0).floor() as u32;
                                this.set_toast("⏱️", format!("Timer: +1m ({}m total)", m));
                            } else if this.mode == AppMode::StaticZoom {
                                let mut pt = POINT::default();
                                let sx = this.screen_x;
                                let sy = this.screen_y;
                                let sw = this.screen_width as f32;
                                let sh = this.screen_height as f32;
                                let (cursor_x, cursor_y) = if GetCursorPos(&mut pt).is_ok() {
                                    ((pt.x - sx) as f32, (pt.y - sy) as f32)
                                } else {
                                    (sw / 2.0, sh / 2.0)
                                };
                                let new_lvl = (this.zoom.level + 0.25).clamp(1.0, 10.0);
                                this.zoom.set_zoom_centered(new_lvl, Point2D::new(cursor_x, cursor_y), sw, sh);
                                this.set_toast("🔎", format!("Zoom {:.2}x", new_lvl));
                            } else {
                                let new_w = (this.stroke_width + 2.0).min(40.0);
                                this.stroke_width = new_w;
                                this.set_toast("🖌️", format!("{} px", new_w.round() as u32));
                            }
                            this.request_repaint();
                        }
                        k if k == VK_DOWN.0 as i32 || k == 189 => { // Down or '-'
                            if this.mode == AppMode::Timer {
                                this.timer_remaining = (this.timer_remaining - 60.0).max(10.0);
                                this.timer_seconds = this.timer_remaining.round() as u32;
                                let m = (this.timer_remaining / 60.0).floor() as u32;
                                this.set_toast("⏱️", format!("Timer: -1m ({}m total)", m));
                            } else if this.mode == AppMode::StaticZoom {
                                let mut pt = POINT::default();
                                let sx = this.screen_x;
                                let sy = this.screen_y;
                                let sw = this.screen_width as f32;
                                let sh = this.screen_height as f32;
                                let (cursor_x, cursor_y) = if GetCursorPos(&mut pt).is_ok() {
                                    ((pt.x - sx) as f32, (pt.y - sy) as f32)
                                } else {
                                    (sw / 2.0, sh / 2.0)
                                };
                                let new_lvl = (this.zoom.level - 0.25).clamp(1.0, 10.0);
                                this.zoom.set_zoom_centered(new_lvl, Point2D::new(cursor_x, cursor_y), sw, sh);
                                this.set_toast("🔎", format!("Zoom {:.2}x", new_lvl));
                            } else {
                                let new_w = (this.stroke_width - 2.0).max(1.0);
                                this.stroke_width = new_w;
                                this.set_toast("🖌️", format!("{} px", new_w.round() as u32));
                            }
                            this.request_repaint();
                        }

                        _ => {}
                    }

                    LRESULT(0)
                }

                windows::Win32::UI::WindowsAndMessaging::WM_NCDESTROY => {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    let _ = Rc::from_raw(raw_ptr);
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }
}
