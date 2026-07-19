//! Geolocation API (issue #552) — CoreLocation-backed implementation.
//!
//! The legacy `location.rs` exposes a 2-arg `(lat, lon)` callback for an
//! older API; this module ships the issue-#552 surface with a 4-arg success
//! callback `(lat, lng, accuracy, timestamp)` plus a separate error callback.

use crate::ffi::sel_registerName;
use crate::ffi::class_addMethod;
use crate::ffi::js_string_from_bytes;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{msg_send, Encode, Encoding, RefEncode};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

extern "C" {
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_closure_call4(closure: *const u8, arg0: f64, arg1: f64, arg2: f64, arg3: f64) -> f64;
    fn js_nanbox_string(ptr: i64) -> f64;
}

extern "C" {
    fn objc_allocateClassPair(
        superclass: *const std::ffi::c_void,
        name: *const i8,
        extra_bytes: usize,
    ) -> *mut std::ffi::c_void;
    fn objc_registerClassPair(cls: *mut std::ffi::c_void);
    fn objc_getClass(name: *const i8) -> *const std::ffi::c_void;
}

#[repr(C)]
#[derive(Copy, Clone)]
struct CLLocationCoordinate2D {
    latitude: f64,
    longitude: f64,
}
unsafe impl Encode for CLLocationCoordinate2D {
    const ENCODING: Encoding = Encoding::Struct(
        "CLLocationCoordinate2D",
        &[Encoding::Double, Encoding::Double],
    );
}
unsafe impl RefEncode for CLLocationCoordinate2D {
    const ENCODING_REF: Encoding = Encoding::Pointer(&<Self as Encode>::ENCODING);
}

struct PendingOneShot {
    on_success: f64,
    on_error: f64,
}

struct WatchEntry {
    callback: f64,
    manager: Retained<AnyObject>,
    delegate: Retained<AnyObject>,
}

struct PermissionRequest {
    callback: f64,
    manager: Retained<AnyObject>,
    delegate: Retained<AnyObject>,
}

thread_local! {
    static PENDING_ONE_SHOTS: RefCell<HashMap<usize, PendingOneShot>> = RefCell::new(HashMap::new());
    static ONE_SHOT_RETAINED: RefCell<HashMap<usize, (Retained<AnyObject>, Retained<AnyObject>)>> =
        RefCell::new(HashMap::new());
    static WATCHES: RefCell<HashMap<i64, WatchEntry>> = RefCell::new(HashMap::new());
    static PERMISSION_REQUESTS: RefCell<HashMap<usize, PermissionRequest>> = RefCell::new(HashMap::new());
    static DELEGATE_REGISTERED: RefCell<bool> = const { RefCell::new(false) };
}
static NEXT_WATCH_ID: AtomicI64 = AtomicI64::new(1);

unsafe fn nanbox_str(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    js_nanbox_string(ptr as i64)
}

unsafe fn pump() {
    js_run_stdlib_pump();
    js_promise_run_microtasks();
}

unsafe fn invoke_success(closure_f64: f64, lat: f64, lng: f64, accuracy: f64, timestamp_ms: f64) {
    pump();
    let ptr = js_nanbox_get_pointer(closure_f64) as *const u8;
    js_closure_call4(ptr, lat, lng, accuracy, timestamp_ms);
}

unsafe fn invoke_error(closure_f64: f64, message: &str) {
    if closure_f64.to_bits() == 0x7FFC_0000_0000_0001 {
        return; // undefined — caller passed nothing
    }
    pump();
    let ptr = js_nanbox_get_pointer(closure_f64) as *const u8;
    let msg = nanbox_str(message);
    js_closure_call1(ptr, msg);
}

unsafe fn invoke_status(closure_f64: f64, status: &str) {
    pump();
    let ptr = js_nanbox_get_pointer(closure_f64) as *const u8;
    let msg = nanbox_str(status);
    js_closure_call1(ptr, msg);
}

fn auth_status_string(status: i32) -> &'static str {
    match status {
        3 | 4 => "granted",
        2 => "denied",
        1 => "restricted",
        _ => "denied",
    }
}

