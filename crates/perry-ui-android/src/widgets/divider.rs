use crate::jni_bridge;
use jni::objects::JValue;

/// Create a horizontal divider (1dp height View with separator color).
pub fn create() -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);
    let activity = super::get_activity(&mut env);

    let view = env
        .new_object(
            "android/view/View",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create View");

    // Light gray separator color (0xFFCCCCCC)
    let color: i32 = 0xFFCCCCCCu32 as i32;
    let _ = env.call_method(&view, "setBackgroundColor", "(I)V", &[JValue::Int(color)]);

    // 1dp height, MATCH_PARENT width
    let height_px = super::dp_to_px(&mut env, 1.0);
    let params = env
        .new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(II)V",
            &[JValue::Int(-1), JValue::Int(height_px)], // MATCH_PARENT, 1dp
        )
        .expect("Failed to create LayoutParams");
    let _ = env.call_method(
        &view,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&params)],
    );

    let global = env
        .new_global_ref(view)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}
