//! Issue #553 — `BottomNavigation` on Android using a horizontal LinearLayout
//! of vertical (ImageView + TextView) tabs with optional badge text.
//!
//! Plain `android.widget.*` (no Material/AndroidX dependency) — matches the
//! repo's existing tabbar.rs convention. Icons are loaded as drawable
//! resource names via `Resources.getIdentifier(..., "drawable", pkg)`; if
//! the resource is missing the icon slot is left empty.

use crate::callback;
use crate::jni_bridge;
use jni::objects::JObject;
use jni::JValue;
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
    /// Issue #706 — explicit ARGB tint for the selected tab. None
    /// keeps the existing default blue (#FF2563EB) used in apply_styling.
    selected_tint: Option<i32>,
    /// Issue #706 — explicit ARGB tint for unselected tabs. None falls
    /// back to the existing default gray (#FF6B7280).
    unselected_tint: Option<i32>,
}

thread_local! {
    static STATES: RefCell<HashMap<i64, BottomNavState>> = RefCell::new(HashMap::new());
}

/// Create a BottomNavigation bar.
pub fn create(on_select: f64) -> i64 {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);
        let activity = super::get_activity(env);

        // Wrapper: vertical LinearLayout with thin top divider + tab row.
        let divider = env
            .new_object(
                jni::jni_str!("android/view/View"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("BottomNav divider");
        let _ = env.call_method(
            &divider,
            jni::jni_str!("setBackgroundColor"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(0xFFE0E0E0u32 as i32)],
        );
        let dp1 = super::dp_to_px(env, 1.0);
        let dlp = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout$LayoutParams"),
                jni::jni_sig!("(II)V"),
                &[JValue::Int(-1), JValue::Int(dp1)],
            )
            .expect("dlp");
        let _ = env.call_method(
            &divider,
            jni::jni_str!("setLayoutParams"),
            jni::jni_sig!("(Landroid/view/ViewGroup$LayoutParams;)V"),
            &[JValue::Object(&dlp)],
        );

        let row = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("BottomNav row");
        let _ = env.call_method(
            &row,
            jni::jni_str!("setOrientation"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(0)],
        ); // HORIZONTAL
        let _ = env.call_method(
            &row,
            jni::jni_str!("setBackgroundColor"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(0xFFFFFFFFu32 as i32)],
        );

        let wrapper = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("BottomNav wrapper");
        let _ = env.call_method(
            &wrapper,
            jni::jni_str!("setOrientation"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(1)],
        ); // VERTICAL
        let _ = env.call_method(
            &wrapper,
            jni::jni_str!("addView"),
            jni::jni_sig!("(Landroid/view/View;)V"),
            &[JValue::Object(&divider)],
        );
        let _ = env.call_method(
            &wrapper,
            jni::jni_str!("addView"),
            jni::jni_sig!("(Landroid/view/View;)V"),
            &[JValue::Object(&row)],
        );
        let wp = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout$LayoutParams"),
                jni::jni_sig!("(II)V"),
                &[JValue::Int(-1), JValue::Int(-2)],
            )
            .expect("wp");
        let _ = env.call_method(
            &wrapper,
            jni::jni_str!("setLayoutParams"),
            jni::jni_sig!("(Landroid/view/ViewGroup$LayoutParams;)V"),
            &[JValue::Object(&wp)],
        );

        let global = jni_bridge::new_global_ref(env, wrapper).expect("BottomNav ref");
        let handle = super::register_widget(global);
        let row_global = jni_bridge::new_global_ref(env, row).expect("BottomNav row ref");
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
                    selected_tint: None,
                    unselected_tint: None,
                },
            );
        });

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
        handle
    })
}

