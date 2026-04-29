use block2::{Block, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, msg_send, AnyThread, Encode, Encoding, RefEncode};
use objc2_foundation::{NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

thread_local! {
    /// Closure passed to `notificationRegisterRemote(onToken)`. Fires when
    /// `application:didRegisterForRemoteNotificationsWithDeviceToken:` runs.
    static ON_REMOTE_TOKEN_CALLBACK: RefCell<Option<f64>> = const { RefCell::new(None) };
    /// Closure passed to `notificationOnReceive(cb)`. Fires for each remote
    /// payload while the app is foregrounded.
    static ON_REMOTE_RECEIVE_CALLBACK: RefCell<Option<f64>> = const { RefCell::new(None) };
    /// Closure passed to `notificationOnBackgroundReceive(cb)`. Fires when
    /// `application:didReceiveRemoteNotification:fetchCompletionHandler:`
    /// runs (background or terminated-app delivery, #98).
    static ON_BACKGROUND_RECEIVE_CALLBACK: RefCell<Option<f64>> = const { RefCell::new(None) };
    /// In-flight UIBackgroundFetchResult completion blocks keyed by handle.
    /// Populated when the background-receive delegate fires; drained when
    /// the user's returned Promise settles (or the safety timer trips).
    static PENDING_COMPLETIONS: RefCell<HashMap<i64, RcBlock<dyn Fn(u64)>>> =
        RefCell::new(HashMap::new());
    /// Closure passed to `notificationOnTap(cb)`. Fires when the user taps
    /// a delivered notification.
    static ON_TAP_CALLBACK: RefCell<Option<f64>> = const { RefCell::new(None) };
    /// Retained `PerryNotificationDelegate` instance.
    static TAP_DELEGATE: RefCell<Option<Retained<PerryNotificationDelegate>>> = const { RefCell::new(None) };
}

/// Monotonic id used to look up a stored completion block from a Promise
/// callback's capture. Starts at 1 so 0 can stay an unambiguous "missing".
static NEXT_COMPLETION_HANDLE: AtomicI64 = AtomicI64::new(1);

/// UIBackgroundFetchResult enum values. iOS uses these as the argument to
/// the completion handler block to signal what work was performed.
const UI_BG_FETCH_NEW_DATA: u64 = 0;
#[allow(dead_code)]
const UI_BG_FETCH_NO_DATA: u64 = 1;
const UI_BG_FETCH_FAILED: u64 = 2;

extern "C" {
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut perry_runtime::string::StringHeader;
    fn js_closure_call1(closure: *const u8, arg0: f64) -> f64;
    fn js_closure_call2(closure: *const u8, arg0: f64, arg1: f64) -> f64;
    fn js_json_parse(text_ptr: *const perry_runtime::string::StringHeader) -> u64;
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
    fn js_is_truthy(value: f64) -> i32;
    fn js_value_is_promise(value: f64) -> i32;
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

pub struct PerryNotificationDelegateIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryNotificationDelegateIos"]
    #[ivars = PerryNotificationDelegateIvars]
    pub struct PerryNotificationDelegate;

    impl PerryNotificationDelegate {
        /// `userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:`
        /// — fires when the user taps a delivered notification (#97).
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive_response(
            &self,
            _center: &AnyObject,
            response: &AnyObject,
            completion: *mut AnyObject,
        ) {
            unsafe {
                dispatch_tap(response);
                if !completion.is_null() {
                    let block: *const block2::Block<dyn Fn()> = completion as *const _;
                    (*block).call(());
                }
            }
        }
    }
);

impl PerryNotificationDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryNotificationDelegateIvars);
        unsafe { msg_send![super(this), init] }
    }
}

