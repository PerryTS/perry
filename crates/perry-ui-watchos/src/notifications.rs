//! Local notifications (perry/system) — watchOS implementation.
//!
//! Ported from `perry-ui-ios/src/notifications.rs`. `UNUserNotificationCenter`
//! and the `UNMutableNotificationContent` / `UNTimeIntervalNotificationTrigger`
//! / `UNCalendarNotificationTrigger` classes are all available on watchOS, so
//! the local-notification path (send / schedule-interval / schedule-calendar /
//! cancel / on-tap) mirrors the iOS impl almost verbatim.
//!
//! Differences from iOS:
//! - `UNLocationNotificationTrigger` is iOS-only, so `schedule_location` stays
//!   a no-op stub in `lib.rs` (CoreLocation geofencing isn't available on
//!   watchOS). Remote push (`register_remote` / `on_receive` /
//!   `on_background_receive`) also stays stubbed — there is no `UIApplication`
//!   `registerForRemoteNotifications` on watchOS.
//! - The delegate ObjC class is registered as `PerryNotificationDelegateWatchos`
//!   so it can never collide with iOS's `PerryNotificationDelegateIos` runtime
//!   registration if both static libs are ever linked into one image.
//! - Integer option / unit masks use `usize` (NSUInteger) so they are correct
//!   on both `arm64_32` (ILP32, 32-bit NSUInteger) and 64-bit watch targets.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, msg_send, AnyThread};
use objc2_foundation::{NSObject, NSString};
use std::cell::RefCell;

extern "C" {
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
    // Matches the crate-wide declaration in audio.rs/media_playback.rs: the
    // runtime returns a `*mut StringHeader`, which the crate consistently types
    // as `i64` here (32-bit ptr zero-extended on arm64_32). Keeping the same
    // signature avoids a `clashing_extern_declarations` warning.
    fn js_string_from_bytes(data: *const u8, len: i32) -> i64;
    fn js_closure_call2(closure: *const u8, arg0: f64, arg1: f64) -> f64;
    fn js_is_truthy(value: f64) -> i32;
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

thread_local! {
    /// Closure passed to `notificationOnTap(cb)`. Fires when the user taps
    /// a delivered notification.
    static ON_TAP_CALLBACK: RefCell<Option<f64>> = const { RefCell::new(None) };
    /// Retained `PerryNotificationDelegate` instance.
    static TAP_DELEGATE: RefCell<Option<Retained<PerryNotificationDelegate>>> = const { RefCell::new(None) };
}

pub struct PerryNotificationDelegateIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryNotificationDelegateWatchos"]
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
    let str_ptr = js_string_from_bytes(utf8, len as i32);
    js_nanbox_string(str_ptr)
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
/// Called once from the app-create path (`app::app_create`, which runs during
/// `perry_main_init` on the main thread) so the permission prompt fires at
/// launch, not on every `notificationSend` call — matching the iOS flow which
/// prompts once from `PerryAppDelegate.didFinishLaunching`.
pub fn request_authorization() {
    unsafe {
        let Some(center_cls) = AnyClass::get(c"UNUserNotificationCenter") else {
            return;
        };
        let center: Retained<AnyObject> = msg_send![center_cls, currentNotificationCenter];
        // UNAuthorizationOptions is NSUInteger — pass as `usize` so the value
        // is the right width on arm64_32 (ILP32) as well as 64-bit watch.
        let _: () = msg_send![
            &*center,
            requestAuthorizationWithOptions: 7usize,
            completionHandler: std::ptr::null::<AnyObject>()
        ];
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
    // UNTimeIntervalNotificationTrigger throws NSInternalInconsistencyException
    // for a non-positive interval, and for < 60s when repeating. Clamp into the
    // valid range (also coercing NaN/inf) so a bad caller value can't crash the
    // app.
    let interval = if seconds.is_finite() { seconds } else { 0.0 };
    let interval = if repeats_bool {
        interval.max(60.0)
    } else {
        interval.max(1.0)
    };

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
/// This is the trigger the times-table app relies on for its 3×-daily
/// reminders.
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
        // NSCalendarUnit is NSUInteger — `usize` keeps it ILP32-safe.
        let units: usize = 4 | 8 | 16 | 32 | 64 | 128;
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

/// Register the JS closure that fires on notification tap (#97). Lazily
/// creates a `PerryNotificationDelegate` instance, retains it, and assigns
/// it as the `UNUserNotificationCenter.delegate`.
///
/// The compiled TS calls `notificationOnTap(cb)` during module init (inside
/// `perry_main_init`, before the WatchKit run loop starts), so the delegate is
/// installed before any tap response — including a cold launch from a tapped
/// notification — is delivered.
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

/// Send a local notification immediately. Relies on authorization already
/// having been granted via `request_authorization()` at app bootstrap.
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
