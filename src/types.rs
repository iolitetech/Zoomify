#![allow(dead_code)]

use std::time::Instant;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

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

#[derive(Debug, Clone)]
pub enum Shape {
    Stroke {
        points: Vec<Point2D>,
        color: ColorPreset,
        width: f32,
        is_highlighter: bool,
    },
    Line {
        start: Point2D,
        end: Point2D,
        color: ColorPreset,
        width: f32,
    },
    Arrow {
        start: Point2D,
        end: Point2D,
        color: ColorPreset,
        width: f32,
    },
    Rectangle {
        start: Point2D,
        end: Point2D,
        color: ColorPreset,
        width: f32,
        rounded: bool,
    },
    Ellipse {
        start: Point2D,
        end: Point2D,
        color: ColorPreset,
        width: f32,
    },
    Text {
        origin: Point2D,
        text: String,
        font_size: f32,
        color: ColorPreset,
    },
    StepBadge {
        center: Point2D,
        number: u32,
        radius: f32,
        color: ColorPreset,
    },
}

#[derive(Debug, Clone)]
pub enum HistoryAction {
    AddShape(Shape),
    Clear(Vec<Shape>),
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
            radius: 160.0,
            pinned: false,
            dim_opacity: 0.65,
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
        let z = self.target_level.max(1.0);
        if z <= 1.001 || screen_w <= 0.0 || screen_h <= 0.0 {
            self.target_view_x = 0.0;
            self.target_view_y = 0.0;
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

#[derive(Debug, Clone)]
pub struct SnipSelection {
    pub active: bool,
    pub start: Point2D,
    pub current: Point2D,
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
}
