use crate::app::str_from_header;
use crate::callback;
use crate::jni_bridge;
use jni::objects::JValue;

/// Create an EditText with placeholder and onChange callback. Returns widget handle.
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

    // Single line by default
    let _ = env.call_method(&edit_text, "setSingleLine", "(Z)V", &[JValue::Bool(1)]);

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

/// Focus an EditText (request focus).
pub fn focus(handle: i64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let _ = env.call_method(view_ref.as_obj(), "requestFocus", "()Z", &[]);
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Set the text of an EditText from a StringHeader pointer.
pub fn set_string_value(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    set_string_str(handle, text);
}

pub fn set_string_str(handle: i64, text: &str) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let jstr = env.new_string(text).expect("Failed to create JNI string");
        let _ = env.call_method(
            view_ref.as_obj(),
            "setText",
            "(Ljava/lang/CharSequence;)V",
            &[JValue::Object(&jstr)],
        );
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
}

/// Get the current text of an EditText. Returns a raw StringHeader pointer.
pub fn get_string_value(handle: i64) -> *const u8 {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        let text_result = env.call_method(
            view_ref.as_obj(),
            "getText",
            "()Landroid/text/Editable;",
            &[],
        );
        if let Ok(text_val) = text_result {
            if let Ok(text_obj) = text_val.l() {
                if !text_obj.is_null() {
                    let jstr_result =
                        env.call_method(&text_obj, "toString", "()Ljava/lang/String;", &[]);
                    if let Ok(jstr_val) = jstr_result {
                        if let Ok(jstr) = jstr_val.l() {
                            if let Ok(rust_str) = env.get_string((&jstr).into()) {
                                // Copy to owned String before pop_local_frame frees JNI refs.
                                // JavaStr's Drop calls ReleaseStringUTFChars — if the jstring
                                // local ref is already freed by pop_local_frame, JNI aborts.
                                let owned: String = rust_str.into();
                                let bytes = owned.as_bytes();
                                let str_ptr = unsafe {
                                    js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64)
                                };
                                unsafe {
                                    env.pop_local_frame(&jni::objects::JObject::null());
                                }
                                return str_ptr;
                            }
                        }
                    }
                }
            }
        }
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}

/// Set whether the text field is borderless (stub).
pub fn set_borderless(handle: i64, borderless: f64) {
    let _ = (handle, borderless);
}

/// Set the background color of the text field (stub).
pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    let _ = (handle, r, g, b, a);
}

/// Set the font size of the text field (stub).
pub fn set_font_size(handle: i64, size: f64) {
    let _ = (handle, size);
}

/// Set the text color of the text field (stub).
pub fn set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    let _ = (handle, r, g, b, a);
}

/// Set a callback for when the user presses Enter/Done on the keyboard.
pub fn set_on_submit(handle: i64, on_submit: f64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);

        let cb_key = crate::callback::register(on_submit);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        // Use PerryBridge to set an OnEditorActionListener
        let _ = env.call_static_method(
            bridge_cls,
            "setOnSubmitCallback",
            "(Landroid/widget/EditText;J)V",
            &[JValue::Object(view_ref.as_obj()), JValue::Long(cb_key)],
        );

        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}
