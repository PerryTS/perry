//! Native Android wheel picker backed by `android.widget.NumberPicker`.

use crate::{callback, jni_bridge};
use jni::objects::{JObject, JValue};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static ITEMS: RefCell<HashMap<i64, Vec<String>>> = RefCell::new(HashMap::new());
}

use perry_ffi::copy_string_from_raw as str_from_header;

pub fn create(on_change: f64) -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(24);
    let activity = super::get_activity(&mut env);
    let picker = env
        .new_object(
            "android/widget/NumberPicker",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create NumberPicker");
    let _ = env.call_method(&picker, "setEnabled", "(Z)V", &[JValue::Bool(0)]);

    if on_change != 0.0 {
        let callback_key = callback::register(on_change);
        let bridge =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_class: &jni::objects::JClass = (&bridge).into();
        env.call_static_method(
            bridge_class,
            "setNumberPickerCallback",
            "(Landroid/widget/NumberPicker;J)V",
            &[JValue::Object(&picker), JValue::Long(callback_key)],
        )
        .expect("Failed to install NumberPicker callback");
    }

    let global = env
        .new_global_ref(picker)
        .expect("Failed to create NumberPicker global ref");
    let handle = super::register_widget(global);
    ITEMS.with(|m| m.borrow_mut().insert(handle, Vec::new()));
    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    handle
}

pub fn add_item(handle: i64, title_ptr: *const u8) {
    let title = unsafe { str_from_header(title_ptr) }.to_string();
    let items = ITEMS.with(|m| {
        let mut all = m.borrow_mut();
        let Some(items) = all.get_mut(&handle) else {
            return Vec::new();
        };
        items.push(title);
        items.clone()
    });
    if !items.is_empty() {
        refresh_items(handle, &items);
    }
}

fn refresh_items(handle: i64, items: &[String]) {
    let Some(view) = super::get_widget(handle) else {
        return;
    };
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(24 + items.len() as i32);
    let string_class = env.find_class("java/lang/String").expect("String class");
    let values = env
        .new_object_array(items.len() as i32, &string_class, &JObject::null())
        .expect("Failed to create NumberPicker values");
    for (index, item) in items.iter().enumerate() {
        let value = env.new_string(item).expect("Failed to create picker value");
        env.set_object_array_element(&values, index as i32, &value)
            .expect("Failed to store picker value");
    }

    // NumberPicker requires clearing displayedValues before its range changes.
    let null = JObject::null();
    let _ = env.call_method(
        view.as_obj(),
        "setDisplayedValues",
        "([Ljava/lang/String;)V",
        &[JValue::Object(&null)],
    );
    let _ = env.call_method(view.as_obj(), "setMinValue", "(I)V", &[JValue::Int(0)]);
    let _ = env.call_method(
        view.as_obj(),
        "setMaxValue",
        "(I)V",
        &[JValue::Int(items.len() as i32 - 1)],
    );
    let _ = env.call_method(
        view.as_obj(),
        "setDisplayedValues",
        "([Ljava/lang/String;)V",
        &[JValue::Object(&values)],
    );
    let _ = env.call_method(
        view.as_obj(),
        "setWrapSelectorWheel",
        "(Z)V",
        &[JValue::Bool((items.len() > 2) as u8)],
    );
    let _ = env.call_method(view.as_obj(), "setEnabled", "(Z)V", &[JValue::Bool(1)]);
    unsafe {
        env.pop_local_frame(&JObject::null());
    }
}

pub fn set_selected(handle: i64, index: i64) {
    let valid = ITEMS.with(|m| {
        m.borrow()
            .get(&handle)
            .is_some_and(|items| index >= 0 && (index as usize) < items.len())
    });
    if !valid {
        return;
    }
    if let Some(view) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.call_method(
            view.as_obj(),
            "setValue",
            "(I)V",
            &[JValue::Int(index as i32)],
        );
    }
}

pub fn get_selected(handle: i64) -> i64 {
    let has_items = ITEMS.with(|m| {
        m.borrow()
            .get(&handle)
            .is_some_and(|items| !items.is_empty())
    });
    if !has_items {
        return -1;
    }
    let Some(view) = super::get_widget(handle) else {
        return -1;
    };
    let mut env = jni_bridge::get_env();
    env.call_method(view.as_obj(), "getValue", "()I", &[])
        .and_then(|value| value.i())
        .map(i64::from)
        .unwrap_or(-1)
}
