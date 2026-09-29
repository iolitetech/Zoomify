//! Flatten the annotation layer to a standalone SVG file.
//!
//! This is deliberately a second implementation of "how to draw a `Shape`",
//! not a refactor of `renderer/shapes.rs` to share one — the renderer targets
//! a live Direct2D context and this targets a text format a designer can open
//! in Illustrator or Inkscape years from now, and coupling those together
//! would make either one harder to change for the other's sake. Where the
//! renderer's geometry helpers in `shapes.rs` are already pure (curve
//! control points, arrow head triangles, contained-text layout), this reuses
//! them so the two pictures cannot drift apart on the actual math.
//!
//! Text sizing is the one thing this cannot compute itself — that needs a
//! live DirectWrite layout. Callers pass in a `(width, height)` per text
//! annotation, measured the same way the renderer measures it before it ever
//! reaches [`build_svg`], which is what keeps this module a pure function
//! `overlay.rs` can unit test around without a window.

use crate::shapes::{
    arrow_head_points, arrow_head_size, arrow_shaft, contained_text_origin, curve_control,
    label_rides_on_shape, normalize_rect, points_to_bezier_segments, shape_bounds,
};
use crate::types::{
    Annotation, ArrowHead, ArrowStyle, BadgeShape, CanvasBackground, ColorPreset, FillMode,
    Point2D, Shape, ShapeId, StrokePattern, TextCardStyle, TextFontFamily,
};
use std::collections::HashMap;
use std::fmt::Write as _;

/// Everything [`build_svg`] needs, gathered by the caller so this stays a
/// pure function of its inputs.
pub struct SvgExportInput<'a> {
    /// Canvas size in DIPs — the same space every `Shape`'s points live in.
    pub logical_w: f32,
    pub logical_h: f32,
    /// The live static-zoom transform, baked in so the export matches what
    /// Save/Copy currently show rather than the un-zoomed canvas.
    pub zoom_level: f32,
    pub view_x: f32,
    pub view_y: f32,
    pub bg_type: CanvasBackground,
    /// Top-down BGRA at native screen resolution — `None` in Whiteboard or
    /// Blackboard mode, which paint a flat colour and carry no capture.
    pub bg_pixels: Option<&'a [u8]>,
    pub bg_px_w: u32,
    pub bg_px_h: u32,
    /// Physical pixels per DIP, to map canvas-space rects onto `bg_pixels`.
    pub dpi_scale: f32,
    pub shapes: &'a [Annotation],
    /// `(block_w, block_h)` for every `Shape::Text`, DIP-measured exactly as
    /// the renderer measures it. A missing entry falls back to a rough
    /// character-count estimate rather than dropping the label.
    pub text_layout: &'a HashMap<ShapeId, (f32, f32)>,
}

pub fn build_svg(input: &SvgExportInput) -> String {
    let mut out = String::with_capacity(4096);
    let _ = write!(
        out,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" \
width="{w}" height="{h}" viewBox="0 0 {w} {h}">
"#,
        w = FNum(input.logical_w),
        h = FNum(input.logical_h),
    );

    // Everything — background included — sits under the same zoom transform
    // the live overlay applies, so the export matches what Save/Copy show.
    let z = input.zoom_level.max(1.0);
    // A 1x view can still be panned (infinite canvas / whiteboard drag), so
    // gate on the pan offset too - PNG export always applies this transform
    // and SVG must match it even when zoom itself is at rest.
    let panned = input.view_x.abs() > 0.001 || input.view_y.abs() > 0.001;
    if z > 1.001 || panned {
        let _ = write!(
            out,
            r#"<g transform="matrix({a} 0 0 {a} {e} {f})">"#,
            a = FNum(z),
            e = FNum(-input.view_x * z),
            f = FNum(-input.view_y * z),
        );
    } else {
        out.push_str("<g>");
    }

    write_background(&mut out, input);

    for a in input.shapes.iter().filter(|a| a.container.is_none()) {
        write_annotation(&mut out, a, input);
    }
    for a in input.shapes.iter() {
        let Some(cid) = a.container else { continue };
        let Some(owner) = input.shapes.iter().find(|o| o.id == cid) else {
            continue;
        };
        let bounds = shape_bounds(&owner.shape);
        let rides = label_rides_on_shape(&owner.shape);
        write_contained_text(&mut out, a, bounds, rides, input.text_layout);
    }

    out.push_str("</g></svg>\n");
    out
}

fn write_background(out: &mut String, input: &SvgExportInput) {
    match input.bg_type {
        CanvasBackground::Whiteboard => {
            let _ = write!(
                out,
                r##"<rect x="0" y="0" width="{w}" height="{h}" fill="#fafafc"/>"##,
                w = FNum(input.logical_w),
                h = FNum(input.logical_h),
            );
        }
        CanvasBackground::Blackboard => {
            let _ = write!(
                out,
                r##"<rect x="0" y="0" width="{w}" height="{h}" fill="#1c1e24"/>"##,
                w = FNum(input.logical_w),
                h = FNum(input.logical_h),
            );
        }
        CanvasBackground::Transparent => {
            if let Some(pixels) = input.bg_pixels
                && let Some(uri) = png_data_uri(pixels, input.bg_px_w, input.bg_px_h)
            {
                let _ = write!(
                    out,
                    r#"<image x="0" y="0" width="{w}" height="{h}" href="{uri}"/>"#,
                    w = FNum(input.logical_w),
                    h = FNum(input.logical_h),
                );
            }
        }
    }
}

