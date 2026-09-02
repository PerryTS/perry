//! ProgressView — Android ProgressBar

use crate::jni_bridge;
use jni::JValue;

pub fn create() -> i64 {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);

        let activity = super::get_activity(env);

        // ProgressBar with horizontal style
        // Use the constructor with style: android.R.attr.progressBarStyleHorizontal = 0x01010078
        let progress_bar = env
            .new_object(
                jni::jni_str!("android/widget/ProgressBar"),
                jni::jni_sig!("(Landroid/content/Context;Landroid/util/AttributeSet;II)V"),
                &[
                    JValue::Object(&activity),
                    JValue::Object(&jni::objects::JObject::null()),
                    JValue::Int(0),
                    JValue::Int(0x01010078), // android.R.attr.progressBarStyleHorizontal
                ],
            )
            .expect("Failed to create ProgressBar");

        let _ = env.call_method(
            &progress_bar,
            jni::jni_str!("setMax"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(1000)],
        );
        let _ = env.call_method(
            &progress_bar,
            jni::jni_str!("setIndeterminate"),
            jni::jni_sig!("(Z)V"),
            &[JValue::Bool(true)],
        );

        let global =
            jni_bridge::new_global_ref(env, progress_bar).expect("Failed to create global ref");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}

pub fn set_value(handle: i64, value: f64) {
    if let Some(view_ref) = super::get_widget(handle) {
        jni_bridge::with_env(|env| {
            let _ = jni_bridge::push_local_frame(env, 8);

            if value < 0.0 {
                // Indeterminate
                let _ = env.call_method(
                    view_ref.as_obj(),
                    jni::jni_str!("setIndeterminate"),
                    jni::jni_sig!("(Z)V"),
                    &[JValue::Bool(true)],
                );
            } else {
                let _ = env.call_method(
                    view_ref.as_obj(),
                    jni::jni_str!("setIndeterminate"),
                    jni::jni_sig!("(Z)V"),
                    &[JValue::Bool(false)],
                );
                let progress = (value * 1000.0) as i32;
                let _ = env.call_method(
                    view_ref.as_obj(),
                    jni::jni_str!("setProgress"),
                    jni::jni_sig!("(I)V"),
                    &[JValue::Int(progress)],
                );
            }

            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
            }
        })
    }
}
