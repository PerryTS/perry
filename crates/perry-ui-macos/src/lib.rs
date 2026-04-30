pub mod app;
pub mod audio;
pub mod clipboard;
pub mod crash_log;
pub mod file_dialog;
pub mod keychain;
pub mod location;
pub mod media_playback;
pub mod menu;
pub mod notifications;
pub mod state;
pub mod string_header;
pub mod widgets;

pub mod screenshot;

#[cfg(feature = "geisterhand")]
pub mod geisterhand_style;

/// Run a closure, catching any Rust panics so they don't abort across the FFI boundary.
/// The global panic hook (installed by crash_log) writes to crash.log first;
/// if we catch the panic here (non-fatal), we clear the log so it doesn't
/// get reported as a crash on next launch.
pub fn catch_callback_panic<F: FnOnce() + std::panic::UnwindSafe>(label: &str, f: F) {
    if let Err(e) = std::panic::catch_unwind(f) {
        // Panic hook already wrote to crash.log — clear it since we caught this one
        crash_log::clear_crash_log();

        let msg = if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            format!("{:?}", e)
        };
        eprintln!("[perry] panic in {} (caught): {}", label, msg);
    }
}

// =============================================================================
// FFI exports — these are the functions called from codegen-generated code
// =============================================================================

/// Create an app. title_ptr=raw string, width/height as f64.
/// Returns app handle (i64).
#[no_mangle]
pub extern "C" fn perry_ui_app_create(title_ptr: i64, width: f64, height: f64) -> i64 {
    app::app_create(title_ptr as *const u8, width, height)
}

/// Set the root widget of an app.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_body(app_handle: i64, root_handle: i64) {
    app::app_set_body(app_handle, root_handle);
}

/// Run the app event loop (blocks until window closes).
#[no_mangle]
pub extern "C" fn perry_ui_app_run(app_handle: i64) {
    app::app_run(app_handle);
}

/// Set the application dock icon from a file path.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_icon(path_ptr: i64) {
    app::app_set_icon(path_ptr as *const u8);
}

/// Resize the main app window.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_size(app_handle: i64, width: f64, height: f64) {
    app::app_set_size(app_handle, width, height);
}

/// Set frameless window mode (no titlebar). value = NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_frameless(app_handle: i64, value: f64) {
    app::app_set_frameless(app_handle, value);
}

/// Set window level. value_ptr = string pointer ("floating", "statusBar", etc.).
#[no_mangle]
pub extern "C" fn perry_ui_app_set_level(app_handle: i64, value_ptr: i64) {
    app::app_set_level(app_handle, value_ptr as *const u8);
}

/// Set window transparency. value = NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_transparent(app_handle: i64, value: f64) {
    app::app_set_transparent(app_handle, value);
}

/// Set vibrancy material. value_ptr = string pointer ("sidebar", etc.).
#[no_mangle]
pub extern "C" fn perry_ui_app_set_vibrancy(app_handle: i64, value_ptr: i64) {
    app::app_set_vibrancy(app_handle, value_ptr as *const u8);
}

/// Set activation policy. value_ptr = string pointer ("regular", "accessory", "background").
#[no_mangle]
pub extern "C" fn perry_ui_app_set_activation_policy(app_handle: i64, value_ptr: i64) {
    app::app_set_activation_policy(app_handle, value_ptr as *const u8);
}

/// Poll for pending file-open requests (from macOS Open With or argv).
/// Returns a StringHeader pointer (empty string if none pending).
#[no_mangle]
pub extern "C" fn perry_ui_poll_open_file() -> i64 {
    let path = app::poll_open_file();
    if path.is_empty() {
        // Return empty string
        extern "C" {
            fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
        }
        unsafe { js_string_from_bytes(std::ptr::null(), 0) }
    } else {
        extern "C" {
            fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
        }
        unsafe { js_string_from_bytes(path.as_ptr(), path.len() as i32) }
    }
}

/// Register an external NSView (from a native library) as a Perry widget.
/// Returns widget handle usable with widgetAddChild, widgetSetWidth, etc.
#[no_mangle]
pub extern "C" fn perry_ui_embed_nsview(nsview_ptr: i64) -> i64 {
    widgets::register_external_nsview(nsview_ptr)
}

/// Create a Text label. text_ptr = raw string pointer. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_text_create(text_ptr: i64) -> i64 {
    widgets::text::create(text_ptr as *const u8)
}

/// Create a Button. label_ptr = raw string, on_press = NaN-boxed closure.
/// Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_button_create(label_ptr: i64, on_press: f64) -> i64 {
    widgets::button::create(label_ptr as *const u8, on_press)
}

/// Create a VStack container. spacing = f64. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_vstack_create(spacing: f64) -> i64 {
    widgets::vstack::create(spacing)
}

/// Create an HStack container. spacing = f64. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_hstack_create(spacing: f64) -> i64 {
    widgets::hstack::create(spacing)
}

/// Add a child widget to a parent widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child(parent_handle: i64, child_handle: i64) {
    widgets::add_child(parent_handle, child_handle);
}

