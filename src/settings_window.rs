#![allow(dead_code)]
#![allow(unsafe_op_in_unsafe_fn)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::needless_return)]

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS_IMMEDIATELY, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE,
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWriteCreateFactory,
    IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
    GetClientRect, GetWindowLongPtrW, IDC_ARROW, IDC_HAND, LoadCursorW,
    PostMessageW, RegisterClassExW, SW_HIDE, SW_SHOW, SetCursor,
    SetForegroundWindow, SetWindowLongPtrW, ShowWindow, WM_CLOSE, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WM_SYSKEYDOWN, WNDCLASSEXW, WS_CAPTION,
    WS_EX_APPWINDOW, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};
use windows::core::{PCWSTR, Result, w};
use windows_numerics::Vector2;

use crate::config::{AppConfig, HotkeyBinding};

pub const WM_SETTINGS_APPLIED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 300;

const SETTINGS_CLASS_NAME: PCWSTR = w!("ZoomifySettingsWindowClass");
const CLIENT_WIDTH: f32 = 720.0;
const CLIENT_HEIGHT: f32 = 560.0;
const RAIL_WIDTH: f32 = 180.0;
const FOOTER_HEIGHT: f32 = 56.0;
const FOOTER_Y: f32 = CLIENT_HEIGHT - FOOTER_HEIGHT; // 504.0
const BTN_Y: f32 = FOOTER_Y + 11.0; // 515.0
const BTN_HEIGHT: f32 = 34.0;

#[inline]
fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

