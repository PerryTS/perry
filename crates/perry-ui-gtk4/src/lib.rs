pub mod app;
pub mod audio;
pub mod clipboard;
pub mod dialog;
pub mod file_dialog;
pub mod keychain;
pub mod location;
pub mod menu;
pub mod sheet;
pub mod state;
pub mod system;
pub mod toolbar;
pub mod widgets;
pub mod window;

pub mod screenshot;

#[cfg(feature = "geisterhand")]
pub mod geisterhand_style;

// =============================================================================
// FFI exports — these are the functions called from codegen-generated code
// =============================================================================

/// Create an app. title_ptr=raw string, width/height as f64.
/// Returns app handle (i64).
#[no_mangle]
pub extern "C" fn perry_ui_app_create(title_ptr: i64, width: f64, height: f64) -> i64 {
    let result = app::app_create(title_ptr as *const u8, width, height);
    result
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

/// Resize the main app window.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_size(app_handle: i64, width: f64, height: f64) {
    app::app_set_size(app_handle, width, height);
}

/// Set frameless window mode (no decorations). value = NaN-boxed boolean.
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

/// Set vibrancy material. value_ptr = string pointer.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_vibrancy(app_handle: i64, value_ptr: i64) {
    app::app_set_vibrancy(app_handle, value_ptr as *const u8);
}

/// Set activation policy. value_ptr = string pointer ("regular", "accessory", "background").
#[no_mangle]
pub extern "C" fn perry_ui_app_set_activation_policy(app_handle: i64, value_ptr: i64) {
    app::app_set_activation_policy(app_handle, value_ptr as *const u8);
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

/// Register callback for app activation.
#[no_mangle]
pub extern "C" fn perry_ui_app_on_activate(callback: f64) {
    app::on_activate(callback);
}

/// Register callback for app termination.
#[no_mangle]
pub extern "C" fn perry_ui_app_on_terminate(callback: f64) {
    app::on_terminate(callback);
}

/// Set a repeating timer.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_timer(_app_handle: i64, interval_ms: f64, callback: f64) {
    app::set_timer(interval_ms, callback);
}

// =============================================================================
// Multi-Window
// =============================================================================

/// Create a new window.
#[no_mangle]
pub extern "C" fn perry_ui_window_create(title_ptr: i64, width: f64, height: f64) -> i64 {
    window::create(title_ptr as *const u8, width, height)
}

/// Set the body of a window.
#[no_mangle]
pub extern "C" fn perry_ui_window_set_body(window_handle: i64, widget_handle: i64) {
    window::set_body(window_handle, widget_handle);
}

/// Show a window.
#[no_mangle]
pub extern "C" fn perry_ui_window_show(window_handle: i64) {
    window::show(window_handle);
}

/// Close a window.
#[no_mangle]
pub extern "C" fn perry_ui_window_close(window_handle: i64) {
    window::close(window_handle);
}

/// Hide a window without destroying it.
#[no_mangle]
pub extern "C" fn perry_ui_window_hide(window_handle: i64) {
    window::hide(window_handle);
}

/// Set window size.
#[no_mangle]
pub extern "C" fn perry_ui_window_set_size(window_handle: i64, width: f64, height: f64) {
    window::set_size(window_handle, width, height);
}

/// Register a callback for when the window loses focus.
#[no_mangle]
pub extern "C" fn perry_ui_window_on_focus_lost(window_handle: i64, callback: f64) {
    window::on_focus_lost(window_handle, callback);
}

// =============================================================================
// Widget Creation
// =============================================================================

/// Embed an external GtkWidget (from a native FFI library) as a Perry widget.
/// The ptr is a raw GtkWidget pointer (as returned by hone_editor_nsview).
/// Returns a Perry widget handle usable with widgetAddChild, VStack, etc.
#[no_mangle]
pub extern "C" fn perry_ui_embed_nsview(ptr: i64) -> i64 {
    eprintln!("[perry-ui] perry_ui_embed_nsview({:#x})", ptr);
    if ptr == 0 {
        eprintln!("[perry-ui] perry_ui_embed_nsview: null ptr, returning 0");
        return 0;
    }
    let widget: gtk4::Widget =
        unsafe { gtk4::glib::translate::from_glib_none(ptr as *mut gtk4::ffi::GtkWidget) };
    let handle = widgets::register_widget(widget);
    eprintln!("[perry-ui] perry_ui_embed_nsview -> handle {}", handle);
    handle
}

