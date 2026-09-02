//! Picker — Spinner with ArrayAdapter

use crate::callback;
use crate::jni_bridge;
use jni::JValue;
use std::cell::RefCell;
use std::collections::HashMap;

use perry_ffi::copy_string_from_raw as str_from_header;

struct PickerState {
    items: Vec<String>,
    on_change: f64,
}

thread_local! {
    static PICKER_STATES: RefCell<HashMap<i64, PickerState>> = RefCell::new(HashMap::new());
}

pub(crate) fn scan_android_picker_gc_roots(visitor: &mut perry_ffi::GcRootVisitor<'_>) {
    PICKER_STATES.with(|states| {
        for state in states.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.on_change);
        }
    });
}

pub fn create(label_ptr: *const u8, on_change: f64, _style: i64) -> i64 {
    let _label = unsafe { str_from_header(label_ptr) };
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);

        let activity = super::get_activity(env);
        let spinner = env
            .new_object(
                jni::jni_str!("android/widget/Spinner"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create Spinner");

        // Set up selection callback via PerryBridge
        if on_change != 0.0 {
            let cb_key = callback::register(on_change);
            let bridge_class =
                jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
            let bridge_cls: &jni::objects::JClass = &bridge_class;
            let _ = env.call_static_method(
                bridge_cls,
                jni::jni_str!("setSpinnerCallback"),
                jni::jni_sig!("(Landroid/widget/Spinner;J)V"),
                &[JValue::Object(&spinner), JValue::Long(cb_key)],
            );
        }

        let global = jni_bridge::new_global_ref(env, spinner).expect("Failed to create global ref");
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
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}

pub fn add_item(handle: i64, title_ptr: *const u8) {
    let title = unsafe { str_from_header(title_ptr) }.to_string();
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
        jni_bridge::with_env(|env| {
            let _ = jni_bridge::push_local_frame(env, 8);
            let _ = env.call_method(
                view_ref.as_obj(),
                jni::jni_str!("setSelection"),
                jni::jni_sig!("(I)V"),
                &[JValue::Int(index as i32)],
            );
            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
            }
        })
    }
}

pub fn get_selected(handle: i64) -> i64 {
    if let Some(view_ref) = super::get_widget(handle) {
        return jni_bridge::with_env(|env| {
            let _ = jni_bridge::push_local_frame(env, 8);
            let result = env.call_method(
                view_ref.as_obj(),
                jni::jni_str!("getSelectedItemPosition"),
                jni::jni_sig!("()I"),
                &[],
            );
            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
            }
            result.and_then(|value| value.i()).unwrap_or(-1) as i64
        });
    }
    -1
}

fn refresh_adapter(handle: i64, items: &[String]) {
    if let Some(view_ref) = super::get_widget(handle) {
        jni_bridge::with_env(|env| {
            let _ = jni_bridge::push_local_frame(env, 32 + items.len() as i32);

            let activity = super::get_activity(env);

            // Create String array
            let str_class = env
                .find_class(jni::jni_str!("java/lang/String"))
                .expect("String class");
            let arr = env
                .new_object_array(
                    items.len() as i32,
                    &str_class,
                    &jni::objects::JObject::null(),
                )
                .expect("Failed to create String array");

            for (i, item) in items.iter().enumerate() {
                let jstr = env.new_string(item).expect("Failed to create JNI string");
                let _ = arr.set_element(env, i, &jstr);
            }

            // Create ArrayAdapter(context, android.R.layout.simple_spinner_item, items)
            let adapter = env
                .new_object(
                    jni::jni_str!("android/widget/ArrayAdapter"),
                    jni::jni_sig!("(Landroid/content/Context;I[Ljava/lang/Object;)V"),
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
                jni::jni_str!("setDropDownViewResource"),
                jni::jni_sig!("(I)V"),
                &[JValue::Int(0x01090009)], // android.R.layout.simple_spinner_dropdown_item
            );

            let _ = env.call_method(
                view_ref.as_obj(),
                jni::jni_str!("setAdapter"),
                jni::jni_sig!("(Landroid/widget/SpinnerAdapter;)V"),
                &[JValue::Object(&adapter)],
            );

            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
            }
        })
    }
}
