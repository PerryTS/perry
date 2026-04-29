use crate::callback;
use crate::jni_bridge;
use jni::objects::{JObject, JValue};
use std::cell::RefCell;
use std::collections::HashMap;

struct TabBarState {
    tab_items: Vec<i64>, // Widget handles for each tab TextView
    layout_handle: i64,  // Inner horizontal layout handle
    callback_key: i64,   // Registered callback key for on_select
    selected: usize,
}

thread_local! {
    static TABBAR_STATE: RefCell<HashMap<i64, TabBarState>> = RefCell::new(HashMap::new());
}

/// Create a tab bar (horizontal LinearLayout at the bottom).
/// `on_select` is called with the tab index when a tab is tapped.
pub fn create(on_select: f64) -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);
    let activity = super::get_activity(&mut env);

    // Create a horizontal LinearLayout for tab buttons
    let layout = env
        .new_object(
            "android/widget/LinearLayout",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("TabBar LinearLayout");
    let _ = env.call_method(&layout, "setOrientation", "(I)V", &[JValue::Int(0)]);
    let _ = env.call_method(
        &layout,
        "setBackgroundColor",
        "(I)V",
        &[JValue::Int(0xFFF8F8F8u32 as i32)],
    );

    // Create top divider line
    let divider = env
        .new_object(
            "android/view/View",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("divider");
    let _ = env.call_method(
        &divider,
        "setBackgroundColor",
        "(I)V",
        &[JValue::Int(0xFFE0E0E0u32 as i32)],
    );
    let dp1 = super::dp_to_px(&mut env, 1.0);
    let dp = env
        .new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(II)V",
            &[JValue::Int(-1), JValue::Int(dp1)],
        )
        .expect("dp");
    let _ = env.call_method(
        &divider,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&dp)],
    );

    // Wrapper: vertical layout with divider + tab row
    let wrapper = env
        .new_object(
            "android/widget/LinearLayout",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("wrapper");
    let _ = env.call_method(&wrapper, "setOrientation", "(I)V", &[JValue::Int(1)]);
    let _ = env.call_method(
        &wrapper,
        "addView",
        "(Landroid/view/View;)V",
        &[JValue::Object(&divider)],
    );
    let _ = env.call_method(
        &wrapper,
        "addView",
        "(Landroid/view/View;)V",
        &[JValue::Object(&layout)],
    );

    let wp = env
        .new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(II)V",
            &[JValue::Int(-1), JValue::Int(-2)],
        )
        .expect("wp");
    let _ = env.call_method(
        &wrapper,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&wp)],
    );

    let global = env.new_global_ref(wrapper).expect("TabBar ref");
    let handle = super::register_widget(global);

    let layout_global = env.new_global_ref(layout).expect("TabBar layout ref");
    let layout_handle = super::register_widget(layout_global);

    let cb_key = callback::register(on_select);

    TABBAR_STATE.with(|s| {
        s.borrow_mut().insert(
            handle,
            TabBarState {
                tab_items: Vec::new(),
                layout_handle,
                callback_key: cb_key,
                selected: 0,
            },
        );
    });

    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    handle
}

/// Add a tab to the tab bar.
pub fn add_tab(tabbar_handle: i64, label_ptr: *const u8) {
    let label = crate::app::str_from_header(label_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);
    let activity = super::get_activity(&mut env);

    let (layout_handle, cb_key) = TABBAR_STATE.with(|s| {
        let map = s.borrow();
        let st = match map.get(&tabbar_handle) {
            Some(st) => st,
            None => return (0i64, 0i64),
        };
        (st.layout_handle, st.callback_key)
    });

    let layout_ref = match super::get_widget(layout_handle) {
        Some(r) => r,
        None => {
            unsafe {
                env.pop_local_frame(&JObject::null());
            }
            return;
        }
    };

    // Create TextView for the tab
    let tv = env
        .new_object(
            "android/widget/TextView",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Tab TV");

    let jstr = env.new_string(&label).expect("tab label str");
    let _ = env.call_method(
        &tv,
        "setText",
        "(Ljava/lang/CharSequence;)V",
        &[JValue::Object(&jstr)],
    );
    let _ = env.call_method(
        &tv,
        "setTextSize",
        "(IF)V",
        &[JValue::Int(2), JValue::Float(14.0)],
    );
    let _ = env.call_method(&tv, "setGravity", "(I)V", &[JValue::Int(17)]); // CENTER

    let dp12 = super::dp_to_px(&mut env, 12.0);
    let dp8 = super::dp_to_px(&mut env, 8.0);
    let _ = env.call_method(
        &tv,
        "setPadding",
        "(IIII)V",
        &[
            JValue::Int(dp12),
            JValue::Int(dp8),
            JValue::Int(dp12),
            JValue::Int(dp8),
        ],
    );

    // Equal weight layout params
    let params = env
        .new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(IIF)V",
            &[JValue::Int(0), JValue::Int(-2), JValue::Float(1.0)],
        )
        .expect("tab lp");
    let _ = env.call_method(
        &tv,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&params)],
    );

    // Add to layout
    let _ = env.call_method(
        layout_ref.as_obj(),
        "addView",
        "(Landroid/view/View;)V",
        &[JValue::Object(&tv)],
    );

    let global = env.new_global_ref(tv).expect("tab ref");
    let tab_handle = super::register_widget(global);

    let tab_index = TABBAR_STATE.with(|s| {
        let mut map = s.borrow_mut();
        if let Some(state) = map.get_mut(&tabbar_handle) {
            let idx = state.tab_items.len();
            state.tab_items.push(tab_handle);
            idx
        } else {
            0
        }
    });

    // Set click handler via PerryBridge.setOnClickCallbackWithArg
    if let Some(tab_ref) = super::get_widget(tab_handle) {
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "setOnClickCallbackWithArg",
            "(Landroid/view/View;JD)V",
            &[
                JValue::Object(tab_ref.as_obj()),
                JValue::Long(cb_key),
                JValue::Double(tab_index as f64),
            ],
        );
    }

    // Initial color: gray (inactive)
    if let Some(tab_ref) = super::get_widget(tab_handle) {
        let _ = env.call_method(
            tab_ref.as_obj(),
            "setTextColor",
            "(I)V",
            &[JValue::Int(0xFF6B7280u32 as i32)],
        );
    }

    unsafe {
        env.pop_local_frame(&JObject::null());
    }
}

/// Set the selected tab index, updating visual state.
pub fn set_selected(tabbar_handle: i64, index: i64) {
    TABBAR_STATE.with(|s| {
        let mut map = s.borrow_mut();
        if let Some(state) = map.get_mut(&tabbar_handle) {
            state.selected = index as usize;
            let mut env = jni_bridge::get_env();
            let _ = env.push_local_frame(16);

            for (i, &item_handle) in state.tab_items.iter().enumerate() {
                if let Some(item_ref) = super::get_widget(item_handle) {
                    let color = if i == index as usize {
                        0xFF2563EBu32 as i32 // Active: blue
                    } else {
                        0xFF6B7280u32 as i32 // Inactive: gray
                    };
                    let _ = env.call_method(
                        item_ref.as_obj(),
                        "setTextColor",
                        "(I)V",
                        &[JValue::Int(color)],
                    );
                }
            }

            unsafe {
                env.pop_local_frame(&JObject::null());
            }
        }
    });
}
