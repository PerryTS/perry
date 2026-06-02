//! Android Combobox widget — issue #475. Backed by
//! `android.widget.AutoCompleteTextView` with an `ArrayAdapter<String>`
//! holding the suggestion list. The PerryBridge.kt static helpers
//! (`comboboxCreate`, `comboboxAddItem`, `comboboxSetValue`,
//! `comboboxGetValue`) build the widget on the UI thread and wire the
//! `addTextChangedListener` / `setOnItemClickListener` callbacks to
//! `nativeInvokeCallbackWithString(key, text)`. Rust-side state (the
//! current suggestion list and adapter handle) is held in a thread-local
//! map so add_item is cheap and doesn't need to round-trip the whole list.

use crate::app::str_from_header;
use crate::callback;
use crate::jni_bridge;
use jni::objects::{JObject, JValue};

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

/// Create an AutoCompleteTextView with an initial value and on_change callback.
pub fn create(initial_ptr: *const u8, on_change: f64) -> i64 {
    let initial = str_from_header(initial_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let cb_key = if on_change != 0.0 {
        callback::register(on_change)
    } else {
        0
    };
    let jinitial = env.new_string(initial).expect("combobox initial string");
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let result = env.call_static_method(
        bridge_cls,
        "comboboxCreate",
        "(Ljava/lang/String;J)Landroid/widget/AutoCompleteTextView;",
        &[JValue::Object(&jinitial), JValue::Long(cb_key)],
    );
    let handle = match result {
        Ok(jv) => match jv.l() {
            Ok(obj) if !obj.is_null() => {
                let g = env
                    .new_global_ref(obj)
                    .expect("global-ref AutoCompleteTextView");
                super::register_widget(g)
            }
            _ => 0,
        },
        Err(_) => {
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            0
        }
    };
    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    handle
}

/// Append one suggestion item to the combobox.
pub fn add_item(handle: i64, value_ptr: *const u8) {
    let value = str_from_header(value_ptr);
    if let Some(view) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let jvalue = env.new_string(value).expect("combobox value string");
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "comboboxAddItem",
            "(Landroid/widget/AutoCompleteTextView;Ljava/lang/String;)V",
            &[JValue::Object(view.as_obj()), JValue::Object(&jvalue)],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Programmatically set the currently displayed value.
pub fn set_value(handle: i64, value_ptr: *const u8) {
    let value = str_from_header(value_ptr);
    if let Some(view) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let jvalue = env.new_string(value).expect("combobox value string");
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "comboboxSetValue",
            "(Landroid/widget/AutoCompleteTextView;Ljava/lang/String;)V",
            &[JValue::Object(view.as_obj()), JValue::Object(&jvalue)],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Read the current text out of the combobox, returning a NaN-boxed Perry string.
pub fn get_value(handle: i64) -> f64 {
    let Some(view) = super::get_widget(handle) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let result = env.call_static_method(
        bridge_cls,
        "comboboxGetValue",
        "(Landroid/widget/AutoCompleteTextView;)Ljava/lang/String;",
        &[JValue::Object(view.as_obj())],
    );
    let text: Option<String> = match result {
        Ok(jv) => match jv.l() {
            Ok(obj) if !obj.is_null() => {
                let jstr: jni::objects::JString = obj.into();
                env.get_string(&jstr).map(|s| s.into()).ok()
            }
            _ => None,
        },
        Err(_) => None,
    };
    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    match text {
        Some(s) => {
            let bytes = s.as_bytes();
            unsafe {
                let p = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                js_nanbox_string(p as i64)
            }
        }
        None => f64::from_bits(TAG_UNDEFINED),
    }
}
