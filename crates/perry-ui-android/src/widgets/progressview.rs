//! ProgressView — Android ProgressBar

use crate::jni_bridge;
use jni::objects::JValue;

pub fn create() -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);

    // ProgressBar with horizontal style
    // Use the constructor with style: android.R.attr.progressBarStyleHorizontal = 0x01010078
    let progress_bar = env
        .new_object(
            "android/widget/ProgressBar",
            "(Landroid/content/Context;Landroid/util/AttributeSet;II)V",
            &[
                JValue::Object(&activity),
                JValue::Object(&jni::objects::JObject::null()),
                JValue::Int(0),
                JValue::Int(0x01010078), // android.R.attr.progressBarStyleHorizontal
            ],
        )
        .expect("Failed to create ProgressBar");

    let _ = env.call_method(&progress_bar, "setMax", "(I)V", &[JValue::Int(1000)]);
    let _ = env.call_method(
        &progress_bar,
        "setIndeterminate",
        "(Z)V",
        &[JValue::Bool(1)],
    );

    let global = env
        .new_global_ref(progress_bar)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}

pub fn set_value(handle: i64, value: f64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);

        if value < 0.0 {
            // Indeterminate
            let _ = env.call_method(
                view_ref.as_obj(),
                "setIndeterminate",
                "(Z)V",
                &[JValue::Bool(1)],
            );
        } else {
            let _ = env.call_method(
                view_ref.as_obj(),
                "setIndeterminate",
                "(Z)V",
                &[JValue::Bool(0)],
            );
            let progress = (value * 1000.0) as i32;
            let _ = env.call_method(
                view_ref.as_obj(),
                "setProgress",
                "(I)V",
                &[JValue::Int(progress)],
            );
        }

        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}
