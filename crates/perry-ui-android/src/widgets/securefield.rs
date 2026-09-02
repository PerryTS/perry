//! SecureField — EditText with password input type

use crate::callback;
use crate::jni_bridge;
use jni::JValue;

use perry_ffi::copy_string_from_raw as str_from_header;

pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    let placeholder = unsafe { str_from_header(placeholder_ptr) };
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);

        let activity = super::get_activity(env);
        let edit_text = env
            .new_object(
                jni::jni_str!("android/widget/EditText"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create EditText");

        // Set input type to password (TYPE_CLASS_TEXT | TYPE_TEXT_VARIATION_PASSWORD = 0x81)
        let _ = env.call_method(
            &edit_text,
            jni::jni_str!("setInputType"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(0x81)],
        );

        // Set placeholder
        let jstr = env
            .new_string(placeholder)
            .expect("Failed to create JNI string");
        let _ = env.call_method(
            &edit_text,
            jni::jni_str!("setHint"),
            jni::jni_sig!("(Ljava/lang/CharSequence;)V"),
            &[JValue::Object(&jstr)],
        );

        // Set single line
        let _ = env.call_method(
            &edit_text,
            jni::jni_str!("setSingleLine"),
            jni::jni_sig!("(Z)V"),
            &[JValue::Bool(true)],
        );

        // Register change callback via PerryBridge
        if on_change != 0.0 {
            let cb_key = callback::register(on_change);
            let bridge_class =
                jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
            let bridge_cls: &jni::objects::JClass = &bridge_class;
            // Use the same JNI bridge entrypoint as textfield/textarea —
            // PerryBridge.setTextChangedCallback. The stale `setTextWatcher`
            // name didn't exist on the Kotlin side and aborted with
            // NoSuchMethodError whenever a SecureField was constructed.
            let _ = env.call_static_method(
                bridge_cls,
                jni::jni_str!("setTextChangedCallback"),
                jni::jni_sig!("(Landroid/widget/EditText;J)V"),
                &[JValue::Object(&edit_text), JValue::Long(cb_key)],
            );
        }

        let global =
            jni_bridge::new_global_ref(env, edit_text).expect("Failed to create global ref");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}
