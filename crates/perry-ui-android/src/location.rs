//! Location API — request one-shot location via Android LocationManager.

use crate::callback;
use crate::jni_bridge;
use jni::objects::JValue;

/// Request a one-shot location. The callback receives (lat, lon) on success
/// or (NaN, NaN) on error/denial. The Java side handles permission requests.
pub fn request_location(callback_f64: f64) {
    let key = callback::register(callback_f64);

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());

    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(bridge_cls, "requestLocation", "(J)V", &[JValue::Long(key)]);

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}