/// Create a Text label.
#[no_mangle]
pub extern "C" fn perry_ui_text_create(text_ptr: i64) -> i64 {
    widgets::text::create(text_ptr as *const u8)
}

/// Create a Button.
#[no_mangle]
pub extern "C" fn perry_ui_button_create(label_ptr: i64, on_press: f64) -> i64 {
    widgets::button::create(label_ptr as *const u8, on_press)
}

/// Create a VStack container.
#[no_mangle]
pub extern "C" fn perry_ui_vstack_create(spacing: f64) -> i64 {
    widgets::vstack::create(spacing)
}

/// Create an HStack container.
#[no_mangle]
pub extern "C" fn perry_ui_hstack_create(spacing: f64) -> i64 {
    widgets::hstack::create(spacing)
}

/// Create a Spacer.
#[no_mangle]
pub extern "C" fn perry_ui_spacer_create() -> i64 {
    widgets::spacer::create()
}

/// Create a Divider.
#[no_mangle]
pub extern "C" fn perry_ui_divider_create() -> i64 {
    widgets::divider::create()
}

/// Create a TextField.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::textfield::create(placeholder_ptr as *const u8, on_change)
}

/// Create a TextArea (multi-line text input).
#[no_mangle]
pub extern "C" fn perry_ui_textarea_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::textarea::create(placeholder_ptr as *const u8, on_change)
}

/// Set the text content of a TextArea.
#[no_mangle]
pub extern "C" fn perry_ui_textarea_set_string(handle: i64, text_ptr: i64) {
    widgets::textarea::set_string(handle, text_ptr as *const u8);
}

/// Get the text content of a TextArea as a StringHeader pointer.
#[no_mangle]
pub extern "C" fn perry_ui_textarea_get_string(handle: i64) -> i64 {
    widgets::textarea::get_string(handle) as i64
}

/// Create a SecureField (password entry).
#[no_mangle]
pub extern "C" fn perry_ui_securefield_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::securefield::create(placeholder_ptr as *const u8, on_change)
}

/// Create a Toggle.
#[no_mangle]
pub extern "C" fn perry_ui_toggle_create(label_ptr: i64, on_change: f64) -> i64 {
    widgets::toggle::create(label_ptr as *const u8, on_change)
}

/// Create a Slider.
#[no_mangle]
pub extern "C" fn perry_ui_slider_create(min: f64, max: f64, on_change: f64) -> i64 {
    // Codegen emits 3-arg `Slider(min, max, onChange)` per the TS surface.
    // Default initial value to `min` so users get a valid widget without
    // a 4th NaN-from-uninitialized-register arg corrupting GtkAdjustment.
    widgets::slider::create(min, max, min, on_change)
}

/// Create a ScrollView.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_create() -> i64 {
    widgets::scrollview::create()
}

/// Create a Canvas.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_create(width: f64, height: f64) -> i64 {
    widgets::canvas::create(width, height)
}

/// Create a Form container.
#[no_mangle]
pub extern "C" fn perry_ui_form_create() -> i64 {
    widgets::form::create()
}

/// Create a Section with title.
#[no_mangle]
pub extern "C" fn perry_ui_section_create(title_ptr: i64) -> i64 {
    widgets::form::section_create(title_ptr as *const u8)
}

/// Create a ZStack (overlay container).
#[no_mangle]
pub extern "C" fn perry_ui_zstack_create() -> i64 {
    widgets::zstack::create()
}

/// Create a SplitView (horizontal split-pane). Mirrors macOS
/// `perry_ui_splitview_create` (`NSSplitView`). On GTK4 backed by
/// `gtk::Paned` — supports exactly 2 children (vs N on macOS).
#[no_mangle]
pub extern "C" fn perry_ui_splitview_create(left_width: f64) -> i64 {
    widgets::splitview::create(left_width)
}

/// Add a child to a SplitView. First call → start child, second →
/// end child, third+ → no-op + warning (GTK4 `Paned` cap).
#[no_mangle]
pub extern "C" fn perry_ui_splitview_add_child(parent: i64, child: i64, index: f64) {
    widgets::splitview::add_child(parent, child, index as i64);
}

/// Create a LazyVStack.
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_create(count: f64, render_closure: f64) -> i64 {
    widgets::lazyvstack::create(count, render_closure)
}

/// Update a LazyVStack with a new item count.
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_update(handle: i64, count: i64) {
    widgets::lazyvstack::update(handle, count);
}