fn write_annotation(out: &mut String, a: &Annotation, input: &SvgExportInput) {
    let _ = write!(out, r#"<g opacity="{}">"#, FNum(a.opacity.clamp(0.0, 1.0)));
    match &a.shape {
        Shape::Stroke { .. } => write_stroke(out, &a.shape),
        Shape::Line { .. } => write_line(out, &a.shape),
        Shape::Arrow { .. } => write_arrow(out, &a.shape),
        Shape::Rectangle { .. } => write_rectangle(out, &a.shape),
        Shape::Ellipse { .. } => write_ellipse(out, &a.shape),
        Shape::Text { .. } => write_text(out, a.id, &a.shape, input.text_layout),
        Shape::StepBadge { .. } => write_step_badge(out, &a.shape),
        Shape::Blur { .. } => write_blur(out, &a.shape, input),
        Shape::Image { start, end, pixels } => {
            let (l, t, r, b) = normalize_rect(*start, *end);
            if let Some(uri) = png_data_uri(&pixels.bgra, pixels.width, pixels.height) {
                let _ = write!(
                    out,
                    r#"<image x="{x}" y="{y}" width="{w}" height="{h}" href="{uri}"/>"#,
                    x = FNum(l),
                    y = FNum(t),
                    w = FNum(r - l),
                    h = FNum(b - t),
                );
            }
        }
    }
    out.push_str("</g>");
}

fn write_stroke(out: &mut String, shape: &Shape) {
    let Shape::Stroke {
        points,
        color,
        width,
        is_highlighter,
        pattern,
        ..
    } = shape
    else {
        return;
    };
    if points.is_empty() {
        return;
    }
    let alpha = if *is_highlighter { 0.45 } else { 1.0 };
    let w = if *is_highlighter {
        *width * 2.2
    } else {
        *width
    };
    if points.len() == 1 {
        let _ = write!(
            out,
            r#"<circle cx="{x}" cy="{y}" r="{r}" fill="{c}" fill-opacity="{a}"/>"#,
            x = FNum(points[0].x),
            y = FNum(points[0].y),
            r = FNum(w / 2.0),
            c = hex(*color),
            a = FNum(alpha),
        );
        return;
    }
    let d = path_d(points);
    let _ = write!(
        out,
        r#"<path d="{d}" fill="none" stroke="{c}" stroke-opacity="{a}" stroke-width="{w}" stroke-linecap="round" stroke-linejoin="round"{dash}/>"#,
        c = hex(*color),
        a = FNum(alpha),
        w = FNum(w),
        dash = dash_attr(*pattern, w),
    );
}

/// Cubic-bezier path through points, matching `points_to_bezier_segments` —
/// a straight `M x y L x y` for two points, the same Catmull-Rom-derived
/// curve the renderer draws for three or more.
fn path_d(points: &[Point2D]) -> String {
    let mut d = format!("M {} {}", FNum(points[0].x), FNum(points[0].y));
    if points.len() >= 3 {
        for (c1, c2, p) in points_to_bezier_segments(points) {
            let _ = write!(
                d,
                " C {} {} {} {} {} {}",
                FNum(c1.x),
                FNum(c1.y),
                FNum(c2.x),
                FNum(c2.y),
                FNum(p.x),
                FNum(p.y)
            );
        }
    } else {
        for p in &points[1..] {
            let _ = write!(d, " L {} {}", FNum(p.x), FNum(p.y));
        }
    }
    d
}

fn write_line(out: &mut String, shape: &Shape) {
    let Shape::Line {
        start,
        end,
        color,
        width,
        pattern,
        curve,
    } = shape
    else {
        return;
    };
    let d = curved_d(*start, *end, *curve);
    let _ = write!(
        out,
        r#"<path d="{d}" fill="none" stroke="{c}" stroke-width="{w}" stroke-linecap="round"{dash}/>"#,
        c = hex(*color),
        w = FNum(*width),
        dash = dash_attr(*pattern, *width),
    );
}

/// `M`+`L`, or `M`+`Q` through the same quadratic control point the renderer
/// bows the curve around — exact, not a polyline approximation.
fn curved_d(start: Point2D, end: Point2D, curve: f32) -> String {
    if curve.abs() < 0.01 {
        format!(
            "M {} {} L {} {}",
            FNum(start.x),
            FNum(start.y),
            FNum(end.x),
            FNum(end.y)
        )
    } else {
        let c = curve_control(start, end, curve);
        format!(
            "M {} {} Q {} {} {} {}",
            FNum(start.x),
            FNum(start.y),
            FNum(c.x),
            FNum(c.y),
            FNum(end.x),
            FNum(end.y)
        )
    }
}

fn write_arrow(out: &mut String, shape: &Shape) {
    let Shape::Arrow {
        start,
        end,
        color,
        width,
        style,
        pattern,
        head,
        curve,
    } = shape
    else {
        return;
    };
    let length = start.distance(end);
    let (head_len, half_width) = arrow_head_size(*width, length);
    let head_at_start = *style == ArrowStyle::Double || *style == ArrowStyle::Dimension;
    let col = hex(*color);

    // The shaft is drawn full-length; a filled head painted on top hides the
    // rounded cap poking past the point. Matches the renderer's `hides_shaft`
    // choice for which head styles need that trim — an SVG stroke's round
    // cap is a static shape either way, so trimming and overlapping reads
    // the same as trimming-then-abutting would.
    let shaft_inset = if matches!(head, ArrowHead::Triangle | ArrowHead::Diamond) {
        head_len * 0.92
    } else {
        0.0
    };
    if let Some((s, e)) = arrow_shaft(*start, *end, head_len, head_at_start, true) {
        let _ = write!(
            out,
            r#"<path d="{d}" fill="none" stroke="{c}" stroke-width="{w}" stroke-linecap="round"{dash}/>"#,
            d = if curve.abs() < 0.01 {
                format!(
                    "M {} {} L {} {}",
                    FNum(s.x),
                    FNum(s.y),
                    FNum(e.x),
                    FNum(e.y)
                )
            } else {
                // A curved arrow's shaft follows the same bow as its chord;
                // reusing the curve control point keeps it on the arc rather
                // than cutting the corner between the inset shaft ends.
                curved_d(s, e, *curve)
            },
            c = col,
            w = FNum(*width),
            dash = dash_attr(*pattern, *width),
        );
    }
    let _ = shaft_inset; // documents the D2D parity note above; SVG needs no numeric use of it

    let (aim_end, aim_start) = if curve.abs() < 0.01 {
        (*start, *end)
    } else {
        // The tangent at each end of the quadratic, not the chord — matters
        // once the bow is deep enough that the head would otherwise point
        // the wrong way.
        let c = curve_control(*start, *end, *curve);
        (quad_lerp(c, *end, 0.1), quad_lerp(c, *start, 0.1))
    };
    write_arrow_head(
        out, aim_end, *end, head_len, half_width, *head, &col, *width,
    );
    if head_at_start {
        write_arrow_head(
            out, aim_start, *start, head_len, half_width, *head, &col, *width,
        );
    }

    if *style == ArrowStyle::Dimension && length > 1.0 {
        let ux = (end.x - start.x) / length;
        let uy = (end.y - start.y) / length;
        let (px, py) = (-uy, ux);
        let tick = half_width * 1.1;
        for p in [start, end] {
            let _ = write!(
                out,
                r#"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{c}" stroke-width="{w}"/>"#,
                x1 = FNum(p.x + px * tick),
                y1 = FNum(p.y + py * tick),
                x2 = FNum(p.x - px * tick),
                y2 = FNum(p.y - py * tick),
                c = col,
                w = FNum(*width * 1.2),
            );
        }
    }
}

/// A point a small step from `c` toward `towards`, standing in for the
/// tangent direction at the curve's end — cheap and close enough for aiming
/// an arrowhead; it need not be the true derivative.
fn quad_lerp(c: Point2D, towards: Point2D, t: f32) -> Point2D {
    Point2D::new(c.x + (towards.x - c.x) * t, c.y + (towards.y - c.y) * t)
}

#[allow(clippy::too_many_arguments)]
fn write_arrow_head(
    out: &mut String,
    from: Point2D,
    to: Point2D,
    head_len: f32,
    half_width: f32,
    head: ArrowHead,
    col: &str,
    width: f32,
) {
    let (tip, left, right) = arrow_head_points(from, to, head_len, half_width);
    match head {
        ArrowHead::Triangle => {
            let _ = write!(
                out,
                r#"<polygon points="{},{} {},{} {},{}" fill="{c}"/>"#,
                FNum(tip.x),
                FNum(tip.y),
                FNum(left.x),
                FNum(left.y),
                FNum(right.x),
                FNum(right.y),
                c = col,
            );
        }
        ArrowHead::Open => {
            let _ = write!(
                out,
                r#"<polyline points="{},{} {},{} {},{}" fill="none" stroke="{c}" stroke-width="{w}" stroke-linecap="round"/>"#,
                FNum(left.x),
                FNum(left.y),
                FNum(tip.x),
                FNum(tip.y),
                FNum(right.x),
                FNum(right.y),
                c = col,
                w = FNum(width),
            );
        }
        ArrowHead::Circle => {
            let r = half_width * 0.8;
            let ux = tip.x - (left.x + right.x) * 0.5;
            let uy = tip.y - (left.y + right.y) * 0.5;
            let len = (ux * ux + uy * uy).sqrt().max(0.001);
            let _ = write!(
                out,
                r#"<circle cx="{x}" cy="{y}" r="{r}" fill="{c}"/>"#,
                x = FNum(tip.x - ux / len * r),
                y = FNum(tip.y - uy / len * r),
                r = FNum(r),
                c = col,
            );
        }
        ArrowHead::Diamond => {
            let mid = Point2D::new((left.x + right.x) * 0.5, (left.y + right.y) * 0.5);
            let back = Point2D::new(mid.x - (tip.x - mid.x), mid.y - (tip.y - mid.y));
            let _ = write!(
                out,
                r#"<polygon points="{},{} {},{} {},{} {},{}" fill="{c}"/>"#,
                FNum(tip.x),
                FNum(tip.y),
                FNum(left.x),
                FNum(left.y),
                FNum(back.x),
                FNum(back.y),
                FNum(right.x),
                FNum(right.y),
                c = col,
            );
        }
        ArrowHead::Bar => {
            let _ = write!(
                out,
                r#"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{c}" stroke-width="{w}"/>"#,
                x1 = FNum(tip.x + (left.x - right.x) * 0.5),
                y1 = FNum(tip.y + (left.y - right.y) * 0.5),
                x2 = FNum(tip.x - (left.x - right.x) * 0.5),
                y2 = FNum(tip.y - (left.y - right.y) * 0.5),
                c = col,
                w = FNum(width * 1.3),
            );
        }
    }
}

fn write_rectangle(out: &mut String, shape: &Shape) {
    let Shape::Rectangle {
        start,
        end,
        color,
        width,
        rounded,
        fill,
        pattern,
    } = shape
    else {
        return;
    };
    let (l, t, r, b) = normalize_rect(*start, *end);
    let rx = if *rounded { 12.0 } else { 0.0 };
    let (fill_col, fill_op) = fill_attrs(*fill, *color);
    let _ = write!(
        out,
        r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" fill="{fc}" fill-opacity="{fo}" stroke="{c}" stroke-width="{sw}"{dash}/>"#,
        x = FNum(l),
        y = FNum(t),
        w = FNum(r - l),
        h = FNum(b - t),
        rx = FNum(rx),
        fc = fill_col,
        fo = FNum(fill_op),
        c = hex(*color),
        sw = FNum(*width),
        dash = dash_attr(*pattern, *width),
    );
}

fn write_ellipse(out: &mut String, shape: &Shape) {
    let Shape::Ellipse {
        start,
        end,
        color,
        width,
        fill,
        pattern,
    } = shape
    else {
        return;
    };
    let (l, t, r, b) = normalize_rect(*start, *end);
    let (fill_col, fill_op) = fill_attrs(*fill, *color);
    let _ = write!(
        out,
        r#"<ellipse cx="{cx}" cy="{cy}" rx="{rx}" ry="{ry}" fill="{fc}" fill-opacity="{fo}" stroke="{c}" stroke-width="{sw}"{dash}/>"#,
        cx = FNum((l + r) / 2.0),
        cy = FNum((t + b) / 2.0),
        rx = FNum((r - l) / 2.0),
        ry = FNum((b - t) / 2.0),
        fc = fill_col,
        fo = FNum(fill_op),
        c = hex(*color),
        sw = FNum(*width),
        dash = dash_attr(*pattern, *width),
    );
}

/// `(fill colour, fill-opacity)` for a fill mode — `fill-opacity="0"` for
/// `None` rather than omitting the attribute, so callers can always emit it.
fn fill_attrs(fill: FillMode, color: ColorPreset) -> (String, f32) {
    match fill {
        FillMode::None => (hex(color), 0.0),
        FillMode::Tinted => (hex(color), 0.22),
        FillMode::Solid => (hex(color), 1.0),
    }
}

fn write_text(out: &mut String, id: ShapeId, shape: &Shape, layout: &HashMap<ShapeId, (f32, f32)>) {
    let Shape::Text {
        origin,
        text,
        font_size,
        color,
        is_bold,
        is_italic,
        card_style,
        font_family,
    } = shape
    else {
        return;
    };
    if text.is_empty() {
        return;
    }
    let (block_w, block_h) = layout
        .get(&id)
        .copied()
        .unwrap_or_else(|| estimate_text_size(text, *font_size));
    let layout_w = block_w.max(30.0) + 16.0;
    let layout_h = block_h + 8.0;

    match card_style {
        TextCardStyle::Badge => {
            write_card_rect(
                out,
                *origin,
                layout_w,
                layout_h,
                "#141820",
                0.65,
                Some("#ffffff"),
            );
        }
        TextCardStyle::Solid => {
            write_card_rect(
                out,
                *origin,
                layout_w,
                layout_h,
                "#1f222b",
                0.96,
                Some(&hex(*color)),
            );
        }
        TextCardStyle::Transparent => {
            // A soft drop shadow rather than a card: an offset copy of the
            // text in translucent black behind the real glyphs.
            write_text_run(
                out,
                Point2D::new(origin.x + 1.2, origin.y + 1.5),
                text,
                *font_size,
                *is_bold,
                *is_italic,
                *font_family,
                "#000000",
                0.70,
            );
        }
    }
    write_text_run(
        out,
        *origin,
        text,
        *font_size,
        *is_bold,
        *is_italic,
        *font_family,
        &hex(*color),
        1.0,
    );
}

fn write_card_rect(
    out: &mut String,
    origin: Point2D,
    w: f32,
    h: f32,
    fill: &str,
    fill_op: f32,
    border: Option<&str>,
) {
    let _ = write!(
        out,
        r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="6" fill="{fill}" fill-opacity="{fo}""#,
        x = FNum(origin.x - 6.0),
        y = FNum(origin.y - 4.0),
        w = FNum(w + 12.0),
        h = FNum(h + 6.0),
        fo = FNum(fill_op),
    );
    if let Some(b) = border {
        let _ = write!(out, r#" stroke="{b}" stroke-width="1.5""#);
    }
    out.push_str("/>");
}

#[allow(clippy::too_many_arguments)]
fn write_text_run(
    out: &mut String,
    origin: Point2D,
    text: &str,
    font_size: f32,
    is_bold: bool,
    is_italic: bool,
    font_family: TextFontFamily,
    fill: &str,
    fill_op: f32,
) {
    let _ = write!(
        out,
        r#"<text x="{x}" y="{y}" xml:space="preserve" font-family="{ff}" font-size="{fs}" font-weight="{fw}" font-style="{fs2}" fill="{fill}" fill-opacity="{fo}" style="dominant-baseline:hanging">"#,
        x = FNum(origin.x),
        y = FNum(origin.y),
        ff = font_family_css(font_family),
        fs = FNum(font_size),
        fw = if is_bold { 700 } else { 600 },
        fs2 = if is_italic { "italic" } else { "normal" },
        fill = fill,
        fo = FNum(fill_op),
    );
    for (i, line) in text.split('\n').enumerate() {
        if i == 0 {
            let _ = write!(
                out,
                r#"<tspan x="{x}" dy="0">{t}</tspan>"#,
                x = FNum(origin.x),
                t = esc(line)
            );
        } else {
            let _ = write!(
                out,
                r#"<tspan x="{x}" dy="{dy}">{t}</tspan>"#,
                x = FNum(origin.x),
                dy = FNum(font_size * 1.25),
                t = esc(line),
            );
        }
    }
    out.push_str("</text>");
}

fn font_family_css(f: TextFontFamily) -> &'static str {
    match f {
        TextFontFamily::SegoeUI => "'Segoe UI Variable Display','Segoe UI',sans-serif",
        TextFontFamily::CascadiaCode => "'Cascadia Code','Consolas',monospace",
        TextFontFamily::SegoePrint => "'Segoe Print','Comic Sans MS',cursive",
    }
}

/// Rough fallback layout for a `Shape::Text` the caller didn't measure —
/// keeps a label from vanishing rather than matching pixel-for-pixel.
fn estimate_text_size(text: &str, font_size: f32) -> (f32, f32) {
    let widest = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
    let lines = text.lines().count().max(1);
    (
        widest as f32 * font_size * 0.56,
        lines as f32 * font_size * 1.25,
    )
}

fn write_contained_text(
    out: &mut String,
    a: &Annotation,
    container: (f32, f32, f32, f32),
    rides_on: bool,
    layout: &HashMap<ShapeId, (f32, f32)>,
) {
    let Shape::Text {
        text,
        font_size,
        color,
        is_bold,
        is_italic,
        font_family,
        ..
    } = &a.shape
    else {
        return;
    };
    if text.is_empty() {
        return;
    }
    let (w, h) = layout
        .get(&a.id)
        .copied()
        .unwrap_or_else(|| estimate_text_size(text, *font_size));
    let origin = contained_text_origin(container, w, h);

    let _ = write!(out, r#"<g opacity="{}">"#, FNum(a.opacity.clamp(0.0, 1.0)));
    if rides_on {
        let _ = write!(
            out,
            r##"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="4" fill="#1a1c24" fill-opacity="0.88"/>"##,
            x = FNum(origin.x - 6.0),
            y = FNum(origin.y - 3.0),
            w = FNum(w + 12.0),
            h = FNum(h + 6.0),
        );
    }
    write_text_run(
        out,
        origin,
        text,
        *font_size,
        *is_bold,
        *is_italic,
        *font_family,
        &hex(*color),
        1.0,
    );
    out.push_str("</g>");
}

fn write_step_badge(out: &mut String, shape: &Shape) {
    let Shape::StepBadge {
        center,
        number,
        radius,
        color,
        shape: badge_shape,
        fill,
        stroke_width,
        pattern,
    } = shape
    else {
        return;
    };
    let border_w = stroke_width.max(1.0);
    let (fill_col, fill_op) = fill_attrs(*fill, *color);
    let backplate = if *fill == FillMode::Tinted {
        Some("#141a24")
    } else {
        None
    };

    match badge_shape {
        BadgeShape::Circle => {
            if let Some(bp) = backplate {
                let _ = write!(
                    out,
                    r#"<circle cx="{x}" cy="{y}" r="{r}" fill="{bp}" fill-opacity="0.70"/>"#,
                    x = FNum(center.x),
                    y = FNum(center.y),
                    r = FNum(*radius),
                );
            }
            let _ = write!(
                out,
                r#"<circle cx="{x}" cy="{y}" r="{r}" fill="{fc}" fill-opacity="{fo}" stroke="{c}" stroke-width="{sw}"{dash}/>"#,
                x = FNum(center.x),
                y = FNum(center.y),
                r = FNum(*radius),
                fc = fill_col,
                fo = FNum(fill_op),
                c = hex(*color),
                sw = FNum(border_w),
                dash = dash_attr(*pattern, border_w),
            );
        }
        BadgeShape::Square => {
            if let Some(bp) = backplate {
                let _ = write!(
                    out,
                    r#"<rect x="{x}" y="{y}" width="{s}" height="{s}" rx="6" fill="{bp}" fill-opacity="0.70"/>"#,
                    x = FNum(center.x - radius),
                    y = FNum(center.y - radius),
                    s = FNum(radius * 2.0),
                );
            }
            let _ = write!(
                out,
                r#"<rect x="{x}" y="{y}" width="{s}" height="{s}" rx="6" fill="{fc}" fill-opacity="{fo}" stroke="{c}" stroke-width="{sw}"{dash}/>"#,
                x = FNum(center.x - radius),
                y = FNum(center.y - radius),
                s = FNum(radius * 2.0),
                fc = fill_col,
                fo = FNum(fill_op),
                c = hex(*color),
                sw = FNum(border_w),
                dash = dash_attr(*pattern, border_w),
            );
        }
        BadgeShape::Hexagon => {
            let pts = hexagon_points(*center, *radius);
            if let Some(bp) = backplate {
                let _ = write!(
                    out,
                    r#"<polygon points="{pts}" fill="{bp}" fill-opacity="0.70"/>"#
                );
            }
            let _ = write!(
                out,
                r#"<polygon points="{pts}" fill="{fc}" fill-opacity="{fo}" stroke="{c}" stroke-width="{sw}"{dash}/>"#,
                fc = fill_col,
                fo = FNum(fill_op),
                c = hex(*color),
                sw = FNum(border_w),
                dash = dash_attr(*pattern, border_w),
            );
        }
    }

    let text_col = match fill {
        FillMode::Solid => match color {
            ColorPreset::Yellow | ColorPreset::Cyan | ColorPreset::White => "#1a1c26",
            _ => "#ffffff",
        },
        FillMode::Tinted => "#ffffff",
        FillMode::None => &hex(*color),
    };
    let font_size = (*radius * 0.95).max(11.0);
    let _ = write!(
        out,
        r#"<text x="{x}" y="{y}" font-family="'Segoe UI Variable Display','Segoe UI',sans-serif" font-size="{fs}" font-weight="700" fill="{c}" text-anchor="middle" style="dominant-baseline:central">{n}</text>"#,
        x = FNum(center.x),
        y = FNum(center.y),
        fs = FNum(font_size),
        c = text_col,
        n = number,
    );
}

fn hexagon_points(center: Point2D, radius: f32) -> String {
    let mut s = String::new();
    for i in 0..6 {
        let angle = (i as f32 * std::f32::consts::PI / 3.0) - std::f32::consts::FRAC_PI_2;
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(
            s,
            "{},{}",
            FNum(center.x + radius * angle.cos()),
            FNum(center.y + radius * angle.sin())
        );
    }
    s
}

fn write_blur(out: &mut String, shape: &Shape, input: &SvgExportInput) {
    let Shape::Blur {
        start,
        end,
        block_size,
    } = shape
    else {
        return;
    };
    let (l, t, r, b) = normalize_rect(*start, *end);
    let w = r - l;
    let h = b - t;
    if w < 1.0 || h < 1.0 {
        return;
    }
    // input.bg_pixels is None on a Whiteboard/Blackboard (see export_svg()),
    // since that frozen screenshot is exactly what the user covered the
    // desktop to hide - Blur must not mosaic a pixelated copy of it. Falls
    // back to the same flat redaction block the live renderer and PNG
    // export draw when there is nothing to mosaic, rather than silently
    // omitting the shape.
    let Some(bg) = input.bg_pixels else {
        write_blur_fallback_block(out, l, t, r, b);
        return;
    };
    let b_size = block_size.clamp(4.0, 64.0);
    // start/end come straight from a Shape::Blur, which a crafted or
    // corrupted session file controls - w/h (and so cols/rows) are not
    // otherwise bounded by anything real like a screen size. Capped well
    // past what any real redaction needs, so a garbage rect cannot demand
    // a many-gigapixel mosaic or overflow the pixel-buffer arithmetic
    // below.
    let cols = ((w / b_size).ceil().max(1.0) as u32).min(4096);
    let rows = ((h / b_size).ceil().max(1.0) as u32).min(4096);

    let Some(mosaic) = mosaic_tiles(
        bg,
        input.bg_px_w,
        input.bg_px_h,
        l,
        t,
        w,
        h,
        input.dpi_scale,
        cols,
        rows,
    ) else {
        write_blur_fallback_block(out, l, t, r, b);
        return;
    };
    if let Some(uri) = png_data_uri(&mosaic, cols, rows) {
        // `image-rendering: pixelated` upsamples the tiny tile grid with
        // nearest-neighbour, the same look as the renderer's own
        // downsample-then-upsample mosaic.
        let _ = write!(
            out,
            r#"<image x="{x}" y="{y}" width="{w}" height="{h}" href="{uri}" style="image-rendering:pixelated" preserveAspectRatio="none"/>"#,
            x = FNum(l),
            y = FNum(t),
            w = FNum(w),
            h = FNum(h),
        );
    }
}

/// The same flat redaction block the live renderer and PNG export fall back
/// to when there is no background to mosaic.
fn write_blur_fallback_block(out: &mut String, l: f32, t: f32, r: f32, b: f32) {
    let _ = write!(
        out,
        r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="rgba(26,26,26,0.85)" stroke="rgba(102,179,255,0.45)" stroke-width="1"/>"#,
        x = FNum(l),
        y = FNum(t),
        w = FNum(r - l),
        h = FNum(b - t),
    );
}

/// Box-average `bg` (top-down BGRA at `bg_w`x`bg_h` physical pixels) over the
/// DIP rect `(x, y, w, h)` into a `cols`x`rows` top-down BGRA tile grid.
#[allow(clippy::too_many_arguments)]
fn mosaic_tiles(
    bg: &[u8],
    bg_w: u32,
    bg_h: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    dpi_scale: f32,
    cols: u32,
    rows: u32,
) -> Option<Vec<u8>> {
    if bg_w == 0 || bg_h == 0 || bg.len() < (bg_w as usize * bg_h as usize * 4) {
        return None;
    }
    let px_x = (x * dpi_scale).max(0.0);
    let px_y = (y * dpi_scale).max(0.0);
    let px_w = (w * dpi_scale).max(1.0);
    let px_h = (h * dpi_scale).max(1.0);

    // Defense in depth alongside write_blur()'s cap on cols/rows: reject
    // rather than silently wrap if this is ever called with cols/rows large
    // enough (both near u32::MAX) to overflow the pixel-count arithmetic.
    let total = (cols as usize).checked_mul(rows as usize)?.checked_mul(4)?;
    let mut out = vec![0u8; total];
    for row in 0..rows {
        for col in 0..cols {
            let tile_l = (px_x + px_w * col as f32 / cols as f32).round() as i64;
            let tile_r = (px_x + px_w * (col + 1) as f32 / cols as f32).round() as i64;
            let tile_t = (px_y + px_h * row as f32 / rows as f32).round() as i64;
            let tile_b = (px_y + px_h * (row + 1) as f32 / rows as f32).round() as i64;

            let mut sum = [0u64; 4];
            let mut n = 0u64;
            for py in tile_t.max(0)..tile_b.min(bg_h as i64) {
                for px in tile_l.max(0)..tile_r.min(bg_w as i64) {
                    let idx = (py as usize * bg_w as usize + px as usize) * 4;
                    if idx + 3 < bg.len() {
                        sum[0] += bg[idx] as u64;
                        sum[1] += bg[idx + 1] as u64;
                        sum[2] += bg[idx + 2] as u64;
                        sum[3] += bg[idx + 3] as u64;
                        n += 1;
                    }
                }
            }
            let out_idx = (row * cols + col) as usize * 4;
            #[allow(clippy::manual_checked_ops)]
            if n > 0 {
                out[out_idx] = (sum[0] / n) as u8;
                out[out_idx + 1] = (sum[1] / n) as u8;
                out[out_idx + 2] = (sum[2] / n) as u8;
                out[out_idx + 3] = 255;
            }
        }
    }
    Some(out)
}

/// A `data:image/png;base64,...` URI from top-down BGRA.
fn png_data_uri(bgra: &[u8], width: u32, height: u32) -> Option<String> {
    if width == 0 || height == 0 || bgra.len() < (width as usize * height as usize * 4) {
        return None;
    }
    let mut rgba = bgra.to_vec();
    for chunk in rgba.as_chunks_mut::<4>().0 {
        chunk.swap(0, 2);
    }
    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    image::ImageEncoder::write_image(
        encoder,
        &rgba,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )
    .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        crate::types::b64::encode(&png_bytes)
    ))
}

