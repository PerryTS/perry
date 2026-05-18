pub mod app;
pub mod audio;
pub mod clipboard;
pub mod deeplinks_stub;
pub mod dialog;
pub mod file_dialog;
pub mod issue_552_stub;
pub mod keychain;
pub mod location;
pub mod media_playback;
pub mod menu;
pub mod network_stub;
pub mod sheet;
pub mod state;
pub mod system;
pub mod toolbar;
pub mod widgets;
pub mod window;

pub mod screenshot;

// Tray icon (issue #490). The body uses `ksni` (which transitively
// pulls `zbus` + `tokio`) — all gated to `cfg(target_os = "linux")` in
// Cargo.toml (mirrors `media_playback`'s mpris module). The FFI
// exports themselves stay unconditional below so the link surface is
// stable on macOS / Windows hosts that build the gtk4 crate without a
// real GTK FFI wired up.
#[cfg(target_os = "linux")]
pub mod tray;

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

/// Issue #707 — cap visible lines on a Text widget (GtkLabel.set_lines).
#[no_mangle]
pub extern "C" fn perry_ui_text_set_number_of_lines(handle: i64, lines: i64) {
    widgets::text::set_number_of_lines(handle, lines);
}
/// Issue #707 — truncation mode (GtkLabel.set_ellipsize).
#[no_mangle]
pub extern "C" fn perry_ui_text_set_truncation_mode(handle: i64, mode: i64) {
    widgets::text::set_truncation_mode(handle, mode);
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

/// Rich tooltip (issue #479) — real GTK4 impl via GtkPopover anchored
/// on the host widget, shown after `hover_delay_ms` of hover.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_rich_tooltip(
    handle: i64,
    content_handle: i64,
    hover_delay_ms: f64,
) {
    let delay = if hover_delay_ms.is_nan() || hover_delay_ms < 0.0 {
        500
    } else {
        hover_delay_ms as u32
    };
    widgets::rich_tooltip::set_rich_tooltip(handle, content_handle, delay);
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

// =============================================================================
// Tray icon (issue #490) — KSNI / StatusNotifierItem on Linux.
// FFI symbols stay defined unconditionally so the link surface is
// stable; only the body branches on cfg.
// =============================================================================

/// Create a tray icon with an initial PNG path. Returns 0 on systems
/// without StatusNotifierItem support.
#[no_mangle]
pub extern "C" fn perry_ui_tray_create(icon_path_ptr: i64) -> i64 {
    #[cfg(target_os = "linux")]
    {
        return tray::create(icon_path_ptr as *const u8);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = icon_path_ptr;
        eprintln!(
            "[perry] warning: tray icons require Linux + StatusNotifierItem \
            (KDE / GNOME-with-extension / XFCE) — gtk4 build on this host \
            doesn't support them (#490)"
        );
        0
    }
}

/// Hot-update the tray icon (no service re-creation).
#[no_mangle]
pub extern "C" fn perry_ui_tray_set_icon(tray_handle: i64, icon_path_ptr: i64) {
    #[cfg(target_os = "linux")]
    {
        tray::set_icon(tray_handle, icon_path_ptr as *const u8);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (tray_handle, icon_path_ptr);
    }
}

/// Hot-update the tray tooltip.
#[no_mangle]
pub extern "C" fn perry_ui_tray_set_tooltip(tray_handle: i64, tooltip_ptr: i64) {
    #[cfg(target_os = "linux")]
    {
        tray::set_tooltip(tray_handle, tooltip_ptr as *const u8);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (tray_handle, tooltip_ptr);
    }
}

/// Attach a menu (handle from `menuCreate` / `menuAddItem` / etc.) so
/// right-click pops it up. Re-uses the existing menu storage in
/// `menu.rs` rather than building a parallel system.
#[no_mangle]
pub extern "C" fn perry_ui_tray_attach_menu(tray_handle: i64, menu_handle: i64) {
    #[cfg(target_os = "linux")]
    {
        tray::attach_menu(tray_handle, menu_handle);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (tray_handle, menu_handle);
    }
}

/// Register a JS click callback (NaN-boxed closure pointer). Fires on
/// SNI's primary `Activate` action — usually left-click on the tray
/// icon. Right-click pops the attached menu instead.
#[no_mangle]
pub extern "C" fn perry_ui_tray_on_click(tray_handle: i64, callback: f64) {
    #[cfg(target_os = "linux")]
    {
        tray::on_click(tray_handle, callback);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (tray_handle, callback);
    }
}

/// Destroy the tray icon. Subsequent calls on the same handle no-op.
#[no_mangle]
pub extern "C" fn perry_ui_tray_destroy(tray_handle: i64) {
    #[cfg(target_os = "linux")]
    {
        tray::destroy(tray_handle);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = tray_handle;
    }
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
// Issue #478 — Rich text editor — real GTK4 impl via GtkTextView + tags.
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

// Issue #516 — PdfView stubs. Linux — Poppler (libpoppler-glib) is a
// future iteration.
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_create(_w: f64, _h: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_load_file(_h: i64, _p: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_get_page_count(_h: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_go_to_page(_h: i64, _i: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_get_current_page(_h: i64) -> i64 {
    -1
}
#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_set_scale(_h: i64, _s: f64) {}

// Issue #517 — MapView via libshumate (GTK4-native vector tile widget).
#[no_mangle]
pub extern "C" fn perry_ui_map_view_create(w: f64, h: f64) -> i64 {
    widgets::map_view::create(w, h)
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_region(
    h: i64,
    lat: f64,
    lon: f64,
    lat_span: f64,
    lon_span: f64,
) {
    widgets::map_view::set_region(h, lat, lon, lat_span, lon_span);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_add_pin(h: i64, lat: f64, lon: f64, title_ptr: i64) {
    widgets::map_view::add_pin(h, lat, lon, title_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_clear_pins(h: i64) {
    widgets::map_view::clear_pins(h);
}
#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_map_type(h: i64, style: i64) {
    widgets::map_view::set_map_type(h, style);
}

// Issue #477 — Command palette — real GTK4 impl via floating GtkWindow.
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_register(id: i64, l: i64, s: i64, cb: f64) {
    widgets::command_palette::register(id as *const u8, l as *const u8, s as *const u8, cb)
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_unregister(id: i64) {
    widgets::command_palette::unregister(id as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_clear() {
    widgets::command_palette::clear()
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_show() {
    widgets::command_palette::show()
}
#[no_mangle]
pub extern "C" fn perry_ui_command_palette_hide() {
    widgets::command_palette::hide()
}

// Issue #474 — Chart widget — real GTK4 impl via Cairo on GtkDrawingArea.
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

// Issue #481 — Calendar widget — real impl on GTK4 via GtkCalendar.
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

/// TreeView (#480) — real GTK4 impl via GtkTreeView + GtkTreeStore.
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_create(id_ptr: i64, label_ptr: i64) -> i64 {
    widgets::tree_view::node_create(id_ptr as *const u8, label_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_add_child(parent: i64, child: i64) {
    widgets::tree_view::node_add_child(parent, child)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_create(root: i64, on_select: f64) -> i64 {
    widgets::tree_view::create(root, on_select)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_expand_all(handle: i64) {
    widgets::tree_view::expand_all(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_collapse_all(handle: i64) {
    widgets::tree_view::collapse_all(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_get_selected_id(handle: i64) -> f64 {
    widgets::tree_view::get_selected_id(handle)
}

/// Combobox (#475) — real GTK4 impl via `GtkEntry` + `EntryCompletion`.
#[no_mangle]
pub extern "C" fn perry_ui_combobox_create(initial_ptr: i64, on_change: f64) -> i64 {
    widgets::combobox::create(initial_ptr as *const u8, on_change)
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_add_item(handle: i64, value_ptr: i64) {
    widgets::combobox::add_item(handle, value_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_combobox_set_value(handle: i64, value_ptr: i64) {
    widgets::combobox::set_value(handle, value_ptr as *const u8)
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

/// #635 stub: remote URL images aren't fetched on GTK4 yet —
/// register an empty image widget so layout still works.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_url(_url_ptr: i64, _alt_ptr: i64) -> i64 {
    widgets::image::create_symbol(0 as *const u8)
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
/// Bug-report-flow utility: stable OS-version string. GTK4/Linux
/// stub — native impl would read `/etc/os-release` or
/// `uname -r`.
#[no_mangle]
pub extern "C" fn perry_system_get_os_version() -> i64 {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_get_os_version",
        "Linux getOSVersion (/etc/os-release) not yet implemented",
        None,
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
// Issue #553 — Real GTK4 implementations.
//
// BottomNavigation: GtkBox+GtkButton tab strip with GtkImage icon + GtkLabel
// (Adwaita CSS classes for selected styling).
//
// ImageGallery: GtkScrolledWindow + horizontal GtkBox of fixed-size GtkPicture
// pages; index-tracking via GtkAdjustment::value-changed.
//
// onScrollEnd: GtkAdjustment::value-changed with backpressure.
//
// Pull-to-refresh stays no-op on GTK4 (no native idiom). LazyVStack
// scroll-end is a no-op too — current GTK4 LazyVStack is essentially a
// fully-realized GtkBox; the ScrollView path is the one production apps
// reach for.
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_create(on_select: f64) -> i64 {
    widgets::bottom_nav::create(on_select)
}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_add_item(handle: i64, icon_ptr: i64, label_ptr: i64) {
    widgets::bottom_nav::add_item(handle, icon_ptr as *const u8, label_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_badge(handle: i64, index: i64, badge_ptr: i64) {
    widgets::bottom_nav::set_badge(handle, index, badge_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_selected(handle: i64, index: i64) {
    widgets::bottom_nav::set_selected(handle, index)
}

/// Issue #706 — GTK4 bottom-nav active-tab tint via Pango AttrColor on
/// the per-item label.
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::bottom_nav::set_tint_color(handle, r, g, b, a);
}

/// Issue #706 — GTK4 bottom-nav inactive-tabs tint.
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
    widgets::scrollview::set_scroll_end_callback(handle, callback, threshold_px)
}

#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_create(on_index_change: f64) -> i64 {
    widgets::image_gallery::create(on_index_change)
}
#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_add_image(handle: i64, url_ptr: i64, alt_ptr: i64) {
    widgets::image_gallery::add_image(handle, url_ptr as *const u8, alt_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_set_index(handle: i64, index: i64) {
    widgets::image_gallery::set_index(handle, index)
}

// =============================================================================
// FFI parity stubs / impls — symbols that exist on macOS / Windows / Android /
// iOS so codegen-emitted programs link uniformly. Without these, a Linux user
// who calls `Button({image: "..."})`, `TextField({onSubmit, onFocus})`, `TabBar`,
// `QRCode`, `FrameSplit`, etc. would hit `Undefined symbols: ...` at link time.
// Mirrors the macOS pattern of stubbing iOS-only widgets (tabbar / vbox /
// frame_split / scrollview pull-to-refresh) for link stability.
// =============================================================================

/// Set an icon on a button (e.g. `Button({label, image})`). GTK4: maps the icon
/// name to GtkButton::set_icon_name (icon-naming-spec / SF-Symbols-style names).
#[no_mangle]
pub extern "C" fn perry_ui_button_set_image(handle: i64, name_ptr: i64) {
    widgets::button::set_image(handle, name_ptr as *const u8);
}

/// Wire onSubmit (Enter key → callback with current text). Real GTK4 impl.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_submit(handle: i64, on_submit: f64) {
    widgets::textfield::set_on_submit(handle, on_submit);
}

/// Wire onFocus (gain keyboard focus → callback). Real GTK4 impl via
/// EventControllerFocus.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_focus(handle: i64, on_focus: f64) {
    widgets::textfield::set_on_focus(handle, on_focus);
}

/// Drop focus from every registered text field (mirrors macOS `blurAll()`).
#[no_mangle]
pub extern "C" fn perry_ui_textfield_blur_all() {
    widgets::textfield::blur_all();
}

/// QRCode widget — stub on GTK4 (mirrors macOS pre-real-impl shape on iOS-only
/// widgets). Returns 0 for "not supported"; perry's widget chain handles a 0
/// handle gracefully. Real impl pending qrcodegen + GdkPixbuf wiring.
#[no_mangle]
pub extern "C" fn perry_ui_qrcode_create(_data_ptr: i64, _size: f64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_set_data(_handle: i64, _data_ptr: i64) {}

/// TabBar — iOS-shaped widget (UITabBarController equivalent). Stub on
/// desktop; matches macOS / Windows shape exactly. A real GTK4 impl would
/// use GtkStackSwitcher; tracked as a follow-up.
#[no_mangle]
pub extern "C" fn perry_ui_tabbar_create(_on_change: f64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_tabbar_add_tab(_handle: i64, _label_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_tabbar_set_selected(_handle: i64, _index: i64) {}

/// ScrollView pull-to-refresh — iOS / Android idiom, no native GTK4 equivalent.
/// Stub mirrors macOS (which also stubs because AppKit has no pull-to-refresh).
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_refresh_control(_handle: i64, _callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_end_refreshing(_handle: i64) {}

/// VBox / FrameSplit — iOS-internal helpers (UIStackView vertical / split-view
/// layout primitive). Other desktops stub these and route real layout through
/// VStack / SplitView. Same here.
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

// --- WebView (issue #658) — stub on this platform; link-stability shape
//     matching the macOS / iOS / visionOS surface. v1 returns a 0 handle
//     and the imperative ops are no-ops; user code that imports WebView
//     still compiles and runs but the widget is invisible. Real backend
//     deferred to a later phase per #658's roadmap.
// --- WebView (issue #658 Phase 4) — real Linux backend via WebKitGTK 6.0.
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

// AttributedText (Issue #710) — GTK4 GtkLabel + Pango AttrList.
#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_create() -> i64 {
    widgets::attributed_text::create()
}
#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_append(
    h: i64,
    t: i64,
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
        h,
        t as *const u8,
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
pub extern "C" fn perry_ui_attributed_text_clear(h: i64) {
    widgets::attributed_text::clear(h);
}

// ---- In-app screen capture (issue #918) ----
/// Capture the active window as a PNG and return a base64-encoded string.
/// Returns an empty string if no window is available or capture fails.
#[no_mangle]
pub extern "C" fn perry_system_take_screenshot() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
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
        js_string_from_bytes(encoded.as_ptr(), encoded.len() as i64) as i64
    }
}
