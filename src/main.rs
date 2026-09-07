#![windows_subsystem = "windows"]

mod capture;
mod capture_wgc;
mod clipboard;
mod config;
mod hotkeys;
mod live_zoom;
mod monitor;
mod overlay;
mod renderer;
mod session;
mod settings_window;
mod shapes;
mod tray;
mod types;

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GWLP_USERDATA, GetMessageW,
    GetWindowLongPtrW, MB_ICONINFORMATION, MB_OK, MB_SYSTEMMODAL, MSG, MessageBoxW,
    PostQuitMessage, RegisterClassExW, SetWindowLongPtrW, TranslateMessage, WM_COMMAND, WM_DESTROY,
    WM_DISPLAYCHANGE, WM_HOTKEY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSEXW,
    WS_OVERLAPPEDWINDOW,
};
use windows::core::{PCWSTR, Result, w};

use hotkeys::*;
use overlay::OverlayWindow;
use tray::*;
use types::{AppMode, CanvasBackground, DrawTool};

fn ensure_live_zoom_stopped(overlay: &mut OverlayWindow) {
    if overlay.live_zoom.is_active() || overlay.mode == AppMode::LiveZoom {
        overlay.stop_live_zoom();
        overlay.mode = AppMode::Idle;
        // No sleep here: the capture path waits on DWM composition instead, so
        // the message pump is not blocked while the overlay borrow is held.
    }
}

struct AppContext {
    overlay: Rc<RefCell<OverlayWindow>>,
    tray: TrayIcon,
    hotkeys: HotkeyManager,
    settings_window: Rc<RefCell<settings_window::SettingsWindow>>,
}

const TRAY_HOST_CLASS: PCWSTR = w!("ZoomifyTrayHostClass");

/// Posted to ourselves once the tray icon is up, to build the screen-capture
/// pipeline before the user first asks for it. Activating the WinRT capture
/// class costs over 100ms, which is otherwise paid on the first Ctrl+2.
const WM_PREWARM_CAPTURE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 500;

/// Drives Live Zoom's smooth pan. Only runs while a session is active - it used
/// to tick at 62 Hz for the entire life of the process.
const TIMER_ID_LIVE_ZOOM_PAN: usize = 1;

unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        let raw_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppContext;
        if raw_ptr.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }

        let ctx = &mut *raw_ptr;

        // Explorer restarted and dropped every tray icon: claim ours back.
        // Registered at runtime, so it cannot be a `match` arm.
        let taskbar_created = ctx.tray.taskbar_created_msg();
        if taskbar_created != 0 && msg == taskbar_created {
            ctx.tray.re_add();
            return LRESULT(0);
        }

        match msg {
            WM_HOTKEY => {
                let hotkey_id = wparam.0 as i32;
                let mut overlay = ctx.overlay.borrow_mut();

                match hotkey_id {
                    HOTKEY_STATIC_ZOOM => {
                        ensure_live_zoom_stopped(&mut overlay);
                        if overlay.mode == AppMode::StaticZoom {
                            overlay.exit_overlay();
                        } else {
                            overlay.enter_static_zoom();
                        }
                    }
                    HOTKEY_DRAW => {
                        ensure_live_zoom_stopped(&mut overlay);
                        if overlay.mode == AppMode::Draw {
                            overlay.exit_overlay();
                        } else {
                            overlay.enter_draw_mode();
                        }
                    }
                    HOTKEY_SPOTLIGHT => {
                        ensure_live_zoom_stopped(&mut overlay);
                        if overlay.mode == AppMode::Spotlight {
                            overlay.exit_overlay();
                        } else if overlay.mode == AppMode::StaticZoom
                            || overlay.mode == AppMode::Draw
                        {
                            overlay.toggle_spotlight();
                        } else {
                            overlay.enter_spotlight_mode();
                        }
                    }
                    HOTKEY_LIVE_ZOOM => {
                        if overlay.live_zoom.is_active() {
                            overlay.stop_live_zoom();
                            overlay.mode = AppMode::Idle;
                            overlay.hide_window();
                        } else {
                            if overlay.mode != AppMode::Idle {
                                overlay.exit_overlay();
                            }
                            overlay.enter_live_zoom();
                        }
                    }
                    HOTKEY_LIVE_ZOOM_IN | HOTKEY_LIVE_ZOOM_IN_PLUS => {
                        // Only registered while a session is running.
                        if overlay.live_zoom.is_active() {
                            overlay.live_zoom.adjust_zoom(0.25);
                        }
                    }
                    HOTKEY_LIVE_ZOOM_OUT | HOTKEY_LIVE_ZOOM_OUT_MINUS => {
                        if overlay.live_zoom.is_active() {
                            overlay.live_zoom.adjust_zoom(-0.25);
                        }
                    }
                    HOTKEY_TIMER => {
                        ensure_live_zoom_stopped(&mut overlay);
                        if overlay.mode == AppMode::Timer {
                            overlay.exit_overlay();
                        } else {
                            overlay.enter_timer_mode(0);
                        }
                    }
                    HOTKEY_LOUPE => {
                        ensure_live_zoom_stopped(&mut overlay);
                        if overlay.mode == AppMode::Loupe {
                            overlay.exit_overlay();
                        } else {
                            overlay.enter_loupe_mode();
                        }
                    }
                    HOTKEY_SETTINGS => {
                        ctx.settings_window.borrow_mut().show();
                    }
                    _ => {}
                }
                LRESULT(0)
            }

            windows::Win32::UI::WindowsAndMessaging::WM_TIMER => {
                if wparam.0 == TIMER_ID_LIVE_ZOOM_PAN {
                    let mut overlay = ctx.overlay.borrow_mut();
                    if overlay.live_zoom.is_active() {
                        overlay.live_zoom.tick_smooth_pan(0.25);
                    }
                }
                LRESULT(0)
            }

            WM_TRAY_ICON => {
                let event = lparam.0 as u32;
                if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                    ctx.tray.show_menu();
                } else if event == WM_LBUTTONDBLCLK {
                    let mut overlay = ctx.overlay.borrow_mut();
                    if overlay.live_zoom.is_active() {
                        overlay.stop_live_zoom();
                    }
                    overlay.enter_static_zoom();
                }
                LRESULT(0)
            }

            overlay::WM_LIVE_ZOOM_STATE => {
                let active = wparam.0 != 0;
                ctx.hotkeys.set_live_zoom_hotkeys(active);
                if active {
                    windows::Win32::UI::WindowsAndMessaging::SetTimer(
                        Some(hwnd),
                        TIMER_ID_LIVE_ZOOM_PAN,
                        16,
                        None,
                    );
                } else {
                    let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(
                        Some(hwnd),
                        TIMER_ID_LIVE_ZOOM_PAN,
                    );
                }
                LRESULT(0)
            }

            WM_DISPLAYCHANGE => {
                let mut overlay = ctx.overlay.borrow_mut();
                overlay.refresh_monitors();
                let count = overlay.available_monitors.len();
                let label = if count > 1 {
                    format!("Displays updated: {} screens detected", count)
                } else {
                    "Displays updated: 1 screen detected".to_string()
                };
                overlay.set_toast("🖥️", label);
                LRESULT(0)
            }

            WM_COMMAND => {
                let id = wparam.0 & 0xFFFF;

                match id {
                    ID_TRAY_LIVE_ZOOM => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        if overlay.live_zoom.is_active() {
                            overlay.stop_live_zoom();
                            overlay.mode = AppMode::Idle;
                            overlay.hide_window();
                        } else {
                            overlay.enter_live_zoom();
                        }
                    }
                    ID_TRAY_STATIC_ZOOM => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        ensure_live_zoom_stopped(&mut overlay);
                        overlay.enter_static_zoom();
                    }
                    ID_TRAY_DRAW => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        ensure_live_zoom_stopped(&mut overlay);
                        overlay.enter_draw_mode();
                    }
                    ID_TRAY_SPOTLIGHT => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        ensure_live_zoom_stopped(&mut overlay);
                        overlay.enter_spotlight_mode();
                    }
                    ID_TRAY_TIMER => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        ensure_live_zoom_stopped(&mut overlay);
                        overlay.enter_timer_mode(0);
                    }
                    ID_TRAY_LOUPE => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        ensure_live_zoom_stopped(&mut overlay);
                        overlay.enter_loupe_mode();
                    }
                    ID_TRAY_LASER => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        ensure_live_zoom_stopped(&mut overlay);
                        overlay.enter_draw_mode();
                        overlay.current_tool = DrawTool::LaserPointer;
                        overlay.set_toast("🔴", "Laser Pointer Active (K)");
                        overlay.request_repaint();
                    }
                    ID_TRAY_ERASER => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        ensure_live_zoom_stopped(&mut overlay);
                        overlay.enter_draw_mode();
                        overlay.current_tool = DrawTool::Eraser;
                        overlay.set_toast("🧹", "Stroke Eraser Active (X)");
                        overlay.request_repaint();
                    }
                    ID_TRAY_BLUR => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        ensure_live_zoom_stopped(&mut overlay);
                        overlay.enter_draw_mode();
                        overlay.current_tool = DrawTool::Blur;
                        overlay.set_toast("░", "Redact / Blur Tool Active (Shift+X)");
                        overlay.request_repaint();
                    }
                    ID_TRAY_WHITEBOARD => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        ensure_live_zoom_stopped(&mut overlay);
                        overlay.enter_draw_mode();
                        overlay.background_type = CanvasBackground::Whiteboard;
                        overlay.set_toast("📄", "Whiteboard Canvas Active (W)");
                        overlay.request_repaint();
                    }
                    ID_TRAY_BLACKBOARD => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        ensure_live_zoom_stopped(&mut overlay);
                        overlay.enter_draw_mode();
                        overlay.background_type = CanvasBackground::Blackboard;
                        overlay.set_toast("⬛", "Blackboard Canvas Active (Shift+K)");
                        overlay.request_repaint();
                    }
                    ID_TRAY_RESET_TOOLBAR => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        overlay.toolbar.custom_position = None;
                        overlay.toolbar.collapsed = false;
                        let w = overlay.screen_width as f32;
                        let h = overlay.screen_height as f32;
                        overlay.toolbar.update_layout(w, h);
                        overlay.set_toast("📌", "Toolbar Position Reset to Top Center");
                        overlay.request_repaint();
                    }
                    ID_TRAY_SETTINGS => {
                        ctx.settings_window.borrow_mut().show();
                    }
                    ID_TRAY_OPEN_CONFIG => {
                        if let Some(appdata) = std::env::var_os("APPDATA") {
                            let mut path = std::path::PathBuf::from(appdata);
                            path.push("Zoomify");
                            path.push("config.json");
                            let _ = std::process::Command::new("notepad.exe").arg(path).spawn();
                        }
                    }
                    ID_TRAY_CHEATSHEET => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        ensure_live_zoom_stopped(&mut overlay);
                        overlay.enter_draw_mode();
                        overlay.show_cheat_sheet = true;
                        overlay.request_repaint();
                    }
                    ID_TRAY_ABOUT => {
                        let text = w!("Zoomify v0.1.0 (Next-Gen ZoomIt in Rust)\n\n\
                            Built with Direct2D, DirectWrite & Windows Magnification API.\n\n\
                            Key Shortcuts:\n\
                            • Ctrl+1: Static Freeze Zoom (Wheel zoom, Pan, Click to draw)\n\
                            • Ctrl+2: Draw Mode (P/H/L/A/R/U/Q/T/N)\n\
                            • Ctrl+3 / F3: Spotlight Mode (Ctrl+Wheel to resize, Click / Space to pin)\n\
                            • Ctrl+4: Live Zoom (Ctrl+Wheel or Ctrl+Up/Down to zoom)\n\
                            • Ctrl+5: Presentation Countdown Timer\n\
                            • Ctrl+6: Magnifier Loupe Lens (Tab=Shape, Space=Pin, R=Reticle, Wheel=Zoom)\n\
                            • Ctrl+,: Settings & Hotkeys Configurator\n\
                            • F1: Shortcut Cheat Sheet Overlay | F2: Toggle HUD\n\
                            • Mouse Modifiers: Shift=Line, Ctrl=Rect, Tab=Ellipse, Shift+Ctrl=Arrow\n\
                            • Colors: r, g, b, y, o, c | Shift+P: Pink | Shift+W: White | Shift+B: Black");
                        let _ = MessageBoxW(
                            None,
                            text,
                            w!("About Zoomify"),
                            MB_OK | MB_ICONINFORMATION | MB_SYSTEMMODAL,
                        );
                    }
                    ID_TRAY_EXIT => {
                        ctx.overlay.borrow_mut().exit_overlay();
                        PostQuitMessage(0);
                    }
                    _ => {}
                }
                LRESULT(0)
            }

            WM_PREWARM_CAPTURE => {
                if config::AppConfig::load().use_graphics_capture {
                    let mut pt = windows::Win32::Foundation::POINT::default();
                    let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt);
                    capture_wgc::prewarm(pt.x, pt.y);
                }
                LRESULT(0)
            }

            settings_window::WM_SETTINGS_APPLIED => {
                let cfg = config::AppConfig::load();
                capture::set_use_graphics_capture(cfg.use_graphics_capture);
                let failed = ctx.hotkeys.reload_from_config(&cfg);
                let mut overlay = ctx.overlay.borrow_mut();
                overlay.stroke_width = cfg.default_stroke_width;
                overlay.spotlight.radius = cfg.spotlight_radius;
                overlay.timer_seconds = cfg.timer_duration_mins * 60;
                overlay.timer_sound_enabled = cfg.timer_sound_enabled;
                overlay.show_minimap = cfg.show_minimap;
                overlay.snap_to_shapes = cfg.snap_to_shapes;
                overlay.default_zoom_level = cfg.default_zoom_level;
                overlay.allow_monitor_cycling = cfg.allow_monitor_cycling;
                overlay.monitor_target = cfg.monitor_target.clone();
                overlay.current_color = config::AppConfig::parse_color(&cfg.default_color);
                // A shortcut another app already owns fails to register; saying so
                // beats leaving the user with a key that quietly does nothing.
                if failed.is_empty() {
                    overlay.set_toast("⚙️", "Settings & Hotkeys Applied");
                } else {
                    overlay.set_toast(
                        "⚠️",
                        format!("In use by another app: {}", failed.join(", ")),
                    );
                }
                overlay.request_repaint();
                LRESULT(0)
            }

            WM_DESTROY => {
                ctx.overlay.borrow_mut().exit_overlay();
                PostQuitMessage(0);
                LRESULT(0)
            }

            windows::Win32::UI::WindowsAndMessaging::WM_QUERYENDSESSION => {
                ctx.overlay.borrow_mut().exit_overlay();
                LRESULT(1)
            }

            windows::Win32::UI::WindowsAndMessaging::WM_ENDSESSION => {
                if wparam.0 != 0 {
                    ctx.overlay.borrow_mut().exit_overlay();
                }
                LRESULT(0)
            }

            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn main() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = windows::Win32::System::Ole::OleInitialize(None);

        // Named mutex scoped to local session so duplicate instances are prevented
        let mutex_handle = CreateMutexW(None, true, w!("Local\\Zoomify_App_Session_Mutex"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = MessageBoxW(
                None,
                w!(
                    "Zoomify is already running in your System Tray!\n\nHotkeys ready:\n• Ctrl+1: Zoom\n• Ctrl+2: Draw\n• Ctrl+3: Spotlight\n• Ctrl+4: Live Zoom\n• Ctrl+5: Timer\n• Ctrl+6: Loupe"
                ),
                w!("Zoomify Running"),
                MB_OK | MB_ICONINFORMATION | MB_SYSTEMMODAL,
            );
            return Ok(());
        }

        let hinstance: HINSTANCE = GetModuleHandleW(None)?.into();

        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(tray_wnd_proc),
            hInstance: hinstance,
            lpszClassName: TRAY_HOST_CLASS,
            ..Default::default()
        };

        RegisterClassExW(&wnd_class);

        let tray_hwnd = CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
            TRAY_HOST_CLASS,
            w!("Zoomify Host Window"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance),
            None,
        )?;

        capture::set_use_graphics_capture(
            config::AppConfig::load().use_graphics_capture,
        );

        let overlay = OverlayWindow::create()?;
        overlay.borrow_mut().set_host_hwnd(tray_hwnd);
        let settings_window = settings_window::SettingsWindow::create(tray_hwnd)?;
        let mut tray = TrayIcon::new(tray_hwnd);
        let mut hotkeys = HotkeyManager::new(tray_hwnd);
        let failed_hotkeys = hotkeys.register_all();

        // Tray balloon is the only UI available at startup, so any hotkey
        // conflict has to be reported here.
        if failed_hotkeys.is_empty() {
            tray.show_balloon(
                "Zoomify is Ready!",
                "Hotkeys:\n• Ctrl+1: Zoom\n• Ctrl+2: Draw\n• Ctrl+3: Spotlight\n• Ctrl+4: Live Zoom\n• Ctrl+5: Timer\n• Ctrl+6: Loupe\n• Ctrl+,: Settings",
            );
        } else {
            tray.show_balloon(
                "Zoomify: some hotkeys unavailable",
                &format!(
                    "Another app already owns: {}.
Choose different combos in Settings (Ctrl+,).",
                    failed_hotkeys.join(", ")
                ),
            );
        }

        let app_ctx = Box::new(AppContext {
            overlay,
            tray,
            hotkeys,
            settings_window,
        });

        let app_ctx_ptr = Box::into_raw(app_ctx);
        SetWindowLongPtrW(tray_hwnd, GWLP_USERDATA, app_ctx_ptr as isize);

        // Warm the capture pipeline now that the app is visibly up, rather
        // than making the first overlay wait for it.
        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
            Some(tray_hwnd),
            WM_PREWARM_CAPTURE,
            WPARAM(0),
            LPARAM(0),
        );

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);

            // Autosave runs while the overlay tears down, where a toast can no
            // longer be drawn. Anything it could not report lands here.
            if let Some(err) = session::take_error() {
                let ctx = &mut *app_ctx_ptr;
                ctx.tray.show_balloon("Zoomify: session not saved", &err);
            }
        }

        let mut app_ctx = Box::from_raw(app_ctx_ptr);
        app_ctx.hotkeys.unregister_all();
        app_ctx.overlay.borrow_mut().exit_overlay();
        app_ctx.settings_window.borrow_mut().hide();

        windows::Win32::System::Ole::OleUninitialize();
        if let Ok(h) = mutex_handle {
            let _ = windows::Win32::Foundation::CloseHandle(h);
        }

        Ok(())
    }
}
