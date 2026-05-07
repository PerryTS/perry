//! Issue #553 — `BottomNavigation` on Android using a horizontal LinearLayout
//! of vertical (ImageView + TextView) tabs with optional badge text.
//!
//! Plain `android.widget.*` (no Material/AndroidX dependency) — matches the
//! repo's existing tabbar.rs convention. Icons are loaded as drawable
//! resource names via `Resources.getIdentifier(..., "drawable", pkg)`; if
//! the resource is missing the icon slot is left empty.

use crate::callback;
use crate::jni_bridge;
use jni::objects::{JObject, JValue};
use std::cell::RefCell;
use std::collections::HashMap;

struct ItemViews {
    container: i64,
    icon: i64,
    label: i64,
    badge: Option<i64>,
}

struct BottomNavState {
    layout_handle: i64,
    items: Vec<ItemViews>,
    callback_key: i64,
    selected: i64,
}

thread_local! {
    static STATES: RefCell<HashMap<i64, BottomNavState>> = RefCell::new(HashMap::new());
}

/// Create a BottomNavigation bar.
pub fn create(on_select: f64) -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);
    let activity = super::get_activity(&mut env);

    // Wrapper: vertical LinearLayout with thin top divider + tab row.
    let divider = env
        .new_object(
            "android/view/View",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("BottomNav divider");
    let _ = env.call_method(
        &divider,
        "setBackgroundColor",
        "(I)V",
        &[JValue::Int(0xFFE0E0E0u32 as i32)],
    );
    let dp1 = super::dp_to_px(&mut env, 1.0);
    let dlp = env
        .new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(II)V",
            &[JValue::Int(-1), JValue::Int(dp1)],
        )
        .expect("dlp");
    let _ = env.call_method(
        &divider,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&dlp)],
    );

    let row = env
        .new_object(
            "android/widget/LinearLayout",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("BottomNav row");
    let _ = env.call_method(&row, "setOrientation", "(I)V", &[JValue::Int(0)]); // HORIZONTAL
    let _ = env.call_method(
        &row,
        "setBackgroundColor",
        "(I)V",
        &[JValue::Int(0xFFFFFFFFu32 as i32)],
    );

    let wrapper = env
        .new_object(
            "android/widget/LinearLayout",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("BottomNav wrapper");
    let _ = env.call_method(&wrapper, "setOrientation", "(I)V", &[JValue::Int(1)]); // VERTICAL
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
        &[JValue::Object(&row)],
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

    let global = env.new_global_ref(wrapper).expect("BottomNav ref");
    let handle = super::register_widget(global);
    let row_global = env.new_global_ref(row).expect("BottomNav row ref");
    let layout_handle = super::register_widget(row_global);

    let cb_key = callback::register(on_select);
    STATES.with(|s| {
        s.borrow_mut().insert(
            handle,
            BottomNavState {
                layout_handle,
                items: Vec::new(),
                callback_key: cb_key,
                selected: 0,
            },
        );
    });

    unsafe {
        let _ = env.pop_local_frame(&JObject::null());
    }
    handle
}

/// Add a tab item (icon drawable name + label).
pub fn add_item(handle: i64, icon_ptr: *const u8, label_ptr: *const u8) {
    let icon = crate::app::str_from_header(icon_ptr);
    let label = crate::app::str_from_header(label_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);
    let activity = super::get_activity(&mut env);

    let (layout_handle, cb_key, idx) = STATES.with(|s| {
        let map = s.borrow();
        match map.get(&handle) {
            Some(st) => (st.layout_handle, st.callback_key, st.items.len() as i64),
            None => (0, 0, 0),
        }
    });
    let Some(layout_ref) = super::get_widget(layout_handle) else {
        unsafe {
            let _ = env.pop_local_frame(&JObject::null());
        }
        return;
    };

    // Tab container: vertical LinearLayout with icon on top, label below.
    let tab = env
        .new_object(
            "android/widget/LinearLayout",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Tab container");
    let _ = env.call_method(&tab, "setOrientation", "(I)V", &[JValue::Int(1)]); // VERTICAL
    let _ = env.call_method(&tab, "setGravity", "(I)V", &[JValue::Int(17)]); // CENTER
    let _ = env.call_method(&tab, "setClickable", "(Z)V", &[JValue::Bool(1)]);

    let dp8 = super::dp_to_px(&mut env, 8.0);
    let _ = env.call_method(
        &tab,
        "setPadding",
        "(IIII)V",
        &[
            JValue::Int(dp8),
            JValue::Int(dp8),
            JValue::Int(dp8),
            JValue::Int(dp8),
        ],
    );

    // Equal-weight layout params so each tab gets the same width.
    let lp = env
        .new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(IIF)V",
            &[JValue::Int(0), JValue::Int(-2), JValue::Float(1.0)],
        )
        .expect("tab lp");
    let _ = env.call_method(
        &tab,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&lp)],
    );

    // Icon: ImageView with drawable lookup by resource name.
    let iv = env
        .new_object(
            "android/widget/ImageView",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("icon iv");
    let dp24 = super::dp_to_px(&mut env, 24.0);
    let icon_lp = env
        .new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(II)V",
            &[JValue::Int(dp24), JValue::Int(dp24)],
        )
        .expect("icon lp");
    let _ = env.call_method(
        &iv,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&icon_lp)],
    );
    if !icon.is_empty() {
        // Resources.getIdentifier(icon, "drawable", pkg)
        if let Ok(resources) = env.call_method(
            &activity,
            "getResources",
            "()Landroid/content/res/Resources;",
            &[],
        ) {
            if let Ok(res_obj) = resources.l() {
                let pkg = env
                    .call_method(&activity, "getPackageName", "()Ljava/lang/String;", &[])
                    .ok()
                    .and_then(|p| p.l().ok());
                let icon_str = env.new_string(&icon).ok();
                let drawable_str = env.new_string("drawable").ok();
                if let (Some(pkg_obj), Some(icon_str), Some(drawable_str)) =
                    (pkg, icon_str, drawable_str)
                {
                    let id = env
                        .call_method(
                            &res_obj,
                            "getIdentifier",
                            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I",
                            &[
                                JValue::Object(&icon_str),
                                JValue::Object(&drawable_str),
                                JValue::Object(&pkg_obj),
                            ],
                        )
                        .ok()
                        .and_then(|v| v.i().ok())
                        .unwrap_or(0);
                    if id != 0 {
                        let _ =
                            env.call_method(&iv, "setImageResource", "(I)V", &[JValue::Int(id)]);
                    }
                }
            }
        }
    }
    // Initial tint: gray.
    let _ = env.call_method(
        &iv,
        "setColorFilter",
        "(I)V",
        &[JValue::Int(0xFF6B7280u32 as i32)],
    );

    let _ = env.call_method(
        &tab,
        "addView",
        "(Landroid/view/View;)V",
        &[JValue::Object(&iv)],
    );

    // Label
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
        &[JValue::Int(2), JValue::Float(11.0)],
    );
    let _ = env.call_method(&tv, "setGravity", "(I)V", &[JValue::Int(17)]);
    let _ = env.call_method(
        &tv,
        "setTextColor",
        "(I)V",
        &[JValue::Int(0xFF6B7280u32 as i32)],
    );
    let _ = env.call_method(
        &tab,
        "addView",
        "(Landroid/view/View;)V",
        &[JValue::Object(&tv)],
    );

    let _ = env.call_method(
        layout_ref.as_obj(),
        "addView",
        "(Landroid/view/View;)V",
        &[JValue::Object(&tab)],
    );

    let tab_global = env.new_global_ref(tab).expect("tab ref");
    let tab_handle = super::register_widget(tab_global);
    let iv_global = env.new_global_ref(iv).expect("icon ref");
    let icon_handle = super::register_widget(iv_global);
    let tv_global = env.new_global_ref(tv).expect("label ref");
    let label_handle = super::register_widget(tv_global);

    // Click handler.
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
                JValue::Double(idx as f64),
            ],
        );
    }

    STATES.with(|s| {
        if let Some(state) = s.borrow_mut().get_mut(&handle) {
            state.items.push(ItemViews {
                container: tab_handle,
                icon: icon_handle,
                label: label_handle,
                badge: None,
            });
        }
    });
    apply_styling(handle);

    unsafe {
        let _ = env.pop_local_frame(&JObject::null());
    }
}

