// FFI: widget construction (Text, Button, stacks, TextField, Toggle, Slider, etc.).
use crate::widgets;

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
