//! Photo-library image picker (issue #552) — Android implementation.
//!
//! Delegates to PerryBridge.requestImagePickerPick which presents the
//! Photo Picker (API 33+) or ACTION_GET_CONTENT, copies each picked
//! content URI into the app's cache dir, and routes the resulting
//! file paths back through `nativeInvokeCallbackWithStringArray`.

use crate::callback;
use crate::jni_bridge;
use jni::objects::JValue;

pub fn pick(max_count: f64, allow_multiple: f64, callback_f64: f64) {
    let key = callback::register(callback_f64);

    let max = if max_count.is_finite() && max_count > 0.0 {
        max_count as i32
    } else {
        0
    };
    // NaN-boxed booleans arrive as 0x7FFC_0000_0000_0004 (TRUE) /
    // 0x7FFC_0000_0000_0003 (FALSE). Numbers (0/non-zero) also pass through
    // f64 — accept either by treating any non-FALSE non-zero as truthy.
    let multi_bits = allow_multiple.to_bits();
    let allow_multi = multi_bits == 0x7FFC_0000_0000_0004
        || (multi_bits != 0x7FFC_0000_0000_0003
            && allow_multiple != 0.0
            && !allow_multiple.is_nan());

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "requestImagePickerPick",
        "(IZJ)V",
        &[
            JValue::Int(max),
            JValue::Bool(if allow_multi { 1 } else { 0 }),
            JValue::Long(key),
        ],
    );

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}