/// Set the uniform row height. GTK4 eager-renders rows so this is advisory —
/// it's stored but has no effect until GTK4 gets a virtualized backend.
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_row_height(_handle: i64, _height: f64) {}

// Table (stub — not yet implemented on GTK4)
#[no_mangle]
pub extern "C" fn perry_ui_table_create(_row_count: f64, _col_count: f64, _render: f64) -> i64 {
    0
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

/// Create a ProgressView.
#[no_mangle]
pub extern "C" fn perry_ui_progressview_create() -> i64 {
    widgets::progressview::create()
}

/// Set progress value (0.0-1.0, negative = indeterminate).
#[no_mangle]
pub extern "C" fn perry_ui_progressview_set_value(handle: i64, value: f64) {
    widgets::progressview::set_value(handle, value);
}

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

// =============================================================================
// Text Styling
// =============================================================================

/// Set the text content of a Text widget.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_string(handle: i64, text_ptr: i64) {
    widgets::text::set_string(handle, text_ptr as *const u8);
}

/// Set the text color.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::text::set_color(handle, r, g, b, a);
}

/// Set the font size.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_size(handle: i64, size: f64) {
    widgets::text::set_font_size(handle, size);
}

/// Set the font weight.
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

/// Set whether text is selectable.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_selectable(handle: i64, selectable: f64) {
    widgets::text::set_selectable(handle, selectable != 0.0);
}

/// Set the font family.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_family(handle: i64, family_ptr: i64) {
    widgets::text::set_font_family(handle, family_ptr as *const u8);
}

// =============================================================================
// Button Ops
// =============================================================================

/// Set whether a button has a border.
#[no_mangle]
pub extern "C" fn perry_ui_button_set_bordered(handle: i64, bordered: f64) {
    widgets::button::set_bordered(handle, bordered != 0.0);
}

/// Set the title of a button.
#[no_mangle]
pub extern "C" fn perry_ui_button_set_title(handle: i64, title_ptr: i64) {
    widgets::button::set_title(handle, title_ptr as *const u8);
}

/// Set the text color of a button's label.
#[no_mangle]
pub extern "C" fn perry_ui_button_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::button::set_text_color(handle, r, g, b, a);
}

/// Set the tint color of a button's image/icon.
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

/// Set the position of the image relative to the label on a button.
#[no_mangle]
pub extern "C" fn perry_ui_button_set_image_position(handle: i64, position: i64) {
    widgets::button::set_image_position(handle, position);
}

// =============================================================================
// TextField Ops
// =============================================================================

/// Focus a TextField.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_focus(handle: i64) {
    widgets::textfield::focus(handle);
}

/// Set the text value of a TextField.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_string(handle: i64, text_ptr: i64) {
    widgets::textfield::set_string_value(handle, text_ptr as *const u8);
}

// =============================================================================
// ScrollView
// =============================================================================

/// Set the child of a ScrollView.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_child(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::set_child(scroll_handle, child_handle);
}

/// Scroll to make a child visible.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_scroll_to(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::scroll_to(scroll_handle, child_handle);
}

/// Get scroll offset.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_get_offset(scroll_handle: i64) -> f64 {
    widgets::scrollview::get_offset(scroll_handle)
}

/// Set scroll offset.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_offset(scroll_handle: i64, offset: f64) {
    widgets::scrollview::set_offset(scroll_handle, offset);
}

// =============================================================================
// Styling
// =============================================================================

/// Set background color.
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

/// Set background gradient.
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

/// Set corner radius.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_corner_radius(handle: i64, radius: f64) {
    widgets::set_corner_radius(handle, radius);
}

/// Set drop shadow on a widget via CSS `box-shadow` (issue #185 Phase B).
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

/// Set opacity on any widget (issue #185 Phase B). GTK4 has a built-in
/// `Widget::set_opacity` so this is a one-line passthrough.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_opacity(handle: i64, opacity: f64) {
    widgets::set_opacity(handle, opacity);
}

/// Set border color (issue #185 Phase B). Joint state with set_border_width.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::set_border_color(handle, r, g, b, a);
}

/// Set border width (issue #185 Phase B). Joint state with set_border_color.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_width(handle: i64, width: f64) {
    widgets::set_border_width(handle, width);
}

/// Set context menu on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_context_menu(widget_handle: i64, menu_handle: i64) {
    menu::set_context_menu(widget_handle, menu_handle);
}

