// FFI: System APIs (URL/share/AppGroup/dark mode/prefs/keychain/notifications/locale)
// + Weather App Extensions (location).
use crate::{keychain, location, system};

// =============================================================================
// System API
// =============================================================================

/// Open a URL in the default browser.
#[no_mangle]
pub extern "C" fn perry_system_open_url(url_ptr: i64) {
    system::open_url(url_ptr as *const u8);
}

/// #917 — system share sheet stub on GTK4 / Linux. A real
/// implementation would launch the XDG desktop portal's
/// "Open URI" / "Send" flow via `org.freedesktop.portal.Email` /
/// `Sharing`; landed as a #917 follow-up. MVP stub + first-call
/// warning.
#[no_mangle]
pub extern "C" fn perry_system_share_text(_text_ptr: i64, _title_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_share_text",
        "GTK4/Linux XDG share portal not yet wired (#917 follow-up)",
        Some("#917"),
    );
}
#[no_mangle]
pub extern "C" fn perry_system_share_url(_url_ptr: i64, _title_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_share_url",
        "GTK4/Linux XDG share portal not yet wired (#917 follow-up)",
        Some("#917"),
    );
}

// #675 — App Group stubs on GTK4 / Linux. Native impl will use a
// per-user XDG path (`$XDG_DATA_HOME/<bundle_id>/shared/`) for the
// blob shape and a small JSON-backed kv store; tracked as #675
// follow-up.
#[no_mangle]
pub extern "C" fn perry_system_app_group_set(_key_ptr: i64, _value_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_app_group_set",
        "GTK4/Linux XDG-backed App Group not implemented (#675 follow-up)",
        Some("#675"),
    );
}
#[no_mangle]
pub extern "C" fn perry_system_app_group_get(_key_ptr: i64) -> i64 {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_app_group_get",
        "GTK4/Linux XDG-backed App Group not implemented (#675 follow-up)",
        Some("#675"),
    );
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}
#[no_mangle]
pub extern "C" fn perry_system_app_group_delete(_key_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_app_group_delete",
        "GTK4/Linux XDG-backed App Group not implemented (#675 follow-up)",
        Some("#675"),
    );
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

/// perry/system hapticPlay — documented no-op: Linux desktops have no
/// general haptic engine (the API contract is "no-op on platforms
/// without one", so no stub warning).
#[no_mangle]
pub extern "C" fn perry_system_haptic_play(_type_ptr: i64) {}

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
