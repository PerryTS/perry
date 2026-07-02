// FFI: table sort/filter/multi-select (#473), TreeView (#480), ComboBox (#475), Picker.
use crate::widgets;

// Issue #473 — table sort / filter / multi-select. Real LVS_REPORT impl
// lives in `widgets::table`; the bare ABI shape mirrors macOS so dispatch
// signatures match across platforms (#7 — ListView with column headers).
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_sort_change(h: i64, cb: f64) {
    widgets::table::set_on_sort_change(h, cb);
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_allows_multiple_selection(h: i64, allow: i64) {
    widgets::table::set_allows_multiple_selection(h, allow);
}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_rows_count(h: i64) -> i64 {
    widgets::table::get_selected_rows_count(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row_at(h: i64, n: i64) -> i64 {
    widgets::table::get_selected_row_at(h, n)
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_filter_text(h: i64, t: i64) {
    widgets::table::set_filter_text(h, t as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_filter_text(h: i64) -> f64 {
    let ptr = widgets::table::get_filter_text(h);
    // Wrap as NaN-boxed STRING_TAG (top16 = 0x7FFF, lower 48 = pointer).
    f64::from_bits(0x7FFF_0000_0000_0000_u64 | (ptr as u64 & 0x0000_FFFF_FFFF_FFFF))
}

/// TreeView (#480) — real Win32 impl via SysTreeView32 + TVN_SELCHANGEDW.
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_create(id_ptr: i64, label_ptr: i64) -> i64 {
    widgets::tree_view::node_create(id_ptr as *const u8, label_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_add_child(parent: i64, child: i64) {
    widgets::tree_view::node_add_child(parent, child)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_create(root: i64, on_select: f64) -> i64 {
    widgets::tree_view::create(root, on_select)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_expand_all(handle: i64) {
    widgets::tree_view::expand_all(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_collapse_all(handle: i64) {
    widgets::tree_view::collapse_all(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_get_selected_id(handle: i64) -> f64 {
    widgets::tree_view::get_selected_id(handle)
}

/// Combobox (#475) — real Win32 impl via CBS_DROPDOWN ComboBox.
#[no_mangle]
pub extern "C" fn perry_ui_combobox_create(initial_ptr: i64, on_change: f64) -> i64 {
    widgets::combobox::create(initial_ptr as *const u8, on_change)
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_add_item(handle: i64, value_ptr: i64) {
    widgets::combobox::add_item(handle, value_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_set_value(handle: i64, value_ptr: i64) {
    widgets::combobox::set_value(handle, value_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_get_value(handle: i64) -> f64 {
    widgets::combobox::get_value(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_picker_create(on_change: f64) -> i64 {
    // The dispatch table (perry-dispatch `ui_table`) passes a single
    // `Closure` arg, matching the TS `Picker(onChange)` API. A 3-arg
    // `(label_ptr, on_change, style)` signature mis-binds `on_change` on
    // the Windows x64 ABI, where register slots are assigned by argument
    // position regardless of type: the lone f64 lands in XMM0, but a
    // positional arg-1 f64 is read from XMM1, so the callback pointer is
    // never stored and `onChange` never fires (issue #5491). On SysV
    // (macOS/Linux) float args are classed independently, so it happened
    // to work. label/style were never wired from TS — pass null/0.
    widgets::picker::create(std::ptr::null(), on_change, 0)
}

/// Add an item to a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_add_item(handle: i64, title_ptr: i64) {
    widgets::picker::add_item(handle, title_ptr as *const u8);
}

/// Set the selected item of a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_set_selected(handle: i64, index: i64) {
    widgets::picker::set_selected(handle, index);
}

/// Get the selected item of a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_get_selected(handle: i64) -> i64 {
    widgets::picker::get_selected(handle)
}
