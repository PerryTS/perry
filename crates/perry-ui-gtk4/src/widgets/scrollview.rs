use gtk4::prelude::*;
use gtk4::ScrolledWindow;

/// Create a GtkScrolledWindow with vertical scrollbar. Returns widget handle.
pub fn create() -> i64 {
    crate::app::ensure_gtk_init();
    let scrolled = ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.set_hexpand(true);
    scrolled.set_propagate_natural_height(true);
    super::register_widget(scrolled.upcast())
}

/// Set the content child of a scroll view.
pub fn set_child(scroll_handle: i64, child_handle: i64) {
    if let (Some(scroll_widget), Some(child)) = (
        super::get_widget(scroll_handle),
        super::get_widget(child_handle),
    ) {
        if let Some(scrolled) = scroll_widget.downcast_ref::<ScrolledWindow>() {
            // Ensure child fills the viewport width (matches macOS ScrollView behavior)
            child.set_hexpand(true);
            child.set_halign(gtk4::Align::Fill);
            scrolled.set_child(Some(&child));
        }
    }
}

/// Scroll so that the given child widget is visible.
/// In GTK4, we compute the child's allocation and scroll to it.
pub fn scroll_to(scroll_handle: i64, child_handle: i64) {
    if let (Some(scroll_widget), Some(child)) = (
        super::get_widget(scroll_handle),
        super::get_widget(child_handle),
    ) {
        if let Some(scrolled) = scroll_widget.downcast_ref::<ScrolledWindow>() {
            // Get the child's allocation relative to the scrolled window content
            let alloc = child.allocation();
            let vadj = scrolled.vadjustment();

            // Scroll so the child is visible
            let child_top = alloc.y() as f64;
            let child_bottom = child_top + alloc.height() as f64;
            let page_top = vadj.value();
            let page_bottom = page_top + vadj.page_size();

            if child_top < page_top {
                vadj.set_value(child_top);
            } else if child_bottom > page_bottom {
                vadj.set_value(child_bottom - vadj.page_size());
            }
        }
    }
}

/// Get the vertical scroll offset.
pub fn get_offset(scroll_handle: i64) -> f64 {
    if let Some(scroll_widget) = super::get_widget(scroll_handle) {
        if let Some(scrolled) = scroll_widget.downcast_ref::<ScrolledWindow>() {
            return scrolled.vadjustment().value();
        }
    }
    0.0
}

/// Set the vertical scroll offset.
pub fn set_offset(scroll_handle: i64, offset: f64) {
    if let Some(scroll_widget) = super::get_widget(scroll_handle) {
        if let Some(scrolled) = scroll_widget.downcast_ref::<ScrolledWindow>() {
            scrolled.vadjustment().set_value(offset);
        }
    }
}
