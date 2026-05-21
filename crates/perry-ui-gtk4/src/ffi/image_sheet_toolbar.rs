// FFI: Image, Sheet (modal), Toolbar.
use crate::{sheet, toolbar, widgets};

// =============================================================================
// Image
// =============================================================================

/// Create an image from a file path.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_file(path_ptr: i64) -> i64 {
    widgets::image::create_file(path_ptr as *const u8)
}

/// Create an image from a named icon/symbol.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_symbol(name_ptr: i64) -> i64 {
    widgets::image::create_symbol(name_ptr as *const u8)
}

/// #635 stub: remote URL images aren't fetched on GTK4 yet —
/// register an empty image widget so layout still works.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_url(_url_ptr: i64, _alt_ptr: i64) -> i64 {
    widgets::image::create_symbol(0 as *const u8)
}

/// Set the size of an image.
#[no_mangle]
pub extern "C" fn perry_ui_image_set_size(handle: i64, width: f64, height: f64) {
    widgets::image::set_size(handle, width, height);
}

/// Set the tint color of an image.
#[no_mangle]
pub extern "C" fn perry_ui_image_set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::image::set_tint(handle, r, g, b, a);
}

// =============================================================================
// Sheet
// =============================================================================

/// Create a sheet (modal window). #1033: signature aligned with the
/// perry-dispatch row `[Widget, F64, F64]`.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_create(body_handle: i64, width: f64, height: f64) -> i64 {
    sheet::create(body_handle, width, height)
}

/// Present (show) a sheet.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_present(sheet_handle: i64) {
    sheet::present(sheet_handle);
}

/// Dismiss (close) a sheet.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_dismiss(sheet_handle: i64) {
    sheet::dismiss(sheet_handle);
}

// =============================================================================
// Toolbar
// =============================================================================

/// Create a toolbar.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_create() -> i64 {
    toolbar::create()
}

/// Add an item to a toolbar.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_add_item(
    toolbar_handle: i64,
    label_ptr: i64,
    icon_ptr: i64,
    callback: f64,
) {
    toolbar::add_item(
        toolbar_handle,
        label_ptr as *const u8,
        icon_ptr as *const u8,
        callback,
    );
}

/// Attach a toolbar to the current window.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_attach(toolbar_handle: i64) {
    toolbar::attach(toolbar_handle);
}
