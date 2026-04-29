//! LazyVStack — ScrollView + LinearLayout, render-all approach

use crate::jni_bridge;
use jni::objects::JValue;
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: f64, arg: f64) -> f64;
}

struct LazyState {
    scroll_handle: i64,
    container_handle: i64,
    render_closure: f64,
}

thread_local! {
    static LAZY_STATES: RefCell<HashMap<i64, LazyState>> = RefCell::new(HashMap::new());
}

pub fn create(count: f64, render_closure: f64) -> i64 {
    // Create ScrollView
    let scroll_handle = super::scrollview::create();

    // Create inner VStack
    let container_handle = super::vstack::create(0.0);

    // Set as child
    super::scrollview::set_child(scroll_handle, container_handle);

    LAZY_STATES.with(|s| {
        s.borrow_mut().insert(
            scroll_handle,
            LazyState {
                scroll_handle,
                container_handle,
                render_closure,
            },
        );
    });

    // Render initial items
    let n = count as i64;
    for i in 0..n {
        let child_f64 = unsafe { js_closure_call1(render_closure, i as f64) };
        let child_handle = child_f64.to_bits() as i64;
        if child_handle > 0 {
            super::add_child(container_handle, child_handle);
        }
    }

    scroll_handle
}

pub fn update(handle: i64, count: i64) {
    LAZY_STATES.with(|s| {
        let states = s.borrow();
        if let Some(state) = states.get(&handle) {
            let container = state.container_handle;
            let closure = state.render_closure;

            // Clear existing children
            super::clear_children(container);

            // Re-render all items
            for i in 0..count {
                let child_f64 = unsafe { js_closure_call1(closure, i as f64) };
                let child_handle = child_f64.to_bits() as i64;
                if child_handle > 0 {
                    super::add_child(container, child_handle);
                }
            }
        }
    });
}
