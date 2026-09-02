//! Form / Section — LinearLayout containers with styling

use crate::jni_bridge;
use jni::JValue;

use perry_ffi::copy_string_from_raw as str_from_header;

/// Create a Form — vertical LinearLayout with padding.
pub fn create() -> i64 {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);

        let activity = super::get_activity(env);
        let layout = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create LinearLayout");

        // Set vertical orientation
        let _ = env.call_method(
            &layout,
            jni::jni_str!("setOrientation"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(1)],
        );

        // Set padding (16dp)
        let pad = super::dp_to_px(env, 16.0);
        let _ = env.call_method(
            &layout,
            jni::jni_str!("setPadding"),
            jni::jni_sig!("(IIII)V"),
            &[
                JValue::Int(pad),
                JValue::Int(pad),
                JValue::Int(pad),
                JValue::Int(pad),
            ],
        );

        let global = jni_bridge::new_global_ref(env, layout).expect("Failed to create global ref");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}

/// Create a Section — vertical LinearLayout with a title label.
pub fn section_create(title_ptr: *const u8) -> i64 {
    let title = unsafe { str_from_header(title_ptr) };
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);

        let activity = super::get_activity(env);

        // Outer layout
        let layout = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create LinearLayout");
        let _ = env.call_method(
            &layout,
            jni::jni_str!("setOrientation"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(1)],
        );

        let pad = super::dp_to_px(env, 8.0);
        let _ = env.call_method(
            &layout,
            jni::jni_str!("setPadding"),
            jni::jni_sig!("(IIII)V"),
            &[
                JValue::Int(pad),
                JValue::Int(pad),
                JValue::Int(pad),
                JValue::Int(pad),
            ],
        );

        // Add title label
        if !title.is_empty() {
            let title_view = env
                .new_object(
                    jni::jni_str!("android/widget/TextView"),
                    jni::jni_sig!("(Landroid/content/Context;)V"),
                    &[JValue::Object(&activity)],
                )
                .expect("Failed to create TextView");

            let jstr = env.new_string(title).expect("Failed to create JNI string");
            let _ = env.call_method(
                &title_view,
                jni::jni_str!("setText"),
                jni::jni_sig!("(Ljava/lang/CharSequence;)V"),
                &[JValue::Object(&jstr)],
            );

            // Bold title
            let _ = env.call_method(
                &title_view,
                jni::jni_str!("setTypeface"),
                jni::jni_sig!("(Landroid/graphics/Typeface;I)V"),
                &[
                    JValue::Object(&jni::objects::JObject::null()),
                    JValue::Int(1),
                ],
            );

            // setTextSize(TypedValue.COMPLEX_UNIT_SP=2, 14)
            let _ = env.call_method(
                &title_view,
                jni::jni_str!("setTextSize"),
                jni::jni_sig!("(IF)V"),
                &[JValue::Int(2), JValue::Float(14.0)],
            );

            let bottom_pad = super::dp_to_px(env, 4.0);
            let _ = env.call_method(
                &title_view,
                jni::jni_str!("setPadding"),
                jni::jni_sig!("(IIII)V"),
                &[
                    JValue::Int(0),
                    JValue::Int(0),
                    JValue::Int(0),
                    JValue::Int(bottom_pad),
                ],
            );

            let _ = env.call_method(
                &layout,
                jni::jni_str!("addView"),
                jni::jni_sig!("(Landroid/view/View;)V"),
                &[JValue::Object(&title_view)],
            );
        }

        let global = jni_bridge::new_global_ref(env, layout).expect("Failed to create global ref");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}
