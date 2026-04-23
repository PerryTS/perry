pub mod app;
pub mod audio;
pub mod camera;
pub mod clipboard;
pub mod crash_log;
pub mod file_dialog;
pub mod location;
pub mod menu;
pub mod screenshot;
pub mod state;
pub mod websocket;
pub mod widgets;

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
            let _: () = unsafe { objc2::msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false] };
            widgets::register_widget(view)
        },
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
pub extern "C" fn perry_ui_splitview_add_child(parent_handle: i64, child_handle: i64, child_index: f64) {
    if let (Some(parent), Some(child)) = (widgets::get_widget(parent_handle), widgets::get_widget(child_handle)) {
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
    if let (Some(parent), Some(child)) = (widgets::get_widget(parent_handle), widgets::get_widget(child_handle)) {
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
    if let (Some(parent), Some(child)) = (widgets::get_widget(parent_handle), widgets::get_widget(child_handle)) {
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
pub extern "C" fn perry_ui_state_bind_text_numeric(state_handle: i64, text_handle: i64, prefix_ptr: i64, suffix_ptr: i64) {
    state::bind_text_numeric(state_handle, text_handle, prefix_ptr as *const u8, suffix_ptr as *const u8);
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
    state::bind_text_template(text_handle, num_parts, types_ptr as *const i32, values_ptr as *const i64);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_visibility(state_handle: i64, show_handle: i64, hide_handle: i64) {
    state::bind_visibility(state_handle, show_handle, hide_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_set_widget_hidden(handle: i64, hidden: i64) {
    widgets::set_hidden(handle, hidden != 0);
}

#[no_mangle]
pub extern "C" fn perry_ui_for_each_init(container_handle: i64, state_handle: i64, render_closure: f64) {
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
pub extern "C" fn perry_ui_vstack_create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64 {
    widgets::vstack::create_with_insets(spacing, top, left, bottom, right)
}

#[no_mangle]
pub extern "C" fn perry_ui_hstack_create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64 {
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
pub extern "C" fn perry_system_get_app_icon(_path: i64) -> i64 { 0 }

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

#[no_mangle]
pub extern "C" fn perry_ui_text_set_selectable(handle: i64, selectable: f64) {
    widgets::text::set_selectable(handle, selectable != 0.0);
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
pub extern "C" fn perry_ui_button_set_content_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
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
pub extern "C" fn perry_ui_widget_reorder_child(parent_handle: i64, from_index: f64, to_index: f64) {
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
            let dist = if distribution < 0.0 { 0_i64 } else { distribution as i64 };
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
pub extern "C" fn perry_ui_menu_add_item_with_shortcut(menu_handle: i64, title_ptr: i64, callback: f64, shortcut_ptr: i64) {
    menu::add_item_with_shortcut(menu_handle, title_ptr as *const u8, callback, shortcut_ptr as *const u8);
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

/// Remove all items from a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_clear(menu_handle: i64) {
    menu::clear(menu_handle);
}

/// Add a menu item with a standard action (no-op on iOS — macOS responder chain concept).
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_standard_action(_menu_handle: i64, _title_ptr: i64, _selector_ptr: i64, _shortcut_ptr: i64) {
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
pub extern "C" fn perry_ui_textfield_set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
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
pub extern "C" fn perry_ui_app_set_timer(interval_ms: f64, callback: f64) {
    app::set_timer(interval_ms, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_color(
    handle: i64, r: f64, g: f64, b: f64, a: f64,
) {
    widgets::set_background_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_gradient(
    handle: i64, r1: f64, g1: f64, b1: f64, a1: f64,
    r2: f64, g2: f64, b2: f64, a2: f64, direction: f64,
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
    handle: i64, r: f64, g: f64, b: f64, a: f64, line_width: f64,
) {
    widgets::canvas::stroke(handle, r, g, b, a, line_width);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_gradient(
    handle: i64, r1: f64, g1: f64, b1: f64, a1: f64,
    r2: f64, g2: f64, b2: f64, a2: f64, direction: f64,
) {
    widgets::canvas::fill_gradient(handle, r1, g1, b1, a1, r2, g2, b2, a2, direction);
}

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
pub extern "C" fn perry_ui_navstack_create(title_ptr: i64, body_handle: i64) -> i64 {
    widgets::navstack::create(title_ptr as *const u8, body_handle)
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

/// Set a tooltip on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_tooltip(handle: i64, text_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() { return ""; }
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
pub extern "C" fn perry_ui_widget_animate_position(handle: i64, dx: f64, dy: f64, duration_secs: f64) {
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
        if ptr.is_null() { return ""; }
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
        let url: *mut objc2::runtime::AnyObject = objc2::msg_send![url_cls, URLWithString: &*ns_url_str];
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
        if tc.is_null() { return 0; }
        let style: i64 = objc2::msg_send![tc, userInterfaceStyle];
        if style == 2 { 1 } else { 0 } // 2 = UIUserInterfaceStyleDark
    }
}

/// Set a preference value (UserDefaults).
#[no_mangle]
pub extern "C" fn perry_system_preferences_set(key_ptr: i64, value: f64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() { return ""; }
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
        let defaults: *mut objc2::runtime::AnyObject = objc2::msg_send![defaults_cls, standardUserDefaults];
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
        if ptr.is_null() { return ""; }
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
        let defaults: *mut objc2::runtime::AnyObject = objc2::msg_send![defaults_cls, standardUserDefaults];
        let ns_key = objc2_foundation::NSString::from_str(key);
        let obj: *mut objc2::runtime::AnyObject = objc2::msg_send![defaults, objectForKey: &*ns_key];
        if obj.is_null() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        if let Some(str_cls) = objc2::runtime::AnyClass::get(c"NSString") {
            let is_string: bool = objc2::msg_send![obj, isKindOfClass: str_cls];
            if is_string {
                let ns_str: &objc2_foundation::NSString = &*(obj as *const objc2_foundation::NSString);
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
                    let first: *mut objc2::runtime::AnyObject = objc2::msg_send![obj, objectAtIndex: 0usize];
                    if !first.is_null() {
                        if let Some(str_cls2) = objc2::runtime::AnyClass::get(c"NSString") {
                            let is_str: bool = objc2::msg_send![first, isKindOfClass: str_cls2];
                            if is_str {
                                let ns_str: &objc2_foundation::NSString = &*(first as *const objc2_foundation::NSString);
                                let rust_str = ns_str.to_string();
                                let bytes = rust_str.as_bytes();
                                let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
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
                extern "C" { fn CGColorRelease(color: *mut std::ffi::c_void); }
                CGColorRelease(cg_color);
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
pub extern "C" fn perry_ui_widget_set_edge_insets(handle: i64, top: f64, left: f64, bottom: f64, right: f64) {
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
                let insets = objc2_ui_kit::UIEdgeInsets { top, left, bottom, right };
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
        if ptr.is_null() { return ""; }
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
            let font: objc2::rc::Retained<objc2::runtime::AnyObject> = if family == "monospaced" || family == "monospace" {
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
pub extern "C" fn perry_ui_save_file_dialog(_callback: f64, _default_name: i64, _allowed_types: i64) {
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
pub extern "C" fn perry_ui_widget_set_overlay_frame(_handle: i64, _x: f64, _y: f64, _w: f64, _h: f64) {
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

#[no_mangle]
pub extern "C" fn perry_ui_alert(_title: i64, _message: i64, _buttons: i64, _callback: f64) {
    // iOS: UIAlertController — stub for now
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
pub extern "C" fn perry_ui_toolbar_add_item(_toolbar: i64, _label: i64, _icon: i64, _callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_attach(_toolbar: i64) {}

// =============================================================================
// Keychain (iOS — uses SecItem API with data protection keychain)
// =============================================================================

fn keychain_str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() { return ""; }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

extern "C" {
    fn SecItemAdd(attributes: *const std::ffi::c_void, result: *mut *const std::ffi::c_void) -> i32;
    fn SecItemCopyMatching(query: *const std::ffi::c_void, result: *mut *const std::ffi::c_void) -> i32;
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
        let update: objc2::rc::Retained<objc2::runtime::AnyObject> = objc2::msg_send![dict_cls, new];
        let _: () = objc2::msg_send![&*update, setObject: &*value_data, forKey: kSecValueData as *const objc2::runtime::AnyObject];
        let status = SecItemUpdate(&*query as *const _ as *const std::ffi::c_void, &*update as *const _ as *const std::ffi::c_void);
        if status == -25300 { // errSecItemNotFound
            let add = keychain_make_query(key);
            let _: () = objc2::msg_send![&*add, setObject: &*value_data, forKey: kSecValueData as *const objc2::runtime::AnyObject];
            SecItemAdd(&*add as *const _ as *const std::ffi::c_void, std::ptr::null_mut());
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
        let status = SecItemCopyMatching(&*dict as *const _ as *const std::ffi::c_void, &mut result);
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
pub extern "C" fn perry_system_notification_send(_title: i64, _body: i64) {}

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
        let file_manager: *const objc2::runtime::AnyObject =
            objc2::msg_send![objc2::runtime::AnyClass::get(c"NSFileManager").unwrap(), defaultManager];
        // NSDocumentDirectory = 9, NSUserDomainMask = 1
        let urls: objc2::rc::Retained<objc2_foundation::NSArray<objc2_foundation::NSURL>> =
            objc2::msg_send![file_manager, URLsForDirectory: 9u64, inDomains: 1u64];
        let count: usize = objc2::msg_send![&*urls, count];
        if count > 0 {
            let url: *const objc2::runtime::AnyObject = objc2::msg_send![&*urls, objectAtIndex: 0usize];
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
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/hone-ws-debug.log") {
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
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/hone-ws-debug.log") {
        use std::io::Write;
        let _ = writeln!(f, "__wrapper_hone_ws_connect called, nanboxed={}, extracted_ptr={}", url_nanboxed, ptr);
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
