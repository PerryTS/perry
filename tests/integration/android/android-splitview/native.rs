//! Device fixture: real production JNI bridge, widget registry and SplitView ABI.
mod ffi;
mod jni_bridge;
mod widgets;
include!("panic_boundary.rs");
use jni::{objects::JClass, EnvUnowned, JValue};
use std::sync::atomic::{AtomicI64, Ordering};
static SPLIT: AtomicI64 = AtomicI64::new(0);

fn text(label: &str) -> i64 {
    jni_bridge::with_env(|env| {
        let bridge = jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let class: &JClass = &bridge;
        let activity = env
            .call_static_method(
                class,
                jni::jni_str!("getActivity"),
                jni::jni_sig!("()Landroid/app/Activity;"),
                &[],
            )
            .unwrap()
            .l()
            .unwrap();
        let view = env
            .new_object(
                jni::jni_str!("android/widget/TextView"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .unwrap();
        let label = env.new_string(label).unwrap();
        env.call_method(
            &view,
            jni::jni_str!("setText"),
            jni::jni_sig!("(Ljava/lang/CharSequence;)V"),
            &[JValue::Object(&label)],
        )
        .unwrap();
        widgets::register_widget(jni_bridge::new_global_ref(env, view).unwrap())
    })
}

#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeInit(mut env: EnvUnowned, _: JClass) {
    jni_bridge::with_unowned_env(&mut env, |env| {
        jni_bridge::ensure_vm(env);
        jni_bridge::init_cache(env);
    });
}

#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeMain(_: EnvUnowned, _: JClass) {
    let split = ffi::perry_ui_splitview_create();
    assert!(split > 0);
    SPLIT.store(split, Ordering::Relaxed);
    ffi::perry_ui_splitview_add_child(split, text("LEFT"));
    ffi::perry_ui_splitview_add_child(split, text("RIGHT"));
    // Invalid handles must be harmless.
    ffi::perry_ui_splitview_add_child(0, 0);
    ffi::perry_ui_splitview_add_child(split, -1);
    jni_bridge::with_env(|env| {
        let bridge = jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let class: &JClass = &bridge;
        let view = widgets::get_widget(split).unwrap();
        env.call_static_method(
            class,
            jni::jni_str!("setContentView"),
            jni::jni_sig!("(Landroid/view/View;)V"),
            &[JValue::Object(view.as_obj())],
        )
        .unwrap();
    });
}

#[no_mangle]
pub extern "system" fn Java_com_perry_app_SplitViewTest_addThird(_: EnvUnowned, _: JClass) {
    ffi::perry_ui_splitview_add_child(SPLIT.load(Ordering::Relaxed), text("THIRD"));
}

#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeShutdown(_: EnvUnowned, _: JClass) {}
#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeMemoryPressure(
    _: EnvUnowned,
    _: JClass,
    _: i32,
) {
}

#[link(name = "log")]
extern "C" {
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}
