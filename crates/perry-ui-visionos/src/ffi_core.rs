//! Core FFI exports: app, layout primitives, basic widgets, state, and
//! WebView. Split out of `lib.rs` to keep each file under 2k LOC. Behavior
//! is unchanged — these are the same `#[no_mangle]` entry points that the
//! codegen-emitted `perry_ui_*` declarations resolve to at link time.

use super::*;
use crate::ws_log;

// =============================================================================
// FFI exports — identical signatures to perry-ui-macos
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

/// Register an external UIView (from a native library) as a Perry widget.
/// Alias of perry_ui_embed_nsview so cross-platform Perry code works unchanged on iOS.
#[no_mangle]
pub extern "C" fn perry_ui_embed_nsview(uiview_ptr: i64) -> i64 {
    use objc2::rc::Retained;
    use objc2_ui_kit::UIView;
    if uiview_ptr == 0 {
        return 0;
    }
    match unsafe { Retained::retain(uiview_ptr as *mut UIView) } {
        Some(view) => {
            // Disable autoresizing mask → Auto Layout constraint translation.
            // Without this, the embedded view's autoresizing mask conflicts with
            // UIStackView layout constraints, causing black screen in HStack.
            let _: () = unsafe {
                objc2::msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false]
            };
            widgets::register_widget(view)
        }
        None => 0,
    }
}

/// Create a split view container (plain UIView with Auto Layout, not UIStackView).
/// Left panel gets fixed width; right panel fills remaining space.
#[no_mangle]
pub extern "C" fn perry_ui_splitview_create(left_width: f64) -> i64 {
    widgets::splitview::create(left_width)
}

/// Add a child to a split view. First call adds left panel, second adds right panel.
#[no_mangle]
pub extern "C" fn perry_ui_splitview_add_child(
    parent_handle: i64,
    child_handle: i64,
    child_index: f64,
) {
    if let (Some(parent), Some(child)) = (
        widgets::get_widget(parent_handle),
        widgets::get_widget(child_handle),
    ) {
        widgets::splitview::add_child(&parent, &child, child_index as usize);
    }
}

/// Create a vertical layout container (plain UIView, not UIStackView).
#[no_mangle]
pub extern "C" fn perry_ui_vbox_create() -> i64 {
    widgets::splitview::create_vbox()
}

/// Add a child to a vbox at a slot: 0=top, 1=middle(fills), 2=bottom.
#[no_mangle]
pub extern "C" fn perry_ui_vbox_add_child(parent_handle: i64, child_handle: i64, slot: f64) {
    if let (Some(parent), Some(child)) = (
        widgets::get_widget(parent_handle),
        widgets::get_widget(child_handle),
    ) {
        widgets::splitview::vbox_add_child(&parent, &child, slot as usize);
    }
}

/// Finalize vbox layout by connecting middle.bottom to bottom.top.
#[no_mangle]
pub extern "C" fn perry_ui_vbox_finalize(parent_handle: i64) {
    if let Some(parent) = widgets::get_widget(parent_handle) {
        widgets::splitview::vbox_finalize(&parent);
    }
}

/// Create a frame-based horizontal split container.
/// Uses layoutSubviews for child positioning (no Auto Layout on children).
/// This avoids constraint conflicts with embedded UIViews.
#[no_mangle]
pub extern "C" fn perry_ui_frame_split_create(left_width: f64) -> i64 {
    widgets::splitview::create_frame_split(left_width)
}

/// Add a child to a frame-based split container.
/// Children use frame-based layout (translatesAutoresizingMaskIntoConstraints = true).
#[no_mangle]
pub extern "C" fn perry_ui_frame_split_add_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (
        widgets::get_widget(parent_handle),
        widgets::get_widget(child_handle),
    ) {
        widgets::splitview::frame_split_add_child(&parent, &child);
    }
}

