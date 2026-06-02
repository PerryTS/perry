// FFI: Command palette (#477), Chart (#474), Calendar (#481).
use crate::widgets;

// Issue #477 — Command palette stubs.
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_register(
    id: i64,
    label: i64,
    subtitle: i64,
    on_run: f64,
) {
    widgets::command_palette::register(
        id as *const u8,
        label as *const u8,
        subtitle as *const u8,
        on_run,
    );
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_unregister(id: i64) {
    widgets::command_palette::unregister(id as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_clear() {
    widgets::command_palette::clear();
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_show() {
    widgets::command_palette::show();
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_hide() {
    widgets::command_palette::hide();
}

// Issue #474 — Chart widget — real Windows impl via GDI on owner-draw HWND.
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

// Issue #481 — Calendar widget — real Windows impl via SysMonthCal32.
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