/// Add a child as a floating overlay (not arranged in stack layout).
#[no_mangle]
pub extern "C" fn perry_ui_widget_add_overlay(parent_handle: i64, child_handle: i64) {
    widgets::add_overlay(parent_handle, child_handle);
}

/// Set the frame (position + size) of an overlay child.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_overlay_frame(handle: i64, x: f64, y: f64, w: f64, h: f64) {
    widgets::set_overlay_frame(handle, x, y, w, h);
}

/// Remove a child widget from a parent widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_remove_child(parent_handle: i64, child_handle: i64) {
    widgets::remove_child(parent_handle, child_handle);
}

/// Reorder a child widget within a parent (NSStackView) by index.
#[no_mangle]
pub extern "C" fn perry_ui_widget_reorder_child(
    parent_handle: i64,
    from_index: f64,
    to_index: f64,
) {
    widgets::reorder_child(parent_handle, from_index as i64, to_index as i64);
}

/// Create a reactive state cell. initial = f64 value. Returns state handle.
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

/// Bind a text widget to a state cell with prefix and suffix strings.
/// When the state changes, text updates to "{prefix}{value}{suffix}".
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

/// Create a Spacer (flexible space). Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_spacer_create() -> i64 {
    widgets::spacer::create()
}

/// Create a Divider (horizontal separator). Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_divider_create() -> i64 {
    widgets::divider::create()
}

/// Create an editable TextField. placeholder_ptr = string, on_change = NaN-boxed closure.
/// Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::textfield::create(placeholder_ptr as *const u8, on_change)
}

/// Create a Toggle (switch + label). label_ptr = string, on_change = NaN-boxed closure.
/// Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_toggle_create(label_ptr: i64, on_change: f64) -> i64 {
    widgets::toggle::create(label_ptr as *const u8, on_change)
}

/// Create a Slider. min/max are f64, on_change = NaN-boxed closure.
/// Returns widget handle. Initial value defaults to `min` (the TS
/// surface `Slider(min, max, onChange)` doesn't expose `initial`, and
/// codegen emits a 3-arg call — the prior 4-arg FFI relied on
/// undefined register state for the missing arg, silently NaN on
/// some calling conventions).
#[no_mangle]
pub extern "C" fn perry_ui_slider_create(min: f64, max: f64, on_change: f64) -> i64 {
    widgets::slider::create(min, max, min, on_change)
}

// =============================================================================
// Phase 4: Advanced Reactive UI
// =============================================================================

/// Bind a slider to a state cell (two-way binding).
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_slider(state_handle: i64, slider_handle: i64) {
    state::bind_slider(state_handle, slider_handle);
}

/// Bind a toggle to a state cell (two-way binding).
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

/// Bind visibility of widgets to a state cell (conditional rendering).
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_visibility(
    state_handle: i64,
    show_handle: i64,
    hide_handle: i64,
) {
    state::bind_visibility(state_handle, show_handle, hide_handle);
}

/// Set the hidden state of a widget. hidden: 0=visible, 1=hidden.
#[no_mangle]
pub extern "C" fn perry_ui_set_widget_hidden(handle: i64, hidden: i64) {
    widgets::set_hidden(handle, hidden != 0);
}

/// Set detachesHiddenViews on an NSStackView.
/// When flag=0, hidden views still participate in layout.
#[no_mangle]
pub extern "C" fn perry_ui_stack_set_detaches_hidden(handle: i64, flag: i64) {
    widgets::set_detaches_hidden_views(handle, flag != 0);
}

/// Set distribution on an NSStackView.
/// 0 = Fill (default), 1 = FillEqually, 2 = FillProportionally,
/// 3 = EqualSpacing, 4 = EqualCentering, -1 = GravityAreas.
#[no_mangle]
pub extern "C" fn perry_ui_stack_set_distribution(handle: i64, distribution: f64) {
    widgets::set_distribution(handle, distribution as i64);
}

/// Set alignment on an NSStackView.
/// For vertical stacks: Leading=5, CenterX=9, Width=7.
/// For horizontal stacks: CenterY=12, Top=3, Bottom=4.
#[no_mangle]
pub extern "C" fn perry_ui_stack_set_alignment(handle: i64, alignment: f64) {
    widgets::set_alignment(handle, alignment as i64);
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

/// Remove all children from a container widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_clear_children(handle: i64) {
    widgets::clear_children(handle);
}

// =============================================================================
// Phase A.1: Text Mutation & Layout Control
// =============================================================================

/// Set the text content of a Text widget (NSTextField label).
#[no_mangle]
pub extern "C" fn perry_ui_text_set_string(handle: i64, text_ptr: i64) {
    widgets::text::set_string(handle, text_ptr as *const u8);
}

/// Create a VStack with custom edge insets.
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

/// Create an HStack with custom edge insets.
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

/// Create a ScrollView. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_create() -> i64 {
    widgets::scrollview::create()
}

/// Set the content child of a ScrollView.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_child(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::set_child(scroll_handle, child_handle);
}

/// Read text from the system clipboard. Returns NaN-boxed string.
#[no_mangle]
pub extern "C" fn perry_ui_clipboard_read() -> f64 {
    clipboard::read()
}

