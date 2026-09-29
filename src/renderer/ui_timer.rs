use super::{D2DRenderer, v2};
use crate::types::{TimerAction, TimerWidgetState};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_ROUNDED_RECT, ID2D1RenderTarget,
};
use windows_core::Interface;

/// Same cap as `shapes::GEOMETRY_CACHE_MAX_ENTRIES` (kept as its own constant
/// rather than importing a private sibling item): both clear the same shared
/// `geometry_cache` when it grows past this, so any consistent bound works.
const GEOMETRY_CACHE_MAX_ENTRIES: usize = 512;

impl D2DRenderer {
    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn render_countdown_timer(
        &self,
        rt: &ID2D1RenderTarget,
        screen_w: f32,
        screen_h: f32,
        mins: u32,
        secs: u32,
        progress: f32,
        paused: bool,
        is_overtime: bool,
        widget: &TimerWidgetState,
    ) {
        unsafe {
            if widget.minimized {
                // Render Corner Mini-Pill based on widget.pill_corner
                let (left, top, right, bottom) = widget.get_pill_rect(screen_w, screen_h);
                let pill_rect = D2D_RECT_F {
                    left,
                    top,
                    right,
                    bottom,
                };
                let rrect = D2D1_ROUNDED_RECT {
                    rect: pill_rect,
                    radiusX: 23.0,
                    radiusY: 23.0,
                };

                let bg_col = D2D1_COLOR_F {
                    r: 0.07,
                    g: 0.08,
                    b: 0.12,
                    a: 0.95,
                };
                if let Some(bg_brush) = self.solid_brush(rt, &bg_col) {
                    rt.FillRoundedRectangle(&rrect, &bg_brush);
                }

                let border_col = if is_overtime {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 0.25,
                        b: 0.25,
                        a: 0.9,
                    }
                } else if paused {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 0.7,
                        b: 0.2,
                        a: 0.85,
                    }
                } else {
                    D2D1_COLOR_F {
                        r: 0.0,
                        g: 0.47,
                        b: 0.83,
                        a: 0.9,
                    }
                };
                if let Some(b_brush) = self.solid_brush(rt, &border_col) {
                    rt.DrawRoundedRectangle(&rrect, &b_brush, 1.5, None);
                }

                // b0: Cycle Corner button (left + 6.0 .. left + 34.0)
                let b0_rect = D2D_RECT_F {
                    left: left + 6.0,
                    top: top + 8.0,
                    right: left + 34.0,
                    bottom: bottom - 8.0,
                };
                let b0_rrect = D2D1_ROUNDED_RECT {
                    rect: b0_rect,
                    radiusX: 6.0,
                    radiusY: 6.0,
                };
                let is_b0_hover = widget.hover_action == Some(TimerAction::CycleCorner);
                let b0_bg = if is_b0_hover {
                    D2D1_COLOR_F {
                        r: 0.0,
                        g: 0.47,
                        b: 0.83,
                        a: 0.8,
                    }
                } else {
                    D2D1_COLOR_F {
                        r: 0.2,
                        g: 0.22,
                        b: 0.28,
                        a: 0.5,
                    }
                };
                if let Some(br) = self.solid_brush(rt, &b0_bg) {
                    rt.FillRoundedRectangle(&b0_rrect, &br);
                }
                let u0: Vec<u16> = "🔄".encode_utf16().collect();
                if let Some(wbrush) = self.solid_brush(
                    rt,
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.9,
                    },
                ) {
                    // Dedicated pre-centered instance - see its field
                    // comment for why this must not clone-and-mutate the
                    // shared text_format_hud.
                    let centered = self.text_format_hud_centered.clone();
                    let tr0 = D2D_RECT_F {
                        left: b0_rect.left,
                        top: b0_rect.top + 3.0,
                        right: b0_rect.right,
                        bottom: b0_rect.bottom,
                    };
                    rt.DrawText(
                        &u0,
                        &centered,
                        &tr0,
                        &wbrush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }

                // Time text
                let time_str = if is_overtime {
                    format!("⏱️ -{:02}:{:02}", mins, secs)
                } else {
                    format!("⏱️ {:02}:{:02}", mins, secs)
                };
                let utf16: Vec<u16> = time_str.encode_utf16().collect();
                let text_rect = D2D_RECT_F {
                    left: left + 38.0,
                    top: top + 12.0,
                    right: right - 110.0,
                    bottom: bottom - 8.0,
                };
                let text_col = if is_overtime {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 0.35,
                        b: 0.35,
                        a: 1.0,
                    }
                } else {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    }
                };
                if let Some(tbrush) = self.solid_brush(rt, &text_col) {
                    rt.DrawText(
                        &utf16,
                        &self.text_format_hud,
                        &text_rect,
                        &tbrush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }

                // Mini action buttons: [Play/Pause], [Expand], [Close]
                let btn_bg = D2D1_COLOR_F {
                    r: 0.2,
                    g: 0.22,
                    b: 0.28,
                    a: 0.6,
                };
                let btn_brush = self.solid_brush(rt, &btn_bg);

                // 1. Play/Pause
                let b1_rect = D2D_RECT_F {
                    left: right - 105.0,
                    top: top + 8.0,
                    right: right - 72.0,
                    bottom: bottom - 8.0,
                };
                let b1_rrect = D2D1_ROUNDED_RECT {
                    rect: b1_rect,
                    radiusX: 6.0,
                    radiusY: 6.0,
                };
                if let Some(ref br) = btn_brush {
                    rt.FillRoundedRectangle(&b1_rrect, br);
                }
                let icon1 = if paused { "▶" } else { "⏸" };
                let u1: Vec<u16> = icon1.encode_utf16().collect();
                if let Some(wbrush) = self.solid_brush(
                    rt,
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.9,
                    },
                ) {
                    // Dedicated pre-centered instance - see its field
                    // comment for why this must not clone-and-mutate the
                    // shared text_format_hud.
                    let centered = self.text_format_hud_centered.clone();
                    let tr1 = D2D_RECT_F {
                        left: b1_rect.left,
                        top: b1_rect.top + 4.0,
                        right: b1_rect.right,
                        bottom: b1_rect.bottom,
                    };
                    rt.DrawText(
                        &u1,
                        &centered,
                        &tr1,
                        &wbrush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }

                // 2. Expand
                let b2_rect = D2D_RECT_F {
                    left: right - 68.0,
                    top: top + 8.0,
                    right: right - 38.0,
                    bottom: bottom - 8.0,
                };
                let b2_rrect = D2D1_ROUNDED_RECT {
                    rect: b2_rect,
                    radiusX: 6.0,
                    radiusY: 6.0,
                };
                if let Some(ref br) = btn_brush {
                    rt.FillRoundedRectangle(&b2_rrect, br);
                }
                let u2: Vec<u16> = "🗖".encode_utf16().collect();
                if let Some(wbrush) = self.solid_brush(
                    rt,
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.9,
                    },
                ) {
                    // Dedicated pre-centered instance - see its field
                    // comment for why this must not clone-and-mutate the
                    // shared text_format_hud.
                    let centered = self.text_format_hud_centered.clone();
                    let tr2 = D2D_RECT_F {
                        left: b2_rect.left,
                        top: b2_rect.top + 4.0,
                        right: b2_rect.right,
                        bottom: b2_rect.bottom,
                    };
                    rt.DrawText(
                        &u2,
                        &centered,
                        &tr2,
                        &wbrush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }

                // 3. Close
                let b3_rect = D2D_RECT_F {
                    left: right - 35.0,
                    top: top + 8.0,
                    right: right - 5.0,
                    bottom: bottom - 8.0,
                };
                let b3_rrect = D2D1_ROUNDED_RECT {
                    rect: b3_rect,
                    radiusX: 6.0,
                    radiusY: 6.0,
                };
                if let Some(ref br) = btn_brush {
                    rt.FillRoundedRectangle(&b3_rrect, br);
                }
                let u3: Vec<u16> = "✕".encode_utf16().collect();
                if let Some(wbrush) = self.solid_brush(
                    rt,
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.9,
                    },
                ) {
                    // Dedicated pre-centered instance - see its field
                    // comment for why this must not clone-and-mutate the
                    // shared text_format_hud.
                    let centered = self.text_format_hud_centered.clone();
                    let tr3 = D2D_RECT_F {
                        left: b3_rect.left,
                        top: b3_rect.top + 4.0,
                        right: b3_rect.right,
                        bottom: b3_rect.bottom,
                    };
                    rt.DrawText(
                        &u3,
                        &centered,
                        &tr3,
                        &wbrush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
                return;
            }

            // Full Center Card Mode (Draggable)
            let (card_left, card_top, card_right, card_bottom, cx, cy) =
                widget.get_card_bounds(screen_w, screen_h);
            let card_rect = D2D_RECT_F {
                left: card_left,
                top: card_top,
                right: card_right,
                bottom: card_bottom,
            };

            let is_warning = mins == 0 && secs <= 30 && !paused && !is_overtime;

            let bg_col = D2D1_COLOR_F {
                r: 0.06,
                g: 0.07,
                b: 0.1,
                a: 0.95,
            };
            let border_col = if is_overtime {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.22,
                    b: 0.22,
                    a: 0.95,
                }
            } else if is_warning {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.45,
                    b: 0.15,
                    a: 0.9,
                }
            } else if paused {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.65,
                    b: 0.15,
                    a: 0.85,
                }
            } else {
                D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.47,
                    b: 0.83,
                    a: 0.85,
                }
            };

            // Elevation drop shadow
            let shadow_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: card_rect.left - 4.0,
                    top: card_rect.top + 4.0,
                    right: card_rect.right + 4.0,
                    bottom: card_rect.bottom + 12.0,
                },
                radiusX: 24.0,
                radiusY: 24.0,
            };
            if let Some(s_brush) = self.solid_brush(
                rt,
                &D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.45,
                },
            ) {
                rt.FillRoundedRectangle(&shadow_rect, &s_brush);
            }

            let rrect = D2D1_ROUNDED_RECT {
                rect: card_rect,
                radiusX: 20.0,
                radiusY: 20.0,
            };

            if let Some(bg_brush) = self.solid_brush(rt, &bg_col) {
                rt.FillRoundedRectangle(&rrect, &bg_brush);
            }
            if let Some(b_brush) = self.solid_brush(rt, &border_col) {
                let stroke_sz = if is_overtime { 2.5 } else { 1.5 };
                rt.DrawRoundedRectangle(&rrect, &b_brush, stroke_sz, None);
            }

            // 1. Session Title Header with Top-Right Close Button
            let title_utf16: Vec<u16> = widget.session_title.encode_utf16().collect();
            let title_rect = D2D_RECT_F {
                left: cx - 200.0,
                top: cy - 205.0,
                right: cx + 200.0,
                bottom: cy - 180.0,
            };
            // get_text_format() is already centered.
            if let Ok(t_fmt) = self.get_text_format(13.0) {
                if let Some(t_brush) = self.solid_brush(
                    rt,
                    &D2D1_COLOR_F {
                        r: 0.65,
                        g: 0.72,
                        b: 0.85,
                        a: 0.85,
                    },
                ) {
                    rt.DrawText(
                        &title_utf16,
                        &t_fmt,
                        &title_rect,
                        &t_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }

            // Top-Right Close Button (✕)
            let close_cx = card_right - 32.0;
            let close_cy = card_top + 28.0;
            let is_close_hover = widget.hover_action == Some(TimerAction::Close);
            let close_el = D2D1_ELLIPSE {
                point: v2(close_cx, close_cy),
                radiusX: 16.0,
                radiusY: 16.0,
            };
            let close_bg = if is_close_hover {
                D2D1_COLOR_F {
                    r: 0.9,
                    g: 0.2,
                    b: 0.25,
                    a: 0.80,
                }
            } else {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.08,
                }
            };
            if let Some(cbrush) = self.solid_brush(rt, &close_bg) {
                rt.FillEllipse(&close_el, &cbrush);
            }
            // Crisp vector cross lines
            if let Some(wbrush) = self.solid_brush(
                rt,
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.90,
                },
            ) {
                rt.DrawLine(
                    v2(close_cx - 5.0, close_cy - 5.0),
                    v2(close_cx + 5.0, close_cy + 5.0),
                    &wbrush,
                    1.8,
                    None,
                );
                rt.DrawLine(
                    v2(close_cx - 5.0, close_cy + 5.0),
                    v2(close_cx + 5.0, close_cy - 5.0),
                    &wbrush,
                    1.8,
                    None,
                );
            }

            // 2. Quick Duration Segmented Pills: [5m] [10m] [15m] [25m] [30m]
            let pill_w = 60.0;
            let pill_h = 30.0;
            let pill_gap = 10.0;
            let total_pills_w = 5.0 * pill_w + 4.0 * pill_gap;
            let pill_row_x = cx - total_pills_w / 2.0;
            let pill_row_y = cy - 162.0;

            let durations = [
                (5, "5m"),
                (10, "10m"),
                (15, "15m"),
                (25, "25m"),
                (30, "30m"),
            ];
            // get_text_format() is already centered.
            let pill_fmt = self.get_text_format(13.0).ok();

            for (i, &(dur, label)) in durations.iter().enumerate() {
                let px = pill_row_x + i as f32 * (pill_w + pill_gap);
                let p_rect = D2D_RECT_F {
                    left: px,
                    top: pill_row_y,
                    right: px + pill_w,
                    bottom: pill_row_y + pill_h,
                };
                let p_rrect = D2D1_ROUNDED_RECT {
                    rect: p_rect,
                    radiusX: 15.0,
                    radiusY: 15.0,
                };

                let is_hover = widget.hover_action == Some(TimerAction::SetDuration(dur));
                let is_current = mins == dur && !is_overtime;

                let p_bg = if is_current {
                    D2D1_COLOR_F {
                        r: 0.0,
                        g: 0.47,
                        b: 0.83,
                        a: 0.55,
                    }
                } else if is_hover {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.12,
                    }
                } else {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.05,
                    }
                };

                if let Some(pbrush) = self.solid_brush(rt, &p_bg) {
                    rt.FillRoundedRectangle(&p_rrect, &pbrush);
                }

                let p_border = if is_current {
                    D2D1_COLOR_F {
                        r: 0.38,
                        g: 0.80,
                        b: 1.0,
                        a: 0.95,
                    }
                } else {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.12,
                    }
                };
                if let Some(pb_brush) = self.solid_brush(rt, &p_border) {
                    rt.DrawRoundedRectangle(&p_rrect, &pb_brush, 1.0, None);
                }

                let p_utf16: Vec<u16> = label.encode_utf16().collect();
                if let Some(ref pf) = pill_fmt
                    && let Some(wbrush) = self.solid_brush(
                        rt,
                        &D2D1_COLOR_F {
                            r: 0.92,
                            g: 0.94,
                            b: 0.98,
                            a: 0.95,
                        },
                    )
                {
                    rt.DrawText(
                        &p_utf16,
                        pf,
                        &p_rect,
                        &wbrush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }

            // 3. Central Glowing Progress Ring
            let ring_radius = 100.0;
            let ring_center = v2(cx, cy - 10.0);

            // Ring track background
            let track_col = D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.08,
            };
            if let Some(track_brush) = self.solid_brush(rt, &track_col) {
                let el = D2D1_ELLIPSE {
                    point: ring_center,
                    radiusX: ring_radius,
                    radiusY: ring_radius,
                };
                rt.DrawEllipse(&el, &track_brush, 7.0, None);
            }

            // Smooth Progress Arc (128 smooth segments)
            if let Some(arc_brush) = self.solid_brush(rt, &border_col) {
                let segments = (128.0 * progress.clamp(0.0, 1.0)) as usize;
                if segments > 1 {
                    // Keyed on segments+center (radius is fixed at 100.0): the
                    // card can force a repaint for unrelated reasons (caret
                    // blink, a fading toast) far more often than this arc's
                    // own discrete segment count actually advances.
                    let key = {
                        use std::hash::{Hash, Hasher};
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        0xA4C5_u32.hash(&mut hasher);
                        segments.hash(&mut hasher);
                        ring_center.X.to_bits().hash(&mut hasher);
                        ring_center.Y.to_bits().hash(&mut hasher);
                        hasher.finish()
                    };
                    let mut cache = self.geometry_cache.borrow_mut();
                    let rt_id = rt.as_raw() as usize;
                    if cache.0 != rt_id {
                        cache.0 = rt_id;
                        cache.1.clear();
                    }
                    let path = if let Some(p) = cache.1.get(&key) {
                        Some(p.clone())
                    } else {
                        let built = self.factory.CreatePathGeometry().ok().and_then(|path| {
                            let sink = path.Open().ok()?;
                            let start_angle = -std::f32::consts::FRAC_PI_2;
                            let p0_x = ring_center.X + ring_radius * start_angle.cos();
                            let p0_y = ring_center.Y + ring_radius * start_angle.sin();

                            sink.BeginFigure(v2(p0_x, p0_y), D2D1_FIGURE_BEGIN_HOLLOW);
                            for i in 1..=segments {
                                let angle =
                                    start_angle + (i as f32 / 128.0) * std::f32::consts::PI * 2.0;
                                let px = ring_center.X + ring_radius * angle.cos();
                                let py = ring_center.Y + ring_radius * angle.sin();
                                sink.AddLine(v2(px, py));
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
                        rt.DrawGeometry(&path, &arc_brush, 7.0, Some(&self.round_stroke_style));
                    }

                    // Leading glowing bead indicator
                    let head_angle = -std::f32::consts::FRAC_PI_2
                        + (segments as f32 / 128.0) * std::f32::consts::PI * 2.0;
                    let bead_x = ring_center.X + ring_radius * head_angle.cos();
                    let bead_y = ring_center.Y + ring_radius * head_angle.sin();
                    let bead_el = D2D1_ELLIPSE {
                        point: v2(bead_x, bead_y),
                        radiusX: 6.0,
                        radiusY: 6.0,
                    };
                    if let Some(bead_brush) = self.solid_brush(
                        rt,
                        &D2D1_COLOR_F {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 0.95,
                        },
                    ) {
                        rt.FillEllipse(&bead_el, &bead_brush);
                    }
                }
            }

            // Overtime pulsing halo
            if is_overtime {
                let pulse_col = D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.1,
                    b: 0.1,
                    a: 0.25,
                };
                if let Some(pulse_brush) = self.solid_brush(rt, &pulse_col) {
                    let el = D2D1_ELLIPSE {
                        point: ring_center,
                        radiusX: ring_radius + 9.0,
                        radiusY: ring_radius + 9.0,
                    };
                    rt.DrawEllipse(&el, &pulse_brush, 4.0, None);
                }
            }

            // Time text inside ring (well-proportioned 52px font size)
            let time_str = if is_overtime {
                format!("-{:02}:{:02}", mins, secs)
            } else {
                format!("{:02}:{:02}", mins, secs)
            };
            let time_utf16: Vec<u16> = time_str.encode_utf16().collect();
            let time_rect = D2D_RECT_F {
                left: cx - 150.0,
                top: cy - 46.0,
                right: cx + 150.0,
                bottom: cy + 18.0,
            };

            let text_col = if is_overtime {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.25,
                    b: 0.25,
                    a: 1.0,
                }
            } else {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                }
            };
            // get_text_format() is already centered.
            if let Ok(t_fmt) = self.get_text_format(52.0) {
                if let Some(tbrush) = self.solid_brush(rt, &text_col) {
                    rt.DrawText(
                        &time_utf16,
                        &t_fmt,
                        &time_rect,
                        &tbrush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }

            // Subtitle label below digits
            let sub_label = if is_overtime {
                "OVERTIME"
            } else if paused {
                "PAUSED"
            } else {
                "TIME REMAINING"
            };
            let sub_label_utf16: Vec<u16> = sub_label.encode_utf16().collect();
            let sub_rect = D2D_RECT_F {
                left: cx - 120.0,
                top: cy + 24.0,
                right: cx + 120.0,
                bottom: cy + 42.0,
            };
            // get_text_format() is already centered.
            if let Ok(s_fmt) = self.get_text_format(11.0) {
                if let Some(st_brush) = self.solid_brush(
                    rt,
                    &D2D1_COLOR_F {
                        r: 0.65,
                        g: 0.72,
                        b: 0.85,
                        a: 0.75,
                    },
                ) {
                    rt.DrawText(
                        &sub_label_utf16,
                        &s_fmt,
                        &sub_rect,
                        &st_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }

            // 4. Modern Action Controls: [-1m] [⟲] [ Hero Play/Pause ] [+1m] [🗗]
            let btn_y = cy + 130.0;

            // 4.1 Hero Play/Pause Button in the center (radius 28.0 = 56px diameter)
            let is_hero_hover = widget.hover_action == Some(TimerAction::PlayPause);
            let hero_bg = if is_hero_hover {
                D2D1_COLOR_F {
                    r: 0.12,
                    g: 0.58,
                    b: 0.95,
                    a: 0.98,
                }
            } else {
                D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.47,
                    b: 0.83,
                    a: 0.92,
                }
            };
            let hero_el = D2D1_ELLIPSE {
                point: v2(cx, btn_y),
                radiusX: 28.0,
                radiusY: 28.0,
            };
            if let Some(hbrush) = self.solid_brush(rt, &hero_bg) {
                rt.FillEllipse(&hero_el, &hbrush);
            }
            if let Some(hb_brush) = self.solid_brush(
                rt,
                &D2D1_COLOR_F {
                    r: 0.45,
                    g: 0.82,
                    b: 1.0,
                    a: 0.95,
                },
            ) {
                rt.DrawEllipse(&hero_el, &hb_brush, 1.5, None);
            }

            // Vector Hero Icons (Pause = two vertical bars, Play = triangle)
            if let Some(white_brush) = self.solid_brush(
                rt,
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
            ) {
                if paused {
                    // Vector Play Triangle - keyed on position (the shape
                    // itself is fixed), same reasoning as the progress arc
                    // above.
                    let key = {
                        use std::hash::{Hash, Hasher};
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        0x9A17_u32.hash(&mut hasher);
                        cx.to_bits().hash(&mut hasher);
                        btn_y.to_bits().hash(&mut hasher);
                        hasher.finish()
                    };
                    let mut cache = self.geometry_cache.borrow_mut();
                    let rt_id = rt.as_raw() as usize;
                    if cache.0 != rt_id {
                        cache.0 = rt_id;
                        cache.1.clear();
                    }
                    let path = if let Some(p) = cache.1.get(&key) {
                        Some(p.clone())
                    } else {
                        let built = self.factory.CreatePathGeometry().ok().and_then(|path| {
                            let sink = path.Open().ok()?;
                            sink.BeginFigure(v2(cx - 6.0, btn_y - 9.0), D2D1_FIGURE_BEGIN_FILLED);
                            sink.AddLine(v2(cx - 6.0, btn_y + 9.0));
                            sink.AddLine(v2(cx + 9.0, btn_y));
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
                        rt.FillGeometry(&path, &white_brush, None);
                    }
                } else {
                    // Vector Pause Double Bars (no emoji box/tofu glyph!)
                    let bar1 = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: cx - 7.0,
                            top: btn_y - 9.0,
                            right: cx - 2.0,
                            bottom: btn_y + 9.0,
                        },
                        radiusX: 1.5,
                        radiusY: 1.5,
                    };
                    let bar2 = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: cx + 2.0,
                            top: btn_y - 9.0,
                            right: cx + 7.0,
                            bottom: btn_y + 9.0,
                        },
                        radiusX: 1.5,
                        radiusY: 1.5,
                    };
                    rt.FillRoundedRectangle(&bar1, &white_brush);
                    rt.FillRoundedRectangle(&bar2, &white_brush);
                }
            }

            // 4.2 Flanking Secondary Action Controls (radius 22.0 = 44px diameter)
            let secondary_buttons = [
                (cx - 120.0, "-1m", TimerAction::SubMinute),
                (cx - 60.0, "↺", TimerAction::Reset),
                (cx + 60.0, "+1m", TimerAction::AddMinute),
                (cx + 120.0, "mini", TimerAction::ToggleMinimize),
            ];

            // get_text_format() is already centered.
            let sec_fmt = self.get_text_format(13.0).ok();
            let reset_fmt = self.get_text_format(18.0).ok();

            for &(scx, label, action) in &secondary_buttons {
                let is_hover = widget.hover_action == Some(action);
                let sec_el = D2D1_ELLIPSE {
                    point: v2(scx, btn_y),
                    radiusX: 22.0,
                    radiusY: 22.0,
                };
                let sec_bg = if is_hover {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.16,
                    }
                } else {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.07,
                    }
                };
                if let Some(sbrush) = self.solid_brush(rt, &sec_bg) {
                    rt.FillEllipse(&sec_el, &sbrush);
                }
                let sec_border = if is_hover {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.35,
                    }
                } else {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.12,
                    }
                };
                if let Some(sb_brush) = self.solid_brush(rt, &sec_border) {
                    rt.DrawEllipse(&sec_el, &sb_brush, 1.0, None);
                }

                if action == TimerAction::ToggleMinimize {
                    // Vector PIP / Mini window icon (two crisp overlapping rectangles)
                    if let Some(wbrush) = self.solid_brush(
                        rt,
                        &D2D1_COLOR_F {
                            r: 0.95,
                            g: 0.95,
                            b: 0.98,
                            a: 0.95,
                        },
                    ) {
                        let outer_win = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: scx - 8.0,
                                top: btn_y - 7.0,
                                right: scx + 8.0,
                                bottom: btn_y + 7.0,
                            },
                            radiusX: 2.0,
                            radiusY: 2.0,
                        };
                        rt.DrawRoundedRectangle(&outer_win, &wbrush, 1.4, None);
                        let inner_pip = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: scx + 1.0,
                                top: btn_y,
                                right: scx + 7.0,
                                bottom: btn_y + 6.0,
                            },
                            radiusX: 1.0,
                            radiusY: 1.0,
                        };
                        rt.FillRoundedRectangle(&inner_pip, &wbrush);
                    }
                } else if action == TimerAction::Reset {
                    let l_utf16: Vec<u16> = label.encode_utf16().collect();
                    let txt_rect = D2D_RECT_F {
                        left: scx - 22.0,
                        top: btn_y - 22.0,
                        right: scx + 22.0,
                        bottom: btn_y + 22.0,
                    };
                    if let Some(ref rf) = reset_fmt
                        && let Some(wbrush) = self.solid_brush(
                            rt,
                            &D2D1_COLOR_F {
                                r: 0.95,
                                g: 0.95,
                                b: 0.98,
                                a: 0.95,
                            },
                        )
                    {
                        rt.DrawText(
                            &l_utf16,
                            rf,
                            &txt_rect,
                            &wbrush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                } else {
                    let l_utf16: Vec<u16> = label.encode_utf16().collect();
                    let txt_rect = D2D_RECT_F {
                        left: scx - 22.0,
                        top: btn_y - 22.0,
                        right: scx + 22.0,
                        bottom: btn_y + 22.0,
                    };
                    if let Some(ref sf) = sec_fmt
                        && let Some(wbrush) = self.solid_brush(
                            rt,
                            &D2D1_COLOR_F {
                                r: 0.95,
                                g: 0.95,
                                b: 0.98,
                                a: 0.95,
                            },
                        )
                    {
                        rt.DrawText(
                            &l_utf16,
                            sf,
                            &txt_rect,
                            &wbrush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                }
            }

            // 5. Sub-hint: Shortcuts
            let sub_text = if is_overtime {
                "⏰ OVERTIME! • Space: Reset | Wheel: ±1m | Drag to Move | Esc: Exit"
            } else {
                "Space: Pause | Wheel: ±1m | Drag Card to Move | Tab: Mini"
            };
            let sub_utf16: Vec<u16> = sub_text.encode_utf16().collect();
            let sub_rect = D2D_RECT_F {
                left: cx - 240.0,
                top: cy + 185.0,
                right: cx + 240.0,
                bottom: cy + 208.0,
            };
            let sub_col = if is_overtime {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.6,
                    b: 0.6,
                    a: 0.95,
                }
            } else {
                D2D1_COLOR_F {
                    r: 0.65,
                    g: 0.70,
                    b: 0.80,
                    a: 0.75,
                }
            };
            // get_text_format() is already centered.
            if let Ok(s_fmt) = self.get_text_format(12.0) {
                if let Some(sbrush) = self.solid_brush(rt, &sub_col) {
                    rt.DrawText(
                        &sub_utf16,
                        &s_fmt,
                        &sub_rect,
                        &sbrush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }
        }
    }
}