unsafe fn extract_location(loc: *mut AnyObject) -> Option<(f64, f64, f64, f64)> {
    if loc.is_null() {
        return None;
    }
    let coord: CLLocationCoordinate2D = msg_send![loc, coordinate];
    let accuracy: f64 = msg_send![loc, horizontalAccuracy];
    // CLLocation `timestamp` is NSDate; convert to epoch seconds.
    let ts_obj: *mut AnyObject = msg_send![loc, timestamp];
    let mut ts_ms = 0.0_f64;
    if !ts_obj.is_null() {
        let secs: f64 = msg_send![ts_obj, timeIntervalSince1970];
        ts_ms = secs * 1000.0;
    }
    Some((coord.latitude, coord.longitude, accuracy, ts_ms))
}

unsafe extern "C" fn did_update_locations(
    this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    _manager: *mut AnyObject,
    locations: *mut AnyObject,
) {
    let last: *mut AnyObject = msg_send![locations, lastObject];
    let key = this as usize;

    // 1. One-shot: take + invoke + drop the manager retain.
    let pending = PENDING_ONE_SHOTS.with(|p| p.borrow_mut().remove(&key));
    if let Some(p) = pending {
        ONE_SHOT_RETAINED.with(|r| {
            r.borrow_mut().remove(&key);
        });
        match extract_location(last) {
            Some((lat, lng, acc, ts)) => invoke_success(p.on_success, lat, lng, acc, ts),
            None => invoke_error(p.on_error, "no-location"),
        }
        return;
    }

    // 2. Watch: invoke without removing.
    let watch_cb = WATCHES.with(|w| {
        for (_id, entry) in w.borrow().iter() {
            if Retained::as_ptr(&entry.delegate) as *mut AnyObject == this {
                return Some(entry.callback);
            }
        }
        None
    });
    if let Some(cb) = watch_cb {
        if let Some((lat, lng, acc, ts)) = extract_location(last) {
            invoke_success(cb, lat, lng, acc, ts);
        }
    }
}

