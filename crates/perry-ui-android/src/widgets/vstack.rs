use crate::jni_bridge;
use jni::JValue;

/// Create a LinearLayout with VERTICAL orientation.
pub fn create(spacing: f64) -> i64 {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);
        let activity = super::get_activity(env);

        let layout = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create LinearLayout");

        // VERTICAL = 1
        let _ = env.call_method(
            &layout,
            jni::jni_str!("setOrientation"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(1)],
        );

        // Set spacing between children via a custom divider or padding approach.
        // LinearLayout doesn't have a direct "spacing" API, but we can set
        // showDividers + dividerPadding, or we handle spacing in add_child.
        // For simplicity, store spacing and apply as margins on children via PerryBridge.
        let spacing_px = super::dp_to_px(env, spacing as f32);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("setLinearLayoutSpacing"),
            jni::jni_sig!("(Landroid/widget/LinearLayout;I)V"),
            &[JValue::Object(&layout), JValue::Int(spacing_px)],
        );

        // No default padding — matches macOS/iOS behavior (VStack has zero insets)

        // LayoutParams: MATCH_PARENT width, WRAP_CONTENT height
        // Height defaults to WRAP_CONTENT so VStacks don't expand to fill parents.
        // Use widgetMatchParentHeight() to opt-in to filling (e.g. root appBody).
        let params = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout$LayoutParams"),
                jni::jni_sig!("(II)V"),
                &[JValue::Int(-1), JValue::Int(-2)], // MATCH_PARENT width, WRAP_CONTENT height
            )
            .expect("Failed to create LayoutParams");
        let _ = env.call_method(
            &layout,
            jni::jni_str!("setLayoutParams"),
            jni::jni_sig!("(Landroid/view/ViewGroup$LayoutParams;)V"),
            &[JValue::Object(&params)],
        );

        let global = jni_bridge::new_global_ref(env, layout).expect("Failed to create global ref");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}

/// Create a LinearLayout with VERTICAL orientation and custom padding (insets).
pub fn create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64 {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);
        let activity = super::get_activity(env);

        let layout = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create LinearLayout");

        // VERTICAL = 1
        let _ = env.call_method(
            &layout,
            jni::jni_str!("setOrientation"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(1)],
        );

        let spacing_px = super::dp_to_px(env, spacing as f32);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("setLinearLayoutSpacing"),
            jni::jni_sig!("(Landroid/widget/LinearLayout;I)V"),
            &[JValue::Object(&layout), JValue::Int(spacing_px)],
        );

        // Set custom padding (convert dp to px)
        let top_px = super::dp_to_px(env, top as f32);
        let left_px = super::dp_to_px(env, left as f32);
        let bottom_px = super::dp_to_px(env, bottom as f32);
        let right_px = super::dp_to_px(env, right as f32);
        let _ = env.call_method(
            &layout,
            jni::jni_str!("setPadding"),
            jni::jni_sig!("(IIII)V"),
            &[
                JValue::Int(left_px),
                JValue::Int(top_px),
                JValue::Int(right_px),
                JValue::Int(bottom_px),
            ],
        );

        let params = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout$LayoutParams"),
                jni::jni_sig!("(II)V"),
                &[JValue::Int(-1), JValue::Int(-2)],
            )
            .expect("Failed to create LayoutParams");
        let _ = env.call_method(
            &layout,
            jni::jni_str!("setLayoutParams"),
            jni::jni_sig!("(Landroid/view/ViewGroup$LayoutParams;)V"),
            &[JValue::Object(&params)],
        );

        let global = jni_bridge::new_global_ref(env, layout).expect("Failed to create global ref");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}
