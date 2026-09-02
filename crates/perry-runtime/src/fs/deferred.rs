//! #9442 — deferred async fs writes.
//!
//! Node's `fs.writeFile` / `fs.appendFile` — both the callback form and the
//! `fs/promises` form — hand the work to the libuv thread pool and return.
//! Nothing has touched the file when the call returns, and `process.exit()`
//! does not drain that pool: an exit in the same tick abandons the write.
//!
//! Perry did the write SYNCHRONOUSLY inside the call and returned an
//! already-settled promise (or a deferred callback over an already-committed
//! write). There was no in-flight work left for `process.exit()` to abandon, so
//! five fire-and-forget `appendFile` calls followed by `process.exit(0)` landed
//! five records where Node lands none — perry committed state a program had
//! deliberately walked away from.
//!
//! The fix keeps the work synchronous but moves it OFF the calling turn: the
//! operation is parked on a zero-delay callback timer, the same mechanism
//! `fs::stream`'s `schedule_drain` uses. That buys three properties at once,
//! all of them already load-bearing elsewhere in the runtime, none of them
//! newly invented here:
//!
//!   * the timer queue GC-roots the closure and therefore its captured path /
//!     data / options / sink values, so they survive the turn;
//!   * a pending callback timer is a live event source
//!     (`js_callback_timer_has_pending`), so a program that ends by draining
//!     its loop still lands every record, and an `await` on the returned
//!     promise still resolves;
//!   * `process.exit()` terminates through `libc::_exit` without ticking the
//!     timer queues, so the parked write is dropped exactly as Node drops it.
//!
//! Argument validation that THROWS stays on the calling turn (see the
//! `validate::*` calls in the schedulers below), so a bad path or a bad options
//! object still raises where it did before rather than turning into an uncaught
//! exception inside a timer.

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64, ClosureHeader,
};

use super::validate;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

/// Capture layout for the parked operation. Slot 4 holds `Mode`.
const CAP_PATH: u32 = 0;
const CAP_DATA: u32 = 1;
const CAP_OPTIONS: u32 = 2;
const CAP_SINK: u32 = 3;
const CAP_MODE: u32 = 4;
const CAPTURE_COUNT: u32 = 5;

const MODE_APPEND_PROMISE: f64 = 0.0;
const MODE_WRITE_PROMISE: f64 = 1.0;
const MODE_APPEND_CALLBACK: f64 = 2.0;
const MODE_WRITE_CALLBACK: f64 = 3.0;

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

/// Park `perform_deferred_fs_write` on the next event-loop turn with the
/// operation's five captured values.
///
/// The JS values are rooted across the closure allocation. `js_closure_alloc`
/// is a GC allocation, and under the default copied-minor scavenge a raw
/// NaN-boxed `f64` copy held only in a Rust local names from-space the moment
/// one runs — the same shape as the bare-pointer-across-allocation class the
/// moving collector made live. The closure itself needs no handle here:
/// `js_set_timeout_callback` roots it for exactly this reason (see
/// `schedule_callback_timer`'s own `RuntimeHandleScope`).
fn park(path: f64, data: f64, options: f64, sink: f64, mode: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let path_handle = scope.root_nanbox_f64(path);
    let data_handle = scope.root_nanbox_f64(data);
    let options_handle = scope.root_nanbox_f64(options);
    let sink_handle = scope.root_nanbox_f64(sink);
    let closure = js_closure_alloc(perform_deferred_fs_write as *const u8, CAPTURE_COUNT);
    js_closure_set_capture_f64(closure, CAP_PATH, path_handle.get_nanbox_f64());
    js_closure_set_capture_f64(closure, CAP_DATA, data_handle.get_nanbox_f64());
    js_closure_set_capture_f64(closure, CAP_OPTIONS, options_handle.get_nanbox_f64());
    js_closure_set_capture_f64(closure, CAP_SINK, sink_handle.get_nanbox_f64());
    js_closure_set_capture_f64(closure, CAP_MODE, mode);
    // Zero delay: the operation runs on the next turn of whichever loop is
    // driving — the generated event loop, or the poll loop an `await` spins.
    // A pending callback timer is a live event source, so a program that ends
    // by draining its loop still performs it; `process.exit()` never ticks the
    // timer queues, so an exit in the same tick drops it, as Node does.
    let _ = crate::timer::js_set_timeout_callback(closure as i64, 0.0);
}

/// Park a promise-form operation and hand back its PENDING promise.
///
/// Every JS value here is held across two allocations (`js_promise_new` and,
/// inside `park`, the closure plus the timer's async-resource init), so the
/// returned promise is re-read from its handle rather than from the local that
/// created it.
fn park_promise(path: f64, data: f64, options: f64, mode: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let path_handle = scope.root_nanbox_f64(path);
    let data_handle = scope.root_nanbox_f64(data);
    let options_handle = scope.root_nanbox_f64(options);
    let promise = crate::promise::js_promise_new();
    let promise_handle = scope
        .root_nanbox_f64(f64::from_bits(
            crate::value::JSValue::pointer(promise as *const u8).bits(),
        ));
    park(
        path_handle.get_nanbox_f64(),
        data_handle.get_nanbox_f64(),
        options_handle.get_nanbox_f64(),
        promise_handle.get_nanbox_f64(),
        mode,
    );
    promise_handle.get_nanbox_f64()
}

