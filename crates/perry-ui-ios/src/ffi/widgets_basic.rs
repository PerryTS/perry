//! FFI exports: core widgets, reactive bindings, text mutation, scrollview, styling, focus/scroll, context menu, tray
//!
//! Extracted from `lib.rs` for file-size hygiene. No behavior changes.

use crate::*;

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

// =============================================================================
// Phase A.1: Text Mutation & Layout Control
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_string(handle: i64, text_ptr: i64) {
    widgets::text::set_string(handle, text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_vstack_create_with_insets(
    spacing: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> i64 {
    widgets::vstack::create_with_insets(spacing, top, left, bottom, right)
}

#[no_mangle]
pub extern "C" fn perry_ui_hstack_create_with_insets(
    spacing: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> i64 {
    widgets::hstack::create_with_insets(spacing, top, left, bottom, right)
}

// =============================================================================
// Phase A.2: ScrollView, Clipboard & Keyboard Shortcuts
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_create() -> i64 {
    widgets::scrollview::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_child(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::set_child(scroll_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_clipboard_read() -> f64 {
    clipboard::read()
}

#[no_mangle]
pub extern "C" fn perry_ui_clipboard_write(text_ptr: i64) {
    clipboard::write(text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_add_keyboard_shortcut(key_ptr: i64, modifiers: f64, callback: f64) {
    app::add_keyboard_shortcut(key_ptr as *const u8, modifiers, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_register_global_hotkey(_key: i64, _mods: f64, _cb: f64) {}

// Continuous keyboard events (issue #1864) — real iOS impl via UIPress/UIKey.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_key_down(handle: i64, cb: f64) {
    crate::keyboard::set_on_key_down(handle, cb);
    crate::keyboard::make_first_responder();
}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_key_up(handle: i64, cb: f64) {
    crate::keyboard::set_on_key_up(handle, cb);
    crate::keyboard::make_first_responder();
}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_on_key_down(cb: f64) {
    crate::keyboard::set_on_key_down(0, cb);
    crate::keyboard::make_first_responder();
}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_on_key_up(cb: f64) {
    crate::keyboard::set_on_key_up(0, cb);
    crate::keyboard::make_first_responder();
}
#[no_mangle]
pub extern "C" fn perry_ui_focus_widget(handle: i64) {
    crate::keyboard::focus_widget(handle);
}
#[no_mangle]
pub extern "C" fn perry_ui_blur_widget(handle: i64) {
    crate::keyboard::blur_widget(handle);
}
#[no_mangle]
pub extern "C" fn perry_ui_is_key_down(code: f64) -> i32 {
    let raw = code as i32;
    if !(0..=u16::MAX as i32).contains(&raw) {
        return 0;
    }
    if crate::keyboard::is_key_down(raw as u16) {
        1
    } else {
        0
    }
}
#[no_mangle]
pub extern "C" fn perry_ui_current_modifiers() -> i32 {
    crate::keyboard::current_modifiers() as i32
}

#[no_mangle]
pub extern "C" fn perry_system_get_app_icon(_path: i64) -> i64 {
    0
}

// =============================================================================
// Phase A.3: Text Styling & Button Styling
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::text::set_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_size(handle: i64, size: f64) {
    widgets::text::set_font_size(handle, size);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_weight(handle: i64, size: f64, weight: f64) {
    widgets::text::set_font_weight(handle, size, weight);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_wraps(handle: i64, max_width: f64) {
    widgets::text::set_wraps(handle, max_width);
}

/// Text decoration (issue #185 Phase B). 0=none, 1=underline, 2=strikethrough.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_decoration(handle: i64, decoration: i64) {
    widgets::text::set_decoration(handle, decoration);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_selectable(handle: i64, selectable: f64) {
    widgets::text::set_selectable(handle, selectable != 0.0);
}

/// Issue #707 — cap visible lines on a Text widget. `lines = 0` is unlimited.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_number_of_lines(handle: i64, lines: i64) {
    widgets::text::set_number_of_lines(handle, lines);
}

/// Issue #707 — set truncation mode. 0=word-wrap, 1=head, 2=middle, 3=tail.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_truncation_mode(handle: i64, mode: i64) {
    widgets::text::set_truncation_mode(handle, mode);
}

/// Issue #3621 — horizontal text alignment. `alignment`: 0=left, 1=right,
/// 2=center, 3=justified, 4=natural.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_text_alignment(handle: i64, alignment: i64) {
    widgets::text::set_text_alignment(handle, alignment);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_bordered(handle: i64, bordered: f64) {
    widgets::button::set_bordered(handle, bordered != 0.0);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_title(handle: i64, title_ptr: i64) {
    widgets::button::set_title(handle, title_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::button::set_text_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_image(handle: i64, name_ptr: i64) {
    widgets::button::set_image(handle, name_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_image_position(handle: i64, position: i64) {
    widgets::button::set_image_position(handle, position);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_content_tint_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::button::set_content_tint_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_width(handle: i64, width: f64) {
    widgets::set_width(handle, width);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_height(handle: i64, height: f64) {
    widgets::set_height(handle, height);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_hugging(handle: i64, priority: f64) {
    widgets::set_hugging_priority(handle, priority);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_remove_child(parent_handle: i64, child_handle: i64) {
    widgets::remove_child(parent_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_reorder_child(
    parent_handle: i64,
    from_index: f64,
    to_index: f64,
) {
    widgets::reorder_child(parent_handle, from_index as i64, to_index as i64);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_width(handle: i64) {
    widgets::match_parent_width(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_height(handle: i64) {
    widgets::match_parent_height(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_stack_set_detaches_hidden(handle: i64, flag: i64) {
    widgets::set_detaches_hidden_views(handle, flag != 0);
}

#[no_mangle]
pub extern "C" fn perry_ui_stack_set_distribution(handle: i64, distribution: f64) {
    // UIStackView distribution: 0=Fill, 1=FillEqually, 2=FillProportionally, 3=EqualSpacing, 4=EqualCentering
    if let Some(view) = widgets::get_widget(handle) {
        let is_stack = if let Some(cls) = objc2::runtime::AnyClass::get(c"UIStackView") {
            use objc2_foundation::NSObjectProtocol;
            view.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            let dist = if distribution < 0.0 {
                0_i64
            } else {
                distribution as i64
            };
            unsafe {
                let _: () = objc2::msg_send![&*view, setDistribution: dist];
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_ui_stack_set_alignment(handle: i64, alignment: f64) {
    // UIStackView alignment: 0=Fill, 1=Leading, 2=FirstBaseline, 3=Center, 4=Trailing, 5=LastBaseline
    if let Some(view) = widgets::get_widget(handle) {
        let is_stack = if let Some(cls) = objc2::runtime::AnyClass::get(c"UIStackView") {
            use objc2_foundation::NSObjectProtocol;
            view.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            let align = alignment as i64;
            unsafe {
                let _: () = objc2::msg_send![&*view, setAlignment: align];
            }
        }
    }
}

// =============================================================================
// Phase A.4: Focus & Scroll-To
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_textfield_focus(handle: i64) {
    widgets::textfield::focus(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_scroll_to(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::scroll_to(scroll_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_get_offset(scroll_handle: i64) -> f64 {
    widgets::scrollview::get_offset(scroll_handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_offset(scroll_handle: i64, offset: f64) {
    widgets::scrollview::set_offset(scroll_handle, offset);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_refresh_control(scroll_handle: i64, callback: f64) {
    widgets::scrollview::set_refresh_control(scroll_handle, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_end_refreshing(scroll_handle: i64) {
    widgets::scrollview::end_refreshing(scroll_handle);
}

// =============================================================================
// Phase A.5: Context Menus, File Dialog & Window Sizing
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_menu_create() -> i64 {
    menu::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item(menu_handle: i64, title_ptr: i64, callback: f64) {
    menu::add_item(menu_handle, title_ptr as *const u8, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_context_menu(widget_handle: i64, menu_handle: i64) {
    menu::set_context_menu(widget_handle, menu_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item_with_shortcut(
    menu_handle: i64,
    title_ptr: i64,
    shortcut_ptr: i64,
    callback: f64,
) {
    // Arg order matches the TS-side API: `menuAddItemWithShortcut(menu, title, shortcut, callback)`.
    menu::add_item_with_shortcut(
        menu_handle,
        title_ptr as *const u8,
        callback,
        shortcut_ptr as *const u8,
    );
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_separator(menu_handle: i64) {
    menu::add_separator(menu_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_submenu(menu_handle: i64, title_ptr: i64, submenu_handle: i64) {
    menu::add_submenu(menu_handle, title_ptr as *const u8, submenu_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_create() -> i64 {
    menu::menubar_create()
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_add_menu(bar_handle: i64, title_ptr: i64, menu_handle: i64) {
    menu::menubar_add_menu(bar_handle, title_ptr as *const u8, menu_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_attach(bar_handle: i64) {
    menu::menubar_attach(bar_handle);
}

// =============================================================================
// Tray icon (issue #490) — no-op on iOS (no system tray concept).
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_tray_create(_icon_path_ptr: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_tray_set_icon(_tray_handle: i64, _icon_path_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tray_set_tooltip(_tray_handle: i64, _tooltip_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tray_attach_menu(_tray_handle: i64, _menu_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tray_on_click(_tray_handle: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tray_destroy(_tray_handle: i64) {}

/// Remove all items from a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_clear(menu_handle: i64) {
    menu::clear(menu_handle);
}

/// Add a menu item with a standard action (no-op on iOS — macOS responder chain concept).
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_standard_action(
    _menu_handle: i64,
    _title_ptr: i64,
    _selector_ptr: i64,
    _shortcut_ptr: i64,
) {
    // No-op on iOS — standard Edit menu actions are handled by UIResponder chain natively
}

#[no_mangle]
pub extern "C" fn perry_ui_open_file_dialog(callback: f64) {
    file_dialog::open_dialog(callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_min_size(app_handle: i64, w: f64, h: f64) {
    app::set_min_size(app_handle, w, h);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_max_size(app_handle: i64, w: f64, h: f64) {
    app::set_max_size(app_handle, w, h);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_string(handle: i64, text_ptr: i64) {
    widgets::textfield::set_string_value(handle, text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_get_string(handle: i64) -> i64 {
    widgets::textfield::get_string_value(handle) as i64
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_submit(handle: i64, on_submit: f64) {
    widgets::textfield::set_on_submit(handle, on_submit);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_focus(handle: i64, on_focus: f64) {
    // TODO: implement iOS textfield focus observer
    let _ = (handle, on_focus);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_blur_all() {
    // TODO: implement iOS blur
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_next_key_view(_handle: i64, _next_handle: i64) {
    // iOS handles tab navigation automatically
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_borderless(handle: i64, borderless: f64) {
    widgets::textfield::set_borderless(handle, borderless);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_background_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::textfield::set_background_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_font_size(handle: i64, size: f64) {
    widgets::textfield::set_font_size(handle, size);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::textfield::set_text_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child_at(parent_handle: i64, child_handle: i64, index: f64) {
    widgets::add_child_at(parent_handle, child_handle, index as i64);
}
