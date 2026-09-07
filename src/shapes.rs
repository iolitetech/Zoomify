use crate::types::{
    ColorPreset, DrawTool, FillMode, Point2D, SelectionHandle, Shape, StrokePattern,
};

pub fn snap_to_angle(start: Point2D, current: Point2D) -> Point2D {
    let dx = current.x - start.x;
    let dy = current.y - start.y;
    let dist = (dx * dx + dy * dy).sqrt();

    if dist < 1.0 {
        return current;
    }

    let angle = dy.atan2(dx);
    let step = std::f32::consts::PI / 4.0; // 45 degrees
    let snapped_angle = (angle / step).round() * step;

    Point2D {
        x: start.x + dist * snapped_angle.cos(),
        y: start.y + dist * snapped_angle.sin(),
    }
}

pub fn snap_to_square(start: Point2D, current: Point2D) -> Point2D {
    let dx = current.x - start.x;
    let dy = current.y - start.y;
    let side = dx.abs().max(dy.abs());

    let sign_x = if dx >= 0.0 { 1.0 } else { -1.0 };
    let sign_y = if dy >= 0.0 { 1.0 } else { -1.0 };

    Point2D {
        x: start.x + sign_x * side,
        y: start.y + sign_y * side,
    }
}

pub fn normalize_rect(p1: Point2D, p2: Point2D) -> (f32, f32, f32, f32) {
    let left = p1.x.min(p2.x);
    let top = p1.y.min(p2.y);
    let right = p1.x.max(p2.x);
    let bottom = p1.y.max(p2.y);
    (left, top, right, bottom)
}

pub fn points_to_bezier_segments(points: &[Point2D]) -> Vec<(Point2D, Point2D, Point2D)> {
    if points.len() < 2 {
        return Vec::new();
    }
    if points.len() == 2 {
        let p0 = points[0];
        let p1 = points[1];
        let c1 = Point2D::new(p0.x + (p1.x - p0.x) / 3.0, p0.y + (p1.y - p0.y) / 3.0);
        let c2 = Point2D::new(
            p0.x + 2.0 * (p1.x - p0.x) / 3.0,
            p0.y + 2.0 * (p1.y - p0.y) / 3.0,
        );
        return vec![(c1, c2, p1)];
    }
    let n = points.len();
    let mut segments = Vec::with_capacity(n - 1);
    for i in 0..(n - 1) {
        let p_prev = if i == 0 { points[0] } else { points[i - 1] };
        let p_cur = points[i];
        let p_next = points[i + 1];
        let p_after = if i + 2 < n { points[i + 2] } else { p_next };

        let c1 = Point2D::new(
            p_cur.x + (p_next.x - p_prev.x) / 6.0,
            p_cur.y + (p_next.y - p_prev.y) / 6.0,
        );
        let c2 = Point2D::new(
            p_next.x - (p_after.x - p_cur.x) / 6.0,
            p_next.y - (p_after.y - p_cur.y) / 6.0,
        );
        segments.push((c1, c2, p_next));
    }
    segments
}

pub fn point_to_segment_distance(pt: Point2D, a: Point2D, b: Point2D) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 0.0001 {
        return pt.distance(&a);
    }
    let t = (((pt.x - a.x) * dx + (pt.y - a.y) * dy) / len_sq).clamp(0.0, 1.0);
    let proj = Point2D::new(a.x + t * dx, a.y + t * dy);
    pt.distance(&proj)
}

pub fn shape_intersects_circle(shape: &Shape, center: Point2D, radius: f32) -> bool {
    match shape {
        Shape::Stroke { points, width, .. } => {
            let threshold = radius + *width / 2.0;
            if points.len() == 1 {
                return points[0].distance(&center) <= threshold;
            }
            for i in 0..(points.len().saturating_sub(1)) {
                if point_to_segment_distance(center, points[i], points[i + 1]) <= threshold {
                    return true;
                }
            }
            false
        }
        Shape::Line {
            start,
            end,
            width,
            curve,
            ..
        }
        | Shape::Arrow {
            start,
            end,
            width,
            curve,
            ..
        } => {
            let threshold = radius + *width / 2.0;
            // Walk the arc; on a straight line this is the single chord.
            let pts = sample_curve(*start, *end, *curve);
            pts.windows(2)
                .any(|w| point_to_segment_distance(center, w[0], w[1]) <= threshold)
        }
        Shape::Rectangle {
            start, end, width, ..
        } => {
            let (l, t, r, b) = normalize_rect(*start, *end);
            let threshold = radius + *width / 2.0;
            if center.x >= l && center.x <= r && center.y >= t && center.y <= b {
                return true;
            }
            let p1 = Point2D::new(l, t);
            let p2 = Point2D::new(r, t);
            let p3 = Point2D::new(r, b);
            let p4 = Point2D::new(l, b);
            point_to_segment_distance(center, p1, p2) <= threshold
                || point_to_segment_distance(center, p2, p3) <= threshold
                || point_to_segment_distance(center, p3, p4) <= threshold
                || point_to_segment_distance(center, p4, p1) <= threshold
        }
        Shape::Ellipse {
            start, end, width, ..
        } => {
            let (l, t, r, b) = normalize_rect(*start, *end);
            let cx = (l + r) / 2.0;
            let cy = (t + b) / 2.0;
            let rx = ((r - l) / 2.0).max(1.0);
            let ry = ((b - t) / 2.0).max(1.0);
            let threshold = radius + *width / 2.0;
            let dx = center.x - cx;
            let dy = center.y - cy;
            let val = (dx * dx) / (rx * rx) + (dy * dy) / (ry * ry);
            if val <= 1.0 {
                return true;
            }
            center.x >= l - threshold
                && center.x <= r + threshold
                && center.y >= t - threshold
                && center.y <= b + threshold
        }
        Shape::Text {
            origin,
            font_size,
            text,
            ..
        } => {
            let (block_w, block_h) = crate::types::measure_text_block(text, *font_size);
            let est_width = block_w.max(20.0);
            let est_height = block_h;
            let l = origin.x;
            let t = origin.y;
            let r = l + est_width;
            let b = t + est_height;
            center.x >= l - radius
                && center.x <= r + radius
                && center.y >= t - radius
                && center.y <= b + radius
        }
        Shape::StepBadge {
            center: c,
            radius: r,
            ..
        } => center.distance(c) <= (r + radius),
        Shape::Blur { start, end, .. } => {
            let (l, t, r, b) = normalize_rect(*start, *end);
            let threshold = radius + 2.0;
            if center.x >= l && center.x <= r && center.y >= t && center.y <= b {
                return true;
            }
            let p1 = Point2D::new(l, t);
            let p2 = Point2D::new(r, t);
            let p3 = Point2D::new(r, b);
            let p4 = Point2D::new(l, b);
            point_to_segment_distance(center, p1, p2) <= threshold
                || point_to_segment_distance(center, p2, p3) <= threshold
                || point_to_segment_distance(center, p3, p4) <= threshold
                || point_to_segment_distance(center, p4, p1) <= threshold
        }
    }
}

pub fn recognize_smart_shape(
    points: &[Point2D],
    stroke_width: f32,
    color: ColorPreset,
) -> Option<Shape> {
    if points.len() < 8 {
        return None;
    }

    let start = points[0];
    let end = points[points.len() - 1];
    let direct_dist = start.distance(&end);

    let mut path_len = 0.0;
    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    for i in 0..(points.len() - 1) {
        path_len += points[i].distance(&points[i + 1]);
        min_x = min_x.min(points[i].x);
        max_x = max_x.max(points[i].x);
        min_y = min_y.min(points[i].y);
        max_y = max_y.max(points[i].y);
    }
    min_x = min_x.min(end.x);
    max_x = max_x.max(end.x);
    min_y = min_y.min(end.y);
    max_y = max_y.max(end.y);

    let bb_w = max_x - min_x;
    let bb_h = max_y - min_y;

    // 1. Check for Line (direct distance close to total path length)
    if path_len > 40.0 && (direct_dist / path_len) >= 0.88 {
        let mut max_dev: f32 = 0.0;
        for pt in points {
            let dev = point_to_segment_distance(*pt, start, end);
            if dev > max_dev {
                max_dev = dev;
            }
        }
        if max_dev < 18.0 {
            return Some(Shape::Line {
                start,
                end,
                color,
                width: stroke_width,
                pattern: StrokePattern::Solid,
                curve: 0.0,
            });
        }
    }

    // 2. Check for closed loop shapes (Ellipse vs Rectangle)
    let is_closed = direct_dist < 45.0 || (direct_dist / path_len) < 0.25;
    if is_closed && bb_w > 30.0 && bb_h > 30.0 {
        let mut area2: f32 = 0.0;
        for i in 0..(points.len() - 1) {
            area2 += points[i].x * points[i + 1].y - points[i + 1].x * points[i].y;
        }
        area2 += end.x * start.y - start.x * end.y;
        let area = area2.abs() / 2.0;

        let circularity = (4.0 * std::f32::consts::PI * area) / (path_len * path_len);
        let bb_area = bb_w * bb_h;
        let fill_ratio = area / bb_area;

        // Measure distance from bounding box corners to stroke to reliably distinguish Ellipse from Rectangle.
        // For any circle/ellipse, the contour curves inward and stays far from the corners (corner_ratio >= 0.14).
        // For a rectangle, the stroke passes directly through all 4 corners (corner_ratio < 0.14).
        let c1 = Point2D::new(min_x, min_y);
        let c2 = Point2D::new(max_x, min_y);
        let c3 = Point2D::new(max_x, max_y);
        let c4 = Point2D::new(min_x, max_y);

        let d1 = points
            .iter()
            .map(|p| p.distance(&c1))
            .fold(f32::MAX, f32::min);
        let d2 = points
            .iter()
            .map(|p| p.distance(&c2))
            .fold(f32::MAX, f32::min);
        let d3 = points
            .iter()
            .map(|p| p.distance(&c3))
            .fold(f32::MAX, f32::min);
        let d4 = points
            .iter()
            .map(|p| p.distance(&c4))
            .fold(f32::MAX, f32::min);
        let avg_corner_dist = (d1 + d2 + d3 + d4) / 4.0;
        let min_dim = bb_w.min(bb_h);
        let corner_ratio = avg_corner_dist / min_dim;

        if corner_ratio >= 0.14 && (circularity >= 0.45 || fill_ratio >= 0.60) {
            return Some(Shape::Ellipse {
                start: Point2D::new(min_x, min_y),
                end: Point2D::new(max_x, max_y),
                color,
                width: stroke_width,
                fill: FillMode::None,
                pattern: StrokePattern::Solid,
            });
        } else if corner_ratio < 0.14 && fill_ratio >= 0.70 {
            return Some(Shape::Rectangle {
                start: Point2D::new(min_x, min_y),
                end: Point2D::new(max_x, max_y),
                color,
                width: stroke_width,
                rounded: false,
                fill: FillMode::None,
                pattern: StrokePattern::Solid,
            });
        }
    }

    None
}

