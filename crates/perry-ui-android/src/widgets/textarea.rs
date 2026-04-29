use crate::app::str_from_header;
use crate::callback;
use crate::jni_bridge;
use jni::objects::JValue;

/// Create a multi-line EditText (TextArea). Returns widget handle.
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

    // Set hint (placeholder)
    let hint_str = env
        .new_string(placeholder)
        .expect("Failed to create JNI string");
    let _ = env.call_method(
        &edit_text,
        "setHint",
        "(Ljava/lang/CharSequence;)V",
        &[JValue::Object(&hint_str)],
    );

    // Multi-line: do NOT call setSingleLine (default is multi-line)
    // Set min lines and gravity to top-left for textarea behavior
    let _ = env.call_method(&edit_text, "setMinLines", "(I)V", &[JValue::Int(4)]);
    let _ = env.call_method(
        &edit_text,
        "setGravity",
        "(I)V",
        &[JValue::Int(0x30 | 0x03)], // TOP | LEFT
    );

    // MATCH_PARENT width, WRAP_CONTENT height
    let params = env
        .new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(II)V",
            &[JValue::Int(-1), JValue::Int(-2)],
        )
        .expect("Failed to create LayoutParams");
    let _ = env.call_method(
        &edit_text,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&params)],
    );

    // Register callback and set up TextWatcher via PerryBridge
    let cb_key = callback::register(on_change);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "setTextChangedCallback",
        "(Landroid/widget/EditText;J)V",
        &[JValue::Object(&edit_text), JValue::Long(cb_key)],
    );

    let global = env
        .new_global_ref(edit_text)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}
