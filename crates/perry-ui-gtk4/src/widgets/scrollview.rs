use gtk4::prelude::*;
use gtk4::ScrolledWindow;
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

struct ScrollEndState {
    closure: f64,
    threshold_px: f64,
    armed: bool,
}

thread_local! {
    static SCROLL_END_STATES: RefCell<HashMap<i64, ScrollEndState>> = RefCell::new(HashMap::new());
}

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

/// Issue #553 — fire `callback` once when the user scrolls within
/// `threshold_px` of the bottom of the inner content. Re-arms after the
/// user scrolls back up past the threshold. Backed by GtkAdjustment's
/// `value-changed` signal.
pub fn set_scroll_end_callback(scroll_handle: i64, callback: f64, threshold_px: f64) {
    let Some(scroll_widget) = super::get_widget(scroll_handle) else {
        return;
    };
    let Some(scrolled) = scroll_widget.downcast_ref::<ScrolledWindow>() else {
        return;
    };
    SCROLL_END_STATES.with(|s| {
        s.borrow_mut().insert(
            scroll_handle,
            ScrollEndState {
                closure: callback,
                threshold_px: if threshold_px > 0.0 {
                    threshold_px
                } else {
                    200.0
                },
                armed: true,
            },
        );
    });
    let h = scroll_handle;
    scrolled.vadjustment().connect_value_changed(move |adj| {
        let visible_bottom = adj.value() + adj.page_size();
        let upper = adj.upper();
        let (closure, in_zone, should_fire) = SCROLL_END_STATES.with(|s| {
            let mut states = s.borrow_mut();
            let Some(state) = states.get_mut(&h) else {
                return (0.0, false, false);
            };
            let in_zone = visible_bottom >= upper - state.threshold_px;
            let mut fire = false;
            if in_zone && state.armed {
                state.armed = false;
                fire = true;
            } else if !in_zone && !state.armed {
                state.armed = true;
            }
            (state.closure, in_zone, fire)
        });
        let _ = in_zone;
        if should_fire && closure != 0.0 {
            unsafe {
                let ptr = js_nanbox_get_pointer(closure) as *const u8;
                js_closure_call0(ptr);
            }
        }
    });
}
