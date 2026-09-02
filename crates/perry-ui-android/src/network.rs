//! Network reachability (issue #582) — Android implementation.
//!
//! Delegates to PerryBridge.kt, which owns the
//! `ConnectivityManager.registerDefaultNetworkCallback` machinery (Java
//! NetworkCallback + the per-listener callback-key map). The Kotlin side
//! routes events back through `nativeInvokeNetworkCallback`, which converts
//! `(connected, kind)` into NaN-boxed JS values and dispatches to the
//! registered Perry closure.

use crate::callback;
use crate::jni_bridge;
use jni::JValue;

/// Read the current network status. The supplied callback fires synchronously
/// with `(connected, kind)`. `kind` is one of
/// `"wifi" | "cellular" | "ethernet" | "none" | "unknown"`.
pub fn get_status(cb: f64) {
    let key = callback::register(cb);

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("networkGetStatus"),
            jni::jni_sig!("(J)V"),
            &[JValue::Long(key)],
        );

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
    })
}

/// Subscribe to network reachability change events. Returns a numeric id;
/// pass it to `stop_on_change` to unsubscribe.
pub fn on_change(cb: f64) -> f64 {
    let key = callback::register(cb);

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let id: i64 = match env.call_static_method(
            bridge_cls,
            jni::jni_str!("networkOnChange"),
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

pub fn stop_on_change(id: f64) {
    let id_long = id as i64;
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("networkStopOnChange"),
            jni::jni_sig!("(J)V"),
            &[JValue::Long(id_long)],
        );

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
    })
}
