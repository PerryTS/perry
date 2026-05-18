pub mod app;
pub mod audio;
pub mod background;
pub mod camera;
pub mod clipboard;
pub mod crash_log;
pub mod deeplinks;
pub mod file_dialog;
pub mod geolocation;
pub mod image_picker;
pub mod location;
pub mod media_playback;
pub mod menu;
pub mod network;
pub mod notifications;
pub mod screenshot;
pub mod state;
pub mod websocket;
pub mod widgets;

#[cfg(feature = "geisterhand")]
pub mod geisterhand_style;

/// Debug logging macro that writes to a file (NSLog/eprintln don't work reliably on iOS)
#[macro_export]
macro_rules! ws_log {
    ($($arg:tt)*) => {{
        use std::io::Write;
        let msg = format!($($arg)*);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/hone-ws-ios.log") {
            let _ = writeln!(f, "{}", msg);
        }
    }};
}

/// Run a closure, catching any Rust panics so they don't abort across the FFI boundary.
/// Clears the crash log since the panic was caught (non-fatal).
pub fn catch_callback_panic<F: FnOnce() + std::panic::UnwindSafe>(label: &str, f: F) {
    if let Err(e) = std::panic::catch_unwind(f) {
        crash_log::clear_crash_log();

        let msg = if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            format!("{:?}", e)
        };
        // Log to file since iOS eprintln is invisible
        ws_log!("[perry] panic in {} (caught): {}", label, msg);
    }
}

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

