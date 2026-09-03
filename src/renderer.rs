use std::cell::RefCell;
use std::collections::HashMap;

use windows::core::{w, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F,
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_ALTERNATE, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1Factory, ID2D1Geometry, ID2D1GeometryGroup,
    ID2D1HwndRenderTarget, ID2D1RenderTarget, ID2D1StrokeStyle,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_DASH, D2D1_DASH_STYLE_DOT, D2D1_DASH_STYLE_SOLID,
    D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_LINE_JOIN_ROUND, D2D1_PRESENT_OPTIONS_IMMEDIATELY, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
    D2D1_STROKE_STYLE_PROPERTIES, D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, RGBQUAD,
};
use windows_numerics::{Matrix3x2, Vector2};

use crate::capture::ScreenCapture;
use crate::shapes::{calculate_arrow_head, normalize_rect, points_to_bezier_segments};
use crate::types::{
    AppMode, ArrowStyle, BadgeShape, BadgeSize, CanvasBackground, ColorPreset, DrawTool,
    FillMode, FluentAction, FluentToolbarState, LaserTrailPoint, Point2D, Shape, SnipSelection,
    SnipShape, SpotlightState, StrokePattern, TextCardStyle, TextEditorState, TextFontFamily,
    TimerAction, TimerWidgetState, ToastNotification, ZoomState,
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
    pub dashed_stroke_style: ID2D1StrokeStyle,
    pub dotted_stroke_style: ID2D1StrokeStyle,
    #[allow(dead_code)]
    pub text_format_badge: IDWriteTextFormat,
    #[allow(dead_code)]
    pub text_format_timer: IDWriteTextFormat,
    pub text_format_hud: IDWriteTextFormat,
    pub text_format_cheat_title: IDWriteTextFormat,
    pub text_format_cheat_item: IDWriteTextFormat,
    #[allow(dead_code)]
    pub text_format_toolbar: IDWriteTextFormat,
    pub text_format_toolbar_small: IDWriteTextFormat,
    pub text_format_fluent_icons: IDWriteTextFormat,
    pub text_format_toast_title: IDWriteTextFormat,
    pub text_format_toast_sub: IDWriteTextFormat,
    pub text_formats_cache: RefCell<HashMap<u32, IDWriteTextFormat>>,
    pub spotlight_geometry_cache: RefCell<Option<(u32, ID2D1GeometryGroup)>>,
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

            let dashed_props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_DASH,
                dashOffset: 0.0,
            };
            let dashed_stroke_style = factory.CreateStrokeStyle(&dashed_props, None)?;

            let dotted_props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_DOT,
                dashOffset: 0.0,
            };
            let dotted_stroke_style = factory.CreateStrokeStyle(&dotted_props, None)?;

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

            let text_format_toolbar = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                15.0,
                w!("en-us"),
            )?;
            let _ = text_format_toolbar.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = text_format_toolbar.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let text_format_toolbar_small = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                12.0,
                w!("en-us"),
            )?;
            let _ = text_format_toolbar_small.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = text_format_toolbar_small.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let text_format_fluent_icons = dwrite_factory.CreateTextFormat(
                w!("Segoe Fluent Icons"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                14.0,
                w!("en-us"),
            ).or_else(|_| {
                dwrite_factory.CreateTextFormat(
                    w!("Segoe MDL2 Assets"),
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    14.0,
                    w!("en-us"),
                )
            })?;
            let _ = text_format_fluent_icons.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = text_format_fluent_icons.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let text_format_toast_title = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Display"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                13.5,
                w!("en-us"),
            ).or_else(|_| {
                dwrite_factory.CreateTextFormat(
                    w!("Segoe UI"),
                    None,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    13.5,
                    w!("en-us"),
                )
            })?;
            let _ = text_format_toast_title.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
            let _ = text_format_toast_title.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let text_format_toast_sub = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Text"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                11.0,
                w!("en-us"),
            ).or_else(|_| {
                dwrite_factory.CreateTextFormat(
                    w!("Segoe UI"),
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    11.0,
                    w!("en-us"),
                )
            })?;
            let _ = text_format_toast_sub.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
            let _ = text_format_toast_sub.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            Ok(Self {
                factory,
                dwrite_factory,
                render_target: None,
                round_stroke_style,
                dashed_stroke_style,
                dotted_stroke_style,
                text_format_badge,
                text_format_timer,
                text_format_hud,
                text_format_cheat_title,
                text_format_cheat_item,
                text_format_toolbar,
                text_format_toolbar_small,
                text_format_fluent_icons,
                text_format_toast_title,
                text_format_toast_sub,
                text_formats_cache: RefCell::new(HashMap::new()),
                spotlight_geometry_cache: RefCell::new(None),
            })
        }
    }

    pub fn get_stroke_style(&self, pattern: StrokePattern) -> &ID2D1StrokeStyle {
        match pattern {
            StrokePattern::Solid => &self.round_stroke_style,
            StrokePattern::Dashed => &self.dashed_stroke_style,
            StrokePattern::Dotted => &self.dotted_stroke_style,
        }
    }

    pub fn get_text_format(&self, font_size: f32) -> Result<IDWriteTextFormat> {
        self.get_custom_text_format(font_size, true, false, TextFontFamily::SegoeUI)
    }

    pub fn get_custom_text_format(
        &self,
        font_size: f32,
        is_bold: bool,
        is_italic: bool,
        font_family: TextFontFamily,
    ) -> Result<IDWriteTextFormat> {
        let sz = (font_size.round() as u32).clamp(8, 200);
        let b_flag = if is_bold { 1u32 << 16 } else { 0 };
        let i_flag = if is_italic { 1u32 << 17 } else { 0 };
        let f_flag = (font_family as u32) << 18;
        let key = sz | b_flag | i_flag | f_flag;

        let mut cache = self.text_formats_cache.borrow_mut();
        if let Some(format) = cache.get(&key) {
            return Ok(format.clone());
        }

        let family_name = match font_family {
            TextFontFamily::SegoeUI => w!("Segoe UI Variable Display"),
            TextFontFamily::CascadiaCode => w!("Cascadia Code"),
            TextFontFamily::SegoePrint => w!("Segoe Print"),
        };
        let weight = if is_bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_SEMI_BOLD };
        let style = if is_italic { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL };

        unsafe {
            let format = self.dwrite_factory.CreateTextFormat(
                family_name,
                None,
                weight,
                style,
                DWRITE_FONT_STRETCH_NORMAL,
                sz as f32,
                w!("en-us"),
            )?;
            cache.insert(key, format.clone());
            Ok(format)
        }
    }

    pub fn init_hwnd(&mut self, hwnd: HWND, width: u32, height: u32) -> Result<()> {
        unsafe {
            let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_IGNORE,
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
        text_input: Option<&TextEditorState>,
        snip: Option<&SnipSelection>,
        current_tool: DrawTool,
        current_color: ColorPreset,
        current_stroke_width: f32,
        toast: Option<&ToastNotification>,
        show_cheat_sheet: bool,
        show_hud: bool,
        timer_info: Option<(u32, u32, f32, bool, bool)>,
        timer_widget: &TimerWidgetState,
        toolbar: &FluentToolbarState,
        laser_trail: &[LaserTrailPoint],
        laser_pos: Option<Point2D>,
        eraser_pos: Option<Point2D>,
    ) {
        let rt = match &self.render_target {
            Some(rt) => rt,
            None => return,
        };

        if mode == AppMode::LiveZoom || mode == AppMode::Idle {
            return;
        }

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

            // ── Background Layer (Zoomed with canvas) ──
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

            // ── Shapes Layer (Zoomed with canvas) ──
            for shape in shapes {
                self.render_single_shape(rt, shape);
            }

            if let Some(shape) = active_shape {
                self.render_single_shape(rt, shape);
            }

            if let Some(editor) = text_input {
                let show_caret = (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    / 500)
                    % 2
                    == 0;
                self.render_text_editor(rt, editor, show_caret);
            }

            // ── Spotlight Mask (Screen Space for full edge-to-edge coverage) ──
            if spotlight.active {
                rt.SetTransform(&identity);
                self.render_spotlight_mask(
                    rt,
                    width,
                    height,
                    spotlight.x,
                    spotlight.y,
                    spotlight.radius,
                    spotlight.dim_opacity,
                    spotlight.pinned,
                );
                rt.SetTransform(&canvas_matrix);
            }

            // ── Laser Pointer (Zoomed with canvas) ──
            if !laser_trail.is_empty() || laser_pos.is_some() {
                self.render_laser_pointer(rt, laser_trail, laser_pos, current_color);
            }

            // ── Screen Space Layer (Snip Overlay + Timer + Eraser + HUD + Toast + Modal) ──
            rt.SetTransform(&identity);

            // ── Eraser Cursor Indicator ──
            if current_tool == DrawTool::Eraser {
                if let Some(epos) = eraser_pos {
                    self.render_eraser_indicator(rt, epos);
                }
            }

            // If in Timer mode, draw background dim overlay over desktop slides
            if mode == AppMode::Timer {
                let dim_val = timer_widget.dim_opacity.clamp(0.0, 0.95);
                let dim_col = D2D1_COLOR_F { r: 0.02, g: 0.03, b: 0.05, a: dim_val };
                if let Ok(dim_brush) = rt.CreateSolidColorBrush(&dim_col, None) {
                    let full_rect = D2D_RECT_F { left: 0.0, top: 0.0, right: width, bottom: height };
                    rt.FillRectangle(&full_rect, &dim_brush);
                }
            }

            if let Some(snip_sel) = snip {
                if snip_sel.active {
                    self.render_snip_overlay(rt, width, height, snip_sel);
                }
            }

            if let Some((mins, secs, progress, paused, is_overtime)) = timer_info {
                self.render_countdown_timer(rt, width, height, mins, secs, progress, paused, is_overtime, timer_widget);
            }

            if mode != AppMode::Timer {
                self.render_fluent_toolbar(
                    rt,
                    toolbar,
                    mode,
                    current_tool,
                    current_color,
                    spotlight,
                );
            }

            if show_hud && mode != AppMode::Timer {
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
                    self.render_toast(rt, width, height, t, toolbar);
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
        bg_pixels: Option<&[u8]>,
        bg_type: CanvasBackground,
        zoom_state: &ZoomState,
        spotlight: &SpotlightState,
        shapes: &[Shape],
        active_shape: Option<&Shape>,
        text_input: Option<&TextEditorState>,
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
                                Matrix3x2 {
                                    M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0,
                                }
                            };
                            let identity = Matrix3x2 {
                                M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0,
                            };

                            dc_rt.SetTransform(&canvas_matrix);

                            if bg_type == CanvasBackground::Transparent {
                                if let Some(pixels) = bg_pixels {
                                    let size = windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U { width, height };
                                    let props = windows::Win32::Graphics::Direct2D::D2D1_BITMAP_PROPERTIES {
                                        pixelFormat: D2D1_PIXEL_FORMAT {
                                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                                            alphaMode: windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_IGNORE,
                                        },
                                        dpiX: 96.0,
                                        dpiY: 96.0,
                                    };
                                    if let Ok(bmp) = dc_rt.CreateBitmap(size, Some(pixels.as_ptr() as *const std::ffi::c_void), width * 4, &props) {
                                        let dst_rect = D2D_RECT_F {
                                            left: 0.0,
                                            top: 0.0,
                                            right: width as f32,
                                            bottom: height as f32,
                                        };
                                        dc_rt.DrawBitmap(
                                            &bmp,
                                            Some(&dst_rect),
                                            1.0,
                                            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                            None,
                                        );
                                    }
                                }
                            }

                            for shape in shapes {
                                self.render_single_shape(&dc_rt, shape);
                            }

                            if let Some(shape) = active_shape {
                                self.render_single_shape(&dc_rt, shape);
                            }

                            if let Some(editor) = text_input {
                                self.render_text_editor(&dc_rt, editor, false);
                            }

                            if include_spotlight && spotlight.active {
                                let screen_pt = zoom_state.canvas_to_screen(Point2D::new(spotlight.x, spotlight.y));
                                dc_rt.SetTransform(&identity);
                                self.render_spotlight_mask(&dc_rt, width as f32, height as f32, screen_pt.x, screen_pt.y, spotlight.radius, spotlight.dim_opacity, spotlight.pinned);
                                dc_rt.SetTransform(&canvas_matrix);
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

                if let Ok(rect_geom) = self.factory.CreateRectangleGeometry(&rect) {
                    if let Ok(ellipse_geom) = self.factory.CreateEllipseGeometry(&ellipse) {
                        let geometries: [Option<ID2D1Geometry>; 2] = [
                            Some(rect_geom.into()),
                            Some(ellipse_geom.into()),
                        ];
                        if let Ok(group) = self.factory.CreateGeometryGroup(D2D1_FILL_MODE_ALTERNATE, &geometries) {
                            *cache = Some((radius_key, group));
                        }
                    }
                }
            }

            if let Some((_, group)) = cache.as_ref() {
                // Hardware GPU translation directly to (cx, cy)
                let trans = Matrix3x2 {
                    M11: 1.0, M12: 0.0,
                    M21: 0.0, M22: 1.0,
                    M31: cx, M32: cy,
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
                    D2D1_COLOR_F { r: 1.0, g: 0.35, b: 0.2, a: 0.9 }
                } else {
                    D2D1_COLOR_F { r: 0.25, g: 0.75, b: 1.0, a: 0.85 }
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
                    M11: 1.0, M12: 0.0,
                    M21: 0.0, M22: 1.0,
                    M31: 0.0, M32: 0.0,
                };
                rt.SetTransform(&identity);
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
                    pattern,
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

                        // If Double or Dimension: also draw arrow head at start
                        if *style == ArrowStyle::Double || *style == ArrowStyle::Dimension {
                            let (tip2, left2, right2) = calculate_arrow_head(*end, *start, head_len);
                            if let Ok(path2) = self.factory.CreatePathGeometry() {
                                if let Ok(sink2) = path2.Open() {
                                    sink2.BeginFigure(v2(tip2.x, tip2.y), D2D1_FIGURE_BEGIN_FILLED);
                                    sink2.AddLine(v2(left2.x, left2.y));
                                    sink2.AddLine(v2(right2.x, right2.y));
                                    sink2.EndFigure(D2D1_FIGURE_END_CLOSED);
                                    let _ = sink2.Close();
                                    rt.FillGeometry(&path2, &brush, None);
                                }
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

                                let s_top = v2(start.x + perp_x * tick_h, start.y + perp_y * tick_h);
                                let s_bot = v2(start.x - perp_x * tick_h, start.y - perp_y * tick_h);
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
                    let rect = D2D_RECT_F { left, top, right, bottom };
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
                    if let Ok(custom_format) = self.get_custom_text_format(*font_size, *is_bold, *is_italic, *font_family) {
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
                                if let Ok(card_bg) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.08, g: 0.09, b: 0.12, a: 0.65 }, None) {
                                    rt.FillRoundedRectangle(&card_rrect, &card_bg);
                                }
                                if let Ok(card_border) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.15 }, None) {
                                    rt.DrawRoundedRectangle(&card_rrect, &card_border, 1.0, None);
                                }
                            }
                            TextCardStyle::Solid => {
                                if let Ok(card_bg) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.12, g: 0.13, b: 0.17, a: 0.96 }, None) {
                                    rt.FillRoundedRectangle(&card_rrect, &card_bg);
                                }
                                if let Ok(card_border) = rt.CreateSolidColorBrush(&col, None) {
                                    rt.DrawRoundedRectangle(&card_rrect, &card_border, 1.5, None);
                                }
                            }
                            TextCardStyle::Transparent => {
                                if let Ok(sh_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.70 }, None) {
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
                        FillMode::Solid => {
                            rt.CreateSolidColorBrush(&col, None).ok()
                        }
                    };

                    let backplate_brush = if *fill == FillMode::Tinted {
                        let bp_col = D2D1_COLOR_F { r: 0.08, g: 0.10, b: 0.14, a: 0.70 };
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
                            for i in 0..6 {
                                let angle = (i as f32 * std::f32::consts::PI / 3.0) - std::f32::consts::FRAC_PI_2;
                                points[i] = Point2D::new(center.x + radius * angle.cos(), center.y + radius * angle.sin());
                            }
                            if let Ok(path) = self.factory.CreatePathGeometry() {
                                if let Ok(sink) = path.Open() {
                                    sink.BeginFigure(v2(points[0].x, points[0].y), D2D1_FIGURE_BEGIN_FILLED);
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
                    }

                    // Number text inside badge
                    let text = number.to_string();
                    let text_utf16: Vec<u16> = text.encode_utf16().collect();
                    let text_col = match fill {
                        FillMode::Solid => D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
                        FillMode::Tinted => D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
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

    unsafe fn render_text_editor(
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
                    if let Ok(bg_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.08, g: 0.09, b: 0.12, a: 0.65 }, None) {
                        rt.FillRoundedRectangle(&rrect, &bg_brush);
                    }
                    if let Ok(border_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.38, g: 0.72, b: 0.98, a: 0.85 }, None) {
                        rt.DrawRoundedRectangle(&rrect, &border_brush, 1.5, None);
                    }
                }
                TextCardStyle::Solid => {
                    if let Ok(bg_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.12, g: 0.13, b: 0.17, a: 0.96 }, None) {
                        rt.FillRoundedRectangle(&rrect, &bg_brush);
                    }
                    if let Ok(border_brush) = rt.CreateSolidColorBrush(&col, None) {
                        rt.DrawRoundedRectangle(&rrect, &border_brush, 1.5, None);
                    }
                }
                TextCardStyle::Transparent => {
                    if let Ok(bg_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.05, g: 0.05, b: 0.08, a: 0.45 }, None) {
                        rt.FillRoundedRectangle(&rrect, &bg_brush);
                    }
                    if let Ok(border_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.38, g: 0.72, b: 0.98, a: 0.80 }, None) {
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

                if let Ok(format) = self.get_custom_text_format(font_size, editor.is_bold, editor.is_italic, editor.font_family) {
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

                    let mask_col = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.6 };
                    if let Ok(brush) = rt.CreateSolidColorBrush(&mask_col, None) {
                        rt.FillGeometry(&path, &brush, None);
                    }
                }
            }

            let border_col = D2D1_COLOR_F { r: 0.15, g: 0.65, b: 1.0, a: 1.0 };
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
                    let snip_rect = D2D_RECT_F { left, top, right, bottom };
                    rt.DrawRectangle(&snip_rect, &brush, 2.0, None);
                }
            }

            let w_px = (right - left).round() as u32;
            let h_px = (bottom - top).round() as u32;
            let shape_tag = if snip.shape == SnipShape::Ellipse { "⭕ Circle" } else { "🔲 Rect" };
            let badge_text = format!("✂️ {} ({}×{} px) • Tab: Switch | Shift: Square", shape_tag, w_px, h_px);
            let utf16: Vec<u16> = badge_text.encode_utf16().collect();

            let badge_x = left.max(10.0);
            let badge_y = if top > 40.0 { top - 32.0 } else { bottom + 8.0 };

            let badge_rect = D2D_RECT_F {
                left: badge_x,
                top: badge_y,
                right: badge_x + 310.0,
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

    unsafe fn render_countdown_timer(
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
                let pill_rect = D2D_RECT_F { left, top, right, bottom };
                let rrect = D2D1_ROUNDED_RECT {
                    rect: pill_rect,
                    radiusX: 23.0,
                    radiusY: 23.0,
                };

                let bg_col = D2D1_COLOR_F { r: 0.07, g: 0.08, b: 0.12, a: 0.95 };
                if let Ok(bg_brush) = rt.CreateSolidColorBrush(&bg_col, None) {
                    rt.FillRoundedRectangle(&rrect, &bg_brush);
                }

                let border_col = if is_overtime {
                    D2D1_COLOR_F { r: 1.0, g: 0.25, b: 0.25, a: 0.9 }
                } else if paused {
                    D2D1_COLOR_F { r: 1.0, g: 0.7, b: 0.2, a: 0.85 }
                } else {
                    D2D1_COLOR_F { r: 0.0, g: 0.47, b: 0.83, a: 0.9 }
                };
                if let Ok(b_brush) = rt.CreateSolidColorBrush(&border_col, None) {
                    rt.DrawRoundedRectangle(&rrect, &b_brush, 1.5, None);
                }

                // b0: Cycle Corner button (left + 6.0 .. left + 34.0)
                let b0_rect = D2D_RECT_F { left: left + 6.0, top: top + 8.0, right: left + 34.0, bottom: bottom - 8.0 };
                let b0_rrect = D2D1_ROUNDED_RECT { rect: b0_rect, radiusX: 6.0, radiusY: 6.0 };
                let is_b0_hover = widget.hover_action == Some(TimerAction::CycleCorner);
                let b0_bg = if is_b0_hover {
                    D2D1_COLOR_F { r: 0.0, g: 0.47, b: 0.83, a: 0.8 }
                } else {
                    D2D1_COLOR_F { r: 0.2, g: 0.22, b: 0.28, a: 0.5 }
                };
                if let Ok(br) = rt.CreateSolidColorBrush(&b0_bg, None) {
                    rt.FillRoundedRectangle(&b0_rrect, &br);
                }
                let u0: Vec<u16> = "🔄".encode_utf16().collect();
                if let Ok(wbrush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.9 }, None) {
                    let centered = self.text_format_hud.clone();
                    let _ = centered.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                    let tr0 = D2D_RECT_F { left: b0_rect.left, top: b0_rect.top + 3.0, right: b0_rect.right, bottom: b0_rect.bottom };
                    rt.DrawText(&u0, &centered, &tr0, &wbrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
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
                    D2D1_COLOR_F { r: 1.0, g: 0.35, b: 0.35, a: 1.0 }
                } else {
                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
                };
                if let Ok(tbrush) = rt.CreateSolidColorBrush(&text_col, None) {
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
                let btn_bg = D2D1_COLOR_F { r: 0.2, g: 0.22, b: 0.28, a: 0.6 };
                let btn_brush = rt.CreateSolidColorBrush(&btn_bg, None).ok();

                // 1. Play/Pause
                let b1_rect = D2D_RECT_F { left: right - 105.0, top: top + 8.0, right: right - 72.0, bottom: bottom - 8.0 };
                let b1_rrect = D2D1_ROUNDED_RECT { rect: b1_rect, radiusX: 6.0, radiusY: 6.0 };
                if let Some(ref br) = btn_brush { rt.FillRoundedRectangle(&b1_rrect, br); }
                let icon1 = if paused { "▶" } else { "⏸" };
                let u1: Vec<u16> = icon1.encode_utf16().collect();
                if let Ok(wbrush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.9 }, None) {
                    let centered = self.text_format_hud.clone();
                    let _ = centered.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                    let tr1 = D2D_RECT_F { left: b1_rect.left, top: b1_rect.top + 4.0, right: b1_rect.right, bottom: b1_rect.bottom };
                    rt.DrawText(&u1, &centered, &tr1, &wbrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                }

                // 2. Expand
                let b2_rect = D2D_RECT_F { left: right - 68.0, top: top + 8.0, right: right - 38.0, bottom: bottom - 8.0 };
                let b2_rrect = D2D1_ROUNDED_RECT { rect: b2_rect, radiusX: 6.0, radiusY: 6.0 };
                if let Some(ref br) = btn_brush { rt.FillRoundedRectangle(&b2_rrect, br); }
                let u2: Vec<u16> = "🗖".encode_utf16().collect();
                if let Ok(wbrush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.9 }, None) {
                    let centered = self.text_format_hud.clone();
                    let _ = centered.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                    let tr2 = D2D_RECT_F { left: b2_rect.left, top: b2_rect.top + 4.0, right: b2_rect.right, bottom: b2_rect.bottom };
                    rt.DrawText(&u2, &centered, &tr2, &wbrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                }

                // 3. Close
                let b3_rect = D2D_RECT_F { left: right - 35.0, top: top + 8.0, right: right - 5.0, bottom: bottom - 8.0 };
                let b3_rrect = D2D1_ROUNDED_RECT { rect: b3_rect, radiusX: 6.0, radiusY: 6.0 };
                if let Some(ref br) = btn_brush { rt.FillRoundedRectangle(&b3_rrect, br); }
                let u3: Vec<u16> = "✕".encode_utf16().collect();
                if let Ok(wbrush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.9 }, None) {
                    let centered = self.text_format_hud.clone();
                    let _ = centered.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                    let tr3 = D2D_RECT_F { left: b3_rect.left, top: b3_rect.top + 4.0, right: b3_rect.right, bottom: b3_rect.bottom };
                    rt.DrawText(&u3, &centered, &tr3, &wbrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                }
                return;
            }

            // Full Center Card Mode (Draggable)
            let (card_left, card_top, card_right, card_bottom, cx, cy) = widget.get_card_bounds(screen_w, screen_h);
            let card_rect = D2D_RECT_F {
                left: card_left,
                top: card_top,
                right: card_right,
                bottom: card_bottom,
            };

            let is_warning = mins == 0 && secs <= 30 && !paused && !is_overtime;

            let bg_col = D2D1_COLOR_F { r: 0.06, g: 0.07, b: 0.1, a: 0.95 };
            let border_col = if is_overtime {
                D2D1_COLOR_F { r: 1.0, g: 0.22, b: 0.22, a: 0.95 }
            } else if is_warning {
                D2D1_COLOR_F { r: 1.0, g: 0.45, b: 0.15, a: 0.9 }
            } else if paused {
                D2D1_COLOR_F { r: 1.0, g: 0.65, b: 0.15, a: 0.85 }
            } else {
                D2D1_COLOR_F { r: 0.0, g: 0.47, b: 0.83, a: 0.85 }
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
            if let Ok(s_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.45 }, None) {
                rt.FillRoundedRectangle(&shadow_rect, &s_brush);
            }

            let rrect = D2D1_ROUNDED_RECT {
                rect: card_rect,
                radiusX: 20.0,
                radiusY: 20.0,
            };

            if let Ok(bg_brush) = rt.CreateSolidColorBrush(&bg_col, None) {
                rt.FillRoundedRectangle(&rrect, &bg_brush);
            }
            if let Ok(b_brush) = rt.CreateSolidColorBrush(&border_col, None) {
                let stroke_sz = if is_overtime { 2.5 } else { 1.5 };
                rt.DrawRoundedRectangle(&rrect, &b_brush, stroke_sz, None);
            }

            let centered = self.text_format_hud.clone();
            let _ = centered.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);

            // 1. Session Title Header with Top-Right Close Button
            let title_utf16: Vec<u16> = widget.session_title.encode_utf16().collect();
            let title_rect = D2D_RECT_F {
                left: cx - 200.0,
                top: cy - 205.0,
                right: cx + 200.0,
                bottom: cy - 180.0,
            };
            if let Ok(t_fmt) = self.get_text_format(13.0) {
                let _ = t_fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let _ = t_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let Ok(t_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.65, g: 0.72, b: 0.85, a: 0.85 }, None) {
                    rt.DrawText(&title_utf16, &t_fmt, &title_rect, &t_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                }
            }

            // Top-Right Close Button (✕)
            let close_cx = card_right - 32.0;
            let close_cy = card_top + 28.0;
            let is_close_hover = widget.hover_action == Some(TimerAction::Close);
            let close_el = D2D1_ELLIPSE { point: v2(close_cx, close_cy), radiusX: 16.0, radiusY: 16.0 };
            let close_bg = if is_close_hover {
                D2D1_COLOR_F { r: 0.9, g: 0.2, b: 0.25, a: 0.80 }
            } else {
                D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.08 }
            };
            if let Ok(cbrush) = rt.CreateSolidColorBrush(&close_bg, None) {
                rt.FillEllipse(&close_el, &cbrush);
            }
            // Crisp vector cross lines
            if let Ok(wbrush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.90 }, None) {
                rt.DrawLine(v2(close_cx - 5.0, close_cy - 5.0), v2(close_cx + 5.0, close_cy + 5.0), &wbrush, 1.8, None);
                rt.DrawLine(v2(close_cx - 5.0, close_cy + 5.0), v2(close_cx + 5.0, close_cy - 5.0), &wbrush, 1.8, None);
            }

            // 2. Quick Duration Segmented Pills: [5m] [10m] [15m] [25m] [30m]
            let pill_w = 60.0;
            let pill_h = 30.0;
            let pill_gap = 10.0;
            let total_pills_w = 5.0 * pill_w + 4.0 * pill_gap;
            let pill_row_x = cx - total_pills_w / 2.0;
            let pill_row_y = cy - 162.0;

            let durations = [(5, "5m"), (10, "10m"), (15, "15m"), (25, "25m"), (30, "30m")];
            let pill_fmt = self.get_text_format(13.0).ok();
            if let Some(ref pf) = pill_fmt {
                let _ = pf.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let _ = pf.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            }

            for (i, &(dur, label)) in durations.iter().enumerate() {
                let px = pill_row_x + i as f32 * (pill_w + pill_gap);
                let p_rect = D2D_RECT_F { left: px, top: pill_row_y, right: px + pill_w, bottom: pill_row_y + pill_h };
                let p_rrect = D2D1_ROUNDED_RECT { rect: p_rect, radiusX: 15.0, radiusY: 15.0 };

                let is_hover = widget.hover_action == Some(TimerAction::SetDuration(dur));
                let is_current = mins == dur && !is_overtime;

                let p_bg = if is_current {
                    D2D1_COLOR_F { r: 0.0, g: 0.47, b: 0.83, a: 0.55 }
                } else if is_hover {
                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 }
                } else {
                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.05 }
                };

                if let Ok(pbrush) = rt.CreateSolidColorBrush(&p_bg, None) {
                    rt.FillRoundedRectangle(&p_rrect, &pbrush);
                }

                let p_border = if is_current {
                    D2D1_COLOR_F { r: 0.38, g: 0.80, b: 1.0, a: 0.95 }
                } else {
                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 }
                };
                if let Ok(pb_brush) = rt.CreateSolidColorBrush(&p_border, None) {
                    rt.DrawRoundedRectangle(&p_rrect, &pb_brush, 1.0, None);
                }

                let p_utf16: Vec<u16> = label.encode_utf16().collect();
                if let Some(ref pf) = pill_fmt {
                    if let Ok(wbrush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.92, g: 0.94, b: 0.98, a: 0.95 }, None) {
                        rt.DrawText(&p_utf16, pf, &p_rect, &wbrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                    }
                }
            }

            // 3. Central Glowing Progress Ring
            let ring_radius = 100.0;
            let ring_center = v2(cx, cy - 10.0);

            // Ring track background
            let track_col = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.08 };
            if let Ok(track_brush) = rt.CreateSolidColorBrush(&track_col, None) {
                let el = D2D1_ELLIPSE {
                    point: ring_center,
                    radiusX: ring_radius,
                    radiusY: ring_radius,
                };
                rt.DrawEllipse(&el, &track_brush, 7.0, None);
            }

            // Smooth Progress Arc (128 smooth segments)
            if let Ok(arc_brush) = rt.CreateSolidColorBrush(&border_col, None) {
                let segments = (128.0 * progress.clamp(0.0, 1.0)) as usize;
                if segments > 1 {
                    if let Ok(path) = self.factory.CreatePathGeometry() {
                        if let Ok(sink) = path.Open() {
                            let start_angle = -std::f32::consts::FRAC_PI_2;
                            let p0_x = ring_center.X + ring_radius * start_angle.cos();
                            let p0_y = ring_center.Y + ring_radius * start_angle.sin();

                            sink.BeginFigure(v2(p0_x, p0_y), D2D1_FIGURE_BEGIN_HOLLOW);
                            for i in 1..=segments {
                                let angle = start_angle + (i as f32 / 128.0) * std::f32::consts::PI * 2.0;
                                let px = ring_center.X + ring_radius * angle.cos();
                                let py = ring_center.Y + ring_radius * angle.sin();
                                sink.AddLine(v2(px, py));
                            }
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                            let _ = sink.Close();
                            rt.DrawGeometry(&path, &arc_brush, 7.0, Some(&self.round_stroke_style));
                        }
                    }

                    // Leading glowing bead indicator
                    let head_angle = -std::f32::consts::FRAC_PI_2 + (segments as f32 / 128.0) * std::f32::consts::PI * 2.0;
                    let bead_x = ring_center.X + ring_radius * head_angle.cos();
                    let bead_y = ring_center.Y + ring_radius * head_angle.sin();
                    let bead_el = D2D1_ELLIPSE { point: v2(bead_x, bead_y), radiusX: 6.0, radiusY: 6.0 };
                    if let Ok(bead_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 }, None) {
                        rt.FillEllipse(&bead_el, &bead_brush);
                    }
                }
            }

            // Overtime pulsing halo
            if is_overtime {
                let pulse_col = D2D1_COLOR_F { r: 1.0, g: 0.1, b: 0.1, a: 0.25 };
                if let Ok(pulse_brush) = rt.CreateSolidColorBrush(&pulse_col, None) {
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
                D2D1_COLOR_F { r: 1.0, g: 0.25, b: 0.25, a: 1.0 }
            } else {
                D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
            };
            if let Ok(t_fmt) = self.get_text_format(52.0) {
                let _ = t_fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let _ = t_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let Ok(tbrush) = rt.CreateSolidColorBrush(&text_col, None) {
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
            let sub_label = if is_overtime { "OVERTIME" } else if paused { "PAUSED" } else { "TIME REMAINING" };
            let sub_label_utf16: Vec<u16> = sub_label.encode_utf16().collect();
            let sub_rect = D2D_RECT_F {
                left: cx - 120.0,
                top: cy + 24.0,
                right: cx + 120.0,
                bottom: cy + 42.0,
            };
            if let Ok(s_fmt) = self.get_text_format(11.0) {
                let _ = s_fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let _ = s_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let Ok(st_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.65, g: 0.72, b: 0.85, a: 0.75 }, None) {
                    rt.DrawText(&sub_label_utf16, &s_fmt, &sub_rect, &st_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                }
            }

            // 4. Modern Action Controls: [-1m] [⟲] [ Hero Play/Pause ] [+1m] [🗗]
            let btn_y = cy + 130.0;

            // 4.1 Hero Play/Pause Button in the center (radius 28.0 = 56px diameter)
            let is_hero_hover = widget.hover_action == Some(TimerAction::PlayPause);
            let hero_bg = if is_hero_hover {
                D2D1_COLOR_F { r: 0.12, g: 0.58, b: 0.95, a: 0.98 }
            } else {
                D2D1_COLOR_F { r: 0.0, g: 0.47, b: 0.83, a: 0.92 }
            };
            let hero_el = D2D1_ELLIPSE { point: v2(cx, btn_y), radiusX: 28.0, radiusY: 28.0 };
            if let Ok(hbrush) = rt.CreateSolidColorBrush(&hero_bg, None) {
                rt.FillEllipse(&hero_el, &hbrush);
            }
            if let Ok(hb_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.45, g: 0.82, b: 1.0, a: 0.95 }, None) {
                rt.DrawEllipse(&hero_el, &hb_brush, 1.5, None);
            }

            // Vector Hero Icons (Pause = two vertical bars, Play = triangle)
            if let Ok(white_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None) {
                if paused {
                    // Vector Play Triangle
                    if let Ok(path) = self.factory.CreatePathGeometry() {
                        if let Ok(sink) = path.Open() {
                            sink.BeginFigure(v2(cx - 6.0, btn_y - 9.0), D2D1_FIGURE_BEGIN_FILLED);
                            sink.AddLine(v2(cx - 6.0, btn_y + 9.0));
                            sink.AddLine(v2(cx + 9.0, btn_y));
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                            let _ = sink.Close();
                            rt.FillGeometry(&path, &white_brush, None);
                        }
                    }
                } else {
                    // Vector Pause Double Bars (no emoji box/tofu glyph!)
                    let bar1 = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: cx - 7.0, top: btn_y - 9.0, right: cx - 2.0, bottom: btn_y + 9.0 },
                        radiusX: 1.5,
                        radiusY: 1.5,
                    };
                    let bar2 = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: cx + 2.0, top: btn_y - 9.0, right: cx + 7.0, bottom: btn_y + 9.0 },
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

            let sec_fmt = self.get_text_format(13.0).ok();
            if let Some(ref sf) = sec_fmt {
                let _ = sf.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let _ = sf.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            }

            let reset_fmt = self.get_text_format(18.0).ok();
            if let Some(ref rf) = reset_fmt {
                let _ = rf.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let _ = rf.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            }

            for &(scx, label, action) in &secondary_buttons {
                let is_hover = widget.hover_action == Some(action);
                let sec_el = D2D1_ELLIPSE { point: v2(scx, btn_y), radiusX: 22.0, radiusY: 22.0 };
                let sec_bg = if is_hover {
                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.16 }
                } else {
                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.07 }
                };
                if let Ok(sbrush) = rt.CreateSolidColorBrush(&sec_bg, None) {
                    rt.FillEllipse(&sec_el, &sbrush);
                }
                let sec_border = if is_hover {
                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.35 }
                } else {
                    D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 }
                };
                if let Ok(sb_brush) = rt.CreateSolidColorBrush(&sec_border, None) {
                    rt.DrawEllipse(&sec_el, &sb_brush, 1.0, None);
                }

                if action == TimerAction::ToggleMinimize {
                    // Vector PIP / Mini window icon (two crisp overlapping rectangles)
                    if let Ok(wbrush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.95, g: 0.95, b: 0.98, a: 0.95 }, None) {
                        let outer_win = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: scx - 8.0, top: btn_y - 7.0, right: scx + 8.0, bottom: btn_y + 7.0 },
                            radiusX: 2.0,
                            radiusY: 2.0,
                        };
                        rt.DrawRoundedRectangle(&outer_win, &wbrush, 1.4, None);
                        let inner_pip = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: scx + 1.0, top: btn_y, right: scx + 7.0, bottom: btn_y + 6.0 },
                            radiusX: 1.0,
                            radiusY: 1.0,
                        };
                        rt.FillRoundedRectangle(&inner_pip, &wbrush);
                    }
                } else if action == TimerAction::Reset {
                    let l_utf16: Vec<u16> = label.encode_utf16().collect();
                    let txt_rect = D2D_RECT_F { left: scx - 22.0, top: btn_y - 22.0, right: scx + 22.0, bottom: btn_y + 22.0 };
                    if let Some(ref rf) = reset_fmt {
                        if let Ok(wbrush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.95, g: 0.95, b: 0.98, a: 0.95 }, None) {
                            rt.DrawText(&l_utf16, rf, &txt_rect, &wbrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                        }
                    }
                } else {
                    let l_utf16: Vec<u16> = label.encode_utf16().collect();
                    let txt_rect = D2D_RECT_F { left: scx - 22.0, top: btn_y - 22.0, right: scx + 22.0, bottom: btn_y + 22.0 };
                    if let Some(ref sf) = sec_fmt {
                        if let Ok(wbrush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.95, g: 0.95, b: 0.98, a: 0.95 }, None) {
                            rt.DrawText(&l_utf16, sf, &txt_rect, &wbrush, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                        }
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
                D2D1_COLOR_F { r: 1.0, g: 0.6, b: 0.6, a: 0.95 }
            } else {
                D2D1_COLOR_F { r: 0.65, g: 0.70, b: 0.80, a: 0.75 }
            };
            if let Ok(s_fmt) = self.get_text_format(12.0) {
                let _ = s_fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let _ = s_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                if let Ok(sbrush) = rt.CreateSolidColorBrush(&sub_col, None) {
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

    unsafe fn render_laser_pointer(
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
                let white_col = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
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

    unsafe fn render_eraser_indicator(&self, rt: &ID2D1RenderTarget, pos: Point2D) {
        unsafe {
            let ring_col = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.75 };
            let fill_col = D2D1_COLOR_F { r: 1.0, g: 0.3, b: 0.3, a: 0.15 };
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

    unsafe fn render_fluent_toolbar(
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
            if let Ok(shadow_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.35 }, None) {
                rt.FillRoundedRectangle(&shadow_rrect, &shadow_brush);
            }

            // 2. Windows 11 Acrylic base surface
            let main_rrect = D2D1_ROUNDED_RECT {
                rect: *bar_rect,
                radiusX: 12.0,
                radiusY: 12.0,
            };
            let bg_acrylic = D2D1_COLOR_F { r: 0.11, g: 0.12, b: 0.15, a: 0.94 };
            let border_fluent = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.14 };

            if let Ok(bg_brush) = rt.CreateSolidColorBrush(&bg_acrylic, None) {
                rt.FillRoundedRectangle(&main_rrect, &bg_brush);
            }
            if let Ok(border_brush) = rt.CreateSolidColorBrush(&border_fluent, None) {
                rt.DrawRoundedRectangle(&main_rrect, &border_brush, 1.0, None);
            }

            // 2.5. Render 6-dot drag grip handle on the left
            if let Ok(grip_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.28 }, None) {
                let gx = (toolbar.grip_rect.left + toolbar.grip_rect.right) / 2.0;
                let gy = (toolbar.grip_rect.top + toolbar.grip_rect.bottom) / 2.0;
                let col1_x = gx - 2.5;
                let col2_x = gx + 2.5;
                for row in [-7.0, 0.0, 7.0] {
                    let dot1 = D2D1_ELLIPSE { point: v2(col1_x, gy + row), radiusX: 1.5, radiusY: 1.5 };
                    let dot2 = D2D1_ELLIPSE { point: v2(col2_x, gy + row), radiusX: 1.5, radiusY: 1.5 };
                    rt.FillEllipse(&dot1, &grip_brush);
                    rt.FillEllipse(&dot2, &grip_brush);
                }
            }

            // If collapsed:
            if toolbar.collapsed {
                let is_hover = toolbar.hover_action == Some(FluentAction::ToggleCollapse);
                if is_hover {
                    if let Ok(hover_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.10 }, None) {
                        rt.FillRoundedRectangle(&main_rrect, &hover_brush);
                    }
                }
                if let Ok(txt_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.92, g: 0.94, b: 0.98, a: 1.0 }, None) {
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
            let active_bg_fill = D2D1_COLOR_F { r: 0.0, g: 0.47, b: 0.83, a: 0.38 };
            let active_border_col = D2D1_COLOR_F { r: 0.38, g: 0.72, b: 0.98, a: 0.90 };
            let hover_fill = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.09 };
            let divider_col = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.18 };
            let text_col = D2D1_COLOR_F { r: 0.95, g: 0.96, b: 0.98, a: 1.0 };
            let text_dim = D2D1_COLOR_F { r: 0.70, g: 0.72, b: 0.76, a: 1.0 };

            let active_bg_brush = rt.CreateSolidColorBrush(&active_bg_fill, None).ok();
            let active_border_brush = rt.CreateSolidColorBrush(&active_border_col, None).ok();
            let hover_brush = rt.CreateSolidColorBrush(&hover_fill, None).ok();
            let divider_brush = rt.CreateSolidColorBrush(&divider_col, None).ok();
            let text_brush = rt.CreateSolidColorBrush(&text_col, None).ok();
            let dim_text_brush = rt.CreateSolidColorBrush(&text_dim, None).ok();

            // 4. Render items
            let mut tooltip_text: Option<(&'static str, D2D_RECT_F)> = None;

            for item in &toolbar.items {
                let is_hover = toolbar.hover_action == Some(item.action);

                // Determine active state
                let is_active = match item.action {
                    FluentAction::ModeZoom => mode == AppMode::StaticZoom && !spotlight.active,
                    FluentAction::ModeDraw => mode == AppMode::Draw,
                    FluentAction::ModeSpotlight => spotlight.active,
                    FluentAction::ModeTimer => mode == AppMode::Timer,
                    FluentAction::ModeSnip => mode == AppMode::Snip,
                    FluentAction::Tool(t) => tool == t && mode == AppMode::Draw,
                    FluentAction::Color(c) => color == c,
                    _ => false,
                };

                let is_color_item = matches!(item.action, FluentAction::Color(_));

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
                    } else if is_hover {
                        if let Some(b) = &hover_brush {
                            rt.FillRoundedRectangle(&btn_rrect, b);
                        }
                    }
                }

                // Draw button content
                match item.action {
                    FluentAction::Color(c) => {
                        let dot_center = v2(
                            (item.rect.left + item.rect.right) / 2.0,
                            (item.rect.top + item.rect.bottom) / 2.0,
                        );

                        // Subtle circular hover backdrop
                        if is_hover && !is_active {
                            if let Ok(hb) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 }, None) {
                                let halo = D2D1_ELLIPSE { point: dot_center, radiusX: 11.0, radiusY: 11.0 };
                                rt.FillEllipse(&halo, &hb);
                            }
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
                                D2D1_COLOR_F { r: 0.38, g: 0.72, b: 0.98, a: 1.0 }
                            } else {
                                D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 }
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
                            FluentAction::ModeSnip => ("\u{F406}", "Snip (Ctrl+Shift+S)"),
                            FluentAction::CycleDisplay => ("\u{E7F4}", "Switch Display (Ctrl+Tab)"),
                            FluentAction::Tool(DrawTool::Pen) => ("\u{ED63}", "Pen (P)"),
                            FluentAction::Tool(DrawTool::LaserPointer) => ("\u{EA3A}", "Laser Pointer (K)"),
                            FluentAction::Tool(DrawTool::Highlighter) => ("\u{E7E6}", "Highlighter (H)"),
                            FluentAction::Tool(DrawTool::Eraser) => ("\u{E75C}", "Eraser (X)"),
                            FluentAction::Tool(DrawTool::Arrow) => ("\u{E72A}", "Arrow (A)"),
                            FluentAction::Tool(DrawTool::Line) => ("\u{E790}", "Line (L)"),
                            FluentAction::Tool(DrawTool::Rectangle) => ("\u{E771}", "Rectangle (R)"),
                            FluentAction::Tool(DrawTool::Ellipse) => ("\u{EA3B}", "Ellipse (E)"),
                            FluentAction::Tool(DrawTool::StepBadge) => ("\u{E8EC}", "Step Badge (N)"),
                            FluentAction::Tool(DrawTool::Text) => ("\u{E8D2}", "Text (T)"),
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
                            let tb = if is_active || is_hover { &text_brush } else { &dim_text_brush };
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
                            tooltip_text = Some((tip, item.rect));
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
            let mut subbar_tooltip: Option<(&'static str, D2D_RECT_F)> = None;
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
                if let Ok(sb_sh_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.30 }, None) {
                    rt.FillRoundedRectangle(&sb_shadow, &sb_sh_brush);
                }

                // Subbar base surface
                let sb_main = D2D1_ROUNDED_RECT {
                    rect: sb_rect,
                    radiusX: 8.0,
                    radiusY: 8.0,
                };
                if let Ok(sb_bg) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.13, g: 0.14, b: 0.18, a: 0.94 }, None) {
                    rt.FillRoundedRectangle(&sb_main, &sb_bg);
                }
                if let Ok(sb_border) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 }, None) {
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
                        FluentAction::SetFontSize(sz) => (toolbar.current_font_size - sz).abs() < 0.5,
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
                    } else if is_hover {
                        if let Some(b) = &hover_brush {
                            rt.FillRoundedRectangle(&item_rrect, b);
                        }
                    }

                    let (label, tip): (&str, &'static str) = match s_item.action {
                        FluentAction::SetStrokeWidth(w) => {
                            if (w - 2.0).abs() < 0.1 { ("2px", "Fine stroke: 2px") }
                            else if (w - 4.0).abs() < 0.1 { ("4px", "Medium stroke: 4px") }
                            else if (w - 8.0).abs() < 0.1 { ("8px", "Thick stroke: 8px") }
                            else { ("14px", "Heavy stroke: 14px") }
                        }
                        FluentAction::SetFillMode(fm) => {
                            match fm {
                                FillMode::None => ("Outline", "Outline wireframe (F)"),
                                FillMode::Tinted => ("Tint", "Tinted highlight fill (F)"),
                                FillMode::Solid => ("Solid", "Solid block fill (F)"),
                            }
                        }
                        FluentAction::SetStrokePattern(sp) => {
                            match sp {
                                StrokePattern::Solid => ("──", "Solid line pattern (D)"),
                                StrokePattern::Dashed => ("- -", "Dashed line pattern (D)"),
                                StrokePattern::Dotted => ("···", "Dotted line pattern (D)"),
                            }
                        }
                        FluentAction::SetArrowStyle(as_) => {
                            match as_ {
                                ArrowStyle::Single => ("──►", "Single arrow pointer"),
                                ArrowStyle::Double => ("◄──►", "Double-ended arrow"),
                                ArrowStyle::Dimension => ("|◄►|", "Dimension callout line"),
                            }
                        }
                        FluentAction::SetBadgeSize(bs) => {
                            match bs {
                                BadgeSize::Small => ("S", "Small badge (14px) [ or ]"),
                                BadgeSize::Medium => ("M", "Medium badge (18px) [ or ]"),
                                BadgeSize::Large => ("L", "Large badge (24px) [ or ]"),
                                BadgeSize::ExtraLarge => ("XL", "Extra large badge (30px) [ or ]"),
                            }
                        }
                        FluentAction::SetBadgeShape(bsh) => {
                            match bsh {
                                BadgeShape::Circle => ("●", "Circle badge"),
                                BadgeShape::Square => ("■", "Square badge"),
                                BadgeShape::Hexagon => ("⬡", "Hexagon badge"),
                            }
                        }
                        FluentAction::ResetBadgeCounter => {
                            ("↺ #1", "Reset badge counter to #1 (R or 0)")
                        }
                        FluentAction::SetFontSize(sz) => {
                            if (sz - 14.0).abs() < 0.5 { ("14px", "Small font: 14px ([ or ])") }
                            else if (sz - 20.0).abs() < 0.5 { ("20px", "Medium font: 20px ([ or ])") }
                            else if (sz - 28.0).abs() < 0.5 { ("28px", "Heading font: 28px ([ or ])") }
                            else { ("38px", "Title font: 38px ([ or ])") }
                        }
                        FluentAction::ToggleBold => ("B", "Toggle Bold (Ctrl+B)"),
                        FluentAction::ToggleItalic => ("I", "Toggle Italic (Ctrl+I)"),
                        FluentAction::SetTextCardStyle(cs) => {
                            match cs {
                                TextCardStyle::Transparent => ("None", "Transparent floating text"),
                                TextCardStyle::Badge => ("Badge", "Translucent acrylic pill badge"),
                                TextCardStyle::Solid => ("Card", "Solid callout card with border"),
                            }
                        }
                        FluentAction::SetFontFamily(ff) => {
                            match ff {
                                TextFontFamily::SegoeUI => ("Sans", "Segoe UI (Fluent Interface)"),
                                TextFontFamily::CascadiaCode => ("Mono", "Cascadia Code (Monospace)"),
                                TextFontFamily::SegoePrint => ("Hand", "Segoe Print (Handwriting)"),
                            }
                        }
                        _ => ("", ""),
                    };

                    if is_hover && !tip.is_empty() {
                        subbar_tooltip = Some((tip, s_item.rect));
                    }

                    if !label.is_empty() {
                        if let Some(tb) = &text_brush {
                            let l_utf16: Vec<u16> = label.encode_utf16().collect();
                            let centered = self.text_format_toolbar_small.clone();
                            let _ = centered.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                            let _ = centered.SetParagraphAlignment(windows::Win32::Graphics::DirectWrite::DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                            let _ = centered.SetWordWrapping(windows::Win32::Graphics::DirectWrite::DWRITE_WORD_WRAPPING_NO_WRAP);
                            let tr = D2D_RECT_F {
                                left: s_item.rect.left,
                                top: s_item.rect.top,
                                right: s_item.rect.right,
                                bottom: s_item.rect.bottom,
                            };
                            rt.DrawText(&l_utf16, &centered, &tr, tb, D2D1_DRAW_TEXT_OPTIONS_NONE, windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL);
                        }
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
                if let Ok(tip_bg) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.15, g: 0.16, b: 0.20, a: 0.95 }, None) {
                    rt.FillRoundedRectangle(&tip_rrect, &tip_bg);
                }
                if let Ok(tip_border) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.18 }, None) {
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

    unsafe fn render_toast(&self, rt: &ID2D1RenderTarget, screen_w: f32, _screen_h: f32, toast: &ToastNotification, toolbar: &FluentToolbarState) {
        unsafe {
            let opacity = toast.opacity();
            if opacity <= 0.01 {
                return;
            }

            let toast_w = (toast.message.len() as f32 * 8.0 + 72.0).clamp(200.0, 480.0);
            let toast_h = 40.0;
            let toast_x = screen_w - toast_w - 24.0;
            let toast_y = if toolbar.visible && toolbar.bar_rect.bottom > toolbar.bar_rect.top {
                toolbar.bar_rect.top + (toolbar.bar_rect.bottom - toolbar.bar_rect.top - toast_h) / 2.0
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
            if let Ok(shadow_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.40 * opacity }, None) {
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
            let bg_col = D2D1_COLOR_F { r: 0.10, g: 0.11, b: 0.15, a: 0.95 * opacity };
            let border_col = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.16 * opacity };

            if let Ok(bg_brush) = rt.CreateSolidColorBrush(&bg_col, None) {
                rt.FillRoundedRectangle(&main_rrect, &bg_brush);
            }
            if let Ok(border_brush) = rt.CreateSolidColorBrush(&border_col, None) {
                rt.DrawRoundedRectangle(&main_rrect, &border_brush, 1.0, None);
            }

            // 3. Left circular icon badge
            let badge_center = v2(toast_x + 22.0, toast_y + 22.0);
            let badge_bg_col = D2D1_COLOR_F { r: 0.0, g: 0.47, b: 0.83, a: 0.35 * opacity };
            if let Ok(badge_brush) = rt.CreateSolidColorBrush(&badge_bg_col, None) {
                let badge_el = D2D1_ELLIPSE { point: badge_center, radiusX: 13.0, radiusY: 13.0 };
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
            if let Ok(white_brush) = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: opacity }, None) {
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

            let text_col = D2D1_COLOR_F { r: 0.96, g: 0.97, b: 0.99, a: opacity };
            let sub_col = D2D1_COLOR_F { r: 0.74, g: 0.78, b: 0.85, a: 0.90 * opacity };

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
