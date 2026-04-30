//! Streaming media playback (`perry/media`) — stub implementation.
//!
//! Issue #351 ships AVPlayer-backed implementations on the Apple platforms
//! (macOS / iOS / tvOS / visionOS). Other platforms link cleanly against
//! the same FFI surface but no-op at runtime until their respective
//! backends are wired up:
//!
//! - Android — `android.media.MediaPlayer` via JNI + `MediaSessionCompat`
//! - GTK4 / Linux — `gst::ElementFactory::make("playbin")` + MPRIS
//! - Windows — `IMFMediaEngine` + `SystemMediaTransportControls`
//! - HarmonyOS — `@ohos.multimedia.media.AVPlayer` via napi
//! - watchOS — limited; AVPlayer is available but Now Playing has a
//!   different shape (WKApplication's NowPlaying API)
//! - Web (`--target web`) — `<audio>` element + Media Session API
//!
//! Calls return `0` from `createPlayer` and silently succeed elsewhere so
//! a `perry/media` consumer can ship a single binary that gracefully
//! degrades on unsupported targets while the platform impl is being
//! finished.

#![allow(dead_code, unused_variables)]

pub fn create_player(_url_ptr: *const u8) -> i64 {
    0
}
pub fn play(_handle: f64) {}
pub fn pause(_handle: f64) {}
pub fn stop(_handle: f64) {}
pub fn seek(_handle: f64, _seconds: f64) {}
pub fn set_volume(_handle: f64, _volume: f64) {}
pub fn set_rate(_handle: f64, _rate: f64) {}
pub fn get_current_time(_handle: f64) -> f64 {
    0.0
}
pub fn get_duration(_handle: f64) -> f64 {
    0.0
}
pub fn get_state(_handle: f64) -> i64 {
    // "idle" via js_string_from_bytes — but we don't link runtime here
    // unconditionally, so callers must tolerate a 0 (which the codegen
    // will NaN-box as STRING_TAG | 0 — interpreted as the empty string).
    0
}
pub fn is_playing(_handle: f64) -> f64 {
    0.0
}
pub fn on_state_change(_handle: f64, _closure: f64) {}
pub fn on_time_update(_handle: f64, _closure: f64) {}
pub fn set_now_playing(
    _handle: f64,
    _title_ptr: *const u8,
    _artist_ptr: *const u8,
    _album_ptr: *const u8,
    _artwork_ptr: *const u8,
) {
}
pub fn destroy(_handle: f64) {}
