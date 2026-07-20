use crate::ffi::js_string_from_bytes;
use crate::*;

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

/// Issue #1280 — initial window state. value_ptr = StringHeader pointer to
/// one of "normal" | "maximized" | "fullscreen". Applied on app_run().
#[no_mangle]
pub extern "C" fn perry_ui_app_set_window_state(app_handle: i64, value_ptr: i64) {
    app::set_window_state(app_handle, value_ptr as *const u8);
}

/// Poll for pending file-open requests (from macOS Open With or argv).
/// Returns a StringHeader pointer (empty string if none pending).
#[no_mangle]
pub extern "C" fn perry_ui_poll_open_file() -> i64 {
    let path = app::poll_open_file();
    if path.is_empty() {
        // Return empty string
        unsafe { js_string_from_bytes(std::ptr::null(), 0) as i64 }
    } else {
        unsafe { js_string_from_bytes(path.as_ptr(), path.len() as u32) as i64 }
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

/// Create an AdBanner (#867). `unit_id_ptr` / `size_ptr` are string
/// headers (ad unit id + size key). On macOS this is a layout-only
/// placeholder — Google Mobile Ads ships no macOS SDK — sized to match
/// the real banner slot on iOS/Android. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_adbanner_create(unit_id_ptr: i64, size_ptr: i64) -> i64 {
    widgets::adbanner::create(unit_id_ptr as *const u8, size_ptr as *const u8)
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

/// Set an existing Toggle's on/off state (issue #5076). `on` is 0 for
/// off, non-zero for on.
#[no_mangle]
pub extern "C" fn perry_ui_toggle_set_state(handle: i64, on: i64) {
    widgets::toggle::set_state(handle, on);
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