unsafe extern "C" fn did_fail_with_error(
    this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    _manager: *mut AnyObject,
    error: *mut AnyObject,
) {
    let key = this as usize;
    let pending = PENDING_ONE_SHOTS.with(|p| p.borrow_mut().remove(&key));
    if let Some(p) = pending {
        ONE_SHOT_RETAINED.with(|r| {
            r.borrow_mut().remove(&key);
        });
        let mut msg = "location-error".to_string();
        if !error.is_null() {
            let desc: *mut AnyObject = msg_send![error, localizedDescription];
            if !desc.is_null() {
                let utf8: *const i8 = msg_send![desc, UTF8String];
                if !utf8.is_null() {
                    msg = std::ffi::CStr::from_ptr(utf8)
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
        invoke_error(p.on_error, &msg);
    }
}

unsafe extern "C" fn did_change_authorization(
    this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    manager: *mut AnyObject,
) {
    let key = this as usize;
    let status: i32 = msg_send![manager, authorizationStatus];

    // Permission request: deliver the status string, drop retain.
    let perm = PERMISSION_REQUESTS.with(|p| p.borrow_mut().remove(&key));
    if let Some(p) = perm {
        invoke_status(p.callback, auth_status_string(status));
        // Manager + delegate auto-drop here.
        return;
    }

    let pending_one_shot = PENDING_ONE_SHOTS.with(|p| p.borrow().contains_key(&key));
    if pending_one_shot {
        if status == 3 || status == 4 {
            let _: () = msg_send![manager, requestLocation];
        } else if status == 1 || status == 2 {
            let pending = PENDING_ONE_SHOTS.with(|p| p.borrow_mut().remove(&key));
            ONE_SHOT_RETAINED.with(|r| {
                r.borrow_mut().remove(&key);
            });
            if let Some(p) = pending {
                invoke_error(p.on_error, "permission-denied");
            }
        }
    }
}

fn register_delegate_class() {
    DELEGATE_REGISTERED.with(|reg| {
        if *reg.borrow() {
            return;
        }
        *reg.borrow_mut() = true;
        unsafe {
            let superclass = objc_getClass(c"NSObject".as_ptr());
            let cls = objc_allocateClassPair(superclass, c"PerryGeolocationDelegate".as_ptr(), 0);
            if cls.is_null() {
                return;
            }
            class_addMethod(
                cls,
                sel_registerName(c"locationManager:didUpdateLocations:".as_ptr()),
                did_update_locations as *const std::ffi::c_void,
                c"v@:@@".as_ptr(),
            );
            class_addMethod(
                cls,
                sel_registerName(c"locationManager:didFailWithError:".as_ptr()),
                did_fail_with_error as *const std::ffi::c_void,
                c"v@:@@".as_ptr(),
            );
            class_addMethod(
                cls,
                sel_registerName(c"locationManagerDidChangeAuthorization:".as_ptr()),
                did_change_authorization as *const std::ffi::c_void,
                c"v@:@".as_ptr(),
            );
            objc_registerClassPair(cls);
        }
    });
}

unsafe fn make_manager_and_delegate() -> (Retained<AnyObject>, Retained<AnyObject>) {
    let mgr_cls = AnyClass::get(c"CLLocationManager")
        .expect("CLLocationManager not found — link CoreLocation.framework");
    let manager: Retained<AnyObject> = msg_send![mgr_cls, new];
    let del_cls = AnyClass::get(c"PerryGeolocationDelegate").unwrap();
    let delegate: Retained<AnyObject> = msg_send![del_cls, new];
    let _: () = msg_send![&*manager, setDelegate: &*delegate];
    (manager, delegate)
}

pub fn get_current(on_success: f64, on_error: f64) {
    register_delegate_class();
    unsafe {
        let (manager, delegate) = make_manager_and_delegate();
        let key = Retained::as_ptr(&delegate) as *const AnyObject as usize;

        PENDING_ONE_SHOTS.with(|p| {
            p.borrow_mut().insert(
                key,
                PendingOneShot {
                    on_success,
                    on_error,
                },
            );
        });
        ONE_SHOT_RETAINED.with(|r| {
            r.borrow_mut().insert(key, (manager.clone(), delegate));
        });

        let status: i32 = msg_send![&*manager, authorizationStatus];
        if status == 3 || status == 4 {
            let _: () = msg_send![&*manager, requestLocation];
        } else if status == 0 {
            let _: () = msg_send![&*manager, requestWhenInUseAuthorization];
        } else {
            let pending = PENDING_ONE_SHOTS.with(|p| p.borrow_mut().remove(&key));
            ONE_SHOT_RETAINED.with(|r| {
                r.borrow_mut().remove(&key);
            });
            if let Some(p) = pending {
                invoke_error(p.on_error, "permission-denied");
            }
        }
    }
}

pub fn watch(callback: f64) -> f64 {
    register_delegate_class();
    let id = NEXT_WATCH_ID.fetch_add(1, Ordering::Relaxed);
    unsafe {
        let (manager, delegate) = make_manager_and_delegate();
        let _: () = msg_send![&*manager, startUpdatingLocation];
        WATCHES.with(|w| {
            w.borrow_mut().insert(
                id,
                WatchEntry {
                    callback,
                    manager,
                    delegate,
                },
            );
        });
    }
    id as f64
}

pub fn stop_watch(id: f64) {
    let id = id as i64;
    let entry = WATCHES.with(|w| w.borrow_mut().remove(&id));
    if let Some(entry) = entry {
        unsafe {
            let _: () = msg_send![&*entry.manager, stopUpdatingLocation];
        }
        let _ = entry.delegate; // drop
    }
}

pub fn request_permission(callback: f64) {
    register_delegate_class();
    unsafe {
        let (manager, delegate) = make_manager_and_delegate();
        let key = Retained::as_ptr(&delegate) as *const AnyObject as usize;
        let status: i32 = msg_send![&*manager, authorizationStatus];

        if status != 0 {
            // Already determined — fire immediately.
            invoke_status(callback, auth_status_string(status));
            return;
        }

        PERMISSION_REQUESTS.with(|p| {
            p.borrow_mut().insert(
                key,
                PermissionRequest {
                    callback,
                    manager: manager.clone(),
                    delegate,
                },
            );
        });
        let _: () = msg_send![&*manager, requestWhenInUseAuthorization];
    }
}
