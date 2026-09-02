//! Dialog — save file dialog and alert

use crate::callback;
use crate::jni_bridge;
use jni::JValue;

use perry_ffi::copy_string_from_raw as str_from_header;

extern "C" {
    fn js_closure_call1(closure: f64, arg: f64) -> f64;
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
            js_closure_call1(callback, val);
        }
    }
}

/// Show an alert dialog with title, message, buttons and callback.
pub fn alert(title_ptr: *const u8, message_ptr: *const u8, _buttons_ptr: *const u8, callback: f64) {
    let title = unsafe { str_from_header(title_ptr) };
    let message = unsafe { str_from_header(message_ptr) };

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);

        let activity = crate::widgets::get_activity(env);

        // Use PerryBridge.showAlert(Activity, String title, String message, long callback)
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());

        let jtitle = env.new_string(title).expect("Failed to create JNI string");
        let jmessage = env
            .new_string(message)
            .expect("Failed to create JNI string");
        let cb_key = if callback != 0.0 {
            callback::register(callback)
        } else {
            0
        };

        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("showAlert"),
            jni::jni_sig!("(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;J)V"),
            &[
                JValue::Object(&activity),
                JValue::Object(&jtitle),
                JValue::Object(&jmessage),
                JValue::Long(cb_key),
            ],
        );

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
    })
}
