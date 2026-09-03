use super::{D2DRenderer, v2};
use crate::shapes::{calculate_arrow_head, normalize_rect, points_to_bezier_segments};
use crate::types::{
    ArrowStyle, BadgeShape, ColorPreset, FillMode, LaserTrailPoint, Point2D, Shape, StrokePattern,
    TextCardStyle, TextEditorState,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_ROUNDED_RECT, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
};

impl D2DRenderer {
    pub(super) unsafe fn render_single_shape(&self, rt: &ID2D1RenderTarget, shape: &Shape) {
        unsafe {
            match shape {
                Shape::Stroke {
                    points,
                    color,
                    width,
                    is_highlighter,
                    pattern,
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
                    let col = color.to_d2d_color(alpha);

                    if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
                        if points.len() == 1 {
                            let dot = D2D1_ELLIPSE {
                                point: v2(points[0].x, points[0].y),
                                radiusX: actual_width / 2.0,
                                radiusY: actual_width / 2.0,
                            };
                            rt.FillEllipse(&dot, &brush);
                        } else if let Ok(path) = self.factory.CreatePathGeometry()
                            && let Ok(sink) = path.Open()
                        {
                            sink.BeginFigure(
                                v2(points[0].x, points[0].y),
                                D2D1_FIGURE_BEGIN_HOLLOW,
                            );
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
                } => {
                    let col = color.to_d2d_color(1.0);
                    if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
                        let p0 = v2(start.x, start.y);
                        let p1 = v2(end.x, end.y);
                        let stroke_style = self.get_stroke_style(*pattern);
                        rt.DrawLine(p0, p1, &brush, *width, Some(stroke_style));
                    }
                }

                Shape::Arrow {
                    start,
                    end,
                    color,
                    width,
                    style,
                    pattern,
                } => {
                    let col = color.to_d2d_color(1.0);
                    if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
                        let p0 = v2(start.x, start.y);
                        let p1 = v2(end.x, end.y);
                        let stroke_style = self.get_stroke_style(*pattern);
                        rt.DrawLine(p0, p1, &brush, *width, Some(stroke_style));

                        let head_len = (*width * 5.0 + 12.0).clamp(16.0, 48.0);

                        // Draw arrow head at end
                        let (tip, left, right) = calculate_arrow_head(*start, *end, head_len);
                        if let Ok(path) = self.factory.CreatePathGeometry()
                            && let Ok(sink) = path.Open()
                        {
                            sink.BeginFigure(v2(tip.x, tip.y), D2D1_FIGURE_BEGIN_FILLED);
                            sink.AddLine(v2(left.x, left.y));
                            sink.AddLine(v2(right.x, right.y));
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                            let _ = sink.Close();
                            rt.FillGeometry(&path, &brush, None);
                        }

                        // If Double or Dimension: also draw arrow head at start
                        if *style == ArrowStyle::Double || *style == ArrowStyle::Dimension {
                            let (tip2, left2, right2) =
                                calculate_arrow_head(*end, *start, head_len);
                            if let Ok(path2) = self.factory.CreatePathGeometry()
                                && let Ok(sink2) = path2.Open()
                            {
                                sink2.BeginFigure(v2(tip2.x, tip2.y), D2D1_FIGURE_BEGIN_FILLED);
                                sink2.AddLine(v2(left2.x, left2.y));
                                sink2.AddLine(v2(right2.x, right2.y));
                                sink2.EndFigure(D2D1_FIGURE_END_CLOSED);
                                let _ = sink2.Close();
                                rt.FillGeometry(&path2, &brush, None);
                            }
                        }

                        // If Dimension: draw perpendicular end-caps (ticks)
                        if *style == ArrowStyle::Dimension {
                            let dx = end.x - start.x;
                            let dy = end.y - start.y;
                            let len = (dx * dx + dy * dy).sqrt();
                            if len > 1.0 {
                                let perp_x = -dy / len;
                                let perp_y = dx / len;
                                let tick_h = head_len * 0.9;

                                let s_top =
                                    v2(start.x + perp_x * tick_h, start.y + perp_y * tick_h);
                                let s_bot =
                                    v2(start.x - perp_x * tick_h, start.y - perp_y * tick_h);
                                rt.DrawLine(s_top, s_bot, &brush, *width * 1.2, None);

                                let e_top = v2(end.x + perp_x * tick_h, end.y + perp_y * tick_h);
                                let e_bot = v2(end.x - perp_x * tick_h, end.y - perp_y * tick_h);
                                rt.DrawLine(e_top, e_bot, &brush, *width * 1.2, None);
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
                            let fill_col = color.to_d2d_color(0.22);
                            if let Ok(fbrush) = rt.CreateSolidColorBrush(&fill_col, None) {
                                if *rounded {
                                    rt.FillRoundedRectangle(&rrect, &fbrush);
                                } else {
                                    rt.FillRectangle(&rect, &fbrush);
                                }
                            }
                        }
                        FillMode::Solid => {
                            let fill_col = color.to_d2d_color(1.0);
                            if let Ok(fbrush) = rt.CreateSolidColorBrush(&fill_col, None) {
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
                    let col = color.to_d2d_color(1.0);
                    if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
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
                            let fill_col = color.to_d2d_color(0.22);
                            if let Ok(fbrush) = rt.CreateSolidColorBrush(&fill_col, None) {
                                rt.FillEllipse(&ellipse, &fbrush);
                            }
                        }
                        FillMode::Solid => {
                            let fill_col = color.to_d2d_color(1.0);
                            if let Ok(fbrush) = rt.CreateSolidColorBrush(&fill_col, None) {
                                rt.FillEllipse(&ellipse, &fbrush);
                            }
                        }
                        FillMode::None => {}
                    }

                    let col = color.to_d2d_color(1.0);
                    if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
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
                    let col = color.to_d2d_color(1.0);
                    let text_utf16: Vec<u16> = text.encode_utf16().collect();
                    if let Ok(custom_format) =
                        self.get_custom_text_format(*font_size, *is_bold, *is_italic, *font_family)
                    {
                        let layout_w = (text.len() as f32 * font_size * 0.70).max(30.0) + 16.0;
                        let layout_h = font_size * 1.5 + 8.0;

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
                                if let Ok(card_bg) = rt.CreateSolidColorBrush(
                                    &D2D1_COLOR_F {
                                        r: 0.08,
                                        g: 0.09,
                                        b: 0.12,
                                        a: 0.65,
                                    },
                                    None,
                                ) {
                                    rt.FillRoundedRectangle(&card_rrect, &card_bg);
                                }
                                if let Ok(card_border) = rt.CreateSolidColorBrush(
                                    &D2D1_COLOR_F {
                                        r: 1.0,
                                        g: 1.0,
                                        b: 1.0,
                                        a: 0.15,
                                    },
                                    None,
                                ) {
                                    rt.DrawRoundedRectangle(&card_rrect, &card_border, 1.0, None);
                                }
                            }
                            TextCardStyle::Solid => {
                                if let Ok(card_bg) = rt.CreateSolidColorBrush(
                                    &D2D1_COLOR_F {
                                        r: 0.12,
                                        g: 0.13,
                                        b: 0.17,
                                        a: 0.96,
                                    },
                                    None,
                                ) {
                                    rt.FillRoundedRectangle(&card_rrect, &card_bg);
                                }
                                if let Ok(card_border) = rt.CreateSolidColorBrush(&col, None) {
                                    rt.DrawRoundedRectangle(&card_rrect, &card_border, 1.5, None);
                                }
                            }
                            TextCardStyle::Transparent => {
                                if let Ok(sh_brush) = rt.CreateSolidColorBrush(
                                    &D2D1_COLOR_F {
                                        r: 0.0,
                                        g: 0.0,
                                        b: 0.0,
                                        a: 0.70,
                                    },
                                    None,
                                ) {
                                    let sh_rect = D2D_RECT_F {
                                        left: text_rect.left + 1.2,
                                        top: text_rect.top + 1.5,
                                        right: text_rect.right + 1.2,
                                        bottom: text_rect.bottom + 1.5,
                                    };
                                    rt.DrawText(
                                        &text_utf16,
                                        &custom_format,
                                        &sh_rect,
                                        &sh_brush,
                                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                                    );
                                }
                            }
                        }

                        if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
                            rt.DrawText(
                                &text_utf16,
                                &custom_format,
                                &text_rect,
                                &brush,
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                                windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
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
                    let col = color.to_d2d_color(1.0);
                    let border_w = stroke_width.max(1.0);
                    let border_brush = rt.CreateSolidColorBrush(&col, None).ok();

                    let fill_brush = match fill {
                        FillMode::None => None,
                        FillMode::Tinted => {
                            let fill_col = color.to_d2d_color(0.30);
                            rt.CreateSolidColorBrush(&fill_col, None).ok()
                        }
                        FillMode::Solid => rt.CreateSolidColorBrush(&col, None).ok(),
                    };

                    let backplate_brush = if *fill == FillMode::Tinted {
                        let bp_col = D2D1_COLOR_F {
                            r: 0.08,
                            g: 0.10,
                            b: 0.14,
                            a: 0.70,
                        };
                        rt.CreateSolidColorBrush(&bp_col, None).ok()
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
                            let mut points = [Point2D::default(); 6];
                            for (i, p) in points.iter_mut().enumerate() {
                                let angle = (i as f32 * std::f32::consts::PI / 3.0)
                                    - std::f32::consts::FRAC_PI_2;
                                *p = Point2D::new(
                                    center.x + radius * angle.cos(),
                                    center.y + radius * angle.sin(),
                                );
                            }
                            if let Ok(path) = self.factory.CreatePathGeometry()
                                && let Ok(sink) = path.Open()
                            {
                                sink.BeginFigure(
                                    v2(points[0].x, points[0].y),
                                    D2D1_FIGURE_BEGIN_FILLED,
                                );
                                for pt in &points[1..] {
                                    sink.AddLine(v2(pt.x, pt.y));
                                }
                                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                                let _ = sink.Close();
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
                        let _ = custom_fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                        let _ = custom_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                        let text_rect = D2D_RECT_F {
                            left: center.x - *radius,
                            top: center.y - *radius,
                            right: center.x + *radius,
                            bottom: center.y + *radius,
                        };
                        if let Ok(tbrush) = rt.CreateSolidColorBrush(&text_col, None) {
                            rt.DrawText(&text_utf16, &custom_fmt, &text_rect, &tbrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                        }
                    }
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
            let origin = &editor.origin;
            let text = &editor.text;
            let cursor = editor.cursor;
            let col = editor.color.to_d2d_color(1.0);
            let font_size = editor.font_size;

            let estimated_w = (text.len() as f32 * font_size * 0.70).max(140.0) + 30.0;
            let estimated_h = font_size * 1.6 + 14.0;
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

            match editor.card_style {
                TextCardStyle::Badge => {
                    if let Ok(bg_brush) = rt.CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 0.08,
                            g: 0.09,
                            b: 0.12,
                            a: 0.65,
                        },
                        None,
                    ) {
                        rt.FillRoundedRectangle(&rrect, &bg_brush);
                    }
                    if let Ok(border_brush) = rt.CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 0.38,
                            g: 0.72,
                            b: 0.98,
                            a: 0.85,
                        },
                        None,
                    ) {
                        rt.DrawRoundedRectangle(&rrect, &border_brush, 1.5, None);
                    }
                }
                TextCardStyle::Solid => {
                    if let Ok(bg_brush) = rt.CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 0.12,
                            g: 0.13,
                            b: 0.17,
                            a: 0.96,
                        },
                        None,
                    ) {
                        rt.FillRoundedRectangle(&rrect, &bg_brush);
                    }
                    if let Ok(border_brush) = rt.CreateSolidColorBrush(&col, None) {
                        rt.DrawRoundedRectangle(&rrect, &border_brush, 1.5, None);
                    }
                }
                TextCardStyle::Transparent => {
                    if let Ok(bg_brush) = rt.CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 0.05,
                            g: 0.05,
                            b: 0.08,
                            a: 0.45,
                        },
                        None,
                    ) {
                        rt.FillRoundedRectangle(&rrect, &bg_brush);
                    }
                    if let Ok(border_brush) = rt.CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 0.38,
                            g: 0.72,
                            b: 0.98,
                            a: 0.80,
                        },
                        None,
                    ) {
                        rt.DrawRoundedRectangle(&rrect, &border_brush, 1.0, None);
                    }
                }
            }

            if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
                let display_text = if show_caret {
                    let mut s = text.to_string();
                    let safe_idx = cursor.min(s.len());
                    s.insert(safe_idx, '|');
                    s
                } else {
                    text.to_string()
                };
                let utf16: Vec<u16> = display_text.encode_utf16().collect();
                let layout_rect = D2D_RECT_F {
                    left: origin.x,
                    top: origin.y,
                    right: origin.x + estimated_w,
                    bottom: origin.y + estimated_h,
                };

                if let Ok(format) = self.get_custom_text_format(
                    font_size,
                    editor.is_bold,
                    editor.is_italic,
                    editor.font_family,
                ) {
                    rt.DrawText(
                        &utf16,
                        &format,
                        &layout_rect,
                        &brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }
        }
    }

    pub(super) unsafe fn render_laser_pointer(
        &self,
        rt: &ID2D1RenderTarget,
        trail: &[LaserTrailPoint],
        current_pos: Option<Point2D>,
        color: ColorPreset,
    ) {
        unsafe {
            let now = std::time::Instant::now();
            let base_col = color.to_d2d_color(1.0);

            // Draw decaying trail segments
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
                    if let Ok(brush) = rt.CreateSolidColorBrush(&seg_col, None) {
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
                if let Ok(brush) = rt.CreateSolidColorBrush(&halo_col, None) {
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
                if let Ok(brush) = rt.CreateSolidColorBrush(&mid_col, None) {
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
                if let Ok(brush) = rt.CreateSolidColorBrush(&white_col, None) {
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
            if let Ok(fbrush) = rt.CreateSolidColorBrush(&fill_col, None) {
                rt.FillEllipse(&el, &fbrush);
            }
            if let Ok(rbrush) = rt.CreateSolidColorBrush(&ring_col, None) {
                rt.DrawEllipse(&el, &rbrush, 1.5, None);
            }
        }
    }
}
