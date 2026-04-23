use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{msg_send, Encode, Encoding, RefEncode};
use std::cell::RefCell;

extern "C" {
    fn js_stdlib_process_pending();
    fn js_promise_run_microtasks() -> i32;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call2(closure: *const u8, arg1: f64, arg2: f64) -> f64;
}

// Raw ObjC runtime FFI for dynamic class registration
extern "C" {
    fn objc_allocateClassPair(
        superclass: *const std::ffi::c_void,
        name: *const i8,
        extra_bytes: usize,
    ) -> *mut std::ffi::c_void;
    fn objc_registerClassPair(cls: *mut std::ffi::c_void);
    fn class_addMethod(
        cls: *mut std::ffi::c_void,
        sel: *const std::ffi::c_void,
        imp: *const std::ffi::c_void,
        types: *const i8,
    ) -> bool;
    fn sel_registerName(name: *const i8) -> *const std::ffi::c_void;
    fn objc_getClass(name: *const i8) -> *const std::ffi::c_void;
}

thread_local! {
    static LOCATION_CALLBACK: RefCell<Option<f64>> = RefCell::new(None);
    /// Prevent the CLLocationManager and delegate from being deallocated.
    static RETAINED_MANAGER: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
    static RETAINED_DELEGATE: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
    static DELEGATE_REGISTERED: RefCell<bool> = RefCell::new(false);
}

/// Invoke the stored JS callback with (lat, lon), draining promises first.
unsafe fn invoke_callback(lat: f64, lon: f64) {
    let cb = LOCATION_CALLBACK.with(|c| c.borrow_mut().take());
    if let Some(closure_f64) = cb {
        js_stdlib_process_pending();
        js_promise_run_microtasks();
        let closure_ptr = js_nanbox_get_pointer(closure_f64);
        js_closure_call2(closure_ptr as *const u8, lat, lon);
    }
}

// =============================================================================
// CLLocationManagerDelegate methods (registered dynamically)
// =============================================================================

/// locationManager:didUpdateLocations:
unsafe extern "C" fn did_update_locations(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    _manager: *mut AnyObject,
    locations: *mut AnyObject,
) {
    println!("[location] didUpdateLocations called");
    // [locations lastObject]
    let location: *mut AnyObject = msg_send![locations, lastObject];
    if location.is_null() {
        println!("[location] location is null");
        invoke_callback(f64::NAN, f64::NAN);
        return;
    }
    let coord: CLLocationCoordinate2D = get_coordinate(location);
    println!("[location] got coordinates: {}, {}", coord.latitude, coord.longitude);
    invoke_callback(coord.latitude, coord.longitude);
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

/// Extract coordinate from a CLLocation* via objc_msgSend_stret / direct.
unsafe fn get_coordinate(location: *mut AnyObject) -> CLLocationCoordinate2D {
    // On arm64, structs up to 4 registers (32 bytes) are returned in registers,
    // not via stret. CLLocationCoordinate2D is 16 bytes, so direct msg_send works.
    let coord: CLLocationCoordinate2D = msg_send![location, coordinate];
    coord
}

/// locationManager:didFailWithError:
unsafe extern "C" fn did_fail_with_error(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    _manager: *mut AnyObject,
    _error: *mut AnyObject,
) {
    println!("[location] didFailWithError called");
    invoke_callback(f64::NAN, f64::NAN);
}

/// locationManagerDidChangeAuthorization:
unsafe extern "C" fn did_change_authorization(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    manager: *mut AnyObject,
) {
    // Check authorization status
    let status: i32 = msg_send![manager, authorizationStatus];
    println!("[location] didChangeAuthorization, status: {}", status);
    // 3 = authorizedWhenInUse, 4 = authorizedAlways
    if status == 3 || status == 4 {
        // Only request if we have a pending callback
        let has_callback = LOCATION_CALLBACK.with(|c| c.borrow().is_some());
        if has_callback {
            let _: () = msg_send![manager, requestLocation];
        }
    } else if status == 2 {
        // 2 = denied
        unsafe { invoke_callback(f64::NAN, f64::NAN); }
    }
    // status 0 = notDetermined — wait for user to respond
    // status 1 = restricted — wait or fail
}

/// Register the PerryLocationDelegate class dynamically at runtime.
fn register_location_delegate() {
    DELEGATE_REGISTERED.with(|reg| {
        if *reg.borrow() {
            return;
        }
        *reg.borrow_mut() = true;

        unsafe {
            let superclass = objc_getClass(c"NSObject".as_ptr());
            let cls = objc_allocateClassPair(superclass, c"PerryLocationDelegate".as_ptr(), 0);
            if cls.is_null() {
                return; // Already registered
            }

            // locationManager:didUpdateLocations: — type: v@:@@
            let sel1 = sel_registerName(c"locationManager:didUpdateLocations:".as_ptr());
            class_addMethod(cls, sel1, did_update_locations as *const std::ffi::c_void, c"v@:@@".as_ptr());

            // locationManager:didFailWithError: — type: v@:@@
            let sel2 = sel_registerName(c"locationManager:didFailWithError:".as_ptr());
            class_addMethod(cls, sel2, did_fail_with_error as *const std::ffi::c_void, c"v@:@@".as_ptr());

            // locationManagerDidChangeAuthorization: — type: v@:@
            let sel3 = sel_registerName(c"locationManagerDidChangeAuthorization:".as_ptr());
            class_addMethod(cls, sel3, did_change_authorization as *const std::ffi::c_void, c"v@:@".as_ptr());

            objc_registerClassPair(cls);
        }
    });
}

/// Request a one-shot location. Callback receives (lat, lon) on success or (NaN, NaN) on error.
/// Handles authorization automatically — if not determined, requests When In Use first.
pub fn request_location(callback: f64) {
    println!("[location] request_location called, callback bits: {:016x}", callback.to_bits());
    register_location_delegate();

    // Store the callback
    LOCATION_CALLBACK.with(|c| {
        *c.borrow_mut() = Some(callback);
    });

    unsafe {
        // Create CLLocationManager
        let mgr_cls = AnyClass::get(c"CLLocationManager").expect("CLLocationManager not found — link CoreLocation.framework");
        let manager: Retained<AnyObject> = msg_send![mgr_cls, new];
        println!("[location] CLLocationManager created");

        // Create delegate
        let del_cls = AnyClass::get(c"PerryLocationDelegate").unwrap();
        let delegate: Retained<AnyObject> = msg_send![del_cls, new];
        println!("[location] delegate created");

        // Set delegate on manager
        let _: () = msg_send![&*manager, setDelegate: &*delegate];

        // Check current authorization status
        let status: i32 = msg_send![&*manager, authorizationStatus];
        println!("[location] authorization status: {}", status);

        // Retain manager and delegate so they stay alive
        RETAINED_MANAGER.with(|m| { *m.borrow_mut() = Some(manager.clone()); });
        RETAINED_DELEGATE.with(|d| { *d.borrow_mut() = Some(delegate); });

        if status == 3 || status == 4 {
            println!("[location] already authorized, requesting location");
            let _: () = msg_send![&*manager, requestLocation];
        } else if status == 0 {
            println!("[location] not determined, requesting authorization");
            let _: () = msg_send![&*manager, requestWhenInUseAuthorization];
        } else {
            println!("[location] denied/restricted (status={}), sending NaN", status);
            invoke_callback(f64::NAN, f64::NAN);
        }
    }
}
