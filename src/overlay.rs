#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1,
    VK_MENU,
    VK_F2, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GWLP_USERDATA, GetCursorPos, GetWindowLongPtrW, HWND_TOPMOST,
    IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_IBEAM, IDC_SIZEALL, LoadCursorW, RegisterClassExW, SW_HIDE,
    SW_SHOW, SWP_SHOWWINDOW, SetCursor, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    CS_DBLCLKS, ShowWindow, WM_CHAR, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_POINTERDOWN, WM_POINTERUP,
    WM_POINTERUPDATE,
    WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_TIMER, WNDCLASSEXW,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, Result, w};

use crate::capture::ScreenCapture;
use crate::clipboard::{copy_bgra_to_clipboard, get_clipboard_text};
use crate::live_zoom::LiveZoomEngine;
use crate::renderer::D2DRenderer;
use crate::shapes::{
    ARROW_BINDING_GAP, AlignTo, align_offsets, distribute_offsets, SELECTION_HANDLE_SIZE, SELECTION_HANDLE_SLOP, SNAP_TOLERANCE_DIP,
    SnapGuide, can_bind_arrow, can_contain_text,
    collect_anchors, container_height_for, handle_at, label_rides_on_shape, normalize_rect,
    push_pressure,
    recognize_smart_shape, resolve_arrow_ends, with_arrow_ends, resize_shape, resized_bounds, shape_bounds, shape_intersects_circle,
    snap_point, snap_to_angle, snap_to_square, snap_translation, translate_shape,
};
use crate::types::{
    AppMode, ArrowStyle, ArrowToolSettings, BadgeShape, BadgeSize, BlurToolSettings,
    Annotation, CanvasBackground, ColorPickerState, ColorPreset, DragKind, DrawTool, FillMode,
    FluentAction,
    FluentToolbarState,
    HistoryAction, LaserRipple, LaserTrailPoint, LoupeState, MinimapState, Point2D, Shape,
    Selection, ShapeId, ShapeToolSettings, SpotlightState, StepBadgeToolSettings,
    StrokePattern,
    StrokeToolSettings,
    TextCardStyle, TextEditorState, TextFontFamily, TextToolSettings, TimerAction,
    TimerWidgetState, ToastNotification, ZoomState,
};

/// What a pen or finger reported for one pointer message.
struct PointerSample {
    /// Client-space position in physical pixels, matching what a mouse message
    /// would have packed into lparam.
    client_x: i16,
    client_y: i16,
    /// Normalised 0..=1, only for a pen that reports a pressure axis.
    pressure: Option<f32>,
    in_contact: bool,
}

/// Read a WM_POINTER* message. Returns `None` for pointers that are really the
/// mouse, which keeps the existing WM_MOUSE* path authoritative for it.
unsafe fn pointer_sample(hwnd: HWND, wparam: WPARAM) -> Option<PointerSample> {
    use windows::Win32::Graphics::Gdi::ScreenToClient;
    use windows::Win32::UI::Input::Pointer::{
        GetPointerInfo, GetPointerPenInfo, POINTER_FLAG_INCONTACT, POINTER_INFO, POINTER_PEN_INFO,
    };
    use windows::Win32::UI::WindowsAndMessaging::{PEN_MASK_PRESSURE, PT_PEN, PT_TOUCH};

    unsafe {
        let id = (wparam.0 & 0xFFFF) as u32;
        let mut info = POINTER_INFO::default();
        GetPointerInfo(id, &mut info).ok()?;
        if info.pointerType != PT_PEN && info.pointerType != PT_TOUCH {
            return None;
        }

        let mut pt = info.ptPixelLocation;
        let _ = ScreenToClient(hwnd, &mut pt);

        let pressure = if info.pointerType == PT_PEN {
            let mut pen = POINTER_PEN_INFO::default();
            if GetPointerPenInfo(id, &mut pen).is_ok() && (pen.penMask & PEN_MASK_PRESSURE) != 0 {
                // The pen pressure axis is 0..1024 with 0 meaning "not reported".
                Some((pen.pressure as f32 / 1024.0).clamp(0.0, 1.0))
            } else {
                None
            }
        } else {
            None
        };

        Some(PointerSample {
            client_x: pt.x as i16,
            client_y: pt.y as i16,
            pressure,
            in_contact: (info.pointerFlags.0 & POINTER_FLAG_INCONTACT.0) != 0,
        })
    }
}

const OVERLAY_CLASS_NAME: PCWSTR = w!("ZoomifyFullscreenOverlay");
const TIMER_ID_ANIMATION: usize = 1001;

/// Upper bound on laser trail points held at once.
const MAX_LASER_TRAIL_POINTS: usize = 160;

/// Posted to the tray host window whenever a Live Zoom session starts (wparam 1)
/// or stops (wparam 0), so the host can claim / release the Ctrl+Up/Down/+/- keys.
pub const WM_LIVE_ZOOM_STATE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 400;

unsafe extern "system" {
    fn MessageBeep(utype: u32) -> i32;
}

