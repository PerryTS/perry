//! Picker — Spinner with ArrayAdapter

use crate::callback;
use crate::jni_bridge;
use jni::objects::JValue;
use std::cell::RefCell;
use std::collections::HashMap;

fn str_from_header(ptr: *const u8) -> &'static str {
    crate::app::str_from_header(ptr)
}

struct PickerState {
    items: Vec<String>,
    on_change: f64,
}

thread_local! {
    static PICKER_STATES: RefCell<HashMap<i64, PickerState>> = RefCell::new(HashMap::new());
}

pub fn create(label_ptr: *const u8, on_change: f64, _style: i64) -> i64 {
    let _label = str_from_header(label_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);
    let spinner = env
        .new_object(
            "android/widget/Spinner",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create Spinner");

    // Set up selection callback via PerryBridge
    if on_change != 0.0 {
        let cb_key = callback::register(on_change);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "setSpinnerCallback",
            "(Landroid/widget/Spinner;J)V",
            &[JValue::Object(&spinner), JValue::Long(cb_key)],
        );
    }

    let global = env
        .new_global_ref(spinner)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);

    PICKER_STATES.with(|s| {
        s.borrow_mut().insert(
            handle,
            PickerState {
                items: Vec::new(),
                on_change,
            },
        );
    });

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}

pub fn add_item(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr).to_string();
    PICKER_STATES.with(|s| {
        let mut states = s.borrow_mut();
        if let Some(state) = states.get_mut(&handle) {
            state.items.push(title);
            refresh_adapter(handle, &state.items);
        }
    });
}

pub fn set_selected(handle: i64, index: i64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let _ = env.call_method(
            view_ref.as_obj(),
            "setSelection",
            "(I)V",
            &[JValue::Int(index as i32)],
        );
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

pub fn get_selected(handle: i64) -> i64 {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let result = env.call_method(view_ref.as_obj(), "getSelectedItemPosition", "()I", &[]);
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
        if let Ok(jni::objects::JValueGen::Int(i)) = result {
            return i as i64;
        }
    }
    -1
}

fn refresh_adapter(handle: i64, items: &[String]) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(32 + items.len() as i32);

        let activity = super::get_activity(&mut env);

        // Create String array
        let str_class = env.find_class("java/lang/String").expect("String class");
        let arr = env
            .new_object_array(
                items.len() as i32,
                &str_class,
                &jni::objects::JObject::null(),
            )
            .expect("Failed to create String array");

        for (i, item) in items.iter().enumerate() {
            let jstr = env.new_string(item).expect("Failed to create JNI string");
            let _ = env.set_object_array_element(&arr, i as i32, &jstr);
        }

        // Create ArrayAdapter(context, android.R.layout.simple_spinner_item, items)
        let adapter = env
            .new_object(
                "android/widget/ArrayAdapter",
                "(Landroid/content/Context;I[Ljava/lang/Object;)V",
                &[
                    JValue::Object(&activity),
                    JValue::Int(0x01090008), // android.R.layout.simple_spinner_item
                    JValue::Object(&arr),
                ],
            )
            .expect("Failed to create ArrayAdapter");

        // Set dropdown layout
        let _ = env.call_method(
            &adapter,
            "setDropDownViewResource",
            "(I)V",
            &[JValue::Int(0x01090009)], // android.R.layout.simple_spinner_dropdown_item
        );

        let _ = env.call_method(
            view_ref.as_obj(),
            "setAdapter",
            "(Landroid/widget/SpinnerAdapter;)V",
            &[JValue::Object(&adapter)],
        );

        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}
