use crate::app::str_from_header;
use crate::callback;
use crate::jni_bridge;
use jni::JValue;

/// Create an EditText with placeholder and onChange callback. Returns widget handle.
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

        // Single line by default
        let _ = env.call_method(
            &edit_text,
            jni::jni_str!("setSingleLine"),
            jni::jni_sig!("(Z)V"),
            &[JValue::Bool(true)],
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

/// Focus an EditText (request focus).
pub fn focus(handle: i64) {
    if let Some(view_ref) = super::get_widget(handle) {
        jni_bridge::with_env(|env| {
            let _ = jni_bridge::push_local_frame(env, 8);
            let _ = env.call_method(
                view_ref.as_obj(),
                jni::jni_str!("requestFocus"),
                jni::jni_sig!("()Z"),
                &[],
            );
            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
            }
        })
    }
}

/// Set the text of an EditText from a StringHeader pointer.
pub fn set_string_value(handle: i64, text_ptr: *const u8) {
    let text = unsafe { str_from_header(text_ptr) };
    set_string_str(handle, &text);
}

pub fn set_string_str(handle: i64, text: &str) {
    if let Some(view_ref) = super::get_widget(handle) {
        jni_bridge::with_env(|env| {
            let _ = jni_bridge::push_local_frame(env, 8);
            let jstr = env.new_string(text).expect("Failed to create JNI string");
            let _ = env.call_method(
                view_ref.as_obj(),
                jni::jni_str!("setText"),
                jni::jni_sig!("(Ljava/lang/CharSequence;)V"),
                &[JValue::Object(&jstr)],
            );
            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
            }
        })
    }
}

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
}

/// Get the current text of an EditText. Returns a raw StringHeader pointer.
pub fn get_string_value(handle: i64) -> *const u8 {
    if let Some(view_ref) = super::get_widget(handle) {
        let result = jni_bridge::with_env(|env| {
            let _ = jni_bridge::push_local_frame(env, 16);
            let text_result = env.call_method(
                view_ref.as_obj(),
                jni::jni_str!("getText"),
                jni::jni_sig!("()Landroid/text/Editable;"),
                &[],
            );
            if let Ok(text_val) = text_result {
                if let Ok(text_obj) = text_val.l() {
                    if !text_obj.is_null() {
                        let jstr_result = env.call_method(
                            &text_obj,
                            jni::jni_str!("toString"),
                            jni::jni_sig!("()Ljava/lang/String;"),
                            &[],
                        );
                        if let Ok(jstr_val) = jstr_result {
                            if let Ok(jstr) = jstr_val.l() {
                                let jstr = unsafe {
                                    jni::objects::JString::from_raw(env, jstr.into_raw())
                                };
                                if let Ok(rust_str) = jstr.try_to_string(env) {
                                    // Copy to owned String before pop_local_frame frees JNI refs.
                                    let bytes = rust_str.as_bytes();
                                    let str_ptr = unsafe {
                                        js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64)
                                    };
                                    unsafe {
                                        let _ = jni_bridge::pop_local_frame(
                                            env,
                                            &jni::objects::JObject::null(),
                                        );
                                    }
                                    return Some(str_ptr);
                                }
                            }
                        }
                    }
                }
            }
            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
            }
            None
        });
        if let Some(str_ptr) = result {
            return str_ptr;
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
        jni_bridge::with_env(|env| {
            let _ = jni_bridge::push_local_frame(env, 16);

            let cb_key = crate::callback::register(on_submit);
            let bridge_class =
                jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
            let bridge_cls: &jni::objects::JClass = &bridge_class;
            // Use PerryBridge to set an OnEditorActionListener
            let _ = env.call_static_method(
                bridge_cls,
                jni::jni_str!("setOnSubmitCallback"),
                jni::jni_sig!("(Landroid/widget/EditText;J)V"),
                &[JValue::Object(view_ref.as_obj()), JValue::Long(cb_key)],
            );

            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
            }
        })
    }
}