/// Set or clear the badge string on a tab. Empty clears the badge.
pub fn set_badge(handle: i64, index: i64, badge_ptr: *const u8) {
    let badge = crate::app::str_from_header(badge_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let (item_container, existing_badge) = STATES.with(|s| {
        let map = s.borrow();
        match map.get(&handle).and_then(|st| st.items.get(index as usize)) {
            Some(item) => (item.container, item.badge),
            None => (0, None),
        }
    });
    if item_container == 0 {
        unsafe {
            let _ = env.pop_local_frame(&JObject::null());
        }
        return;
    }

    // Remove existing badge.
    if let Some(existing) = existing_badge {
        if let Some(badge_ref) = super::get_widget(existing) {
            // Detach from parent: ((ViewGroup) badge.getParent()).removeView(badge)
            if let Ok(parent) = env.call_method(
                badge_ref.as_obj(),
                "getParent",
                "()Landroid/view/ViewParent;",
                &[],
            ) {
                if let Ok(parent_obj) = parent.l() {
                    let _ = env.call_method(
                        &parent_obj,
                        "removeView",
                        "(Landroid/view/View;)V",
                        &[JValue::Object(badge_ref.as_obj())],
                    );
                }
            }
        }
    }

    let new_badge_handle = if badge.is_empty() {
        None
    } else {
        // Append a small TextView with red background as a badge.
        let activity = super::get_activity(&mut env);
        let tv = env
            .new_object(
                "android/widget/TextView",
                "(Landroid/content/Context;)V",
                &[JValue::Object(&activity)],
            )
            .expect("badge TV");
        let jstr = env.new_string(&badge).expect("badge str");
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
            &[JValue::Int(2), JValue::Float(9.0)],
        );
        let _ = env.call_method(
            &tv,
            "setTextColor",
            "(I)V",
            &[JValue::Int(0xFFFFFFFFu32 as i32)],
        );
        let _ = env.call_method(
            &tv,
            "setBackgroundColor",
            "(I)V",
            &[JValue::Int(0xFFD83333u32 as i32)],
        );
        let dp4 = super::dp_to_px(&mut env, 4.0);
        let _ = env.call_method(
            &tv,
            "setPadding",
            "(IIII)V",
            &[
                JValue::Int(dp4),
                JValue::Int(0),
                JValue::Int(dp4),
                JValue::Int(0),
            ],
        );
        let _ = env.call_method(&tv, "setGravity", "(I)V", &[JValue::Int(17)]);

        if let Some(tab_ref) = super::get_widget(item_container) {
            let _ = env.call_method(
                tab_ref.as_obj(),
                "addView",
                "(Landroid/view/View;)V",
                &[JValue::Object(&tv)],
            );
        }
        let badge_global = env.new_global_ref(tv).expect("badge ref");
        let badge_handle = super::register_widget(badge_global);
        Some(badge_handle)
    };

    STATES.with(|s| {
        if let Some(state) = s.borrow_mut().get_mut(&handle) {
            if let Some(item) = state.items.get_mut(index as usize) {
                item.badge = new_badge_handle;
            }
        }
    });

    unsafe {
        let _ = env.pop_local_frame(&JObject::null());
    }
}

