mod shapes;
mod ui_hud;
mod ui_timer;
mod ui_toolbar;

use std::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_CAP_STYLE_ROUND,
    D2D1_DASH_STYLE_DASH, D2D1_DASH_STYLE_DOT, D2D1_DASH_STYLE_SOLID,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_LINE_JOIN_ROUND,
    D2D1_PRESENT_OPTIONS_IMMEDIATELY, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE, D2D1CreateFactory, ID2D1Bitmap, ID2D1Factory,
    ID2D1GeometryGroup, ID2D1HwndRenderTarget, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GetDC, RGBQUAD, ReleaseDC, SelectObject,
};
use windows::core::{Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::capture::ScreenCapture;
use crate::types::{
    AppMode, CanvasBackground, ColorPreset, DrawTool, FluentToolbarState, LaserRipple,
    LaserTrailPoint, Point2D, Shape, SpotlightState, StrokePattern, TextEditorState,
    TextFontFamily, TimerWidgetState, ToastNotification, ZoomState,
};

#[inline]
pub(crate) fn v2(x: f32, y: f32) -> Vector2 {
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
            let _ =
                text_format_toolbar_small.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let text_format_fluent_icons = dwrite_factory
                .CreateTextFormat(
                    w!("Segoe Fluent Icons"),
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    14.0,
                    w!("en-us"),
                )
                .or_else(|_| {
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
            let _ =
                text_format_fluent_icons.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let text_format_toast_title = dwrite_factory
                .CreateTextFormat(
                    w!("Segoe UI Variable Display"),
                    None,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    13.5,
                    w!("en-us"),
                )
                .or_else(|_| {
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
            let _ =
                text_format_toast_title.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let text_format_toast_sub = dwrite_factory
                .CreateTextFormat(
                    w!("Segoe UI Variable Text"),
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    11.0,
                    w!("en-us"),
                )
                .or_else(|_| {
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
        let weight = if is_bold {
            DWRITE_FONT_WEIGHT_BOLD
        } else {
            DWRITE_FONT_WEIGHT_SEMI_BOLD
        };
        let style = if is_italic {
            DWRITE_FONT_STYLE_ITALIC
        } else {
            DWRITE_FONT_STYLE_NORMAL
        };

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

            let rt = self
                .factory
                .CreateHwndRenderTarget(&rt_props, &hwnd_props)?;
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

    #[allow(clippy::too_many_arguments)]
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
        laser_ripples: &[LaserRipple],
        laser_pos: Option<Point2D>,
        eraser_pos: Option<Point2D>,
        snap_guides: bool,
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
                CanvasBackground::Transparent => D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                },
                CanvasBackground::Whiteboard => D2D1_COLOR_F {
                    r: 0.98,
                    g: 0.98,
                    b: 0.99,
                    a: 1.0,
                },
                CanvasBackground::Blackboard => D2D1_COLOR_F {
                    r: 0.11,
                    g: 0.12,
                    b: 0.14,
                    a: 1.0,
                },
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

            if bg_type == CanvasBackground::Transparent
                && let Some(bitmap) = bg_bitmap
            {
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

            // ── Shapes Layer (Zoomed with canvas) ──
            for shape in shapes {
                self.render_single_shape(rt, shape, bg_bitmap);
            }

            if let Some(shape) = active_shape {
                self.render_single_shape(rt, shape, bg_bitmap);
                if snap_guides {
                    self.render_drawing_snap_guides(rt, shape);
                }
            }

            if let Some(editor) = text_input {
                let show_caret = (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    / 500)
                    .is_multiple_of(2);
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
            if !laser_trail.is_empty() || !laser_ripples.is_empty() || laser_pos.is_some() {
                self.render_laser_pointer(rt, laser_trail, laser_ripples, laser_pos, current_color);
            }

            // ── Screen Space Layer (Timer + Eraser + HUD + Toast + Modal) ──
            rt.SetTransform(&identity);

            // ── Eraser Cursor Indicator ──
            if current_tool == DrawTool::Eraser
                && let Some(epos) = eraser_pos
            {
                self.render_eraser_indicator(rt, epos);
            }

            // If in Timer mode, draw background dim overlay over desktop slides
            if mode == AppMode::Timer {
                let dim_val = timer_widget.dim_opacity.clamp(0.0, 0.95);
                let dim_col = D2D1_COLOR_F {
                    r: 0.02,
                    g: 0.03,
                    b: 0.05,
                    a: dim_val,
                };
                if let Ok(dim_brush) = rt.CreateSolidColorBrush(&dim_col, None) {
                    let full_rect = D2D_RECT_F {
                        left: 0.0,
                        top: 0.0,
                        right: width,
                        bottom: height,
                    };
                    rt.FillRectangle(&full_rect, &dim_brush);
                }
            }

            if let Some((mins, secs, progress, paused, is_overtime)) = timer_info {
                self.render_countdown_timer(
                    rt,
                    width,
                    height,
                    mins,
                    secs,
                    progress,
                    paused,
                    is_overtime,
                    timer_widget,
                );
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

            if let Some(t) = toast
                && !t.is_expired()
            {
                self.render_toast(rt, width, height, t, toolbar);
            }

            if show_cheat_sheet {
                self.render_cheat_sheet_modal(rt, width, height);
            }

            let _ = rt.EndDraw(None, None);
        }
    }

    #[allow(clippy::too_many_arguments)]
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
            let hbitmap =
                CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0);

            if let Ok(hbm) = hbitmap
                && !hbm.is_invalid()
                && !bits_ptr.is_null()
            {
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
                            CanvasBackground::Transparent => D2D1_COLOR_F {
                                r: 0.0,
                                g: 0.0,
                                b: 0.0,
                                a: 1.0,
                            },
                            CanvasBackground::Whiteboard => D2D1_COLOR_F {
                                r: 0.98,
                                g: 0.98,
                                b: 0.99,
                                a: 1.0,
                            },
                            CanvasBackground::Blackboard => D2D1_COLOR_F {
                                r: 0.11,
                                g: 0.12,
                                b: 0.14,
                                a: 1.0,
                            },
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
                                M11: 1.0,
                                M12: 0.0,
                                M21: 0.0,
                                M22: 1.0,
                                M31: 0.0,
                                M32: 0.0,
                            }
                        };
                        let identity = Matrix3x2 {
                            M11: 1.0,
                            M12: 0.0,
                            M21: 0.0,
                            M22: 1.0,
                            M31: 0.0,
                            M32: 0.0,
                        };

                        dc_rt.SetTransform(&canvas_matrix);

                        let bg_bmp = if bg_type == CanvasBackground::Transparent
                            && let Some(pixels) = bg_pixels
                        {
                            let size = windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U {
                                width,
                                height,
                            };
                            let props = windows::Win32::Graphics::Direct2D::D2D1_BITMAP_PROPERTIES {
                                pixelFormat: D2D1_PIXEL_FORMAT {
                                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                                    alphaMode: windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_IGNORE,
                                },
                                dpiX: 96.0,
                                dpiY: 96.0,
                            };
                            dc_rt
                                .CreateBitmap(
                                    size,
                                    Some(pixels.as_ptr() as *const std::ffi::c_void),
                                    width * 4,
                                    &props,
                                )
                                .ok()
                        } else {
                            None
                        };

                        if let Some(bmp) = &bg_bmp {
                            let dst_rect = D2D_RECT_F {
                                left: 0.0,
                                top: 0.0,
                                right: width as f32,
                                bottom: height as f32,
                            };
                            dc_rt.DrawBitmap(
                                bmp,
                                Some(&dst_rect),
                                1.0,
                                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                None,
                            );
                        }

                        for shape in shapes {
                            self.render_single_shape(&dc_rt, shape, bg_bmp.as_ref());
                        }

                        if let Some(shape) = active_shape {
                            self.render_single_shape(&dc_rt, shape, bg_bmp.as_ref());
                        }

                        if let Some(editor) = text_input {
                            self.render_text_editor(&dc_rt, editor, false);
                        }

                        if include_spotlight && spotlight.active {
                            let screen_pt =
                                zoom_state.canvas_to_screen(Point2D::new(spotlight.x, spotlight.y));
                            dc_rt.SetTransform(&identity);
                            self.render_spotlight_mask(
                                &dc_rt,
                                width as f32,
                                height as f32,
                                screen_pt.x,
                                screen_pt.y,
                                spotlight.radius,
                                spotlight.dim_opacity,
                                spotlight.pinned,
                            );
                            dc_rt.SetTransform(&canvas_matrix);
                        }

                        let _ = dc_rt.EndDraw(None, None);

                        let total_bytes = (width * height * 4) as usize;
                        let mut pixels = vec![0u8; total_bytes];
                        std::ptr::copy_nonoverlapping(
                            bits_ptr as *const u8,
                            pixels.as_mut_ptr(),
                            total_bytes,
                        );

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

            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, screen_dc);
            None
        }
    }
}
