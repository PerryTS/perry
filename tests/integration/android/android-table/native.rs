//! Production Table/JNI/FFI modules with a controlled cell-render callback fixture.
mod ffi;
mod jni_bridge;
mod widgets;
include!("panic_boundary.rs");
mod app {
    pub(crate) use perry_ffi::copy_string_from_raw as str_from_header;
}
mod callback {
    // The production callback registry is unchanged; these keys stand in for JS closures.
    pub fn register(value: f64) -> i64 {
        value as i64
    }
    pub fn get(key: i64) -> Option<f64> {
        Some(key as f64)
    }
}
use jni::{objects::JClass, EnvUnowned, JValue};
use std::sync::atomic::{AtomicI64, Ordering};
static TABLE: AtomicI64 = AtomicI64::new(0);
static RENDERS: AtomicI64 = AtomicI64::new(0);

#[no_mangle]
pub extern "C" fn js_nanbox_get_pointer(value: f64) -> i64 {
    value as i64
}
#[no_mangle]
pub extern "C" fn js_closure_call2(closure: *const u8, row: f64, col: f64) -> f64 {
    assert_eq!(closure as usize, 42);
    RENDERS.fetch_add(1, Ordering::Relaxed);
    let handle = jni_bridge::with_env(|env| {
        let obj = env
            .call_static_method(
                jni::jni_str!("com/perry/app/TableTest"),
                jni::jni_str!("renderCell"),
                jni::jni_sig!("(II)Landroid/view/View;"),
                &[JValue::Int(row as i32), JValue::Int(col as i32)],
            )
            .unwrap()
            .l()
            .unwrap();
        widgets::register_widget(jni_bridge::new_global_ref(env, obj).unwrap())
    });
    f64::from_bits(0x7FFD_0000_0000_0000 | handle as u64)
}
#[no_mangle]
pub unsafe extern "C" fn js_string_from_bytes(bytes: *const u8, len: i64) -> *const u8 {
    let bytes = std::slice::from_raw_parts(bytes, len as usize);
    let text = std::str::from_utf8(bytes).unwrap();
    // Leak fixture strings for the short device run; the real runtime owns/GCs these.
    let words = (std::mem::size_of::<perry_ffi::StringHeader>() + bytes.len() + 3) / 4;
    let data = Box::leak(vec![0u32; words].into_boxed_slice())
        .as_mut_ptr()
        .cast::<u8>();
    data.cast::<perry_ffi::StringHeader>()
        .write(perry_ffi::StringHeader {
            utf16_len: text.encode_utf16().count() as u32,
            byte_len: len as u32,
            capacity: len as u32,
            refcount: 1,
            flags: 0,
        });
    std::ptr::copy_nonoverlapping(
        bytes.as_ptr(),
        data.add(std::mem::size_of::<perry_ffi::StringHeader>()),
        bytes.len(),
    );
    data
}
fn string(text: &str) -> i64 {
    unsafe { js_string_from_bytes(text.as_ptr(), text.len() as i64) as i64 }
}
#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeInit(mut raw: EnvUnowned, _: JClass) {
    jni_bridge::with_unowned_env(&mut raw, |env| {
        jni_bridge::ensure_vm(env);
        jni_bridge::init_cache(env);
    });
}
#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeMain(_: EnvUnowned, _: JClass) {
    let handle = ffi::perry_ui_table_create(3.0, 4.0, 42.0);
    TABLE.store(handle, Ordering::Relaxed);
    if handle == 0 {
        return;
    } // Baseline is observed by the test, not a native assertion/crash.
    for col in 0..4 {
        ffi::perry_ui_table_set_column_header(handle, col, string(&format!("Header {col}")));
        ffi::perry_ui_table_set_column_width(handle, col, 80.0 + 20.0 * col as f64);
    }
    ffi::perry_ui_table_set_on_row_select(handle, 43.0);
    ffi::perry_ui_table_set_on_sort_change(handle, 44.0);
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
pub extern "system" fn Java_com_perry_app_TableTest_query(
    _: EnvUnowned,
    _: JClass,
    kind: i32,
    index: i64,
) -> i64 {
    let h = TABLE.load(Ordering::Relaxed);
    match kind {
        0 => h,
        1 => ffi::perry_ui_table_get_selected_row(h),
        2 => ffi::perry_ui_table_get_selected_rows_count(h),
        3 => ffi::perry_ui_table_get_selected_row_at(h, index),
        4 => RENDERS.load(Ordering::Relaxed),
        5 => {
            let p = ffi::perry_ui_table_get_filter_text(h);
            i64::from(unsafe { app::str_from_header(p as i64 as *const u8) } == "café ☕")
        }
        _ => -100,
    }
}
#[no_mangle]
pub extern "system" fn Java_com_perry_app_TableTest_update(
    _: EnvUnowned,
    _: JClass,
    kind: i32,
    value: i64,
) {
    let h = TABLE.load(Ordering::Relaxed);
    match kind {
        0 => ffi::perry_ui_table_update_row_count(h, value),
        1 => ffi::perry_ui_table_set_allows_multiple_selection(h, value),
        2 => ffi::perry_ui_table_set_filter_text(h, string("café ☕")),
        3 => ffi::perry_ui_table_set_column_width(h, 1, value as f64),
        _ => {}
    }
}
#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeInvokeCallback1(
    mut raw: EnvUnowned,
    _: JClass,
    key: i64,
    row: f64,
) {
    assert_eq!(key, 43);
    jni_bridge::with_unowned_env(&mut raw, |env| {
        env.call_static_method(
            jni::jni_str!("com/perry/app/TableTest"),
            jni::jni_str!("selected"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(row as i32)],
        )
        .unwrap();
    });
}
#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeInvokeCallback2(
    mut raw: EnvUnowned,
    _: JClass,
    key: i64,
    col: f64,
    asc: f64,
) {
    assert_eq!(key, 44);
    jni_bridge::with_unowned_env(&mut raw, |env| {
        env.call_static_method(
            jni::jni_str!("com/perry/app/TableTest"),
            jni::jni_str!("sorted"),
            jni::jni_sig!("(ID)V"),
            &[JValue::Int(col as i32), JValue::Double(asc)],
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
