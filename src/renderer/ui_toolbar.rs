use super::{D2DRenderer, v2};
use crate::types::{
    AppMode, ArrowStyle, BadgeShape, BadgeSize, ColorPreset, DrawTool, FillMode, FluentAction,
    FluentToolbarState, SpotlightState, StrokePattern, TextCardStyle, TextFontFamily,
};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_ROUNDED_RECT, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_CENTER;

impl D2DRenderer {
    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn render_fluent_toolbar(
        &self,
        rt: &ID2D1RenderTarget,
        toolbar: &FluentToolbarState,
        mode: AppMode,
        tool: DrawTool,
        color: ColorPreset,
        spotlight: &SpotlightState,
    ) {
        if !toolbar.visible {
            return;
        }

        let bar_rect = &toolbar.bar_rect;
        if bar_rect.right <= bar_rect.left {
            return;
        }

        unsafe {
            // 1. Soft elevation drop shadow
            let shadow_rrect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: bar_rect.left,
                    top: bar_rect.top + 3.0,
                    right: bar_rect.right,
                    bottom: bar_rect.bottom + 4.0,
                },
                radiusX: 14.0,
                radiusY: 14.0,
            };
            if let Ok(shadow_brush) = rt.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.35,
                },
                None,
            ) {
                rt.FillRoundedRectangle(&shadow_rrect, &shadow_brush);
            }

            // 2. Windows 11 Acrylic base surface
            let main_rrect = D2D1_ROUNDED_RECT {
                rect: *bar_rect,
                radiusX: 12.0,
                radiusY: 12.0,
            };
            let bg_acrylic = D2D1_COLOR_F {
                r: 0.11,
                g: 0.12,
                b: 0.15,
                a: 0.94,
            };
            let border_fluent = D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.14,
            };

            if let Ok(bg_brush) = rt.CreateSolidColorBrush(&bg_acrylic, None) {
                rt.FillRoundedRectangle(&main_rrect, &bg_brush);
            }
            if let Ok(border_brush) = rt.CreateSolidColorBrush(&border_fluent, None) {
                rt.DrawRoundedRectangle(&main_rrect, &border_brush, 1.0, None);
            }

            // 2.5. Render 6-dot drag grip handle on the left
            if let Ok(grip_brush) = rt.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.28,
                },
                None,
            ) {
                let gx = (toolbar.grip_rect.left + toolbar.grip_rect.right) / 2.0;
                let gy = (toolbar.grip_rect.top + toolbar.grip_rect.bottom) / 2.0;
                let col1_x = gx - 2.5;
                let col2_x = gx + 2.5;
                for row in [-7.0, 0.0, 7.0] {
                    let dot1 = D2D1_ELLIPSE {
                        point: v2(col1_x, gy + row),
                        radiusX: 1.5,
                        radiusY: 1.5,
                    };
                    let dot2 = D2D1_ELLIPSE {
                        point: v2(col2_x, gy + row),
                        radiusX: 1.5,
                        radiusY: 1.5,
                    };
                    rt.FillEllipse(&dot1, &grip_brush);
                    rt.FillEllipse(&dot2, &grip_brush);
                }
            }

            // If collapsed:
            if toolbar.collapsed {
                let is_hover = toolbar.hover_action == Some(FluentAction::ToggleCollapse);
                if is_hover
                    && let Ok(hover_brush) = rt.CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 0.10,
                        },
                        None,
                    )
                {
                    rt.FillRoundedRectangle(&main_rrect, &hover_brush);
                }
                if let Ok(txt_brush) = rt.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 0.92,
                        g: 0.94,
                        b: 0.98,
                        a: 1.0,
                    },
                    None,
                ) {
                    let label_utf16: Vec<u16> = "🎨 Zoomify ▾".encode_utf16().collect();
                    rt.DrawText(
                        &label_utf16,
                        &self.text_format_toolbar_small,
                        bar_rect,
                        &txt_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
                return;
            }

            // 3. Brushes for active items, hover, text, and dividers
            let active_bg_fill = D2D1_COLOR_F {
                r: 0.0,
                g: 0.47,
                b: 0.83,
                a: 0.38,
            };
            let active_border_col = D2D1_COLOR_F {
                r: 0.38,
                g: 0.72,
                b: 0.98,
                a: 0.90,
            };
            let hover_fill = D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.09,
            };
            let divider_col = D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.18,
            };
            let text_col = D2D1_COLOR_F {
                r: 0.95,
                g: 0.96,
                b: 0.98,
                a: 1.0,
            };
            let text_dim = D2D1_COLOR_F {
                r: 0.70,
                g: 0.72,
                b: 0.76,
                a: 1.0,
            };

            let active_bg_brush = rt.CreateSolidColorBrush(&active_bg_fill, None).ok();
            let active_border_brush = rt.CreateSolidColorBrush(&active_border_col, None).ok();
            let hover_brush = rt.CreateSolidColorBrush(&hover_fill, None).ok();
            let divider_brush = rt.CreateSolidColorBrush(&divider_col, None).ok();
            let text_brush = rt.CreateSolidColorBrush(&text_col, None).ok();
            let dim_text_brush = rt.CreateSolidColorBrush(&text_dim, None).ok();

            // 4. Render items
            let mut tooltip_text: Option<(String, D2D_RECT_F)> = None;

            for item in &toolbar.items {
                let is_hover = toolbar.hover_action == Some(item.action);

                // Determine active state
                let is_active = match item.action {
                    FluentAction::ModeZoom => mode == AppMode::StaticZoom && !spotlight.active,
                    FluentAction::ModeDraw => mode == AppMode::Draw,
                    FluentAction::ModeSpotlight => spotlight.active,
                    FluentAction::ModeTimer => mode == AppMode::Timer,
                    FluentAction::ModeLoupe => mode == AppMode::Loupe,
                    FluentAction::Tool(t) => tool == t && mode == AppMode::Draw,
                    FluentAction::Color(c) => color == c,
                    _ => false,
                };

                let is_color_item = matches!(
                    item.action,
                    FluentAction::Color(_) | FluentAction::OpenColorPicker
                );

                let btn_rrect = D2D1_ROUNDED_RECT {
                    rect: item.rect,
                    radiusX: 6.0,
                    radiusY: 6.0,
                };

                // Draw button background (never draw rectangular button backgrounds for color swatches)
                if !is_color_item {
                    if is_active {
                        if let Some(b) = &active_bg_brush {
                            rt.FillRoundedRectangle(&btn_rrect, b);
                        }
                        if let Some(b) = &active_border_brush {
                            rt.DrawRoundedRectangle(&btn_rrect, b, 1.0, None);
                        }
                    } else if is_hover && let Some(b) = &hover_brush {
                        rt.FillRoundedRectangle(&btn_rrect, b);
                    }
                }

                // Draw button content
                match item.action {
                    FluentAction::OpenColorPicker => {
                        let cx = (item.rect.left + item.rect.right) / 2.0;
                        let cy = (item.rect.top + item.rect.bottom) / 2.0;
                        let center = v2(cx, cy);
                        let showing_custom = matches!(color, ColorPreset::Custom(..));

                        if is_hover
                            && let Some(hb) = self.solid_brush(
                                rt,
                                &D2D1_COLOR_F {
                                    r: 1.0,
                                    g: 1.0,
                                    b: 1.0,
                                    a: 0.12,
                                },
                            )
                        {
                            rt.FillEllipse(
                                &D2D1_ELLIPSE {
                                    point: center,
                                    radiusX: 11.0,
                                    radiusY: 11.0,
                                },
                                &hb,
                            );
                        }

                        // Filled with the live custom colour, or hollow when a
                        // preset is active.
                        if showing_custom
                            && let Some(b) = self.solid_brush(rt, &color.to_d2d_color(1.0))
                        {
                            rt.FillEllipse(
                                &D2D1_ELLIPSE {
                                    point: center,
                                    radiusX: 7.5,
                                    radiusY: 7.5,
                                },
                                &b,
                            );
                        }

                        if let Some(b) = self.solid_brush(
                            rt,
                            &D2D1_COLOR_F {
                                r: 0.85,
                                g: 0.85,
                                b: 0.88,
                                a: 0.95,
                            },
                        ) {
                            rt.DrawEllipse(
                                &D2D1_ELLIPSE {
                                    point: center,
                                    radiusX: 7.0,
                                    radiusY: 7.0,
                                },
                                &b,
                                1.3,
                                None,
                            );
                            // "+" glyph
                            if !showing_custom {
                                rt.DrawLine(v2(cx - 3.5, cy), v2(cx + 3.5, cy), &b, 1.6, None);
                                rt.DrawLine(v2(cx, cy - 3.5), v2(cx, cy + 3.5), &b, 1.6, None);
                            }
                        }
                    }
                    FluentAction::Color(c) => {
                        let dot_center = v2(
                            (item.rect.left + item.rect.right) / 2.0,
                            (item.rect.top + item.rect.bottom) / 2.0,
                        );

                        // Subtle circular hover backdrop
                        if is_hover
                            && !is_active
                            && let Ok(hb) = rt.CreateSolidColorBrush(
                                &D2D1_COLOR_F {
                                    r: 1.0,
                                    g: 1.0,
                                    b: 1.0,
                                    a: 0.12,
                                },
                                None,
                            )
                        {
                            let halo = D2D1_ELLIPSE {
                                point: dot_center,
                                radiusX: 11.0,
                                radiusY: 11.0,
                            };
                            rt.FillEllipse(&halo, &hb);
                        }

                        // Circular color swatch dot
                        let dot_col = c.to_d2d_color(1.0);
                        if let Ok(dot_brush) = rt.CreateSolidColorBrush(&dot_col, None) {
                            let r = if is_active { 7.5 } else { 6.5 };
                            let el = D2D1_ELLIPSE {
                                point: dot_center,
                                radiusX: r,
                                radiusY: r,
                            };
                            rt.FillEllipse(&el, &dot_brush);
                        }

                        // Concentric selection ring when active (Windows 11 Fluent style)
                        if is_active {
                            let ring_col = if c == ColorPreset::White {
                                D2D1_COLOR_F {
                                    r: 0.38,
                                    g: 0.72,
                                    b: 0.98,
                                    a: 1.0,
                                }
                            } else {
                                D2D1_COLOR_F {
                                    r: 1.0,
                                    g: 1.0,
                                    b: 1.0,
                                    a: 0.95,
                                }
                            };
                            if let Ok(ring_brush) = rt.CreateSolidColorBrush(&ring_col, None) {
                                let ring = D2D1_ELLIPSE {
                                    point: dot_center,
                                    radiusX: 10.5,
                                    radiusY: 10.5,
                                };
                                rt.DrawEllipse(&ring, &ring_brush, 2.0, None);
                            }
                        }

                        if is_hover {
                            tooltip_text = Some((c.name(), item.rect));
                        }
                    }
                    _ => {
                        let (icon_str, tip) = match item.action {
                            FluentAction::ModeZoom => ("\u{E721}", "Zoom (Ctrl+1)"),
                            FluentAction::ModeDraw => ("\u{E70F}", "Draw (Ctrl+2)"),
                            FluentAction::ModeSpotlight => ("\u{E706}", "Spotlight (F3)"),
                            FluentAction::ModeTimer => ("\u{E916}", "Timer (Ctrl+5)"),
                            FluentAction::ModeLoupe => ("\u{E1A3}", "Magnifier Loupe (Ctrl+6)"),
                            FluentAction::CycleDisplay => ("\u{E7F4}", "Switch Display (Ctrl+Tab)"),
                            FluentAction::Tool(DrawTool::Select) => {
                                ("\u{E8B3}", "Select / Edit (V)")
                            }
                            FluentAction::Tool(DrawTool::Pen) => ("\u{ED63}", "Pen (P)"),
                            FluentAction::Tool(DrawTool::LaserPointer) => {
                                ("\u{EA3A}", "Laser Pointer (K)")
                            }
                            FluentAction::Tool(DrawTool::Highlighter) => {
                                ("\u{E7E6}", "Highlighter (H)")
                            }
                            FluentAction::Tool(DrawTool::Eraser) => ("\u{E75C}", "Eraser (X)"),
                            FluentAction::Tool(DrawTool::Arrow) => ("\u{E72A}", "Arrow (A)"),
                            FluentAction::Tool(DrawTool::Line) => ("\u{E790}", "Line (L)"),
                            FluentAction::Tool(DrawTool::Rectangle) => {
                                ("\u{E771}", "Rectangle (R)")
                            }
                            FluentAction::Tool(DrawTool::Ellipse) => ("\u{EA3B}", "Ellipse (E)"),
                            FluentAction::Tool(DrawTool::StepBadge) => {
                                ("\u{E8EC}", "Step Badge (N)")
                            }
                            FluentAction::Tool(DrawTool::Text) => ("\u{E8D2}", "Text (T)"),
                            FluentAction::Tool(DrawTool::StickyNote) => {
                                ("\u{E70B}", "Sticky Note (Shift+S)")
                            }
                            FluentAction::Tool(DrawTool::Blur) => {
                                ("\u{E80A}", "Redact / Blur (Shift+X)")
                            }
                            FluentAction::Tool(_) => ("", ""),
                            FluentAction::Undo => ("\u{E7A7}", "Undo (Ctrl+Z)"),
                            FluentAction::Clear => ("\u{E74D}", "Clear All (E)"),
                            FluentAction::Copy => ("\u{E8C8}", "Copy (Ctrl+C)"),
                            FluentAction::Save => ("\u{E74E}", "Save (Ctrl+S)"),
                            FluentAction::Close => ("\u{E8BB}", "Close (Esc)"),
                            FluentAction::ToggleCollapse => ("\u{E70E}", "Collapse (F2)"),
                            _ => ("", ""),
                        };

                        if !icon_str.is_empty() {
                            let icon_utf16: Vec<u16> = icon_str.encode_utf16().collect();
                            let tb = if is_active || is_hover {
                                &text_brush
                            } else {
                                &dim_text_brush
                            };
                            if let Some(brush) = tb {
                                rt.DrawText(
                                    &icon_utf16,
                                    &self.text_format_fluent_icons,
                                    &item.rect,
                                    brush,
                                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                                );
                            }
                        }

                        if is_hover && !tip.is_empty() {
                            tooltip_text = Some((tip.to_string(), item.rect));
                        }
                    }
                }
            }

            // Render main toolbar vertical separators
            if let Some(db) = &divider_brush {
                for &sep_x in &toolbar.separators {
                    let p1 = v2(sep_x, bar_rect.top + 7.0);
                    let p2 = v2(sep_x, bar_rect.bottom - 7.0);
                    rt.DrawLine(p1, p2, db, 1.0, None);
                }
            }

            // 4.5. Render Dynamic Context Sub-Bar (if active)
            let mut subbar_tooltip: Option<(String, D2D_RECT_F)> = None;
            if let Some(sb_rect) = toolbar.subbar_rect {
                // Drop shadow
                let sb_shadow = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: sb_rect.left,
                        top: sb_rect.top + 2.0,
                        right: sb_rect.right,
                        bottom: sb_rect.bottom + 3.0,
                    },
                    radiusX: 10.0,
                    radiusY: 10.0,
                };
                if let Ok(sb_sh_brush) = rt.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.30,
                    },
                    None,
                ) {
                    rt.FillRoundedRectangle(&sb_shadow, &sb_sh_brush);
                }

                // Subbar base surface
                let sb_main = D2D1_ROUNDED_RECT {
                    rect: sb_rect,
                    radiusX: 8.0,
                    radiusY: 8.0,
                };
                if let Ok(sb_bg) = rt.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 0.13,
                        g: 0.14,
                        b: 0.18,
                        a: 0.94,
                    },
                    None,
                ) {
                    rt.FillRoundedRectangle(&sb_main, &sb_bg);
                }
                if let Ok(sb_border) = rt.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.12,
                    },
                    None,
                ) {
                    rt.DrawRoundedRectangle(&sb_main, &sb_border, 1.0, None);
                }

                // Render Subbar Items
                for s_item in &toolbar.subbar_items {
                    let is_active = match s_item.action {
                        FluentAction::SetStrokeWidth(w) => (toolbar.stroke_width - w).abs() < 0.1,
                        FluentAction::SetFillMode(fm) => toolbar.current_fill_mode == fm,
                        FluentAction::SetStrokePattern(sp) => toolbar.current_stroke_pattern == sp,
                        FluentAction::SetArrowStyle(as_) => toolbar.current_arrow_style == as_,
                        FluentAction::SetBadgeSize(bs) => toolbar.current_badge_size == bs,
                        FluentAction::SetBadgeShape(bsh) => toolbar.current_badge_shape == bsh,
                        FluentAction::ResetBadgeCounter => false,
                        FluentAction::SetFontSize(sz) => {
                            (toolbar.current_font_size - sz).abs() < 0.5
                        }
                        FluentAction::ToggleBold => toolbar.text_is_bold,
                        FluentAction::ToggleItalic => toolbar.text_is_italic,
                        FluentAction::SetTextCardStyle(cs) => toolbar.text_card_style == cs,
                        FluentAction::SetFontFamily(ff) => toolbar.text_font_family == ff,
                        _ => false,
                    };
                    let is_hover = toolbar.hover_action == Some(s_item.action);

                    let item_rrect = D2D1_ROUNDED_RECT {
                        rect: s_item.rect,
                        radiusX: 5.0,
                        radiusY: 5.0,
                    };

                    if is_active {
                        if let Some(b) = &active_bg_brush {
                            rt.FillRoundedRectangle(&item_rrect, b);
                        }
                        if let Some(b) = &active_border_brush {
                            rt.DrawRoundedRectangle(&item_rrect, b, 1.0, None);
                        }
                    } else if is_hover && let Some(b) = &hover_brush {
                        rt.FillRoundedRectangle(&item_rrect, b);
                    }

                    let (label, tip): (&str, &'static str) = match s_item.action {
                        FluentAction::SetStrokeWidth(w) => {
                            if toolbar.active_tool == Some(DrawTool::Blur) {
                                if (w - 8.0).abs() < 0.1 {
                                    ("8px", "Fine mosaic: 8px")
                                } else if (w - 14.0).abs() < 0.1 {
                                    ("14px", "Medium mosaic: 14px")
                                } else if (w - 22.0).abs() < 0.1 {
                                    ("22px", "Coarse mosaic: 22px")
                                } else {
                                    ("32px", "Heavy mosaic: 32px")
                                }
                            } else if (w - 2.0).abs() < 0.1 {
                                ("2px", "Fine stroke: 2px")
                            } else if (w - 4.0).abs() < 0.1 {
                                ("4px", "Medium stroke: 4px")
                            } else if (w - 6.0).abs() < 0.1 {
                                ("6px", "Thick stroke: 6px")
                            } else if (w - 8.0).abs() < 0.1 {
                                ("8px", "Thick stroke: 8px")
                            } else {
                                ("14px", "Heavy stroke: 14px")
                            }
                        }
                        FluentAction::SetFillMode(fm) => match fm {
                            FillMode::None => ("Outline", "Outline wireframe (F)"),
                            FillMode::Tinted => ("Tint", "Tinted highlight fill (F)"),
                            FillMode::Solid => ("Solid", "Solid block fill (F)"),
                        },
                        FluentAction::SetStrokePattern(sp) => match sp {
                            StrokePattern::Solid => ("──", "Solid line pattern (D)"),
                            StrokePattern::Dashed => ("- -", "Dashed line pattern (D)"),
                            StrokePattern::Dotted => ("···", "Dotted line pattern (D)"),
                        },
                        FluentAction::SetArrowStyle(as_) => match as_ {
                            ArrowStyle::Single => ("──►", "Single arrow pointer"),
                            ArrowStyle::Double => ("◄──►", "Double-ended arrow"),
                            ArrowStyle::Dimension => ("|◄►|", "Dimension callout line"),
                        },
                        FluentAction::SetBadgeSize(bs) => match bs {
                            BadgeSize::Small => ("S", "Small badge (14px) [ or ]"),
                            BadgeSize::Medium => ("M", "Medium badge (18px) [ or ]"),
                            BadgeSize::Large => ("L", "Large badge (24px) [ or ]"),
                            BadgeSize::ExtraLarge => ("XL", "Extra large badge (30px) [ or ]"),
                        },
                        FluentAction::SetBadgeShape(bsh) => match bsh {
                            BadgeShape::Circle => ("●", "Circle badge"),
                            BadgeShape::Square => ("■", "Square badge"),
                            BadgeShape::Hexagon => ("⬡", "Hexagon badge"),
                        },
                        FluentAction::ResetBadgeCounter => {
                            ("↺ #1", "Reset badge counter to #1 (R or 0)")
                        }
                        FluentAction::SetFontSize(sz) => {
                            if (sz - 14.0).abs() < 0.5 {
                                ("14px", "Small font: 14px ([ or ])")
                            } else if (sz - 20.0).abs() < 0.5 {
                                ("20px", "Medium font: 20px ([ or ])")
                            } else if (sz - 28.0).abs() < 0.5 {
                                ("28px", "Heading font: 28px ([ or ])")
                            } else {
                                ("38px", "Title font: 38px ([ or ])")
                            }
                        }
                        FluentAction::ToggleBold => ("B", "Toggle Bold (Ctrl+B)"),
                        FluentAction::ToggleItalic => ("I", "Toggle Italic (Ctrl+I)"),
                        FluentAction::SetTextCardStyle(cs) => match cs {
                            TextCardStyle::Transparent => ("None", "Transparent floating text"),
                            TextCardStyle::Badge => ("Badge", "Translucent acrylic pill badge"),
                            TextCardStyle::Solid => ("Card", "Solid callout card with border"),
                        },
                        FluentAction::SetFontFamily(ff) => match ff {
                            TextFontFamily::SegoeUI => ("Sans", "Segoe UI (Fluent Interface)"),
                            TextFontFamily::CascadiaCode => ("Mono", "Cascadia Code (Monospace)"),
                            TextFontFamily::SegoePrint => ("Hand", "Segoe Print (Handwriting)"),
                        },
                        _ => ("", ""),
                    };

                    if is_hover && !tip.is_empty() {
                        subbar_tooltip = Some((tip.to_string(), s_item.rect));
                    }

                    if !label.is_empty()
                        && let Some(tb) = &text_brush
                    {
                        let l_utf16: Vec<u16> = label.encode_utf16().collect();
                        let centered = self.text_format_toolbar_small.clone();
                        let _ = centered.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                        let _ = centered.SetParagraphAlignment(windows::Win32::Graphics::DirectWrite::DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                        let _ = centered.SetWordWrapping(
                            windows::Win32::Graphics::DirectWrite::DWRITE_WORD_WRAPPING_NO_WRAP,
                        );
                        let tr = D2D_RECT_F {
                            left: s_item.rect.left,
                            top: s_item.rect.top,
                            right: s_item.rect.right,
                            bottom: s_item.rect.bottom,
                        };
                        rt.DrawText(
                            &l_utf16,
                            &centered,
                            &tr,
                            tb,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                }

                // Render subbar vertical separators
                if let Some(db) = &divider_brush {
                    for &sep_x in &toolbar.subbar_separators {
                        let p1 = v2(sep_x, sb_rect.top + 6.0);
                        let p2 = v2(sep_x, sb_rect.bottom - 6.0);
                        rt.DrawLine(p1, p2, db, 1.0, None);
                    }
                }
            }

            // 5. Hover tooltip pill below toolbar
            let active_tip = subbar_tooltip.or(tooltip_text);
            if let Some((tip, btn_rect)) = active_tip {
                let tip_w = (tip.len() as f32 * 7.5).max(75.0);
                let tip_h = 24.0;
                let btn_center_x = (btn_rect.left + btn_rect.right) / 2.0;
                let tip_left = (btn_center_x - tip_w / 2.0).max(10.0);
                let tip_top = if let Some(sb) = toolbar.subbar_rect {
                    sb.bottom + 4.0
                } else {
                    bar_rect.bottom + 6.0
                };

                let tip_rect = D2D_RECT_F {
                    left: tip_left,
                    top: tip_top,
                    right: tip_left + tip_w,
                    bottom: tip_top + tip_h,
                };
                let tip_rrect = D2D1_ROUNDED_RECT {
                    rect: tip_rect,
                    radiusX: 6.0,
                    radiusY: 6.0,
                };
                if let Ok(tip_bg) = rt.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 0.15,
                        g: 0.16,
                        b: 0.20,
                        a: 0.95,
                    },
                    None,
                ) {
                    rt.FillRoundedRectangle(&tip_rrect, &tip_bg);
                }
                if let Ok(tip_border) = rt.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.18,
                    },
                    None,
                ) {
                    rt.DrawRoundedRectangle(&tip_rrect, &tip_border, 1.0, None);
                }
                if let Some(tb) = &text_brush {
                    let tip_utf16: Vec<u16> = tip.encode_utf16().collect();
                    rt.DrawText(
                        &tip_utf16,
                        &self.text_format_toolbar_small,
                        &tip_rect,
                        tb,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }
        }
    }
}