/// Set control size.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_control_size(handle: i64, size: i64) {
    widgets::set_control_size(handle, size);
}

/// Set enabled/disabled.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_enabled(handle: i64, enabled: i64) {
    widgets::set_enabled(handle, enabled != 0);
}

/// Set tooltip.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_tooltip(handle: i64, text_ptr: i64) {
    widgets::set_tooltip(handle, text_ptr as *const u8);
}

/// Set hidden state.
#[no_mangle]
pub extern "C" fn perry_ui_set_widget_hidden(handle: i64, hidden: i64) {
    widgets::set_hidden(handle, hidden != 0);
}

// =============================================================================
// Canvas
// =============================================================================

/// Clear a canvas.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_clear(handle: i64) {
    widgets::canvas::clear(handle);
}

/// Begin a new path on a canvas.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_begin_path(handle: i64) {
    widgets::canvas::begin_path(handle);
}

/// Move the path cursor.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_move_to(handle: i64, x: f64, y: f64) {
    widgets::canvas::move_to(handle, x, y);
}

/// Draw a line to a point.
#[no_mangle]
pub extern "C" fn perry_ui_canvas_line_to(handle: i64, x: f64, y: f64) {
    widgets::canvas::line_to(handle, x, y);
}

/// Stroke the current path.
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

/// Fill the current path with a gradient.
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

// Stateful 2D-context API stubs (full implementation tracked in perry-ui-test as `U`).
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
// Menu
// =============================================================================

/// Create a context menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_create() -> i64 {
    menu::create()
}

/// Add an item to a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item(menu_handle: i64, title_ptr: i64, callback: f64) {
    menu::add_item(menu_handle, title_ptr as *const u8, callback);
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

/// Remove all items from a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_clear(menu_handle: i64) {
    menu::clear(menu_handle);
}

/// Add a menu item with a standard action (no-op on GTK4 — macOS responder chain concept).
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_standard_action(
    _menu_handle: i64,
    _title_ptr: i64,
    _selector_ptr: i64,
    _shortcut_ptr: i64,
) {
    // No-op on GTK4 — standard actions are handled by GtkTextView built-in bindings
}

// =============================================================================
// Clipboard
// =============================================================================

/// Read from clipboard.
#[no_mangle]
pub extern "C" fn perry_ui_clipboard_read() -> f64 {
    clipboard::read()
}

/// Write to clipboard.
#[no_mangle]
pub extern "C" fn perry_ui_clipboard_write(text_ptr: i64) {
    clipboard::write(text_ptr as *const u8);
}

// =============================================================================
// Dialog
// =============================================================================

/// Open a file dialog.
#[no_mangle]
pub extern "C" fn perry_ui_open_file_dialog(callback: f64) {
    file_dialog::open_dialog(callback);
}

/// Open a folder picker. Mirrors macOS `perry_ui_open_folder_dialog`.
#[no_mangle]
pub extern "C" fn perry_ui_open_folder_dialog(callback: f64) {
    file_dialog::open_folder_dialog(callback);
}

/// Open a save file dialog.
#[no_mangle]
pub extern "C" fn perry_ui_save_file_dialog(
    callback: f64,
    default_name_ptr: i64,
    allowed_types_ptr: i64,
) {
    dialog::save_file_dialog(
        callback,
        default_name_ptr as *const u8,
        allowed_types_ptr as *const u8,
    );
}

/// Show an alert dialog with custom buttons.
/// `buttons` is a NaN-boxed JS array of string labels; the callback is
/// invoked with the 0-based index of the clicked button.
#[no_mangle]
pub extern "C" fn perry_ui_alert(title_ptr: i64, message_ptr: i64, buttons: f64, callback: f64) {
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    let buttons_ptr = unsafe { js_nanbox_get_pointer(buttons) } as *const u8;
    dialog::alert(
        title_ptr as *const u8,
        message_ptr as *const u8,
        buttons_ptr,
        callback,
    );
}

/// Show a simple alert (title, message, OK button). Called from `alert(title, message)`.
#[no_mangle]
pub extern "C" fn perry_ui_alert_simple(title_ptr: i64, message_ptr: i64) {
    dialog::alert_simple(title_ptr as *const u8, message_ptr as *const u8);
}

// =============================================================================
// Keyboard Shortcut
// =============================================================================

/// Add a keyboard shortcut.
#[no_mangle]
pub extern "C" fn perry_ui_add_keyboard_shortcut(key_ptr: i64, modifiers: f64, callback: f64) {
    app::add_keyboard_shortcut(key_ptr as *const u8, modifiers, callback);
}

