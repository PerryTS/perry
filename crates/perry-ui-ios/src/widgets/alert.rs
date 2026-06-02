//! Issue #708 — `alertWithButtons` modal dialog for iOS via UIAlertController.

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::NSString;
use perry_runtime::string::StringHeader;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_array_get_length(arr: i64) -> i64;
    fn js_array_get_element(arr: i64, index: i64) -> f64;
    fn js_get_string_pointer_unified(val: f64) -> i64;
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

unsafe extern "C" fn alert_callback_trampoline(ctx: *mut std::ffi::c_void) {
    let _ = std::panic::catch_unwind(|| {
        let pkg = Box::from_raw(ctx as *mut (f64, i64));
        let (closure, idx) = *pkg;
        let ptr = js_nanbox_get_pointer(closure) as *const u8;
        if !ptr.is_null() {
            js_closure_call1(ptr, idx as f64);
        }
    });
}

/// Locate the topmost view controller for presenting modals. Walks up from
/// the keyWindow's rootViewController past any already-presented modals so
/// we can stack alerts.
unsafe fn topmost_view_controller() -> *mut AnyObject {
    let app_cls = match AnyClass::get(c"UIApplication") {
        Some(c) => c,
        None => return std::ptr::null_mut(),
    };
    let app: *mut AnyObject = msg_send![app_cls, sharedApplication];
    if app.is_null() {
        return std::ptr::null_mut();
    }

    // Scene-based lookup (iOS 13+).
    let mut root_vc: *mut AnyObject = std::ptr::null_mut();
    let scenes: *mut AnyObject = msg_send![app, connectedScenes];
    if !scenes.is_null() {
        let enumerator: *mut AnyObject = msg_send![scenes, objectEnumerator];
        if !enumerator.is_null() {
            loop {
                let scene: *mut AnyObject = msg_send![enumerator, nextObject];
                if scene.is_null() {
                    break;
                }
                if let Some(ws_cls) = AnyClass::get(c"UIWindowScene") {
                    let is_ws: bool = msg_send![scene, isKindOfClass: ws_cls];
                    if !is_ws {
                        continue;
                    }
                }
                let windows: *mut AnyObject = msg_send![scene, windows];
                if windows.is_null() {
                    continue;
                }
                let count: i64 = msg_send![windows, count];
                for i in 0..count {
                    let w: *mut AnyObject = msg_send![windows, objectAtIndex: i as u64];
                    if w.is_null() {
                        continue;
                    }
                    let is_key: bool = msg_send![w, isKeyWindow];
                    if is_key {
                        root_vc = msg_send![w, rootViewController];
                        break;
                    }
                }
                if !root_vc.is_null() {
                    break;
                }
            }
        }
    }

    if root_vc.is_null() {
        // Deprecated fallback for very old contexts.
        let key_window: *mut AnyObject = msg_send![app, keyWindow];
        if !key_window.is_null() {
            root_vc = msg_send![key_window, rootViewController];
        }
    }

    while !root_vc.is_null() {
        let presented: *mut AnyObject = msg_send![root_vc, presentedViewController];
        if presented.is_null() {
            break;
        }
        root_vc = presented;
    }
    root_vc
}

/// Show a UIAlertController (.alert style) with the given buttons.
/// `arr_ptr` is the raw (already-unboxed) pointer to a JS array of strings.
/// `callback` is a NaN-boxed closure receiving the 0-based button index.
pub fn show(title_ptr: *const u8, message_ptr: *const u8, arr_ptr: i64, callback: f64) {
    let title = str_from_header(title_ptr).to_string();
    let message = str_from_header(message_ptr).to_string();

    // Collect labels up-front so the dispatch closure owns them.
    let mut labels: Vec<String> = Vec::new();
    if arr_ptr != 0 {
        let len = unsafe { js_array_get_length(arr_ptr) };
        for i in 0..len {
            let elem = unsafe { js_array_get_element(arr_ptr, i) };
            let str_ptr = unsafe { js_get_string_pointer_unified(elem) } as *const u8;
            labels.push(str_from_header(str_ptr).to_string());
        }
    }
    if labels.is_empty() {
        labels.push("OK".to_string());
    }

    // Dispatch presentation onto the main queue. The runtime call site
    // may not already be on the main thread, and UIAlertController must
    // be created + presented from the main thread.
    let pkg = Box::new((title, message, labels, callback));
    unsafe {
        dispatch_async_f(
            &_dispatch_main_q as *const _ as *const std::ffi::c_void,
            Box::into_raw(pkg) as *mut std::ffi::c_void,
            present_alert_trampoline,
        );
    }
}

unsafe extern "C" fn present_alert_trampoline(ctx: *mut std::ffi::c_void) {
    let _ = std::panic::catch_unwind(|| {
        let pkg = Box::from_raw(ctx as *mut (String, String, Vec<String>, f64));
        let (title, message, labels, callback) = *pkg;
        present_alert(&title, &message, &labels, callback);
    });
}

unsafe fn present_alert(title: &str, message: &str, labels: &[String], callback: f64) {
    let alert_cls = match AnyClass::get(c"UIAlertController") {
        Some(c) => c,
        None => return,
    };
    let ns_title = NSString::from_str(title);
    let ns_message = NSString::from_str(message);
    // UIAlertControllerStyleAlert = 1
    let alert: *mut AnyObject = msg_send![
        alert_cls,
        alertControllerWithTitle: &*ns_title,
        message: &*ns_message,
        preferredStyle: 1i64
    ];
    if alert.is_null() {
        return;
    }

    let action_cls = match AnyClass::get(c"UIAlertAction") {
        Some(c) => c,
        None => return,
    };

    for (idx, label) in labels.iter().enumerate() {
        let ns_label = NSString::from_str(label);
        let idx_i = idx as i64;
        // UIAlertActionStyleDefault = 0
        let handler = block2::RcBlock::new(move |_action: *mut AnyObject| {
            if callback != 0.0 {
                let pkg = Box::new((callback, idx_i));
                dispatch_async_f(
                    &_dispatch_main_q as *const _ as *const std::ffi::c_void,
                    Box::into_raw(pkg) as *mut std::ffi::c_void,
                    alert_callback_trampoline,
                );
            }
        });
        let action: *mut AnyObject = msg_send![
            action_cls,
            actionWithTitle: &*ns_label,
            style: 0i64,
            handler: &*handler
        ];
        if !action.is_null() {
            let _: () = msg_send![alert, addAction: action];
        }
    }

    let host_vc = topmost_view_controller();
    if host_vc.is_null() {
        return;
    }
    // UIAlertController is autoreleased (Apple convention: factory methods
    // other than alloc/new/copy/mutableCopy return +0). presentViewController
    // synchronously retains the controller for its presented-VC tracking, so
    // we don't need to retain it ourselves.
    let nil: *mut AnyObject = std::ptr::null_mut();
    let _: () = msg_send![host_vc, presentViewController: alert, animated: true, completion: nil];
}

/// Simple OK-only alert. Mirrors macOS `alert(title, message)`.
pub fn show_simple(title_ptr: *const u8, message_ptr: *const u8) {
    show(title_ptr, message_ptr, 0, 0.0);
}