unsafe fn dispatch_tap(response: &AnyObject) {
    let cb = ON_TAP_CALLBACK.with(|c| *c.borrow());
    let Some(callback) = cb else {
        return;
    };

    let notification: *mut AnyObject = msg_send![response, notification];
    if notification.is_null() {
        return;
    }
    let request: *mut AnyObject = msg_send![notification, request];
    if request.is_null() {
        return;
    }
    let id_str: *mut AnyObject = msg_send![request, identifier];
    let id_value = nsstring_to_perry(id_str);

    let action_id_str: *mut AnyObject = msg_send![response, actionIdentifier];
    let action_value = if action_id_str.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        let utf8: *const u8 = msg_send![action_id_str, UTF8String];
        if utf8.is_null() {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            let len = libc::strlen(utf8 as *const i8);
            let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(utf8, len));
            if s == "com.apple.UNNotificationDefaultActionIdentifier"
                || s == "com.apple.UNNotificationDismissActionIdentifier"
            {
                f64::from_bits(TAG_UNDEFINED)
            } else {
                nsstring_to_perry(action_id_str)
            }
        }
    };

    js_run_stdlib_pump();
    js_promise_run_microtasks();

    let ptr = js_nanbox_get_pointer(callback) as *const u8;
    if !ptr.is_null() {
        js_closure_call2(ptr, id_value, action_value);
    }
}

unsafe fn nsstring_to_perry(s: *mut AnyObject) -> f64 {
    if s.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let utf8: *const u8 = msg_send![s, UTF8String];
    if utf8.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let len = libc::strlen(utf8 as *const i8);
    let str_ptr = js_string_from_bytes(utf8, len as u32);
    js_nanbox_string(str_ptr as i64)
}

unsafe fn build_content(title: &str, body: &str) -> Option<Retained<AnyObject>> {
    let content_cls = AnyClass::get(c"UNMutableNotificationContent")?;
    let content: Retained<AnyObject> = msg_send![content_cls, new];
    let ns_title = NSString::from_str(title);
    let _: () = msg_send![&*content, setTitle: &*ns_title];
    let ns_body = NSString::from_str(body);
    let _: () = msg_send![&*content, setBody: &*ns_body];
    Some(content)
}

unsafe fn submit_request(identifier: &str, content: &AnyObject, trigger: &AnyObject) {
    let Some(request_cls) = AnyClass::get(c"UNNotificationRequest") else {
        return;
    };
    let ident = NSString::from_str(identifier);
    let request: Retained<AnyObject> = msg_send![
        request_cls,
        requestWithIdentifier: &*ident,
        content: content,
        trigger: trigger
    ];
    let Some(center_cls) = AnyClass::get(c"UNUserNotificationCenter") else {
        return;
    };
    let center: Retained<AnyObject> = msg_send![center_cls, currentNotificationCenter];
    let _: () = msg_send![
        &*center,
        addNotificationRequest: &*request,
        withCompletionHandler: std::ptr::null::<AnyObject>()
    ];
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Ask the user for alert + badge + sound permission (options bitmask = 7).
/// Called once from app bootstrap (`PerryAppDelegate.application:didFinishLaunchingWithOptions:`)
/// so the permission prompt fires at launch, not on every `notificationSend` call.
pub fn request_authorization() {
    unsafe {
        let Some(center_cls) = AnyClass::get(c"UNUserNotificationCenter") else {
            return;
        };
        let center: Retained<AnyObject> = msg_send![center_cls, currentNotificationCenter];
        let _: () = msg_send![
            &*center,
            requestAuthorizationWithOptions: 7i64,
            completionHandler: std::ptr::null::<AnyObject>()
        ];
    }
}

/// Store the token callback and ask UIApplication to negotiate an APNs device
/// token. The `application:didRegisterForRemoteNotificationsWithDeviceToken:`
/// delegate method on `PerryAppDelegate` is what actually invokes the closure.
/// Requires the app to be signed with the `aps-environment` entitlement and a
/// matching provisioning profile; without it the fail-to-register delegate
/// fires (logged to stderr).
pub fn register_remote(callback: f64) {
    ON_REMOTE_TOKEN_CALLBACK.with(|c| *c.borrow_mut() = Some(callback));
    unsafe {
        let Some(app_cls) = AnyClass::get(c"UIApplication") else {
            return;
        };
        let app: *mut AnyObject = msg_send![app_cls, sharedApplication];
        if app.is_null() {
            return;
        }
        let _: () = msg_send![app, registerForRemoteNotifications];
    }
}

/// Store the receive callback. Invoked from
/// `application:didReceiveRemoteNotification:` with the payload converted
/// from NSDictionary → JSON → Perry object.
pub fn on_receive(callback: f64) {
    ON_REMOTE_RECEIVE_CALLBACK.with(|c| *c.borrow_mut() = Some(callback));
}

/// Store the background-receive callback (#98). Invoked from
/// `application:didReceiveRemoteNotification:fetchCompletionHandler:` with
/// the same NSDictionary→JSON→object shape as the foreground path. The
/// returned Promise gates when iOS is told the background fetch finished.
pub fn on_background_receive(callback: f64) {
    ON_BACKGROUND_RECEIVE_CALLBACK.with(|c| *c.borrow_mut() = Some(callback));
}

/// Promise.then trampoline: looks up the stored completion block by the
/// handle stashed in capture[0], invokes it with the result code stashed in
/// capture[1], and lets the RcBlock's Drop release the underlying Block_copy.
///
/// Called by the runtime's microtask pump when the user's background
/// callback's Promise settles. Idempotent — second-fire (e.g., a Promise
/// fulfilled then later rejected via a downstream chain) finds an empty
/// slot and exits cleanly.
#[no_mangle]
unsafe extern "C" fn perry_ios_notification_completion_trampoline(
    closure: *const perry_runtime::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    let handle = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0);
    let result_code = perry_runtime::closure::js_closure_get_capture_ptr(closure, 1) as u64;
    invoke_pending_completion(handle, result_code);
    f64::from_bits(TAG_UNDEFINED)
}

