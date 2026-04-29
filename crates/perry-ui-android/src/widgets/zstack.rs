//! ZStack — FrameLayout (overlapping children)

use crate::jni_bridge;
use jni::objects::JValue;

pub fn create() -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);
    let frame_layout = env
        .new_object(
            "android/widget/FrameLayout",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create FrameLayout");

    let global = env
        .new_global_ref(frame_layout)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}
