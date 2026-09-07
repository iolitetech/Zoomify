#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
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
    Loupe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawTool {
    /// Pick an existing annotation to move, resize or delete it.
    Select,
    /// A filled card that opens straight into its own label.
    StickyNote,
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
    Blur,
}

impl DrawTool {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::StickyNote => "Sticky Note",
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
            Self::Blur => "Redact (Blur)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Arbitrary sRGB chosen from the in-overlay picker.
    Custom(u8, u8, u8),
}

impl ColorPreset {
    /// Black or white, whichever will read against this colour.
    ///
    /// A sticky note is a solid block of the current colour, and a label in
    /// that same colour would be invisible on it.
    pub fn contrasting_ink(self) -> Self {
        let (r, g, b) = self.rgb_f32();
        // Rec. 709 luma: green dominates perceived brightness.
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        if luma > 0.55 { Self::Black } else { Self::White }
    }

    /// Straight sRGB components in 0..=1.
    pub fn rgb_f32(self) -> (f32, f32, f32) {
        match self {
            Self::Red => (0.95, 0.15, 0.15),
            Self::Green => (0.15, 0.85, 0.25),
            Self::Blue => (0.15, 0.55, 0.98),
            Self::Yellow => (1.0, 0.88, 0.1),
            Self::Orange => (1.0, 0.55, 0.05),
            Self::Pink => (0.98, 0.25, 0.65),
            Self::Cyan => (0.05, 0.88, 0.95),
            Self::White => (0.98, 0.98, 0.98),
            Self::Black => (0.10, 0.10, 0.12),
            Self::Custom(r, g, b) => {
                (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
            }
        }
    }

    pub fn to_d2d_color(self, alpha: f32) -> D2D1_COLOR_F {
        let (r, g, b) = self.rgb_f32();
        D2D1_COLOR_F { r, g, b, a: alpha }
    }

    /// 8-bit sRGB triple, the form the picker and config round-trip through.
    pub fn rgb_u8(self) -> (u8, u8, u8) {
        let (r, g, b) = self.rgb_f32();
        (
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8,
        )
    }

    pub fn name(&self) -> String {
        match self {
            Self::Red => "Red".to_string(),
            Self::Green => "Green".to_string(),
            Self::Blue => "Blue".to_string(),
            Self::Yellow => "Yellow".to_string(),
            Self::Orange => "Orange".to_string(),
            Self::Pink => "Pink".to_string(),
            Self::Cyan => "Cyan".to_string(),
            Self::White => "White".to_string(),
            Self::Black => "Black".to_string(),
            Self::Custom(r, g, b) => format!("#{:02X}{:02X}{:02X}", r, g, b),
        }
    }

    /// Parse the config form: a preset name, or `#RRGGBB` / `RRGGBB`.
    pub fn from_config_str(s: &str) -> Option<Self> {
        let t = s.trim();
        match t.to_ascii_lowercase().as_str() {
            "red" => return Some(Self::Red),
            "green" => return Some(Self::Green),
            "blue" => return Some(Self::Blue),
            "yellow" => return Some(Self::Yellow),
            "orange" => return Some(Self::Orange),
            "pink" => return Some(Self::Pink),
            "cyan" => return Some(Self::Cyan),
            "white" => return Some(Self::White),
            "black" => return Some(Self::Black),
            _ => {}
        }
        let hex = t.strip_prefix('#').unwrap_or(t);
        if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            let v = u32::from_str_radix(hex, 16).ok()?;
            return Some(Self::Custom(
                ((v >> 16) & 0xFF) as u8,
                ((v >> 8) & 0xFF) as u8,
                (v & 0xFF) as u8,
            ));
        }
        None
    }
}

/// Convert HSV (h in degrees 0..360, s/v in 0..=1) to 8-bit sRGB.
/// The picker works in HSV because that is what the hue/sat/value bars expose.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0);
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);

    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r1, g1, b1) = match h as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    (
        ((r1 + m) * 255.0).round() as u8,
        ((g1 + m) * 255.0).round() as u8,
        ((b1 + m) * 255.0).round() as u8,
    )
}

/// Inverse of `hsv_to_rgb`, used to seed the picker from the current color.
pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;

    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let d = max - min;

    let h = if d <= f32::EPSILON {
        0.0
    } else if max == rf {
        60.0 * (((gf - bf) / d) % 6.0)
    } else if max == gf {
        60.0 * (((bf - rf) / d) + 2.0)
    } else {
        60.0 * (((rf - gf) / d) + 4.0)
    };

    let s = if max <= f32::EPSILON { 0.0 } else { d / max };
    (h.rem_euclid(360.0), s, max)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasBackground {
    Transparent,
    Whiteboard,
    Blackboard,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// Which edge of a group everything lines up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignTo {
    Left,
    HCentre,
    Right,
    Top,
    VCentre,
    Bottom,
}

/// The shape of an arrow's head, independent of `ArrowStyle`, which says
/// which *ends* carry one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ArrowHead {
    #[default]
    Triangle,
    /// Two strokes meeting at the tip, leaving the point open.
    Open,
    Circle,
    Diamond,
    /// A perpendicular bar, for a plain terminator.
    Bar,
}

