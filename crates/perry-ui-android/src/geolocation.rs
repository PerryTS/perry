//! Geolocation API (issue #552) — Android implementation.
//!
//! Delegates to PerryBridge.kt. The Kotlin side handles permission checks,
//! LocationManager interaction, and routing the result back through
//! `nativeInvokeCallback4` (success) or `nativeInvokeCallbackWithString`
//! (error / permission status).

use crate::callback;
use crate::jni_bridge;
use jni::JValue;

/// Resolve the device's current position. `success_cb` receives
/// (lat, lng, accuracy, timestamp_ms); `error_cb` receives a single
/// status-string argument on permission denial / timeout / OS failure.
pub fn get_current(success_cb: f64, error_cb: f64) {
    let success_key = callback::register(success_cb);
    let error_key = callback::register(error_cb);

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 16);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("requestGeolocationGetCurrent"),
            jni::jni_sig!("(JJ)V"),
            &[JValue::Long(success_key), JValue::Long(error_key)],
        );

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
    })
}

/// Subscribe to position updates. Returns a numeric watch id; the Kotlin
/// side keeps a parallel id → LocationListener map so `stop_watch` can
/// remove it.
pub fn watch(callback_f64: f64) -> f64 {
    let key = callback::register(callback_f64);

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 16);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let id: i64 = match env.call_static_method(
            bridge_cls,
            jni::jni_str!("requestGeolocationWatch"),
            jni::jni_sig!("(J)J"),
            &[JValue::Long(key)],
        ) {
            Ok(v) => v.j().unwrap_or(0),
            Err(_) => 0,
        };

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }

        id as f64
    })
}

pub fn stop_watch(id: f64) {
    let id_long = id as i64;
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("stopGeolocationWatch"),
            jni::jni_sig!("(J)V"),
            &[JValue::Long(id_long)],
        );

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
    })
}

/// Request location permission. Callback receives the status string
/// (`"granted"` | `"denied"` | `"restricted"` | `"unsupported-platform"`).
pub fn request_permission(callback_f64: f64) {
    let key = callback::register(callback_f64);

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("requestGeolocationPermission"),
            jni::jni_sig!("(J)V"),
            &[JValue::Long(key)],
        );

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
    })
}
