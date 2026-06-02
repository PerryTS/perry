// FFI: Issue #553 — Real GTK4 implementations.
//
// BottomNavigation: GtkBox+GtkButton tab strip with GtkImage icon + GtkLabel
// (Adwaita CSS classes for selected styling).
//
// ImageGallery: GtkScrolledWindow + horizontal GtkBox of fixed-size GtkPicture
// pages; index-tracking via GtkAdjustment::value-changed.
//
// onScrollEnd: GtkAdjustment::value-changed with backpressure.
//
// Pull-to-refresh stays no-op on GTK4 (no native idiom). LazyVStack
// scroll-end is a no-op too — current GTK4 LazyVStack is essentially a
// fully-realized GtkBox; the ScrollView path is the one production apps
// reach for.
use crate::widgets;

#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_create(on_select: f64) -> i64 {
    widgets::bottom_nav::create(on_select)
}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_add_item(handle: i64, icon_ptr: i64, label_ptr: i64) {
    widgets::bottom_nav::add_item(handle, icon_ptr as *const u8, label_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_badge(handle: i64, index: i64, badge_ptr: i64) {
    widgets::bottom_nav::set_badge(handle, index, badge_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_selected(handle: i64, index: i64) {
    widgets::bottom_nav::set_selected(handle, index)
}

/// Issue #706 — GTK4 bottom-nav active-tab tint via Pango AttrColor on
/// the per-item label.
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::bottom_nav::set_tint_color(handle, r, g, b, a);
}

/// Issue #706 — GTK4 bottom-nav inactive-tabs tint.
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_unselected_tint_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::bottom_nav::set_unselected_tint_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_refresh_control(_handle: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_end_refreshing(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_scroll_end_callback(
    _handle: i64,
    _callback: f64,
    _threshold_items: i64,
) {
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_scroll_end_callback(
    handle: i64,
    callback: f64,
    threshold_px: f64,
) {
    widgets::scrollview::set_scroll_end_callback(handle, callback, threshold_px)
}

#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_create(on_index_change: f64) -> i64 {
    widgets::image_gallery::create(on_index_change)
}
#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_add_image(handle: i64, url_ptr: i64, alt_ptr: i64) {
    widgets::image_gallery::add_image(handle, url_ptr as *const u8, alt_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_set_index(handle: i64, index: i64) {
    widgets::image_gallery::set_index(handle, index)
}
