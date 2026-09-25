use crate::jni_bridge;
use jni::{objects::JObject, JValue};

pub fn create() -> i64 {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);
        let bridge = jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let class: &jni::objects::JClass = &bridge;
        let view = env
            .call_static_method(
                class,
                jni::jni_str!("createSplitView"),
                jni::jni_sig!("()Landroid/view/View;"),
                &[],
            )
            .expect("Failed to create SplitView")
            .l()
            .expect("SplitView is not an object");
        let global = jni_bridge::new_global_ref(env, view).expect("Failed to retain SplitView");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
        handle
    })
}

pub fn add_child(parent: i64, child: i64) {
    let (Some(parent), Some(child)) = (super::get_widget(parent), super::get_widget(child)) else {
        return;
    };
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);
        let bridge = jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let class: &jni::objects::JClass = &bridge;
        env.call_static_method(
            class,
            jni::jni_str!("splitViewAddChild"),
            jni::jni_sig!("(Landroid/view/View;Landroid/view/View;)V"),
            &[
                JValue::Object(parent.as_obj()),
                JValue::Object(child.as_obj()),
            ],
        )
        .expect("Failed to add SplitView child");
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
    });
}
