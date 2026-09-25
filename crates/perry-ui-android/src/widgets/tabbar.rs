use crate::{callback, jni_bridge};
use jni::{objects::JObject, JValue};

pub fn create(on_select: f64) -> i64 {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);
        let bridge = jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let class: &jni::objects::JClass = &bridge;
        let view = env
            .call_static_method(
                class,
                jni::jni_str!("createTabBar"),
                jni::jni_sig!("(J)Landroid/view/View;"),
                &[JValue::Long(callback::register(on_select))],
            )
            .expect("Failed to create TabBar")
            .l()
            .expect("TabBar is not an object");
        let global = jni_bridge::new_global_ref(env, view).expect("Failed to retain TabBar");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
        handle
    })
}

pub fn add_tab(parent: i64, label_ptr: *const u8, content: i64) {
    let (Some(parent), Some(content)) = (super::get_widget(parent), super::get_widget(content))
    else {
        return;
    };
    let label = unsafe { crate::app::str_from_header(label_ptr) };
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);
        let bridge = jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let class: &jni::objects::JClass = &bridge;
        let label = env.new_string(label).expect("Failed to create tab title");
        env.call_static_method(
            class,
            jni::jni_str!("tabBarAddTab"),
            jni::jni_sig!("(Landroid/view/View;Ljava/lang/String;Landroid/view/View;)V"),
            &[
                JValue::Object(parent.as_obj()),
                JValue::Object(&label),
                JValue::Object(content.as_obj()),
            ],
        )
        .expect("Failed to add tab content");
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
    });
}

pub fn set_selected(parent: i64, index: i64) {
    let Some(parent) = super::get_widget(parent) else {
        return;
    };
    // Reject before narrowing to Android's index type.
    let Ok(index) = i32::try_from(index) else {
        return;
    };
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);
        let bridge = jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let class: &jni::objects::JClass = &bridge;
        env.call_static_method(
            class,
            jni::jni_str!("tabBarSetSelected"),
            jni::jni_sig!("(Landroid/view/View;I)V"),
            &[JValue::Object(parent.as_obj()), JValue::Int(index)],
        )
        .expect("Failed to select tab");
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
    });
}