/// Pop the stored completion block for `handle` (if any) and call it with
/// `result_code`. The RcBlock drop runs `_Block_release` so the heap-copied
/// block doesn't leak.
fn invoke_pending_completion(handle: i64, result_code: u64) {
    let block = PENDING_COMPLETIONS.with(|m| m.borrow_mut().remove(&handle));
    if let Some(block) = block {
        block.call((result_code,));
    }
}

/// Allocate a Perry closure whose func_ptr is the trampoline above and whose
/// captures are `(handle, result_code)`. The returned pointer is what
/// `js_promise_then` expects (a `*const ClosureHeader`).
unsafe fn make_completion_closure(handle: i64, result_code: u64) -> *const u8 {
    let closure = perry_runtime::closure::js_closure_alloc(
        perry_ios_notification_completion_trampoline as *const u8,
        2,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(closure, 0, handle);
    perry_runtime::closure::js_closure_set_capture_ptr(closure, 1, result_code as i64);
    closure as *const u8
}

/// Hex-format an APNs device token and invoke the stored closure.
pub unsafe fn dispatch_device_token(device_token: *mut AnyObject) {
    let cb = ON_REMOTE_TOKEN_CALLBACK.with(|c| *c.borrow());
    let Some(callback) = cb else {
        return;
    };
    if device_token.is_null() {
        return;
    }

    let bytes: *const u8 = msg_send![device_token, bytes];
    let length: usize = msg_send![device_token, length];
    if bytes.is_null() || length == 0 {
        return;
    }

    let slice = std::slice::from_raw_parts(bytes, length);
    let mut hex = String::with_capacity(length * 2);
    for b in slice {
        hex.push_str(&format!("{:02X}", b));
    }

    js_run_stdlib_pump();
    js_promise_run_microtasks();

    let str_ptr = js_string_from_bytes(hex.as_ptr(), hex.len() as u32);
    let boxed = js_nanbox_string(str_ptr as i64);
    let ptr = js_nanbox_get_pointer(callback) as *const u8;
    if !ptr.is_null() {
        js_closure_call1(ptr, boxed);
    }
}

/// Log an APNs registration failure.
pub unsafe fn dispatch_registration_failure(error: *mut AnyObject) {
    if error.is_null() {
        eprintln!("[perry] registerForRemoteNotifications failed (unknown error)");
        return;
    }
    let desc: *mut AnyObject = msg_send![error, localizedDescription];
    if desc.is_null() {
        eprintln!("[perry] registerForRemoteNotifications failed");
        return;
    }
    let utf8: *const u8 = msg_send![desc, UTF8String];
    if utf8.is_null() {
        eprintln!("[perry] registerForRemoteNotifications failed");
        return;
    }
    let len = libc::strlen(utf8 as *const i8);
    let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(utf8, len));
    eprintln!("[perry] registerForRemoteNotifications failed: {}", s);
}

