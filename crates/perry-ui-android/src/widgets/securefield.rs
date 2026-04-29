//! SecureField — EditText with password input type

use crate::callback;
use crate::jni_bridge;
use jni::objects::JValue;

fn str_from_header(ptr: *const u8) -> &'static str {
    crate::app::str_from_header(ptr)
}

pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    let placeholder = str_from_header(placeholder_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);
    let edit_text = env
        .new_object(
            "android/widget/EditText",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create EditText");

    // Set input type to password (TYPE_CLASS_TEXT | TYPE_TEXT_VARIATION_PASSWORD = 0x81)
    let _ = env.call_method(&edit_text, "setInputType", "(I)V", &[JValue::Int(0x81)]);

    // Set placeholder
    let jstr = env
        .new_string(placeholder)
        .expect("Failed to create JNI string");
    let _ = env.call_method(
        &edit_text,
        "setHint",
        "(Ljava/lang/CharSequence;)V",
        &[JValue::Object(&jstr)],
    );

    // Set single line
    let _ = env.call_method(&edit_text, "setSingleLine", "(Z)V", &[JValue::Bool(1)]);

    // Register change callback via PerryBridge
    if on_change != 0.0 {
        let cb_key = callback::register(on_change);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "setTextWatcher",
            "(Landroid/widget/EditText;J)V",
            &[JValue::Object(&edit_text), JValue::Long(cb_key)],
        );
    }

    let global = env
        .new_global_ref(edit_text)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}
