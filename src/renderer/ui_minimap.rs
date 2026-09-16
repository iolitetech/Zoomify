use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_ROUNDED_RECT, ID2D1Bitmap, ID2D1RenderTarget,
};

use super::D2DRenderer;
use crate::types::{MinimapState, ZoomState};

impl D2DRenderer {
    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn render_minimap(
        &self,
        rt: &ID2D1RenderTarget,
        screen_w: f32,
        screen_h: f32,
        bg_bitmap: Option<&ID2D1Bitmap>,
        zoom: &ZoomState,
        minimap: &MinimapState,
    ) {
        let (card_l, card_t, card_r, card_b) = minimap.get_card_bounds(screen_w, screen_h);
        let (inner_l, inner_t, inner_r, inner_b) =
            minimap.get_inner_preview_rect(screen_w, screen_h);
        let (vp_l, vp_t, vp_r, vp_b) = minimap.get_viewport_rect(screen_w, screen_h, zoom);

        unsafe {
            // 1. Soft ambient shadow behind minimap card
            for i in 1..=4 {
                let offset = i as f32 * 1.5;
                let alpha = 0.08 / (i as f32);
                let shadow_col = D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: alpha,
                };
                if let Some(brush) = self.solid_brush(rt, &shadow_col) {
                    let s_rrect = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: card_l - offset,
                            top: card_t - offset + 2.0,
                            right: card_r + offset,
                            bottom: card_b + offset + 2.0,
                        },
                        radiusX: 8.0 + offset,
                        radiusY: 8.0 + offset,
                    };
                    rt.FillRoundedRectangle(&s_rrect, &brush);
                }
            }

            // 2. Main Card Background & Border
            let card_rrect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: card_l,
                    top: card_t,
                    right: card_r,
                    bottom: card_b,
                },
                radiusX: 8.0,
                radiusY: 8.0,
            };
            let bg_color = D2D1_COLOR_F {
                r: 0.11,
                g: 0.12,
                b: 0.14,
                a: 0.94,
            };
            if let Some(brush) = self.solid_brush(rt, &bg_color) {
                rt.FillRoundedRectangle(&card_rrect, &brush);
            }

            let border_color = if minimap.is_hovered || minimap.is_dragging {
                D2D1_COLOR_F {
                    r: 0.38,
                    g: 0.80,
                    b: 1.0,
                    a: 0.75,
                }
            } else {
                D2D1_COLOR_F {
                    r: 0.28,
                    g: 0.29,
                    b: 0.32,
                    a: 0.85,
                }
            };
            if let Some(brush) = self.solid_brush(rt, &border_color) {
                let stroke_w = if minimap.is_hovered || minimap.is_dragging {
                    1.5
                } else {
                    1.0
                };
                rt.DrawRoundedRectangle(&card_rrect, &brush, stroke_w, None);
            }

            // 3. Miniature Desktop Preview
            let inner_rect = D2D_RECT_F {
                left: inner_l,
                top: inner_t,
                right: inner_r,
                bottom: inner_b,
            };

            if let Some(bitmap) = bg_bitmap {
                // Draw background thumbnail scaled to inner rect
                rt.DrawBitmap(
                    bitmap,
                    Some(&inner_rect),
                    0.88,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                );

                // Subtle dark scrim overlay to heighten viewport contrast
                let scrim_color = D2D1_COLOR_F {
                    r: 0.05,
                    g: 0.06,
                    b: 0.08,
                    a: 0.25,
                };
                if let Some(brush) = self.solid_brush(rt, &scrim_color) {
                    rt.FillRectangle(&inner_rect, &brush);
                }
            } else {
                // Fallback flat preview background
                let empty_color = D2D1_COLOR_F {
                    r: 0.18,
                    g: 0.19,
                    b: 0.22,
                    a: 1.0,
                };
                if let Some(brush) = self.solid_brush(rt, &empty_color) {
                    rt.FillRectangle(&inner_rect, &brush);
                }
            }

            // 4. Highlighted Viewport Region
            let vp_rect = D2D_RECT_F {
                left: vp_l,
                top: vp_t,
                right: vp_r,
                bottom: vp_b,
            };

            // Viewport fill (Fluent cyan/blue transparent wash)
            let vp_fill_col = D2D1_COLOR_F {
                r: 0.38,
                g: 0.80,
                b: 1.0,
                a: 0.24,
            };
            if let Some(brush) = self.solid_brush(rt, &vp_fill_col) {
                rt.FillRectangle(&vp_rect, &brush);
            }

            // Viewport glowing border
            let vp_border_col = D2D1_COLOR_F {
                r: 0.38,
                g: 0.80,
                b: 1.0,
                a: 0.95,
            };
            if let Some(brush) = self.solid_brush(rt, &vp_border_col) {
                rt.DrawRectangle(&vp_rect, &brush, 2.0, None);
            }
        }
    }
}
