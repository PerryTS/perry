//! System API surface: open URL / share / app-group stubs, dark-mode,
//! preferences, keychain, notifications (foreground/background/scheduled),
//! location, geolocation, image picker, network reachability, deep links,
//! background tasks (WorkManager) and locale. Originally `lib.rs` lines
//! 1410-1714.

use crate::{
    background, deeplinks, geolocation, image_picker, jni_bridge, keychain, location, network,
    system,
};

// =============================================================================
// System API (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_system_open_url(url_ptr: i64) {
    system::open_url(url_ptr as *const u8);
}

/// #917 — system share sheet stub on Android. Native impl will use
/// `Intent.ACTION_SEND` with `Intent.EXTRA_TEXT` + `Intent.EXTRA_TITLE`
/// wrapped via `Intent.createChooser`, dispatched through JNI. MVP
/// stub + first-call warning.
#[no_mangle]
pub extern "C" fn perry_system_share_text(_text_ptr: i64, _title_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_share_text",
        "Android Intent.ACTION_SEND not yet implemented (#917 follow-up)",
        Some("#917"),
    );
}
#[no_mangle]
pub extern "C" fn perry_system_share_url(_url_ptr: i64, _title_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_share_url",
        "Android Intent.ACTION_SEND not yet implemented (#917 follow-up)",
        Some("#917"),
    );
}

// #675 — App Group stubs on Android. The iOS side was wired to
// `UserDefaults(suiteName:)` under #1178; the Android equivalent is
// `context.getSharedPreferences("perry_shared", MODE_PRIVATE)` —
// matching the read path the Glance widget bridge already uses in
// `perry-codegen-glance/src/emit_glue.rs` (`sharedStorageGet`). The
// JNI plumbing for the WRITE path needs a Kotlin-side helper on
// `PerryBridge` plus a JNI call here; tracked as #1178 Android
// follow-up (separate PR).
#[no_mangle]
pub extern "C" fn perry_system_app_group_set(_key_ptr: i64, _value_ptr: i64) {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_app_group_set",
        "Android SharedPreferences write path not yet implemented (#1178 Android follow-up). The Glance widget already reads `perry_shared` SharedPreferences; wiring the write side needs a Kotlin helper on PerryBridge.",
        Some("#1178"),
    );
}
#[no_mangle]
pub extern "C" fn perry_system_app_group_get(_key_ptr: i64) -> i64 {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_app_group_get",
        "Android SharedPreferences read path not yet implemented (#1178 Android follow-up).",
        Some("#1178"),
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
        "Android SharedPreferences delete path not yet implemented (#1178 Android follow-up).",
        Some("#1178"),
    );
}

#[no_mangle]
pub extern "C" fn perry_system_is_dark_mode() -> i64 {
    system::is_dark_mode()
}

#[no_mangle]
pub extern "C" fn perry_system_preferences_set(key_ptr: i64, value: f64) {
    system::preferences_set(key_ptr as *const u8, value);
}

#[no_mangle]
pub extern "C" fn perry_system_preferences_get(key_ptr: i64) -> f64 {
    system::preferences_get(key_ptr as *const u8)
}

