use super::{D2DRenderer, v2};
use crate::shapes::{
    SELECTION_HANDLE_SIZE, arrow_head_points, arrow_head_size, arrow_shaft, contained_text_origin,
    normalize_rect, points_to_bezier_segments, pressure_stroke_figures, pressure_width_factor,
    sample_curve, selection_handle_points, trim_polyline_end,
};
use crate::types::{
    ArrowHead, ArrowStyle, BadgeShape, ColorPreset, FillMode, LaserRipple, LaserTrailPoint,
    Point2D, Shape, StrokePattern, TextCardStyle, TextEditorState,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
    D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_ROUNDED_RECT, ID2D1Bitmap, ID2D1Brush, ID2D1Factory, ID2D1PathGeometry, ID2D1RenderTarget,
    ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::DWRITE_HIT_TEST_METRICS;
use windows_core::Interface;

/// Once the geometry cache holds more entries than this, a new insert clears
/// it outright rather than growing further. There is no per-entry eviction,
/// so without a cap a long freehand stroke (one new cache entry per point
/// while it is still being drawn) or a long drag (one entry per unique
/// position) would otherwise grow the cache for as long as the gesture lasts
/// and never give any of it back.
const GEOMETRY_CACHE_MAX_ENTRIES: usize = 512;

/// Each entry is its own small render target (a real GPU resource), so this
/// stays far smaller than the geometry cache's cap - a session redacting
/// dozens of distinct regions is already an unusual amount of blur.
const BLUR_MOSAIC_CACHE_MAX_ENTRIES: usize = 64;

impl D2DRenderer {
    /// Build a stroke's outline as a hollow path geometry: a straight polyline
    /// for two points, or the same Bezier smoothing `points_to_bezier_segments`
    /// produces for three or more. Shared by the cached and uncached draw
    /// paths in `render_single_shape` so there is exactly one place that
    /// builds this geometry.
    unsafe fn build_stroke_geometry(
        factory: &ID2D1Factory,
        points: &[Point2D],
    ) -> Option<ID2D1PathGeometry> {
        unsafe {
            let path = factory.CreatePathGeometry().ok()?;
            let sink = path.Open().ok()?;
            sink.BeginFigure(v2(points[0].x, points[0].y), D2D1_FIGURE_BEGIN_HOLLOW);
            if points.len() >= 3 {
                let segments = points_to_bezier_segments(points);
                for (c1, c2, p) in segments {
                    let bz = D2D1_BEZIER_SEGMENT {
                        point1: v2(c1.x, c1.y),
                        point2: v2(c2.x, c2.y),
                        point3: v2(p.x, p.y),
                    };
                    sink.AddBezier(&bz);
                }
            } else {
                for pt in &points[1..] {
                    sink.AddLine(v2(pt.x, pt.y));
                }
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            let _ = sink.Close();
            Some(path)
        }
    }

    pub(super) unsafe fn render_single_shape(
        &self,
        rt: &ID2D1RenderTarget,
        shape: &Shape,
        bg_bitmap: Option<&ID2D1Bitmap>,
        // Scales every colour's alpha. It lives on the Annotation because it
        // applies to all shapes equally and none of them care what it is.
        opacity: f32,
        // Whether this shape's geometry is worth caching at all. False for
        // the shape still being drawn or dragged: its points differ every
        // frame, so every cache lookup for it would miss and every draw
        // would insert a brand-new entry that is never reused - pure growth
        // with none of the caching benefit. True for a committed annotation,
        // whose geometry is stable from one frame to the next once the
        // gesture that produced it ends.
        cacheable: bool,
    ) {
        unsafe {
            match shape {
                Shape::Stroke {
                    points,
                    color,
                    width,
                    is_highlighter,
                    pattern,
                    pressures,
                } => {
                    if points.is_empty() {
                        return;
                    }

                    let alpha = if *is_highlighter { 0.45 } else { 1.0 };
                    let actual_width = if *is_highlighter {
                        *width * 2.2
                    } else {
                        *width
                    };
                    let col = color.to_d2d_color((alpha) * opacity);

                    // A pen stroke carries a pressure per point, so it is drawn
                    // segment by segment with a width that follows the press.
                    // Bezier smoothing needs one width for the whole figure, so
                    // it only applies to the uniform case.
                    let has_pressure = !pressures.is_empty() && pressures.len() == points.len();

                    if let Some(brush) = self.solid_brush(rt, &col) {
                        if points.len() == 1 {
                            let w = if has_pressure {
                                actual_width * pressure_width_factor(pressures[0])
                            } else {
                                actual_width
                            };
                            let dot = D2D1_ELLIPSE {
                                point: v2(points[0].x, points[0].y),
                                radiusX: w / 2.0,
                                radiusY: w / 2.0,
                            };
                            rt.FillEllipse(&dot, &brush);
                        } else if has_pressure {
                            // A committed solid stroke is one cached filled
                            // shape instead of a DrawLine per segment (25k a
                            // frame across 50 long strokes). The stroke still
                            // being drawn changes every frame, and a dashed
                            // pattern is applied per segment, so both keep the
                            // segment-by-segment path below.
                            let cached = if cacheable && *pattern == StrokePattern::Solid {
                                self.pressure_stroke_geometry(rt, points, pressures, actual_width)
                            } else {
                                None
                            };
                            if let Some(path) = cached {
                                rt.FillGeometry(&path, &brush, None);
                            } else {
                                let stroke_style = self.get_stroke_style(*pattern);
                                for i in 0..points.len() - 1 {
                                    // Average the endpoints so neighbouring segments
                                    // meet at the same width and the line reads as
                                    // one tapering stroke rather than a staircase.
                                    let f = (pressure_width_factor(pressures[i])
                                        + pressure_width_factor(pressures[i + 1]))
                                        * 0.5;
                                    rt.DrawLine(
                                        v2(points[i].x, points[i].y),
                                        v2(points[i + 1].x, points[i + 1].y),
                                        &brush,
                                        actual_width * f,
                                        Some(stroke_style),
                                    );
                                }
                            }
                        } else if cacheable {
                            // Keyed on the point *values*, not the Vec's address:
                            // a moved/dragged/re-cloned Vec of identical points at
                            // an identical width must hit the same cached
                            // geometry, and a freed-then-reused allocation must
                            // never be mistaken for a different stroke that
                            // happens to share its old address and length.
                            let key = {
                                use std::hash::{Hash, Hasher};
                                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                for p in points.iter() {
                                    p.x.to_bits().hash(&mut hasher);
                                    p.y.to_bits().hash(&mut hasher);
                                }
                                actual_width.to_bits().hash(&mut hasher);
                                hasher.finish()
                            };
                            let mut cache = self.geometry_cache.borrow_mut();
                            let rt_id = self.rt_key(rt);
                            if cache.0 != rt_id {
                                cache.0 = rt_id;
                                cache.1.clear();
                            }
                            let path = if let Some(p) = cache.1.get(&key) {
                                Some(p.clone())
                            } else {
                                let built = Self::build_stroke_geometry(&self.factory, points);
                                if let Some(p) = &built {
                                    if cache.1.len() > GEOMETRY_CACHE_MAX_ENTRIES {
                                        cache.1.clear();
                                    }
                                    cache.1.insert(key, p.clone());
                                }
                                built
                            };
                            if let Some(path) = path {
                                let stroke_style = self.get_stroke_style(*pattern);
                                rt.DrawGeometry(&path, &brush, actual_width, Some(stroke_style));
                            }
                        } else if let Some(path) =
                            Self::build_stroke_geometry(&self.factory, points)
                        {
                            let stroke_style = self.get_stroke_style(*pattern);
                            rt.DrawGeometry(&path, &brush, actual_width, Some(stroke_style));
                        }
                    }
                }

                Shape::Line {
                    start,
                    end,
                    color,
                    width,
                    pattern,
                    curve,
                } => {
                    let col = color.to_d2d_color((1.0) * opacity);
                    if let Some(brush) = self.solid_brush(rt, &col) {
                        let stroke_style = self.get_stroke_style(*pattern);
                        if curve.abs() < 0.01 {
                            rt.DrawLine(
                                v2(start.x, start.y),
                                v2(end.x, end.y),
                                &brush,
                                *width,
                                Some(stroke_style),
                            );
                        } else {
                            self.stroke_polyline(
                                rt,
                                &sample_curve(*start, *end, *curve),
                                &brush,
                                *width,
                                Some(stroke_style),
                            );
                        }
                    }
                }

                Shape::Arrow {
                    start,
                    end,
                    color,
                    width,
                    style,
                    pattern,
                    head,
                    curve,
                } => {
                    let col = color.to_d2d_color((1.0) * opacity);
                    if let Some(brush) = self.solid_brush(rt, &col) {
                        let length = start.distance(end);
                        let (head_len, half_width) = arrow_head_size(*width, length);
                        let head_at_start =
                            *style == ArrowStyle::Double || *style == ArrowStyle::Dimension;

                        // The shaft stops where the head begins. Running it to
                        // the tip and filling the head over it leaves the round
                        // cap poking out past the point, and swallows the head
                        // entirely once the stroke gets thick.
                        // Only a filled head hides the shaft's end. An open,
                        // circle or bar head would leave a visible gap if the
                        // shaft stopped short, so for those it runs to the tip.
                        let hides_shaft = matches!(head, ArrowHead::Triangle | ArrowHead::Diamond);
                        let inset = if hides_shaft { head_len * 0.92 } else { 0.0 };
                        let stroke_style = self.get_stroke_style(*pattern);
                        if curve.abs() < 0.01 {
                            if let Some((s, e)) =
                                arrow_shaft(*start, *end, head_len, head_at_start, true)
                            {
                                rt.DrawLine(
                                    v2(s.x, s.y),
                                    v2(e.x, e.y),
                                    &brush,
                                    *width,
                                    Some(stroke_style),
                                );
                            }
                        } else {
                            // Trim along the arc rather than the chord, or the
                            // shaft would stop in the wrong place on a deep bow.
                            let pts = sample_curve(*start, *end, *curve);
                            let pts = trim_polyline_end(&pts, inset);
                            let pts = if head_at_start {
                                let mut r: Vec<Point2D> = pts.into_iter().rev().collect();
                                r = trim_polyline_end(&r, inset);
                                r.into_iter().rev().collect()
                            } else {
                                pts
                            };
                            self.stroke_polyline(rt, &pts, &brush, *width, Some(stroke_style));
                        }

                        let fill_head = |from: Point2D, to: Point2D| {
                            let (tip, left, right) =
                                arrow_head_points(from, to, head_len, half_width);
                            // An arrow head is a 3-4 point path, cheap enough to
                            // build fresh every frame - simpler and safer than
                            // caching it by (from, to, head_len, half_width): a
                            // dragged or bound-and-following arrow changes those
                            // float coordinates on nearly every frame, so a
                            // cache here only ever inserted new entries and
                            // never evicted any, growing for as long as the
                            // arrow existed.
                            let filled = |pts: &[Point2D]| {
                                if let Ok(path) = self.factory.CreatePathGeometry()
                                    && let Ok(sink) = path.Open()
                                {
                                    sink.BeginFigure(
                                        v2(pts[0].x, pts[0].y),
                                        D2D1_FIGURE_BEGIN_FILLED,
                                    );
                                    for p in &pts[1..] {
                                        sink.AddLine(v2(p.x, p.y));
                                    }
                                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                                    let _ = sink.Close();
                                    rt.FillGeometry(&path, &brush, None);
                                }
                            };
                            match head {
                                ArrowHead::Triangle => filled(&[tip, left, right]),
                                ArrowHead::Open => {
                                    // Two strokes to the tip, leaving it open.
                                    rt.DrawLine(
                                        v2(left.x, left.y),
                                        v2(tip.x, tip.y),
                                        &brush,
                                        *width,
                                        None,
                                    );
                                    rt.DrawLine(
                                        v2(right.x, right.y),
                                        v2(tip.x, tip.y),
                                        &brush,
                                        *width,
                                        None,
                                    );
                                }
                                ArrowHead::Circle => {
                                    // Set back from the tip by its own radius,
                                    // so the disc ends where a point would.
                                    let r = half_width * 0.8;
                                    let ux = tip.x - (left.x + right.x) * 0.5;
                                    let uy = tip.y - (left.y + right.y) * 0.5;
                                    let len = (ux * ux + uy * uy).sqrt().max(0.001);
                                    let c = D2D1_ELLIPSE {
                                        point: v2(tip.x - ux / len * r, tip.y - uy / len * r),
                                        radiusX: r,
                                        radiusY: r,
                                    };
                                    rt.FillEllipse(&c, &brush);
                                }
                                ArrowHead::Diamond => {
                                    let mid = Point2D::new(
                                        (left.x + right.x) * 0.5,
                                        (left.y + right.y) * 0.5,
                                    );
                                    let back = Point2D::new(
                                        mid.x - (tip.x - mid.x),
                                        mid.y - (tip.y - mid.y),
                                    );
                                    filled(&[tip, left, back, right]);
                                }
                                ArrowHead::Bar => {
                                    rt.DrawLine(
                                        v2(
                                            tip.x + (left.x - right.x) * 0.5,
                                            tip.y + (left.y - right.y) * 0.5,
                                        ),
                                        v2(
                                            tip.x - (left.x - right.x) * 0.5,
                                            tip.y - (left.y - right.y) * 0.5,
                                        ),
                                        &brush,
                                        *width * 1.3,
                                        None,
                                    );
                                }
                            }
                        };

                        // A bowed arrow's head follows the tangent where the
                        // curve actually arrives, not the chord between ends.
                        let (aim_end, aim_start) = if curve.abs() < 0.01 {
                            (*start, *end)
                        } else {
                            let pts = sample_curve(*start, *end, *curve);
                            if pts.len() < 2 {
                                return;
                            }
                            (pts[pts.len() - 2], pts[1])
                        };
                        fill_head(aim_end, *end);
                        if head_at_start {
                            fill_head(aim_start, *start);
                        }

                        // Dimension style: perpendicular ticks at both ends.
                        if *style == ArrowStyle::Dimension && length > 1.0 {
                            let ux = (end.x - start.x) / length;
                            let uy = (end.y - start.y) / length;
                            let (px, py) = (-uy, ux);
                            let tick = half_width * 1.1;
                            for p in [start, end] {
                                rt.DrawLine(
                                    v2(p.x + px * tick, p.y + py * tick),
                                    v2(p.x - px * tick, p.y - py * tick),
                                    &brush,
                                    *width * 1.2,
                                    None,
                                );
                            }
                        }
                    }
                }

                Shape::Rectangle {
                    start,
                    end,
                    color,
                    width,
                    rounded,
                    fill,
                    pattern,
                } => {
                    let (left, top, right, bottom) = normalize_rect(*start, *end);
                    let rect = D2D_RECT_F {
                        left,
                        top,
                        right,
                        bottom,
                    };
                    let rrect = D2D1_ROUNDED_RECT {
                        rect,
                        radiusX: 12.0,
                        radiusY: 12.0,
                    };

                    // Fill if requested
                    match fill {
                        FillMode::Tinted => {
                            let fill_col = color.to_d2d_color((0.22) * opacity);
                            if let Some(fbrush) = self.solid_brush(rt, &fill_col) {
                                if *rounded {
                                    rt.FillRoundedRectangle(&rrect, &fbrush);
                                } else {
                                    rt.FillRectangle(&rect, &fbrush);
                                }
                            }
                        }
                        FillMode::Solid => {
                            let fill_col = color.to_d2d_color((1.0) * opacity);
                            if let Some(fbrush) = self.solid_brush(rt, &fill_col) {
                                if *rounded {
                                    rt.FillRoundedRectangle(&rrect, &fbrush);
                                } else {
                                    rt.FillRectangle(&rect, &fbrush);
                                }
                            }
                        }
                        FillMode::None => {}
                    }

                    // Stroke border
                    let col = color.to_d2d_color((1.0) * opacity);
                    if let Some(brush) = self.solid_brush(rt, &col) {
                        let stroke_style = self.get_stroke_style(*pattern);
                        if *rounded {
                            rt.DrawRoundedRectangle(&rrect, &brush, *width, Some(stroke_style));
                        } else {
                            rt.DrawRectangle(&rect, &brush, *width, Some(stroke_style));
                        }
                    }
                }

                Shape::Ellipse {
                    start,
                    end,
                    color,
                    width,
                    fill,
                    pattern,
                } => {
                    let (left, top, right, bottom) = normalize_rect(*start, *end);
                    let cx = (left + right) / 2.0;
                    let cy = (top + bottom) / 2.0;
                    let rx = (right - left) / 2.0;
                    let ry = (bottom - top) / 2.0;

                    let ellipse = D2D1_ELLIPSE {
                        point: v2(cx, cy),
                        radiusX: rx,
                        radiusY: ry,
                    };

                    match fill {
                        FillMode::Tinted => {
                            let fill_col = color.to_d2d_color((0.22) * opacity);
                            if let Some(fbrush) = self.solid_brush(rt, &fill_col) {
                                rt.FillEllipse(&ellipse, &fbrush);
                            }
                        }
                        FillMode::Solid => {
                            let fill_col = color.to_d2d_color((1.0) * opacity);
                            if let Some(fbrush) = self.solid_brush(rt, &fill_col) {
                                rt.FillEllipse(&ellipse, &fbrush);
                            }
                        }
                        FillMode::None => {}
                    }

                    let col = color.to_d2d_color((1.0) * opacity);
                    if let Some(brush) = self.solid_brush(rt, &col) {
                        let stroke_style = self.get_stroke_style(*pattern);
                        rt.DrawEllipse(&ellipse, &brush, *width, Some(stroke_style));
                    }
                }

                Shape::Text {
                    origin,
                    text,
                    font_size,
                    color,
                    is_bold,
                    is_italic,
                    card_style,
                    font_family,
                } => {
                    let col = color.to_d2d_color((1.0) * opacity);
                    // One cached IDWriteTextLayout serves both the box math
                    // below and the two DrawTextLayout calls further down -
                    // previously this measured with one layout and then
                    // DrawText built a second, internal one on every draw.
                    if let Some((text_layout, block_w, block_h)) = self.get_or_build_text_layout(
                        text,
                        *font_size,
                        *is_bold,
                        *is_italic,
                        *font_family,
                        f32::MAX,
                    ) {
                        let layout_w = block_w.max(30.0) + 16.0;
                        let layout_h = block_h + 8.0;

                        let text_rect = D2D_RECT_F {
                            left: origin.x,
                            top: origin.y,
                            right: origin.x + layout_w,
                            bottom: origin.y + layout_h,
                        };

                        let card_rrect = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: origin.x - 6.0,
                                top: origin.y - 4.0,
                                right: origin.x + layout_w + 6.0,
                                bottom: origin.y + layout_h + 2.0,
                            },
                            radiusX: 6.0,
                            radiusY: 6.0,
                        };

                        match card_style {
                            TextCardStyle::Badge => {
                                if let Some(card_bg) = self.solid_brush(
                                    rt,
                                    &D2D1_COLOR_F {
                                        r: 0.08,
                                        g: 0.09,
                                        b: 0.12,
                                        a: 0.65,
                                    },
                                ) {
                                    rt.FillRoundedRectangle(&card_rrect, &card_bg);
                                }
                                if let Some(card_border) = self.solid_brush(
                                    rt,
                                    &D2D1_COLOR_F {
                                        r: 1.0,
                                        g: 1.0,
                                        b: 1.0,
                                        a: 0.15,
                                    },
                                ) {
                                    rt.DrawRoundedRectangle(&card_rrect, &card_border, 1.0, None);
                                }
                            }
                            TextCardStyle::Solid => {
                                if let Some(card_bg) = self.solid_brush(
                                    rt,
                                    &D2D1_COLOR_F {
                                        r: 0.12,
                                        g: 0.13,
                                        b: 0.17,
                                        a: 0.96,
                                    },
                                ) {
                                    rt.FillRoundedRectangle(&card_rrect, &card_bg);
                                }
                                if let Some(card_border) = self.solid_brush(rt, &col) {
                                    rt.DrawRoundedRectangle(&card_rrect, &card_border, 1.5, None);
                                }
                            }
                            TextCardStyle::Transparent => {
                                if let Some(sh_brush) = self.solid_brush(
                                    rt,
                                    &D2D1_COLOR_F {
                                        r: 0.0,
                                        g: 0.0,
                                        b: 0.0,
                                        a: 0.70,
                                    },
                                ) {
                                    rt.DrawTextLayout(
                                        v2(text_rect.left + 1.2, text_rect.top + 1.5),
                                        &text_layout,
                                        &sh_brush,
                                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                                    );
                                }
                            }
                        }

                        if let Some(brush) = self.solid_brush(rt, &col) {
                            rt.DrawTextLayout(
                                v2(text_rect.left, text_rect.top),
                                &text_layout,
                                &brush,
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                            );
                        }
                    }
                }

                Shape::StepBadge {
                    center,
                    number,
                    radius,
                    color,
                    shape,
                    fill,
                    stroke_width,
                    pattern,
                } => {
                    let col = color.to_d2d_color((1.0) * opacity);
                    let border_w = stroke_width.max(1.0);
                    let border_brush = self.solid_brush(rt, &col);

                    let fill_brush = match fill {
                        FillMode::None => None,
                        FillMode::Tinted => {
                            let fill_col = color.to_d2d_color((0.30) * opacity);
                            self.solid_brush(rt, &fill_col)
                        }
                        FillMode::Solid => self.solid_brush(rt, &col),
                    };

                    let backplate_brush = if *fill == FillMode::Tinted {
                        let bp_col = D2D1_COLOR_F {
                            r: 0.08,
                            g: 0.10,
                            b: 0.14,
                            a: 0.70,
                        };
                        self.solid_brush(rt, &bp_col)
                    } else {
                        None
                    };

                    let stroke_style = match pattern {
                        StrokePattern::Solid => None,
                        StrokePattern::Dashed => Some(&self.dashed_stroke_style),
                        StrokePattern::Dotted => Some(&self.dashed_stroke_style),
                    };

                    match shape {
                        BadgeShape::Circle => {
                            let el = D2D1_ELLIPSE {
                                point: v2(center.x, center.y),
                                radiusX: *radius,
                                radiusY: *radius,
                            };
                            if let Some(bp) = &backplate_brush {
                                rt.FillEllipse(&el, bp);
                            }
                            if let Some(fb) = &fill_brush {
                                rt.FillEllipse(&el, fb);
                            }
                            if let Some(bb) = &border_brush {
                                rt.DrawEllipse(&el, bb, border_w, stroke_style);
                            }
                        }
                        BadgeShape::Square => {
                            let rrect = D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F {
                                    left: center.x - *radius,
                                    top: center.y - *radius,
                                    right: center.x + *radius,
                                    bottom: center.y + *radius,
                                },
                                radiusX: 6.0,
                                radiusY: 6.0,
                            };
                            if let Some(bp) = &backplate_brush {
                                rt.FillRoundedRectangle(&rrect, bp);
                            }
                            if let Some(fb) = &fill_brush {
                                rt.FillRoundedRectangle(&rrect, fb);
                            }
                            if let Some(bb) = &border_brush {
                                rt.DrawRoundedRectangle(&rrect, bb, border_w, stroke_style);
                            }
                        }
                        BadgeShape::Hexagon => {
                            // Keyed on center+radius (a discriminant tag keeps
                            // this from ever colliding with the stroke
                            // geometries sharing the same cache): a hexagon
                            // badge that isn't being dragged rebuilds nothing.
                            let key = {
                                use std::hash::{Hash, Hasher};
                                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                0xBADE_u32.hash(&mut hasher);
                                center.x.to_bits().hash(&mut hasher);
                                center.y.to_bits().hash(&mut hasher);
                                radius.to_bits().hash(&mut hasher);
                                hasher.finish()
                            };
                            let mut cache = self.geometry_cache.borrow_mut();
                            let rt_id = self.rt_key(rt);
                            if cache.0 != rt_id {
                                cache.0 = rt_id;
                                cache.1.clear();
                            }
                            let path = if let Some(p) = cache.1.get(&key) {
                                Some(p.clone())
                            } else {
                                let mut points = [Point2D::default(); 6];
                                for (i, p) in points.iter_mut().enumerate() {
                                    let angle = (i as f32 * std::f32::consts::PI / 3.0)
                                        - std::f32::consts::FRAC_PI_2;
                                    *p = Point2D::new(
                                        center.x + radius * angle.cos(),
                                        center.y + radius * angle.sin(),
                                    );
                                }
                                let built =
                                    self.factory.CreatePathGeometry().ok().and_then(|path| {
                                        let sink = path.Open().ok()?;
                                        sink.BeginFigure(
                                            v2(points[0].x, points[0].y),
                                            D2D1_FIGURE_BEGIN_FILLED,
                                        );
                                        for pt in &points[1..] {
                                            sink.AddLine(v2(pt.x, pt.y));
                                        }
                                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                                        let _ = sink.Close();
                                        Some(path)
                                    });
                                if let Some(p) = &built {
                                    if cache.1.len() > GEOMETRY_CACHE_MAX_ENTRIES {
                                        cache.1.clear();
                                    }
                                    cache.1.insert(key, p.clone());
                                }
                                built
                            };
                            drop(cache);
                            if let Some(path) = path {
                                if let Some(bp) = &backplate_brush {
                                    rt.FillGeometry(&path, bp, None);
                                }
                                if let Some(fb) = &fill_brush {
                                    rt.FillGeometry(&path, fb, None);
                                }
                                if let Some(bb) = &border_brush {
                                    rt.DrawGeometry(&path, bb, border_w, stroke_style);
                                }
                            }
                        }
                    }

                    // Number text inside badge
                    let text = number.to_string();
                    let text_utf16: Vec<u16> = text.encode_utf16().collect();
                    let text_col = match fill {
                        FillMode::Solid => match color {
                            ColorPreset::Yellow | ColorPreset::Cyan | ColorPreset::White => {
                                D2D1_COLOR_F {
                                    r: 0.10,
                                    g: 0.11,
                                    b: 0.15,
                                    a: 1.0,
                                }
                            }
                            _ => D2D1_COLOR_F {
                                r: 1.0,
                                g: 1.0,
                                b: 1.0,
                                a: 1.0,
                            },
                        },
                        FillMode::Tinted => D2D1_COLOR_F {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        },
                        FillMode::None => col,
                    };
                    let font_size = (*radius * 0.95).max(11.0);
                    if let Ok(custom_fmt) = self.get_text_format(font_size) {
                        // Already centered - get_text_format's cache key
                        // reserves this format for centered callers only.
                        let text_rect = D2D_RECT_F {
                            left: center.x - *radius,
                            top: center.y - *radius,
                            right: center.x + *radius,
                            bottom: center.y + *radius,
                        };
                        if let Some(tbrush) = self.solid_brush(rt, &text_col) {
                            rt.DrawText(&text_utf16, &custom_fmt, &text_rect, &tbrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                        }
                    }
                }
                Shape::Image { start, end, pixels } => {
                    if let Some(bitmap) = self.image_bitmap(rt, pixels) {
                        let (l, t, r, b) = normalize_rect(*start, *end);
                        let dst = D2D_RECT_F {
                            left: l,
                            top: t,
                            right: r,
                            bottom: b,
                        };
                        rt.DrawBitmap(
                            &bitmap,
                            Some(&dst),
                            opacity,
                            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                            None,
                        );
                    }
                }

                Shape::Blur {
                    start,
                    end,
                    block_size,
                } => {
                    let (l, t, r, b) = crate::shapes::normalize_rect(*start, *end);
                    let w = r - l;
                    let h = b - t;
                    if w < 1.0 || h < 1.0 {
                        return;
                    }

                    let b_size = block_size.clamp(4.0, 64.0);

                    // bg_bitmap is captured at 96 DPI (capture.rs), so its
                    // own coordinate space is physical pixels, while l/t/r/b
                    // are canvas DIPs at rt's actual DPI - 1:1 only at 100%
                    // scaling. Used below to convert the region read *from*
                    // the bitmap; the mosaic's block count and its drawn
                    // extent on screen both stay in DIPs, which is what a
                    // "block_size" the user sees on screen should mean.
                    let mut dpi_x = 96.0f32;
                    let mut dpi_y = 96.0f32;
                    rt.GetDpi(&mut dpi_x, &mut dpi_y);
                    let s = (dpi_x / 96.0).max(0.01);

                    // Mosaic via downsample-then-upsample: shrink the region to one
                    // texel per block (linear, so each texel averages its block),
                    // then blow it back up with nearest-neighbour.
                    //
                    // The downsample pass itself only runs once per distinct
                    // (background, rect, block size) - cached in
                    // blur_mosaic_cache keyed by content, the same approach as
                    // the stroke geometry cache - rather than every single
                    // frame regardless of whether anything about this blur
                    // changed. Each key gets its *own* dedicated render
                    // target instead of sharing one scratch RT across every
                    // blur on the canvas: ID2D1BitmapRenderTarget::GetBitmap()
                    // returns a live view of that target's own backing
                    // surface, so two blurs sharing one scratch RT could
                    // otherwise show each other's content depending on draw
                    // order.
                    let mosaic = bg_bitmap.and_then(|bmp| {
                        let cols = (w / b_size).ceil().max(1.0);
                        let rows = (h / b_size).ceil().max(1.0);
                        let small = D2D_SIZE_F {
                            width: cols,
                            height: rows,
                        };

                        let key = {
                            use std::hash::{Hash, Hasher};
                            let mut hasher = std::collections::hash_map::DefaultHasher::new();
                            (bmp.as_raw() as usize).hash(&mut hasher);
                            start.x.to_bits().hash(&mut hasher);
                            start.y.to_bits().hash(&mut hasher);
                            end.x.to_bits().hash(&mut hasher);
                            end.y.to_bits().hash(&mut hasher);
                            b_size.to_bits().hash(&mut hasher);
                            hasher.finish()
                        };

                        let rt_id = self.rt_key(rt);
                        let mut cache = self.blur_mosaic_cache.borrow_mut();
                        if cache.0 != rt_id {
                            cache.0 = rt_id;
                            cache.1.clear();
                        }

                        let tiny = if let Some(existing) = cache.1.get(&key) {
                            existing.clone()
                        } else {
                            let new_rt = rt
                                .CreateCompatibleRenderTarget(
                                    Some(&small),
                                    None,
                                    None,
                                    D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE,
                                )
                                .ok()?;
                            new_rt.BeginDraw();
                            new_rt.Clear(None);
                            new_rt.DrawBitmap(
                                bmp,
                                Some(&D2D_RECT_F {
                                    left: 0.0,
                                    top: 0.0,
                                    right: cols,
                                    bottom: rows,
                                }),
                                1.0,
                                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                // Source rect is within bg_bitmap's own
                                // (physical-pixel) space, so the DIP rect
                                // has to be scaled up to match - reading it
                                // unscaled is what made this sample the
                                // wrong region above 100% DPI.
                                Some(&D2D_RECT_F {
                                    left: l * s,
                                    top: t * s,
                                    right: r * s,
                                    bottom: b * s,
                                }),
                            );
                            if new_rt.EndDraw(None, None).is_err() {
                                return None;
                            }
                            if cache.1.len() > BLUR_MOSAIC_CACHE_MAX_ENTRIES {
                                cache.1.clear();
                            }
                            cache.1.insert(key, new_rt.clone());
                            new_rt
                        };
                        tiny.GetBitmap().ok().map(|bm| (bm, cols, rows))
                    });

                    if let Some((small_bmp, cols, rows)) = mosaic {
                        rt.DrawBitmap(
                            &small_bmp,
                            Some(&D2D_RECT_F {
                                left: l,
                                top: t,
                                right: r,
                                bottom: b,
                            }),
                            1.0,
                            D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                            Some(&D2D_RECT_F {
                                left: 0.0,
                                top: 0.0,
                                right: cols,
                                bottom: rows,
                            }),
                        );
                    } else if let Some(brush) = self.solid_brush(
                        rt,
                        &D2D1_COLOR_F {
                            r: 0.1,
                            g: 0.1,
                            b: 0.1,
                            a: 0.85,
                        },
                    ) {
                        let rect = D2D_RECT_F {
                            left: l,
                            top: t,
                            right: r,
                            bottom: b,
                        };
                        rt.FillRectangle(&rect, &brush);
                    }

                    // Subtle glass outline around the redacted region
                    if let Some(border_brush) = self.solid_brush(
                        rt,
                        &D2D1_COLOR_F {
                            r: 0.4,
                            g: 0.7,
                            b: 1.0,
                            a: 0.45,
                        },
                    ) {
                        let border_rect = D2D_RECT_F {
                            left: l,
                            top: t,
                            right: r,
                            bottom: b,
                        };
                        rt.DrawRectangle(&border_rect, &border_brush, 1.0, None);
                    }
                }
            }
        }
    }

    /// A label centred inside the shape holding it.
    ///
    /// No card of its own: the container is the card. The position is derived
    /// from the container's bounds every frame rather than stored, which is
    /// what makes the label follow moves and resizes without any bookkeeping.
    pub(super) unsafe fn render_contained_text(
        &self,
        rt: &ID2D1RenderTarget,
        shape: &Shape,
        container: (f32, f32, f32, f32),
        // A label on a line needs a chip behind it, or the line strikes
        // straight through the words.
        rides_on: bool,
        opacity: f32,
    ) {
        unsafe {
            let Shape::Text {
                text,
                font_size,
                color,
                is_bold,
                is_italic,
                font_family,
                ..
            } = shape
            else {
                return;
            };
            if text.is_empty() {
                return;
            }

            let pad = TextEditorState::CONTAINER_PADDING;
            let wrap = if rides_on {
                f32::MAX
            } else {
                ((container.2 - container.0) - pad * 2.0).max(24.0)
            };
            let Some((text_layout, w, h)) = self.get_or_build_text_layout(
                text,
                *font_size,
                *is_bold,
                *is_italic,
                *font_family,
                wrap,
            ) else {
                return;
            };
            let origin = contained_text_origin(container, w, h);

            if rides_on
                && let Some(chip) = self.solid_brush(
                    rt,
                    &D2D1_COLOR_F {
                        r: 0.10,
                        g: 0.11,
                        b: 0.14,
                        a: 0.88,
                    },
                )
            {
                let chip_rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: origin.x - 6.0,
                        top: origin.y - 3.0,
                        right: origin.x + w + 6.0,
                        bottom: origin.y + h + 3.0,
                    },
                    radiusX: 4.0,
                    radiusY: 4.0,
                };
                rt.FillRoundedRectangle(&chip_rect, &chip);
            }

            let Some(brush) = self.solid_brush(rt, &color.to_d2d_color((1.0) * opacity)) else {
                return;
            };
            rt.DrawTextLayout(
                v2(origin.x, origin.y),
                &text_layout,
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
    }

    /// Alignment feedback for a snap in effect.
    ///
    /// Screen space, so the guides stay hairline-thin and the markers keep
    /// their size however far the canvas is zoomed. Points arrive already
    /// converted by the caller.
    pub(super) unsafe fn render_snap_guides(
        &self,
        rt: &ID2D1RenderTarget,
        guides: &[(f32, f32, f32, f32, bool)],
    ) {
        unsafe {
            let accent = D2D1_COLOR_F {
                r: 1.0,
                g: 0.32,
                b: 0.62,
                a: 0.95,
            };
            let Some(brush) = self.solid_brush(rt, &accent) else {
                return;
            };
            let dashed = self.get_stroke_style(StrokePattern::Dashed);

            for (ax, ay, bx, by, marker) in guides {
                if *marker {
                    // A filled diamond reads as "landed on this point" without
                    // being mistaken for a selection grip. Keyed on position
                    // (the size, 5.0, is fixed) via the same content-hash
                    // geometry_cache the hexagon badge and strokes use, so a
                    // drag that keeps re-snapping to the same handful of
                    // anchor points isn't rebuilding this every frame.
                    let s = 5.0;
                    let key = {
                        use std::hash::{Hash, Hasher};
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        0xD1A5_u32.hash(&mut hasher);
                        ax.to_bits().hash(&mut hasher);
                        ay.to_bits().hash(&mut hasher);
                        hasher.finish()
                    };
                    let mut cache = self.geometry_cache.borrow_mut();
                    let rt_id = self.rt_key(rt);
                    if cache.0 != rt_id {
                        cache.0 = rt_id;
                        cache.1.clear();
                    }
                    let path = if let Some(p) = cache.1.get(&key) {
                        Some(p.clone())
                    } else {
                        let built = self.factory.CreatePathGeometry().ok().and_then(|path| {
                            let sink = path.Open().ok()?;
                            sink.BeginFigure(v2(*ax, ay - s), D2D1_FIGURE_BEGIN_FILLED);
                            sink.AddLine(v2(ax + s, *ay));
                            sink.AddLine(v2(*ax, ay + s));
                            sink.AddLine(v2(ax - s, *ay));
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                            let _ = sink.Close();
                            Some(path)
                        });
                        if let Some(p) = &built {
                            if cache.1.len() > GEOMETRY_CACHE_MAX_ENTRIES {
                                cache.1.clear();
                            }
                            cache.1.insert(key, p.clone());
                        }
                        built
                    };
                    drop(cache);
                    if let Some(path) = path {
                        rt.FillGeometry(&path, &brush, None);
                    }
                } else {
                    rt.DrawLine(v2(*ax, *ay), v2(*bx, *by), &brush, 1.2, Some(dashed));
                }
            }
        }
    }

    /// The rubber-band rectangle swept to select several annotations at once.
    pub(super) unsafe fn render_marquee(
        &self,
        rt: &ID2D1RenderTarget,
        bounds: (f32, f32, f32, f32),
    ) {
        unsafe {
            let (l, t, r, b) = bounds;
            let rect = D2D_RECT_F {
                left: l,
                top: t,
                right: r,
                bottom: b,
            };
            let accent = D2D1_COLOR_F {
                r: 0.38,
                g: 0.72,
                b: 0.98,
                a: 1.0,
            };
            // A faint wash makes the swept area obvious without hiding what is
            // underneath it.
            if let Some(fill) = self.solid_brush(
                rt,
                &D2D1_COLOR_F {
                    r: 0.38,
                    g: 0.72,
                    b: 0.98,
                    a: 0.14,
                },
            ) {
                rt.FillRectangle(&rect, &fill);
            }
            if let Some(edge) = self.solid_brush(rt, &accent) {
                let dashed = self.get_stroke_style(StrokePattern::Dashed);
                rt.DrawRectangle(&rect, &edge, 1.2, Some(dashed));
            }
        }
    }

    /// Stroke a polyline as one path, so joins are smooth and a dash pattern
    /// runs continuously instead of restarting at every segment.
    /// Draws a curved line/arrow's sampled polyline. Keyed on the point
    /// values + width (a discriminant tag keeps it from colliding with the
    /// freehand-stroke or badge/marker entries sharing the same cache): the
    /// curve only actually changes while its handle is being dragged, so
    /// this is a CreatePathGeometry saved on every other frame it's drawn.
    /// The cached filled outline of a committed pressure stroke; see
    /// `crate::shapes::pressure_stroke_figures`. Keyed on the point and
    /// pressure values plus width, with its own discriminant tag.
    unsafe fn pressure_stroke_geometry(
        &self,
        rt: &ID2D1RenderTarget,
        points: &[Point2D],
        pressures: &[f32],
        width: f32,
    ) -> Option<ID2D1PathGeometry> {
        unsafe {
            let key = {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                0x9E55_u32.hash(&mut hasher);
                for (p, pr) in points.iter().zip(pressures) {
                    p.x.to_bits().hash(&mut hasher);
                    p.y.to_bits().hash(&mut hasher);
                    pr.to_bits().hash(&mut hasher);
                }
                width.to_bits().hash(&mut hasher);
                hasher.finish()
            };
            let mut cache = self.geometry_cache.borrow_mut();
            let rt_id = self.rt_key(rt);
            if cache.0 != rt_id {
                cache.0 = rt_id;
                cache.1.clear();
            }
            if let Some(p) = cache.1.get(&key) {
                return Some(p.clone());
            }
            let figures = pressure_stroke_figures(points, pressures, width);
            if figures.is_empty() {
                return None;
            }
            let path = self.factory.CreatePathGeometry().ok()?;
            let sink = path.Open().ok()?;
            sink.SetFillMode(D2D1_FILL_MODE_WINDING);
            for fig in &figures {
                sink.BeginFigure(v2(fig[0].x, fig[0].y), D2D1_FIGURE_BEGIN_FILLED);
                for pt in &fig[1..] {
                    sink.AddLine(v2(pt.x, pt.y));
                }
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            }
            sink.Close().ok()?;
            if cache.1.len() > GEOMETRY_CACHE_MAX_ENTRIES {
                cache.1.clear();
            }
            cache.1.insert(key, path.clone());
            Some(path)
        }
    }

    pub(super) unsafe fn stroke_polyline(
        &self,
        rt: &ID2D1RenderTarget,
        pts: &[Point2D],
        brush: &ID2D1Brush,
        width: f32,
        stroke: Option<&ID2D1StrokeStyle>,
    ) {
        unsafe {
            if pts.len() < 2 {
                return;
            }
            let key = {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                0xC0BE_u32.hash(&mut hasher);
                for p in pts {
                    p.x.to_bits().hash(&mut hasher);
                    p.y.to_bits().hash(&mut hasher);
                }
                width.to_bits().hash(&mut hasher);
                hasher.finish()
            };
            let mut cache = self.geometry_cache.borrow_mut();
            let rt_id = self.rt_key(rt);
            if cache.0 != rt_id {
                cache.0 = rt_id;
                cache.1.clear();
            }
            let path = if let Some(p) = cache.1.get(&key) {
                Some(p.clone())
            } else {
                let built = self.factory.CreatePathGeometry().ok().and_then(|path| {
                    let sink = path.Open().ok()?;
                    sink.BeginFigure(v2(pts[0].x, pts[0].y), D2D1_FIGURE_BEGIN_HOLLOW);
                    for p in &pts[1..] {
                        sink.AddLine(v2(p.x, p.y));
                    }
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    let _ = sink.Close();
                    Some(path)
                });
                if let Some(p) = &built {
                    if cache.1.len() > GEOMETRY_CACHE_MAX_ENTRIES {
                        cache.1.clear();
                    }
                    cache.1.insert(key, p.clone());
                }
                built
            };
            drop(cache);
            if let Some(path) = path {
                rt.DrawGeometry(&path, brush, width, stroke);
            }
        }
    }

    /// Round grips on the two ends of a selected line or arrow.
    ///
    /// A line has no interior to scale, so a bounding box would offer eight
    /// grips that mostly do the wrong thing. Its ends are what you actually
    /// want to grab, and dropping one on a shape re-anchors it.
    pub(super) unsafe fn render_endpoint_grips(
        &self,
        rt: &ID2D1RenderTarget,
        a: (f32, f32),
        b: (f32, f32),
        // The bow handle, drawn smaller so it does not read as a third end.
        bow: Option<(f32, f32)>,
    ) {
        unsafe {
            let accent = D2D1_COLOR_F {
                r: 0.38,
                g: 0.72,
                b: 0.98,
                a: 1.0,
            };
            let fill = self.solid_brush(
                rt,
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
            );
            let edge = self.solid_brush(rt, &accent);
            let halo = self.solid_brush(
                rt,
                &D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.45,
                },
            );
            if let Some((bx, by)) = bow {
                let br = SELECTION_HANDLE_SIZE * 0.42;
                let ring = D2D1_ELLIPSE {
                    point: v2(bx, by),
                    radiusX: br,
                    radiusY: br,
                };
                if let Some(h) = &halo {
                    rt.DrawEllipse(&ring, h, 2.5, None);
                }
                if let Some(e) = &edge {
                    rt.FillEllipse(&ring, e);
                }
            }

            let r = SELECTION_HANDLE_SIZE * 0.62;
            for (x, y) in [a, b] {
                let ring = D2D1_ELLIPSE {
                    point: v2(x, y),
                    radiusX: r,
                    radiusY: r,
                };
                if let Some(h) = &halo {
                    rt.DrawEllipse(&ring, h, 3.0, None);
                }
                if let Some(f) = &fill {
                    rt.FillEllipse(&ring, f);
                }
                if let Some(e) = &edge {
                    rt.DrawEllipse(&ring, e, 1.6, None);
                }
            }
        }
    }

    /// Marching-ants box plus eight grips around the selected annotation.
    ///
    /// Drawn in screen space with the identity transform, so the grips stay a
    /// fixed size no matter how far the canvas is zoomed in.
    pub(super) unsafe fn render_selection(
        &self,
        rt: &ID2D1RenderTarget,
        screen_bounds: (f32, f32, f32, f32),
    ) {
        unsafe {
            let (l, t, r, b) = screen_bounds;
            let pad = 4.0;
            let rect = D2D_RECT_F {
                left: l - pad,
                top: t - pad,
                right: r + pad,
                bottom: b + pad,
            };
            let rrect = D2D1_ROUNDED_RECT {
                rect,
                radiusX: 3.0,
                radiusY: 3.0,
            };

            // Dark halo first so the box reads on light and dark content alike.
            if let Some(halo) = self.solid_brush(
                rt,
                &D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.55,
                },
            ) {
                rt.DrawRoundedRectangle(&rrect, &halo, 3.0, None);
            }
            let accent = D2D1_COLOR_F {
                r: 0.38,
                g: 0.72,
                b: 0.98,
                a: 1.0,
            };
            if let Some(line) = self.solid_brush(rt, &accent) {
                rt.DrawRoundedRectangle(&rrect, &line, 1.4, None);
            }

            let half = SELECTION_HANDLE_SIZE * 0.5;
            let fill = self.solid_brush(
                rt,
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
            );
            let edge = self.solid_brush(rt, &accent);
            for (_, c) in selection_handle_points((rect.left, rect.top, rect.right, rect.bottom)) {
                let h = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: c.x - half,
                        top: c.y - half,
                        right: c.x + half,
                        bottom: c.y + half,
                    },
                    radiusX: 2.0,
                    radiusY: 2.0,
                };
                if let Some(f) = &fill {
                    rt.FillRoundedRectangle(&h, f);
                }
                if let Some(e) = &edge {
                    rt.DrawRoundedRectangle(&h, e, 1.2, None);
                }
            }
        }
    }

    pub(super) unsafe fn render_text_editor(
        &self,
        rt: &ID2D1RenderTarget,
        editor: &TextEditorState,
        show_caret: bool,
    ) {
        unsafe {
            let text = &editor.text;
            let cursor = editor.cursor;
            let col = editor.color.to_d2d_color(1.0);
            let font_size = editor.font_size;

            // Measure with the caret always present so the blink doesn't make
            // the card pulse, and keep an input-field-sized floor.
            let mut measured = text.to_string();
            let caret_idx = cursor.min(measured.len());
            measured.insert(caret_idx, '|');
            let (block_w, block_h) = self.measure_text_block(
                &measured,
                font_size,
                editor.is_bold,
                editor.is_italic,
                editor.font_family,
                editor.wrap_width(),
            );

            // A label typed into a shape is centred in it and wears no card of
            // its own — the shape is the card. Free text keeps its own.
            let (origin, estimated_w, estimated_h) = match editor.container_bounds {
                Some(b) => {
                    let o = contained_text_origin(b, block_w, block_h);
                    (o, block_w, block_h)
                }
                None => (editor.origin, block_w.max(140.0) + 30.0, block_h + 14.0),
            };
            let rect = D2D_RECT_F {
                left: origin.x - 8.0,
                top: origin.y - 6.0,
                right: origin.x + estimated_w,
                bottom: origin.y + estimated_h,
            };

            let rrect = D2D1_ROUNDED_RECT {
                rect,
                radiusX: 6.0,
                radiusY: 6.0,
            };

            // A label being typed onto a line needs the same chip the committed
            // one gets, or the line strikes through the words while editing.
            if editor.container_bounds.is_some()
                && !editor.container_wraps
                && let Some(chip) = self.solid_brush(
                    rt,
                    &D2D1_COLOR_F {
                        r: 0.10,
                        g: 0.11,
                        b: 0.14,
                        a: 0.88,
                    },
                )
            {
                let chip_rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: origin.x - 6.0,
                        top: origin.y - 3.0,
                        right: origin.x + estimated_w + 6.0,
                        bottom: origin.y + estimated_h + 3.0,
                    },
                    radiusX: 4.0,
                    radiusY: 4.0,
                };
                rt.FillRoundedRectangle(&chip_rect, &chip);
            }

            if editor.container_bounds.is_none() {
                match editor.card_style {
                    TextCardStyle::Badge => {
                        if let Some(bg_brush) = self.solid_brush(
                            rt,
                            &D2D1_COLOR_F {
                                r: 0.08,
                                g: 0.09,
                                b: 0.12,
                                a: 0.65,
                            },
                        ) {
                            rt.FillRoundedRectangle(&rrect, &bg_brush);
                        }
                        if let Some(border_brush) = self.solid_brush(
                            rt,
                            &D2D1_COLOR_F {
                                r: 0.38,
                                g: 0.72,
                                b: 0.98,
                                a: 0.85,
                            },
                        ) {
                            rt.DrawRoundedRectangle(&rrect, &border_brush, 1.5, None);
                        }
                    }
                    TextCardStyle::Solid => {
                        if let Some(bg_brush) = self.solid_brush(
                            rt,
                            &D2D1_COLOR_F {
                                r: 0.12,
                                g: 0.13,
                                b: 0.17,
                                a: 0.96,
                            },
                        ) {
                            rt.FillRoundedRectangle(&rrect, &bg_brush);
                        }
                        if let Some(border_brush) = self.solid_brush(rt, &col) {
                            rt.DrawRoundedRectangle(&rrect, &border_brush, 1.5, None);
                        }
                    }
                    TextCardStyle::Transparent => {
                        if let Some(bg_brush) = self.solid_brush(
                            rt,
                            &D2D1_COLOR_F {
                                r: 0.05,
                                g: 0.05,
                                b: 0.08,
                                a: 0.45,
                            },
                        ) {
                            rt.FillRoundedRectangle(&rrect, &bg_brush);
                        }
                        if let Some(border_brush) = self.solid_brush(
                            rt,
                            &D2D1_COLOR_F {
                                r: 0.38,
                                g: 0.72,
                                b: 0.98,
                                a: 0.80,
                            },
                        ) {
                            rt.DrawRoundedRectangle(&rrect, &border_brush, 1.0, None);
                        }
                    }
                }
            }

            // One cached layout for the real text (no synthetic caret
            // character spliced in) - drawn as-is, with the blinking caret
            // itself drawn separately as a line from HitTestTextPosition.
            // The previous approach rebuilt a whole new layout from a freshly
            // copied-and-mutated string on every blink tick, for a card that
            // is open and blinking far more of the time than its text is
            // actually being edited.
            if let Some(brush) = self.solid_brush(rt, &col)
                && let Some((text_layout, _, _)) = self.get_or_build_text_layout(
                    text,
                    font_size,
                    editor.is_bold,
                    editor.is_italic,
                    editor.font_family,
                    editor.wrap_width(),
                )
            {
                rt.DrawTextLayout(
                    v2(origin.x, origin.y),
                    &text_layout,
                    &brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );

                if show_caret {
                    let safe_idx = cursor.min(text.len());
                    let utf16_pos = text[..safe_idx].encode_utf16().count() as u32;
                    let mut px = 0.0f32;
                    let mut py = 0.0f32;
                    let mut metrics = DWRITE_HIT_TEST_METRICS::default();
                    if text_layout
                        .HitTestTextPosition(utf16_pos, false, &mut px, &mut py, &mut metrics)
                        .is_ok()
                    {
                        let caret_h = metrics.height.max(font_size);
                        rt.DrawLine(
                            v2(origin.x + px, origin.y + py),
                            v2(origin.x + px, origin.y + py + caret_h),
                            &brush,
                            1.6,
                            None,
                        );
                    }
                }
            }
        }
    }

    pub(super) unsafe fn render_laser_pointer(
        &self,
        rt: &ID2D1RenderTarget,
        trail: &[LaserTrailPoint],
        ripples: &[LaserRipple],
        current_pos: Option<Point2D>,
        color: ColorPreset,
    ) {
        unsafe {
            let now = std::time::Instant::now();
            let base_col = color.to_d2d_color(1.0);

            // 1. Draw animated shockwave pulse ripples
            for ripple in ripples {
                let age = now.duration_since(ripple.timestamp).as_secs_f32();
                let duration = 0.85;
                let t = (age / duration).clamp(0.0, 1.0);
                if t >= 1.0 {
                    continue;
                }
                let rip_col = ripple.color.to_d2d_color(1.0);

                // Main expanding shockwave ring
                let ease_out = 1.0 - (1.0 - t).powi(3);
                let radius = 8.0 + ease_out * 56.0;
                let alpha = ((1.0 - t).powi(2) * 0.85).max(0.0);
                let stroke_w = (4.0 - ease_out * 2.5).max(1.0);

                let ring_col = D2D1_COLOR_F {
                    r: rip_col.r,
                    g: rip_col.g,
                    b: rip_col.b,
                    a: alpha,
                };
                if let Some(brush) = self.scratch_brush(rt, &ring_col) {
                    let el = D2D1_ELLIPSE {
                        point: v2(ripple.center.x, ripple.center.y),
                        radiusX: radius,
                        radiusY: radius,
                    };
                    rt.DrawEllipse(&el, &brush, stroke_w, None);
                }

                // Secondary trailing echo ring
                if t > 0.12 {
                    let t2 = ((t - 0.12) / 0.88).clamp(0.0, 1.0);
                    let ease_out2 = 1.0 - (1.0 - t2).powi(3);
                    let radius2 = 6.0 + ease_out2 * 40.0;
                    let alpha2 = ((1.0 - t2).powi(2) * 0.55).max(0.0);
                    let stroke_w2 = (2.5 - ease_out2 * 1.5).max(1.0);

                    let echo_col = D2D1_COLOR_F {
                        r: rip_col.r,
                        g: rip_col.g,
                        b: rip_col.b,
                        a: alpha2,
                    };
                    if let Some(brush2) = self.scratch_brush(rt, &echo_col) {
                        let el2 = D2D1_ELLIPSE {
                            point: v2(ripple.center.x, ripple.center.y),
                            radiusX: radius2,
                            radiusY: radius2,
                        };
                        rt.DrawEllipse(&el2, &brush2, stroke_w2, None);
                    }
                }

                // Initial impact center flash
                if t < 0.22 {
                    let flash_t = t / 0.22;
                    let flash_alpha = (1.0 - flash_t) * 0.55;
                    let flash_radius = 5.0 + flash_t * 16.0;
                    let flash_col = D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: flash_alpha,
                    };
                    if let Some(fbrush) = self.scratch_brush(rt, &flash_col) {
                        let fel = D2D1_ELLIPSE {
                            point: v2(ripple.center.x, ripple.center.y),
                            radiusX: flash_radius,
                            radiusY: flash_radius,
                        };
                        rt.FillEllipse(&fel, &fbrush);
                    }
                }
            }

            // 2. Draw decaying trail segments
            if trail.len() >= 2 {
                for i in 0..(trail.len() - 1) {
                    let age = now.duration_since(trail[i].timestamp).as_secs_f32();
                    let life = (1.0 - age / 1.2).clamp(0.0, 1.0);
                    if life <= 0.01 {
                        continue;
                    }
                    let seg_alpha = life * 0.75;
                    let seg_width = 3.0 + life * 5.0;
                    let seg_col = D2D1_COLOR_F {
                        r: base_col.r,
                        g: base_col.g,
                        b: base_col.b,
                        a: seg_alpha,
                    };
                    if let Some(brush) = self.scratch_brush(rt, &seg_col) {
                        rt.DrawLine(
                            v2(trail[i].pt.x, trail[i].pt.y),
                            v2(trail[i + 1].pt.x, trail[i + 1].pt.y),
                            &brush,
                            seg_width,
                            Some(&self.round_stroke_style),
                        );
                    }
                }
            }

            // Draw glowing pointer dot at current position
            if let Some(pos) = current_pos {
                // Outer halo
                let halo_col = D2D1_COLOR_F {
                    r: base_col.r,
                    g: base_col.g,
                    b: base_col.b,
                    a: 0.35,
                };
                if let Some(brush) = self.solid_brush(rt, &halo_col) {
                    let el = D2D1_ELLIPSE {
                        point: v2(pos.x, pos.y),
                        radiusX: 14.0,
                        radiusY: 14.0,
                    };
                    rt.FillEllipse(&el, &brush);
                }

                // Mid core
                let mid_col = D2D1_COLOR_F {
                    r: base_col.r,
                    g: base_col.g,
                    b: base_col.b,
                    a: 0.90,
                };
                if let Some(brush) = self.solid_brush(rt, &mid_col) {
                    let el = D2D1_ELLIPSE {
                        point: v2(pos.x, pos.y),
                        radiusX: 7.0,
                        radiusY: 7.0,
                    };
                    rt.FillEllipse(&el, &brush);
                }

                // Intense white center
                let white_col = D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                };
                if let Some(brush) = self.solid_brush(rt, &white_col) {
                    let el = D2D1_ELLIPSE {
                        point: v2(pos.x, pos.y),
                        radiusX: 3.0,
                        radiusY: 3.0,
                    };
                    rt.FillEllipse(&el, &brush);
                }
            }
        }
    }

    pub(super) unsafe fn render_eraser_indicator(&self, rt: &ID2D1RenderTarget, pos: Point2D) {
        unsafe {
            let ring_col = D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.75,
            };
            let fill_col = D2D1_COLOR_F {
                r: 1.0,
                g: 0.3,
                b: 0.3,
                a: 0.15,
            };
            let el = D2D1_ELLIPSE {
                point: v2(pos.x, pos.y),
                radiusX: 18.0,
                radiusY: 18.0,
            };
            if let Some(fbrush) = self.solid_brush(rt, &fill_col) {
                rt.FillEllipse(&el, &fbrush);
            }
            if let Some(rbrush) = self.solid_brush(rt, &ring_col) {
                rt.DrawEllipse(&el, &rbrush, 1.5, None);
            }
        }
    }

    pub(super) unsafe fn render_drawing_snap_guides(&self, rt: &ID2D1RenderTarget, shape: &Shape) {
        unsafe {
            let guide_col = D2D1_COLOR_F {
                r: 0.25,
                g: 0.75,
                b: 1.0,
                a: 0.65,
            };
            let amber_col = D2D1_COLOR_F {
                r: 1.0,
                g: 0.85,
                b: 0.25,
                a: 0.80,
            };
            let card_bg = D2D1_COLOR_F {
                r: 0.10,
                g: 0.12,
                b: 0.16,
                a: 0.92,
            };
            let card_border = D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.22,
            };
            let text_col = D2D1_COLOR_F {
                r: 0.95,
                g: 0.96,
                b: 0.98,
                a: 1.0,
            };

            let guide_brush = self.solid_brush(rt, &guide_col);
            let amber_brush = self.solid_brush(rt, &amber_col);
            let bg_brush = self.solid_brush(rt, &card_bg);
            let border_brush = self.solid_brush(rt, &card_border);
            let text_brush = self.solid_brush(rt, &text_col);

            match shape {
                Shape::Line { start, end, .. } | Shape::Arrow { start, end, .. } => {
                    let dx = end.x - start.x;
                    let dy = end.y - start.y;
                    let dist = (dx * dx + dy * dy).sqrt();
                    if dist < 6.0 {
                        return;
                    }

                    // 1. Subtle dashed projection ray along the snapped angle
                    if let Some(gb) = &guide_brush {
                        let unit_x = dx / dist;
                        let unit_y = dy / dist;
                        let p1 = v2(start.x - unit_x * 24.0, start.y - unit_y * 24.0);
                        let p2 = v2(end.x + unit_x * 40.0, end.y + unit_y * 40.0);
                        rt.DrawLine(p1, p2, gb, 1.2, Some(&self.dashed_stroke_style));
                    }

                    // 2. Angle & Distance badge
                    let raw_deg = dy.atan2(dx).to_degrees();
                    let norm_deg = (raw_deg.round() as i32).rem_euclid(360);
                    let angle_label = match norm_deg {
                        0 | 360 => "0° (H)",
                        45 => "45°",
                        90 => "90° (V)",
                        135 => "135°",
                        180 => "180° (H)",
                        225 => "225°",
                        270 => "270° (V)",
                        315 => "315°",
                        _ => "Free",
                    };

                    let badge_text = format!("{} • {:.0} px", angle_label, dist);
                    let badge_utf16: Vec<u16> = badge_text.encode_utf16().collect();

                    let badge_w = (badge_text.len() as f32 * 8.0 + 16.0).max(80.0);
                    let badge_h = 22.0;
                    let badge_x = ((start.x + end.x) / 2.0 - badge_w / 2.0).max(4.0);
                    let badge_y = ((start.y + end.y) / 2.0 - 28.0).max(4.0);

                    let badge_rect = D2D_RECT_F {
                        left: badge_x,
                        top: badge_y,
                        right: badge_x + badge_w,
                        bottom: badge_y + badge_h,
                    };
                    let badge_rrect = D2D1_ROUNDED_RECT {
                        rect: badge_rect,
                        radiusX: 5.0,
                        radiusY: 5.0,
                    };

                    if let (Some(bgb), Some(bdb), Some(tb)) =
                        (&bg_brush, &border_brush, &text_brush)
                    {
                        rt.FillRoundedRectangle(&badge_rrect, bgb);
                        rt.DrawRoundedRectangle(&badge_rrect, bdb, 1.0, None);
                        rt.DrawText(
                            &badge_utf16,
                            &self.text_format_toolbar_small,
                            &badge_rect,
                            tb,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                }

                Shape::Rectangle {
                    start,
                    end,
                    rounded,
                    ..
                } => {
                    let (left, top, right, bottom) = normalize_rect(*start, *end);
                    let w = right - left;
                    let h = bottom - top;
                    if w < 8.0 || h < 8.0 {
                        return;
                    }

                    // 1. Dashed diagonal guideline showing 45° square symmetry
                    if let Some(gb) = &guide_brush {
                        rt.DrawLine(
                            v2(left, top),
                            v2(right, bottom),
                            gb,
                            1.2,
                            Some(&self.dashed_stroke_style),
                        );
                    }

                    // 2. 1:1 Square dimension badge
                    let tag = if *rounded {
                        "1:1 R-Square"
                    } else {
                        "1:1 Square"
                    };
                    let badge_text = format!("{} • {:.0} × {:.0} px", tag, w, h);
                    let badge_utf16: Vec<u16> = badge_text.encode_utf16().collect();

                    let badge_w = (badge_text.len() as f32 * 8.0 + 16.0).max(110.0);
                    let badge_h = 22.0;
                    let badge_x = (right - badge_w).max(left);
                    let badge_y = bottom + 6.0;

                    let badge_rect = D2D_RECT_F {
                        left: badge_x,
                        top: badge_y,
                        right: badge_x + badge_w,
                        bottom: badge_y + badge_h,
                    };
                    let badge_rrect = D2D1_ROUNDED_RECT {
                        rect: badge_rect,
                        radiusX: 5.0,
                        radiusY: 5.0,
                    };

                    if let (Some(bgb), Some(bdb), Some(tb)) =
                        (&bg_brush, &border_brush, &text_brush)
                    {
                        rt.FillRoundedRectangle(&badge_rrect, bgb);
                        rt.DrawRoundedRectangle(&badge_rrect, bdb, 1.0, None);
                        rt.DrawText(
                            &badge_utf16,
                            &self.text_format_toolbar_small,
                            &badge_rect,
                            tb,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                }

                Shape::Ellipse { start, end, .. } => {
                    let (left, top, right, bottom) = normalize_rect(*start, *end);
                    let w = right - left;
                    let h = bottom - top;
                    if w < 12.0 || h < 12.0 {
                        return;
                    }
                    let cx = (left + right) / 2.0;
                    let cy = (top + bottom) / 2.0;
                    let rx = w / 2.0;

                    // 1. Dashed bounding square
                    if let Some(gb) = &guide_brush {
                        let rect = D2D_RECT_F {
                            left,
                            top,
                            right,
                            bottom,
                        };
                        rt.DrawRectangle(&rect, gb, 1.0, Some(&self.dashed_stroke_style));
                    }

                    // 2. Center crosshair (+)
                    if let Some(ab) = &amber_brush {
                        let arm = 8.0;
                        rt.DrawLine(v2(cx - arm, cy), v2(cx + arm, cy), ab, 1.5, None);
                        rt.DrawLine(v2(cx, cy - arm), v2(cx, cy + arm), ab, 1.5, None);
                    }

                    // 3. 1:1 Circle dimension badge
                    let badge_text = format!("1:1 Circle • ⌀ {:.0} px", rx * 2.0);
                    let badge_utf16: Vec<u16> = badge_text.encode_utf16().collect();

                    let badge_w = (badge_text.len() as f32 * 8.0 + 16.0).max(110.0);
                    let badge_h = 22.0;
                    let badge_x = (right - badge_w).max(left);
                    let badge_y = bottom + 6.0;

                    let badge_rect = D2D_RECT_F {
                        left: badge_x,
                        top: badge_y,
                        right: badge_x + badge_w,
                        bottom: badge_y + badge_h,
                    };
                    let badge_rrect = D2D1_ROUNDED_RECT {
                        rect: badge_rect,
                        radiusX: 5.0,
                        radiusY: 5.0,
                    };

                    if let (Some(bgb), Some(bdb), Some(tb)) =
                        (&bg_brush, &border_brush, &text_brush)
                    {
                        rt.FillRoundedRectangle(&badge_rrect, bgb);
                        rt.DrawRoundedRectangle(&badge_rrect, bdb, 1.0, None);
                        rt.DrawText(
                            &badge_utf16,
                            &self.text_format_toolbar_small,
                            &badge_rect,
                            tb,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                }

                _ => {}
            }
        }
    }
}
