use crate::ffi::js_string_from_bytes;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, msg_send, AnyThread};
use objc2_foundation::{NSObject, NSString};
use std::cell::RefCell;

thread_local! {
    /// Closure passed to `notificationRegisterRemote(onToken)`. Fires when
    /// `application:didRegisterForRemoteNotificationsWithDeviceToken:` runs.
    static ON_REMOTE_TOKEN_CALLBACK: RefCell<Option<f64>> = const { RefCell::new(None) };
    /// Closure passed to `notificationOnReceive(cb)`. Fires for each remote
    /// payload while the app is foregrounded.
    static ON_REMOTE_RECEIVE_CALLBACK: RefCell<Option<f64>> = const { RefCell::new(None) };
    /// Closure passed to `notificationOnTap(cb)`. Fires when the user taps a
    /// delivered notification. Receives `(id, action?)` — `action` is the
    /// action-button identifier or `undefined` for the default banner tap.
    static ON_TAP_CALLBACK: RefCell<Option<f64>> = const { RefCell::new(None) };
    /// Retained `PerryNotificationDelegate` instance. Lazily created when
    /// `set_on_tap` is called and reused for the lifetime of the app.
    static TAP_DELEGATE: RefCell<Option<Retained<PerryNotificationDelegate>>> = const { RefCell::new(None) };
}

extern "C" {
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_closure_call1(closure: *const u8, arg0: f64) -> f64;
    fn js_closure_call2(closure: *const u8, arg0: f64, arg1: f64) -> f64;
    fn js_json_parse(text_ptr: *const crate::string_header::StringHeader) -> u64;
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
    fn js_is_truthy(value: f64) -> i32;
}

/// `f64::from_bits(TAG_UNDEFINED)` — used as the `action` argument when the
/// user tapped the notification banner (no action button).
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