pub struct OverlayWindow {
    pub hwnd: HWND,
    /// Tray host window that owns the global hotkeys; notified on Live Zoom state changes.
    pub host_hwnd: HWND,
    pub renderer: D2DRenderer,
    pub mode: AppMode,
    pub background_capture: Option<ScreenCapture>,
    pub background_bitmap: Option<ID2D1Bitmap>,
    pub background_type: CanvasBackground,
    pub zoom: ZoomState,
    pub minimap: MinimapState,
    pub show_minimap: bool,
    /// Zoom factor applied when entering Static Zoom / Live Zoom (Canvas settings).
    pub default_zoom_level: f32,
    /// When false, the display-cycling command is refused (General settings).
    pub allow_monitor_cycling: bool,
    /// Which display the overlay targets: "cursor" (default) or "primary".
    pub monitor_target: String,
    pub spotlight: SpotlightState,
    pub loupe: LoupeState,
    pub was_shifted_during_draw: bool,
    pub shapes: Vec<Annotation>,
    pub undo_history: Vec<HistoryAction>,
    pub redo_history: Vec<HistoryAction>,
    pub active_shape: Option<Shape>,
    /// Pressure from the most recent pen sample, 0..=1. `None` for mouse,
    /// touch, or a pen with no pressure axis.
    pub pen_pressure: Option<f32>,
    /// True between a pen/touch contact and its release, so stray mouse
    /// messages can be ignored for the duration.
    pub pen_active: bool,
    /// Alignment feedback for the snap currently in effect, in canvas space.
    /// Whether shapes snap to each other at all; Alt overrides per gesture.
    pub snap_to_shapes: bool,
    pub snap_guides: Vec<SnapGuide>,
    /// Cached anchors of every shape except the one being drawn or dragged,
    /// rebuilt when a gesture starts rather than on every mouse move.
    snap_anchors: Vec<Point2D>,
    /// Annotation picked with the Select tool, if any.
    pub selection: Option<Selection>,
    /// Rubber-band rectangle being swept, in canvas coordinates.
    pub marquee: Option<(Point2D, Point2D)>,
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
    pub pen_settings: StrokeToolSettings,
    pub highlighter_settings: StrokeToolSettings,
    pub line_settings: StrokeToolSettings,
    pub arrow_settings: ArrowToolSettings,
    pub rect_settings: ShapeToolSettings,
    pub rounded_rect_settings: ShapeToolSettings,
    pub ellipse_settings: ShapeToolSettings,
    pub badge_settings: StepBadgeToolSettings,
    pub text_settings: TextToolSettings,
    pub blur_settings: BlurToolSettings,
    pub toast: Option<ToastNotification>,
    pub show_cheat_sheet: bool,
    pub show_hud: bool,
    pub toolbar: FluentToolbarState,
    pub color_picker: ColorPickerState,
    pub laser_trail: Vec<LaserTrailPoint>,
    pub laser_ripples: Vec<LaserRipple>,
    pub laser_pos: Option<Point2D>,
    pub eraser_pos: Option<Point2D>,
    pub stroke_start_time: Option<Instant>,
    pub last_mouse_dwell_time: Option<Instant>,
    pub last_mouse_pos: Point2D,
    pub timer_seconds: u32,
    pub timer_remaining: f64,
    pub timer_paused: bool,
    pub timer_alarm_sounded: bool,
    pub timer_sound_enabled: bool,
    pub timer_last_tick: Instant,
    pub timer_widget: TimerWidgetState,
    pub screen_x: i32,
    pub screen_y: i32,
    pub screen_width: u32,
    pub screen_height: u32,
    /// Display density of the monitor the overlay currently covers (96 = 100%).
    /// screen_width/height stay in physical pixels for window and capture APIs;
    /// all layout and hit-testing works in DIPs derived from them.
    pub dpi: u32,
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
                // Needed for WM_LBUTTONDBLCLK (double-click to edit a text
                // annotation with the Select tool).
                style: CS_DBLCLKS,
                hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
                ..Default::default()
            };

            RegisterClassExW(&wnd_class);

            let available_monitors = crate::monitor::MonitorManager::enumerate_monitors();
            let current_monitor = available_monitors.first().cloned().unwrap_or_default();
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
            let spotlight = SpotlightState {
                radius: cfg.spotlight_radius,
                ..Default::default()
            };
            let timer_secs = (cfg.timer_duration_mins * 60).max(60);
            // Accepts a preset name or a #RRGGBB custom colour.
            let initial_color =
                ColorPreset::from_config_str(&cfg.default_color).unwrap_or(ColorPreset::Red);

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

            let toolbar = FluentToolbarState {
                collapsed: cfg.toolbar_collapsed,
                monitor_count: available_monitors.len(),
                current_monitor_index: 0,
                custom_position: cfg
                    .toolbar_custom_position
                    .map(|(cx, cy)| Point2D::new(cx, cy)),
                current_fill_mode: fill_mode,
                current_stroke_pattern: stroke_pattern,
                current_arrow_style: arrow_style,
                current_badge_size: badge_size,
                current_badge_shape: badge_shape,
                stroke_width: cfg.default_stroke_width,
                badge_counter: 1,
                active_tool: None,
                ..Default::default()
            };

            let state = Rc::new(RefCell::new(Self {
                hwnd,
                host_hwnd: HWND::default(),
                renderer,
                mode: AppMode::Idle,
                background_capture: None,
                background_bitmap: None,
                background_type: CanvasBackground::Transparent,
                zoom: ZoomState::default(),
                minimap: MinimapState::default(),
                show_minimap: cfg.show_minimap,
                default_zoom_level: cfg.default_zoom_level,
                allow_monitor_cycling: cfg.allow_monitor_cycling,
                monitor_target: cfg.monitor_target.clone(),
                spotlight,
                loupe: LoupeState::default(),
                was_shifted_during_draw: false,
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
                pen_settings: StrokeToolSettings {
                    stroke_width: 3.0,
                    pattern: StrokePattern::Solid,
                },
                highlighter_settings: StrokeToolSettings {
                    stroke_width: 14.0,
                    pattern: StrokePattern::Solid,
                },
                line_settings: StrokeToolSettings {
                    stroke_width: 3.0,
                    pattern: StrokePattern::Solid,
                },
                arrow_settings: ArrowToolSettings {
                    stroke_width: 4.0,
                    style: ArrowStyle::Single,
                    pattern: StrokePattern::Solid,
                },
                rect_settings: ShapeToolSettings {
                    stroke_width: 3.0,
                    fill_mode: FillMode::None,
                    pattern: StrokePattern::Solid,
                },
                rounded_rect_settings: ShapeToolSettings {
                    stroke_width: 3.0,
                    fill_mode: FillMode::None,
                    pattern: StrokePattern::Solid,
                },
                ellipse_settings: ShapeToolSettings {
                    stroke_width: 3.0,
                    fill_mode: FillMode::None,
                    pattern: StrokePattern::Solid,
                },
                badge_settings: StepBadgeToolSettings {
                    size: BadgeSize::Medium,
                    shape: BadgeShape::Circle,
                    fill: FillMode::Solid,
                    stroke_width: 2.0,
                },
                text_settings: TextToolSettings {
                    font_size: 20.0,
                    is_bold: false,
                    is_italic: false,
                    card_style: TextCardStyle::Transparent,
                    font_family: TextFontFamily::SegoeUI,
                },
                blur_settings: BlurToolSettings { block_size: 14.0 },
                live_zoom: LiveZoomEngine::new(),
                pen_pressure: None,
                pen_active: false,
                snap_to_shapes: cfg.snap_to_shapes,
                snap_guides: Vec::new(),
                snap_anchors: Vec::new(),
                selection: None,
                marquee: None,
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
                color_picker: {
                    let mut cp = ColorPickerState {
                        recent: cfg
                            .recent_custom_colors
                            .iter()
                            .filter_map(|h| ColorPreset::from_config_str(h))
                            .collect(),
                        ..Default::default()
                    };
                    cp.seed_from(initial_color);
                    cp
                },
                laser_trail: Vec::new(),
                laser_ripples: Vec::new(),
                laser_pos: None,
                eraser_pos: None,
                stroke_start_time: None,
                last_mouse_dwell_time: None,
                last_mouse_pos: Point2D::default(),
                timer_seconds: timer_secs,
                timer_remaining: timer_secs as f64,
                timer_paused: false,
                timer_alarm_sounded: false,
                timer_sound_enabled: cfg.timer_sound_enabled,
                timer_last_tick: Instant::now(),
                timer_widget: TimerWidgetState::default(),
                screen_x,
                screen_y,
                screen_width,
                screen_height,
                dpi: 96,
                available_monitors,
                current_monitor_index: 0,
                current_monitor,
            }));

            let raw_ptr = Rc::into_raw(Rc::clone(&state));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw_ptr as isize);

            // No animation timer yet: it is started when the overlay is shown and
            // killed when it is hidden. Ticking at 62 Hz for the whole life of a
            // tray app burns wakeups while nothing is on screen.

            Ok(state)
        }
    }

    /// Re-anchor the picker panel under the toolbar's `+` swatch.
    pub fn relayout_color_picker(&mut self) {
        if !self.color_picker.open {
            return;
        }
        let anchor = self
            .toolbar
            .items
            .iter()
            .find(|i| i.action == FluentAction::OpenColorPicker)
            .map(|i| i.rect)
            .unwrap_or(self.toolbar.bar_rect);
        // Sit below the sub-bar when one is showing, otherwise below the bar.
        let below = self
            .toolbar
            .subbar_rect
            .map(|r| r.bottom)
            .unwrap_or(self.toolbar.bar_rect.bottom);
        let w = self.logical_w();
        self.color_picker.update_layout(anchor, w, below);
    }

    /// Adopt a colour chosen in the picker, remembering it for next time.
    pub fn apply_picked_color(&mut self, color: ColorPreset) {
        self.current_color = color;
        self.color_picker.push_recent(color);
        self.relayout_color_picker();
    }

    /// Physical pixels per DIP for the current monitor.
    #[inline]
    pub fn dpi_scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }

    /// Overlay width in DIPs - the unit all layout and hit-testing uses.
    #[inline]
    pub fn logical_w(&self) -> f32 {
        self.screen_width as f32 / self.dpi_scale()
    }

    /// Overlay height in DIPs.
    #[inline]
    pub fn logical_h(&self) -> f32 {
        self.screen_height as f32 / self.dpi_scale()
    }

    /// Convert a physical-pixel coordinate (mouse input, cursor position) to the
    /// DIP space that layout and rendering work in.
    #[inline]
    pub fn px_to_dip(&self, v: f32) -> f32 {
        v / self.dpi_scale()
    }

    /// Wire the overlay to the tray host window that owns the global hotkeys.
    pub fn set_host_hwnd(&mut self, host: HWND) {
        self.host_hwnd = host;
    }

    fn notify_live_zoom_state(&self, active: bool) {
        if self.host_hwnd.is_invalid() {
            return;
        }
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                Some(self.host_hwnd),
                WM_LIVE_ZOOM_STATE,
                WPARAM(active as usize),
                LPARAM(0),
            );
        }
    }

    /// Start the magnification engine and claim the Ctrl+Up/Down/+/- zoom keys.
    pub fn start_live_zoom_engine(&mut self, level: f32) -> bool {
        let started = self.live_zoom.start(level);
        if started {
            self.notify_live_zoom_state(true);
        } else {
            self.mode = AppMode::Idle;
            self.set_toast("⚠️", "Live Zoom unavailable on this system");
        }
        started
    }

    /// Stop the magnification engine and release the zoom keys back to other apps.
    /// Safe to call when no session is running.
    pub fn stop_live_zoom(&mut self) {
        let was_active = self.live_zoom.is_active();
        self.live_zoom.stop();
        if was_active {
            self.notify_live_zoom_state(false);
        }
    }

    pub fn enter_live_zoom(&mut self) {
        self.loupe.active = false;
        self.loupe.pinned = false;
        self.spotlight.active = false;
        self.spotlight.pinned = false;
        self.zoom = ZoomState::default();
        self.is_drawing = false;
        self.active_shape = None;
        self.commit_text_editor();
        self.target_monitor_under_cursor();
        self.toolbar.active_tool = None;
        self.mode = AppMode::LiveZoom;
        self.background_bitmap = None;
        self.background_capture = None;
        self.hide_window();
        let level = self.default_zoom_level.clamp(1.25, 10.0);
        self.start_live_zoom_engine(level);
    }

    pub fn enter_static_zoom(&mut self) {
        if self.live_zoom.is_active() || self.mode == AppMode::LiveZoom {
            self.stop_live_zoom();
        }
        self.target_monitor_under_cursor();
        if self.background_bitmap.is_none() {
            self.capture_current_screen();
        }
        let mut cursor_screen = Point2D::new(
            self.logical_w() / 2.0,
            self.logical_h() / 2.0,
        );
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                cursor_screen = Point2D::new(
                    self.px_to_dip((pt.x - self.screen_x) as f32),
                    self.px_to_dip((pt.y - self.screen_y) as f32),
                );
            }
        }
        self.zoom.set_zoom_centered(
            self.default_zoom_level.clamp(1.25, 10.0),
            cursor_screen,
            self.logical_w(),
            self.logical_h(),
        );
        self.mode = AppMode::StaticZoom;
        self.loupe.active = false;
        self.loupe.pinned = false;
        self.spotlight.active = false;
        self.spotlight.pinned = false;
        self.is_drawing = false;
        self.active_shape = None;
        self.commit_text_editor();
        self.toolbar.active_tool = None;
        let sw = self.logical_w();
        let sh = self.logical_h();
        self.toolbar.update_layout(sw, sh);
        self.show_window();
        self.set_toast("🔎", "Static Zoom (Wheel: Zoom | Pan)");
        self.request_repaint();
    }

    pub fn sync_tool_to_toolbar(&mut self) {
        match self.current_tool {
            DrawTool::Pen => {
                self.stroke_width = self.pen_settings.stroke_width;
                self.stroke_pattern = self.pen_settings.pattern;
                self.toolbar.stroke_width = self.pen_settings.stroke_width;
                self.toolbar.current_stroke_pattern = self.pen_settings.pattern;
            }
            DrawTool::Highlighter => {
                self.stroke_width = self.highlighter_settings.stroke_width;
                self.stroke_pattern = self.highlighter_settings.pattern;
                self.toolbar.stroke_width = self.highlighter_settings.stroke_width;
                self.toolbar.current_stroke_pattern = self.highlighter_settings.pattern;
            }
            DrawTool::Line => {
                self.stroke_width = self.line_settings.stroke_width;
                self.stroke_pattern = self.line_settings.pattern;
                self.toolbar.stroke_width = self.line_settings.stroke_width;
                self.toolbar.current_stroke_pattern = self.line_settings.pattern;
            }
            DrawTool::Arrow => {
                self.stroke_width = self.arrow_settings.stroke_width;
                self.stroke_pattern = self.arrow_settings.pattern;
                self.arrow_style = self.arrow_settings.style;
                self.toolbar.stroke_width = self.arrow_settings.stroke_width;
                self.toolbar.current_stroke_pattern = self.arrow_settings.pattern;
                self.toolbar.current_arrow_style = self.arrow_settings.style;
            }
            DrawTool::Rectangle => {
                self.stroke_width = self.rect_settings.stroke_width;
                self.fill_mode = self.rect_settings.fill_mode;
                self.stroke_pattern = self.rect_settings.pattern;
                self.toolbar.stroke_width = self.rect_settings.stroke_width;
                self.toolbar.current_fill_mode = self.rect_settings.fill_mode;
                self.toolbar.current_stroke_pattern = self.rect_settings.pattern;
            }
            DrawTool::RoundedRectangle => {
                self.stroke_width = self.rounded_rect_settings.stroke_width;
                self.fill_mode = self.rounded_rect_settings.fill_mode;
                self.stroke_pattern = self.rounded_rect_settings.pattern;
                self.toolbar.stroke_width = self.rounded_rect_settings.stroke_width;
                self.toolbar.current_fill_mode = self.rounded_rect_settings.fill_mode;
                self.toolbar.current_stroke_pattern = self.rounded_rect_settings.pattern;
            }
            DrawTool::Ellipse => {
                self.stroke_width = self.ellipse_settings.stroke_width;
                self.fill_mode = self.ellipse_settings.fill_mode;
                self.stroke_pattern = self.ellipse_settings.pattern;
                self.toolbar.stroke_width = self.ellipse_settings.stroke_width;
                self.toolbar.current_fill_mode = self.ellipse_settings.fill_mode;
                self.toolbar.current_stroke_pattern = self.ellipse_settings.pattern;
            }
            DrawTool::StepBadge => {
                self.badge_size = self.badge_settings.size;
                self.badge_shape = self.badge_settings.shape;
                self.fill_mode = self.badge_settings.fill;
                self.stroke_width = self.badge_settings.stroke_width;
                self.stroke_pattern = StrokePattern::Solid;
                self.toolbar.current_badge_size = self.badge_settings.size;
                self.toolbar.current_badge_shape = self.badge_settings.shape;
                self.toolbar.current_fill_mode = self.badge_settings.fill;
                self.toolbar.stroke_width = self.badge_settings.stroke_width;
                self.toolbar.current_stroke_pattern = StrokePattern::Solid;
            }
            DrawTool::Text => {
                self.font_size = self.text_settings.font_size;
                self.text_is_bold = self.text_settings.is_bold;
                self.text_is_italic = self.text_settings.is_italic;
                self.text_card_style = self.text_settings.card_style;
                self.text_font_family = self.text_settings.font_family;
                self.toolbar.current_font_size = self.text_settings.font_size;
                self.toolbar.text_is_bold = self.text_settings.is_bold;
                self.toolbar.text_is_italic = self.text_settings.is_italic;
                self.toolbar.text_card_style = self.text_settings.card_style;
                self.toolbar.text_font_family = self.text_settings.font_family;
            }
            DrawTool::Blur => {
                self.stroke_width = self.blur_settings.block_size;
                self.toolbar.stroke_width = self.blur_settings.block_size;
            }
            _ => {}
        }
    }

    pub fn ensure_draw_mode(&mut self) {
        if self.mode != AppMode::Draw && self.mode != AppMode::StaticZoom {
            self.mode = AppMode::Draw;
            self.loupe.active = false;
            self.loupe.pinned = false;
            self.spotlight.active = false;
            self.spotlight.pinned = false;
            self.zoom = ZoomState::default();
            self.is_drawing = false;
            self.active_shape = None;
            self.commit_text_editor();
            self.toolbar.active_tool = Some(self.current_tool);
            let sw = self.logical_w();
            let sh = self.logical_h();
            self.toolbar.update_layout(sw, sh);
        }
    }

    pub fn enter_draw_mode(&mut self) {
        if self.live_zoom.is_active() || self.mode == AppMode::LiveZoom {
            self.stop_live_zoom();
            self.zoom = ZoomState::default();
        } else if self.mode == AppMode::Idle
            || self.mode == AppMode::Loupe
            || self.mode == AppMode::Timer
            || self.mode == AppMode::Spotlight
            || self.background_bitmap.is_none()
        {
            self.zoom = ZoomState::default();
        }
        self.target_monitor_under_cursor();
        if self.background_bitmap.is_none() {
            self.capture_current_screen();
        }
        self.mode = AppMode::Draw;
        self.loupe.active = false;
        self.loupe.pinned = false;
        self.spotlight.active = false;
        self.spotlight.pinned = false;
        self.is_drawing = false;
        self.active_shape = None;
        self.commit_text_editor();
        self.toolbar.active_tool = Some(self.current_tool);
        let sw = self.logical_w();
        let sh = self.logical_h();
        self.toolbar.update_layout(sw, sh);
        self.show_window();
        self.set_toast("✏️", "Draw Mode Active");
        self.request_repaint();
    }

    pub fn enter_spotlight_mode(&mut self) {
        if self.live_zoom.is_active() || self.mode == AppMode::LiveZoom {
            self.stop_live_zoom();
        }
        self.zoom = ZoomState::default();
        self.background_type = CanvasBackground::Transparent;
        self.target_monitor_under_cursor();
        if self.background_bitmap.is_none() {
            self.capture_current_screen();
        }
        self.mode = AppMode::Spotlight;
        self.loupe.active = false;
        self.loupe.pinned = false;
        self.is_drawing = false;
        self.active_shape = None;
        self.commit_text_editor();
        self.toolbar.active_tool = None;
        self.spotlight.active = true;
        self.spotlight.pinned = false;

        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                let screen_pt = Point2D::new(
                    self.px_to_dip((pt.x - self.screen_x) as f32),
                    self.px_to_dip((pt.y - self.screen_y) as f32),
                );
                self.spotlight.x = screen_pt.x;
                self.spotlight.y = screen_pt.y;
            }
        }
        let sw = self.logical_w();
        let sh = self.logical_h();
        self.toolbar.update_layout(sw, sh);
        self.show_window();
        self.set_toast("🔦", "Spotlight Active (Ctrl+Wheel: Resize | Space: Pin)");
        self.request_repaint();
    }

    pub fn enter_timer_mode(&mut self, minutes: u32) {
        if self.live_zoom.is_active() || self.mode == AppMode::LiveZoom {
            self.stop_live_zoom();
        }
        self.zoom = ZoomState::default();
        self.background_type = CanvasBackground::Transparent;
        self.target_monitor_under_cursor();
        if self.background_bitmap.is_none() {
            self.capture_current_screen();
        }
        self.mode = AppMode::Timer;
        self.loupe.active = false;
        self.loupe.pinned = false;
        self.spotlight.active = false;
        self.spotlight.pinned = false;
        self.is_drawing = false;
        self.active_shape = None;
        self.commit_text_editor();
        self.toolbar.active_tool = None;
        let mins = if minutes > 0 {
            minutes
        } else {
            (self.timer_seconds / 60).max(1)
        };
        self.timer_seconds = mins * 60;
        self.timer_remaining = self.timer_seconds as f64;
        self.timer_paused = false;
        self.timer_alarm_sounded = false;
        self.timer_last_tick = Instant::now();
        let sw = self.logical_w();
        let sh = self.logical_h();
        self.toolbar.update_layout(sw, sh);
        self.show_window();
        self.set_toast(
            "⏱️",
            format!(
                "Presentation Timer: {}m (Wheel: ±1m | Ctrl+Wheel: Dim | Tab: Mini)",
                mins
            ),
        );
        self.request_repaint();
    }

    pub fn toggle_spotlight(&mut self) {
        self.spotlight.active = !self.spotlight.active;
        if self.spotlight.active {
            self.loupe.active = false;
            self.loupe.pinned = false;
            unsafe {
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    let screen_pt = Point2D::new(
                        self.px_to_dip((pt.x - self.screen_x) as f32),
                        self.px_to_dip((pt.y - self.screen_y) as f32),
                    );
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

    pub fn enter_loupe_mode(&mut self) {
        if self.live_zoom.is_active() || self.mode == AppMode::LiveZoom {
            self.stop_live_zoom();
        }
        self.zoom = ZoomState::default();
        self.background_type = CanvasBackground::Transparent;
        self.target_monitor_under_cursor();
        if self.background_bitmap.is_none() {
            self.capture_current_screen();
        }
        self.mode = AppMode::Loupe;
        self.loupe.active = true;
        self.loupe.pinned = false;
        self.spotlight.active = false;
        self.spotlight.pinned = false;
        self.is_drawing = false;
        self.active_shape = None;
        self.commit_text_editor();
        self.toolbar.active_tool = None;

        let mut pt = POINT::default();
        let (cx, cy) = if unsafe { GetCursorPos(&mut pt).is_ok() } {
            (
                self.px_to_dip((pt.x - self.screen_x) as f32),
                self.px_to_dip((pt.y - self.screen_y) as f32),
            )
        } else {
            (
                self.logical_w() / 2.0,
                self.logical_h() / 2.0,
            )
        };
        self.loupe.x = cx;
        self.loupe.y = cy;

        let sw = self.logical_w();
        let sh = self.logical_h();
        self.toolbar.update_layout(sw, sh);
        self.show_window();
        self.set_toast(
            "🔍",
            format!(
                "Loupe {:.1}x (Wheel: Zoom | Shift+Wheel: Size | Space: Pin)",
                self.loupe.magnification
            ),
        );
        self.request_repaint();
    }

    /// Extend or shorten the countdown by `delta_secs`, keeping the total and the
    /// remaining time in lockstep. `timer_seconds` is the denominator of the
    /// progress ring and the value persisted as the default duration, so it must
    /// stay a *duration* — never be overwritten with whatever is left on the clock.
    pub fn adjust_timer(&mut self, delta_secs: f64) {
        let (total, remaining) = crate::types::adjust_timer_values(
            self.timer_seconds,
            self.timer_remaining,
            delta_secs,
        );
        self.timer_seconds = total;
        self.timer_remaining = remaining;

        // Putting time back on the clock re-arms the end-of-timer chime.
        if self.timer_remaining > 0.0 {
            self.timer_alarm_sounded = false;
        }
    }

    /// Whole minutes of total duration, for toasts.
    pub fn timer_total_mins(&self) -> u32 {
        (self.timer_seconds as f32 / 60.0).round().max(1.0) as u32
    }

    pub fn save_config(&self) {
        let mut cfg = crate::config::AppConfig::load();
        cfg.default_stroke_width = self.stroke_width;
        cfg.spotlight_radius = self.spotlight.radius;
        cfg.timer_duration_mins = (self.timer_seconds / 60).max(1);
        cfg.toolbar_collapsed = self.toolbar.collapsed;
        cfg.show_minimap = self.show_minimap;
        // Unconditional: a `Some`-only write means "Reset Toolbar Position"
        // never clears the stale position from config.json.
        cfg.toolbar_custom_position = self.toolbar.custom_position.map(|p| (p.x, p.y));
        cfg.default_color = self.current_color.name();
        cfg.recent_custom_colors = self
            .color_picker
            .recent
            .iter()
            .map(|c| c.name())
            .collect();
        cfg.default_fill_mode = match self.fill_mode {
            FillMode::None => "None".to_string(),
            FillMode::Tinted => "Tinted".to_string(),
            FillMode::Solid => "Solid".to_string(),
        };
        cfg.default_stroke_pattern = match self.stroke_pattern {
            StrokePattern::Dashed => "Dashed".to_string(),
            StrokePattern::Dotted => "Dotted".to_string(),
            StrokePattern::Solid => "Solid".to_string(),
        };
        cfg.default_badge_size = match self.badge_size {
            BadgeSize::Small => "Small".to_string(),
            BadgeSize::Large => "Large".to_string(),
            BadgeSize::ExtraLarge => "ExtraLarge".to_string(),
            BadgeSize::Medium => "Medium".to_string(),
        };
        cfg.save();
    }

    pub fn exit_overlay(&mut self) {
        self.save_config();
        // Capture the canvas before teardown clears it.
        if crate::config::AppConfig::load().autosave_sessions {
            self.save_session(false);
        }
        if self.live_zoom.is_active() {
            self.stop_live_zoom();
        }
        self.mode = AppMode::Idle;
        self.spotlight.active = false;
        self.spotlight.pinned = false;
        self.loupe.active = false;
        self.loupe.pinned = false;
        self.is_drawing = false;
        self.active_shape = None;
        self.selection = None;
        self.text_editor = None;
        self.show_cheat_sheet = false;
        self.background_bitmap = None;
        self.background_capture = None;
        self.zoom = ZoomState::default();
        self.shapes.clear();
        self.undo_history.clear();
        self.redo_history.clear();
        self.background_type = CanvasBackground::Transparent;
        self.laser_trail.clear();
        self.laser_ripples.clear();
        self.laser_pos = None;
        self.eraser_pos = None;
        self.toolbar.active_tool = None;
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

            let is_visible = unsafe {
                windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(self.hwnd).as_bool()
            };
            let flags =
                if is_visible && self.mode != AppMode::Idle && self.mode != AppMode::LiveZoom {
                    SWP_SHOWWINDOW
                } else {
                    windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
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

            // Adopt the target monitor's density before laying anything out.
            let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(self.hwnd) };
            self.dpi = if dpi == 0 { 96 } else { dpi };
            self.renderer.set_dpi(self.dpi);

            self.toolbar.update_layout(self.logical_w(), self.logical_h());
        }
    }

    pub fn cycle_next_monitor(&mut self) {
        if !self.allow_monitor_cycling {
            self.set_toast("🖥️", "Display cycling is disabled in Settings");
            self.request_repaint();
            return;
        }
        self.refresh_monitors();
        if self.available_monitors.len() <= 1 {
            self.set_toast("🖥️", "Single display active");
            self.request_repaint();
            return;
        }

        let next_idx = (self.current_monitor_index + 1) % self.available_monitors.len();
        self.set_active_monitor(next_idx);

        // Recapture screen for new monitor if in visual freeze modes
        if self.mode == AppMode::StaticZoom
            || self.mode == AppMode::Draw
            || self.mode == AppMode::Spotlight
            || self.mode == AppMode::Timer
            || self.mode == AppMode::Loupe
        {
            self.capture_current_screen();
        }

        let name = self.current_monitor.name.clone();
        self.set_toast("🖥️", format!("Switched to {}", name));
        self.request_repaint();
    }

    pub fn target_monitor_under_cursor(&mut self) {
        self.refresh_monitors();

        // "primary" pins the overlay to the main display regardless of where the
        // pointer is; "cursor" (default) follows the pointer.
        if self.monitor_target.eq_ignore_ascii_case("primary") {
            let idx = self
                .available_monitors
                .iter()
                .position(|m| m.is_primary)
                .unwrap_or(0);
            self.set_active_monitor(idx);
            return;
        }

        // Locate the cursor against the list refresh_monitors() just built rather
        // than calling get_monitor_from_cursor(), which enumerates all over again.
        let mut pt = POINT::default();
        let target_idx = if unsafe { GetCursorPos(&mut pt) }.is_ok() {
            self.available_monitors
                .iter()
                .position(|m| m.contains_point(pt.x, pt.y))
                .unwrap_or(0)
        } else {
            0
        };
        self.set_active_monitor(target_idx);
    }

    /// Take the foreground reliably.
    ///
    /// A bare `SetForegroundWindow` is rejected by Windows' foreground lock
    /// whenever the caller does not own the current foreground window, which
    /// leaves the overlay visible but without keyboard focus - every shortcut
    /// silently does nothing. Briefly sharing an input queue with the current
    /// foreground thread lifts the restriction.
    fn force_foreground(&self) {
        use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
        use windows::Win32::UI::WindowsAndMessaging::{
            BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId,
        };

        unsafe {
            let this_thread = GetCurrentThreadId();
            let fg = GetForegroundWindow();
            let fg_thread = if fg.is_invalid() {
                0
            } else {
                GetWindowThreadProcessId(fg, None)
            };

            let attached = fg_thread != 0
                && fg_thread != this_thread
                && AttachThreadInput(this_thread, fg_thread, true).as_bool();

            let _ = SetForegroundWindow(self.hwnd);
            let _ = BringWindowToTop(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));

            if attached {
                let _ = AttachThreadInput(this_thread, fg_thread, false);
            }
        }
    }

    /// The ~60 Hz animation tick drives toasts, the countdown, laser decay and
    /// smooth panning - all of which only exist while the overlay is on screen.
    /// Running it while hidden is pure wakeup cost for a tray-resident app.
    fn set_animation_timer(&self, running: bool) {
        unsafe {
            if running {
                windows::Win32::UI::WindowsAndMessaging::SetTimer(
                    Some(self.hwnd),
                    TIMER_ID_ANIMATION,
                    16,
                    None,
                );
            } else {
                let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(
                    Some(self.hwnd),
                    TIMER_ID_ANIMATION,
                );
            }
        }
    }

    pub fn show_window(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
        }
        self.set_animation_timer(true);
        self.force_foreground();
        self.toolbar
            .update_layout(self.logical_w(), self.logical_h());
        self.request_repaint();
    }

    pub fn hide_window(&mut self) {
        self.set_animation_timer(false);
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// Block until DWM has composed a fresh frame.
    ///
    /// This is the barrier the capture path actually needs: it covers both "the
    /// overlay just hid itself" and "fullscreen magnification just reset". The
    /// mode-entry paths used to approximate it with a fixed 25 ms sleep, which
    /// is a guess rather than a synchronization point and blocked the message
    /// pump for longer than necessary while a RefCell borrow was held.
    fn wait_for_compose() {
        unsafe {
            if windows::Win32::Graphics::Dwm::DwmFlush().is_err() {
                // Only if the compositor is unavailable (e.g. DWM disabled).
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }

    pub fn capture_current_screen(&mut self) {
        let is_visible = unsafe {
            windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(self.hwnd).as_bool()
        };
        if is_visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }

        // Always wait, not just when we had to hide: entering a mode straight
        // from Live Zoom leaves the window already hidden but the magnification
        // transform still on screen.
        Self::wait_for_compose();

        if let Some(cap) = ScreenCapture::capture_rect(
            self.screen_x,
            self.screen_y,
            self.screen_width,
            self.screen_height,
        ) {
            if let Some(rt) = &self.renderer.render_target
                && let Ok(bmp) = cap.create_d2d_bitmap(rt)
            {
                self.background_bitmap = Some(bmp);
            }
            self.background_capture = Some(cap);
        }

        if is_visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
            }
            self.force_foreground();
        }
    }

    pub fn request_repaint(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    /// Recreate the frozen-background bitmap from the CPU-side capture we still
    /// hold. Needed after the render target is rebuilt, since an ID2D1Bitmap
    /// belongs to the target that made it.
    fn rebuild_background_bitmap(&mut self) {
        let rebuilt = match (&self.background_capture, &self.renderer.render_target) {
            (Some(cap), Some(rt)) => cap.create_d2d_bitmap(rt).ok(),
            _ => None,
        };
        self.background_bitmap = rebuilt;
    }

    /// Rebuild the render target and its dependent resources if the GPU device
    /// was lost during the last frame. Without this the overlay renders nothing
    /// forever after a driver reset, RDP transition, or display mode change.
    pub fn recover_device_if_needed(&mut self) {
        if self.renderer.recover_if_device_lost() {
            self.rebuild_background_bitmap();
            self.request_repaint();
        }
    }

    pub fn set_toast(&mut self, icon: &'static str, message: impl Into<String>) {
        self.toast = Some(ToastNotification::new(icon, message));
        self.request_repaint();
    }

    pub fn push_shape(&mut self, shape: Shape) -> ShapeId {
        self.selection = None;
        let annotation = Annotation::new(shape);
        let id = annotation.id;
        self.shapes.push(annotation.clone());
        let prev_counter = match &annotation.shape {
            Shape::StepBadge { number, .. } => Some(*number),
            _ => None,
        };
        let action = match prev_counter {
            Some(prev) => HistoryAction::AddStepBadge {
                shape: annotation,
                prev_counter: prev,
            },
            None => HistoryAction::AddShape(annotation),
        };
        self.undo_history.push(action);
        self.redo_history.clear();
        id
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
                    self.redo_history
                        .push(HistoryAction::DeleteShape { index, shape });
                    self.set_toast("↩️", "Restored Erased Shape");
                }
                HistoryAction::Reorder { id, from, to } => {
                    if let Some(now) = self.shapes.iter().position(|a| a.id == id) {
                        let a = self.shapes.remove(now);
                        self.shapes.insert(from.min(self.shapes.len()), a);
                        self.redo_history
                            .push(HistoryAction::Reorder { id, from, to });
                        self.set_toast("↩️", "Undo Reorder");
                    }
                }
                HistoryAction::DeleteShapes { items } => {
                    // Ascending, so each insert lands before the next one's index.
                    for (index, shape) in &items {
                        let at = (*index).min(self.shapes.len());
                        self.shapes.insert(at, shape.clone());
                    }
                    self.redo_history
                        .push(HistoryAction::DeleteShapes { items });
                    self.set_toast("↩️", "Restored Deleted Annotation");
                }
                HistoryAction::Clear(prev_shapes) => {
                    let current_shapes = std::mem::replace(&mut self.shapes, prev_shapes);
                    self.redo_history.push(HistoryAction::Clear(current_shapes));
                    self.set_toast("↩️", "Restored Cleared Canvas");
                }
                HistoryAction::TransformShapes { items } => {
                    for (id, before, _) in &items {
                        if let Some(a) = self.shapes.iter_mut().find(|a| a.id == *id) {
                            a.shape = before.clone();
                        }
                    }
                    self.redo_history
                        .push(HistoryAction::TransformShapes { items });
                    self.set_toast("↩️", "Undo Edit");
                }
            }
            self.selection = None;
            self.settle_bindings();
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
                HistoryAction::AddStepBadge {
                    shape,
                    prev_counter,
                } => {
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
                    self.undo_history
                        .push(HistoryAction::DeleteShape { index, shape });
                    self.set_toast("↪️", "Re-erased Shape");
                }
                HistoryAction::Reorder { id, from, to } => {
                    if let Some(now) = self.shapes.iter().position(|a| a.id == id) {
                        let a = self.shapes.remove(now);
                        self.shapes.insert(to.min(self.shapes.len()), a);
                        self.undo_history
                            .push(HistoryAction::Reorder { id, from, to });
                        self.set_toast("↪️", "Redo Reorder");
                    }
                }
                HistoryAction::DeleteShapes { items } => {
                    // Descending, so removing one does not shift the next.
                    for (index, _) in items.iter().rev() {
                        if *index < self.shapes.len() {
                            self.shapes.remove(*index);
                        }
                    }
                    self.undo_history
                        .push(HistoryAction::DeleteShapes { items });
                    self.set_toast("↪️", "Re-deleted Annotation");
                }
                HistoryAction::Clear(_prev_shapes) => {
                    let current_shapes = std::mem::take(&mut self.shapes);
                    self.undo_history.push(HistoryAction::Clear(current_shapes));
                    self.set_toast("↪️", "Re-cleared Canvas");
                }
                HistoryAction::TransformShapes { items } => {
                    for (id, _, after) in &items {
                        if let Some(a) = self.shapes.iter_mut().find(|a| a.id == *id) {
                            a.shape = after.clone();
                        }
                    }
                    self.undo_history
                        .push(HistoryAction::TransformShapes { items });
                    self.set_toast("↪️", "Redo Edit");
                }
            }
            self.selection = None;
            self.settle_bindings();
            self.request_repaint();
        }
    }

    pub fn clear_all(&mut self) {
        if !self.shapes.is_empty() {
            let old = std::mem::take(&mut self.shapes);
            self.undo_history.push(HistoryAction::Clear(old));
            self.redo_history.clear();
            self.selection = None;
            self.set_toast("🧹", "Canvas Cleared (Ctrl+Z to Undo)");
            self.request_repaint();
        }
    }

    // ─────────────────────── Select tool ───────────────────────

    /// Canvas bounds of a shape. Text is measured with real DirectWrite
    /// metrics so the selection box hugs the rendered card rather than a
    /// character-count guess.
    pub fn shape_bounds_exact(&self, shape: &Shape) -> (f32, f32, f32, f32) {
        if let Shape::Text {
            origin,
            text,
            font_size,
            is_bold,
            is_italic,
            font_family,
            ..
        } = shape
        {
            let (w, h) =
                self.renderer
                    .measure_text_block(text, *font_size, *is_bold, *is_italic, *font_family, f32::MAX);
            return (origin.x, origin.y, origin.x + w.max(20.0), origin.y + h);
        }
        shape_bounds(shape)
    }

    /// Bounds of the current selection, in canvas coordinates.
    /// Union bounds of everything selected, in canvas coordinates.
    pub fn selection_bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let sel = self.selection.as_ref()?;
        let mut out: Option<(f32, f32, f32, f32)> = None;
        for id in &sel.ids {
            let Some(a) = self.annotation(*id) else {
                continue;
            };
            let b = self.shape_bounds_exact(&a.shape);
            out = Some(match out {
                None => b,
                Some(u) => (u.0.min(b.0), u.1.min(b.1), u.2.max(b.2), u.3.max(b.3)),
            });
        }
        out
    }

    /// A selected annotation, if it is still on the canvas.
    fn annotation(&self, id: ShapeId) -> Option<&Annotation> {
        self.shapes.iter().find(|a| a.id == id)
    }

    fn annotation_index(&self, id: ShapeId) -> Option<usize> {
        self.shapes.iter().position(|a| a.id == id)
    }

    /// The same bounds converted to screen DIPs, where the grips live.
    pub fn selection_bounds_screen(&self) -> Option<(f32, f32, f32, f32)> {
        let (l, t, r, b) = self.selection_bounds()?;
        let tl = self.zoom.canvas_to_screen(Point2D::new(l, t));
        let br = self.zoom.canvas_to_screen(Point2D::new(r, b));
        Some((tl.x, tl.y, br.x, br.y))
    }

    /// The two ends of a singly-selected line or arrow, in screen DIPs.
    ///
    /// `None` for anything else, which is what makes the renderer fall back to
    /// the eight-grip bounding box.
    pub fn selection_endpoints_screen(&self) -> Option<((f32, f32), (f32, f32))> {
        let id = self.selection.as_ref()?.only()?;
        let a = self.annotation(id)?;
        let (s, e) = match &a.shape {
            Shape::Line { start, end, .. } | Shape::Arrow { start, end, .. } => (*start, *end),
            _ => return None,
        };
        let s = self.zoom.canvas_to_screen(s);
        let e = self.zoom.canvas_to_screen(e);
        Some(((s.x, s.y), (e.x, e.y)))
    }

    /// Which end of the selected line sits under `screen_pt`, if either.
    fn endpoint_at(&self, screen_pt: Point2D) -> Option<bool> {
        let ((sx, sy), (ex, ey)) = self.selection_endpoints_screen()?;
        let reach = SELECTION_HANDLE_SIZE * 0.5 + SELECTION_HANDLE_SLOP;
        let near = |x: f32, y: f32| {
            (screen_pt.x - x).abs() <= reach && (screen_pt.y - y).abs() <= reach
        };
        if near(sx, sy) {
            Some(true)
        } else if near(ex, ey) {
            Some(false)
        } else {
            None
        }
    }

    /// The marquee rectangle currently being swept, in screen DIPs.
    pub fn marquee_screen(&self) -> Option<(f32, f32, f32, f32)> {
        let (a, b) = self.marquee?;
        let (l, t, r, bo) = normalize_rect(a, b);
        let tl = self.zoom.canvas_to_screen(Point2D::new(l, t));
        let br = self.zoom.canvas_to_screen(Point2D::new(r, bo));
        Some((tl.x, tl.y, br.x, br.y))
    }

    /// Drop anything in the selection that is no longer on the canvas.
    fn validate_selection(&mut self) {
        let live: Vec<ShapeId> = self.shapes.iter().map(|a| a.id).collect();
        if let Some(sel) = &mut self.selection {
            sel.ids.retain(|id| live.contains(id));
            if sel.ids.is_empty() {
                self.selection = None;
            }
        }
    }

    fn selected_ids(&self) -> Vec<ShapeId> {
        self.selection
            .as_ref()
            .map(|s| s.ids.clone())
            .unwrap_or_default()
    }

    /// Snapshot the selection so a drag has a fixed origin to map from.
    fn begin_drag(&mut self, kind: DragKind, canvas_pt: Point2D) {
        let ids = self.selected_ids();
        let originals: Vec<(ShapeId, Shape)> = ids
            .iter()
            .filter_map(|id| self.annotation(*id).map(|a| (*id, a.shape.clone())))
            .collect();
        let bounds = self.selection_bounds().unwrap_or((0.0, 0.0, 0.0, 0.0));
        // Snapping must not offer the selection its own anchors back.
        self.rebuild_snap_anchors_excluding(&ids);
        if let Some(sel) = &mut self.selection {
            sel.drag = Some(kind);
            sel.grab = canvas_pt;
            sel.originals = originals;
            sel.original_bounds = bounds;
        }
    }

    /// Begin a selection drag, or start a marquee on empty canvas.
    /// Returns true if the click was consumed.
    fn select_press(&mut self, screen_pt: Point2D, canvas_pt: Point2D, additive: bool) -> bool {
        // A grip on the current selection wins over picking a new shape,
        // because grips sit outside the shape's own outline.
        // A line's own ends win over any bounding box, since they sit inside it.
        if !additive
            && let Some(is_start) = self.endpoint_at(screen_pt)
        {
            self.begin_drag(DragKind::Endpoint(is_start), canvas_pt);
            return true;
        }
        if !additive
            && let Some(screen_bounds) = self.selection_bounds_screen()
            && let Some(handle) = handle_at(screen_bounds, screen_pt)
        {
            self.begin_drag(DragKind::Resize(handle), canvas_pt);
            return true;
        }

        // Topmost shape under the cursor. The tolerance is in canvas units, so
        // divide out the zoom to keep the grab area constant on screen.
        //
        // Text inside a container is not selectable on its own: the container
        // owns it, and clicking the label should grab the box.
        let tol = 10.0 / self.zoom.level.max(1.0);
        let hit = self
            .shapes
            .iter()
            .rev()
            .filter(|a| !a.is_contained_text())
            .find(|a| shape_intersects_circle(&a.shape, canvas_pt, tol))
            .map(|a| a.id);

        match hit {
            Some(id) => {
                let already = self
                    .selection
                    .as_ref()
                    .map(|s| s.ids.contains(&id))
                    .unwrap_or(false);
                if additive {
                    // Shift toggles, so a mis-click can be taken back without
                    // starting the whole selection over.
                    if already {
                        if let Some(sel) = &mut self.selection {
                            sel.ids.retain(|x| *x != id);
                            if sel.ids.is_empty() {
                                self.selection = None;
                            }
                        }
                        return true;
                    }
                    match &mut self.selection {
                        Some(sel) => sel.ids.push(id),
                        None => self.selection = Some(Selection::single(id)),
                    }
                } else if !already {
                    self.selection = Some(Selection::single(id));
                }
                self.begin_drag(DragKind::Move, canvas_pt);
                true
            }
            None => {
                // Empty canvas: sweep out a marquee rather than only clearing.
                if !additive {
                    self.selection = None;
                }
                self.marquee = Some((canvas_pt, canvas_pt));
                true
            }
        }
    }

    /// Apply the in-flight drag. Returns true if anything changed.
    fn select_drag(&mut self, canvas_pt: Point2D) -> bool {
        if let Some((anchor, _)) = self.marquee {
            self.marquee = Some((anchor, canvas_pt));
            return true;
        }
        let Some(sel) = &self.selection else {
            return false;
        };
        let Some(kind) = sel.drag else {
            return false;
        };
        let dx = canvas_pt.x - sel.grab.x;
        let dy = canvas_pt.y - sel.grab.y;
        let grab = sel.grab;
        let originals = sel.originals.clone();
        let original_bounds = sel.original_bounds;
        let snapping = self.snapping_enabled();
        let tol = self.snap_tolerance();

        let updates: Vec<(ShapeId, Shape)> = match kind {
            DragKind::Move => {
                let (dx, dy) = if snapping && !originals.is_empty() {
                    // Snap the group by whichever member lines up best.
                    let (sx, sy, guides) =
                        snap_translation(&originals[0].1, dx, dy, &self.snap_anchors, tol);
                    self.snap_guides = guides;
                    (sx, sy)
                } else {
                    self.snap_guides.clear();
                    (dx, dy)
                };
                originals
                    .iter()
                    .map(|(id, s)| {
                        let mut u = s.clone();
                        translate_shape(&mut u, dx, dy);
                        (*id, u)
                    })
                    .collect()
            }
            DragKind::Endpoint(is_start) => {
                let target = if snapping {
                    match snap_point(canvas_pt, &self.snap_anchors, tol) {
                        Some((p, guides)) => {
                            self.snap_guides = guides;
                            p
                        }
                        None => {
                            self.snap_guides.clear();
                            canvas_pt
                        }
                    }
                } else {
                    self.snap_guides.clear();
                    canvas_pt
                };
                originals
                    .iter()
                    .map(|(id, s)| {
                        let mut u = s.clone();
                        match &mut u {
                            Shape::Line { start, end, .. } | Shape::Arrow { start, end, .. } => {
                                if is_start {
                                    *start = target;
                                } else {
                                    *end = target;
                                }
                            }
                            _ => {}
                        }
                        (*id, u)
                    })
                    .collect()
            }
            DragKind::Resize(handle) => {
                // Snap the grip itself, then rebuild the box from where it landed.
                let grip = Point2D::new(grab.x + dx, grab.y + dy);
                let grip = if snapping {
                    match snap_point(grip, &self.snap_anchors, tol) {
                        Some((p, guides)) => {
                            self.snap_guides = guides;
                            p
                        }
                        None => {
                            self.snap_guides.clear();
                            grip
                        }
                    }
                } else {
                    self.snap_guides.clear();
                    grip
                };
                let to = resized_bounds(original_bounds, handle, grip.x - grab.x, grip.y - grab.y);
                originals
                    .iter()
                    .map(|(id, s)| {
                        let mut u = s.clone();
                        // Every member maps through the same box change, so the
                        // group scales as a unit and keeps its arrangement.
                        resize_shape(&mut u, original_bounds, to);
                        (*id, u)
                    })
                    .collect()
            }
        };

        let mut changed = false;
        for (id, shape) in updates {
            if let Some(i) = self.annotation_index(id) {
                self.shapes[i].shape = shape;
                changed = true;
            }
        }
        if changed {
            self.settle_bindings();
        }
        changed
    }

    /// Finish a drag or a marquee, recording anything that actually moved.
    fn select_release(&mut self) {
        self.snap_guides.clear();

        if let Some((a, b)) = self.marquee.take() {
            let (l, t, r, bo) = normalize_rect(a, b);
            // A marquee that never really opened is just a click on nothing.
            if (r - l) > 3.0 || (bo - t) > 3.0 {
                let hits: Vec<ShapeId> = self
                    .shapes
                    .iter()
                    .filter(|s| !s.is_contained_text())
                    .filter(|s| {
                        let sb = self.shape_bounds_exact(&s.shape);
                        // Touching counts, as in most editors: requiring full
                        // containment makes sweeping up a diagram fiddly.
                        sb.0 <= r && sb.2 >= l && sb.1 <= bo && sb.3 >= t
                    })
                    .map(|s| s.id)
                    .collect();
                if !hits.is_empty() {
                    let n = hits.len();
                    self.selection = Some(Selection {
                        ids: hits,
                        drag: None,
                        grab: Point2D::default(),
                        originals: Vec::new(),
                        original_bounds: (0.0, 0.0, 0.0, 0.0),
                    });
                    self.set_toast("▢", format!("{} selected", n));
                }
            }
            self.request_repaint();
            return;
        }

        let Some(sel) = &mut self.selection else {
            return;
        };
        let Some(kind) = sel.drag.take() else {
            return;
        };
        let originals = sel.originals.clone();
        if let DragKind::Endpoint(is_start) = kind {
            self.rebind_endpoint(is_start);
        }
        let mut items: Vec<(ShapeId, Shape, Shape)> = Vec::new();
        for (id, before) in originals {
            if let Some(after) = self.annotation(id).map(|a| a.shape.clone())
                && before != after
            {
                items.push((id, before, after));
            }
        }
        if !items.is_empty() {
            self.undo_history
                .push(HistoryAction::TransformShapes { items });
            self.redo_history.clear();
        }
        // Shrinking a box below its label would clip words, so it grows back.
        for id in self.selected_ids() {
            if self.label_of(id).is_some() {
                self.grow_container_to_fit(id);
            }
        }
        self.settle_bindings();
    }

    /// Remove everything selected, undoably.
    fn delete_selection(&mut self) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return;
        }
        self.selection = None;
        let n = ids.len();
        if self.delete_annotations(&ids) {
            self.set_toast(
                "🗑️",
                if n == 1 {
                    "Deleted Annotation".to_string()
                } else {
                    format!("Deleted {} Annotations", n)
                },
            );
            self.request_repaint();
        }
    }

    fn delete_annotation(&mut self, id: ShapeId) -> bool {
        self.delete_annotations(&[id])
    }

    /// Remove annotations and any labels bound to them, as one undo step.
    ///
    /// A label whose container is gone would render nowhere, so the two have
    /// to travel together, and come back together.
    fn delete_annotations(&mut self, ids: &[ShapeId]) -> bool {
        let mut all: Vec<ShapeId> = ids.to_vec();
        for id in ids {
            if let Some(label) = self.label_of(*id) {
                all.push(label.id);
            }
        }
        let mut indices: Vec<usize> = all
            .iter()
            .filter_map(|i| self.annotation_index(*i))
            .collect();
        indices.sort_unstable();
        indices.dedup();

        let mut items: Vec<(usize, Annotation)> = Vec::new();
        // Highest index first, so each removal leaves the earlier ones valid.
        for index in indices.into_iter().rev() {
            items.push((index, self.shapes.remove(index)));
        }
        if items.is_empty() {
            return false;
        }
        items.reverse();
        self.undo_history.push(HistoryAction::DeleteShapes { items });
        self.redo_history.clear();
        self.settle_bindings();
        true
    }

    /// Arrow-key nudge, in canvas units.
    fn nudge_selection(&mut self, dx: f32, dy: f32) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return;
        }
        let mut items: Vec<(ShapeId, Shape, Shape)> = Vec::new();
        for id in ids {
            let Some(index) = self.annotation_index(id) else {
                continue;
            };
            let before = self.shapes[index].shape.clone();
            let mut after = before.clone();
            translate_shape(&mut after, dx, dy);
            self.shapes[index].shape = after.clone();
            items.push((id, before, after));
        }
        if items.is_empty() {
            return;
        }
        self.undo_history
            .push(HistoryAction::TransformShapes { items });
        self.redo_history.clear();
        self.settle_bindings();
        self.request_repaint();
    }

    /// Double-clicking a text annotation re-opens it in the editor. The shape
    /// is lifted out of the list, so committing puts the edited version back.
    fn reopen_selected_text(&mut self) -> bool {
        let Some(id) = self.selection.as_ref().and_then(|s| s.only()) else {
            return false;
        };
        let Some(index) = self.annotation_index(id) else {
            return false;
        };
        if !matches!(self.shapes[index].shape, Shape::Text { .. }) {
            return false;
        }
        let annotation = self.shapes.remove(index);
        let Shape::Text {
            origin,
            text,
            font_size,
            color,
            is_bold,
            is_italic,
            card_style,
            font_family,
        } = annotation.shape.clone()
        else {
            return false;
        };
        self.undo_history.push(HistoryAction::DeleteShape {
            index,
            shape: annotation,
        });
        self.redo_history.clear();
        self.selection = None;

        let mut editor = TextEditorState::new(
            origin, color, font_size, is_bold, is_italic, card_style, font_family,
        );
        editor.cursor = text.len();
        editor.text = text;
        self.text_editor = Some(editor);
        self.set_toast("✏️", "Editing Text (Esc to finish)");
        self.request_repaint();
        true
    }

    pub fn commit_text_editor(&mut self) {
        if let Some(editor) = self.text_editor.take() {
            if !editor.text.trim().is_empty() {
                let container = editor.container;
                let id = self.push_shape(Shape::Text {
                    origin: editor.origin,
                    text: editor.text,
                    font_size: editor.font_size,
                    color: editor.color,
                    is_bold: editor.is_bold,
                    is_italic: editor.is_italic,
                    card_style: editor.card_style,
                    font_family: editor.font_family,
                });
                if let Some(cid) = container {
                    if self.annotation(cid).is_some() {
                        if let Some(a) = self.shapes.iter_mut().find(|a| a.id == id) {
                            a.container = Some(cid);
                        }
                        self.grow_container_to_fit(cid);
                    } else {
                        // The container went away mid-edit; leave the text as
                        // free-floating rather than losing what was typed.
                        if let Some(a) = self.shapes.iter_mut().find(|a| a.id == id) {
                            a.container = None;
                        }
                    }
                }
            }
            self.request_repaint();
        }
    }

    /// Line the selection up, or spread it evenly.
    ///
    /// Both are one undo step. Labels are excluded: a label has no position of
    /// its own to align, it goes wherever its container is.
    fn arrange_selection(&mut self, align: Option<AlignTo>, distribute_h: Option<bool>) {
        let ids: Vec<ShapeId> = self
            .selected_ids()
            .into_iter()
            .filter(|id| {
                self.annotation(*id)
                    .map(|a| !a.is_contained_text())
                    .unwrap_or(false)
            })
            .collect();
        if ids.len() < 2 {
            self.set_toast("↔", "Select two or more first");
            return;
        }
        let boxes: Vec<(f32, f32, f32, f32)> = ids
            .iter()
            .filter_map(|id| self.annotation(*id).map(|a| self.shape_bounds_exact(&a.shape)))
            .collect();
        if boxes.len() != ids.len() {
            return;
        }

        let (offsets, label) = match (align, distribute_h) {
            (Some(to), _) => (
                align_offsets(&boxes, to),
                match to {
                    AlignTo::Left => "Aligned Left",
                    AlignTo::Right => "Aligned Right",
                    AlignTo::HCentre => "Centred Horizontally",
                    AlignTo::Top => "Aligned Top",
                    AlignTo::Bottom => "Aligned Bottom",
                    AlignTo::VCentre => "Centred Vertically",
                },
            ),
            (None, Some(h)) => {
                if ids.len() < 3 {
                    self.set_toast("↔", "Select three or more to distribute");
                    return;
                }
                (
                    distribute_offsets(&boxes, h),
                    if h {
                        "Distributed Horizontally"
                    } else {
                        "Distributed Vertically"
                    },
                )
            }
            _ => return,
        };

        let mut items: Vec<(ShapeId, Shape, Shape)> = Vec::new();
        for (id, (dx, dy)) in ids.iter().zip(offsets) {
            if dx.abs() < 0.01 && dy.abs() < 0.01 {
                continue;
            }
            let Some(index) = self.annotation_index(*id) else {
                continue;
            };
            let before = self.shapes[index].shape.clone();
            let mut after = before.clone();
            translate_shape(&mut after, dx, dy);
            self.shapes[index].shape = after.clone();
            items.push((*id, before, after));
        }
        if items.is_empty() {
            return;
        }
        self.undo_history
            .push(HistoryAction::TransformShapes { items });
        self.redo_history.clear();
        self.settle_bindings();
        self.set_toast("↔", label);
        self.request_repaint();
    }

    // Stacking and duplication

    /// Move the selection to the front or the back of the stack.
    ///
    /// Later in the list means drawn later, so the end of the vector is the
    /// front. A label rides with its container regardless, because labels are
    /// drawn in a pass of their own after every other shape.
    fn restack_selection(&mut self, to_front: bool) {
        let Some(id) = self.selection.as_ref().and_then(|s| s.only()) else {
            return;
        };
        let Some(from) = self.annotation_index(id) else {
            return;
        };
        let to = if to_front { self.shapes.len() - 1 } else { 0 };
        if from == to {
            return;
        }
        let a = self.shapes.remove(from);
        self.shapes.insert(to, a);
        self.undo_history
            .push(HistoryAction::Reorder { id, from, to });
        self.redo_history.clear();
        let (icon, label) = if to_front {
            ("⬆", "Brought to Front")
        } else {
            ("⬇", "Sent to Back")
        };
        self.set_toast(icon, label);
        self.request_repaint();
    }

    /// Copy the selection, offset a little, and select the copy.
    ///
    /// A container brings its label, with the copy's label pointed at the
    /// copy — otherwise both boxes would share one label and moving either
    /// would drag the same words around.
    fn duplicate_selection(&mut self) {
        let Some(id) = self.selection.as_ref().and_then(|s| s.only()) else {
            return;
        };
        let Some(source) = self.annotation(id).cloned() else {
            return;
        };
        const OFFSET: f32 = 16.0;

        let mut copy = source.clone();
        copy.id = ShapeId::fresh();
        translate_shape(&mut copy.shape, OFFSET, OFFSET);
        let new_id = copy.id;

        let label_copy = self.label_of(source.id).cloned().map(|mut l| {
            l.id = ShapeId::fresh();
            l.container = Some(new_id);
            translate_shape(&mut l.shape, OFFSET, OFFSET);
            l
        });

        self.shapes.push(copy.clone());
        self.undo_history.push(HistoryAction::AddShape(copy));
        if let Some(l) = label_copy {
            self.shapes.push(l.clone());
            self.undo_history.push(HistoryAction::AddShape(l));
        }
        self.redo_history.clear();

        self.selection = Some(Selection::single(new_id));
        self.set_toast("⧉", "Duplicated");
        self.request_repaint();
    }

    // ─────────────────────── Arrow binding ───────────────────────

    /// Rewrite every bound arrow's endpoints from what it is anchored to.
    ///
    /// Endpoints are derived, so rather than teaching hit-testing, bounds,
    /// snapping and export about bindings, they are written back into the
    /// stored geometry after any mutation and everything downstream carries on
    /// reading plain shapes.
    fn settle_bindings(&mut self) {
        let bound: Vec<usize> = self
            .shapes
            .iter()
            .enumerate()
            .filter(|(_, a)| a.start_bound.is_some() || a.end_bound.is_some())
            .map(|(i, _)| i)
            .collect();
        if bound.is_empty() {
            return;
        }
        for i in bound {
            let (shape, sb, eb) = {
                let a = &self.shapes[i];
                (a.shape.clone(), a.start_bound, a.end_bound)
            };
            // A target that is gone keeps its binding and its last endpoint, so
            // undoing the deletion re-attaches instead of orphaning the arrow.
            let start_target = sb.and_then(|id| self.annotation(id)).map(|a| a.shape.clone());
            let end_target = eb.and_then(|id| self.annotation(id)).map(|a| a.shape.clone());
            if let Some((s, e)) = resolve_arrow_ends(
                &shape,
                start_target.as_ref(),
                end_target.as_ref(),
                ARROW_BINDING_GAP,
            ) {
                self.shapes[i].shape = with_arrow_ends(&shape, s, e);
            }
        }
    }

    /// Re-anchor one end of the selected arrow to whatever it was dropped on.
    ///
    /// Dropping it on empty canvas frees that end, which is the only way to
    /// detach an arrow once it has latched on.
    fn rebind_endpoint(&mut self, is_start: bool) {
        let Some(id) = self.selection.as_ref().and_then(|s| s.only()) else {
            return;
        };
        let Some(index) = self.annotation_index(id) else {
            return;
        };
        let (start, end) = match &self.shapes[index].shape {
            Shape::Arrow { start, end, .. } | Shape::Line { start, end, .. } => (*start, *end),
            _ => return,
        };
        let at = if is_start { start } else { end };
        let target = self.bind_target_at(at, id);
        // Both ends on one shape is an arrow to nowhere.
        let other = if is_start {
            self.shapes[index].end_bound
        } else {
            self.shapes[index].start_bound
        };
        let target = if target.is_some() && target == other {
            None
        } else {
            target
        };
        if is_start {
            self.shapes[index].start_bound = target;
        } else {
            self.shapes[index].end_bound = target;
        }
        self.settle_bindings();
        self.set_toast(
            "🔗",
            if target.is_some() {
                "Anchored"
            } else {
                "Detached"
            },
        );
    }

    /// The topmost shape an arrow could anchor to at this point.
    fn bind_target_at(&self, canvas_pt: Point2D, exclude: ShapeId) -> Option<ShapeId> {
        self.shapes
            .iter()
            .rev()
            .find(|a| {
                a.id != exclude
                    && !a.is_contained_text()
                    && can_bind_arrow(&a.shape)
                    && shape_intersects_circle(&a.shape, canvas_pt, 4.0)
            })
            .map(|a| a.id)
    }

    /// Anchor a freshly drawn line or arrow to whatever its ends landed on.
    fn bind_new_arrow(&mut self, id: ShapeId) {
        let Some(index) = self.annotation_index(id) else {
            return;
        };
        let (start, end) = match &self.shapes[index].shape {
            Shape::Arrow { start, end, .. } | Shape::Line { start, end, .. } => (*start, *end),
            _ => return,
        };
        let sb = self.bind_target_at(start, id);
        let eb = self.bind_target_at(end, id);
        // Both ends on the same shape would be an arrow to nowhere.
        let (sb, eb) = if sb.is_some() && sb == eb {
            (None, None)
        } else {
            (sb, eb)
        };
        if sb.is_none() && eb.is_none() {
            return;
        }
        self.shapes[index].start_bound = sb;
        self.shapes[index].end_bound = eb;
        self.settle_bindings();
        let what = match (sb.is_some(), eb.is_some()) {
            (true, true) => "Arrow anchored at both ends",
            (true, false) => "Arrow anchored at its tail",
            _ => "Arrow anchored at its head",
        };
        self.set_toast("🔗", what);
    }

    /// The label bound to `container`, if it has one.
    fn label_of(&self, container: ShapeId) -> Option<&Annotation> {
        self.shapes.iter().find(|a| a.container == Some(container))
    }

    /// Grow a container's height until its label fits.
    ///
    /// Only ever grows: the box is the thing the user sized, but silently
    /// clipping words is worse than a taller box.
    fn grow_container_to_fit(&mut self, container: ShapeId) {
        let Some(label) = self.label_of(container) else {
            return;
        };
        let Shape::Text {
            text,
            font_size,
            is_bold,
            is_italic,
            font_family,
            ..
        } = label.shape.clone()
        else {
            return;
        };
        let Some(idx) = self.annotation_index(container) else {
            return;
        };
        if label_rides_on_shape(&self.shapes[idx].shape) {
            return;
        }
        let bounds = shape_bounds(&self.shapes[idx].shape);
        let pad = TextEditorState::CONTAINER_PADDING;
        let wrap = ((bounds.2 - bounds.0) - pad * 2.0).max(24.0);
        let (_, h) = self.renderer.measure_text_block(
            &text, font_size, is_bold, is_italic, font_family, wrap,
        );
        let needed = container_height_for(bounds, h, pad);
        if needed > bounds.3 - bounds.1 + 0.5 {
            let to = (bounds.0, bounds.1, bounds.2, bounds.1 + needed);
            let mut grown = self.shapes[idx].shape.clone();
            resize_shape(&mut grown, bounds, to);
            self.shapes[idx].shape = grown;
        }
    }

    /// Open the editor on `container`'s label, creating one if it has none.
    fn edit_container_label(&mut self, container: ShapeId) -> bool {
        let Some(owner) = self.annotation(container) else {
            return false;
        };
        if !can_contain_text(&owner.shape) {
            return false;
        }
        let owner_shape = owner.shape.clone();
        let bounds = shape_bounds(&owner_shape);

        // An existing label is lifted out and put back on commit, so editing
        // and creating follow exactly the same path.
        let existing = self.label_of(container).map(|a| (a.id, a.shape.clone()));
        let (text, font_size, color, is_bold, is_italic, card_style, font_family) = match existing {
            Some((
                id,
                Shape::Text {
                    text,
                    font_size,
                    color,
                    is_bold,
                    is_italic,
                    card_style,
                    font_family,
                    ..
                },
            )) => {
                if let Some(index) = self.annotation_index(id) {
                    let removed = self.shapes.remove(index);
                    self.undo_history.push(HistoryAction::DeleteShape {
                        index,
                        shape: removed,
                    });
                    self.redo_history.clear();
                }
                (
                    text,
                    font_size,
                    color,
                    is_bold,
                    is_italic,
                    card_style,
                    font_family,
                )
            }
            _ => (
                String::new(),
                self.font_size,
                self.current_color,
                self.text_is_bold,
                self.text_is_italic,
                self.text_card_style,
                self.text_font_family,
            ),
        };

        let mut editor = TextEditorState::new(
            Point2D::new(bounds.0, bounds.1),
            color,
            font_size,
            is_bold,
            is_italic,
            card_style,
            font_family,
        );
        editor.cursor = text.len();
        editor.text = text;
        editor.container = Some(container);
        editor.container_bounds = Some(bounds);
        editor.container_wraps = !label_rides_on_shape(&owner_shape);
        self.text_editor = Some(editor);
        self.selection = None;
        self.set_toast("✏️", "Label (Esc to finish)");
        self.request_repaint();
        true
    }

    /// The topmost container-capable shape under a canvas point.
    fn container_at(&self, canvas_pt: Point2D) -> Option<ShapeId> {
        self.shapes
            .iter()
            .rev()
            .find(|a| {
                can_contain_text(&a.shape)
                    && shape_intersects_circle(&a.shape, canvas_pt, 2.0)
            })
            .map(|a| a.id)
    }

    pub fn get_composite_capture(&self, include_spotlight: bool) -> Option<ScreenCapture> {
        let text_input = self.text_editor.as_ref();
        let bg_pixels = self
            .background_capture
            .as_ref()
            .map(|c| c.pixels.as_slice());
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
        let Some(composite) = self.get_composite_capture(self.spotlight.active) else {
            self.set_toast("❌", "Nothing to copy");
            return;
        };
        // Previously a failed copy was silent, so the user had no idea the
        // clipboard still held whatever was there before.
        if copy_bgra_to_clipboard(composite.width, composite.height, &composite.pixels) {
            self.set_toast("📋", "Copied Screen + Drawings to Clipboard!");
        } else {
            self.set_toast("❌", "Clipboard busy — copy failed, try again");
        }
    }

    /// Resolve the user's real Pictures folder. `%USERPROFILE%\Pictures` is wrong
    /// whenever the folder is redirected - to OneDrive, another drive, or a
    /// network share - which is common. The shell knows where it actually is.
    // ─────────────────────── Sessions ───────────────────────

    /// Suspend always-on-top while a modal shell dialog is up. A topmost
    /// fullscreen overlay would otherwise render straight over the dialog and
    /// leave it unreachable.
    fn with_topmost_suspended<T>(&self, f: impl FnOnce() -> T) -> T {
        use windows::Win32::UI::WindowsAndMessaging::{
            HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
        };
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_NOTOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE,
            );
        }
        let out = f();
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE,
            );
        }
        out
    }

    /// Write the canvas to a session file.
    ///
    /// `manual` is the Ctrl+Shift+S path, which reports through a toast. The
    /// autosave path runs while the overlay is tearing down, where a toast can
    /// no longer be drawn, so it routes failures to a tray balloon instead.
    pub fn save_session(&mut self, manual: bool) {
        if self.shapes.is_empty() {
            if manual {
                self.set_toast("❌", "Nothing to save");
            }
            return;
        }
        let cfg = crate::config::AppConfig::load();
        let dir = crate::session::sessions_dir(&cfg);
        let path = crate::session::next_session_path(&dir);
        let sess = crate::session::Session::new(
            self.shapes.clone(),
            self.background_type,
            self.step_counter,
        );

        if let Err(e) = crate::session::save(&path, &sess) {
            let msg = format!("Session save failed: {}", e);
            if manual {
                self.set_toast("❌", msg);
            } else {
                crate::session::report_error(msg);
            }
            return;
        }

        if cfg.session_export_png
            && let Some(composite) = self.get_composite_capture(false)
        {
            let png = path.with_extension("png");
            let _ = composite.save_png(&png.to_string_lossy());
        }

        crate::session::prune(&dir, cfg.session_keep_last as usize);

        if manual {
            let name = path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            let count = self.shapes.len();
            self.set_toast(
                "💾",
                format!("Saved {} annotation(s) — {}", count, name),
            );
        }
    }

    /// Replace the canvas with a saved session. Undoable in one step.
    pub fn load_session_from(&mut self, path: &std::path::Path) {
        match crate::session::load(path) {
            Ok(sess) => {
                let count = sess.shapes.len();
                let background = sess.background_enum();
                let previous = std::mem::replace(&mut self.shapes, sess.shapes);
                self.undo_history.push(HistoryAction::Clear(previous));
                self.redo_history.clear();
                self.selection = None;
                self.active_shape = None;
                self.text_editor = None;
                self.is_drawing = false;
                self.background_type = background;
                self.step_counter = sess.step_counter;
                self.toolbar.badge_counter = self.step_counter;
                self.settle_bindings();
                self.set_toast(
                    "📂",
                    format!("Loaded {} annotation(s) — {}", count, sess.saved_at),
                );
                self.request_repaint();
            }
            Err(e) => self.set_toast("❌", format!("Load failed: {}", e)),
        }
    }

    /// Ctrl+O: pick a session file and load it.
    pub fn load_session_via_dialog(&mut self) {
        let cfg = crate::config::AppConfig::load();
        let dir = crate::session::sessions_dir(&cfg);
        let _ = std::fs::create_dir_all(&dir);
        let hwnd = self.hwnd;
        let picked = self.with_topmost_suspended(|| crate::session::pick_session_file(hwnd, &dir));
        match picked {
            Some(path) => {
                self.load_session_from(&path);
                self.force_foreground();
            }
            None => {
                self.force_foreground();
                self.request_repaint();
            }
        }
    }

    pub fn save_snapshot(&mut self) {
        let Some(composite) = self.get_composite_capture(self.spotlight.active) else {
            self.set_toast("❌", "Nothing to save");
            return;
        };

        let dir = crate::session::pictures_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.set_toast("❌", format!("Cannot open Pictures folder: {}", e));
            return;
        }

        // Local wall-clock stamp, readable and sortable.
        let stamp = unsafe {
            let t = windows::Win32::System::SystemInformation::GetLocalTime();
            format!(
                "{:04}{:02}{:02}-{:02}{:02}{:02}",
                t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
            )
        };

        // Second-resolution stamps collide when saving in quick succession, which
        // previously overwrote the earlier shot without a word. Suffix instead.
        let mut path = dir.join(format!("Zoomify_{}.png", stamp));
        let mut n = 2;
        while path.exists() && n < 1000 {
            path = dir.join(format!("Zoomify_{}_{}.png", stamp, n));
            n += 1;
        }

        let name = path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| "screenshot.png".to_string());

        match composite.save_png(&path.to_string_lossy()) {
            Ok(()) => self.set_toast("💾", format!("Saved {}", name)),
            Err(e) => self.set_toast("❌", format!("Save failed: {}", e)),
        }
    }

    // ─────────────────────── Snapping ───────────────────────

    /// Snap tolerance in canvas units. The constant is in screen DIPs, so
    /// dividing by the zoom keeps the pull feeling identical however far in
    /// the canvas is scaled.
    fn snap_tolerance(&self) -> f32 {
        SNAP_TOLERANCE_DIP / self.zoom.level.max(0.05)
    }

    /// True unless the user is holding Alt, the live override for one gesture.
    fn snapping_enabled(&self) -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::VK_MENU;
        if !self.snap_to_shapes {
            return false;
        }
        let alt = unsafe { GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000 } != 0;
        !alt
    }

    /// Rebuild the anchor set at the start of a gesture, leaving out `skip`
    /// so a shape never snaps to itself.
    fn rebuild_snap_anchors(&mut self, skip: Option<ShapeId>) {
        match skip {
            Some(id) => self.rebuild_snap_anchors_excluding(&[id]),
            None => self.rebuild_snap_anchors_excluding(&[]),
        }
    }

    fn rebuild_snap_anchors_excluding(&mut self, skip: &[ShapeId]) {
        self.snap_anchors = if self.snapping_enabled() {
            let live: Vec<Shape> = self
                .shapes
                .iter()
                .filter(|a| !skip.contains(&a.id) && !a.is_contained_text())
                .map(|a| a.shape.clone())
                .collect();
            collect_anchors(&live)
        } else {
            Vec::new()
        };
    }

    /// Pull a moving point onto nearby geometry, recording the guides to draw.
    fn apply_point_snap(&mut self, pt: Point2D) -> Point2D {
        if !self.snapping_enabled() {
            self.snap_guides.clear();
            return pt;
        }
        let tol = self.snap_tolerance();
        match snap_point(pt, &self.snap_anchors, tol) {
            Some((snapped, guides)) => {
                self.snap_guides = guides;
                snapped
            }
            None => {
                self.snap_guides.clear();
                pt
            }
        }
    }

    fn clear_snap(&mut self) {
        self.snap_guides.clear();
        self.snap_anchors.clear();
    }

    /// First entry of a new stroke's pressure track, or empty for input with
    /// no pressure axis (mouse, touch), which keeps the stroke uniform-width.
    fn pressure_seed(&self) -> Vec<f32> {
        match self.pen_pressure {
            Some(p) => vec![p],
            None => Vec::new(),
        }
    }

    pub fn create_shape_for_tool(&self, canvas_pt: Point2D) -> Shape {
        match self.current_tool {
            DrawTool::Highlighter => Shape::Stroke {
                points: vec![canvas_pt],
                color: self.current_color,
                width: self.highlighter_settings.stroke_width,
                is_highlighter: true,
                pattern: StrokePattern::Solid,
                pressures: self.pressure_seed(),
            },
            DrawTool::Line => Shape::Line {
                start: canvas_pt,
                end: canvas_pt,
                color: self.current_color,
                width: self.line_settings.stroke_width,
                pattern: self.line_settings.pattern,
            },
            DrawTool::Arrow => Shape::Arrow {
                start: canvas_pt,
                end: canvas_pt,
                color: self.current_color,
                width: self.arrow_settings.stroke_width,
                style: self.arrow_settings.style,
                pattern: self.arrow_settings.pattern,
            },
            DrawTool::Rectangle => Shape::Rectangle {
                start: canvas_pt,
                end: canvas_pt,
                color: self.current_color,
                width: self.rect_settings.stroke_width,
                rounded: false,
                fill: self.rect_settings.fill_mode,
                pattern: self.rect_settings.pattern,
            },
            DrawTool::RoundedRectangle => Shape::Rectangle {
                start: canvas_pt,
                end: canvas_pt,
                color: self.current_color,
                width: self.rounded_rect_settings.stroke_width,
                rounded: true,
                fill: self.rounded_rect_settings.fill_mode,
                pattern: self.rounded_rect_settings.pattern,
            },
            DrawTool::Ellipse => Shape::Ellipse {
                start: canvas_pt,
                end: canvas_pt,
                color: self.current_color,
                width: self.ellipse_settings.stroke_width,
                fill: self.ellipse_settings.fill_mode,
                pattern: self.ellipse_settings.pattern,
            },
            DrawTool::Blur => Shape::Blur {
                start: canvas_pt,
                end: canvas_pt,
                block_size: self.blur_settings.block_size,
            },
            _ => Shape::Stroke {
                points: vec![canvas_pt],
                color: self.current_color,
                width: self.pen_settings.stroke_width,
                is_highlighter: false,
                pattern: self.pen_settings.pattern,
                pressures: self.pressure_seed(),
            },
        }
    }

    pub fn handle_fluent_action(&mut self, action: FluentAction) -> LRESULT {
        match action {
            FluentAction::ModeZoom => {
                self.enter_static_zoom();
            }
            FluentAction::ModeDraw => {
                self.enter_draw_mode();
            }
            FluentAction::ModeSpotlight => {
                if self.mode == AppMode::Spotlight {
                    self.exit_overlay();
                } else if self.mode == AppMode::StaticZoom || self.mode == AppMode::Draw {
                    self.toggle_spotlight();
                } else {
                    self.enter_spotlight_mode();
                }
            }
            FluentAction::ModeTimer => {
                self.enter_timer_mode(0);
            }
            FluentAction::ModeLoupe => {
                self.enter_loupe_mode();
            }
            FluentAction::CycleDisplay => {
                self.cycle_next_monitor();
            }
            FluentAction::Tool(t) => {
                self.ensure_draw_mode();
                self.current_tool = t;
                self.toolbar.active_tool = Some(t);
                self.sync_tool_to_toolbar();
                let sw = self.logical_w();
                let sh = self.logical_h();
                self.toolbar.update_layout(sw, sh);
                self.set_toast("🛠️", format!("Tool: {}", t.name()));
            }
            FluentAction::OpenColorPicker => {
                self.ensure_draw_mode();
                let open = !self.color_picker.open;
                self.color_picker.open = open;
                if open {
                    let cur = self.current_color;
                    self.color_picker.seed_from(cur);
                    self.relayout_color_picker();
                    self.set_toast("🎨", "Custom colour — drag H/S/V, click a recent swatch");
                }
            }
            FluentAction::Color(c) => {
                self.ensure_draw_mode();
                self.current_color = c;
                self.set_toast("🎨", format!("Color: {}", c.name()));
            }
            FluentAction::Undo => {
                self.undo();
            }
            FluentAction::Clear => {
                self.clear_all();
            }
            FluentAction::Copy => {
                self.copy_screen_to_clipboard();
            }
            FluentAction::Save => {
                self.save_snapshot();
            }
            FluentAction::Close => {
                self.exit_overlay();
                return LRESULT(0);
            }
            FluentAction::ToggleCollapse => {
                let sw = self.logical_w();
                let sh = self.logical_h();
                self.toolbar.collapsed = !self.toolbar.collapsed;
                self.toolbar.update_layout(sw, sh);
            }
            FluentAction::SetStrokeWidth(w) => {
                match self.current_tool {
                    DrawTool::Pen => self.pen_settings.stroke_width = w,
                    DrawTool::Highlighter => self.highlighter_settings.stroke_width = w,
                    DrawTool::Line => self.line_settings.stroke_width = w,
                    DrawTool::Arrow => self.arrow_settings.stroke_width = w,
                    DrawTool::Rectangle => self.rect_settings.stroke_width = w,
                    DrawTool::RoundedRectangle => self.rounded_rect_settings.stroke_width = w,
                    DrawTool::Ellipse => self.ellipse_settings.stroke_width = w,
                    DrawTool::StepBadge => self.badge_settings.stroke_width = w,
                    DrawTool::Blur => self.blur_settings.block_size = w,
                    _ => {}
                }
                self.stroke_width = w;
                self.toolbar.stroke_width = w;
                let sw = self.logical_w();
                let sh = self.logical_h();
                self.toolbar.update_layout(sw, sh);
                // Not persisted per click; save_config() runs on overlay exit.
                if self.current_tool == DrawTool::Blur {
                    self.set_toast("░", format!("Mosaic Block Size: {:.0}px", w));
                } else {
                    self.set_toast("✏️", format!("Stroke Width: {:.0}px", w));
                }
            }
            FluentAction::SetFillMode(fm) => {
                match self.current_tool {
                    DrawTool::Rectangle => self.rect_settings.fill_mode = fm,
                    DrawTool::RoundedRectangle => self.rounded_rect_settings.fill_mode = fm,
                    DrawTool::Ellipse => self.ellipse_settings.fill_mode = fm,
                    DrawTool::StepBadge => self.badge_settings.fill = fm,
                    _ => {}
                }
                self.fill_mode = fm;
                self.toolbar.current_fill_mode = fm;
                let sw = self.logical_w();
                let sh = self.logical_h();
                self.toolbar.update_layout(sw, sh);
                self.set_toast("🎨", format!("Fill: {}", fm.name()));
            }
            FluentAction::SetStrokePattern(sp) => {
                match self.current_tool {
                    DrawTool::Pen => self.pen_settings.pattern = sp,
                    DrawTool::Line => self.line_settings.pattern = sp,
                    DrawTool::Arrow => self.arrow_settings.pattern = sp,
                    DrawTool::Rectangle => self.rect_settings.pattern = sp,
                    DrawTool::RoundedRectangle => self.rounded_rect_settings.pattern = sp,
                    DrawTool::Ellipse => self.ellipse_settings.pattern = sp,
                    _ => {}
                }
                self.stroke_pattern = sp;
                self.toolbar.current_stroke_pattern = sp;
                let sw = self.logical_w();
                let sh = self.logical_h();
                self.toolbar.update_layout(sw, sh);
                self.set_toast("✏️", format!("Pattern: {}", sp.name()));
            }
            FluentAction::SetArrowStyle(as_) => {
                self.arrow_settings.style = as_;
                self.arrow_style = as_;
                self.toolbar.current_arrow_style = as_;
                let sw = self.logical_w();
                let sh = self.logical_h();
                self.toolbar.update_layout(sw, sh);
                self.set_toast("🏹", format!("Arrow: {}", as_.name()));
            }
            FluentAction::SetBadgeSize(bs) => {
                self.badge_settings.size = bs;
                self.badge_size = bs;
                self.toolbar.current_badge_size = bs;
                let sw = self.logical_w();
                let sh = self.logical_h();
                self.toolbar.update_layout(sw, sh);
                self.set_toast("🔢", format!("Badge Size: {}", bs.name()));
            }
            FluentAction::SetBadgeShape(bsh) => {
                self.badge_settings.shape = bsh;
                self.badge_shape = bsh;
                self.toolbar.current_badge_shape = bsh;
                let sw = self.logical_w();
                let sh = self.logical_h();
                self.toolbar.update_layout(sw, sh);
                self.set_toast("🔢", format!("Badge Shape: {}", bsh.name()));
            }
            FluentAction::ResetBadgeCounter => {
                self.step_counter = 1;
                self.toolbar.badge_counter = 1;
                let sw = self.logical_w();
                let sh = self.logical_h();
                self.toolbar.update_layout(sw, sh);
                self.set_toast("↺", "Step badge reset to #1");
            }
            FluentAction::SetFontSize(sz) => {
                self.text_settings.font_size = sz;
                self.font_size = sz;
                self.toolbar.current_font_size = sz;
                if let Some(ed) = &mut self.text_editor {
                    ed.font_size = sz;
                }
                let sw = self.logical_w();
                let sh = self.logical_h();
                self.toolbar.update_layout(sw, sh);
                self.set_toast("🔤", format!("Font Size: {:.0}px", sz));
            }
            FluentAction::ToggleBold => {
                self.text_settings.is_bold = !self.text_settings.is_bold;
                self.text_is_bold = self.text_settings.is_bold;
                let bold = self.text_is_bold;
                self.toolbar.text_is_bold = bold;
                if let Some(ed) = &mut self.text_editor {
                    ed.is_bold = bold;
                }
                let sw = self.logical_w();
                let sh = self.logical_h();
                self.toolbar.update_layout(sw, sh);
                self.set_toast("𝐁", if bold { "Bold: On" } else { "Bold: Off" });
            }
            FluentAction::ToggleItalic => {
                self.text_settings.is_italic = !self.text_settings.is_italic;
                self.text_is_italic = self.text_settings.is_italic;
                let italic = self.text_is_italic;
                self.toolbar.text_is_italic = italic;
                if let Some(ed) = &mut self.text_editor {
                    ed.is_italic = italic;
                }
                let sw = self.logical_w();
                let sh = self.logical_h();
                self.toolbar.update_layout(sw, sh);
                self.set_toast("𝐼", if italic { "Italic: On" } else { "Italic: Off" });
            }
            FluentAction::SetTextCardStyle(cs) => {
                self.text_settings.card_style = cs;
                self.text_card_style = cs;
                self.toolbar.text_card_style = cs;
                if let Some(ed) = &mut self.text_editor {
                    ed.card_style = cs;
                }
                let sw = self.logical_w();
                let sh = self.logical_h();
                self.toolbar.update_layout(sw, sh);
                self.set_toast("🏷️", format!("Text Card: {}", cs.name()));
            }
            FluentAction::SetFontFamily(ff) => {
                self.text_settings.font_family = ff;
                self.text_font_family = ff;
                self.toolbar.text_font_family = ff;
                if let Some(ed) = &mut self.text_editor {
                    ed.font_family = ff;
                }
                let sw = self.logical_w();
                let sh = self.logical_h();
                self.toolbar.update_layout(sw, sh);
                self.set_toast("🔤", format!("Font: {}", ff.name()));
            }
        }
        self.request_repaint();
        LRESULT(0)
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

            // ── Pen and touch ──
            //
            // Pointer messages are translated into their mouse equivalents so
            // every tool works from one code path, with the pen's pressure
            // recorded on the side. Handling them here (rather than letting
            // DefWindowProc promote them) also stops each contact arriving
            // twice, once as a pointer and once as a synthetic mouse click.
            let (msg, lparam, from_pointer) = match msg {
                WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP => {
                    let Some(sample) = pointer_sample(hwnd, wparam) else {
                        // A mouse wearing a pointer's clothes: leave it to the
                        // WM_MOUSE* path so nothing is handled twice.
                        return DefWindowProcW(hwnd, msg, wparam, lparam);
                    };
                    this.pen_pressure = sample.pressure;
                    let packed = LPARAM(
                        ((sample.client_x as u16 as isize) & 0xFFFF)
                            | (((sample.client_y as u16 as isize) & 0xFFFF) << 16),
                    );
                    let mapped = match msg {
                        WM_POINTERDOWN => {
                            this.pen_active = true;
                            WM_LBUTTONDOWN
                        }
                        WM_POINTERUP => {
                            this.pen_active = false;
                            WM_LBUTTONUP
                        }
                        // A hovering pen still moves the cursor; only a stroke
                        // in progress draws, which `is_drawing` already gates.
                        _ => WM_MOUSEMOVE,
                    };
                    (mapped, packed, true)
                }
                other => (other, lparam, false),
            };

            // While a pen or finger is down the mouse is a bystander. Windows
            // still moves and restores the system cursor around an injected or
            // real pen stroke, and a stray move promoted from that would splice
            // the cursor's position straight into the middle of the stroke.
            if this.pen_active
                && !from_pointer
                && matches!(
                    msg,
                    WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK
                )
            {
                return LRESULT(0);
            }

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
                        let progress = if is_overtime {
                            1.0
                        } else {
                            1.0 - (abs_rem / total).clamp(0.0, 1.0)
                        };
                        timer_info = Some((mins, secs, progress, this.timer_paused, is_overtime));
                    }

                    let text_input = this.text_editor.as_ref();

                    let is_shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                    let snap_guides = this.is_drawing && (is_shift || this.was_shifted_during_draw);

                    this.renderer.render_frame(
                        this.mode,
                        this.logical_w(),
                        this.logical_h(),
                        this.background_bitmap.as_ref(),
                        this.background_type,
                        &this.zoom,
                        &this.spotlight,
                        &this.loupe,
                        &this.shapes,
                        this.active_shape.as_ref(),
                        text_input,
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
                        &this.laser_ripples,
                        this.laser_pos,
                        this.eraser_pos,
                        snap_guides,
                        &this.minimap,
                        this.show_minimap,
                        &this.color_picker,
                        if this.current_tool == DrawTool::Select {
                            this.selection_bounds_screen()
                        } else {
                            None
                        },
                        &this
                            .snap_guides
                            .iter()
                            .map(|g| {
                                let a = this.zoom.canvas_to_screen(g.a);
                                let b = this.zoom.canvas_to_screen(g.b);
                                (a.x, a.y, b.x, b.y, g.marker)
                            })
                            .collect::<Vec<_>>(),
                        if this.current_tool == DrawTool::Select {
                            this.marquee_screen()
                        } else {
                            None
                        },
                        if this.current_tool == DrawTool::Select {
                            this.selection_endpoints_screen()
                        } else {
                            None
                        },
                    );

                    let _ = windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);

                    // Must run after EndPaint: recovery invalidates the window
                    // again, and EndPaint would validate that away.
                    this.recover_device_if_needed();
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
                            if prev_rem > 0.0
                                && this.timer_remaining <= 0.0
                                && !this.timer_alarm_sounded
                            {
                                this.timer_alarm_sounded = true;
                                if this.timer_sound_enabled {
                                    let _ = MessageBeep(0);
                                }
                            }
                            needs_paint = true;
                        } else {
                            this.timer_last_tick = Instant::now();
                        }

                        if let Some(t) = &this.toast
                            && !t.is_expired()
                        {
                            needs_paint = true;
                        }

                        if this.text_editor.is_some() {
                            needs_paint = true;
                        }

                        if this.mode == AppMode::StaticZoom || this.mode == AppMode::Draw {
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            if this.zoom.tick_smooth_pan(0.35, sw, sh) {
                                needs_paint = true;
                            }
                        }

                        // Laser pointer trail decay
                        if !this.laser_trail.is_empty() {
                            let now = Instant::now();
                            let prev_len = this.laser_trail.len();
                            this.laser_trail
                                .retain(|p| now.duration_since(p.timestamp).as_secs_f32() <= 1.2);
                            if !this.laser_trail.is_empty() || prev_len > 0 {
                                needs_paint = true;
                            }
                        }

                        // Laser ripple pulse shockwaves animation & decay
                        if !this.laser_ripples.is_empty() {
                            let now = Instant::now();
                            let prev_len = this.laser_ripples.len();
                            this.laser_ripples
                                .retain(|r| now.duration_since(r.timestamp).as_secs_f32() <= 0.85);
                            if !this.laser_ripples.is_empty() || prev_len > 0 {
                                needs_paint = true;
                            }
                        }

                        // Hold-to-snap smart shape dwell detection (350ms dwell)
                        if this.is_drawing
                            && this.current_tool == DrawTool::Pen
                            && let Some(dwell_t) = this.last_mouse_dwell_time
                            && dwell_t.elapsed().as_millis() >= 350
                            && let Some(Shape::Stroke {
                                points,
                                width,
                                color,
                                ..
                            }) = &this.active_shape
                            && let Some(smart_shape) = recognize_smart_shape(points, *width, *color)
                        {
                            this.active_shape = Some(smart_shape);
                            this.last_mouse_dwell_time = None;
                            this.set_toast("✨", "Auto-snapped Shape");
                            needs_paint = true;
                        }

                        if needs_paint {
                            this.request_repaint();
                        }
                    }
                    LRESULT(0)
                }

                WM_SETCURSOR => {
                    let mut pt = POINT::default();
                    if GetCursorPos(&mut pt).is_ok() {
                        let cx = this.px_to_dip((pt.x - this.screen_x) as f32);
                        let cy = this.px_to_dip((pt.y - this.screen_y) as f32);
                        let sw = this.logical_w();
                        let sh = this.logical_h();
                        if this.toolbar.is_point_inside(cx, cy) {
                            let _ =
                                SetCursor(Some(LoadCursorW(None, IDC_HAND).unwrap_or_default()));
                            return LRESULT(1);
                        }
                        if this.show_minimap
                            && this.zoom.level > 1.05
                            && (this.mode == AppMode::StaticZoom || this.mode == AppMode::Draw)
                            && this.minimap.hit_test(sw, sh, cx, cy)
                        {
                            let cursor_type = if this.minimap.is_dragging {
                                IDC_SIZEALL
                            } else {
                                IDC_HAND
                            };
                            let _ =
                                SetCursor(Some(LoadCursorW(None, cursor_type).unwrap_or_default()));
                            return LRESULT(1);
                        }
                        // Select tool: arrow by default, move cursor over a
                        // picked shape or one of its grips.
                        if this.current_tool == DrawTool::Select
                            && (this.mode == AppMode::Draw || this.mode == AppMode::StaticZoom)
                        {
                            let over = this
                                .selection_bounds_screen()
                                .map(|(l, t, r, b)| {
                                    let reach = 8.0;
                                    cx >= l - reach
                                        && cx <= r + reach
                                        && cy >= t - reach
                                        && cy <= b + reach
                                })
                                .unwrap_or(false);
                            let cursor_type = if over { IDC_SIZEALL } else { IDC_ARROW };
                            let _ =
                                SetCursor(Some(LoadCursorW(None, cursor_type).unwrap_or_default()));
                            return LRESULT(1);
                        }
                    }
                    if this.mode == AppMode::Timer {
                        if this.timer_widget.hover_action.is_some() {
                            let _ =
                                SetCursor(Some(LoadCursorW(None, IDC_HAND).unwrap_or_default()));
                        } else if this.timer_widget.is_dragging {
                            let _ =
                                SetCursor(Some(LoadCursorW(None, IDC_SIZEALL).unwrap_or_default()));
                        } else {
                            let _ =
                                SetCursor(Some(LoadCursorW(None, IDC_ARROW).unwrap_or_default()));
                        }
                        return LRESULT(1);
                    }
                    if this.mode == AppMode::Loupe {
                        let _ = SetCursor(Some(LoadCursorW(None, IDC_ARROW).unwrap_or_default()));
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
                    // Mouse messages are physical pixels; everything below is DIPs.
                    let x = this.px_to_dip((lparam.0 & 0xFFFF) as i16 as f32);
                    let y = this.px_to_dip(((lparam.0 >> 16) & 0xFFFF) as i16 as f32);
                    let screen_pt = Point2D::new(x, y);
                    let canvas_pt = this.zoom.screen_to_canvas(screen_pt);
                    let sw = this.logical_w();
                    let sh = this.logical_h();

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
                        this.timer_widget.hover_action =
                            this.timer_widget.get_action_at(screen_pt, sw, sh);
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

                    // Minimap dragging & hover
                    if this.show_minimap
                        && this.zoom.level > 1.05
                        && (this.mode == AppMode::StaticZoom || this.mode == AppMode::Draw)
                    {
                        if this.minimap.is_dragging {
                            let target_canvas = this.minimap.minimap_pt_to_canvas_pt(sw, sh, x, y);
                            this.zoom.center_on_canvas_point(target_canvas, sw, sh);
                            this.request_repaint();
                            return LRESULT(0);
                        }

                        let is_hov = this.minimap.hit_test(sw, sh, x, y);
                        if is_hov != this.minimap.is_hovered {
                            this.minimap.is_hovered = is_hov;
                            this.request_repaint();
                        }
                        if is_hov {
                            return LRESULT(0);
                        }
                    } else if this.minimap.is_hovered {
                        this.minimap.is_hovered = false;
                    }

                    // Laser pointer tracking
                    if this.current_tool == DrawTool::LaserPointer {
                        this.laser_pos = Some(canvas_pt);

                        // Only record a point once the cursor has actually moved,
                        // and cap the trail. Previously every mouse message pushed
                        // a point, so a high-polling-rate mouse could pile up over
                        // a thousand near-coincident points, all redrawn each frame.
                        let far_enough = match this.laser_trail.last() {
                            Some(last) => last.pt.distance(&canvas_pt) >= 2.0,
                            None => true,
                        };
                        if far_enough {
                            if this.laser_trail.len() >= MAX_LASER_TRAIL_POINTS {
                                this.laser_trail.remove(0);
                            }
                            this.laser_trail.push(LaserTrailPoint {
                                pt: canvas_pt,
                                timestamp: Instant::now(),
                            });
                            this.request_repaint();
                        }
                    } else {
                        this.laser_pos = None;
                    }

                    // Select tool: drag the picked shape or one of its grips.
                    if this.current_tool == DrawTool::Select {
                        if this.select_drag(canvas_pt) {
                            this.request_repaint();
                        }
                        return LRESULT(0);
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
                            if shape_intersects_circle(&shape.shape, canvas_pt, 16.0) {
                                erased_idx = Some(idx);
                                break;
                            }
                        }
                        if let Some(idx) = erased_idx {
                            let erased_id = this.shapes[idx].id;
                            this.selection = None;
                            if this.delete_annotation(erased_id) {
                                this.set_toast("🧹", "Erased Shape");
                            }
                            this.request_repaint();
                        }
                        return LRESULT(0);
                    }

                    if let Some(bar) = this.color_picker.dragging {
                        this.color_picker.set_from_x(bar, x);
                        let c = this.color_picker.current();
                        this.current_color = c;
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.spotlight.active && !this.spotlight.pinned {
                        this.spotlight.x = x;
                        this.spotlight.y = y;
                        this.request_repaint();
                    }

                    if this.mode == AppMode::Loupe && !this.loupe.pinned {
                        this.loupe.x = x;
                        this.loupe.y = y;
                        this.request_repaint();
                    }

                    if (this.mode == AppMode::StaticZoom
                        || (this.mode == AppMode::Draw && this.zoom.level > 1.001))
                        && !this.is_drawing
                        && !this.zoom.is_dragging
                    {
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

                    let prev_mouse_pos = this.last_mouse_pos;
                    this.last_mouse_pos = screen_pt;

                    if this.is_drawing {
                        if this.current_tool == DrawTool::Pen
                            && screen_pt.distance(&prev_mouse_pos) > 5.0
                        {
                            this.last_mouse_dwell_time = Some(Instant::now());
                        }

                        let is_shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                        let is_ctrl = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
                        this.was_shifted_during_draw = is_shift;
                        let start_pt = this.draw_start_pt;

                        // A held modifier is an explicit constraint of its own,
                        // so shape snapping stands aside rather than fighting it.
                        let freehand = matches!(this.active_shape, Some(Shape::Stroke { .. }));
                        let canvas_pt = if freehand || is_shift || is_ctrl {
                            if !freehand {
                                this.snap_guides.clear();
                            }
                            canvas_pt
                        } else {
                            this.apply_point_snap(canvas_pt)
                        };

                        let pen_pressure = this.pen_pressure;
                        match &mut this.active_shape {
                            Some(Shape::Stroke {
                                points, pressures, ..
                            }) => {
                                let should_push = match points.last() {
                                    Some(last) => last.distance(&canvas_pt) >= 1.5,
                                    None => true,
                                };
                                if should_push {
                                    points.push(canvas_pt);
                                    push_pressure(pressures, pen_pressure, points.len());
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
                            Some(Shape::Blur { end, .. }) => {
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

                WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                    // Mouse messages are physical pixels; everything below is DIPs.
                    let x = this.px_to_dip((lparam.0 & 0xFFFF) as i16 as f32);
                    let y = this.px_to_dip(((lparam.0 >> 16) & 0xFFFF) as i16 as f32);
                    let screen_pt = Point2D::new(x, y);
                    let canvas_pt = this.zoom.screen_to_canvas(screen_pt);

                    // If clicked on toolbar grip handle, start dragging toolbar
                    if this.toolbar.hit_test_grip(x, y) {
                        this.toolbar.is_dragging = true;
                        this.toolbar.drag_start_mouse = screen_pt;
                        this.toolbar.drag_start_bar =
                            Point2D::new(this.toolbar.bar_rect.left, this.toolbar.bar_rect.top);
                        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(this.hwnd);
                        return LRESULT(0);
                    }

                    // Colour picker sits above the canvas: claim the click before
                    // any drawing starts, and close it when clicking elsewhere.
                    if this.color_picker.open {
                        if this.color_picker.contains(x, y) {
                            if let Some(bar) = this.color_picker.bar_at(x, y) {
                                this.color_picker.dragging = Some(bar);
                                this.color_picker.set_from_x(bar, x);
                                let c = this.color_picker.current();
                                this.current_color = c;
                                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(
                                    this.hwnd,
                                );
                            } else if let Some(c) = this.color_picker.recent_at(x, y) {
                                this.apply_picked_color(c);
                                this.color_picker.seed_from(c);
                                let name = c.name();
                                this.set_toast("🎨", format!("Colour {}", name));
                            }
                            this.request_repaint();
                            return LRESULT(0);
                        }
                        if !this.toolbar.is_point_inside(x, y) {
                            // Clicking away commits the colour and dismisses.
                            let c = this.color_picker.current();
                            this.apply_picked_color(c);
                            this.color_picker.open = false;
                            this.request_repaint();
                            return LRESULT(0);
                        }
                    }

                    // If clicked on Fluent toolbar items, execute action without drawing
                    if this.toolbar.is_point_inside(x, y) {
                        if let Some(action) = this.toolbar.hit_test(x, y) {
                            return this.handle_fluent_action(action);
                        }
                        return LRESULT(0);
                    }

                    let sw = this.logical_w();
                    let sh = this.logical_h();

                    // If clicked on Minimap, jump viewport center and start dragging
                    if this.show_minimap
                        && this.zoom.level > 1.05
                        && (this.mode == AppMode::StaticZoom || this.mode == AppMode::Draw)
                        && this.minimap.hit_test(sw, sh, x, y)
                    {
                        let target_canvas = this.minimap.minimap_pt_to_canvas_pt(sw, sh, x, y);
                        this.zoom.center_on_canvas_point(target_canvas, sw, sh);
                        this.minimap.is_dragging = true;
                        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(this.hwnd);
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    let is_shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                    let is_ctrl = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
                    let is_tab = (GetKeyState(VK_TAB.0 as i32) as u16 & 0x8000) != 0;

                    // If in Timer mode, handle button clicks or clock face clicks or card dragging
                    if this.mode == AppMode::Timer {
                        let sw = this.logical_w();
                        let sh = this.logical_h();
                        if let Some(action) = this.timer_widget.get_action_at(screen_pt, sw, sh) {
                            match action {
                                TimerAction::SetDuration(mins) => {
                                    this.enter_timer_mode(mins);
                                }
                                TimerAction::CycleCorner => {
                                    this.timer_widget.pill_corner =
                                        (this.timer_widget.pill_corner + 1) % 4;
                                    let corner_name = match this.timer_widget.pill_corner {
                                        1 => "Top-Left",
                                        2 => "Bottom-Left",
                                        3 => "Bottom-Right",
                                        _ => "Top-Right",
                                    };
                                    this.set_toast(
                                        "🔄",
                                        format!("Mini-pill docked to {}", corner_name),
                                    );
                                }
                                TimerAction::PlayPause => {
                                    let paused = !this.timer_paused;
                                    this.timer_paused = paused;
                                    let label = if paused {
                                        "Timer Paused"
                                    } else {
                                        "Timer Resumed"
                                    };
                                    this.set_toast("⏱️", label);
                                }
                                TimerAction::AddMinute => {
                                    this.adjust_timer(60.0);
                                    let m = this.timer_total_mins();
                                    this.set_toast("⏱️", format!("Timer: +1m ({}m total)", m));
                                }
                                TimerAction::SubMinute => {
                                    this.adjust_timer(-60.0);
                                    let m = this.timer_total_mins();
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
                                    let label = if this.timer_widget.minimized {
                                        "Timer Minimized to Corner Pill"
                                    } else {
                                        "Timer Expanded"
                                    };
                                    this.set_toast("⏱️", label);
                                }
                                TimerAction::Close => {
                                    this.exit_overlay();
                                    return LRESULT(0);
                                }
                            }
                            this.request_repaint();
                        } else if !this.timer_widget.minimized {
                            let (cl, ct, cr, cb, cx, cy) =
                                this.timer_widget.get_card_bounds(sw, sh);
                            if screen_pt.x >= cl
                                && screen_pt.x <= cr
                                && screen_pt.y >= ct
                                && screen_pt.y <= cb
                            {
                                this.timer_widget.is_dragging = true;
                                this.timer_widget.drag_start_mouse = screen_pt;
                                this.timer_widget.drag_start_pos = Point2D::new(cx, cy);
                                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(
                                    this.hwnd,
                                );
                            }
                        }
                        return LRESULT(0);
                    }

                    // If in Spotlight mode, clicking toggles pinning in place
                    if this.mode == AppMode::Spotlight {
                        let pinned = !this.spotlight.pinned;
                        this.spotlight.pinned = pinned;
                        this.set_toast(
                            "🔦",
                            if pinned {
                                "Spotlight Pinned (Click to unpin)"
                            } else {
                                "Spotlight Following Cursor"
                            },
                        );
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    // If in Static Zoom mode, clicking transitions into Draw mode
                    if this.mode == AppMode::StaticZoom {
                        this.mode = AppMode::Draw;
                        this.toolbar.active_tool = Some(this.current_tool);
                        let sw = this.logical_w();
                        let sh = this.logical_h();
                        this.toolbar.update_layout(sw, sh);
                        this.set_toast("✏️", "Draw Mode (Drag to draw | Esc to exit)");
                    }

                    // If in Loupe mode, clicking pins or unpins the lens
                    if this.mode == AppMode::Loupe {
                        this.loupe.pinned = !this.loupe.pinned;
                        let label = if this.loupe.pinned {
                            "📌 Loupe Pinned (Click or Space to unpin)"
                        } else {
                            "🔍 Loupe Following Cursor"
                        };
                        this.set_toast("🔍", label);
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.current_tool == DrawTool::Text {
                        this.commit_text_editor();
                        // Clicking inside a box types a label into it rather
                        // than dropping loose text on top.
                        if let Some(cid) = this.container_at(canvas_pt)
                            && this.edit_container_label(cid)
                        {
                            return LRESULT(0);
                        }
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
                        let rad = this.badge_settings.size.radius();
                        let col = this.current_color;
                        let bshape = this.badge_settings.shape;
                        let bfill = this.badge_settings.fill;
                        let bwidth = this.badge_settings.stroke_width;
                        let bpattern = StrokePattern::Solid;
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
                        let sw = this.logical_w();
                        let sh = this.logical_h();
                        this.toolbar.update_layout(sw, sh);
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.current_tool == DrawTool::LaserPointer {
                        this.laser_pos = Some(canvas_pt);
                        let laser_color = this.current_color;
                        this.laser_trail.push(LaserTrailPoint {
                            pt: canvas_pt,
                            timestamp: Instant::now(),
                        });
                        this.laser_ripples.push(LaserRipple {
                            center: canvas_pt,
                            timestamp: Instant::now(),
                            color: laser_color,
                        });
                        if this.laser_ripples.len() > 10 {
                            this.laser_ripples.remove(0);
                        }
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.current_tool == DrawTool::Select {
                        this.validate_selection();
                        // Shift extends the selection rather than replacing it.
                        let additive =
                            (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                        let consumed = this.select_press(screen_pt, canvas_pt, additive);
                        // Double-clicking a text annotation puts it back in the
                        // editor instead of starting a drag.
                        if msg == WM_LBUTTONDBLCLK && consumed {
                            let picked = this.selection.as_ref().and_then(|s| s.only());
                            if let Some(id) = picked
                                && this.edit_container_label(id)
                            {
                                return LRESULT(0);
                            }
                            if this.reopen_selected_text() {
                                return LRESULT(0);
                            }
                        }
                        if consumed {
                            let _ =
                                windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(this.hwnd);
                        }
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    if this.current_tool == DrawTool::Eraser {
                        this.is_drawing = true;
                        let mut erased_idx = None;
                        for (idx, shape) in this.shapes.iter().enumerate().rev() {
                            if shape_intersects_circle(&shape.shape, canvas_pt, 16.0) {
                                erased_idx = Some(idx);
                                break;
                            }
                        }
                        if let Some(idx) = erased_idx {
                            let erased_id = this.shapes[idx].id;
                            this.selection = None;
                            if this.delete_annotation(erased_id) {
                                this.set_toast("🧹", "Erased Shape");
                            }
                        }
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    this.stroke_start_time = Some(Instant::now());
                    this.last_mouse_dwell_time = Some(Instant::now());
                    this.last_mouse_pos = screen_pt;

                    // Freehand is never snapped; for everything else the first
                    // corner is pulled onto nearby geometry just like the last.
                    this.rebuild_snap_anchors(None);
                    let canvas_pt = if this.current_tool == DrawTool::Pen
                        || this.current_tool == DrawTool::Highlighter
                    {
                        this.clear_snap();
                        canvas_pt
                    } else {
                        this.apply_point_snap(canvas_pt)
                    };

                    this.is_drawing = true;
                    this.draw_start_pt = canvas_pt;
                    this.was_shifted_during_draw = is_shift;

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
                                width: this.pen_settings.stroke_width,
                                is_highlighter: false,
                                pattern: this.pen_settings.pattern,
                                pressures: this.pressure_seed(),
                            }
                        }
                    } else {
                        this.create_shape_for_tool(canvas_pt)
                    };

                    this.active_shape = Some(shape_to_create);
                    this.request_repaint();
                    LRESULT(0)
                }

                WM_LBUTTONUP => {
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                    // Guides belong to the gesture that produced them.
                    this.clear_snap();
                    if this.color_picker.dragging.take().is_some() {
                        let c = this.color_picker.current();
                        this.apply_picked_color(c);
                        this.request_repaint();
                        return LRESULT(0);
                    }
                    if this.minimap.is_dragging {
                        this.minimap.is_dragging = false;
                        return LRESULT(0);
                    }

                    if this.toolbar.is_dragging {
                        this.toolbar.is_dragging = false;
                        return LRESULT(0);
                    }

                    if this.timer_widget.is_dragging {
                        this.timer_widget.is_dragging = false;
                        return LRESULT(0);
                    }

                    if this.current_tool == DrawTool::Select {
                        this.select_release();
                        this.request_repaint();
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

                    if this.is_drawing {
                        this.is_drawing = false;
                        let was_shift = this.was_shifted_during_draw
                            || (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                        this.was_shifted_during_draw = false;

                        if let Some(mut shape) = this.active_shape.take() {
                            let start_pt = this.draw_start_pt;
                            if was_shift {
                                match &mut shape {
                                    Shape::Line { end, .. } | Shape::Arrow { end, .. } => {
                                        *end = snap_to_angle(start_pt, *end);
                                    }
                                    Shape::Rectangle { end, .. }
                                    | Shape::Ellipse { end, .. }
                                    | Shape::Blur { end, .. } => {
                                        *end = snap_to_square(start_pt, *end);
                                    }
                                    _ => {}
                                }
                            }

                            let should_keep = match &shape {
                                Shape::Stroke { points, .. } => points.len() > 1,
                                Shape::Line { start, end, .. } => start.distance(end) > 2.0,
                                Shape::Arrow { start, end, .. } => start.distance(end) > 2.0,
                                Shape::Rectangle { start, end, .. } => start.distance(end) > 2.0,
                                Shape::Ellipse { start, end, .. } => start.distance(end) > 2.0,
                                Shape::Blur { start, end, .. } => start.distance(end) > 2.0,
                                _ => true,
                            };
                            if should_keep {
                                let bindable =
                                    matches!(shape, Shape::Line { .. } | Shape::Arrow { .. });
                                let id = this.push_shape(shape);
                                if bindable {
                                    this.bind_new_arrow(id);
                                }
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
                    // If cheat sheet open, close cheat sheet
                    if this.show_cheat_sheet {
                        this.show_cheat_sheet = false;
                        this.request_repaint();
                        return LRESULT(0);
                    }
                    // If in Loupe mode, right click unpins if pinned, or exits overlay
                    if this.mode == AppMode::Loupe {
                        if this.loupe.pinned {
                            this.loupe.pinned = false;
                            this.set_toast("🔍", "Loupe Unpinned");
                            this.request_repaint();
                            return LRESULT(0);
                        }
                        this.exit_overlay();
                        return LRESULT(0);
                    }
                    // In Draw mode: if zoomed in, return to StaticZoom; if not zoomed in, exit overlay!
                    if this.mode == AppMode::Draw {
                        if this.zoom.level > 1.001 {
                            this.mode = AppMode::StaticZoom;
                            let w = this.logical_w();
                            let h = this.logical_h();
                            this.toolbar.update_layout(w, h);
                            this.set_toast(
                                "🔎",
                                "Switched to Pan & Zoom Mode (Right-click again to Exit)",
                            );
                            this.request_repaint();
                            return LRESULT(0);
                        } else {
                            this.exit_overlay();
                            return LRESULT(0);
                        }
                    }
                    // Otherwise exit overlay completely
                    this.exit_overlay();
                    LRESULT(0)
                }

                WM_RBUTTONUP => LRESULT(0),

                WM_MBUTTONDOWN => {
                    // Mouse messages are physical pixels; everything below is DIPs.
                    let x = this.px_to_dip((lparam.0 & 0xFFFF) as i16 as f32);
                    let y = this.px_to_dip(((lparam.0 >> 16) & 0xFFFF) as i16 as f32);
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
                            let new_dim =
                                (this.timer_widget.dim_opacity + delta * 0.05).clamp(0.15, 0.95);
                            this.timer_widget.dim_opacity = new_dim;
                            this.set_toast(
                                "🌓",
                                format!("Background Dim: {:.0}%", new_dim * 100.0),
                            );
                        } else {
                            let change = if delta > 0.0 { 60.0 } else { -60.0 };
                            this.adjust_timer(change);
                            let m = this.timer_total_mins();
                            this.set_toast("⏱️", format!("Timer: {}m total", m));
                        }
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    // Loupe Magnifier mode controls (Wheel: zoom, Shift+Wheel: resize diameter)
                    if this.mode == AppMode::Loupe {
                        if is_shift {
                            let new_rad = (this.loupe.radius + delta * 15.0).clamp(60.0, 500.0);
                            this.loupe.radius = new_rad;
                            let diam = (new_rad * 2.0).round() as u32;
                            this.set_toast("🔍", format!("Loupe Size ⌀{} px", diam));
                        } else {
                            let new_mag =
                                (this.loupe.magnification + (delta * 0.25)).clamp(1.25, 12.0);
                            this.loupe.magnification = new_mag;
                            this.set_toast("🔍", format!("Loupe Zoom {:.1}x", new_mag));
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

                    // 2. Shift + Wheel: Adjusts brush stroke width in Draw mode
                    if is_shift && this.mode == AppMode::Draw {
                        let new_width = (this.stroke_width + delta * 1.5).clamp(1.0, 40.0);
                        match this.current_tool {
                            DrawTool::Pen => this.pen_settings.stroke_width = new_width,
                            DrawTool::Highlighter => {
                                this.highlighter_settings.stroke_width = new_width
                            }
                            DrawTool::Line => this.line_settings.stroke_width = new_width,
                            DrawTool::Arrow => this.arrow_settings.stroke_width = new_width,
                            DrawTool::Rectangle => this.rect_settings.stroke_width = new_width,
                            DrawTool::RoundedRectangle => {
                                this.rounded_rect_settings.stroke_width = new_width
                            }
                            DrawTool::Ellipse => this.ellipse_settings.stroke_width = new_width,
                            DrawTool::Blur => this.blur_settings.block_size = new_width,
                            _ => {}
                        }
                        this.stroke_width = new_width;
                        this.toolbar.stroke_width = new_width;
                        let sw = this.logical_w();
                        let sh = this.logical_h();
                        this.toolbar.update_layout(sw, sh);
                        let w_val = new_width.round() as u32;
                        if this.current_tool == DrawTool::Blur {
                            this.set_toast("░", format!("Mosaic Block Size {} px", w_val));
                        } else {
                            this.set_toast("🖌️", format!("Stroke Width {} px", w_val));
                        }
                        this.request_repaint();
                        return LRESULT(0);
                    }

                    // 3. Normal Wheel: ALWAYS ZOOMS in Freeze modes (StaticZoom, Draw, Spotlight)!
                    if this.mode == AppMode::StaticZoom
                        || this.mode == AppMode::Draw
                        || this.mode == AppMode::Spotlight
                    {
                        let mut pt = POINT::default();
                        let sx = this.screen_x;
                        let sy = this.screen_y;
                        let sw = this.logical_w();
                        let sh = this.logical_h();
                        let (cursor_x, cursor_y) = if GetCursorPos(&mut pt).is_ok() {
                            (
                                this.px_to_dip((pt.x - sx) as f32),
                                this.px_to_dip((pt.y - sy) as f32),
                            )
                        } else {
                            (sw / 2.0, sh / 2.0)
                        };
                        let new_lvl = (this.zoom.level + delta * 0.25).clamp(1.0, 10.0);
                        this.zoom.set_zoom_centered(
                            new_lvl,
                            Point2D::new(cursor_x, cursor_y),
                            sw,
                            sh,
                        );
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
                        if ch == '\x08'
                            || ch == '\x1b'
                            || ch == '\x7f'
                            || ch == '\r'
                            || ch == '\n'
                        {
                            // Handled in WM_KEYDOWN (Enter inserts a newline there)
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

                    if key == VK_SHIFT.0 as i32 {
                        let mut pt = POINT::default();
                        let current_screen_pt = if GetCursorPos(&mut pt).is_ok() {
                            Point2D::new(
                                this.px_to_dip((pt.x - this.screen_x) as f32),
                                this.px_to_dip((pt.y - this.screen_y) as f32),
                            )
                        } else {
                            this.last_mouse_pos
                        };
                        this.last_mouse_pos = current_screen_pt;

                        if this.is_drawing {
                            this.was_shifted_during_draw = true;
                            let canvas_pt = this.zoom.screen_to_canvas(current_screen_pt);
                            let start_pt = this.draw_start_pt;
                            match &mut this.active_shape {
                                Some(Shape::Line { end, .. }) | Some(Shape::Arrow { end, .. }) => {
                                    *end = snap_to_angle(start_pt, canvas_pt);
                                    this.request_repaint();
                                }
                                Some(Shape::Rectangle { end, .. })
                                | Some(Shape::Ellipse { end, .. })
                                | Some(Shape::Blur { end, .. }) => {
                                    *end = snap_to_square(start_pt, canvas_pt);
                                    this.request_repaint();
                                }
                                _ => {}
                            }
                        }
                        return LRESULT(0);
                    }

                    // ── Text editor intercepts all keys first ──
                    if this.text_editor.is_some() {
                        if key == VK_ESCAPE.0 as i32 {
                            // Esc keeps what was typed; right-click discards.
                            this.commit_text_editor();
                            return LRESULT(0);
                        } else if key == VK_RETURN.0 as i32 {
                            if is_ctrl {
                                this.commit_text_editor();
                            } else if let Some(ed) = &mut this.text_editor {
                                ed.insert_newline();
                                this.request_repaint();
                            }
                            return LRESULT(0);
                        } else if is_ctrl && key == 'B' as i32 {
                            this.text_is_bold = !this.text_is_bold;
                            let bold = this.text_is_bold;
                            this.toolbar.text_is_bold = bold;
                            if let Some(ed) = &mut this.text_editor {
                                ed.is_bold = bold;
                            }
                            let sw = this.logical_w();
                            let sh = this.logical_h();
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
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("𝐼", if italic { "Italic: On" } else { "Italic: Off" });
                            this.request_repaint();
                            return LRESULT(0);
                        } else if is_ctrl && key == VK_UP.0 as i32 {
                            let new_sz = if let Some(ed) = &mut this.text_editor {
                                ed.font_size = (ed.font_size + 4.0).min(96.0);
                                ed.font_size
                            } else {
                                22.0
                            };
                            this.font_size = new_sz;
                            this.toolbar.current_font_size = new_sz;
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            this.request_repaint();
                            return LRESULT(0);
                        } else if is_ctrl && key == VK_DOWN.0 as i32 {
                            let new_sz = if let Some(ed) = &mut this.text_editor {
                                ed.font_size = (ed.font_size - 4.0).max(12.0);
                                ed.font_size
                            } else {
                                22.0
                            };
                            this.font_size = new_sz;
                            this.toolbar.current_font_size = new_sz;
                            let sw = this.logical_w();
                            let sh = this.logical_h();
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
                            } else if key == VK_UP.0 as i32 {
                                editor.move_up();
                            } else if key == VK_DOWN.0 as i32 {
                                editor.move_down();
                            } else if key == VK_HOME.0 as i32 {
                                if is_ctrl {
                                    editor.cursor = 0;
                                } else {
                                    editor.move_line_start();
                                }
                            } else if key == VK_END.0 as i32 {
                                if is_ctrl {
                                    editor.cursor = editor.text.len();
                                } else {
                                    editor.move_line_end();
                                }
                            } else if is_ctrl
                                && key == 'V' as i32
                                && let Some(clip_text) = get_clipboard_text()
                            {
                                // Normalise CRLF/CR so line splitting stays on '\n'.
                                let normalised = clip_text.replace("\r\n", "\n").replace('\r', "\n");
                                editor.insert_str(&normalised);
                            }
                            this.request_repaint();
                            return LRESULT(0);
                        }
                        return LRESULT(0);
                    }

                    // Ctrl+Alt+arrows line the selection up; Ctrl+Alt+H/V spread
                    // it. Checked before the plain-arrow nudge below.
                    if this.current_tool == DrawTool::Select
                        && this.selection.is_some()
                        && is_ctrl
                        && (GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0
                    {
                        let align = match key {
                            k if k == VK_LEFT.0 as i32 => Some(AlignTo::Left),
                            k if k == VK_RIGHT.0 as i32 => Some(AlignTo::Right),
                            k if k == VK_UP.0 as i32 => Some(AlignTo::Top),
                            k if k == VK_DOWN.0 as i32 => Some(AlignTo::Bottom),
                            k if k == 'C' as i32 => Some(AlignTo::HCentre),
                            k if k == 'M' as i32 => Some(AlignTo::VCentre),
                            _ => None,
                        };
                        let spread = match key {
                            k if k == 'H' as i32 => Some(true),
                            k if k == 'V' as i32 => Some(false),
                            _ => None,
                        };
                        if align.is_some() || spread.is_some() {
                            this.arrange_selection(align, spread);
                            return LRESULT(0);
                        }
                    }

                    // ── Selection edits claim Delete and the arrows, which
                    //    otherwise clear the canvas / adjust zoom ──
                    if this.current_tool == DrawTool::Select && this.selection.is_some() {
                        if key == VK_DELETE.0 as i32 || key == VK_BACK.0 as i32 {
                            this.delete_selection();
                            return LRESULT(0);
                        }
                        if key == VK_ESCAPE.0 as i32 {
                            this.selection = None;
                            this.marquee = None;
                            this.request_repaint();
                            return LRESULT(0);
                        }
                        let step = if is_shift { 10.0 } else { 1.0 };
                        let nudge = match key {
                            k if k == VK_LEFT.0 as i32 => Some((-step, 0.0)),
                            k if k == VK_RIGHT.0 as i32 => Some((step, 0.0)),
                            k if k == VK_UP.0 as i32 => Some((0.0, -step)),
                            k if k == VK_DOWN.0 as i32 => Some((0.0, step)),
                            _ => None,
                        };
                        if let Some((dx, dy)) = nudge {
                            this.nudge_selection(dx, dy);
                            return LRESULT(0);
                        }
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
                                } else if this.mode == AppMode::StaticZoom
                                    || this.mode == AppMode::Draw
                                {
                                    this.toggle_spotlight();
                                } else {
                                    this.enter_spotlight_mode();
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
                            k if k == '6' as i32 => {
                                if this.mode == AppMode::Loupe {
                                    this.exit_overlay();
                                } else {
                                    this.enter_loupe_mode();
                                }
                            }
                            k if k == 'Z' as i32 && !is_shift => {
                                this.undo();
                            }
                            k if k == 'Z' as i32 && is_shift => {
                                this.redo();
                            }
                            k if k == 'Y' as i32 => {
                                this.redo();
                            }
                            k if k == 'C' as i32 => {
                                this.copy_screen_to_clipboard();
                            }
                            k if k == 'S' as i32 => {
                                if is_shift {
                                    this.save_session(true);
                                } else {
                                    this.save_snapshot();
                                }
                            }
                            k if k == 'O' as i32 => {
                                this.load_session_via_dialog();
                            }
                            k if k == 'D' as i32 => {
                                this.duplicate_selection();
                            }
                            // Ctrl+] / Ctrl+[ restack, as in most editors.
                            k if k == 0xDD => {
                                this.restack_selection(true);
                            }
                            k if k == 0xDB => {
                                this.restack_selection(false);
                            }
                            k if k == VK_TAB.0 as i32 => {
                                this.cycle_next_monitor();
                            }
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
                            } else if this.mode == AppMode::Loupe && this.loupe.pinned {
                                this.loupe.pinned = false;
                                this.set_toast("🔍", "Loupe Unpinned (Tracking Cursor)");
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
                            this.set_toast(
                                "🖥️",
                                if show {
                                    "Toolbar Visible"
                                } else {
                                    "Toolbar Hidden"
                                },
                            );
                            this.request_repaint();
                        }

                        k if k == windows::Win32::UI::Input::KeyboardAndMouse::VK_F3.0 as i32 => {
                            if this.mode == AppMode::Spotlight {
                                this.exit_overlay();
                            } else if this.mode == AppMode::StaticZoom || this.mode == AppMode::Draw
                            {
                                this.toggle_spotlight();
                            } else {
                                this.enter_spotlight_mode();
                            }
                        }

                        k if k == windows::Win32::UI::Input::KeyboardAndMouse::VK_F4.0 as i32 => {
                            this.cycle_next_monitor();
                        }

                        k if k == VK_TAB.0 as i32 => {
                            if this.mode == AppMode::Loupe {
                                this.loupe.is_rect = !this.loupe.is_rect;
                                let shape_name = if this.loupe.is_rect {
                                    "Rounded Rectangle"
                                } else {
                                    "Circle"
                                };
                                this.set_toast("🔍", format!("Loupe Shape: {}", shape_name));
                                this.request_repaint();
                                return LRESULT(0);
                            }
                            if this.mode == AppMode::Timer {
                                this.timer_widget.minimized = !this.timer_widget.minimized;
                                let label = if this.timer_widget.minimized {
                                    "Timer Minimized to Corner Pill"
                                } else {
                                    "Timer Expanded"
                                };
                                this.set_toast("⏱️", label);
                                this.request_repaint();
                                return LRESULT(0);
                            }
                        }

                        k if k == VK_SPACE.0 as i32 => {
                            if this.mode == AppMode::Loupe {
                                this.loupe.pinned = !this.loupe.pinned;
                                let label = if this.loupe.pinned {
                                    "📌 Loupe Pinned (Click or Space to unpin)"
                                } else {
                                    "🔍 Loupe Following Cursor"
                                };
                                this.set_toast("🔍", label);
                                this.request_repaint();
                                return LRESULT(0);
                            } else if this.mode == AppMode::Timer {
                                if this.timer_remaining <= 0.0 {
                                    this.timer_remaining = this.timer_seconds as f64;
                                    this.timer_paused = false;
                                    this.timer_alarm_sounded = false;
                                    this.set_toast("⏱️", "Timer Reset");
                                } else {
                                    let paused = !this.timer_paused;
                                    this.timer_paused = paused;
                                    this.set_toast(
                                        "⏱️",
                                        if paused {
                                            "Timer Paused"
                                        } else {
                                            "Timer Resumed"
                                        },
                                    );
                                }
                                this.request_repaint();
                            } else if this.mode == AppMode::Spotlight {
                                let pinned = !this.spotlight.pinned;
                                this.spotlight.pinned = pinned;
                                this.set_toast(
                                    "🔦",
                                    if pinned {
                                        "Spotlight Pinned"
                                    } else {
                                        "Spotlight Following"
                                    },
                                );
                                this.request_repaint();
                            } else {
                                if this.mode == AppMode::Draw {
                                    this.mode = AppMode::StaticZoom;
                                    this.toolbar.active_tool = None;
                                    let sw = this.logical_w();
                                    let sh = this.logical_h();
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("🔎", "Pan & Zoom Mode");
                                } else if this.mode == AppMode::StaticZoom {
                                    this.mode = AppMode::Draw;
                                    this.toolbar.active_tool = None;
                                    let sw = this.logical_w();
                                    let sh = this.logical_h();
                                    this.toolbar.update_layout(sw, sh);
                                    this.set_toast("✏️", "Draw Mode");
                                }
                                this.request_repaint();
                            }
                        }

                        // ─── Colors (single key, no modifier = color) ───
                        k if k == 'R' as i32 && !is_shift => {
                            if this.mode == AppMode::Loupe {
                                this.loupe.show_reticle = !this.loupe.show_reticle;
                                let status = if this.loupe.show_reticle {
                                    "Reticle Enabled"
                                } else {
                                    "Reticle Disabled"
                                };
                                this.set_toast("🔍", format!("Loupe: {}", status));
                                this.request_repaint();
                                return LRESULT(0);
                            }
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
                                let sw = this.logical_w();
                                let sh = this.logical_h();
                                this.toolbar.update_layout(sw, sh);
                                this.set_toast("↺", "Step badge reset to #1");
                                this.request_repaint();
                                return LRESULT(0);
                            }
                            this.ensure_draw_mode();
                            this.current_color = ColorPreset::Red;
                            this.set_toast("🔴", "Red");
                            this.request_repaint();
                        }
                        k if k == 'G' as i32 => {
                            this.ensure_draw_mode();
                            this.current_color = ColorPreset::Green;
                            this.set_toast("🟢", "Green");
                            this.request_repaint();
                        }
                        k if k == 'B' as i32 && !is_shift => {
                            this.ensure_draw_mode();
                            this.current_color = ColorPreset::Blue;
                            this.set_toast("🔵", "Blue");
                            this.request_repaint();
                        }
                        k if k == 'B' as i32 && is_shift => {
                            this.ensure_draw_mode();
                            this.current_color = ColorPreset::Black;
                            this.set_toast("⚫", "Black Pen");
                            this.request_repaint();
                        }
                        k if k == 'Y' as i32 => {
                            this.ensure_draw_mode();
                            this.current_color = ColorPreset::Yellow;
                            this.set_toast("🟡", "Yellow");
                            this.request_repaint();
                        }
                        k if k == 'O' as i32 => {
                            this.ensure_draw_mode();
                            this.current_color = ColorPreset::Orange;
                            this.set_toast("🟠", "Orange");
                            this.request_repaint();
                        }
                        k if k == 'P' as i32 && is_shift => {
                            this.ensure_draw_mode();
                            this.current_color = ColorPreset::Pink;
                            this.set_toast("🌸", "Pink");
                            this.request_repaint();
                        }
                        k if k == 'C' as i32 || k == 'I' as i32 => {
                            this.ensure_draw_mode();
                            this.current_color = ColorPreset::Cyan;
                            this.set_toast("🩵", "Cyan");
                            this.request_repaint();
                        }

                        // ─── Minimap Radar Toggle ───
                        k if k == 'M' as i32 => {
                            this.show_minimap = !this.show_minimap;
                            let status = if this.show_minimap {
                                "Minimap Radar: Enabled"
                            } else {
                                "Minimap Radar: Disabled"
                            };
                            this.set_toast("🗺️", status);
                            this.request_repaint();
                            return LRESULT(0);
                        }

                        // ─── Canvas Slate & Color Modes ───
                        k if k == 'W' as i32 && is_shift => {
                            this.ensure_draw_mode();
                            this.current_color = ColorPreset::White;
                            this.set_toast("⚪", "White Pen");
                            this.request_repaint();
                        }
                        k if k == 'W' as i32 && !is_shift => {
                            this.ensure_draw_mode();
                            this.background_type =
                                if this.background_type == CanvasBackground::Whiteboard {
                                    CanvasBackground::Transparent
                                } else {
                                    CanvasBackground::Whiteboard
                                };
                            this.current_color = ColorPreset::Red;
                            this.set_toast("⚪", "Whiteboard");
                            this.request_repaint();
                        }
                        k if k == 'K' as i32 && is_shift => {
                            this.ensure_draw_mode();
                            this.background_type =
                                if this.background_type == CanvasBackground::Blackboard {
                                    CanvasBackground::Transparent
                                } else {
                                    CanvasBackground::Blackboard
                                };
                            this.current_color = ColorPreset::White;
                            this.set_toast("⚫", "Blackboard");
                            this.request_repaint();
                        }
                        k if k == 'K' as i32 && !is_shift => {
                            this.ensure_draw_mode();
                            this.current_tool = DrawTool::LaserPointer;
                            this.set_toast("🔴", "Laser Pointer");
                            this.request_repaint();
                        }

                        // ─── Drawing Tools & Attributes ───
                        k if k == 'T' as i32 => {
                            this.ensure_draw_mode();
                            this.current_tool = DrawTool::Text;
                            this.toolbar.active_tool = Some(DrawTool::Text);
                            this.sync_tool_to_toolbar();
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("🔤", "Text — click to type");
                            this.request_repaint();
                        }
                        k if k == 'H' as i32 => {
                            this.ensure_draw_mode();
                            this.current_tool = DrawTool::Highlighter;
                            this.toolbar.active_tool = Some(DrawTool::Highlighter);
                            this.sync_tool_to_toolbar();
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("🖍️", "Highlighter");
                            this.request_repaint();
                        }
                        k if k == 'L' as i32 => {
                            this.ensure_draw_mode();
                            this.current_tool = DrawTool::Line;
                            this.toolbar.active_tool = Some(DrawTool::Line);
                            this.sync_tool_to_toolbar();
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("📏", "Line");
                            this.request_repaint();
                        }
                        k if k == 'A' as i32 => {
                            this.ensure_draw_mode();
                            this.current_tool = DrawTool::Arrow;
                            this.toolbar.active_tool = Some(DrawTool::Arrow);
                            this.sync_tool_to_toolbar();
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("➜", "Arrow");
                            this.request_repaint();
                        }
                        k if k == 'R' as i32 && is_shift => {
                            this.ensure_draw_mode();
                            this.current_tool = DrawTool::Rectangle;
                            this.toolbar.active_tool = Some(DrawTool::Rectangle);
                            this.sync_tool_to_toolbar();
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("▭", "Rectangle");
                            this.request_repaint();
                        }
                        k if k == 'U' as i32 => {
                            this.ensure_draw_mode();
                            this.current_tool = DrawTool::RoundedRectangle;
                            this.toolbar.active_tool = Some(DrawTool::RoundedRectangle);
                            this.sync_tool_to_toolbar();
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("▢", "Rounded Rectangle");
                            this.request_repaint();
                        }
                        k if k == 'Q' as i32 => {
                            this.ensure_draw_mode();
                            this.current_tool = DrawTool::Ellipse;
                            this.toolbar.active_tool = Some(DrawTool::Ellipse);
                            this.sync_tool_to_toolbar();
                            let sw = this.logical_w();
                            let sh = this.logical_h();
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
                            match this.current_tool {
                                DrawTool::Rectangle => this.rect_settings.fill_mode = new_fm,
                                DrawTool::RoundedRectangle => {
                                    this.rounded_rect_settings.fill_mode = new_fm
                                }
                                DrawTool::Ellipse => this.ellipse_settings.fill_mode = new_fm,
                                DrawTool::StepBadge => this.badge_settings.fill = new_fm,
                                _ => {}
                            }
                            this.fill_mode = new_fm;
                            this.toolbar.current_fill_mode = new_fm;
                            let sw = this.logical_w();
                            let sh = this.logical_h();
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
                            match this.current_tool {
                                DrawTool::Pen => this.pen_settings.pattern = new_sp,
                                DrawTool::Line => this.line_settings.pattern = new_sp,
                                DrawTool::Arrow => this.arrow_settings.pattern = new_sp,
                                DrawTool::Rectangle => this.rect_settings.pattern = new_sp,
                                DrawTool::RoundedRectangle => {
                                    this.rounded_rect_settings.pattern = new_sp
                                }
                                DrawTool::Ellipse => this.ellipse_settings.pattern = new_sp,
                                _ => {}
                            }
                            this.stroke_pattern = new_sp;
                            this.toolbar.current_stroke_pattern = new_sp;
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            let nm = new_sp.name();
                            this.set_toast("✏️", format!("Pattern: {}", nm));
                            this.request_repaint();
                        }
                        k if (k == 'N' as i32 && is_shift) || k == '0' as i32 => {
                            this.ensure_draw_mode();
                            this.step_counter = 1;
                            this.toolbar.badge_counter = 1;
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("🔢", "Step badge counter reset to 1");
                            this.request_repaint();
                        }
                        k if k == 'N' as i32 => {
                            this.ensure_draw_mode();
                            let next_num = this.step_counter;
                            this.current_tool = DrawTool::StepBadge;
                            this.toolbar.active_tool = Some(DrawTool::StepBadge);
                            this.sync_tool_to_toolbar();
                            this.toolbar.badge_counter = this.step_counter;
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("🔢", format!("Step Badge (next: #{})", next_num));
                            this.request_repaint();
                        }
                        k if k == 'V' as i32 && !is_shift => {
                            this.ensure_draw_mode();
                            this.current_tool = DrawTool::Select;
                            this.toolbar.active_tool = Some(DrawTool::Select);
                            this.sync_tool_to_toolbar();
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("↖", "Select (click a shape to edit)");
                            this.request_repaint();
                        }
                        k if k == 'P' as i32 && !is_shift => {
                            this.ensure_draw_mode();
                            this.current_tool = DrawTool::Pen;
                            this.toolbar.active_tool = Some(DrawTool::Pen);
                            this.sync_tool_to_toolbar();
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("✏️", "Pen");
                            this.request_repaint();
                        }
                        k if k == 'X' as i32 && is_shift => {
                            this.ensure_draw_mode();
                            this.current_tool = DrawTool::Blur;
                            this.toolbar.active_tool = Some(DrawTool::Blur);
                            this.sync_tool_to_toolbar();
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("░", "Redact / Blur (drag to hide)");
                            this.request_repaint();
                        }
                        k if k == 'X' as i32 && !is_shift => {
                            this.ensure_draw_mode();
                            this.current_tool = DrawTool::Eraser;
                            this.toolbar.active_tool = Some(DrawTool::Eraser);
                            this.sync_tool_to_toolbar();
                            let sw = this.logical_w();
                            let sh = this.logical_h();
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
                            match this.current_tool {
                                DrawTool::Pen => this.pen_settings.stroke_width = w,
                                DrawTool::Highlighter => this.highlighter_settings.stroke_width = w,
                                DrawTool::Line => this.line_settings.stroke_width = w,
                                DrawTool::Arrow => this.arrow_settings.stroke_width = w,
                                DrawTool::Rectangle => this.rect_settings.stroke_width = w,
                                DrawTool::RoundedRectangle => {
                                    this.rounded_rect_settings.stroke_width = w
                                }
                                DrawTool::Ellipse => this.ellipse_settings.stroke_width = w,
                                DrawTool::StepBadge => this.badge_settings.stroke_width = w,
                                DrawTool::Blur => this.blur_settings.block_size = w,
                                _ => {}
                            }
                            this.stroke_width = w;
                            this.toolbar.stroke_width = w;
                            let sw = this.logical_w();
                            let sh = this.logical_h();
                            this.toolbar.update_layout(sw, sh);
                            this.set_toast("🖌️", format!("{} px", w as u32));
                            this.request_repaint();
                        }

                        // ─── [ ] Bracket Sizing ───
                        219 => {
                            // [
                            if this.mode == AppMode::Loupe {
                                let new_rad = (this.loupe.radius - 20.0).clamp(60.0, 500.0);
                                this.loupe.radius = new_rad;
                                this.set_toast(
                                    "🔍",
                                    format!("Loupe ⌀{} px", (new_rad * 2.0).round() as u32),
                                );
                            } else if this.spotlight.active {
                                let new_rad = (this.spotlight.radius - 20.0).max(30.0);
                                this.spotlight.radius = new_rad;
                                this.set_toast(
                                    "🔦",
                                    format!("⌀{} px", (new_rad * 2.0).round() as u32),
                                );
                            } else if this.current_tool == DrawTool::StepBadge {
                                let new_bs = match this.badge_settings.size {
                                    BadgeSize::ExtraLarge => BadgeSize::Large,
                                    BadgeSize::Large => BadgeSize::Medium,
                                    _ => BadgeSize::Small,
                                };
                                this.badge_settings.size = new_bs;
                                this.badge_size = new_bs;
                                this.toolbar.current_badge_size = new_bs;
                                let sw = this.logical_w();
                                let sh = this.logical_h();
                                this.toolbar.update_layout(sw, sh);
                                let nm = new_bs.name();
                                this.set_toast("🔢", format!("Badge Size: {}", nm));
                            } else {
                                let new_w = (this.stroke_width - 2.0).max(1.0);
                                match this.current_tool {
                                    DrawTool::Pen => this.pen_settings.stroke_width = new_w,
                                    DrawTool::Highlighter => {
                                        this.highlighter_settings.stroke_width = new_w
                                    }
                                    DrawTool::Line => this.line_settings.stroke_width = new_w,
                                    DrawTool::Arrow => this.arrow_settings.stroke_width = new_w,
                                    DrawTool::Rectangle => this.rect_settings.stroke_width = new_w,
                                    DrawTool::RoundedRectangle => {
                                        this.rounded_rect_settings.stroke_width = new_w
                                    }
                                    DrawTool::Ellipse => this.ellipse_settings.stroke_width = new_w,
                                    DrawTool::Blur => this.blur_settings.block_size = new_w,
                                    _ => {}
                                }
                                this.stroke_width = new_w;
                                this.toolbar.stroke_width = new_w;
                                let sw = this.logical_w();
                                let sh = this.logical_h();
                                this.toolbar.update_layout(sw, sh);
                                this.set_toast("🖌️", format!("{} px", new_w.round() as u32));
                            }
                            this.request_repaint();
                        }
                        221 => {
                            // ]
                            if this.mode == AppMode::Loupe {
                                let new_rad = (this.loupe.radius + 20.0).clamp(60.0, 500.0);
                                this.loupe.radius = new_rad;
                                this.set_toast(
                                    "🔍",
                                    format!("Loupe ⌀{} px", (new_rad * 2.0).round() as u32),
                                );
                            } else if this.spotlight.active {
                                let new_rad = (this.spotlight.radius + 20.0).min(700.0);
                                this.spotlight.radius = new_rad;
                                this.set_toast(
                                    "🔦",
                                    format!("⌀{} px", (new_rad * 2.0).round() as u32),
                                );
                            } else if this.current_tool == DrawTool::StepBadge {
                                let new_bs = match this.badge_settings.size {
                                    BadgeSize::Small => BadgeSize::Medium,
                                    BadgeSize::Medium => BadgeSize::Large,
                                    _ => BadgeSize::ExtraLarge,
                                };
                                this.badge_settings.size = new_bs;
                                this.badge_size = new_bs;
                                this.toolbar.current_badge_size = new_bs;
                                let sw = this.logical_w();
                                let sh = this.logical_h();
                                this.toolbar.update_layout(sw, sh);
                                let nm = new_bs.name();
                                this.set_toast("🔢", format!("Badge Size: {}", nm));
                            } else {
                                let new_w = (this.stroke_width + 2.0).min(40.0);
                                match this.current_tool {
                                    DrawTool::Pen => this.pen_settings.stroke_width = new_w,
                                    DrawTool::Highlighter => {
                                        this.highlighter_settings.stroke_width = new_w
                                    }
                                    DrawTool::Line => this.line_settings.stroke_width = new_w,
                                    DrawTool::Arrow => this.arrow_settings.stroke_width = new_w,
                                    DrawTool::Rectangle => this.rect_settings.stroke_width = new_w,
                                    DrawTool::RoundedRectangle => {
                                        this.rounded_rect_settings.stroke_width = new_w
                                    }
                                    DrawTool::Ellipse => this.ellipse_settings.stroke_width = new_w,
                                    DrawTool::Blur => this.blur_settings.block_size = new_w,
                                    _ => {}
                                }
                                this.stroke_width = new_w;
                                this.toolbar.stroke_width = new_w;
                                let sw = this.logical_w();
                                let sh = this.logical_h();
                                this.toolbar.update_layout(sw, sh);
                                this.set_toast("🖌️", format!("{} px", new_w.round() as u32));
                            }
                            this.request_repaint();
                        }

                        // ─── Arrow Keys & Zoom/Timer Controls ───
                        k if k == VK_UP.0 as i32 || k == 187 => {
                            // Up or '+'
                            if this.mode == AppMode::Loupe {
                                let new_lvl = (this.loupe.magnification + 0.25).clamp(1.25, 12.0);
                                this.loupe.magnification = new_lvl;
                                this.set_toast("🔍", format!("Loupe Zoom {:.2}x", new_lvl));
                            } else if this.mode == AppMode::Timer {
                                this.adjust_timer(60.0);
                                let m = this.timer_total_mins();
                                this.set_toast("⏱️", format!("Timer: +1m ({}m total)", m));
                            } else if this.mode == AppMode::StaticZoom {
                                let mut pt = POINT::default();
                                let sx = this.screen_x;
                                let sy = this.screen_y;
                                let sw = this.logical_w();
                                let sh = this.logical_h();
                                let (cursor_x, cursor_y) = if GetCursorPos(&mut pt).is_ok() {
                                    (
                                this.px_to_dip((pt.x - sx) as f32),
                                this.px_to_dip((pt.y - sy) as f32),
                            )
                                } else {
                                    (sw / 2.0, sh / 2.0)
                                };
                                let new_lvl = (this.zoom.level + 0.25).clamp(1.0, 10.0);
                                this.zoom.set_zoom_centered(
                                    new_lvl,
                                    Point2D::new(cursor_x, cursor_y),
                                    sw,
                                    sh,
                                );
                                this.set_toast("🔎", format!("Zoom {:.2}x", new_lvl));
                            } else {
                                let new_w = (this.stroke_width + 2.0).min(40.0);
                                this.stroke_width = new_w;
                                this.set_toast("🖌️", format!("{} px", new_w.round() as u32));
                            }
                            this.request_repaint();
                        }
                        k if k == VK_DOWN.0 as i32 || k == 189 => {
                            // Down or '-'
                            if this.mode == AppMode::Loupe {
                                let new_lvl = (this.loupe.magnification - 0.25).clamp(1.25, 12.0);
                                this.loupe.magnification = new_lvl;
                                this.set_toast("🔍", format!("Loupe Zoom {:.2}x", new_lvl));
                            } else if this.mode == AppMode::Timer {
                                this.adjust_timer(-60.0);
                                let m = this.timer_total_mins();
                                this.set_toast("⏱️", format!("Timer: -1m ({}m total)", m));
                            } else if this.mode == AppMode::StaticZoom {
                                let mut pt = POINT::default();
                                let sx = this.screen_x;
                                let sy = this.screen_y;
                                let sw = this.logical_w();
                                let sh = this.logical_h();
                                let (cursor_x, cursor_y) = if GetCursorPos(&mut pt).is_ok() {
                                    (
                                this.px_to_dip((pt.x - sx) as f32),
                                this.px_to_dip((pt.y - sy) as f32),
                            )
                                } else {
                                    (sw / 2.0, sh / 2.0)
                                };
                                let new_lvl = (this.zoom.level - 0.25).clamp(1.0, 10.0);
                                this.zoom.set_zoom_centered(
                                    new_lvl,
                                    Point2D::new(cursor_x, cursor_y),
                                    sw,
                                    sh,
                                );
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

                windows::Win32::UI::WindowsAndMessaging::WM_KEYUP => {
                    let key = wparam.0 as i32;
                    if key == VK_SHIFT.0 as i32 {
                        let mut pt = POINT::default();
                        let current_screen_pt = if GetCursorPos(&mut pt).is_ok() {
                            Point2D::new(
                                this.px_to_dip((pt.x - this.screen_x) as f32),
                                this.px_to_dip((pt.y - this.screen_y) as f32),
                            )
                        } else {
                            this.last_mouse_pos
                        };
                        this.last_mouse_pos = current_screen_pt;

                        this.request_repaint();
                        return LRESULT(0);
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