/// Register a system-wide global hotkey (not yet supported on Linux).
#[no_mangle]
pub extern "C" fn perry_ui_register_global_hotkey(key_ptr: i64, modifiers: f64, callback: f64) {
    app::register_global_hotkey(key_ptr as *const u8, modifiers, callback);
}

/// Get the icon for an application at the given path. Returns a widget handle or 0.
#[no_mangle]
pub extern "C" fn perry_system_get_app_icon(path_ptr: i64) -> i64 {
    app::get_app_icon(path_ptr as *const u8)
}

// =============================================================================
// Events
// =============================================================================

/// Set an on-hover callback.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_hover(handle: i64, callback: f64) {
    widgets::set_on_hover(handle, callback);
}

/// Set a single-click callback.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_click(handle: i64, callback: f64) {
    widgets::set_on_click(handle, callback);
}

/// Set a double-click callback.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_double_click(handle: i64, callback: f64) {
    widgets::set_on_double_click(handle, callback);
}

// =============================================================================
// Animation
// =============================================================================

/// Animate opacity. `duration_secs` is in seconds.
#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_opacity(handle: i64, target: f64, duration_secs: f64) {
    widgets::animate_opacity(handle, target, duration_secs);
}

/// Animate position. `duration_secs` is in seconds.
#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_position(
    handle: i64,
    dx: f64,
    dy: f64,
    duration_secs: f64,
) {
    widgets::animate_position(handle, dx, dy, duration_secs);
}

// =============================================================================
// Layout — width and hugging (GTK4 equivalents of NSLayoutConstraint)
// =============================================================================

/// Set a fixed width constraint on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_width(handle: i64, width: f64) {
    widgets::set_width(handle, width);
}

/// Set content hugging priority: high (≥249) → resist hexpand; low → allow hexpand.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_hugging(handle: i64, priority: f64) {
    widgets::set_hugging_priority(handle, priority);
}

/// Set edge insets (padding) on a widget.
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

/// Get the current text content of a TextField.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_get_string(handle: i64) -> i64 {
    widgets::textfield::get_string_value(handle) as i64
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_next_key_view(_handle: i64, _next_handle: i64) {
    // GTK4 handles tab navigation automatically via the widget tree
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

/// Make a widget expand to fill its parent's width.
#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_width(handle: i64) {
    widgets::match_parent_width(handle);
}

/// Make a widget expand to fill its parent's height.
#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_height(handle: i64) {
    widgets::match_parent_height(handle);
}

/// Set a fixed height constraint on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_height(handle: i64, height: f64) {
    widgets::set_height(handle, height);
}

/// Set distribution on a stack (GtkBox).
#[no_mangle]
pub extern "C" fn perry_ui_stack_set_distribution(handle: i64, distribution: f64) {
    widgets::set_distribution(handle, distribution as i64);
}

/// Set alignment on a stack (GtkBox).
/// macOS NSLayoutAttribute values: Leading=5, CenterX=9, Width=7, Top=3, CenterY=12, Bottom=4.
#[no_mangle]
pub extern "C" fn perry_ui_stack_set_alignment(handle: i64, alignment: f64) {
    widgets::set_alignment(handle, alignment as i64);
}

/// GTK4 already excludes non-visible children from layout — this is a no-op stub.
#[no_mangle]
pub extern "C" fn perry_ui_stack_set_detaches_hidden(handle: i64, flag: i64) {
    widgets::set_detaches_hidden(handle, flag != 0);
}

/// Set the application icon.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_icon(path_ptr: i64) {
    let path = crate::widgets::image::str_from_header(path_ptr as *const u8);
    if path.is_empty() {
        return;
    }

    // Resolve path: try relative to executable, then relative to cwd
    let resolved = resolve_asset_path(path);
    if !resolved.exists() {
        return;
    }

    // In GTK4, window icons are set via the icon theme.
    // Add the icon's parent directory to the theme search path.
    if let Some(display) = gtk4::gdk::Display::default() {
        let theme = gtk4::IconTheme::for_display(&display);
        if let Some(parent) = resolved.parent() {
            theme.add_search_path(parent);
        }
        if let Some(stem) = resolved.file_stem().and_then(|s| s.to_str()) {
            gtk4::Window::set_default_icon_name(stem);
        }
    }
}

