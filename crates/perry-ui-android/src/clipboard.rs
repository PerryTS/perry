use crate::jni_bridge;
use jni::objects::JValue;

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

/// Read the current text from the system clipboard.
/// Returns a NaN-boxed string (f64) or TAG_UNDEFINED if empty.
pub fn read() -> f64 {
    let mut env = jni_bridge::get_env();

    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let result = env.call_static_method(bridge_cls, "clipboardRead", "()Ljava/lang/String;", &[]);

    if let Ok(val) = result {
        if let Ok(obj) = val.l() {
            if !obj.is_null() {
                let jstr: jni::objects::JString = obj.into();
                let s: String = env
                    .get_string(&jstr)
                    .map(|js| js.into())
                    .unwrap_or_default();
                let bytes = s.as_bytes();
                unsafe {
                    let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                    return js_nanbox_string(str_ptr as i64);
                }
            }
        }
    }

    // Return TAG_UNDEFINED
    f64::from_bits(0x7FFC_0000_0000_0001)
}

/// Write text to the system clipboard.
pub fn write(text_ptr: *const u8) {
    let text = crate::app::str_from_header(text_ptr);
    let mut env = jni_bridge::get_env();

    let jstr = env.new_string(text).expect("Failed to create JNI string");
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "clipboardWrite",
        "(Ljava/lang/String;)V",
        &[JValue::Object(&jstr)],
    );
}
