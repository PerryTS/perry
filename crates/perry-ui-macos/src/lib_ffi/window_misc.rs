use crate::ffi::js_string_from_bytes;
use crate::*;

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

// ---- Issue #473 — sort + filter + multi-select extensions ----

/// Register a `(colIndex, ascending) => void` callback that fires when
/// the user clicks a column header. Installing the callback also turns
/// on per-column sort indicators.
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_sort_change(handle: i64, callback: f64) {
    widgets::table::set_on_sort_change(handle, callback)
}

#[no_mangle]
pub extern "C" fn perry_ui_table_set_allows_multiple_selection(handle: i64, allow: i64) {
    widgets::table::set_allows_multiple_selection(handle, allow != 0)
}

#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_rows_count(handle: i64) -> i64 {
    widgets::table::get_selected_rows_count(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row_at(handle: i64, n: i64) -> i64 {
    widgets::table::get_selected_row_at(handle, n)
}

#[no_mangle]
pub extern "C" fn perry_ui_table_set_filter_text(handle: i64, text_ptr: i64) {
    widgets::table::set_filter_text(handle, text_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_ui_table_get_filter_text(handle: i64) -> i64 {
    widgets::table::get_filter_text(handle) as i64
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

/// perry_get_device_idiom() → "mac" — the device form-factor string for
/// macOS per the perry/system contract ("phone" / "pad" / "mac" / "tv" /
/// "watch" / "vision" / "desktop"). Returns a raw `*mut StringHeader`
/// (i64); the dispatch row is `ReturnKind::Str`, so codegen NaN-boxes it
/// with STRING_TAG. Not a screen-detection stub: this used to return the
/// numeric phone code (0.0) here, which both violated the string contract
/// and misreported a Mac as a phone.
#[no_mangle]
pub extern "C" fn perry_get_device_idiom() -> i64 {
    unsafe { js_string_from_bytes(b"mac".as_ptr(), 3) as i64 }
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

// --- ScrollView refresh control stubs (macOS has no native pull-to-refresh idiom) ---

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_refresh_control(_handle: i64, _callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_end_refreshing(_handle: i64) {}

// --- Issue #553: ScrollView + LazyVStack onScrollEnd, LazyVStack pull-to-refresh ---

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_scroll_end_callback(
    handle: i64,
    callback: f64,
    threshold_px: f64,
) {
    widgets::scrollview::set_scroll_end_callback(handle, callback, threshold_px);
}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_refresh_control(handle: i64, callback: f64) {
    widgets::lazyvstack::set_refresh_control(handle, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_end_refreshing(handle: i64) {
    widgets::lazyvstack::end_refreshing(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_scroll_end_callback(
    handle: i64,
    callback: f64,
    threshold_items: i64,
) {
    widgets::lazyvstack::set_scroll_end_callback(handle, callback, threshold_items);
}

// --- Issue #553: BottomNavigation (5-tab bottom bar with icon + label + badge) ---

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

// --- Issue #553: ImageGallery (swipeable carousel) ---

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

/// `Text(content, id)` — 2-arg form that registers the created NSTextField
/// with the per-id text registry so a later `state.set(...)` (which lowers
/// to `js_state_set`) can find this widget and update its `setStringValue:`
/// without needing the JS side to track the handle.
///
/// Pre-fix this stub created the widget but ignored `id_ptr` entirely, so
/// `state.text()` (which desugars to `Text(initial, synth_id)` via
/// `state_desugar.rs::state_text_call`) never landed in the registry —
/// `state.set("AFTER UPDATE")` then fired `setText(synth_id, "AFTER UPDATE")`,
/// the registry lookup missed, and the widget kept its initial text.
/// Issue #599.
#[no_mangle]
pub extern "C" fn perry_ui_text_create_with_id(text_ptr: i64, id_ptr: i64) -> i64 {
    let handle = perry_ui_text_create(text_ptr);
    if id_ptr != 0 {
        unsafe {
            let p = id_ptr as *const u8;
            let header = p as *const crate::string_header::StringHeader;
            let len = (*header).byte_len as usize;
            let data = p.add(std::mem::size_of::<crate::string_header::StringHeader>());
            widgets::text_registry::register_text_id_handler(handle, data, len);
        }
    }
    handle
}

/// `setText(id, value)` — direct C-ABI entry point. The runtime's
/// `js_state_set` pump routes through the registered cross-platform
/// `set_text_handler` (see `app.rs::js_register_set_text_handler`), so
/// most setText calls flow through that path. This direct shim covers
/// the `import { setText } from "perry/ui"` user-code surface for
/// completeness; it reads the StringHeaders and forwards to the same
/// handler. Issue #599.
#[no_mangle]
pub extern "C" fn perry_ui_set_text(id_ptr: i64, value_ptr: i64) {
    if id_ptr == 0 {
        return;
    }
    unsafe {
        let ip = id_ptr as *const u8;
        let id_header = ip as *const crate::string_header::StringHeader;
        let id_len = (*id_header).byte_len as usize;
        let id_data = ip.add(std::mem::size_of::<crate::string_header::StringHeader>());
        let (val_data, val_len) = if value_ptr == 0 {
            (std::ptr::null::<u8>(), 0usize)
        } else {
            let vp = value_ptr as *const u8;
            let v_header = vp as *const crate::string_header::StringHeader;
            let v_len = (*v_header).byte_len as usize;
            let v_data = vp.add(std::mem::size_of::<crate::string_header::StringHeader>());
            (v_data, v_len)
        };
        widgets::text_registry::set_text_handler(id_data, id_len, val_data, val_len);
    }
}

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
