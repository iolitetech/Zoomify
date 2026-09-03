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
    GetKeyState, SetFocus, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1,
    VK_F2, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos,
    GetWindowLongPtrW, LoadCursorW, RegisterClassExW, SetCursor, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_USERDATA, HWND_TOPMOST, IDC_ARROW,
    IDC_CROSS, IDC_HAND, IDC_IBEAM, IDC_SIZEALL,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WM_CHAR, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_TIMER, WNDCLASSEXW, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::capture::ScreenCapture;
use crate::clipboard::{copy_bgra_to_clipboard, get_clipboard_text};
use crate::live_zoom::LiveZoomEngine;
use crate::renderer::D2DRenderer;
use crate::shapes::{recognize_smart_shape, shape_intersects_circle, snap_to_angle, snap_to_square};
use crate::types::{
    AppMode, ArrowStyle, BadgeShape, BadgeSize, CanvasBackground, ColorPreset, DrawTool,
    FillMode, FluentAction, FluentToolbarState, HistoryAction, LaserTrailPoint, Point2D,
    Shape, SnipSelection, SnipShape, SpotlightState, StrokePattern, TextCardStyle, TextEditorState,
    TextFontFamily, TimerAction, TimerWidgetState, ToastNotification, ZoomState,
};

const OVERLAY_CLASS_NAME: PCWSTR = w!("ZoomifyFullscreenOverlay");
const TIMER_ID_ANIMATION: usize = 1001;

