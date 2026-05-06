//! Calendar widget — Win32 `SysMonthCal32` (MonthCalendar control).
//!
//! `MCM_SETCURSEL` and `MCM_GETCURSEL` round-trip through a Win32
//! `SYSTEMTIME` struct. `MCN_SELCHANGE` (delivered as `WM_NOTIFY`)
//! posts the selection-change event; the per-handle callback is
//! resolved through `CALENDAR_CALLBACKS` and invoked by the central
//! WM_NOTIFY router in `widgets::mod`.

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
const MCM_FIRST: u32 = 0x1000;
#[cfg(target_os = "windows")]
const MCM_GETCURSEL: u32 = MCM_FIRST + 1;
#[cfg(target_os = "windows")]
const MCM_SETCURSEL: u32 = MCM_FIRST + 2;

thread_local! {
    static CALENDAR_CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Default, Copy, Clone)]
struct SystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    millis: u16,
}

pub fn create(year: i64, month: i64, on_change: f64) -> i64 {
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("SysMonthCal32");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(std::ptr::null()),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0),
                0,
                0,
                240,
                180,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            );
            let Ok(hwnd) = hwnd else {
                return register_widget(HWND(std::ptr::null_mut()), WidgetKind::Calendar, control_id);
            };

            let handle = register_widget(hwnd, WidgetKind::Calendar, control_id);
            CALENDAR_CALLBACKS.with(|m| {
                m.borrow_mut().insert(handle, on_change);
            });

            // Initial selected date — defaults to today if year/month are
            // out of range. SysMonthCal32 needs a valid SYSTEMTIME so we
            // pick day=1 when the caller didn't specify one.
            let mut st = SystemTime::default();
            st.year = if year > 0 { year as u16 } else { 2026 };
            st.month = if (1..=12).contains(&month) { month as u16 } else { 1 };
            st.day = 1;
            st.day_of_week = 0;
            let _ = SendMessageW(
                hwnd,
                MCM_SETCURSEL,
                WPARAM(0),
                LPARAM(&st as *const _ as isize),
            );
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (year, month, on_change);
        register_widget(0, WidgetKind::Calendar, control_id)
    }
}

pub fn set_date(handle: i64, year: i64, month: i64, day: i64) {
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = super::get_hwnd(handle) else { return };
        let mut st = SystemTime::default();
        st.year = year.clamp(1, 9999) as u16;
        st.month = month.clamp(1, 12) as u16;
        st.day = day.clamp(1, 31) as u16;
        unsafe {
            SendMessageW(
                hwnd,
                MCM_SETCURSEL,
                WPARAM(0),
                LPARAM(&st as *const _ as isize),
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, year, month, day);
    }
}

pub fn get_selected_date(handle: i64) -> f64 {
    let undefined = f64::from_bits(0x7FFC_0000_0000_0001);
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = super::get_hwnd(handle) else { return undefined };
        let mut st = SystemTime::default();
        unsafe {
            SendMessageW(
                hwnd,
                MCM_GETCURSEL,
                WPARAM(0),
                LPARAM(&mut st as *mut _ as isize),
            );
        }
        let iso = format!("{:04}-{:02}-{:02}", st.year, st.month, st.day);
        let bytes = iso.as_bytes();
        unsafe {
            let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
            js_nanbox_string(header as i64)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
        undefined
    }
}

/// Called by the WM_NOTIFY router when MCN_SELCHANGE arrives. Reads the
/// current selection via MCM_GETCURSEL and fires the registered
/// callback with `yyyy-MM-dd`.
#[cfg(target_os = "windows")]
pub fn handle_selection_change(handle: i64) {
    let Some(hwnd) = super::get_hwnd(handle) else { return };
    let on = CALENDAR_CALLBACKS.with(|m| m.borrow().get(&handle).copied().unwrap_or(0.0));
    if on == 0.0 {
        return;
    }
    let mut st = SystemTime::default();
    unsafe {
        SendMessageW(
            hwnd,
            MCM_GETCURSEL,
            WPARAM(0),
            LPARAM(&mut st as *mut _ as isize),
        );
    }
    let iso = format!("{:04}-{:02}-{:02}", st.year, st.month, st.day);
    let bytes = iso.as_bytes();
    unsafe {
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        let arg = js_nanbox_string(header as i64);
        let closure_ptr = js_nanbox_get_pointer(on) as *const u8;
        js_closure_call1(closure_ptr, arg);
    }
}