// Property setters
//
// Each returns whether it changed anything, so a caller can record one undo
// entry for a whole selection and skip it entirely when nothing moved.

/// Which tool would have drawn this shape.
///
/// Lets the Select tool borrow the right property controls for whatever is
/// selected, rather than showing controls for whichever tool was last used.
pub fn shape_kind(shape: &Shape) -> DrawTool {
    match shape {
        Shape::Stroke { is_highlighter, .. } => {
            if *is_highlighter {
                DrawTool::Highlighter
            } else {
                DrawTool::Pen
            }
        }
        Shape::Line { .. } => DrawTool::Line,
        Shape::Arrow { .. } => DrawTool::Arrow,
        Shape::Rectangle { rounded, .. } => {
            if *rounded {
                DrawTool::RoundedRectangle
            } else {
                DrawTool::Rectangle
            }
        }
        Shape::Ellipse { .. } => DrawTool::Ellipse,
        Shape::Text { .. } => DrawTool::Text,
        Shape::StepBadge { .. } => DrawTool::StepBadge,
        Shape::Blur { .. } => DrawTool::Blur,
    }
}

pub fn set_shape_color(shape: &mut Shape, c: ColorPreset) -> bool {
    let slot = match shape {
        Shape::Stroke { color, .. }
        | Shape::Line { color, .. }
        | Shape::Arrow { color, .. }
        | Shape::Rectangle { color, .. }
        | Shape::Ellipse { color, .. }
        | Shape::Text { color, .. }
        | Shape::StepBadge { color, .. } => color,
        // A blur has no colour of its own; it shows what is underneath.
        Shape::Blur { .. } => return false,
    };
    if *slot == c {
        return false;
    }
    *slot = c;
    true
}

pub fn set_shape_width(shape: &mut Shape, w: f32) -> bool {
    let slot = match shape {
        Shape::Stroke { width, .. }
        | Shape::Line { width, .. }
        | Shape::Arrow { width, .. }
        | Shape::Rectangle { width, .. }
        | Shape::Ellipse { width, .. } => width,
        Shape::StepBadge { stroke_width, .. } => stroke_width,
        // For a blur the equivalent knob is how coarse the mosaic is.
        Shape::Blur { block_size, .. } => block_size,
        Shape::Text { .. } => return false,
    };
    if (*slot - w).abs() < 0.01 {
        return false;
    }
    *slot = w;
    true
}

pub fn set_shape_fill(shape: &mut Shape, f: FillMode) -> bool {
    let slot = match shape {
        Shape::Rectangle { fill, .. }
        | Shape::Ellipse { fill, .. }
        | Shape::StepBadge { fill, .. } => fill,
        _ => return false,
    };
    if *slot == f {
        return false;
    }
    *slot = f;
    true
}

pub fn set_shape_pattern(shape: &mut Shape, p: StrokePattern) -> bool {
    let slot = match shape {
        Shape::Stroke { pattern, .. }
        | Shape::Line { pattern, .. }
        | Shape::Arrow { pattern, .. }
        | Shape::Rectangle { pattern, .. }
        | Shape::Ellipse { pattern, .. }
        | Shape::StepBadge { pattern, .. } => pattern,
        _ => return false,
    };
    if *slot == p {
        return false;
    }
    *slot = p;
    true
}


// Alignment

pub use crate::types::AlignTo;

/// How far each box has to move to line up with the others.
///
/// Alignment is to the outer extent of the group, which is what people mean by
/// "align left": everything goes to the leftmost edge, not to the average.
/// Returns one delta per input, in the same order.
pub fn align_offsets(boxes: &[(f32, f32, f32, f32)], to: AlignTo) -> Vec<(f32, f32)> {
    if boxes.len() < 2 {
        return vec![(0.0, 0.0); boxes.len()];
    }
    let l = boxes.iter().fold(f32::MAX, |a, b| a.min(b.0));
    let t = boxes.iter().fold(f32::MAX, |a, b| a.min(b.1));
    let r = boxes.iter().fold(f32::MIN, |a, b| a.max(b.2));
    let bo = boxes.iter().fold(f32::MIN, |a, b| a.max(b.3));
    let cx = (l + r) * 0.5;
    let cy = (t + bo) * 0.5;

    boxes
        .iter()
        .map(|b| match to {
            AlignTo::Left => (l - b.0, 0.0),
            AlignTo::Right => (r - b.2, 0.0),
            AlignTo::HCentre => (cx - (b.0 + b.2) * 0.5, 0.0),
            AlignTo::Top => (0.0, t - b.1),
            AlignTo::Bottom => (0.0, bo - b.3),
            AlignTo::VCentre => (0.0, cy - (b.1 + b.3) * 0.5),
        })
        .collect()
}

/// How far each box has to move for even gaps between them.
///
/// The outermost two stay put — they define the span — and everything between
/// is spread so the *gaps* are equal, which looks right even when the shapes
/// are different sizes. Spacing centres instead would bunch wide ones together.
pub fn distribute_offsets(boxes: &[(f32, f32, f32, f32)], horizontal: bool) -> Vec<(f32, f32)> {
    let n = boxes.len();
    if n < 3 {
        return vec![(0.0, 0.0); n];
    }
    let key = |b: &(f32, f32, f32, f32)| if horizontal { b.0 } else { b.1 };
    let size = |b: &(f32, f32, f32, f32)| {
        if horizontal {
            b.2 - b.0
        } else {
            b.3 - b.1
        }
    };

    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| key(&boxes[*a]).partial_cmp(&key(&boxes[*b])).unwrap());

    let first = &boxes[order[0]];
    let last = &boxes[order[n - 1]];
    let span = if horizontal {
        last.2 - first.0
    } else {
        last.3 - first.1
    };
    let total: f32 = order.iter().map(|i| size(&boxes[*i])).sum();
    let gap = (span - total) / (n as f32 - 1.0);

    let mut out = vec![(0.0, 0.0); n];
    let mut cursor = key(first) + size(first) + gap;
    for i in order.iter().take(n - 1).skip(1) {
        let b = &boxes[*i];
        let delta = cursor - key(b);
        out[*i] = if horizontal { (delta, 0.0) } else { (0.0, delta) };
        cursor += size(b) + gap;
    }
    out
}

// Curved lines
//
// One number describes the bow: how far the middle sits off the straight
// chord. That is enough for the arcs people actually draw between boxes, and
// it stays a single draggable handle rather than a polyline to manage.

/// How many segments a curve is sampled into for drawing and hit-testing.
pub const CURVE_SAMPLES: usize = 24;

/// The quadratic control point that bows a chord by `curve`.
///
/// A quadratic sits at half the control's offset at its midpoint, so the
/// control goes twice as far out as the bow the caller asked for.
pub fn curve_control(start: Point2D, end: Point2D, curve: f32) -> Point2D {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let len = (dx * dx + dy * dy).sqrt();
    let mid = Point2D::new((start.x + end.x) * 0.5, (start.y + end.y) * 0.5);
    if len < 0.001 {
        return mid;
    }
    let (px, py) = (-dy / len, dx / len);
    Point2D::new(mid.x + px * curve * 2.0, mid.y + py * curve * 2.0)
}

/// A point on the quadratic at `t` in 0..=1.
pub fn quad_point(p0: Point2D, c: Point2D, p1: Point2D, t: f32) -> Point2D {
    let u = 1.0 - t;
    Point2D::new(
        u * u * p0.x + 2.0 * u * t * c.x + t * t * p1.x,
        u * u * p0.y + 2.0 * u * t * c.y + t * t * p1.y,
    )
}

/// The curve as a polyline. A straight line is just its two ends.
pub fn sample_curve(start: Point2D, end: Point2D, curve: f32) -> Vec<Point2D> {
    if curve.abs() < 0.01 {
        return vec![start, end];
    }
    let c = curve_control(start, end, curve);
    (0..=CURVE_SAMPLES)
        .map(|i| quad_point(start, c, end, i as f32 / CURVE_SAMPLES as f32))
        .collect()
}

/// Where the bow handle sits: the actual midpoint of the curve.
pub fn curve_handle(start: Point2D, end: Point2D, curve: f32) -> Point2D {
    if curve.abs() < 0.01 {
        return Point2D::new((start.x + end.x) * 0.5, (start.y + end.y) * 0.5);
    }
    quad_point(start, curve_control(start, end, curve), end, 0.5)
}

/// The bow that would put the curve's midpoint under `at`.
///
/// Signed, so dragging through the chord flips the curve to the other side
/// rather than stopping flat.
pub fn curve_from_handle(start: Point2D, end: Point2D, at: Point2D) -> f32 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.001 {
        return 0.0;
    }
    let (px, py) = (-dy / len, dx / len);
    let mid = Point2D::new((start.x + end.x) * 0.5, (start.y + end.y) * 0.5);
    (at.x - mid.x) * px + (at.y - mid.y) * py
}

/// Trim a polyline to `keep` units short of its end, for an arrow head.
pub fn trim_polyline_end(points: &[Point2D], keep: f32) -> Vec<Point2D> {
    if keep <= 0.0 || points.len() < 2 {
        return points.to_vec();
    }
    let mut out = points.to_vec();
    let mut budget = keep;
    while out.len() >= 2 {
        let last = out[out.len() - 1];
        let prev = out[out.len() - 2];
        let seg = last.distance(&prev);
        if seg > budget {
            // Land partway along this segment.
            let t = (seg - budget) / seg;
            let n = out.len();
            out[n - 1] = Point2D::new(
                prev.x + (last.x - prev.x) * t,
                prev.y + (last.y - prev.y) * t,
            );
            return out;
        }
        budget -= seg;
        out.pop();
    }
    out
}

// ─────────────────────── Arrow proportions ───────────────────────
//
// The head has to stay recognisably wider than the shaft at every stroke
// width, and the shaft has to stop where the head begins. Drawing the shaft
// all the way to the tip and filling a fixed-size head over it looks fine at
// 2px and turns into a rounded bar with two fins by 36px.

/// Head length and half-width for a stroke of `width` on an arrow of `length`.
pub fn arrow_head_size(width: f32, length: f32) -> (f32, f32) {
    // Grow with the stroke, but never eat more than part of a short arrow.
    let head_len = (width * 4.0 + 10.0).clamp(12.0, 120.0).min(length * 0.45);
    // A head narrower than about twice the shaft stops reading as a head.
    let half_width = (head_len * 0.42).max(width * 1.15);
    // ...but that floor can outrun the length cap on a short, thick arrow and
    // leave a head wider than it is long, which is a fin rather than a point.
    // Keeping the aspect sane wins over honouring the length cap; the shaft
    // simply disappears, which is the honest rendering of that shape.
    let head_len = head_len.max(half_width * 1.2);
    (head_len, half_width)
}

