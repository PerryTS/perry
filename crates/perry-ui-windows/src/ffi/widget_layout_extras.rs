// FFI: widget sizing/layout/overlay/insets, button color, embed HWND,
// plugin stubs, QR code, scroll-refresh stubs, stack distribution.
use crate::{app, widgets};

/// Add a child widget at a specific index.
#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child_at(parent_handle: i64, child_handle: i64, index: f64) {
    widgets::add_child_at(parent_handle, child_handle, index as i64);
    app::request_layout();
}

// =============================================================================
// Stubs for symbols referenced by codegen but not yet implemented on Windows
// =============================================================================

/// Set button text color.
#[no_mangle]
pub extern "C" fn perry_ui_button_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::button::set_text_color(handle, r, g, b, a);
}

/// Set widget width (DPI-scaled).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_width(handle: i64, width: f64) {
    let scaled = (width * app::get_dpi_scale()) as i32;
    widgets::set_fixed_width(handle, scaled);
}

/// Set widget hugging priority.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_hugging(handle: i64, priority: f64) {
    widgets::set_hugging_priority(handle, priority);
}

/// Set on-click callback (stub — not yet implemented on Windows).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_click(handle: i64, callback: f64) {
    let _ = handle;
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
            perry_geisterhand_register(handle, 0, 0, callback, std::ptr::null());
        }
    }
}

/// Set widget height (fixed, DPI-scaled).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_height(handle: i64, height: f64) {
    let scaled = (height * app::get_dpi_scale()) as i32;
    widgets::set_fixed_height(handle, scaled);
}

/// Match parent height — marks the widget to stretch vertically to fill its parent.
#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_height(handle: i64) {
    widgets::set_match_parent_height(handle, true);
}

/// Match parent width — marks the widget to stretch horizontally to fill its parent.
#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_width(handle: i64) {
    widgets::set_match_parent_width(handle, true);
}

/// Set hidden state (perry_ui_widget_set_hidden — matches macOS naming convention).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_hidden(handle: i64, hidden: i64) {
    widgets::set_hidden(handle, hidden != 0);
}

/// Stack: detach hidden children from layout calculation.
/// When enabled, hidden children don't occupy any space.
#[no_mangle]
pub extern "C" fn perry_ui_stack_set_detaches_hidden(handle: i64, flag: i64) {
    widgets::set_detaches_hidden(handle, flag != 0);
}

/// Embed a native HWND into the Perry widget system.
/// Takes the HWND pointer value and returns a 1-based widget handle.
/// The widget is marked as fills_remaining so it absorbs remaining space in VStack/HStack.
#[no_mangle]
pub extern "C" fn perry_ui_embed_nsview(hwnd_ptr: i64) -> i64 {
    if hwnd_ptr == 0 {
        return 0;
    }
    #[cfg(target_os = "windows")]
    {
        let hwnd = windows::Win32::Foundation::HWND(hwnd_ptr as *mut std::ffi::c_void);
        let handle = widgets::register_widget(hwnd, widgets::WidgetKind::Canvas, 0);
        widgets::set_fills_remaining(handle, true);
        handle
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd_ptr;
        0
    }
}

/// Request location permission (stub — not available on Windows desktop).
#[no_mangle]
pub extern "C" fn perry_system_request_location(_callback: f64) {}

/// Load a plugin (stub — not yet implemented on Windows).
#[no_mangle]
pub extern "C" fn perry_plugin_load(_path_ptr: i64) -> i64 {
    0
}

/// Unload a plugin (stub — not yet implemented on Windows).
#[no_mangle]
pub extern "C" fn perry_plugin_unload(_handle: i64) {}

// NOTE: backOff, js_crypto_random_bytes_buffer, js_fetch_*, js_ws_handle_to_i64,
// and js_fetch_stream_status are provided by perry-stdlib. When linking the IDE
// (which uses both perry-stdlib and perry-ui-windows), these stubs caused
// duplicate symbol errors (LNK2005). Removed — perry-stdlib provides the real
// implementations.

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_create(data_ptr: i64, size: f64) -> i64 {
    widgets::qrcode::create(data_ptr as *const u8, size)
}

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_set_data(handle: i64, data_ptr: i64) {
    widgets::qrcode::set_data(handle, data_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_end_refreshing(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_refresh_control(_handle: i64, _callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_stack_set_distribution(handle: i64, distribution: f64) {
    // Dispatch declares this as `[Widget, F64]` (matches every other platform).
    // The Windows runtime previously took `i64` — on Win64 ABI the f64 arg lands
    // in XMM1 while `i64` is read from RDX (uninitialized garbage), so the
    // distribution enum tag was random.
    widgets::set_distribution(handle, distribution as i64);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_reorder_child(_parent: i64, _child: i64, _index: i64) {}

// perry_debug_trace_init and perry_debug_trace_init_done are provided by perry_runtime

// =============================================================================
// Stack alignment + Widget overlay & edge insets
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_stack_set_alignment(handle: i64, alignment: f64) {
    widgets::set_alignment(handle, alignment as i64);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_overlay(_parent: i64, _child: i64) {
    // For now, treat as regular add_child
    widgets::add_child(_parent, _child);
    app::request_layout();
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_overlay_frame(
    _handle: i64,
    _x: f64,
    _y: f64,
    _w: f64,
    _h: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_edge_insets(
    handle: i64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) {
    widgets::set_insets(handle, top, left, bottom, right);
}
