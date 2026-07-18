// FFI: widget create/factory functions.
use crate::{app, widgets};

/// Create a Text label.
#[no_mangle]
pub extern "C" fn perry_ui_text_create(text_ptr: i64) -> i64 {
    widgets::text::create(text_ptr as *const u8)
}

/// Create an AdBanner placeholder (#867). `unit_id_ptr` / `size_ptr` are
/// string pointers per the `AdBanner(unitId, size)` dispatch row. This
/// symbol was previously absent from the Windows staticlib, so any TS
/// program using AdBanner failed at link time.
#[no_mangle]
pub extern "C" fn perry_ui_adbanner_create(unit_id_ptr: i64, size_ptr: i64) -> i64 {
    widgets::ad_banner::create(unit_id_ptr as *const u8, size_ptr as *const u8)
}

/// Create a Button.
#[no_mangle]
pub extern "C" fn perry_ui_button_create(label_ptr: i64, on_press: f64) -> i64 {
    widgets::button::create(label_ptr as *const u8, on_press)
}

/// Create a VStack container (spacing DPI-scaled).
#[no_mangle]
pub extern "C" fn perry_ui_vstack_create(spacing: f64) -> i64 {
    widgets::vstack::create(spacing * app::get_dpi_scale())
}

/// Create an HStack container (spacing DPI-scaled).
#[no_mangle]
pub extern "C" fn perry_ui_hstack_create(spacing: f64) -> i64 {
    widgets::hstack::create(spacing * app::get_dpi_scale())
}

/// Create a Spacer.
#[no_mangle]
pub extern "C" fn perry_ui_spacer_create() -> i64 {
    widgets::spacer::create()
}

/// Create a Divider.
#[no_mangle]
pub extern "C" fn perry_ui_divider_create() -> i64 {
    widgets::divider::create()
}

/// Create a TextField.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::textfield::create(placeholder_ptr as *const u8, on_change)
}

/// Create a SecureField (password entry).
#[no_mangle]
pub extern "C" fn perry_ui_securefield_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::securefield::create(placeholder_ptr as *const u8, on_change)
}

/// Create a Toggle.
#[no_mangle]
pub extern "C" fn perry_ui_toggle_create(label_ptr: i64, on_change: f64) -> i64 {
    widgets::toggle::create(label_ptr as *const u8, on_change)
}

/// Create a Slider.
#[no_mangle]
pub extern "C" fn perry_ui_slider_create(min: f64, max: f64, on_change: f64) -> i64 {
    // Codegen emits 3-arg `Slider(min, max, onChange)`; default initial=min.
    widgets::slider::create(min, max, min, on_change)
}

/// Create a ScrollView.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_create() -> i64 {
    widgets::scrollview::create()
}

/// Create a Canvas.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_create(width: f64, height: f64) -> i64 {
    widgets::canvas::create(width, height)
}

/// Create a BloomView render-surface host (issue #2395). Returns a widget
/// handle; pair with `perry_ui_bloomview_get_hwnd` to embed an external GPU
/// renderer (the Bloom engine) into the reserved child window.
#[no_mangle]
pub extern "C" fn perry_ui_bloomview_create(width: f64, height: f64) -> i64 {
    widgets::bloomview::create(width, height)
}

/// Return the raw HWND value for a BloomView handle (as an integer), so user
/// TypeScript can hand it to the Bloom package's `attachToHwnd`.
#[no_mangle]
pub extern "C" fn perry_ui_bloomview_get_hwnd(handle: i64) -> i64 {
    widgets::bloomview::get_hwnd_value(handle)
}

/// Create a Form container.
#[no_mangle]
pub extern "C" fn perry_ui_form_create() -> i64 {
    widgets::form::create()
}

/// Create a Section with title.
#[no_mangle]
pub extern "C" fn perry_ui_section_create(title_ptr: i64) -> i64 {
    widgets::form::section_create(title_ptr as *const u8)
}

/// Create a ZStack (overlay container).
#[no_mangle]
pub extern "C" fn perry_ui_zstack_create() -> i64 {
    widgets::zstack::create()
}

/// Create a LazyVStack.
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_create(count: f64, render_closure: f64) -> i64 {
    widgets::lazyvstack::create(count, render_closure)
}

/// Update a LazyVStack with a new item count.
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_update(handle: i64, count: i64) {
    widgets::lazyvstack::update(handle, count);
}

/// Advisory on Windows — eager-render path doesn't consult row_height yet.
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_row_height(_handle: i64, _height: f64) {}

// Table (stub — not yet implemented on Windows)
#[no_mangle]
pub extern "C" fn perry_ui_table_create(row_count: f64, col_count: f64, render: f64) -> i64 {
    widgets::table::create(row_count, col_count, render)
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_header(handle: i64, col: i64, title_ptr: i64) {
    widgets::table::set_column_header(handle, col, title_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_width(handle: i64, col: i64, width: f64) {
    widgets::table::set_column_width(handle, col, width);
}
#[no_mangle]
pub extern "C" fn perry_ui_table_update_row_count(handle: i64, count: i64) {
    widgets::table::update_row_count(handle, count);
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_row_select(handle: i64, callback: f64) {
    widgets::table::set_on_row_select(handle, callback);
}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row(handle: i64) -> i64 {
    widgets::table::get_selected_row(handle)
}

/// Create a ProgressView.
#[no_mangle]
pub extern "C" fn perry_ui_progressview_create() -> i64 {
    widgets::progressview::create()
}

/// Set progress value (0.0-1.0, negative = indeterminate).
#[no_mangle]
pub extern "C" fn perry_ui_progressview_set_value(handle: i64, value: f64) {
    widgets::progressview::set_value(handle, value);
}

// =============================================================================
// Layout — VStack/HStack with insets
// =============================================================================

/// Create a VStack with custom insets (DPI-scaled).
#[no_mangle]
pub extern "C" fn perry_ui_vstack_create_with_insets(
    spacing: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> i64 {
    let s = app::get_dpi_scale();
    widgets::vstack::create_with_insets(spacing * s, top * s, left * s, bottom * s, right * s)
}

/// Create an HStack with custom insets (DPI-scaled).
#[no_mangle]
pub extern "C" fn perry_ui_hstack_create_with_insets(
    spacing: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> i64 {
    let s = app::get_dpi_scale();
    widgets::hstack::create_with_insets(spacing * s, top * s, left * s, bottom * s, right * s)
}
