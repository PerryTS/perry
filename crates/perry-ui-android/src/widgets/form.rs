//! Form / Section — LinearLayout containers with styling

use crate::jni_bridge;
use jni::objects::JValue;

fn str_from_header(ptr: *const u8) -> &'static str {
    crate::app::str_from_header(ptr)
}

/// Create a Form — vertical LinearLayout with padding.
pub fn create() -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);
    let layout = env
        .new_object(
            "android/widget/LinearLayout",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create LinearLayout");

    // Set vertical orientation
    let _ = env.call_method(&layout, "setOrientation", "(I)V", &[JValue::Int(1)]);

    // Set padding (16dp)
    let pad = super::dp_to_px(&mut env, 16.0);
    let _ = env.call_method(
        &layout,
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
        .new_global_ref(layout)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}

/// Create a Section — vertical LinearLayout with a title label.
pub fn section_create(title_ptr: *const u8) -> i64 {
    let title = str_from_header(title_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);

    // Outer layout
    let layout = env
        .new_object(
            "android/widget/LinearLayout",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create LinearLayout");
    let _ = env.call_method(&layout, "setOrientation", "(I)V", &[JValue::Int(1)]);

    let pad = super::dp_to_px(&mut env, 8.0);
    let _ = env.call_method(
        &layout,
        "setPadding",
        "(IIII)V",
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
                "android/widget/TextView",
                "(Landroid/content/Context;)V",
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create TextView");

        let jstr = env.new_string(title).expect("Failed to create JNI string");
        let _ = env.call_method(
            &title_view,
            "setText",
            "(Ljava/lang/CharSequence;)V",
            &[JValue::Object(&jstr)],
        );

        // Bold title
        let _ = env.call_method(
            &title_view,
            "setTypeface",
            "(Landroid/graphics/Typeface;I)V",
            &[
                JValue::Object(&jni::objects::JObject::null()),
                JValue::Int(1),
            ],
        );

        // setTextSize(TypedValue.COMPLEX_UNIT_SP=2, 14)
        let _ = env.call_method(
            &title_view,
            "setTextSize",
            "(IF)V",
            &[JValue::Int(2), JValue::Float(14.0)],
        );

        let bottom_pad = super::dp_to_px(&mut env, 4.0);
        let _ = env.call_method(
            &title_view,
            "setPadding",
            "(IIII)V",
            &[
                JValue::Int(0),
                JValue::Int(0),
                JValue::Int(0),
                JValue::Int(bottom_pad),
            ],
        );

        let _ = env.call_method(
            &layout,
            "addView",
            "(Landroid/view/View;)V",
            &[JValue::Object(&title_view)],
        );
    }

    let global = env
        .new_global_ref(layout)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}
