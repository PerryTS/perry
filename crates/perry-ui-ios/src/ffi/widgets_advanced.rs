//! FFI exports: timer/canvas, new widgets (SecureField/ProgressView/Image/Picker/Form/NavStack/ZStack), cross-cutting (enabled/hover/tooltip)
//!
//! Extracted from `lib.rs` for file-size hygiene. No behavior changes.

use crate::*;

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
pub extern "C" fn perry_ui_canvas_set_fill_color(h: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::canvas::set_fill_color(h, r, g, b, a);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_stroke_color(h: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::canvas::set_stroke_color(h, r, g, b, a);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_set_line_width(h: i64, w: f64) {
    widgets::canvas::set_line_width(h, w);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_rect(h: i64, x: f64, y: f64, w: f64, ht: f64) {
    widgets::canvas::fill_rect(h, x, y, w, ht);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke_rect(h: i64, x: f64, y: f64, w: f64, ht: f64) {
    widgets::canvas::stroke_rect(h, x, y, w, ht);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_clear_rect(h: i64, _x: f64, _y: f64, _w: f64, _ht: f64) {
    widgets::canvas::clear(h);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_arc(_h: i64, _x: f64, _y: f64, _r: f64, _sa: f64, _ea: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_close_path(h: i64) {
    widgets::canvas::close_path(h);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill(h: i64) {
    widgets::canvas::fill(h);
}
#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke_path(h: i64) {
    widgets::canvas::stroke_path(h);
}
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

/// Continuous pointer events (issue #1868). Callback receives a
/// PointerEvent { x, y, button, pointerType: "touch" } object.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_mouse_down(handle: i64, callback: f64) {
    crate::pointer::set_on_mouse_down(handle, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_mouse_up(handle: i64, callback: f64) {
    crate::pointer::set_on_mouse_up(handle, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_mouse_move(handle: i64, callback: f64) {
    crate::pointer::set_on_mouse_move(handle, callback);
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
#[no_mangle]
pub extern "C" fn perry_ui_canvas_draw_image(
    h: i64,
    image: i64,
    sx: f64,
    sy: f64,
    sw: f64,
    sh: f64,
    dx: f64,
    dy: f64,
    dw: f64,
    dh: f64,
) {
    widgets::canvas::draw_image(h, image, sx, sy, sw, sh, dx, dy, dw, dh);
}

/// Load an image asset for Canvas.drawImage. Native backends expose the FFI
/// symbol now; platform decoding/drawing support can fill this handle in.
#[no_mangle]
pub extern "C" fn perry_ui_load_image(url_ptr: i64) -> i64 {
    widgets::canvas::load_image(url_ptr as *const u8)
}
