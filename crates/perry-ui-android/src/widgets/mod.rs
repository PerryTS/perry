pub mod button;
pub mod canvas;
pub mod divider;
pub mod form;
pub mod hstack;
pub mod image;
pub mod lazyvstack;
pub mod navstack;
pub mod picker;
pub mod progressview;
pub mod qrcode;
pub mod scrollview;
pub mod securefield;
pub mod slider;
pub mod spacer;
pub mod tabbar;
pub mod text;
pub mod text_registry;
pub mod textarea;
pub mod textfield;
pub mod toast;
pub mod toggle;
pub mod vstack;
pub mod zstack;

use jni::objects::{GlobalRef, JObject, JValue};
use std::sync::Mutex;

use crate::jni_bridge;

extern "C" {
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}

/// Global widget registry — shared across threads so widgets created on
/// the native thread can be accessed from UI-thread callbacks.
static WIDGETS: Mutex<Vec<GlobalRef>> = Mutex::new(Vec::new());

/// Store an Android View and return its handle (1-based i64).
pub fn register_widget(view: GlobalRef) -> i64 {
    let mut widgets = WIDGETS.lock().unwrap();
    widgets.push(view);
    widgets.len() as i64
}

/// Retrieve the JNI GlobalRef for a given widget handle.
pub fn get_widget(handle: i64) -> Option<GlobalRef> {
    let widgets = WIDGETS.lock().unwrap();
    let idx = (handle - 1) as usize;
    widgets.get(idx).cloned()
}

