//! Slider widget — Win32 TRACKBAR_CLASS (msctls_trackbar32)

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct SliderInfo {
    min: f64,
    max: f64,
    callback_ptr: *const u8,
}

thread_local! {
    static SLIDER_INFO: RefCell<HashMap<i64, SliderInfo>> = RefCell::new(HashMap::new());
}

/// TBM_GETPOS is not exported by the windows crate 0.58 — define it manually.
/// WM_USER (0x0400) + 0 = 1024
#[cfg(target_os = "windows")]
const TBM_GETPOS: u32 = 1024;

/// The trackbar uses integer positions. We map [min, max] to [0, 1000] internally.
const TRACKBAR_RANGE: i32 = 1000;

fn value_to_pos(value: f64, min: f64, max: f64) -> i32 {
    if (max - min).abs() < f64::EPSILON {
        return 0;
    }
    (((value - min) / (max - min)) * TRACKBAR_RANGE as f64) as i32
}

fn pos_to_value(pos: i32, min: f64, max: f64) -> f64 {
    min + (pos as f64 / TRACKBAR_RANGE as f64) * (max - min)
}

/// Create a Slider. Returns widget handle.
pub fn create(min: f64, max: f64, initial: f64, on_change: f64) -> i64 {
    let callback_ptr = unsafe { js_nanbox_get_pointer(on_change) } as *const u8;
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                TRACKBAR_CLASSW,
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(
                    TBS_HORZ as u32
                        | TBS_AUTOTICKS as u32
                        | WS_CHILD.0
                        | WS_VISIBLE.0
                        | WS_TABSTOP.0,
                ),
                0,
                0,
                200,
                24,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            // Set range [0, 1000]
            SendMessageW(hwnd, TBM_SETRANGEMIN, WPARAM(0), LPARAM(0));
            SendMessageW(
                hwnd,
                TBM_SETRANGEMAX,
                WPARAM(1),
                LPARAM(TRACKBAR_RANGE as isize),
            );

            // Set initial position
            let pos = value_to_pos(initial, min, max);
            SendMessageW(hwnd, TBM_SETPOS, WPARAM(1), LPARAM(pos as isize));

            let handle = register_widget(hwnd, WidgetKind::Slider, control_id);
            SLIDER_INFO.with(|info| {
                info.borrow_mut().insert(
                    handle,
                    SliderInfo {
                        min,
                        max,
                        callback_ptr,
                    },
                );
            });

            #[cfg(feature = "geisterhand")]
            {
                extern "C" {
                    fn perry_geisterhand_register(
                        handle: i64,
                        widget_type: u8,
                        callback_kind: u8,
                        closure_f64: f64,
                        label_ptr: *const u8,
                    );
                }
                perry_geisterhand_register(handle, 2, 1, on_change, std::ptr::null());
            }

            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let handle = register_widget(0, WidgetKind::Slider, control_id);
        SLIDER_INFO.with(|info| {
            info.borrow_mut().insert(
                handle,
                SliderInfo {
                    min,
                    max,
                    callback_ptr,
                },
            );
        });

        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_register(
                    handle: i64,
                    widget_type: u8,
                    callback_kind: u8,
                    closure_f64: f64,
                    label_ptr: *const u8,
                );
            }
            unsafe {
                perry_geisterhand_register(handle, 2, 1, on_change, std::ptr::null());
            }
        }

        handle
    }
}

/// Handle WM_HSCROLL from trackbar — read position and call callback.
pub fn handle_scroll(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            let pos = unsafe { SendMessageW(hwnd, TBM_GETPOS, WPARAM(0), LPARAM(0)).0 as i32 };

            SLIDER_INFO.with(|info| {
                let info = info.borrow();
                if let Some(si) = info.get(&handle) {
                    let value = pos_to_value(pos, si.min, si.max);
                    unsafe { js_closure_call1(si.callback_ptr, value) };
                }
            });
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

/// Set the slider value (for two-way state binding).
pub fn set_value(handle: i64, value: f64) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            SLIDER_INFO.with(|info| {
                let info = info.borrow();
                if let Some(si) = info.get(&handle) {
                    let pos = value_to_pos(value, si.min, si.max);
                    unsafe {
                        SendMessageW(hwnd, TBM_SETPOS, WPARAM(1), LPARAM(pos as isize));
                    }
                }
            });
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, value);
    }
}