fn dash_attr(pattern: StrokePattern, width: f32) -> String {
    let w = width.max(1.0);
    match pattern {
        StrokePattern::Solid => String::new(),
        StrokePattern::Dashed => {
            format!(r#" stroke-dasharray="{} {}""#, FNum(w * 2.5), FNum(w * 1.8))
        }
        StrokePattern::Dotted => format!(
            r#" stroke-dasharray="0.1 {}" stroke-linecap="round""#,
            FNum(w * 2.0)
        ),
    }
}

fn hex(color: ColorPreset) -> String {
    let (r, g, b) = color.rgb_u8();
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

struct FNum(f32);
impl std::fmt::Display for FNum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.0.is_finite() {
            eprintln!("svg_export: refusing to write non-finite value {}", self.0);
            return write!(f, "0");
        }
        let r = (self.0 * 1000.0).round() / 1000.0;
        if r == r.trunc() {
            write!(f, "{}", r as i64)
        } else {
            write!(f, "{:.3}", r)
        }
    }
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            // XML 1.0 forbids these control characters outright (tab/LF/CR
            // are the only C0 codes it allows) - a pasted control byte would
            // otherwise produce an SVG no browser or editor can parse.
            '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' => {}
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ArrowStyle as AS, StrokePattern as SP};

    fn input<'a>(
        shapes: &'a [Annotation],
        layout: &'a HashMap<ShapeId, (f32, f32)>,
    ) -> SvgExportInput<'a> {
        SvgExportInput {
            logical_w: 400.0,
            logical_h: 300.0,
            zoom_level: 1.0,
            view_x: 0.0,
            view_y: 0.0,
            bg_type: CanvasBackground::Whiteboard,
            bg_pixels: None,
            bg_px_w: 0,
            bg_px_h: 0,
            dpi_scale: 1.0,
            shapes,
            text_layout: layout,
        }
    }

    #[test]
    fn test_build_svg_is_well_formed_and_sized_to_the_canvas() {
        let shapes = vec![Annotation::new(Shape::Rectangle {
            start: Point2D::new(10.0, 10.0),
            end: Point2D::new(100.0, 80.0),
            color: ColorPreset::Red,
            width: 3.0,
            rounded: false,
            fill: FillMode::Solid,
            pattern: SP::Solid,
        })];
        let layout = HashMap::new();
        let svg = build_svg(&input(&shapes, &layout));
        assert!(svg.starts_with("<?xml"));
        assert!(svg.contains(r#"width="400""#));
        assert!(svg.contains(r#"height="300""#));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("<rect"));
        assert!(svg.contains(&hex(ColorPreset::Red)));
    }

    #[test]
    fn test_every_shape_variant_emits_something_without_panicking() {
        let mut layout = HashMap::new();
        let text = Annotation::new(Shape::Text {
            origin: Point2D::new(5.0, 5.0),
            text: "hi\nthere".to_string(),
            font_size: 16.0,
            color: ColorPreset::Black,
            is_bold: false,
            is_italic: false,
            card_style: TextCardStyle::Solid,
            font_family: TextFontFamily::SegoeUI,
        });
        layout.insert(text.id, (40.0, 40.0));

        let shapes = vec![
            Annotation::new(Shape::Stroke {
                points: vec![
                    Point2D::new(0.0, 0.0),
                    Point2D::new(5.0, 5.0),
                    Point2D::new(10.0, 0.0),
                ],
                color: ColorPreset::Blue,
                width: 2.0,
                is_highlighter: false,
                pattern: SP::Dashed,
                pressures: Vec::new(),
            }),
            Annotation::new(Shape::Line {
                start: Point2D::new(0.0, 0.0),
                end: Point2D::new(50.0, 50.0),
                color: ColorPreset::Green,
                width: 2.0,
                pattern: SP::Dotted,
                curve: 20.0,
            }),
            Annotation::new(Shape::Arrow {
                start: Point2D::new(0.0, 0.0),
                end: Point2D::new(80.0, 0.0),
                color: ColorPreset::Orange,
                width: 3.0,
                style: AS::Double,
                pattern: SP::Solid,
                head: ArrowHead::Diamond,
                curve: 0.0,
            }),
            Annotation::new(Shape::Ellipse {
                start: Point2D::new(0.0, 0.0),
                end: Point2D::new(30.0, 20.0),
                color: ColorPreset::Cyan,
                width: 1.0,
                fill: FillMode::Tinted,
                pattern: SP::Solid,
            }),
            text,
            Annotation::new(Shape::StepBadge {
                center: Point2D::new(60.0, 60.0),
                number: 3,
                radius: 14.0,
                color: ColorPreset::Yellow,
                shape: BadgeShape::Hexagon,
                fill: FillMode::Solid,
                stroke_width: 2.0,
                pattern: SP::Solid,
            }),
            Annotation::new(Shape::Blur {
                start: Point2D::new(0.0, 0.0),
                end: Point2D::new(20.0, 20.0),
                block_size: 8.0,
            }),
        ];
        let svg = build_svg(&input(&shapes, &layout));
        // Blur with no background pixels draws the flat redaction fallback
        // rather than panicking or silently vanishing — everything else
        // must have left a mark too.
        assert!(svg.contains("<path"));
        assert!(svg.contains("<polygon"));
        assert!(svg.contains("<ellipse"));
        assert!(svg.contains("<text"));
        assert!(svg.contains("tspan"));
        assert!(svg.contains("rgba(26,26,26,0.85)"));
    }

    #[test]
    fn test_contained_text_is_centred_in_its_owner_and_rendered_after_it() {
        let owner = Annotation::new(Shape::Rectangle {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(100.0, 100.0),
            color: ColorPreset::Blue,
            width: 2.0,
            rounded: false,
            fill: FillMode::None,
            pattern: SP::Solid,
        });
        let mut label = Annotation::new(Shape::Text {
            origin: Point2D::default(),
            text: "centred".to_string(),
            font_size: 14.0,
            color: ColorPreset::White,
            is_bold: false,
            is_italic: false,
            card_style: TextCardStyle::Transparent,
            font_family: TextFontFamily::SegoeUI,
        });
        label.container = Some(owner.id);
        let mut layout = HashMap::new();
        layout.insert(label.id, (40.0, 16.0));

        let shapes = vec![owner, label];
        let svg = build_svg(&input(&shapes, &layout));
        let rect_pos = svg.find("<rect").unwrap();
        let text_pos = svg.find("<text").unwrap();
        assert!(
            rect_pos < text_pos,
            "the container must be drawn before its label"
        );
        // Centred in a 0..100 box with a 40-wide, 16-tall block: (30, 42).
        assert!(svg.contains(r#"x="30""#));
        assert!(svg.contains(r#"y="42""#));
    }

    #[test]
    fn test_zoom_transform_is_only_emitted_above_1x() {
        let shapes: Vec<Annotation> = Vec::new();
        let layout = HashMap::new();
        let mut i = input(&shapes, &layout);
        assert!(!build_svg(&i).contains("matrix"));
        i.zoom_level = 2.0;
        i.view_x = 5.0;
        i.view_y = 5.0;
        let svg = build_svg(&i);
        assert!(svg.contains("matrix(2 0 0 2 -10 -10)"));
    }

    #[test]
    fn test_mosaic_tiles_averages_a_solid_colour_region_to_itself() {
        // A 4x4 solid-red native bitmap, DIP-space rect covering it exactly.
        let mut bg = vec![0u8; 4 * 4 * 4];
        for px in bg.chunks_exact_mut(4) {
            px.copy_from_slice(&[10, 20, 30, 255]);
        }
        let tiles = mosaic_tiles(&bg, 4, 4, 0.0, 0.0, 4.0, 4.0, 1.0, 2, 2).unwrap();
        for px in tiles.chunks_exact(4) {
            assert_eq!(px, &[10, 20, 30, 255]);
        }
    }

    #[test]
    fn test_mosaic_tiles_rejects_a_cols_rows_pair_that_would_overflow() {
        // cols*rows*4 overflows a u64 once both are near u32::MAX; must
        // fail cleanly rather than allocate a wrapped (too-small) buffer
        // and panic indexing into it as if it were cols*rows big.
        let bg = vec![0u8; 4];
        assert!(mosaic_tiles(&bg, 1, 1, 0.0, 0.0, 1.0, 1.0, 1.0, u32::MAX, u32::MAX).is_none());
    }

    #[test]
    fn test_write_blur_with_an_absurd_rect_does_not_panic() {
        // start/end come straight from a Shape::Blur, which a crafted or
        // corrupted session file controls - a garbage rect must fall back
        // to the flat redaction block rather than panic trying to mosaic a
        // many-gigapixel region.
        let bg = vec![0u8; 4 * 4 * 4];
        let mut out = String::new();
        let shape = Shape::Blur {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(1.0e20, 1.0e20),
            block_size: 4.0,
        };
        let shapes: Vec<Annotation> = Vec::new();
        let layout = HashMap::new();
        let mut i = input(&shapes, &layout);
        i.bg_pixels = Some(&bg);
        i.bg_px_w = 4;
        i.bg_px_h = 4;
        write_blur(&mut out, &shape, &i);
        assert!(!out.is_empty());
    }

    #[test]
    fn test_dash_attr_is_empty_for_solid_and_present_otherwise() {
        assert_eq!(dash_attr(StrokePattern::Solid, 3.0), "");
        assert!(dash_attr(StrokePattern::Dashed, 3.0).contains("stroke-dasharray"));
        assert!(dash_attr(StrokePattern::Dotted, 3.0).contains("stroke-dasharray"));
    }

    #[test]
    fn test_xml_escaping_covers_the_five_reserved_characters() {
        assert_eq!(esc("<a & \"b\">"), "&lt;a &amp; &quot;b&quot;&gt;");
    }

    #[test]
    fn test_xml_escaping_strips_illegal_control_characters_but_keeps_tab_lf_cr() {
        let s = "a\u{0}b\u{1}\tc\nd\re\u{1f}f";
        assert_eq!(esc(s), "ab\tc\nd\ref");
    }

    #[test]
    fn test_transform_is_emitted_for_a_pure_pan_at_1x_zoom() {
        let shapes: Vec<Annotation> = Vec::new();
        let layout = HashMap::new();
        let mut i = input(&shapes, &layout);
        i.view_x = 5.0;
        i.view_y = 3.0;
        let svg = build_svg(&i);
        assert!(svg.contains("matrix(1 0 0 1 -5 -3)"));
    }

    #[test]
    fn test_fnum_writes_zero_for_non_finite_values_instead_of_panicking() {
        assert_eq!(FNum(f32::NAN).to_string(), "0");
        assert_eq!(FNum(f32::INFINITY).to_string(), "0");
        assert_eq!(FNum(f32::NEG_INFINITY).to_string(), "0");
    }
}
