#![allow(dead_code)]

use windows::core::{w, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F,
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_ALTERNATE, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1PathGeometry,
    ID2D1RenderTarget, ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_LINE_JOIN_ROUND, D2D1_PRESENT_OPTIONS_IMMEDIATELY,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, RGBQUAD,
};
use windows_numerics::{Matrix3x2, Vector2};

use crate::capture::ScreenCapture;
use crate::shapes::{calculate_arrow_head, normalize_rect};
use crate::types::{
    AppMode, CanvasBackground, ColorPreset, DrawTool, Point2D, Shape, SnipSelection, SpotlightState,
    ToastNotification, ZoomState,
};

#[inline]
fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

pub struct D2DRenderer {
    pub factory: ID2D1Factory,
    pub dwrite_factory: IDWriteFactory,
    pub render_target: Option<ID2D1HwndRenderTarget>,
    pub round_stroke_style: ID2D1StrokeStyle,
    pub text_format_normal: IDWriteTextFormat,
    pub text_format_bold: IDWriteTextFormat,
    pub text_format_badge: IDWriteTextFormat,
    pub text_format_timer: IDWriteTextFormat,
    pub text_format_hud: IDWriteTextFormat,
    pub text_format_cheat_title: IDWriteTextFormat,
    pub text_format_cheat_item: IDWriteTextFormat,
}