#[no_mangle]
pub extern "C" fn perry_ui_text_create(text_ptr: i64) -> i64 {
    widgets::text::create(text_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_ui_button_create(label_ptr: i64, on_press: f64) -> i64 {
    widgets::button::create(label_ptr as *const u8, on_press)
}

#[no_mangle]
pub extern "C" fn perry_ui_vstack_create(spacing: f64) -> i64 {
    widgets::vstack::create(spacing)
}

#[no_mangle]
pub extern "C" fn perry_ui_hstack_create(spacing: f64) -> i64 {
    widgets::hstack::create(spacing)
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child(parent_handle: i64, child_handle: i64) {
    widgets::add_child(parent_handle, child_handle);
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
pub extern "C" fn perry_ui_textarea_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::textarea::create(placeholder_ptr as *const u8, on_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_textarea_set_string(handle: i64, text_ptr: i64) {
    widgets::textarea::set_string(handle, text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_textarea_get_string(handle: i64) -> i64 {
    widgets::textarea::get_string(handle) as i64
}

// --- WebView (WKWebView) — issue #658 Phase 1 ---

#[no_mangle]
pub extern "C" fn perry_ui_webview_create(
    url_ptr: i64,
    width: f64,
    height: f64,
    ephemeral: f64,
) -> i64 {
    widgets::webview::create(url_ptr as *const u8, width, height, ephemeral)
}

/// Create a BloomView render-surface host (issue #2395).
#[no_mangle]
pub extern "C" fn perry_ui_bloomview_create(width: f64, height: f64) -> i64 {
    widgets::bloomview::create(width, height)
}

/// Return the BloomView's native view pointer as an integer (issue #2395).
#[no_mangle]
pub extern "C" fn perry_ui_bloomview_get_hwnd(handle: i64) -> i64 {
    widgets::bloomview::get_native_handle(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_user_agent(handle: i64, ua_ptr: i64) {
    widgets::webview::set_user_agent(handle, ua_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_allowed_domains(handle: i64, arr_handle: i64) {
    widgets::webview::set_allowed_domains(handle, arr_handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_ephemeral(handle: i64, ephemeral: i64) {
    widgets::webview::set_ephemeral(handle, ephemeral)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_should_navigate(handle: i64, closure: f64) {
    widgets::webview::set_on_should_navigate(handle, closure)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_loaded(handle: i64, closure: f64) {
    widgets::webview::set_on_loaded(handle, closure)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_error(handle: i64, closure: f64) {
    widgets::webview::set_on_error(handle, closure)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_load_url(handle: i64, url_ptr: i64) {
    widgets::webview::load_url(handle, url_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_reload(handle: i64) {
    widgets::webview::reload(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_go_back(handle: i64) {
    widgets::webview::go_back(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_go_forward(handle: i64) {
    widgets::webview::go_forward(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_can_go_back(handle: i64) -> i64 {
    widgets::webview::can_go_back(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_evaluate_js(handle: i64, js_ptr: i64, callback: f64) {
    widgets::webview::evaluate_js(handle, js_ptr as *const u8, callback)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_clear_cookies(handle: i64) {
    widgets::webview::clear_cookies(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_toggle_create(label_ptr: i64, on_change: f64) -> i64 {
    match std::panic::catch_unwind(|| widgets::toggle::create(label_ptr as *const u8, on_change)) {
        Ok(handle) => handle,
        Err(e) => {
            crash_log::clear_crash_log();
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                format!("{:?}", e)
            };
            ws_log!("[perry] panic in perry_ui_toggle_create (caught): {}", msg);
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_ui_slider_create(min: f64, max: f64, initial: f64, on_change: f64) -> i64 {
    widgets::slider::create(min, max, initial, on_change)
}

/// Set an existing Toggle's on/off state (issue #5076). `on` is 0 for
/// off, non-zero for on.
#[no_mangle]
pub extern "C" fn perry_ui_toggle_set_state(handle: i64, on: i64) {
    widgets::toggle::set_state(handle, on);
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