/// Background-delivery dispatch (#98). Same NSDictionary→JSON→object shape
/// as `dispatch_remote_payload`, but routes through the background callback
/// registered via `notificationOnBackgroundReceive` and gates the iOS
/// completion handler on the user's returned Promise.
///
/// Three terminal cases:
/// 1. No callback registered → call completion with `.NoData` immediately
///    so iOS doesn't think we hung.
/// 2. Callback returned a Promise → register `.then`/`.catch` trampolines
///    that fire the completion handler with `.NewData`/`.Failed` once the
///    Promise settles. The RcBlock holding the handler lives in
///    `PENDING_COMPLETIONS` until then.
/// 3. Callback returned anything else (or no value) → treat as "synchronous
///    work done" and fire `.NewData` immediately. iOS still gets a
///    completion signal within the delegate's stack frame.
///
/// `completion` is the raw `void(^)(UIBackgroundFetchResult)` block iOS
/// hands the delegate. We `Block_copy` it (via `RcBlock::copy`) so it
/// outlives the delegate stack frame.
pub unsafe fn dispatch_remote_payload_with_completion(
    user_info: *mut AnyObject,
    completion: *mut AnyObject,
) {
    let cb = ON_BACKGROUND_RECEIVE_CALLBACK.with(|c| *c.borrow());
    let completion_block = if completion.is_null() {
        None
    } else {
        let block_ptr = completion as *mut Block<dyn Fn(u64)>;
        RcBlock::copy(block_ptr)
    };

    let Some(callback) = cb else {
        // No background handler — tell iOS we found nothing so it doesn't
        // wait for the full 30-second budget before suspending us.
        if let Some(block) = completion_block {
            block.call((UI_BG_FETCH_NO_DATA,));
        }
        return;
    };

    let Some(payload_value) = json_payload_to_perry(user_info) else {
        if let Some(block) = completion_block {
            block.call((UI_BG_FETCH_NO_DATA,));
        }
        return;
    };

    js_run_stdlib_pump();
    js_promise_run_microtasks();

    let cb_ptr = js_nanbox_get_pointer(callback) as *const u8;
    if cb_ptr.is_null() {
        if let Some(block) = completion_block {
            block.call((UI_BG_FETCH_NO_DATA,));
        }
        return;
    }
    let result = js_closure_call1(cb_ptr, payload_value);

    let Some(block) = completion_block else {
        // Caller passed a null completion (e.g., direct invocation from a
        // test harness). Drain any microtasks the callback queued so the
        // Promise it returned still resolves; iOS-side this branch never
        // hits because UIKit always passes a real block.
        js_promise_run_microtasks();
        return;
    };

    if js_value_is_promise(result) == 0 {
        // Synchronous return (or undefined) — the user's work is done.
        block.call((UI_BG_FETCH_NEW_DATA,));
        return;
    }

    let promise_ptr = js_nanbox_get_pointer(result) as *mut perry_runtime::promise::Promise;
    if promise_ptr.is_null() {
        block.call((UI_BG_FETCH_NEW_DATA,));
        return;
    }

    let handle = NEXT_COMPLETION_HANDLE.fetch_add(1, Ordering::Relaxed);
    PENDING_COMPLETIONS.with(|m| m.borrow_mut().insert(handle, block));

    let on_fulfilled = make_completion_closure(handle, UI_BG_FETCH_NEW_DATA)
        as *const perry_runtime::closure::ClosureHeader;
    let on_rejected = make_completion_closure(handle, UI_BG_FETCH_FAILED)
        as *const perry_runtime::closure::ClosureHeader;
    perry_runtime::promise::js_promise_then(promise_ptr, on_fulfilled, on_rejected);

    // Kick the pump once so an already-resolved Promise (e.g., user wrote
    // an async fn that returned without awaiting anything) fires the
    // completion handler before we hand control back to UIKit.
    js_promise_run_microtasks();
}

