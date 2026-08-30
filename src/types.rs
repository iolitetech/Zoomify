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
    pub center_x: f32,
    pub center_y: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub is_dragging: bool,
    pub drag_start_mouse: Point2D,
    pub drag_start_offset: Point2D,
}

impl Default for ZoomState {
    fn default() -> Self {
        Self {
            level: 2.0,
            center_x: 0.0,
            center_y: 0.0,
            offset_x: 0.0,
            offset_y: 0.0,
            is_dragging: false,
            drag_start_mouse: Point2D::default(),
            drag_start_offset: Point2D::default(),
        }
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
}