// =============================================================================
// Timer, Background Styling & Canvas
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_app_set_timer(_app_handle: i64, interval_ms: f64, callback: f64) {
    app::set_timer(interval_ms, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::set_background_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_gradient(
    handle: i64,
    r1: f64,
    g1: f64,
    b1: f64,
    a1: f64,
    r2: f64,
    g2: f64,
    b2: f64,
    a2: f64,
    direction: f64,
) {
    widgets::set_background_gradient(handle, r1, g1, b1, a1, r2, g2, b2, a2, direction);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_corner_radius(handle: i64, radius: f64) {
    widgets::set_corner_radius(handle, radius);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_create(width: f64, height: f64) -> i64 {
    widgets::canvas::create(width, height)
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_clear(handle: i64) {
    widgets::canvas::clear(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_begin_path(handle: i64) {
    widgets::canvas::begin_path(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_move_to(handle: i64, x: f64, y: f64) {
    widgets::canvas::move_to(handle, x, y);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_line_to(handle: i64, x: f64, y: f64) {
    widgets::canvas::line_to(handle, x, y);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
    line_width: f64,
) {
    widgets::canvas::stroke(handle, r, g, b, a, line_width);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_gradient(
    handle: i64,
    r1: f64,
    g1: f64,
    b1: f64,
    a1: f64,
    r2: f64,
    g2: f64,
    b2: f64,
    a2: f64,
    direction: f64,
) {
    widgets::canvas::fill_gradient(handle, r1, g1, b1, a1, r2, g2, b2, a2, direction);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_fill_color(_h: i64, _r: f64, _g: f64, _b: f64, _a: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_stroke_color(_h: i64, _r: f64, _g: f64, _b: f64, _a: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_line_width(_h: i64, _w: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_rect(_h: i64, _x: f64, _y: f64, _w: f64, _ht: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke_rect(_h: i64, _x: f64, _y: f64, _w: f64, _ht: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_clear_rect(h: i64, _x: f64, _y: f64, _w: f64, _ht: f64) {
    widgets::canvas::clear(h);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_arc(_h: i64, _x: f64, _y: f64, _r: f64, _sa: f64, _ea: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_close_path(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke_path(_h: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_text(_h: i64, _ptr: i64, _x: f64, _y: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_font(_h: i64, _ptr: i64) {}

// =============================================================================
// New Widgets: SecureField, ProgressView, Image, Picker, Form, NavStack, ZStack
// =============================================================================

/// Create a SecureField (password input). Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_securefield_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::securefield::create(placeholder_ptr as *const u8, on_change)
}

/// Create an indeterminate ProgressView (spinner). Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_progressview_create() -> i64 {
    widgets::progressview::create()
}

/// Set determinate progress value (0.0-1.0).
#[no_mangle]
pub extern "C" fn perry_ui_progressview_set_value(handle: i64, value: f64) {
    widgets::progressview::set_value(handle, value);
}

/// Create an Image from an SF Symbol name. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_symbol(name_ptr: i64) -> i64 {
    widgets::image::create_symbol(name_ptr as *const u8)
}

/// Create an Image from a file path. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_file(path_ptr: i64) -> i64 {
    widgets::image::create_file(path_ptr as *const u8)
}

/// Create an Image whose bytes are fetched from a URL (or `data:` URI)
/// asynchronously and applied on the main thread (#635). `alt`, when
/// non-empty, becomes the view's accessibility label.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_url(url_ptr: i64, alt_ptr: i64) -> i64 {
    widgets::image::create_url(url_ptr as *const u8, alt_ptr as *const u8)
}

/// Set the size of an Image widget.
#[no_mangle]
pub extern "C" fn perry_ui_image_set_size(handle: i64, width: f64, height: f64) {
    widgets::image::set_size(handle, width, height);
}

/// Set the tint color of an Image widget.
#[no_mangle]
pub extern "C" fn perry_ui_image_set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::image::set_tint(handle, r, g, b, a);
}

/// Create a Picker (dropdown). style: 0=dropdown, 1=segmented. Returns widget handle.
#[no_mangle]
// Issue #478 — Rich text editor — real iOS impl via UITextView.
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_create(w: f64, h: f64, cb: f64) -> i64 {
    widgets::rich_text::create(w, h, cb)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_set_string(h: i64, t: i64) {
    widgets::rich_text::set_string(h, t as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_get_string(h: i64) -> f64 {
    widgets::rich_text::get_string(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_set_html(h: i64, html: i64) -> i64 {
    widgets::rich_text::set_html(h, html as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_get_html(h: i64) -> f64 {
    widgets::rich_text::get_html(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_bold(h: i64) {
    widgets::rich_text::toggle_bold(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_italic(h: i64) {
    widgets::rich_text::toggle_italic(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_underline(h: i64) {
    widgets::rich_text::toggle_underline(h)
}

// Issue #516 — PdfView (iOS) — real impl via PDFView.
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_create(w: f64, h: f64) -> i64 {
    widgets::pdf_view::create(w, h)
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_load_file(h: i64, p: i64) -> i64 {
    if widgets::pdf_view::load_file(h, p as *const u8) {
        1
    } else {
        0
    }
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_get_page_count(h: i64) -> i64 {
    widgets::pdf_view::get_page_count(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_go_to_page(h: i64, i: i64) {
    widgets::pdf_view::go_to_page(h, i)
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_get_current_page(h: i64) -> i64 {
    widgets::pdf_view::get_current_page(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_set_scale(h: i64, s: f64) {
    widgets::pdf_view::set_scale(h, s)
}

// Issue #517 — MapView (iOS) — real impl via MKMapView.
#[no_mangle]
pub extern "C" fn perry_ui_map_view_create(w: f64, h: f64) -> i64 {
    widgets::map_view::create(w, h)
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_region(h: i64, lat: f64, lon: f64, ls: f64, os: f64) {
    widgets::map_view::set_region(h, lat, lon, ls, os)
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_add_pin(h: i64, lat: f64, lon: f64, t: i64) {
    widgets::map_view::add_pin(h, lat, lon, t as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_clear_pins(h: i64) {
    widgets::map_view::clear_pins(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_map_type(h: i64, s: i64) {
    widgets::map_view::set_map_type(h, s)
}

// Issue #477 — Command palette stubs.
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_register(_id: i64, _l: i64, _s: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_unregister(_id: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_clear() {}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_show() {}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_hide() {}

// Issue #474 — Chart (iOS) — UIView subclass + CoreGraphics drawRect:.
#[no_mangle]
pub extern "C" fn perry_ui_chart_create(kind: i64, w: f64, h: f64) -> i64 {
    widgets::chart::create(kind, w, h)
}
#[no_mangle]
pub extern "C" fn perry_ui_chart_add_data_point(h: i64, l: i64, v: f64) {
    widgets::chart::add_data_point(h, l as *const u8, v)
}
#[no_mangle]
pub extern "C" fn perry_ui_chart_clear_data(h: i64) {
    widgets::chart::clear_data(h)
}
#[no_mangle]
pub extern "C" fn perry_ui_chart_set_title(h: i64, t: i64) {
    widgets::chart::set_title(h, t as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_chart_reload(h: i64) {
    widgets::chart::reload(h)
}

// Issue #481 — Calendar widget — real iOS impl via UIDatePicker.inline.
#[no_mangle]
pub extern "C" fn perry_ui_calendar_create(year: i64, month: i64, on_change: f64) -> i64 {
    widgets::calendar::create(year, month, on_change)
}
#[no_mangle]
pub extern "C" fn perry_ui_calendar_set_date(h: i64, y: i64, m: i64, d: i64) {
    widgets::calendar::set_date(h, y, m, d)
}
#[no_mangle]
pub extern "C" fn perry_ui_calendar_get_selected_date(h: i64) -> f64 {
    widgets::calendar::get_selected_date(h)
}

// Issue #473 — table sort/filter/multi-select stubs.
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_sort_change(_h: i64, _cb: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_allows_multiple_selection(_h: i64, _allow: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_rows_count(_h: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row_at(_h: i64, _n: i64) -> i64 {
    -1
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_filter_text(_h: i64, _t: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_filter_text(_h: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}

/// TreeView (issue #480). iOS — `UITableView` over a depth-flattened
/// tree with per-row indentation and a chevron disclosure button.
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_create(id_ptr: i64, label_ptr: i64) -> i64 {
    widgets::tree_view::node_create(id_ptr as *const u8, label_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_add_child(parent: i64, child: i64) {
    widgets::tree_view::node_add_child(parent, child);
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_create(root: i64, on_select: f64) -> i64 {
    widgets::tree_view::create(root, on_select)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_expand_all(handle: i64) {
    widgets::tree_view::expand_all(handle);
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_collapse_all(handle: i64) {
    widgets::tree_view::collapse_all(handle);
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_get_selected_id(handle: i64) -> f64 {
    widgets::tree_view::get_selected_id(handle)
}

/// Combobox (issue #475). iOS — UITextField + UIPickerView inputView.
#[no_mangle]
pub extern "C" fn perry_ui_combobox_create(initial_ptr: i64, on_change: f64) -> i64 {
    widgets::combobox::create(initial_ptr as *const u8, on_change)
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_add_item(handle: i64, value_ptr: i64) {
    widgets::combobox::add_item(handle, value_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_set_value(handle: i64, value_ptr: i64) {
    widgets::combobox::set_value(handle, value_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_get_value(handle: i64) -> f64 {
    widgets::combobox::get_value(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_picker_create(label_ptr: i64, on_change: f64, style: i64) -> i64 {
    widgets::picker::create(label_ptr as *const u8, on_change, style)
}

/// Add an item to a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_add_item(handle: i64, title_ptr: i64) {
    widgets::picker::add_item(handle, title_ptr as *const u8);
}

/// Set the selected index of a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_set_selected(handle: i64, index: i64) {
    widgets::picker::set_selected(handle, index);
}

/// Get the selected index of a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_get_selected(handle: i64) -> i64 {
    widgets::picker::get_selected(handle)
}

/// Create a TabBar. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_tabbar_create(on_change: f64) -> i64 {
    widgets::tabbar::create(on_change)
}

/// Add a tab to a TabBar.
#[no_mangle]
pub extern "C" fn perry_ui_tabbar_add_tab(handle: i64, label_ptr: i64) {
    widgets::tabbar::add_tab(handle, label_ptr as *const u8);
}

/// Set the selected tab index.
#[no_mangle]
pub extern "C" fn perry_ui_tabbar_set_selected(handle: i64, index: i64) {
    widgets::tabbar::set_selected(handle, index);
}

/// Create a Form container. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_form_create() -> i64 {
    widgets::form::form_create()
}

/// Create a Section with title. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_section_create(title_ptr: i64) -> i64 {
    widgets::form::section_create(title_ptr as *const u8)
}

/// Create a NavigationStack. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_create() -> i64 {
    // Matches the 0-arg dispatch in perry-dispatch::PERRY_UI_TABLE.
    widgets::navstack::create(std::ptr::null(), 0)
}

/// Push a view onto the NavigationStack.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_push(handle: i64, title_ptr: i64, body_handle: i64) {
    widgets::navstack::push(handle, title_ptr as *const u8, body_handle);
}

/// Pop the top view from the NavigationStack.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_pop(handle: i64) {
    widgets::navstack::pop(handle);
}

/// Create a ZStack (overlay layout). Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_zstack_create() -> i64 {
    widgets::zstack::create()
}

// =============================================================================
// Cross-cutting: Enabled, Hover, DoubleClick, Animations, Tooltip, ControlSize
// =============================================================================

/// Set the enabled state of a widget. enabled: 0=disabled, 1=enabled.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_enabled(handle: i64, enabled: i64) {
    widgets::set_enabled(handle, enabled != 0);
}

/// Rich tooltip (issue #479). iOS — long-press to show, since hover
/// doesn't exist. `hover_delay_ms` is reinterpreted as the
/// UILongPressGestureRecognizer minimumPressDuration.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_rich_tooltip(
    handle: i64,
    content_handle: i64,
    hover_delay_ms: f64,
) {
    let ms = if hover_delay_ms.is_finite() && hover_delay_ms > 0.0 {
        hover_delay_ms as u32
    } else {
        0
    };
    widgets::rich_tooltip::set_rich_tooltip(handle, content_handle, ms);
}

/// Set a tooltip on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_tooltip(handle: i64, text_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    widgets::set_tooltip(handle, str_from_header(text_ptr as *const u8));
}

/// Set the control size of a widget. 0=regular, 1=small, 2=mini, 3=large.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_control_size(handle: i64, size: i64) {
    widgets::set_control_size(handle, size);
}

/// Set an on-hover callback for a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_hover(handle: i64, callback: f64) {
    widgets::set_on_hover(handle, callback);
}

/// Set a single-tap handler for any widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_click(handle: i64, callback: f64) {
    widgets::set_on_click(handle, callback);
}

/// Set a double-click/tap handler for a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_double_click(handle: i64, callback: f64) {
    widgets::set_on_double_click(handle, callback);
}

/// Animate the opacity of a widget. `duration_secs` is in seconds.
#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_opacity(handle: i64, target: f64, duration_secs: f64) {
    widgets::animate_opacity(handle, target, duration_secs);
}

/// Animate the position of a widget by delta. `duration_secs` is in seconds.
#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_position(
    handle: i64,
    dx: f64,
    dy: f64,
    duration_secs: f64,
) {
    widgets::animate_position(handle, dx, dy, duration_secs);
}

/// Register an onChange callback for a state cell.
#[no_mangle]
pub extern "C" fn perry_ui_state_on_change(state_handle: i64, callback: f64) {
    state::state_on_change(state_handle, callback);
}

// =============================================================================
// System APIs (perry/system module)
// =============================================================================

/// Open a URL in the default browser/app.
#[no_mangle]
pub extern "C" fn perry_system_open_url(url_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    let url_str = str_from_header(url_ptr as *const u8);
    unsafe {
        let ns_url_str = objc2_foundation::NSString::from_str(url_str);
        let url_cls = objc2::runtime::AnyClass::get(c"NSURL").unwrap();
        let url: *mut objc2::runtime::AnyObject =
            objc2::msg_send![url_cls, URLWithString: &*ns_url_str];
        if !url.is_null() {
            let app_cls = objc2::runtime::AnyClass::get(c"UIApplication").unwrap();
            let app: *mut objc2::runtime::AnyObject = objc2::msg_send![app_cls, sharedApplication];
            let _: () = objc2::msg_send![app, openURL: url];
        }
    }
}

/// Request one-shot location. Callback receives (lat, lon) or (NaN, NaN) on error.
#[no_mangle]
pub extern "C" fn perry_system_request_location(callback: f64) {
    location::request_location(callback);
}

// ---- Geolocation + image picker (issue #552) ----
#[no_mangle]
pub extern "C" fn perry_system_geolocation_get_current(on_success: f64, on_error: f64) {
    geolocation::get_current(on_success, on_error);
}
#[no_mangle]
pub extern "C" fn perry_system_geolocation_watch(callback: f64) -> f64 {
    geolocation::watch(callback)
}
#[no_mangle]
pub extern "C" fn perry_system_geolocation_stop_watch(id: f64) {
    geolocation::stop_watch(id);
}
#[no_mangle]
pub extern "C" fn perry_system_geolocation_request_permission(callback: f64) {
    geolocation::request_permission(callback);
}
#[no_mangle]
pub extern "C" fn perry_system_image_picker_pick(
    max_count: f64,
    allow_multiple: f64,
    callback: f64,
) {
    image_picker::pick(max_count, allow_multiple, callback);
}

// ---- In-app screen capture (issue #918) ----
/// Capture the key window as a PNG and return a base64-encoded string.
/// Returns an empty string if no key window is available (e.g. before the
/// scene is attached, in tests, or in CLI builds) or capture fails.
#[no_mangle]
pub extern "C" fn perry_system_take_screenshot() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: u32) -> *mut u8;
    }
    use base64::Engine as _;
    unsafe {
        let mut len: usize = 0;
        let ptr = crate::screenshot::perry_ui_screenshot_capture(&mut len as *mut usize);
        if ptr.is_null() || len == 0 {
            return js_string_from_bytes(std::ptr::null(), 0) as i64;
        }
        let bytes = std::slice::from_raw_parts(ptr, len);
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        libc::free(ptr as *mut libc::c_void);
        js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32) as i64
    }
}

// ---- Network reachability (issue #582) ----
#[no_mangle]
pub extern "C" fn perry_system_network_get_status(callback: f64) {
    network::get_status(callback);
}
#[no_mangle]
pub extern "C" fn perry_system_network_on_change(callback: f64) -> f64 {
    network::on_change(callback)
}
#[no_mangle]
pub extern "C" fn perry_system_network_stop_on_change(id: f64) {
    network::stop_on_change(id);
}

// ---- Deep links (issue #583) ----
#[no_mangle]
pub extern "C" fn perry_system_app_on_open_url(callback: f64) {
    deeplinks::set_handler(callback);
}
#[no_mangle]
pub extern "C" fn perry_system_app_get_launch_url() -> i64 {
    let s = deeplinks::launch_url();
    let bytes = s.as_bytes();
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: u32) -> *mut u8;
    }
    unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as i64 }
}

// ---- perry/background (issue #538) — BGTaskScheduler ----
#[no_mangle]
pub extern "C" fn perry_background_register_task(identifier_ptr: i64, handler: f64) {
    background::register_task(identifier_ptr as *const u8, handler);
}
#[no_mangle]
pub extern "C" fn perry_background_schedule(
    identifier_ptr: i64,
    kind_ptr: i64,
    earliest_start_ms: f64,
    requires_network: f64,
    requires_charging: f64,
) {
    background::schedule(
        identifier_ptr as *const u8,
        kind_ptr as *const u8,
        earliest_start_ms,
        requires_network,
        requires_charging,
    );
}
#[no_mangle]
pub extern "C" fn perry_background_cancel(identifier_ptr: i64) {
    background::cancel(identifier_ptr as *const u8);
}

// =============================================================================
// Audio (perry/system) — AVAudioEngine-based microphone capture
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_system_audio_start() -> i64 {
    audio::start()
}

#[no_mangle]
pub extern "C" fn perry_system_audio_stop() {
    audio::stop()
}

#[no_mangle]
pub extern "C" fn perry_system_audio_get_level() -> f64 {
    audio::get_level()
}

#[no_mangle]
pub extern "C" fn perry_system_audio_get_peak() -> f64 {
    audio::get_peak()
}

#[no_mangle]
pub extern "C" fn perry_system_audio_get_waveform(count: f64) -> f64 {
    audio::get_waveform(count)
}

#[no_mangle]
pub extern "C" fn perry_system_get_device_model() -> i64 {
    audio::get_device_model()
}
/// Bug-report-flow utility: stable OS-version string. MVP stub on
/// iOS; native impl will use `[[UIDevice currentDevice] systemVersion]`.
#[no_mangle]
pub extern "C" fn perry_system_get_os_version() -> i64 {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_get_os_version",
        "iOS getOSVersion not yet implemented (UIDevice.systemVersion follow-up)",
        Some("#918"),
    );
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}
#[no_mangle]
pub extern "C" fn perry_system_audio_set_output_filename(filename_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    let filename = str_from_header(filename_ptr as *const u8);
    audio::set_output_filename(filename);
}
#[no_mangle]
pub extern "C" fn perry_system_audio_start_recording() {
    audio::start_recording();
}
#[no_mangle]
pub extern "C" fn perry_system_audio_stop_recording() {
    audio::stop_recording();
}

// =============================================================================
// Camera (perry/ui) — AVCaptureSession-based camera capture
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_camera_create() -> i64 {
    camera::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_start(handle: i64) {
    camera::start(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_stop(handle: i64) {
    camera::stop(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_freeze(handle: i64) {
    camera::freeze(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_unfreeze(handle: i64) {
    camera::unfreeze(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_sample_color(x: f64, y: f64) -> f64 {
    camera::sample_color(x, y)
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_set_on_tap(handle: i64, callback: f64) {
    camera::set_on_tap(handle, callback)
}

/// Check if dark mode is active. Returns 1 if dark, 0 if light.
#[no_mangle]
pub extern "C" fn perry_system_is_dark_mode() -> i64 {
    unsafe {
        let tc_cls = objc2::runtime::AnyClass::get(c"UITraitCollection").unwrap();
        let tc: *mut objc2::runtime::AnyObject = objc2::msg_send![tc_cls, currentTraitCollection];
        if tc.is_null() {
            return 0;
        }
        let style: i64 = objc2::msg_send![tc, userInterfaceStyle];
        if style == 2 {
            1
        } else {
            0
        } // 2 = UIUserInterfaceStyleDark
    }
}

/// Set a preference value (UserDefaults).
#[no_mangle]
pub extern "C" fn perry_system_preferences_set(key_ptr: i64, value: f64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    let key = str_from_header(key_ptr as *const u8);
    let bits = value.to_bits();
    unsafe {
        let defaults_cls = objc2::runtime::AnyClass::get(c"NSUserDefaults").unwrap();
        let defaults: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults_cls, standardUserDefaults];
        let ns_key = objc2_foundation::NSString::from_str(key);
        if (bits >> 48) == 0x7FFF {
            let str_ptr = js_nanbox_get_pointer(value) as *const u8;
            let s = str_from_header(str_ptr);
            let ns_str = objc2_foundation::NSString::from_str(s);
            let _: () = objc2::msg_send![defaults, setObject: &*ns_str, forKey: &*ns_key];
        } else {
            let ns_num: objc2::rc::Retained<objc2::runtime::AnyObject> = objc2::msg_send![
                objc2::runtime::AnyClass::get(c"NSNumber").unwrap(), numberWithDouble: value
            ];
            let _: () = objc2::msg_send![defaults, setObject: &*ns_num, forKey: &*ns_key];
        }
    }
}

/// Get a preference value (UserDefaults). Returns NaN-boxed value or TAG_UNDEFINED.
#[no_mangle]
pub extern "C" fn perry_system_preferences_get(key_ptr: i64) -> f64 {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
        fn js_nanbox_string(ptr: i64) -> f64;
    }
    let key = str_from_header(key_ptr as *const u8);
    unsafe {
        let defaults_cls = objc2::runtime::AnyClass::get(c"NSUserDefaults").unwrap();
        let defaults: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults_cls, standardUserDefaults];
        let ns_key = objc2_foundation::NSString::from_str(key);
        let obj: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults, objectForKey: &*ns_key];
        if obj.is_null() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        if let Some(str_cls) = objc2::runtime::AnyClass::get(c"NSString") {
            let is_string: bool = objc2::msg_send![obj, isKindOfClass: str_cls];
            if is_string {
                let ns_str: &objc2_foundation::NSString =
                    &*(obj as *const objc2_foundation::NSString);
                let rust_str = ns_str.to_string();
                let bytes = rust_str.as_bytes();
                let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                return js_nanbox_string(str_ptr as i64);
            }
        }
        if let Some(num_cls) = objc2::runtime::AnyClass::get(c"NSNumber") {
            let is_number: bool = objc2::msg_send![obj, isKindOfClass: num_cls];
            if is_number {
                let val: f64 = objc2::msg_send![obj, doubleValue];
                return val;
            }
        }
        // NSArray: return first element as string (for AppleLanguages etc.)
        if let Some(arr_cls) = objc2::runtime::AnyClass::get(c"NSArray") {
            let is_array: bool = objc2::msg_send![obj, isKindOfClass: arr_cls];
            if is_array {
                let count: usize = objc2::msg_send![obj, count];
                if count > 0 {
                    let first: *mut objc2::runtime::AnyObject =
                        objc2::msg_send![obj, objectAtIndex: 0usize];
                    if !first.is_null() {
                        if let Some(str_cls2) = objc2::runtime::AnyClass::get(c"NSString") {
                            let is_str: bool = objc2::msg_send![first, isKindOfClass: str_cls2];
                            if is_str {
                                let ns_str: &objc2_foundation::NSString =
                                    &*(first as *const objc2_foundation::NSString);
                                let rust_str = ns_str.to_string();
                                let bytes = rust_str.as_bytes();
                                let str_ptr =
                                    js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                                return js_nanbox_string(str_ptr as i64);
                            }
                        }
                    }
                }
            }
        }
        f64::from_bits(0x7FFC_0000_0000_0001)
    }
}

/// Set border color on a widget via its CALayer.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let layer: *mut objc2::runtime::AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let cg_color = widgets::create_cg_color(r, g, b, a);
                let _: () = objc2::msg_send![layer, setBorderColor: cg_color];
                extern "C" {
                    fn CGColorRelease(color: *mut std::ffi::c_void);
                }
                CGColorRelease(cg_color);
            }
        }
    }
}

/// Set drop shadow on any widget via its CALayer (issue #185 Phase B).
/// Signature mirrors macOS: `(r,g,b,a)` shadow color (alpha → shadowOpacity
/// so a non-1 alpha doesn't double-multiply via the CGColor's alpha),
/// `blur` → shadowRadius, `(offset_x, offset_y)` → shadowOffset CGSize.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_shadow(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
    blur: f64,
    offset_x: f64,
    offset_y: f64,
) {
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let layer: *mut objc2::runtime::AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let cg_color = widgets::create_cg_color(r, g, b, 1.0);
                let _: () = objc2::msg_send![layer, setShadowColor: cg_color];
                extern "C" {
                    fn CGColorRelease(color: *mut std::ffi::c_void);
                }
                CGColorRelease(cg_color);
                let _: () = objc2::msg_send![layer, setShadowOpacity: a as f32];
                let _: () = objc2::msg_send![layer, setShadowRadius: blur];
                let offset = objc2_core_foundation::CGSize::new(offset_x, offset_y);
                let _: () = objc2::msg_send![layer, setShadowOffset: offset];
                // CALayer shadows are clipped by masksToBounds; ensure
                // off so corner-radius widgets still show shadow outside
                // the rounded edge.
                let _: () = objc2::msg_send![layer, setMasksToBounds: false];
            }
        }
    }
}

/// Set border width on a widget via its CALayer.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_width(handle: i64, width: f64) {
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let layer: *mut objc2::runtime::AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let _: () = objc2::msg_send![layer, setBorderWidth: width];
            }
        }
    }
}

/// Set edge insets (padding) on a UIStackView. No-op for other widget types.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_edge_insets(
    handle: i64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) {
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let is_stack = if let Some(cls) = objc2::runtime::AnyClass::get(c"UIStackView") {
                use objc2_foundation::NSObjectProtocol;
                view.isKindOfClass(cls)
            } else {
                false
            };
            if is_stack {
                let _: () = objc2::msg_send![&*view, setLayoutMarginsRelativeArrangement: true];
                let insets = objc2_ui_kit::UIEdgeInsets {
                    top,
                    left,
                    bottom,
                    right,
                };
                let _: () = objc2::msg_send![&*view, setDirectionalLayoutMargins: insets];
            }
        }
    }
}

/// Set view opacity (alpha) in [0.0, 1.0].
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_opacity(handle: i64, alpha: f64) {
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setAlpha: alpha];
        }
    }
}

/// Set the font family on a Text widget.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_family(handle: i64, family_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    let family = str_from_header(family_ptr as *const u8);
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let size: f64 = objc2::msg_send![&*view, font];
            let size = 13.0f64; // Default size for iOS
            let font: objc2::rc::Retained<objc2::runtime::AnyObject> =
                if family == "monospaced" || family == "monospace" {
                    objc2::msg_send![
                        objc2::runtime::AnyClass::get(c"UIFont").unwrap(),
                        monospacedSystemFontOfSize: size,
                        weight: 0.0f64
                    ]
                } else {
                    let ns_name = objc2_foundation::NSString::from_str(family);
                    let raw_font: *mut objc2::runtime::AnyObject = objc2::msg_send![
                        objc2::runtime::AnyClass::get(c"UIFont").unwrap(),
                        fontWithName: &*ns_name,
                        size: size
                    ];
                    if raw_font.is_null() {
                        // Font not found — fall back to system font
                        objc2::msg_send![
                            objc2::runtime::AnyClass::get(c"UIFont").unwrap(),
                            systemFontOfSize: size
                        ]
                    } else {
                        objc2::rc::Retained::retain(raw_font).unwrap()
                    }
                };
            let _: () = objc2::msg_send![&*view, setFont: &*font];
        }
    }
}

// =============================================================================
// QR Code
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_create(data_ptr: i64, size: f64) -> i64 {
    widgets::qrcode::create(data_ptr as *const u8, size)
}

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_set_data(handle: i64, data_ptr: i64) {
    widgets::qrcode::set_data(handle, data_ptr as *const u8);
}

// =============================================================================
// Folder Dialog
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_open_folder_dialog(callback: f64) {
    // iOS: UIDocumentPickerViewController for directories — stub for now
    file_dialog::open_dialog(callback);
}

// =============================================================================
// Save File Dialog
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_save_file_dialog(
    _callback: f64,
    _default_name: i64,
    _allowed_types: i64,
) {
    // iOS: UIDocumentPickerViewController needed — stub for now
}

// =============================================================================
// Poll Open File (stub — iOS uses URL schemes / UIDocumentBrowser instead)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_poll_open_file() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}

// =============================================================================
// Overlay (stub — iOS uses different approach)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_overlay(_parent_handle: i64, _child_handle: i64) {
    // Stub — iOS would use addSubview directly
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_overlay_frame(
    _handle: i64,
    _x: f64,
    _y: f64,
    _w: f64,
    _h: f64,
) {
    // Stub
}

// =============================================================================
// State TextField Binding
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_textfield(state_handle: i64, textfield_handle: i64) {
    state::bind_textfield(state_handle, textfield_handle);
}

// =============================================================================
// Alert Dialog
// =============================================================================

/// Show a UIAlertController with custom buttons. `buttons` is a NaN-boxed
/// JS array of string labels; `callback` (also NaN-boxed) fires with the
/// 0-based index of the tapped button. Issue #708.
#[no_mangle]
pub extern "C" fn perry_ui_alert(title_ptr: i64, message_ptr: i64, buttons: f64, callback: f64) {
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    let buttons_ptr = unsafe { js_nanbox_get_pointer(buttons) };
    widgets::alert::show(
        title_ptr as *const u8,
        message_ptr as *const u8,
        buttons_ptr,
        callback,
    );
}

/// Simple 2-arg alert — single "OK" button, no callback. Issue #708.
#[no_mangle]
pub extern "C" fn perry_ui_alert_simple(title_ptr: i64, message_ptr: i64) {
    widgets::alert::show_simple(title_ptr as *const u8, message_ptr as *const u8);
}

// =============================================================================
// Sheet
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_sheet_create(_width: f64, _height: f64, _title: i64) -> i64 {
    0 // stub
}

#[no_mangle]
pub extern "C" fn perry_ui_sheet_present(_sheet: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_sheet_dismiss(_sheet: i64) {}

// =============================================================================
// Screen Detection (iPad vs iPhone, orientation)
// =============================================================================

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

fn nanbox_static_str(s: &'static [u8]) -> f64 {
    let ptr = unsafe { js_string_from_bytes(s.as_ptr(), s.len() as i64) };
    unsafe { js_nanbox_string(ptr as i64) }
}

/// perry_get_screen_width() → logical width in points (e.g. 820 for iPad Air portrait)
#[no_mangle]
pub extern "C" fn perry_get_screen_width() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        // UIScreen.bounds is orientation-aware since iOS 8
        let bounds: objc2_core_foundation::CGRect = objc2::msg_send![main_screen, bounds];
        bounds.size.width
    }
}

/// perry_get_screen_height() → logical height in points
#[no_mangle]
pub extern "C" fn perry_get_screen_height() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        let bounds: objc2_core_foundation::CGRect = objc2::msg_send![main_screen, bounds];
        bounds.size.height
    }
}

/// perry_get_scale_factor() → device pixel ratio (e.g. 2.0 for iPad, 3.0 for iPhone Pro)
#[no_mangle]
pub extern "C" fn perry_get_scale_factor() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        let scale: f64 = objc2::msg_send![main_screen, scale];
        scale
    }
}

/// perry_get_orientation() → "landscape" or "portrait"
#[no_mangle]
pub extern "C" fn perry_get_orientation() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        let bounds: objc2_core_foundation::CGRect = objc2::msg_send![main_screen, bounds];
        if bounds.size.width > bounds.size.height {
            nanbox_static_str(b"landscape")
        } else {
            nanbox_static_str(b"portrait")
        }
    }
}

/// perry_get_device_idiom() → 0 = phone, 1 = pad
/// Uses UIDevice.model string comparison (more reliable than userInterfaceIdiom
/// which can return 0 before full UIApplication init on iOS 26 simulator).
#[no_mangle]
pub extern "C" fn perry_get_device_idiom() -> f64 {
    unsafe {
        let device_cls = objc2::runtime::AnyClass::get(c"UIDevice").unwrap();
        let current: *mut objc2::runtime::AnyObject = objc2::msg_send![device_cls, currentDevice];

        // Check UIDevice.model — returns @"iPad" on iPad, @"iPhone" on iPhone
        let model: *mut objc2::runtime::AnyObject = objc2::msg_send![current, model];
        let utf8: *const u8 = objc2::msg_send![model, UTF8String];
        if !utf8.is_null() {
            // "iPad" starts with 'i' (0x69) then 'P' (0x50)
            // "iPhone" starts with 'i' (0x69) then 'P' (0x50) too...
            // Actually: "iPad" has 4 chars, "iPhone" has 6 chars
            // Check 3rd char: 'a' (0x61) for iPad vs 'h' (0x68) for iPhone
            let third = *utf8.add(2);
            if third == b'a' {
                // "iPad"
                return 1.0;
            }
        }
        0.0
    }
}

// =============================================================================
// App Lifecycle
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_app_on_terminate(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_on_activate(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_icon(_path_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_size(_app: i64, _w: f64, _h: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_frameless(_app_handle: i64, _value: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_level(_app_handle: i64, _value_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_transparent(_app_handle: i64, _value: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_vibrancy(_app_handle: i64, _value_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_activation_policy(_app_handle: i64, _value_ptr: i64) {}

// =============================================================================
// Toolbar
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_create() -> i64 {
    0 // stub
}

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_add_item(
    _toolbar: i64,
    _label: i64,
    _icon: i64,
    _callback: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_attach(_toolbar: i64) {}

// =============================================================================
// Keychain (iOS — uses SecItem API with data protection keychain)
// =============================================================================

fn keychain_str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

extern "C" {
    fn SecItemAdd(attributes: *const std::ffi::c_void, result: *mut *const std::ffi::c_void)
        -> i32;
    fn SecItemCopyMatching(
        query: *const std::ffi::c_void,
        result: *mut *const std::ffi::c_void,
    ) -> i32;
    fn SecItemUpdate(query: *const std::ffi::c_void, attrs: *const std::ffi::c_void) -> i32;
    fn SecItemDelete(query: *const std::ffi::c_void) -> i32;
    static kSecClass: *const std::ffi::c_void;
    static kSecClassGenericPassword: *const std::ffi::c_void;
    static kSecAttrAccount: *const std::ffi::c_void;
    static kSecAttrService: *const std::ffi::c_void;
    static kSecValueData: *const std::ffi::c_void;
    static kSecReturnData: *const std::ffi::c_void;
    static kSecMatchLimit: *const std::ffi::c_void;
    static kSecMatchLimitOne: *const std::ffi::c_void;
}

unsafe fn keychain_make_query(key: &str) -> objc2::rc::Retained<objc2::runtime::AnyObject> {
    let dict_cls = objc2::runtime::AnyClass::get(c"NSMutableDictionary").unwrap();
    let dict: objc2::rc::Retained<objc2::runtime::AnyObject> = objc2::msg_send![dict_cls, new];
    let _: () = objc2::msg_send![&*dict, setObject: kSecClassGenericPassword as *const objc2::runtime::AnyObject, forKey: kSecClass as *const objc2::runtime::AnyObject];
    let ns_key = objc2_foundation::NSString::from_str(key);
    let _: () = objc2::msg_send![&*dict, setObject: &*ns_key, forKey: kSecAttrAccount as *const objc2::runtime::AnyObject];
    let ns_service = objc2_foundation::NSString::from_str("perry");
    let _: () = objc2::msg_send![&*dict, setObject: &*ns_service, forKey: kSecAttrService as *const objc2::runtime::AnyObject];
    dict
}

#[no_mangle]
pub extern "C" fn perry_system_keychain_save(key_ptr: i64, value_ptr: i64) {
    let key = keychain_str_from_header(key_ptr as *const u8);
    let value = keychain_str_from_header(value_ptr as *const u8);
    unsafe {
        let value_data: objc2::rc::Retained<objc2::runtime::AnyObject> = {
            let ns_str = objc2_foundation::NSString::from_str(value);
            objc2::msg_send![&*ns_str, dataUsingEncoding: 4u64]
        };
        // Try update first
        let query = keychain_make_query(key);
        let dict_cls = objc2::runtime::AnyClass::get(c"NSMutableDictionary").unwrap();
        let update: objc2::rc::Retained<objc2::runtime::AnyObject> =
            objc2::msg_send![dict_cls, new];
        let _: () = objc2::msg_send![&*update, setObject: &*value_data, forKey: kSecValueData as *const objc2::runtime::AnyObject];
        let status = SecItemUpdate(
            &*query as *const _ as *const std::ffi::c_void,
            &*update as *const _ as *const std::ffi::c_void,
        );
        if status == -25300 {
            // errSecItemNotFound
            let add = keychain_make_query(key);
            let _: () = objc2::msg_send![&*add, setObject: &*value_data, forKey: kSecValueData as *const objc2::runtime::AnyObject];
            SecItemAdd(
                &*add as *const _ as *const std::ffi::c_void,
                std::ptr::null_mut(),
            );
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_system_keychain_get(key_ptr: i64) -> f64 {
    let key = keychain_str_from_header(key_ptr as *const u8);
    unsafe {
        let dict = keychain_make_query(key);
        let cf_true: *const objc2::runtime::AnyObject = objc2::msg_send![
            objc2::runtime::AnyClass::get(c"NSNumber").unwrap(), numberWithBool: true
        ];
        let _: () = objc2::msg_send![&*dict, setObject: cf_true, forKey: kSecReturnData as *const objc2::runtime::AnyObject];
        let _: () = objc2::msg_send![&*dict, setObject: kSecMatchLimitOne as *const objc2::runtime::AnyObject, forKey: kSecMatchLimit as *const objc2::runtime::AnyObject];
        let mut result: *const std::ffi::c_void = std::ptr::null();
        let status =
            SecItemCopyMatching(&*dict as *const _ as *const std::ffi::c_void, &mut result);
        if status == 0 && !result.is_null() {
            let data = result as *const objc2::runtime::AnyObject;
            let bytes: *const u8 = objc2::msg_send![data, bytes];
            let length: usize = objc2::msg_send![data, length];
            extern "C" {
                fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
                fn js_nanbox_string(ptr: i64) -> f64;
            }
            let str_ptr = js_string_from_bytes(bytes, length as i64);
            js_nanbox_string(str_ptr as i64)
        } else {
            f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_system_keychain_delete(key_ptr: i64) {
    let key = keychain_str_from_header(key_ptr as *const u8);
    unsafe {
        let query = keychain_make_query(key);
        SecItemDelete(&*query as *const _ as *const std::ffi::c_void);
    }
}

// =============================================================================
// Notifications
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_system_notification_send(title_ptr: i64, body_ptr: i64) {
    notifications::send(title_ptr as *const u8, body_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_system_notification_register_remote(callback: f64) {
    notifications::register_remote(callback);
}

#[no_mangle]
pub extern "C" fn perry_system_notification_on_receive(callback: f64) {
    notifications::on_receive(callback);
}

/// Background-delivery handler (#98). The closure registered here fires from
/// `application:didReceiveRemoteNotification:fetchCompletionHandler:`; iOS's
/// completion handler is invoked once the user's returned Promise settles.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_background_receive(callback: f64) {
    notifications::on_background_receive(callback);
}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_interval(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    seconds: f64,
    repeats: f64,
) {
    notifications::schedule_interval(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        seconds,
        repeats,
    );
}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_calendar(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    timestamp_ms: f64,
) {
    notifications::schedule_calendar(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        timestamp_ms,
    );
}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_location(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    lat: f64,
    lon: f64,
    radius: f64,
) {
    notifications::schedule_location(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        lat,
        lon,
        radius,
    );
}

#[no_mangle]
pub extern "C" fn perry_system_notification_cancel(id_ptr: i64) {
    notifications::cancel(id_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_system_notification_on_tap(callback: f64) {
    notifications::set_on_tap(callback);
}

#[no_mangle]
pub extern "C" fn perry_system_get_locale() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    }
    unsafe {
        // Use currentLocale.languageCode — reflects the actual device language setting
        let ns_locale: *mut objc2::runtime::AnyObject = objc2::msg_send![
            objc2::runtime::AnyClass::get(c"NSLocale").unwrap(),
            currentLocale
        ];
        let lang_code: *mut objc2::runtime::AnyObject = objc2::msg_send![ns_locale, languageCode];
        if lang_code.is_null() {
            let fallback = b"en";
            return js_string_from_bytes(fallback.as_ptr(), 2) as i64;
        }
        let utf8: *const u8 = objc2::msg_send![lang_code, UTF8String];
        let len = libc::strlen(utf8 as *const i8);
        let code_len = if len >= 2 { 2 } else { len };
        js_string_from_bytes(utf8, code_len as i64) as i64
    }
}

// =============================================================================
// Multi-Window
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_window_create(_title: i64, _width: f64, _height: f64) -> i64 {
    0 // stub — iOS uses UIScene for multi-window
}

#[no_mangle]
pub extern "C" fn perry_ui_window_set_body(_window: i64, _widget: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_show(_window: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_close(_window: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_hide(_window: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_set_size(_window: i64, _w: f64, _h: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_on_focus_lost(_window: i64, _callback: f64) {}

// =============================================================================
// LazyVStack
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_create(_count: i64, _render: f64) -> i64 {
    0 // stub
}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_update(_handle: i64, _count: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_row_height(_handle: i64, _height: f64) {}

// =============================================================================
// Issue #553 — BottomNavigation, ImageGallery, scroll-end + pull-to-refresh.
// LazyVStack itself is still a stub on iOS (UITableView wiring is a follow-up
// in its own issue), so its pull-to-refresh + scroll-end FFIs are no-ops
// here too. The ScrollView scroll-end callback IS implemented below — that's
// the version production apps actually reach for today.
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_refresh_control(_handle: i64, _callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_end_refreshing(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_scroll_end_callback(
    _handle: i64,
    _callback: f64,
    _threshold_items: i64,
) {
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_scroll_end_callback(
    handle: i64,
    callback: f64,
    threshold_px: f64,
) {
    widgets::scrollview::set_scroll_end_callback(handle, callback, threshold_px);
}

#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_create(on_select: f64) -> i64 {
    widgets::bottom_nav::create(on_select)
}

#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_add_item(handle: i64, icon_ptr: i64, label_ptr: i64) {
    widgets::bottom_nav::add_item(handle, icon_ptr as *const u8, label_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_badge(handle: i64, index: i64, badge_ptr: i64) {
    widgets::bottom_nav::set_badge(handle, index, badge_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_selected(handle: i64, index: i64) {
    widgets::bottom_nav::set_selected(handle, index);
}

/// Issue #706 — set the tint color of the active tab (RGBA 0.0-1.0).
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::bottom_nav::set_tint_color(handle, r, g, b, a);
}

/// Issue #706 — set the tint color of inactive tabs (RGBA 0.0-1.0).
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_unselected_tint_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::bottom_nav::set_unselected_tint_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_create(on_index_change: f64) -> i64 {
    widgets::image_gallery::create(on_index_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_add_image(handle: i64, url_ptr: i64, alt_ptr: i64) {
    widgets::image_gallery::add_image(handle, url_ptr as *const u8, alt_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_set_index(handle: i64, index: i64) {
    widgets::image_gallery::set_index(handle, index);
}

// =============================================================================
// Table (stub — not yet implemented on iOS)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_table_create(_row_count: f64, _col_count: f64, _render: f64) -> i64 {
    0 // stub
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_header(_handle: i64, _col: i64, _title_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_width(_handle: i64, _col: i64, _width: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_update_row_count(_handle: i64, _count: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_row_select(_handle: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row(_handle: i64) -> i64 {
    -1
}

// =============================================================================
// iOS Documents directory (for persistent storage)
// =============================================================================

/// Returns the app's Documents directory path as a NaN-boxed string.
/// Used by hone-ide's paths.ts for persistent storage on iOS.
#[no_mangle]
pub extern "C" fn hone_get_documents_dir() -> f64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
        fn js_nanbox_string(ptr: i64) -> f64;
    }
    unsafe {
        let file_manager: *const objc2::runtime::AnyObject = objc2::msg_send![
            objc2::runtime::AnyClass::get(c"NSFileManager").unwrap(),
            defaultManager
        ];
        // NSDocumentDirectory = 9, NSUserDomainMask = 1
        let urls: objc2::rc::Retained<objc2_foundation::NSArray<objc2_foundation::NSURL>> =
            objc2::msg_send![file_manager, URLsForDirectory: 9u64, inDomains: 1u64];
        let count: usize = objc2::msg_send![&*urls, count];
        if count > 0 {
            let url: *const objc2::runtime::AnyObject =
                objc2::msg_send![&*urls, objectAtIndex: 0usize];
            let path: objc2::rc::Retained<objc2_foundation::NSString> = objc2::msg_send![url, path];
            let rust_str = path.to_string();
            let bytes = rust_str.as_bytes();
            let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
            js_nanbox_string(str_ptr as i64)
        } else {
            // Return empty string
            let str_ptr = js_string_from_bytes(std::ptr::null(), 0);
            js_nanbox_string(str_ptr as i64)
        }
    }
}

/// Wrapper for Perry codegen (some declare functions use __wrapper_ prefix).
#[no_mangle]
pub extern "C" fn __wrapper_hone_get_documents_dir() -> f64 {
    hone_get_documents_dir()
}

// =============================================================================
// Native iOS WebSocket (bypasses tokio which doesn't work on iOS)
// =============================================================================

#[no_mangle]
pub extern "C" fn hone_ws_connect(url_ptr: i64) -> f64 {
    // Log to file for debugging (Perry GUI apps don't show stderr)
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/hone-ws-debug.log")
    {
        let _ = writeln!(f, "hone_ws_connect called, url_ptr={}", url_ptr);
        let ptr = url_ptr as *const u8;
        if !ptr.is_null() && url_ptr > 0x1000 {
            let header = ptr as *const perry_runtime::string::StringHeader;
            unsafe {
                let len = (*header).byte_len as usize;
                let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
                if let Ok(s) = std::str::from_utf8(std::slice::from_raw_parts(data, len.min(200))) {
                    let _ = writeln!(f, "  url_str={}", s);
                }
            }
        }
    }
    websocket::connect(url_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_connect(url_nanboxed: f64) -> f64 {
    // Wrapper called with f64 NaN-boxed string — extract pointer
    let ptr = perry_runtime::js_get_string_pointer_unified(url_nanboxed);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/hone-ws-debug.log")
    {
        use std::io::Write;
        let _ = writeln!(
            f,
            "__wrapper_hone_ws_connect called, nanboxed={}, extracted_ptr={}",
            url_nanboxed, ptr
        );
    }
    hone_ws_connect(ptr)
}

#[no_mangle]
pub extern "C" fn hone_ws_is_open(handle: f64) -> f64 {
    websocket::is_open(handle)
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_is_open(handle: f64) -> f64 {
    websocket::is_open(handle)
}

#[no_mangle]
pub extern "C" fn hone_ws_send(handle: f64, msg_ptr: i64) {
    websocket::send(handle, msg_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_send(handle: f64, msg_nanboxed: f64) {
    let ptr = perry_runtime::js_get_string_pointer_unified(msg_nanboxed);
    hone_ws_send(handle, ptr)
}

#[no_mangle]
pub extern "C" fn hone_ws_receive(handle: f64) -> f64 {
    websocket::receive(handle)
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_receive(handle: f64) -> f64 {
    websocket::receive(handle)
}

#[no_mangle]
pub extern "C" fn hone_ws_message_count(handle: f64) -> f64 {
    websocket::message_count(handle)
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_message_count(handle: f64) -> f64 {
    websocket::message_count(handle)
}

#[no_mangle]
pub extern "C" fn hone_ws_close(handle: f64) {
    websocket::close(handle)
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_close(handle: f64) {
    websocket::close(handle)
}

// --- Cross-platform toast + reactive setText stubs (Phase 2 v3.3) ---
// Full GTK4 implementation in perry-ui-gtk4. Present here so cross-platform
// code that calls showToast / setText links on iOS targets.

#[no_mangle]
pub extern "C" fn perry_ui_show_toast(_msg_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_text_create_with_id(text_ptr: i64, _id_ptr: i64) -> i64 {
    perry_ui_text_create(text_ptr)
}

#[no_mangle]
pub extern "C" fn perry_ui_set_text(_id_ptr: i64, _value_ptr: i64) {}

// =============================================================================
// perry/media — streaming media playback (issue #351). AVPlayer-backed.
// See `media_playback.rs` for the implementation; everything below is a
// thin FFI thunk that the codegen-emitted `perry_media_*` declarations
// resolve to at link time.
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_media_create_player(url_ptr: i64) -> i64 {
    media_playback::create_player(url_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_media_play(handle: f64) {
    media_playback::play(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_pause(handle: f64) {
    media_playback::pause(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_stop(handle: f64) {
    media_playback::stop(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_seek(handle: f64, seconds: f64) {
    media_playback::seek(handle, seconds);
}

#[no_mangle]
pub extern "C" fn perry_media_set_volume(handle: f64, volume: f64) {
    media_playback::set_volume(handle, volume);
}

#[no_mangle]
pub extern "C" fn perry_media_set_rate(handle: f64, rate: f64) {
    media_playback::set_rate(handle, rate);
}

#[no_mangle]
pub extern "C" fn perry_media_get_current_time(handle: f64) -> f64 {
    media_playback::get_current_time(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_get_duration(handle: f64) -> f64 {
    media_playback::get_duration(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_get_state(handle: f64) -> i64 {
    media_playback::get_state(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_is_playing(handle: f64) -> f64 {
    media_playback::is_playing(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_on_state_change(handle: f64, closure: f64) {
    media_playback::on_state_change(handle, closure);
}

#[no_mangle]
pub extern "C" fn perry_media_on_time_update(handle: f64, closure: f64) {
    media_playback::on_time_update(handle, closure);
}

#[no_mangle]
pub extern "C" fn perry_media_set_now_playing(
    handle: f64,
    title_ptr: i64,
    artist_ptr: i64,
    album_ptr: i64,
    artwork_ptr: i64,
) {
    media_playback::set_now_playing(
        handle,
        title_ptr as *const u8,
        artist_ptr as *const u8,
        album_ptr as *const u8,
        artwork_ptr as *const u8,
    );
}

#[no_mangle]
pub extern "C" fn perry_media_destroy(handle: f64) {
    media_playback::destroy(handle);
}

// =============================================================================
// AttributedText (Issue #710)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_create() -> i64 {
    widgets::attributed_text::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_append(
    handle: i64,
    text_ptr: i64,
    bold: i64,
    italic: i64,
    underline: i64,
    font_size: f64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::attributed_text::append(
        handle,
        text_ptr as *const u8,
        bold,
        italic,
        underline,
        font_size,
        r,
        g,
        b,
        a,
    );
}

#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_clear(handle: i64) {
    widgets::attributed_text::clear(handle);
}
