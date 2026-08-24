//! Scene-relative adaptive-layout information for `perry/ios` (#5536).
//!
//! UIKit's window size, traits, and safe area are the stable public signals
//! for foldable-sized displays, iPad Split View, and Stage Manager. Device
//! model checks are intentionally avoided: a single scene can move through
//! all of these layouts without the hardware changing.

use objc2::msg_send;
use objc2::runtime::{AnyObject, Sel};
use objc2_core_foundation::CGRect;
use objc2_ui_kit::UIEdgeInsets;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, Ordering};

extern "C" {
    fn js_object_alloc(class_id: u32, field_count: u32) -> *mut c_void;
    fn js_object_set_field_by_name(
        obj: *mut c_void,
        key: *const perry_runtime::string::StringHeader,
        value: f64,
    );
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut perry_runtime::string::StringHeader;
    fn js_nanbox_pointer(ptr: i64) -> f64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
}

const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

fn zero_insets() -> UIEdgeInsets {
    UIEdgeInsets {
        top: 0.0,
        left: 0.0,
        bottom: 0.0,
        right: 0.0,
    }
}

#[derive(Clone, Debug, PartialEq)]
struct LayoutSnapshot {
    width: f64,
    height: f64,
    aspect_ratio: f64,
    display_scale: f64,
    horizontal_size_class: &'static str,
    vertical_size_class: &'static str,
    orientation: &'static str,
    window_mode: &'static str,
    is_multitasking: bool,
    is_four_by_three: bool,
    system_frame_x: f64,
    system_frame_y: f64,
    system_frame_width: f64,
    system_frame_height: f64,
    is_interactively_resizing: bool,
    is_interface_orientation_locked: bool,
    safe_area: UIEdgeInsets,
}

thread_local! {
    static LISTENERS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static LAST_SNAPSHOT: RefCell<Option<LayoutSnapshot>> = const { RefCell::new(None) };
}

static NEXT_LISTENER_ID: AtomicI64 = AtomicI64::new(1);

fn size_class_name(value: isize) -> &'static str {
    match value {
        1 => "compact",
        2 => "regular",
        _ => "unspecified",
    }
}

fn nearly_equal(a: f64, b: f64) -> bool {
    (a - b).abs() <= 2.0
}

fn classify_window_mode(
    width: f64,
    height: f64,
    screen_width: f64,
    screen_height: f64,
    is_pad: bool,
) -> (&'static str, bool) {
    let fills_width = nearly_equal(width, screen_width);
    let fills_height = nearly_equal(height, screen_height);
    if fills_width && fills_height {
        return ("fullScreen", false);
    }

    let multitasking = is_pad && (!fills_width || !fills_height);
    if multitasking && fills_height && width + 2.0 < screen_width {
        ("sideBySide", true)
    } else {
        ("windowed", multitasking)
    }
}

fn is_four_by_three(width: f64, height: f64) -> bool {
    let short = width.min(height);
    let long = width.max(height);
    short > 0.0 && ((long / short) - (4.0 / 3.0)).abs() <= 0.04
}

