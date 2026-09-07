use crate::types::{
    ColorPreset, FillMode, Point2D, SelectionHandle, Shape, StrokePattern,
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

pub fn calculate_arrow_head(
    start: Point2D,
    end: Point2D,
    head_length: f32,
) -> (Point2D, Point2D, Point2D) {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length = (dx * dx + dy * dy).sqrt();

    if length < 2.0 {
        return (end, end, end);
    }

    let ux = dx / length;
    let uy = dy / length;

    let max_head = (length * 0.5).max(1.0);
    let actual_head_len = head_length.clamp(6.0, 48.0).min(max_head);
    let head_angle: f32 = 28.0f32.to_radians();

    let base_x = end.x - ux * actual_head_len;
    let base_y = end.y - uy * actual_head_len;

    let perp_x = -uy;
    let perp_y = ux;

    let half_width = actual_head_len * head_angle.tan();

    let left = Point2D {
        x: base_x + perp_x * half_width,
        y: base_y + perp_y * half_width,
    };

    let right = Point2D {
        x: base_x - perp_x * half_width,
        y: base_y - perp_y * half_width,
    };

    (end, left, right)
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
            start, end, width, ..
        }
        | Shape::Arrow {
            start, end, width, ..
        } => {
            let threshold = radius + *width / 2.0;
            point_to_segment_distance(center, *start, *end) <= threshold
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
        Shape::Line { start, end, width, .. } | Shape::Arrow { start, end, width, .. } => {
            let (l, t, r, b) = normalize_rect(*start, *end);
            let half = width * 0.5;
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
    use crate::types::{BadgeShape, TextCardStyle, TextFontFamily};

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
    fn test_calculate_arrow_head() {
        let start = Point2D::new(0.0, 0.0);
        let end = Point2D::new(100.0, 0.0);
        let (tip, left, right) = calculate_arrow_head(start, end, 20.0);
        assert_eq!(tip.x, 100.0);
        assert_eq!(tip.y, 0.0);
        assert!(left.x < 100.0);
        assert!(right.x < 100.0);
        assert!(left.y != right.y);
    }

    #[test]
    fn test_calculate_short_arrow_head() {
        let start = Point2D::new(0.0, 0.0);
        let end = Point2D::new(10.0, 0.0);
        let (tip, left, right) = calculate_arrow_head(start, end, 20.0);
        assert_eq!(tip.x, 10.0);
        assert_eq!(tip.y, 0.0);
        assert!(left.x >= start.x);
        assert!(right.x >= start.x);
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
