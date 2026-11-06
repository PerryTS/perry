//! Real production widget/FFI/JNI modules; callback shim records the Java-to-native boundary.
mod callback {
    pub fn register(value: f64) -> i64 {
        assert_eq!(value, 42.0);
        42
    }
}
mod app {
    pub(crate) use perry_ffi::copy_string_from_raw as str_from_header;
}
mod ffi;
mod jni_bridge;
mod widgets;
include!("panic_boundary.rs");
use jni::{objects::JClass, EnvUnowned, JValue};
use std::sync::atomic::{AtomicI64, Ordering};
static HANDLE: AtomicI64 = AtomicI64::new(0);

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

#[repr(C)]
struct Label {
    header: perry_ffi::StringHeader,
    data: [u8; 1],
}
fn add(handle: i64, label: u8, content: &str) {
    let s = Label {
        header: perry_ffi::StringHeader {
            utf16_len: 1,
            byte_len: 1,
            capacity: 1,
            refcount: 1,
            flags: 0,
        },
        data: [label],
    };
    unsafe {
        perry_ui_tabbar_add_tab(handle, (&s as *const Label) as i64, text(content));
    }
}
// Call through the compiler's public ABI, so a dropped third argument is observable.
extern "C" {
    fn perry_ui_tabbar_add_tab(handle: i64, label: i64, content: i64);
}
#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeMain(_: EnvUnowned, _: JClass) {
    let handle = ffi::perry_ui_tabbar_create(42.0);
    HANDLE.store(handle, Ordering::Relaxed);
    add(handle, b'A', "FIRST CONTENT");
    add(handle, b'B', "SECOND CONTENT");
    jni_bridge::with_env(|env| {
        let bridge = jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let class: &JClass = &bridge;
        env.call_static_method(
            class,
            jni::jni_str!("setContentView"),
            jni::jni_sig!("(Landroid/view/View;)V"),
            &[JValue::Object(
                widgets::get_widget(handle).unwrap().as_obj(),
            )],
        )
        .unwrap();
    });
}
#[no_mangle]
pub extern "system" fn Java_com_perry_app_TabBarTest_select(_: EnvUnowned, _: JClass, index: i64) {
    ffi::perry_ui_tabbar_set_selected(HANDLE.load(Ordering::Relaxed), index);
}
#[no_mangle]
pub extern "system" fn Java_com_perry_app_TabBarTest_append(_: EnvUnowned, _: JClass) {
    add(HANDLE.load(Ordering::Relaxed), b'C', "THIRD CONTENT");
}
#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeInvokeCallback1(
    mut raw: EnvUnowned,
    _: JClass,
    key: i64,
    index: f64,
) {
    assert_eq!(key, 42);
    jni_bridge::with_unowned_env(&mut raw, |env| {
        env.call_static_method(
            jni::jni_str!("com/perry/app/TabBarTest"),
            jni::jni_str!("recordSelection"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(index as i32)],
        )
        .unwrap();
    });
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
