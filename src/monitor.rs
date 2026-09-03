#![allow(dead_code)]

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HDC, HMONITOR,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN,
};

const MONITORINFOF_PRIMARY: u32 = 0x00000001;

#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    pub hmonitor: isize,
    pub name: String,
    pub index: usize,
    pub is_primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub work_x: i32,
    pub work_y: i32,
    pub work_width: u32,
    pub work_height: u32,
}

impl Default for MonitorInfo {
    fn default() -> Self {
        unsafe {
            let sx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let sy = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let sw = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1) as u32;
            let sh = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1) as u32;
            Self {
                hmonitor: 0,
                name: "Primary Display".to_string(),
                index: 0,
                is_primary: true,
                x: sx,
                y: sy,
                width: sw,
                height: sh,
                work_x: sx,
                work_y: sy,
                work_width: sw,
                work_height: sh,
            }
        }
    }
}

impl MonitorInfo {
    #[inline]
    pub fn contains_point(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && px < self.x + self.width as i32
            && py >= self.y
            && py < self.y + self.height as i32
    }

    #[inline]
    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    #[inline]
    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    #[inline]
    pub fn center(&self) -> (f32, f32) {
        (
            self.x as f32 + self.width as f32 / 2.0,
            self.y as f32 + self.height as f32 / 2.0,
        )
    }
}

pub struct MonitorManager;

impl MonitorManager {
    /// Enumerate all active connected monitors and presentation projectors
    pub fn enumerate_monitors() -> Vec<MonitorInfo> {
        let mut monitors: Vec<MonitorInfo> = Vec::new();

        unsafe extern "system" fn enum_proc(
            hmonitor: HMONITOR,
            _hdc: HDC,
            _lprect: *mut RECT,
            lparam: LPARAM,
        ) -> BOOL {
            unsafe {
                let list = &mut *(lparam.0 as *mut Vec<MonitorInfo>);
                let mut mi = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };

                if GetMonitorInfoW(hmonitor, &mut mi).as_bool() {
                    let is_primary = (mi.dwFlags & MONITORINFOF_PRIMARY) != 0;
                    let idx = list.len();
                    let name = if is_primary {
                        format!("Display {} (Primary)", idx + 1)
                    } else {
                        format!("Display {} (Projector/Ext)", idx + 1)
                    };

                    let w = (mi.rcMonitor.right - mi.rcMonitor.left).max(1) as u32;
                    let h = (mi.rcMonitor.bottom - mi.rcMonitor.top).max(1) as u32;
                    let ww = (mi.rcWork.right - mi.rcWork.left).max(1) as u32;
                    let wh = (mi.rcWork.bottom - mi.rcWork.top).max(1) as u32;

                    list.push(MonitorInfo {
                        hmonitor: hmonitor.0 as isize,
                        name,
                        index: idx,
                        is_primary,
                        x: mi.rcMonitor.left,
                        y: mi.rcMonitor.top,
                        width: w,
                        height: h,
                        work_x: mi.rcWork.left,
                        work_y: mi.rcWork.top,
                        work_width: ww,
                        work_height: wh,
                    });
                }
                BOOL(1)
            }
        }

        unsafe {
            let ptr = &mut monitors as *mut Vec<MonitorInfo>;
            let _ = EnumDisplayMonitors(None, None, Some(enum_proc), LPARAM(ptr as isize));
        }

        if monitors.is_empty() {
            monitors.push(MonitorInfo::default());
        }

        // Sort so Primary is always index 0, followed by secondary/projector monitors ordered left to right
        monitors.sort_by(|a, b| {
            if a.is_primary && !b.is_primary {
                std::cmp::Ordering::Less
            } else if !a.is_primary && b.is_primary {
                std::cmp::Ordering::Greater
            } else {
                a.x.cmp(&b.x)
            }
        });

        for (idx, m) in monitors.iter_mut().enumerate() {
            m.index = idx;
            m.name = if m.is_primary {
                format!("Display {} (Primary)", idx + 1)
            } else {
                format!("Display {} (Ext/Projector)", idx + 1)
            };
        }

