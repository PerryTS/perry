use crate::app::str_from_header;
use crate::callback;
use crate::jni_bridge;
use jni::JValue;

/// Create a multi-line EditText (TextArea). Returns widget handle.
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

        // Set hint (placeholder)
        let hint_str = env
            .new_string(placeholder)
            .expect("Failed to create JNI string");
        let _ = env.call_method(
            &edit_text,
            jni::jni_str!("setHint"),
            jni::jni_sig!("(Ljava/lang/CharSequence;)V"),
            &[JValue::Object(&hint_str)],
        );

        // Multi-line: do NOT call setSingleLine (default is multi-line)
        // Set min lines and gravity to top-left for textarea behavior
        let _ = env.call_method(
            &edit_text,
            jni::jni_str!("setMinLines"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(4)],
        );
        let _ = env.call_method(
            &edit_text,
            jni::jni_str!("setGravity"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(0x30 | 0x03)], // TOP | LEFT
        );

        // MATCH_PARENT width, WRAP_CONTENT height
        let params = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout$LayoutParams"),
                jni::jni_sig!("(II)V"),
                &[JValue::Int(-1), JValue::Int(-2)],
            )
            .expect("Failed to create LayoutParams");
        let _ = env.call_method(
            &edit_text,
            jni::jni_str!("setLayoutParams"),
            jni::jni_sig!("(Landroid/view/ViewGroup$LayoutParams;)V"),
            &[JValue::Object(&params)],
        );

        // Register callback and set up TextWatcher via PerryBridge
        let cb_key = callback::register(on_change);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("setTextChangedCallback"),
            jni::jni_sig!("(Landroid/widget/EditText;J)V"),
            &[JValue::Object(&edit_text), JValue::Long(cb_key)],
        );

        let global =
            jni_bridge::new_global_ref(env, edit_text).expect("Failed to create global ref");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}
