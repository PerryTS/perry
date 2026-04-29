use crate::jni_bridge;
use jni::objects::JValue;

/// Create a flexible spacer (Space widget with weight=1 in LinearLayout).
pub fn create() -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);
    let activity = super::get_activity(&mut env);

    let space = env
        .new_object(
            "android/widget/Space",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create Space");

    // Give it weight=1 so it expands to fill available space in a LinearLayout.
    // LinearLayout.LayoutParams(0, 0, 1.0f) — width=0, height=0, weight=1
    let params = env
        .new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(IIF)V",
            &[JValue::Int(0), JValue::Int(0), JValue::Float(1.0)],
        )
        .expect("Failed to create LayoutParams");
    let _ = env.call_method(
        &space,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&params)],
    );

    let global = env
        .new_global_ref(space)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}