pub struct PerryNotificationDelegateIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryNotificationDelegate"]
    #[ivars = PerryNotificationDelegateIvars]
    pub struct PerryNotificationDelegate;

    impl PerryNotificationDelegate {
        /// `userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:`
        /// — fires when the user taps a delivered notification (#97). Extracts
        /// the notification id and action id from the response and dispatches
        /// the registered tap callback with them, then calls the completion
        /// handler so UN can finalize the response.
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

/// Pull notification id + action id off a `UNNotificationResponse` and
/// invoke the stored tap callback with them. `action` is `undefined` for the
/// default banner tap (when the action identifier equals
/// `UNNotificationDefaultActionIdentifier`); otherwise it's the custom
/// action id passed to `UNNotificationAction.actionWithIdentifier:`.
unsafe fn dispatch_tap(response: &AnyObject) {
    let cb = ON_TAP_CALLBACK.with(|c| *c.borrow());
    let Some(callback) = cb else {
        return;
    };

    // response.notification.request.identifier — UTF8String onto the Perry heap.
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

    // response.actionIdentifier — string. If it equals
    // `UNNotificationDefaultActionIdentifier` (`com.apple.UNNotificationDefaultActionIdentifier`),
    // pass `undefined` to JS; if it equals `UNNotificationDismissActionIdentifier`,
    // also `undefined` for now (action-button registration isn't wired yet).
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
            // Apple's default identifier — the user just tapped the banner.
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

/// Copy an `NSString *` onto Perry's heap and return a NaN-boxed string
/// JSValue (`f64::from_bits` of the boxed bits). Returns `undefined` if the
/// argument is null.
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

/// Build a `UNMutableNotificationContent` with title + body. Caller is
/// responsible for keeping the returned `Retained<AnyObject>` alive long
/// enough to attach it to the trigger / request chain.
unsafe fn build_content(title: &str, body: &str) -> Option<Retained<AnyObject>> {
    let content_cls = AnyClass::get(c"UNMutableNotificationContent")?;
    let content: Retained<AnyObject> = msg_send![content_cls, new];
    let ns_title = NSString::from_str(title);
    let _: () = msg_send![&*content, setTitle: &*ns_title];
    let ns_body = NSString::from_str(body);
    let _: () = msg_send![&*content, setBody: &*ns_body];
    Some(content)
}

/// Submit a `UNNotificationRequest` with the given identifier + content +
/// trigger to the current notification center.
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
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Send a local notification with title and body.
/// Note: On macOS, the app must be bundled (.app) for notifications to display.
pub fn send(title_ptr: *const u8, body_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    let body = str_from_header(body_ptr);

    unsafe {
        // Create UNMutableNotificationContent
        let content_cls = AnyClass::get(c"UNMutableNotificationContent");
        if content_cls.is_none() {
            // UNUserNotificationCenter not available — fall back to NSUserNotification (deprecated but works unbundled)
            send_legacy(title, body);
            return;
        }
        let content_cls = content_cls.unwrap();
        let content: Retained<AnyObject> = msg_send![content_cls, new];

        let ns_title = NSString::from_str(title);
        let _: () = msg_send![&*content, setTitle: &*ns_title];

        let ns_body = NSString::from_str(body);
        let _: () = msg_send![&*content, setBody: &*ns_body];

        // Create trigger (immediate)
        let trigger_cls = AnyClass::get(c"UNTimeIntervalNotificationTrigger").unwrap();
        let trigger: Retained<AnyObject> =
            msg_send![trigger_cls, triggerWithTimeInterval: 0.1f64, repeats: false];

        // Create request
        let request_cls = AnyClass::get(c"UNNotificationRequest").unwrap();
        let ident = NSString::from_str("perry_notification");
        let request: Retained<AnyObject> = msg_send![request_cls, requestWithIdentifier: &*ident, content: &*content, trigger: &*trigger];

        // Get notification center and add request
        let center_cls = AnyClass::get(c"UNUserNotificationCenter").unwrap();
        let center: Retained<AnyObject> = msg_send![center_cls, currentNotificationCenter];

        // Request authorization first
        let _: () = msg_send![&*center, requestAuthorizationWithOptions: 7i64, completionHandler: std::ptr::null::<AnyObject>()];

        let _: () = msg_send![&*center, addNotificationRequest: &*request, withCompletionHandler: std::ptr::null::<AnyObject>()];
    }
}

/// Store the token callback and ask NSApplication to negotiate an APNs device
/// token. The `application:didRegisterForRemoteNotificationsWithDeviceToken:`
/// delegate method on `PerryAppDelegate` is what actually invokes the closure
/// once APNs responds. Requires the app to be code-signed with the
/// `aps-environment` entitlement; unsigned/dev builds hit the
/// fail-to-register delegate (logged to stderr).
pub fn register_remote(callback: f64) {
    ON_REMOTE_TOKEN_CALLBACK.with(|c| *c.borrow_mut() = Some(callback));
    unsafe {
        let Some(app_cls) = AnyClass::get(c"NSApplication") else {
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

/// Log an APNs registration failure. #95 doesn't expose an onError callback
/// on the TS surface — that's a separate enhancement if asked for.
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
/// Per UN constraints, `repeats=true` requires `seconds >= 60`; otherwise the
/// OS rejects the trigger silently.
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
        submit_request(id, &content, &trigger);
    }
}

/// Schedule a notification firing once at `timestamp_ms` (#96, calendar
/// trigger). The timestamp is a JS-Date-style millisecond value since the
/// Unix epoch. Decomposed into `NSDateComponents` (year/month/day/hour/
/// minute/second) via `NSCalendar.currentCalendar` because
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
        // NSCalendarUnit bitmask: Year(4) | Month(8) | Day(16) | Hour(32) |
        // Minute(64) | Second(128) = 252.
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
        submit_request(id, &content, &trigger);
    }
}

/// Schedule a notification firing on geofence entry (#96, location trigger).
///
/// Logged-no-op on macOS: `UNLocationNotificationTrigger` is iOS-only.
/// CoreLocation on the desktop OS uses `CLLocationManager`-pushed updates
/// rather than UN-side region triggers; surfacing that as a separate API
/// is out of scope for #96.
pub fn schedule_location(
    _id_ptr: *const u8,
    _title_ptr: *const u8,
    _body_ptr: *const u8,
    _lat: f64,
    _lon: f64,
    _radius: f64,
) {
    eprintln!(
        "[perry] notificationSchedule: trigger.type=\"location\" is iOS-only; \
         macOS has no UNLocationNotificationTrigger equivalent."
    );
}

/// Register the JS closure that fires on notification tap (#97). Lazily
/// creates a `PerryNotificationDelegate` instance, retains it in a thread-
/// local, and assigns it as the `UNUserNotificationCenter.delegate`. Calling
/// `set_on_tap` twice replaces the callback but keeps the delegate.
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

/// Fallback for non-bundled apps using deprecated NSUserNotification
unsafe fn send_legacy(title: &str, body: &str) {
    let notif_cls = AnyClass::get(c"NSUserNotification");
    if notif_cls.is_none() {
        return;
    }
    let notif: Retained<AnyObject> = msg_send![notif_cls.unwrap(), new];

    let ns_title = NSString::from_str(title);
    let _: () = msg_send![&*notif, setTitle: &*ns_title];

    let ns_body = NSString::from_str(body);
    let _: () = msg_send![&*notif, setInformativeText: &*ns_body];

    let center_cls = AnyClass::get(c"NSUserNotificationCenter");
    if center_cls.is_none() {
        return;
    }
    let center: *mut AnyObject = msg_send![center_cls.unwrap(), defaultUserNotificationCenter];
    if !center.is_null() {
        let _: () = msg_send![center, deliverNotification: &*notif];
    }
}