/// Helper: convert an NSDictionary userInfo to a Perry object via
/// NSJSONSerialization + js_json_parse. Returns None if any step fails.
unsafe fn json_payload_to_perry(user_info: *mut AnyObject) -> Option<f64> {
    if user_info.is_null() {
        return None;
    }
    let json_cls = AnyClass::get(c"NSJSONSerialization")?;
    let mut err: *mut AnyObject = std::ptr::null_mut();
    let data: *mut AnyObject = msg_send![
        json_cls,
        dataWithJSONObject: user_info,
        options: 0u64,
        error: &mut err
    ];
    if data.is_null() {
        return None;
    }
    let bytes: *const u8 = msg_send![data, bytes];
    let length: usize = msg_send![data, length];
    if bytes.is_null() || length == 0 {
        return None;
    }
    let str_ptr = js_string_from_bytes(bytes, length as u32);
    let parsed_bits = js_json_parse(str_ptr);
    Some(f64::from_bits(parsed_bits))
}

/// Convert a remote-notification userInfo NSDictionary to a Perry object via
/// NSJSONSerialization + `js_json_parse`, then invoke the stored receive
/// callback with it.
pub unsafe fn dispatch_remote_payload(user_info: *mut AnyObject) {
    let cb = ON_REMOTE_RECEIVE_CALLBACK.with(|c| *c.borrow());
    let Some(callback) = cb else {
        return;
    };
    if user_info.is_null() {
        return;
    }

    let Some(json_cls) = AnyClass::get(c"NSJSONSerialization") else {
        return;
    };
    let mut err: *mut AnyObject = std::ptr::null_mut();
    let data: *mut AnyObject = msg_send![
        json_cls,
        dataWithJSONObject: user_info,
        options: 0u64,
        error: &mut err
    ];
    if data.is_null() {
        return;
    }

    let bytes: *const u8 = msg_send![data, bytes];
    let length: usize = msg_send![data, length];
    if bytes.is_null() || length == 0 {
        return;
    }

    js_run_stdlib_pump();
    js_promise_run_microtasks();

    let str_ptr = js_string_from_bytes(bytes, length as u32);
    let parsed_bits = js_json_parse(str_ptr);
    let parsed_f64 = f64::from_bits(parsed_bits);

    let ptr = js_nanbox_get_pointer(callback) as *const u8;
    if !ptr.is_null() {
        js_closure_call1(ptr, parsed_f64);
    }
}

/// Schedule a notification firing after `seconds` (#96, interval trigger).
/// `repeats` is a NaN-boxed JS value coerced via `js_is_truthy`.
/// Per UN constraints, `repeats=true` requires `seconds >= 60`; otherwise
/// the OS rejects the trigger silently.
pub fn schedule_interval(
    id_ptr: *const u8,
    title_ptr: *const u8,
    body_ptr: *const u8,
    seconds: f64,
    repeats: f64,
) {
    let id = str_from_header(id_ptr);
    let title = str_from_header(title_ptr);
    let body = str_from_header(body_ptr);
    let repeats_bool = unsafe { js_is_truthy(repeats) != 0 };
    let interval = if seconds < 0.0 { 0.0 } else { seconds };

    unsafe {
        let Some(content) = build_content(title, body) else {
            return;
        };
        let Some(trigger_cls) = AnyClass::get(c"UNTimeIntervalNotificationTrigger") else {
            return;
        };
        let trigger: Retained<AnyObject> = msg_send![
            trigger_cls,
            triggerWithTimeInterval: interval,
            repeats: repeats_bool
        ];
        submit_request(id, &*content, &*trigger);
    }
}

