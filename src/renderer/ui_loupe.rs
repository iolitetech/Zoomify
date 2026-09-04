use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_ELLIPSE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_ROUNDED_RECT, ID2D1Bitmap, ID2D1RenderTarget,
};
use windows_numerics::Matrix3x2;

use super::{D2DRenderer, v2};
use crate::types::LoupeState;

impl D2DRenderer {
    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn render_loupe(
        &self,
        rt: &ID2D1RenderTarget,
        _screen_w: f32,
        _screen_h: f32,
        loupe: &LoupeState,
        bg_bitmap: Option<&ID2D1Bitmap>,
    ) {
        if !loupe.active {
            return;
        }

        let bg = match bg_bitmap {
            Some(b) => b,
            None => return,
        };

        unsafe {
            let cx = loupe.x;
            let cy = loupe.y;
            let r = loupe.radius.clamp(60.0, 500.0);
            let m = loupe.magnification.clamp(1.25, 12.0);

            // 1. Setup GPU hardware matrix for magnified bitmap brush:
            // Translate origin to (-cx, -cy), scale by (m, m), then translate back to (cx, cy).
            let brush_props = D2D1_BITMAP_BRUSH_PROPERTIES {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            };

            let Ok(bitmap_brush) = rt.CreateBitmapBrush(bg, Some(&brush_props), None) else {
                return;
            };

            // Direct2D matrix: M31 is X translation, M32 is Y translation, M11 is X scale, M22 is Y scale
            // Transform matrix T = Translate(-cx, -cy) * Scale(m) * Translate(cx, cy):
            // (x - cx) * m + cx = x * m + cx * (1 - m)
            let loupe_matrix = Matrix3x2 {
                M11: m,
                M12: 0.0,
                M21: 0.0,
                M22: m,
                M31: cx * (1.0 - m),
                M32: cy * (1.0 - m),
            };
            bitmap_brush.SetTransform(&loupe_matrix);

            // 2. Soft ambient drop shadow around lens
            for i in 1..=4 {
                let shadow_offset = i as f32 * 2.5;
                let shadow_alpha = (0.16 / (i as f32)).clamp(0.02, 0.2);
                let shadow_col = D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: shadow_alpha,
                };
                if let Ok(s_brush) = rt.CreateSolidColorBrush(&shadow_col, None) {
                    if loupe.is_rect {
                        let s_rrect = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: cx - r - shadow_offset,
                                top: cy - r * 0.7 - shadow_offset + 3.0,
                                right: cx + r + shadow_offset,
                                bottom: cy + r * 0.7 + shadow_offset + 3.0,
                            },
                            radiusX: 18.0 + shadow_offset,
                            radiusY: 18.0 + shadow_offset,
                        };
                        rt.DrawRoundedRectangle(&s_rrect, &s_brush, 3.0, None);
                    } else {
                        let s_ellipse = D2D1_ELLIPSE {
                            point: v2(cx, cy + 2.5),
                            radiusX: r + shadow_offset,
                            radiusY: r + shadow_offset,
                        };
                        rt.DrawEllipse(&s_ellipse, &s_brush, 3.0, None);
                    }
                }
            }

            // 3. Render magnified content inside lens
            if loupe.is_rect {
                let lens_rrect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: cx - r,
                        top: cy - r * 0.7,
                        right: cx + r,
                        bottom: cy + r * 0.7,
                    },
                    radiusX: 16.0,
                    radiusY: 16.0,
                };
                rt.FillRoundedRectangle(&lens_rrect, &bitmap_brush);

                // Bezel outer ring
                let rim_color = if loupe.pinned {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 0.42,
                        b: 0.15,
                        a: 0.95,
                    } // Vibrant orange when pinned
                } else {
                    D2D1_COLOR_F {
                        r: 0.15,
                        g: 0.16,
                        b: 0.18,
                        a: 0.92,
                    } // Deep charcoal bezel
                };
                if let Ok(rim_brush) = rt.CreateSolidColorBrush(&rim_color, None) {
                    rt.DrawRoundedRectangle(&lens_rrect, &rim_brush, 4.0, None);
                }

                // Inner glass sheen specular ring
                let sheen_color = D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.35,
                };
                if let Ok(sheen_brush) = rt.CreateSolidColorBrush(&sheen_color, None) {
                    let inner_rrect = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: cx - r + 1.5,
                            top: cy - r * 0.7 + 1.5,
                            right: cx + r - 1.5,
                            bottom: cy + r * 0.7 - 1.5,
                        },
                        radiusX: 15.0,
                        radiusY: 15.0,
                    };
                    rt.DrawRoundedRectangle(&inner_rrect, &sheen_brush, 1.2, None);
                }
            } else {
                let lens_ellipse = D2D1_ELLIPSE {
                    point: v2(cx, cy),
                    radiusX: r,
                    radiusY: r,
                };
                rt.FillEllipse(&lens_ellipse, &bitmap_brush);

                // Bezel outer ring
                let rim_color = if loupe.pinned {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 0.42,
                        b: 0.15,
                        a: 0.95,
                    } // Vibrant orange when pinned
                } else {
                    D2D1_COLOR_F {
                        r: 0.15,
                        g: 0.16,
                        b: 0.18,
                        a: 0.92,
                    } // Deep charcoal bezel
                };
                if let Ok(rim_brush) = rt.CreateSolidColorBrush(&rim_color, None) {
                    rt.DrawEllipse(&lens_ellipse, &rim_brush, 4.0, None);
                }

                // Inner glass sheen specular ring
                let sheen_color = D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.35,
                };
                if let Ok(sheen_brush) = rt.CreateSolidColorBrush(&sheen_color, None) {
                    let inner_ellipse = D2D1_ELLIPSE {
                        point: v2(cx, cy),
                        radiusX: (r - 1.5).max(1.0),
                        radiusY: (r - 1.5).max(1.0),
                    };
                    rt.DrawEllipse(&inner_ellipse, &sheen_brush, 1.2, None);
                }
            }

            // 4. Precision Reticle / Crosshair (if enabled)
            if loupe.show_reticle {
                let crosshair_col = D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.75,
                };
                let shadow_col = D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.65,
                };

                if let Ok(ch_brush) = rt.CreateSolidColorBrush(&crosshair_col, None)
                    && let Ok(sh_brush) = rt.CreateSolidColorBrush(&shadow_col, None)
                {
                    let arm_len = 12.0;
                    let gap = 5.0;

                    // Shadow crosshair (offset by 1px)
                    rt.DrawLine(
                        v2(cx - arm_len + 1.0, cy + 1.0),
                        v2(cx - gap + 1.0, cy + 1.0),
                        &sh_brush,
                        1.5,
                        None,
                    );
                    rt.DrawLine(
                        v2(cx + gap + 1.0, cy + 1.0),
                        v2(cx + arm_len + 1.0, cy + 1.0),
                        &sh_brush,
                        1.5,
                        None,
                    );
                    rt.DrawLine(
                        v2(cx + 1.0, cy - arm_len + 1.0),
                        v2(cx + 1.0, cy - gap + 1.0),
                        &sh_brush,
                        1.5,
                        None,
                    );
                    rt.DrawLine(
                        v2(cx + 1.0, cy + gap + 1.0),
                        v2(cx + 1.0, cy + arm_len + 1.0),
                        &sh_brush,
                        1.5,
                        None,
                    );

                    // Foreground crosshair
                    rt.DrawLine(v2(cx - arm_len, cy), v2(cx - gap, cy), &ch_brush, 1.5, None);
                    rt.DrawLine(v2(cx + gap, cy), v2(cx + arm_len, cy), &ch_brush, 1.5, None);
                    rt.DrawLine(v2(cx, cy - arm_len), v2(cx, cy - gap), &ch_brush, 1.5, None);
                    rt.DrawLine(v2(cx, cy + gap), v2(cx, cy + arm_len), &ch_brush, 1.5, None);

                    // Center target dot
                    let dot = D2D1_ELLIPSE {
                        point: v2(cx, cy),
                        radiusX: 1.5,
                        radiusY: 1.5,
                    };
                    rt.FillEllipse(&dot, &ch_brush);
                }
            }

            // 5. Magnification Pill Badge attached to the bottom rim
            let badge_text = if loupe.pinned {
                format!("📌 {:.1}x", m)
            } else {
                format!("{:.1}x", m)
            };
            let badge_w = 64.0;
            let badge_h = 24.0;
            let badge_y = if loupe.is_rect {
                cy + r * 0.7 - (badge_h / 2.0)
            } else {
                cy + r - (badge_h / 2.0)
            };
            let badge_rect = D2D_RECT_F {
                left: cx - (badge_w / 2.0),
                top: badge_y,
                right: cx + (badge_w / 2.0),
                bottom: badge_y + badge_h,
            };
            let badge_rrect = D2D1_ROUNDED_RECT {
                rect: badge_rect,
                radiusX: 12.0,
                radiusY: 12.0,
            };

            let badge_bg_col = if loupe.pinned {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.42,
                    b: 0.15,
                    a: 0.95,
                }
            } else {
                D2D1_COLOR_F {
                    r: 0.10,
                    g: 0.11,
                    b: 0.13,
                    a: 0.92,
                }
            };
            if let Ok(badge_bg) = rt.CreateSolidColorBrush(&badge_bg_col, None) {
                rt.FillRoundedRectangle(&badge_rrect, &badge_bg);
            }
            let border_col = D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.35,
            };
            if let Ok(border_brush) = rt.CreateSolidColorBrush(&border_col, None) {
                rt.DrawRoundedRectangle(&badge_rrect, &border_brush, 1.0, None);
            }

            // Badge text via DirectWrite
            let wide_str: Vec<u16> = badge_text.encode_utf16().collect();
            let text_color = D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.95,
            };
            if let Ok(text_brush) = rt.CreateSolidColorBrush(&text_color, None) {
                rt.DrawText(
                    &wide_str,
                    &self.text_format_toolbar_small,
                    &badge_rect,
                    &text_brush,
                    windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }
}
