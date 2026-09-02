//! QR Code widget for Android.
//! Renders the QR data as a centered text label for now.
//! A full QR code renderer (e.g. via ZXing or a Rust crate) can replace this later.

use crate::jni_bridge;
use jni::JValue;

use perry_ffi::copy_string_from_raw as str_from_header;

/// Create a QR code widget displaying the given data string.
/// `size` is the display width/height in dp (QR codes are square).
/// Returns widget handle.
pub fn create(data_ptr: *const u8, size: f64) -> i64 {
    let data_str = unsafe { str_from_header(data_ptr) };
    let display_size = if size > 0.0 { size } else { 200.0 };

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);

        let activity = super::get_activity(env);

        // Create a TextView styled as a QR code placeholder
        let text_view = env
            .new_object(
                jni::jni_str!("android/widget/TextView"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create TextView for QR code");

        // Set the data text
        let display_text = if data_str.is_empty() { "QR" } else { &data_str };
        let jstr = env.new_string(display_text).expect("QR text string");
        let _ = env.call_method(
            &text_view,
            jni::jni_str!("setText"),
            jni::jni_sig!("(Ljava/lang/CharSequence;)V"),
            &[JValue::Object(&jstr)],
        );

        // Center text
        let _ = env.call_method(
            &text_view,
            jni::jni_str!("setGravity"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(0x11)],
        ); // Gravity.CENTER

        // Monospace font for code-like appearance
        let font_name = env.new_string("monospace").expect("font name");
        let tf = env.call_static_method(
            jni::jni_str!("android/graphics/Typeface"),
            jni::jni_str!("create"),
            jni::jni_sig!("(Ljava/lang/String;I)Landroid/graphics/Typeface;"),
            &[JValue::Object(&font_name), JValue::Int(1)],
        ); // BOLD=1
        if let Ok(tf_val) = tf {
            if let Ok(tf_obj) = tf_val.l() {
                let _ = env.call_method(
                    &text_view,
                    jni::jni_str!("setTypeface"),
                    jni::jni_sig!("(Landroid/graphics/Typeface;)V"),
                    &[JValue::Object(&tf_obj)],
                );
            }
        }

        // Small text size
        let _ = env.call_method(
            &text_view,
            jni::jni_str!("setTextSize"),
            jni::jni_sig!("(IF)V"),
            &[JValue::Int(2), JValue::Float(10.0)],
        ); // COMPLEX_UNIT_SP=2

        // Set a border-like background
        let gd = env
            .new_object(
                jni::jni_str!("android/graphics/drawable/GradientDrawable"),
                jni::jni_sig!("()V"),
                &[],
            )
            .expect("GradientDrawable");
        let _ = env.call_method(
            &gd,
            jni::jni_str!("setColor"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(0xFFFFFFFFu32 as i32)],
        ); // White background
        let _ = env.call_method(
            &gd,
            jni::jni_str!("setStroke"),
            jni::jni_sig!("(II)V"),
            &[JValue::Int(2), JValue::Int(0xFF000000u32 as i32)],
        ); // Black border
        let corner_px = super::dp_to_px(env, 4.0);
        let _ = env.call_method(
            &gd,
            jni::jni_str!("setCornerRadius"),
            jni::jni_sig!("(F)V"),
            &[JValue::Float(corner_px as f32)],
        );
        let _ = env.call_method(
            &text_view,
            jni::jni_str!("setBackground"),
            jni::jni_sig!("(Landroid/graphics/drawable/Drawable;)V"),
            &[JValue::Object(&gd)],
        );

        // Text color black
        let _ = env.call_method(
            &text_view,
            jni::jni_str!("setTextColor"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(0xFF000000u32 as i32)],
        );

        // Set fixed size
        let size_px = super::dp_to_px(env, display_size as f32);
        let params = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout$LayoutParams"),
                jni::jni_sig!("(II)V"),
                &[JValue::Int(size_px), JValue::Int(size_px)],
            )
            .expect("LayoutParams");
        let _ = env.call_method(
            &text_view,
            jni::jni_str!("setLayoutParams"),
            jni::jni_sig!("(Landroid/view/ViewGroup$LayoutParams;)V"),
            &[JValue::Object(&params)],
        );

        // Padding
        let pad = super::dp_to_px(env, 8.0);
        let _ = env.call_method(
            &text_view,
            jni::jni_str!("setPadding"),
            jni::jni_sig!("(IIII)V"),
            &[
                JValue::Int(pad),
                JValue::Int(pad),
                JValue::Int(pad),
                JValue::Int(pad),
            ],
        );

        let global =
            jni_bridge::new_global_ref(env, text_view).expect("Failed to create global ref");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}

/// Update the QR code content of an existing widget.
pub fn set_data(handle: i64, data_ptr: *const u8) {
    let data_str = unsafe { str_from_header(data_ptr) };
    if let Some(view_ref) = super::get_widget(handle) {
        jni_bridge::with_env(|env| {
            let _ = jni_bridge::push_local_frame(env, 8);
            let jstr = env.new_string(data_str).expect("QR text string");
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