/// The three points of an arrow head pointing from `from` to `to`.
pub fn arrow_head_points(
    from: Point2D,
    to: Point2D,
    head_len: f32,
    half_width: f32,
) -> (Point2D, Point2D, Point2D) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.001 {
        return (to, to, to);
    }
    let (ux, uy) = (dx / len, dy / len);
    let base = Point2D::new(to.x - ux * head_len, to.y - uy * head_len);
    // Perpendicular, for the two flanks.
    let (px, py) = (-uy, ux);
    (
        to,
        Point2D::new(base.x + px * half_width, base.y + py * half_width),
        Point2D::new(base.x - px * half_width, base.y - py * half_width),
    )
}

/// Where the shaft should run, pulled back from whichever ends carry a head.
///
/// `None` when the arrow is all head and no shaft, which is what a very short
/// or very thick one becomes.
pub fn arrow_shaft(
    start: Point2D,
    end: Point2D,
    head_len: f32,
    head_at_start: bool,
    head_at_end: bool,
) -> Option<(Point2D, Point2D)> {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.001 {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len);
    // Stop just inside the head so the two overlap rather than seam.
    let inset = head_len * 0.92;
    let from_t = if head_at_start { inset } else { 0.0 };
    let to_t = len - if head_at_end { inset } else { 0.0 };
    if to_t - from_t < 0.5 {
        return None;
    }
    Some((
        Point2D::new(start.x + ux * from_t, start.y + uy * from_t),
        Point2D::new(start.x + ux * to_t, start.y + uy * to_t),
    ))
}

// ─────────────────────── Arrow binding ───────────────────────

/// Breathing room between a bound arrow's tip and the shape it points at.
pub const ARROW_BINDING_GAP: f32 = 6.0;

/// Whether an arrow can anchor to this shape.
///
/// It needs a boundary worth aiming at. A freehand scribble, a line or a blur
/// patch has no meaningful "edge" to stop against.
pub fn can_bind_arrow(shape: &Shape) -> bool {
    matches!(
        shape,
        Shape::Rectangle { .. } | Shape::Ellipse { .. } | Shape::StepBadge { .. } | Shape::Text { .. }
    )
}

/// Where a ray from a shape's centre toward `toward` leaves its outline,
/// pushed out by `gap`.
///
/// This is what keeps a bound arrow touching the edge of a box rather than
/// burying its head in the middle of it, at whatever angle the two end up.
pub fn boundary_point(shape: &Shape, toward: Point2D, gap: f32) -> Point2D {
    let (l, t, r, b) = shape_bounds(shape);
    let c = Point2D::new((l + r) * 0.5, (t + b) * 0.5);
    let dx = toward.x - c.x;
    let dy = toward.y - c.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.001 {
        return c;
    }
    let (ux, uy) = (dx / len, dy / len);
    let rx = ((r - l) * 0.5).max(0.5);
    let ry = ((b - t) * 0.5).max(0.5);

    let hit = match shape {
        Shape::Ellipse { .. } => {
            let a = (ux / rx).powi(2) + (uy / ry).powi(2);
            if a > 1e-9 { 1.0 / a.sqrt() } else { rx.min(ry) }
        }
        Shape::StepBadge { radius, .. } => *radius,
        _ => {
            // Rectangle: whichever axis the ray crosses first.
            let tx = if ux.abs() > 1e-6 { rx / ux.abs() } else { f32::MAX };
            let ty = if uy.abs() > 1e-6 { ry / uy.abs() } else { f32::MAX };
            tx.min(ty)
        }
    };

    // Never reach past the point being aimed at, or a short arrow between two
    // touching boxes would turn itself inside out.
    let d = (hit + gap).min(len);
    Point2D::new(c.x + ux * d, c.y + uy * d)
}

/// The endpoints a bound line or arrow actually draws between.
///
/// Returns `None` when nothing is bound, so callers can skip the clone.
/// Each end aims at the *centre* of whatever the other end is attached to,
/// which keeps the two resolutions independent rather than chasing each other.
pub fn resolve_arrow_ends(
    shape: &Shape,
    start_target: Option<&Shape>,
    end_target: Option<&Shape>,
    gap: f32,
) -> Option<(Point2D, Point2D)> {
    if start_target.is_none() && end_target.is_none() {
        return None;
    }
    let (start, end) = match shape {
        Shape::Arrow { start, end, .. } | Shape::Line { start, end, .. } => (*start, *end),
        _ => return None,
    };

    let centre = |s: &Shape| {
        let (l, t, r, b) = shape_bounds(s);
        Point2D::new((l + r) * 0.5, (t + b) * 0.5)
    };
    let aim_for_start = end_target.map(centre).unwrap_or(end);
    let aim_for_end = start_target.map(centre).unwrap_or(start);

    let new_start = match start_target {
        Some(s) => boundary_point(s, aim_for_start, gap),
        None => start,
    };
    let new_end = match end_target {
        Some(s) => boundary_point(s, aim_for_end, gap),
        None => end,
    };
    Some((new_start, new_end))
}

/// Put resolved endpoints back into a copy of the shape.
pub fn with_arrow_ends(shape: &Shape, start: Point2D, end: Point2D) -> Shape {
    let mut out = shape.clone();
    match &mut out {
        Shape::Arrow { start: s, end: e, .. } | Shape::Line { start: s, end: e, .. } => {
            *s = start;
            *e = end;
        }
        _ => {}
    }
    out
}

// ─────────────────────── Container labels ───────────────────────

/// Whether a shape can hold a label.
///
/// Boxes and discs read as containers; a line, a scribble or a blur patch does
/// not, and free-floating text should stay free-floating.
pub fn can_contain_text(shape: &Shape) -> bool {
    matches!(
        shape,
        Shape::Rectangle { .. }
            | Shape::Ellipse { .. }
            | Shape::StepBadge { .. }
            | Shape::Line { .. }
            | Shape::Arrow { .. }
    )
}

/// Whether a label on this shape sits *on* it rather than *inside* it.
///
/// A line has no interior, so its label straddles the midpoint and needs a
/// chip behind it or the line strikes straight through the words. It also
/// cannot grow to fit, and wrapping it to the bounding box would squeeze a
/// near-vertical arrow's label into a column one character wide.
pub fn label_rides_on_shape(shape: &Shape) -> bool {
    matches!(shape, Shape::Line { .. } | Shape::Arrow { .. })
}

/// Where a label of the given size sits inside its container: centred on both
/// axes, which is what makes it look deliberate rather than dropped in.
pub fn contained_text_origin(
    container: (f32, f32, f32, f32),
    text_w: f32,
    text_h: f32,
) -> Point2D {
    let (l, t, r, b) = container;
    Point2D::new(
        l + ((r - l) - text_w) * 0.5,
        t + ((b - t) - text_h) * 0.5,
    )
}

/// The height a container needs to hold a label of `text_h`, never shrinking
/// it below what it already is.
///
/// Growing rather than clipping is the less surprising behaviour: the box is
/// the thing the user sized, but losing words is worse than a taller box.
pub fn container_height_for(current: (f32, f32, f32, f32), text_h: f32, pad: f32) -> f32 {
    let have = current.3 - current.1;
    have.max(text_h + pad * 2.0)
}

// ─────────────────────── Snapping ───────────────────────
//
// Two kinds, both measured in screen DIPs so the pull feels the same at any
// zoom: a *point* snap pulls onto another shape's corner, edge midpoint or
// centre, and an *alignment* snap pulls one axis into line with another shape,
// which is what keeps a row of boxes tidy.

/// How close, in screen DIPs, before a snap takes hold.
pub const SNAP_TOLERANCE_DIP: f32 = 8.0;

/// How far a guide runs past the two points it connects, in canvas units. An
/// alignment guide lies exactly along the edge it is aligning, so without an
/// overhang it disappears underneath that edge and shows the user nothing.
pub const SNAP_GUIDE_OVERHANG: f32 = 18.0;

/// Stretch an axis-aligned guide out past both ends so it stays visible.
fn overhang(a: Point2D, b: Point2D) -> (Point2D, Point2D) {
    let m = SNAP_GUIDE_OVERHANG;
    if (a.x - b.x).abs() < 0.01 {
        let (lo, hi) = if a.y <= b.y { (a.y, b.y) } else { (b.y, a.y) };
        (Point2D::new(a.x, lo - m), Point2D::new(a.x, hi + m))
    } else if (a.y - b.y).abs() < 0.01 {
        let (lo, hi) = if a.x <= b.x { (a.x, b.x) } else { (b.x, a.x) };
        (Point2D::new(lo - m, a.y), Point2D::new(hi + m, a.y))
    } else {
        (a, b)
    }
}

/// Feedback to draw for a snap that took hold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapGuide {
    pub a: Point2D,
    pub b: Point2D,
    /// Draw a marker at `a` rather than a line from `a` to `b`.
    pub marker: bool,
}

/// Points on `shape` that something else can snap to.
///
/// Freehand strokes contribute none: snapping to a scribble is noise, not
/// alignment.
pub fn anchor_points(shape: &Shape) -> Vec<Point2D> {
    match shape {
        Shape::Stroke { .. } => Vec::new(),
        Shape::Line {
            start, end, curve, ..
        }
        | Shape::Arrow {
            start, end, curve, ..
        } => vec![*start, *end, curve_handle(*start, *end, *curve)],
        Shape::StepBadge { center, radius, .. } => vec![
            *center,
            Point2D::new(center.x - radius, center.y),
            Point2D::new(center.x + radius, center.y),
            Point2D::new(center.x, center.y - radius),
            Point2D::new(center.x, center.y + radius),
        ],
        _ => {
            let (l, t, r, b) = shape_bounds(shape);
            let cx = (l + r) * 0.5;
            let cy = (t + b) * 0.5;
            vec![
                Point2D::new(l, t),
                Point2D::new(cx, t),
                Point2D::new(r, t),
                Point2D::new(r, cy),
                Point2D::new(r, b),
                Point2D::new(cx, b),
                Point2D::new(l, b),
                Point2D::new(l, cy),
                Point2D::new(cx, cy),
            ]
        }
    }
}

/// Every anchor offered by `shapes`, flattened.
pub fn collect_anchors(shapes: &[Shape]) -> Vec<Point2D> {
    shapes.iter().flat_map(anchor_points).collect()
}