impl ArrowHead {
    pub fn next(self) -> Self {
        match self {
            Self::Triangle => Self::Open,
            Self::Open => Self::Circle,
            Self::Circle => Self::Diamond,
            Self::Diamond => Self::Bar,
            Self::Bar => Self::Triangle,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Triangle => "Solid",
            Self::Open => "Open",
            Self::Circle => "Circle",
            Self::Diamond => "Diamond",
            Self::Bar => "Bar",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// Stable identity for an annotation.
///
/// Relationships between annotations — a label inside a box, an arrow anchored
/// to one — have to survive deletion, undo and reordering. A position in a
/// `Vec` survives none of those: delete one shape and every later index shifts,
/// silently re-pointing anything that referred to them.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct ShapeId(pub u64);

static NEXT_SHAPE_ID: AtomicU64 = AtomicU64::new(1);

impl ShapeId {
    pub fn fresh() -> Self {
        Self(NEXT_SHAPE_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Push the counter past everything in a loaded session, so newly drawn
    /// annotations cannot collide with the ids that came off disk.
    pub fn reserve_above(highest: ShapeId) {
        let want = highest.0 + 1;
        let mut current = NEXT_SHAPE_ID.load(Ordering::Relaxed);
        while current < want {
            match NEXT_SHAPE_ID.compare_exchange_weak(
                current,
                want,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }
}

/// A shape plus everything about it that is not geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: ShapeId,
    pub shape: Shape,
    /// For a `Text`: the annotation it is centred inside.
    ///
    /// While this is set the text's own `origin` is unused — its layout is
    /// derived from the container's bounds every frame, which is exactly what
    /// makes it follow the container through moves and resizes for free.
    #[serde(default)]
    pub container: Option<ShapeId>,
    /// For a `Line` or `Arrow`: what each end is anchored to, if anything.
    ///
    /// The stored endpoints are kept in step with these by a settle pass after
    /// any mutation, so everything else — hit-testing, bounds, snapping,
    /// export — keeps reading plain geometry and needs no idea bindings exist.
    #[serde(default)]
    pub start_bound: Option<ShapeId>,
    #[serde(default)]
    pub end_bound: Option<ShapeId>,
    /// Which group this belongs to, if any. Members are selected together.
    ///
    /// Reuses ShapeId for its ids, so a group can never collide with an
    /// annotation or with another group.
    #[serde(default)]
    pub group: Option<ShapeId>,
    /// 0..=1, multiplied into every colour this annotation draws with.
    ///
    /// It sits here rather than on each `Shape` variant because it applies to
    /// all of them equally and none of them care what it is.
    #[serde(default = "full_opacity")]
    pub opacity: f32,
}

fn full_opacity() -> f32 {
    1.0
}

impl Annotation {
    pub fn new(shape: Shape) -> Self {
        Self {
            id: ShapeId::fresh(),
            shape,
            container: None,
            start_bound: None,
            end_bound: None,
            group: None,
            opacity: 1.0,
        }
    }

    pub fn is_contained_text(&self) -> bool {
        self.container.is_some() && matches!(self.shape, Shape::Text { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    Stroke {
        points: Vec<Point2D>,
        color: ColorPreset,
        width: f32,
        is_highlighter: bool,
        pattern: StrokePattern,
        /// Per-point pen pressure in 0..=1, parallel to `points`. Empty when
        /// the stroke came from a mouse or a pen with no pressure axis, in
        /// which case `width` applies uniformly.
        #[serde(default)]
        pressures: Vec<f32>,
    },
    Line {
        start: Point2D,
        end: Point2D,
        color: ColorPreset,
        width: f32,
        pattern: StrokePattern,
        /// How far the middle bows off the straight chord, in canvas units.
        /// Zero is a straight line; the sign picks which side it bows to.
        #[serde(default)]
        curve: f32,
    },
    Arrow {
        start: Point2D,
        end: Point2D,
        color: ColorPreset,
        width: f32,
        style: ArrowStyle,
        pattern: StrokePattern,
        #[serde(default)]
        head: ArrowHead,
        /// How far the middle bows off the straight chord, in canvas units.
        /// Zero is a straight arrow; the sign picks which side it bows to.
        #[serde(default)]
        curve: f32,
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
    Blur {
        start: Point2D,
        end: Point2D,
        block_size: f32,
    },
    Image {
        start: Point2D,
        end: Point2D,
        /// Top-down BGRA, one byte per channel.
        pixels: ImagePixels,
    },
}

/// Raw image bytes with their dimensions.
///
/// Sessions are JSON, so the bytes go out as base64 rather than an array of
/// numbers — a 1920x1080 paste would otherwise serialise to tens of megabytes
/// of decimal digits.
#[derive(Debug, Clone, PartialEq)]
pub struct ImagePixels {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl ImagePixels {
    /// A stable key for the renderer's bitmap cache.
    ///
    /// Content-derived, so the same image pasted twice shares one GPU bitmap
    /// and a session reload keys to the same entry it did before.
    pub fn cache_key(&self) -> u64 {
        // FNV-1a over the dimensions and a sample of the bytes. Hashing every
        // byte of a large paste on each frame would cost more than it saves.
        let mut h: u64 = 0xcbf29ce484222325;
        let mut eat = |b: u8| {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        };
        for b in self.width.to_le_bytes() {
            eat(b);
        }
        for b in self.height.to_le_bytes() {
            eat(b);
        }
        for b in (self.bgra.len() as u64).to_le_bytes() {
            eat(b);
        }
        let step = (self.bgra.len() / 4096).max(1);
        for i in (0..self.bgra.len()).step_by(step) {
            eat(self.bgra[i]);
        }
        h
    }
}

/// Minimal base64, so image bytes survive a JSON round trip.
///
/// `pub(crate)` rather than private: the SVG/PDF exporters reuse `encode` to
/// embed raster data (pasted images, mosaic blur patches) without a second
/// implementation.
pub(crate) mod b64 {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            out.push(ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(ALPHABET[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                ALPHABET[(n >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                ALPHABET[n as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }

    pub fn decode(s: &str) -> Option<Vec<u8>> {
        let mut lookup = [255u8; 256];
        for (i, c) in ALPHABET.iter().enumerate() {
            lookup[*c as usize] = i as u8;
        }
        let cleaned: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        if cleaned.len() % 4 != 0 {
            return None;
        }
        let mut out = Vec::with_capacity(cleaned.len() / 4 * 3);
        for chunk in cleaned.chunks(4) {
            let pad = chunk.iter().filter(|b| **b == b'=').count();
            let mut n: u32 = 0;
            for b in chunk {
                let v = if *b == b'=' { 0 } else { lookup[*b as usize] };
                if v == 255 {
                    return None;
                }
                n = (n << 6) | v as u32;
            }
            out.push((n >> 16) as u8);
            if pad < 2 {
                out.push((n >> 8) as u8);
            }
            if pad < 1 {
                out.push(n as u8);
            }
        }
        Some(out)
    }
}

impl Serialize for ImagePixels {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("ImagePixels", 3)?;
        st.serialize_field("width", &self.width)?;
        st.serialize_field("height", &self.height)?;
        st.serialize_field("bgra_base64", &b64::encode(&self.bgra))?;
        st.end()
    }
}

impl<'de> Deserialize<'de> for ImagePixels {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            width: u32,
            height: u32,
            bgra_base64: String,
        }
        let raw = Raw::deserialize(d)?;
        let bgra = b64::decode(&raw.bgra_base64)
            .ok_or_else(|| serde::de::Error::custom("image bytes are not valid base64"))?;
        let want = raw.width as usize * raw.height as usize * 4;
        if bgra.len() != want {
            return Err(serde::de::Error::custom(format!(
                "image is {} bytes, but {}x{} needs {}",
                bgra.len(),
                raw.width,
                raw.height,
                want
            )));
        }
        Ok(Self {
            width: raw.width,
            height: raw.height,
            bgra,
        })
    }
}

#[derive(Debug, Clone)]
pub struct TextEditorState {
    pub origin: Point2D,
    /// The shape this label is being typed into, if any. While it is set the
    /// editor is laid out from the container's bounds rather than `origin`.
    pub container: Option<ShapeId>,
    /// Bounds of that container, refreshed as it is edited.
    pub container_bounds: Option<(f32, f32, f32, f32)>,
    /// False for a label riding on a line, which has no width to wrap to.
    pub container_wraps: bool,
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
            container: None,
            container_bounds: None,
            container_wraps: true,
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

    /// Padding between a label and the edge of the shape holding it.
    pub const CONTAINER_PADDING: f32 = 10.0;

    /// Width the editor wraps at: the container's inner width, or unbounded
    /// for free-floating text, which only breaks where the author does.
    pub fn wrap_width(&self) -> f32 {
        match self.container_bounds {
            Some((l, _, r, _)) if self.container_wraps => {
                (r - l - Self::CONTAINER_PADDING * 2.0).max(24.0)
            }
            _ => f32::MAX,
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

    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    /// Byte range of the line containing `at`, excluding the terminator.
    fn line_bounds(&self, at: usize) -> (usize, usize) {
        let at = at.min(self.text.len());
        let start = self.text[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let end = self.text[at..]
            .find('\n')
            .map(|i| at + i)
            .unwrap_or(self.text.len());
        (start, end)
    }

    /// Byte index `col` characters into the line starting at `start`, clamped to
    /// `end`. Columns are counted in characters so multi-byte text lands right.
    fn byte_at_col(&self, start: usize, end: usize, col: usize) -> usize {
        let mut idx = start;
        let mut seen = 0;
        for ch in self.text[start..end].chars() {
            if seen == col {
                return idx;
            }
            idx += ch.len_utf8();
            seen += 1;
        }
        end
    }

    /// Character offset of the caret within its line.
    fn current_col(&self) -> usize {
        let (start, _) = self.line_bounds(self.cursor);
        self.text[start..self.cursor].chars().count()
    }

    pub fn move_up(&mut self) {
        let (start, _) = self.line_bounds(self.cursor);
        if start == 0 {
            self.cursor = 0;
            return;
        }
        let col = self.current_col();
        let prev_nl = start - 1; // the newline that ended the previous line
        let (prev_start, _) = self.line_bounds(prev_nl);
        self.cursor = self.byte_at_col(prev_start, prev_nl, col);
    }

    pub fn move_down(&mut self) {
        let (_, end) = self.line_bounds(self.cursor);
        if end >= self.text.len() {
            self.cursor = self.text.len();
            return;
        }
        let col = self.current_col();
        let next_start = end + 1; // skip the newline
        let (_, next_end) = self.line_bounds(next_start);
        self.cursor = self.byte_at_col(next_start, next_end, col);
    }

    pub fn move_line_start(&mut self) {
        let (start, _) = self.line_bounds(self.cursor);
        self.cursor = start;
    }

    pub fn move_line_end(&mut self) {
        let (_, end) = self.line_bounds(self.cursor);
        self.cursor = end;
    }

    pub fn line_count(&self) -> usize {
        self.text.split('\n').count().max(1)
    }
}

/// Rough extent of a block of annotation text, in DIPs, with no padding.
///
/// Width tracks the longest line rather than the total character count, so a
/// paragraph no longer produces an ever-widening single-line box. Callers add
/// their own padding; the renderer prefers exact DirectWrite metrics and only
/// falls back to this.
pub fn measure_text_block(text: &str, font_size: f32) -> (f32, f32) {
    let mut longest = 0usize;
    let mut lines = 0usize;
    for line in text.split('\n') {
        longest = longest.max(line.chars().count());
        lines += 1;
    }
    let lines = lines.max(1);
    (
        longest as f32 * font_size * 0.6,
        lines as f32 * font_size * 1.25,
    )
}

#[derive(Debug, Clone)]
pub enum HistoryAction {
    AddShape(Annotation),
    AddStepBadge {
        shape: Annotation,
        prev_counter: u32,
    },
    DeleteShape {
        /// Where to put it back; the annotation carries its own identity, so
        /// anything anchored to it survives the round trip.
        index: usize,
        shape: Annotation,
    },
    /// Several annotations removed together — a container and its label — so
    /// one undo brings the whole thing back.
    DeleteShapes {
        /// Ascending by index, which is the order they go back in.
        items: Vec<(usize, Annotation)>,
    },
    /// A z-order change: the annotation moved from one slot to another.
    Reorder {
        id: ShapeId,
        from: usize,
        to: usize,
    },
    /// Grouping is not part of the shape either.
    SetGroup {
        items: Vec<(ShapeId, Option<ShapeId>, Option<ShapeId>)>,
    },
    /// Opacity is not part of the shape, so it gets its own entry rather than
    /// riding on TransformShapes.
    SetOpacity {
        items: Vec<(ShapeId, f32, f32)>,
    },
    Clear(Vec<Annotation>),
    /// A move or resize applied to already-committed shapes. Addressed by id
    /// rather than position, which shifts under them. Several at once so
    /// dragging a group is one undo step, not one per shape.
    TransformShapes {
        items: Vec<(ShapeId, Shape, Shape)>,
    },
}

/// One page of annotations that can be tabbed to independently of the
/// others — see `OverlayWindow::boards`. Only the parts of the overlay's
/// state that a fresh page should start clean carry over here (its shapes
/// and their own undo/redo stack); everything else — background capture,
/// zoom, current tool and colour, selection — stays shared on
/// `OverlayWindow` itself, since switching boards is meant to feel like
/// flipping to a new sheet mid-presentation, not like reopening the app.
#[derive(Debug, Clone, Default)]
pub struct Board {
    pub shapes: Vec<Annotation>,
    pub undo_history: Vec<HistoryAction>,
    pub redo_history: Vec<HistoryAction>,
}

/// The eight grips around a selected shape's bounding box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionHandle {
    NW,
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
}

impl SelectionHandle {
    /// Which box edges this grip moves: (left, top, right, bottom).
    pub fn edges(self) -> (bool, bool, bool, bool) {
        match self {
            Self::NW => (true, true, false, false),
            Self::N => (false, true, false, false),
            Self::NE => (false, true, true, false),
            Self::E => (false, false, true, false),
            Self::SE => (false, false, true, true),
            Self::S => (false, false, false, true),
            Self::SW => (true, false, false, true),
            Self::W => (true, false, false, false),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragKind {
    Move,
    Resize(SelectionHandle),
    /// Dragging the middle of a line to bow it.
    Bow,
    /// Dragging one end of a line or arrow. `true` for the start.
    ///
    /// A line has no meaningful interior to scale, so its ends are grabbed
    /// directly rather than through a bounding box — and dropping an end on a
    /// shape re-anchors it there.
    Endpoint(bool),
}

/// A shape picked with the Select tool, plus any drag in progress.
#[derive(Debug, Clone)]
pub struct Selection {
    /// Everything selected, by id rather than position, because positions
    /// shift under deletions and undo.
    pub ids: Vec<ShapeId>,
    pub drag: Option<DragKind>,
    /// Canvas point where the current drag started.
    pub grab: Point2D,
    /// Each selected shape as it was when the drag started — the history
    /// "before" for every one of them.
    pub originals: Vec<(ShapeId, Shape)>,
    /// Union bounds when the drag started, so a resize maps from a fixed
    /// origin instead of compounding rounding on each mouse move.
    pub original_bounds: (f32, f32, f32, f32),
}

impl Selection {
    pub fn single(id: ShapeId) -> Self {
        Self {
            ids: vec![id],
            drag: None,
            grab: Point2D::default(),
            originals: Vec::new(),
            original_bounds: (0.0, 0.0, 0.0, 0.0),
        }
    }

    pub fn many(ids: Vec<ShapeId>) -> Self {
        Self {
            ids,
            drag: None,
            grab: Point2D::default(),
            originals: Vec::new(),
            original_bounds: (0.0, 0.0, 0.0, 0.0),
        }
    }

    pub fn is_single(&self) -> bool {
        self.ids.len() == 1
    }

    pub fn only(&self) -> Option<ShapeId> {
        if self.ids.len() == 1 {
            self.ids.first().copied()
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct LaserTrailPoint {
    pub pt: Point2D,
    pub timestamp: Instant,
}

#[derive(Debug, Clone)]
pub struct LaserRipple {
    pub center: Point2D,
    pub timestamp: Instant,
    pub color: ColorPreset,
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
pub struct LoupeState {
    pub active: bool,
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub magnification: f32,
    pub pinned: bool,
    pub is_rect: bool,
    pub show_reticle: bool,
}

impl Default for LoupeState {
    fn default() -> Self {
        Self {
            active: false,
            x: 0.0,
            y: 0.0,
            radius: 160.0,
            magnification: 2.5,
            pinned: false,
            is_rect: false,
            show_reticle: true,
        }
    }
}

impl LoupeState {
    pub fn clamp_values(&mut self) {
        self.radius = self.radius.clamp(60.0, 500.0);
        self.magnification = self.magnification.clamp(1.25, 12.0);
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

    pub fn update_target_from_cursor(
        &mut self,
        cursor_x: f32,
        cursor_y: f32,
        screen_w: f32,
        screen_h: f32,
    ) {
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

    pub fn set_zoom_centered(
        &mut self,
        new_level: f32,
        center_screen: Point2D,
        screen_w: f32,
        screen_h: f32,
    ) {
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

    pub fn center_on_canvas_point(&mut self, canvas_pt: Point2D, screen_w: f32, screen_h: f32) {
        let z = self.level.max(1.0);
        let view_w = screen_w / z;
        let view_h = screen_h / z;
        self.target_view_x = canvas_pt.x - (view_w / 2.0);
        self.target_view_y = canvas_pt.y - (view_h / 2.0);
        self.clamp_viewport(screen_w, screen_h);
        self.view_x = self.target_view_x;
        self.view_y = self.target_view_y;
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

/// Which slider of the colour picker the pointer is currently dragging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerBar {
    Hue,
    Saturation,
    Value,
}

/// In-overlay HSV colour picker, opened from the toolbar's `+` swatch.
#[derive(Debug, Clone)]
pub struct ColorPickerState {
    pub open: bool,
    pub hue: f32,
    pub sat: f32,
    pub val: f32,
    pub dragging: Option<PickerBar>,
    /// Most-recently-used custom colours, newest first; persisted to config.
    pub recent: Vec<ColorPreset>,
    pub panel: D2D_RECT_F,
    pub hue_bar: D2D_RECT_F,
    pub sat_bar: D2D_RECT_F,
    pub val_bar: D2D_RECT_F,
    pub preview: D2D_RECT_F,
    pub recent_swatches: Vec<(ColorPreset, D2D_RECT_F)>,
}

pub const PICKER_MAX_RECENT: usize = 8;

impl Default for ColorPickerState {
    fn default() -> Self {
        Self {
            open: false,
            hue: 0.0,
            sat: 0.85,
            val: 0.95,
            dragging: None,
            recent: Vec::new(),
            panel: D2D_RECT_F::default(),
            hue_bar: D2D_RECT_F::default(),
            sat_bar: D2D_RECT_F::default(),
            val_bar: D2D_RECT_F::default(),
            preview: D2D_RECT_F::default(),
            recent_swatches: Vec::new(),
        }
    }
}

impl ColorPickerState {
    /// The colour the sliders currently describe.
    pub fn current(&self) -> ColorPreset {
        let (r, g, b) = hsv_to_rgb(self.hue, self.sat, self.val);
        ColorPreset::Custom(r, g, b)
    }

    /// Seed the sliders from an existing colour so opening the picker starts
    /// from whatever is already selected.
    pub fn seed_from(&mut self, color: ColorPreset) {
        let (r, g, b) = color.rgb_u8();
        let (h, s, v) = rgb_to_hsv(r, g, b);
        self.hue = h;
        // A greyscale seed has no meaningful hue/saturation; keep the existing
        // hue so the bars stay usable instead of collapsing to red.
        if s > 0.01 {
            self.sat = s;
        }
        self.val = v;
    }

    pub fn push_recent(&mut self, color: ColorPreset) {
        self.recent.retain(|c| *c != color);
        self.recent.insert(0, color);
        self.recent.truncate(PICKER_MAX_RECENT);
    }

    /// Lay the panel out under `anchor` (the toolbar's `+` swatch), keeping it
    /// on screen.
    pub fn update_layout(&mut self, anchor: D2D_RECT_F, screen_w: f32, below_y: f32) {
        const W: f32 = 264.0;
        const H: f32 = 150.0;
        const PAD: f32 = 12.0;
        const BAR_H: f32 = 16.0;
        const BAR_GAP: f32 = 10.0;

        let anchor_cx = (anchor.left + anchor.right) / 2.0;
        let left = (anchor_cx - W / 2.0).clamp(6.0, (screen_w - W - 6.0).max(6.0));
        let top = below_y + 6.0;

        self.panel = D2D_RECT_F {
            left,
            top,
            right: left + W,
            bottom: top + H,
        };

        let bar_left = left + PAD;
        let bar_right = left + W - PAD - 40.0; // leave room for the preview chip
        let mut y = top + PAD;
        let bar = |y: f32| D2D_RECT_F {
            left: bar_left,
            top: y,
            right: bar_right,
            bottom: y + BAR_H,
        };

        self.hue_bar = bar(y);
        y += BAR_H + BAR_GAP;
        self.sat_bar = bar(y);
        y += BAR_H + BAR_GAP;
        self.val_bar = bar(y);

        self.preview = D2D_RECT_F {
            left: bar_right + 10.0,
            top: top + PAD,
            right: left + W - PAD,
            bottom: top + PAD + BAR_H * 3.0 + BAR_GAP * 2.0,
        };

        // Recent swatch strip along the bottom.
        self.recent_swatches.clear();
        let sw = 22.0;
        let gap = 6.0;
        let sy = self.val_bar.bottom + 14.0;
        for (i, c) in self.recent.iter().enumerate() {
            let sx = bar_left + i as f32 * (sw + gap);
            if sx + sw > left + W - PAD {
                break;
            }
            self.recent_swatches.push((
                *c,
                D2D_RECT_F {
                    left: sx,
                    top: sy,
                    right: sx + sw,
                    bottom: sy + sw,
                },
            ));
        }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.open
            && x >= self.panel.left
            && x <= self.panel.right
            && y >= self.panel.top
            && y <= self.panel.bottom
    }

    /// Which bar (if any) a point falls on, with a few px of vertical slack so
    /// the thin sliders stay easy to grab.
    pub fn bar_at(&self, x: f32, y: f32) -> Option<PickerBar> {
        let hit = |r: &D2D_RECT_F| {
            x >= r.left - 4.0 && x <= r.right + 4.0 && y >= r.top - 5.0 && y <= r.bottom + 5.0
        };
        if hit(&self.hue_bar) {
            Some(PickerBar::Hue)
        } else if hit(&self.sat_bar) {
            Some(PickerBar::Saturation)
        } else if hit(&self.val_bar) {
            Some(PickerBar::Value)
        } else {
            None
        }
    }

    /// Apply a pointer x-position to `bar`, as a 0..1 fraction of its width.
    pub fn set_from_x(&mut self, bar: PickerBar, x: f32) {
        let r = match bar {
            PickerBar::Hue => self.hue_bar,
            PickerBar::Saturation => self.sat_bar,
            PickerBar::Value => self.val_bar,
        };
        let w = (r.right - r.left).max(1.0);
        let t = ((x - r.left) / w).clamp(0.0, 1.0);
        match bar {
            PickerBar::Hue => self.hue = t * 360.0,
            PickerBar::Saturation => self.sat = t,
            PickerBar::Value => self.val = t,
        }
    }

    pub fn recent_at(&self, x: f32, y: f32) -> Option<ColorPreset> {
        self.recent_swatches.iter().find_map(|(c, r)| {
            (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom).then_some(*c)
        })
    }
}

#[derive(Debug, Clone)]
pub struct MinimapState {
    pub is_dragging: bool,
    pub is_hovered: bool,
    pub width: f32,
    pub margin: f32,
}

impl Default for MinimapState {
    fn default() -> Self {
        Self {
            is_dragging: false,
            is_hovered: false,
            width: 190.0,
            margin: 20.0,
        }
    }
}

impl MinimapState {
    /// Returns the outer card bounds in screen space: (left, top, right, bottom).
    /// Height is dynamically calculated from screen aspect ratio.
    pub fn get_card_bounds(&self, screen_w: f32, screen_h: f32) -> (f32, f32, f32, f32) {
        let aspect = if screen_w > 0.0 {
            (screen_h / screen_w).clamp(0.25, 1.5)
        } else {
            9.0 / 16.0
        };
        let card_w = self.width.clamp(120.0, 360.0);
        let card_h = (card_w * aspect).clamp(70.0, 220.0);
        let right = screen_w - self.margin;
        let left = right - card_w;
        let bottom = screen_h - self.margin;
        let top = bottom - card_h;
        (left, top, right, bottom)
    }

    /// Returns the inner preview rectangle (inset by 4px).
    pub fn get_inner_preview_rect(&self, screen_w: f32, screen_h: f32) -> (f32, f32, f32, f32) {
        let (l, t, r, b) = self.get_card_bounds(screen_w, screen_h);
        (l + 4.0, t + 4.0, r - 4.0, b - 4.0)
    }

    /// Returns the viewport indicator rect on the minimap: (left, top, right, bottom).
    pub fn get_viewport_rect(
        &self,
        screen_w: f32,
        screen_h: f32,
        zoom: &ZoomState,
    ) -> (f32, f32, f32, f32) {
        let (il, it, ir, ib) = self.get_inner_preview_rect(screen_w, screen_h);
        let iw = (ir - il).max(1.0);
        let ih = (ib - it).max(1.0);

        let z = zoom.level.max(1.0);
        let norm_x = if screen_w > 0.0 {
            (zoom.view_x / screen_w).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let norm_y = if screen_h > 0.0 {
            (zoom.view_y / screen_h).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let norm_w = (1.0 / z).clamp(0.05, 1.0);
        let norm_h = (1.0 / z).clamp(0.05, 1.0);

        let vp_l = il + norm_x * iw;
        let vp_t = it + norm_y * ih;
        let vp_r = (vp_l + norm_w * iw).min(ir);
        let vp_b = (vp_t + norm_h * ih).min(ib);
        (vp_l, vp_t, vp_r, vp_b)
    }

    /// Converts a point inside the minimap to a target canvas point.
    pub fn minimap_pt_to_canvas_pt(
        &self,
        screen_w: f32,
        screen_h: f32,
        mouse_x: f32,
        mouse_y: f32,
    ) -> Point2D {
        let (il, it, ir, ib) = self.get_inner_preview_rect(screen_w, screen_h);
        let iw = (ir - il).max(1.0);
        let ih = (ib - it).max(1.0);

        let norm_x = ((mouse_x - il) / iw).clamp(0.0, 1.0);
        let norm_y = ((mouse_y - it) / ih).clamp(0.0, 1.0);

        Point2D::new(norm_x * screen_w, norm_y * screen_h)
    }

    /// Checks if a screen-space point is inside the minimap card bounds.
    pub fn hit_test(&self, screen_w: f32, screen_h: f32, x: f32, y: f32) -> bool {
        let (l, t, r, b) = self.get_card_bounds(screen_w, screen_h);
        x >= l && x <= r && y >= t && y <= b
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlurToolSettings {
    pub block_size: f32,
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
        (
            cx - card_w / 2.0,
            cy - card_h / 2.0,
            cx + card_w / 2.0,
            cy + card_h / 2.0,
            cx,
            cy,
        )
    }

    pub fn get_pill_rect(&self, screen_w: f32, screen_h: f32) -> (f32, f32, f32, f32) {
        let pill_w = 280.0;
        let pill_h = 46.0;
        let margin = 24.0;
        match self.pill_corner {
            1 => (margin, margin, margin + pill_w, margin + pill_h), // Top-Left
            2 => (
                margin,
                screen_h - margin - pill_h,
                margin + pill_w,
                screen_h - margin,
            ), // Bottom-Left
            3 => (
                screen_w - margin - pill_w,
                screen_h - margin - pill_h,
                screen_w - margin,
                screen_h - margin,
            ), // Bottom-Right
            _ => (
                screen_w - margin - pill_w,
                margin,
                screen_w - margin,
                margin + pill_h,
            ), // Top-Right (default 0)
        }
    }

    pub fn get_action_at(&self, pt: Point2D, screen_w: f32, screen_h: f32) -> Option<TimerAction> {
        if self.minimized {
            let (left, top, right, bottom) = self.get_pill_rect(screen_w, screen_h);

            if pt.x < left || pt.x > right || pt.y < top || pt.y > bottom {
                return None;
            }

            // b0: Corner cycle button (left + 6.0 .. left + 34.0)
            if pt.x >= left + 6.0
                && pt.x <= left + 34.0
                && pt.y >= top + 8.0
                && pt.y <= bottom - 8.0
            {
                return Some(TimerAction::CycleCorner);
            }

            // b1: Play/Pause (right - 105.0 .. right - 72.0, top + 8.0 .. bottom - 8.0)
            if pt.x >= right - 105.0
                && pt.x <= right - 72.0
                && pt.y >= top + 8.0
                && pt.y <= bottom - 8.0
            {
                return Some(TimerAction::PlayPause);
            }
            // b2: Expand / ToggleMinimize (right - 68.0 .. right - 38.0, top + 8.0 .. bottom - 8.0)
            if pt.x >= right - 68.0
                && pt.x <= right - 38.0
                && pt.y >= top + 8.0
                && pt.y <= bottom - 8.0
            {
                return Some(TimerAction::ToggleMinimize);
            }
            // b3: Close (right - 35.0 .. right - 5.0, top + 8.0 .. bottom - 8.0)
            if pt.x >= right - 35.0
                && pt.x <= right - 5.0
                && pt.y >= top + 8.0
                && pt.y <= bottom - 8.0
            {
                return Some(TimerAction::Close);
            }

            return Some(TimerAction::ToggleMinimize);
        }

        let (card_left, card_top, card_right, card_bottom, cx, cy) =
            self.get_card_bounds(screen_w, screen_h);

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
            if pt.x >= px
                && pt.x <= px + pill_w
                && pt.y >= pill_row_y
                && pt.y <= pill_row_y + pill_h
            {
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

/// Shift a countdown by `delta_secs`, keeping the total duration and the time
/// remaining consistent. Returns `(new_total_secs, new_remaining_secs)`.
///
/// The total is the denominator of the progress ring and the value persisted as
/// the user's default duration, so it must stay a *duration*; overwriting it
/// with whatever is left on the clock corrupts both.
pub fn adjust_timer_values(total_secs: u32, remaining: f64, delta_secs: f64) -> (u32, f64) {
    const MIN_TOTAL: f64 = 60.0;
    const MAX_TOTAL: f64 = 24.0 * 3600.0;

    let old_total = total_secs as f64;
    let new_total = (old_total + delta_secs).clamp(MIN_TOTAL, MAX_TOTAL);
    let applied = new_total - old_total;
    if applied == 0.0 {
        return (total_secs, remaining);
    }

    // Never let the clock read more than the total it counts down from.
    let new_remaining = (remaining + applied).clamp(0.0, new_total);
    (new_total.round() as u32, new_remaining)
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
    ModeLoupe,
    CycleDisplay,
    Tool(DrawTool),
    Color(ColorPreset),
    /// The `+` swatch: opens the in-overlay HSV picker.
    OpenColorPicker,
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
    // Select Sub-bar Actions
    SetArrowHead(ArrowHead),
    AdjustOpacity(f32),
    Align(AlignTo),
    /// True spreads horizontally.
    Distribute(bool),
    /// True brings to front.
    Restack(bool),
    Duplicate,
    /// True groups, false ungroups.
    SetGroup(bool),
    DeleteSelection,
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
    /// Which arrowhead button shows as active.
    pub current_arrow_head: ArrowHead,
    /// The kind of shape selected, when they are all the same kind.
    ///
    /// This is what the Select tool's sub-bar describes. None when nothing is
    /// selected or the selection is mixed, since there is then no single set
    /// of properties to offer.
    pub selection_kind: Option<DrawTool>,
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
            current_arrow_head: ArrowHead::default(),
            selection_kind: None,
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
        let (bar, items, grip, seps) = compute_toolbar_layout(
            screen_w,
            screen_h,
            self.collapsed,
            self.custom_position,
            self.monitor_count,
        );
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
                self.selection_kind,
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
            if x >= item.rect.left
                && x <= item.rect.right
                && y >= item.rect.top
                && y <= item.rect.bottom
            {
                return Some(item.action);
            }
        }
        for item in &self.subbar_items {
            if x >= item.rect.left
                && x <= item.rect.right
                && y >= item.rect.top
                && y <= item.rect.bottom
            {
                return Some(item.action);
            }
        }
        None
    }

    pub fn is_point_inside(&self, x: f32, y: f32) -> bool {
        if !self.visible {
            return false;
        }
        let in_main = x >= self.bar_rect.left
            && x <= self.bar_rect.right
            && y >= self.bar_rect.top
            && y <= self.bar_rect.bottom;
        if in_main {
            return true;
        }
        if let Some(sb) = self.subbar_rect
            && x >= sb.left
            && x <= sb.right
            && y >= sb.top
            && y <= sb.bottom
        {
            return true;
        }
        false
    }
}

#[allow(clippy::too_many_arguments)]
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
    selection_kind: Option<DrawTool>,
) -> (Option<D2D_RECT_F>, Vec<ToolbarItemBounds>, Vec<f32>) {
    let tool = match active_tool {
        Some(t) => t,
        None => return (None, Vec::new(), Vec::new()),
    };

    let mut groups: Vec<Vec<(FluentAction, f32)>> = Vec::with_capacity(5);

    // With the Select tool the panel describes the *selection*: it borrows
    // the property controls for whatever kind of shape is picked, then adds
    // the commands that only apply to something already drawn. Every other
    // tool shows its own properties, which arm the next shape.
    if tool == DrawTool::Select {
        if let Some(kind) = selection_kind {
            groups.extend(property_groups(kind));
        }
        groups.extend(select_command_groups());
    } else {
        groups.extend(property_groups(tool));
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

    let subbar_rect = D2D_RECT_F {
        left,
        top,
        right,
        bottom,
    };

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
    item_specs.push((FluentAction::ModeLoupe, 34.0));
    if monitor_count > 1 {
        item_specs.push((FluentAction::CycleDisplay, 34.0));
    }

    let mode_end_idx = if monitor_count > 1 { 5 } else { 4 };

    // Tools (13 items)
    let tools = [
        DrawTool::Select,
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
        DrawTool::StickyNote,
        DrawTool::Blur,
    ];
    for t in tools {
        item_specs.push((FluentAction::Tool(t), 32.0));
    }
    let tools_end_idx = mode_end_idx + 13;

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
    // Trailing "+" swatch opens the custom colour picker.
    item_specs.push((FluentAction::OpenColorPicker, 22.0));
    let colors_end_idx = tools_end_idx + 9;

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

    // The full bar is ~990 DIPs. On a narrow or heavily scaled display that
    // overruns the screen, and only `left` was ever clamped - so the right-hand
    // buttons (Copy, Save, Close) simply could not be reached. Shrink the whole
    // bar to fit instead, keeping every control on screen and hit-testable.
    let available = screen_w - 12.0;
    let fit = if total_w > available && available > 0.0 {
        (available / total_w).max(0.35)
    } else {
        1.0
    };

    let height = height * fit;
    let pad_x = pad_x * fit;
    let btn_pad_y = btn_pad_y * fit;
    let grip_w = grip_w * fit;
    let spacing = spacing * fit;
    let divider_spacing = divider_spacing * fit;
    let total_w = total_w * fit;
    for spec in item_specs.iter_mut() {
        spec.1 *= fit;
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
    fn test_image_pixels_round_trip_through_json() {
        // Every byte value once, so a bad base64 alphabet or padding edge
        // would show up rather than hiding in an all-zero buffer.
        let bgra: Vec<u8> = (0..=255u8).cycle().take(4 * 3 * 2).collect();
        let img = ImagePixels {
            width: 3,
            height: 2,
            bgra,
        };
        let json = serde_json::to_string(&img).unwrap();
        // The wire format is base64, not a huge JSON array of numbers.
        assert!(json.contains("bgra_base64"));
        assert!(!json.contains("[0,1,2"));

        let back: ImagePixels = serde_json::from_str(&json).unwrap();
        assert_eq!(back, img);
    }

    #[test]
    fn test_image_pixels_rejects_a_length_that_does_not_match_dimensions() {
        // A crafted or truncated file must fail to load rather than panic
        // later on an out-of-bounds row read in the renderer.
        let bad = r#"{"width":10,"height":10,"bgra_base64":"AAAA"}"#;
        assert!(serde_json::from_str::<ImagePixels>(bad).is_err());
    }

    #[test]
    fn test_image_cache_key_is_stable_and_content_sensitive() {
        let a = ImagePixels {
            width: 4,
            height: 4,
            bgra: vec![10u8; 4 * 4 * 4],
        };
        let b = ImagePixels {
            width: 4,
            height: 4,
            bgra: vec![10u8; 4 * 4 * 4],
        };
        let mut c = b.clone();
        c.bgra[0] = 200;

        assert_eq!(a.cache_key(), b.cache_key(), "identical content must share a key");
        assert_ne!(a.cache_key(), c.cache_key(), "changed content must not collide");
    }

    #[test]
    fn test_adjust_timer_keeps_total_and_remaining_consistent() {
        // Adding a minute extends both the total and the clock.
        let (total, rem) = adjust_timer_values(600, 600.0, 60.0);
        assert_eq!(total, 660);
        assert_eq!(rem, 660.0);

        // Mid-countdown: total grows, remaining grows by the same amount.
        let (total, rem) = adjust_timer_values(600, 120.0, 60.0);
        assert_eq!(total, 660);
        assert_eq!(rem, 180.0);

        // Regression: the total must never be replaced by the remaining time.
        // A 10-minute timer with 2 minutes left is still a 10-minute timer.
        let (total, _) = adjust_timer_values(600, 120.0, 0.0);
        assert_eq!(total, 600);
    }

    #[test]
    fn test_adjust_timer_clamps() {
        // Total floors at one minute, so it can never reach zero and make the
        // progress ring divide by nothing.
        let (total, rem) = adjust_timer_values(60, 60.0, -60.0);
        assert_eq!(total, 60);
        assert_eq!(rem, 60.0);

        // Remaining is never greater than the total.
        let (total, rem) = adjust_timer_values(120, 120.0, -60.0);
        assert_eq!(total, 60);
        assert!(rem <= total as f64, "remaining {} exceeded total {}", rem, total);

        // Subtracting past the end parks at zero rather than inventing overtime.
        let (_, rem) = adjust_timer_values(600, 30.0, -60.0);
        assert_eq!(rem, 0.0);

        // Total is capped at 24h.
        let (total, _) = adjust_timer_values(86400, 10.0, 60.0);
        assert_eq!(total, 86400);
    }

    #[test]
    fn test_adjust_timer_recovers_from_overtime() {
        // Clock has run 30s past zero; adding a minute puts real time back on it.
        let (total, rem) = adjust_timer_values(600, -30.0, 60.0);
        assert_eq!(total, 660);
        assert_eq!(rem, 30.0);
        assert!(rem > 0.0, "adding time must lift the clock out of overtime");
    }

    #[test]
    fn test_toolbar_fits_within_narrow_screens() {
        // The full bar is ~990 DIPs wide. On anything narrower it must shrink to
        // fit: previously only `left` was clamped, so the right-hand actions
        // (Copy, Save, Close) fell off the screen and could not be clicked.
        for screen_w in [1920.0_f32, 1366.0, 1024.0, 900.0, 800.0] {
            let (bar, items, grip, _) =
                compute_toolbar_layout(screen_w, 768.0, false, None, 1);

            assert!(
                bar.left >= 0.0 && bar.right <= screen_w,
                "bar {}..{} escapes a {}px screen",
                bar.left,
                bar.right,
                screen_w
            );
            assert!(grip.right <= screen_w, "grip off-screen at {}px", screen_w);
            assert!(!items.is_empty());

            for item in &items {
                assert!(
                    item.rect.left >= 0.0 && item.rect.right <= screen_w,
                    "{:?} at {}..{} is unreachable on a {}px screen",
                    item.action,
                    item.rect.left,
                    item.rect.right,
                    screen_w
                );
                assert!(item.rect.right > item.rect.left, "item collapsed to zero width");
            }

            // The last action must stay hit-testable.
            let last = items.last().expect("items");
            let mid_x = (last.rect.left + last.rect.right) / 2.0;
            let mid_y = (last.rect.top + last.rect.bottom) / 2.0;
            let mut tb = FluentToolbarState {
                monitor_count: 1,
                ..Default::default()
            };
            tb.update_layout(screen_w, 768.0);
            assert!(
                tb.hit_test(mid_x, mid_y).is_some(),
                "last toolbar item not hit-testable at {}px",
                screen_w
            );
        }
    }

    #[test]
    fn test_toolbar_unscaled_on_wide_screens() {
        // A screen with room to spare must not shrink the bar.
        let (wide, _, _, _) = compute_toolbar_layout(2560.0, 1440.0, false, None, 1);
        let (fhd, _, _, _) = compute_toolbar_layout(1920.0, 1080.0, false, None, 1);
        let wide_w = wide.right - wide.left;
        let fhd_w = fhd.right - fhd.left;
        assert!(
            (wide_w - fhd_w).abs() < 0.5,
            "bar width changed between roomy screens: {} vs {}",
            wide_w,
            fhd_w
        );
    }

    #[test]
    fn test_hsv_rgb_roundtrip() {
        // Saturated primaries must survive a round trip exactly.
        for (h, s, v, expect) in [
            (0.0, 1.0, 1.0, (255u8, 0u8, 0u8)),
            (120.0, 1.0, 1.0, (0, 255, 0)),
            (240.0, 1.0, 1.0, (0, 0, 255)),
            (60.0, 1.0, 1.0, (255, 255, 0)),
        ] {
            assert_eq!(hsv_to_rgb(h, s, v), expect, "hsv({}, {}, {})", h, s, v);
            let (rh, rs, rv) = rgb_to_hsv(expect.0, expect.1, expect.2);
            assert!((rh - h).abs() < 0.5, "hue {} != {}", rh, h);
            assert!((rs - s).abs() < 0.01);
            assert!((rv - v).abs() < 0.01);
        }

        // Arbitrary colours should round trip within rounding error.
        for rgb in [(18u8, 200u8, 77u8), (250, 12, 190), (99, 99, 99), (0, 0, 0)] {
            let (h, s, v) = rgb_to_hsv(rgb.0, rgb.1, rgb.2);
            let back = hsv_to_rgb(h, s, v);
            assert!(
                (back.0 as i32 - rgb.0 as i32).abs() <= 1
                    && (back.1 as i32 - rgb.1 as i32).abs() <= 1
                    && (back.2 as i32 - rgb.2 as i32).abs() <= 1,
                "{:?} -> hsv -> {:?}",
                rgb,
                back
            );
        }
    }

    #[test]
    fn test_custom_color_config_roundtrip() {
        let c = ColorPreset::Custom(0x1E, 0xC8, 0x4D);
        assert_eq!(c.name(), "#1EC84D");
        assert_eq!(ColorPreset::from_config_str("#1EC84D"), Some(c));
        assert_eq!(ColorPreset::from_config_str("1ec84d"), Some(c));

        // Preset names still parse, case-insensitively.
        assert_eq!(ColorPreset::from_config_str("Pink"), Some(ColorPreset::Pink));
        assert_eq!(ColorPreset::from_config_str("cyan"), Some(ColorPreset::Cyan));

        // Junk is rejected rather than silently becoming a colour.
        assert_eq!(ColorPreset::from_config_str("#12345"), None);
        assert_eq!(ColorPreset::from_config_str("nope"), None);
        assert_eq!(ColorPreset::from_config_str("#ZZZZZZ"), None);
    }

    #[test]
    fn test_picker_bars_and_recents() {
        let mut p = ColorPickerState {
            open: true,
            ..Default::default()
        };
        p.recent = vec![
            ColorPreset::Custom(1, 2, 3),
            ColorPreset::Custom(4, 5, 6),
        ];
        let anchor = D2D_RECT_F {
            left: 900.0,
            top: 12.0,
            right: 922.0,
            bottom: 44.0,
        };
        p.update_layout(anchor, 1920.0, 56.0);

        assert!(p.panel.right <= 1920.0 && p.panel.left >= 0.0);
        assert!(p.contains(p.panel.left + 5.0, p.panel.top + 5.0));
        assert!(!p.contains(p.panel.left - 20.0, p.panel.top + 5.0));

        // Dragging each bar to its far right saturates that channel.
        p.set_from_x(PickerBar::Hue, p.hue_bar.right + 50.0);
        assert!((p.hue - 360.0).abs() < 0.01);
        p.set_from_x(PickerBar::Saturation, p.sat_bar.left - 50.0);
        assert_eq!(p.sat, 0.0);
        p.set_from_x(PickerBar::Value, (p.val_bar.left + p.val_bar.right) / 2.0);
        assert!((p.val - 0.5).abs() < 0.02);

        // Each bar is identified where it is drawn.
        let mid = |r: D2D_RECT_F| ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
        let (hx, hy) = mid(p.hue_bar);
        assert_eq!(p.bar_at(hx, hy), Some(PickerBar::Hue));
        let (sx, sy) = mid(p.sat_bar);
        assert_eq!(p.bar_at(sx, sy), Some(PickerBar::Saturation));

        // Recents are newest-first, de-duplicated, and capped.
        let mut q = ColorPickerState::default();
        for i in 0..(PICKER_MAX_RECENT as u8 + 4) {
            q.push_recent(ColorPreset::Custom(i, 0, 0));
        }
        assert_eq!(q.recent.len(), PICKER_MAX_RECENT);
        assert_eq!(q.recent[0], ColorPreset::Custom(PICKER_MAX_RECENT as u8 + 3, 0, 0));

        q.push_recent(ColorPreset::Custom(0, 0, 0));
        q.push_recent(ColorPreset::Custom(0, 0, 0));
        assert_eq!(
            q.recent.iter().filter(|c| **c == ColorPreset::Custom(0, 0, 0)).count(),
            1,
            "re-picking a colour must not duplicate it"
        );
    }

    #[test]
    fn test_point_distance() {
        let p1 = Point2D::new(0.0, 0.0);
        let p2 = Point2D::new(3.0, 4.0);
        assert_eq!(p1.distance(&p2), 5.0);
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
        let zoom = ZoomState {
            level: 2.0,
            target_level: 2.0,
            view_x: 100.0,
            view_y: 50.0,
            ..Default::default()
        };

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
        let mut zoom = ZoomState {
            level: 2.0,
            ..Default::default()
        };
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
        let mut tb = FluentToolbarState {
            monitor_count: 2,
            ..Default::default()
        };
        tb.update_layout(screen_w, screen_h);

        // Verify CycleDisplay is present in items
        let cycle_display_item = tb
            .items
            .iter()
            .find(|it| it.action == FluentAction::CycleDisplay);
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
        let (_card_left, card_top, card_right, _card_bottom, cx, cy) =
            widget.get_card_bounds(screen_w, screen_h);

        // 1. Full center card mode
        // Clock face center click toggles PlayPause
        let clock_center = Point2D::new(cx, cy - 10.0);
        assert_eq!(
            widget.get_action_at(clock_center, screen_w, screen_h),
            Some(TimerAction::PlayPause)
        );

        // Quick duration pill (5m)
        let pill_5m = Point2D::new(cx - 150.0, cy - 150.0);
        assert_eq!(
            widget.get_action_at(pill_5m, screen_w, screen_h),
            Some(TimerAction::SetDuration(5))
        );

        // Far away click hits nothing
        let outside = Point2D::new(50.0, 50.0);
        assert_eq!(widget.get_action_at(outside, screen_w, screen_h), None);

        // Buttons row
        let btn_y = cy + 130.0;

        // Hero Play/Pause button (center)
        let hero_pt = Point2D::new(cx, btn_y);
        assert_eq!(
            widget.get_action_at(hero_pt, screen_w, screen_h),
            Some(TimerAction::PlayPause)
        );

        // +1m button (cx + 60.0)
        let plus1_pt = Point2D::new(cx + 60.0, btn_y);
        assert_eq!(
            widget.get_action_at(plus1_pt, screen_w, screen_h),
            Some(TimerAction::AddMinute)
        );

        // -1m button (cx - 120.0)
        let minus1_pt = Point2D::new(cx - 120.0, btn_y);
        assert_eq!(
            widget.get_action_at(minus1_pt, screen_w, screen_h),
            Some(TimerAction::SubMinute)
        );

        // Reset button (cx - 60.0)
        let reset_pt = Point2D::new(cx - 60.0, btn_y);
        assert_eq!(
            widget.get_action_at(reset_pt, screen_w, screen_h),
            Some(TimerAction::Reset)
        );

        // Mini button (cx + 120.0)
        let mini_pt = Point2D::new(cx + 120.0, btn_y);
        assert_eq!(
            widget.get_action_at(mini_pt, screen_w, screen_h),
            Some(TimerAction::ToggleMinimize)
        );

        // Close button (top right: card_right - 32.0, card_top + 28.0)
        let close_pt = Point2D::new(card_right - 32.0, card_top + 28.0);
        assert_eq!(
            widget.get_action_at(close_pt, screen_w, screen_h),
            Some(TimerAction::Close)
        );

        // 2. Corner mini-pill mode
        widget.minimized = true;
        let (left, top, right, _bottom) = widget.get_pill_rect(screen_w, screen_h);

        // Cycle corner button in mini-pill (left + 15, top + 15)
        let mini_cycle = Point2D::new(left + 15.0, top + 15.0);
        assert_eq!(
            widget.get_action_at(mini_cycle, screen_w, screen_h),
            Some(TimerAction::CycleCorner)
        );

        // Play/Pause button in mini-pill (right - 90, top + 15)
        let mini_play = Point2D::new(right - 90.0, top + 15.0);
        assert_eq!(
            widget.get_action_at(mini_play, screen_w, screen_h),
            Some(TimerAction::PlayPause)
        );

        // Expand button in mini-pill (right - 50, top + 15)
        let mini_expand = Point2D::new(right - 50.0, top + 15.0);
        assert_eq!(
            widget.get_action_at(mini_expand, screen_w, screen_h),
            Some(TimerAction::ToggleMinimize)
        );

        // Close button in mini-pill (right - 20, top + 15)
        let mini_close = Point2D::new(right - 20.0, top + 15.0);
        assert_eq!(
            widget.get_action_at(mini_close, screen_w, screen_h),
            Some(TimerAction::Close)
        );
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
    fn test_property_setters_reach_every_variant_that_has_the_field() {
        use crate::shapes::{set_shape_color, set_shape_fill, set_shape_pattern, set_shape_width};

        let mut rect = Shape::Rectangle {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(10.0, 10.0),
            color: ColorPreset::Red,
            width: 2.0,
            rounded: false,
            fill: FillMode::None,
            pattern: StrokePattern::Solid,
        };
        assert!(set_shape_color(&mut rect, ColorPreset::Blue));
        assert!(set_shape_width(&mut rect, 8.0));
        assert!(set_shape_fill(&mut rect, FillMode::Solid));
        assert!(set_shape_pattern(&mut rect, StrokePattern::Dashed));
        // Setting the same value again is not a change, so no undo entry.
        assert!(!set_shape_color(&mut rect, ColorPreset::Blue));
        assert!(!set_shape_width(&mut rect, 8.0));
    }

    #[test]
    fn test_property_setters_decline_fields_a_shape_does_not_have() {
        use crate::shapes::{set_shape_color, set_shape_fill, set_shape_width};

        let mut blur = Shape::Blur {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(10.0, 10.0),
            block_size: 14.0,
        };
        // A blur shows what is underneath, so it has no colour to set.
        assert!(!set_shape_color(&mut blur, ColorPreset::Blue));
        // Its width knob is how coarse the mosaic is.
        assert!(set_shape_width(&mut blur, 30.0));

        let mut text = Shape::Text {
            origin: Point2D::new(0.0, 0.0),
            text: "hi".to_string(),
            font_size: 20.0,
            color: ColorPreset::Red,
            is_bold: false,
            is_italic: false,
            card_style: TextCardStyle::Badge,
            font_family: TextFontFamily::SegoeUI,
        };
        assert!(set_shape_color(&mut text, ColorPreset::Green));
        // Text has no stroke width and no fill mode.
        assert!(!set_shape_width(&mut text, 8.0));
        assert!(!set_shape_fill(&mut text, FillMode::Solid));
    }

    #[test]
    fn test_the_select_subbar_describes_whatever_is_selected() {
        let bar = D2D_RECT_F {
            left: 100.0,
            top: 40.0,
            right: 1000.0,
            bottom: 84.0,
        };
        let build = |kind: Option<DrawTool>| {
            compute_subbar_layout(
                bar,
                Some(DrawTool::Select),
                FillMode::None,
                StrokePattern::Solid,
                ArrowStyle::Single,
                BadgeSize::Medium,
                BadgeShape::Circle,
                4.0,
                1,
                kind,
            )
            .1
        };

        // Nothing selected: only the commands, which need no shape to exist.
        let bare = build(None);
        assert!(bare.iter().any(|i| matches!(i.action, FluentAction::Align(_))));
        assert!(
            !bare
                .iter()
                .any(|i| matches!(i.action, FluentAction::SetFillMode(_))),
            "no shape is selected, so there is nothing to set a fill on"
        );

        // A rectangle selected: its own controls appear alongside the commands.
        let with_rect = build(Some(DrawTool::Rectangle));
        assert!(
            with_rect
                .iter()
                .any(|i| matches!(i.action, FluentAction::SetFillMode(_))),
            "a selected rectangle should offer its fill modes"
        );
        assert!(
            with_rect
                .iter()
                .any(|i| matches!(i.action, FluentAction::Align(_))),
            "the commands stay available too"
        );

        // Text has no fill, so selecting text must not offer one.
        let with_text = build(Some(DrawTool::Text));
        assert!(
            with_text
                .iter()
                .any(|i| matches!(i.action, FluentAction::SetFontSize(_)))
        );
        assert!(
            !with_text
                .iter()
                .any(|i| matches!(i.action, FluentAction::SetFillMode(_)))
        );
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
        assert!(
            tb.subbar_items
                .iter()
                .any(|i| i.action == FluentAction::SetArrowStyle(ArrowStyle::Double))
        );

        // 3. StepBadge tool selected -> subbar should contain sizes, shapes, fills, widths, reset
        tb.active_tool = Some(DrawTool::StepBadge);
        tb.update_layout(1920.0, 1080.0);
        assert!(
            tb.subbar_items
                .iter()
                .any(|i| i.action == FluentAction::ResetBadgeCounter)
        );
        assert!(
            tb.subbar_items
                .iter()
                .any(|i| i.action == FluentAction::SetBadgeSize(BadgeSize::Large))
        );
        assert!(
            tb.subbar_items
                .iter()
                .any(|i| i.action == FluentAction::SetBadgeShape(BadgeShape::Hexagon))
        );
        assert!(
            tb.subbar_items
                .iter()
                .any(|i| i.action == FluentAction::SetFillMode(FillMode::Solid))
        );
        assert!(
            tb.subbar_items
                .iter()
                .any(|i| i.action == FluentAction::SetStrokeWidth(4.0))
        );

        // 4. Text tool selected -> subbar should contain font sizes, bold, italic, card style, fonts
        tb.active_tool = Some(DrawTool::Text);
        tb.update_layout(1920.0, 1080.0);
        assert!(tb.subbar_rect.is_some());
        assert!(
            tb.subbar_items
                .iter()
                .any(|i| i.action == FluentAction::SetFontSize(20.0))
        );
        assert!(
            tb.subbar_items
                .iter()
                .any(|i| i.action == FluentAction::ToggleBold)
        );
        assert!(
            tb.subbar_items
                .iter()
                .any(|i| i.action == FluentAction::SetTextCardStyle(TextCardStyle::Badge))
        );
        assert!(
            tb.subbar_items
                .iter()
                .any(|i| i.action == FluentAction::SetFontFamily(TextFontFamily::CascadiaCode))
        );

        // 5. Blur tool selected -> subbar contains block size options
        tb.active_tool = Some(DrawTool::Blur);
        tb.update_layout(1920.0, 1080.0);
        assert!(tb.subbar_rect.is_some());
        assert_eq!(tb.subbar_items.len(), 4);
        assert!(
            tb.subbar_items
                .iter()
                .any(|i| i.action == FluentAction::SetStrokeWidth(8.0))
        );
        assert!(
            tb.subbar_items
                .iter()
                .any(|i| i.action == FluentAction::SetStrokeWidth(14.0))
        );

        // 6. Deselect tool (None) -> subbar disappears completely
        tb.active_tool = None;
        tb.update_layout(1920.0, 1080.0);
        assert!(tb.subbar_rect.is_none());
        assert!(tb.subbar_items.is_empty());
    }

    #[test]
    fn test_timer_corner_positions_and_custom_drag() {
        let screen_w = 1920.0;
        let screen_h = 1080.0;
        let mut widget = TimerWidgetState {
            custom_pos: Some(Point2D::new(400.0, 300.0)),
            ..Default::default()
        };

        // Check custom dragging position
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
        let pen = StrokeToolSettings {
            stroke_width: 2.0,
            pattern: StrokePattern::Solid,
        };
        let mut highlighter = StrokeToolSettings {
            stroke_width: 14.0,
            pattern: StrokePattern::Solid,
        };
        let mut arrow = ArrowToolSettings {
            stroke_width: 4.0,
            style: ArrowStyle::Single,
            pattern: StrokePattern::Solid,
        };
        let mut rect = ShapeToolSettings {
            stroke_width: 3.0,
            fill_mode: FillMode::None,
            pattern: StrokePattern::Solid,
        };
        let badge = StepBadgeToolSettings {
            size: BadgeSize::Medium,
            shape: BadgeShape::Circle,
            fill: FillMode::Solid,
            stroke_width: 2.0,
        };
        let mut text = TextToolSettings {
            font_size: 20.0,
            is_bold: false,
            is_italic: false,
            card_style: TextCardStyle::Transparent,
            font_family: TextFontFamily::SegoeUI,
        };

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
    fn test_laser_ripple_initialization() {
        let ripple = LaserRipple {
            center: Point2D::new(250.0, 350.0),
            timestamp: Instant::now(),
            color: ColorPreset::Red,
        };
        assert_eq!(ripple.center.x, 250.0);
        assert_eq!(ripple.center.y, 350.0);
        assert_eq!(ripple.color, ColorPreset::Red);
    }

    #[test]
    fn test_loupe_state_defaults_and_clamping() {
        let mut loupe = LoupeState::default();
        assert!(!loupe.active);
        assert_eq!(loupe.radius, 160.0);
        assert_eq!(loupe.magnification, 2.5);
        assert!(!loupe.pinned);
        assert!(!loupe.is_rect);
        assert!(loupe.show_reticle);

        // Test clamping
        loupe.radius = 10.0;
        loupe.magnification = 0.5;
        loupe.clamp_values();
        assert_eq!(loupe.radius, 60.0);
        assert_eq!(loupe.magnification, 1.25);

        loupe.radius = 1000.0;
        loupe.magnification = 50.0;
        loupe.clamp_values();
        assert_eq!(loupe.radius, 500.0);
        assert_eq!(loupe.magnification, 12.0);
    }

    #[test]
    fn test_fluent_toolbar_loupe_action() {
        let screen_w = 1920.0;
        let screen_h = 1080.0;
        let mut tb = FluentToolbarState::default();
        tb.update_layout(screen_w, screen_h);

        let loupe_item = tb
            .items
            .iter()
            .find(|it| it.action == FluentAction::ModeLoupe);
        assert!(loupe_item.is_some());
        let item = loupe_item.unwrap();
        let mid_x = (item.rect.left + item.rect.right) / 2.0;
        let mid_y = (item.rect.top + item.rect.bottom) / 2.0;
        assert_eq!(tb.hit_test(mid_x, mid_y), Some(FluentAction::ModeLoupe));
    }

    #[test]
    fn test_minimap_bounds_and_aspect_ratio() {
        let minimap = MinimapState::default();
        let sw = 1920.0;
        let sh = 1080.0;
        let (l, t, r, b) = minimap.get_card_bounds(sw, sh);
        assert_eq!(r, sw - minimap.margin);
        assert_eq!(b, sh - minimap.margin);
        let w = r - l;
        let h = b - t;
        assert!((w - 190.0).abs() < 0.01);
        let expected_h = 190.0 * (1080.0 / 1920.0);
        assert!((h - expected_h).abs() < 0.01);

        assert!(minimap.hit_test(sw, sh, l + 10.0, t + 10.0));
        assert!(!minimap.hit_test(sw, sh, 50.0, 50.0));
    }

    #[test]
    fn test_minimap_viewport_and_click_mapping() {
        let minimap = MinimapState::default();
        let sw = 1920.0;
        let sh = 1080.0;
        let mut zoom = ZoomState {
            level: 2.0,
            view_x: 480.0,
            view_y: 270.0,
            ..Default::default()
        };

        let (vl, vt, vr, vb) = minimap.get_viewport_rect(sw, sh, &zoom);
        let (il, it, ir, ib) = minimap.get_inner_preview_rect(sw, sh);
        let iw = ir - il;
        let ih = ib - it;

        // At 2x zoom centered, viewport should occupy 50% width and 50% height, centered
        let expected_vl = il + 0.25 * iw;
        let expected_vt = it + 0.25 * ih;
        assert!((vl - expected_vl).abs() < 0.5);
        assert!((vt - expected_vt).abs() < 0.5);
        assert!(((vr - vl) - 0.5 * iw).abs() < 0.5);
        assert!(((vb - vt) - 0.5 * ih).abs() < 0.5);

        // Click conversion test
        let click_pt = minimap.minimap_pt_to_canvas_pt(sw, sh, il + 0.5 * iw, it + 0.5 * ih);
        assert!((click_pt.x - 960.0).abs() < 1.0);
        assert!((click_pt.y - 540.0).abs() < 1.0);

        // Center on canvas point test
        zoom.center_on_canvas_point(click_pt, sw, sh);
        assert!((zoom.view_x - 480.0).abs() < 1.0);
        assert!((zoom.view_y - 270.0).abs() < 1.0);
    }
    fn editor(text: &str, cursor: usize) -> TextEditorState {
        let mut ed = TextEditorState::new(
            Point2D::new(0.0, 0.0),
            ColorPreset::Red,
            22.0,
            false,
            false,
            TextCardStyle::Badge,
            TextFontFamily::SegoeUI,
        );
        ed.text = text.to_string();
        ed.cursor = cursor;
        ed
    }

    #[test]
    fn test_editor_enter_inserts_newline_at_caret() {
        let mut ed = editor("abcd", 2);
        ed.insert_newline();
        assert_eq!(ed.text, "ab\ncd");
        assert_eq!(ed.cursor, 3);
        assert_eq!(ed.line_count(), 2);
    }

    #[test]
    fn test_editor_move_up_down_preserves_column() {
        // Caret on line 2 col 3; up lands on line 1 col 3, down returns.
        let mut ed = editor("hello\nworld\nhi", 9);
        ed.move_up();
        assert_eq!(ed.cursor, 3);
        ed.move_down();
        assert_eq!(ed.cursor, 9);
    }

    #[test]
    fn test_editor_move_down_clamps_to_short_line() {
        // Column 4 on a 2-char line clamps to that line's end, not past it.
        let mut ed = editor("abcdef\nxy", 4);
        ed.move_down();
        assert_eq!(ed.cursor, 9); // end of "xy"
    }

    #[test]
    fn test_editor_move_up_at_first_line_goes_to_start() {
        let mut ed = editor("abc\ndef", 2);
        ed.move_up();
        assert_eq!(ed.cursor, 0);
    }

    #[test]
    fn test_editor_move_down_at_last_line_goes_to_end() {
        let mut ed = editor("abc\ndef", 5);
        ed.move_down();
        assert_eq!(ed.cursor, 7);
    }

    #[test]
    fn test_editor_home_end_are_line_scoped() {
        let mut ed = editor("abc\ndefgh\nij", 6);
        ed.move_line_start();
        assert_eq!(ed.cursor, 4);
        ed.move_line_end();
        assert_eq!(ed.cursor, 9);
    }

    #[test]
    fn test_editor_column_counted_in_chars_not_bytes() {
        // Line 1 is 3 chars but 5 bytes; the caret must land on a boundary.
        let mut ed = editor("áéb\nxyz", 9);
        ed.move_up();
        assert!(ed.text.is_char_boundary(ed.cursor));
        assert_eq!(ed.cursor, 5);
    }

    #[test]
    fn test_measure_text_block_uses_longest_line_not_total_length() {
        let one = measure_text_block("aaaaaaaaaa", 20.0);
        let three = measure_text_block("aaaaaaaaaa\nbb\ncc", 20.0);
        // Same longest line -> same width, even though the text is 3x longer.
        assert!((one.0 - three.0).abs() < 0.01);
        // Three lines -> three times the height.
        assert!((three.1 - one.1 * 3.0).abs() < 0.01);
    }

    #[test]
    fn test_measure_text_block_empty_still_has_one_line() {
        let (w, h) = measure_text_block("", 20.0);
        assert_eq!(w, 0.0);
        assert!(h > 0.0);
    }
}


/// The property controls for one kind of shape.
///
/// Shared, so the Select tool can borrow whichever set matches what is
/// actually selected instead of showing the last tool used.
fn property_groups(tool: DrawTool) -> Vec<Vec<(FluentAction, f32)>> {
    let mut groups: Vec<Vec<(FluentAction, f32)>> = Vec::with_capacity(4);
    match tool {
        // Everything the Select tool can do to what is already on the canvas.
        // These act on the selection rather than setting a default for the
        // next shape, which is what the other sub-bars do.
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
                (FluentAction::SetArrowHead(ArrowHead::Triangle), 30.0),
                (FluentAction::SetArrowHead(ArrowHead::Open), 30.0),
                (FluentAction::SetArrowHead(ArrowHead::Circle), 30.0),
                (FluentAction::SetArrowHead(ArrowHead::Diamond), 30.0),
                (FluentAction::SetArrowHead(ArrowHead::Bar), 30.0),
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
            groups.push(vec![(FluentAction::ResetBadgeCounter, 54.0)]);
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
                (
                    FluentAction::SetTextCardStyle(TextCardStyle::Transparent),
                    44.0,
                ),
                (FluentAction::SetTextCardStyle(TextCardStyle::Badge), 48.0),
                (FluentAction::SetTextCardStyle(TextCardStyle::Solid), 46.0),
            ]);
            groups.push(vec![
                (FluentAction::SetFontFamily(TextFontFamily::SegoeUI), 42.0),
                (
                    FluentAction::SetFontFamily(TextFontFamily::CascadiaCode),
                    46.0,
                ),
            ]);
        }
        DrawTool::Blur => {
            groups.push(vec![
                (FluentAction::SetStrokeWidth(8.0), 38.0),
                (FluentAction::SetStrokeWidth(14.0), 44.0),
                (FluentAction::SetStrokeWidth(22.0), 44.0),
                (FluentAction::SetStrokeWidth(32.0), 44.0),
            ]);
        }
        _ => {}
    }
    groups
}

/// The commands that only make sense on something already drawn.
///
/// Unlike a property control, none of these has a "default for the next
/// shape" meaning: you cannot pre-set *align*.
fn select_command_groups() -> Vec<Vec<(FluentAction, f32)>> {
    let mut groups: Vec<Vec<(FluentAction, f32)>> = Vec::with_capacity(6);
            groups.push(vec![
                (FluentAction::Align(AlignTo::Left), 30.0),
                (FluentAction::Align(AlignTo::HCentre), 30.0),
                (FluentAction::Align(AlignTo::Right), 30.0),
                (FluentAction::Align(AlignTo::Top), 30.0),
                (FluentAction::Align(AlignTo::VCentre), 30.0),
                (FluentAction::Align(AlignTo::Bottom), 30.0),
            ]);
            groups.push(vec![
                (FluentAction::Distribute(true), 30.0),
                (FluentAction::Distribute(false), 30.0),
            ]);
            groups.push(vec![
                (FluentAction::Restack(false), 30.0),
                (FluentAction::Restack(true), 30.0),
            ]);
            groups.push(vec![
                (FluentAction::SetGroup(true), 30.0),
                (FluentAction::SetGroup(false), 30.0),
            ]);
            groups.push(vec![
                (FluentAction::AdjustOpacity(-0.1), 30.0),
                (FluentAction::AdjustOpacity(0.1), 30.0),
            ]);
            groups.push(vec![
                (FluentAction::Duplicate, 30.0),
                (FluentAction::DeleteSelection, 30.0),
            ]);
    groups
}