/// Write text to the system clipboard.
#[no_mangle]
pub extern "C" fn perry_ui_clipboard_write(text_ptr: i64) {
    clipboard::write(text_ptr as *const u8);
}

/// Add a keyboard shortcut to the app menu.
#[no_mangle]
pub extern "C" fn perry_ui_add_keyboard_shortcut(key_ptr: i64, modifiers: f64, callback: f64) {
    app::add_keyboard_shortcut(key_ptr as *const u8, modifiers, callback);
}

/// Register a system-wide global hotkey (fires even when app is in background).
#[no_mangle]
pub extern "C" fn perry_ui_register_global_hotkey(key_ptr: i64, modifiers: f64, callback: f64) {
    app::register_global_hotkey(key_ptr as *const u8, modifiers, callback);
}

// =============================================================================
// Phase A.3: Text Styling & Button Styling
// =============================================================================

/// Set the text color of a Text widget (RGBA 0.0-1.0).
#[no_mangle]
pub extern "C" fn perry_ui_text_set_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::text::set_color(handle, r, g, b, a);
}

/// Set the font size of a Text widget.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_size(handle: i64, size: f64) {
    widgets::text::set_font_size(handle, size);
}

/// Set the font weight of a Text widget (size + weight).
#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_weight(handle: i64, size: f64, weight: f64) {
    widgets::text::set_font_weight(handle, size, weight);
}

/// Enable word wrapping on a Text widget with a max width.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_wraps(handle: i64, max_width: f64) {
    widgets::text::set_wraps(handle, max_width);
}

/// Set text decoration on a Text widget (issue #185 Phase B).
/// `decoration`: 0=none, 1=underline, 2=strikethrough.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_decoration(handle: i64, decoration: i64) {
    widgets::text::set_decoration(handle, decoration);
}

/// Set whether a Text widget is selectable.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_selectable(handle: i64, selectable: f64) {
    widgets::text::set_selectable(handle, selectable != 0.0);
}

/// Set the text color of a Button.
#[no_mangle]
pub extern "C" fn perry_ui_button_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::button::set_text_color(handle, r, g, b, a);
}

/// Set a fixed width constraint on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_width(handle: i64, width: f64) {
    widgets::set_width(handle, width);
}

/// Set a fixed height constraint on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_height(handle: i64, height: f64) {
    widgets::set_height(handle, height);
}

/// Set the content hugging priority on a widget (both axes).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_hugging(handle: i64, priority: f64) {
    widgets::set_hugging_priority(handle, priority);
}

/// Pin a child view's leading and trailing to its superview so it fills the parent width.
#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_width(handle: i64) {
    widgets::match_parent_width(handle);
}

/// Pin a child view's top and bottom to its superview so it fills the parent height.
#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_height(handle: i64) {
    widgets::match_parent_height(handle);
}

/// Set whether a Button has a border.
#[no_mangle]
pub extern "C" fn perry_ui_button_set_bordered(handle: i64, bordered: f64) {
    widgets::button::set_bordered(handle, bordered != 0.0);
}

/// Set the title of a Button.
#[no_mangle]
pub extern "C" fn perry_ui_button_set_title(handle: i64, title_ptr: i64) {
    widgets::button::set_title(handle, title_ptr as *const u8);
}

/// Set an SF Symbol image on a Button.
#[no_mangle]
pub extern "C" fn perry_ui_button_set_image(handle: i64, name_ptr: i64) {
    widgets::button::set_image(handle, name_ptr as *const u8);
}

/// Set the content tint color of a Button (for SF Symbol icon coloring).
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

/// Set the image position of a Button (0=NoImage, 1=ImageOnly, 2=Left, 7=Leading).
#[no_mangle]
pub extern "C" fn perry_ui_button_set_image_position(handle: i64, position: i64) {
    widgets::button::set_image_position(handle, position);
}

// =============================================================================
// Phase A.4: Focus & Scroll-To
// =============================================================================

/// Focus a TextField (make it the first responder).
#[no_mangle]
pub extern "C" fn perry_ui_textfield_focus(handle: i64) {
    widgets::textfield::focus(handle);
}

/// Scroll a ScrollView to make a child visible.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_scroll_to(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::scroll_to(scroll_handle, child_handle);
}

/// Get the vertical scroll offset.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_get_offset(scroll_handle: i64) -> f64 {
    widgets::scrollview::get_offset(scroll_handle)
}

/// Set the vertical scroll offset.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_offset(scroll_handle: i64, offset: f64) {
    widgets::scrollview::set_offset(scroll_handle, offset);
}

// =============================================================================
// Phase A.5: Context Menus, File Dialog & Window Sizing
// =============================================================================

/// Create a context menu. Returns menu handle.
#[no_mangle]
pub extern "C" fn perry_ui_menu_create() -> i64 {
    menu::create()
}

/// Add an item to a context menu with title and callback.
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item(menu_handle: i64, title_ptr: i64, callback: f64) {
    menu::add_item(menu_handle, title_ptr as *const u8, callback);
}

/// Set a context menu on a widget (right-click menu).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_context_menu(widget_handle: i64, menu_handle: i64) {
    menu::set_context_menu(widget_handle, menu_handle);
}

/// Add a menu item with a keyboard shortcut.
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

