//! Auto-split from `crates/perry-ui-tvos/src/lib.rs`. See `ffi/mod.rs`.

#![allow(clippy::missing_safety_doc)]

use crate::*;

// =============================================================================
// New Widgets: SecureField, ProgressView, Image, Picker, Form, NavStack, ZStack
// =============================================================================

/// Create a SecureField (password input). Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_securefield_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::securefield::create(placeholder_ptr as *const u8, on_change)
}

/// Create an indeterminate ProgressView (spinner). Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_progressview_create() -> i64 {
    widgets::progressview::create()
}

/// Set determinate progress value (0.0-1.0).
#[no_mangle]
pub extern "C" fn perry_ui_progressview_set_value(handle: i64, value: f64) {
    widgets::progressview::set_value(handle, value);
}

/// Create an Image from an SF Symbol name. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_symbol(name_ptr: i64) -> i64 {
    widgets::image::create_symbol(name_ptr as *const u8)
}

/// Create an Image from a file path. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_file(path_ptr: i64) -> i64 {
    widgets::image::create_file(path_ptr as *const u8)
}

/// #635 stub: remote URL images aren't fetched on tvOS yet —
/// register an empty UIImageView so layout still works.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_url(_url_ptr: i64, _alt_ptr: i64) -> i64 {
    widgets::image::create_file(0 as *const u8)
}

/// Set the size of an Image widget.
#[no_mangle]
pub extern "C" fn perry_ui_image_set_size(handle: i64, width: f64, height: f64) {
    widgets::image::set_size(handle, width, height);
}

/// Set the tint color of an Image widget.
#[no_mangle]
pub extern "C" fn perry_ui_image_set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::image::set_tint(handle, r, g, b, a);
}

/// Create a Picker (dropdown). style: 0=dropdown, 1=segmented. Returns widget handle.
#[no_mangle]
// Issue #478 — Rich text editor stubs.
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_create(_w: f64, _h: f64, _cb: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_set_string(_h: i64, _t: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_get_string(_h: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_set_html(_h: i64, _html: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_get_html(_h: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_bold(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_italic(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_underline(_h: i64) {}

// Issue #516 — PdfView stubs.
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_create(_w: f64, _h: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_load_file(_h: i64, _p: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_get_page_count(_h: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_go_to_page(_h: i64, _i: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_get_current_page(_h: i64) -> i64 {
    -1
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_set_scale(_h: i64, _s: f64) {}

// Issue #517 — MapView (tvOS MKMapView, real impl).
#[no_mangle]
pub extern "C" fn perry_ui_map_view_create(w: f64, h: f64) -> i64 {
    widgets::map_view::create(w, h)
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_region(
    handle: i64,
    lat: f64,
    lon: f64,
    lat_span: f64,
    lon_span: f64,
) {
    widgets::map_view::set_region(handle, lat, lon, lat_span, lon_span);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_add_pin(handle: i64, lat: f64, lon: f64, title_ptr: i64) {
    widgets::map_view::add_pin(handle, lat, lon, title_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_clear_pins(handle: i64) {
    widgets::map_view::clear_pins(handle);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_map_type(handle: i64, style: i64) {
    widgets::map_view::set_map_type(handle, style);
}

// Issue #477 — Command palette stubs.
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_register(_id: i64, _l: i64, _s: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_unregister(_id: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_clear() {}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_show() {}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_hide() {}

// Issue #474 — Chart widget stubs.
#[no_mangle]
pub extern "C" fn perry_ui_chart_create(_kind: i64, _w: f64, _h: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_chart_add_data_point(_h: i64, _l: i64, _v: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_chart_clear_data(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_chart_set_title(_h: i64, _t: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_chart_reload(_h: i64) {}

// Issue #481 — Calendar widget stubs.
#[no_mangle]
pub extern "C" fn perry_ui_calendar_create(_year: i64, _month: i64, _on_change: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_calendar_set_date(_h: i64, _y: i64, _m: i64, _d: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_calendar_get_selected_date(_h: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}

// Issue #473 — table sort/filter/multi-select stubs.
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_sort_change(_h: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_allows_multiple_selection(_h: i64, _allow: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_rows_count(_h: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row_at(_h: i64, _n: i64) -> i64 {
    -1
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_filter_text(_h: i64, _t: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_filter_text(_h: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}

/// TreeView stubs (#480). tvOS — outline-style focus list is future.
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_create(_id_ptr: i64, _label_ptr: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_add_child(_parent: i64, _child: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_create(_root: i64, _on_select: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_expand_all(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_collapse_all(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_get_selected_id(_handle: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}

/// Combobox stub (#475). tvOS focus model + remote-driven picker UI
/// has no native filterable-dropdown; falls back to text field.
#[no_mangle]
pub extern "C" fn perry_ui_combobox_create(initial_ptr: i64, on_change: f64) -> i64 {
    crate::ffi::core_widgets::perry_ui_textfield_create(initial_ptr, on_change)
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_add_item(_handle: i64, _value_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_set_value(_handle: i64, _value_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_get_value(_handle: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[no_mangle]
pub extern "C" fn perry_ui_picker_create(label_ptr: i64, on_change: f64, style: i64) -> i64 {
    widgets::picker::create(label_ptr as *const u8, on_change, style)
}

/// Add an item to a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_add_item(handle: i64, title_ptr: i64) {
    widgets::picker::add_item(handle, title_ptr as *const u8);
}

/// Set the selected index of a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_set_selected(handle: i64, index: i64) {
    widgets::picker::set_selected(handle, index);
}

/// Get the selected index of a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_get_selected(handle: i64) -> i64 {
    widgets::picker::get_selected(handle)
}

/// Create a TabBar. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_tabbar_create(on_change: f64) -> i64 {
    widgets::tabbar::create(on_change)
}

/// Add a tab to a TabBar.
#[no_mangle]
pub extern "C" fn perry_ui_tabbar_add_tab(handle: i64, label_ptr: i64) {
    widgets::tabbar::add_tab(handle, label_ptr as *const u8);
}

/// Set the selected tab index.
#[no_mangle]
pub extern "C" fn perry_ui_tabbar_set_selected(handle: i64, index: i64) {
    widgets::tabbar::set_selected(handle, index);
}

/// Create a Form container. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_form_create() -> i64 {
    widgets::form::form_create()
}

/// Create a Section with title. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_section_create(title_ptr: i64) -> i64 {
    widgets::form::section_create(title_ptr as *const u8)
}

/// Create a NavigationStack. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_create() -> i64 {
    // Matches the 0-arg dispatch in perry-dispatch::PERRY_UI_TABLE.
    widgets::navstack::create(std::ptr::null(), 0)
}

/// Push a view onto the NavigationStack.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_push(handle: i64, title_ptr: i64, body_handle: i64) {
    widgets::navstack::push(handle, title_ptr as *const u8, body_handle);
}

/// Pop the top view from the NavigationStack.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_pop(handle: i64) {
    widgets::navstack::pop(handle);
}

/// Create a ZStack (overlay layout). Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_zstack_create() -> i64 {
    widgets::zstack::create()
}
