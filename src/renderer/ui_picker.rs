use super::{D2DRenderer, v2};
use crate::types::{ColorPickerState, TextFontFamily, hsv_to_rgb};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F, D2D1_GRADIENT_STOP};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL;
use windows_numerics::Vector2;

/// Gradient stops sampled across each slider - a true gradient brush
/// interpolates between them, so this is only about matching the previous
/// per-strip look's colour resolution, not faking smoothness the way
/// discrete filled strips had to.
const STOPS: usize = 64;

#[inline]
fn rgb_color(r: u8, g: u8, b: u8) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}

#[inline]
fn white(a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a,
    }
}

impl D2DRenderer {
    pub(super) unsafe fn render_color_picker(
        &self,
        rt: &ID2D1RenderTarget,
        picker: &ColorPickerState,
    ) {
        if !picker.open {
            return;
        }

        unsafe {
            let card = D2D1_ROUNDED_RECT {
                rect: picker.panel,
                radiusX: 9.0,
                radiusY: 9.0,
            };
            if let Some(b) = self.solid_brush(
                rt,
                &D2D1_COLOR_F {
                    r: 0.13,
                    g: 0.13,
                    b: 0.14,
                    a: 0.97,
                },
            ) {
                rt.FillRoundedRectangle(&card, &b);
            }
            if let Some(b) = self.solid_brush(rt, &white(0.14)) {
                rt.DrawRoundedRectangle(&card, &b, 1.0, None);
            }

            // 0 = hue ramp, 1 = saturation at the current hue, 2 = value.
            let bars: [(D2D_RECT_F, u8, f32); 3] = [
                (picker.hue_bar, 0, picker.hue / 360.0),
                (picker.sat_bar, 1, picker.sat),
                (picker.val_bar, 2, picker.val),
            ];

            for (rect, kind, pos) in bars {
                let w = rect.right - rect.left;

                // A single gradient brush per bar instead of up to STRIPS
                // separate solid-colour brushes: dragging the hue slider
                // changes every stop of the saturation/value bars every
                // frame (they depend on picker.hue), which used to mean up
                // to ~128 new colours a frame flooding the shared
                // solid_brush_cache (shared with every on-canvas shape) past
                // its cap and forcing a full flush, repeatedly, for as long
                // as the drag continued. A gradient stop collection is its
                // own resource, never touching that cache at all.
                let stops: Vec<D2D1_GRADIENT_STOP> = (0..STOPS)
                    .map(|i| {
                        let t = i as f32 / (STOPS - 1) as f32;
                        let (r, g, b) = match kind {
                            0 => hsv_to_rgb(t * 360.0, 1.0, 1.0),
                            1 => hsv_to_rgb(picker.hue, t, picker.val.max(0.15)),
                            _ => hsv_to_rgb(picker.hue, picker.sat, t),
                        };
                        D2D1_GRADIENT_STOP {
                            position: t,
                            color: rgb_color(r, g, b),
                        }
                    })
                    .collect();
                if let Ok(stop_collection) =
                    rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                    && let Ok(gradient) = rt.CreateLinearGradientBrush(
                        &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                            startPoint: Vector2 {
                                X: rect.left,
                                Y: rect.top,
                            },
                            endPoint: Vector2 {
                                X: rect.right,
                                Y: rect.top,
                            },
                        },
                        None,
                        &stop_collection,
                    )
                {
                    rt.FillRectangle(&rect, &gradient);
                }

                // Thumb: dark halo then white ring, so it reads on any ramp.
                let tx = rect.left + pos.clamp(0.0, 1.0) * w;
                let cy = (rect.top + rect.bottom) / 2.0;
                let thumb = D2D1_ELLIPSE {
                    point: v2(tx, cy),
                    radiusX: 7.0,
                    radiusY: 7.0,
                };
                if let Some(b) = self.solid_brush(
                    rt,
                    &D2D1_COLOR_F {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.55,
                    },
                ) {
                    rt.DrawEllipse(&thumb, &b, 3.0, None);
                }
                if let Some(b) = self.solid_brush(rt, &white(1.0)) {
                    rt.DrawEllipse(&thumb, &b, 1.6, None);
                }
            }

            // Live preview chip
            let cur = picker.current();
            let chip = D2D1_ROUNDED_RECT {
                rect: picker.preview,
                radiusX: 6.0,
                radiusY: 6.0,
            };
            if let Some(b) = self.solid_brush(rt, &cur.to_d2d_color(1.0)) {
                rt.FillRoundedRectangle(&chip, &b);
            }
            if let Some(b) = self.solid_brush(rt, &white(0.35)) {
                rt.DrawRoundedRectangle(&chip, &b, 1.0, None);
            }

            // Hex readout
            if let Ok(fmt) =
                self.get_custom_text_format(11.0, true, false, TextFontFamily::CascadiaCode)
                && let Some(b) = self.solid_brush(
                    rt,
                    &D2D1_COLOR_F {
                        r: 0.86,
                        g: 0.86,
                        b: 0.88,
                        a: 1.0,
                    },
                )
            {
                let label: Vec<u16> = cur.name().encode_utf16().collect();
                let tr = D2D_RECT_F {
                    left: picker.panel.left + 12.0,
                    top: picker.val_bar.bottom + 40.0,
                    right: picker.panel.right - 12.0,
                    bottom: picker.panel.bottom - 2.0,
                };
                rt.DrawText(
                    &label,
                    &fmt,
                    &tr,
                    &b,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            // Recently used customs
            for (c, r) in &picker.recent_swatches {
                let sw = D2D1_ROUNDED_RECT {
                    rect: *r,
                    radiusX: 5.0,
                    radiusY: 5.0,
                };
                if let Some(b) = self.solid_brush(rt, &c.to_d2d_color(1.0)) {
                    rt.FillRoundedRectangle(&sw, &b);
                }
                let selected = *c == cur;
                if let Some(b) = self.solid_brush(rt, &white(if selected { 0.95 } else { 0.25 })) {
                    rt.DrawRoundedRectangle(&sw, &b, if selected { 2.0 } else { 1.0 }, None);
                }
            }
        }
    }
}