/// `fs.promises.appendFile(path, data[, options])`.
pub(crate) fn defer_append_file_promise(path: f64, data: f64, options: f64) -> f64 {
    validate::validate_path_or_fd("path", path, "write");
    validate::validate_string_or_object_options("options", options);
    park_promise(path, data, options, MODE_APPEND_PROMISE)
}

/// `fs.promises.writeFile(path, data[, options])`.
pub(crate) fn defer_write_file_promise(path: f64, data: f64, options: f64) -> f64 {
    validate::validate_path_or_fd("path", path, "write");
    validate::validate_string_or_object_options("options", options);
    park_promise(path, data, options, MODE_WRITE_PROMISE)
}

/// `fs.appendFile(path, data[, options], cb)`.
pub(crate) fn defer_append_file_callback(
    path: f64,
    data: f64,
    options: f64,
    callback: *const ClosureHeader,
) {
    validate::validate_path_or_fd("path", path, "write");
    validate::validate_string_or_object_options("options", options);
    park(
        path,
        data,
        options,
        callback_sink_value(callback),
        MODE_APPEND_CALLBACK,
    );
}

/// `fs.writeFile(path, data[, options], cb)`.
pub(crate) fn defer_write_file_callback(
    path: f64,
    data: f64,
    options: f64,
    callback: *const ClosureHeader,
) {
    validate::validate_path_or_fd("path", path, "write");
    validate::validate_string_or_object_options("options", options);
    park(
        path,
        data,
        options,
        callback_sink_value(callback),
        MODE_WRITE_CALLBACK,
    );
}

fn callback_sink_value(callback: *const ClosureHeader) -> f64 {
    if callback.is_null() {
        undefined()
    } else {
        f64::from_bits(crate::value::JSValue::pointer(callback as *const u8).bits())
    }
}

fn callback_from_sink(sink: f64) -> *const ClosureHeader {
    super::extract_closure_ptr(sink)
}

/// The parked operation. Runs on a later event-loop turn; never on the turn
/// that scheduled it, which is the whole point.
extern "C" fn perform_deferred_fs_write(closure: *const ClosureHeader) -> f64 {
    let mode = js_closure_get_capture_f64(closure, CAP_MODE);
    // The operation allocates (error values, decoded strings), so the captured
    // JS values are read through handles rather than held raw across it — the
    // sink especially, which is used AFTER the operation returns.
    let scope = crate::gc::RuntimeHandleScope::new();
    let path_handle = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, CAP_PATH));
    let data_handle = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, CAP_DATA));
    let options_handle = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, CAP_OPTIONS));
    let sink_handle = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, CAP_SINK));

    // A throw from inside a timer callback would terminate the process; every
    // path that used to throw synchronously has already run on the calling
    // turn, so anything caught here is routed to the operation's own sink.
    let outcome = crate::exception::catch_js_throw(|| {
        let path = path_handle.get_nanbox_f64();
        let data = data_handle.get_nanbox_f64();
        let options = options_handle.get_nanbox_f64();
        let result = if mode == MODE_APPEND_PROMISE || mode == MODE_APPEND_CALLBACK {
            // Matches the pre-#9442 behaviour of both append entry points: the
            // sync helper's 0/1 status was discarded, so an I/O failure settles
            // as success. Deferral must not quietly change that contract.
            let _ = super::js_fs_append_file_sync_options(path, data, options);
            Ok(())
        } else {
            unsafe { super::write_file_path_or_fd_result(path, data, options) }
        };
        match result {
            Ok(()) => undefined(),
            Err(err) => err,
        }
    });

    let error_value = match outcome {
        Ok(value) => {
            if crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
                None
            } else {
                Some(value)
            }
        }
        Err(thrown) => Some(thrown),
    };

    let sink = sink_handle.get_nanbox_f64();
    if mode == MODE_APPEND_PROMISE || mode == MODE_WRITE_PROMISE {
        let promise = crate::value::js_nanbox_get_pointer(sink) as *mut crate::promise::Promise;
        if !promise.is_null() {
            match error_value {
                None => crate::promise::js_promise_resolve(promise, undefined()),
                Some(err) => crate::promise::js_promise_reject(promise, err),
            }
        }
    } else {
        let callback = callback_from_sink(sink);
        if !callback.is_null() {
            match error_value {
                None => super::callbacks::call_cb0(callback),
                Some(err) => unsafe { super::callbacks::call_cb_err1(callback, err) },
            }
        }
    }
    undefined()
}
