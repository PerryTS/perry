//! Network reachability (issue #582) — `NWPathMonitor`-backed macOS implementation.
//!
//! Mirrors `crates/perry-ui-ios/src/network.rs`. Single process-wide monitor
//! on the main dispatch queue; cached status feeds synchronous `getStatus`
//! calls; subscribers in `LISTENERS` fire on every path update. Connection
//! types map onto `"wifi" | "cellular" | "ethernet" | "none" | "unknown"`
//! (cellular is detectable on macOS too — Mac Catalyst apps and tethered
//! iPhone connections both surface as `nw_interface_type_cellular`).

use crate::ffi::js_string_from_bytes;
use block2::RcBlock;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, Ordering};

extern "C" {
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call2(closure: *const u8, arg0: f64, arg1: f64) -> f64;
    fn js_nanbox_string(ptr: i64) -> f64;
}

extern "C" {
    fn nw_path_monitor_create() -> *mut c_void;
    fn nw_path_monitor_set_update_handler(
        monitor: *mut c_void,
        update_handler: *const block2::Block<dyn Fn(*mut c_void)>,
    );
    fn nw_path_monitor_set_queue(monitor: *mut c_void, queue: *mut c_void);
    fn nw_path_monitor_start(monitor: *mut c_void);
    fn nw_path_get_status(path: *mut c_void) -> i32;
    fn nw_path_uses_interface_type(path: *mut c_void, kind: i32) -> bool;

    static _dispatch_main_q: u8;
}

fn dispatch_main_queue() -> *mut c_void {
    unsafe { &_dispatch_main_q as *const u8 as *mut c_void }
}

const NW_PATH_STATUS_SATISFIED: i32 = 1;
const NW_INTERFACE_TYPE_WIFI: i32 = 1;
const NW_INTERFACE_TYPE_CELLULAR: i32 = 2;
const NW_INTERFACE_TYPE_WIRED: i32 = 3;

const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

#[derive(Copy, Clone)]
struct Status {
    connected: bool,
    kind: &'static str,
    initialized: bool,
}

impl Status {
    const fn unknown() -> Self {
        Self {
            connected: false,
            kind: "unknown",
            initialized: false,
        }
    }
}

thread_local! {
    static MONITOR: RefCell<Option<*mut c_void>> = const { RefCell::new(None) };
    static MONITOR_BLOCK: RefCell<Option<RcBlock<dyn Fn(*mut c_void)>>> = const { RefCell::new(None) };
    static CACHED: RefCell<Status> = const { RefCell::new(Status::unknown()) };
    static LISTENERS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
}
static NEXT_LISTENER_ID: AtomicI64 = AtomicI64::new(1);

unsafe fn nanbox_str(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    js_nanbox_string(ptr as i64)
}

fn bool_to_jsvalue(b: bool) -> f64 {
    f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE })
}

unsafe fn invoke_callback(closure_f64: f64, status: Status) {
    js_run_stdlib_pump();
    js_promise_run_microtasks();
    let ptr = js_nanbox_get_pointer(closure_f64) as *const u8;
    if ptr.is_null() {
        return;
    }
    let connected = bool_to_jsvalue(status.connected);
    let kind = nanbox_str(status.kind);
    js_closure_call2(ptr, connected, kind);
}

fn classify(path: *mut c_void) -> Status {
    if path.is_null() {
        return Status {
            connected: false,
            kind: "none",
            initialized: true,
        };
    }
    let status_code = unsafe { nw_path_get_status(path) };
    if status_code != NW_PATH_STATUS_SATISFIED {
        return Status {
            connected: false,
            kind: "none",
            initialized: true,
        };
    }
    let kind = if unsafe { nw_path_uses_interface_type(path, NW_INTERFACE_TYPE_WIFI) } {
        "wifi"
    } else if unsafe { nw_path_uses_interface_type(path, NW_INTERFACE_TYPE_CELLULAR) } {
        "cellular"
    } else if unsafe { nw_path_uses_interface_type(path, NW_INTERFACE_TYPE_WIRED) } {
        "ethernet"
    } else {
        "unknown"
    };
    Status {
        connected: true,
        kind,
        initialized: true,
    }
}

fn ensure_monitor_started() {
    MONITOR.with(|m| {
        if m.borrow().is_some() {
            return;
        }
        unsafe {
            let monitor = nw_path_monitor_create();
            if monitor.is_null() {
                return;
            }
            let block = RcBlock::new(move |path: *mut c_void| {
                let status = classify(path);
                CACHED.with(|c| *c.borrow_mut() = status);
                let listeners: Vec<f64> =
                    LISTENERS.with(|l| l.borrow().values().copied().collect());
                for cb in listeners {
                    invoke_callback(cb, status);
                }
            });
            nw_path_monitor_set_update_handler(monitor, &*block);
            nw_path_monitor_set_queue(monitor, dispatch_main_queue());
            nw_path_monitor_start(monitor);
            MONITOR_BLOCK.with(|b| *b.borrow_mut() = Some(block));
            *m.borrow_mut() = Some(monitor);
        }
    });
}

pub fn get_status(callback: f64) {
    ensure_monitor_started();
    let status = CACHED.with(|c| *c.borrow());
    unsafe {
        invoke_callback(callback, status);
    }
}

pub fn on_change(callback: f64) -> f64 {
    ensure_monitor_started();
    let id = NEXT_LISTENER_ID.fetch_add(1, Ordering::Relaxed);
    LISTENERS.with(|l| {
        l.borrow_mut().insert(id, callback);
    });
    id as f64
}

pub fn stop_on_change(id: f64) {
    let id = id as i64;
    LISTENERS.with(|l| {
        l.borrow_mut().remove(&id);
    });
}
