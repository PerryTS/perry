//! Background tasks (issue #538) — Android WorkManager bridge.
//!
//! `registerTask` stashes the user closure under an integer key, then asks
//! Kotlin to remember the (identifier → key) mapping. `schedule` enqueues
//! a `OneTimeWorkRequest` whose worker (PerryBackgroundWorker.kt) reads
//! the identifier back out of `inputData`, looks up the key, and bounces
//! to the UI thread to invoke the closure via `nativeInvokeCallback0`.
//!
//! `cancel` calls through to `WorkManager.cancelUniqueWork(identifier)`.

use crate::callback;
use crate::jni_bridge;
use jni::JValue;

use perry_ffi::copy_string_from_raw as str_from_header;

const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

fn boolean_truthy(v: f64) -> bool {
    let bits = v.to_bits();
    if bits == TAG_TRUE {
        return true;
    }
    if bits == TAG_FALSE || bits == TAG_UNDEFINED {
        return false;
    }
    v != 0.0 && !v.is_nan()
}

pub fn register_task(identifier_ptr: *const u8, handler: f64) {
    let id = unsafe { str_from_header(identifier_ptr) };
    if id.is_empty() {
        return;
    }
    let key = callback::register(handler);

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let id_jstr = env.new_string(&id).expect("new_string");
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("backgroundRegisterTask"),
            jni::jni_sig!("(Ljava/lang/String;J)V"),
            &[JValue::Object(&id_jstr), JValue::Long(key)],
        );
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
    })
}

pub fn schedule(
    identifier_ptr: *const u8,
    kind_ptr: *const u8,
    earliest_start_ms: f64,
    requires_network: f64,
    requires_charging: f64,
) {
    let id = unsafe { str_from_header(identifier_ptr) };
    if id.is_empty() {
        return;
    }
    let kind = unsafe { str_from_header(kind_ptr) };
    let kind = if kind.is_empty() {
        "appRefresh".to_string()
    } else {
        kind
    };
    let req_net = boolean_truthy(requires_network);
    let req_charge = boolean_truthy(requires_charging);

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 16);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let id_jstr = env.new_string(&id).expect("new_string");
        let kind_jstr = env.new_string(&kind).expect("new_string");
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("backgroundSchedule"),
            jni::jni_sig!("(Ljava/lang/String;Ljava/lang/String;DZZ)V"),
            &[
                JValue::Object(&id_jstr),
                JValue::Object(&kind_jstr),
                JValue::Double(earliest_start_ms),
                JValue::Bool(req_net),
                JValue::Bool(req_charge),
            ],
        );
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
    })
}

pub fn cancel(identifier_ptr: *const u8) {
    let id = unsafe { str_from_header(identifier_ptr) };
    if id.is_empty() {
        return;
    }
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let id_jstr = env.new_string(&id).expect("new_string");
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("backgroundCancel"),
            jni::jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(&id_jstr)],
        );
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
    })
}
