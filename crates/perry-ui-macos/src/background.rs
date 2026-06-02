//! Background tasks (issue #538) — NSBackgroundActivityScheduler-backed
//! macOS implementation.
//!
//! macOS uses `NSBackgroundActivityScheduler` (Foundation, 10.10+) — a
//! different model from iOS's BGTaskScheduler. NSBackgroundActivityScheduler
//! schedules a block to run periodically (or once) when the system decides
//! conditions are favorable. The system can launch the app from suspend if
//! it has been registered as a `LaunchAgent` / `LaunchDaemon`, but for
//! ordinary GUI apps (Perry's typical case) the scheduler only fires while
//! the app is running.
//!
//! API mapping:
//! - `registerTask(id, handler)` — stores handler in registry.
//! - `schedule(id, kind, earliestStartMs, requiresNetwork, requiresCharging)`:
//!     · `kind == "appRefresh"` → one-shot scheduler, `interval = max(0,
//!        earliestStartMs - now) / 1000`, default 30 min if no earliest.
//!     · `kind == "processing"` → same scheduler with longer tolerance and
//!        `qualityOfService = .utility` (vs .background for appRefresh).
//!     · `requiresNetwork` → `scheduler.requiresNetworkConnectivity = ...`.
//!     · `requiresCharging` → no direct equivalent on NSBackgroundActivityScheduler;
//!        we set `qualityOfService = .background` so the OS prefers idle/charging
//!        windows when scheduling, but it's advisory.
//! - `cancel(id)` — calls `invalidate()` on the stored scheduler and drops it.

use objc2::class;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
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

const NSBG_RESULT_FINISHED: i64 = 1;
const QOS_BACKGROUND: i64 = 9;
const QOS_UTILITY: i64 = 17;

thread_local! {
    /// identifier → NaN-boxed handler closure.
    static HANDLERS: RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());
    /// identifier → retained NSBackgroundActivityScheduler. Kept here so
    /// cancel can call `invalidate()` and so the scheduler outlives this
    /// function call (Foundation drops a non-retained scheduler immediately).
    static SCHEDULERS: RefCell<HashMap<String, Retained<AnyObject>>> =
        RefCell::new(HashMap::new());
    /// Keep-alive table for the scheduleWithBlock: closures we hand over.
    static REGISTERED_BLOCKS: RefCell<Vec<block2::RcBlock<dyn Fn(*mut AnyObject)>>> =
        const { RefCell::new(Vec::new()) };
}

fn str_from_header(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
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

unsafe fn invoke_handler(handler: f64) {
    if handler.to_bits() == TAG_UNDEFINED {
        return;
    }
    js_run_stdlib_pump();
    js_promise_run_microtasks();
    let ptr = js_nanbox_get_pointer(handler) as *const u8;
    let result = js_closure_call0(ptr);
    js_promise_run_microtasks();
    let _ = js_value_is_promise(result);
}

pub fn register_task(identifier_ptr: *const u8, handler: f64) {
    let id = str_from_header(identifier_ptr);
    if id.is_empty() {
        return;
    }
    HANDLERS.with(|h| {
        h.borrow_mut().insert(id, handler);
    });
}

pub fn schedule(
    identifier_ptr: *const u8,
    kind_ptr: *const u8,
    earliest_start_ms: f64,
    requires_network: f64,
    _requires_charging: f64,
) {
    let id = str_from_header(identifier_ptr);
    if id.is_empty() {
        return;
    }
    let kind = str_from_header(kind_ptr);
    let req_net = boolean_truthy(requires_network);

    // Compute interval (seconds) from earliestStartMs.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0);
    let interval_secs = if earliest_start_ms > now_ms {
        (earliest_start_ms - now_ms) / 1000.0
    } else if earliest_start_ms > 0.0 && earliest_start_ms < now_ms {
        // earliestStart in the past — fire ASAP (1s nudge).
        1.0
    } else {
        // No earliest specified → 30 min default.
        1800.0
    };
    let tolerance_secs = if kind == "processing" {
        interval_secs / 2.0
    } else {
        interval_secs / 4.0
    };
    let qos = if kind == "processing" {
        QOS_UTILITY
    } else {
        QOS_BACKGROUND
    };

    unsafe {
        let scheduler_cls = class!(NSBackgroundActivityScheduler);
        let id_ns: Retained<NSString> = NSString::from_str(&id);
        let alloc: *mut AnyObject = msg_send![scheduler_cls, alloc];
        let scheduler_raw: *mut AnyObject = msg_send![alloc, initWithIdentifier: &*id_ns];
        if scheduler_raw.is_null() {
            return;
        }
        // Take ownership of the +1 retain returned by alloc/init.
        let scheduler: Retained<AnyObject> = Retained::from_raw(scheduler_raw).unwrap();

        let _: () = msg_send![&*scheduler, setRepeats: false];
        let _: () = msg_send![&*scheduler, setInterval: interval_secs];
        let _: () = msg_send![&*scheduler, setTolerance: tolerance_secs];
        let _: () = msg_send![&*scheduler, setRequiresNetworkConnectivity: req_net];
        let _: () = msg_send![&*scheduler, setQualityOfService: qos];

        let id_owned = id.clone();
        let block = block2::RcBlock::new(move |completion: *mut AnyObject| {
            let handler = HANDLERS.with(|h| h.borrow().get(&id_owned).copied());
            if let Some(h) = handler {
                invoke_handler(h);
            }
            // Signal the OS that we're done. completion is a block
            // taking NSBackgroundActivityResult (i64): 1 = finished.
            if !completion.is_null() {
                let block_ptr: *const block2::Block<dyn Fn(i64)> = completion as *const _;
                (*block_ptr).call((NSBG_RESULT_FINISHED,));
            }
        });
        let _: () = msg_send![&*scheduler, scheduleWithBlock: &*block];
        REGISTERED_BLOCKS.with(|t| t.borrow_mut().push(block));

        // Replace any existing scheduler for this id (invalidate the old one).
        let prev = SCHEDULERS.with(|s| s.borrow_mut().insert(id, scheduler));
        if let Some(old) = prev {
            let _: () = msg_send![&*old, invalidate];
        }
    }
}

pub fn cancel(identifier_ptr: *const u8) {
    let id = str_from_header(identifier_ptr);
    if id.is_empty() {
        return;
    }
    let scheduler = SCHEDULERS.with(|s| s.borrow_mut().remove(&id));
    if let Some(s) = scheduler {
        unsafe {
            let _: () = msg_send![&*s, invalidate];
        }
    }
}
