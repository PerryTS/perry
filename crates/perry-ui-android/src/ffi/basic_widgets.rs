//! Core widget constructors: app/text/button/stack/state/spacer/divider/
//! textfield/toggle/slider. Originally `lib.rs` lines 239-343 plus the
//! "Phase 4: Advanced Reactive UI" follow-ons.

use crate::{app, catch_panic, catch_panic_void, state, widgets};

// =============================================================================
// FFI exports — identical signatures to perry-ui-macos and perry-ui-ios
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_app_create(title_ptr: i64, width: f64, height: f64) -> i64 {
    app::app_create(title_ptr as *const u8, width, height)
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_body(app_handle: i64, root_handle: i64) {
    app::app_set_body(app_handle, root_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_run(app_handle: i64) {
    app::app_run(app_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_create(text_ptr: i64) -> i64 {
    catch_panic("perry_ui_text_create", || {
        widgets::text::create(text_ptr as *const u8)
    })
}

/// `Text(content, id)` — create + register for `setText(id, value)`.
/// Other platforms export this; Android was missing it, so dlopen of apps
/// that use reactive Text ids failed with:
/// `cannot locate symbol "perry_ui_text_create_with_id"`.
#[no_mangle]
pub extern "C" fn perry_ui_text_create_with_id(text_ptr: i64, id_ptr: i64) -> i64 {
    catch_panic("perry_ui_text_create_with_id", || {
        let handle = widgets::text::create(text_ptr as *const u8);
        if id_ptr != 0 {
            let id = app::str_from_header(id_ptr as *const u8);
            widgets::text_registry::register_text_id_handler(handle, id.as_ptr(), id.len());
        }
        handle
    })
}

/// Direct `setText(id, value)` entry for the `import { setText }` surface.
#[no_mangle]
pub extern "C" fn perry_ui_set_text(id_ptr: i64, value_ptr: i64) {
    if id_ptr == 0 {
        return;
    }
    catch_panic_void("perry_ui_set_text", || {
        let id = app::str_from_header(id_ptr as *const u8);
        let val = if value_ptr == 0 {
            ""
        } else {
            app::str_from_header(value_ptr as *const u8)
        };
        widgets::text_registry::set_text_handler(id.as_ptr(), id.len(), val.as_ptr(), val.len());
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_button_create(label_ptr: i64, on_press: f64) -> i64 {
    catch_panic("perry_ui_button_create", || {
        widgets::button::create(label_ptr as *const u8, on_press)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_vstack_create(spacing: f64) -> i64 {
    catch_panic("perry_ui_vstack_create", || {
        widgets::vstack::create(spacing)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_hstack_create(spacing: f64) -> i64 {
    catch_panic("perry_ui_hstack_create", || {
        widgets::hstack::create(spacing)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child(parent_handle: i64, child_handle: i64) {
    catch_panic_void("perry_ui_widget_add_child", || {
        widgets::add_child(parent_handle, child_handle)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_state_create(initial: f64) -> i64 {
    state::state_create(initial)
}

#[no_mangle]
pub extern "C" fn perry_ui_state_get(state_handle: i64) -> f64 {
    state::state_get(state_handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_state_set(state_handle: i64, value: f64) {
    state::state_set(state_handle, value);
}

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

#[no_mangle]
pub extern "C" fn perry_ui_spacer_create() -> i64 {
    widgets::spacer::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_divider_create() -> i64 {
    widgets::divider::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::textfield::create(placeholder_ptr as *const u8, on_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_toggle_create(label_ptr: i64, on_change: f64) -> i64 {
    widgets::toggle::create(label_ptr as *const u8, on_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_slider_create(min: f64, max: f64, on_change: f64) -> i64 {
    // Codegen emits 3-arg `Slider(min, max, onChange)`; default initial=min.
    widgets::slider::create(min, max, min, on_change)
}

// =============================================================================
// Phase 4: Advanced Reactive UI
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_slider(state_handle: i64, slider_handle: i64) {
    state::bind_slider(state_handle, slider_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_toggle(state_handle: i64, toggle_handle: i64) {
    state::bind_toggle(state_handle, toggle_handle);
}

/// Set an existing Toggle's on/off state (issue #5076). `on` is 0 for
/// off, non-zero for on.
#[no_mangle]
pub extern "C" fn perry_ui_toggle_set_state(handle: i64, on: i64) {
    widgets::toggle::set_state(handle, on);
}

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

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_visibility(
    state_handle: i64,
    show_handle: i64,
    hide_handle: i64,
) {
    state::bind_visibility(state_handle, show_handle, hide_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_set_widget_hidden(handle: i64, hidden: i64) {
    widgets::set_hidden(handle, hidden != 0);
}

#[no_mangle]
pub extern "C" fn perry_ui_for_each_init(
    container_handle: i64,
    state_handle: i64,
    render_closure: f64,
) {
    state::for_each_init(container_handle, state_handle, render_closure);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_clear_children(handle: i64) {
    widgets::clear_children(handle);
}
