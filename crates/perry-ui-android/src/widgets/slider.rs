use crate::callback;
use crate::jni_bridge;
use jni::JValue;

/// Set the value of an existing SeekBar widget.
pub fn set_value(handle: i64, value: f64) {
    if let Some(view_ref) = super::get_widget(handle) {
        jni_bridge::with_env(|env| {
            let _ = jni_bridge::push_local_frame(env, 8);
            // SeekBar uses integer progress; we scale based on the stored range.
            // The range is set at creation: max = (max - min) * 100.
            // So progress = (value - min) * 100.
            // We store min in a tag via PerryBridge.
            let bridge_class =
                jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
            let bridge_cls: &jni::objects::JClass = &bridge_class;
            let _ = env.call_static_method(
                bridge_cls,
                jni::jni_str!("setSeekBarValue"),
                jni::jni_sig!("(Landroid/widget/SeekBar;D)V"),
                &[JValue::Object(view_ref.as_obj()), JValue::Double(value)],
            );
            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
            }
        })
    }
}

/// Create a SeekBar with min, max, initial values and onChange callback.
pub fn create(min: f64, max: f64, initial: f64, on_change: f64) -> i64 {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);
        let activity = super::get_activity(env);

        let seek_bar = env
            .new_object(
                jni::jni_str!("android/widget/SeekBar"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create SeekBar");

        // SeekBar uses integer progress values.
        // We use a scale factor of 100 for float precision.
        // max = (max - min) * 100
        let range = ((max - min) * 100.0) as i32;
        let _ = env.call_method(
            &seek_bar,
            jni::jni_str!("setMax"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(range)],
        );

        // Set initial progress
        let initial_progress = ((initial - min) * 100.0) as i32;
        let _ = env.call_method(
            &seek_bar,
            jni::jni_str!("setProgress"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(initial_progress)],
        );

        // MATCH_PARENT width, WRAP_CONTENT height
        let params = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout$LayoutParams"),
                jni::jni_sig!("(II)V"),
                &[JValue::Int(-1), JValue::Int(-2)],
            )
            .expect("Failed to create LayoutParams");
        let _ = env.call_method(
            &seek_bar,
            jni::jni_str!("setLayoutParams"),
            jni::jni_sig!("(Landroid/view/ViewGroup$LayoutParams;)V"),
            &[JValue::Object(&params)],
        );

        // Register callback and set up OnSeekBarChangeListener via PerryBridge
        let cb_key = callback::register(on_change);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("setSeekBarCallback"),
            jni::jni_sig!("(Landroid/widget/SeekBar;JDD)V"),
            &[
                JValue::Object(&seek_bar),
                JValue::Long(cb_key),
                JValue::Double(min),
                JValue::Double(max),
            ],
        );

        let global =
            jni_bridge::new_global_ref(env, seek_bar).expect("Failed to create global ref");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}