        monitors
    }

    /// Retrieve the monitor containing the current cursor position (ideal for presentation mode)
    pub fn get_monitor_from_cursor() -> MonitorInfo {
        let mut pt = POINT::default();
        unsafe {
            if GetCursorPos(&mut pt).is_ok() {
                Self::get_monitor_from_point(pt)
            } else {
                Self::get_primary_monitor()
            }
        }
    }

    /// Retrieve the monitor for an arbitrary screen point
    pub fn get_monitor_from_point(pt: POINT) -> MonitorInfo {
        let mons = Self::enumerate_monitors();
        for m in &mons {
            if m.contains_point(pt.x, pt.y) {
                return m.clone();
            }
        }
        unsafe {
            let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            Self::get_info_for_hmonitor(hmon).unwrap_or_else(Self::get_primary_monitor)
        }
    }

    /// Retrieve the monitor containing a given window
    pub fn get_monitor_from_window(hwnd: HWND) -> MonitorInfo {
        unsafe {
            let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            Self::get_info_for_hmonitor(hmon).unwrap_or_else(Self::get_primary_monitor)
        }
    }

    /// Get primary monitor info
    pub fn get_primary_monitor() -> MonitorInfo {
        let pt = POINT { x: 0, y: 0 };
        unsafe {
            let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
            Self::get_info_for_hmonitor(hmon).unwrap_or_default()
        }
    }

    /// Query monitor details for a specific HMONITOR handle
    pub fn get_info_for_hmonitor(hmon: HMONITOR) -> Option<MonitorInfo> {
        if hmon.is_invalid() {
            return None;
        }

        unsafe {
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };

            if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                let is_primary = (mi.dwFlags & MONITORINFOF_PRIMARY) != 0;
                let w = (mi.rcMonitor.right - mi.rcMonitor.left).max(1) as u32;
                let h = (mi.rcMonitor.bottom - mi.rcMonitor.top).max(1) as u32;
                let ww = (mi.rcWork.right - mi.rcWork.left).max(1) as u32;
                let wh = (mi.rcWork.bottom - mi.rcWork.top).max(1) as u32;

                Some(MonitorInfo {
                    hmonitor: hmon.0 as isize,
                    name: if is_primary { "Primary Display".to_string() } else { "External Display".to_string() },
                    index: 0,
                    is_primary,
                    x: mi.rcMonitor.left,
                    y: mi.rcMonitor.top,
                    width: w,
                    height: h,
                    work_x: mi.rcWork.left,
                    work_y: mi.rcWork.top,
                    work_width: ww,
                    work_height: wh,
                })
            } else {
                None
            }
        }
    }

    /// Calculate combined virtual desktop bounding rectangle spanning all monitors
    pub fn get_virtual_desktop_bounds() -> (i32, i32, u32, u32) {
        unsafe {
            let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let w = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1) as u32;
            let h = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1) as u32;
            (x, y, w, h)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitor_info_bounds_and_contains() {
        let mon = MonitorInfo {
            hmonitor: 12345,
            name: "Projector (Display 2)".to_string(),
            index: 1,
            is_primary: false,
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
            work_x: 1920,
            work_y: 0,
            work_width: 1920,
            work_height: 1040,
        };

        assert_eq!(mon.right(), 3840);
        assert_eq!(mon.bottom(), 1080);
        assert_eq!(mon.center(), (2880.0, 540.0));

        // Inside point
        assert!(mon.contains_point(2000, 500));
        assert!(mon.contains_point(1920, 0));

        // Outside point (on left monitor)
        assert!(!mon.contains_point(100, 500));
        // Outside point (beyond right)
        assert!(!mon.contains_point(3850, 500));
    }

    #[test]
    fn test_negative_coordinates_monitor() {
        // Monitor positioned to the left of primary monitor (common dual screen setup)
        let left_mon = MonitorInfo {
            hmonitor: 999,
            name: "Secondary Left Display".to_string(),
            index: 0,
            is_primary: false,
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
            work_x: -1920,
            work_y: 0,
            work_width: 1920,
            work_height: 1080,
        };

        assert_eq!(left_mon.right(), 0);
        assert_eq!(left_mon.bottom(), 1080);
        assert!(left_mon.contains_point(-1000, 500));
        assert!(!left_mon.contains_point(100, 500));
    }
}