/// Set the hidden state of a widget (View.VISIBLE=0, View.GONE=8).
pub fn set_hidden(handle: i64, hidden: bool) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let visibility = if hidden { 8i32 } else { 0i32 }; // View.GONE=8, View.VISIBLE=0
        let _ = env.call_method(
            view_ref.as_obj(),
            "setVisibility",
            "(I)V",
            &[JValue::Int(visibility)],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Remove all child views from a ViewGroup container.
/// When clearing the root widget, also releases global refs for all child widgets
/// to prevent JNI global reference table overflow on rebuilds.
pub fn clear_children(handle: i64) {
    // Track the first widget that gets clearChildren called — it's the root
    crate::app::track_root_candidate(handle);
    if let Some(parent_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let _ = env.call_method(parent_ref.as_obj(), "removeAllViews", "()V", &[]);
        unsafe {
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            env.pop_local_frame(&JObject::null());
        }

        // Only truncate widget handles when ALL of the following are true:
        // 1. The app has completed its initial build (app_set_body called)
        // 2. The handle being cleared is the root body widget
        // During init, clearChildren may be called on non-root containers (e.g.
        // refreshConnectionList) while sibling widgets are still being created.
        // Truncating during init would destroy those handles.
        if crate::app::is_initialized() {
            let root = crate::app::get_root_handle();
            if handle == root {
                let idx = (handle - 1) as usize;
                let mut widgets = WIDGETS.lock().unwrap();
                if idx < widgets.len() {
                    widgets.truncate(idx + 1);
                }
            }
        }
    }
}

/// Add a child view to a parent ViewGroup.
/// For vertical LinearLayout parents (VStack), sets child width to MATCH_PARENT
/// to match iOS UIStackView fill alignment behavior.
pub fn add_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent_ref), Some(child_ref)) =
        (get_widget(parent_handle), get_widget(child_handle))
    {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);

        // Debug: log parent/child handles and child class
        unsafe {
            __android_log_print(
                3,
                b"PerryWidgets\0".as_ptr(),
                b"add_child: parent=%lld child=%lld\0".as_ptr(),
                parent_handle,
                child_handle,
            );
        }

        let result = env.call_method(
            parent_ref.as_obj(),
            "addView",
            "(Landroid/view/View;)V",
            &[JValue::Object(child_ref.as_obj())],
        );

        // Match iOS UIStackView fill alignment: adjust child LayoutParams
        // based on parent type
        if result.is_ok() {
            if env
                .is_instance_of(parent_ref.as_obj(), "android/widget/FrameLayout")
                .unwrap_or(false)
                && !env
                    .is_instance_of(parent_ref.as_obj(), "android/widget/LinearLayout")
                    .unwrap_or(false)
            {
                // FrameLayout (ZStack): children fill parent by default (match iOS ZStack behavior)
                if let Ok(lp) = env.call_method(
                    child_ref.as_obj(),
                    "getLayoutParams",
                    "()Landroid/view/ViewGroup$LayoutParams;",
                    &[],
                ) {
                    if let Ok(lp_obj) = lp.l() {
                        if !lp_obj.is_null() {
                            let _ = env.set_field(&lp_obj, "width", "I", JValue::Int(-1)); // MATCH_PARENT
                            let _ = env.set_field(&lp_obj, "height", "I", JValue::Int(-1)); // MATCH_PARENT
                            let _ = env.call_method(
                                child_ref.as_obj(),
                                "setLayoutParams",
                                "(Landroid/view/ViewGroup$LayoutParams;)V",
                                &[JValue::Object(&lp_obj)],
                            );
                        }
                    }
                }
            } else if env
                .is_instance_of(parent_ref.as_obj(), "android/widget/LinearLayout")
                .unwrap_or(false)
            {
                let orientation = env
                    .call_method(parent_ref.as_obj(), "getOrientation", "()I", &[])
                    .map(|v| v.i().unwrap_or(-1))
                    .unwrap_or(-1);
                if let Ok(lp) = env.call_method(
                    child_ref.as_obj(),
                    "getLayoutParams",
                    "()Landroid/view/ViewGroup$LayoutParams;",
                    &[],
                ) {
                    if let Ok(lp_obj) = lp.l() {
                        if !lp_obj.is_null() {
                            if orientation == 1 {
                                // VERTICAL — stretch children to fill width
                                let _ = env.set_field(&lp_obj, "width", "I", JValue::Int(-1));
                            // MATCH_PARENT
                            } else if orientation == 0 {
                                // HORIZONTAL — share space equally
                                // If child has MATCH_PARENT width, convert to weight-based
                                // so multiple children share the HStack evenly
                                let cur_w = env
                                    .get_field(&lp_obj, "width", "I")
                                    .map(|v| v.i().unwrap_or(0))
                                    .unwrap_or(0);
                                if cur_w == -1 {
                                    // MATCH_PARENT
                                    let _ = env.set_field(&lp_obj, "width", "I", JValue::Int(0));
                                    if env
                                        .is_instance_of(
                                            &lp_obj,
                                            "android/widget/LinearLayout$LayoutParams",
                                        )
                                        .unwrap_or(false)
                                    {
                                        let _ = env.set_field(
                                            &lp_obj,
                                            "weight",
                                            "F",
                                            JValue::Float(1.0),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        unsafe {
            if env.exception_check().unwrap_or(false) {
                __android_log_print(
                    6,
                    b"PerryWidgets\0".as_ptr(),
                    b"add_child: JNI EXCEPTION!\0".as_ptr(),
                );
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Add a child view to a parent ViewGroup at a specific index.
pub fn add_child_at(parent_handle: i64, child_handle: i64, index: i64) {
    if let (Some(parent_ref), Some(child_ref)) =
        (get_widget(parent_handle), get_widget(child_handle))
    {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let _ = env.call_method(
            parent_ref.as_obj(),
            "addView",
            "(Landroid/view/View;I)V",
            &[
                JValue::Object(child_ref.as_obj()),
                JValue::Int(index as i32),
            ],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Get the Activity context via PerryBridge.
pub fn get_activity<'a>(env: &mut jni::JNIEnv<'a>) -> JObject<'a> {
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let result = env
        .call_static_method(bridge_cls, "getActivity", "()Landroid/app/Activity;", &[])
        .expect("Failed to get Activity");
    result.l().expect("Activity is not an object")
}

/// Set enabled/disabled on a widget.
pub fn set_enabled(handle: i64, enabled: bool) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let _ = env.call_method(
            view_ref.as_obj(),
            "setEnabled",
            "(Z)V",
            &[JValue::Bool(enabled as u8)],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set tooltip (API 26+).
pub fn set_tooltip(handle: i64, text_ptr: *const u8) {
    let text = crate::app::str_from_header(text_ptr);
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let jstr = env.new_string(text).expect("tooltip string");
        let _ = env.call_method(
            view_ref.as_obj(),
            "setTooltipText",
            "(Ljava/lang/CharSequence;)V",
            &[JValue::Object(&jstr)],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set control size (map to scale).
pub fn set_control_size(handle: i64, size: i64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let scale = match size {
            0 => 0.75f32, // mini
            1 => 0.85f32, // small
            3 => 1.15f32, // large
            _ => 1.0f32,  // regular
        };
        let _ = env.call_method(
            view_ref.as_obj(),
            "setScaleX",
            "(F)V",
            &[JValue::Float(scale)],
        );
        let _ = env.call_method(
            view_ref.as_obj(),
            "setScaleY",
            "(F)V",
            &[JValue::Float(scale)],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set corner radius via GradientDrawable.
/// If the view already has a GradientDrawable background, updates its corner radius
/// (preserving the existing color). Otherwise creates a new transparent GradientDrawable.
pub fn set_corner_radius(handle: i64, radius: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        let radius_px = dp_to_px(&mut env, radius as f32) as f32;

        // Try to reuse existing GradientDrawable background (preserving color)
        let mut reused = false;
        if let Ok(bg) = env.call_method(
            view_ref.as_obj(),
            "getBackground",
            "()Landroid/graphics/drawable/Drawable;",
            &[],
        ) {
            if let Ok(bg_obj) = bg.l() {
                if !bg_obj.is_null() {
                    if env
                        .is_instance_of(&bg_obj, "android/graphics/drawable/GradientDrawable")
                        .unwrap_or(false)
                    {
                        let _ = env.call_method(
                            &bg_obj,
                            "setCornerRadius",
                            "(F)V",
                            &[JValue::Float(radius_px)],
                        );
                        reused = true;
                    }
                }
            }
        }
        if !reused {
            let gd = env
                .new_object("android/graphics/drawable/GradientDrawable", "()V", &[])
                .expect("GradientDrawable");
            let _ = env.call_method(&gd, "setCornerRadius", "(F)V", &[JValue::Float(radius_px)]);
            let _ = env.call_method(&gd, "setColor", "(I)V", &[JValue::Int(0)]);
            let _ = env.call_method(
                view_ref.as_obj(),
                "setBackground",
                "(Landroid/graphics/drawable/Drawable;)V",
                &[JValue::Object(&gd)],
            );
        }
        let _ = env.call_method(
            view_ref.as_obj(),
            "setClipToOutline",
            "(Z)V",
            &[JValue::Bool(1)],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set background color using GradientDrawable for compatibility with corner radius.
/// If the view already has a GradientDrawable, updates its color (preserving corner radius).
pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        let color = argb_color(a, r, g, b);

        // Try to reuse existing GradientDrawable (preserving corner radius)
        let mut reused = false;
        if let Ok(bg) = env.call_method(
            view_ref.as_obj(),
            "getBackground",
            "()Landroid/graphics/drawable/Drawable;",
            &[],
        ) {
            if let Ok(bg_obj) = bg.l() {
                if !bg_obj.is_null() {
                    if env
                        .is_instance_of(&bg_obj, "android/graphics/drawable/GradientDrawable")
                        .unwrap_or(false)
                    {
                        let _ = env.call_method(&bg_obj, "setColor", "(I)V", &[JValue::Int(color)]);
                        reused = true;
                    }
                }
            }
        }
        if !reused {
            // Create GradientDrawable so a later set_corner_radius can reuse it
            let gd = env
                .new_object("android/graphics/drawable/GradientDrawable", "()V", &[])
                .expect("GradientDrawable");
            let _ = env.call_method(&gd, "setColor", "(I)V", &[JValue::Int(color)]);
            let _ = env.call_method(
                view_ref.as_obj(),
                "setBackground",
                "(Landroid/graphics/drawable/Drawable;)V",
                &[JValue::Object(&gd)],
            );
        }
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set drop shadow on a widget (issue #185 Phase B).
///
/// Maps the cross-platform `(r, g, b, a, blur, offset_x, offset_y)` shape
/// onto Android's Material elevation model. Android Views don't expose
/// arbitrary blur/offset/color shadows like CSS or CALayer; instead the
/// platform owns the lighting and the app picks the elevation depth (in
/// dp). The mapping:
///   - `elevation = blur / 2` dp (clamped to ≥ 0). A 12-px blur becomes a
///     6 dp elevation, which gives a Material-style soft shadow that
///     reads similarly to the iOS / macOS / Web twins.
///   - `(r, g, b, a)` is forwarded to `setOutlineSpotShadowColor` and
///     `setOutlineAmbientShadowColor` (API 28+). Both calls silently
///     no-op on API 24-27, so we don't gate them — older devices get the
///     default system shadow color, newer devices get the requested tint.
///   - `(offset_x, offset_y)` is intentionally ignored — Android's
///     elevation model derives shadow direction from the device-level
///     light source, not per-widget. Phase C's `style: { shadow: {...} }`
///     ergonomic API will document this caveat.
pub fn set_shadow(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
    blur: f64,
    _offset_x: f64,
    _offset_y: f64,
) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);

        let elevation_dp = (blur / 2.0).max(0.0) as f32;
        let elevation_px = dp_to_px(&mut env, elevation_dp) as f32;
        let _ = env.call_method(
            view_ref.as_obj(),
            "setElevation",
            "(F)V",
            &[JValue::Float(elevation_px)],
        );

        // API 28+ shadow tinting; both call_method results are ignored
        // because older API levels just don't have the method and we
        // accept the system default in that case.
        let color = argb_color(a, r, g, b);
        let _ = env.call_method(
            view_ref.as_obj(),
            "setOutlineSpotShadowColor",
            "(I)V",
            &[JValue::Int(color)],
        );
        let _ = env.call_method(
            view_ref.as_obj(),
            "setOutlineAmbientShadowColor",
            "(I)V",
            &[JValue::Int(color)],
        );

        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set background gradient.
pub fn set_background_gradient(
    handle: i64,
    r1: f64,
    g1: f64,
    b1: f64,
    a1: f64,
    r2: f64,
    g2: f64,
    b2: f64,
    a2: f64,
    direction: f64,
) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);

        let c1 = argb_color(a1, r1, g1, b1);
        let c2 = argb_color(a2, r2, g2, b2);

        let gd = env
            .new_object("android/graphics/drawable/GradientDrawable", "()V", &[])
            .expect("GradientDrawable");

        // Set colors
        let colors = env.new_int_array(2).expect("int array");
        let _ = env.set_int_array_region(&colors, 0, &[c1, c2]);
        let _ = env.call_method(&gd, "setColors", "([I)V", &[JValue::Object(&colors)]);

        // Set orientation
        let orient_name = if direction < 0.5 {
            "TOP_BOTTOM"
        } else {
            "LEFT_RIGHT"
        };
        let orient_class = env
            .find_class("android/graphics/drawable/GradientDrawable$Orientation")
            .expect("Orientation");
        let orient = env
            .get_static_field(
                &orient_class,
                orient_name,
                "Landroid/graphics/drawable/GradientDrawable$Orientation;",
            )
            .expect("orient")
            .l()
            .expect("orient obj");
        let _ = env.call_method(
            &gd,
            "setOrientation",
            "(Landroid/graphics/drawable/GradientDrawable$Orientation;)V",
            &[JValue::Object(&orient)],
        );

        let _ = env.call_method(
            view_ref.as_obj(),
            "setBackground",
            "(Landroid/graphics/drawable/Drawable;)V",
            &[JValue::Object(&gd)],
        );

        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set on-hover callback (no-op on Android touch devices).
pub fn set_on_hover(_handle: i64, _callback: f64) {
    // No-op — hover events are uncommon on touch devices
}

/// Set double-click (double-tap) callback.
pub fn set_on_double_click(_handle: i64, _callback: f64) {
    // Would require GestureDetector setup via PerryBridge
    // No-op for now
}

/// Animate opacity. `duration_secs` is in seconds.
pub fn animate_opacity(handle: i64, target: f64, duration_secs: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let animator = env
            .call_method(
                view_ref.as_obj(),
                "animate",
                "()Landroid/view/ViewPropertyAnimator;",
                &[],
            )
            .expect("animate")
            .l()
            .expect("animator");
        let dur_ms = (duration_secs * 1000.0) as i64;
        let _ = env.call_method(
            &animator,
            "alpha",
            "(F)Landroid/view/ViewPropertyAnimator;",
            &[JValue::Float(target as f32)],
        );
        let _ = env.call_method(
            &animator,
            "setDuration",
            "(J)Landroid/view/ViewPropertyAnimator;",
            &[JValue::Long(dur_ms)],
        );
        let _ = env.call_method(&animator, "start", "()V", &[]);
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Animate position. `duration_secs` is in seconds.
pub fn animate_position(handle: i64, dx: f64, dy: f64, duration_secs: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let animator = env
            .call_method(
                view_ref.as_obj(),
                "animate",
                "()Landroid/view/ViewPropertyAnimator;",
                &[],
            )
            .expect("animate")
            .l()
            .expect("animator");
        let dur_ms = (duration_secs * 1000.0) as i64;
        let _ = env.call_method(
            &animator,
            "translationXBy",
            "(F)Landroid/view/ViewPropertyAnimator;",
            &[JValue::Float(dx as f32)],
        );
        let _ = env.call_method(
            &animator,
            "translationYBy",
            "(F)Landroid/view/ViewPropertyAnimator;",
            &[JValue::Float(dy as f32)],
        );
        let _ = env.call_method(
            &animator,
            "setDuration",
            "(J)Landroid/view/ViewPropertyAnimator;",
            &[JValue::Long(dur_ms)],
        );
        let _ = env.call_method(&animator, "start", "()V", &[]);
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set a fixed width on a widget via LayoutParams.
pub fn set_width(handle: i64, width: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        let width_px = dp_to_px(&mut env, width as f32);
        if let Ok(lp) = env.call_method(
            view_ref.as_obj(),
            "getLayoutParams",
            "()Landroid/view/ViewGroup$LayoutParams;",
            &[],
        ) {
            if let Ok(lp_obj) = lp.l() {
                if !lp_obj.is_null() {
                    let _ = env.set_field(&lp_obj, "width", "I", JValue::Int(width_px));
                    // Clear weight so width is respected
                    if env
                        .is_instance_of(&lp_obj, "android/widget/LinearLayout$LayoutParams")
                        .unwrap_or(false)
                    {
                        let _ = env.set_field(&lp_obj, "weight", "F", JValue::Float(0.0));
                    }
                    let _ = env.call_method(
                        view_ref.as_obj(),
                        "setLayoutParams",
                        "(Landroid/view/ViewGroup$LayoutParams;)V",
                        &[JValue::Object(&lp_obj)],
                    );
                } else {
                    // No LayoutParams yet — create one
                    let params = env.new_object(
                        "android/widget/LinearLayout$LayoutParams",
                        "(II)V",
                        &[JValue::Int(width_px), JValue::Int(-2)], // -2 = WRAP_CONTENT
                    );
                    if let Ok(params) = params {
                        let _ = env.call_method(
                            view_ref.as_obj(),
                            "setLayoutParams",
                            "(Landroid/view/ViewGroup$LayoutParams;)V",
                            &[JValue::Object(&params)],
                        );
                    }
                }
            }
        }
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set a fixed height on a widget via LayoutParams.
pub fn set_height(handle: i64, height: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        let height_px = dp_to_px(&mut env, height as f32);
        if let Ok(lp) = env.call_method(
            view_ref.as_obj(),
            "getLayoutParams",
            "()Landroid/view/ViewGroup$LayoutParams;",
            &[],
        ) {
            if let Ok(lp_obj) = lp.l() {
                if !lp_obj.is_null() {
                    let _ = env.set_field(&lp_obj, "height", "I", JValue::Int(height_px));
                    let _ = env.call_method(
                        view_ref.as_obj(),
                        "setLayoutParams",
                        "(Landroid/view/ViewGroup$LayoutParams;)V",
                        &[JValue::Object(&lp_obj)],
                    );
                } else {
                    let params = env.new_object(
                        "android/widget/LinearLayout$LayoutParams",
                        "(II)V",
                        &[JValue::Int(-2), JValue::Int(height_px)], // -2 = WRAP_CONTENT
                    );
                    if let Ok(params) = params {
                        let _ = env.call_method(
                            view_ref.as_obj(),
                            "setLayoutParams",
                            "(Landroid/view/ViewGroup$LayoutParams;)V",
                            &[JValue::Object(&params)],
                        );
                    }
                }
            }
        }
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Remove a single child view from a parent ViewGroup.
pub fn remove_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent_ref), Some(child_ref)) =
        (get_widget(parent_handle), get_widget(child_handle))
    {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let _ = env.call_method(
            parent_ref.as_obj(),
            "removeView",
            "(Landroid/view/View;)V",
            &[JValue::Object(child_ref.as_obj())],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Reorder a child widget within a parent ViewGroup by index.
pub fn reorder_child(parent_handle: i64, from_index: i64, to_index: i64) {
    if let Some(parent_ref) = get_widget(parent_handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        // Get child at from_index
        let child_result = env.call_method(
            parent_ref.as_obj(),
            "getChildAt",
            "(I)Landroid/view/View;",
            &[JValue::Int(from_index as i32)],
        );
        if let Ok(child_val) = child_result {
            if let Ok(child_obj) = child_val.l() {
                if !child_obj.is_null() {
                    // Remove and re-add at target index
                    let _ = env.call_method(
                        parent_ref.as_obj(),
                        "removeViewAt",
                        "(I)V",
                        &[JValue::Int(from_index as i32)],
                    );
                    let _ = env.call_method(
                        parent_ref.as_obj(),
                        "addView",
                        "(Landroid/view/View;I)V",
                        &[JValue::Object(&child_obj), JValue::Int(to_index as i32)],
                    );
                }
            }
        }
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Pin a child view's width to match its parent (MATCH_PARENT).
pub fn match_parent_width(child_handle: i64) {
    if let Some(view_ref) = get_widget(child_handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        if let Ok(lp) = env.call_method(
            view_ref.as_obj(),
            "getLayoutParams",
            "()Landroid/view/ViewGroup$LayoutParams;",
            &[],
        ) {
            if let Ok(lp_obj) = lp.l() {
                if !lp_obj.is_null() {
                    let _ = env.set_field(&lp_obj, "width", "I", JValue::Int(-1)); // MATCH_PARENT
                    let _ = env.call_method(
                        view_ref.as_obj(),
                        "setLayoutParams",
                        "(Landroid/view/ViewGroup$LayoutParams;)V",
                        &[JValue::Object(&lp_obj)],
                    );
                }
            }
        }
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Pin a child view's height to match its parent (MATCH_PARENT).
pub fn match_parent_height(child_handle: i64) {
    if let Some(view_ref) = get_widget(child_handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        if let Ok(lp) = env.call_method(
            view_ref.as_obj(),
            "getLayoutParams",
            "()Landroid/view/ViewGroup$LayoutParams;",
            &[],
        ) {
            if let Ok(lp_obj) = lp.l() {
                if !lp_obj.is_null() {
                    let _ = env.set_field(&lp_obj, "height", "I", JValue::Int(-1)); // MATCH_PARENT
                    let _ = env.call_method(
                        view_ref.as_obj(),
                        "setLayoutParams",
                        "(Landroid/view/ViewGroup$LayoutParams;)V",
                        &[JValue::Object(&lp_obj)],
                    );
                }
            }
        }
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set detachesHiddenViews equivalent — no-op on Android.
/// LinearLayout already excludes GONE views from layout.
pub fn set_detaches_hidden_views(_handle: i64, _detaches: bool) {
    // No-op: Android LinearLayout already excludes GONE views from layout.
}

/// Set border color on a widget via GradientDrawable stroke.
pub fn set_border_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        let color = argb_color(a, r, g, b);
        // Try to reuse existing GradientDrawable
        if let Ok(bg) = env.call_method(
            view_ref.as_obj(),
            "getBackground",
            "()Landroid/graphics/drawable/Drawable;",
            &[],
        ) {
            if let Ok(bg_obj) = bg.l() {
                if !bg_obj.is_null() {
                    if env
                        .is_instance_of(&bg_obj, "android/graphics/drawable/GradientDrawable")
                        .unwrap_or(false)
                    {
                        let _ = env.call_method(
                            &bg_obj,
                            "setStroke",
                            "(II)V",
                            &[JValue::Int(2), JValue::Int(color)],
                        ); // 2px default width
                        unsafe {
                            env.pop_local_frame(&JObject::null());
                        }
                        return;
                    }
                }
            }
        }
        // Create new GradientDrawable with border
        let gd = env
            .new_object("android/graphics/drawable/GradientDrawable", "()V", &[])
            .expect("GradientDrawable");
        let _ = env.call_method(&gd, "setColor", "(I)V", &[JValue::Int(0)]); // transparent fill
        let _ = env.call_method(
            &gd,
            "setStroke",
            "(II)V",
            &[JValue::Int(2), JValue::Int(color)],
        );
        let _ = env.call_method(
            view_ref.as_obj(),
            "setBackground",
            "(Landroid/graphics/drawable/Drawable;)V",
            &[JValue::Object(&gd)],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set border width on a widget via GradientDrawable stroke.
pub fn set_border_width(handle: i64, width: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        let width_px = dp_to_px(&mut env, width as f32);
        if let Ok(bg) = env.call_method(
            view_ref.as_obj(),
            "getBackground",
            "()Landroid/graphics/drawable/Drawable;",
            &[],
        ) {
            if let Ok(bg_obj) = bg.l() {
                if !bg_obj.is_null() {
                    if env
                        .is_instance_of(&bg_obj, "android/graphics/drawable/GradientDrawable")
                        .unwrap_or(false)
                    {
                        // setStroke requires both width and color
                        let _ = env.call_method(
                            &bg_obj,
                            "setStroke",
                            "(II)V",
                            &[JValue::Int(width_px), JValue::Int(0xFF000000u32 as i32)],
                        ); // black default
                    }
                }
            }
        }
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set edge insets (padding) on a widget.
pub fn set_edge_insets(handle: i64, top: f64, left: f64, bottom: f64, right: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let t = dp_to_px(&mut env, top as f32);
        let l = dp_to_px(&mut env, left as f32);
        let b = dp_to_px(&mut env, bottom as f32);
        let r = dp_to_px(&mut env, right as f32);
        let _ = env.call_method(
            view_ref.as_obj(),
            "setPadding",
            "(IIII)V",
            &[
                JValue::Int(l),
                JValue::Int(t),
                JValue::Int(r),
                JValue::Int(b),
            ],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set view opacity (alpha) in [0.0, 1.0].
pub fn set_opacity(handle: i64, alpha: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let _ = env.call_method(
            view_ref.as_obj(),
            "setAlpha",
            "(F)V",
            &[JValue::Float(alpha as f32)],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set on-click callback for any widget (via PerryBridge).
pub fn set_on_click(handle: i64, callback: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let cb_key = crate::callback::register(callback);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "setOnClickCallback",
            "(Landroid/view/View;J)V",
            &[JValue::Object(view_ref.as_obj()), JValue::Long(cb_key)],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Set content hugging priority (layout weight hint).
/// On Android, this maps to LinearLayout.LayoutParams.weight.
/// A low hugging value means the view WANTS to expand (high weight).
pub fn set_hugging(handle: i64, priority: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);

        // Determine parent orientation to set the correct axis
        let parent_horizontal = (|| -> Option<bool> {
            let parent = env
                .call_method(
                    view_ref.as_obj(),
                    "getParent",
                    "()Landroid/view/ViewParent;",
                    &[],
                )
                .ok()?
                .l()
                .ok()?;
            if parent.is_null() {
                return None;
            }
            if !env
                .is_instance_of(&parent, "android/widget/LinearLayout")
                .unwrap_or(false)
            {
                return None;
            }
            let orient = env
                .call_method(&parent, "getOrientation", "()I", &[])
                .map(|v| v.i().unwrap_or(-1))
                .unwrap_or(-1);
            Some(orient == 0) // 0=HORIZONTAL
        })()
        .unwrap_or(false);

        // Map hugging priority to weight:
        // low hugging (< 100) = high weight (expands), high hugging = compact (wrap content)
        let expand = priority < 100.0;
        let weight = if expand { 1.0f32 } else { 0.0f32 };

        let (w, h) = if parent_horizontal {
            // HStack: weight distributes WIDTH; cross-axis (height) = MATCH_PARENT
            if expand {
                (0, -1)
            } else {
                (-2, -1)
            } // width=0+weight or WRAP_CONTENT; height=MATCH_PARENT
        } else {
            // VStack (default): weight distributes HEIGHT; cross-axis (width) = MATCH_PARENT
            if expand {
                (-1, 0)
            } else {
                (-1, -2)
            } // width=MATCH_PARENT; height=0+weight or WRAP_CONTENT
        };

        let params = env.new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(IIF)V",
            &[JValue::Int(w), JValue::Int(h), JValue::Float(weight)],
        );
        if let Ok(params) = params {
            let _ = env.call_method(
                view_ref.as_obj(),
                "setLayoutParams",
                "(Landroid/view/ViewGroup$LayoutParams;)V",
                &[JValue::Object(&params)],
            );
        }
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

fn argb_color(a: f64, r: f64, g: f64, b: f64) -> i32 {
    let ai = (a * 255.0) as u32;
    let ri = (r * 255.0) as u32;
    let gi = (g * 255.0) as u32;
    let bi = (b * 255.0) as u32;
    ((ai << 24) | (ri << 16) | (gi << 8) | bi) as i32
}

/// Convert dp to pixels via PerryBridge.
pub fn dp_to_px(env: &mut jni::JNIEnv, dp: f32) -> i32 {
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let result = env
        .call_static_method(bridge_cls, "dpToPx", "(F)I", &[JValue::Float(dp)])
        .expect("Failed to convert dp to px");
    result.i().expect("dpToPx did not return int")
}
