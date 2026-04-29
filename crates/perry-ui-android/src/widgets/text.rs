use crate::app::str_from_header;
use crate::jni_bridge;
use jni::objects::{JObject, JValue};

/// Create a TextView. Returns widget handle.
pub fn create(text_ptr: *const u8) -> i64 {
    let text = str_from_header(text_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);
    let text_view = env
        .new_object(
            "android/widget/TextView",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create TextView");

    let jstr = env.new_string(text).expect("Failed to create JNI string");
    let _ = env.call_method(
        &text_view,
        "setText",
        "(Ljava/lang/CharSequence;)V",
        &[JValue::Object(&jstr)],
    );

    let global = env
        .new_global_ref(text_view)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}

/// Update the text of an existing TextView.
pub fn set_text_str(handle: i64, text: &str) {
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

/// Update the text of an existing TextView from a StringHeader pointer.
pub fn set_string(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    set_text_str(handle, text);
}

/// Set the text color of a TextView (RGBA 0.0-1.0).
pub fn set_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let ai = (a * 255.0) as i32;
        let ri = (r * 255.0) as i32;
        let gi = (g * 255.0) as i32;
        let bi = (b * 255.0) as i32;
        let color = (ai << 24) | (ri << 16) | (gi << 8) | bi;
        let _ = env.call_method(
            view_ref.as_obj(),
            "setTextColor",
            "(I)V",
            &[JValue::Int(color)],
        );
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Set the font size of a TextView (in sp, roughly equivalent to pt on iOS).
pub fn set_font_size(handle: i64, size: f64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        // TypedValue.COMPLEX_UNIT_SP = 2
        let _ = env.call_method(
            view_ref.as_obj(),
            "setTextSize",
            "(IF)V",
            &[JValue::Int(2), JValue::Float(size as f32)],
        );
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Set the font weight of a TextView.
/// weight >= 1.0 means bold (Typeface.BOLD=1), otherwise normal (Typeface.NORMAL=0).
pub fn set_font_weight(handle: i64, _size: f64, weight: f64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        let style = if weight >= 0.5 { 1i32 } else { 0i32 }; // Typeface.BOLD=1, NORMAL=0

        // Create a Typeface with the default font family and desired style.
        // Passing null Typeface to setTypeface corrupts the text content,
        // so we must create a valid Typeface via Typeface.defaultFromStyle().
        let typeface = env.call_static_method(
            "android/graphics/Typeface",
            "defaultFromStyle",
            "(I)Landroid/graphics/Typeface;",
            &[JValue::Int(style)],
        );
        if let Ok(tf_val) = typeface {
            if let Ok(tf) = tf_val.l() {
                let _ = env.call_method(
                    view_ref.as_obj(),
                    "setTypeface",
                    "(Landroid/graphics/Typeface;)V",
                    &[JValue::Object(&tf)],
                );
            }
        }

        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Set the font family of a TextView.
pub fn set_font_family(handle: i64, family_ptr: *const u8) {
    let family = str_from_header(family_ptr);
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);

        let family_name = match family {
            "monospace" | "monospaced" => "monospace",
            "system" | "default" => "sans-serif",
            "serif" => "serif",
            other => other,
        };

        let jfamily = env.new_string(family_name).expect("family string");
        // Typeface.create(String, int) → Typeface
        let typeface = env
            .call_static_method(
                "android/graphics/Typeface",
                "create",
                "(Ljava/lang/String;I)Landroid/graphics/Typeface;",
                &[JValue::Object(&jfamily), JValue::Int(0)], // NORMAL=0
            )
            .expect("Typeface.create")
            .l()
            .expect("typeface");

        let _ = env.call_method(
            view_ref.as_obj(),
            "setTypeface",
            "(Landroid/graphics/Typeface;)V",
            &[JValue::Object(&typeface)],
        );

        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Set text wrapping on a TextView.
/// max_width > 0: enable wrapping at that width. max_width <= 0: disable wrapping (single line).
pub fn set_wraps(handle: i64, max_width: f64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        if max_width > 0.0 {
            // Enable wrapping
            let _ = env.call_method(
                view_ref.as_obj(),
                "setSingleLine",
                "(Z)V",
                &[JValue::Bool(0)],
            );
            // Set max width in dp → px
            let max_px = super::dp_to_px(&mut env, max_width as f32);
            let _ = env.call_method(
                view_ref.as_obj(),
                "setMaxWidth",
                "(I)V",
                &[JValue::Int(max_px)],
            );
        } else {
            // Disable wrapping
            let _ = env.call_method(
                view_ref.as_obj(),
                "setSingleLine",
                "(Z)V",
                &[JValue::Bool(1)],
            );
        }
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Set whether a TextView is selectable.
pub fn set_selectable(handle: i64, selectable: bool) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let _ = env.call_method(
            view_ref.as_obj(),
            "setTextIsSelectable",
            "(Z)V",
            &[JValue::Bool(selectable as u8)],
        );
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Set text decoration on a TextView via Paint flags (issue #185 Phase B).
/// `decoration`: 0=none, 1=underline, 2=strikethrough. Uses the Paint
/// flag path on the view's `getPaint()` rather than building a
/// SpannableString — `Paint.UNDERLINE_TEXT_FLAG = 8`,
/// `Paint.STRIKE_THRU_TEXT_FLAG = 16`. Calls `invalidate()` so the
/// view repaints with the new flags.
pub fn set_decoration(handle: i64, decoration: i64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        if let Ok(paint_val) = env.call_method(
            view_ref.as_obj(),
            "getPaint",
            "()Landroid/text/TextPaint;",
            &[],
        ) {
            if let Ok(paint_obj) = paint_val.l() {
                if !paint_obj.is_null() {
                    let flag: i32 = match decoration {
                        1 => 8,
                        2 => 16,
                        _ => 0,
                    };
                    let _ = env.call_method(&paint_obj, "setFlags", "(I)V", &[JValue::Int(flag)]);
                    let _ = env.call_method(view_ref.as_obj(), "invalidate", "()V", &[]);
                }
            }
        }
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}