unsafe extern "system" {
    fn MessageBeep(utype: u32) -> i32;
}

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
    pub fill_mode: FillMode,
    pub stroke_pattern: StrokePattern,
    pub arrow_style: ArrowStyle,
    pub badge_size: BadgeSize,
    pub badge_shape: BadgeShape,
    pub live_zoom: LiveZoomEngine,
    pub is_drawing: bool,
    pub draw_start_pt: Point2D,
    pub step_counter: u32,
    pub text_editor: Option<TextEditorState>,
    pub font_size: f32,
    pub text_is_bold: bool,
    pub text_is_italic: bool,
    pub text_card_style: TextCardStyle,
    pub text_font_family: TextFontFamily,
    pub toast: Option<ToastNotification>,
    pub show_cheat_sheet: bool,
    pub show_hud: bool,
    pub toolbar: FluentToolbarState,
    pub laser_trail: Vec<LaserTrailPoint>,
    pub laser_pos: Option<Point2D>,
    pub eraser_pos: Option<Point2D>,
    pub stroke_start_time: Option<Instant>,
    pub last_mouse_dwell_time: Option<Instant>,
    pub last_mouse_pos: Point2D,
    pub timer_seconds: u32,
    pub timer_remaining: f64,
    pub timer_paused: bool,
    pub timer_alarm_sounded: bool,
    pub timer_last_tick: Instant,
    pub timer_widget: TimerWidgetState,
    pub screen_x: i32,
    pub screen_y: i32,
    pub screen_width: u32,
    pub screen_height: u32,
    pub available_monitors: Vec<crate::monitor::MonitorInfo>,
    pub current_monitor_index: usize,
    pub current_monitor: crate::monitor::MonitorInfo,
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

            let available_monitors = crate::monitor::MonitorManager::enumerate_monitors();
            let current_monitor = available_monitors.get(0).cloned().unwrap_or_default();
            let screen_x = current_monitor.x;
            let screen_y = current_monitor.y;
            let screen_width = current_monitor.width;
            let screen_height = current_monitor.height;

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

            let cfg = crate::config::AppConfig::load();
            let mut toolbar = FluentToolbarState::default();
            toolbar.collapsed = cfg.toolbar_collapsed;
            toolbar.monitor_count = available_monitors.len();
            toolbar.current_monitor_index = 0;
            if let Some((cx, cy)) = cfg.toolbar_custom_position {
                toolbar.custom_position = Some(Point2D::new(cx, cy));
            }
            let mut spotlight = SpotlightState::default();
            spotlight.radius = cfg.spotlight_radius;
            let timer_secs = (cfg.timer_duration_mins * 60).max(60);
            let initial_color = match cfg.default_color.to_lowercase().as_str() {
                "red" => ColorPreset::Red,
                "green" => ColorPreset::Green,
                "blue" => ColorPreset::Blue,
                "yellow" => ColorPreset::Yellow,
                "orange" => ColorPreset::Orange,
                "pink" => ColorPreset::Pink,
                "cyan" => ColorPreset::Cyan,
                "white" => ColorPreset::White,
                "black" => ColorPreset::Black,
                _ => ColorPreset::Red,
            };

            let fill_mode = match cfg.default_fill_mode.as_str() {
                "Tinted" => FillMode::Tinted,
                "Solid" => FillMode::Solid,
                _ => FillMode::None,
            };
            let stroke_pattern = match cfg.default_stroke_pattern.as_str() {
                "Dashed" => StrokePattern::Dashed,
                "Dotted" => StrokePattern::Dotted,
                _ => StrokePattern::Solid,
            };
            let badge_size = match cfg.default_badge_size.as_str() {
                "Small" => BadgeSize::Small,
                "Large" => BadgeSize::Large,
                "ExtraLarge" => BadgeSize::ExtraLarge,
                _ => BadgeSize::Medium,
            };
            let badge_shape = BadgeShape::Circle;
            let arrow_style = ArrowStyle::Single;

            toolbar.current_fill_mode = fill_mode;
            toolbar.current_stroke_pattern = stroke_pattern;
            toolbar.current_arrow_style = arrow_style;
            toolbar.current_badge_size = badge_size;
            toolbar.current_badge_shape = badge_shape;
            toolbar.stroke_width = cfg.default_stroke_width;
            toolbar.badge_counter = 1;
            toolbar.active_tool = None;

            let state = Rc::new(RefCell::new(Self {
                hwnd,
                renderer,
                mode: AppMode::Idle,
                background_capture: None,
                background_bitmap: None,
                background_type: CanvasBackground::Transparent,
                zoom: ZoomState::default(),
                spotlight,
                snip: SnipSelection {
                    active: false,
                    start: Point2D::default(),
                    current: Point2D::default(),
                    shape: SnipShape::Rectangle,
                },
                shapes: Vec::new(),
                undo_history: Vec::new(),
                redo_history: Vec::new(),
                active_shape: None,
                current_tool: DrawTool::Pen,
                current_color: initial_color,
                stroke_width: cfg.default_stroke_width,
                fill_mode,
                stroke_pattern,
                arrow_style,
                badge_size,
                badge_shape,
                live_zoom: LiveZoomEngine::new(),
                is_drawing: false,
                draw_start_pt: Point2D::default(),
                step_counter: 1,
                text_editor: None,
                font_size: 22.0,
                text_is_bold: false,
                text_is_italic: false,
                text_card_style: TextCardStyle::Transparent,
                text_font_family: TextFontFamily::SegoeUI,
                toast: None,
                show_cheat_sheet: false,
                show_hud: true,
                toolbar,
                laser_trail: Vec::new(),
                laser_pos: None,
                eraser_pos: None,
                stroke_start_time: None,
                last_mouse_dwell_time: None,
                last_mouse_pos: Point2D::default(),
                timer_seconds: timer_secs,
                timer_remaining: timer_secs as f64,
                timer_paused: false,
                timer_alarm_sounded: false,
                timer_last_tick: Instant::now(),
                timer_widget: TimerWidgetState::default(),
                screen_x,
                screen_y,
                screen_width,
                screen_height,
                available_monitors,
                current_monitor_index: 0,
                current_monitor,
            }));

            let raw_ptr = Rc::into_raw(Rc::clone(&state));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw_ptr as isize);

            windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(hwnd), TIMER_ID_ANIMATION, 16, None);

            Ok(state)
        }
    }

    pub fn enter_live_zoom(&mut self) {
        self.target_monitor_under_cursor();
        self.toolbar.active_tool = None;
        self.mode = AppMode::LiveZoom;
        self.background_bitmap = None;
        self.background_capture = None;
        self.hide_window();
        self.live_zoom.start(2.0);
    }

    pub fn enter_static_zoom(&mut self) {
        if self.live_zoom.is_active() || self.mode == AppMode::LiveZoom {
            self.live_zoom.stop();
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        self.target_monitor_under_cursor();
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
        self.toolbar.active_tool = None;
        self.show_window();
        self.set_toast("🔎", "Static Zoom (F1: Help)");
    }

    pub fn enter_draw_mode(&mut self) {
        if self.live_zoom.is_active() || self.mode == AppMode::LiveZoom {
            self.live_zoom.stop();
            std::thread::sleep(std::time::Duration::from_millis(25));
            self.zoom = ZoomState::default();
        } else if self.mode == AppMode::Idle || self.background_bitmap.is_none() {
            self.zoom = ZoomState::default();
        }
        self.target_monitor_under_cursor();
        self.capture_current_screen();
        self.mode = AppMode::Draw;
        self.toolbar.active_tool = None;
        self.show_window();
        self.set_toast("✏️", "Draw Mode (F1: Help)");
    }

    pub fn enter_spotlight_mode(&mut self) {
        if self.live_zoom.is_active() || self.mode == AppMode::LiveZoom {
            self.live_zoom.stop();
            std::thread::sleep(std::time::Duration::from_millis(25));
            self.zoom = ZoomState::default();
        } else if self.mode == AppMode::Idle || self.background_bitmap.is_none() {
            self.zoom = ZoomState::default();
        }
        self.target_monitor_under_cursor();
        self.capture_current_screen();
        self.mode = AppMode::Spotlight;
        self.toolbar.active_tool = None;
        self.spotlight.active = true;
        self.spotlight.pinned = false;

        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                let screen_pt = Point2D::new((pt.x - self.screen_x) as f32, (pt.y - self.screen_y) as f32);
                self.spotlight.x = screen_pt.x;
                self.spotlight.y = screen_pt.y;
            }
        }
        self.show_window();
        self.set_toast("🔦", "Spotlight Active (Ctrl+Wheel: Resize | Space: Pin)");
    }

    pub fn enter_timer_mode(&mut self, minutes: u32) {
        if self.live_zoom.is_active() {
            self.live_zoom.stop();
        }
        self.target_monitor_under_cursor();
        self.capture_current_screen();
        self.mode = AppMode::Timer;
        self.toolbar.active_tool = None;
        let mins = if minutes > 0 { minutes } else { (self.timer_seconds / 60).max(1) };
        self.timer_seconds = mins * 60;
        self.timer_remaining = self.timer_seconds as f64;
        self.timer_paused = false;
        self.timer_alarm_sounded = false;
        self.timer_last_tick = Instant::now();
        self.show_window();
        self.set_toast("⏱️", format!("Presentation Timer: {}m (Wheel: ±1m | Ctrl+Wheel: Dim | Tab: Mini)", mins));
    }

    pub fn enter_snip_mode(&mut self) {
        if self.live_zoom.is_active() {
            self.live_zoom.stop();
        }
        let previous_tool = self.current_tool;
        self.target_monitor_under_cursor();
        self.capture_current_screen();
        self.mode = AppMode::Snip;
        self.toolbar.active_tool = None;
        self.current_tool = DrawTool::Snip;
        self.snip.shape = if previous_tool == DrawTool::Ellipse {
            SnipShape::Ellipse
        } else {
            SnipShape::Rectangle
        };
        self.show_window();
        let shape_name = if self.snip.shape == SnipShape::Ellipse { "Circular Snip" } else { "Rectangular Snip" };
        self.set_toast("✂️", format!("{} (Tab: Switch | Drag: Snip)", shape_name));
    }

    pub fn toggle_spotlight(&mut self) {
        self.spotlight.active = !self.spotlight.active;
        if self.spotlight.active {
            unsafe {
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    let screen_pt = Point2D::new((pt.x - self.screen_x) as f32, (pt.y - self.screen_y) as f32);
                    self.spotlight.x = screen_pt.x;
                    self.spotlight.y = screen_pt.y;
                }
            }
            self.set_toast("🔦", "Spotlight Enabled (Ctrl+Wheel: Resize | Space: Pin)");
        } else {
            self.set_toast("🔦", "Spotlight Disabled");
        }
        self.request_repaint();
    }

    pub fn save_config(&self) {
        let existing = crate::config::AppConfig::load();
        let cfg = crate::config::AppConfig {
            default_zoom_level: 2.0,
            spotlight_radius: self.spotlight.radius,
            default_stroke_width: self.stroke_width,
            default_color: self.current_color.name().to_string(),
            timer_duration_mins: (self.timer_seconds / 60).max(1),
            toolbar_collapsed: self.toolbar.collapsed,
            toolbar_custom_position: self.toolbar.custom_position.map(|p| (p.x, p.y)),
            monitor_target: existing.monitor_target,
            allow_monitor_cycling: existing.allow_monitor_cycling,
            default_fill_mode: match self.fill_mode {
                FillMode::Tinted => "Tinted".to_string(),
                FillMode::Solid => "Solid".to_string(),
                FillMode::None => "None".to_string(),
            },
            default_stroke_pattern: match self.stroke_pattern {
                StrokePattern::Dashed => "Dashed".to_string(),
                StrokePattern::Dotted => "Dotted".to_string(),
                StrokePattern::Solid => "Solid".to_string(),
            },
            default_badge_size: match self.badge_size {
                BadgeSize::Small => "Small".to_string(),
                BadgeSize::Large => "Large".to_string(),
                BadgeSize::ExtraLarge => "ExtraLarge".to_string(),
                BadgeSize::Medium => "Medium".to_string(),
            },
        };
        cfg.save();
    }

    pub fn exit_overlay(&mut self) {
        self.save_config();
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

    pub fn refresh_monitors(&mut self) {
        self.available_monitors = crate::monitor::MonitorManager::enumerate_monitors();
        self.toolbar.monitor_count = self.available_monitors.len();
        if self.current_monitor_index >= self.available_monitors.len() {
            self.current_monitor_index = 0;
        }
        if let Some(mon) = self.available_monitors.get(self.current_monitor_index) {
            self.current_monitor = mon.clone();
        }
    }

    pub fn set_active_monitor(&mut self, mon_idx: usize) {
        if mon_idx < self.available_monitors.len() {
            self.current_monitor_index = mon_idx;
            self.current_monitor = self.available_monitors[mon_idx].clone();
            self.toolbar.current_monitor_index = mon_idx;

            let sx = self.current_monitor.x;
            let sy = self.current_monitor.y;
            let sw = self.current_monitor.width;
            let sh = self.current_monitor.height;

            self.screen_x = sx;
            self.screen_y = sy;
            self.screen_width = sw;
            self.screen_height = sh;
            self.live_zoom.set_monitor_bounds(sx, sy, sw, sh);

            let is_visible = unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(self.hwnd).as_bool() };
            let flags = if is_visible && self.mode != AppMode::Idle && self.mode != AppMode::LiveZoom {
                SWP_SHOWWINDOW
            } else {
                windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
            };

            unsafe {
                self.renderer.resize(sw, sh);
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    sx,
                    sy,
                    sw as i32,
                    sh as i32,
                    flags,
                );
            }

            self.toolbar.update_layout(sw as f32, sh as f32);
        }
    }

    pub fn cycle_next_monitor(&mut self) {
        self.refresh_monitors();
        if self.available_monitors.len() <= 1 {
            self.set_toast("🖥️", "Single display active");
            self.request_repaint();
            return;
        }

        let next_idx = (self.current_monitor_index + 1) % self.available_monitors.len();
        self.set_active_monitor(next_idx);

        // Recapture screen for new monitor if in visual freeze modes
        if self.mode == AppMode::StaticZoom || self.mode == AppMode::Draw || self.mode == AppMode::Spotlight || self.mode == AppMode::Timer || self.mode == AppMode::Snip {
            self.capture_current_screen();
        }

        let name = self.current_monitor.name.clone();
        self.set_toast("🖥️", format!("Switched to {}", name));
        self.request_repaint();
    }

    pub fn target_monitor_under_cursor(&mut self) {
        self.refresh_monitors();
        let cursor_mon = crate::monitor::MonitorManager::get_monitor_from_cursor();
        let mut target_idx = 0;
        for (i, m) in self.available_monitors.iter().enumerate() {
            if m.x == cursor_mon.x && m.y == cursor_mon.y {
                target_idx = i;
                break;
            }
        }
        self.set_active_monitor(target_idx);
    }

    pub fn show_window(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));
            self.toolbar.update_layout(self.screen_width as f32, self.screen_height as f32);
            self.request_repaint();
        }
    }

    pub fn hide_window(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    pub fn capture_current_screen(&mut self) {
        let is_visible = unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(self.hwnd).as_bool() };
        if is_visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }

        if let Some(cap) = ScreenCapture::capture_rect(self.screen_x, self.screen_y, self.screen_width, self.screen_height) {
            if let Some(rt) = &self.renderer.render_target {
                if let Ok(bmp) = cap.create_d2d_bitmap(rt) {
                    self.background_bitmap = Some(bmp);
                }
            }
            self.background_capture = Some(cap);
        }

        if is_visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
                let _ = SetForegroundWindow(self.hwnd);
            }
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
        let prev_counter = match &shape {
            Shape::StepBadge { number, .. } => Some(*number),
            _ => None,
        };
        let action = match prev_counter {
            Some(prev) => HistoryAction::AddStepBadge {
                shape,
                prev_counter: prev,
            },
            None => HistoryAction::AddShape(shape),
        };
        self.undo_history.push(action);
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
                HistoryAction::AddStepBadge { prev_counter, .. } => {
                    if let Some(shape) = self.shapes.pop() {
                        self.redo_history.push(HistoryAction::AddStepBadge {
                            shape,
                            prev_counter,
                        });
                        self.step_counter = prev_counter;
                        self.set_toast("↩️", format!("Undo Badge (next: #{})", self.step_counter));
                    }
                }
                HistoryAction::DeleteShape { index, shape } => {
                    let insert_idx = index.min(self.shapes.len());
                    self.shapes.insert(insert_idx, shape.clone());
                    self.redo_history.push(HistoryAction::DeleteShape { index, shape });
                    self.set_toast("↩️", "Restored Erased Shape");
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
                HistoryAction::AddStepBadge { shape, prev_counter } => {
                    self.shapes.push(shape.clone());
                    self.step_counter = prev_counter + 1;
                    self.undo_history.push(HistoryAction::AddStepBadge {
                        shape,
                        prev_counter,
                    });
                    self.set_toast("↪️", format!("Redo Badge (next: #{})", self.step_counter));
                }
                HistoryAction::DeleteShape { index, shape } => {
                    if index < self.shapes.len() {
                        self.shapes.remove(index);
                    } else if !self.shapes.is_empty() {
                        self.shapes.pop();
                    }
                    self.undo_history.push(HistoryAction::DeleteShape { index, shape });
                    self.set_toast("↪️", "Re-erased Shape");
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
        if let Some(editor) = self.text_editor.take() {
            if !editor.text.trim().is_empty() {
                self.push_shape(Shape::Text {
                    origin: editor.origin,
                    text: editor.text,
                    font_size: editor.font_size,
                    color: editor.color,
                    is_bold: editor.is_bold,
                    is_italic: editor.is_italic,
                    card_style: editor.card_style,
                    font_family: editor.font_family,
                });
            }
            self.request_repaint();
        }
    }

    pub fn get_composite_capture(&self, include_spotlight: bool) -> Option<ScreenCapture> {
        let text_input = self.text_editor.as_ref();
        let bg_pixels = self.background_capture.as_ref().map(|c| c.pixels.as_slice());
        self.renderer.render_to_capture(
            self.screen_x,
            self.screen_y,
            self.screen_width,
            self.screen_height,
            bg_pixels,
            self.background_type,
            &self.zoom,
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

            let _ = std::fs::create_dir_all(&pictures_dir);

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
                        let rem = this.timer_remaining;
                        let is_overtime = rem < 0.0;
                        let abs_rem = rem.abs() as f32;
                        let mins = (abs_rem / 60.0).floor() as u32;
                        let secs = (abs_rem % 60.0).floor() as u32;
                        let progress = if is_overtime { 1.0 } else { 1.0 - (abs_rem / total).clamp(0.0, 1.0) };
                        timer_info = Some((mins, secs, progress, this.timer_paused, is_overtime));
                    }

                    let text_input = this.text_editor.as_ref();
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
                        this.show_hud,
                        timer_info,
                        &this.timer_widget,
                        &this.toolbar,
                        &this.laser_trail,
                        this.laser_pos,
                        this.eraser_pos,
                    );

                    let _ = windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
                    LRESULT(0)
                }

                windows::Win32::UI::WindowsAndMessaging::WM_ERASEBKGND => LRESULT(1),

                WM_TIMER => {
                    if wparam.0 == TIMER_ID_ANIMATION {
                        let mut needs_paint = false;

                        if this.mode == AppMode::Timer && !this.timer_paused {
                            let dt = this.timer_last_tick.elapsed().as_secs_f64();
                            this.timer_last_tick = Instant::now();
                            let prev_rem = this.timer_remaining;
                            this.timer_remaining -= dt;
                            if prev_rem > 0.0 && this.timer_remaining <= 0.0 && !this.timer_alarm_sounded {
                                this.timer_alarm_sounded = true;
                                let _ = MessageBeep(0);
                            }
                            needs_paint = true;
                        } else {
                            this.timer_last_tick = Instant::now();
                        }

                        if let Some(t) = &this.toast {
                            if !t.is_expired() {
                                needs_paint = true;
                            }
                        }

                        if this.text_editor.is_some() {
                            needs_paint = true;
                        }

                        // Laser pointer trail decay
                        if !this.laser_trail.is_empty() {
                            let now = Instant::now();
                            let prev_len = this.laser_trail.len();
                            this.laser_trail.retain(|p| now.duration_since(p.timestamp).as_secs_f32() <= 1.2);
                            if !this.laser_trail.is_empty() || prev_len > 0 {
                                needs_paint = true;
                            }
                        }

                        // Hold-to-snap smart shape dwell detection (350ms dwell)
                        if this.is_drawing && this.current_tool == DrawTool::Pen {
                            if let Some(dwell_t) = this.last_mouse_dwell_time {
                                if dwell_t.elapsed().as_millis() >= 350 {
                                    if let Some(Shape::Stroke { points, width, color, .. }) = &this.active_shape {
                                        if let Some(smart_shape) = recognize_smart_shape(points, *width, *color) {
                                            this.active_shape = Some(smart_shape);
                                            this.last_mouse_dwell_time = None;
                                            this.set_toast("✨", "Auto-snapped Shape");
                                            needs_paint = true;
                                        }
                                    }
                                }
                            }
                        }

                        if needs_paint {
                            this.request_repaint();
                        }
                    }
                    LRESULT(0)
                }

                WM_SETCURSOR => {
                    if this.mode == AppMode::Timer {
                        if this.timer_widget.hover_action.is_some() {
                            let _ = SetCursor(Some(LoadCursorW(None, IDC_HAND).unwrap_or_default()));
                        } else if this.timer_widget.is_dragging {
                            let _ = SetCursor(Some(LoadCursorW(None, IDC_SIZEALL).unwrap_or_default()));
                        } else {
                            let _ = SetCursor(Some(LoadCursorW(None, IDC_ARROW).unwrap_or_default()));
                        }
                        return LRESULT(1);
                    }
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

                    // Timer dragging & button hover hit testing
                    if this.mode == AppMode::Timer {
                        if this.timer_widget.is_dragging {
                            let dx = screen_pt.x - this.timer_widget.drag_start_mouse.x;
                            let dy = screen_pt.y - this.timer_widget.drag_start_mouse.y;
                            this.timer_widget.custom_pos = Some(Point2D::new(
                                this.timer_widget.drag_start_pos.x + dx,
                                this.timer_widget.drag_start_pos.y + dy,
                            ));
                            this.request_repaint();
                            return LRESULT(0);
                        }
                        let prev_hover = this.timer_widget.hover_action;
                        this.timer_widget.hover_action = this.timer_widget.get_action_at(screen_pt, sw, sh);
                        if this.timer_widget.hover_action != prev_hover {
                            this.request_repaint();
                        }
                        return LRESULT(0);
                    }

                    // Toolbar dragging
                    if this.toolbar.is_dragging {
                        let dx = screen_pt.x - this.toolbar.drag_start_mouse.x;
                        let dy = screen_pt.y - this.toolbar.drag_start_mouse.y;
                        this.toolbar.custom_position = Some(Point2D::new(
                            this.toolbar.drag_start_bar.x + dx,
                            this.toolbar.drag_start_bar.y + dy,
                        ));
                        this.toolbar.update_layout(sw, sh);
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    // Toolbar hover update & hit testing
                    let hover = this.toolbar.hit_test(x, y);
                    if hover != this.toolbar.hover_action {
                        this.toolbar.hover_action = hover;
                        this.request_repaint();
                    }
                    if this.toolbar.is_point_inside(x, y) {
                        return LRESULT(0);
                    }

                    // Laser pointer tracking
                    if this.current_tool == DrawTool::LaserPointer {
                        this.laser_pos = Some(canvas_pt);
                        this.laser_trail.push(LaserTrailPoint {
                            pt: canvas_pt,
                            timestamp: Instant::now(),
                        });
                        this.request_repaint();
                    } else {
                        this.laser_pos = None;
                    }

                    // Eraser cursor tracking
                    if this.current_tool == DrawTool::Eraser {
                        this.eraser_pos = Some(screen_pt);
                        this.request_repaint();
                    } else {
                        this.eraser_pos = None;
                    }

                    // Eraser execution when dragging
                    if this.is_drawing && this.current_tool == DrawTool::Eraser {
                        let mut erased_idx = None;
                        for (idx, shape) in this.shapes.iter().enumerate().rev() {
                            if shape_intersects_circle(shape, canvas_pt, 16.0) {
                                erased_idx = Some(idx);
                                break;
                            }
                        }
                        if let Some(idx) = erased_idx {
                            let erased_shape = this.shapes.remove(idx);
                            this.undo_history.push(HistoryAction::DeleteShape { index: idx, shape: erased_shape });
                            this.redo_history.clear();
                            this.set_toast("🧹", "Erased Shape");
                            this.request_repaint();
                        }
                        return LRESULT(0);
                    }

                    if this.spotlight.active && !this.spotlight.pinned {
                        this.spotlight.x = x;
                        this.spotlight.y = y;
                        this.request_repaint();
                    }

                    if this.snip.active {
                        let is_shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                        this.snip.current = if is_shift {
                            snap_to_square(this.snip.start, screen_pt)
                        } else {
                            screen_pt
                        };
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if (this.mode == AppMode::StaticZoom || (this.mode == AppMode::Draw && this.zoom.level > 1.001)) && !this.is_drawing && !this.zoom.is_dragging {
                        this.zoom.update_target_from_cursor(x, y, sw, sh);
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
                        if this.current_tool == DrawTool::Pen {
                            if screen_pt.distance(&this.last_mouse_pos) > 5.0 {
                                this.last_mouse_pos = screen_pt;
                                this.last_mouse_dwell_time = Some(Instant::now());
                            }
                        }

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

                    // If clicked on toolbar grip handle, start dragging toolbar
                    if this.toolbar.hit_test_grip(x, y) {
                        this.toolbar.is_dragging = true;
                        this.toolbar.drag_start_mouse = screen_pt;
                        this.toolbar.drag_start_bar = Point2D::new(this.toolbar.bar_rect.left, this.toolbar.bar_rect.top);
                        return LRESULT(0);
                    }

                    // If clicked on Fluent toolbar items, execute action without drawing
                    if this.toolbar.is_point_inside(x, y) {
                        if let Some(action) = this.toolbar.hit_test(x, y) {
                            match action {
                                FluentAction::ModeZoom => {
                                    this.mode = AppMode::StaticZoom;
                                    this.toolbar.active_tool = None;
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("🔎", "Static Zoom (Wheel: Zoom | Drag: Pan)");
                                }
                                FluentAction::ModeDraw => {
                                    this.mode = AppMode::Draw;
                                    this.toolbar.active_tool = None;
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("✏️", "Draw Mode Active");
                                }
                                FluentAction::ModeSpotlight => {
                                    this.toolbar.active_tool = None;
                                    this.toggle_spotlight();
                                }
                                FluentAction::ModeTimer => {
                                    this.enter_timer_mode(0);
                                }
                                FluentAction::ModeSnip => {
                                    this.enter_snip_mode();
                                }
                                FluentAction::CycleDisplay => {
                                    this.cycle_next_monitor();
                                }
                                FluentAction::Tool(t) => {
                                    this.current_tool = t;
                                    this.mode = AppMode::Draw;
                                    if this.toolbar.active_tool == Some(t) {
                                        this.toolbar.active_tool = None;
                                    } else {
                                        this.toolbar.active_tool = Some(t);
                                    }
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("🛠️", format!("Tool: {}", t.name()));
                                }
                                FluentAction::Color(c) => {
                                    this.current_color = c;
                                    this.set_toast("🎨", format!("Color: {}", c.name()));
                                }
                                FluentAction::Undo => {
                                    this.undo();
                                }
                                FluentAction::Clear => {
                                    this.clear_all();
                                }
                                FluentAction::Copy => {
                                    this.copy_screen_to_clipboard();
                                }
                                FluentAction::Save => {
                                    this.save_snapshot();
                                }
                                FluentAction::Close => {
                                    this.exit_overlay();
                                    return LRESULT(0);
                                }
                                FluentAction::ToggleCollapse => {
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.collapsed = !this.toolbar.collapsed;
                                    this.toolbar.update_layout(sw, sh);
                                }
                                FluentAction::SetStrokeWidth(w) => {
                                    this.stroke_width = w;
                                    this.toolbar.stroke_width = w;
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.save_config();
                                    this.set_toast("✏️", format!("Stroke Width: {:.0}px", w));
                                }
                                FluentAction::SetFillMode(fm) => {
                                    this.fill_mode = fm;
                                    this.toolbar.current_fill_mode = fm;
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("🎨", format!("Fill: {}", fm.name()));
                                }
                                FluentAction::SetStrokePattern(sp) => {
                                    this.stroke_pattern = sp;
                                    this.toolbar.current_stroke_pattern = sp;
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("✏️", format!("Pattern: {}", sp.name()));
                                }
                                FluentAction::SetArrowStyle(as_) => {
                                    this.arrow_style = as_;
                                    this.toolbar.current_arrow_style = as_;
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("🏹", format!("Arrow: {}", as_.name()));
                                }
                                FluentAction::SetBadgeSize(bs) => {
                                    this.badge_size = bs;
                                    this.toolbar.current_badge_size = bs;
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("🔢", format!("Badge Size: {}", bs.name()));
                                }
                                FluentAction::SetBadgeShape(bsh) => {
                                    this.badge_shape = bsh;
                                    this.toolbar.current_badge_shape = bsh;
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("🔢", format!("Badge Shape: {}", bsh.name()));
                                }
                                FluentAction::ResetBadgeCounter => {
                                    this.step_counter = 1;
                                    this.toolbar.badge_counter = 1;
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("↺", "Step badge reset to #1");
                                }
                                FluentAction::SetFontSize(sz) => {
                                    this.font_size = sz;
                                    this.toolbar.current_font_size = sz;
                                    if let Some(ed) = &mut this.text_editor {
                                        ed.font_size = sz;
                                    }
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("🔤", format!("Font Size: {:.0}px", sz));
                                }
                                FluentAction::ToggleBold => {
                                    this.text_is_bold = !this.text_is_bold;
                                    let bold = this.text_is_bold;
                                    this.toolbar.text_is_bold = bold;
                                    if let Some(ed) = &mut this.text_editor {
                                        ed.is_bold = bold;
                                    }
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("𝐁", if bold { "Bold: On" } else { "Bold: Off" });
                                }
                                FluentAction::ToggleItalic => {
                                    this.text_is_italic = !this.text_is_italic;
                                    let italic = this.text_is_italic;
                                    this.toolbar.text_is_italic = italic;
                                    if let Some(ed) = &mut this.text_editor {
                                        ed.is_italic = italic;
                                    }
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("𝐼", if italic { "Italic: On" } else { "Italic: Off" });
                                }
                                FluentAction::SetTextCardStyle(cs) => {
                                    this.text_card_style = cs;
                                    this.toolbar.text_card_style = cs;
                                    if let Some(ed) = &mut this.text_editor {
                                        ed.card_style = cs;
                                    }
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("🏷️", format!("Text Card: {}", cs.name()));
                                }
                                FluentAction::SetFontFamily(ff) => {
                                    this.text_font_family = ff;
                                    this.toolbar.text_font_family = ff;
                                    if let Some(ed) = &mut this.text_editor {
                                        ed.font_family = ff;
                                    }
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("🔤", format!("Font: {}", ff.name()));
                                }
                            }
                            this.request_repaint();
                        }
                        return LRESULT(0);
                    }

                    let is_shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                    let is_ctrl = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
                    let is_tab = (GetKeyState(VK_TAB.0 as i32) as u16 & 0x8000) != 0;

                    // If in Timer mode, handle button clicks or clock face clicks or card dragging
                    if this.mode == AppMode::Timer {
                        let sw = this.screen_width as f32;
                        let sh = this.screen_height as f32;
                        if let Some(action) = this.timer_widget.get_action_at(screen_pt, sw, sh) {
                            match action {
                                TimerAction::SetDuration(mins) => {
                                    this.enter_timer_mode(mins);
                                }
                                TimerAction::CycleCorner => {
                                    this.timer_widget.pill_corner = (this.timer_widget.pill_corner + 1) % 4;
                                    let corner_name = match this.timer_widget.pill_corner {
                                        1 => "Top-Left",
                                        2 => "Bottom-Left",
                                        3 => "Bottom-Right",
                                        _ => "Top-Right",
                                    };
                                    this.set_toast("🔄", format!("Mini-pill docked to {}", corner_name));
                                }
                                TimerAction::PlayPause => {
                                    let paused = !this.timer_paused;
                                    this.timer_paused = paused;
                                    let label = if paused { "Timer Paused" } else { "Timer Resumed" };
                                    this.set_toast("⏱️", label);
                                }
                                TimerAction::AddMinute => {
                                    this.timer_remaining += 60.0;
                                    this.timer_seconds = this.timer_remaining.round() as u32;
                                    let m = (this.timer_remaining.abs() / 60.0).floor() as u32;
                                    this.set_toast("⏱️", format!("Timer: +1m ({}m total)", m));
                                }
                                TimerAction::SubMinute => {
                                    this.timer_remaining = (this.timer_remaining - 60.0).max(10.0);
                                    this.timer_seconds = this.timer_remaining.round() as u32;
                                    let m = (this.timer_remaining.abs() / 60.0).floor() as u32;
                                    this.set_toast("⏱️", format!("Timer: -1m ({}m total)", m));
                                }
                                TimerAction::Reset => {
                                    this.timer_remaining = this.timer_seconds as f64;
                                    this.timer_paused = false;
                                    this.timer_alarm_sounded = false;
                                    this.set_toast("⏱️", "Timer Reset");
                                }
                                TimerAction::ToggleMinimize => {
                                    this.timer_widget.minimized = !this.timer_widget.minimized;
                                    let label = if this.timer_widget.minimized { "Timer Minimized to Corner Pill" } else { "Timer Expanded" };
                                    this.set_toast("⏱️", label);
                                }
                                TimerAction::Close => {
                                    this.exit_overlay();
                                    return LRESULT(0);
                                }
                            }
                            this.request_repaint();
                        } else if !this.timer_widget.minimized {
                            let (cl, ct, cr, cb, cx, cy) = this.timer_widget.get_card_bounds(sw, sh);
                            if screen_pt.x >= cl && screen_pt.x <= cr && screen_pt.y >= ct && screen_pt.y <= cb {
                                this.timer_widget.is_dragging = true;
                                this.timer_widget.drag_start_mouse = screen_pt;
                                this.timer_widget.drag_start_pos = Point2D::new(cx, cy);
                            }
                        }
                        return LRESULT(0);
                    }

                    // If in Spotlight mode, clicking toggles pinning in place
                    if this.mode == AppMode::Spotlight {
                        let pinned = !this.spotlight.pinned;
                        this.spotlight.pinned = pinned;
                        this.set_toast("🔦", if pinned { "Spotlight Pinned (Click to unpin)" } else { "Spotlight Following Cursor" });
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    // If in Static Zoom mode, clicking transitions into Draw mode
                    if this.mode == AppMode::StaticZoom {
                        this.mode = AppMode::Draw;
                        this.set_toast("✏️", "Draw Mode (Drag to draw | Esc to exit)");
                    }

                    if this.current_tool == DrawTool::Snip {
                        this.snip.active = true;
                        this.snip.start = screen_pt;
                        this.snip.current = screen_pt;
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.current_tool == DrawTool::Text {
                        this.commit_text_editor();
                        let cur_col = this.current_color;
                        let cur_sz = this.font_size;
                        let is_bold = this.text_is_bold;
                        let is_italic = this.text_is_italic;
                        let card_style = this.text_card_style;
                        let font_family = this.text_font_family;
                        this.text_editor = Some(TextEditorState::new(
                            canvas_pt,
                            cur_col,
                            cur_sz,
                            is_bold,
                            is_italic,
                            card_style,
                            font_family,
                        ));
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.current_tool == DrawTool::StepBadge {
                        let num = this.step_counter;
                        let rad = this.badge_size.radius();
                        let col = this.current_color;
                        let bshape = this.badge_shape;
                        let bfill = this.fill_mode;
                        let bwidth = this.stroke_width;
                        let bpattern = this.stroke_pattern;
                        this.push_shape(Shape::StepBadge {
                            center: canvas_pt,
                            number: num,
                            radius: rad,
                            color: col,
                            shape: bshape,
                            fill: bfill,
                            stroke_width: bwidth,
                            pattern: bpattern,
                        });
                        this.step_counter += 1;
                        this.toolbar.badge_counter = this.step_counter;
                        let sw = this.screen_width as f32;
                        let sh = this.screen_height as f32;
                        this.toolbar.update_layout(sw, sh);
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.current_tool == DrawTool::LaserPointer {
                        this.laser_pos = Some(canvas_pt);
                        this.laser_trail.push(LaserTrailPoint {
                            pt: canvas_pt,
                            timestamp: Instant::now(),
                        });
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.current_tool == DrawTool::Eraser {
                        this.is_drawing = true;
                        let mut erased_idx = None;
                        for (idx, shape) in this.shapes.iter().enumerate().rev() {
                            if shape_intersects_circle(shape, canvas_pt, 16.0) {
                                erased_idx = Some(idx);
                                break;
                            }
                        }
                        if let Some(idx) = erased_idx {
                            let erased_shape = this.shapes.remove(idx);
                            this.undo_history.push(HistoryAction::DeleteShape { index: idx, shape: erased_shape });
                            this.redo_history.clear();
                            this.set_toast("🧹", "Erased Shape");
                        }
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    this.stroke_start_time = Some(Instant::now());
                    this.last_mouse_dwell_time = Some(Instant::now());
                    this.last_mouse_pos = screen_pt;

                    this.is_drawing = true;
                    this.draw_start_pt = canvas_pt;

                    // ZoomIt Standard Modifiers:
                    // Dedicated tool vs Pen quick-draw modifiers
                    let shape_to_create = if this.current_tool == DrawTool::Pen {
                        // Mouse modifiers for drawing shapes on-the-fly from Pen tool (Sysinternals ZoomIt standard):
                        // Shift+Ctrl = Arrow, Shift = Line, Ctrl = Rectangle, Tab = Ellipse
                        if is_shift && is_ctrl {
                            Shape::Arrow {
                                start: canvas_pt,
                                end: canvas_pt,
                                color: this.current_color,
                                width: this.stroke_width,
                                style: this.arrow_style,
                                pattern: this.stroke_pattern,
                            }
                        } else if is_shift && !is_ctrl {
                            Shape::Line {
                                start: canvas_pt,
                                end: canvas_pt,
                                color: this.current_color,
                                width: this.stroke_width,
                                pattern: this.stroke_pattern,
                            }
                        } else if is_ctrl && !is_shift {
                            Shape::Rectangle {
                                start: canvas_pt,
                                end: canvas_pt,
                                color: this.current_color,
                                width: this.stroke_width,
                                rounded: false,
                                fill: this.fill_mode,
                                pattern: this.stroke_pattern,
                            }
                        } else if is_tab {
                            Shape::Ellipse {
                                start: canvas_pt,
                                end: canvas_pt,
                                color: this.current_color,
                                width: this.stroke_width,
                                fill: this.fill_mode,
                                pattern: this.stroke_pattern,
                            }
                        } else {
                            Shape::Stroke {
                                points: vec![canvas_pt],
                                color: this.current_color,
                                width: this.stroke_width,
                                is_highlighter: false,
                                pattern: this.stroke_pattern,
                            }
                        }
                    } else {
                        match this.current_tool {
                            DrawTool::Pen => unreachable!(),
                            DrawTool::Highlighter => Shape::Stroke {
                                points: vec![canvas_pt],
                                color: this.current_color,
                                width: this.stroke_width,
                                is_highlighter: true,
                                pattern: StrokePattern::Solid,
                            },
                            DrawTool::Line => Shape::Line {
                                start: canvas_pt,
                                end: canvas_pt,
                                color: this.current_color,
                                width: this.stroke_width,
                                pattern: this.stroke_pattern,
                            },
                            DrawTool::Arrow => Shape::Arrow {
                                start: canvas_pt,
                                end: canvas_pt,
                                color: this.current_color,
                                width: this.stroke_width,
                                style: this.arrow_style,
                                pattern: this.stroke_pattern,
                            },
                            DrawTool::Rectangle => Shape::Rectangle {
                                start: canvas_pt,
                                end: canvas_pt,
                                color: this.current_color,
                                width: this.stroke_width,
                                rounded: false,
                                fill: this.fill_mode,
                                pattern: this.stroke_pattern,
                            },
                            DrawTool::RoundedRectangle => Shape::Rectangle {
                                start: canvas_pt,
                                end: canvas_pt,
                                color: this.current_color,
                                width: this.stroke_width,
                                rounded: true,
                                fill: this.fill_mode,
                                pattern: this.stroke_pattern,
                            },
                            DrawTool::Ellipse => Shape::Ellipse {
                                start: canvas_pt,
                                end: canvas_pt,
                                color: this.current_color,
                                width: this.stroke_width,
                                fill: this.fill_mode,
                                pattern: this.stroke_pattern,
                            },
                            _ => Shape::Stroke {
                                points: vec![canvas_pt],
                                color: this.current_color,
                                width: this.stroke_width,
                                is_highlighter: false,
                                pattern: this.stroke_pattern,
                            },
                        }
                    };

                    this.active_shape = Some(shape_to_create);
                    this.request_repaint();
                    LRESULT(0)
                }

                WM_LBUTTONUP => {
                    if this.toolbar.is_dragging {
                        this.toolbar.is_dragging = false;
                        return LRESULT(0);
                    }

                    if this.timer_widget.is_dragging {
                        this.timer_widget.is_dragging = false;
                        return LRESULT(0);
                    }

                    if this.current_tool == DrawTool::Eraser {
                        this.is_drawing = false;
                        return LRESULT(0);
                    }

                    if this.current_tool == DrawTool::LaserPointer {
                        return LRESULT(0);
                    }

                    this.stroke_start_time = None;
                    this.last_mouse_dwell_time = None;

                    if this.snip.active {
                        this.snip.active = false;
                        let (l, t, r, b) = this.snip.rect();
                        let w = (r - l).round() as u32;
                        let h = (b - t).round() as u32;

                        if w > 4 && h > 4 {
                            if let Some(composite) = this.get_composite_capture(false) {
                                if let Some(mut cropped) = composite.crop(l as i32 + this.screen_x, t as i32 + this.screen_y, w, h) {
                                    if this.snip.shape == SnipShape::Ellipse {
                                        let cx = w as f32 / 2.0;
                                        let cy = h as f32 / 2.0;
                                        let rx = cx;
                                        let ry = cy;
                                        for py in 0..h {
                                            for px in 0..w {
                                                let dx = px as f32 - cx;
                                                let dy = py as f32 - cy;
                                                if (dx * dx) / (rx * rx) + (dy * dy) / (ry * ry) > 1.0 {
                                                    let idx = ((py * w + px) * 4 + 3) as usize;
                                                    if idx < cropped.pixels.len() {
                                                        cropped.pixels[idx] = 0;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    if copy_bgra_to_clipboard(cropped.width, cropped.height, &cropped.pixels) {
                                        let label = if this.snip.shape == SnipShape::Ellipse { "Circular Snip" } else { "Snip" };
                                        this.set_toast("✂️", format!("{} {}×{} px copied to Clipboard!", label, w, h));
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
                            let should_keep = match &shape {
                                Shape::Stroke { points, .. } => points.len() > 1,
                                Shape::Line { start, end, .. } => start.distance(end) > 2.0,
                                Shape::Arrow { start, end, .. } => start.distance(end) > 2.0,
                                Shape::Rectangle { start, end, .. } => start.distance(end) > 2.0,
                                Shape::Ellipse { start, end, .. } => start.distance(end) > 2.0,
                                _ => true,
                            };
                            if should_keep {
                                this.push_shape(shape);
                            }
                        }
                        this.request_repaint();
                    }
                    LRESULT(0)
                }

                WM_RBUTTONDOWN => {
                    // If text editor is active, right click cancels text editor
                    if this.text_editor.is_some() {
                        this.text_editor = None;
                        this.request_repaint();
                        return LRESULT(0);
                    }
                    // If drawing in progress, right click cancels active drawing
                    if this.is_drawing {
                        this.is_drawing = false;
                        this.active_shape = None;
                        this.request_repaint();
                        return LRESULT(0);
                    }
                    // If snip in progress, cancel snip
                    if this.snip.active {
                        this.snip.active = false;
                        this.request_repaint();
                        return LRESULT(0);
                    }
                    // If cheat sheet open, close cheat sheet
                    if this.show_cheat_sheet {
                        this.show_cheat_sheet = false;
                        this.request_repaint();
                        return LRESULT(0);
                    }
                    // Two-stage exit: If in Draw mode, return to StaticZoom (Pan & Zoom) mode!
                    if this.mode == AppMode::Draw {
                        this.mode = AppMode::StaticZoom;
                        this.set_toast("🔎", "Pan & Zoom Mode (Right-click again to exit)");
                        this.request_repaint();
                        return LRESULT(0);
                    }
                    // Otherwise exit overlay completely
                    this.exit_overlay();
                    LRESULT(0)
                }

                WM_RBUTTONUP => LRESULT(0),

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
                    let is_shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;

                    // 0. Presentation Timer mode controls (Wheel: ±1m, Ctrl+Wheel: Dimming)
                    if this.mode == AppMode::Timer {
                        if is_ctrl {
                            let new_dim = (this.timer_widget.dim_opacity + delta * 0.05).clamp(0.15, 0.95);
                            this.timer_widget.dim_opacity = new_dim;
                            this.set_toast("🌓", format!("Background Dim: {:.0}%", new_dim * 100.0));
                        } else {
                            let change = if delta > 0.0 { 60.0 } else { -60.0 };
                            this.timer_remaining += change;
                            this.timer_seconds = this.timer_remaining.abs().round() as u32;
                            let abs_m = (this.timer_remaining.abs() / 60.0).floor() as u32;
                            let sign = if this.timer_remaining < 0.0 { "-" } else { "" };
                            this.set_toast("⏱️", format!("Timer: {}{}m", sign, abs_m));
                        }
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    // 1. Ctrl + Wheel: Always resizes the Spotlight / Flashlight
                    if is_ctrl {
                        let old_r = this.spotlight.radius;
                        let new_r = (old_r + delta * 20.0).clamp(40.0, 800.0);
                        this.spotlight.radius = new_r;
                        let diam = (new_r * 2.0).round() as u32;
                        this.set_toast("🔦", format!("Spotlight ⌀{} px", diam));
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    // 2. Shift + Wheel: Adjusts brush stroke width in Draw / Snip mode
                    if is_shift && (this.mode == AppMode::Draw || this.mode == AppMode::Snip) {
                        let new_width = (this.stroke_width + delta * 1.5).clamp(1.0, 40.0);
                        this.stroke_width = new_width;
                        let w_val = new_width.round() as u32;
                        this.set_toast("🖌️", format!("Stroke Width {} px", w_val));
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    // 3. Normal Wheel: ALWAYS ZOOMS in Freeze modes (StaticZoom, Draw, Spotlight)!
                    if this.mode == AppMode::StaticZoom || this.mode == AppMode::Draw || this.mode == AppMode::Spotlight {
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

                    // 4. Live Zoom
                    if this.mode == AppMode::LiveZoom {
                        this.live_zoom.adjust_zoom(delta * 0.25);
                        let z_val = this.live_zoom.zoom_level();
                        this.set_toast("🔍", format!("Live Zoom {:.2}x", z_val));
                        return LRESULT(0);
                    }

                    LRESULT(0)
                }

                WM_CHAR => {
                    let ch = char::from_u32(wparam.0 as u32).unwrap_or('\0');
                    if let Some(editor) = &mut this.text_editor {
                        if ch == '\x08' || ch == '\x1b' || ch == '\x7f' {
                            // Handled in WM_KEYDOWN
                        } else if ch == '\r' || ch == '\n' {
                            this.commit_text_editor();
                        } else if !ch.is_control() {
                            editor.insert_char(ch);
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
                    if this.text_editor.is_some() {
                        if key == VK_ESCAPE.0 as i32 {
                            this.text_editor = None;
                            this.request_repaint();
                            return LRESULT(0);
                        } else if key == VK_RETURN.0 as i32 {
                            this.commit_text_editor();
                            return LRESULT(0);
                        } else if is_ctrl && key == 'B' as i32 {
                            this.text_is_bold = !this.text_is_bold;
                            let bold = this.text_is_bold;
                            this.toolbar.text_is_bold = bold;
                            if let Some(ed) = &mut this.text_editor {
                                ed.is_bold = bold;
                            }
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("𝐁", if bold { "Bold: On" } else { "Bold: Off" });
                            this.request_repaint();
                            return LRESULT(0);
                        } else if is_ctrl && key == 'I' as i32 {
                            this.text_is_italic = !this.text_is_italic;
                            let italic = this.text_is_italic;
                            this.toolbar.text_is_italic = italic;
                            if let Some(ed) = &mut this.text_editor {
                                ed.is_italic = italic;
                            }
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("𝐼", if italic { "Italic: On" } else { "Italic: Off" });
                            this.request_repaint();
                            return LRESULT(0);
                        } else if key == VK_UP.0 as i32 {
                            let new_sz = if let Some(ed) = &mut this.text_editor {
                                ed.font_size = (ed.font_size + 4.0).min(96.0);
                                ed.font_size
                            } else { 22.0 };
                            this.font_size = new_sz;
                            this.toolbar.current_font_size = new_sz;
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.request_repaint();
                            return LRESULT(0);
                        } else if key == VK_DOWN.0 as i32 {
                            let new_sz = if let Some(ed) = &mut this.text_editor {
                                ed.font_size = (ed.font_size - 4.0).max(12.0);
                                ed.font_size
                            } else { 22.0 };
                            this.font_size = new_sz;
                            this.toolbar.current_font_size = new_sz;
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.request_repaint();
                            return LRESULT(0);
                        } else if let Some(editor) = &mut this.text_editor {
                            if key == VK_BACK.0 as i32 {
                                editor.backspace();
                            } else if key == VK_DELETE.0 as i32 {
                                editor.delete_forward();
                            } else if key == VK_LEFT.0 as i32 {
                                editor.move_left();
                            } else if key == VK_RIGHT.0 as i32 {
                                editor.move_right();
                            } else if key == VK_HOME.0 as i32 {
                                editor.cursor = 0;
                            } else if key == VK_END.0 as i32 {
                                editor.cursor = editor.text.len();
                            } else if is_ctrl && key == 'V' as i32 {
                                if let Some(clip_text) = get_clipboard_text() {
                                    editor.insert_str(&clip_text);
                                }
                            }
                            this.request_repaint();
                            return LRESULT(0);
                        }
                        return LRESULT(0);
                    }

                    // ── Ctrl combos (always take priority) ──
                    if is_ctrl {
                        match key {
                            k if k == '1' as i32 => {
                                if this.mode == AppMode::StaticZoom {
                                    this.exit_overlay();
                                } else {
                                    this.enter_static_zoom();
                                }
                            }
                            k if k == '2' as i32 => {
                                if this.mode == AppMode::Draw {
                                    this.exit_overlay();
                                } else {
                                    this.enter_draw_mode();
                                }
                            }
                            k if k == '3' as i32 => {
                                if this.mode == AppMode::Spotlight {
                                    this.exit_overlay();
                                } else {
                                    this.toggle_spotlight();
                                }
                            }
                            k if k == '4' as i32 => {
                                this.exit_overlay();
                                this.enter_live_zoom();
                            }
                            k if k == '5' as i32 => {
                                if this.mode == AppMode::Timer {
                                    this.exit_overlay();
                                } else {
                                    this.enter_timer_mode(0);
                                }
                            }
                            k if k == 'Z' as i32 && !is_shift => { this.undo(); }
                            k if k == 'Z' as i32 && is_shift => { this.redo(); }
                            k if k == 'Y' as i32 => { this.redo(); }
                            k if k == 'C' as i32 => { this.copy_screen_to_clipboard(); }
                            k if k == 'S' as i32 && !is_shift => { this.save_snapshot(); }
                            k if k == 'S' as i32 && is_shift => { this.enter_snip_mode(); }
                            k if k == VK_TAB.0 as i32 => { this.cycle_next_monitor(); }
                            _ => {}
                        }
                        return LRESULT(0);
                    }

                    // ── Non-Ctrl keys ──
                    match key {
                        // ─── Navigation / System ───
                        k if k == VK_ESCAPE.0 as i32 => {
                            if this.show_cheat_sheet {
                                this.show_cheat_sheet = false;
                                this.request_repaint();
                            } else if this.active_shape.is_some() {
                                this.active_shape = None;
                                this.is_drawing = false;
                                this.request_repaint();
                            } else if this.snip.active {
                                this.snip.active = false;
                                this.request_repaint();
                            } else {
                                this.exit_overlay();
                            }
                        }

                        k if k == VK_F1.0 as i32 => {
                            this.show_cheat_sheet = !this.show_cheat_sheet;
                            this.request_repaint();
                        }

                        k if k == VK_F2.0 as i32 => {
                            let show = !this.toolbar.visible;
                            this.toolbar.visible = show;
                            this.show_hud = show;
                            this.set_toast("🖥️", if show { "Toolbar Visible" } else { "Toolbar Hidden" });
                            this.request_repaint();
                        }

                        k if k == windows::Win32::UI::Input::KeyboardAndMouse::VK_F3.0 as i32 => {
                            this.toggle_spotlight();
                        }

                        k if k == windows::Win32::UI::Input::KeyboardAndMouse::VK_F4.0 as i32 => {
                            this.cycle_next_monitor();
                        }

                        k if k == VK_TAB.0 as i32 => {
                            if this.mode == AppMode::Timer {
                                this.timer_widget.minimized = !this.timer_widget.minimized;
                                let label = if this.timer_widget.minimized { "Timer Minimized to Corner Pill" } else { "Timer Expanded" };
                                this.set_toast("⏱️", label);
                                this.request_repaint();
                                return LRESULT(0);
                            } else if this.mode == AppMode::Snip || this.snip.active {
                                this.snip.shape = if this.snip.shape == SnipShape::Rectangle {
                                    SnipShape::Ellipse
                                } else {
                                    SnipShape::Rectangle
                                };
                                let label = if this.snip.shape == SnipShape::Ellipse { "Circular Snip" } else { "Rectangular Snip" };
                                this.set_toast("✂️", label);
                                this.request_repaint();
                                return LRESULT(0);
                            }
                        }

                        k if k == VK_SPACE.0 as i32 => {
                            if this.mode == AppMode::Timer {
                                if this.timer_remaining <= 0.0 {
                                    this.timer_remaining = this.timer_seconds as f64;
                                    this.timer_paused = false;
                                    this.timer_alarm_sounded = false;
                                    this.set_toast("⏱️", "Timer Reset");
                                } else {
                                    let paused = !this.timer_paused;
                                    this.timer_paused = paused;
                                    this.set_toast("⏱️", if paused { "Timer Paused" } else { "Timer Resumed" });
                                }
                                this.request_repaint();
                            } else if this.mode == AppMode::Spotlight {
                                let pinned = !this.spotlight.pinned;
                                this.spotlight.pinned = pinned;
                                this.set_toast("🔦", if pinned { "Spotlight Pinned" } else { "Spotlight Following" });
                                this.request_repaint();
                            } else {
                                if this.mode == AppMode::Draw {
                                    this.mode = AppMode::StaticZoom;
                                    this.toolbar.active_tool = None;
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("🔎", "Pan & Zoom Mode");
                                } else if this.mode == AppMode::StaticZoom {
                                    this.mode = AppMode::Draw;
                                    this.toolbar.active_tool = None;
                                    let sw = this.screen_width as f32;
                                    let sh = this.screen_height as f32;
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("✏️", "Draw Mode");
                                }
                                this.request_repaint();
                            }
                        }

                        // ─── Colors (single key, no modifier = color) ───
                        k if k == 'R' as i32 && !is_shift => {
                            if this.mode == AppMode::Timer {
                                this.timer_remaining = this.timer_seconds as f64;
                                this.timer_paused = false;
                                this.timer_alarm_sounded = false;
                                this.set_toast("⏱️", "Timer Reset");
                                this.request_repaint();
                                return LRESULT(0);
                            }
                            if this.current_tool == DrawTool::StepBadge {
                                this.step_counter = 1;
                                this.toolbar.badge_counter = 1;
                                let sw = this.screen_width as f32;
                                let sh = this.screen_height as f32;
                                this.toolbar.update_layout(sw, sh);
                                this.set_toast("↺", "Step badge reset to #1");
                                this.request_repaint();
                                return LRESULT(0);
                            }
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

                        // ─── Canvas Slate & Color Modes (Shift+W: White pen, Shift+K: Black pen) ───
                        k if k == 'W' as i32 && is_shift => {
                            this.current_color = ColorPreset::White;
                            this.set_toast("⚪", "White Pen");
                            this.request_repaint();
                        }
                        k if k == 'W' as i32 && !is_shift => {
                            this.background_type = if this.background_type == CanvasBackground::Whiteboard {
                                CanvasBackground::Transparent
                            } else {
                                CanvasBackground::Whiteboard
                            };
                            this.current_color = ColorPreset::Red;
                            this.set_toast("⚪", "Whiteboard");
                            this.request_repaint();
                        }
                        k if k == 'K' as i32 && is_shift => {
                            this.current_color = ColorPreset::Black;
                            this.set_toast("⚫", "Black Pen");
                            this.request_repaint();
                        }
                        k if k == 'K' as i32 && is_shift => {
                            this.background_type = if this.background_type == CanvasBackground::Blackboard {
                                CanvasBackground::Transparent
                            } else {
                                CanvasBackground::Blackboard
                            };
                            this.current_color = ColorPreset::White;
                            this.set_toast("⚫", "Blackboard");
                            this.request_repaint();
                        }
                        k if k == 'K' as i32 && !is_shift => {
                            this.current_tool = DrawTool::LaserPointer;
                            this.set_toast("🔴", "Laser Pointer");
                            this.request_repaint();
                        }

                        // ─── Drawing Tools & Attributes ───
                        k if k == 'T' as i32 => {
                            this.current_tool = DrawTool::Text;
                            this.toolbar.active_tool = Some(DrawTool::Text);
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("🔤", "Text — click to type");
                            this.request_repaint();
                        }
                        k if k == 'H' as i32 => {
                            this.current_tool = DrawTool::Highlighter;
                            this.toolbar.active_tool = Some(DrawTool::Highlighter);
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("🖍️", "Highlighter");
                            this.request_repaint();
                        }
                        k if k == 'L' as i32 => {
                            this.current_tool = DrawTool::Line;
                            this.toolbar.active_tool = Some(DrawTool::Line);
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("📏", "Line");
                            this.request_repaint();
                        }
                        k if k == 'A' as i32 => {
                            this.current_tool = DrawTool::Arrow;
                            this.toolbar.active_tool = Some(DrawTool::Arrow);
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("➜", "Arrow");
                            this.request_repaint();
                        }
                        k if k == 'R' as i32 && is_shift => {
                            this.current_tool = DrawTool::Rectangle;
                            this.toolbar.active_tool = Some(DrawTool::Rectangle);
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("▭", "Rectangle");
                            this.request_repaint();
                        }
                        k if k == 'U' as i32 => {
                            this.current_tool = DrawTool::RoundedRectangle;
                            this.toolbar.active_tool = Some(DrawTool::RoundedRectangle);
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("▢", "Rounded Rectangle");
                            this.request_repaint();
                        }
                        k if k == 'Q' as i32 => {
                            this.current_tool = DrawTool::Ellipse;
                            this.toolbar.active_tool = Some(DrawTool::Ellipse);
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("⬭", "Ellipse");
                            this.request_repaint();
                        }
                        k if k == 'F' as i32 => {
                            let new_fm = match this.fill_mode {
                                FillMode::None => FillMode::Tinted,
                                FillMode::Tinted => FillMode::Solid,
                                FillMode::Solid => FillMode::None,
                            };
                            this.fill_mode = new_fm;
                            this.toolbar.current_fill_mode = new_fm;
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            let nm = new_fm.name();
                            this.set_toast("🎨", format!("Fill Mode: {}", nm));
                            this.request_repaint();
                        }
                        k if k == 'D' as i32 => {
                            let new_sp = match this.stroke_pattern {
                                StrokePattern::Solid => StrokePattern::Dashed,
                                StrokePattern::Dashed => StrokePattern::Dotted,
                                StrokePattern::Dotted => StrokePattern::Solid,
                            };
                            this.stroke_pattern = new_sp;
                            this.toolbar.current_stroke_pattern = new_sp;
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            let nm = new_sp.name();
                            this.set_toast("✏️", format!("Pattern: {}", nm));
                            this.request_repaint();
                        }
                        k if (k == 'N' as i32 && is_shift) || k == '0' as i32 => {
                            this.step_counter = 1;
                            this.toolbar.badge_counter = 1;
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("🔢", "Step badge counter reset to 1");
                            this.request_repaint();
                        }
                        k if k == 'N' as i32 => {
                            let next_num = this.step_counter;
                            this.current_tool = DrawTool::StepBadge;
                            this.toolbar.active_tool = Some(DrawTool::StepBadge);
                            this.toolbar.badge_counter = this.step_counter;
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("🔢", format!("Step Badge (next: #{})", next_num));
                            this.request_repaint();
                        }
                        k if k == 'P' as i32 => {
                            this.current_tool = DrawTool::Pen;
                            this.toolbar.active_tool = Some(DrawTool::Pen);
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("✏️", "Pen");
                            this.request_repaint();
                        }
                        k if k == 'S' as i32 => {
                            this.current_tool = DrawTool::Snip;
                            this.toolbar.active_tool = Some(DrawTool::Snip);
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("✂️", "Snip — drag to copy");
                            this.request_repaint();
                        }
                        k if k == 'X' as i32 => {
                            this.current_tool = DrawTool::Eraser;
                            this.toolbar.active_tool = Some(DrawTool::Eraser);
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("🧹", "Eraser (drag over strokes)");
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
                            this.toolbar.stroke_width = w;
                            let sw = this.screen_width as f32;
                            let sh = this.screen_height as f32;
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("🖌️", format!("{} px", w as u32));
                            this.request_repaint();
                        }

                        // ─── [ ] Bracket Sizing ───
                        k if k == 219 => { // [
                            if this.spotlight.active {
                                let new_rad = (this.spotlight.radius - 20.0).max(30.0);
                                this.spotlight.radius = new_rad;
                                this.set_toast("🔦", format!("⌀{} px", (new_rad * 2.0).round() as u32));
                            } else if this.current_tool == DrawTool::StepBadge {
                                let new_bs = match this.badge_size {
                                    BadgeSize::ExtraLarge => BadgeSize::Large,
                                    BadgeSize::Large => BadgeSize::Medium,
                                    _ => BadgeSize::Small,
                                };
                                this.badge_size = new_bs;
                                this.toolbar.current_badge_size = new_bs;
                                let sw = this.screen_width as f32;
                                let sh = this.screen_height as f32;
                                this.toolbar.update_layout(sw, sh);
                                let nm = new_bs.name();
                                this.set_toast("🔢", format!("Badge Size: {}", nm));
                            } else {
                                let new_w = (this.stroke_width - 2.0).max(1.0);
                                this.stroke_width = new_w;
                                this.toolbar.stroke_width = new_w;
                                let sw = this.screen_width as f32;
                                let sh = this.screen_height as f32;
                                this.toolbar.update_layout(sw, sh);
                                this.set_toast("🖌️", format!("{} px", new_w.round() as u32));
                            }
                            this.request_repaint();
                        }
                        k if k == 221 => { // ]
                            if this.spotlight.active {
                                let new_rad = (this.spotlight.radius + 20.0).min(700.0);
                                this.spotlight.radius = new_rad;
                                this.set_toast("🔦", format!("⌀{} px", (new_rad * 2.0).round() as u32));
                            } else if this.current_tool == DrawTool::StepBadge {
                                let new_bs = match this.badge_size {
                                    BadgeSize::Small => BadgeSize::Medium,
                                    BadgeSize::Medium => BadgeSize::Large,
                                    _ => BadgeSize::ExtraLarge,
                                };
                                this.badge_size = new_bs;
                                this.toolbar.current_badge_size = new_bs;
                                let sw = this.screen_width as f32;
                                let sh = this.screen_height as f32;
                                this.toolbar.update_layout(sw, sh);
                                let nm = new_bs.name();
                                this.set_toast("🔢", format!("Badge Size: {}", nm));
                            } else {
                                let new_w = (this.stroke_width + 2.0).min(40.0);
                                this.stroke_width = new_w;
                                this.toolbar.stroke_width = new_w;
                                let sw = this.screen_width as f32;
                                let sh = this.screen_height as f32;
                                this.toolbar.update_layout(sw, sh);
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