/// Add a tab item (icon drawable name + label).
pub fn add_item(handle: i64, icon_ptr: *const u8, label_ptr: *const u8) {
    let icon = unsafe { crate::app::str_from_header(icon_ptr) };
    let label = unsafe { crate::app::str_from_header(label_ptr) };
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);
        let activity = super::get_activity(env);

        let (layout_handle, cb_key, idx) = STATES.with(|s| {
            let map = s.borrow();
            match map.get(&handle) {
                Some(st) => (st.layout_handle, st.callback_key, st.items.len() as i64),
                None => (0, 0, 0),
            }
        });
        let Some(layout_ref) = super::get_widget(layout_handle) else {
            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &JObject::null());
            }
            return;
        };

        // Tab container: vertical LinearLayout with icon on top, label below.
        let tab = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Tab container");
        let _ = env.call_method(
            &tab,
            jni::jni_str!("setOrientation"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(1)],
        ); // VERTICAL
        let _ = env.call_method(
            &tab,
            jni::jni_str!("setGravity"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(17)],
        ); // CENTER
        let _ = env.call_method(
            &tab,
            jni::jni_str!("setClickable"),
            jni::jni_sig!("(Z)V"),
            &[JValue::Bool(true)],
        );

        let dp8 = super::dp_to_px(env, 8.0);
        let _ = env.call_method(
            &tab,
            jni::jni_str!("setPadding"),
            jni::jni_sig!("(IIII)V"),
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
                jni::jni_str!("android/widget/LinearLayout$LayoutParams"),
                jni::jni_sig!("(IIF)V"),
                &[JValue::Int(0), JValue::Int(-2), JValue::Float(1.0)],
            )
            .expect("tab lp");
        let _ = env.call_method(
            &tab,
            jni::jni_str!("setLayoutParams"),
            jni::jni_sig!("(Landroid/view/ViewGroup$LayoutParams;)V"),
            &[JValue::Object(&lp)],
        );

        // Icon: ImageView with drawable lookup by resource name.
        let iv = env
            .new_object(
                jni::jni_str!("android/widget/ImageView"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("icon iv");
        let dp24 = super::dp_to_px(env, 24.0);
        let icon_lp = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout$LayoutParams"),
                jni::jni_sig!("(II)V"),
                &[JValue::Int(dp24), JValue::Int(dp24)],
            )
            .expect("icon lp");
        let _ = env.call_method(
            &iv,
            jni::jni_str!("setLayoutParams"),
            jni::jni_sig!("(Landroid/view/ViewGroup$LayoutParams;)V"),
            &[JValue::Object(&icon_lp)],
        );
        if !icon.is_empty() {
            // Resources.getIdentifier(icon, "drawable", pkg)
            if let Ok(resources) = env.call_method(
                &activity,
                jni::jni_str!("getResources"),
                jni::jni_sig!("()Landroid/content/res/Resources;"),
                &[],
            ) {
                if let Ok(res_obj) = resources.l() {
                    let pkg = env
                        .call_method(
                            &activity,
                            jni::jni_str!("getPackageName"),
                            jni::jni_sig!("()Ljava/lang/String;"),
                            &[],
                        )
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
                                jni::jni_str!("getIdentifier"),
                                jni::jni_sig!(
                                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I"
                                ),
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
                            let _ = env.call_method(
                                &iv,
                                jni::jni_str!("setImageResource"),
                                jni::jni_sig!("(I)V"),
                                &[JValue::Int(id)],
                            );
                        }
                    }
                }
            }
        }
        // Initial tint: gray.
        let _ = env.call_method(
            &iv,
            jni::jni_str!("setColorFilter"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(0xFF6B7280u32 as i32)],
        );

        let _ = env.call_method(
            &tab,
            jni::jni_str!("addView"),
            jni::jni_sig!("(Landroid/view/View;)V"),
            &[JValue::Object(&iv)],
        );

        // Label
        let tv = env
            .new_object(
                jni::jni_str!("android/widget/TextView"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Tab TV");
        let jstr = env.new_string(&label).expect("tab label str");
        let _ = env.call_method(
            &tv,
            jni::jni_str!("setText"),
            jni::jni_sig!("(Ljava/lang/CharSequence;)V"),
            &[JValue::Object(&jstr)],
        );
        let _ = env.call_method(
            &tv,
            jni::jni_str!("setTextSize"),
            jni::jni_sig!("(IF)V"),
            &[JValue::Int(2), JValue::Float(11.0)],
        );
        let _ = env.call_method(
            &tv,
            jni::jni_str!("setGravity"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(17)],
        );
        let _ = env.call_method(
            &tv,
            jni::jni_str!("setTextColor"),
            jni::jni_sig!("(I)V"),
            &[JValue::Int(0xFF6B7280u32 as i32)],
        );
        let _ = env.call_method(
            &tab,
            jni::jni_str!("addView"),
            jni::jni_sig!("(Landroid/view/View;)V"),
            &[JValue::Object(&tv)],
        );

        let _ = env.call_method(
            layout_ref.as_obj(),
            jni::jni_str!("addView"),
            jni::jni_sig!("(Landroid/view/View;)V"),
            &[JValue::Object(&tab)],
        );

        let tab_global = jni_bridge::new_global_ref(env, tab).expect("tab ref");
        let tab_handle = super::register_widget(tab_global);
        let iv_global = jni_bridge::new_global_ref(env, iv).expect("icon ref");
        let icon_handle = super::register_widget(iv_global);
        let tv_global = jni_bridge::new_global_ref(env, tv).expect("label ref");
        let label_handle = super::register_widget(tv_global);

        // Click handler.
        if let Some(tab_ref) = super::get_widget(tab_handle) {
            let bridge_class =
                jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
            let bridge_cls: &jni::objects::JClass = &bridge_class;
            let _ = env.call_static_method(
                bridge_cls,
                jni::jni_str!("setOnClickCallbackWithArg"),
                jni::jni_sig!("(Landroid/view/View;JD)V"),
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
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
    })
}

/// Set or clear the badge string on a tab. Empty clears the badge.
pub fn set_badge(handle: i64, index: i64, badge_ptr: *const u8) {
    let badge = unsafe { crate::app::str_from_header(badge_ptr) };
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 16);

        let (item_container, existing_badge) = STATES.with(|s| {
            let map = s.borrow();
            match map.get(&handle).and_then(|st| st.items.get(index as usize)) {
                Some(item) => (item.container, item.badge),
                None => (0, None),
            }
        });
        if item_container == 0 {
            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &JObject::null());
            }
            return;
        }

        // Remove existing badge.
        if let Some(existing) = existing_badge {
            if let Some(badge_ref) = super::get_widget(existing) {
                // Detach from parent: ((ViewGroup) badge.getParent()).removeView(badge)
                if let Ok(parent) = env.call_method(
                    badge_ref.as_obj(),
                    jni::jni_str!("getParent"),
                    jni::jni_sig!("()Landroid/view/ViewParent;"),
                    &[],
                ) {
                    if let Ok(parent_obj) = parent.l() {
                        let _ = env.call_method(
                            &parent_obj,
                            jni::jni_str!("removeView"),
                            jni::jni_sig!("(Landroid/view/View;)V"),
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
            let activity = super::get_activity(env);
            let tv = env
                .new_object(
                    jni::jni_str!("android/widget/TextView"),
                    jni::jni_sig!("(Landroid/content/Context;)V"),
                    &[JValue::Object(&activity)],
                )
                .expect("badge TV");
            let jstr = env.new_string(&badge).expect("badge str");
            let _ = env.call_method(
                &tv,
                jni::jni_str!("setText"),
                jni::jni_sig!("(Ljava/lang/CharSequence;)V"),
                &[JValue::Object(&jstr)],
            );
            let _ = env.call_method(
                &tv,
                jni::jni_str!("setTextSize"),
                jni::jni_sig!("(IF)V"),
                &[JValue::Int(2), JValue::Float(9.0)],
            );
            let _ = env.call_method(
                &tv,
                jni::jni_str!("setTextColor"),
                jni::jni_sig!("(I)V"),
                &[JValue::Int(0xFFFFFFFFu32 as i32)],
            );
            let _ = env.call_method(
                &tv,
                jni::jni_str!("setBackgroundColor"),
                jni::jni_sig!("(I)V"),
                &[JValue::Int(0xFFD83333u32 as i32)],
            );
            let dp4 = super::dp_to_px(env, 4.0);
            let _ = env.call_method(
                &tv,
                jni::jni_str!("setPadding"),
                jni::jni_sig!("(IIII)V"),
                &[
                    JValue::Int(dp4),
                    JValue::Int(0),
                    JValue::Int(dp4),
                    JValue::Int(0),
                ],
            );
            let _ = env.call_method(
                &tv,
                jni::jni_str!("setGravity"),
                jni::jni_sig!("(I)V"),
                &[JValue::Int(17)],
            );

            if let Some(tab_ref) = super::get_widget(item_container) {
                let _ = env.call_method(
                    tab_ref.as_obj(),
                    jni::jni_str!("addView"),
                    jni::jni_sig!("(Landroid/view/View;)V"),
                    &[JValue::Object(&tv)],
                );
            }
            let badge_global = jni_bridge::new_global_ref(env, tv).expect("badge ref");
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
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
    })
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
    let (items, selected, selected_tint, unselected_tint) = STATES.with(|s| {
        let map = s.borrow();
        match map.get(&handle) {
            Some(st) => (
                st.items
                    .iter()
                    .map(|i| (i.icon, i.label))
                    .collect::<Vec<_>>(),
                st.selected,
                st.selected_tint,
                st.unselected_tint,
            ),
            None => (Vec::new(), 0, None, None),
        }
    });
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 16);
        for (i, (icon_handle, label_handle)) in items.iter().enumerate() {
            let is_sel = i as i64 == selected;
            let color = if is_sel {
                selected_tint.unwrap_or(0xFF2563EBu32 as i32)
            } else {
                unselected_tint.unwrap_or(0xFF6B7280u32 as i32)
            };
            if let Some(icon_ref) = super::get_widget(*icon_handle) {
                let _ = env.call_method(
                    icon_ref.as_obj(),
                    jni::jni_str!("setColorFilter"),
                    jni::jni_sig!("(I)V"),
                    &[JValue::Int(color)],
                );
            }
            if let Some(label_ref) = super::get_widget(*label_handle) {
                let _ = env.call_method(
                    label_ref.as_obj(),
                    jni::jni_str!("setTextColor"),
                    jni::jni_sig!("(I)V"),
                    &[JValue::Int(color)],
                );
            }
        }
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
    })
}

/// Pack RGBA 0..1 into Android ARGB int (`0xAARRGGBB`).
fn rgba_to_argb(r: f64, g: f64, b: f64, a: f64) -> i32 {
    let to_u8 = |v: f64| -> u32 { (v.clamp(0.0, 1.0) * 255.0).round() as u32 };
    let argb = (to_u8(a) << 24) | (to_u8(r) << 16) | (to_u8(g) << 8) | to_u8(b);
    argb as i32
}

/// Issue #706 — override the active tab's tint. Stored in `BottomNavState`
/// and applied via `setColorFilter` (icon) + `setTextColor` (label) on the
/// next `apply_styling` pass.
pub fn set_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    let argb = rgba_to_argb(r, g, b, a);
    STATES.with(|s| {
        if let Some(state) = s.borrow_mut().get_mut(&handle) {
            state.selected_tint = Some(argb);
        }
    });
    apply_styling(handle);
}

/// Issue #706 — override inactive tabs' tint.
pub fn set_unselected_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    let argb = rgba_to_argb(r, g, b, a);
    STATES.with(|s| {
        if let Some(state) = s.borrow_mut().get_mut(&handle) {
            state.unselected_tint = Some(argb);
        }
    });
    apply_styling(handle);
}
