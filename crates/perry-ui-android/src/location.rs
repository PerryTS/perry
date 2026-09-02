//! Location API — request one-shot location via Android LocationManager.

use crate::callback;
use crate::jni_bridge;
use jni::JValue;

/// Request a one-shot location. The callback receives (lat, lon) on success
/// or (NaN, NaN) on error/denial. The Java side handles permission requests.
pub fn request_location(callback_f64: f64) {
    let key = callback::register(callback_f64);

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 16);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());

        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("requestLocation"),
            jni::jni_sig!("(J)V"),
            &[JValue::Long(key)],
        );

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
    })
}
