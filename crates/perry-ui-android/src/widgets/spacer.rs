use crate::jni_bridge;
use jni::JValue;

/// Create a flexible spacer (Space widget with weight=1 in LinearLayout).
pub fn create() -> i64 {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);
        let activity = super::get_activity(env);

        let space = env
            .new_object(
                jni::jni_str!("android/widget/Space"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create Space");

        // Give it weight=1 so it expands to fill available space in a LinearLayout.
        // LinearLayout.LayoutParams(0, 0, 1.0f) — width=0, height=0, weight=1
        let params = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout$LayoutParams"),
                jni::jni_sig!("(IIF)V"),
                &[JValue::Int(0), JValue::Int(0), JValue::Float(1.0)],
            )
            .expect("Failed to create LayoutParams");
        let _ = env.call_method(
            &space,
            jni::jni_str!("setLayoutParams"),
            jni::jni_sig!("(Landroid/view/ViewGroup$LayoutParams;)V"),
            &[JValue::Object(&params)],
        );

        let global = jni_bridge::new_global_ref(env, space).expect("Failed to create global ref");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}
