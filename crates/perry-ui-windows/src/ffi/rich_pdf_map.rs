// FFI: RichText editor (#478), PDF view (#516), MapView (#517 / #559).
use crate::widgets;

// Issue #478 — Rich text editor — real Windows impl via RichEdit (MSFTEDIT_CLASS).
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

// PdfView (#516) — Win32 stub-with-state. STATIC label shows
// "[PDF: name — page X/Y @ Z%]" on load + nav. Real page-bitmap
// rendering via `Windows.Data.Pdf` WinRT or PDFium is a follow-up.
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_create(w: f64, h: f64) -> i64 {
    widgets::pdf_view::create(w, h)
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_load_file(h: i64, p: i64) -> i64 {
    widgets::pdf_view::load_file(h, p as *const u8)
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

// MapView (#517 / #559) — Win32 stub-with-state. STATIC label shows
// the current region + pin count. Real WinUI MapControl in XAML
// Islands needs Windows App SDK + WinUI 3 stack + Bing Maps API key
// — tracked under #559 as multi-day follow-up.
#[no_mangle]
pub extern "C" fn perry_ui_map_view_create(w: f64, h: f64) -> i64 {
    widgets::map_view::create(w, h)
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_region(h: i64, lat: f64, lon: f64, ls: f64, os: f64) {
    widgets::map_view::set_region(h, lat, lon, ls, os);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_add_pin(h: i64, lat: f64, lon: f64, t: i64) {
    widgets::map_view::add_pin(h, lat, lon, t as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_clear_pins(h: i64) {
    widgets::map_view::clear_pins(h);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_map_type(h: i64, s: i64) {
    widgets::map_view::set_map_type(h, s);
}
