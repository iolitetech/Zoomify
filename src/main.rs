#![windows_subsystem = "windows"]

mod capture;
mod clipboard;
mod hotkeys;
mod live_zoom;
mod overlay;
mod renderer;
mod shapes;
mod tray;
mod types;

use std::cell::RefCell;
use std::rc::Rc;

use windows::core::{w, PCWSTR, Result};
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, MessageBoxW, PostQuitMessage, RegisterClassExW,
    SetWindowLongPtrW, TranslateMessage, GWLP_USERDATA, MB_ICONINFORMATION, MB_OK, MB_SYSTEMMODAL,
    MSG, WM_COMMAND, WM_DESTROY, WM_HOTKEY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_RBUTTONUP,
    WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};

use hotkeys::*;
use overlay::OverlayWindow;
use tray::*;
use types::AppMode;

struct AppContext {
    overlay: Rc<RefCell<OverlayWindow>>,
    tray: TrayIcon,
    hotkeys: HotkeyManager,
}

const TRAY_HOST_CLASS: PCWSTR = w!("ZoomifyTrayHostClass");

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

        match msg {
            WM_HOTKEY => {
                let hotkey_id = wparam.0 as i32;
                let mut overlay = ctx.overlay.borrow_mut();

                if hotkey_id != HOTKEY_LIVE_ZOOM && overlay.live_zoom.is_active() {
                    overlay.live_zoom.stop();
                }

                match hotkey_id {
                    HOTKEY_STATIC_ZOOM => {
                        overlay.enter_static_zoom();
                    }
                    HOTKEY_DRAW => {
                        overlay.enter_draw_mode();
                    }
                    HOTKEY_SPOTLIGHT => {
                        match overlay.mode {
                            AppMode::StaticZoom | AppMode::Draw | AppMode::Spotlight => {
                                overlay.toggle_spotlight();
                            }
                            _ => {
                                overlay.enter_spotlight_mode();
                            }
                        }
                    }
                    HOTKEY_LIVE_ZOOM => {
                        if overlay.live_zoom.is_active() {
                            overlay.live_zoom.stop();
                            overlay.mode = AppMode::Idle;
                            overlay.hide_window();
                        } else {
                            if overlay.mode != AppMode::Idle {
                                overlay.spotlight.active = false;
                                overlay.hide_window();
                            }
                            overlay.enter_live_zoom();
                        }
                    }
                    HOTKEY_TIMER => {
                        overlay.enter_timer_mode(10);
                    }
                    HOTKEY_SNIP => {
                        overlay.enter_snip_mode();
                    }
                    _ => {}
                }
                LRESULT(0)
            }

            WM_TRAY_ICON => {
                let event = lparam.0 as u32;
                if event == WM_RBUTTONUP {
                    ctx.tray.show_menu();
                } else if event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK {
                    let mut overlay = ctx.overlay.borrow_mut();
                    if overlay.live_zoom.is_active() {
                        overlay.live_zoom.stop();
                    }
                    overlay.enter_static_zoom();
                }
                LRESULT(0)
            }

            WM_COMMAND => {
                let id = (wparam.0 & 0xFFFF) as usize;

                match id {
                    ID_TRAY_LIVE_ZOOM => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        if overlay.live_zoom.is_active() {
                            overlay.live_zoom.stop();
                            overlay.mode = AppMode::Idle;
                            overlay.hide_window();
                        } else {
                            overlay.enter_live_zoom();
                        }
                    }
                    ID_TRAY_STATIC_ZOOM => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        if overlay.live_zoom.is_active() {
                            overlay.live_zoom.stop();
                        }
                        overlay.enter_static_zoom();
                    }
                    ID_TRAY_DRAW => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        if overlay.live_zoom.is_active() {
                            overlay.live_zoom.stop();
                        }
                        overlay.enter_draw_mode();
                    }
                    ID_TRAY_SPOTLIGHT => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        if overlay.live_zoom.is_active() {
                            overlay.live_zoom.stop();
                        }
                        overlay.enter_spotlight_mode();
                    }
                    ID_TRAY_SNIP => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        if overlay.live_zoom.is_active() {
                            overlay.live_zoom.stop();
                        }
                        overlay.enter_snip_mode();
                    }
                    ID_TRAY_TIMER => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        if overlay.live_zoom.is_active() {
                            overlay.live_zoom.stop();
                        }
                        overlay.enter_timer_mode(10);
                    }
                    ID_TRAY_CHEATSHEET => {
                        let mut overlay = ctx.overlay.borrow_mut();
                        if overlay.live_zoom.is_active() {
                            overlay.live_zoom.stop();
                        }
                        overlay.enter_draw_mode();
                        overlay.show_cheat_sheet = true;
                        overlay.request_repaint();
                    }
                    ID_TRAY_ABOUT => {
                        let text = w!(
                            "Zoomify v0.1.0 (Next-Gen ZoomIt in Rust)\n\n\
                            Built with Direct2D, DirectWrite & Windows Magnification API.\n\n\
                            Key Shortcuts:\n\
                            • Ctrl+1: Static Freeze Zoom (Wheel zoom, Pan, Click to draw)\n\
                            • Ctrl+2: Draw Mode (P/H/L/A/R/U/E/T/N)\n\
                            • Ctrl+3 / Tab: Spotlight Mode (Wheel resize, Space to pin)\n\
                            • Ctrl+4: Live Zoom (Real-time hardware magnification)\n\
                            • Ctrl+5: Presentation Countdown Timer\n\
                            • Ctrl+Shift+S / Shift+Drag: Snip Region to Clipboard\n\
                            • F1: Shortcut Cheat Sheet Overlay\n\
                            • Colors: r, g, b, y, o, p, c, w, k"
                        );
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

            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }

            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn main() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        // Named mutex scoped to local session so duplicate instances are prevented
        let mutex_handle = CreateMutexW(None, true, w!("Local\\Zoomify_App_Session_Mutex"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = MessageBoxW(
                None,
                w!("Zoomify is already running in your System Tray!\n\nHotkeys ready:\n• Ctrl+1: Zoom\n• Ctrl+2: Draw\n• Ctrl+3: Spotlight\n• Ctrl+4: Live Zoom\n• Ctrl+5: Timer\n• Ctrl+Shift+S: Snip"),
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

        let overlay = OverlayWindow::create()?;
        let mut tray = TrayIcon::new(tray_hwnd);
        let mut hotkeys = HotkeyManager::new(tray_hwnd);
        hotkeys.register_all();

        // Show non-intrusive Windows tray notification balloon
        tray.show_balloon(
            "Zoomify is Ready!",
            "Hotkeys:\n• Ctrl+1: Zoom\n• Ctrl+2: Draw\n• Ctrl+3: Spotlight\n• Ctrl+4: Live Zoom\n• Ctrl+Shift+S: Snip",
        );

        let app_ctx = Box::new(AppContext {
            overlay,
            tray,
            hotkeys,
        });

        let app_ctx_ptr = Box::into_raw(app_ctx);
        SetWindowLongPtrW(tray_hwnd, GWLP_USERDATA, app_ctx_ptr as isize);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let mut app_ctx = Box::from_raw(app_ctx_ptr);
        app_ctx.hotkeys.unregister_all();
        app_ctx.overlay.borrow_mut().exit_overlay();

        CoUninitialize();
        if let Ok(h) = mutex_handle {
            let _ = windows::Win32::Foundation::CloseHandle(h);
        }

        Ok(())
    }
}
