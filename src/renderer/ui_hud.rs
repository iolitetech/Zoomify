use super::{D2DRenderer, v2};
use crate::types::{
    AppMode, CanvasBackground, ColorPreset, DrawTool, FluentToolbarState, SpotlightState,
    ToastNotification,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_COLOR_F, D2D1_FILL_MODE_ALTERNATE,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_ROUNDED_RECT, ID2D1Geometry, ID2D1RenderTarget,
};
use windows_numerics::Matrix3x2;

impl D2DRenderer {
    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn render_spotlight_mask(
        &self,
        rt: &ID2D1RenderTarget,
        screen_w: f32,
        screen_h: f32,
        cx: f32,
        cy: f32,
        r: f32,
        dim_opacity: f32,
        pinned: bool,
    ) {
        unsafe {
            let radius = r.max(20.0);
            let radius_key = radius.round() as u32;

            let mut cache = self.spotlight_geometry_cache.borrow_mut();
            let need_new_geom = match cache.as_ref() {
                Some((cached_r, _)) => *cached_r != radius_key,
                None => true,
            };

            if need_new_geom {
                let extent_w = (screen_w * 2.5).max(4000.0);
                let extent_h = (screen_h * 2.5).max(3000.0);
                let rect = D2D_RECT_F {
                    left: -extent_w,
                    top: -extent_h,
                    right: extent_w,
                    bottom: extent_h,
                };
                let ellipse = D2D1_ELLIPSE {
                    point: v2(0.0, 0.0),
                    radiusX: radius,
                    radiusY: radius,
                };

                if let Ok(rect_geom) = self.factory.CreateRectangleGeometry(&rect)
                    && let Ok(ellipse_geom) = self.factory.CreateEllipseGeometry(&ellipse)
                {
                    let geometries: [Option<ID2D1Geometry>; 2] =
                        [Some(rect_geom.into()), Some(ellipse_geom.into())];
                    if let Ok(group) = self
                        .factory
                        .CreateGeometryGroup(D2D1_FILL_MODE_ALTERNATE, &geometries)
                    {
                        *cache = Some((radius_key, group));
                    }
                }
            }

            if let Some((_, group)) = cache.as_ref() {
                // Hardware GPU translation directly to (cx, cy)
                let trans = Matrix3x2 {
                    M11: 1.0,
                    M12: 0.0,
                    M21: 0.0,
                    M22: 1.0,
                    M31: cx,
                    M32: cy,
                };
                rt.SetTransform(&trans);

                let dim_brush_color = D2D1_COLOR_F {
                    r: 0.01,
                    g: 0.02,
                    b: 0.03,
                    a: dim_opacity,
                };
                if let Ok(dim_brush) = rt.CreateSolidColorBrush(&dim_brush_color, None) {
                    rt.FillGeometry(group, &dim_brush, None);
                }

                let ring_col = if pinned {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 0.35,
                        b: 0.2,
                        a: 0.9,
                    }
                } else {
                    D2D1_COLOR_F {
                        r: 0.25,
                        g: 0.75,
                        b: 1.0,
                        a: 0.85,
                    }
                };

                if let Ok(ring_brush) = rt.CreateSolidColorBrush(&ring_col, None) {
                    let ellipse = D2D1_ELLIPSE {
                        point: v2(0.0, 0.0),
                        radiusX: radius,
                        radiusY: radius,
                    };
                    rt.DrawEllipse(&ellipse, &ring_brush, 2.5, None);
                }

                // Restore transform to identity
                let identity = Matrix3x2 {
                    M11: 1.0,
                    M12: 0.0,
                    M21: 0.0,
                    M22: 1.0,
                    M31: 0.0,
                    M32: 0.0,
                };
                rt.SetTransform(&identity);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn render_hud(
        &self,
        rt: &ID2D1RenderTarget,
        screen_w: f32,
        screen_h: f32,
        mode: AppMode,
        tool: DrawTool,
        color: ColorPreset,
        stroke_width: f32,
        zoom_level: f32,
        spotlight: &SpotlightState,
        bg_type: CanvasBackground,
    ) {
        unsafe {
            let hud_w = 480.0;
            let hud_h = 36.0;
            let hud_x = (screen_w - hud_w) / 2.0;
            let hud_y = screen_h - hud_h - 18.0;

            let bg_col = D2D1_COLOR_F {
                r: 0.08,
                g: 0.09,
                b: 0.13,
                a: 0.92,
            };
            let border_col = D2D1_COLOR_F {
                r: 0.22,
                g: 0.25,
                b: 0.35,
                a: 0.8,
            };

            let hud_rect = D2D_RECT_F {
                left: hud_x,
                top: hud_y,
                right: hud_x + hud_w,
                bottom: hud_y + hud_h,
            };

            if let Ok(bg_brush) = rt.CreateSolidColorBrush(&bg_col, None) {
                let rrect = D2D1_ROUNDED_RECT {
                    rect: hud_rect,
                    radiusX: 18.0,
                    radiusY: 18.0,
                };
                rt.FillRoundedRectangle(&rrect, &bg_brush);
                if let Ok(b_brush) = rt.CreateSolidColorBrush(&border_col, None) {
                    rt.DrawRoundedRectangle(&rrect, &b_brush, 1.0, None);
                }
            }

            let dot_center = v2(hud_x + 22.0, hud_y + hud_h / 2.0);
            let dot_color = color.to_d2d_color(1.0);
            if let Ok(dot_brush) = rt.CreateSolidColorBrush(&dot_color, None) {
                let el = D2D1_ELLIPSE {
                    point: dot_center,
                    radiusX: 6.0,
                    radiusY: 6.0,
                };
                rt.FillEllipse(&el, &dot_brush);
            }

            let mode_name = match mode {
                AppMode::Idle => "Idle",
                AppMode::LiveZoom => "🔍 Live Zoom",
                AppMode::StaticZoom => "🔎 Static Zoom",
                AppMode::Draw => "✏️ Draw Mode",
                AppMode::Spotlight => "🔦 Spotlight",
                AppMode::Timer => "⏱️ Timer",
                AppMode::Snip => "✂️ Snip",
            };

            let spot_info = if spotlight.active {
                format!(" | 🔦 ⌀{}px", (spotlight.radius * 2.0).round() as u32)
            } else {
                "".to_string()
            };

            let zoom_info = if mode == AppMode::StaticZoom || mode == AppMode::LiveZoom {
                format!(" | {:.1}x", zoom_level)
            } else {
                "".to_string()
            };

            let bg_info = match bg_type {
                CanvasBackground::Transparent => "",
                CanvasBackground::Whiteboard => " | ⚪ Whiteboard",
                CanvasBackground::Blackboard => " | ⚫ Blackboard",
            };

            let text = format!(
                "{} • {} ({}px){}{}{} • F1: Help",
                mode_name,
                tool.name(),
                stroke_width.round() as u32,
                zoom_info,
                spot_info,
                bg_info
            );

            let utf16: Vec<u16> = text.encode_utf16().collect();
            let text_col = D2D1_COLOR_F {
                r: 0.9,
                g: 0.92,
                b: 0.96,
                a: 0.95,
            };
            if let Ok(tbrush) = rt.CreateSolidColorBrush(&text_col, None) {
                let text_rect = D2D_RECT_F {
                    left: hud_x + 36.0,
                    top: hud_y + 8.0,
                    right: hud_x + hud_w - 12.0,
                    bottom: hud_y + hud_h - 4.0,
                };
                rt.DrawText(
                    &utf16,
                    &self.text_format_hud,
                    &text_rect,
                    &tbrush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    pub(super) unsafe fn render_toast(
        &self,
        rt: &ID2D1RenderTarget,
        screen_w: f32,
        _screen_h: f32,
        toast: &ToastNotification,
        toolbar: &FluentToolbarState,
    ) {
        unsafe {
            let opacity = toast.opacity();
            if opacity <= 0.01 {
                return;
            }

            let toast_w = (toast.message.len() as f32 * 8.0 + 72.0).clamp(200.0, 480.0);
            let toast_h = 40.0;
            let toast_x = screen_w - toast_w - 24.0;
            let toast_y = if toolbar.visible && toolbar.bar_rect.bottom > toolbar.bar_rect.top {
                toolbar.bar_rect.top
                    + (toolbar.bar_rect.bottom - toolbar.bar_rect.top - toast_h) / 2.0
            } else {
                16.0
            };

            // 1. Soft elevation drop shadow
            let shadow_rrect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: toast_x,
                    top: toast_y + 4.0,
                    right: toast_x + toast_w,
                    bottom: toast_y + toast_h + 5.0,
                },
                radiusX: 14.0,
                radiusY: 14.0,
            };
            if let Ok(shadow_brush) = rt.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.40 * opacity,
                },
                None,
            ) {
                rt.FillRoundedRectangle(&shadow_rrect, &shadow_brush);
            }

            // 2. Acrylic dark card body
            let main_rrect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: toast_x,
                    top: toast_y,
                    right: toast_x + toast_w,
                    bottom: toast_y + toast_h,
                },
                radiusX: 12.0,
                radiusY: 12.0,
            };
            let bg_col = D2D1_COLOR_F {
                r: 0.10,
                g: 0.11,
                b: 0.15,
                a: 0.95 * opacity,
            };
            let border_col = D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.16 * opacity,
            };

            if let Ok(bg_brush) = rt.CreateSolidColorBrush(&bg_col, None) {
                rt.FillRoundedRectangle(&main_rrect, &bg_brush);
            }
            if let Ok(border_brush) = rt.CreateSolidColorBrush(&border_col, None) {
                rt.DrawRoundedRectangle(&main_rrect, &border_brush, 1.0, None);
            }

            // 3. Left circular icon badge
            let badge_center = v2(toast_x + 22.0, toast_y + 22.0);
            let badge_bg_col = D2D1_COLOR_F {
                r: 0.0,
                g: 0.47,
                b: 0.83,
                a: 0.35 * opacity,
            };
            if let Ok(badge_brush) = rt.CreateSolidColorBrush(&badge_bg_col, None) {
                let badge_el = D2D1_ELLIPSE {
                    point: badge_center,
                    radiusX: 13.0,
                    radiusY: 13.0,
                };
                rt.FillEllipse(&badge_el, &badge_brush);
            }

            // Draw icon inside badge
            let icon_utf16: Vec<u16> = toast.icon.encode_utf16().collect();
            let icon_rect = D2D_RECT_F {
                left: toast_x + 9.0,
                top: toast_y + 9.0,
                right: toast_x + 35.0,
                bottom: toast_y + 35.0,
            };
            if let Ok(white_brush) = rt.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: opacity,
                },
                None,
            ) {
                rt.DrawText(
                    &icon_utf16,
                    &self.text_format_toolbar_small,
                    &icon_rect,
                    &white_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            // 4. Message typography (Title & Subtitle)
            let (title, hint) = if let Some(idx) = toast.message.find(" (") {
                let (t, h) = toast.message.split_at(idx);
                let clean_h = h.trim_start_matches(" (").trim_end_matches(')');
                (t, Some(clean_h))
            } else {
                (toast.message.as_str(), None)
            };

            let text_col = D2D1_COLOR_F {
                r: 0.96,
                g: 0.97,
                b: 0.99,
                a: opacity,
            };
            let sub_col = D2D1_COLOR_F {
                r: 0.74,
                g: 0.78,
                b: 0.85,
                a: 0.90 * opacity,
            };

            if let Some(sub_text) = hint {
                let title_utf16: Vec<u16> = title.encode_utf16().collect();
                let sub_utf16: Vec<u16> = sub_text.encode_utf16().collect();
                let title_rect = D2D_RECT_F {
                    left: toast_x + 44.0,
                    top: toast_y + 4.0,
                    right: toast_x + toast_w - 14.0,
                    bottom: toast_y + 24.0,
                };
                let sub_rect = D2D_RECT_F {
                    left: toast_x + 44.0,
                    top: toast_y + 24.0,
                    right: toast_x + toast_w - 14.0,
                    bottom: toast_y + toast_h - 4.0,
                };
                if let Ok(text_brush) = rt.CreateSolidColorBrush(&text_col, None) {
                    rt.DrawText(
                        &title_utf16,
                        &self.text_format_toast_title,
                        &title_rect,
                        &text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
                if let Ok(sub_brush) = rt.CreateSolidColorBrush(&sub_col, None) {
                    rt.DrawText(
                        &sub_utf16,
                        &self.text_format_toast_sub,
                        &sub_rect,
                        &sub_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            } else {
                let msg_utf16: Vec<u16> = title.encode_utf16().collect();
                let msg_rect = D2D_RECT_F {
                    left: toast_x + 44.0,
                    top: toast_y + 4.0,
                    right: toast_x + toast_w - 14.0,
                    bottom: toast_y + toast_h - 4.0,
                };
                if let Ok(text_brush) = rt.CreateSolidColorBrush(&text_col, None) {
                    rt.DrawText(
                        &msg_utf16,
                        &self.text_format_toast_title,
                        &msg_rect,
                        &text_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }
        }
    }

    pub(super) unsafe fn render_cheat_sheet_modal(
        &self,
        rt: &ID2D1RenderTarget,
        screen_w: f32,
        screen_h: f32,
    ) {
        unsafe {
            let modal_w = 720.0;
            let modal_h = 660.0;
            let mx = (screen_w - modal_w) / 2.0;
            let my = (screen_h - modal_h) / 2.0;

            let bg_col = D2D1_COLOR_F {
                r: 0.08,
                g: 0.09,
                b: 0.13,
                a: 0.97,
            };
            let border_col = D2D1_COLOR_F {
                r: 0.25,
                g: 0.55,
                b: 0.95,
                a: 0.9,
            };

            let modal_rect = D2D_RECT_F {
                left: mx,
                top: my,
                right: mx + modal_w,
                bottom: my + modal_h,
            };

            if let Ok(bg_brush) = rt.CreateSolidColorBrush(&bg_col, None) {
                let rrect = D2D1_ROUNDED_RECT {
                    rect: modal_rect,
                    radiusX: 16.0,
                    radiusY: 16.0,
                };
                rt.FillRoundedRectangle(&rrect, &bg_brush);
                if let Ok(b_brush) = rt.CreateSolidColorBrush(&border_col, None) {
                    rt.DrawRoundedRectangle(&rrect, &b_brush, 2.0, None);
                }
            }

            let title = "🚀 Zoomify Quick Reference Guide";
            let title_utf16: Vec<u16> = title.encode_utf16().collect();
            let title_rect = D2D_RECT_F {
                left: mx + 24.0,
                top: my + 18.0,
                right: mx + modal_w - 24.0,
                bottom: my + 50.0,
            };

            let white_brush = rt
                .CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    },
                    None,
                )
                .ok();
            let accent_brush = rt
                .CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 0.35,
                        g: 0.75,
                        b: 1.0,
                        a: 1.0,
                    },
                    None,
                )
                .ok();
            let desc_brush = rt
                .CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 0.78,
                        g: 0.82,
                        b: 0.9,
                        a: 0.95,
                    },
                    None,
                )
                .ok();

            if let (Some(wbrush), Some(abrush), Some(dbrush)) =
                (white_brush, accent_brush, desc_brush)
            {
                rt.DrawText(
                    &title_utf16,
                    &self.text_format_cheat_title,
                    &title_rect,
                    &wbrush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );

                // Left column entries
                let col1 = [
                    ("GLOBAL SHORTCUTS", ""),
                    ("Ctrl+1", "Static Freeze Zoom (Wheel/Pan/Draw)"),
                    ("Ctrl+2", "Draw / Annotation Mode"),
                    ("Ctrl+3", "Spotlight Mode (Wheel: resize)"),
                    ("Ctrl+4", "Live Zoom (Hardware magnifier)"),
                    ("Ctrl+5", "Presentation Countdown Timer"),
                    ("Ctrl+Shift+S", "Snip Region to Clipboard"),
                    ("", ""),
                    ("DRAW TOOLS & KEYS", ""),
                    ("D / P", "Pen (freehand with Bezier smoothing)"),
                    ("K", "Laser Pointer (glowing fading trail)"),
                    ("X", "Eraser (drag to delete strokes)"),
                    ("H", "Highlighter (translucent)"),
                    ("L", "Straight Line tool"),
                    ("A", "Arrow tool"),
                    ("Shift+R", "Rectangle tool"),
                    ("U", "Rounded Rectangle tool"),
                    ("Q", "Ellipse / Circle tool"),
                    ("T", "Text (click, type, Enter/Esc)"),
                    ("N", "Step Badge (Shift+N: reset #)"),
                    ("S", "Snip tool (drag rectangle)"),
                    ("", ""),
                    ("GESTURES & MODIFIERS", ""),
                    ("Hold Pen", "Hold 350ms to auto-snap shape"),
                    ("⋮⋮ Drag", "Move Fluent Toolbar anywhere"),
                    ("Shift + Drag", "Snap straight line (45°)"),
                    ("Ctrl + Drag", "Snap rectangle"),
                    ("Tab + Drag", "Snap ellipse"),
                ];

                // Right column entries
                let col2 = [
                    ("COLOR PRESETS", ""),
                    ("R", "Red"),
                    ("G", "Green"),
                    ("B", "Blue"),
                    ("Y", "Yellow"),
                    ("O", "Orange"),
                    ("P", "Pink / Purple"),
                    ("C / I", "Cyan"),
                    ("Shift+W / Shift+K", "White Pen / Black Pen"),
                    ("", ""),
                    ("CANVAS MODES", ""),
                    ("W", "Whiteboard slate"),
                    ("Shift+K", "Blackboard slate"),
                    ("", ""),
                    ("ACTIONS & CONTROLS", ""),
                    ("F2", "Toggle Fluent Toolbar & HUD"),
                    ("F3 / Ctrl+3", "Toggle Spotlight on/off"),
                    ("Space", "Toggle Pan/Zoom vs Draw mode"),
                    ("Wheel", "Zoom in / out centered at mouse"),
                    ("Ctrl + Wheel", "Resize Spotlight circle"),
                    ("Shift + Wheel", "Adjust brush stroke width"),
                    ("Ctrl+Z / Ctrl+Y", "Undo / Redo (with badge counter)"),
                    ("Ctrl+C / Ctrl+S", "Copy screen / Save snapshot"),
                    ("E / Delete", "Clear canvas (undoable)"),
                    ("Esc / Right-Click", "Return to Pan mode / Exit overlay"),
                ];

                let mut y1 = my + 60.0;
                for (key, desc) in &col1 {
                    if key.is_empty() {
                        y1 += 6.0;
                        continue;
                    }
                    if desc.is_empty() {
                        let h_utf16: Vec<u16> = key.encode_utf16().collect();
                        let h_rect = D2D_RECT_F {
                            left: mx + 24.0,
                            top: y1,
                            right: mx + 350.0,
                            bottom: y1 + 18.0,
                        };
                        rt.DrawText(
                            &h_utf16,
                            &self.text_format_hud,
                            &h_rect,
                            &abrush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                        );
                        y1 += 20.0;
                    } else {
                        let k_utf16: Vec<u16> = key.encode_utf16().collect();
                        let d_utf16: Vec<u16> = desc.encode_utf16().collect();
                        let k_rect = D2D_RECT_F {
                            left: mx + 28.0,
                            top: y1,
                            right: mx + 130.0,
                            bottom: y1 + 18.0,
                        };
                        let d_rect = D2D_RECT_F {
                            left: mx + 130.0,
                            top: y1,
                            right: mx + 360.0,
                            bottom: y1 + 18.0,
                        };
                        rt.DrawText(
                            &k_utf16,
                            &self.text_format_cheat_item,
                            &k_rect,
                            &wbrush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                        );
                        rt.DrawText(
                            &d_utf16,
                            &self.text_format_cheat_item,
                            &d_rect,
                            &dbrush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                        );
                        y1 += 18.5;
                    }
                }

                let mut y2 = my + 60.0;
                for (key, desc) in &col2 {
                    if key.is_empty() {
                        y2 += 6.0;
                        continue;
                    }
                    if desc.is_empty() {
                        let h_utf16: Vec<u16> = key.encode_utf16().collect();
                        let h_rect = D2D_RECT_F {
                            left: mx + 375.0,
                            top: y2,
                            right: mx + modal_w - 24.0,
                            bottom: y2 + 18.0,
                        };
                        rt.DrawText(
                            &h_utf16,
                            &self.text_format_hud,
                            &h_rect,
                            &abrush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                        );
                        y2 += 20.0;
                    } else {
                        let k_utf16: Vec<u16> = key.encode_utf16().collect();
                        let d_utf16: Vec<u16> = desc.encode_utf16().collect();
                        let k_rect = D2D_RECT_F {
                            left: mx + 380.0,
                            top: y2,
                            right: mx + 495.0,
                            bottom: y2 + 18.0,
                        };
                        let d_rect = D2D_RECT_F {
                            left: mx + 495.0,
                            top: y2,
                            right: mx + modal_w - 24.0,
                            bottom: y2 + 18.0,
                        };
                        rt.DrawText(
                            &k_utf16,
                            &self.text_format_cheat_item,
                            &k_rect,
                            &wbrush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                        );
                        rt.DrawText(
                            &d_utf16,
                            &self.text_format_cheat_item,
                            &d_rect,
                            &dbrush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                        );
                        y2 += 18.5;
                    }
                }
            }
        }
    }
}
