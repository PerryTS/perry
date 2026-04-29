//! Toolbar — LinearLayout-based toolbar on Android

use crate::callback;
use crate::jni_bridge;
use jni::objects::JValue;
use std::cell::RefCell;
use std::collections::HashMap;

fn str_from_header(ptr: *const u8) -> &'static str {
    crate::app::str_from_header(ptr)
}

struct ToolbarState {
    widget_handle: i64,
}

thread_local! {
    static TOOLBARS: RefCell<HashMap<i64, ToolbarState>> = RefCell::new(HashMap::new());
    static NEXT_TOOLBAR_ID: RefCell<i64> = RefCell::new(1);
}

pub fn create() -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = crate::widgets::get_activity(&mut env);

    // Create horizontal LinearLayout for toolbar
    let layout = env
        .new_object(
            "android/widget/LinearLayout",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create LinearLayout");

    // Horizontal orientation
    let _ = env.call_method(&layout, "setOrientation", "(I)V", &[JValue::Int(0)]);

    // Padding
    let pad = crate::widgets::dp_to_px(&mut env, 8.0);
    let _ = env.call_method(
        &layout,
        "setPadding",
        "(IIII)V",
        &[
            JValue::Int(pad),
            JValue::Int(pad),
            JValue::Int(pad),
            JValue::Int(pad),
        ],
    );

    // Background color (light gray)
    let _ = env.call_method(
        &layout,
        "setBackgroundColor",
        "(I)V",
        &[JValue::Int(0xFFE0E0E0u32 as i32)],
    );

    let global = env
        .new_global_ref(layout)
        .expect("Failed to create global ref");
    let widget_handle = crate::widgets::register_widget(global);

    let id = NEXT_TOOLBAR_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    });

    TOOLBARS.with(|t| {
        t.borrow_mut().insert(id, ToolbarState { widget_handle });
    });

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    id
}

pub fn add_item(toolbar_handle: i64, label_ptr: *const u8, _icon_ptr: *const u8, on_press: f64) {
    let label = str_from_header(label_ptr);

    let widget_handle = TOOLBARS.with(|t| t.borrow().get(&toolbar_handle).map(|s| s.widget_handle));

    if let Some(wh) = widget_handle {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(32);

        let activity = crate::widgets::get_activity(&mut env);

        // Create button
        let button = env
            .new_object(
                "android/widget/Button",
                "(Landroid/content/Context;)V",
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create Button");

        let jstr = env.new_string(label).expect("Failed to create JNI string");
        let _ = env.call_method(
            &button,
            "setText",
            "(Ljava/lang/CharSequence;)V",
            &[JValue::Object(&jstr)],
        );

        // Register callback
        if on_press != 0.0 {
            let cb_key = callback::register(on_press);
            let bridge_class = jni_bridge::with_cache(|c| {
                env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap()
            });
            let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
            let _ = env.call_static_method(
                bridge_cls,
                "setOnClickCallback",
                "(Landroid/view/View;J)V",
                &[JValue::Object(&button), JValue::Long(cb_key)],
            );
        }

        // Add to toolbar
        if let Some(toolbar_ref) = crate::widgets::get_widget(wh) {
            let _ = env.call_method(
                toolbar_ref.as_obj(),
                "addView",
                "(Landroid/view/View;)V",
                &[JValue::Object(&button)],
            );
        }

        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

pub fn attach(_toolbar_handle: i64) {
    // On Android, toolbar attachment is typically done by the Activity layout
    // No-op for now since we add toolbar as a normal widget
}
