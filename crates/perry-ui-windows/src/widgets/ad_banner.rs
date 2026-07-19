//! `AdBanner` widget (#867) — Win32 layout placeholder.
//!
//! Google Mobile Ads ships no first-party Windows SDK, so — exactly like
//! the macOS backend (`perry-ui-macos/src/widgets/adbanner.rs`) — the
//! banner is a **layout placeholder**: an empty STATIC child sized to the
//! requested banner slot, so a `perry/ui` layout developed or previewed
//! on Windows reserves exactly the space the real ad occupies on
//! iOS/Android. The `unitId` is accepted for cross-platform API parity
//! but unused since nothing loads.
//!
//! Before this existed, `perry_ui_adbanner_create` was simply absent from
//! the Windows staticlib: any TS program using `AdBanner(...)` failed at
//! link time with an unresolved external (the symbol is a required
//! dispatch row in `perry-dispatch/src/ui_table/part_a.rs`).

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        // Checked parse, not from_utf8_unchecked: runtime strings are WTF-8
        // and may contain lone surrogates, which would be UB to expose as
        // &str. An invalid size key falls back to "" → the default 320×50
        // banner slot.
        std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap_or("")
    }
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Banner dimensions in logical pixels for each size key the d.ts exposes.
/// Values match Google Mobile Ads' standard `AdSize` constants (and the
/// macOS placeholder) so the reserved slot lines up with the real
/// iOS/Android banner.
fn banner_size(size_key: &str) -> (f64, f64) {
    match size_key {
        "large-banner" => (320.0, 100.0),
        "medium-rectangle" => (300.0, 250.0),
        "full-banner" => (468.0, 60.0),
        "leaderboard" => (728.0, 90.0),
        // "banner" / "adaptive" / empty / unknown → standard 320×50.
        _ => (320.0, 50.0),
    }
}

/// Create the banner placeholder sized per `size_ptr`. Returns the widget
/// handle. The slot dimensions are pinned via fixed width/height so the
/// stack layout reserves them exactly (matching the macOS NSView frame).
pub fn create(unit_id_ptr: *const u8, size_ptr: *const u8) -> i64 {
    let _unit_id = str_from_header(unit_id_ptr);
    let size_key = str_from_header(size_ptr);
    let (w, h) = banner_size(size_key);
    let scale = crate::app::get_dpi_scale();
    let w = (w * scale).round() as i32;
    let h = (h * scale).round() as i32;
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("STATIC");
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
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
            super::set_fixed_width(handle, w);
            super::set_fixed_height(handle, h);
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let handle = register_widget(0, WidgetKind::Image, control_id);
        super::set_fixed_width(handle, w);
        super::set_fixed_height(handle, h);
        handle
    }
}
