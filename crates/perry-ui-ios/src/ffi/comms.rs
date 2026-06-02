//! FFI exports: websocket, media playback (AVPlayer), attributed text
//!
//! Extracted from `lib.rs` for file-size hygiene. No behavior changes.

use crate::*;

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
    crate::ffi::widgets_basic::perry_ui_text_create(text_ptr)
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
