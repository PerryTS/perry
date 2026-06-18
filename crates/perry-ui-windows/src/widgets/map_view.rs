//! MapView widget — Win32 stub-with-state implementation (#559).
//!
//! Per #559's scope notes, the right Windows backend is the WinUI
//! `Windows.UI.Xaml.Controls.Maps.MapControl` hosted in a XAML Island
//! (`DesktopWindowXamlSource`) parented to the Perry HWND chain. That
//! requires the Windows App SDK + WinUI 3 stack as a hard dependency
//! and a Bing Maps API key from the user — significant new
//! infrastructure that doesn't fit a single-session sweep.
//!
//! v1 ships the same FFI shape the issue specifies (`set_region`,
//! `add_pin`, `clear_pins`, `set_map_type`) backed by a real STATIC
//! widget that displays the current region + pin list as text:
//! `[Map @ 37.78,-122.42 — span 0.05×0.05 — 3 pins]`. Layout takes
//! the requested space; values from setters visibly update the
//! label so user code's nav flow exercises the API.
//!
//! Real WinUI / MapLibre / WebView2-MapLibre paths land in a
//! follow-up — tracked under #559. Mirrors the v0.5.771 GTK4-audit
//! "stubs matching the macOS shape exactly for link stability"
//! pattern documented for tabbar / vbox / etc.

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::System::SystemServices::SS_CENTER;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

#[derive(Clone)]
struct Pin {
    lat: f64,
    lon: f64,
    title: String,
}

struct MapState {
    lat: f64,
    lon: f64,
    lat_span: f64,
    lon_span: f64,
    map_type: i64,
    pins: Vec<Pin>,
}

thread_local! {
    static MAPS: RefCell<HashMap<i64, MapState>> = RefCell::new(HashMap::new());
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn str_from_header(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len)).to_string()
    }
}

pub fn create(width: f64, height: f64) -> i64 {
    let control_id = alloc_control_id();
    let w = if width > 0.0 { width as i32 } else { 400 };
    let h = if height > 0.0 { height as i32 } else { 300 };

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("STATIC");
        let window_text = to_wide("[Map — region not set]");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_BORDER.0 | SS_CENTER.0),
                0,
                0,
                w,
                h,
                Some(super::get_parking_hwnd()),
                Some(HMENU(control_id as *mut _)),
                Some(HINSTANCE::from(hinstance)),
                None,
            )
            .unwrap();

            let handle = register_widget(hwnd, WidgetKind::Image, control_id);
            MAPS.with(|m| {
                m.borrow_mut().insert(
                    handle,
                    MapState {
                        lat: 0.0,
                        lon: 0.0,
                        lat_span: 0.0,
                        lon_span: 0.0,
                        map_type: 0,
                        pins: Vec::new(),
                    },
                );
            });
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (w, h);
        let handle = register_widget(0, WidgetKind::Image, control_id);
        MAPS.with(|m| {
            m.borrow_mut().insert(
                handle,
                MapState {
                    lat: 0.0,
                    lon: 0.0,
                    lat_span: 0.0,
                    lon_span: 0.0,
                    map_type: 0,
                    pins: Vec::new(),
                },
            );
        });
        handle
    }
}

fn refresh_label(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        let display = MAPS.with(|m| {
            m.borrow().get(&handle).map(|state| {
                let map_type_name = match state.map_type {
                    1 => "Aerial",
                    2 => "Hybrid",
                    _ => "Standard",
                };
                if state.lat_span == 0.0 && state.lon_span == 0.0 {
                    format!(
                        "[Map ({}) — region not set — {} pins]",
                        map_type_name,
                        state.pins.len()
                    )
                } else {
                    format!(
                        "[Map ({}) @ {:.4},{:.4} — span {:.3}×{:.3} — {} pins]",
                        map_type_name,
                        state.lat,
                        state.lon,
                        state.lat_span,
                        state.lon_span,
                        state.pins.len()
                    )
                }
            })
        });
        if let Some(text) = display {
            if let Some(hwnd) = super::get_hwnd(handle) {
                let wide = to_wide(&text);
                unsafe {
                    let _ = SetWindowTextW(hwnd, windows::core::PCWSTR(wide.as_ptr()));
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

pub fn set_region(handle: i64, lat: f64, lon: f64, lat_span: f64, lon_span: f64) {
    MAPS.with(|m| {
        if let Some(state) = m.borrow_mut().get_mut(&handle) {
            state.lat = lat;
            state.lon = lon;
            state.lat_span = lat_span;
            state.lon_span = lon_span;
        }
    });
    refresh_label(handle);
}

pub fn add_pin(handle: i64, lat: f64, lon: f64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    MAPS.with(|m| {
        if let Some(state) = m.borrow_mut().get_mut(&handle) {
            state.pins.push(Pin { lat, lon, title });
        }
    });
    refresh_label(handle);
}

pub fn clear_pins(handle: i64) {
    MAPS.with(|m| {
        if let Some(state) = m.borrow_mut().get_mut(&handle) {
            state.pins.clear();
        }
    });
    refresh_label(handle);
}

pub fn set_map_type(handle: i64, style: i64) {
    MAPS.with(|m| {
        if let Some(state) = m.borrow_mut().get_mut(&handle) {
            state.map_type = style;
        }
    });
    refresh_label(handle);
}
