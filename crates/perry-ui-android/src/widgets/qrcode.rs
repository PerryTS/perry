//! QR Code widget for Android.
//! Renders the QR data as a centered text label for now.
//! A full QR code renderer (e.g. via ZXing or a Rust crate) can replace this later.

use crate::jni_bridge;
use jni::objects::JValue;

fn str_from_header(ptr: *const u8) -> &'static str {
    crate::app::str_from_header(ptr)
}

/// Create a QR code widget displaying the given data string.
/// `size` is the display width/height in dp (QR codes are square).
/// Returns widget handle.
pub fn create(data_ptr: *const u8, size: f64) -> i64 {
    let data_str = str_from_header(data_ptr);
    let display_size = if size > 0.0 { size } else { 200.0 };

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);

    // Create a TextView styled as a QR code placeholder
    let text_view = env
        .new_object(
            "android/widget/TextView",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create TextView for QR code");

    // Set the data text
    let display_text = if data_str.is_empty() { "QR" } else { data_str };
    let jstr = env.new_string(display_text).expect("QR text string");
    let _ = env.call_method(
        &text_view,
        "setText",
        "(Ljava/lang/CharSequence;)V",
        &[JValue::Object(&jstr)],
    );

    // Center text
    let _ = env.call_method(&text_view, "setGravity", "(I)V", &[JValue::Int(0x11)]); // Gravity.CENTER

    // Monospace font for code-like appearance
    let font_name = env.new_string("monospace").expect("font name");
    let tf = env.call_static_method(
        "android/graphics/Typeface",
        "create",
        "(Ljava/lang/String;I)Landroid/graphics/Typeface;",
        &[JValue::Object(&font_name), JValue::Int(1)],
    ); // BOLD=1
    if let Ok(tf_val) = tf {
        if let Ok(tf_obj) = tf_val.l() {
            let _ = env.call_method(
                &text_view,
                "setTypeface",
                "(Landroid/graphics/Typeface;)V",
                &[JValue::Object(&tf_obj)],
            );
        }
    }

    // Small text size
    let _ = env.call_method(
        &text_view,
        "setTextSize",
        "(IF)V",
        &[JValue::Int(2), JValue::Float(10.0)],
    ); // COMPLEX_UNIT_SP=2

    // Set a border-like background
    let gd = env
        .new_object("android/graphics/drawable/GradientDrawable", "()V", &[])
        .expect("GradientDrawable");
    let _ = env.call_method(
        &gd,
        "setColor",
        "(I)V",
        &[JValue::Int(0xFFFFFFFFu32 as i32)],
    ); // White background
    let _ = env.call_method(
        &gd,
        "setStroke",
        "(II)V",
        &[JValue::Int(2), JValue::Int(0xFF000000u32 as i32)],
    ); // Black border
    let corner_px = super::dp_to_px(&mut env, 4.0);
    let _ = env.call_method(
        &gd,
        "setCornerRadius",
        "(F)V",
        &[JValue::Float(corner_px as f32)],
    );
    let _ = env.call_method(
        &text_view,
        "setBackground",
        "(Landroid/graphics/drawable/Drawable;)V",
        &[JValue::Object(&gd)],
    );

    // Text color black
    let _ = env.call_method(
        &text_view,
        "setTextColor",
        "(I)V",
        &[JValue::Int(0xFF000000u32 as i32)],
    );

    // Set fixed size
    let size_px = super::dp_to_px(&mut env, display_size as f32);
    let params = env
        .new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(II)V",
            &[JValue::Int(size_px), JValue::Int(size_px)],
        )
        .expect("LayoutParams");
    let _ = env.call_method(
        &text_view,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&params)],
    );

    // Padding
    let pad = super::dp_to_px(&mut env, 8.0);
    let _ = env.call_method(
        &text_view,
        "setPadding",
        "(IIII)V",
        &[
            JValue::Int(pad),
            JValue::Int(pad),
            JValue::Int(pad),
            JValue::Int(pad),
        ],
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

/// Update the QR code content of an existing widget.
pub fn set_data(handle: i64, data_ptr: *const u8) {
    let data_str = str_from_header(data_ptr);
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let jstr = env.new_string(data_str).expect("QR text string");
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
