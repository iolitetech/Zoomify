use super::{D2DRenderer, v2};
use crate::types::{SnipSelection, SnipShape};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
    D2D1_FILL_MODE_ALTERNATE,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_ROUNDED_RECT, ID2D1RenderTarget,
};

impl D2DRenderer {
    pub(super) unsafe fn render_snip_overlay(
        &self,
        rt: &ID2D1RenderTarget,
        screen_w: f32,
        screen_h: f32,
        snip: &SnipSelection,
    ) {
        unsafe {
            let (left, top, right, bottom) = snip.rect();

            if let Ok(path) = self.factory.CreatePathGeometry()
                && let Ok(sink) = path.Open()
            {
                sink.SetFillMode(D2D1_FILL_MODE_ALTERNATE);

                sink.BeginFigure(v2(0.0, 0.0), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLine(v2(screen_w, 0.0));
                sink.AddLine(v2(screen_w, screen_h));
                sink.AddLine(v2(0.0, screen_h));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);

                if snip.shape == SnipShape::Ellipse {
                    let cx = (left + right) / 2.0;
                    let cy = (top + bottom) / 2.0;
                    let rx = (right - left) / 2.0;
                    let ry = (bottom - top) / 2.0;
                    if rx > 2.0 && ry > 2.0 {
                        let segs = 64;
                        let start_p = v2(cx + rx, cy);
                        sink.BeginFigure(start_p, D2D1_FIGURE_BEGIN_FILLED);
                        for i in 1..=segs {
                            let angle = (i as f32 / segs as f32) * std::f32::consts::PI * 2.0;
                            let px = cx + rx * angle.cos();
                            let py = cy + ry * angle.sin();
                            sink.AddLine(v2(px, py));
                        }
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    }
                } else {
                    sink.BeginFigure(v2(left, top), D2D1_FIGURE_BEGIN_FILLED);
                    sink.AddLine(v2(right, top));
                    sink.AddLine(v2(right, bottom));
                    sink.AddLine(v2(left, bottom));
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                }
                let _ = sink.Close();

                let mask_col = D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.6,
                };
                if let Ok(brush) = rt.CreateSolidColorBrush(&mask_col, None) {
                    rt.FillGeometry(&path, &brush, None);
                }
            }

            let border_col = D2D1_COLOR_F {
                r: 0.15,
                g: 0.65,
                b: 1.0,
                a: 1.0,
            };
            if let Ok(brush) = rt.CreateSolidColorBrush(&border_col, None) {
                if snip.shape == SnipShape::Ellipse {
                    let cx = (left + right) / 2.0;
                    let cy = (top + bottom) / 2.0;
                    let rx = (right - left) / 2.0;
                    let ry = (bottom - top) / 2.0;
                    let ellipse = D2D1_ELLIPSE {
                        point: v2(cx, cy),
                        radiusX: rx,
                        radiusY: ry,
                    };
                    rt.DrawEllipse(&ellipse, &brush, 2.0, None);
                } else {
                    let snip_rect = D2D_RECT_F {
                        left,
                        top,
                        right,
                        bottom,
                    };
                    rt.DrawRectangle(&snip_rect, &brush, 2.0, None);
                }
            }

            // ── Guide Lines When Snipping with Shift ──
            if snip.with_guides {
                let guide_col = D2D1_COLOR_F {
                    r: 0.15,
                    g: 0.75,
                    b: 1.0,
                    a: 0.55,
                };
                let center_guide_col = D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.85,
                    b: 0.2,
                    a: 0.70,
                };
                let grid_col = D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.25,
                };

                if let Ok(guide_brush) = rt.CreateSolidColorBrush(&guide_col, None) {
                    // Full-screen boundary projection lines
                    rt.DrawLine(
                        v2(0.0, top),
                        v2(screen_w, top),
                        &guide_brush,
                        1.0,
                        Some(&self.dashed_stroke_style),
                    );
                    rt.DrawLine(
                        v2(0.0, bottom),
                        v2(screen_w, bottom),
                        &guide_brush,
                        1.0,
                        Some(&self.dashed_stroke_style),
                    );
                    rt.DrawLine(
                        v2(left, 0.0),
                        v2(left, screen_h),
                        &guide_brush,
                        1.0,
                        Some(&self.dashed_stroke_style),
                    );
                    rt.DrawLine(
                        v2(right, 0.0),
                        v2(right, screen_h),
                        &guide_brush,
                        1.0,
                        Some(&self.dashed_stroke_style),
                    );
                }