pub fn set_selected(handle: i64, index: i64) {
    STATES.with(|s| {
        if let Some(state) = s.borrow_mut().get_mut(&handle) {
            state.selected = index;
        }
    });
    apply_styling(handle);
}

fn apply_styling(handle: i64) {
    let (items, selected) = STATES.with(|s| {
        let map = s.borrow();
        match map.get(&handle) {
            Some(st) => (
                st.items
                    .iter()
                    .map(|i| (i.icon, i.label))
                    .collect::<Vec<_>>(),
                st.selected,
            ),
            None => (Vec::new(), 0),
        }
    });
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);
    for (i, (icon_handle, label_handle)) in items.iter().enumerate() {
        let color = if i as i64 == selected {
            0xFF2563EBu32 as i32
        } else {
            0xFF6B7280u32 as i32
        };
        if let Some(icon_ref) = super::get_widget(*icon_handle) {
            let _ = env.call_method(
                icon_ref.as_obj(),
                "setColorFilter",
                "(I)V",
                &[JValue::Int(color)],
            );
        }
        if let Some(label_ref) = super::get_widget(*label_handle) {
            let _ = env.call_method(
                label_ref.as_obj(),
                "setTextColor",
                "(I)V",
                &[JValue::Int(color)],
            );
        }
    }
    unsafe {
        let _ = env.pop_local_frame(&JObject::null());
    }
}
