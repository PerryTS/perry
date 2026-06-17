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

/// Issue #707 — cap visible lines on an Android TextView.
/// `lines = 0` is unlimited (passing Integer.MAX_VALUE to setMaxLines).
pub fn set_number_of_lines(handle: i64, lines: i64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let n: i32 = if lines <= 0 {
            i32::MAX
        } else {
            (lines as i32).max(1)
        };
        let _ = env.call_method(view_ref.as_obj(), "setMaxLines", "(I)V", &[JValue::Int(n)]);
        // When a finite cap is set we also need an ellipsize mode so the
        // tail of the last visible line gets "…" instead of just being
        // clipped. Default to TruncateAt.END; users can override via
        // `set_truncation_mode` afterwards.
        if lines > 0 {
            apply_ellipsize(&mut env, view_ref.as_obj(), 3 /* END */);
        }
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Issue #707 — truncation mode on a TextView. Modes: 0=word-wrap (no
/// ellipsize), 1=head, 2=middle, 3=tail.
pub fn set_truncation_mode(handle: i64, mode: i64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        apply_ellipsize(&mut env, view_ref.as_obj(), mode);
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Set horizontal text alignment on a TextView (issue #3621).
/// Public `alignment` follows the canonical Perry/AppKit scheme:
/// 0=left, 1=right, 2=center, 3=justified, 4=natural. Maps to Android
/// `Gravity` horizontal flags, OR-ed with `CENTER_VERTICAL` so single-line
/// labels stay vertically centered (matching the Apple `UILabel` default).
/// For justified (3) we additionally request inter-word justification via
/// `setJustificationMode` (best-effort; API 26+).
pub fn set_text_alignment(handle: i64, alignment: i64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        // Gravity flags: LEFT=3, RIGHT=5, CENTER_HORIZONTAL=1,
        // START=0x00800003, CENTER_VERTICAL=16.
        const CENTER_VERTICAL: i32 = 16;
        let horiz: i32 = match alignment {
            1 => 5,           // right
            2 => 1,           // center horizontal
            3 => 3,           // justified → left gravity (+ justification mode below)
            4 => 0x0080_0003, // start (locale-natural)
            _ => 3,           // left
        };
        let _ = env.call_method(
            view_ref.as_obj(),
            "setGravity",
            "(I)V",
            &[JValue::Int(horiz | CENTER_VERTICAL)],
        );
        if alignment == 3 {
            // JUSTIFICATION_MODE_INTER_WORD = 1 (TextView, API 26+).
            let _ = env.call_method(
                view_ref.as_obj(),
                "setJustificationMode",
                "(I)V",
                &[JValue::Int(1)],
            );
        }
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Resolve `mode` to the Android `TextUtils.TruncateAt` enum and call
/// TextView.setEllipsize. `mode = 0` clears the ellipsize (null).
fn apply_ellipsize(env: &mut jni::JNIEnv, view: &JObject, mode: i64) {
    // TextUtils.TruncateAt is an enum: START=1, MIDDLE=2, END=3, MARQUEE=4.
    // Public ordinal values: START=0, MIDDLE=1, END=2, MARQUEE=3 in the enum
    // class, but the values() array maps the public API names. We resolve
    // by name to be robust.
    let name = match mode {
        1 => "START",
        2 => "MIDDLE",
        3 => "END",
        _ => "",
    };
    if name.is_empty() {
        // Clear: setEllipsize(null).
        let null_obj = JObject::null();
        let _ = env.call_method(
            view,
            "setEllipsize",
            "(Landroid/text/TextUtils$TruncateAt;)V",
            &[JValue::Object(&null_obj)],
        );
        return;
    }
    let enum_cls = match env.find_class("android/text/TextUtils$TruncateAt") {
        Ok(c) => c,
        Err(_) => return,
    };
    let java_name = match env.new_string(name) {
        Ok(s) => s,
        Err(_) => return,
    };
    let value = env.call_static_method(
        &enum_cls,
        "valueOf",
        "(Ljava/lang/String;)Landroid/text/TextUtils$TruncateAt;",
        &[JValue::Object(&java_name)],
    );
    let Ok(v) = value else { return };
    let Ok(enum_obj) = v.l() else { return };
    let _ = env.call_method(
        view,
        "setEllipsize",
        "(Landroid/text/TextUtils$TruncateAt;)V",
        &[JValue::Object(&enum_obj)],
    );
}
