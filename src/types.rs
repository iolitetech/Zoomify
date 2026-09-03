#![allow(dead_code)]

use std::time::Instant;
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Idle,
    StaticZoom,
    Draw,
    Spotlight,
    LiveZoom,
    Timer,
    Snip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawTool {
    Pen,
    Highlighter,
    LaserPointer,
    Eraser,
    Line,
    Arrow,
    Rectangle,
    RoundedRectangle,
    Ellipse,
    Text,
    StepBadge,
    Snip,
}

impl DrawTool {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Pen => "Pen",
            Self::Highlighter => "Highlighter",
            Self::LaserPointer => "Laser Pointer",
            Self::Eraser => "Eraser",
            Self::Line => "Line",
            Self::Arrow => "Arrow",
            Self::Rectangle => "Rectangle",
            Self::RoundedRectangle => "Rounded Rect",
            Self::Ellipse => "Ellipse",
            Self::Text => "Text",
            Self::StepBadge => "Step Badge",
            Self::Snip => "Snip",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorPreset {
    Red,
    Green,
    Blue,
    Yellow,
    Orange,
    Pink,
    Cyan,
    White,
    Black,
}

impl ColorPreset {
    pub fn to_d2d_color(&self, alpha: f32) -> D2D1_COLOR_F {
        let (r, g, b) = match self {
            Self::Red => (0.95, 0.15, 0.15),
            Self::Green => (0.15, 0.85, 0.25),
            Self::Blue => (0.15, 0.55, 0.98),
            Self::Yellow => (1.0, 0.88, 0.1),
            Self::Orange => (1.0, 0.55, 0.05),
            Self::Pink => (0.98, 0.25, 0.65),
            Self::Cyan => (0.05, 0.88, 0.95),
            Self::White => (0.98, 0.98, 0.98),
            Self::Black => (0.10, 0.10, 0.12),
        };
        D2D1_COLOR_F { r, g, b, a: alpha }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Yellow => "Yellow",
            Self::Orange => "Orange",
            Self::Pink => "Pink",
            Self::Cyan => "Cyan",
            Self::White => "White",
            Self::Black => "Black",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasBackground {
    Transparent,
    Whiteboard,
    Blackboard,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point2D {
    pub x: f32,
    pub y: f32,
}

impl Point2D {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance(&self, other: &Point2D) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillMode {
    None,
    Tinted,
    Solid,
}

impl FillMode {
    pub fn name(&self) -> &'static str {
        match self {
            Self::None => "Outline Only",
            Self::Tinted => "Tinted Fill",
            Self::Solid => "Solid Fill",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokePattern {
    Solid,
    Dashed,
    Dotted,
}

impl StrokePattern {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::Dashed => "Dashed",
            Self::Dotted => "Dotted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowStyle {
    Single,
    Double,
    Dimension,
}

impl ArrowStyle {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Single => "Single Arrow",
            Self::Double => "Double Arrow",
            Self::Dimension => "Dimension Line",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeSize {
    Small,
    Medium,
    Large,
    ExtraLarge,
}

impl BadgeSize {
    pub fn radius(&self) -> f32 {
        match self {
            Self::Small => 14.0,
            Self::Medium => 18.0,
            Self::Large => 24.0,
            Self::ExtraLarge => 30.0,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Small => "Small (14px)",
            Self::Medium => "Medium (18px)",
            Self::Large => "Large (24px)",
            Self::ExtraLarge => "XL (30px)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeShape {
    Circle,
    Square,
    Hexagon,
}

impl BadgeShape {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Circle => "Circle",
            Self::Square => "Square",
            Self::Hexagon => "Hexagon",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCardStyle {
    Transparent,
    Badge,
    Solid,
}

impl TextCardStyle {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Transparent => "None (Float)",
            Self::Badge => "Badge (Pill)",
            Self::Solid => "Card (Solid)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFontFamily {
    SegoeUI,
    CascadiaCode,
    SegoePrint,
}

impl TextFontFamily {
    pub fn name(&self) -> &'static str {
        match self {
            Self::SegoeUI => "Segoe UI",
            Self::CascadiaCode => "Cascadia Code",
            Self::SegoePrint => "Segoe Print",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Shape {
    Stroke {
        points: Vec<Point2D>,
        color: ColorPreset,
        width: f32,
        is_highlighter: bool,
        pattern: StrokePattern,
    },
    Line {
        start: Point2D,
        end: Point2D,
        color: ColorPreset,
        width: f32,
        pattern: StrokePattern,
    },
    Arrow {
        start: Point2D,
        end: Point2D,
        color: ColorPreset,
        width: f32,
        style: ArrowStyle,
        pattern: StrokePattern,
    },
    Rectangle {
        start: Point2D,
        end: Point2D,
        color: ColorPreset,
        width: f32,
        rounded: bool,
        fill: FillMode,
        pattern: StrokePattern,
    },
    Ellipse {
        start: Point2D,
        end: Point2D,
        color: ColorPreset,
        width: f32,
        fill: FillMode,
        pattern: StrokePattern,
    },
    Text {
        origin: Point2D,
        text: String,
        font_size: f32,
        color: ColorPreset,
        is_bold: bool,
        is_italic: bool,
        card_style: TextCardStyle,
        font_family: TextFontFamily,
    },
    StepBadge {
        center: Point2D,
        number: u32,
        radius: f32,
        color: ColorPreset,
        shape: BadgeShape,
        fill: FillMode,
        stroke_width: f32,
        pattern: StrokePattern,
    },
}

#[derive(Debug, Clone)]
pub struct TextEditorState {
    pub origin: Point2D,
    pub text: String,
    pub cursor: usize,
    pub color: ColorPreset,
    pub font_size: f32,
    pub is_bold: bool,
    pub is_italic: bool,
    pub card_style: TextCardStyle,
    pub font_family: TextFontFamily,
}

impl TextEditorState {
    pub fn new(
        origin: Point2D,
        color: ColorPreset,
        font_size: f32,
        is_bold: bool,
        is_italic: bool,
        card_style: TextCardStyle,
        font_family: TextFontFamily,
    ) -> Self {
        Self {
            origin,
            text: String::new(),
            cursor: 0,
            color,
            font_size,
            is_bold,
            is_italic,
            card_style,
            font_family,
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        if self.cursor >= self.text.len() {
            self.text.push(ch);
            self.cursor = self.text.len();
        } else {
            self.text.insert(self.cursor, ch);
            self.cursor += ch.len_utf8();
        }
    }

    pub fn insert_str(&mut self, s: &str) {
        if self.cursor >= self.text.len() {
            self.text.push_str(s);
            self.cursor = self.text.len();
        } else {
            self.text.insert_str(self.cursor, s);
            self.cursor += s.len();
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 && !self.text.is_empty() {
            let mut prev = self.cursor - 1;
            while prev > 0 && !self.text.is_char_boundary(prev) {
                prev -= 1;
            }
            self.text.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    pub fn delete_forward(&mut self) {
        if self.cursor < self.text.len() {
            let mut next = self.cursor + 1;
            while next < self.text.len() && !self.text.is_char_boundary(next) {
                next += 1;
            }
            self.text.drain(self.cursor..next);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            let mut prev = self.cursor - 1;
            while prev > 0 && !self.text.is_char_boundary(prev) {
                prev -= 1;
            }
            self.cursor = prev;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.text.len() {
            let mut next = self.cursor + 1;
            while next < self.text.len() && !self.text.is_char_boundary(next) {
                next += 1;
            }
            self.cursor = next;
        }
    }
}

#[derive(Debug, Clone)]
pub enum HistoryAction {
    AddShape(Shape),
    AddStepBadge {
        shape: Shape,
        prev_counter: u32,
    },
    DeleteShape {
        index: usize,
        shape: Shape,
    },
    Clear(Vec<Shape>),
}

#[derive(Debug, Clone)]
pub struct LaserTrailPoint {
    pub pt: Point2D,
    pub timestamp: Instant,
}

#[derive(Debug, Clone)]
pub struct SpotlightState {
    pub active: bool,
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub pinned: bool,
    pub dim_opacity: f32,
}

impl Default for SpotlightState {
    fn default() -> Self {
        Self {
            active: false,
            x: 0.0,
            y: 0.0,
            radius: 180.0,
            pinned: false,
            dim_opacity: 0.92,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ZoomState {
    pub level: f32,
    pub target_level: f32,
    pub view_x: f32,
    pub view_y: f32,
    pub target_view_x: f32,
    pub target_view_y: f32,
    pub is_dragging: bool,
    pub drag_start_mouse: Point2D,
    pub drag_start_view: Point2D,
}

impl Default for ZoomState {
    fn default() -> Self {
        Self {
            level: 1.0,
            target_level: 1.0,
            view_x: 0.0,
            view_y: 0.0,
            target_view_x: 0.0,
            target_view_y: 0.0,
            is_dragging: false,
            drag_start_mouse: Point2D::default(),
            drag_start_view: Point2D::default(),
        }
    }
}

impl ZoomState {
    pub fn screen_to_canvas(&self, screen_pt: Point2D) -> Point2D {
        if self.level <= 1.001 {
            screen_pt
        } else {
            Point2D {
                x: self.view_x + (screen_pt.x / self.level),
                y: self.view_y + (screen_pt.y / self.level),
            }
        }
    }

    pub fn canvas_to_screen(&self, canvas_pt: Point2D) -> Point2D {
        if self.level <= 1.001 {
            canvas_pt
        } else {
            Point2D {
                x: (canvas_pt.x - self.view_x) * self.level,
                y: (canvas_pt.y - self.view_y) * self.level,
            }
        }
    }

    pub fn clamp_viewport(&mut self, screen_w: f32, screen_h: f32) {
        let z = self.level.max(1.0);
        let view_w = screen_w / z;
        let view_h = screen_h / z;
        let max_x = (screen_w - view_w).max(0.0);
        let max_y = (screen_h - view_h).max(0.0);
        self.target_view_x = self.target_view_x.clamp(0.0, max_x);
        self.target_view_y = self.target_view_y.clamp(0.0, max_y);
        self.view_x = self.view_x.clamp(0.0, max_x);
        self.view_y = self.view_y.clamp(0.0, max_y);
    }

    pub fn update_target_from_cursor(&mut self, cursor_x: f32, cursor_y: f32, screen_w: f32, screen_h: f32) {
        let z = self.level.max(1.0);
        if z <= 1.001 || screen_w <= 0.0 || screen_h <= 0.0 {
            self.target_view_x = 0.0;
            self.target_view_y = 0.0;
            self.view_x = 0.0;
            self.view_y = 0.0;
            return;
        }
        let view_w = screen_w / z;
        let view_h = screen_h / z;
        let max_x = (screen_w - view_w).max(0.0);
        let max_y = (screen_h - view_h).max(0.0);

        let norm_x = (cursor_x / screen_w).clamp(0.0, 1.0);
        let norm_y = (cursor_y / screen_h).clamp(0.0, 1.0);

        self.target_view_x = norm_x * max_x;
        self.target_view_y = norm_y * max_y;
        self.view_x = self.target_view_x;
        self.view_y = self.target_view_y;
    }

    pub fn set_zoom_centered(&mut self, new_level: f32, center_screen: Point2D, screen_w: f32, screen_h: f32) {
        let old_z = self.level.max(1.0);
        let new_z = new_level.clamp(1.0, 10.0);

        // Point on canvas currently under center_screen
        let canvas_cx = self.view_x + (center_screen.x / old_z);
        let canvas_cy = self.view_y + (center_screen.y / old_z);

        self.target_level = new_z;
        self.level = new_z;

        // New viewport top-left so that canvas_cx remains under center_screen
        let new_view_w = screen_w / new_z;
        let new_view_h = screen_h / new_z;
        let max_x = (screen_w - new_view_w).max(0.0);
        let max_y = (screen_h - new_view_h).max(0.0);

        self.view_x = (canvas_cx - center_screen.x / new_z).clamp(0.0, max_x);
        self.view_y = (canvas_cy - center_screen.y / new_z).clamp(0.0, max_y);
        self.target_view_x = self.view_x;
        self.target_view_y = self.view_y;
    }

    pub fn tick_smooth_pan(&mut self, lerp: f32, screen_w: f32, screen_h: f32) -> bool {
        self.clamp_viewport(screen_w, screen_h);
        let dx = self.target_view_x - self.view_x;
        let dy = self.target_view_y - self.view_y;
        let dl = self.target_level - self.level;

        let moving = dx.abs() > 0.2 || dy.abs() > 0.2 || dl.abs() > 0.005;
        if moving {
            let f = lerp.clamp(0.05, 1.0);
            self.view_x += dx * f;
            self.view_y += dy * f;
            self.level += dl * f;
        } else {
            self.view_x = self.target_view_x;
            self.view_y = self.target_view_y;
            self.level = self.target_level;
        }
        moving
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeToolSettings {
    pub stroke_width: f32,
    pub pattern: StrokePattern,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeToolSettings {
    pub stroke_width: f32,
    pub fill_mode: FillMode,
    pub pattern: StrokePattern,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowToolSettings {
    pub stroke_width: f32,
    pub style: ArrowStyle,
    pub pattern: StrokePattern,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepBadgeToolSettings {
    pub size: BadgeSize,
    pub shape: BadgeShape,
    pub fill: FillMode,
    pub stroke_width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextToolSettings {
    pub font_size: f32,
    pub is_bold: bool,
    pub is_italic: bool,
    pub card_style: TextCardStyle,
    pub font_family: TextFontFamily,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnipShape {
    Rectangle,
    Ellipse,
}

#[derive(Debug, Clone)]
pub struct SnipSelection {
    pub active: bool,
    pub start: Point2D,
    pub current: Point2D,
    pub shape: SnipShape,
    pub with_guides: bool,
}

impl Default for SnipSelection {
    fn default() -> Self {
        Self {
            active: false,
            start: Point2D::default(),
            current: Point2D::default(),
            shape: SnipShape::Rectangle,
            with_guides: false,
        }
    }
}

impl SnipSelection {
    pub fn rect(&self) -> (f32, f32, f32, f32) {
        let left = self.start.x.min(self.current.x);
        let top = self.start.y.min(self.current.y);
        let right = self.start.x.max(self.current.x);
        let bottom = self.start.y.max(self.current.y);
        (left, top, right, bottom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerAction {
    PlayPause,
    AddMinute,
    SubMinute,
    Reset,
    ToggleMinimize,
    Close,
    SetDuration(u32),
    CycleCorner,
}

#[derive(Debug, Clone)]
pub struct TimerWidgetState {
    pub dim_opacity: f32,
    pub minimized: bool,
    pub drag_start_mouse: Point2D,
    pub drag_start_pos: Point2D,
    pub is_dragging: bool,
    pub custom_pos: Option<Point2D>,
    pub hover_action: Option<TimerAction>,
    pub pill_corner: u8, // 0: Top-Right, 1: Top-Left, 2: Bottom-Left, 3: Bottom-Right
    pub session_title: String,
}

impl Default for TimerWidgetState {
    fn default() -> Self {
        Self {
            dim_opacity: 0.65,
            minimized: false,
            drag_start_mouse: Point2D::new(0.0, 0.0),
            drag_start_pos: Point2D::new(0.0, 0.0),
            is_dragging: false,
            custom_pos: None,
            hover_action: None,
            pill_corner: 0,
            session_title: "PRESENTATION TIMER".to_string(),
        }
    }
}

impl TimerWidgetState {
    pub fn get_card_bounds(&self, screen_w: f32, screen_h: f32) -> (f32, f32, f32, f32, f32, f32) {
        let card_w = 580.0;
        let card_h = 450.0;
        let (cx, cy) = if let Some(pos) = self.custom_pos {
            (pos.x, pos.y)
        } else {
            (screen_w / 2.0, screen_h / 2.0)
        };
        (cx - card_w / 2.0, cy - card_h / 2.0, cx + card_w / 2.0, cy + card_h / 2.0, cx, cy)
    }

    pub fn get_pill_rect(&self, screen_w: f32, screen_h: f32) -> (f32, f32, f32, f32) {
        let pill_w = 280.0;
        let pill_h = 46.0;
        let margin = 24.0;
        match self.pill_corner {
            1 => (margin, margin, margin + pill_w, margin + pill_h), // Top-Left
            2 => (margin, screen_h - margin - pill_h, margin + pill_w, screen_h - margin), // Bottom-Left
            3 => (screen_w - margin - pill_w, screen_h - margin - pill_h, screen_w - margin, screen_h - margin), // Bottom-Right
            _ => (screen_w - margin - pill_w, margin, screen_w - margin, margin + pill_h), // Top-Right (default 0)
        }
    }

    pub fn get_action_at(&self, pt: Point2D, screen_w: f32, screen_h: f32) -> Option<TimerAction> {
        if self.minimized {
            let (left, top, right, bottom) = self.get_pill_rect(screen_w, screen_h);

            if pt.x < left || pt.x > right || pt.y < top || pt.y > bottom {
                return None;
            }

            // b0: Corner cycle button (left + 6.0 .. left + 34.0)
            if pt.x >= left + 6.0 && pt.x <= left + 34.0 && pt.y >= top + 8.0 && pt.y <= bottom - 8.0 {
                return Some(TimerAction::CycleCorner);
            }

            // b1: Play/Pause (right - 105.0 .. right - 72.0, top + 8.0 .. bottom - 8.0)
            if pt.x >= right - 105.0 && pt.x <= right - 72.0 && pt.y >= top + 8.0 && pt.y <= bottom - 8.0 {
                return Some(TimerAction::PlayPause);
            }
            // b2: Expand / ToggleMinimize (right - 68.0 .. right - 38.0, top + 8.0 .. bottom - 8.0)
            if pt.x >= right - 68.0 && pt.x <= right - 38.0 && pt.y >= top + 8.0 && pt.y <= bottom - 8.0 {
                return Some(TimerAction::ToggleMinimize);
            }
            // b3: Close (right - 35.0 .. right - 5.0, top + 8.0 .. bottom - 8.0)
            if pt.x >= right - 35.0 && pt.x <= right - 5.0 && pt.y >= top + 8.0 && pt.y <= bottom - 8.0 {
                return Some(TimerAction::Close);
            }

            return Some(TimerAction::ToggleMinimize);
        }

        let (card_left, card_top, card_right, card_bottom, cx, cy) = self.get_card_bounds(screen_w, screen_h);

        // Outside card?
        if pt.x < card_left || pt.x > card_right || pt.y < card_top || pt.y > card_bottom {
            return None;
        }

        // Top-Right Close Button (✕)
        let close_cx = card_right - 32.0;
        let close_cy = card_top + 28.0;
        let cdx = pt.x - close_cx;
        let cdy = pt.y - close_cy;
        if (cdx * cdx + cdy * cdy).sqrt() <= 20.0 {
            return Some(TimerAction::Close);
        }

        // Quick Duration Pills Row: [5m] [10m] [15m] [25m] [30m]
        let pill_w = 60.0;
        let pill_h = 30.0;
        let pill_gap = 10.0;
        let total_pills_w = 5.0 * pill_w + 4.0 * pill_gap;
        let pill_row_x = cx - total_pills_w / 2.0;
        let pill_row_y = cy - 162.0;

        let durations = [5, 10, 15, 25, 30];
        for (i, &dur) in durations.iter().enumerate() {
            let px = pill_row_x + i as f32 * (pill_w + pill_gap);
            if pt.x >= px && pt.x <= px + pill_w && pt.y >= pill_row_y && pt.y <= pill_row_y + pill_h {
                return Some(TimerAction::SetDuration(dur));
            }
        }

        // Modern Floating Action Controls: [-1m] [⟲] [ Hero Play/Pause ] [+1m] [🗗]
        let btn_y = cy + 130.0;

        // 1. Center Hero Play/Pause button (radius 28.0)
        let pdx = pt.x - cx;
        let pdy = pt.y - btn_y;
        if (pdx * pdx + pdy * pdy).sqrt() <= 32.0 {
            return Some(TimerAction::PlayPause);
        }

        // 2. Secondary Circular Controls (radius 22.0)
        let secondary_controls = [
            (cx - 120.0, TimerAction::SubMinute),
            (cx - 60.0, TimerAction::Reset),
            (cx + 60.0, TimerAction::AddMinute),
            (cx + 120.0, TimerAction::ToggleMinimize),
        ];

        for &(scx, action) in &secondary_controls {
            let dx = pt.x - scx;
            let dy = pt.y - btn_y;
            if (dx * dx + dy * dy).sqrt() <= 24.0 {
                return Some(action);
            }
        }

        // Central clock circle click toggles pause/play
        let clock_cy = cy - 10.0;
        let dx = pt.x - cx;
        let dy = pt.y - clock_cy;
        if (dx * dx + dy * dy).sqrt() <= 105.0 {
            return Some(TimerAction::PlayPause);
        }

        None
    }
}

#[derive(Debug, Clone)]
pub struct ToastNotification {
    pub icon: &'static str,
    pub message: String,
    pub created_at: Instant,
    pub duration_secs: f32,
}

impl ToastNotification {
    pub fn new(icon: &'static str, message: impl Into<String>) -> Self {
        Self {
            icon,
            message: message.into(),
            created_at: Instant::now(),
            duration_secs: 1.8,
        }
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed().as_secs_f32() > self.duration_secs
    }

    pub fn opacity(&self) -> f32 {
        let elapsed = self.created_at.elapsed().as_secs_f32();
        if elapsed < 0.15 {
            elapsed / 0.15
        } else if elapsed > (self.duration_secs - 0.35) {
            ((self.duration_secs - elapsed) / 0.35).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FluentAction {
    ModeZoom,
    ModeDraw,
    ModeSpotlight,
    ModeTimer,
    ModeSnip,
    CycleDisplay,
    Tool(DrawTool),
    Color(ColorPreset),
    Undo,
    Clear,
    Copy,
    Save,
    Close,
    ToggleCollapse,
    // Context Sub-bar Actions
    SetStrokeWidth(f32),
    SetFillMode(FillMode),
    SetStrokePattern(StrokePattern),
    SetArrowStyle(ArrowStyle),
    SetBadgeSize(BadgeSize),
    SetBadgeShape(BadgeShape),
    ResetBadgeCounter,
    // Text Sub-bar Actions
    SetFontSize(f32),
    ToggleBold,
    ToggleItalic,
    SetTextCardStyle(TextCardStyle),
    SetFontFamily(TextFontFamily),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolbarItemBounds {
    pub action: FluentAction,
    pub rect: D2D_RECT_F,
}

#[derive(Debug, Clone)]
pub struct FluentToolbarState {
    pub visible: bool,
    pub collapsed: bool,
    pub hover_action: Option<FluentAction>,
    pub custom_position: Option<Point2D>,
    pub is_dragging: bool,
    pub drag_start_mouse: Point2D,
    pub drag_start_bar: Point2D,
    pub bar_rect: D2D_RECT_F,
    pub grip_rect: D2D_RECT_F,
    pub items: Vec<ToolbarItemBounds>,
    pub monitor_count: usize,
    pub current_monitor_index: usize,
    pub active_tool: Option<DrawTool>,
    pub current_fill_mode: FillMode,
    pub current_stroke_pattern: StrokePattern,
    pub current_arrow_style: ArrowStyle,
    pub current_badge_size: BadgeSize,
    pub current_badge_shape: BadgeShape,
    pub stroke_width: f32,
    pub badge_counter: u32,
    pub current_font_size: f32,
    pub text_is_bold: bool,
    pub text_is_italic: bool,
    pub text_card_style: TextCardStyle,
    pub text_font_family: TextFontFamily,
    pub subbar_rect: Option<D2D_RECT_F>,
    pub subbar_items: Vec<ToolbarItemBounds>,
    pub separators: Vec<f32>,
    pub subbar_separators: Vec<f32>,
}

impl Default for FluentToolbarState {
    fn default() -> Self {
        Self {
            visible: true,
            collapsed: false,
            hover_action: None,
            custom_position: None,
            is_dragging: false,
            drag_start_mouse: Point2D::default(),
            drag_start_bar: Point2D::default(),
            bar_rect: D2D_RECT_F::default(),
            grip_rect: D2D_RECT_F::default(),
            items: Vec::new(),
            monitor_count: 1,
            current_monitor_index: 0,
            active_tool: None,
            current_fill_mode: FillMode::None,
            current_stroke_pattern: StrokePattern::Solid,
            current_arrow_style: ArrowStyle::Single,
            current_badge_size: BadgeSize::Medium,
            current_badge_shape: BadgeShape::Circle,
            stroke_width: 4.0,
            badge_counter: 1,
            current_font_size: 22.0,
            text_is_bold: false,
            text_is_italic: false,
            text_card_style: TextCardStyle::Transparent,
            text_font_family: TextFontFamily::SegoeUI,
            subbar_rect: None,
            subbar_items: Vec::new(),
            separators: Vec::new(),
            subbar_separators: Vec::new(),
        }
    }
}

impl FluentToolbarState {
    pub fn update_layout(&mut self, screen_w: f32, screen_h: f32) {
        let (bar, items, grip, seps) = compute_toolbar_layout(screen_w, screen_h, self.collapsed, self.custom_position, self.monitor_count);
        self.bar_rect = bar;
        self.items = items;
        self.grip_rect = grip;
        self.separators = seps;

        if !self.collapsed && self.visible && self.active_tool.is_some() {
            let (s_rect, s_items, s_seps) = compute_subbar_layout(
                bar,
                self.active_tool,
                self.current_fill_mode,
                self.current_stroke_pattern,
                self.current_arrow_style,
                self.current_badge_size,
                self.current_badge_shape,
                self.stroke_width,
                self.badge_counter,
            );
            self.subbar_rect = s_rect;
            self.subbar_items = s_items;
            self.subbar_separators = s_seps;
        } else {
            self.subbar_rect = None;
            self.subbar_items.clear();
            self.subbar_separators.clear();
        }
    }

    pub fn hit_test_grip(&self, x: f32, y: f32) -> bool {
        self.visible
            && x >= self.grip_rect.left
            && x <= self.grip_rect.right
            && y >= self.grip_rect.top
            && y <= self.grip_rect.bottom
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<FluentAction> {
        if !self.visible {
            return None;
        }
        for item in &self.items {
            if x >= item.rect.left && x <= item.rect.right && y >= item.rect.top && y <= item.rect.bottom {
                return Some(item.action);
            }
        }
        for item in &self.subbar_items {
            if x >= item.rect.left && x <= item.rect.right && y >= item.rect.top && y <= item.rect.bottom {
                return Some(item.action);
            }
        }
        None
    }

    pub fn is_point_inside(&self, x: f32, y: f32) -> bool {
        if !self.visible {
            return false;
        }
        let in_main = x >= self.bar_rect.left && x <= self.bar_rect.right && y >= self.bar_rect.top && y <= self.bar_rect.bottom;
        if in_main {
            return true;
        }
        if let Some(sb) = self.subbar_rect {
            if x >= sb.left && x <= sb.right && y >= sb.top && y <= sb.bottom {
                return true;
            }
        }
        false
    }
}

pub fn compute_subbar_layout(
    bar_rect: D2D_RECT_F,
    active_tool: Option<DrawTool>,
    _current_fill: FillMode,
    _current_pattern: StrokePattern,
    _current_arrow: ArrowStyle,
    _current_badge_sz: BadgeSize,
    _current_badge_sh: BadgeShape,
    _current_width: f32,
    _badge_count: u32,
) -> (Option<D2D_RECT_F>, Vec<ToolbarItemBounds>, Vec<f32>) {
    let tool = match active_tool {
        Some(t) => t,
        None => return (None, Vec::new(), Vec::new()),
    };

    let mut groups: Vec<Vec<(FluentAction, f32)>> = Vec::with_capacity(5);

    match tool {
        DrawTool::Rectangle | DrawTool::RoundedRectangle | DrawTool::Ellipse => {
            groups.push(vec![
                (FluentAction::SetStrokeWidth(2.0), 30.0),
                (FluentAction::SetStrokeWidth(4.0), 30.0),
                (FluentAction::SetStrokeWidth(8.0), 30.0),
                (FluentAction::SetStrokeWidth(14.0), 34.0),
            ]);
            groups.push(vec![
                (FluentAction::SetFillMode(FillMode::None), 58.0),
                (FluentAction::SetFillMode(FillMode::Tinted), 42.0),
                (FluentAction::SetFillMode(FillMode::Solid), 46.0),
            ]);
            groups.push(vec![
                (FluentAction::SetStrokePattern(StrokePattern::Solid), 32.0),
                (FluentAction::SetStrokePattern(StrokePattern::Dashed), 34.0),
                (FluentAction::SetStrokePattern(StrokePattern::Dotted), 34.0),
            ]);
        }
        DrawTool::Line => {
            groups.push(vec![
                (FluentAction::SetStrokeWidth(2.0), 30.0),
                (FluentAction::SetStrokeWidth(4.0), 30.0),
                (FluentAction::SetStrokeWidth(8.0), 30.0),
                (FluentAction::SetStrokeWidth(14.0), 34.0),
            ]);
            groups.push(vec![
                (FluentAction::SetStrokePattern(StrokePattern::Solid), 32.0),
                (FluentAction::SetStrokePattern(StrokePattern::Dashed), 34.0),
                (FluentAction::SetStrokePattern(StrokePattern::Dotted), 34.0),
            ]);
        }
        DrawTool::Arrow => {
            groups.push(vec![
                (FluentAction::SetStrokeWidth(2.0), 30.0),
                (FluentAction::SetStrokeWidth(4.0), 30.0),
                (FluentAction::SetStrokeWidth(8.0), 30.0),
                (FluentAction::SetStrokeWidth(14.0), 34.0),
            ]);
            groups.push(vec![
                (FluentAction::SetArrowStyle(ArrowStyle::Single), 36.0),
                (FluentAction::SetArrowStyle(ArrowStyle::Double), 42.0),
                (FluentAction::SetArrowStyle(ArrowStyle::Dimension), 44.0),
            ]);
            groups.push(vec![
                (FluentAction::SetStrokePattern(StrokePattern::Solid), 32.0),
                (FluentAction::SetStrokePattern(StrokePattern::Dashed), 34.0),
                (FluentAction::SetStrokePattern(StrokePattern::Dotted), 34.0),
            ]);
        }
        DrawTool::StepBadge => {
            groups.push(vec![
                (FluentAction::SetBadgeSize(BadgeSize::Small), 30.0),
                (FluentAction::SetBadgeSize(BadgeSize::Medium), 30.0),
                (FluentAction::SetBadgeSize(BadgeSize::Large), 30.0),
                (FluentAction::SetBadgeSize(BadgeSize::ExtraLarge), 34.0),
            ]);
            groups.push(vec![
                (FluentAction::SetBadgeShape(BadgeShape::Circle), 32.0),
                (FluentAction::SetBadgeShape(BadgeShape::Square), 32.0),
                (FluentAction::SetBadgeShape(BadgeShape::Hexagon), 32.0),
            ]);
            groups.push(vec![
                (FluentAction::SetFillMode(FillMode::None), 40.0),
                (FluentAction::SetFillMode(FillMode::Tinted), 40.0),
                (FluentAction::SetFillMode(FillMode::Solid), 40.0),
            ]);
            groups.push(vec![
                (FluentAction::SetStrokeWidth(2.0), 28.0),
                (FluentAction::SetStrokeWidth(4.0), 28.0),
                (FluentAction::SetStrokeWidth(6.0), 28.0),
            ]);
            groups.push(vec![
                (FluentAction::ResetBadgeCounter, 54.0),
            ]);
        }
        DrawTool::Pen | DrawTool::Highlighter => {
            groups.push(vec![
                (FluentAction::SetStrokeWidth(2.0), 28.0),
                (FluentAction::SetStrokeWidth(4.0), 28.0),
                (FluentAction::SetStrokeWidth(8.0), 28.0),
                (FluentAction::SetStrokeWidth(14.0), 28.0),
            ]);
            groups.push(vec![
                (FluentAction::SetStrokePattern(StrokePattern::Solid), 32.0),
                (FluentAction::SetStrokePattern(StrokePattern::Dashed), 32.0),
            ]);
        }
        DrawTool::Text => {
            groups.push(vec![
                (FluentAction::SetFontSize(14.0), 34.0),
                (FluentAction::SetFontSize(20.0), 34.0),
                (FluentAction::SetFontSize(28.0), 34.0),
                (FluentAction::SetFontSize(38.0), 36.0),
            ]);
            groups.push(vec![
                (FluentAction::ToggleBold, 30.0),
                (FluentAction::ToggleItalic, 30.0),
            ]);
            groups.push(vec![
                (FluentAction::SetTextCardStyle(TextCardStyle::Transparent), 44.0),
                (FluentAction::SetTextCardStyle(TextCardStyle::Badge), 48.0),
                (FluentAction::SetTextCardStyle(TextCardStyle::Solid), 46.0),
            ]);
            groups.push(vec![
                (FluentAction::SetFontFamily(TextFontFamily::SegoeUI), 42.0),
                (FluentAction::SetFontFamily(TextFontFamily::CascadiaCode), 46.0),
            ]);
        }
        _ => return (None, Vec::new(), Vec::new()),
    }

    if groups.is_empty() {
        return (None, Vec::new(), Vec::new());
    }

    let sub_h = 36.0;
    let pad_x = 8.0;
    let item_spacing = 3.0;
    let group_divider_spacing = 7.0;
    let divider_gap = group_divider_spacing * 2.0 + 1.0;

    let mut total_w = pad_x * 2.0;
    for (g_idx, group) in groups.iter().enumerate() {
        for (i, &(_, w)) in group.iter().enumerate() {
            total_w += w;
            if i + 1 < group.len() {
                total_w += item_spacing;
            }
        }
        if g_idx + 1 < groups.len() {
            total_w += divider_gap;
        }
    }

    let bar_center_x = (bar_rect.left + bar_rect.right) / 2.0;
    let left = (bar_center_x - total_w / 2.0).max(6.0);
    let top = bar_rect.bottom + 6.0;
    let right = left + total_w;
    let bottom = top + sub_h;

    let subbar_rect = D2D_RECT_F { left, top, right, bottom };

    let mut items = Vec::new();
    let mut separators = Vec::new();
    let mut cur_x = left + pad_x;

    for (g_idx, group) in groups.iter().enumerate() {
        for &(action, w) in group {
            let rect = D2D_RECT_F {
                left: cur_x,
                top: top + 4.0,
                right: cur_x + w,
                bottom: bottom - 4.0,
            };
            items.push(ToolbarItemBounds { action, rect });
            cur_x += w + item_spacing;
        }
        cur_x -= item_spacing;

        if g_idx + 1 < groups.len() {
            let sep_x = cur_x + group_divider_spacing;
            separators.push(sep_x);
            cur_x += divider_gap;
        }
    }

    (Some(subbar_rect), items, separators)
}

pub fn compute_toolbar_layout(
    screen_w: f32,
    screen_h: f32,
    collapsed: bool,
    custom_pos: Option<Point2D>,
    monitor_count: usize,
) -> (D2D_RECT_F, Vec<ToolbarItemBounds>, D2D_RECT_F, Vec<f32>) {
    let grip_w = 12.0;
    if collapsed {
        let width = 140.0;
        let height = 34.0;
        let left = if let Some(pos) = custom_pos {
            pos.x.clamp(6.0, (screen_w - width - 6.0).max(6.0))
        } else {
            ((screen_w - width) / 2.0).max(10.0)
        };
        let top = if let Some(pos) = custom_pos {
            pos.y.clamp(6.0, (screen_h - height - 6.0).max(6.0))
        } else {
            12.0
        };
        let bar_rect = D2D_RECT_F {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let grip_rect = D2D_RECT_F {
            left: left + 4.0,
            top: top + 4.0,
            right: left + 4.0 + grip_w,
            bottom: top + height - 4.0,
        };
        let items = vec![ToolbarItemBounds {
            action: FluentAction::ToggleCollapse,
            rect: D2D_RECT_F {
                left: left + 4.0 + grip_w + 2.0,
                top: top + 4.0,
                right: left + width - 4.0,
                bottom: top + height - 4.0,
            },
        }];
        return (bar_rect, items, grip_rect, Vec::new());
    }

    let height = 44.0;
    let pad_x = 8.0;
    let btn_pad_y = 6.0;

    // Define items and their widths:
    let mut item_specs: Vec<(FluentAction, f32)> = Vec::with_capacity(32);

    // Modes (5 items, or 6 items if multi-monitor)
    item_specs.push((FluentAction::ModeZoom, 34.0));
    item_specs.push((FluentAction::ModeDraw, 34.0));
    item_specs.push((FluentAction::ModeSpotlight, 34.0));
    item_specs.push((FluentAction::ModeTimer, 34.0));
    item_specs.push((FluentAction::ModeSnip, 34.0));
    if monitor_count > 1 {
        item_specs.push((FluentAction::CycleDisplay, 34.0));
    }

    let mode_end_idx = if monitor_count > 1 { 5 } else { 4 };

    // Tools (10 items)
    let tools = [
        DrawTool::Pen,
        DrawTool::LaserPointer,
        DrawTool::Highlighter,
        DrawTool::Eraser,
        DrawTool::Arrow,
        DrawTool::Line,
        DrawTool::Rectangle,
        DrawTool::Ellipse,
        DrawTool::StepBadge,
        DrawTool::Text,
    ];
    for t in tools {
        item_specs.push((FluentAction::Tool(t), 32.0));
    }
    let tools_end_idx = mode_end_idx + 10;

    // Colors (8 items)
    let colors = [
        ColorPreset::Red,
        ColorPreset::Green,
        ColorPreset::Blue,
        ColorPreset::Yellow,
        ColorPreset::Orange,
        ColorPreset::Pink,
        ColorPreset::Cyan,
        ColorPreset::White,
    ];
    for c in colors {
        item_specs.push((FluentAction::Color(c), 22.0));
    }
    let colors_end_idx = tools_end_idx + 8;

    // Actions (6 items)
    item_specs.push((FluentAction::Undo, 30.0));
    item_specs.push((FluentAction::Clear, 30.0));
    item_specs.push((FluentAction::Copy, 30.0));
    item_specs.push((FluentAction::Save, 30.0));
    item_specs.push((FluentAction::Close, 30.0));
    item_specs.push((FluentAction::ToggleCollapse, 24.0));

    let spacing = 3.0;
    let divider_spacing = 9.0;

    // Calculate total bar width
    let mut total_w = pad_x * 2.0 + grip_w + 4.0;
    for (i, &(_, w)) in item_specs.iter().enumerate() {
        total_w += w;
        if i + 1 < item_specs.len() {
            if i == mode_end_idx || i == tools_end_idx || i == colors_end_idx {
                total_w += divider_spacing * 2.0 + 1.0;
            } else {
                total_w += spacing;
            }
        }
    }

    let left = if let Some(pos) = custom_pos {
        pos.x.clamp(6.0, (screen_w - total_w - 6.0).max(6.0))
    } else {
        ((screen_w - total_w) / 2.0).max(6.0)
    };
    let top = if let Some(pos) = custom_pos {
        pos.y.clamp(6.0, (screen_h - height - 6.0).max(6.0))
    } else {
        12.0
    };

    let bar_rect = D2D_RECT_F {
        left,
        top,
        right: left + total_w,
        bottom: top + height,
    };

    let grip_rect = D2D_RECT_F {
        left: left + 4.0,
        top: top + btn_pad_y,
        right: left + 4.0 + grip_w,
        bottom: top + height - btn_pad_y,
    };

    let mut items = Vec::with_capacity(item_specs.len());
    let mut separators = Vec::with_capacity(4);

    // Separator between grip handle and first mode button
    separators.push(grip_rect.right + pad_x / 2.0);

    let mut cur_x = left + 4.0 + grip_w + pad_x;
    for (i, &(action, w)) in item_specs.iter().enumerate() {
        let rect = D2D_RECT_F {
            left: cur_x,
            top: top + btn_pad_y,
            right: cur_x + w,
            bottom: top + height - btn_pad_y,
        };
        items.push(ToolbarItemBounds { action, rect });
        cur_x += w;

        if i + 1 < item_specs.len() {
            if i == mode_end_idx || i == tools_end_idx || i == colors_end_idx {
                let sep_x = cur_x + divider_spacing;
                separators.push(sep_x);
                cur_x += divider_spacing * 2.0 + 1.0;
            } else {
                cur_x += spacing;
            }
        }
    }

    (bar_rect, items, grip_rect, separators)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_distance() {
        let p1 = Point2D::new(0.0, 0.0);
        let p2 = Point2D::new(3.0, 4.0);
        assert_eq!(p1.distance(&p2), 5.0);
    }

    #[test]
    fn test_snip_rect_normalization() {
        let snip = SnipSelection {
            active: true,
            start: Point2D::new(200.0, 300.0),
            current: Point2D::new(100.0, 150.0),
            shape: SnipShape::Rectangle,
            with_guides: false,
        };
        let (l, t, r, b) = snip.rect();
        assert_eq!(l, 100.0);
        assert_eq!(t, 150.0);
        assert_eq!(r, 200.0);
        assert_eq!(b, 300.0);
    }

    #[test]
    fn test_color_presets() {
        let red = ColorPreset::Red.to_d2d_color(1.0);
        assert!(red.r > 0.8 && red.g < 0.3 && red.b < 0.3);

        let cyan = ColorPreset::Cyan.to_d2d_color(0.5);
        assert_eq!(cyan.a, 0.5);
        assert!(cyan.b > 0.8 && cyan.g > 0.7);
    }

    #[test]
    fn test_draw_tool_names() {
        assert_eq!(DrawTool::Pen.name(), "Pen");
        assert_eq!(DrawTool::StepBadge.name(), "Step Badge");
        assert_eq!(DrawTool::Highlighter.name(), "Highlighter");
    }

    #[test]
    fn test_zoom_state_coordinate_mapping() {
        let mut zoom = ZoomState::default();
        zoom.level = 2.0;
        zoom.target_level = 2.0;
        zoom.view_x = 100.0;
        zoom.view_y = 50.0;

        let screen_pt = Point2D::new(400.0, 200.0);
        let canvas_pt = zoom.screen_to_canvas(screen_pt);
        assert_eq!(canvas_pt.x, 300.0); // 100 + 400/2
        assert_eq!(canvas_pt.y, 150.0); // 50 + 200/2

        let roundtrip = zoom.canvas_to_screen(canvas_pt);
        assert_eq!(roundtrip.x, 400.0);
        assert_eq!(roundtrip.y, 200.0);
    }

    #[test]
    fn test_zoom_state_centering() {
        let mut zoom = ZoomState::default();
        let screen_w = 1920.0;
        let screen_h = 1080.0;
        let center = Point2D::new(960.0, 540.0);

        zoom.set_zoom_centered(2.0, center, screen_w, screen_h);
        assert_eq!(zoom.level, 2.0);
        assert_eq!(zoom.view_x, 480.0); // 960 - 960/2
        assert_eq!(zoom.view_y, 270.0); // 540 - 540/2
    }

    #[test]
    fn test_text_editor_operations() {
        let mut editor = TextEditorState::new(
            Point2D::new(0.0, 0.0),
            ColorPreset::Red,
            24.0,
            false,
            false,
            TextCardStyle::Transparent,
            TextFontFamily::SegoeUI,
        );
        editor.insert_str("Hello");
        assert_eq!(editor.text, "Hello");
        assert_eq!(editor.cursor, 5);

        editor.backspace();
        assert_eq!(editor.text, "Hell");
        assert_eq!(editor.cursor, 4);

        editor.move_left();
        assert_eq!(editor.cursor, 3);

        editor.insert_char('p');
        assert_eq!(editor.text, "Helpl");
        assert_eq!(editor.cursor, 4);

        editor.delete_forward();
        assert_eq!(editor.text, "Help");
        assert_eq!(editor.cursor, 4);
    }

    #[test]
    fn test_direct_cursor_pan() {
        let mut zoom = ZoomState::default();
        zoom.level = 2.0;
        let screen_w = 1920.0;
        let screen_h = 1080.0;

        zoom.update_target_from_cursor(960.0, 540.0, screen_w, screen_h);
        assert_eq!(zoom.view_x, 480.0);
        assert_eq!(zoom.view_y, 270.0);
        assert_eq!(zoom.target_view_x, 480.0);
        assert_eq!(zoom.target_view_y, 270.0);

        zoom.update_target_from_cursor(0.0, 0.0, screen_w, screen_h);
        assert_eq!(zoom.view_x, 0.0);
        assert_eq!(zoom.view_y, 0.0);
    }

    #[test]
    fn test_spotlight_defaults_and_resizing() {
        let mut spot = SpotlightState::default();
        assert!(!spot.active);
        assert_eq!(spot.radius, 180.0);
        assert_eq!(spot.dim_opacity, 0.92);

        // Test wheel resize logic
        let delta = 1.0; // scroll up
        spot.radius = (spot.radius + delta * 20.0).clamp(40.0, 800.0);
        assert_eq!(spot.radius, 200.0);

        // Test lower clamp
        spot.radius = (spot.radius - 20.0 * 20.0).clamp(40.0, 800.0);
        assert_eq!(spot.radius, 40.0);

        // Test upper clamp
        spot.radius = (spot.radius + 50.0 * 20.0).clamp(40.0, 800.0);
        assert_eq!(spot.radius, 800.0);
    }

    #[test]
    fn test_zoom_wheel_delta() {
        let mut zoom = ZoomState::default();
        assert_eq!(zoom.level, 1.0);

        // Scroll up delta
        let delta = 1.0;
        zoom.level = (zoom.level + delta * 0.25).clamp(1.0, 10.0);
        assert_eq!(zoom.level, 1.25);

        // Scroll down clamp
        zoom.level = (zoom.level - 5.0 * 0.25).clamp(1.0, 10.0);
        assert_eq!(zoom.level, 1.0);
    }

    #[test]
    fn test_fluent_toolbar_layout_and_hit_test() {
        let screen_w = 1920.0;
        let screen_h = 1080.0;
        let mut tb = FluentToolbarState::default();
        tb.update_layout(screen_w, screen_h);

        assert!(tb.visible);
        assert!(!tb.collapsed);
        assert!(tb.bar_rect.right > tb.bar_rect.left);
        assert_eq!(tb.bar_rect.top, 12.0);
        assert!(!tb.items.is_empty());

        // Test grip handle hit test
        let grip_mid_x = (tb.grip_rect.left + tb.grip_rect.right) / 2.0;
        let grip_mid_y = (tb.grip_rect.top + tb.grip_rect.bottom) / 2.0;
        assert!(tb.hit_test_grip(grip_mid_x, grip_mid_y));
        assert!(!tb.hit_test_grip(grip_mid_x + 200.0, grip_mid_y));

        // Test first item hit test (ModeZoom)
        let first = &tb.items[0];
        assert_eq!(first.action, FluentAction::ModeZoom);
        let mid_x = (first.rect.left + first.rect.right) / 2.0;
        let mid_y = (first.rect.top + first.rect.bottom) / 2.0;
        assert_eq!(tb.hit_test(mid_x, mid_y), Some(FluentAction::ModeZoom));

        // Outside toolbar
        assert_eq!(tb.hit_test(10.0, 500.0), None);
        assert!(!tb.is_point_inside(10.0, 500.0));
        assert!(tb.is_point_inside(mid_x, mid_y));

        // Test dragging to custom position
        tb.custom_position = Some(Point2D::new(400.0, 800.0));
        tb.update_layout(screen_w, screen_h);
        assert_eq!(tb.bar_rect.left, 400.0);
        assert_eq!(tb.bar_rect.top, 800.0);

        // Test collapsed state
        tb.collapsed = true;
        tb.update_layout(screen_w, screen_h);
        assert_eq!(tb.items.len(), 1);
        assert_eq!(tb.items[0].action, FluentAction::ToggleCollapse);
    }

    #[test]
    fn test_fluent_toolbar_multi_monitor_cycle_display() {
        let screen_w = 1920.0;
        let screen_h = 1080.0;
        let mut tb = FluentToolbarState::default();
        tb.monitor_count = 2;
        tb.update_layout(screen_w, screen_h);

        // Verify CycleDisplay is present in items
        let cycle_display_item = tb.items.iter().find(|it| it.action == FluentAction::CycleDisplay);
        assert!(cycle_display_item.is_some());
        let item = cycle_display_item.unwrap();
        let mid_x = (item.rect.left + item.rect.right) / 2.0;
        let mid_y = (item.rect.top + item.rect.bottom) / 2.0;
        assert_eq!(tb.hit_test(mid_x, mid_y), Some(FluentAction::CycleDisplay));
    }

    #[test]
    fn test_timer_widget_hit_test() {
        let screen_w = 1920.0;
        let screen_h = 1080.0;
        let mut widget = TimerWidgetState::default();
        let (_card_left, card_top, card_right, _card_bottom, cx, cy) = widget.get_card_bounds(screen_w, screen_h);

        // 1. Full center card mode
        // Clock face center click toggles PlayPause
        let clock_center = Point2D::new(cx, cy - 10.0);
        assert_eq!(widget.get_action_at(clock_center, screen_w, screen_h), Some(TimerAction::PlayPause));

        // Quick duration pill (5m)
        let pill_5m = Point2D::new(cx - 150.0, cy - 150.0);
        assert_eq!(widget.get_action_at(pill_5m, screen_w, screen_h), Some(TimerAction::SetDuration(5)));

        // Far away click hits nothing
        let outside = Point2D::new(50.0, 50.0);
        assert_eq!(widget.get_action_at(outside, screen_w, screen_h), None);

        // Buttons row
        let btn_y = cy + 130.0;

        // Hero Play/Pause button (center)
        let hero_pt = Point2D::new(cx, btn_y);
        assert_eq!(widget.get_action_at(hero_pt, screen_w, screen_h), Some(TimerAction::PlayPause));

        // +1m button (cx + 60.0)
        let plus1_pt = Point2D::new(cx + 60.0, btn_y);
        assert_eq!(widget.get_action_at(plus1_pt, screen_w, screen_h), Some(TimerAction::AddMinute));

        // -1m button (cx - 120.0)
        let minus1_pt = Point2D::new(cx - 120.0, btn_y);
        assert_eq!(widget.get_action_at(minus1_pt, screen_w, screen_h), Some(TimerAction::SubMinute));

        // Reset button (cx - 60.0)
        let reset_pt = Point2D::new(cx - 60.0, btn_y);
        assert_eq!(widget.get_action_at(reset_pt, screen_w, screen_h), Some(TimerAction::Reset));

        // Mini button (cx + 120.0)
        let mini_pt = Point2D::new(cx + 120.0, btn_y);
        assert_eq!(widget.get_action_at(mini_pt, screen_w, screen_h), Some(TimerAction::ToggleMinimize));

        // Close button (top right: card_right - 32.0, card_top + 28.0)
        let close_pt = Point2D::new(card_right - 32.0, card_top + 28.0);
        assert_eq!(widget.get_action_at(close_pt, screen_w, screen_h), Some(TimerAction::Close));

        // 2. Corner mini-pill mode
        widget.minimized = true;
        let (left, top, right, _bottom) = widget.get_pill_rect(screen_w, screen_h);

        // Cycle corner button in mini-pill (left + 15, top + 15)
        let mini_cycle = Point2D::new(left + 15.0, top + 15.0);
        assert_eq!(widget.get_action_at(mini_cycle, screen_w, screen_h), Some(TimerAction::CycleCorner));

        // Play/Pause button in mini-pill (right - 90, top + 15)
        let mini_play = Point2D::new(right - 90.0, top + 15.0);
        assert_eq!(widget.get_action_at(mini_play, screen_w, screen_h), Some(TimerAction::PlayPause));

        // Expand button in mini-pill (right - 50, top + 15)
        let mini_expand = Point2D::new(right - 50.0, top + 15.0);
        assert_eq!(widget.get_action_at(mini_expand, screen_w, screen_h), Some(TimerAction::ToggleMinimize));

        // Close button in mini-pill (right - 20, top + 15)
        let mini_close = Point2D::new(right - 20.0, top + 15.0);
        assert_eq!(widget.get_action_at(mini_close, screen_w, screen_h), Some(TimerAction::Close));
    }

    #[test]
    fn test_snip_shape_and_circle_mask() {
        let snip = SnipSelection {
            active: true,
            start: Point2D::new(100.0, 100.0),
            current: Point2D::new(300.0, 300.0),
            shape: SnipShape::Ellipse,
            with_guides: false,
        };
        assert_eq!(snip.shape, SnipShape::Ellipse);
        let (l, t, r, b) = snip.rect();
        assert_eq!((l, t, r, b), (100.0, 100.0, 300.0, 300.0));

        let w = (r - l) as u32;
        let h = (b - t) as u32;
        let cx = w as f32 / 2.0;
        let cy = h as f32 / 2.0;
        let rx = cx;
        let ry = cy;

        // Center pixel is inside ellipse
        let d_center = ((0.0) / (rx * rx)) + ((0.0) / (ry * ry));
        assert!(d_center <= 1.0);

        // Corner pixel (0, 0) is strictly outside circle
        let dx_corner = 0.0 - cx;
        let dy_corner = 0.0 - cy;
        let d_corner = (dx_corner * dx_corner) / (rx * rx) + (dy_corner * dy_corner) / (ry * ry);
        assert!(d_corner > 1.0);
    }

    #[test]
    fn test_drawing_attributes_and_enums() {
        assert_eq!(FillMode::None.name(), "Outline Only");
        assert_eq!(FillMode::Tinted.name(), "Tinted Fill");
        assert_eq!(FillMode::Solid.name(), "Solid Fill");

        assert_eq!(StrokePattern::Solid.name(), "Solid");
        assert_eq!(StrokePattern::Dashed.name(), "Dashed");
        assert_eq!(StrokePattern::Dotted.name(), "Dotted");

        assert_eq!(ArrowStyle::Single.name(), "Single Arrow");
        assert_eq!(ArrowStyle::Double.name(), "Double Arrow");
        assert_eq!(ArrowStyle::Dimension.name(), "Dimension Line");

        assert_eq!(BadgeSize::Small.radius(), 14.0);
        assert_eq!(BadgeSize::Medium.radius(), 18.0);
        assert_eq!(BadgeSize::Large.radius(), 24.0);
        assert_eq!(BadgeSize::ExtraLarge.radius(), 30.0);

        assert_eq!(BadgeShape::Circle.name(), "Circle");
        assert_eq!(BadgeShape::Square.name(), "Square");
        assert_eq!(BadgeShape::Hexagon.name(), "Hexagon");
    }

    #[test]
    fn test_dynamic_subbar_layout_and_hit_testing() {
        let mut tb = FluentToolbarState::default();
        assert!(tb.visible);

        // 0. Default state: active_tool is None -> no subbar
        assert_eq!(tb.active_tool, None);
        tb.update_layout(1920.0, 1080.0);
        assert!(tb.subbar_rect.is_none());
        assert!(tb.subbar_items.is_empty());

        // 1. Rectangle tool selected -> subbar should contain widths, fills, patterns
        tb.active_tool = Some(DrawTool::Rectangle);
        tb.update_layout(1920.0, 1080.0);
        assert!(tb.subbar_rect.is_some());
        let items_len = tb.subbar_items.len();
        assert!(items_len >= 10); // 4 widths + 3 fills + 3 patterns = 10 items

        let sb = tb.subbar_rect.unwrap();
        // Point in subbar should be inside toolbar
        assert!(tb.is_point_inside(sb.left + 15.0, sb.top + 15.0));

        // 2. Arrow tool selected -> subbar should contain arrow styles
        tb.active_tool = Some(DrawTool::Arrow);
        tb.update_layout(1920.0, 1080.0);
        assert!(tb.subbar_items.iter().any(|i| i.action == FluentAction::SetArrowStyle(ArrowStyle::Double)));

        // 3. StepBadge tool selected -> subbar should contain sizes, shapes, fills, widths, reset
        tb.active_tool = Some(DrawTool::StepBadge);
        tb.update_layout(1920.0, 1080.0);
        assert!(tb.subbar_items.iter().any(|i| i.action == FluentAction::ResetBadgeCounter));
        assert!(tb.subbar_items.iter().any(|i| i.action == FluentAction::SetBadgeSize(BadgeSize::Large)));
        assert!(tb.subbar_items.iter().any(|i| i.action == FluentAction::SetBadgeShape(BadgeShape::Hexagon)));
        assert!(tb.subbar_items.iter().any(|i| i.action == FluentAction::SetFillMode(FillMode::Solid)));
        assert!(tb.subbar_items.iter().any(|i| i.action == FluentAction::SetStrokeWidth(4.0)));

        // 4. Text tool selected -> subbar should contain font sizes, bold, italic, card style, fonts
        tb.active_tool = Some(DrawTool::Text);
        tb.update_layout(1920.0, 1080.0);
        assert!(tb.subbar_rect.is_some());
        assert!(tb.subbar_items.iter().any(|i| i.action == FluentAction::SetFontSize(20.0)));
        assert!(tb.subbar_items.iter().any(|i| i.action == FluentAction::ToggleBold));
        assert!(tb.subbar_items.iter().any(|i| i.action == FluentAction::SetTextCardStyle(TextCardStyle::Badge)));
        assert!(tb.subbar_items.iter().any(|i| i.action == FluentAction::SetFontFamily(TextFontFamily::CascadiaCode)));

        // 5. Deselect tool (None) -> subbar disappears completely
        tb.active_tool = None;
        tb.update_layout(1920.0, 1080.0);
        assert!(tb.subbar_rect.is_none());
        assert!(tb.subbar_items.is_empty());
    }

    #[test]
    fn test_timer_corner_positions_and_custom_drag() {
        let screen_w = 1920.0;
        let screen_h = 1080.0;
        let mut widget = TimerWidgetState::default();

        // Check custom dragging position
        widget.custom_pos = Some(Point2D::new(400.0, 300.0));
        let (cl, ct, cr, cb, cx, cy) = widget.get_card_bounds(screen_w, screen_h);
        assert_eq!(cx, 400.0);
        assert_eq!(cy, 300.0);
        assert_eq!(cr - cl, 580.0);
        assert_eq!(cb - ct, 450.0);

        // Check 4-corner pill docking
        widget.minimized = true;

        // Corner 0: Top-Right
        widget.pill_corner = 0;
        let (_l0, t0, r0, _b0) = widget.get_pill_rect(screen_w, screen_h);
        assert_eq!(r0, screen_w - 24.0);
        assert_eq!(t0, 24.0);

        // Corner 1: Top-Left
        widget.pill_corner = 1;
        let (l1, t1, _r1, _b1) = widget.get_pill_rect(screen_w, screen_h);
        assert_eq!(l1, 24.0);
        assert_eq!(t1, 24.0);

        // Corner 2: Bottom-Left
        widget.pill_corner = 2;
        let (l2, _t2, _r2, b2) = widget.get_pill_rect(screen_w, screen_h);
        assert_eq!(l2, 24.0);
        assert_eq!(b2, screen_h - 24.0);

        // Corner 3: Bottom-Right
        widget.pill_corner = 3;
        let (_l3, _t3, r3, b3) = widget.get_pill_rect(screen_w, screen_h);
        assert_eq!(r3, screen_w - 24.0);
        assert_eq!(b3, screen_h - 24.0);
    }

    #[test]
    fn test_per_tool_settings_independence() {
        // Verify tool settings are independent structs and can be mutated without cross-contamination
        let pen = StrokeToolSettings { stroke_width: 2.0, pattern: StrokePattern::Solid };
        let mut highlighter = StrokeToolSettings { stroke_width: 14.0, pattern: StrokePattern::Solid };
        let mut arrow = ArrowToolSettings { stroke_width: 4.0, style: ArrowStyle::Single, pattern: StrokePattern::Solid };
        let mut rect = ShapeToolSettings { stroke_width: 3.0, fill_mode: FillMode::None, pattern: StrokePattern::Solid };
        let badge = StepBadgeToolSettings { size: BadgeSize::Medium, shape: BadgeShape::Circle, fill: FillMode::Solid, stroke_width: 2.0 };
        let mut text = TextToolSettings { font_size: 20.0, is_bold: false, is_italic: false, card_style: TextCardStyle::Transparent, font_family: TextFontFamily::SegoeUI };

        // Mutate highlighter stroke width
        highlighter.stroke_width = 24.0;
        assert_eq!(highlighter.stroke_width, 24.0);
        assert_eq!(pen.stroke_width, 2.0); // Pen remains untouched!
        assert_eq!(badge.stroke_width, 2.0); // Badge remains untouched!
        assert_eq!(rect.stroke_width, 3.0); // Rect remains untouched!

        // Mutate arrow style and pattern
        arrow.style = ArrowStyle::Double;
        arrow.pattern = StrokePattern::Dashed;
        assert_eq!(arrow.style, ArrowStyle::Double);
        assert_eq!(arrow.pattern, StrokePattern::Dashed);
        assert_eq!(rect.pattern, StrokePattern::Solid); // Rect pattern is Solid!
        assert_eq!(pen.pattern, StrokePattern::Solid); // Pen pattern is Solid!

        // Mutate rect fill
        rect.fill_mode = FillMode::Tinted;
        assert_eq!(rect.fill_mode, FillMode::Tinted);
        assert_eq!(badge.fill, FillMode::Solid); // Badge fill is still Solid!

        // Mutate text font size
        text.font_size = 32.0;
        text.is_bold = true;
        assert_eq!(text.font_size, 32.0);
        assert!(text.is_bold);
        assert_eq!(badge.size, BadgeSize::Medium);
    }

    #[test]
    fn test_snip_with_guides() {
        let mut snip = SnipSelection::default();
        assert!(!snip.active);
        assert!(!snip.with_guides);

        snip.active = true;
        snip.start = Point2D::new(100.0, 100.0);
        snip.current = Point2D::new(300.0, 200.0);
        snip.with_guides = true;

        let (l, t, r, b) = snip.rect();
        assert_eq!((l, t, r, b), (100.0, 100.0, 300.0, 200.0));
        assert!(snip.with_guides);
    }
}