/// Pull a single moving point onto nearby geometry.
///
/// A full point snap wins outright; otherwise each axis is considered on its
/// own, so a point can line up vertically with one shape and horizontally with
/// another at the same time.
pub fn snap_point(
    target: Point2D,
    anchors: &[Point2D],
    tolerance: f32,
) -> Option<(Point2D, Vec<SnapGuide>)> {
    if anchors.is_empty() || tolerance <= 0.0 {
        return None;
    }

    // 1. Land directly on an anchor if one is within reach.
    let mut best: Option<(f32, Point2D)> = None;
    for a in anchors {
        let d = a.distance(&target);
        if d <= tolerance && best.map(|(bd, _)| d < bd).unwrap_or(true) {
            best = Some((d, *a));
        }
    }
    if let Some((_, a)) = best {
        return Some((
            a,
            vec![SnapGuide {
                a,
                b: a,
                marker: true,
            }],
        ));
    }

    // 2. Otherwise line up per axis.
    let mut snapped = target;
    let mut guides = Vec::new();

    let mut best_x: Option<(f32, Point2D)> = None;
    let mut best_y: Option<(f32, Point2D)> = None;
    for a in anchors {
        let dx = (a.x - target.x).abs();
        if dx <= tolerance && best_x.map(|(bd, _)| dx < bd).unwrap_or(true) {
            best_x = Some((dx, *a));
        }
        let dy = (a.y - target.y).abs();
        if dy <= tolerance && best_y.map(|(bd, _)| dy < bd).unwrap_or(true) {
            best_y = Some((dy, *a));
        }
    }
    if let Some((_, a)) = best_x {
        snapped.x = a.x;
        let (ga, gb) = overhang(a, Point2D::new(a.x, target.y));
        guides.push(SnapGuide {
            a: ga,
            b: gb,
            marker: false,
        });
    }
    if let Some((_, a)) = best_y {
        snapped.y = a.y;
        let (ga, gb) = overhang(a, Point2D::new(target.x, a.y));
        guides.push(SnapGuide {
            a: ga,
            b: gb,
            marker: false,
        });
    }

    if guides.is_empty() {
        None
    } else {
        Some((snapped, guides))
    }
}

/// Adjust a drag so the moving shape lines up with the others.
///
/// Returns the corrected delta plus whatever guides should be drawn. Each axis
/// takes the smallest correction any of the shape's own anchors can offer, so
/// dragging a box snaps by whichever of its edges or centre is closest to
/// something.
pub fn snap_translation(
    moving: &Shape,
    dx: f32,
    dy: f32,
    anchors: &[Point2D],
    tolerance: f32,
) -> (f32, f32, Vec<SnapGuide>) {
    if anchors.is_empty() || tolerance <= 0.0 {
        return (dx, dy, Vec::new());
    }
    let own = anchor_points(moving);
    if own.is_empty() {
        return (dx, dy, Vec::new());
    }

    // (correction, from, to) for the closest match on each axis.
    let mut best_x: Option<(f32, Point2D, Point2D)> = None;
    let mut best_y: Option<(f32, Point2D, Point2D)> = None;

    for o in &own {
        let moved = Point2D::new(o.x + dx, o.y + dy);
        for a in anchors {
            let cx = a.x - moved.x;
            if cx.abs() <= tolerance && best_x.map(|(b, _, _)| cx.abs() < b.abs()).unwrap_or(true) {
                best_x = Some((cx, moved, *a));
            }
            let cy = a.y - moved.y;
            if cy.abs() <= tolerance && best_y.map(|(b, _, _)| cy.abs() < b.abs()).unwrap_or(true) {
                best_y = Some((cy, moved, *a));
            }
        }
    }

    let mut guides = Vec::new();
    let mut out_dx = dx;
    let mut out_dy = dy;
    if let Some((c, from, to)) = best_x {
        out_dx += c;
        let (ga, gb) = overhang(to, Point2D::new(to.x, from.y));
        guides.push(SnapGuide {
            a: ga,
            b: gb,
            marker: false,
        });
    }
    if let Some((c, from, to)) = best_y {
        out_dy += c;
        let (ga, gb) = overhang(to, Point2D::new(from.x, to.y));
        guides.push(SnapGuide {
            a: ga,
            b: gb,
            marker: false,
        });
    }
    (out_dx, out_dy, guides)
}

// ─────────────────────── Pen pressure ───────────────────────

/// Maps pen pressure (0..=1) onto a stroke-width multiplier.
///
/// The floor keeps a feather-light touch visible rather than invisible, and the
/// ceiling stops a hard press from ballooning past the chosen nib size.
pub fn pressure_width_factor(pressure: f32) -> f32 {
    const MIN: f32 = 0.35;
    const MAX: f32 = 1.45;
    MIN + (MAX - MIN) * pressure.clamp(0.0, 1.0)
}

/// Extend a stroke's pressure track alongside its points.
///
/// A stroke that started without pressure stays without it, so mouse strokes
/// keep the cheaper uniform-width path. Once a stroke has pressure the track is
/// kept exactly as long as `points`, repeating the last sample if the pen stops
/// reporting, because the renderer only honours a track that lines up.
pub fn push_pressure(pressures: &mut Vec<f32>, sample: Option<f32>, point_count: usize) {
    if pressures.is_empty() {
        return;
    }
    let next = sample.or_else(|| pressures.last().copied()).unwrap_or(1.0);
    pressures.push(next);
    // A dropped pointer message must not desynchronise the two vectors.
    while pressures.len() < point_count {
        pressures.push(next);
    }
    pressures.truncate(point_count);
}

// ─────────────────────── Selection geometry ───────────────────────
//
// The Select tool works entirely off a shape's axis-aligned bounding box:
// dragging the body translates it, dragging a grip maps the old box onto a
// new one and every point rides along.

/// Side length of a selection grip, in screen DIPs (constant on screen, so it
/// does not balloon when the canvas is zoomed in).
pub const SELECTION_HANDLE_SIZE: f32 = 9.0;
/// Extra slop around a grip so it can be grabbed without pixel-hunting.
pub const SELECTION_HANDLE_SLOP: f32 = 5.0;
/// A resized box is never allowed to collapse below this, in canvas units.
pub const MIN_SHAPE_EXTENT: f32 = 6.0;

/// Axis-aligned bounds of a shape in canvas coordinates, as (l, t, r, b).
///
/// `Text` is measured with the character estimate; the overlay overrides it
/// with exact DirectWrite metrics where a render target is available.
pub fn shape_bounds(shape: &Shape) -> (f32, f32, f32, f32) {
    match shape {
        Shape::Stroke { points, width, .. } => {
            if points.is_empty() {
                return (0.0, 0.0, 0.0, 0.0);
            }
            let half = width * 0.5;
            let mut l = f32::MAX;
            let mut t = f32::MAX;
            let mut r = f32::MIN;
            let mut b = f32::MIN;
            for p in points {
                l = l.min(p.x);
                t = t.min(p.y);
                r = r.max(p.x);
                b = b.max(p.y);
            }
            (l - half, t - half, r + half, b + half)
        }
        Shape::Line {
            start,
            end,
            width,
            curve,
            ..
        }
        | Shape::Arrow {
            start,
            end,
            width,
            curve,
            ..
        } => {
            // A bowed line reaches outside the box its ends describe, so the
            // arc is what gets measured.
            let pts = sample_curve(*start, *end, *curve);
            let half = width * 0.5;
            let mut l = f32::MAX;
            let mut t = f32::MAX;
            let mut r = f32::MIN;
            let mut b = f32::MIN;
            for p in &pts {
                l = l.min(p.x);
                t = t.min(p.y);
                r = r.max(p.x);
                b = b.max(p.y);
            }
            (l - half, t - half, r + half, b + half)
        }
        Shape::Rectangle { start, end, width, .. } | Shape::Ellipse { start, end, width, .. } => {
            let (l, t, r, b) = normalize_rect(*start, *end);
            let half = width * 0.5;
            (l - half, t - half, r + half, b + half)
        }
        Shape::Blur { start, end, .. } => normalize_rect(*start, *end),
        Shape::Text {
            origin,
            text,
            font_size,
            ..
        } => {
            let (w, h) = crate::types::measure_text_block(text, *font_size);
            (origin.x, origin.y, origin.x + w.max(20.0), origin.y + h)
        }
        Shape::StepBadge {
            center,
            radius,
            stroke_width,
            ..
        } => {
            let e = radius + stroke_width * 0.5;
            (center.x - e, center.y - e, center.x + e, center.y + e)
        }
    }
}

/// Centres of the eight grips for a bounding box, in the box's own space.
pub fn selection_handle_points(
    bounds: (f32, f32, f32, f32),
) -> [(SelectionHandle, Point2D); 8] {
    let (l, t, r, b) = bounds;
    let cx = (l + r) * 0.5;
    let cy = (t + b) * 0.5;
    [
        (SelectionHandle::NW, Point2D::new(l, t)),
        (SelectionHandle::N, Point2D::new(cx, t)),
        (SelectionHandle::NE, Point2D::new(r, t)),
        (SelectionHandle::E, Point2D::new(r, cy)),
        (SelectionHandle::SE, Point2D::new(r, b)),
        (SelectionHandle::S, Point2D::new(cx, b)),
        (SelectionHandle::SW, Point2D::new(l, b)),
        (SelectionHandle::W, Point2D::new(l, cy)),
    ]
}

/// Which grip, if any, sits under `pt`. Both `bounds` and `pt` must be in the
/// same space — the caller works in screen DIPs so grips stay a fixed size.
pub fn handle_at(bounds: (f32, f32, f32, f32), pt: Point2D) -> Option<SelectionHandle> {
    let reach = SELECTION_HANDLE_SIZE * 0.5 + SELECTION_HANDLE_SLOP;
    selection_handle_points(bounds)
        .into_iter()
        .find(|(_, c)| (pt.x - c.x).abs() <= reach && (pt.y - c.y).abs() <= reach)
        .map(|(h, _)| h)
}

/// The box produced by dragging `handle` of `orig` by (dx, dy). The opposite
/// edges stay put, and neither extent is allowed to collapse or invert.
pub fn resized_bounds(
    orig: (f32, f32, f32, f32),
    handle: SelectionHandle,
    dx: f32,
    dy: f32,
) -> (f32, f32, f32, f32) {
    let (mut l, mut t, mut r, mut b) = orig;
    let (m_l, m_t, m_r, m_b) = handle.edges();
    if m_l {
        l = (l + dx).min(r - MIN_SHAPE_EXTENT);
    }
    if m_t {
        t = (t + dy).min(b - MIN_SHAPE_EXTENT);
    }
    if m_r {
        r = (r + dx).max(l + MIN_SHAPE_EXTENT);
    }
    if m_b {
        b = (b + dy).max(t + MIN_SHAPE_EXTENT);
    }
    (l, t, r, b)
}

