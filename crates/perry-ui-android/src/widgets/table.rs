//! Android Table: Java owns view/selection state; callback keys retain GC-scanned closures.
use crate::{callback, jni_bridge};
use jni::{
    objects::{JClass, JObject, JString},
    Env, EnvUnowned, JValue,
};
extern "C" {
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call2(closure: *const u8, row: f64, col: f64) -> f64;
    fn js_string_from_bytes(bytes: *const u8, len: i64) -> *const u8;
}
fn with_table<R>(handle: i64, default: R, f: impl FnOnce(&mut Env, &JObject) -> R) -> R {
    let Some(view) = super::get_widget(handle) else {
        return default;
    };
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);
        let result = f(env, view.as_obj());
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
        result
    })
}
pub fn create(rows: f64, columns: f64, render: f64) -> i64 {
    if !rows.is_finite()
        || !columns.is_finite()
        || rows < 0.0
        || columns < 0.0
        || rows > i32::MAX as f64
        || columns > i32::MAX as f64
    {
        return 0;
    }
    let key = callback::register(render);
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);
        let bridge = jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let class: &JClass = &bridge;
        let view = env
            .call_static_method(
                class,
                jni::jni_str!("createTable"),
                jni::jni_sig!("(IIJ)Landroid/view/View;"),
                &[
                    JValue::Int(rows as i32),
                    JValue::Int(columns as i32),
                    JValue::Long(key),
                ],
            )
            .expect("Failed to create Table")
            .l()
            .unwrap();
        let handle = super::register_widget(jni_bridge::new_global_ref(env, view).unwrap());
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
        handle
    })
}

#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeTableRenderCell(
    mut raw: EnvUnowned,
    _: JClass,
    key: i64,
    row: i32,
    col: i32,
) -> jni::sys::jobject {
    crate::catch_panic(
        "Table renderCell",
        std::panic::AssertUnwindSafe(|| {
            let Some(closure) = callback::get(key) else {
                return 0;
            };
            let value = unsafe {
                js_closure_call2(
                    js_nanbox_get_pointer(closure) as *const u8,
                    row as f64,
                    col as f64,
                )
            };
            let Some(view) = super::get_widget(value.to_bits() as i64) else {
                return 0;
            };
            jni_bridge::with_unowned_env(&mut raw, |env| {
                env.new_local_ref(view.as_obj()).unwrap().into_raw() as i64
            })
        }),
    ) as jni::sys::jobject
}

pub fn set_column_header(handle: i64, col: i64, title: i64) {
    let Ok(col) = i32::try_from(col) else {
        return;
    };
    let title = unsafe { crate::app::str_from_header(title as *const u8) };
    with_table(handle, (), |env, view| {
        let text = env.new_string(title).unwrap();
        env.call_method(
            view,
            jni::jni_str!("setHeader"),
            jni::jni_sig!("(ILjava/lang/String;)V"),
            &[JValue::Int(col), JValue::Object(&text)],
        )
        .expect("Table setHeader failed");
    })
}

pub fn set_column_width(handle: i64, col: i64, width: f64) {
    let Ok(col) = i32::try_from(col) else {
        return;
    };
    with_table(handle, (), |env, view| {
        env.call_method(
            view,
            jni::jni_str!("setColumnWidth"),
            jni::jni_sig!("(ID)V"),
            &[JValue::Int(col), JValue::Double(width)],
        )
        .expect("Table setColumnWidth failed");
    })
}

pub fn update_row_count(handle: i64, count: i64) {
    let Ok(count) = i32::try_from(count) else {
        return;
    };
    with_table(handle, (), |env, view| {
        env.call_method(
            view,
            jni::jni_str!("updateRows"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(count)],
        )
        .expect("Table updateRows failed");
    })
}

pub fn set_on_row_select(handle: i64, closure: f64) {
    let key = callback::register(closure);
    with_table(handle, (), |env, view| {
        env.call_method(
            view,
            jni::jni_str!("setSelectionCallback"),
            jni::jni_sig!("(J)V"),
            &[JValue::Long(key)],
        )
        .expect("Table setSelectionCallback failed");
    })
}

pub fn get_selected_row(handle: i64) -> i64 {
    with_table(handle, -1, |env, view| {
        env.call_method(
            view,
            jni::jni_str!("selectedRow"),
            jni::jni_sig!("()I"),
            &[],
        )
        .expect("Table selectedRow failed")
        .i()
        .unwrap() as i64
    })
}

pub fn set_on_sort_change(handle: i64, closure: f64) {
    let key = callback::register(closure);
    with_table(handle, (), |env, view| {
        env.call_method(
            view,
            jni::jni_str!("setSortCallback"),
            jni::jni_sig!("(J)V"),
            &[JValue::Long(key)],
        )
        .expect("Table setSortCallback failed");
    })
}

pub fn set_allows_multiple_selection(handle: i64, allow: i64) {
    with_table(handle, (), |env, view| {
        env.call_method(
            view,
            jni::jni_str!("setMultiple"),
            jni::jni_sig!("(Z)V"),
            &[JValue::Bool(allow != 0)],
        )
        .expect("Table setMultiple failed");
    })
}

pub fn get_selected_rows_count(handle: i64) -> i64 {
    with_table(handle, 0, |env, view| {
        env.call_method(
            view,
            jni::jni_str!("selectedCount"),
            jni::jni_sig!("()I"),
            &[],
        )
        .expect("Table selectedCount failed")
        .i()
        .unwrap() as i64
    })
}

pub fn get_selected_row_at(handle: i64, index: i64) -> i64 {
    let Ok(index) = i32::try_from(index) else {
        return -1;
    };
    with_table(handle, -1, |env, view| {
        env.call_method(
            view,
            jni::jni_str!("selectedAt"),
            jni::jni_sig!("(I)I"),
            &[JValue::Int(index)],
        )
        .expect("Table selectedAt failed")
        .i()
        .unwrap() as i64
    })
}

pub fn set_filter_text(handle: i64, title: i64) {
    let title = unsafe { crate::app::str_from_header(title as *const u8) };
    with_table(handle, (), |env, view| {
        let text = env.new_string(title).unwrap();
        env.call_method(
            view,
            jni::jni_str!("setFilter"),
            jni::jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(&text)],
        )
        .expect("Table setFilter failed");
    })
}

pub fn get_filter_text(handle: i64) -> i64 {
    let text = with_table(handle, String::new(), |env, view| {
        let obj = env
            .call_method(
                view,
                jni::jni_str!("getFilter"),
                jni::jni_sig!("()Ljava/lang/String;"),
                &[],
            )
            .unwrap()
            .l()
            .unwrap();
        let text = unsafe { JString::from_raw(env, obj.into_raw()) };
        text.try_to_string(env)
            .expect("Table filter is not a string")
    });
    unsafe { js_string_from_bytes(text.as_ptr(), text.len() as i64) as i64 }
}
