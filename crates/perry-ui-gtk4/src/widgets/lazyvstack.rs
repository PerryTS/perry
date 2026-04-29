use gtk4::prelude::*;
use gtk4::{Orientation, PolicyType, ScrolledWindow};
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

thread_local! {
    static LAZY_VSTACKS: RefCell<HashMap<i64, LazyVStackState>> = RefCell::new(HashMap::new());
}

struct LazyVStackState {
    inner_box: gtk4::Box,
    render_closure: f64,
}

/// Create a lazy vertical stack that renders items from a closure.
/// count = number of items, render_closure = NaN-boxed closure(index) -> widget_handle
pub fn create(count: f64, render_closure: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let scrolled = ScrolledWindow::new();
    scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.set_hexpand(true);
    scrolled.set_propagate_natural_height(true);

    let inner = gtk4::Box::new(Orientation::Vertical, 0);
    scrolled.set_child(Some(&inner));

    let handle = super::register_widget(scrolled.upcast());

    // Render all items (render-all approach, no virtualization)
    let closure_ptr = unsafe { js_nanbox_get_pointer(render_closure) } as *const u8;
    let n = count as i64;
    for i in 0..n {
        let child_f64 = unsafe { js_closure_call1(closure_ptr, i as f64) };
        let child_handle = unsafe { js_nanbox_get_pointer(child_f64) };
        if let Some(child) = super::get_widget(child_handle) {
            if child.parent().is_some() {
                child.unparent();
            }
            inner.append(&child);
        }
    }

    LAZY_VSTACKS.with(|l| {
        l.borrow_mut().insert(
            handle,
            LazyVStackState {
                inner_box: inner,
                render_closure,
            },
        );
    });

    handle
}

/// Update the lazy vstack with a new count. Re-renders all items.
pub fn update(handle: i64, count: i64) {
    LAZY_VSTACKS.with(|l| {
        let stacks = l.borrow();
        if let Some(state) = stacks.get(&handle) {
            // Clear existing children
            while let Some(child) = state.inner_box.first_child() {
                state.inner_box.remove(&child);
            }
            // Re-render
            let closure_ptr = unsafe { js_nanbox_get_pointer(state.render_closure) } as *const u8;
            for i in 0..count {
                let child_f64 = unsafe { js_closure_call1(closure_ptr, i as f64) };
                let child_handle = unsafe { js_nanbox_get_pointer(child_f64) };
                if let Some(child) = super::get_widget(child_handle) {
                    if child.parent().is_some() {
                        child.unparent();
                    }
                    state.inner_box.append(&child);
                }
            }
        }
    });
}