#[inline]
unsafe fn draw_text(
    rt: &ID2D1HwndRenderTarget,
    string: &[u16],
    textformat: &IDWriteTextFormat,
    layoutrect: &D2D_RECT_F,
    defaultfillbrush: &ID2D1SolidColorBrush,
) {
    unsafe {
        rt.DrawText(
            string,
            textformat,
            layoutrect,
            defaultfillbrush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    Hotkeys = 0,
    General = 1,
    Canvas = 2,
    Timer = 3,
    Sessions = 4,
}

pub struct SettingsWindow {
    pub hwnd: HWND,
    notify_hwnd: HWND,
    render_target: Option<ID2D1HwndRenderTarget>,
    dwrite_factory: IDWriteFactory,
    format_title: IDWriteTextFormat,
    format_section: IDWriteTextFormat,
    format_body: IDWriteTextFormat,
    format_desc: IDWriteTextFormat,
    format_button: IDWriteTextFormat,
    format_badge: IDWriteTextFormat,
    active_tab: SettingsTab,
    config: AppConfig,
    capturing_hotkey_idx: Option<usize>,
    hover_item: Option<String>,
    /// Why the last attempted hotkey capture was rejected, shown under the list.
    hotkey_error: Option<String>,
}

impl SettingsWindow {
    pub fn create(notify_hwnd: HWND) -> Result<Rc<RefCell<Self>>> {
        unsafe {
            let hinstance = GetModuleHandleW(None)?;

            let wnd_class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(Self::wnd_proc),
                hInstance: hinstance.into(),
                lpszClassName: SETTINGS_CLASS_NAME,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassExW(&wnd_class);

            let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;

            // Open on the monitor under the cursor, at that monitor's density.
            // Centring via SM_CXSCREEN always landed on the primary display, and
            // a fixed pixel size rendered the window at 80% on a 125% monitor.
            let mon = crate::monitor::MonitorManager::get_monitor_from_cursor();
            let dpi = Self::dpi_for_point(mon.x + mon.width as i32 / 2, mon.y + mon.height as i32 / 2);
            let scale = dpi as f32 / 96.0;

            let mut wr = RECT {
                left: 0,
                top: 0,
                right: (CLIENT_WIDTH * scale).round() as i32,
                bottom: (CLIENT_HEIGHT * scale).round() as i32,
            };
            let _ = AdjustWindowRectEx(&mut wr, style, false, WS_EX_APPWINDOW);
            let win_w = wr.right - wr.left;
            let win_h = wr.bottom - wr.top;

            let x = mon.work_x + (mon.work_width as i32 - win_w) / 2;
            let y = mon.work_y + (mon.work_height as i32 - win_h) / 2;

            let hwnd = CreateWindowExW(
                WS_EX_APPWINDOW,
                SETTINGS_CLASS_NAME,
                w!("Zoomify Settings"),
                style,
                x,
                y,
                win_w,
                win_h,
                None,
                None,
                Some(hinstance.into()),
                None,
            )?;

            // Apply Windows 11 Dark Mode Title Bar
            let dark_mode: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_mode as *const _ as _,
                std::mem::size_of::<i32>() as u32,
            );

            // Apply Windows 11 Rounded Window Corners
            let corner_pref = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner_pref as *const _ as _,
                std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            );

            // DirectWrite factory & typography formats
            let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            let format_title = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Display"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                17.0,
                w!("en-us"),
            )?;

            let format_section = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Display"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                13.0,
                w!("en-us"),
            )?;

            let format_body = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Text"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                12.5,
                w!("en-us"),
            )?;

            let format_desc = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Text"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                10.5,
                w!("en-us"),
            )?;

            let format_button = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Text"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                12.0,
                w!("en-us"),
            )?;
            let _ = format_button.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = format_button.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

            let format_badge = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Text"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                10.0,
                w!("en-us"),
            )?;

            let config = AppConfig::load();

            let state = Rc::new(RefCell::new(Self {
                hwnd,
                notify_hwnd,
                render_target: None,
                dwrite_factory,
                format_title,
                format_section,
                format_body,
                format_desc,
                format_button,
                format_badge,
                active_tab: SettingsTab::Hotkeys,
                config,
                capturing_hotkey_idx: None,
                hover_item: None,
                hotkey_error: None,
            }));

            let raw_ptr = Rc::into_raw(Rc::clone(&state));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw_ptr as isize);

            // Initialize Direct2D render target
            state.borrow_mut().init_render_target()?;

            Ok(state)
        }
    }

    /// Effective DPI for the monitor containing a screen point, falling back to
    /// the system value when per-monitor lookup is unavailable.
    fn dpi_for_point(x: i32, y: i32) -> u32 {
        use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint};
        use windows::Win32::UI::HiDpi::{MDT_EFFECTIVE_DPI, GetDpiForMonitor};

        unsafe {
            let hmon = MonitorFromPoint(
                windows::Win32::Foundation::POINT { x, y },
                MONITOR_DEFAULTTONEAREST,
            );
            let mut dx = 0u32;
            let mut dy = 0u32;
            if GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_ok() && dx > 0 {
                return dx;
            }
        }
        96
    }

    fn init_render_target(&mut self) -> Result<()> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;

            let mut rect = RECT::default();
            GetClientRect(self.hwnd, &mut rect)?;
            let width = (rect.right - rect.left).max(1) as u32;
            let height = (rect.bottom - rect.top).max(1) as u32;

            let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: windows::Win32::Graphics::Direct2D::Common::D2D1_PIXEL_FORMAT {
                    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };

            let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: self.hwnd,
                pixelSize: windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U { width, height },
                presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
            };

            let rt = factory.CreateHwndRenderTarget(&rt_props, &hwnd_props)?;
            let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(self.hwnd);
            let dpi = if dpi == 0 { 96.0 } else { dpi as f32 };
            rt.SetDpi(dpi, dpi);
            rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);

            self.render_target = Some(rt);
            Ok(())
        }
    }

    pub fn show(&mut self) {
        self.config = AppConfig::load();
        self.capturing_hotkey_idx = None;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    fn request_repaint(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            let raw_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<Self>;
            if raw_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }

            let mut this = match (*raw_ptr).try_borrow_mut() {
                Ok(b) => b,
                Err(_) => return DefWindowProcW(hwnd, msg, wparam, lparam),
            };

            match msg {
                WM_PAINT => {
                    let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
                    let _ = windows::Win32::Graphics::Gdi::BeginPaint(hwnd, &mut ps);
                    this.render();
                    let _ = windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
                    LRESULT(0)
                }

                WM_ERASEBKGND => LRESULT(1),

                WM_SETCURSOR => {
                    if this.hover_item.is_some() {
                        let _ = SetCursor(Some(LoadCursorW(None, IDC_HAND).unwrap_or_default()));
                        return LRESULT(1);
                    }
                    let _ = SetCursor(Some(LoadCursorW(None, IDC_ARROW).unwrap_or_default()));
                    LRESULT(1)
                }

                WM_MOUSEMOVE => {
                    let x = (lparam.0 & 0xFFFF) as i16 as f32;
                    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                    let prev_hover = this.hover_item.clone();
                    this.update_hover(x, y);
                    if this.hover_item != prev_hover {
                        this.request_repaint();
                    }
                    LRESULT(0)
                }

                WM_LBUTTONDOWN => {
                    let x = (lparam.0 & 0xFFFF) as i16 as f32;
                    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                    this.handle_click(x, y);
                    LRESULT(0)
                }

                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    let key = wparam.0 as i32;
                    if let Some(slot) = this.capturing_hotkey_idx {
                        if key == VK_ESCAPE.0 as i32 {
                            this.capturing_hotkey_idx = None;
                            this.request_repaint();
                            return LRESULT(0);
                        }

                        let mut mods = 0u32;
                        if (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 {
                            mods |= 0x0002; // MOD_CONTROL
                        }
                        if (GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0 {
                            mods |= 0x0001; // MOD_ALT
                        }
                        if (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 {
                            mods |= 0x0004; // MOD_SHIFT
                        }
                        if (GetKeyState(VK_LWIN.0 as i32) as u16 & 0x8000) != 0
                            || (GetKeyState(VK_RWIN.0 as i32) as u16 & 0x8000) != 0
                        {
                            mods |= 0x0008; // MOD_WIN
                        }

                        // Ignore if only a modifier key is pressed
                        let is_mod_only = key == VK_CONTROL.0 as i32
                            || key == VK_MENU.0 as i32
                            || key == VK_SHIFT.0 as i32
                            || key == VK_LWIN.0 as i32
                            || key == VK_RWIN.0 as i32;

                        if !is_mod_only {
                            let new_binding = HotkeyBinding::new(mods, key as u32);
                            match this.rejection_reason(slot, &new_binding) {
                                None => {
                                    this.store_binding(slot, new_binding);
                                    this.capturing_hotkey_idx = None;
                                    this.hotkey_error = None;
                                }
                                // Stay in capture mode so the user can try again.
                                Some(msg) => this.hotkey_error = Some(msg),
                            }
                            this.request_repaint();
                            return LRESULT(0);
                        }
                    }
                    LRESULT(0)
                }

                WM_CLOSE => {
                    this.hide();
                    LRESULT(0)
                }

                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }

    const HOTKEY_SLOT_NAMES: [&str; 6] = [
        "Static Freeze Zoom",
        "Draw Mode",
        "Spotlight Flashlight",
        "Live Zoom",
        "Presentation Timer",
        "Magnifier Loupe Lens",
    ];

    fn binding_for_slot(&self, slot: usize) -> &HotkeyBinding {
        match slot {
            0 => &self.config.hotkey_static_zoom,
            1 => &self.config.hotkey_draw,
            2 => &self.config.hotkey_spotlight,
            3 => &self.config.hotkey_live_zoom,
            4 => &self.config.hotkey_timer,
            _ => &self.config.hotkey_loupe,
        }
    }

    fn store_binding(&mut self, slot: usize, b: HotkeyBinding) {
        match slot {
            0 => self.config.hotkey_static_zoom = b,
            1 => self.config.hotkey_draw = b,
            2 => self.config.hotkey_spotlight = b,
            3 => self.config.hotkey_live_zoom = b,
            4 => self.config.hotkey_timer = b,
            5 => self.config.hotkey_loupe = b,
            _ => {}
        }
    }

    /// Reject bindings the OS would accept but the user would regret: a bare key
    /// (registered globally, it swallows that key desktop-wide) or a combo already
    /// claimed by another action (the second RegisterHotKey silently fails, leaving
    /// a dead shortcut with no feedback).
    fn rejection_reason(&self, slot: usize, candidate: &HotkeyBinding) -> Option<String> {
        if !candidate.has_modifier() {
            return Some(format!(
                "\"{}\" needs a modifier (Ctrl, Alt, Shift or Win)",
                candidate.format_display()
            ));
        }

        if *candidate == crate::config::RESERVED_SETTINGS_HOTKEY {
            return Some("Ctrl + , is reserved for opening Settings".to_string());
        }

        let bindings: [&HotkeyBinding; 6] = [
            self.binding_for_slot(0),
            self.binding_for_slot(1),
            self.binding_for_slot(2),
            self.binding_for_slot(3),
            self.binding_for_slot(4),
            self.binding_for_slot(5),
        ];
        if let Some(other) = crate::config::conflicting_slot(&bindings, slot, candidate) {
            return Some(format!(
                "{} is already assigned to \"{}\"",
                candidate.format_display(),
                Self::HOTKEY_SLOT_NAMES[other]
            ));
        }
        None
    }

    fn update_hover(&mut self, x: f32, y: f32) {
        // 1. Check tabs in left rail
        let tab_start_y = 72.0;
        let tab_w = 160.0;
        let tab_h = 38.0;

        for i in 0..5 {
            let ty = tab_start_y + (i as f32 * 44.0);
            if x >= 10.0 && x <= 10.0 + tab_w && y >= ty && y <= ty + tab_h {
                self.hover_item = Some(format!("tab_{}", i));
                return;
            }
        }

        // 2. Check footer buttons (always anchored at BTN_Y)
        if y >= BTN_Y && y <= BTN_Y + BTN_HEIGHT {
            if x >= 20.0 && x <= 155.0 {
                self.hover_item = Some("btn_defaults".to_string());
                return;
            } else if x >= 485.0 && x <= 575.0 {
                self.hover_item = Some("btn_cancel".to_string());
                return;
            } else if x >= 585.0 && x <= 700.0 {
                self.hover_item = Some("btn_save".to_string());
                return;
            }
        }

        // 3. Check content area items
        if x >= 200.0 && x <= 705.0 && y >= 70.0 && y <= FOOTER_Y {
            match self.active_tab {
                SettingsTab::Hotkeys => {
                    for i in 0..6 {
                        let cy = 76.0 + (i as f32 * 69.0);
                        if x >= 545.0 && x <= 686.0 && y >= cy + 14.0 && y <= cy + 48.0 {
                            self.hover_item = Some(format!("hk_slot_{}", i));
                            return;
                        }
                    }
                }
                SettingsTab::Sessions => {
                    for i in 0..2 {
                        let cy = 78.0 + (i as f32 * 82.0);
                        if x >= 620.0 && x <= 685.0 && y >= cy + 20.0 && y <= cy + 54.0 {
                            self.hover_item = Some(format!("sess_toggle_{}", i));
                            return;
                        }
                    }
                    let cy_keep = 242.0;
                    if y >= cy_keep + 20.0 && y <= cy_keep + 52.0 {
                        if x >= 566.0 && x <= 598.0 {
                            self.hover_item = Some("keep_minus".to_string());
                            return;
                        } else if x >= 652.0 && x <= 684.0 {
                            self.hover_item = Some("keep_plus".to_string());
                            return;
                        }
                    }
                    let cy_folder = 324.0;
                    if y >= cy_folder + 19.0 && y <= cy_folder + 53.0 {
                        if x >= 496.0 && x <= 568.0 {
                            self.hover_item = Some("btn_folder_default".to_string());
                            return;
                        } else if x >= 576.0 && x <= 686.0 {
                            self.hover_item = Some("btn_folder_browse".to_string());
                            return;
                        }
                    }
                    let cy_open = 406.0;
                    if x >= 576.0 && x <= 686.0 && y >= cy_open + 19.0 && y <= cy_open + 53.0 {
                        self.hover_item = Some("btn_open_sessions".to_string());
                        return;
                    }
                }
                SettingsTab::General => {
                    for i in 0..3 {
                        let cy = 78.0 + (i as f32 * 82.0);
                        if x >= 620.0 && x <= 685.0 && y >= cy + 20.0 && y <= cy + 54.0 {
                            self.hover_item = Some(format!("gen_toggle_{}", i));
                            return;
                        }
                    }
                    // Config open button
                    let cy_cfg = 78.0 + (3.0 * 82.0);
                    if x >= 575.0 && x <= 686.0 && y >= cy_cfg + 20.0 && y <= cy_cfg + 54.0 {
                        self.hover_item = Some("btn_open_cfg".to_string());
                        return;
                    }
                    let cy_wgc = 406.0;
                    if x >= 620.0 && x <= 685.0 && y >= cy_wgc + 20.0 && y <= cy_wgc + 54.0 {
                        self.hover_item = Some("gen_toggle_wgc".to_string());
                        return;
                    }
                }
                SettingsTab::Canvas => {
                    // 1. Zoom stepper [-] [+]
                    let cy1 = 78.0;
                    if y >= cy1 + 18.0 && y <= cy1 + 50.0 {
                        if x >= 566.0 && x <= 598.0 {
                            self.hover_item = Some("zoom_minus".to_string());
                            return;
                        } else if x >= 652.0 && x <= 684.0 {
                            self.hover_item = Some("zoom_plus".to_string());
                            return;
                        }
                    }
                    // 2. Zoom Minimap toggle switch
                    let cy2 = 154.0;
                    if x >= 620.0 && x <= 685.0 && y >= cy2 + 16.0 && y <= cy2 + 52.0 {
                        self.hover_item = Some("canvas_minimap_toggle".to_string());
                        return;
                    }
                    // 3. Stroke width stepper [-] [+]
                    let cy3 = 230.0;
                    if y >= cy3 + 18.0 && y <= cy3 + 50.0 {
                        if x >= 566.0 && x <= 598.0 {
                            self.hover_item = Some("stroke_minus".to_string());
                            return;
                        } else if x >= 652.0 && x <= 684.0 {
                            self.hover_item = Some("stroke_plus".to_string());
                            return;
                        }
                    }
                    // 4. Color swatches
                    let cy4 = 306.0;
                    let chip_y = cy4 + 58.0;
                    for c in 0..7 {
                        let cx = 236.0 + (c as f32 * 42.0);
                        if (x - cx).powi(2) + (y - chip_y).powi(2) <= 16.0 * 16.0 {
                            self.hover_item = Some(format!("color_{}", c));
                            return;
                        }
                    }
                }
                SettingsTab::Timer => {
                    // Duration stepper [-] [+]
                    let cy1 = 78.0;
                    if y >= cy1 + 21.0 && y <= cy1 + 53.0 {
                        if x >= 566.0 && x <= 598.0 {
                            self.hover_item = Some("timer_minus".to_string());
                            return;
                        } else if x >= 652.0 && x <= 684.0 {
                            self.hover_item = Some("timer_plus".to_string());
                            return;
                        }
                    }
                    // Sound chime toggle
                    let cy2 = 160.0;
                    if x >= 620.0 && x <= 685.0 && y >= cy2 + 20.0 && y <= cy2 + 54.0 {
                        self.hover_item = Some("timer_sound_toggle".to_string());
                        return;
                    }
                    // Spotlight radius stepper [-] [+]
                    let cy3 = 242.0;
                    if y >= cy3 + 21.0 && y <= cy3 + 53.0 {
                        if x >= 566.0 && x <= 598.0 {
                            self.hover_item = Some("spotlight_minus".to_string());
                            return;
                        } else if x >= 652.0 && x <= 684.0 {
                            self.hover_item = Some("spotlight_plus".to_string());
                            return;
                        }
                    }
                }
            }
        }

        self.hover_item = None;
    }

    fn handle_click(&mut self, x: f32, y: f32) {
        // 1. Navigation Rail Tabs
        let tab_start_y = 72.0;
        let tab_w = 160.0;
        let tab_h = 38.0;

        for i in 0..5 {
            let ty = tab_start_y + (i as f32 * 44.0);
            if x >= 10.0 && x <= 10.0 + tab_w && y >= ty && y <= ty + tab_h {
                self.active_tab = match i {
                    0 => SettingsTab::Hotkeys,
                    1 => SettingsTab::General,
                    2 => SettingsTab::Canvas,
                    3 => SettingsTab::Timer,
                    _ => SettingsTab::Sessions,
                };
                self.capturing_hotkey_idx = None;
                self.hotkey_error = None;
                self.request_repaint();
                return;
            }
        }

        // 2. Footer Action Buttons
        if y >= BTN_Y && y <= BTN_Y + BTN_HEIGHT {
            if x >= 20.0 && x <= 155.0 {
                // Restore Defaults
                self.config = AppConfig::default();
                self.capturing_hotkey_idx = None;
                self.hotkey_error = None;
                self.request_repaint();
                return;
            } else if x >= 485.0 && x <= 575.0 {
                // Cancel
                self.hide();
                return;
            } else if x >= 585.0 && x <= 700.0 {
                // Save & Apply
                self.config.save();
                unsafe {
                    let _ = PostMessageW(
                        Some(self.notify_hwnd),
                        WM_SETTINGS_APPLIED,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
                self.hide();
                return;
            }
        }

        // 3. Tab Content Interactions
        match self.active_tab {
            SettingsTab::Hotkeys => {
                for i in 0..6 {
                    let cy = 76.0 + (i as f32 * 69.0);
                    if x >= 545.0 && x <= 686.0 && y >= cy + 14.0 && y <= cy + 48.0 {
                        if self.capturing_hotkey_idx == Some(i) {
                            self.capturing_hotkey_idx = None;
                        } else {
                            self.capturing_hotkey_idx = Some(i);
                        }
                        self.hotkey_error = None;
                        self.request_repaint();
                        return;
                    }
                }
                self.capturing_hotkey_idx = None;
                self.request_repaint();
            }

            SettingsTab::Sessions => {
                let cy0 = 78.0;
                if x >= 620.0 && x <= 685.0 && y >= cy0 + 20.0 && y <= cy0 + 54.0 {
                    self.config.autosave_sessions = !self.config.autosave_sessions;
                    self.request_repaint();
                    return;
                }
                let cy1 = 160.0;
                if x >= 620.0 && x <= 685.0 && y >= cy1 + 20.0 && y <= cy1 + 54.0 {
                    self.config.session_export_png = !self.config.session_export_png;
                    self.request_repaint();
                    return;
                }
                let cy_keep = 242.0;
                if y >= cy_keep + 20.0 && y <= cy_keep + 52.0 {
                    if x >= 566.0 && x <= 598.0 {
                        // 0 means "keep everything"; step down into it, not past.
                        self.config.session_keep_last =
                            self.config.session_keep_last.saturating_sub(1);
                        self.request_repaint();
                        return;
                    } else if x >= 652.0 && x <= 684.0 {
                        self.config.session_keep_last =
                            (self.config.session_keep_last + 1).min(99);
                        self.request_repaint();
                        return;
                    }
                }
                let cy_folder = 324.0;
                if y >= cy_folder + 19.0 && y <= cy_folder + 53.0 {
                    if x >= 496.0 && x <= 568.0 {
                        self.config.session_folder.clear();
                        self.request_repaint();
                        return;
                    } else if x >= 576.0 && x <= 686.0 {
                        let start = crate::session::sessions_dir(&self.config);
                        if let Some(dir) = crate::session::pick_folder(self.hwnd, &start) {
                            self.config.session_folder = dir.to_string_lossy().into_owned();
                        }
                        self.request_repaint();
                        return;
                    }
                }
                let cy_open = 406.0;
                if x >= 576.0 && x <= 686.0 && y >= cy_open + 19.0 && y <= cy_open + 53.0 {
                    let dir = crate::session::sessions_dir(&self.config);
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
                    return;
                }
            }

            SettingsTab::General => {
                let cy0 = 78.0;
                if x >= 620.0 && x <= 685.0 && y >= cy0 + 20.0 && y <= cy0 + 54.0 {
                    self.config.start_with_windows = !self.config.start_with_windows;
                    self.request_repaint();
                    return;
                }
                let cy1 = 160.0;
                if x >= 620.0 && x <= 685.0 && y >= cy1 + 20.0 && y <= cy1 + 54.0 {
                    self.config.toolbar_collapsed = !self.config.toolbar_collapsed;
                    self.request_repaint();
                    return;
                }
                let cy2 = 242.0;
                if x >= 620.0 && x <= 685.0 && y >= cy2 + 20.0 && y <= cy2 + 54.0 {
                    self.config.allow_monitor_cycling = !self.config.allow_monitor_cycling;
                    self.request_repaint();
                    return;
                }
                let cy_wgc = 406.0;
                if x >= 620.0 && x <= 685.0 && y >= cy_wgc + 20.0 && y <= cy_wgc + 54.0 {
                    self.config.use_graphics_capture = !self.config.use_graphics_capture;
                    self.request_repaint();
                    return;
                }
                let cy_cfg = 324.0;
                if x >= 575.0 && x <= 686.0 && y >= cy_cfg + 20.0 && y <= cy_cfg + 54.0 {
                    if let Some(path) = AppConfig::config_path() {
                        let _ = std::process::Command::new("notepad.exe").arg(path).spawn();
                    }
                    return;
                }
            }

            SettingsTab::Canvas => {
                // 1. Zoom stepper [-] [+]
                let cy1 = 78.0;
                if y >= cy1 + 18.0 && y <= cy1 + 50.0 {
                    if x >= 566.0 && x <= 598.0 {
                        self.config.default_zoom_level =
                            (self.config.default_zoom_level - 0.25).clamp(1.25, 5.0);
                        self.request_repaint();
                        return;
                    } else if x >= 652.0 && x <= 684.0 {
                        self.config.default_zoom_level =
                            (self.config.default_zoom_level + 0.25).clamp(1.25, 5.0);
                        self.request_repaint();
                        return;
                    }
                }

                // 2. Zoom Minimap toggle switch
                let cy2 = 154.0;
                if x >= 620.0 && x <= 685.0 && y >= cy2 + 16.0 && y <= cy2 + 52.0 {
                    self.config.show_minimap = !self.config.show_minimap;
                    self.request_repaint();
                    return;
                }

                // 3. Stroke width stepper [-] [+]
                let cy3 = 230.0;
                if y >= cy3 + 18.0 && y <= cy3 + 50.0 {
                    if x >= 566.0 && x <= 598.0 {
                        self.config.default_stroke_width =
                            (self.config.default_stroke_width - 1.0).clamp(1.0, 24.0);
                        self.request_repaint();
                        return;
                    } else if x >= 652.0 && x <= 684.0 {
                        self.config.default_stroke_width =
                            (self.config.default_stroke_width + 1.0).clamp(1.0, 24.0);
                        self.request_repaint();
                        return;
                    }
                }

                // 4. Color swatches
                let cy4 = 306.0;
                let chip_y = cy4 + 58.0;
                let colors = ["Red", "Green", "Blue", "Yellow", "Orange", "Pink", "Cyan"];
                for (c, &name) in colors.iter().enumerate() {
                    let cx = 236.0 + (c as f32 * 42.0);
                    if (x - cx).powi(2) + (y - chip_y).powi(2) <= 16.0 * 16.0 {
                        self.config.default_color = name.to_string();
                        self.request_repaint();
                        return;
                    }
                }
            }

            SettingsTab::Timer => {
                let cy1 = 78.0;
                if y >= cy1 + 21.0 && y <= cy1 + 53.0 {
                    if x >= 566.0 && x <= 598.0 {
                        self.config.timer_duration_mins =
                            self.config.timer_duration_mins.saturating_sub(1).max(1);
                        self.request_repaint();
                        return;
                    } else if x >= 652.0 && x <= 684.0 {
                        self.config.timer_duration_mins =
                            (self.config.timer_duration_mins + 1).min(120);
                        self.request_repaint();
                        return;
                    }
                }

                let cy2 = 160.0;
                if x >= 620.0 && x <= 685.0 && y >= cy2 + 20.0 && y <= cy2 + 54.0 {
                    self.config.timer_sound_enabled = !self.config.timer_sound_enabled;
                    self.request_repaint();
                    return;
                }

                let cy3 = 242.0;
                if y >= cy3 + 21.0 && y <= cy3 + 53.0 {
                    if x >= 566.0 && x <= 598.0 {
                        self.config.spotlight_radius =
                            (self.config.spotlight_radius - 20.0).clamp(60.0, 600.0);
                        self.request_repaint();
                        return;
                    } else if x >= 652.0 && x <= 684.0 {
                        self.config.spotlight_radius =
                            (self.config.spotlight_radius + 20.0).clamp(60.0, 600.0);
                        self.request_repaint();
                        return;
                    }
                }
            }
        }
    }

    fn render(&mut self) {
        // Clone the COM handle so `self` stays free for the recovery step below.
        let rt = match self.render_target.as_ref() {
            Some(rt) => rt.clone(),
            None => return,
        };
        let rt = &rt;
        let mut device_lost = false;

        unsafe {
            rt.BeginDraw();

            // Background fill (Windows 11 Fluent dark #1F1F1F)
            let bg_color = D2D1_COLOR_F {
                r: 0.12,
                g: 0.12,
                b: 0.12,
                a: 1.0,
            };
            rt.Clear(Some(&bg_color));

            // Left Navigation Rail (#181818)
            let rail_rect = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: RAIL_WIDTH,
                bottom: FOOTER_Y,
            };
            let rail_color = D2D1_COLOR_F {
                r: 0.09,
                g: 0.09,
                b: 0.09,
                a: 1.0,
            };
            if let Ok(b) = rt.CreateSolidColorBrush(&rail_color, None) {
                rt.FillRectangle(&rail_rect, &b);
            }

            // Rail right divider line (#262626)
            let sep_color = D2D1_COLOR_F {
                r: 0.15,
                g: 0.15,
                b: 0.15,
                a: 1.0,
            };
            if let Ok(b) = rt.CreateSolidColorBrush(&sep_color, None) {
                let p1 = v2(RAIL_WIDTH, 0.0);
                let p2 = v2(RAIL_WIDTH, FOOTER_Y);
                rt.DrawLine(p1, p2, &b, 1.0, None);
            }

            // App Header in Sidebar
            let text_color_white = D2D1_COLOR_F {
                r: 0.98,
                g: 0.98,
                b: 0.98,
                a: 1.0,
            };
            if let Ok(b) = rt.CreateSolidColorBrush(&text_color_white, None) {
                let title_rect = D2D_RECT_F {
                    left: 16.0,
                    top: 18.0,
                    right: 175.0,
                    bottom: 44.0,
                };
                let title_w = w!("Zoomify");
                draw_text(rt, title_w.as_wide(), &self.format_title, &title_rect, &b);
            }

            let text_color_sub = D2D1_COLOR_F {
                r: 0.55,
                g: 0.55,
                b: 0.55,
                a: 1.0,
            };
            if let Ok(b) = rt.CreateSolidColorBrush(&text_color_sub, None) {
                let sub_rect = D2D_RECT_F {
                    left: 16.0,
                    top: 42.0,
                    right: 175.0,
                    bottom: 60.0,
                };
                let sub_w = w!("Preferences");
                draw_text(rt, sub_w.as_wide(), &self.format_desc, &sub_rect, &b);
            }

            // Navigation Rail Tabs
            let tabs = [
                ("⌨  Hotkeys", SettingsTab::Hotkeys),
                ("⚙  General", SettingsTab::General),
                ("🖌  Canvas", SettingsTab::Canvas),
                ("⏱  Timer", SettingsTab::Timer),
                ("💾  Sessions", SettingsTab::Sessions),
            ];

            let active_pill_color = D2D1_COLOR_F {
                r: 0.18,
                g: 0.18,
                b: 0.18,
                a: 1.0,
            };
            let hover_pill_color = D2D1_COLOR_F {
                r: 0.13,
                g: 0.13,
                b: 0.13,
                a: 1.0,
            };
            let accent_bar_color = D2D1_COLOR_F {
                r: 0.0,
                g: 0.47,
                b: 0.83,
                a: 1.0,
            };
            let tab_start_y = 72.0;
            let tab_w = 160.0;
            let tab_h = 38.0;

            for (i, (label, tab)) in tabs.iter().enumerate() {
                let ty = tab_start_y + (i as f32 * 44.0);
                let is_active = self.active_tab == *tab;
                let is_hover = self.hover_item == Some(format!("tab_{}", i));

                let tab_rect = D2D_RECT_F {
                    left: 10.0,
                    top: ty,
                    right: 10.0 + tab_w,
                    bottom: ty + tab_h,
                };
                let tab_rrect = D2D1_ROUNDED_RECT {
                    rect: tab_rect,
                    radiusX: 6.0,
                    radiusY: 6.0,
                };

                if is_active {
                    if let Ok(b) = rt.CreateSolidColorBrush(&active_pill_color, None) {
                        rt.FillRoundedRectangle(&tab_rrect, &b);
                    }
                    // Windows 11 Active Indicator Pill
                    let ind_rect = D2D_RECT_F {
                        left: 12.0,
                        top: ty + 9.0,
                        right: 15.5,
                        bottom: ty + tab_h - 9.0,
                    };
                    let ind_rrect = D2D1_ROUNDED_RECT {
                        rect: ind_rect,
                        radiusX: 1.75,
                        radiusY: 1.75,
                    };
                    if let Ok(b) = rt.CreateSolidColorBrush(&accent_bar_color, None) {
                        rt.FillRoundedRectangle(&ind_rrect, &b);
                    }
                } else if is_hover && let Ok(b) = rt.CreateSolidColorBrush(&hover_pill_color, None)
                {
                    rt.FillRoundedRectangle(&tab_rrect, &b);
                }

                let text_col = if is_active {
                    text_color_white
                } else if is_hover {
                    D2D1_COLOR_F {
                        r: 0.90,
                        g: 0.90,
                        b: 0.90,
                        a: 1.0,
                    }
                } else {
                    D2D1_COLOR_F {
                        r: 0.70,
                        g: 0.70,
                        b: 0.70,
                        a: 1.0,
                    }
                };
                if let Ok(b) = rt.CreateSolidColorBrush(&text_col, None) {
                    let label_utf16: Vec<u16> = label.encode_utf16().collect();
                    let text_rect = D2D_RECT_F {
                        left: 24.0,
                        top: ty + 9.0,
                        right: 165.0,
                        bottom: ty + 31.0,
                    };
                    draw_text(rt, &label_utf16, &self.format_body, &text_rect, &b);
                }
            }

            // Version info at bottom of sidebar
            let ver_col = D2D1_COLOR_F {
                r: 0.40,
                g: 0.40,
                b: 0.40,
                a: 1.0,
            };
            if let Ok(b) = rt.CreateSolidColorBrush(&ver_col, None) {
                let ver_rect = D2D_RECT_F {
                    left: 16.0,
                    top: FOOTER_Y - 26.0,
                    right: 175.0,
                    bottom: FOOTER_Y - 10.0,
                };
                let ver_str = w!("v0.1.0 • Rust D2D");
                draw_text(rt, ver_str.as_wide(), &self.format_badge, &ver_rect, &b);
            }

            // Main Content Area
            self.render_content(rt);

            // Bottom Footer Action Bar (#181818)
            let footer_rect = D2D_RECT_F {
                left: 0.0,
                top: FOOTER_Y,
                right: CLIENT_WIDTH,
                bottom: CLIENT_HEIGHT,
            };
            let footer_color = D2D1_COLOR_F {
                r: 0.09,
                g: 0.09,
                b: 0.09,
                a: 1.0,
            };
            if let Ok(b) = rt.CreateSolidColorBrush(&footer_color, None) {
                rt.FillRectangle(&footer_rect, &b);
            }
            if let Ok(b) = rt.CreateSolidColorBrush(&sep_color, None) {
                let p1 = v2(0.0, FOOTER_Y);
                let p2 = v2(CLIENT_WIDTH, FOOTER_Y);
                rt.DrawLine(p1, p2, &b, 1.0, None);
            }

            // Footer Buttons
            self.render_footer_buttons(rt);

            if let Err(e) = rt.EndDraw(None, None)
                && e.code() == D2DERR_RECREATE_TARGET
            {
                device_lost = true;
            }
        }

        // A lost GPU device leaves the target permanently dead; rebuild it or
        // the settings window renders nothing until the app restarts.
        if device_lost {
            self.render_target = None;
            if self.init_render_target().is_ok() {
                self.request_repaint();
            }
        }
    }

    unsafe fn render_page_header(&self, rt: &ID2D1HwndRenderTarget, title: &str, desc: &str) {
        let text_primary = D2D1_COLOR_F {
            r: 0.98,
            g: 0.98,
            b: 0.98,
            a: 1.0,
        };
        let text_secondary = D2D1_COLOR_F {
            r: 0.58,
            g: 0.58,
            b: 0.58,
            a: 1.0,
        };

        if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
            let utf16: Vec<u16> = title.encode_utf16().collect();
            let tr = D2D_RECT_F {
                left: 205.0,
                top: 18.0,
                right: 700.0,
                bottom: 44.0,
            };
            draw_text(rt, &utf16, &self.format_title, &tr, &b);
        }

        if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
            let utf16: Vec<u16> = desc.encode_utf16().collect();
            let tr = D2D_RECT_F {
                left: 205.0,
                top: 44.0,
                right: 700.0,
                bottom: 64.0,
            };
            draw_text(rt, &utf16, &self.format_desc, &tr, &b);
        }
    }

    unsafe fn render_content(&self, rt: &ID2D1HwndRenderTarget) {
        unsafe {
            match self.active_tab {
                SettingsTab::Hotkeys => self.render_hotkeys_tab(rt),
                SettingsTab::General => self.render_general_tab(rt),
                SettingsTab::Sessions => self.render_sessions_tab(rt),
                SettingsTab::Canvas => self.render_canvas_tab(rt),
                SettingsTab::Timer => self.render_timer_tab(rt),
            }
        }
    }

    unsafe fn render_hotkeys_tab(&self, rt: &ID2D1HwndRenderTarget) {
        self.render_page_header(
            rt,
            "Keyboard Shortcuts",
            "Configure global keyboard triggers for all screen tools",
        );

        let items = [
            (
                "Static Freeze Zoom",
                "Freeze frame and inspect with mouse pan",
                &self.config.hotkey_static_zoom,
            ),
            (
                "Draw Mode",
                "Screen annotations with pens, arrows & badges",
                &self.config.hotkey_draw,
            ),
            (
                "Spotlight Flashlight",
                "Dim surrounding screen to emphasize focus",
                &self.config.hotkey_spotlight,
            ),
            (
                "Live Zoom",
                "Magnify active interactive Windows desktop",
                &self.config.hotkey_live_zoom,
            ),
            (
                "Presentation Timer",
                "High-contrast countdown timer overlay",
                &self.config.hotkey_timer,
            ),
            (
                "Magnifier Loupe Lens",
                "Hardware-accelerated floating circular lens",
                &self.config.hotkey_loupe,
            ),
        ];

        let card_bg = D2D1_COLOR_F {
            r: 0.14,
            g: 0.14,
            b: 0.14,
            a: 1.0,
        };
        let card_border = D2D1_COLOR_F {
            r: 0.20,
            g: 0.20,
            b: 0.20,
            a: 1.0,
        };
        let text_primary = D2D1_COLOR_F {
            r: 0.95,
            g: 0.95,
            b: 0.95,
            a: 1.0,
        };
        let text_secondary = D2D1_COLOR_F {
            r: 0.58,
            g: 0.58,
            b: 0.58,
            a: 1.0,
        };
        let btn_bg = D2D1_COLOR_F {
            r: 0.19,
            g: 0.19,
            b: 0.19,
            a: 1.0,
        };
        let accent_color = D2D1_COLOR_F {
            r: 0.0,
            g: 0.47,
            b: 0.83,
            a: 1.0,
        };

        for (i, (title, desc, binding)) in items.iter().enumerate() {
            let cy = 76.0 + (i as f32 * 69.0);
            let card_rect = D2D_RECT_F {
                left: 205.0,
                top: cy,
                right: 700.0,
                bottom: cy + 62.0,
            };
            let rrect = D2D1_ROUNDED_RECT {
                rect: card_rect,
                radiusX: 7.0,
                radiusY: 7.0,
            };

            if let Ok(b) = rt.CreateSolidColorBrush(&card_bg, None) {
                rt.FillRoundedRectangle(&rrect, &b);
            }
            if let Ok(b) = rt.CreateSolidColorBrush(&card_border, None) {
                rt.DrawRoundedRectangle(&rrect, &b, 1.0, None);
            }

            // Title & Description
            if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
                let utf16: Vec<u16> = title.encode_utf16().collect();
                let tr = D2D_RECT_F {
                    left: 220.0,
                    top: cy + 10.0,
                    right: 535.0,
                    bottom: cy + 30.0,
                };
                draw_text(rt, &utf16, &self.format_section, &tr, &b);
            }
            if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
                let utf16: Vec<u16> = desc.encode_utf16().collect();
                let tr = D2D_RECT_F {
                    left: 220.0,
                    top: cy + 32.0,
                    right: 535.0,
                    bottom: cy + 52.0,
                };
                draw_text(rt, &utf16, &self.format_desc, &tr, &b);
            }

            // Hotkey Button Slot
            let is_capturing = self.capturing_hotkey_idx == Some(i);
            let is_hover = self.hover_item == Some(format!("hk_slot_{}", i));
            let btn_rect = D2D_RECT_F {
                left: 545.0,
                top: cy + 14.0,
                right: 686.0,
                bottom: cy + 48.0,
            };
            let btn_rrect = D2D1_ROUNDED_RECT {
                rect: btn_rect,
                radiusX: 6.0,
                radiusY: 6.0,
            };

            let cur_btn_bg = if is_capturing {
                accent_color
            } else if is_hover {
                D2D1_COLOR_F {
                    r: 0.25,
                    g: 0.25,
                    b: 0.25,
                    a: 1.0,
                }
            } else {
                btn_bg
            };

            if let Ok(b) = rt.CreateSolidColorBrush(&cur_btn_bg, None) {
                rt.FillRoundedRectangle(&btn_rrect, &b);
            }
            let cur_btn_border = if is_capturing {
                text_primary
            } else {
                card_border
            };
            if let Ok(b) = rt.CreateSolidColorBrush(&cur_btn_border, None) {
                rt.DrawRoundedRectangle(&btn_rrect, &b, 1.0, None);
            }

            let btn_text = if is_capturing {
                "Press combo...".to_string()
            } else {
                binding.format_display()
            };
            let utf16: Vec<u16> = btn_text.encode_utf16().collect();
            if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
                draw_text(rt, &utf16, &self.format_button, &btn_rect, &b);
            }
        }

        // Rejected-binding banner, sits between the last card and the footer.
        if let Some(err) = &self.hotkey_error {
            let warn = D2D1_COLOR_F {
                r: 1.0,
                g: 0.42,
                b: 0.38,
                a: 1.0,
            };
            let msg = format!("⚠  {}", err);
            let utf16: Vec<u16> = msg.encode_utf16().collect();
            let tr = D2D_RECT_F {
                left: 220.0,
                top: 486.0,
                right: 700.0,
                bottom: 502.0,
            };
            if let Ok(b) = rt.CreateSolidColorBrush(&warn, None) {
                draw_text(rt, &utf16, &self.format_desc, &tr, &b);
            }
        }
    }

    unsafe fn render_general_tab(&self, rt: &ID2D1HwndRenderTarget) {
        self.render_page_header(
            rt,
            "General Settings",
            "System auto-start, toolbar appearance, and multi-monitor behavior",
        );

        let items = [
            (
                "Start with Windows",
                "Automatically launch Zoomify minimized to system tray on login",
                self.config.start_with_windows,
            ),
            (
                "Toolbar Collapsed by Default",
                "Start on-screen drawing toolbar in minimized compact pill mode",
                self.config.toolbar_collapsed,
            ),
            (
                "Allow Multi-Monitor Cycling",
                "Shift drawing canvas across connected displays using Tab key",
                self.config.allow_monitor_cycling,
            ),
        ];

        let card_bg = D2D1_COLOR_F {
            r: 0.14,
            g: 0.14,
            b: 0.14,
            a: 1.0,
        };
        let card_border = D2D1_COLOR_F {
            r: 0.20,
            g: 0.20,
            b: 0.20,
            a: 1.0,
        };
        let text_primary = D2D1_COLOR_F {
            r: 0.95,
            g: 0.95,
            b: 0.95,
            a: 1.0,
        };
        let text_secondary = D2D1_COLOR_F {
            r: 0.58,
            g: 0.58,
            b: 0.58,
            a: 1.0,
        };

        for (i, (title, desc, state)) in items.iter().enumerate() {
            let cy = 78.0 + (i as f32 * 82.0);
            let card_rect = D2D_RECT_F {
                left: 205.0,
                top: cy,
                right: 700.0,
                bottom: cy + 72.0,
            };
            let rrect = D2D1_ROUNDED_RECT {
                rect: card_rect,
                radiusX: 7.0,
                radiusY: 7.0,
            };

            if let Ok(b) = rt.CreateSolidColorBrush(&card_bg, None) {
                rt.FillRoundedRectangle(&rrect, &b);
            }
            if let Ok(b) = rt.CreateSolidColorBrush(&card_border, None) {
                rt.DrawRoundedRectangle(&rrect, &b, 1.0, None);
            }

            // Title & Description
            if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
                let utf16: Vec<u16> = title.encode_utf16().collect();
                let tr = D2D_RECT_F {
                    left: 220.0,
                    top: cy + 14.0,
                    right: 590.0,
                    bottom: cy + 34.0,
                };
                draw_text(rt, &utf16, &self.format_section, &tr, &b);
            }
            if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
                let utf16: Vec<u16> = desc.encode_utf16().collect();
                let tr = D2D_RECT_F {
                    left: 220.0,
                    top: cy + 38.0,
                    right: 590.0,
                    bottom: cy + 58.0,
                };
                draw_text(rt, &utf16, &self.format_desc, &tr, &b);
            }

            // Toggle switch
            self.render_toggle_switch(rt, 636.0, cy + 26.0, *state);
        }

        // Card 4: Raw Config File Action
        let cy_cfg = 78.0 + (3.0 * 82.0);
        let card_rect = D2D_RECT_F {
            left: 205.0,
            top: cy_cfg,
            right: 700.0,
            bottom: cy_cfg + 72.0,
        };
        let rrect = D2D1_ROUNDED_RECT {
            rect: card_rect,
            radiusX: 7.0,
            radiusY: 7.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&card_bg, None) {
            rt.FillRoundedRectangle(&rrect, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&card_border, None) {
            rt.DrawRoundedRectangle(&rrect, &b, 1.0, None);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy_cfg + 14.0,
                right: 560.0,
                bottom: cy_cfg + 34.0,
            };
            draw_text(
                rt,
                w!("Configuration Storage").as_wide(),
                &self.format_section,
                &tr,
                &b,
            );
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy_cfg + 38.0,
                right: 560.0,
                bottom: cy_cfg + 58.0,
            };
            draw_text(
                rt,
                w!("Directly view and edit raw JSON preferences in Notepad").as_wide(),
                &self.format_desc,
                &tr,
                &b,
            );
        }

        let is_hover_cfg = self.hover_item == Some("btn_open_cfg".to_string());
        let cfg_btn_bg = if is_hover_cfg {
            D2D1_COLOR_F {
                r: 0.25,
                g: 0.25,
                b: 0.25,
                a: 1.0,
            }
        } else {
            D2D1_COLOR_F {
                r: 0.19,
                g: 0.19,
                b: 0.19,
                a: 1.0,
            }
        };
        let btn_cfg_rect = D2D_RECT_F {
            left: 575.0,
            top: cy_cfg + 19.0,
            right: 686.0,
            bottom: cy_cfg + 53.0,
        };
        let btn_cfg_rrect = D2D1_ROUNDED_RECT {
            rect: btn_cfg_rect,
            radiusX: 6.0,
            radiusY: 6.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&cfg_btn_bg, None) {
            rt.FillRoundedRectangle(&btn_cfg_rrect, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&card_border, None) {
            rt.DrawRoundedRectangle(&btn_cfg_rrect, &b, 1.0, None);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
            draw_text(
                rt,
                w!("Open File").as_wide(),
                &self.format_button,
                &btn_cfg_rect,
                &b,
            );
        }

        // Card 5: capture backend
        let cy_wgc = 406.0;
        let card5 = D2D_RECT_F {
            left: 205.0,
            top: cy_wgc,
            right: 700.0,
            bottom: cy_wgc + 72.0,
        };
        let rr5 = D2D1_ROUNDED_RECT {
            rect: card5,
            radiusX: 7.0,
            radiusY: 7.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&card_bg, None) {
            rt.FillRoundedRectangle(&rr5, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&card_border, None) {
            rt.DrawRoundedRectangle(&rr5, &b, 1.0, None);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy_wgc + 14.0,
                right: 590.0,
                bottom: cy_wgc + 34.0,
            };
            draw_text(
                rt,
                w!("Hardware Screen Capture").as_wide(),
                &self.format_section,
                &tr,
                &b,
            );
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy_wgc + 38.0,
                right: 590.0,
                bottom: cy_wgc + 58.0,
            };
            draw_text(
                rt,
                w!("Capture composed frames so video and protected content are not black")
                    .as_wide(),
                &self.format_desc,
                &tr,
                &b,
            );
        }
        self.render_toggle_switch(rt, 636.0, cy_wgc + 26.0, self.config.use_graphics_capture);
    }

    unsafe fn render_sessions_tab(&self, rt: &ID2D1HwndRenderTarget) {
        self.render_page_header(
            rt,
            "Sessions",
            "Save annotations to disk and reload them later (Ctrl+Shift+S / Ctrl+O)",
        );

        let card_bg = D2D1_COLOR_F {
            r: 0.14,
            g: 0.14,
            b: 0.14,
            a: 1.0,
        };
        let card_border = D2D1_COLOR_F {
            r: 0.20,
            g: 0.20,
            b: 0.20,
            a: 1.0,
        };
        let text_primary = D2D1_COLOR_F {
            r: 0.95,
            g: 0.95,
            b: 0.95,
            a: 1.0,
        };
        let text_secondary = D2D1_COLOR_F {
            r: 0.58,
            g: 0.58,
            b: 0.58,
            a: 1.0,
        };

        // A card with a title and a description line; returns its top.
        let card = |cy: f32, title: &str, desc: &str, text_right: f32| unsafe {
            let rect = D2D_RECT_F {
                left: 205.0,
                top: cy,
                right: 700.0,
                bottom: cy + 72.0,
            };
            let rrect = D2D1_ROUNDED_RECT {
                rect,
                radiusX: 7.0,
                radiusY: 7.0,
            };
            if let Ok(b) = rt.CreateSolidColorBrush(&card_bg, None) {
                rt.FillRoundedRectangle(&rrect, &b);
            }
            if let Ok(b) = rt.CreateSolidColorBrush(&card_border, None) {
                rt.DrawRoundedRectangle(&rrect, &b, 1.0, None);
            }
            if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
                let utf16: Vec<u16> = title.encode_utf16().collect();
                let tr = D2D_RECT_F {
                    left: 220.0,
                    top: cy + 14.0,
                    right: text_right,
                    bottom: cy + 34.0,
                };
                draw_text(rt, &utf16, &self.format_section, &tr, &b);
            }
            if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
                let utf16: Vec<u16> = desc.encode_utf16().collect();
                let tr = D2D_RECT_F {
                    left: 220.0,
                    top: cy + 38.0,
                    right: text_right,
                    bottom: cy + 58.0,
                };
                draw_text(rt, &utf16, &self.format_desc, &tr, &b);
            }
        };

        // A right-aligned button; `id` matches the hover ids above.
        let button = |left: f32, right: f32, cy: f32, label: &str, id: &str| unsafe {
            let is_hover = self.hover_item.as_deref() == Some(id);
            let bg = if is_hover {
                D2D1_COLOR_F {
                    r: 0.25,
                    g: 0.25,
                    b: 0.25,
                    a: 1.0,
                }
            } else {
                D2D1_COLOR_F {
                    r: 0.19,
                    g: 0.19,
                    b: 0.19,
                    a: 1.0,
                }
            };
            let rect = D2D_RECT_F {
                left,
                top: cy + 19.0,
                right,
                bottom: cy + 53.0,
            };
            let rrect = D2D1_ROUNDED_RECT {
                rect,
                radiusX: 6.0,
                radiusY: 6.0,
            };
            if let Ok(b) = rt.CreateSolidColorBrush(&bg, None) {
                rt.FillRoundedRectangle(&rrect, &b);
            }
            if let Ok(b) = rt.CreateSolidColorBrush(&card_border, None) {
                rt.DrawRoundedRectangle(&rrect, &b, 1.0, None);
            }
            if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
                let utf16: Vec<u16> = label.encode_utf16().collect();
                draw_text(rt, &utf16, &self.format_button, &rect, &b);
            }
        };

        // 1. Autosave on exit
        card(
            78.0,
            "Autosave on Exit",
            "Write the canvas to a session file each time the overlay closes",
            590.0,
        );
        self.render_toggle_switch(rt, 636.0, 78.0 + 26.0, self.config.autosave_sessions);

        // 2. PNG alongside
        card(
            160.0,
            "Export PNG Alongside",
            "Also write a flattened image next to each saved session",
            590.0,
        );
        self.render_toggle_switch(rt, 636.0, 160.0 + 26.0, self.config.session_export_png);

        // 3. Retention
        let keep = self.config.session_keep_last;
        card(
            242.0,
            "Keep Last Sessions",
            "Older sessions are deleted after each save; 0 keeps every one",
            550.0,
        );
        let keep_label = if keep == 0 {
            "All".to_string()
        } else {
            keep.to_string()
        };
        self.render_stepper(rt, 566.0, 242.0 + 18.0, &keep_label, "keep_minus", "keep_plus");

        // 4. Folder
        let dir = crate::session::sessions_dir(&self.config);
        let shown = dir.to_string_lossy();
        // Long paths would run under the buttons; keep the tail, which is the
        // part that identifies the folder.
        let shown = if shown.chars().count() > 48 {
            let tail: String = shown
                .chars()
                .skip(shown.chars().count().saturating_sub(45))
                .collect();
            format!("...{}", tail)
        } else {
            shown.into_owned()
        };
        card(324.0, "Session Folder", &shown, 490.0);
        button(496.0, 568.0, 324.0, "Default", "btn_folder_default");
        button(576.0, 686.0, 324.0, "Browse", "btn_folder_browse");

        // 5. Reveal in Explorer
        card(
            406.0,
            "Saved Sessions",
            "Open the folder to copy, rename or delete saved sessions",
            560.0,
        );
        button(576.0, 686.0, 406.0, "Open Folder", "btn_open_sessions");
    }

    unsafe fn render_canvas_tab(&self, rt: &ID2D1HwndRenderTarget) {
        self.render_page_header(
            rt,
            "Canvas & Tools",
            "Initial magnification scale, stroke width, and drawing colors",
        );

        let card_bg = D2D1_COLOR_F {
            r: 0.14,
            g: 0.14,
            b: 0.14,
            a: 1.0,
        };
        let card_border = D2D1_COLOR_F {
            r: 0.20,
            g: 0.20,
            b: 0.20,
            a: 1.0,
        };
        let text_primary = D2D1_COLOR_F {
            r: 0.95,
            g: 0.95,
            b: 0.95,
            a: 1.0,
        };
        let text_secondary = D2D1_COLOR_F {
            r: 0.58,
            g: 0.58,
            b: 0.58,
            a: 1.0,
        };

        // 1. Default Zoom Level
        let cy1 = 78.0;
        let c1 = D2D_RECT_F {
            left: 205.0,
            top: cy1,
            right: 700.0,
            bottom: cy1 + 68.0,
        };
        let r1 = D2D1_ROUNDED_RECT {
            rect: c1,
            radiusX: 7.0,
            radiusY: 7.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&card_bg, None) {
            rt.FillRoundedRectangle(&r1, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&card_border, None) {
            rt.DrawRoundedRectangle(&r1, &b, 1.0, None);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy1 + 12.0,
                right: 550.0,
                bottom: cy1 + 32.0,
            };
            draw_text(
                rt,
                w!("Default Zoom Magnification").as_wide(),
                &self.format_section,
                &tr,
                &b,
            );
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy1 + 34.0,
                right: 550.0,
                bottom: cy1 + 54.0,
            };
            draw_text(
                rt,
                w!("Starting scale factor when activating Static Freeze Zoom").as_wide(),
                &self.format_desc,
                &tr,
                &b,
            );
        }
        self.render_stepper(
            rt,
            566.0,
            cy1 + 18.0,
            &format!("{:.2}x", self.config.default_zoom_level),
            "zoom_minus",
            "zoom_plus",
        );

        // 2. Zoom Viewport Minimap (Radar Overview)
        let cy2 = 154.0;
        let c2 = D2D_RECT_F {
            left: 205.0,
            top: cy2,
            right: 700.0,
            bottom: cy2 + 68.0,
        };
        let r2 = D2D1_ROUNDED_RECT {
            rect: c2,
            radiusX: 7.0,
            radiusY: 7.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&card_bg, None) {
            rt.FillRoundedRectangle(&r2, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&card_border, None) {
            rt.DrawRoundedRectangle(&r2, &b, 1.0, None);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy2 + 12.0,
                right: 590.0,
                bottom: cy2 + 32.0,
            };
            draw_text(
                rt,
                w!("Zoom Minimap (Radar Overview)").as_wide(),
                &self.format_section,
                &tr,
                &b,
            );
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy2 + 34.0,
                right: 590.0,
                bottom: cy2 + 54.0,
            };
            draw_text(
                rt,
                w!("Display miniature screen preview in corner when zoomed in to track viewport")
                    .as_wide(),
                &self.format_desc,
                &tr,
                &b,
            );
        }
        self.render_toggle_switch(rt, 636.0, cy2 + 23.0, self.config.show_minimap);

        // 3. Default Stroke Width
        let cy3 = 230.0;
        let c3 = D2D_RECT_F {
            left: 205.0,
            top: cy3,
            right: 700.0,
            bottom: cy3 + 68.0,
        };
        let r3 = D2D1_ROUNDED_RECT {
            rect: c3,
            radiusX: 7.0,
            radiusY: 7.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&card_bg, None) {
            rt.FillRoundedRectangle(&r3, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&card_border, None) {
            rt.DrawRoundedRectangle(&r3, &b, 1.0, None);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy3 + 12.0,
                right: 550.0,
                bottom: cy3 + 32.0,
            };
            draw_text(
                rt,
                w!("Default Drawing Stroke Width").as_wide(),
                &self.format_section,
                &tr,
                &b,
            );
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy3 + 34.0,
                right: 550.0,
                bottom: cy3 + 54.0,
            };
            draw_text(
                rt,
                w!("Pen, arrow, and shape line thickness in pixels").as_wide(),
                &self.format_desc,
                &tr,
                &b,
            );
        }
        self.render_stepper(
            rt,
            566.0,
            cy3 + 18.0,
            &format!("{:.0} px", self.config.default_stroke_width),
            "stroke_minus",
            "stroke_plus",
        );

        // 4. Default Pen Color (height 86px)
        let cy4 = 306.0;
        let c4 = D2D_RECT_F {
            left: 205.0,
            top: cy4,
            right: 700.0,
            bottom: cy4 + 86.0,
        };
        let r4 = D2D1_ROUNDED_RECT {
            rect: c4,
            radiusX: 7.0,
            radiusY: 7.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&card_bg, None) {
            rt.FillRoundedRectangle(&r4, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&card_border, None) {
            rt.DrawRoundedRectangle(&r4, &b, 1.0, None);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy4 + 10.0,
                right: 500.0,
                bottom: cy4 + 30.0,
            };
            draw_text(
                rt,
                w!("Default Drawing Color").as_wide(),
                &self.format_section,
                &tr,
                &b,
            );
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
            let label = format!("Active Startup Swatch: {}", self.config.default_color);
            let utf16: Vec<u16> = label.encode_utf16().collect();
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy4 + 30.0,
                right: 500.0,
                bottom: cy4 + 46.0,
            };
            draw_text(rt, &utf16, &self.format_desc, &tr, &b);
        }

        let colors = [
            (
                "Red",
                D2D1_COLOR_F {
                    r: 0.96,
                    g: 0.26,
                    b: 0.21,
                    a: 1.0,
                },
            ),
            (
                "Green",
                D2D1_COLOR_F {
                    r: 0.30,
                    g: 0.69,
                    b: 0.31,
                    a: 1.0,
                },
            ),
            (
                "Blue",
                D2D1_COLOR_F {
                    r: 0.13,
                    g: 0.59,
                    b: 0.95,
                    a: 1.0,
                },
            ),
            (
                "Yellow",
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.92,
                    b: 0.23,
                    a: 1.0,
                },
            ),
            (
                "Orange",
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.60,
                    b: 0.0,
                    a: 1.0,
                },
            ),
            (
                "Pink",
                D2D1_COLOR_F {
                    r: 0.91,
                    g: 0.12,
                    b: 0.39,
                    a: 1.0,
                },
            ),
            (
                "Cyan",
                D2D1_COLOR_F {
                    r: 0.10,
                    g: 0.85,
                    b: 0.90,
                    a: 1.0,
                },
            ),
        ];

        let chip_y = cy4 + 58.0;
        for (c, (name, col)) in colors.iter().enumerate() {
            let cx = 236.0 + (c as f32 * 42.0);
            let is_selected = self.config.default_color.eq_ignore_ascii_case(name);

            if let Ok(b) = rt.CreateSolidColorBrush(col, None) {
                let el = D2D1_ELLIPSE {
                    point: v2(cx, chip_y),
                    radiusX: 13.0,
                    radiusY: 13.0,
                };
                rt.FillEllipse(&el, &b);
            }

            if is_selected {
                let brush_res = rt.CreateSolidColorBrush(&text_primary, None);
                if let Ok(b) = brush_res {
                    let el = D2D1_ELLIPSE {
                        point: v2(cx, chip_y),
                        radiusX: 16.5,
                        radiusY: 16.5,
                    };
                    rt.DrawEllipse(&el, &b, 2.0, None);
                }
            }
        }

        // 5. Helper Pro Tip Card
        let cy5 = 400.0;
        let c5 = D2D_RECT_F {
            left: 205.0,
            top: cy5,
            right: 700.0,
            bottom: cy5 + 68.0,
        };
        let r5 = D2D1_ROUNDED_RECT {
            rect: c5,
            radiusX: 7.0,
            radiusY: 7.0,
        };
        let tip_bg = D2D1_COLOR_F {
            r: 0.11,
            g: 0.14,
            b: 0.18,
            a: 1.0,
        };
        let tip_border = D2D1_COLOR_F {
            r: 0.15,
            g: 0.25,
            b: 0.35,
            a: 1.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&tip_bg, None) {
            rt.FillRoundedRectangle(&r5, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&tip_border, None) {
            rt.DrawRoundedRectangle(&r5, &b, 1.0, None);
        }
        let accent_text = D2D1_COLOR_F {
            r: 0.35,
            g: 0.75,
            b: 1.0,
            a: 1.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&accent_text, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy5 + 10.0,
                right: 685.0,
                bottom: cy5 + 28.0,
            };
            draw_text(
                rt,
                w!("💡 Quick Drawing & Radar Shortcuts").as_wide(),
                &self.format_section,
                &tr,
                &b,
            );
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy5 + 32.0,
                right: 685.0,
                bottom: cy5 + 56.0,
            };
            draw_text(
                rt,
                w!("Press R, G, B, Y, O, P, C for colors. Press M to toggle Radar Minimap. Ctrl+Z to undo.")
                    .as_wide(),
                &self.format_desc,
                &tr,
                &b,
            );
        }
    }

    unsafe fn render_timer_tab(&self, rt: &ID2D1HwndRenderTarget) {
        self.render_page_header(
            rt,
            "Presentation Timer",
            "Set countdown length, alert chimes, and spotlight radius",
        );

        let card_bg = D2D1_COLOR_F {
            r: 0.14,
            g: 0.14,
            b: 0.14,
            a: 1.0,
        };
        let card_border = D2D1_COLOR_F {
            r: 0.20,
            g: 0.20,
            b: 0.20,
            a: 1.0,
        };
        let text_primary = D2D1_COLOR_F {
            r: 0.95,
            g: 0.95,
            b: 0.95,
            a: 1.0,
        };
        let text_secondary = D2D1_COLOR_F {
            r: 0.58,
            g: 0.58,
            b: 0.58,
            a: 1.0,
        };

        // 1. Timer Duration
        let cy1 = 78.0;
        let c1 = D2D_RECT_F {
            left: 205.0,
            top: cy1,
            right: 700.0,
            bottom: cy1 + 72.0,
        };
        let r1 = D2D1_ROUNDED_RECT {
            rect: c1,
            radiusX: 7.0,
            radiusY: 7.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&card_bg, None) {
            rt.FillRoundedRectangle(&r1, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&card_border, None) {
            rt.DrawRoundedRectangle(&r1, &b, 1.0, None);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy1 + 14.0,
                right: 550.0,
                bottom: cy1 + 34.0,
            };
            draw_text(
                rt,
                w!("Default Presentation Timer Duration").as_wide(),
                &self.format_section,
                &tr,
                &b,
            );
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy1 + 38.0,
                right: 550.0,
                bottom: cy1 + 58.0,
            };
            draw_text(
                rt,
                w!("Initial countdown length in minutes").as_wide(),
                &self.format_desc,
                &tr,
                &b,
            );
        }
        self.render_stepper(
            rt,
            566.0,
            cy1 + 20.0,
            &format!("{} mins", self.config.timer_duration_mins),
            "timer_minus",
            "timer_plus",
        );

        // 2. Alarm Sound Chime
        let cy2 = 160.0;
        let c2 = D2D_RECT_F {
            left: 205.0,
            top: cy2,
            right: 700.0,
            bottom: cy2 + 72.0,
        };
        let r2 = D2D1_ROUNDED_RECT {
            rect: c2,
            radiusX: 7.0,
            radiusY: 7.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&card_bg, None) {
            rt.FillRoundedRectangle(&r2, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&card_border, None) {
            rt.DrawRoundedRectangle(&r2, &b, 1.0, None);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy2 + 14.0,
                right: 590.0,
                bottom: cy2 + 34.0,
            };
            draw_text(
                rt,
                w!("Play Sound Alarm on Expiry").as_wide(),
                &self.format_section,
                &tr,
                &b,
            );
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy2 + 38.0,
                right: 590.0,
                bottom: cy2 + 58.0,
            };
            draw_text(
                rt,
                w!("Play chime when presentation timer reaches 00:00").as_wide(),
                &self.format_desc,
                &tr,
                &b,
            );
        }
        self.render_toggle_switch(rt, 636.0, cy2 + 26.0, self.config.timer_sound_enabled);

        // 3. Spotlight Radius
        let cy3 = 242.0;
        let c3 = D2D_RECT_F {
            left: 205.0,
            top: cy3,
            right: 700.0,
            bottom: cy3 + 72.0,
        };
        let r3 = D2D1_ROUNDED_RECT {
            rect: c3,
            radiusX: 7.0,
            radiusY: 7.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&card_bg, None) {
            rt.FillRoundedRectangle(&r3, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&card_border, None) {
            rt.DrawRoundedRectangle(&r3, &b, 1.0, None);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_primary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy3 + 14.0,
                right: 550.0,
                bottom: cy3 + 34.0,
            };
            draw_text(
                rt,
                w!("Default Spotlight Radius").as_wide(),
                &self.format_section,
                &tr,
                &b,
            );
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy3 + 38.0,
                right: 550.0,
                bottom: cy3 + 58.0,
            };
            draw_text(
                rt,
                w!("Starting flashlight aperture radius in pixels").as_wide(),
                &self.format_desc,
                &tr,
                &b,
            );
        }
        self.render_stepper(
            rt,
            566.0,
            cy3 + 20.0,
            &format!("{:.0} px", self.config.spotlight_radius),
            "spotlight_minus",
            "spotlight_plus",
        );

        // 4. Timer Controls Pro Tip Card
        let cy4 = 324.0;
        let c4 = D2D_RECT_F {
            left: 205.0,
            top: cy4,
            right: 700.0,
            bottom: cy4 + 72.0,
        };
        let r4 = D2D1_ROUNDED_RECT {
            rect: c4,
            radiusX: 7.0,
            radiusY: 7.0,
        };
        let tip_bg = D2D1_COLOR_F {
            r: 0.17,
            g: 0.15,
            b: 0.10,
            a: 1.0,
        };
        let tip_border = D2D1_COLOR_F {
            r: 0.35,
            g: 0.30,
            b: 0.18,
            a: 1.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&tip_bg, None) {
            rt.FillRoundedRectangle(&r4, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&tip_border, None) {
            rt.DrawRoundedRectangle(&r4, &b, 1.0, None);
        }
        let gold_text = D2D1_COLOR_F {
            r: 1.0,
            g: 0.82,
            b: 0.30,
            a: 1.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&gold_text, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy4 + 14.0,
                right: 685.0,
                bottom: cy4 + 34.0,
            };
            draw_text(
                rt,
                w!("💡 Presentation Controls Tip").as_wide(),
                &self.format_section,
                &tr,
                &b,
            );
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_secondary, None) {
            let tr = D2D_RECT_F {
                left: 220.0,
                top: cy4 + 38.0,
                right: 685.0,
                bottom: cy4 + 58.0,
            };
            draw_text(
                rt,
                w!(
                    "Spacebar pauses/resumes countdown. Up/Down arrow keys add or subtract 1 minute"
                )
                .as_wide(),
                &self.format_desc,
                &tr,
                &b,
            );
        }
    }

    unsafe fn render_stepper(
        &self,
        rt: &ID2D1HwndRenderTarget,
        x: f32,
        y: f32,
        val_str: &str,
        minus_hover_id: &str,
        plus_hover_id: &str,
    ) {
        let is_hover_minus = self.hover_item == Some(minus_hover_id.to_string());
        let is_hover_plus = self.hover_item == Some(plus_hover_id.to_string());

        let btn_bg = D2D1_COLOR_F {
            r: 0.19,
            g: 0.19,
            b: 0.19,
            a: 1.0,
        };
        let btn_bg_hover = D2D1_COLOR_F {
            r: 0.26,
            g: 0.26,
            b: 0.26,
            a: 1.0,
        };
        let btn_border = D2D1_COLOR_F {
            r: 0.25,
            g: 0.25,
            b: 0.25,
            a: 1.0,
        };
        let text_col = D2D1_COLOR_F {
            r: 0.95,
            g: 0.95,
            b: 0.95,
            a: 1.0,
        };

        // [-] Button
        let r_minus = D2D_RECT_F {
            left: x,
            top: y,
            right: x + 32.0,
            bottom: y + 32.0,
        };
        let rr_minus = D2D1_ROUNDED_RECT {
            rect: r_minus,
            radiusX: 6.0,
            radiusY: 6.0,
        };
        let cur_minus_bg = if is_hover_minus { btn_bg_hover } else { btn_bg };
        if let Ok(b) = rt.CreateSolidColorBrush(&cur_minus_bg, None) {
            rt.FillRoundedRectangle(&rr_minus, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&btn_border, None) {
            rt.DrawRoundedRectangle(&rr_minus, &b, 1.0, None);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_col, None) {
            draw_text(rt, w!("-").as_wide(), &self.format_button, &r_minus, &b);
        }

        // Value Label
        let r_val = D2D_RECT_F {
            left: x + 32.0,
            top: y,
            right: x + 86.0,
            bottom: y + 32.0,
        };
        let utf16: Vec<u16> = val_str.encode_utf16().collect();
        if let Ok(b) = rt.CreateSolidColorBrush(&text_col, None) {
            draw_text(rt, &utf16, &self.format_button, &r_val, &b);
        }

        // [+] Button
        let r_plus = D2D_RECT_F {
            left: x + 86.0,
            top: y,
            right: x + 118.0,
            bottom: y + 32.0,
        };
        let rr_plus = D2D1_ROUNDED_RECT {
            rect: r_plus,
            radiusX: 6.0,
            radiusY: 6.0,
        };
        let cur_plus_bg = if is_hover_plus { btn_bg_hover } else { btn_bg };
        if let Ok(b) = rt.CreateSolidColorBrush(&cur_plus_bg, None) {
            rt.FillRoundedRectangle(&rr_plus, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&btn_border, None) {
            rt.DrawRoundedRectangle(&rr_plus, &b, 1.0, None);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_col, None) {
            draw_text(rt, w!("+").as_wide(), &self.format_button, &r_plus, &b);
        }
    }

    unsafe fn render_toggle_switch(&self, rt: &ID2D1HwndRenderTarget, x: f32, y: f32, on: bool) {
        let pill_rect = D2D_RECT_F {
            left: x,
            top: y,
            right: x + 44.0,
            bottom: y + 22.0,
        };
        let rrect = D2D1_ROUNDED_RECT {
            rect: pill_rect,
            radiusX: 11.0,
            radiusY: 11.0,
        };

        let pill_bg = if on {
            D2D1_COLOR_F {
                r: 0.0,
                g: 0.47,
                b: 0.83,
                a: 1.0,
            }
        } else {
            D2D1_COLOR_F {
                r: 0.16,
                g: 0.16,
                b: 0.16,
                a: 1.0,
            }
        };
        let pill_border = if on {
            D2D1_COLOR_F {
                r: 0.0,
                g: 0.50,
                b: 0.88,
                a: 1.0,
            }
        } else {
            D2D1_COLOR_F {
                r: 0.35,
                g: 0.35,
                b: 0.35,
                a: 1.0,
            }
        };

        if let Ok(b) = rt.CreateSolidColorBrush(&pill_bg, None) {
            rt.FillRoundedRectangle(&rrect, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&pill_border, None) {
            rt.DrawRoundedRectangle(&rrect, &b, 1.0, None);
        }

        // Knob
        let knob_x = if on { x + 33.0 } else { x + 11.0 };
        let knob_y = y + 11.0;
        let knob_col = if on {
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            }
        } else {
            D2D1_COLOR_F {
                r: 0.80,
                g: 0.80,
                b: 0.80,
                a: 1.0,
            }
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&knob_col, None) {
            let el = D2D1_ELLIPSE {
                point: v2(knob_x, knob_y),
                radiusX: 7.0,
                radiusY: 7.0,
            };
            rt.FillEllipse(&el, &b);
        }
    }

    unsafe fn render_footer_buttons(&self, rt: &ID2D1HwndRenderTarget) {
        let is_hover_def = self.hover_item == Some("btn_defaults".to_string());
        let is_hover_can = self.hover_item == Some("btn_cancel".to_string());
        let is_hover_sav = self.hover_item == Some("btn_save".to_string());

        let text_white = D2D1_COLOR_F {
            r: 0.98,
            g: 0.98,
            b: 0.98,
            a: 1.0,
        };
        let text_dim = D2D1_COLOR_F {
            r: 0.70,
            g: 0.70,
            b: 0.70,
            a: 1.0,
        };
        let sec_btn_bg = if is_hover_def {
            D2D1_COLOR_F {
                r: 0.22,
                g: 0.22,
                b: 0.22,
                a: 1.0,
            }
        } else {
            D2D1_COLOR_F {
                r: 0.16,
                g: 0.16,
                b: 0.16,
                a: 1.0,
            }
        };
        let can_btn_bg = if is_hover_can {
            D2D1_COLOR_F {
                r: 0.22,
                g: 0.22,
                b: 0.22,
                a: 1.0,
            }
        } else {
            D2D1_COLOR_F {
                r: 0.16,
                g: 0.16,
                b: 0.16,
                a: 1.0,
            }
        };
        let sec_btn_border = D2D1_COLOR_F {
            r: 0.25,
            g: 0.25,
            b: 0.25,
            a: 1.0,
        };
        let accent_btn_bg = if is_hover_sav {
            D2D1_COLOR_F {
                r: 0.10,
                g: 0.52,
                b: 0.88,
                a: 1.0,
            }
        } else {
            D2D1_COLOR_F {
                r: 0.0,
                g: 0.47,
                b: 0.83,
                a: 1.0,
            }
        };

        // 1. "Restore Defaults" button (left)
        let r_def = D2D_RECT_F {
            left: 20.0,
            top: BTN_Y,
            right: 155.0,
            bottom: BTN_Y + BTN_HEIGHT,
        };
        let rr_def = D2D1_ROUNDED_RECT {
            rect: r_def,
            radiusX: 6.0,
            radiusY: 6.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&sec_btn_bg, None) {
            rt.FillRoundedRectangle(&rr_def, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&sec_btn_border, None) {
            rt.DrawRoundedRectangle(&rr_def, &b, 1.0, None);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_dim, None) {
            draw_text(
                rt,
                w!("Restore Defaults").as_wide(),
                &self.format_button,
                &r_def,
                &b,
            );
        }

        // 2. "Cancel" button
        let r_can = D2D_RECT_F {
            left: 485.0,
            top: BTN_Y,
            right: 575.0,
            bottom: BTN_Y + BTN_HEIGHT,
        };
        let rr_can = D2D1_ROUNDED_RECT {
            rect: r_can,
            radiusX: 6.0,
            radiusY: 6.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&can_btn_bg, None) {
            rt.FillRoundedRectangle(&rr_can, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&sec_btn_border, None) {
            rt.DrawRoundedRectangle(&rr_can, &b, 1.0, None);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_white, None) {
            draw_text(rt, w!("Cancel").as_wide(), &self.format_button, &r_can, &b);
        }

        // 3. "Save & Apply" button
        let r_save = D2D_RECT_F {
            left: 585.0,
            top: BTN_Y,
            right: 700.0,
            bottom: BTN_Y + BTN_HEIGHT,
        };
        let rr_save = D2D1_ROUNDED_RECT {
            rect: r_save,
            radiusX: 6.0,
            radiusY: 6.0,
        };
        if let Ok(b) = rt.CreateSolidColorBrush(&accent_btn_bg, None) {
            rt.FillRoundedRectangle(&rr_save, &b);
        }
        if let Ok(b) = rt.CreateSolidColorBrush(&text_white, None) {
            draw_text(
                rt,
                w!("Save & Apply").as_wide(),
                &self.format_button,
                &r_save,
                &b,
            );
        }
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
