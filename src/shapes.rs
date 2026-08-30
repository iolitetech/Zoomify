#![allow(dead_code)]

use crate::types::Point2D;

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

pub fn calculate_arrow_head(start: Point2D, end: Point2D, head_length: f32) -> (Point2D, Point2D, Point2D) {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length = (dx * dx + dy * dy).sqrt();

    if length < 2.0 {
        return (end, end, end);
    }

    let ux = dx / length;
    let uy = dy / length;

    let actual_head_len = head_length.min(length * 0.45).max(12.0);
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