/// Resolve an asset path relative to the executable directory.
fn resolve_asset_path(path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if p.is_absolute() && p.exists() {
        return p.to_path_buf();
    }
    // Try relative to executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(path);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    // Fallback to the path as-is (relative to cwd)
    p.to_path_buf()
}

/// Create a VStack with custom insets.
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

/// Create an HStack with custom insets.
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
// Navigation
// =============================================================================

/// Create a NavigationStack with initial page.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_create() -> i64 {
    // Matches the 0-arg dispatch in perry-dispatch::PERRY_UI_TABLE.
    widgets::navstack::create(std::ptr::null(), 0)
}

/// Push a page onto the navigation stack.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_push(handle: i64, title_ptr: i64, body_handle: i64) {
    widgets::navstack::push(handle, title_ptr as *const u8, body_handle);
}

/// Pop the top page from the navigation stack.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_pop(handle: i64) {
    widgets::navstack::pop(handle);
}

// =============================================================================
// Picker
// =============================================================================

/// Create a Picker (dropdown).
#[no_mangle]
pub extern "C" fn perry_ui_picker_create(label_ptr: i64, on_change: f64, style: i64) -> i64 {
    widgets::picker::create(label_ptr as *const u8, on_change, style)
}

/// Add an item to a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_add_item(handle: i64, title_ptr: i64) {
    widgets::picker::add_item(handle, title_ptr as *const u8);
}

/// Set the selected item of a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_set_selected(handle: i64, index: i64) {
    widgets::picker::set_selected(handle, index);
}

/// Get the selected item of a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_get_selected(handle: i64) -> i64 {
    widgets::picker::get_selected(handle)
}

// =============================================================================
// Image
// =============================================================================

/// Create an image from a file path.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_file(path_ptr: i64) -> i64 {
    widgets::image::create_file(path_ptr as *const u8)
}

/// Create an image from a named icon/symbol.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_symbol(name_ptr: i64) -> i64 {
    widgets::image::create_symbol(name_ptr as *const u8)
}

/// Set the size of an image.
#[no_mangle]
pub extern "C" fn perry_ui_image_set_size(handle: i64, width: f64, height: f64) {
    widgets::image::set_size(handle, width, height);
}

/// Set the tint color of an image.
#[no_mangle]
pub extern "C" fn perry_ui_image_set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::image::set_tint(handle, r, g, b, a);
}

// =============================================================================
// Sheet
// =============================================================================

/// Create a sheet (modal window).
#[no_mangle]
pub extern "C" fn perry_ui_sheet_create(width: f64, height: f64, title_val: f64) -> i64 {
    sheet::create(width, height, title_val)
}

/// Present (show) a sheet.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_present(sheet_handle: i64) {
    sheet::present(sheet_handle);
}

/// Dismiss (close) a sheet.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_dismiss(sheet_handle: i64) {
    sheet::dismiss(sheet_handle);
}

// =============================================================================
// Toolbar
// =============================================================================

/// Create a toolbar.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_create() -> i64 {
    toolbar::create()
}

/// Add an item to a toolbar.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_add_item(
    toolbar_handle: i64,
    label_ptr: i64,
    icon_ptr: i64,
    callback: f64,
) {
    toolbar::add_item(
        toolbar_handle,
        label_ptr as *const u8,
        icon_ptr as *const u8,
        callback,
    );
}

/// Attach a toolbar to the current window.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_attach(toolbar_handle: i64) {
    toolbar::attach(toolbar_handle);
}

// =============================================================================
// System API
// =============================================================================

/// Open a URL in the default browser.
#[no_mangle]
pub extern "C" fn perry_system_open_url(url_ptr: i64) {
    system::open_url(url_ptr as *const u8);
}

/// Check if dark mode is enabled.
#[no_mangle]
pub extern "C" fn perry_system_is_dark_mode() -> i64 {
    system::is_dark_mode()
}

/// Set a preference value.
#[no_mangle]
pub extern "C" fn perry_system_preferences_set(key_ptr: i64, value: f64) {
    system::preferences_set(key_ptr as *const u8, value);
}

/// Get a preference value.
#[no_mangle]
pub extern "C" fn perry_system_preferences_get(key_ptr: i64) -> f64 {
    system::preferences_get(key_ptr as *const u8)
}

/// Save a value to the keychain.
#[no_mangle]
pub extern "C" fn perry_system_keychain_save(key_ptr: i64, value_ptr: i64) {
    keychain::save(key_ptr as *const u8, value_ptr as *const u8);
}

