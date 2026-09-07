use super::{D2DRenderer, v2};
use crate::types::{ColorPickerState, TextFontFamily, hsv_to_rgb};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_ROUNDED_RECT, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL;

/// Strips used to fake a gradient across each slider. Enough to read as smooth
/// at these widths, and the colours repeat every frame so the brush cache holds
/// them after the first paint.
const STRIPS: usize = 64;

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
                let step = w / STRIPS as f32;

                for i in 0..STRIPS {
                    let t = i as f32 / (STRIPS - 1) as f32;
                    let (r, g, b) = match kind {
                        0 => hsv_to_rgb(t * 360.0, 1.0, 1.0),
                        1 => hsv_to_rgb(picker.hue, t, picker.val.max(0.15)),
                        _ => hsv_to_rgb(picker.hue, picker.sat, t),
                    };
                    if let Some(brush) = self.solid_brush(rt, &rgb_color(r, g, b)) {
                        let x = rect.left + i as f32 * step;
                        rt.FillRectangle(
                            &D2D_RECT_F {
                                left: x,
                                top: rect.top,
                                // Slight overlap so no seams show between strips.
                                right: (x + step + 0.75).min(rect.right),
                                bottom: rect.bottom,
                            },
                            &brush,
                        );
                    }
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
