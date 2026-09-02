use crate::jni_bridge;
use jni::JValue;

/// Create a horizontal divider (1dp height View with separator color).
pub fn create() -> i64 {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);
        let activity = super::get_activity(env);

        let view = env
            .new_object(
                jni::jni_str!("android/view/View"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create View");

        // Light gray separator color (0xFFCCCCCC)
        let color: i32 = 0xFFCCCCCCu32 as i32;
        let _ = env.call_method(
            &view,
            jni::jni_str!("setBackgroundColor"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(color)],
        );

        // 1dp height, MATCH_PARENT width
        let height_px = super::dp_to_px(env, 1.0);
        let params = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout$LayoutParams"),
                jni::jni_sig!("(II)V"),
                &[JValue::Int(-1), JValue::Int(height_px)], // MATCH_PARENT, 1dp
            )
            .expect("Failed to create LayoutParams");
        let _ = env.call_method(
            &view,
            jni::jni_str!("setLayoutParams"),
            jni::jni_sig!("(Landroid/view/ViewGroup$LayoutParams;)V"),
            &[JValue::Object(&params)],
        );

        let global = jni_bridge::new_global_ref(env, view).expect("Failed to create global ref");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}