                // Rule-of-Thirds Grid inside the selection
                let sw_w = right - left;
                let sw_h = bottom - top;
                if sw_w > 40.0
                    && sw_h > 40.0
                    && let Ok(grid_brush) = rt.CreateSolidColorBrush(&grid_col, None)
                {
                    let x1 = left + sw_w / 3.0;
                    let x2 = left + sw_w * 2.0 / 3.0;
                    let y1 = top + sw_h / 3.0;
                    let y2 = top + sw_h * 2.0 / 3.0;

                    rt.DrawLine(
                        v2(x1, top),
                        v2(x1, bottom),
                        &grid_brush,
                        1.0,
                        Some(&self.dotted_stroke_style),
                    );
                    rt.DrawLine(
                        v2(x2, top),
                        v2(x2, bottom),
                        &grid_brush,
                        1.0,
                        Some(&self.dotted_stroke_style),
                    );
                    rt.DrawLine(
                        v2(left, y1),
                        v2(right, y1),
                        &grid_brush,
                        1.0,
                        Some(&self.dotted_stroke_style),
                    );
                    rt.DrawLine(
                        v2(left, y2),
                        v2(right, y2),
                        &grid_brush,
                        1.0,
                        Some(&self.dotted_stroke_style),
                    );
                }

                // Center crosshair
                let mid_x = (left + right) / 2.0;
                let mid_y = (top + bottom) / 2.0;
                if let Ok(c_brush) = rt.CreateSolidColorBrush(&center_guide_col, None) {
                    rt.DrawLine(
                        v2(mid_x - 12.0, mid_y),
                        v2(mid_x + 12.0, mid_y),
                        &c_brush,
                        1.5,
                        None,
                    );
                    rt.DrawLine(
                        v2(mid_x, mid_y - 12.0),
                        v2(mid_x, mid_y + 12.0),
                        &c_brush,
                        1.5,
                        None,
                    );
                }
            }

            let w_px = (right - left).round() as u32;
            let h_px = (bottom - top).round() as u32;
            let shape_tag = if snip.shape == SnipShape::Ellipse {
                "⭕ Circle"
            } else {
                "🔲 Rect"
            };
            let badge_text = if snip.with_guides {
                format!(
                    "✂️ {} ({}×{} px 1:1) • Guides Active (Shift)",
                    shape_tag, w_px, h_px
                )
            } else {
                format!(
                    "✂️ {} ({}×{} px) • Tab: Switch | Shift: Square & Guides",
                    shape_tag, w_px, h_px
                )
            };
            let utf16: Vec<u16> = badge_text.encode_utf16().collect();

            let badge_x = left.max(10.0);
            let badge_y = if top > 40.0 { top - 32.0 } else { bottom + 8.0 };

            let badge_rect = D2D_RECT_F {
                left: badge_x,
                top: badge_y,
                right: badge_x + 330.0,
                bottom: badge_y + 26.0,
            };

            let badge_bg = D2D1_COLOR_F {
                r: 0.08,
                g: 0.09,
                b: 0.12,
                a: 0.9,
            };
            if let Ok(bg_brush) = rt.CreateSolidColorBrush(&badge_bg, None) {
                let rrect = D2D1_ROUNDED_RECT {
                    rect: badge_rect,
                    radiusX: 4.0,
                    radiusY: 4.0,
                };
                rt.FillRoundedRectangle(&rrect, &bg_brush);
            }

            let text_col = D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.95,
            };
            if let Ok(tbrush) = rt.CreateSolidColorBrush(&text_col, None) {
                let text_draw_rect = D2D_RECT_F {
                    left: badge_x + 8.0,
                    top: badge_y + 3.0,
                    right: badge_x + 305.0,
                    bottom: badge_y + 25.0,
                };
                rt.DrawText(
                    &utf16,
                    &self.text_format_hud,
                    &text_draw_rect,
                    &tbrush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }
}
