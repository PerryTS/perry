//! macOS render handler for `ForEach(state<number>, render)` re-rendering.
//!
//! Issue #610. Mirrors `text_registry.rs`'s shape:
//!
//! 1. The state_desugar pass rewrites `ForEach(stateBinding, render)` into
//!    an IIFE that creates a host VStack and calls
//!    `__foreach_register("synth_id", host_handle, render_closure)`.
//! 2. The runtime FFI `js_foreach_register` records the binding in
//!    `FOREACH_REGISTRY` (in `perry-runtime/src/ui_text_registry.rs`) and
//!    invokes our `render_handler` with the current state value to paint
//!    the initial children.
//! 3. When user code calls `state.set(n)` later, the runtime's
//!    `js_state_set` walks `FOREACH_REGISTRY` for the synth_id and fires
//!    `render_handler(host, render_closure, n)` for each binding —
//!    re-painting the children.
//!
//! `render_handler` clears the host's existing children (via
//! `widgets::clear_children`), then calls `render_closure(i)` for each
//! `i in [0..count)` and adds each returned widget. The closure
//! invocation goes through `js_closure_call1` (returns a NaN-boxed widget
//! handle); we extract the integer handle via `js_nanbox_get_pointer`
//! and call `widgets::add_child`.

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

/// Cross-platform render-handler entry point. Registered with
/// `js_register_foreach_render_handler` at app startup.
pub extern "C" fn render_handler(container_handle: i64, render_closure: f64, count: f64) {
    // Truncate `count` to a non-negative integer, matching JS's `Math.floor`
    // / `>>> 0`-style coercion. Negative or NaN counts produce zero
    // children (no-op clear-and-rebuild — same shape as `count.set(0)`).
    let n = if count.is_nan() || count <= 0.0 {
        0i64
    } else {
        count as i64
    };

    // Clear existing children before re-rendering. `clear_children`
    // walks the host's arrangedSubviews and removes each — matching the
    // initial-paint shape so the second render produces a fresh tree.
    crate::widgets::clear_children(container_handle);

    let closure_ptr = unsafe { js_nanbox_get_pointer(render_closure) } as *const u8;
    if closure_ptr.is_null() {
        return;
    }
    for i in 0..n {
        let child_f64 = unsafe { js_closure_call1(closure_ptr, i as f64) };
        let child_handle = unsafe { js_nanbox_get_pointer(child_f64) };
        if child_handle != 0 {
            crate::widgets::add_child(container_handle, child_handle);
        }
    }
}