fn current_snapshot() -> Option<LayoutSnapshot> {
    crate::app::APPS.with(|apps| {
        let apps = apps.borrow();
        let window = &apps.last()?.window;
        unsafe {
            let bounds: CGRect = msg_send![&**window, bounds];
            let screen: *mut AnyObject = msg_send![&**window, screen];
            let screen_bounds: CGRect = if screen.is_null() {
                bounds
            } else {
                msg_send![screen, bounds]
            };
            let scale: f64 = if screen.is_null() {
                1.0
            } else {
                msg_send![screen, scale]
            };

            // iOS 27's effectiveGeometry is the authoritative scene frame and
            // interactive-resize state. Selectors keep this binary compatible
            // with earlier deployment targets and SDK-built UI archives.
            let scene: *mut AnyObject = msg_send![&**window, windowScene];
            let effective_geometry_selector = Sel::register(c"effectiveGeometry");
            let has_effective_geometry = !scene.is_null()
                && msg_send![scene, respondsToSelector: effective_geometry_selector];
            let effective_geometry: *mut AnyObject = if has_effective_geometry {
                msg_send![scene, effectiveGeometry]
            } else {
                std::ptr::null_mut()
            };
            let window_frame: CGRect = msg_send![&**window, frame];
            let system_frame: CGRect = if effective_geometry.is_null() {
                window_frame
            } else {
                msg_send![effective_geometry, systemFrame]
            };
            let interactive_selector = Sel::register(c"isInteractivelyResizing");
            let is_interactively_resizing = !effective_geometry.is_null()
                && msg_send![effective_geometry, respondsToSelector: interactive_selector]
                && msg_send![effective_geometry, isInteractivelyResizing];
            let orientation_locked_selector = Sel::register(c"isInterfaceOrientationLocked");
            let is_interface_orientation_locked = !effective_geometry.is_null()
                && msg_send![effective_geometry, respondsToSelector: orientation_locked_selector]
                && msg_send![effective_geometry, isInterfaceOrientationLocked];

            let traits: *mut AnyObject = msg_send![&**window, traitCollection];
            let horizontal: isize = if traits.is_null() {
                0
            } else {
                msg_send![traits, horizontalSizeClass]
            };
            let vertical: isize = if traits.is_null() {
                0
            } else {
                msg_send![traits, verticalSizeClass]
            };
            let idiom: isize = if traits.is_null() {
                -1
            } else {
                msg_send![traits, userInterfaceIdiom]
            };

            let root: *mut AnyObject = msg_send![&**window, rootViewController];
            let safe_area = if root.is_null() {
                zero_insets()
            } else {
                let view: *mut AnyObject = msg_send![root, view];
                if view.is_null() {
                    zero_insets()
                } else {
                    msg_send![view, safeAreaInsets]
                }
            };

            let width = bounds.size.width.max(0.0);
            let height = bounds.size.height.max(0.0);
            let aspect_ratio = if height > 0.0 { width / height } else { 0.0 };
            let orientation = if nearly_equal(width, height) {
                "square"
            } else if width > height {
                "landscape"
            } else {
                "portrait"
            };
            let (window_mode, is_multitasking) = classify_window_mode(
                system_frame.size.width,
                system_frame.size.height,
                screen_bounds.size.width,
                screen_bounds.size.height,
                idiom == 1, // UIUserInterfaceIdiomPad
            );

            Some(LayoutSnapshot {
                width,
                height,
                aspect_ratio,
                display_scale: scale,
                horizontal_size_class: size_class_name(horizontal),
                vertical_size_class: size_class_name(vertical),
                orientation,
                window_mode,
                is_multitasking,
                is_four_by_three: is_four_by_three(width, height),
                system_frame_x: system_frame.origin.x,
                system_frame_y: system_frame.origin.y,
                system_frame_width: system_frame.size.width,
                system_frame_height: system_frame.size.height,
                is_interactively_resizing,
                is_interface_orientation_locked,
                safe_area,
            })
        }
    })
}

unsafe fn string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    js_nanbox_string(ptr as i64)
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

unsafe fn set_field(object: *mut c_void, name: &str, value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(object, key, value);
}

unsafe fn snapshot_object(snapshot: &LayoutSnapshot) -> i64 {
    let object = js_object_alloc(0, 20);
    if object.is_null() {
        return 0;
    }
    set_field(object, "width", snapshot.width);
    set_field(object, "height", snapshot.height);
    set_field(object, "aspectRatio", snapshot.aspect_ratio);
    set_field(object, "displayScale", snapshot.display_scale);
    set_field(
        object,
        "horizontalSizeClass",
        string_value(snapshot.horizontal_size_class),
    );
    set_field(
        object,
        "verticalSizeClass",
        string_value(snapshot.vertical_size_class),
    );
    set_field(object, "orientation", string_value(snapshot.orientation));
    set_field(object, "windowMode", string_value(snapshot.window_mode));
    set_field(
        object,
        "isMultitasking",
        bool_value(snapshot.is_multitasking),
    );
    set_field(
        object,
        "isFourByThree",
        bool_value(snapshot.is_four_by_three),
    );
    set_field(object, "systemFrameX", snapshot.system_frame_x);
    set_field(object, "systemFrameY", snapshot.system_frame_y);
    set_field(object, "systemFrameWidth", snapshot.system_frame_width);
    set_field(object, "systemFrameHeight", snapshot.system_frame_height);
    set_field(
        object,
        "isInteractivelyResizing",
        bool_value(snapshot.is_interactively_resizing),
    );
    set_field(
        object,
        "isInterfaceOrientationLocked",
        bool_value(snapshot.is_interface_orientation_locked),
    );
    set_field(object, "safeAreaTop", snapshot.safe_area.top);
    set_field(object, "safeAreaRight", snapshot.safe_area.right);
    set_field(object, "safeAreaBottom", snapshot.safe_area.bottom);
    set_field(object, "safeAreaLeft", snapshot.safe_area.left);
    object as i64
}

