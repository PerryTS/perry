//! FFI exports for the extended widget catalogue: SecureField, ProgressView,
//! Image, RichText, PdfView, MapView, command-palette stubs, Chart, Calendar,
//! Table stubs, TreeView, Combobox, Picker, TabBar, Form, NavStack, ZStack.
//! Behavior is unchanged from the pre-split `lib.rs`.

use super::*;

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

/// #635 stub: remote URL images aren't fetched on visionOS yet —
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

// Issue #478 — Rich text editor — visionOS impl via UITextView.
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_create(w: f64, h: f64, cb: f64) -> i64 {
    widgets::rich_text::create(w, h, cb)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_set_string(h: i64, t: i64) {
    widgets::rich_text::set_string(h, t as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_get_string(h: i64) -> f64 {
    widgets::rich_text::get_string(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_set_html(h: i64, html: i64) -> i64 {
    widgets::rich_text::set_html(h, html as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_get_html(h: i64) -> f64 {
    widgets::rich_text::get_html(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_bold(h: i64) {
    widgets::rich_text::toggle_bold(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_italic(h: i64) {
    widgets::rich_text::toggle_italic(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_underline(h: i64) {
    widgets::rich_text::toggle_underline(h)
}

// Issue #516 — PdfView — visionOS impl via PDFView.
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_create(w: f64, h: f64) -> i64 {
    widgets::pdf_view::create(w, h)
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_load_file(h: i64, p: i64) -> i64 {
    if widgets::pdf_view::load_file(h, p as *const u8) {
        1
    } else {
        0
    }
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_get_page_count(h: i64) -> i64 {
    widgets::pdf_view::get_page_count(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_go_to_page(h: i64, i: i64) {
    widgets::pdf_view::go_to_page(h, i)
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_get_current_page(h: i64) -> i64 {
    widgets::pdf_view::get_current_page(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_set_scale(h: i64, s: f64) {
    widgets::pdf_view::set_scale(h, s)
}

// Issue #517 — MapView — visionOS impl via MKMapView.
#[no_mangle]
pub extern "C" fn perry_ui_map_view_create(w: f64, h: f64) -> i64 {
    widgets::map_view::create(w, h)
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_region(h: i64, lat: f64, lon: f64, ls: f64, os: f64) {
    widgets::map_view::set_region(h, lat, lon, ls, os)
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_add_pin(h: i64, lat: f64, lon: f64, t: i64) {
    widgets::map_view::add_pin(h, lat, lon, t as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_clear_pins(h: i64) {
    widgets::map_view::clear_pins(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_map_type(h: i64, s: i64) {
    widgets::map_view::set_map_type(h, s)
}

// Issue #477 — Command palette stubs (defer — needs spatial-aware presentation).
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

// Issue #474 — Chart — visionOS impl via UIView+CoreGraphics.
#[no_mangle]
pub extern "C" fn perry_ui_chart_create(kind: i64, w: f64, h: f64) -> i64 {
    widgets::chart::create(kind, w, h)
}
#[no_mangle]
pub extern "C" fn perry_ui_chart_add_data_point(h: i64, l: i64, v: f64) {
    widgets::chart::add_data_point(h, l as *const u8, v)
}
#[no_mangle]
pub extern "C" fn perry_ui_chart_clear_data(h: i64) {
    widgets::chart::clear_data(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_chart_set_title(h: i64, t: i64) {
    widgets::chart::set_title(h, t as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_chart_reload(h: i64) {
    widgets::chart::reload(h)
}

// Issue #481 — Calendar — visionOS impl via UIDatePicker.inline.
#[no_mangle]
pub extern "C" fn perry_ui_calendar_create(year: i64, month: i64, on_change: f64) -> i64 {
    widgets::calendar::create(year, month, on_change)
}
#[no_mangle]
pub extern "C" fn perry_ui_calendar_set_date(h: i64, y: i64, m: i64, d: i64) {
    widgets::calendar::set_date(h, y, m, d)
}
#[no_mangle]
pub extern "C" fn perry_ui_calendar_get_selected_date(h: i64) -> f64 {
    widgets::calendar::get_selected_date(h)
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

/// TreeView (#480). visionOS — UITableView depth-flattened tree (same
/// UIKit impl as iOS).
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_create(id_ptr: i64, label_ptr: i64) -> i64 {
    widgets::tree_view::node_create(id_ptr as *const u8, label_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_add_child(parent: i64, child: i64) {
    widgets::tree_view::node_add_child(parent, child);
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_create(root: i64, on_select: f64) -> i64 {
    widgets::tree_view::create(root, on_select)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_expand_all(handle: i64) {
    widgets::tree_view::expand_all(handle);
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_collapse_all(handle: i64) {
    widgets::tree_view::collapse_all(handle);
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_get_selected_id(handle: i64) -> f64 {
    widgets::tree_view::get_selected_id(handle)
}

/// Combobox (#475). visionOS — UITextField + UIPickerView inputView
/// (UIKit, same impl as iOS).
#[no_mangle]
pub extern "C" fn perry_ui_combobox_create(initial_ptr: i64, on_change: f64) -> i64 {
    widgets::combobox::create(initial_ptr as *const u8, on_change)
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_add_item(handle: i64, value_ptr: i64) {
    widgets::combobox::add_item(handle, value_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_set_value(handle: i64, value_ptr: i64) {
    widgets::combobox::set_value(handle, value_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_get_value(handle: i64) -> f64 {
    widgets::combobox::get_value(handle)
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