/// Add a menu item with a standard action (nil target → first responder).
/// Used for Edit menu: Copy, Paste, Cut, Undo, Redo, Select All.
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_standard_action(
    menu_handle: i64,
    title_ptr: i64,
    selector_ptr: i64,
    shortcut_ptr: i64,
) {
    menu::add_standard_action(
        menu_handle,
        title_ptr as *const u8,
        selector_ptr as *const u8,
        shortcut_ptr as *const u8,
    );
}

/// Remove all items from a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_clear(menu_handle: i64) {
    menu::clear(menu_handle);
}

/// Add a separator to a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_separator(menu_handle: i64) {
    menu::add_separator(menu_handle);
}

/// Add a submenu to a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_submenu(menu_handle: i64, title_ptr: i64, submenu_handle: i64) {
    menu::add_submenu(menu_handle, title_ptr as *const u8, submenu_handle);
}

/// Create a menu bar. Returns bar handle.
#[no_mangle]
pub extern "C" fn perry_ui_menubar_create() -> i64 {
    menu::menubar_create()
}

/// Add a menu to a menu bar with a title.
#[no_mangle]
pub extern "C" fn perry_ui_menubar_add_menu(bar_handle: i64, title_ptr: i64, menu_handle: i64) {
    menu::menubar_add_menu(bar_handle, title_ptr as *const u8, menu_handle);
}

/// Attach a menu bar to the application.
#[no_mangle]
pub extern "C" fn perry_ui_menubar_attach(bar_handle: i64) {
    menu::menubar_attach(bar_handle);
}

/// Open a file dialog. Calls callback with selected path or undefined if cancelled.
#[no_mangle]
pub extern "C" fn perry_ui_open_file_dialog(callback: f64) {
    file_dialog::open_dialog(callback);
}

/// Open a folder dialog. Calls callback with selected directory path or undefined.
#[no_mangle]
pub extern "C" fn perry_ui_open_folder_dialog(callback: f64) {
    file_dialog::open_folder_dialog(callback);
}

/// Set minimum window size.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_min_size(app_handle: i64, w: f64, h: f64) {
    app::set_min_size(app_handle, w, h);
}

/// Set maximum window size.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_max_size(app_handle: i64, w: f64, h: f64) {
    app::set_max_size(app_handle, w, h);
}

/// Set the text value of an editable TextField.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_string(handle: i64, text_ptr: i64) {
    widgets::textfield::set_string_value(handle, text_ptr as *const u8);
}

/// Get the current text content of a TextField.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_get_string(handle: i64) -> i64 {
    widgets::textfield::get_string_value(handle) as i64
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_submit(handle: i64, on_submit: f64) {
    widgets::textfield::set_on_submit(handle, on_submit);
}

/// Set an onFocus callback for a text field (fires when editing begins).
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_focus(handle: i64, on_focus: f64) {
    widgets::textfield::set_on_focus(handle, on_focus);
}

/// Resign first responder from the key window (blur all text fields).
#[no_mangle]
pub extern "C" fn perry_ui_textfield_blur_all() {
    widgets::textfield::blur_all();
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_next_key_view(handle: i64, next_handle: i64) {
    widgets::textfield::set_next_key_view(handle, next_handle);
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

// --- TextArea (multi-line editor) ---

/// Create a multi-line text area with onChange callback. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_textarea_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::textarea::create(placeholder_ptr as *const u8, on_change)
}

/// Set the text of a TextArea.
#[no_mangle]
pub extern "C" fn perry_ui_textarea_set_string(handle: i64, text_ptr: i64) {
    widgets::textarea::set_string(handle, text_ptr as *const u8);
}

/// Get the text of a TextArea as a StringHeader pointer.
#[no_mangle]
pub extern "C" fn perry_ui_textarea_get_string(handle: i64) -> i64 {
    widgets::textarea::get_string(handle) as i64
}

/// Add a child widget to a parent widget at a specific position.
#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child_at(parent_handle: i64, child_handle: i64, index: f64) {
    widgets::add_child_at(parent_handle, child_handle, index as i64);
}

// =============================================================================
// Weather App Extensions
// =============================================================================

/// Set a recurring timer on the UI event loop.
/// Calls js_stdlib_process_pending() before each callback invocation.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_timer(_app_handle: i64, interval_ms: f64, callback: f64) {
    app::set_timer(interval_ms, callback);
}

/// Set a linear gradient background on any widget.
/// direction: 0=vertical (top→bottom), 1=horizontal (left→right)
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

/// Set a solid background color on any widget.
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

/// Set corner radius on any widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_corner_radius(handle: i64, radius: f64) {
    widgets::set_corner_radius(handle, radius);
}

/// Set border color on any widget via its CALayer.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::set_border_color(handle, r, g, b, a);
}

/// Set border width on any widget via its CALayer.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_width(handle: i64, width: f64) {
    widgets::set_border_width(handle, width);
}

/// Set drop shadow on any widget via its CALayer (issue #185 Phase B).
/// (r,g,b,a) is shadow color; alpha lands in `shadowOpacity`. `blur` is
/// `shadowRadius`; `(offset_x, offset_y)` is `shadowOffset` (positive y =
/// downward, matching HTML `box-shadow`).
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
    widgets::set_shadow(handle, r, g, b, a, blur, offset_x, offset_y);
}