/// Schedule a notification firing once at `timestamp_ms` (#96, calendar
/// trigger). The timestamp is a JS-Date-style millisecond value since the
/// Unix epoch. Decomposed into `NSDateComponents` via `NSCalendar` because
/// `UNCalendarNotificationTrigger` requires components, not an `NSDate`.
pub fn schedule_calendar(
    id_ptr: *const u8,
    title_ptr: *const u8,
    body_ptr: *const u8,
    timestamp_ms: f64,
) {
    let id = str_from_header(id_ptr);
    let title = str_from_header(title_ptr);
    let body = str_from_header(body_ptr);

    unsafe {
        let Some(content) = build_content(title, body) else {
            return;
        };
        let Some(date_cls) = AnyClass::get(c"NSDate") else {
            return;
        };
        let date: Retained<AnyObject> = msg_send![
            date_cls,
            dateWithTimeIntervalSince1970: timestamp_ms / 1000.0
        ];
        let Some(cal_cls) = AnyClass::get(c"NSCalendar") else {
            return;
        };
        let cal: Retained<AnyObject> = msg_send![cal_cls, currentCalendar];
        // NSCalendarUnit bitmask: Year(4)|Month(8)|Day(16)|Hour(32)|Minute(64)|Second(128) = 252.
        let units: u64 = 4 | 8 | 16 | 32 | 64 | 128;
        let comps: Retained<AnyObject> = msg_send![
            &*cal,
            components: units,
            fromDate: &*date
        ];
        let Some(trigger_cls) = AnyClass::get(c"UNCalendarNotificationTrigger") else {
            return;
        };
        let trigger: Retained<AnyObject> = msg_send![
            trigger_cls,
            triggerWithDateMatchingComponents: &*comps,
            repeats: false
        ];
        submit_request(id, &*content, &*trigger);
    }
}

/// Schedule a notification firing on geofence entry (#96, location trigger).
/// Uses `CLCircularRegion` (CoreLocation) wrapped in
/// `UNLocationNotificationTrigger`. CoreLocation must be linked
/// (`-framework CoreLocation` in compile.rs's iOS link line) and the app
/// must request `NSLocationWhenInUseUsageDescription` in Info.plist.
pub fn schedule_location(
    id_ptr: *const u8,
    title_ptr: *const u8,
    body_ptr: *const u8,
    lat: f64,
    lon: f64,
    radius: f64,
) {
    let id = str_from_header(id_ptr);
    let title = str_from_header(title_ptr);
    let body = str_from_header(body_ptr);

    unsafe {
        let Some(content) = build_content(title, body) else {
            return;
        };
        let Some(region_cls) = AnyClass::get(c"CLCircularRegion") else {
            eprintln!(
                "[perry] schedule_location: CoreLocation not loaded — region trigger skipped"
            );
            return;
        };
        // CLLocationCoordinate2D is two consecutive f64s — pass via the
        // `initWithCenter:radius:identifier:` selector that takes the struct
        // by value. `CLCircularRegion *` is allocated then init'd.
        let region_alloc: *mut AnyObject = msg_send![region_cls, alloc];
        let coord = CLLocationCoordinate2D {
            latitude: lat,
            longitude: lon,
        };
        let ident_ns = NSString::from_str(id);
        let region_raw: *mut AnyObject = msg_send![
            region_alloc,
            initWithCenter: coord,
            radius: radius,
            identifier: &*ident_ns
        ];
        if region_raw.is_null() {
            return;
        }
        // notifyOnEntry / notifyOnExit default to true on iOS 14+, but be explicit.
        let _: () = msg_send![region_raw, setNotifyOnEntry: true];
        let _: () = msg_send![region_raw, setNotifyOnExit: false];

        let Some(trigger_cls) = AnyClass::get(c"UNLocationNotificationTrigger") else {
            return;
        };
        let trigger: Retained<AnyObject> = msg_send![
            trigger_cls,
            triggerWithRegion: region_raw,
            repeats: false
        ];
        submit_request(id, &*content, &*trigger);
    }
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
    const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
}

