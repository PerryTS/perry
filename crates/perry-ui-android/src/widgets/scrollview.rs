use crate::callback;
use crate::jni_bridge;
use jni::objects::JValue;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Track SwipeRefreshLayout wrappers for scroll views that have refresh controls.
    static REFRESH_LAYOUTS: RefCell<HashMap<i64, i64>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}

/// Create a ScrollView. Returns widget handle.
pub fn create() -> i64 {
    unsafe {
        __android_log_print(
            3,
            b"PerryScrollView\0".as_ptr(),
            b"create: getting env\0".as_ptr(),
        );
    }
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);
    unsafe {
        __android_log_print(
            3,
            b"PerryScrollView\0".as_ptr(),
            b"create: got env, getting activity\0".as_ptr(),
        );
    }
    let activity = super::get_activity(&mut env);
    unsafe {
        __android_log_print(
            3,
            b"PerryScrollView\0".as_ptr(),
            b"create: got activity, creating ScrollView\0".as_ptr(),
        );
    }

    let scroll_result = env.new_object(
        "android/widget/ScrollView",
        "(Landroid/content/Context;)V",
        &[JValue::Object(&activity)],
    );
    let scroll = match scroll_result {
        Ok(s) => {
            unsafe {
                __android_log_print(
                    3,
                    b"PerryScrollView\0".as_ptr(),
                    b"create: ScrollView created OK\0".as_ptr(),
                );
            }
            s
        }
        Err(e) => {
            let msg = format!("Failed to create ScrollView: {:?}\0", e);
            unsafe {
                __android_log_print(
                    6,
                    b"PerryScrollView\0".as_ptr(),
                    b"create: ERROR: %s\0".as_ptr(),
                    msg.as_ptr(),
                );
            }
            // Check if there's a pending JNI exception
            if env.exception_check().unwrap_or(false) {
                unsafe {
                    __android_log_print(
                        6,
                        b"PerryScrollView\0".as_ptr(),
                        b"create: JNI exception pending, describing:\0".as_ptr(),
                    );
                }
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            panic!("Failed to create ScrollView: {:?}", e);
        }
    };

    // Fill viewport so content can expand
    let _ = env.call_method(&scroll, "setFillViewport", "(Z)V", &[JValue::Bool(1)]);

    // MATCH_PARENT for both dimensions
    let params = env
        .new_object(
            "android/widget/FrameLayout$LayoutParams",
            "(II)V",
            &[JValue::Int(-1), JValue::Int(-1)],
        )
        .expect("Failed to create LayoutParams");
    let _ = env.call_method(
        &scroll,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&params)],
    );

    let global = env
        .new_global_ref(scroll)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}

/// Set the content child of a ScrollView.
/// ScrollView can only have one direct child. Remove existing children first.
pub fn set_child(scroll_handle: i64, child_handle: i64) {
    if let (Some(scroll_ref), Some(child_ref)) = (
        super::get_widget(scroll_handle),
        super::get_widget(child_handle),
    ) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        // Remove existing children
        let _ = env.call_method(scroll_ref.as_obj(), "removeAllViews", "()V", &[]);
        // Add the new child
        let _ = env.call_method(
            scroll_ref.as_obj(),
            "addView",
            "(Landroid/view/View;)V",
            &[JValue::Object(child_ref.as_obj())],
        );
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Scroll so that the given child widget is visible.
pub fn scroll_to(_scroll_handle: i64, child_handle: i64) {
    if let Some(child_ref) = super::get_widget(child_handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        // Get the child's top position and scroll its parent to it
        let top = env
            .call_method(child_ref.as_obj(), "getTop", "()I", &[])
            .map(|v| v.i().unwrap_or(0))
            .unwrap_or(0);

        // Get the parent ScrollView
        let parent = env.call_method(
            child_ref.as_obj(),
            "getParent",
            "()Landroid/view/ViewParent;",
            &[],
        );
        if let Ok(parent_val) = parent {
            if let Ok(parent_obj) = parent_val.l() {
                if !parent_obj.is_null() {
                    let _ = env.call_method(
                        &parent_obj,
                        "smoothScrollTo",
                        "(II)V",
                        &[JValue::Int(0), JValue::Int(top)],
                    );
                }
            }
        }
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Get the vertical scroll offset.
pub fn get_offset(scroll_handle: i64) -> f64 {
    if let Some(scroll_ref) = super::get_widget(scroll_handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let result = env.call_method(scroll_ref.as_obj(), "getScrollY", "()I", &[]);
        let offset = if let Ok(val) = result {
            val.i().unwrap_or(0) as f64
        } else {
            0.0
        };
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
        return offset;
    }
    0.0
}

/// Set up a pull-to-refresh control on this ScrollView.
/// On Android, this is a no-op stub since SwipeRefreshLayout requires AndroidX.
/// The callback is stored but refresh is triggered via the scroll overscroll behavior.
pub fn set_refresh_control(_scroll_handle: i64, _callback: f64) {
    // SwipeRefreshLayout requires wrapping the ScrollView in a SwipeRefreshLayout,
    // which would require adding the AndroidX SwipeRefreshLayout dependency.
    // For now this is a no-op — pull-to-refresh is not yet supported on Android.
    // The app will still work, just without pull-to-refresh.
}

/// End the refresh animation (no-op on Android without SwipeRefreshLayout).
pub fn end_refreshing(_scroll_handle: i64) {
    // No-op — see set_refresh_control.
}

/// Set the vertical scroll offset.
pub fn set_offset(scroll_handle: i64, offset: f64) {
    if let Some(scroll_ref) = super::get_widget(scroll_handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let _ = env.call_method(
            scroll_ref.as_obj(),
            "smoothScrollTo",
            "(II)V",
            &[JValue::Int(0), JValue::Int(offset as i32)],
        );
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}