impl D2DRenderer {
    pub fn new() -> Result<Self> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            let stroke_props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.0,
            };
            let round_stroke_style = factory.CreateStrokeStyle(&stroke_props, None)?;

            let text_format_normal = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                24.0,
                w!("en-us"),
            )?;

            let text_format_bold = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                24.0,
                w!("en-us"),
            )?;

            let text_format_badge = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                16.0,
                w!("en-us"),
            )?;
            let _ = text_format_badge.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = text_format_badge.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let text_format_timer = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                92.0,
                w!("en-us"),
            )?;
            let _ = text_format_timer.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = text_format_timer.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let text_format_hud = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                14.0,
                w!("en-us"),
            )?;

            let text_format_cheat_title = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                20.0,
                w!("en-us"),
            )?;

            let text_format_cheat_item = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                13.5,
                w!("en-us"),
            )?;

            Ok(Self {
                factory,
                dwrite_factory,
                render_target: None,
                round_stroke_style,
                text_format_normal,
                text_format_bold,
                text_format_badge,
                text_format_timer,
                text_format_hud,
                text_format_cheat_title,
                text_format_cheat_item,
            })
        }
    }

    pub fn init_hwnd(&mut self, hwnd: HWND, width: u32, height: u32) -> Result<()> {
        unsafe {
            let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: windows::Win32::Graphics::Direct2D::D2D1_FEATURE_LEVEL_DEFAULT,
            };

            let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd,
                pixelSize: windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U { width, height },
                presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
            };

            let rt = self.factory.CreateHwndRenderTarget(&rt_props, &hwnd_props)?;
            rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);

            self.render_target = Some(rt);
            Ok(())
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(rt) = &self.render_target {
            unsafe {
                let size = windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U { width, height };
                let _ = rt.Resize(&size);
            }
        }
    }

    pub fn render_frame(
        &self,
        mode: AppMode,
        width: f32,
        height: f32,
        bg_bitmap: Option<&ID2D1Bitmap>,
        bg_type: CanvasBackground,
        zoom_state: &ZoomState,
        spotlight: &SpotlightState,
        shapes: &[Shape],
        active_shape: Option<&Shape>,
        text_input: Option<(&Point2D, &str, &ColorPreset, f32)>,
        snip: Option<&SnipSelection>,
        current_tool: DrawTool,
        current_color: ColorPreset,
        current_stroke_width: f32,
        toast: Option<&ToastNotification>,
        show_cheat_sheet: bool,
        timer_remaining: Option<(u32, u32, f32, bool)>,
    ) {
        let rt = match &self.render_target {
            Some(rt) => rt,
            None => return,
        };

        unsafe {
            rt.BeginDraw();

            let clear_color = match bg_type {
                CanvasBackground::Transparent => D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
                CanvasBackground::Whiteboard => D2D1_COLOR_F { r: 0.98, g: 0.98, b: 0.99, a: 1.0 },
                CanvasBackground::Blackboard => D2D1_COLOR_F { r: 0.11, g: 0.12, b: 0.14, a: 1.0 },
            };
            rt.Clear(Some(&clear_color));

            let identity = Matrix3x2 {
                M11: 1.0,
                M12: 0.0,
                M21: 0.0,
                M22: 1.0,
                M31: 0.0,
                M32: 0.0,
            };

            let z = zoom_state.level.max(1.0);
            let canvas_matrix = if z > 1.001 {
                Matrix3x2 {
                    M11: z,
                    M12: 0.0,
                    M21: 0.0,
                    M22: z,
                    M31: -zoom_state.view_x * z,
                    M32: -zoom_state.view_y * z,
                }
            } else {
                identity
            };

            // ── Canvas Layer (Background + Spotlight + Shapes + Active Shape + Text Input) ──
            rt.SetTransform(&canvas_matrix);

            if bg_type == CanvasBackground::Transparent {
                if let Some(bitmap) = bg_bitmap {
                    let dst_rect = D2D_RECT_F {
                        left: 0.0,
                        top: 0.0,
                        right: width,
                        bottom: height,
                    };
                    rt.DrawBitmap(
                        bitmap,
                        Some(&dst_rect),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    );
                }
            }

            if spotlight.active {
                self.render_spotlight_mask(rt, width, height, spotlight);
            }

            for shape in shapes {
                self.render_single_shape(rt, shape);
            }

            if let Some(shape) = active_shape {
                self.render_single_shape(rt, shape);
            }

            if let Some((pos, text, color, font_size)) = text_input {
                let show_caret = (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    / 500)
                    % 2
                    == 0;
                self.render_text_editor(rt, pos, text, color, font_size, show_caret);
            }

            // ── Screen Space Layer (Snip Overlay + Timer + HUD + Toast + Modal) ──
            rt.SetTransform(&identity);

            if let Some(snip_sel) = snip {
                if snip_sel.active {
                    self.render_snip_overlay(rt, width, height, snip_sel);
                }
            }

            if let Some((mins, secs, progress, paused)) = timer_remaining {
                self.render_countdown_timer(rt, width, height, mins, secs, progress, paused);
            }

            if mode != AppMode::Timer {
                self.render_hud(
                    rt,
                    width,
                    height,
                    mode,
                    current_tool,
                    current_color,
                    current_stroke_width,
                    zoom_state.level,
                    spotlight,
                    bg_type,
                );
            }

            if let Some(t) = toast {
                if !t.is_expired() {
                    self.render_toast(rt, width, t);
                }
            }

            if show_cheat_sheet {
                self.render_cheat_sheet_modal(rt, width, height);
            }

            let _ = rt.EndDraw(None, None);
        }
    }

    pub fn render_to_capture(
        &self,
        screen_x: i32,
        screen_y: i32,
        width: u32,
        height: u32,
        bg_bitmap: Option<&ID2D1Bitmap>,
        bg_type: CanvasBackground,
        spotlight: &SpotlightState,
        shapes: &[Shape],
        active_shape: Option<&Shape>,
        text_input: Option<(&Point2D, &str, &ColorPreset, f32)>,
        include_spotlight: bool,
    ) -> Option<ScreenCapture> {
        if width == 0 || height == 0 {
            return None;
        }

        unsafe {
            let screen_dc = GetDC(None);
            if screen_dc.is_invalid() {
                return None;
            }

            let mem_dc = CreateCompatibleDC(Some(screen_dc));
            if mem_dc.is_invalid() {
                let _ = ReleaseDC(None, screen_dc);
                return None;
            }

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    biHeight: -(height as i32), // Negative height = top-down DIB
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    biSizeImage: width * height * 4,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [RGBQUAD {
                    rgbBlue: 0,
                    rgbGreen: 0,
                    rgbRed: 0,
                    rgbReserved: 0,
                }],
            };

            let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
            let hbitmap = CreateDIBSection(
                Some(mem_dc),
                &bmi,
                DIB_RGB_COLORS,
                &mut bits_ptr,
                None,
                0,
            );

            if let Ok(hbm) = hbitmap {
                if !hbm.is_invalid() && !bits_ptr.is_null() {
                    let old_bmp = SelectObject(mem_dc, hbm.into());

                    let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
                        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        dpiX: 96.0,
                        dpiY: 96.0,
                        usage: D2D1_RENDER_TARGET_USAGE_NONE,
                        minLevel: windows::Win32::Graphics::Direct2D::D2D1_FEATURE_LEVEL_DEFAULT,
                    };

                    if let Ok(dc_rt) = self.factory.CreateDCRenderTarget(&rt_props) {
                        let rect = windows::Win32::Foundation::RECT {
                            left: 0,
                            top: 0,
                            right: width as i32,
                            bottom: height as i32,
                        };
                        if dc_rt.BindDC(mem_dc, &rect).is_ok() {
                            dc_rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                            dc_rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);
                            dc_rt.BeginDraw();

                            let clear_color = match bg_type {
                                CanvasBackground::Transparent => D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 1.0 },
                                CanvasBackground::Whiteboard => D2D1_COLOR_F { r: 0.98, g: 0.98, b: 0.99, a: 1.0 },
                                CanvasBackground::Blackboard => D2D1_COLOR_F { r: 0.11, g: 0.12, b: 0.14, a: 1.0 },
                            };
                            dc_rt.Clear(Some(&clear_color));

                            if bg_type == CanvasBackground::Transparent {
                                if let Some(bitmap) = bg_bitmap {
                                    let dst_rect = D2D_RECT_F {
                                        left: 0.0,
                                        top: 0.0,
                                        right: width as f32,
                                        bottom: height as f32,
                                    };
                                    dc_rt.DrawBitmap(
                                        bitmap,
                                        Some(&dst_rect),
                                        1.0,
                                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                        None,
                                    );
                                }
                            }

                            if include_spotlight && spotlight.active {
                                self.render_spotlight_mask(&dc_rt, width as f32, height as f32, spotlight);
                            }

                            for shape in shapes {
                                self.render_single_shape(&dc_rt, shape);
                            }

                            if let Some(shape) = active_shape {
                                self.render_single_shape(&dc_rt, shape);
                            }

                            if let Some((pos, text, color, font_size)) = text_input {
                                self.render_text_editor(&dc_rt, pos, text, color, font_size, false);
                            }

                            let _ = dc_rt.EndDraw(None, None);

                            let total_bytes = (width * height * 4) as usize;
                            let mut pixels = vec![0u8; total_bytes];
                            std::ptr::copy_nonoverlapping(bits_ptr as *const u8, pixels.as_mut_ptr(), total_bytes);

                            for chunk in pixels.chunks_exact_mut(4) {
                                chunk[3] = 255;
                            }

                            let _ = SelectObject(mem_dc, old_bmp);
                            let _ = DeleteObject(hbm.into());
                            let _ = DeleteDC(mem_dc);
                            let _ = ReleaseDC(None, screen_dc);

                            return Some(ScreenCapture {
                                x: screen_x,
                                y: screen_y,
                                width,
                                height,
                                pixels,
                            });
                        }
                    }

                    let _ = SelectObject(mem_dc, old_bmp);
                    let _ = DeleteObject(hbm.into());
                }
            }

            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            None
        }
    }

    unsafe fn render_spotlight_mask(
        &self,
        rt: &ID2D1RenderTarget,
        screen_w: f32,
        screen_h: f32,
        spotlight: &SpotlightState,
    ) {
        unsafe {
            let geom: Result<ID2D1PathGeometry> = self.factory.CreatePathGeometry();
            if let Ok(path) = geom {
                if let Ok(sink) = path.Open() {
                    sink.SetFillMode(D2D1_FILL_MODE_ALTERNATE);

                    // Outer screen rectangle
                    sink.BeginFigure(v2(0.0, 0.0), D2D1_FIGURE_BEGIN_FILLED);
                    sink.AddLine(v2(screen_w, 0.0));
                    sink.AddLine(v2(screen_w, screen_h));
                    sink.AddLine(v2(0.0, screen_h));
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);

                    // Inner circle hole with 4 cubic bezier quadrant arcs (exact circle)
                    let r = spotlight.radius.max(20.0);
                    let cx = spotlight.x;
                    let cy = spotlight.y;
                    let k = 0.55228475 * r;

                    sink.BeginFigure(v2(cx, cy - r), D2D1_FIGURE_BEGIN_FILLED);
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: v2(cx + k, cy - r),
                        point2: v2(cx + r, cy - k),
                        point3: v2(cx + r, cy),
                    });
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: v2(cx + r, cy + k),
                        point2: v2(cx + k, cy + r),
                        point3: v2(cx, cy + r),
                    });
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: v2(cx - k, cy + r),
                        point2: v2(cx - r, cy + k),
                        point3: v2(cx - r, cy),
                    });
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: v2(cx - r, cy - k),
                        point2: v2(cx - k, cy - r),
                        point3: v2(cx, cy - r),
                    });
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    let _ = sink.Close();

                    let dim_brush_color = D2D1_COLOR_F {
                        r: 0.03,
                        g: 0.04,
                        b: 0.06,
                        a: spotlight.dim_opacity,
                    };
                    if let Ok(dim_brush) = rt.CreateSolidColorBrush(&dim_brush_color, None) {
                        rt.FillGeometry(&path, &dim_brush, None);
                    }

                    let ring_col = if spotlight.pinned {
                        D2D1_COLOR_F { r: 1.0, g: 0.35, b: 0.2, a: 0.9 }
                    } else {
                        D2D1_COLOR_F { r: 0.25, g: 0.75, b: 1.0, a: 0.85 }
                    };

                    if let Ok(ring_brush) = rt.CreateSolidColorBrush(&ring_col, None) {
                        let ellipse = D2D1_ELLIPSE {
                            point: v2(cx, cy),
                            radiusX: r,
                            radiusY: r,
                        };
                        rt.DrawEllipse(&ellipse, &ring_brush, 2.5, None);
                    }
                }
            }
        }
    }

    unsafe fn render_single_shape(&self, rt: &ID2D1RenderTarget, shape: &Shape) {
        unsafe {
            match shape {
                Shape::Stroke {
                    points,
                    color,
                    width,
                    is_highlighter,
                } => {
                    if points.is_empty() {
                        return;
                    }

                    let alpha = if *is_highlighter { 0.45 } else { 1.0 };
                    let actual_width = if *is_highlighter { *width * 2.2 } else { *width };
                    let col = color.to_d2d_color(alpha);

                    if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
                        if points.len() == 1 {
                            let dot = D2D1_ELLIPSE {
                                point: v2(points[0].x, points[0].y),
                                radiusX: actual_width / 2.0,
                                radiusY: actual_width / 2.0,
                            };
                            rt.FillEllipse(&dot, &brush);
                        } else if let Ok(path) = self.factory.CreatePathGeometry() {
                            if let Ok(sink) = path.Open() {
                                sink.BeginFigure(v2(points[0].x, points[0].y), D2D1_FIGURE_BEGIN_HOLLOW);
                                for pt in &points[1..] {
                                    sink.AddLine(v2(pt.x, pt.y));
                                }
                                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                                let _ = sink.Close();
                                rt.DrawGeometry(&path, &brush, actual_width, Some(&self.round_stroke_style));
                            }
                        }
                    }
                }

                Shape::Line {
                    start,
                    end,
                    color,
                    width,
                } => {
                    let col = color.to_d2d_color(1.0);
                    if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
                        let p0 = v2(start.x, start.y);
                        let p1 = v2(end.x, end.y);
                        rt.DrawLine(p0, p1, &brush, *width, None);
                    }
                }

                Shape::Arrow {
                    start,
                    end,
                    color,
                    width,
                } => {
                    let col = color.to_d2d_color(1.0);
                    if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
                        let p0 = v2(start.x, start.y);
                        let p1 = v2(end.x, end.y);
                        rt.DrawLine(p0, p1, &brush, *width, None);

                        let (tip, left, right) = calculate_arrow_head(*start, *end, *width * 5.0 + 12.0);
                        if let Ok(path) = self.factory.CreatePathGeometry() {
                            if let Ok(sink) = path.Open() {
                                sink.BeginFigure(v2(tip.x, tip.y), D2D1_FIGURE_BEGIN_FILLED);
                                sink.AddLine(v2(left.x, left.y));
                                sink.AddLine(v2(right.x, right.y));
                                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                                let _ = sink.Close();
                                rt.FillGeometry(&path, &brush, None);
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
                } => {
                    let (left, top, right, bottom) = normalize_rect(*start, *end);
                    let col = color.to_d2d_color(1.0);
                    if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
                        let rect = D2D_RECT_F { left, top, right, bottom };
                        if *rounded {
                            let rrect = D2D1_ROUNDED_RECT {
                                rect,
                                radiusX: 12.0,
                                radiusY: 12.0,
                            };
                            rt.DrawRoundedRectangle(&rrect, &brush, *width, None);
                        } else {
                            rt.DrawRectangle(&rect, &brush, *width, None);
                        }
                    }
                }

                Shape::Ellipse {
                    start,
                    end,
                    color,
                    width,
                } => {
                    let (left, top, right, bottom) = normalize_rect(*start, *end);
                    let cx = (left + right) / 2.0;
                    let cy = (top + bottom) / 2.0;
                    let rx = (right - left) / 2.0;
                    let ry = (bottom - top) / 2.0;

                    let col = color.to_d2d_color(1.0);
                    if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
                        let ellipse = D2D1_ELLIPSE {
                            point: v2(cx, cy),
                            radiusX: rx,
                            radiusY: ry,
                        };
                        rt.DrawEllipse(&ellipse, &brush, *width, None);
                    }
                }

                Shape::Text {
                    origin,
                    text,
                    font_size,
                    color,
                } => {
                    let col = color.to_d2d_color(1.0);
                    if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
                        let text_utf16: Vec<u16> = text.encode_utf16().collect();
                        let layout_rect = D2D_RECT_F {
                            left: origin.x,
                            top: origin.y,
                            right: origin.x + 1200.0,
                            bottom: origin.y + 400.0,
                        };

                        if let Ok(custom_format) = self.dwrite_factory.CreateTextFormat(
                            w!("Segoe UI"),
                            None,
                            DWRITE_FONT_WEIGHT_BOLD,
                            DWRITE_FONT_STYLE_NORMAL,
                            DWRITE_FONT_STRETCH_NORMAL,
                            *font_size,
                            w!("en-us"),
                        ) {
                            rt.DrawText(
                                &text_utf16,
                                &custom_format,
                                &layout_rect,
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
                } => {
                    let col = color.to_d2d_color(1.0);
                    let shadow_col = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.4 };
                    let text_col = if *color == ColorPreset::White || *color == ColorPreset::Yellow {
                        D2D1_COLOR_F { r: 0.1, g: 0.1, b: 0.1, a: 1.0 }
                    } else {
                        D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
                    };

                    if let Ok(sbrush) = rt.CreateSolidColorBrush(&shadow_col, None) {
                        let shadow_ellipse = D2D1_ELLIPSE {
                            point: v2(center.x + 1.5, center.y + 2.0),
                            radiusX: *radius,
                            radiusY: *radius,
                        };
                        rt.FillEllipse(&shadow_ellipse, &sbrush);
                    }

                    if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
                        let ellipse = D2D1_ELLIPSE {
                            point: v2(center.x, center.y),
                            radiusX: *radius,
                            radiusY: *radius,
                        };
                        rt.FillEllipse(&ellipse, &brush);

                        let white_ring = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.9 };
                        if let Ok(wbrush) = rt.CreateSolidColorBrush(&white_ring, None) {
                            rt.DrawEllipse(&ellipse, &wbrush, 2.0, None);
                        }
                    }

                    if let Ok(tbrush) = rt.CreateSolidColorBrush(&text_col, None) {
                        let num_str = format!("{}", number);
                        let utf16: Vec<u16> = num_str.encode_utf16().collect();
                        let text_rect = D2D_RECT_F {
                            left: center.x - *radius,
                            top: center.y - *radius,
                            right: center.x + *radius,
                            bottom: center.y + *radius,
                        };
                        rt.DrawText(
                            &utf16,
                            &self.text_format_badge,
                            &text_rect,
                            &tbrush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                }
            }
        }
    }

    unsafe fn render_text_editor(
        &self,
        rt: &ID2D1RenderTarget,
        origin: &Point2D,
        text: &str,
        color: &ColorPreset,
        font_size: f32,
        show_caret: bool,
    ) {
        unsafe {
            let col = color.to_d2d_color(1.0);
            let bg_col = D2D1_COLOR_F { r: 0.05, g: 0.05, b: 0.08, a: 0.85 };
            let border_col = D2D1_COLOR_F { r: 0.3, g: 0.65, b: 1.0, a: 0.95 };

            let estimated_w = (text.len() as f32 * font_size * 0.65).max(140.0) + 30.0;
            let estimated_h = font_size * 1.6 + 14.0;
            let rect = D2D_RECT_F {
                left: origin.x - 8.0,
                top: origin.y - 6.0,
                right: origin.x + estimated_w,
                bottom: origin.y + estimated_h,
            };

            if let Ok(bg_brush) = rt.CreateSolidColorBrush(&bg_col, None) {
                let rrect = D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: 6.0,
                    radiusY: 6.0,
                };
                rt.FillRoundedRectangle(&rrect, &bg_brush);
                if let Ok(border_brush) = rt.CreateSolidColorBrush(&border_col, None) {
                    rt.DrawRoundedRectangle(&rrect, &border_brush, 1.5, None);
                }
            }

            if let Ok(brush) = rt.CreateSolidColorBrush(&col, None) {
                let display_text = if show_caret {
                    format!("{}|", text)
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

                if let Ok(format) = self.dwrite_factory.CreateTextFormat(
                    w!("Segoe UI"),
                    None,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    font_size,
                    w!("en-us"),
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

    unsafe fn render_snip_overlay(
        &self,
        rt: &ID2D1RenderTarget,
        screen_w: f32,
        screen_h: f32,
        snip: &SnipSelection,
    ) {
        unsafe {
            let (left, top, right, bottom) = snip.rect();

            if let Ok(path) = self.factory.CreatePathGeometry() {
                if let Ok(sink) = path.Open() {
                    sink.SetFillMode(D2D1_FILL_MODE_ALTERNATE);

                    sink.BeginFigure(v2(0.0, 0.0), D2D1_FIGURE_BEGIN_FILLED);
                    sink.AddLine(v2(screen_w, 0.0));
                    sink.AddLine(v2(screen_w, screen_h));
                    sink.AddLine(v2(0.0, screen_h));
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);

                    sink.BeginFigure(v2(left, top), D2D1_FIGURE_BEGIN_FILLED);
                    sink.AddLine(v2(right, top));
                    sink.AddLine(v2(right, bottom));
                    sink.AddLine(v2(left, bottom));
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    let _ = sink.Close();

                    let mask_col = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.6 };
                    if let Ok(brush) = rt.CreateSolidColorBrush(&mask_col, None) {
                        rt.FillGeometry(&path, &brush, None);
                    }
                }
            }

            let border_col = D2D1_COLOR_F { r: 0.15, g: 0.65, b: 1.0, a: 1.0 };
            if let Ok(brush) = rt.CreateSolidColorBrush(&border_col, None) {
                let snip_rect = D2D_RECT_F { left, top, right, bottom };
                rt.DrawRectangle(&snip_rect, &brush, 2.0, None);
            }

            let w_px = (right - left).round() as u32;
            let h_px = (bottom - top).round() as u32;
            let badge_text = format!("✂️ {} × {} px (Release to copy)", w_px, h_px);
            let utf16: Vec<u16> = badge_text.encode_utf16().collect();

            let badge_x = left.max(10.0);
            let badge_y = if top > 40.0 { top - 32.0 } else { bottom + 8.0 };

            let badge_rect = D2D_RECT_F {
                left: badge_x,
                top: badge_y,
                right: badge_x + 240.0,
                bottom: badge_y + 26.0,
            };

            let badge_bg = D2D1_COLOR_F { r: 0.08, g: 0.09, b: 0.12, a: 0.9 };
            if let Ok(bg_brush) = rt.CreateSolidColorBrush(&badge_bg, None) {
                let rrect = D2D1_ROUNDED_RECT {
                    rect: badge_rect,
                    radiusX: 4.0,
                    radiusY: 4.0,
                };
                rt.FillRoundedRectangle(&rrect, &bg_brush);
            }

            let text_col = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 };
            if let Ok(tbrush) = rt.CreateSolidColorBrush(&text_col, None) {
                let text_draw_rect = D2D_RECT_F {
                    left: badge_x + 8.0,
                    top: badge_y + 3.0,
                    right: badge_x + 235.0,
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

    unsafe fn render_countdown_timer(
        &self,
        rt: &ID2D1RenderTarget,
        screen_w: f32,
        screen_h: f32,
        mins: u32,
        secs: u32,
        progress: f32,
        paused: bool,
    ) {
        unsafe {
            let card_w = 480.0;
            let card_h = 340.0;
            let cx = screen_w / 2.0;
            let cy = screen_h / 2.0;

            let card_rect = D2D_RECT_F {
                left: cx - card_w / 2.0,
                top: cy - card_h / 2.0,
                right: cx + card_w / 2.0,
                bottom: cy + card_h / 2.0,
            };

            let is_expired = mins == 0 && secs == 0;
            let is_warning = mins == 0 && secs <= 30 && !paused;

            let bg_col = D2D1_COLOR_F { r: 0.06, g: 0.07, b: 0.1, a: 0.92 };
            let border_col = if is_expired {
                D2D1_COLOR_F { r: 1.0, g: 0.2, b: 0.2, a: 0.95 }
            } else if is_warning {
                D2D1_COLOR_F { r: 1.0, g: 0.45, b: 0.15, a: 0.9 }
            } else if paused {
                D2D1_COLOR_F { r: 1.0, g: 0.65, b: 0.15, a: 0.85 }
            } else {
                D2D1_COLOR_F { r: 0.2, g: 0.65, b: 1.0, a: 0.85 }
            };

            if let Ok(bg_brush) = rt.CreateSolidColorBrush(&bg_col, None) {
                let rrect = D2D1_ROUNDED_RECT {
                    rect: card_rect,
                    radiusX: 20.0,
                    radiusY: 20.0,
                };
                rt.FillRoundedRectangle(&rrect, &bg_brush);
                if let Ok(b_brush) = rt.CreateSolidColorBrush(&border_col, None) {
                    let stroke_sz = if is_expired { 3.0 } else { 2.0 };
                    rt.DrawRoundedRectangle(&rrect, &b_brush, stroke_sz, None);
                }
            }

            let ring_radius = 110.0;
            let ring_center = v2(cx, cy - 20.0);

            let track_col = D2D1_COLOR_F { r: 0.2, g: 0.22, b: 0.28, a: 0.5 };
            if let Ok(track_brush) = rt.CreateSolidColorBrush(&track_col, None) {
                let el = D2D1_ELLIPSE {
                    point: ring_center,
                    radiusX: ring_radius,
                    radiusY: ring_radius,
                };
                rt.DrawEllipse(&el, &track_brush, 6.0, None);
            }

            if let Ok(arc_brush) = rt.CreateSolidColorBrush(&border_col, None) {
                let segments = (64.0 * progress.clamp(0.0, 1.0)) as usize;
                if segments > 1 {
                    if let Ok(path) = self.factory.CreatePathGeometry() {
                        if let Ok(sink) = path.Open() {
                            let start_angle = -std::f32::consts::FRAC_PI_2;
                            let p0_x = ring_center.X + ring_radius * start_angle.cos();
                            let p0_y = ring_center.Y + ring_radius * start_angle.sin();

                            sink.BeginFigure(v2(p0_x, p0_y), D2D1_FIGURE_BEGIN_HOLLOW);
                            for i in 1..=segments {
                                let angle = start_angle + (i as f32 / 64.0) * std::f32::consts::PI * 2.0;
                                let px = ring_center.X + ring_radius * angle.cos();
                                let py = ring_center.Y + ring_radius * angle.sin();
                                sink.AddLine(v2(px, py));
                            }
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                            let _ = sink.Close();
                            rt.DrawGeometry(&path, &arc_brush, 6.0, None);
                        }
                    }
                }
            }

            let time_str = format!("{:02}:{:02}", mins, secs);
            let time_utf16: Vec<u16> = time_str.encode_utf16().collect();
            let time_rect = D2D_RECT_F {
                left: cx - 200.0,
                top: cy - 85.0,
                right: cx + 200.0,
                bottom: cy + 35.0,
            };

            let text_col = if is_expired {
                D2D1_COLOR_F { r: 1.0, g: 0.3, b: 0.3, a: 1.0 }
            } else {
                D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
            };
            if let Ok(tbrush) = rt.CreateSolidColorBrush(&text_col, None) {
                rt.DrawText(
                    &time_utf16,
                    &self.text_format_timer,
                    &time_rect,
                    &tbrush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            let sub_text = if is_expired {
                "⏰ TIME'S UP! • Space: Reset | ↑/↓: ±1m | Esc: Exit"
            } else if paused {
                "PAUSED • Space: Resume | ↑/↓: ±1m | Esc: Exit"
            } else {
                "Space: Pause | ↑/↓: ±1m | Esc: Exit"
            };
            let sub_utf16: Vec<u16> = sub_text.encode_utf16().collect();
            let sub_rect = D2D_RECT_F {
                left: cx - 220.0,
                top: cy + 95.0,
                right: cx + 220.0,
                bottom: cy + 125.0,
            };

            let sub_col = if is_expired {
                D2D1_COLOR_F { r: 1.0, g: 0.6, b: 0.6, a: 0.95 }
            } else {
                D2D1_COLOR_F { r: 0.75, g: 0.8, b: 0.9, a: 0.9 }
            };
            if let Ok(sbrush) = rt.CreateSolidColorBrush(&sub_col, None) {
                let centered_hud = self.text_format_hud.clone();
                let _ = centered_hud.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                rt.DrawText(
                    &sub_utf16,
                    &centered_hud,
                    &sub_rect,
                    &sbrush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    unsafe fn render_hud(
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

            let bg_col = D2D1_COLOR_F { r: 0.08, g: 0.09, b: 0.13, a: 0.92 };
            let border_col = D2D1_COLOR_F { r: 0.22, g: 0.25, b: 0.35, a: 0.8 };

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
            let text_col = D2D1_COLOR_F { r: 0.9, g: 0.92, b: 0.96, a: 0.95 };
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

    unsafe fn render_toast(&self, rt: &ID2D1RenderTarget, screen_w: f32, toast: &ToastNotification) {
        unsafe {
            let opacity = toast.opacity();
            if opacity <= 0.01 {
                return;
            }

            let toast_w = 360.0;
            let toast_h = 42.0;
            let toast_x = (screen_w - toast_w) / 2.0;
            let toast_y = 30.0;

            let bg_col = D2D1_COLOR_F {
                r: 0.1,
                g: 0.12,
                b: 0.16,
                a: 0.94 * opacity,
            };
            let border_col = D2D1_COLOR_F {
                r: 0.3,
                g: 0.7,
                b: 1.0,
                a: 0.9 * opacity,
            };

            let rect = D2D_RECT_F {
                left: toast_x,
                top: toast_y,
                right: toast_x + toast_w,
                bottom: toast_y + toast_h,
            };

            if let Ok(bg_brush) = rt.CreateSolidColorBrush(&bg_col, None) {
                let rrect = D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: 10.0,
                    radiusY: 10.0,
                };
                rt.FillRoundedRectangle(&rrect, &bg_brush);
                if let Ok(b_brush) = rt.CreateSolidColorBrush(&border_col, None) {
                    rt.DrawRoundedRectangle(&rrect, &b_brush, 1.5, None);
                }
            }

            let message = format!("{} {}", toast.icon, toast.message);
            let utf16: Vec<u16> = message.encode_utf16().collect();
            let text_col = D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: opacity,
            };

            if let Ok(tbrush) = rt.CreateSolidColorBrush(&text_col, None) {
                let text_rect = D2D_RECT_F {
                    left: toast_x + 14.0,
                    top: toast_y + 11.0,
                    right: toast_x + toast_w - 14.0,
                    bottom: toast_y + toast_h - 6.0,
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

    unsafe fn render_cheat_sheet_modal(&self, rt: &ID2D1RenderTarget, screen_w: f32, screen_h: f32) {
        unsafe {
            let modal_w = 720.0;
            let modal_h = 660.0;
            let mx = (screen_w - modal_w) / 2.0;
            let my = (screen_h - modal_h) / 2.0;

            let bg_col = D2D1_COLOR_F { r: 0.08, g: 0.09, b: 0.13, a: 0.97 };
            let border_col = D2D1_COLOR_F { r: 0.25, g: 0.55, b: 0.95, a: 0.9 };

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

            let white_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None).ok();
            let accent_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.35, g: 0.75, b: 1.0, a: 1.0 }, None).ok();
            let desc_brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.78, g: 0.82, b: 0.9, a: 0.95 }, None).ok();

            if let (Some(wbrush), Some(abrush), Some(dbrush)) = (white_brush, accent_brush, desc_brush) {
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
                    ("D / P", "Pen (default freehand)"),
                    ("H", "Highlighter (translucent)"),
                    ("L", "Straight Line tool"),
                    ("A", "Arrow tool"),
                    ("Shift+R", "Rectangle tool"),
                    ("U", "Rounded Rectangle tool"),
                    ("T", "Text (click, type, Enter/Esc)"),
                    ("N", "Step Badge (Shift+N: reset #)"),
                    ("S / X", "Snip tool (drag rectangle)"),
                    ("", ""),
                    ("MOUSE MODIFIERS", ""),
                    ("Left Drag", "Draw with active tool"),
                    ("Shift + Drag", "Snap straight line (45°)"),
                    ("Ctrl + Drag", "Snap rectangle"),
                    ("Tab + Drag", "Snap ellipse"),
                    ("Shift+Ctrl + Drag", "Snap arrow"),
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
                    ("", ""),
                    ("CANVAS MODES", ""),
                    ("W", "Whiteboard slate"),
                    ("K", "Blackboard slate"),
                    ("", ""),
                    ("ACTIONS & CONTROLS", ""),
                    ("Space", "Pin Spotlight / Pause Timer"),
                    ("Tab", "Toggle Spotlight on/off"),
                    ("1..9 / [ / ]", "Brush size / Spotlight radius"),
                    ("Up / Down", "Adjust brush size / Timer ±1m"),
                    ("Ctrl+Z / Ctrl+Y", "Undo / Redo (restores clears)"),
                    ("Ctrl+C", "Copy full screen with drawings"),
                    ("Ctrl+S", "Save PNG snapshot to Pictures"),
                    ("E / Delete", "Clear canvas (undoable)"),
                    ("Esc / Right-Click", "Exit overlay"),
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
                        rt.DrawText(&h_utf16, &self.text_format_hud, &h_rect, &abrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                        y1 += 20.0;
                    } else {
                        let k_utf16: Vec<u16> = key.encode_utf16().collect();
                        let d_utf16: Vec<u16> = desc.encode_utf16().collect();
                        let k_rect = D2D_RECT_F { left: mx + 28.0, top: y1, right: mx + 130.0, bottom: y1 + 18.0 };
                        let d_rect = D2D_RECT_F { left: mx + 130.0, top: y1, right: mx + 360.0, bottom: y1 + 18.0 };
                        rt.DrawText(&k_utf16, &self.text_format_cheat_item, &k_rect, &wbrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                        rt.DrawText(&d_utf16, &self.text_format_cheat_item, &d_rect, &dbrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
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
                        rt.DrawText(&h_utf16, &self.text_format_hud, &h_rect, &abrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                        y2 += 20.0;
                    } else {
                        let k_utf16: Vec<u16> = key.encode_utf16().collect();
                        let d_utf16: Vec<u16> = desc.encode_utf16().collect();
                        let k_rect = D2D_RECT_F { left: mx + 380.0, top: y2, right: mx + 495.0, bottom: y2 + 18.0 };
                        let d_rect = D2D_RECT_F { left: mx + 495.0, top: y2, right: mx + modal_w - 24.0, bottom: y2 + 18.0 };
                        rt.DrawText(&k_utf16, &self.text_format_cheat_item, &k_rect, &wbrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                        rt.DrawText(&d_utf16, &self.text_format_cheat_item, &d_rect, &dbrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                        y2 += 18.5;
                    }
                }
            }
        }
    }
}
