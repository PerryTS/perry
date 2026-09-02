//! ZStack — FrameLayout (overlapping children)

use crate::jni_bridge;
use jni::JValue;

pub fn create() -> i64 {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);

        let activity = super::get_activity(env);
        let frame_layout = env
            .new_object(
                jni::jni_str!("android/widget/FrameLayout"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create FrameLayout");

        let global =
            jni_bridge::new_global_ref(env, frame_layout).expect("Failed to create global ref");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}
