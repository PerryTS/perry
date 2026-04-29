use crate::jni_bridge;
use jni::objects::JValue;

/// Create a LinearLayout with HORIZONTAL orientation.
pub fn create(spacing: f64) -> i64 {
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

    // HORIZONTAL = 0
    let _ = env.call_method(&layout, "setOrientation", "(I)V", &[JValue::Int(0)]);

    // Set vertical centering: gravity = Gravity.CENTER_VERTICAL = 16
    let _ = env.call_method(&layout, "setGravity", "(I)V", &[JValue::Int(16)]);

    let spacing_px = super::dp_to_px(&mut env, spacing as f32);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "setLinearLayoutSpacing",
        "(Landroid/widget/LinearLayout;I)V",
        &[JValue::Object(&layout), JValue::Int(spacing_px)],
    );

    let params = env
        .new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(II)V",
            &[JValue::Int(-1), JValue::Int(-2)], // MATCH_PARENT, WRAP_CONTENT
        )
        .expect("Failed to create LayoutParams");
    let _ = env.call_method(
        &layout,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&params)],
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

/// Create a LinearLayout with HORIZONTAL orientation and custom padding.
pub fn create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64 {
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

    // HORIZONTAL = 0
    let _ = env.call_method(&layout, "setOrientation", "(I)V", &[JValue::Int(0)]);
    let _ = env.call_method(&layout, "setGravity", "(I)V", &[JValue::Int(16)]);

    let spacing_px = super::dp_to_px(&mut env, spacing as f32);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "setLinearLayoutSpacing",
        "(Landroid/widget/LinearLayout;I)V",
        &[JValue::Object(&layout), JValue::Int(spacing_px)],
    );

    let top_px = super::dp_to_px(&mut env, top as f32);
    let left_px = super::dp_to_px(&mut env, left as f32);
    let bottom_px = super::dp_to_px(&mut env, bottom as f32);
    let right_px = super::dp_to_px(&mut env, right as f32);
    let _ = env.call_method(
        &layout,
        "setPadding",
        "(IIII)V",
        &[
            JValue::Int(left_px),
            JValue::Int(top_px),
            JValue::Int(right_px),
            JValue::Int(bottom_px),
        ],
    );

    let params = env
        .new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(II)V",
            &[JValue::Int(-1), JValue::Int(-2)],
        )
        .expect("Failed to create LayoutParams");
    let _ = env.call_method(
        &layout,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&params)],
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