/// Set edge insets (padding) on an NSStackView widget. No-op for other widget types.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_edge_insets(
    handle: i64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) {
    widgets::set_edge_insets(handle, top, left, bottom, right);
}

/// Set view opacity in [0.0, 1.0].
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_opacity(handle: i64, alpha: f64) {
    widgets::set_opacity(handle, alpha);
}

/// Create a Canvas widget for custom drawing.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_create(width: f64, height: f64) -> i64 {
    widgets::canvas::create(width, height)
}

/// Clear all drawing commands from a Canvas.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_clear(handle: i64) {
    widgets::canvas::clear(handle);
}

/// Begin a new path on a Canvas.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_begin_path(handle: i64) {
    widgets::canvas::begin_path(handle);
}

/// Move the pen to (x, y) on a Canvas.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_move_to(handle: i64, x: f64, y: f64) {
    widgets::canvas::move_to(handle, x, y);
}

/// Add a line segment to (x, y) on a Canvas.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_line_to(handle: i64, x: f64, y: f64) {
    widgets::canvas::line_to(handle, x, y);
}

/// Stroke the current path with color and line width.
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

/// Fill the current path with a linear gradient.
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

/// Create a QR code image view. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_qrcode_create(data_ptr: i64, size: f64) -> i64 {
    widgets::qrcode::create(data_ptr as *const u8, size)
}

/// Update QR code content.
#[no_mangle]
pub extern "C" fn perry_ui_qrcode_set_data(handle: i64, data_ptr: i64) {
    widgets::qrcode::set_data(handle, data_ptr as *const u8);
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

/// Set a tooltip on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_tooltip(handle: i64, text_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const crate::string_header::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
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

/// Set a double-click/tap handler for a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_double_click(handle: i64, callback: f64) {
    widgets::set_on_double_click(handle, callback);
}

/// Set a single-click handler for any widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_click(handle: i64, callback: f64) {
    widgets::set_on_click(handle, callback);
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
            let header = ptr as *const crate::string_header::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
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
            let workspace_cls = objc2::runtime::AnyClass::get(c"NSWorkspace").unwrap();
            let workspace: *mut objc2::runtime::AnyObject =
                objc2::msg_send![workspace_cls, sharedWorkspace];
            let _: bool = objc2::msg_send![workspace, openURL: url];
        }
    }
}

/// Check if dark mode is active. Returns 1 if dark, 0 if light.
#[no_mangle]
pub extern "C" fn perry_system_is_dark_mode() -> i64 {
    unsafe {
        // Method 1: NSUserDefaults — works before the window exists and for
        // explicit Dark mode. Returns nil for Auto mode.
        let defaults_cls = objc2::runtime::AnyClass::get(c"NSUserDefaults").unwrap();
        let defaults: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults_cls, standardUserDefaults];
        let key = objc2_foundation::NSString::from_str("AppleInterfaceStyle");
        let style: *mut objc2::runtime::AnyObject = objc2::msg_send![defaults, stringForKey: &*key];
        if !style.is_null() {
            let dark_str = objc2_foundation::NSString::from_str("Dark");
            let is_dark: bool = objc2::msg_send![style, isEqualToString: &*dark_str];
            if is_dark {
                return 1;
            }
        }

        // Method 2: NSApp.effectiveAppearance — works once the app is initialized.
        let app_cls = objc2::runtime::AnyClass::get(c"NSApplication").unwrap();
        let app: *mut objc2::runtime::AnyObject = objc2::msg_send![app_cls, sharedApplication];
        let appearance: *mut objc2::runtime::AnyObject = objc2::msg_send![app, effectiveAppearance];
        if !appearance.is_null() {
            let name: *mut objc2::runtime::AnyObject = objc2::msg_send![appearance, name];
            if !name.is_null() {
                let dark_name = objc2_foundation::NSString::from_str("NSAppearanceNameDarkAqua");
                let is_dark: bool = objc2::msg_send![name, isEqualToString: &*dark_name];
                if is_dark {
                    return 1;
                }
            }
        }
        0
    }
}

