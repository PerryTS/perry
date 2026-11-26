//! Dialog — save file dialog and alert

use crate::callback;
use crate::jni_bridge;
use jni::JValue;

use perry_ffi::copy_string_from_raw as str_from_header;

extern "C" {
    fn js_closure_call1(closure: f64, this: perry_ffi::JsThis, arg: f64) -> f64;
    fn js_string_from_bytes(ptr: *const u8, len: usize) -> *const u8;
    fn js_nanbox_string(ptr: *const u8) -> f64;
}

/// Show a save file dialog. On Android, uses PerryBridge helper or Intent.
pub fn save_file_dialog(
    callback: f64,
    _default_name_ptr: *const u8,
    _allowed_types_ptr: *const u8,
) {
    // Android save file dialog requires Activity.startActivityForResult with Intent(ACTION_CREATE_DOCUMENT)
    // For now, call callback with empty string (no file selected)
    if callback != 0.0 {
        let empty = b"\0";
        let s = unsafe { js_string_from_bytes(empty.as_ptr(), 0) };
        let val = unsafe { js_nanbox_string(s) };
        unsafe {
            js_closure_call1(callback, perry_ffi::JsThis::UNDEFINED, val);
        }
    }
}

/// `alertWithButtons(title, message, buttons, cb)`: an AlertDialog with the
/// given labels; `cb` gets the 0-based index. `arr_ptr` is the unboxed JS
/// array of label strings (0 = just "OK").
pub fn alert(title_ptr: *const u8, message_ptr: *const u8, arr_ptr: i64, callback: f64) {
    extern "C" {
        fn js_array_get_length(arr: i64) -> i64;
        fn js_array_get_element(arr: i64, index: i64) -> f64;
        fn js_get_string_pointer_unified(val: f64) -> i64;
    }
    let title = if title_ptr.is_null() {
        String::new()
    } else {
        unsafe { str_from_header(title_ptr) }.to_string()
    };
    let message = if message_ptr.is_null() {
        String::new()
    } else {
        unsafe { str_from_header(message_ptr) }.to_string()
    };
    let mut labels: Vec<String> = Vec::new();
    if arr_ptr != 0 {
        let len = unsafe { js_array_get_length(arr_ptr) };
        for i in 0..len {
            let elem = unsafe { js_array_get_element(arr_ptr, i) };
            let p = unsafe { js_get_string_pointer_unified(elem) } as *const u8;
            labels.push(if p.is_null() {
                String::new()
            } else {
                unsafe { str_from_header(p) }.to_string()
            });
        }
    }
    if labels.is_empty() {
        labels.push("OK".to_string());
    }

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 16);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let jtitle = env.new_string(&title).expect("title");
        let jmessage = env.new_string(&message).expect("message");
        // Labels joined by U+001F (unit separator); split in Kotlin.
        let joined = labels.join("\u{1f}");
        let jlabels = env.new_string(&joined).expect("labels");
        let cb_key = if callback != 0.0 {
            callback::register(callback)
        } else {
            0
        };
        if env
            .call_static_method(
                bridge_cls,
                jni::jni_str!("showButtonsAlert"),
                jni::jni_sig!("(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;J)V"),
                &[
                    JValue::Object(&jtitle),
                    JValue::Object(&jmessage),
                    JValue::Object(&jlabels),
                    JValue::Long(cb_key),
                ],
            )
            .is_err()
        {
            let _ = env.exception_clear();
        }
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
    })
}
