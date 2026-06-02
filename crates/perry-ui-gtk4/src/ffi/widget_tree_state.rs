// FFI: child management, state system, and state bindings.
use crate::{state, widgets};

// =============================================================================
// Child Management
// =============================================================================

/// Add a child widget to a parent.
#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child(parent_handle: i64, child_handle: i64) {
    widgets::add_child(parent_handle, child_handle);
}

/// Add a child at a specific index.
#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child_at(parent_handle: i64, child_handle: i64, index: f64) {
    widgets::add_child_at(parent_handle, child_handle, index as i64);
}

/// Remove all children from a container.
#[no_mangle]
pub extern "C" fn perry_ui_widget_clear_children(handle: i64) {
    widgets::clear_children(handle);
}

/// Remove a single child from its parent. Mirrors macOS
/// `perry_ui_widget_remove_child` (NSView `removeFromSuperview`).
#[no_mangle]
pub extern "C" fn perry_ui_widget_remove_child(parent_handle: i64, child_handle: i64) {
    widgets::remove_child(parent_handle, child_handle);
}

/// Reorder a child within its parent by positional index. Mirrors macOS
/// `perry_ui_widget_reorder_child` — args are f64 to match the macOS
/// signature, internally cast to i64.
#[no_mangle]
pub extern "C" fn perry_ui_widget_reorder_child(
    parent_handle: i64,
    from_index: f64,
    to_index: f64,
) {
    widgets::reorder_child(parent_handle, from_index as i64, to_index as i64);
}

/// Add an overlay child on top of a parent. Mirrors macOS
/// `perry_ui_widget_add_overlay`. On GTK4 the parent must be an `Overlay`
/// (i.e. a `ZStack`) for the overlay to truly float above siblings.
#[no_mangle]
pub extern "C" fn perry_ui_widget_add_overlay(parent_handle: i64, child_handle: i64) {
    widgets::add_overlay(parent_handle, child_handle);
}

/// Position + size an overlay child. Mirrors macOS
/// `perry_ui_widget_set_overlay_frame` (CGRect on a subview).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_overlay_frame(handle: i64, x: f64, y: f64, w: f64, h: f64) {
    widgets::set_overlay_frame(handle, x, y, w, h);
}

// =============================================================================
// State System
// =============================================================================

/// Create a reactive state cell.
#[no_mangle]
pub extern "C" fn perry_ui_state_create(initial: f64) -> i64 {
    state::state_create(initial)
}

/// Get the current value of a state cell.
#[no_mangle]
pub extern "C" fn perry_ui_state_get(state_handle: i64) -> f64 {
    state::state_get(state_handle)
}

/// Set a new value on a state cell.
#[no_mangle]
pub extern "C" fn perry_ui_state_set(state_handle: i64, value: f64) {
    state::state_set(state_handle, value);
}

/// Register an onChange callback for a state cell.
#[no_mangle]
pub extern "C" fn perry_ui_state_on_change(state_handle: i64, callback: f64) {
    state::on_change(state_handle, callback);
}

// =============================================================================
// State Bindings
// =============================================================================

/// Bind a text widget to a state cell with prefix/suffix.
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_text_numeric(
    state_handle: i64,
    text_handle: i64,
    prefix_ptr: i64,
    suffix_ptr: i64,
) {
    state::bind_text_numeric(
        state_handle,
        text_handle,
        prefix_ptr as *const u8,
        suffix_ptr as *const u8,
    );
}

/// Bind a slider to a state cell (two-way).
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_slider(state_handle: i64, slider_handle: i64) {
    state::bind_slider(state_handle, slider_handle);
}

/// Bind a toggle to a state cell (two-way).
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_toggle(state_handle: i64, toggle_handle: i64) {
    state::bind_toggle(state_handle, toggle_handle);
}

/// Bind a text widget to multiple states with a template.
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_text_template(
    text_handle: i64,
    num_parts: i32,
    types_ptr: i64,
    values_ptr: i64,
) {
    state::bind_text_template(
        text_handle,
        num_parts,
        types_ptr as *const i32,
        values_ptr as *const i64,
    );
}

/// Bind visibility of widgets to a state cell.
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_visibility(
    state_handle: i64,
    show_handle: i64,
    hide_handle: i64,
) {
    state::bind_visibility(state_handle, show_handle, hide_handle);
}

/// Bind a textfield to a state cell (two-way).
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_textfield(state_handle: i64, textfield_handle: i64) {
    state::bind_textfield(state_handle, textfield_handle);
}

/// Initialize a ForEach dynamic list binding.
#[no_mangle]
pub extern "C" fn perry_ui_for_each_init(
    container_handle: i64,
    state_handle: i64,
    render_closure: f64,
) {
    state::for_each_init(container_handle, state_handle, render_closure);
}