/// Get a value from the keychain.
#[no_mangle]
pub extern "C" fn perry_system_keychain_get(key_ptr: i64) -> f64 {
    keychain::get(key_ptr as *const u8)
}

/// Delete a value from the keychain.
#[no_mangle]
pub extern "C" fn perry_system_keychain_delete(key_ptr: i64) {
    keychain::delete(key_ptr as *const u8);
}

/// Send a desktop notification.
#[no_mangle]
pub extern "C" fn perry_system_notification_send(title_ptr: i64, body_ptr: i64) {
    system::notification_send(title_ptr as *const u8, body_ptr as *const u8);
}

/// Stub: GTK4 has no remote-push pipeline. Symbol exists so TS code that
/// calls `notificationRegisterRemote` links and runs without crashing — the
/// callback simply never fires.
#[no_mangle]
pub extern "C" fn perry_system_notification_register_remote(_callback: f64) {}

/// Stub: see `perry_system_notification_register_remote` above.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_receive(_callback: f64) {}

/// Stub (#98): GTK4 has no equivalent of FCM/APNs background delivery; the
/// symbol exists so cross-platform user code linking against perry-ui-gtk4
/// resolves cleanly. Callback is silently dropped.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_background_receive(_callback: f64) {}

/// Stub: GTK4 has no scheduled-notification pipeline; GLib timer + glib
/// notification re-emit would be best-effort and is out of scope for #96.
#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_interval(
    _id_ptr: i64,
    _title_ptr: i64,
    _body_ptr: i64,
    _seconds: f64,
    _repeats: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_calendar(
    _id_ptr: i64,
    _title_ptr: i64,
    _body_ptr: i64,
    _timestamp_ms: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_location(
    _id_ptr: i64,
    _title_ptr: i64,
    _body_ptr: i64,
    _lat: f64,
    _lon: f64,
    _radius: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_system_notification_cancel(_id_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_system_notification_on_tap(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_system_get_locale() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    }
    let lang = std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LANGUAGE"))
        .unwrap_or_else(|_| "en".to_string());
    // Extract language code: "de_DE.UTF-8" -> "de"
    let code = if lang.len() >= 2 { &lang[..2] } else { "en" };
    unsafe { js_string_from_bytes(code.as_ptr(), code.len() as i64) as i64 }
}

// =============================================================================
// Weather App Extensions
// =============================================================================

/// Request location via IP geolocation (async, calls back on main thread).
#[no_mangle]
pub extern "C" fn perry_system_request_location(callback: f64) {
    location::request_location(callback);
}

// =============================================================================
// Platform Detection — __wrapper_perry_* required by Perry codegen
//
// Perry codegen emits calls to __wrapper_perry_X for every `declare function
// perry_X(...)` in TypeScript. These wrappers follow Perry's calling convention:
//   first param: i64 closure_ptr (ignored for FFI wrappers)
//   remaining params: f64 (NaN-boxed)
//   return: f64 (NaN-boxed string, plain f64, or TAG_UNDEFINED)
// =============================================================================

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

/// TAG_UNDEFINED — returned by void-returning wrappers.
const TAG_UNDEFINED: f64 = unsafe { std::mem::transmute(0x7FFC_0000_0000_0001_u64) };

fn nanbox_static_str(s: &'static [u8]) -> f64 {
    let ptr = unsafe { js_string_from_bytes(s.as_ptr(), s.len() as i64) };
    unsafe { js_nanbox_string(ptr as i64) }
}

/// perry_get_platform() → "linux"
#[no_mangle]
pub extern "C" fn __wrapper_perry_get_platform(_closure_ptr: i64) -> f64 {
    nanbox_static_str(b"linux")
}

/// perry_get_screen_width() → 1920 (desktop default; layout mode computed once at startup)
#[no_mangle]
pub extern "C" fn __wrapper_perry_get_screen_width(_closure_ptr: i64) -> f64 {
    1920.0
}

/// perry_get_screen_height() → 1080 (desktop default)
#[no_mangle]
pub extern "C" fn __wrapper_perry_get_screen_height(_closure_ptr: i64) -> f64 {
    1080.0
}

/// perry_get_scale_factor() → 1.0 (non-HiDPI default)
#[no_mangle]
pub extern "C" fn __wrapper_perry_get_scale_factor(_closure_ptr: i64) -> f64 {
    1.0
}

/// perry_get_orientation() → "landscape" (desktop is always landscape)
#[no_mangle]
pub extern "C" fn __wrapper_perry_get_orientation(_closure_ptr: i64) -> f64 {
    nanbox_static_str(b"landscape")
}

/// perry_has_hardware_keyboard() → true (desktop always has a keyboard)
#[no_mangle]
pub extern "C" fn __wrapper_perry_has_hardware_keyboard(_closure_ptr: i64) -> f64 {
    1.0
}

thread_local! {
    static RESIZE_CALLBACK: std::cell::RefCell<Option<f64>> = std::cell::RefCell::new(None);
}

/// perry_on_resize(callback) — store callback; called with (width, height) on resize.
#[no_mangle]
pub extern "C" fn __wrapper_perry_on_resize(_closure_ptr: i64, callback: f64) -> f64 {
    RESIZE_CALLBACK.with(|rc| {
        *rc.borrow_mut() = Some(callback);
    });
    TAG_UNDEFINED
}

/// perry_on_orientation_change(callback) — no-op on desktop (orientation never changes).
#[no_mangle]
pub extern "C" fn __wrapper_perry_on_orientation_change(_closure_ptr: i64, _callback: f64) -> f64 {
    TAG_UNDEFINED
}

/// perry_ui_poll_open_file() -> i64 — stub for Linux (macOS "Open With" not applicable).
/// Returns an empty string pointer; the IDE's checkOpenFileRequests() polls this every 500ms.
#[no_mangle]
pub extern "C" fn perry_ui_poll_open_file() -> i64 {
    unsafe { js_string_from_bytes(std::ptr::null(), 0) as i64 }
}

/// perry_get_device_idiom() → 0 — Linux is always a desktop (not phone or pad).
/// Called by iOS-specific branches in platform.ts that are dead code on Linux;
/// the symbol must exist for the linker even though it is never called at runtime.
#[no_mangle]
pub extern "C" fn perry_get_device_idiom(_closure_ptr: i64) -> f64 {
    0.0 // 0 = phone-like; value is irrelevant on Linux (dead code branch)
}

// Audio capture (PulseAudio simple API)
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

/// hone_get_documents_dir() — iOS sandbox documents dir stub.
/// Returns empty string; only reachable on iOS (__platform__ === 1), which is dead code on Linux.
#[no_mangle]
pub extern "C" fn __wrapper_hone_get_documents_dir(_closure_ptr: i64) -> f64 {
    nanbox_static_str(b"")
}

/// hone_get_app_files_dir() — Android app files dir stub.
/// Returns empty string; only reachable on Android (__platform__ === 2), dead code on Linux.
#[no_mangle]
pub extern "C" fn __wrapper_hone_get_app_files_dir(_closure_ptr: i64) -> f64 {
    nanbox_static_str(b"")
}

// --- Camera stubs (issue #191) ---
// Real implementations live in `perry-ui-ios` and `perry-ui-android`. On
// Linux, integrating GStreamer / V4L2 for live preview is a separate scope;
// these no-ops let user code that targets multiple platforms link cleanly.

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

// --- Cross-platform toast + reactive setText (Phase 2 v3.3) ---

/// Show a brief slide-down toast banner on the main window.
/// msg_ptr is a raw StringHeader pointer (NaN-boxed string, unboxed to i64 by codegen).
#[no_mangle]
pub extern "C" fn perry_ui_show_toast(msg_ptr: i64) {
    let msg = app::str_from_header(msg_ptr as *const u8);
    widgets::toast::show_toast(msg);
}

/// Create a Text (GtkLabel) widget and register it under a string id so that
/// perry_ui_set_text can update it imperatively later.
/// Returns the widget handle (1-based i64, NaN-boxed by codegen).
#[no_mangle]
pub extern "C" fn perry_ui_text_create_with_id(text_ptr: i64, id_ptr: i64) -> i64 {
    let handle = widgets::text::create(text_ptr as *const u8);
    let id = app::str_from_header(id_ptr as *const u8);
    widgets::text_registry::register(id, handle);
    handle
}

/// Update the label of a Text widget previously registered via
/// perry_ui_text_create_with_id.
#[no_mangle]
pub extern "C" fn perry_ui_set_text(id_ptr: i64, value_ptr: i64) {
    let id = app::str_from_header(id_ptr as *const u8);
    let value = app::str_from_header(value_ptr as *const u8);
    widgets::text_registry::set_text_for_id(id, value);
}