/// Set a preference value (UserDefaults). Supports strings and numbers.
#[no_mangle]
pub extern "C" fn perry_system_preferences_set(key_ptr: i64, value: f64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const crate::string_header::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
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
            // NaN-boxed string — extract string pointer
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

/// Get a preference value (UserDefaults). Returns NaN-boxed string, number, or TAG_UNDEFINED.
#[no_mangle]
pub extern "C" fn perry_system_preferences_get(key_ptr: i64) -> f64 {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const crate::string_header::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
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
            return f64::from_bits(0x7FFC_0000_0000_0001); // TAG_UNDEFINED
        }
        // Check if it's an NSString
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
        // Check if it's an NSNumber
        if let Some(num_cls) = objc2::runtime::AnyClass::get(c"NSNumber") {
            let is_number: bool = objc2::msg_send![obj, isKindOfClass: num_cls];
            if is_number {
                let val: f64 = objc2::msg_send![obj, doubleValue];
                return val;
            }
        }
        f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
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
            let header = ptr as *const crate::string_header::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    let family = str_from_header(family_ptr as *const u8);
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let tf: &objc2_app_kit::NSTextField =
                &*(objc2::rc::Retained::as_ptr(&view) as *const objc2_app_kit::NSTextField);
            // Get current font size (default 13.0 if none)
            let current_font: Option<objc2::rc::Retained<objc2_app_kit::NSFont>> = tf.font();
            let size = current_font.as_ref().map(|f| f.pointSize()).unwrap_or(13.0);

            let font: objc2::rc::Retained<objc2_app_kit::NSFont> =
                if family == "monospaced" || family == "monospace" {
                    objc2::msg_send![
                        objc2::runtime::AnyClass::get(c"NSFont").unwrap(),
                        monospacedSystemFontOfSize: size as objc2_core_foundation::CGFloat,
                        weight: 0.0 as objc2_core_foundation::CGFloat
                    ]
                } else {
                    let ns_name = objc2_foundation::NSString::from_str(family);
                    let result: *mut objc2_app_kit::NSFont = objc2::msg_send![
                        objc2::runtime::AnyClass::get(c"NSFont").unwrap(),
                        fontWithName: &*ns_name,
                        size: size as objc2_core_foundation::CGFloat
                    ];
                    if result.is_null() {
                        // Fallback to system font
                        objc2::msg_send![
                            objc2::runtime::AnyClass::get(c"NSFont").unwrap(),
                            systemFontOfSize: size as objc2_core_foundation::CGFloat
                        ]
                    } else {
                        objc2::rc::Retained::retain(result).unwrap()
                    }
                };
            tf.setFont(Some(&font));
        }
    }
}

// =============================================================================
// Save File Dialog
// =============================================================================

/// Open a save file dialog. Calls callback with selected path or undefined.
#[no_mangle]
pub extern "C" fn perry_ui_save_file_dialog(
    callback: f64,
    default_name_ptr: i64,
    allowed_types_ptr: i64,
) {
    file_dialog::save_dialog(
        callback,
        default_name_ptr as *const u8,
        allowed_types_ptr as *const u8,
    );
}

// =============================================================================
// State TextField Binding (two-way)
// =============================================================================

/// Bind a TextField to a state cell (two-way binding).
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_textfield(state_handle: i64, textfield_handle: i64) {
    state::bind_textfield(state_handle, textfield_handle);
}

// =============================================================================
// Alert Dialog
// =============================================================================

/// Show an alert dialog with custom buttons.
/// `buttons` is a NaN-boxed JS array of string labels; the callback is
/// invoked with the 0-based index of the clicked button. Called from
/// `alertWithButtons(title, message, buttons, cb)` in TS.
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

/// Simple 2-arg alert: shows NSAlert with the title/message and a single
/// "OK" button. Called from `alert(title, message)` in TS.
#[no_mangle]
pub extern "C" fn perry_ui_alert_simple(title_ptr: i64, message_ptr: i64) {
    widgets::alert::show_simple(title_ptr as *const u8, message_ptr as *const u8);
}

// =============================================================================
// Sheet (Modal Panel)
// =============================================================================

/// Create a sheet (panel). Returns handle.
/// title_val arrives as NaN-boxed f64 from codegen — extract pointer internally.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_create(width: f64, height: f64, title_val: f64) -> i64 {
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    let title_ptr = unsafe { js_nanbox_get_pointer(title_val) } as *const u8;
    widgets::sheet::create(width, height, title_ptr)
}

/// Present a sheet on the key window.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_present(sheet_handle: i64) {
    widgets::sheet::present(sheet_handle);
}

/// Dismiss a sheet.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_dismiss(sheet_handle: i64) {
    widgets::sheet::dismiss(sheet_handle);
}

// =============================================================================
// App Lifecycle Hooks
// =============================================================================

/// Register an onTerminate callback.
#[no_mangle]
pub extern "C" fn perry_ui_app_on_terminate(callback: f64) {
    app::register_on_terminate(callback);
}

/// Register an onActivate callback.
#[no_mangle]
pub extern "C" fn perry_ui_app_on_activate(callback: f64) {
    app::register_on_activate(callback);
}

// =============================================================================
// Toolbar
// =============================================================================

/// Create a toolbar. Returns handle.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_create() -> i64 {
    widgets::toolbar::create()
}

/// Add an item to a toolbar.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_add_item(
    toolbar_handle: i64,
    label_ptr: i64,
    icon_ptr: i64,
    callback: f64,
) {
    widgets::toolbar::add_item(
        toolbar_handle,
        label_ptr as *const u8,
        icon_ptr as *const u8,
        callback,
    );
}

/// Attach a toolbar to the key window.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_attach(toolbar_handle: i64) {
    widgets::toolbar::attach(toolbar_handle);
}

// =============================================================================
// Keychain (perry/system)
// =============================================================================

/// Save a value to the keychain.
#[no_mangle]
pub extern "C" fn perry_system_keychain_save(key_ptr: i64, value_ptr: i64) {
    crate::keychain::save(key_ptr as *const u8, value_ptr as *const u8);
}

