use jni::objects::JValue;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::app::str_from_header;
use crate::callback;
use crate::jni_bridge;
use jni::objects::GlobalRef;

thread_local! {
    /// Maps widget handle (the HStack container) -> Switch GlobalRef,
    /// so set_state() can target the inner Switch.
    static TOGGLE_SWITCHES: RefCell<HashMap<i64, GlobalRef>> = RefCell::new(HashMap::new());
}

/// Set the on/off state of an existing toggle widget.
pub fn set_state(handle: i64, on: i64) {
    TOGGLE_SWITCHES.with(|switches| {
        if let Some(switch_ref) = switches.borrow().get(&handle) {
            let mut env = jni_bridge::get_env();
            let _ = env.push_local_frame(8);
            let _ = env.call_method(
                switch_ref.as_obj(),
                "setChecked",
                "(Z)V",
                &[JValue::Bool(if on != 0 { 1 } else { 0 })],
            );
            unsafe {
                env.pop_local_frame(&jni::objects::JObject::null());
            }
        }
    });
}

/// Create a Switch with a label and onChange callback.
/// Returns a widget handle for a LinearLayout(HORIZONTAL) containing the label and switch.
pub fn create(label_ptr: *const u8, on_change: f64) -> i64 {
    let label = str_from_header(label_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);

    // Create the Switch widget
    let switch = env
        .new_object(
            "android/widget/Switch",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create Switch");

    // Set label text on the Switch itself (Switch extends CompoundButton extends TextView)
    let jstr = env.new_string(label).expect("Failed to create JNI string");
    let _ = env.call_method(
        &switch,
        "setText",
        "(Ljava/lang/CharSequence;)V",
        &[JValue::Object(&jstr)],
    );

    // Register callback and set up OnCheckedChangeListener via PerryBridge
    let cb_key = callback::register(on_change);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "setOnCheckedChangeCallback",
        "(Landroid/widget/CompoundButton;J)V",
        &[JValue::Object(&switch), JValue::Long(cb_key)],
    );

    // Create horizontal container (to match iOS pattern where toggle = label + switch)
    let container = env
        .new_object(
            "android/widget/LinearLayout",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create LinearLayout");

    // HORIZONTAL = 0
    let _ = env.call_method(&container, "setOrientation", "(I)V", &[JValue::Int(0)]);
    // CENTER_VERTICAL = 16
    let _ = env.call_method(&container, "setGravity", "(I)V", &[JValue::Int(16)]);

    // Add the Switch (which already has the label text)
    let _ = env.call_method(
        &container,
        "addView",
        "(Landroid/view/View;)V",
        &[JValue::Object(&switch)],
    );

    let switch_global = env
        .new_global_ref(&switch)
        .expect("Failed to create global ref");
    let container_global = env
        .new_global_ref(container)
        .expect("Failed to create global ref");
    let handle = super::register_widget(container_global);

    TOGGLE_SWITCHES.with(|switches| {
        switches.borrow_mut().insert(handle, switch_global);
    });

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}