/// Play a haptic feedback effect (perry/system hapticPlay) —
/// VibrationEffect.createPredefined on API 29+, vibrate(long) before.
#[no_mangle]
pub extern "C" fn perry_system_haptic_play(type_ptr: i64) {
    system::haptic_play(type_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_system_keychain_save(key_ptr: i64, value_ptr: i64) {
    keychain::save(key_ptr as *const u8, value_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_system_keychain_get(key_ptr: i64) -> f64 {
    keychain::get(key_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_system_keychain_delete(key_ptr: i64) {
    keychain::delete(key_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_system_notification_send(title_ptr: i64, body_ptr: i64) {
    system::notification_send(title_ptr as *const u8, body_ptr as *const u8);
}

/// Real impl (#95): kick off FCM token fetch + register the JS closure that
/// fires when FCM hands us a registration token. Requires a real
/// `google-services.json` to actually work — the placeholder bundled with
/// the template lets the build succeed but the SDK rejects it at runtime.
#[no_mangle]
pub extern "C" fn perry_system_notification_register_remote(callback: f64) {
    system::notification_register_remote(callback);
}

/// Real impl (#95): register the JS closure that fires for foreground FCM
/// payloads. `PerryFirebaseMessagingService.onMessageReceived` forwards
/// the JSON-serialized RemoteMessage to native via JNI.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_receive(callback: f64) {
    system::notification_on_receive(callback);
}

/// Real impl (#98): register the JS closure that fires for background FCM
/// payloads. Routes through the same `PerryFirebaseMessagingService`
/// pipeline as foreground delivery — Android doesn't split the two at the
/// service layer — so the callback fires for every payload that reaches
/// `nativeNotificationBackgroundReceive`. See system.rs for the v1
/// trade-offs around Promise gating and cold-start.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_background_receive(callback: f64) {
    system::notification_on_background_receive(callback);
}

/// Schedule a fire-after-N-seconds notification via AlarmManager (#96).
#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_interval(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    seconds: f64,
    repeats: f64,
) {
    system::notification_schedule_interval(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        seconds,
        repeats,
    );
}

/// Schedule a fire-at-wallclock-ms notification via AlarmManager (#96).
#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_calendar(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    timestamp_ms: f64,
) {
    system::notification_schedule_calendar(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        timestamp_ms,
    );
}

/// Logged no-op — Geofencing API requires `FUSED_LOCATION_PROVIDER` + a
/// runtime `ACCESS_FINE_LOCATION` grant. Deferred to #96 follow-up.
#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_location(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    lat: f64,
    lon: f64,
    radius: f64,
) {
    system::notification_schedule_location(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        lat,
        lon,
        radius,
    );
}

/// Cancel a scheduled or already-displayed notification by id (#96).
#[no_mangle]
pub extern "C" fn perry_system_notification_cancel(id_ptr: i64) {
    system::notification_cancel(id_ptr as *const u8);
}

/// Real impl (#97): register the tap callback so `PerryNotificationReceiver`
/// can dispatch back to it when the user taps a delivered notification.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_tap(callback: f64) {
    system::notification_on_tap(callback);
}

#[no_mangle]
pub extern "C" fn perry_system_request_location(callback: f64) {
    location::request_location(callback);
}

// ---- Geolocation + image picker (issue #552) ----
#[no_mangle]
pub extern "C" fn perry_system_geolocation_get_current(on_success: f64, on_error: f64) {
    geolocation::get_current(on_success, on_error);
}
#[no_mangle]
pub extern "C" fn perry_system_geolocation_watch(callback: f64) -> f64 {
    geolocation::watch(callback)
}
#[no_mangle]
pub extern "C" fn perry_system_geolocation_stop_watch(id: f64) {
    geolocation::stop_watch(id);
}
#[no_mangle]
pub extern "C" fn perry_system_geolocation_request_permission(callback: f64) {
    geolocation::request_permission(callback);
}
#[no_mangle]
pub extern "C" fn perry_system_image_picker_pick(
    max_count: f64,
    allow_multiple: f64,
    callback: f64,
) {
    image_picker::pick(max_count, allow_multiple, callback);
}

// ---- Network reachability (issue #582) ----
#[no_mangle]
pub extern "C" fn perry_system_network_get_status(callback: f64) {
    network::get_status(callback);
}
#[no_mangle]
pub extern "C" fn perry_system_network_on_change(callback: f64) -> f64 {
    network::on_change(callback)
}
#[no_mangle]
pub extern "C" fn perry_system_network_stop_on_change(id: f64) {
    network::stop_on_change(id);
}

// ---- Deep links (issue #583) ----
#[no_mangle]
pub extern "C" fn perry_system_app_on_open_url(callback: f64) {
    deeplinks::set_handler(callback);
}
#[no_mangle]
pub extern "C" fn perry_system_app_get_launch_url() -> i64 {
    let s = deeplinks::launch_url();
    let bytes = s.as_bytes();
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    }
    unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) as i64 }
}

// ---- perry/background (issue #538) — WorkManager ----
#[no_mangle]
pub extern "C" fn perry_background_register_task(identifier_ptr: i64, handler: f64) {
    background::register_task(identifier_ptr as *const u8, handler);
}
#[no_mangle]
pub extern "C" fn perry_background_schedule(
    identifier_ptr: i64,
    kind_ptr: i64,
    earliest_start_ms: f64,
    requires_network: f64,
    requires_charging: f64,
) {
    background::schedule(
        identifier_ptr as *const u8,
        kind_ptr as *const u8,
        earliest_start_ms,
        requires_network,
        requires_charging,
    );
}
#[no_mangle]
pub extern "C" fn perry_background_cancel(identifier_ptr: i64) {
    background::cancel(identifier_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_system_get_locale() -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let locale_class = env.find_class("java/util/Locale").expect("Locale class");
    let default_locale = env
        .call_static_method(locale_class, "getDefault", "()Ljava/util/Locale;", &[])
        .expect("getDefault")
        .l()
        .expect("locale obj");
    let lang = env
        .call_method(&default_locale, "getLanguage", "()Ljava/lang/String;", &[])
        .expect("getLanguage")
        .l()
        .expect("lang string");
    let jstr: jni::objects::JString = lang.into();
    let s: String = env.get_string(&jstr).expect("get string").into();
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    let bytes = s.as_bytes();
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    }
    unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) as i64 }
}
