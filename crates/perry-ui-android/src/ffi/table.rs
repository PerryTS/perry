use crate::{catch_panic, catch_panic_void, widgets::table};

#[no_mangle]
pub extern "C" fn perry_ui_table_create(rows: f64, columns: f64, render: f64) -> i64 {
    catch_panic("table_create", || table::create(rows, columns, render))
}

#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_header(handle: i64, col: i64, title: i64) {
    catch_panic_void("table_set_column_header", || {
        table::set_column_header(handle, col, title)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_width(handle: i64, col: i64, width: f64) {
    catch_panic_void("table_set_column_width", || {
        table::set_column_width(handle, col, width)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_table_update_row_count(handle: i64, count: i64) {
    catch_panic_void("table_update_row_count", || {
        table::update_row_count(handle, count)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_row_select(handle: i64, closure: f64) {
    catch_panic_void("table_set_on_row_select", || {
        table::set_on_row_select(handle, closure)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row(handle: i64) -> i64 {
    catch_panic("table_get_selected_row", || table::get_selected_row(handle))
}

#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_sort_change(handle: i64, closure: f64) {
    catch_panic_void("table_set_on_sort_change", || {
        table::set_on_sort_change(handle, closure)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_table_set_allows_multiple_selection(handle: i64, allow: i64) {
    catch_panic_void("table_set_allows_multiple_selection", || {
        table::set_allows_multiple_selection(handle, allow)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_rows_count(handle: i64) -> i64 {
    catch_panic("table_get_selected_rows_count", || {
        table::get_selected_rows_count(handle)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row_at(handle: i64, index: i64) -> i64 {
    catch_panic("table_get_selected_row_at", || {
        table::get_selected_row_at(handle, index)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_table_set_filter_text(handle: i64, title: i64) {
    catch_panic_void("table_set_filter_text", || {
        table::set_filter_text(handle, title)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_table_get_filter_text(handle: i64) -> i64 {
    catch_panic("table_get_filter_text", || table::get_filter_text(handle))
}