/// Register the JS closure that fires on notification tap (#97). Lazily
/// creates a `PerryNotificationDelegate` instance, retains it, and assigns
/// it as the `UNUserNotificationCenter.delegate`.
pub fn set_on_tap(callback: f64) {
    ON_TAP_CALLBACK.with(|c| *c.borrow_mut() = Some(callback));
    unsafe {
        TAP_DELEGATE.with(|d| {
            let mut d = d.borrow_mut();
            if d.is_none() {
                *d = Some(PerryNotificationDelegate::new());
            }
            let Some(delegate) = d.as_ref() else {
                return;
            };
            let Some(center_cls) = AnyClass::get(c"UNUserNotificationCenter") else {
                return;
            };
            let center: Retained<AnyObject> = msg_send![center_cls, currentNotificationCenter];
            let delegate_ref: *const AnyObject = &**delegate as *const _ as *const AnyObject;
            let _: () = msg_send![&*center, setDelegate: delegate_ref];
        });
    }
}

/// Cancel a previously scheduled notification by id (#96).
pub fn cancel(id_ptr: *const u8) {
    let id = str_from_header(id_ptr);
    unsafe {
        let Some(center_cls) = AnyClass::get(c"UNUserNotificationCenter") else {
            return;
        };
        let center: Retained<AnyObject> = msg_send![center_cls, currentNotificationCenter];
        let ident = NSString::from_str(id);
        let Some(arr_cls) = AnyClass::get(c"NSArray") else {
            return;
        };
        let ident_ref: *const AnyObject = &*ident as *const NSString as *const AnyObject;
        let arr: Retained<AnyObject> = msg_send![
            arr_cls,
            arrayWithObjects: &ident_ref,
            count: 1usize
        ];
        let _: () = msg_send![&*center, removePendingNotificationRequestsWithIdentifiers: &*arr];
        let _: () = msg_send![&*center, removeDeliveredNotificationsWithIdentifiers: &*arr];
    }
}

/// Send a local notification. Relies on authorization already having been granted
/// via `request_authorization()` at app bootstrap.
pub fn send(title_ptr: *const u8, body_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    let body = str_from_header(body_ptr);

    unsafe {
        let Some(content_cls) = AnyClass::get(c"UNMutableNotificationContent") else {
            return;
        };
        let content: Retained<AnyObject> = msg_send![content_cls, new];

        let ns_title = NSString::from_str(title);
        let _: () = msg_send![&*content, setTitle: &*ns_title];

        let ns_body = NSString::from_str(body);
        let _: () = msg_send![&*content, setBody: &*ns_body];

        let Some(trigger_cls) = AnyClass::get(c"UNTimeIntervalNotificationTrigger") else {
            return;
        };
        let trigger: Retained<AnyObject> = msg_send![
            trigger_cls,
            triggerWithTimeInterval: 0.1f64,
            repeats: false
        ];

        let Some(request_cls) = AnyClass::get(c"UNNotificationRequest") else {
            return;
        };
        let ident = NSString::from_str("perry_notification");
        let request: Retained<AnyObject> = msg_send![
            request_cls,
            requestWithIdentifier: &*ident,
            content: &*content,
            trigger: &*trigger
        ];

        let Some(center_cls) = AnyClass::get(c"UNUserNotificationCenter") else {
            return;
        };
        let center: Retained<AnyObject> = msg_send![center_cls, currentNotificationCenter];

        let _: () = msg_send![
            &*center,
            addNotificationRequest: &*request,
            withCompletionHandler: std::ptr::null::<AnyObject>()
        ];
    }
}
