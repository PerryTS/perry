use crate::*;

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

/// Create a Combobox (NSComboBox: editable filterable text field +
/// dropdown). `initial_ptr` is the starting text (may be null/empty);
/// `on_change` fires with the current string value when the user picks
/// from the dropdown or commits free text via Return. See issue #475.
#[no_mangle]
pub extern "C" fn perry_ui_combobox_create(initial_ptr: i64, on_change: f64) -> i64 {
    widgets::combobox::create(initial_ptr as *const u8, on_change)
}

/// Append a suggestion item to a Combobox dropdown.
#[no_mangle]
pub extern "C" fn perry_ui_combobox_add_item(handle: i64, value_ptr: i64) {
    widgets::combobox::add_item(handle, value_ptr as *const u8);
}

/// Replace the editable text content of a Combobox.
#[no_mangle]
pub extern "C" fn perry_ui_combobox_set_value(handle: i64, value_ptr: i64) {
    widgets::combobox::set_value(handle, value_ptr as *const u8);
}

/// Get the current editable text content of a Combobox as a NaN-boxed
/// string (STRING_TAG-tagged f64).
#[no_mangle]
pub extern "C" fn perry_ui_combobox_get_value(handle: i64) -> f64 {
    widgets::combobox::get_value(handle)
}

// ---- Rich text editor (issue #478) ----

#[no_mangle]
pub extern "C" fn perry_ui_rich_text_create(width: f64, height: f64, on_change: f64) -> i64 {
    widgets::rich_text::create(width, height, on_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_rich_text_set_string(handle: i64, text_ptr: i64) {
    widgets::rich_text::set_string(handle, text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_rich_text_get_string(handle: i64) -> f64 {
    widgets::rich_text::get_string(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_rich_text_set_html(handle: i64, html_ptr: i64) -> i64 {
    widgets::rich_text::set_html(handle, html_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_ui_rich_text_get_html(handle: i64) -> f64 {
    widgets::rich_text::get_html(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_bold(handle: i64) {
    widgets::rich_text::toggle_bold(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_italic(handle: i64) {
    widgets::rich_text::toggle_italic(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_rich_text_toggle_underline(handle: i64) {
    widgets::rich_text::toggle_underline(handle);
}

// ---- PdfView (issue #516) ----

#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_create(width: f64, height: f64) -> i64 {
    widgets::pdf_view::create(width, height)
}

#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_load_file(handle: i64, path_ptr: i64) -> i64 {
    if widgets::pdf_view::load_file(handle, path_ptr as *const u8) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_get_page_count(handle: i64) -> i64 {
    widgets::pdf_view::get_page_count(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_go_to_page(handle: i64, page_index: i64) {
    widgets::pdf_view::go_to_page(handle, page_index);
}

#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_get_current_page(handle: i64) -> i64 {
    widgets::pdf_view::get_current_page(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_pdf_view_set_scale(handle: i64, scale: f64) {
    widgets::pdf_view::set_scale(handle, scale);
}

// ---- MapView (issue #517) ----

#[no_mangle]
pub extern "C" fn perry_ui_map_view_create(width: f64, height: f64) -> i64 {
    widgets::map_view::create(width, height)
}

#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_region(
    handle: i64,
    lat: f64,
    lon: f64,
    lat_span: f64,
    lon_span: f64,
) {
    widgets::map_view::set_region(handle, lat, lon, lat_span, lon_span);
}

#[no_mangle]
pub extern "C" fn perry_ui_map_view_add_pin(handle: i64, lat: f64, lon: f64, title_ptr: i64) {
    widgets::map_view::add_pin(handle, lat, lon, title_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_map_view_clear_pins(handle: i64) {
    widgets::map_view::clear_pins(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_map_view_set_map_type(handle: i64, style: i64) {
    widgets::map_view::set_map_type(handle, style);
}

// ---- Command palette (issue #477) ----

#[no_mangle]
pub extern "C" fn perry_ui_command_palette_register(
    id_ptr: i64,
    label_ptr: i64,
    subtitle_ptr: i64,
    on_run: f64,
) {
    widgets::command_palette::register(
        id_ptr as *const u8,
        label_ptr as *const u8,
        subtitle_ptr as *const u8,
        on_run,
    );
}

#[no_mangle]
pub extern "C" fn perry_ui_command_palette_unregister(id_ptr: i64) {
    widgets::command_palette::unregister(id_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_command_palette_clear() {
    widgets::command_palette::clear();
}

#[no_mangle]
pub extern "C" fn perry_ui_command_palette_show() {
    widgets::command_palette::show();
}

#[no_mangle]
pub extern "C" fn perry_ui_command_palette_hide() {
    widgets::command_palette::hide();
}

// ---- Chart (issue #474) ----

/// Create a Chart widget. `kind` is 0=line, 1=bar, 2=pie.
#[no_mangle]
pub extern "C" fn perry_ui_chart_create(kind: i64, width: f64, height: f64) -> i64 {
    widgets::chart::create(kind, width, height)
}

#[no_mangle]
pub extern "C" fn perry_ui_chart_add_data_point(handle: i64, label_ptr: i64, value: f64) {
    widgets::chart::add_data_point(handle, label_ptr as *const u8, value);
}

#[no_mangle]
pub extern "C" fn perry_ui_chart_clear_data(handle: i64) {
    widgets::chart::clear_data(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_chart_set_title(handle: i64, title_ptr: i64) {
    widgets::chart::set_title(handle, title_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_chart_reload(handle: i64) {
    widgets::chart::reload(handle);
}

// ---- Calendar (issue #481) ----

/// Create an `NSDatePicker` calendar in graphical month-grid mode.
/// `year` and `month` are 1-based; pass <=0 / out-of-range to default
/// to 2026-01. `on_change` fires with the selected date as `yyyy-MM-dd`.
#[no_mangle]
pub extern "C" fn perry_ui_calendar_create(year: i64, month: i64, on_change: f64) -> i64 {
    widgets::calendar::create(year, month, on_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_calendar_set_date(handle: i64, year: i64, month: i64, day: i64) {
    widgets::calendar::set_date(handle, year, month, day);
}

#[no_mangle]
pub extern "C" fn perry_ui_calendar_get_selected_date(handle: i64) -> f64 {
    widgets::calendar::get_selected_date(handle)
}

// ---- TreeView (issue #480) ----

/// Register a tree node with `id` and `label`. Standalone — wire into
/// a topology via `treeNodeAddChild`, then mount via `TreeView`.
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_create(id_ptr: i64, label_ptr: i64) -> i64 {
    widgets::tree_view::node_create(id_ptr as *const u8, label_ptr as *const u8)
}

/// Append `child` as the last child of `parent`.
#[no_mangle]
pub extern "C" fn perry_ui_tree_node_add_child(parent: i64, child: i64) {
    widgets::tree_view::node_add_child(parent, child);
}

/// Mount the `root_node` topology in an `NSOutlineView`. `on_select`
/// is invoked with the picked node's `id` when selection changes.
#[no_mangle]
pub extern "C" fn perry_ui_tree_view_create(root_node: i64, on_select: f64) -> i64 {
    widgets::tree_view::create(root_node, on_select)
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

/// Set a rich tooltip on a widget. The tooltip body is `content_handle`
/// (any pre-built widget tree) and is presented in a borderless NSPanel
/// after `hover_delay_ms` of mouse hover. See issue #479.
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
