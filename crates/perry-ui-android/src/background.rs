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
use jni::objects::JValue;

fn str_from_header(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

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
    let id = str_from_header(identifier_ptr);
    if id.is_empty() {
        return;
    }
    let key = callback::register(handler);

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let id_jstr = env.new_string(&id).expect("new_string");
    let _ = env.call_static_method(
        bridge_cls,
        "backgroundRegisterTask",
        "(Ljava/lang/String;J)V",
        &[JValue::Object(&id_jstr), JValue::Long(key)],
    );
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

pub fn schedule(
    identifier_ptr: *const u8,
    kind_ptr: *const u8,
    earliest_start_ms: f64,
    requires_network: f64,
    requires_charging: f64,
) {
    let id = str_from_header(identifier_ptr);
    if id.is_empty() {
        return;
    }
    let kind = str_from_header(kind_ptr);
    let kind = if kind.is_empty() {
        "appRefresh".to_string()
    } else {
        kind
    };
    let req_net = boolean_truthy(requires_network);
    let req_charge = boolean_truthy(requires_charging);

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let id_jstr = env.new_string(&id).expect("new_string");
    let kind_jstr = env.new_string(&kind).expect("new_string");
    let _ = env.call_static_method(
        bridge_cls,
        "backgroundSchedule",
        "(Ljava/lang/String;Ljava/lang/String;DZZ)V",
        &[
            JValue::Object(&id_jstr),
            JValue::Object(&kind_jstr),
            JValue::Double(earliest_start_ms),
            JValue::Bool(if req_net { 1 } else { 0 }),
            JValue::Bool(if req_charge { 1 } else { 0 }),
        ],
    );
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

pub fn cancel(identifier_ptr: *const u8) {
    let id = str_from_header(identifier_ptr);
    if id.is_empty() {
        return;
    }
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let id_jstr = env.new_string(&id).expect("new_string");
    let _ = env.call_static_method(
        bridge_cls,
        "backgroundCancel",
        "(Ljava/lang/String;)V",
        &[JValue::Object(&id_jstr)],
    );
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}