/// Get a value from the keychain. Returns NaN-boxed string or TAG_UNDEFINED.
#[no_mangle]
pub extern "C" fn perry_system_keychain_get(key_ptr: i64) -> f64 {
    crate::keychain::get(key_ptr as *const u8)
}

/// Delete a value from the keychain.
#[no_mangle]
pub extern "C" fn perry_system_keychain_delete(key_ptr: i64) {
    crate::keychain::delete(key_ptr as *const u8);
}

// =============================================================================
// Notifications (perry/system)
// =============================================================================

/// Send a local notification.
#[no_mangle]
pub extern "C" fn perry_system_notification_send(title_ptr: i64, body_ptr: i64) {
    crate::notifications::send(title_ptr as *const u8, body_ptr as *const u8);
}

/// Register for remote (APNs) notifications. `callback` is invoked once with
/// the device token hex string when iOS/macOS negotiates one.
#[no_mangle]
pub extern "C" fn perry_system_notification_register_remote(callback: f64) {
    crate::notifications::register_remote(callback);
}

/// Register a handler for foreground remote-notification payloads.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_receive(callback: f64) {
    crate::notifications::on_receive(callback);
}

/// Background-receive (#98) — no-op on macOS. Desktop apps don't have an
/// equivalent of `application:didReceiveRemoteNotification:fetchCompletionHandler:`;
/// the foreground delegate fires for both foregrounded and background app
/// states (NSApplication doesn't suspend background processes the way iOS
/// does), so user code targeting macOS should register `notificationOnReceive`
/// instead. Stub kept so cross-platform user code linking in macOS doesn't
/// fail to resolve the symbol.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_background_receive(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_interval(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    seconds: f64,
    repeats: f64,
) {
    crate::notifications::schedule_interval(
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
    crate::notifications::schedule_calendar(
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
    crate::notifications::schedule_location(
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
    crate::notifications::cancel(id_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_system_notification_on_tap(callback: f64) {
    crate::notifications::set_on_tap(callback);
}

// =============================================================================
// Location (perry/system) — stub on macOS, iOS only
// =============================================================================

/// Request one-shot location.
#[no_mangle]
pub extern "C" fn perry_system_request_location(callback: f64) {
    location::request_location(callback);
}

// =============================================================================
// Audio (perry/system) — AVAudioEngine-based microphone capture
// =============================================================================

/// Start audio capture. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn perry_system_audio_start() -> i64 {
    audio::start()
}

/// Stop audio capture.
#[no_mangle]
pub extern "C" fn perry_system_audio_stop() {
    audio::stop()
}

/// Get current smoothed dB(A) level.
#[no_mangle]
pub extern "C" fn perry_system_audio_get_level() -> f64 {
    audio::get_level()
}

/// Get current peak sample amplitude.
#[no_mangle]
pub extern "C" fn perry_system_audio_get_peak() -> f64 {
    audio::get_peak()
}

/// Get recent dB samples for waveform rendering.
#[no_mangle]
pub extern "C" fn perry_system_audio_get_waveform(count: f64) -> f64 {
    audio::get_waveform(count)
}

/// Get device model identifier string.
#[no_mangle]
pub extern "C" fn perry_system_get_device_model() -> i64 {
    audio::get_device_model()
}

/// Get the icon for a file/application at the given path. Returns a widget handle (NSImageView).
#[no_mangle]
pub extern "C" fn perry_system_get_app_icon(path_ptr: i64) -> i64 {
    app::get_app_icon(path_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_system_get_locale() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe {
        let ns_locale: *mut objc2::runtime::AnyObject = objc2::msg_send![
            objc2::runtime::AnyClass::get(c"NSLocale").unwrap(),
            currentLocale
        ];
        let lang_code: *mut objc2::runtime::AnyObject = objc2::msg_send![ns_locale, languageCode];
        if lang_code.is_null() {
            let fallback = b"en";
            return js_string_from_bytes(fallback.as_ptr(), 2);
        }
        let utf8: *const u8 = objc2::msg_send![lang_code, UTF8String];
        let len = libc::strlen(utf8 as *const i8);
        let code_len = if len >= 2 { 2 } else { len };
        js_string_from_bytes(utf8, code_len as i32)
    }
}

// =============================================================================
// Multi-Window
// =============================================================================

/// Create a new window. Returns window handle.
#[no_mangle]
pub extern "C" fn perry_ui_window_create(title_ptr: i64, width: f64, height: f64) -> i64 {
    app::window_create(title_ptr as *const u8, width, height)
}

/// Set the root widget of a window.
#[no_mangle]
pub extern "C" fn perry_ui_window_set_body(window_handle: i64, widget_handle: i64) {
    app::window_set_body(window_handle, widget_handle);
}

/// Show a window.
#[no_mangle]
pub extern "C" fn perry_ui_window_show(window_handle: i64) {
    app::window_show(window_handle);
}

/// Close a window.
#[no_mangle]
pub extern "C" fn perry_ui_window_close(window_handle: i64) {
    app::window_close(window_handle);
}

/// Hide a window without destroying it.
#[no_mangle]
pub extern "C" fn perry_ui_window_hide(window_handle: i64) {
    app::window_hide(window_handle);
}

/// Set window size.
#[no_mangle]
pub extern "C" fn perry_ui_window_set_size(window_handle: i64, width: f64, height: f64) {
    app::window_set_size(window_handle, width, height);
}

/// Register a callback for when the window loses focus.
#[no_mangle]
pub extern "C" fn perry_ui_window_on_focus_lost(window_handle: i64, callback: f64) {
    app::window_on_focus_lost(window_handle, callback);
}

// =============================================================================
// LazyVStack (Virtualized List)
// =============================================================================

/// Create a LazyVStack with row count and render closure. Returns handle.
/// count arrives as f64 from codegen — cast to i64 internally.
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_create(count: f64, render_closure: f64) -> i64 {
    widgets::lazyvstack::create(count as i64, render_closure)
}

/// Update the row count of a LazyVStack.
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_update(handle: i64, count: i64) {
    widgets::lazyvstack::update_count(handle, count);
}

/// Set the uniform row height on a virtualized LazyVStack.
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_row_height(handle: i64, height: f64) {
    widgets::lazyvstack::set_row_height(handle, height);
}

// =============================================================================
// Table (NSTableView)
// =============================================================================

/// Create a Table with row_count rows, col_count columns, and a render closure.
/// row_count and col_count arrive as f64 (JS numbers) — cast to i64 internally.
#[no_mangle]
pub extern "C" fn perry_ui_table_create(row_count: f64, col_count: f64, render: f64) -> i64 {
    widgets::table::create(row_count as i64, col_count as i64, render)
}

/// Set the header title of column col (0-based). title_ptr is a StringHeader pointer.
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_header(handle: i64, col: i64, title_ptr: i64) {
    widgets::table::set_column_header(handle, col, title_ptr as *const u8)
}

/// Set the width of column col (0-based).
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_width(handle: i64, col: i64, width: f64) {
    widgets::table::set_column_width(handle, col, width)
}

/// Update the total row count and reload the table view.
#[no_mangle]
pub extern "C" fn perry_ui_table_update_row_count(handle: i64, count: i64) {
    widgets::table::update_row_count(handle, count)
}

/// Register a selection callback (row: number) => void.
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_row_select(handle: i64, callback: f64) {
    widgets::table::set_on_row_select(handle, callback)
}

/// Return the index of the currently selected row, or -1 if none.
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row(handle: i64) -> i64 {
    widgets::table::get_selected_row(handle)
}

// =============================================================================
// Splitview / VBox stubs — these are iOS-only layout containers.
// macOS uses NSStackView which handles all layouts fine.
// Stubs are needed so the linker resolves the symbols.
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_splitview_create(_left_width: f64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_splitview_add_child(_parent: i64, _child: i64, _index: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_vbox_create() -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_vbox_add_child(_parent: i64, _child: i64, _slot: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_vbox_finalize(_parent: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_frame_split_create(_left_width: f64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_frame_split_add_child(_parent: i64, _child: i64) {}

// =============================================================================
// Screen detection stubs — iOS-only, macOS uses desktop defaults in TS.
// Return 0/NaN so the TS validation rejects them and falls back to defaults.
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_get_screen_width() -> f64 {
    0.0
}

#[no_mangle]
pub extern "C" fn perry_get_screen_height() -> f64 {
    0.0
}

#[no_mangle]
pub extern "C" fn perry_get_scale_factor() -> f64 {
    0.0
}

#[no_mangle]
pub extern "C" fn perry_get_orientation() -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_on_layout_change(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_get_device_idiom() -> f64 {
    0.0
}

// --- TabBar stubs (not yet implemented for macOS) ---

#[no_mangle]
pub extern "C" fn perry_ui_tabbar_create(_on_change: f64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_tabbar_add_tab(_handle: i64, _label_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_tabbar_set_selected(_handle: i64, _index: i64) {}

// --- ScrollView refresh control stubs (not yet implemented for macOS) ---

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_refresh_control(_handle: i64, _callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_end_refreshing(_handle: i64) {}

// --- Camera stubs (issue #191) ---
// Real implementations live in `perry-ui-ios` (AVCaptureSession) and
// `perry-ui-android` (Camera2). macOS has working AVFoundation but the
// preview-layer plumbing isn't wired through perry-ui-macos yet — these
// no-ops let cross-platform user code link cleanly today and can be
// replaced incrementally.

#[no_mangle]
pub extern "C" fn perry_ui_camera_create() -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_start(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_camera_stop(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_camera_freeze(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_camera_unfreeze(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_camera_sample_color(_x: f64, _y: f64) -> f64 {
    -1.0
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_set_on_tap(_handle: i64, _callback: f64) {}

// --- Cross-platform toast + reactive setText stubs (Phase 2 v3.3) ---
// Full GTK4 implementation in perry-ui-gtk4. Symbols present here so code
// that calls showToast / setText links on macOS targets today. Replace with
// real AppKit/UNUserNotificationCenter + NSLabel implementations in #326.

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
// See `media_playback.rs` for the actual implementation; everything below
// is a thin FFI thunk that the codegen-emitted `perry_media_*` declarations
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
