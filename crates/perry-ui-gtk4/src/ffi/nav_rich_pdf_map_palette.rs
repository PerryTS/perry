// FFI: NavigationStack, RichText editor, PdfView stubs, MapView, Command palette.
use crate::widgets;

// =============================================================================
// Navigation
// =============================================================================

/// Create a NavigationStack with initial page.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_create() -> i64 {
    // Matches the 0-arg dispatch in perry-dispatch::PERRY_UI_TABLE.
    widgets::navstack::create(std::ptr::null(), 0)
}

/// Push a page onto the navigation stack.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_push(handle: i64, title_ptr: i64, body_handle: i64) {
    widgets::navstack::push(handle, title_ptr as *const u8, body_handle);
}

/// Pop the top page from the navigation stack.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_pop(handle: i64) {
    widgets::navstack::pop(handle);
}

// Issue #478 — Rich text editor — real GTK4 impl via GtkTextView + tags.
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

// Issue #516 — PdfView stubs. Linux — Poppler (libpoppler-glib) is a
// future iteration.
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

// Issue #517 — MapView via libshumate (GTK4-native vector tile widget).
#[no_mangle]
pub extern "C" fn perry_ui_map_view_create(w: f64, h: f64) -> i64 {
    widgets::map_view::create(w, h)
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_region(
    h: i64,
    lat: f64,
    lon: f64,
    lat_span: f64,
    lon_span: f64,
) {
    widgets::map_view::set_region(h, lat, lon, lat_span, lon_span);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_add_pin(h: i64, lat: f64, lon: f64, title_ptr: i64) {
    widgets::map_view::add_pin(h, lat, lon, title_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_clear_pins(h: i64) {
    widgets::map_view::clear_pins(h);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_map_type(h: i64, style: i64) {
    widgets::map_view::set_map_type(h, style);
}

// Issue #477 — Command palette — real GTK4 impl via floating GtkWindow.
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_register(id: i64, l: i64, s: i64, cb: f64) {
    widgets::command_palette::register(id as *const u8, l as *const u8, s as *const u8, cb)
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_unregister(id: i64) {
    widgets::command_palette::unregister(id as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_clear() {
    widgets::command_palette::clear()
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_show() {
    widgets::command_palette::show()
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_hide() {
    widgets::command_palette::hide()
}
