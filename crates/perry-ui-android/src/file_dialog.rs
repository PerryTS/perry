use crate::callback;
use crate::jni_bridge;
use jni::objects::JValue;

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

/// Open a file picker dialog.
/// Uses Android's Intent(ACTION_OPEN_DOCUMENT) via PerryBridge.
/// The callback receives the file content as a NaN-boxed string,
/// or TAG_UNDEFINED if the user cancelled.
pub fn open_dialog(cb: f64) {
    let cb_key = callback::register(cb);
    let mut env = jni_bridge::get_env();

    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "openFileDialog",
        "(J)V",
        &[JValue::Long(cb_key)],
    );
}

/// JNI entry point: called from Java when a file is selected.
/// Receives the file content as a Java String (or null if cancelled).
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeFileDialogResult(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
    content: jni::objects::JString,
) {
    let arg = if content.is_null() {
        // User cancelled — return TAG_UNDEFINED
        f64::from_bits(0x7FFC_0000_0000_0001)
    } else {
        // Convert Java String to NaN-boxed Perry string
        let rust_str: String = env
            .get_string(&content)
            .map(|s| s.into())
            .unwrap_or_default();
        let bytes = rust_str.as_bytes();
        unsafe {
            let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
            js_nanbox_string(str_ptr as i64)
        }
    };

    callback::invoke1(key as i64, arg);
}
