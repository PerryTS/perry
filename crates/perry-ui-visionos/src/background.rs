//! Background tasks (issue #538) — BGTaskScheduler-backed visionOS implementation.
//!
//! BGTaskScheduler is available on visionOS 1.0+, with the same API surface
//! as iOS: `BGAppRefreshTaskRequest` for short refreshes, `BGProcessingTaskRequest`
//! for longer constrained work. Identifiers MUST appear in the bundle's
//! `BGTaskSchedulerPermittedIdentifiers` Info.plist array.
//!
//! visionOS apps are SwiftUI-hosted; Perry's `app_run()` runs before
//! UIApplicationMain takes over, so we call `flush_pending_registrations()`
//! from there to satisfy "register before submit" — the underlying
//! BGTaskScheduler doesn't impose a hard "must be called from
//! didFinishLaunching" check, only that registration must happen before
//! a wake-up could fire.

use objc2::class;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::NSString;
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_value_is_promise(value: f64) -> i32;
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

thread_local! {
    static HANDLERS: RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());
    static LAUNCH_COMPLETED: RefCell<bool> = const { RefCell::new(false) };
    static REGISTERED_BLOCKS: RefCell<Vec<block2::RcBlock<dyn Fn(*mut AnyObject)>>> =
        const { RefCell::new(Vec::new()) };
}

fn str_from_header(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn boolean_truthy(v: f64) -> bool {
    let bits = v.to_bits();
    if bits == TAG_TRUE {
        return true;
    }
    if bits == TAG_FALSE || bits == TAG_UNDEFINED {
        return false;
    }
    v != 0.0 && !v.is_nan()
}

unsafe fn invoke_handler(handler: f64, task: *mut AnyObject) {
    if handler.to_bits() == TAG_UNDEFINED {
        let _: () = msg_send![task, setTaskCompletedWithSuccess: true];
        return;
    }
    js_run_stdlib_pump();
    js_promise_run_microtasks();
    let ptr = js_nanbox_get_pointer(handler) as *const u8;
    let result = js_closure_call0(ptr);
    js_promise_run_microtasks();
    let _ = js_value_is_promise(result);
    let _: () = msg_send![task, setTaskCompletedWithSuccess: true];
}

pub fn register_task(identifier_ptr: *const u8, handler: f64) {
    let id = str_from_header(identifier_ptr);
    if id.is_empty() {
        return;
    }
    HANDLERS.with(|h| {
        h.borrow_mut().insert(id.clone(), handler);
    });
    let already_launched = LAUNCH_COMPLETED.with(|b| *b.borrow());
    if already_launched {
        unsafe {
            register_one(&id);
        }
    }
}

pub fn schedule(
    identifier_ptr: *const u8,
    kind_ptr: *const u8,
    earliest_start_ms: f64,
    requires_network: f64,
    requires_charging: f64,
) {
    let id = str_from_header(identifier_ptr);
    if id.is_empty() {
        return;
    }
    let kind = str_from_header(kind_ptr);
    let req_net = boolean_truthy(requires_network);
    let req_charge = boolean_truthy(requires_charging);

    unsafe {
        let scheduler_cls = match AnyClass::get(c"BGTaskScheduler") {
            Some(c) => c,
            None => return,
        };
        let scheduler: *mut AnyObject = msg_send![scheduler_cls, sharedScheduler];

        let request_cls_name = if kind == "processing" {
            c"BGProcessingTaskRequest"
        } else {
            c"BGAppRefreshTaskRequest"
        };
        let request_cls = match AnyClass::get(request_cls_name) {
            Some(c) => c,
            None => return,
        };

        let id_ns: Retained<NSString> = NSString::from_str(&id);
        let request_alloc: *mut AnyObject = msg_send![request_cls, alloc];
        let request: *mut AnyObject = msg_send![request_alloc, initWithIdentifier: &*id_ns];

        if earliest_start_ms > 0.0 && earliest_start_ms.is_finite() {
            let secs = earliest_start_ms / 1000.0;
            let date: *mut AnyObject =
                msg_send![class!(NSDate), dateWithTimeIntervalSince1970: secs];
            let _: () = msg_send![request, setEarliestBeginDate: date];
        }

        if kind == "processing" {
            let _: () = msg_send![request, setRequiresNetworkConnectivity: req_net];
            let _: () = msg_send![request, setRequiresExternalPower: req_charge];
        }

        let mut error: *mut AnyObject = std::ptr::null_mut();
        let _ok: bool = msg_send![scheduler, submitTaskRequest: request, error: &mut error];
    }
}

pub fn cancel(identifier_ptr: *const u8) {
    let id = str_from_header(identifier_ptr);
    if id.is_empty() {
        return;
    }
    unsafe {
        let scheduler_cls = match AnyClass::get(c"BGTaskScheduler") {
            Some(c) => c,
            None => return,
        };
        let scheduler: *mut AnyObject = msg_send![scheduler_cls, sharedScheduler];
        let id_ns: Retained<NSString> = NSString::from_str(&id);
        let _: () = msg_send![scheduler, cancelTaskRequestWithIdentifier: &*id_ns];
    }
}

unsafe fn register_one(identifier: &str) {
    let scheduler_cls = match AnyClass::get(c"BGTaskScheduler") {
        Some(c) => c,
        None => return,
    };
    let scheduler: *mut AnyObject = msg_send![scheduler_cls, sharedScheduler];
    let id_ns: Retained<NSString> = NSString::from_str(identifier);
    let id_owned = identifier.to_string();
    let block = block2::RcBlock::new(move |task: *mut AnyObject| {
        let handler = HANDLERS.with(|h| h.borrow().get(&id_owned).copied());
        if let Some(h) = handler {
            unsafe {
                invoke_handler(h, task);
            }
        } else if !task.is_null() {
            unsafe {
                let _: () = msg_send![task, setTaskCompletedWithSuccess: false];
            }
        }
    });
    let _ok: bool = msg_send![
        scheduler,
        registerForTaskWithIdentifier: &*id_ns,
        usingQueue: std::ptr::null_mut::<AnyObject>(),
        launchHandler: &*block
    ];
    REGISTERED_BLOCKS.with(|t| {
        t.borrow_mut().push(block);
    });
}

/// Drain pending registrations into BGTaskScheduler. Called from the Rust
/// side of `app_run` before the SwiftUI host takes over UIApplicationMain.
pub fn flush_pending_registrations() {
    let identifiers: Vec<String> = HANDLERS.with(|h| h.borrow().keys().cloned().collect());
    for id in &identifiers {
        unsafe {
            register_one(id);
        }
    }
    LAUNCH_COMPLETED.with(|b| *b.borrow_mut() = true);
}