pub fn translate_shape(shape: &mut Shape, dx: f32, dy: f32) {
    let mv = |p: &mut Point2D| {
        p.x += dx;
        p.y += dy;
    };
    match shape {
        Shape::Stroke { points, .. } => points.iter_mut().for_each(mv),
        Shape::Line { start, end, .. }
        | Shape::Arrow { start, end, .. }
        | Shape::Rectangle { start, end, .. }
        | Shape::Ellipse { start, end, .. }
        | Shape::Blur { start, end, .. } => {
            mv(start);
            mv(end);
        }
        Shape::Text { origin, .. } => mv(origin),
        Shape::StepBadge { center, .. } => mv(center),
    }
}

/// Remap `shape` so that the box `from` becomes the box `to`.
///
/// `from` and `to` are *padded* bounds — what `shape_bounds` reports, half the
/// stroke width included. The geometry inside that padding is what gets mapped,
/// so `shape_bounds` afterwards lands exactly on `to` and the grips stay under
/// the cursor. Stroke weight is deliberately left alone: resizing a box should
/// not re-weight its outline.
///
/// `StepBadge` and `Text` are content-shaped rather than box-shaped — a badge
/// stays circular (inscribed in `to`) and text reflows at a scaled point size,
/// so for those two the reported bounds track the content, not `to` exactly.
pub fn resize_shape(shape: &mut Shape, from: (f32, f32, f32, f32), to: (f32, f32, f32, f32)) {
    fn inset(b: (f32, f32, f32, f32), pad: f32) -> (f32, f32, f32, f32) {
        (b.0 + pad, b.1 + pad, b.2 - pad, b.3 - pad)
    }
    // Map a point from box `f` onto box `t`.
    fn mapper(f: (f32, f32, f32, f32), t: (f32, f32, f32, f32)) -> impl Fn(&mut Point2D) {
        let fw = (f.2 - f.0).abs().max(f32::EPSILON);
        let fh = (f.3 - f.1).abs().max(f32::EPSILON);
        let sx = (t.2 - t.0) / fw;
        let sy = (t.3 - t.1) / fh;
        move |p: &mut Point2D| {
            p.x = t.0 + (p.x - f.0) * sx;
            p.y = t.1 + (p.y - f.1) * sy;
        }
    }

    match shape {
        Shape::Stroke { points, width, .. } => {
            let pad = *width * 0.5;
            let map = mapper(inset(from, pad), inset(to, pad));
            points.iter_mut().for_each(map);
        }
        Shape::Line {
            start, end, width, ..
        }
        | Shape::Arrow {
            start, end, width, ..
        }
        | Shape::Rectangle {
            start, end, width, ..
        }
        | Shape::Ellipse {
            start, end, width, ..
        } => {
            let pad = *width * 0.5;
            let map = mapper(inset(from, pad), inset(to, pad));
            map(start);
            map(end);
        }
        Shape::Blur { start, end, .. } => {
            let map = mapper(from, to);
            map(start);
            map(end);
        }
        Shape::Text {
            origin, font_size, ..
        } => {
            let fh = (from.3 - from.1).abs().max(f32::EPSILON);
            let sy = ((to.3 - to.1) / fh).abs().max(0.05);
            origin.x = to.0;
            origin.y = to.1;
            *font_size = (*font_size * sy).clamp(8.0, 200.0);
        }
        Shape::StepBadge {
            center,
            radius,
            stroke_width,
            ..
        } => {
            // A badge is a disc: it inscribes the new box rather than stretching.
            center.x = (to.0 + to.2) * 0.5;
            center.y = (to.1 + to.3) * 0.5;
            let half = ((to.2 - to.0).abs().min((to.3 - to.1).abs())) * 0.5;
            *radius = (half - *stroke_width * 0.5).clamp(6.0, 400.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ArrowHead, ArrowStyle, BadgeShape, TextCardStyle, TextFontFamily};

    #[test]
    fn test_normalize_rect() {
        let p1 = Point2D::new(100.0, 200.0);
        let p2 = Point2D::new(50.0, 300.0);
        let (l, t, r, b) = normalize_rect(p1, p2);
        assert_eq!(l, 50.0);
        assert_eq!(t, 200.0);
        assert_eq!(r, 100.0);
        assert_eq!(b, 300.0);
    }

    #[test]
    fn test_snap_to_square() {
        let start = Point2D::new(0.0, 0.0);
        let current = Point2D::new(100.0, 60.0);
        let snapped = snap_to_square(start, current);
        assert_eq!(snapped.x, 100.0);
        assert_eq!(snapped.y, 100.0);
    }

    #[test]
    fn test_snap_to_angle() {
        let start = Point2D::new(0.0, 0.0);
        let current = Point2D::new(100.0, 10.0);
        let snapped = snap_to_angle(start, current);
        assert!((snapped.y).abs() < 1.0);
        assert!((snapped.x - 100.5).abs() < 1.0);
    }



    #[test]
    fn test_bezier_smoothing() {
        let points = vec![
            Point2D::new(0.0, 0.0),
            Point2D::new(50.0, 20.0),
            Point2D::new(100.0, 0.0),
            Point2D::new(150.0, 50.0),
        ];
        let segments = points_to_bezier_segments(&points);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].2, Point2D::new(50.0, 20.0));
        assert_eq!(segments[2].2, Point2D::new(150.0, 50.0));
    }

    #[test]
    fn test_shape_eraser_intersection() {
        let line = Shape::Line {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(100.0, 0.0),
            color: ColorPreset::Red,
            width: 4.0,
            pattern: StrokePattern::Solid,
            curve: 0.0,
        };
        // Circle right on the line
        assert!(shape_intersects_circle(
            &line,
            Point2D::new(50.0, 2.0),
            10.0
        ));
        // Circle far from the line
        assert!(!shape_intersects_circle(
            &line,
            Point2D::new(50.0, 50.0),
            10.0
        ));

        let rect = Shape::Rectangle {
            start: Point2D::new(10.0, 10.0),
            end: Point2D::new(90.0, 90.0),
            color: ColorPreset::Blue,
            width: 4.0,
            rounded: false,
            fill: FillMode::None,
            pattern: StrokePattern::Solid,
        };
        // Inside rectangle
        assert!(shape_intersects_circle(
            &rect,
            Point2D::new(50.0, 50.0),
            5.0
        ));
        // Outside rectangle
        assert!(!shape_intersects_circle(
            &rect,
            Point2D::new(200.0, 200.0),
            5.0
        ));
    }

    #[test]
    fn test_smart_shape_recognition() {
        // Approximate a straight line
        let mut line_points = Vec::new();
        for i in 0..15 {
            let x = i as f32 * 10.0;
            line_points.push(Point2D::new(x, (i % 2) as f32)); // tiny jitter
        }
        let recognized_line = recognize_smart_shape(&line_points, 4.0, ColorPreset::Green);
        assert!(matches!(recognized_line, Some(Shape::Line { .. })));

        // Approximate a circle
        let mut circle_points = Vec::new();
        let cx = 100.0;
        let cy = 100.0;
        let r = 50.0;
        for i in 0..20 {
            let angle = i as f32 * (std::f32::consts::PI * 2.0 / 20.0);
            circle_points.push(Point2D::new(cx + r * angle.cos(), cy + r * angle.sin()));
        }
        circle_points.push(circle_points[0]); // close it
        let recognized_circle = recognize_smart_shape(&circle_points, 4.0, ColorPreset::Yellow);
        assert!(matches!(recognized_circle, Some(Shape::Ellipse { .. })));

        // Hand-drawn rectangle with 4 corners
        let mut rect_points = Vec::new();
        for x in (0..=100).step_by(10) {
            rect_points.push(Point2D::new(x as f32, 0.0));
        }
        for y in (0..=100).step_by(10) {
            rect_points.push(Point2D::new(100.0, y as f32));
        }
        for x in (0..=100).rev().step_by(10) {
            rect_points.push(Point2D::new(x as f32, 100.0));
        }
        for y in (0..=100).rev().step_by(10) {
            rect_points.push(Point2D::new(0.0, y as f32));
        }
        rect_points.push(rect_points[0]);
        let recognized_rect = recognize_smart_shape(&rect_points, 4.0, ColorPreset::Blue);
        assert!(matches!(recognized_rect, Some(Shape::Rectangle { .. })));

        // Jittery circle (which previously failed into rectangle due to fill_ratio >= 0.68)
        let mut jitter_circle = Vec::new();
        for i in 0..24 {
            let angle = i as f32 * (std::f32::consts::PI * 2.0 / 24.0);
            let jitter = if i % 2 == 0 { 2.5 } else { -2.5 };
            jitter_circle.push(Point2D::new(
                cx + (r + jitter) * angle.cos(),
                cy + (r + jitter) * angle.sin(),
            ));
        }
        jitter_circle.push(jitter_circle[0]);
        let recognized_jitter = recognize_smart_shape(&jitter_circle, 4.0, ColorPreset::Cyan);
        assert!(matches!(recognized_jitter, Some(Shape::Ellipse { .. })));
        assert!(!matches!(recognized_jitter, Some(Shape::Rectangle { .. })));
    }

    fn rect(l: f32, t: f32, r: f32, b: f32) -> Shape {
        Shape::Rectangle {
            start: Point2D::new(l, t),
            end: Point2D::new(r, b),
            color: ColorPreset::Red,
            width: 0.0,
            rounded: false,
            fill: FillMode::None,
            pattern: StrokePattern::Solid,
        }
    }

    fn arrow(x0: f32, y0: f32, x1: f32, y1: f32) -> Shape {
        Shape::Arrow {
            start: Point2D::new(x0, y0),
            end: Point2D::new(x1, y1),
            color: ColorPreset::Red,
            width: 2.0,
            style: ArrowStyle::Single,
            head: ArrowHead::default(),
            curve: 0.0,
            pattern: StrokePattern::Solid,
        }
    }

    #[test]
    fn test_a_zero_bow_is_just_the_two_ends() {
        let a = Point2D::new(0.0, 0.0);
        let b = Point2D::new(100.0, 0.0);
        assert_eq!(sample_curve(a, b, 0.0), vec![a, b]);
        assert_eq!(curve_handle(a, b, 0.0), Point2D::new(50.0, 0.0));
    }

    #[test]
    fn test_the_bow_handle_sits_where_the_curve_actually_is() {
        // The control point goes twice as far out as the bow, because a
        // quadratic only reaches halfway to it at its midpoint.
        let a = Point2D::new(0.0, 0.0);
        let b = Point2D::new(100.0, 0.0);
        let h = curve_handle(a, b, 30.0);
        assert!((h.x - 50.0).abs() < 0.01);
        assert!((h.y.abs() - 30.0).abs() < 0.01, "handle at {:?}", h);
    }

    #[test]
    fn test_dragging_the_handle_round_trips_to_the_same_bow() {
        let a = Point2D::new(10.0, 20.0);
        let b = Point2D::new(210.0, 60.0);
        for want in [-80.0f32, -5.0, 12.0, 45.0] {
            let h = curve_handle(a, b, want);
            let got = curve_from_handle(a, b, h);
            assert!((got - want).abs() < 0.01, "{} -> {}", want, got);
        }
    }

    #[test]
    fn test_dragging_through_the_chord_flips_the_bow() {
        // Signed, so a curve can be pulled to the other side rather than
        // flattening out and refusing to go further.
        let a = Point2D::new(0.0, 0.0);
        let b = Point2D::new(100.0, 0.0);
        let up = curve_from_handle(a, b, Point2D::new(50.0, -20.0));
        let down = curve_from_handle(a, b, Point2D::new(50.0, 20.0));
        assert!(up * down < 0.0, "{} and {} should differ in sign", up, down);
    }

    #[test]
    fn test_a_bowed_line_reaches_outside_its_endpoints_box() {
        let straight = Shape::Line {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(100.0, 0.0),
            color: ColorPreset::Red,
            width: 0.0,
            pattern: StrokePattern::Solid,
            curve: 0.0,
        };
        let bowed = Shape::Line {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(100.0, 0.0),
            color: ColorPreset::Red,
            width: 0.0,
            pattern: StrokePattern::Solid,
            curve: 40.0,
        };
        let sb = shape_bounds(&straight);
        let bb = shape_bounds(&bowed);
        assert!((sb.3 - sb.1).abs() < 0.01, "straight box should be flat");
        // The arc bulges about `curve` off the chord, so the box grows to suit.
        assert!(bb.3 - bb.1 > 35.0, "bowed box is {:?}", bb);
    }

    #[test]
    fn test_hit_testing_follows_the_arc_not_the_chord() {
        let bowed = Shape::Arrow {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(100.0, 0.0),
            color: ColorPreset::Red,
            width: 2.0,
            style: ArrowStyle::Single,
            pattern: StrokePattern::Solid,
            head: ArrowHead::Triangle,
            curve: 40.0,
        };
        // On the arc's apex: a hit.
        assert!(shape_intersects_circle(&bowed, Point2D::new(50.0, 40.0), 6.0));
        // On the straight chord, where the line no longer is: a miss.
        assert!(!shape_intersects_circle(&bowed, Point2D::new(50.0, 0.0), 6.0));
    }

    #[test]
    fn test_trimming_a_polyline_shortens_it_from_the_end() {
        let pts = vec![
            Point2D::new(0.0, 0.0),
            Point2D::new(50.0, 0.0),
            Point2D::new(100.0, 0.0),
        ];
        let cut = trim_polyline_end(&pts, 30.0);
        let last = cut.last().unwrap();
        assert!((last.x - 70.0).abs() < 0.01, "ends at {:?}", last);
        // Trimming past the whole thing leaves nothing to draw, not garbage.
        assert!(trim_polyline_end(&pts, 500.0).len() < 2);
    }

    #[test]
    fn test_align_moves_everything_to_the_outer_edge() {
        let boxes = [(10.0, 0.0, 40.0, 20.0), (100.0, 50.0, 160.0, 90.0)];
        // "Align left" means the leftmost edge, not the average of the two.
        let off = align_offsets(&boxes, AlignTo::Left);
        assert_eq!(off[0], (0.0, 0.0));
        assert_eq!(off[1], (-90.0, 0.0));
        // Right goes the other way, to the rightmost edge.
        let off = align_offsets(&boxes, AlignTo::Right);
        assert_eq!(off[0], (120.0, 0.0));
        assert_eq!(off[1], (0.0, 0.0));
    }

    #[test]
    fn test_align_centres_on_the_group_not_the_first_shape() {
        let boxes = [(0.0, 0.0, 20.0, 10.0), (80.0, 0.0, 100.0, 10.0)];
        let off = align_offsets(&boxes, AlignTo::HCentre);
        // Group spans 0..100, centre 50; each box is 20 wide so each centre
        // must land on 50.
        assert_eq!(off[0].0, 40.0);
        assert_eq!(off[1].0, -40.0);
    }

    #[test]
    fn test_align_is_a_no_op_below_two_shapes() {
        assert_eq!(align_offsets(&[], AlignTo::Left).len(), 0);
        let one = [(0.0, 0.0, 10.0, 10.0)];
        assert_eq!(align_offsets(&one, AlignTo::Left), vec![(0.0, 0.0)]);
    }

    #[test]
    fn test_distribute_equalises_gaps_not_centres() {
        // Different widths: spacing centres evenly would bunch the wide one up
        // against a neighbour. Equal gaps is what looks right.
        let boxes = [
            (0.0, 0.0, 10.0, 10.0),   // width 10
            (20.0, 0.0, 80.0, 10.0),  // width 60, badly placed
            (200.0, 0.0, 210.0, 10.0), // width 10
        ];
        let off = distribute_offsets(&boxes, true);
        // Outermost two are the span and must not move.
        assert_eq!(off[0], (0.0, 0.0));
        assert_eq!(off[2], (0.0, 0.0));
        // Span 0..210 holds 80 of shape, so two gaps of 65 each.
        let moved_left = boxes[1].0 + off[1].0;
        assert!((moved_left - 75.0).abs() < 0.01, "landed at {}", moved_left);
    }

    #[test]
    fn test_distribute_needs_three() {
        let two = [(0.0, 0.0, 10.0, 10.0), (50.0, 0.0, 60.0, 10.0)];
        assert_eq!(distribute_offsets(&two, true), vec![(0.0, 0.0); 2]);
    }

    #[test]
    fn test_distribute_sorts_before_spreading() {
        // Given out of order, the result still spreads them by position rather
        // than by the order they happen to be selected in.
        let boxes = [
            (200.0, 0.0, 210.0, 10.0),
            (0.0, 0.0, 10.0, 10.0),
            (100.0, 0.0, 110.0, 10.0),
        ];
        let off = distribute_offsets(&boxes, true);
        assert_eq!(off[0], (0.0, 0.0));
        assert_eq!(off[1], (0.0, 0.0));
        let mid = boxes[2].0 + off[2].0;
        assert!((mid - 100.0).abs() < 0.01, "middle landed at {}", mid);
    }

    #[test]
    fn test_arrow_head_is_always_wider_than_its_shaft() {
        // This is what broke before: past about 8px the head stopped growing
        // while the shaft kept fattening, until it read as a bar with fins.
        for width in [1.0, 2.0, 6.0, 12.0, 22.0, 36.0] {
            let (head_len, half_width) = arrow_head_size(width, 600.0);
            assert!(
                half_width * 2.0 > width * 2.0,
                "width {}: head {} vs shaft {}",
                width,
                half_width * 2.0,
                width
            );
            assert!(head_len > width, "width {}: head length {}", width, head_len);
        }
    }

    #[test]
    fn test_arrow_head_stays_a_fraction_of_a_short_arrow() {
        // At an ordinary stroke width the length cap holds.
        let (head_len, _) = arrow_head_size(6.0, 60.0);
        assert!(head_len <= 60.0 * 0.45 + 0.01, "{}", head_len);
    }

    #[test]
    fn test_a_stubby_arrow_keeps_a_pointed_head_over_the_length_cap() {
        // 36px of stroke on a 14px arrow cannot satisfy both the length cap
        // and a head that is longer than it is wide. The head wins: a fin is
        // not an arrow, and the shaft simply vanishes.
        let (head_len, half_width) = arrow_head_size(36.0, 14.0);
        assert!(head_len > half_width, "fin: {} x {}", head_len, half_width);
        assert!(head_len > 14.0 * 0.45);
    }

    #[test]
    fn test_shaft_stops_short_of_the_tip() {
        let s = Point2D::new(0.0, 0.0);
        let e = Point2D::new(400.0, 0.0);
        let (head_len, _) = arrow_head_size(12.0, 400.0);
        let (a, b) = arrow_shaft(s, e, head_len, false, true).unwrap();
        assert_eq!(a, s);
        // It ends before the point, so the round cap cannot poke out past it.
        assert!(b.x < e.x - head_len * 0.8, "shaft ends at {:?}", b);
    }

    #[test]
    fn test_shaft_is_pulled_back_at_both_ends_for_a_double_arrow() {
        let s = Point2D::new(0.0, 0.0);
        let e = Point2D::new(400.0, 0.0);
        let (head_len, _) = arrow_head_size(6.0, 400.0);
        let (a, b) = arrow_shaft(s, e, head_len, true, true).unwrap();
        assert!(a.x > 0.0 && b.x < 400.0);
        assert!((a.x - (400.0 - b.x)).abs() < 0.01, "asymmetric: {:?} {:?}", a, b);
    }

    #[test]
    fn test_an_arrow_too_short_for_a_shaft_is_head_only() {
        let s = Point2D::new(0.0, 0.0);
        let e = Point2D::new(14.0, 0.0);
        let (head_len, _) = arrow_head_size(36.0, 14.0);
        // Nothing sensible left to draw as a shaft; the head stands alone
        // rather than the shaft being drawn backwards.
        assert!(arrow_shaft(s, e, head_len, true, true).is_none());
    }

    #[test]
    fn test_head_points_straddle_the_line_and_meet_at_the_tip() {
        let (tip, left, right) = arrow_head_points(
            Point2D::new(0.0, 0.0),
            Point2D::new(100.0, 0.0),
            20.0,
            8.0,
        );
        assert_eq!(tip, Point2D::new(100.0, 0.0));
        // Base sits one head-length back, flanks symmetric about the axis.
        assert!((left.x - 80.0).abs() < 0.01 && (right.x - 80.0).abs() < 0.01);
        assert!((left.y + right.y).abs() < 0.01);
        assert!((left.y.abs() - 8.0).abs() < 0.01);
    }

    #[test]
    fn test_head_points_are_degenerate_for_a_zero_length_arrow() {
        let p = Point2D::new(5.0, 5.0);
        let (tip, left, right) = arrow_head_points(p, p, 20.0, 8.0);
        assert_eq!((tip, left, right), (p, p, p));
    }

    #[test]
    fn test_only_shapes_with_an_edge_can_be_anchored_to() {
        assert!(can_bind_arrow(&rect(0.0, 0.0, 10.0, 10.0)));
        assert!(!can_bind_arrow(&arrow(0.0, 0.0, 10.0, 10.0)));
        assert!(!can_bind_arrow(&Shape::Blur {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(10.0, 10.0),
            block_size: 8.0,
        }));
    }

    #[test]
    fn test_boundary_point_leaves_a_box_through_the_facing_edge() {
        let b = rect(0.0, 0.0, 100.0, 100.0); // centre (50,50), half-extent 50
        // Straight to the right: exits the right edge, plus the gap.
        let p = boundary_point(&b, Point2D::new(500.0, 50.0), 6.0);
        assert!((p.x - 106.0).abs() < 0.01, "{:?}", p);
        assert!((p.y - 50.0).abs() < 0.01);
        // Straight up: exits the top edge.
        let p = boundary_point(&b, Point2D::new(50.0, -500.0), 6.0);
        assert!((p.y - (-6.0)).abs() < 0.01, "{:?}", p);
    }

    #[test]
    fn test_boundary_point_never_overshoots_its_target() {
        // Aiming at something inside the box must not fling the tip past it.
        let b = rect(0.0, 0.0, 100.0, 100.0);
        let toward = Point2D::new(60.0, 50.0);
        let p = boundary_point(&b, toward, 6.0);
        assert!(p.x <= toward.x + 0.01, "{:?}", p);
    }

    #[test]
    fn test_boundary_point_on_a_disc_uses_its_radius() {
        let badge = Shape::StepBadge {
            center: Point2D::new(0.0, 0.0),
            number: 1,
            radius: 20.0,
            color: ColorPreset::Red,
            shape: BadgeShape::Circle,
            fill: FillMode::Solid,
            stroke_width: 0.0,
            pattern: StrokePattern::Solid,
        };
        let p = boundary_point(&badge, Point2D::new(1000.0, 0.0), 5.0);
        assert!((p.x - 25.0).abs() < 0.01, "{:?}", p);
    }

    #[test]
    fn test_unbound_arrows_are_left_exactly_as_drawn() {
        assert!(resolve_arrow_ends(&arrow(0.0, 0.0, 10.0, 10.0), None, None, 6.0).is_none());
    }

    #[test]
    fn test_bound_arrow_ends_sit_on_the_two_boxes() {
        let a = rect(0.0, 0.0, 100.0, 100.0); // centre (50,50)
        let b = rect(300.0, 0.0, 400.0, 100.0); // centre (350,50)
        // Authored anywhere: the resolved ends come from the boxes, not this.
        let arr = arrow(0.0, 0.0, 0.0, 0.0);
        let (s, e) = resolve_arrow_ends(&arr, Some(&a), Some(&b), 6.0).unwrap();
        assert!((s.x - 106.0).abs() < 0.01, "{:?}", s);
        assert!((e.x - 294.0).abs() < 0.01, "{:?}", e);
        assert!((s.y - 50.0).abs() < 0.01 && (e.y - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_one_bound_end_leaves_the_other_where_it_was() {
        let a = rect(0.0, 0.0, 100.0, 100.0);
        let arr = arrow(7.0, 9.0, 400.0, 50.0);
        let (s, e) = resolve_arrow_ends(&arr, Some(&a), None, 6.0).unwrap();
        // The free end is untouched.
        assert_eq!(e, Point2D::new(400.0, 50.0));
        // The bound end left the box toward it.
        assert!(s.x > 100.0, "{:?}", s);
    }

    #[test]
    fn test_moving_the_target_moves_the_bound_end() {
        let arr = arrow(0.0, 0.0, 0.0, 0.0);
        let far = rect(300.0, 0.0, 400.0, 100.0);
        let near = rect(0.0, 0.0, 100.0, 100.0);
        let (s1, _) = resolve_arrow_ends(&arr, Some(&near), Some(&far), 6.0).unwrap();
        // Drop the anchored box 200 down; its end of the arrow must follow.
        let moved = rect(0.0, 200.0, 100.0, 300.0);
        let (s2, _) = resolve_arrow_ends(&arr, Some(&moved), Some(&far), 6.0).unwrap();
        assert!(s2.y > s1.y + 150.0, "{:?} -> {:?}", s1, s2);
    }

    #[test]
    fn test_with_arrow_ends_replaces_only_the_endpoints() {
        let arr = arrow(0.0, 0.0, 10.0, 10.0);
        let out = with_arrow_ends(&arr, Point2D::new(1.0, 2.0), Point2D::new(3.0, 4.0));
        let Shape::Arrow {
            start, end, width, ..
        } = &out
        else {
            panic!("shape changed variant")
        };
        assert_eq!(*start, Point2D::new(1.0, 2.0));
        assert_eq!(*end, Point2D::new(3.0, 4.0));
        assert_eq!(*width, 2.0);
    }

    #[test]
    fn test_lines_hold_labels_that_ride_on_them() {
        let line = Shape::Line {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(10.0, 10.0),
            color: ColorPreset::Red,
            width: 1.0,
            pattern: StrokePattern::Solid,
            curve: 0.0,
        };
        // A line can carry a label, but the label sits on it, not inside it.
        assert!(can_contain_text(&line));
        assert!(label_rides_on_shape(&line));
        // A box holds its label inside, so it can wrap and grow to fit.
        assert!(!label_rides_on_shape(&rect(0.0, 0.0, 10.0, 10.0)));
    }

    #[test]
    fn test_shapes_that_hold_labels() {
        assert!(can_contain_text(&rect(0.0, 0.0, 10.0, 10.0)));
        assert!(can_contain_text(&Shape::Ellipse {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(10.0, 10.0),
            color: ColorPreset::Red,
            width: 1.0,
            fill: FillMode::None,
            pattern: StrokePattern::Solid,
        }));
        // A scribble or a blur patch is not a container.
        assert!(!can_contain_text(&Shape::Blur {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(10.0, 10.0),
            block_size: 8.0,
        }));
    }

    #[test]
    fn test_contained_text_is_centred_both_ways() {
        let o = contained_text_origin((0.0, 0.0, 100.0, 60.0), 40.0, 20.0);
        assert_eq!(o, Point2D::new(30.0, 20.0));
    }

    #[test]
    fn test_contained_text_centres_even_when_it_overflows() {
        // Wider than the box: it still centres, so it spills evenly rather
        // than hanging off one side.
        let o = contained_text_origin((0.0, 0.0, 50.0, 60.0), 90.0, 20.0);
        assert_eq!(o.x, -20.0);
    }

    #[test]
    fn test_container_grows_for_a_tall_label_but_never_shrinks() {
        let box_ = (0.0, 0.0, 100.0, 60.0);
        // Tall label: the box has to grow to text + padding on both sides.
        assert_eq!(container_height_for(box_, 80.0, 10.0), 100.0);
        // Short label leaves the height the user chose alone.
        assert_eq!(container_height_for(box_, 10.0, 10.0), 60.0);
    }

    #[test]
    fn test_anchor_points_ignores_freehand_scribbles() {
        let s = Shape::Stroke {
            points: vec![Point2D::new(0.0, 0.0), Point2D::new(10.0, 10.0)],
            color: ColorPreset::Red,
            width: 2.0,
            is_highlighter: false,
            pattern: StrokePattern::Solid,
            pressures: Vec::new(),
        };
        assert!(anchor_points(&s).is_empty());
    }

    #[test]
    fn test_anchor_points_of_a_box_are_corners_edges_and_centre() {
        let pts = anchor_points(&rect(0.0, 0.0, 100.0, 60.0));
        assert_eq!(pts.len(), 9);
        for expected in [
            Point2D::new(0.0, 0.0),
            Point2D::new(100.0, 60.0),
            Point2D::new(50.0, 0.0),
            Point2D::new(50.0, 30.0),
        ] {
            assert!(pts.contains(&expected), "missing {:?} in {:?}", expected, pts);
        }
    }

    #[test]
    fn test_anchor_points_of_a_line_are_its_own_ends_not_its_box() {
        // A diagonal line's bounding-box corners are not on the line, so
        // offering them would snap things to empty space.
        let s = Shape::Line {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(100.0, 100.0),
            color: ColorPreset::Red,
            width: 2.0,
            pattern: StrokePattern::Solid,
            curve: 0.0,
        };
        let pts = anchor_points(&s);
        assert_eq!(pts.len(), 3);
        assert!(pts.contains(&Point2D::new(50.0, 50.0)));
        assert!(!pts.contains(&Point2D::new(100.0, 0.0)));
    }

    #[test]
    fn test_snap_point_lands_on_a_nearby_anchor() {
        let anchors = anchor_points(&rect(0.0, 0.0, 100.0, 60.0));
        let (p, guides) = snap_point(Point2D::new(103.0, 62.0), &anchors, 8.0).unwrap();
        assert_eq!(p, Point2D::new(100.0, 60.0));
        assert_eq!(guides.len(), 1);
        assert!(guides[0].marker);
    }

    #[test]
    fn test_snap_point_aligns_each_axis_independently() {
        // Far from any single anchor, but level with one on x and another on y.
        let anchors = vec![Point2D::new(200.0, 10.0), Point2D::new(10.0, 400.0)];
        let (p, guides) = snap_point(Point2D::new(203.0, 397.0), &anchors, 8.0).unwrap();
        assert_eq!(p, Point2D::new(200.0, 400.0));
        assert_eq!(guides.len(), 2);
        assert!(guides.iter().all(|g| !g.marker));
    }

    #[test]
    fn test_snap_point_leaves_distant_points_alone() {
        let anchors = anchor_points(&rect(0.0, 0.0, 100.0, 60.0));
        assert!(snap_point(Point2D::new(500.0, 500.0), &anchors, 8.0).is_none());
    }

    #[test]
    fn test_snap_translation_pulls_a_dragged_box_into_line() {
        let moving = rect(0.0, 0.0, 50.0, 50.0);
        let anchors = anchor_points(&rect(200.0, 103.0, 300.0, 163.0));
        // Dropping it 100 down puts its top edge 3px off the other's top edge.
        let (dx, dy, guides) = snap_translation(&moving, 0.0, 100.0, &anchors, 8.0);
        assert_eq!(dx, 0.0);
        assert_eq!(dy, 103.0);
        assert!(!guides.is_empty());
    }

    #[test]
    fn test_snap_translation_is_a_no_op_when_nothing_is_close() {
        let moving = rect(0.0, 0.0, 50.0, 50.0);
        let anchors = anchor_points(&rect(900.0, 900.0, 950.0, 950.0));
        let (dx, dy, guides) = snap_translation(&moving, 7.0, 11.0, &anchors, 8.0);
        assert_eq!((dx, dy), (7.0, 11.0));
        assert!(guides.is_empty());
    }

    #[test]
    fn test_snapping_off_when_there_is_nothing_to_snap_to() {
        let moving = rect(0.0, 0.0, 50.0, 50.0);
        let (dx, dy, guides) = snap_translation(&moving, 3.0, 4.0, &[], 8.0);
        assert_eq!((dx, dy), (3.0, 4.0));
        assert!(guides.is_empty());
        assert!(snap_point(Point2D::new(1.0, 1.0), &[], 8.0).is_none());
    }

    #[test]
    fn test_pressure_width_factor_is_monotonic_and_bounded() {
        let light = pressure_width_factor(0.0);
        let mid = pressure_width_factor(0.5);
        let hard = pressure_width_factor(1.0);
        assert!(light < mid && mid < hard);
        // A feather touch stays visible; a hard press stays near the nib size.
        assert!(light > 0.2, "{}", light);
        assert!(hard < 1.6, "{}", hard);
        // Out-of-range readings clamp rather than invert the stroke.
        assert_eq!(pressure_width_factor(-1.0), light);
        assert_eq!(pressure_width_factor(9.0), hard);
    }

    #[test]
    fn test_push_pressure_ignores_strokes_that_never_had_any() {
        // A mouse stroke must stay on the cheaper uniform-width path.
        let mut p: Vec<f32> = Vec::new();
        push_pressure(&mut p, Some(0.8), 5);
        assert!(p.is_empty());
    }

    #[test]
    fn test_push_pressure_tracks_point_count() {
        let mut p = vec![0.5];
        push_pressure(&mut p, Some(0.6), 2);
        push_pressure(&mut p, Some(0.7), 3);
        assert_eq!(p, vec![0.5, 0.6, 0.7]);
    }

    #[test]
    fn test_push_pressure_repeats_last_sample_when_the_pen_goes_quiet() {
        let mut p = vec![0.4];
        push_pressure(&mut p, None, 2);
        assert_eq!(p, vec![0.4, 0.4]);
    }

    #[test]
    fn test_push_pressure_resyncs_after_a_dropped_message() {
        // The renderer only honours a track the same length as the points, so a
        // gap must be filled rather than left short.
        let mut p = vec![0.3];
        push_pressure(&mut p, Some(0.9), 4);
        assert_eq!(p.len(), 4);
        assert_eq!(p[0], 0.3);
        assert!(p[1..].iter().all(|v| *v == 0.9));
    }

    #[test]
    fn test_push_pressure_trims_if_it_ever_runs_ahead() {
        let mut p = vec![0.1, 0.2, 0.3, 0.4];
        push_pressure(&mut p, Some(0.5), 2);
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn test_shape_bounds_stroke_covers_every_point_plus_half_width() {
        let s = Shape::Stroke {
            points: vec![
                Point2D::new(10.0, 40.0),
                Point2D::new(60.0, 10.0),
                Point2D::new(30.0, 70.0),
            ],
            color: ColorPreset::Red,
            width: 4.0,
            is_highlighter: false,
            pressures: Vec::new(),
            pattern: StrokePattern::Solid,
        };
        assert_eq!(shape_bounds(&s), (8.0, 8.0, 62.0, 72.0));
    }

    #[test]
    fn test_shape_bounds_badge_is_square_around_centre() {
        let s = Shape::StepBadge {
            center: Point2D::new(100.0, 100.0),
            number: 1,
            radius: 20.0,
            color: ColorPreset::Red,
            shape: BadgeShape::Circle,
            fill: FillMode::Solid,
            stroke_width: 2.0,
            pattern: StrokePattern::Solid,
        };
        assert_eq!(shape_bounds(&s), (79.0, 79.0, 121.0, 121.0));
    }

    #[test]
    fn test_translate_moves_every_stroke_point() {
        let mut s = Shape::Stroke {
            points: vec![Point2D::new(0.0, 0.0), Point2D::new(10.0, 20.0)],
            color: ColorPreset::Red,
            width: 2.0,
            is_highlighter: false,
            pressures: Vec::new(),
            pattern: StrokePattern::Solid,
        };
        translate_shape(&mut s, 5.0, -3.0);
        let Shape::Stroke { points, .. } = &s else {
            panic!("shape changed variant")
        };
        assert_eq!(points[0], Point2D::new(5.0, -3.0));
        assert_eq!(points[1], Point2D::new(15.0, 17.0));
    }

    #[test]
    fn test_resized_bounds_moves_only_the_dragged_edges() {
        let orig = (0.0, 0.0, 100.0, 50.0);
        // The east grip moves the right edge and leaves the rest alone.
        assert_eq!(
            resized_bounds(orig, SelectionHandle::E, 20.0, 999.0),
            (0.0, 0.0, 120.0, 50.0)
        );
        // The north-west grip moves left and top together.
        assert_eq!(
            resized_bounds(orig, SelectionHandle::NW, 10.0, 5.0),
            (10.0, 5.0, 100.0, 50.0)
        );
    }

    #[test]
    fn test_resized_bounds_cannot_invert_or_collapse() {
        let orig = (0.0, 0.0, 100.0, 50.0);
        // Dragging the west grip far past the right edge stops at the minimum.
        let b = resized_bounds(orig, SelectionHandle::W, 500.0, 0.0);
        assert_eq!(b.0, 100.0 - MIN_SHAPE_EXTENT);
        assert!(b.2 - b.0 >= MIN_SHAPE_EXTENT);
        // Same going the other way on the south grip.
        let b = resized_bounds(orig, SelectionHandle::S, 0.0, -500.0);
        assert!(b.3 - b.1 >= MIN_SHAPE_EXTENT);
    }

    #[test]
    fn test_resize_maps_old_box_onto_new_box() {
        let mut s = rect(0.0, 0.0, 100.0, 100.0);
        let from = shape_bounds(&s);
        let to = (0.0, 0.0, 200.0, 50.0);
        resize_shape(&mut s, from, to);
        assert_eq!(shape_bounds(&s), to);
    }

    #[test]
    fn test_resize_keeps_stroke_weight() {
        // Growing a box must not re-weight its outline, or repeated drags
        // would compound into a blob.
        let mut s = Shape::Rectangle {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(50.0, 50.0),
            color: ColorPreset::Red,
            width: 6.0,
            rounded: false,
            fill: FillMode::None,
            pattern: StrokePattern::Solid,
        };
        let from = shape_bounds(&s);
        resize_shape(&mut s, from, (0.0, 0.0, 400.0, 400.0));
        let Shape::Rectangle { width, .. } = &s else {
            panic!("shape changed variant")
        };
        assert_eq!(*width, 6.0);
    }

    #[test]
    fn test_resize_badge_stays_circular_and_centred() {
        let mut s = Shape::StepBadge {
            center: Point2D::new(0.0, 0.0),
            number: 3,
            radius: 10.0,
            color: ColorPreset::Red,
            shape: BadgeShape::Circle,
            fill: FillMode::Solid,
            stroke_width: 0.0,
            pattern: StrokePattern::Solid,
        };
        let from = shape_bounds(&s);
        // A wide, short target box: the disc inscribes it rather than stretching.
        resize_shape(&mut s, from, (0.0, 0.0, 200.0, 60.0));
        let Shape::StepBadge { center, radius, .. } = &s else {
            panic!("shape changed variant")
        };
        assert_eq!(*center, Point2D::new(100.0, 30.0));
        assert_eq!(*radius, 30.0);
    }

    #[test]
    fn test_resize_then_bounds_match_for_every_variant() {
        // Whatever the shape, the box it reports after a resize is the box the
        // drag asked for — this is what keeps the grips under the cursor.
        let shapes = vec![
            rect(10.0, 10.0, 60.0, 40.0),
            Shape::Ellipse {
                start: Point2D::new(10.0, 10.0),
                end: Point2D::new(60.0, 40.0),
                color: ColorPreset::Red,
                width: 3.0,
                fill: FillMode::None,
                pattern: StrokePattern::Solid,
            },
            Shape::Line {
                start: Point2D::new(10.0, 10.0),
                end: Point2D::new(60.0, 40.0),
                color: ColorPreset::Red,
                width: 5.0,
                pattern: StrokePattern::Solid,
                curve: 0.0,
            },
            Shape::Blur {
                start: Point2D::new(10.0, 10.0),
                end: Point2D::new(60.0, 40.0),
                block_size: 14.0,
            },
            Shape::Stroke {
                points: vec![
                    Point2D::new(10.0, 10.0),
                    Point2D::new(35.0, 25.0),
                    Point2D::new(60.0, 40.0),
                ],
                color: ColorPreset::Red,
                width: 8.0,
                is_highlighter: false,
                pressures: Vec::new(),
                pattern: StrokePattern::Solid,
            },
        ];
        let to = (100.0, 200.0, 180.0, 260.0);
        for mut s in shapes {
            let from = shape_bounds(&s);
            resize_shape(&mut s, from, to);
            let got = shape_bounds(&s);
            for (a, b) in [(got.0, to.0), (got.1, to.1), (got.2, to.2), (got.3, to.3)] {
                assert!((a - b).abs() < 0.01, "{:?} != {:?}", got, to);
            }
        }
    }

    #[test]
    fn test_handle_at_finds_corners_and_misses_the_middle() {
        let b = (0.0, 0.0, 100.0, 60.0);
        assert_eq!(
            handle_at(b, Point2D::new(0.0, 0.0)),
            Some(SelectionHandle::NW)
        );
        assert_eq!(
            handle_at(b, Point2D::new(100.0, 30.0)),
            Some(SelectionHandle::E)
        );
        // Dead centre is the body, not a grip.
        assert_eq!(handle_at(b, Point2D::new(50.0, 30.0)), None);
    }

    #[test]
    fn test_handle_at_tolerates_a_few_pixels_of_slop() {
        let b = (0.0, 0.0, 100.0, 60.0);
        let reach = SELECTION_HANDLE_SIZE * 0.5 + SELECTION_HANDLE_SLOP;
        assert!(handle_at(b, Point2D::new(reach - 0.5, reach - 0.5)).is_some());
        assert!(handle_at(b, Point2D::new(reach + 2.0, reach + 2.0)).is_none());
    }

    #[test]
    fn test_text_resize_scales_font_not_just_position() {
        let mut s = Shape::Text {
            origin: Point2D::new(0.0, 0.0),
            text: "hello".to_string(),
            font_size: 20.0,
            color: ColorPreset::Red,
            is_bold: false,
            is_italic: false,
            card_style: TextCardStyle::Badge,
            font_family: TextFontFamily::SegoeUI,
        };
        let from = shape_bounds(&s);
        let to = (0.0, 0.0, from.2, from.3 * 2.0);
        resize_shape(&mut s, from, to);
        let Shape::Text { font_size, .. } = &s else {
            panic!("shape changed variant")
        };
        assert!((*font_size - 40.0).abs() < 0.01, "got {}", font_size);
    }

    #[test]
    fn test_shape_eraser_blur_intersection() {
        let blur = Shape::Blur {
            start: Point2D::new(100.0, 100.0),
            end: Point2D::new(200.0, 200.0),
            block_size: 14.0,
        };
        // Center of blur rect
        assert!(shape_intersects_circle(
            &blur,
            Point2D::new(150.0, 150.0),
            10.0
        ));
        // Outside blur rect
        assert!(!shape_intersects_circle(
            &blur,
            Point2D::new(300.0, 300.0),
            10.0
        ));
    }
}