unsafe fn invoke_listener(callback: f64, snapshot: &LayoutSnapshot) {
    let closure = js_nanbox_get_pointer(callback) as *const u8;
    if closure.is_null() {
        return;
    }
    let object = snapshot_object(snapshot);
    if object == 0 {
        return;
    }
    js_run_stdlib_pump();
    js_closure_call1(closure, js_nanbox_pointer(object));
    js_promise_run_microtasks();
}

/// Called after UIKit lays out the root controller and after scene creation.
/// Duplicate layouts are suppressed so animation/layout passes don't flood JS.
pub(crate) fn notify_if_changed() {
    let Some(snapshot) = current_snapshot() else {
        return;
    };
    let changed = LAST_SNAPSHOT.with(|last| {
        let mut last = last.borrow_mut();
        if last.as_ref() == Some(&snapshot) {
            false
        } else {
            *last = Some(snapshot.clone());
            true
        }
    });
    if !changed {
        return;
    }
    let callbacks =
        LISTENERS.with(|listeners| listeners.borrow().values().copied().collect::<Vec<_>>());
    for callback in callbacks {
        unsafe { invoke_listener(callback, &snapshot) };
    }
}

#[no_mangle]
pub extern "C" fn perry_ios_get_layout_environment() -> i64 {
    let snapshot = current_snapshot().unwrap_or(LayoutSnapshot {
        width: 0.0,
        height: 0.0,
        aspect_ratio: 0.0,
        display_scale: 1.0,
        horizontal_size_class: "unspecified",
        vertical_size_class: "unspecified",
        orientation: "square",
        window_mode: "windowed",
        is_multitasking: false,
        is_four_by_three: false,
        system_frame_x: 0.0,
        system_frame_y: 0.0,
        system_frame_width: 0.0,
        system_frame_height: 0.0,
        is_interactively_resizing: false,
        is_interface_orientation_locked: false,
        safe_area: zero_insets(),
    });
    unsafe { snapshot_object(&snapshot) }
}

#[no_mangle]
pub extern "C" fn perry_ios_on_layout_change(callback: f64) -> i64 {
    let id = NEXT_LISTENER_ID.fetch_add(1, Ordering::Relaxed);
    LISTENERS.with(|listeners| {
        listeners.borrow_mut().insert(id, callback);
    });
    if let Some(snapshot) = current_snapshot() {
        unsafe { invoke_listener(callback, &snapshot) };
    }
    id
}

#[no_mangle]
pub extern "C" fn perry_ios_off_layout_change(subscription: f64) {
    if subscription.is_finite() && subscription > 0.0 {
        LISTENERS.with(|listeners| {
            listeners.borrow_mut().remove(&(subscription as i64));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_fullscreen_split_and_windowed_scenes() {
        assert_eq!(
            classify_window_mode(1024.0, 1366.0, 1024.0, 1366.0, true),
            ("fullScreen", false)
        );
        assert_eq!(
            classify_window_mode(507.0, 1366.0, 1024.0, 1366.0, true),
            ("sideBySide", true)
        );
        assert_eq!(
            classify_window_mode(800.0, 1000.0, 1024.0, 1366.0, true),
            ("windowed", true)
        );
    }

    #[test]
    fn detects_four_by_three_in_both_orientations() {
        assert!(is_four_by_three(1024.0, 768.0));
        assert!(is_four_by_three(768.0, 1024.0));
        assert!(!is_four_by_three(390.0, 844.0));
    }
}
