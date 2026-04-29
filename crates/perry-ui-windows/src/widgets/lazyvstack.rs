//! LazyVStack widget — ScrollView + VStack that renders all items via a closure.
//! On Win32 we render all items eagerly (no virtualization) inside a scrollable container.

use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

struct LazyVStackState {
    scroll_handle: i64,
    vstack_handle: i64,
    render_closure: *const u8,
}

thread_local! {
    static LAZYVSTACK_STATES: RefCell<HashMap<i64, LazyVStackState>> = RefCell::new(HashMap::new());
}

/// Create a LazyVStack that renders `count` items using a closure.
/// Returns the outer scrollview handle (which is the LazyVStack handle).
/// count = number of items, render_closure = NaN-boxed closure(index) -> widget handle.
pub fn create(count: f64, render_closure: f64) -> i64 {
    let closure_ptr = unsafe { js_nanbox_get_pointer(render_closure) } as *const u8;
    let item_count = count as i64;

    // Create a ScrollView as the outer container
    let scroll_handle = super::scrollview::create();

    // Create an inner VStack with spacing=4
    let vstack_handle = super::vstack::create(4.0);

    // Set the VStack as the ScrollView's content
    super::scrollview::set_child(scroll_handle, vstack_handle);

    // Render all items
    for i in 0..item_count {
        let child_handle = unsafe { js_closure_call1(closure_ptr, i as f64) };
        let child = child_handle as i64;
        if child > 0 {
            super::add_child(vstack_handle, child);
        }
    }

    // Store state for update
    LAZYVSTACK_STATES.with(|states| {
        states.borrow_mut().insert(
            scroll_handle,
            LazyVStackState {
                scroll_handle,
                vstack_handle,
                render_closure: closure_ptr,
            },
        );
    });

    scroll_handle
}

/// Update (re-render) the LazyVStack with a new item count.
/// Clears existing children and re-renders all items.
pub fn update(handle: i64, count: i64) {
    LAZYVSTACK_STATES.with(|states| {
        let states = states.borrow();
        if let Some(state) = states.get(&handle) {
            let vstack_handle = state.vstack_handle;
            let closure_ptr = state.render_closure;

            // Clear existing children
            super::clear_children(vstack_handle);

            // Re-render all items
            for i in 0..count {
                let child_handle = unsafe { js_closure_call1(closure_ptr, i as f64) };
                let child = child_handle as i64;
                if child > 0 {
                    super::add_child(vstack_handle, child);
                }
            }
        }
    });
}
