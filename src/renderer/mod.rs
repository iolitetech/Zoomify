mod shapes;
mod ui_hud;
mod ui_loupe;
mod ui_minimap;
mod ui_picker;
mod ui_timer;
mod ui_toolbar;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
    D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_BITMAP_PROPERTIES, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_DASH, D2D1_DASH_STYLE_DOT,
    D2D1_DASH_STYLE_SOLID, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_LINE_JOIN_ROUND, D2D1_PRESENT_OPTIONS_IMMEDIATELY, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE, D2D1CreateFactory, ID2D1Bitmap, ID2D1BitmapBrush,
    ID2D1BitmapRenderTarget, ID2D1Factory, ID2D1GeometryGroup, ID2D1HwndRenderTarget,
    ID2D1PathGeometry, ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_METRICS, DWriteCreateFactory, IDWriteFactory,
    IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GetDC, RGBQUAD, ReleaseDC, SelectObject,
};
use windows::core::{Interface, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::capture::ScreenCapture;
use crate::types::{
    Annotation, AppMode, CanvasBackground, ColorPickerState, ColorPreset, DrawTool,
    FluentToolbarState, LaserRipple, LaserTrailPoint, LoupeState, MinimapState, Point2D, Shape,
    SpotlightState, StrokePattern, TextEditorState, TextFontFamily, TimerWidgetState,
    ToastNotification, ZoomState,
};

#[inline]
pub(crate) fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

/// Same reasoning as `shapes::GEOMETRY_CACHE_MAX_ENTRIES`: unbounded growth
/// during a long text edit (a new entry per keystroke while content is still
/// changing) would otherwise never give anything back.
const TEXT_LAYOUT_CACHE_MAX_ENTRIES: usize = 256;

/// Every input the HUD status line's text depends on, discretized to exact
/// (`Eq`-able) values matching what actually reaches the displayed string -
/// so equality here really does mean "the text would come out identical".
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct HudTextKey {
    mode: AppMode,
    tool: DrawTool,
    stroke_width: u32,
    zoom_tenths: Option<i32>,
    spot_diameter: Option<u32>,
    bg_type: CanvasBackground,
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
    /// Same font as `text_format_hud`, pre-centered, as its own instance -
    /// several Timer-mode glyphs used to clone() text_format_hud (a COM
    /// AddRef, the same underlying object) and call SetTextAlignment on
    /// that "clone" directly, which permanently centered every other HUD
    /// text using the shared field too.
    pub text_format_hud_centered: IDWriteTextFormat,
    pub text_format_cheat_title: IDWriteTextFormat,
    pub text_format_cheat_item: IDWriteTextFormat,
    #[allow(dead_code)]
    pub text_format_toolbar: IDWriteTextFormat,
    pub text_format_toolbar_small: IDWriteTextFormat,
    /// Same font as `text_format_toolbar_small`, but with word-wrapping
    /// disabled - kept as its own instance rather than mutated in place on
    /// demand, since IDWriteTextFormat::clone() is a COM AddRef (the same
    /// underlying object, not a copy): setting NO_WRAP on a "clone" of
    /// `text_format_toolbar_small` for the sub-bar's single-line labels used
    /// to permanently switch every other user of that shared field to
    /// NO_WRAP too (tooltips, toast, loupe badge, snap badges).
    pub text_format_toolbar_small_nowrap: IDWriteTextFormat,
    pub text_format_fluent_icons: IDWriteTextFormat,
    pub text_format_toast_title: IDWriteTextFormat,
    pub text_format_toast_sub: IDWriteTextFormat,
    pub text_formats_cache: RefCell<HashMap<u32, IDWriteTextFormat>>,
    pub spotlight_geometry_cache: RefCell<Option<(u32, ID2D1GeometryGroup)>>,
    /// Solid brushes keyed by packed RGBA, alongside the render target they
    /// belong to. Brushes are device resources, so the cache is dropped whenever
    /// the target changes (the offscreen target used for export is a different
    /// one) or the device is lost.
    solid_brush_cache: RefCell<(usize, HashMap<u32, ID2D1SolidColorBrush>)>,
    /// One reusable brush for elements whose colour/alpha changes
    /// continuously frame to frame (the laser trail, its ripples, toast
    /// fades) - repainted via SetColor before each draw rather than
    /// inserted into `solid_brush_cache` under a new key for every alpha a
    /// fade passes through, which used to flood that 256-entry cache
    /// (shared with every on-canvas shape) and force repeated full
    /// flushes for as long as the fade lasted. D2D brush state (colour,
    /// opacity) is captured at the moment each draw call is issued, so
    /// reusing one brush this way is standard and safe, not a race.
    scratch_brush: RefCell<(usize, Option<ID2D1SolidColorBrush>)>,
    /// The HUD status line's last-built UTF-16 text plus the discretized
    /// state it was built from - not device-dependent, so it survives device
    /// loss. The HUD redraws every frame regardless (it sits over whatever's
    /// under it), but this skips re-running `format!`/`encode_utf16` on the
    /// vast majority of frames where nothing it displays actually changed.
    hud_text_cache: RefCell<Option<(HudTextKey, Vec<u16>)>>,
    /// The loupe's magnifying bitmap brush, rebuilt only when the render
    /// target or the background bitmap it wraps changes - every other frame
    /// just updates its transform, which is the only part that actually
    /// varies as the loupe follows the cursor.
    loupe_brush_cache: RefCell<(usize, usize, Option<ID2D1BitmapBrush>)>,
    /// `IDWriteTextLayout`s (plus their measured (width, height)), keyed by
    /// content - not by render target: unlike a D2D brush/geometry, a
    /// DirectWrite layout is a CPU-side object with no device dependency, so
    /// this survives export (a different render target) and device loss
    /// alike, and needs no clearing in `recover_if_device_lost`. This is
    /// what makes measuring and drawing a `Shape::Text` share one
    /// `CreateTextLayout` instead of each frame paying for two (measure,
    /// then `DrawText`'s own internal layout).
    text_layout_cache: RefCell<HashMap<u64, (IDWriteTextLayout, f32, f32)>>,
    /// Finished blur mosaics, keyed by content (background bitmap identity +
    /// rect + block size) rather than shared by size the way this used to
    /// be - see the doc comment at its use in shapes.rs.
    pub blur_mosaic_cache: RefCell<(usize, HashMap<u64, ID2D1BitmapRenderTarget>)>,
    pub geometry_cache: RefCell<(usize, HashMap<u64, ID2D1PathGeometry>)>,
    /// Pasted images, keyed by content, alongside the render target they were
    /// uploaded to. Re-uploading a full-screen paste every frame would be the
    /// single most expensive thing the renderer does.
    image_cache: RefCell<(usize, HashMap<u64, ID2D1Bitmap>)>,
    /// Window + size the HWND render target was built for, so it can be rebuilt
    /// after the GPU device is lost (driver reset/TDR, RDP transition, mode change).
    target_hwnd: HWND,
    target_width: u32,
    target_height: u32,
    /// Display density the target is configured for (96 = 100%).
    dpi: f32,
    /// Set when Direct2D reports D2DERR_RECREATE_TARGET. Rendering is a `&self`
    /// path, so the flag is drained afterwards by `recover_if_device_lost`.
    device_lost: Cell<bool>,
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
            let text_format_hud_centered = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                14.0,
                w!("en-us"),
            )?;
            let _ = text_format_hud_centered.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);

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

            let text_format_toolbar_small_nowrap = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                12.0,
                w!("en-us"),
            )?;
            let _ = text_format_toolbar_small_nowrap.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = text_format_toolbar_small_nowrap
                .SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = text_format_toolbar_small_nowrap.SetWordWrapping(
                windows::Win32::Graphics::DirectWrite::DWRITE_WORD_WRAPPING_NO_WRAP,
            );

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
                text_format_hud_centered,
                text_format_cheat_title,
                text_format_cheat_item,
                text_format_toolbar,
                text_format_toolbar_small,
                text_format_toolbar_small_nowrap,
                text_format_fluent_icons,
                text_format_toast_title,
                text_format_toast_sub,
                text_formats_cache: RefCell::new(HashMap::new()),
                spotlight_geometry_cache: RefCell::new(None),
                solid_brush_cache: RefCell::new((0, HashMap::new())),
                scratch_brush: RefCell::new((0, None)),
                hud_text_cache: RefCell::new(None),
                loupe_brush_cache: RefCell::new((0, 0, None)),
                text_layout_cache: RefCell::new(HashMap::new()),
                blur_mosaic_cache: RefCell::new((0, HashMap::new())),
                geometry_cache: RefCell::new((0, HashMap::new())),
                image_cache: RefCell::new((0, HashMap::new())),
                target_hwnd: HWND::default(),
                target_width: 0,
                target_height: 0,
                dpi: 96.0,
                device_lost: Cell::new(false),
            })
        }
    }

    /// A solid brush for `color`, reused across shapes and frames.
    ///
    /// Every shape used to allocate its brushes from scratch on every frame,
    /// which is a COM allocation per shape per color at 60 Hz.
    pub(crate) fn solid_brush(
        &self,
        rt: &ID2D1RenderTarget,
        color: &D2D1_COLOR_F,
    ) -> Option<ID2D1SolidColorBrush> {
        #[inline]
        fn chan(v: f32) -> u32 {
            (v.clamp(0.0, 1.0) * 255.0).round() as u32
        }
        let key =
            (chan(color.r) << 24) | (chan(color.g) << 16) | (chan(color.b) << 8) | chan(color.a);

        let rt_id = rt.as_raw() as usize;
        let mut cache = self.solid_brush_cache.borrow_mut();
        if cache.0 != rt_id {
            // Different render target: its brushes are not usable here.
            cache.0 = rt_id;
            cache.1.clear();
        }
        if let Some(b) = cache.1.get(&key) {
            return Some(b.clone());
        }

        let brush = unsafe { rt.CreateSolidColorBrush(color, None) }.ok()?;
        if cache.1.len() > 256 {
            cache.1.clear();
        }
        cache.1.insert(key, brush.clone());
        Some(brush)
    }

    /// See the `scratch_brush` field's doc comment. Only for an element
    /// whose colour is expected to change every draw anyway (a fade, a
    /// continuously-decaying alpha) - never for a shape whose colour is
    /// reused across many frames, which belongs in `solid_brush`'s cache.
    pub(crate) fn scratch_brush(
        &self,
        rt: &ID2D1RenderTarget,
        color: &D2D1_COLOR_F,
    ) -> Option<ID2D1SolidColorBrush> {
        let rt_id = rt.as_raw() as usize;
        let mut slot = self.scratch_brush.borrow_mut();
        if slot.0 != rt_id || slot.1.is_none() {
            slot.0 = rt_id;
            slot.1 = unsafe { rt.CreateSolidColorBrush(color, None) }.ok();
        } else if let Some(b) = &slot.1 {
            unsafe { b.SetColor(color) };
        }
        slot.1.clone()
    }

    pub fn get_stroke_style(&self, pattern: StrokePattern) -> &ID2D1StrokeStyle {
        match pattern {
            StrokePattern::Solid => &self.round_stroke_style,
            StrokePattern::Dashed => &self.dashed_stroke_style,
            StrokePattern::Dotted => &self.dotted_stroke_style,
        }
    }

    /// Bold, SegoeUI, pre-centered (both text and paragraph alignment) - for
    /// the many small on-screen labels (timer digits, step badges) that
    /// always want centered text. Deliberately a *different* cache key than
    /// `get_custom_text_format`'s equivalent left-aligned request (see
    /// `get_custom_text_format_impl`), so callers here can never bleed their
    /// alignment into a Shape::Text annotation that happens to share the
    /// same size.
    /// The largest a single dimension of a bitmap this device can create is
    /// allowed to be. Used both to clamp an export's supersample factor and
    /// to downscale an oversized pasted image before it ever becomes a
    /// Shape::Image - CreateBitmap failing past this (image_bitmap, below)
    /// otherwise left the image on the canvas, selectable, but rendering as
    /// nothing at all.
    pub fn max_bitmap_size(&self) -> u32 {
        self.render_target
            .as_ref()
            .map(|rt| unsafe { rt.GetMaximumBitmapSize() })
            .unwrap_or(16384)
    }

    pub fn get_text_format(&self, font_size: f32) -> Result<IDWriteTextFormat> {
        self.get_custom_text_format_impl(font_size, true, false, TextFontFamily::SegoeUI, true)
    }

    /// Upload a pasted image once and hand back the cached bitmap.
    ///
    /// Keyed by content and by the render target it belongs to: bitmaps are
    /// device resources, and the offscreen target used for export is a
    /// different device from the window's.
    fn image_bitmap(
        &self,
        rt: &ID2D1RenderTarget,
        pixels: &crate::types::ImagePixels,
    ) -> Option<ID2D1Bitmap> {
        // Nothing evicts a single deleted/undone image's entry, so without
        // some bound a long session of pasting many different images would
        // keep every one of them - each a full-size GPU bitmap - alive for
        // the rest of the process. A typical session pastes a handful of
        // images at most, so clearing the whole cache past this is a rare
        // one-off re-upload, not a steady-state cost.
        const IMAGE_CACHE_MAX_ENTRIES: usize = 64;

        unsafe {
            let target_key = rt.as_raw() as usize;
            let mut cache = self.image_cache.borrow_mut();
            if cache.0 != target_key {
                cache.0 = target_key;
                cache.1.clear();
            }
            let key = pixels.cache_key();
            if let Some(b) = cache.1.get(&key) {
                return Some(b.clone());
            }
            let size = D2D_SIZE_U {
                width: pixels.width,
                height: pixels.height,
            };
            let props = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 96.0,
                dpiY: 96.0,
            };
            let bitmap = rt
                .CreateBitmap(
                    size,
                    Some(pixels.bgra.as_ptr() as *const _),
                    pixels.width * 4,
                    &props,
                )
                .ok()?;
            if cache.1.len() > IMAGE_CACHE_MAX_ENTRIES {
                cache.1.clear();
            }
            cache.1.insert(key, bitmap.clone());
            Some(bitmap)
        }
    }

    /// Left-aligned (DirectWrite's default) - what every `Shape::Text`
    /// annotation renders with. See `get_custom_text_format_impl` for why
    /// this needs its own cache key, distinct from `get_text_format`'s.
    pub fn get_custom_text_format(
        &self,
        font_size: f32,
        is_bold: bool,
        is_italic: bool,
        font_family: TextFontFamily,
    ) -> Result<IDWriteTextFormat> {
        self.get_custom_text_format_impl(font_size, is_bold, is_italic, font_family, false)
    }

    /// `centered` sets both text and paragraph alignment to CENTER at
    /// creation time and folds into the cache key, so a caller that wants
    /// centered text (badges, the timer) can never share a cache entry -
    /// and therefore never share underlying alignment state - with one that
    /// wants DirectWrite's default left alignment (any `Shape::Text`
    /// annotation). Before this existed, a StepBadge or the timer would
    /// fetch the *same* cached format a same-sized/weight/family
    /// Shape::Text used and call SetTextAlignment/SetParagraphAlignment on
    /// it directly - since the format is shared by reference, that flipped
    /// every other holder of it to centered too, permanently.
    fn get_custom_text_format_impl(
        &self,
        font_size: f32,
        is_bold: bool,
        is_italic: bool,
        font_family: TextFontFamily,
        centered: bool,
    ) -> Result<IDWriteTextFormat> {
        let sz = (font_size.round() as u32).clamp(8, 200);
        let b_flag = if is_bold { 1u32 << 16 } else { 0 };
        let i_flag = if is_italic { 1u32 << 17 } else { 0 };
        let f_flag = (font_family as u32) << 18;
        let c_flag = if centered { 1u32 << 20 } else { 0 };
        let key = sz | b_flag | i_flag | f_flag | c_flag;

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
            if centered {
                let _ = format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            }
            cache.insert(key, format.clone());
            Ok(format)
        }
    }

    /// Exact layout box for a (possibly multi-line) run of annotation text, in
    /// DIPs. Falls back to the character-count estimate if DirectWrite refuses
    /// to build a layout, so callers always get usable numbers.
    pub fn measure_text_block(
        &self,
        text: &str,
        font_size: f32,
        is_bold: bool,
        is_italic: bool,
        font_family: TextFontFamily,
        // Wrap at this width; f32::MAX for a single unbroken run per line.
        max_width: f32,
    ) -> (f32, f32) {
        match self.get_or_build_text_layout(
            text,
            font_size,
            is_bold,
            is_italic,
            font_family,
            max_width,
        ) {
            Some((_, w, h)) => (w, h),
            None => crate::types::measure_text_block(text, font_size),
        }
    }

    /// The cached `IDWriteTextLayout` behind `measure_text_block`, also
    /// available to draw from directly (`rt.DrawTextLayout`) so a
    /// `Shape::Text` never pays for a second, `DrawText`-internal layout on
    /// top of the one it was just measured with.
    ///
    /// Keyed on content, not identity: a moved/re-cloned annotation with the
    /// same text/font/wrap hits the same entry, same reasoning as the
    /// stroke/badge geometry caches.
    pub(crate) fn get_or_build_text_layout(
        &self,
        text: &str,
        font_size: f32,
        is_bold: bool,
        is_italic: bool,
        font_family: TextFontFamily,
        max_width: f32,
    ) -> Option<(IDWriteTextLayout, f32, f32)> {
        let wrap = if max_width.is_finite() {
            max_width.max(1.0)
        } else {
            f32::MAX / 4.0
        };
        let key = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            text.hash(&mut hasher);
            font_size.to_bits().hash(&mut hasher);
            is_bold.hash(&mut hasher);
            is_italic.hash(&mut hasher);
            (font_family as u32).hash(&mut hasher);
            wrap.to_bits().hash(&mut hasher);
            hasher.finish()
        };

        {
            let cache = self.text_layout_cache.borrow();
            if let Some((layout, w, h)) = cache.get(&key) {
                return Some((layout.clone(), *w, *h));
            }
        }

        let format = self
            .get_custom_text_format(font_size, is_bold, is_italic, font_family)
            .ok()?;
        let utf16: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let layout = self
                .dwrite_factory
                .CreateTextLayout(&utf16, &format, wrap, f32::MAX / 4.0)
                .ok()?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics).ok()?;
            // A trailing empty line has zero measured width but still needs a
            // row, and an all-empty buffer still needs a caret-tall box.
            let lines = metrics.lineCount.max(1) as f32;
            let h = metrics.height.max(lines * font_size * 1.25);
            let w = metrics.width.max(font_size * 0.6);

            let mut cache = self.text_layout_cache.borrow_mut();
            if cache.len() > TEXT_LAYOUT_CACHE_MAX_ENTRIES {
                cache.clear();
            }
            cache.insert(key, (layout.clone(), w, h));
            Some((layout, w, h))
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

            rt.SetDpi(self.dpi, self.dpi);

            self.render_target = Some(rt);
            self.target_hwnd = hwnd;
            self.target_width = width;
            self.target_height = height;
            self.device_lost.set(false);
            Ok(())
        }
    }

    /// Tell Direct2D the display density so drawing coordinates are treated as
    /// DIPs and scaled to physical pixels for us - fonts, strokes and geometry
    /// all included. Without this the whole UI renders at 1 DIP = 1 pixel and
    /// comes out at 80% size on a 125% display.
    pub fn set_dpi(&mut self, dpi: u32) {
        let dpi = (dpi.max(48) as f32).min(600.0);
        if (dpi - self.dpi).abs() < 0.5 {
            return;
        }
        self.dpi = dpi;
        if let Some(rt) = &self.render_target {
            unsafe { rt.SetDpi(dpi, dpi) };
        }
    }

    /// Rebuild the render target after a lost device. Returns true once a fresh
    /// target is live, meaning every device-dependent resource the caller owns
    /// (bitmaps, in particular) must be recreated from it.
    ///
    /// If recreation fails the lost flag stays set and the next frame retries;
    /// `render_frame` no-ops while there is no target, so this cannot spin.
    pub fn recover_if_device_lost(&mut self) -> bool {
        if !self.device_lost.get() {
            return false;
        }

        // Drop the dead target before asking the factory for a new one.
        self.render_target = None;
        self.spotlight_geometry_cache.borrow_mut().take();
        {
            let mut cache = self.solid_brush_cache.borrow_mut();
            cache.0 = 0;
            cache.1.clear();
        }
        self.scratch_brush.borrow_mut().1 = None;
        {
            let mut cache = self.loupe_brush_cache.borrow_mut();
            cache.0 = 0;
            cache.1 = 0;
            cache.2 = None;
        }
        {
            let mut cache = self.blur_mosaic_cache.borrow_mut();
            cache.0 = 0;
            cache.1.clear();
        }
        self.clear_geometry_cache();
        self.clear_image_cache();

        if self.target_hwnd.is_invalid() || self.target_width == 0 || self.target_height == 0 {
            self.device_lost.set(false);
            return false;
        }

        let (hwnd, w, h) = (self.target_hwnd, self.target_width, self.target_height);
        self.init_hwnd(hwnd, w, h).is_ok()
    }

    /// Drop every cached stroke/arrow-head path geometry. The cache has no
    /// per-entry eviction (it is bounded only by clearing itself outright
    /// once it grows past a cap - see `render_single_shape`), so this is the
    /// only way to reclaim it between overlay sessions rather than carrying
    /// every distinct geometry ever drawn for the life of the process.
    pub fn clear_geometry_cache(&self) {
        let mut cache = self.geometry_cache.borrow_mut();
        cache.0 = 0;
        cache.1.clear();
    }

    /// Drop every cached pasted-image GPU bitmap. Without this a bitmap
    /// belonging to a lost device stayed pinned in the cache, and if the
    /// freshly-created replacement render target happened to land at the
    /// same address (plausible - the old one is freed right before the new
    /// one is created), the stale key would keep matching and the dead
    /// bitmaps would be reused instead of ever being replaced.
    pub fn clear_image_cache(&self) {
        let mut cache = self.image_cache.borrow_mut();
        cache.0 = 0;
        cache.1.clear();
    }

    /// Flag a lost device if this is the HRESULT Direct2D uses to report one.
    fn note_draw_result(&self, result: windows::core::Result<()>) {
        if let Err(e) = result
            && e.code() == D2DERR_RECREATE_TARGET
        {
            self.device_lost.set(true);
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(rt) = &self.render_target {
            unsafe {
                let size = windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U { width, height };
                if let Err(e) = rt.Resize(&size) {
                    if e.code() == D2DERR_RECREATE_TARGET {
                        self.device_lost.set(true);
                    }
                    return;
                }
            }
        }
        self.target_width = width;
        self.target_height = height;
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
        loupe: &LoupeState,
        shapes: &[Annotation],
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
        minimap: &MinimapState,
        show_minimap: bool,
        color_picker: &ColorPickerState,
        selection_screen_bounds: Option<(f32, f32, f32, f32)>,
        snap_guides_screen: &[(f32, f32, f32, f32, bool)],
        marquee_screen: Option<(f32, f32, f32, f32)>,
        selection_endpoints: Option<((f32, f32), (f32, f32))>,
        selection_bow: Option<(f32, f32)>,
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

            let effective_bg_type =
                if mode == AppMode::Loupe || mode == AppMode::Spotlight || mode == AppMode::Timer {
                    CanvasBackground::Transparent
                } else {
                    bg_type
                };

            let clear_color = match effective_bg_type {
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

            // Draw mode also panned with no zoom at all (level == 1) is the
            // infinite-canvas gesture: dragging a whiteboard/blackboard past
            // the screen edge rather than zooming into a captured screen.
            // `z > 1.001` alone would miss that case since it stays 1.0.
            // Excluded for a live screen capture: it has nothing to show
            // past its own edge, so a leftover pan would just shift the
            // captured bitmap off its bounds instead of doing anything useful.
            let panning = mode == AppMode::Draw
                && effective_bg_type != CanvasBackground::Transparent
                && (zoom_state.view_x != 0.0 || zoom_state.view_y != 0.0);
            let z = if mode == AppMode::StaticZoom
                || (mode == AppMode::Draw && (zoom_state.level > 1.001 || panning))
            {
                zoom_state.level.max(1.0)
            } else {
                1.0
            };
            let canvas_matrix = if z > 1.001 || panning {
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

            if effective_bg_type == CanvasBackground::Transparent
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

            // ── Shapes Layer (Zoomed with canvas; Draw & StaticZoom only) ──
            if mode == AppMode::Draw || mode == AppMode::StaticZoom {
                // render_single_shape's only use of a background bitmap is
                // Shape::Blur's source to redact. On a Whiteboard/Blackboard
                // the frozen screenshot underneath is exactly what the user
                // covered the desktop to hide - handing it to Blur regardless
                // used to let it mosaic a pixelated copy of the hidden
                // desktop instead of the board itself.
                let blur_bg = if bg_type == CanvasBackground::Transparent {
                    bg_bitmap
                } else {
                    None
                };
                for a in shapes {
                    // A label is drawn from its container's bounds, so it has
                    // to wait until the container is on screen.
                    if a.container.is_some() {
                        continue;
                    }
                    self.render_single_shape(rt, &a.shape, blur_bg, a.opacity, true);
                }
                for a in shapes {
                    if let Some(cid) = a.container
                        && let Some(owner) = shapes.iter().find(|o| o.id == cid)
                    {
                        let bounds = crate::shapes::shape_bounds(&owner.shape);
                        let rides = crate::shapes::label_rides_on_shape(&owner.shape);
                        self.render_contained_text(rt, &a.shape, bounds, rides, a.opacity);
                    }
                }

                if let Some(shape) = active_shape {
                    // Not cacheable: this is the shape still being drawn (a pen
                    // stroke gaining a point every mouse move, a drag preview),
                    // so its geometry differs every frame - caching it would
                    // only ever insert, never hit.
                    self.render_single_shape(rt, shape, blur_bg, 1.0, false);
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
            }

            // ── Spotlight Mask (Screen Space for full edge-to-edge coverage) ──
            let show_spotlight = mode == AppMode::Spotlight
                || ((mode == AppMode::StaticZoom || mode == AppMode::Draw) && spotlight.active);
            if show_spotlight {
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

            // ── Laser Pointer (Zoomed with canvas; Draw & StaticZoom only) ──
            if (mode == AppMode::Draw || mode == AppMode::StaticZoom)
                && (!laser_trail.is_empty() || !laser_ripples.is_empty() || laser_pos.is_some())
            {
                self.render_laser_pointer(rt, laser_trail, laser_ripples, laser_pos, current_color);
            }

            // ── Screen Space Layer (Timer + Eraser + HUD + Toast + Modal) ──
            rt.SetTransform(&identity);

            // ── Snap guides (screen space, under the selection chrome) ──
            if (mode == AppMode::Draw || mode == AppMode::StaticZoom)
                && !snap_guides_screen.is_empty()
            {
                self.render_snap_guides(rt, snap_guides_screen);
            }

            if (mode == AppMode::Draw || mode == AppMode::StaticZoom)
                && let Some(m) = marquee_screen
            {
                self.render_marquee(rt, m);
            }

            // ── Selection chrome (screen space so grips keep their size) ──
            if mode == AppMode::Draw || mode == AppMode::StaticZoom {
                // A line offers its ends; everything else offers a box.
                if let Some((a, b)) = selection_endpoints {
                    self.render_endpoint_grips(rt, a, b, selection_bow);
                } else if let Some(sb) = selection_screen_bounds {
                    self.render_selection(rt, sb);
                }
            }

            // ── Eraser Cursor Indicator ──
            if (mode == AppMode::Draw || mode == AppMode::StaticZoom)
                && current_tool == DrawTool::Eraser
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
                if let Some(dim_brush) = self.solid_brush(rt, &dim_col) {
                    let full_rect = D2D_RECT_F {
                        left: 0.0,
                        top: 0.0,
                        right: width,
                        bottom: height,
                    };
                    rt.FillRectangle(&full_rect, &dim_brush);
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
            }

            // ── Floating Magnifier Loupe Lens ──
            if mode == AppMode::Loupe && loupe.active {
                self.render_loupe(rt, width, height, loupe, bg_bitmap);
            }

            // ── Zoom Viewport Minimap (Radar Overview) ──
            if show_minimap
                && zoom_state.level > 1.05
                && (mode == AppMode::StaticZoom || mode == AppMode::Draw)
            {
                let infinite = mode == AppMode::Draw && bg_type != CanvasBackground::Transparent;
                self.render_minimap(rt, width, height, bg_bitmap, zoom_state, minimap, infinite);
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

            self.render_color_picker(rt, color_picker);

            if let Some(t) = toast
                && !t.is_expired()
            {
                self.render_toast(rt, width, height, t, toolbar);
            }

            if show_cheat_sheet {
                self.render_cheat_sheet_modal(rt, width, height);
            }

            self.note_draw_result(rt.EndDraw(None, None));
        }
    }

    #[allow(clippy::too_many_arguments)]
    /// Export gets its own throwaway set of device caches. Every one of them
    /// is keyed by the render target, and the export's DC target is a new
    /// one each call, so sharing them meant the export wiped the on-screen
    /// brushes, geometries, mosaics and uploaded images - and the next
    /// on-screen frame then wiped the export's entries in turn, re-uploading
    /// every pasted image (a visible hitch after each Copy/Save/PDF).
    /// Swapping the on-screen contents out and back leaves them untouched.
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
        shapes: &[Annotation],
        active_shape: Option<&Shape>,
        text_input: Option<&TextEditorState>,
        include_spotlight: bool,
        supersample: f32,
    ) -> Option<ScreenCapture> {
        let saved_brushes = self.solid_brush_cache.replace((0, HashMap::new()));
        let saved_scratch = self.scratch_brush.replace((0, None));
        let saved_loupe = self.loupe_brush_cache.replace((0, 0, None));
        let saved_blur = self.blur_mosaic_cache.replace((0, HashMap::new()));
        let saved_geometry = self.geometry_cache.replace((0, HashMap::new()));
        let saved_images = self.image_cache.replace((0, HashMap::new()));

        let out = self.render_to_capture_inner(
            screen_x,
            screen_y,
            width,
            height,
            bg_pixels,
            bg_type,
            zoom_state,
            spotlight,
            shapes,
            active_shape,
            text_input,
            include_spotlight,
            supersample,
        );

        self.solid_brush_cache.replace(saved_brushes);
        self.scratch_brush.replace(saved_scratch);
        self.loupe_brush_cache.replace(saved_loupe);
        self.blur_mosaic_cache.replace(saved_blur);
        self.geometry_cache.replace(saved_geometry);
        self.image_cache.replace(saved_images);
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn render_to_capture_inner(
        &self,
        screen_x: i32,
        screen_y: i32,
        width: u32,
        height: u32,
        bg_pixels: Option<&[u8]>,
        bg_type: CanvasBackground,
        zoom_state: &ZoomState,
        spotlight: &SpotlightState,
        shapes: &[Annotation],
        active_shape: Option<&Shape>,
        text_input: Option<&TextEditorState>,
        include_spotlight: bool,
        supersample: f32,
    ) -> Option<ScreenCapture> {
        if width == 0 || height == 0 {
            return None;
        }

        // The DIB is physical pixels; drawing happens in DIPs. `width`/`height`
        // stay the *native* screen-capture size (the background bitmap is this
        // size, always). The output DIB — what actually gets saved/copied — is
        // scaled up by `supersample`: Direct2D re-renders every vector shape at
        // that higher DPI, and the native background bitmap is bilinearly
        // upscaled into it by the existing DrawBitmap call below.
        // Clamp so neither exported dimension exceeds what the GPU can
        // actually allocate as a bitmap - an export scale (1x/2x/3x) applied
        // to an already-large or multi-monitor-spanning capture could
        // otherwise ask for a target past that limit, and (before EndDraw's
        // result was even checked, see below) silently produce a blank
        // export rather than a smaller one.
        let max_size = self.max_bitmap_size() as f32;
        let supersample = supersample.max(1.0);
        let supersample = supersample
            .min(max_size / width.max(1) as f32)
            .min(max_size / height.max(1) as f32)
            .max(1.0);
        let out_w = ((width as f32) * supersample).round().max(1.0) as u32;
        let out_h = ((height as f32) * supersample).round().max(1.0) as u32;
        let render_dpi = self.dpi * supersample;
        let scale = self.dpi / 96.0;
        let logical_w = width as f32 / scale;
        let logical_h = height as f32 / scale;

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
                    biWidth: out_w as i32,
                    biHeight: -(out_h as i32), // Negative height = top-down DIB
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    biSizeImage: out_w * out_h * 4,
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
                        right: out_w as i32,
                        bottom: out_h as i32,
                    };
                    if dc_rt.BindDC(mem_dc, &rect).is_ok() {
                        // Same DPI as the on-screen target (times any export
                        // supersample) so DIP coordinates map onto these physical
                        // pixels exactly as they do on screen, just denser.
                        dc_rt.SetDpi(render_dpi, render_dpi);
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
                        // A pure infinite-canvas pan (level == 1, view_x/y
                        // nonzero) still needs the translation applied, not
                        // just a zoom — `z > 1.001` alone misses it. Excluded
                        // for a live screen capture, which has nothing to
                        // show past its own edge (see render_frame's twin).
                        let panning = bg_type != CanvasBackground::Transparent
                            && (zoom_state.view_x != 0.0 || zoom_state.view_y != 0.0);
                        let canvas_matrix = if z > 1.001 || panning {
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
                            && pixels.len() >= (width as usize) * (height as usize) * 4
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
                                right: logical_w,
                                bottom: logical_h,
                            };
                            dc_rt.DrawBitmap(
                                bmp,
                                Some(&dst_rect),
                                1.0,
                                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                None,
                            );
                        }

                        // Same two passes as the live path, so an exported
                        // image has its labels in the same places.
                        for a in shapes {
                            if a.container.is_some() {
                                continue;
                            }
                            self.render_single_shape(
                                &dc_rt,
                                &a.shape,
                                bg_bmp.as_ref(),
                                a.opacity,
                                true,
                            );
                        }
                        for a in shapes {
                            if let Some(cid) = a.container
                                && let Some(owner) = shapes.iter().find(|o| o.id == cid)
                            {
                                let bounds = crate::shapes::shape_bounds(&owner.shape);
                                let rides = crate::shapes::label_rides_on_shape(&owner.shape);
                                self.render_contained_text(
                                    &dc_rt, &a.shape, bounds, rides, a.opacity,
                                );
                            }
                        }

                        if let Some(shape) = active_shape {
                            self.render_single_shape(&dc_rt, shape, bg_bmp.as_ref(), 1.0, false);
                        }

                        if let Some(editor) = text_input {
                            self.render_text_editor(&dc_rt, editor, false);
                        }

                        if include_spotlight && spotlight.active {
                            // spotlight.x/y are already screen-space DIPs (see the
                            // on-screen render_frame path), not canvas coordinates -
                            // converting them again here shifted the exported
                            // spotlight whenever the view was zoomed or panned.
                            dc_rt.SetTransform(&identity);
                            self.render_spotlight_mask(
                                &dc_rt,
                                logical_w,
                                logical_h,
                                spotlight.x,
                                spotlight.y,
                                spotlight.radius,
                                spotlight.dim_opacity,
                                spotlight.pinned,
                            );
                            dc_rt.SetTransform(&canvas_matrix);
                        }

                        // A silent EndDraw failure (an oversized supersampled
                        // export can ask for a target bigger than the GPU's
                        // max bitmap size, among other reasons) used to fall
                        // straight through to copying whatever the DIB
                        // happened to already hold - typically all zero -
                        // and returning it as a successful, blank capture.
                        if dc_rt.EndDraw(None, None).is_err() {
                            let _ = SelectObject(mem_dc, old_bmp);
                            let _ = DeleteObject(hbm.into());
                            let _ = DeleteDC(mem_dc);
                            let _ = ReleaseDC(None, screen_dc);
                            return None;
                        }

                        let total_bytes = (out_w * out_h * 4) as usize;
                        let mut pixels = vec![0u8; total_bytes];
                        std::ptr::copy_nonoverlapping(
                            bits_ptr as *const u8,
                            pixels.as_mut_ptr(),
                            total_bytes,
                        );

                        for chunk in pixels.as_chunks_mut::<4>().0 {
                            chunk[3] = 255;
                        }

                        let _ = SelectObject(mem_dc, old_bmp);
                        let _ = DeleteObject(hbm.into());
                        let _ = DeleteDC(mem_dc);
                        let _ = ReleaseDC(None, screen_dc);

                        return Some(ScreenCapture {
                            x: screen_x,
                            y: screen_y,
                            width: out_w,
                            height: out_h,
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
