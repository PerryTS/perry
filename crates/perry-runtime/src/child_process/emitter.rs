use super::*;

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr, js_native_call_value,
    js_register_closure_arity, ClosureHeader,
};
use crate::object::js_implicit_this_set;
use crate::string::js_string_from_bytes;
use crate::value::JSValue;

/// Hidden field key holding the listener array for `event`.
pub(crate) fn cp_listener_key(event: &str) -> Vec<u8> {
    let mut k = b"__cpL_".to_vec();
    k.extend_from_slice(event.as_bytes());
    k
}

/// Append a listener closure to `target`'s `event` list (the `.on` body).
pub(crate) fn cp_register(target: f64, event: f64, cb: f64) {
    let name = match cp_value_to_string(event) {
        Some(n) => n,
        None => return,
    };
    let key = cp_listener_key(&name);
    let arr = match cp_array_ptr(cp_get_field(target, &key)) {
        Some(a) => a,
        None => crate::array::js_array_alloc(2),
    };
    let arr = crate::array::js_array_push_f64(arr, cb);
    cp_set_field(target, &key, cp_box_ptr(arr as *const u8));
}

/// Invoke every listener registered on `target` for `event`. Returns whether
/// any fired. The listener array is re-read each iteration so a moving GC
/// during a handler call can't strand us on a stale array pointer.
pub(crate) fn cp_emit(target: f64, event: &str, args: &[f64]) -> bool {
    if event == "message"
        && args
            .first()
            .copied()
            .is_some_and(|msg| crate::cluster::consume_internal_message(target, msg))
    {
        return true;
    }

    let key = cp_listener_key(event);
    let mut i: u32 = 0;
    let mut fired = false;
    loop {
        let arr = match cp_array_ptr(cp_get_field(target, &key)) {
            Some(a) => a,
            None => break,
        };
        if i >= crate::array::js_array_length(arr) {
            break;
        }
        let cb = crate::array::js_array_get_f64(arr, i);
        let prev = js_implicit_this_set(target);
        unsafe {
            let _ = js_native_call_value(cb, args.as_ptr(), args.len());
        }
        js_implicit_this_set(prev);
        fired = true;
        i += 1;
    }

    // `cp_build_readable` installs node:stream's async iterator on stdout/stderr,
    // and that iterator registers its `data`/`end`/`error` listeners in node:stream's
    // registry rather than the one above. Forward there too, so a `for await` over a
    // child's output sees the chunks the reactor delivers.
    crate::node_stream::emit_to_stream_listeners(target, event.as_bytes(), args);

    fired
}

// ----- method bodies (each receives the closure; slot 0 = host `this`) -----

pub(crate) extern "C" fn cp_method_on(closure: *const ClosureHeader, event: f64, cb: f64) -> f64 {
    let this = cp_this(closure);
    cp_register(this, event, cb);
    this
}
pub(crate) extern "C" fn cp_method_emit(
    closure: *const ClosureHeader,
    event: f64,
    arg: f64,
) -> f64 {
    let this = cp_this(closure);
    let name = match cp_value_to_string(event) {
        Some(n) => n,
        None => return TAG_FALSE_F64,
    };
    if cp_emit(this, &name, &[arg]) {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}
pub(crate) extern "C" fn cp_method_this0(closure: *const ClosureHeader) -> f64 {
    cp_this(closure)
}
pub(crate) extern "C" fn cp_method_this1(closure: *const ClosureHeader, _a: f64) -> f64 {
    cp_this(closure)
}
pub(crate) extern "C" fn cp_method_kill(closure: *const ClosureHeader, signal: f64) -> f64 {
    let this = cp_this(closure);
    cp_set_field(this, b"killed", TAG_TRUE_F64);
    // #1934: signal the live child if one is still running. `__cpHandle` is the
    // reactor registry key set by `spawn`. Returns true when the signal was
    // delivered (Node's `kill()` returns a boolean).
    if let Some(handle) = cp_handle_of(this) {
        if reactor::cp_live_kill(handle, signal) {
            return TAG_TRUE_F64;
        }
    }
    TAG_TRUE_F64
}
/// `child[Symbol.dispose]()` — Node aliases this to `kill()` and returns
/// `undefined`, so `using child = spawn(...)` terminates the subprocess on
/// scope exit. #2556.
pub(crate) extern "C" fn cp_method_dispose(closure: *const ClosureHeader) -> f64 {
    let _ = cp_method_kill(closure, cp_undefined());
    cp_undefined()
}
pub(crate) fn js_fork_child(args_len: usize) -> f64 {
    if args_len < 2 {
        crate::node_submodules::diagnostics::throw_type_error_no_code(
            b"Cannot destructure property 'initMessageChannel' of 'serialization[serializationMode]' as it is undefined.",
        );
    }
    f64::from_bits(JSValue::undefined().bits())
}
/// `removeListener(event, cb)` / `off(event, cb)` — rebuild the `event`
/// listener array without the matching closure (compared by NaN-boxed bits).
/// #1780.
pub(crate) extern "C" fn cp_method_remove_listener(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    let this = cp_this(closure);
    if let Some(name) = cp_value_to_string(event) {
        let key = cp_listener_key(&name);
        if let Some(arr) = cp_array_ptr(cp_get_field(this, &key)) {
            let n = crate::array::js_array_length(arr);
            let mut out = crate::array::js_array_alloc(n);
            for i in 0..n {
                let v = crate::array::js_array_get_f64(arr, i);
                if v.to_bits() != cb.to_bits() {
                    out = crate::array::js_array_push_f64(out, v);
                }
            }
            cp_set_field(this, &key, cp_box_ptr(out as *const u8));
        }
    }
    this
}

/// `removeAllListeners([event])` — clear one event's listener list, or every
/// `__cpL_*` list when called with no event. #1780.
pub(crate) extern "C" fn cp_method_remove_all_listeners(
    closure: *const ClosureHeader,
    event: f64,
) -> f64 {
    let this = cp_this(closure);
    if let Some(name) = cp_value_to_string(event) {
        let key = cp_listener_key(&name);
        let empty = crate::array::js_array_alloc(0);
        cp_set_field(this, &key, cp_box_ptr(empty as *const u8));
        return this;
    }
    // No event argument: clear every listener array on the object.
    if let Some(obj) = cp_object_ptr(this) {
        let keys = crate::object::js_object_keys(obj);
        if !keys.is_null() {
            let n = crate::array::js_array_length(keys);
            for i in 0..n {
                if let Some(k) = cp_value_to_string(crate::array::js_array_get_f64(keys, i)) {
                    if k.as_bytes().starts_with(b"__cpL_") {
                        let empty = crate::array::js_array_alloc(0);
                        cp_set_field(this, k.as_bytes(), cp_box_ptr(empty as *const u8));
                    }
                }
            }
        }
    }
    this
}

pub(crate) extern "C" fn cp_method_read(_closure: *const ClosureHeader, _n: f64) -> f64 {
    TAG_NULL_F64
}

/// `child.stdout.pipe(dest)` — forward every `data` chunk to `dest.write(chunk)`
/// and call `dest.end()` at source EOF. Node skips the end-call for
/// `process.stdout`/`process.stderr`; those stream objects expose no `end`
/// method, so the lookup-miss skip below matches that naturally. Returns
/// `dest` (Node returns the destination for chaining).
pub(crate) extern "C" fn cp_method_pipe(closure: *const ClosureHeader, dest: f64) -> f64 {
    let this = cp_this(closure);
    js_register_closure_arity(cp_pipe_data_thunk as *const u8, 1);
    js_register_closure_arity(cp_pipe_end_thunk as *const u8, 0);

    let data_thunk = js_closure_alloc(cp_pipe_data_thunk as *const u8, 1);
    js_closure_set_capture_ptr(data_thunk, 0, dest.to_bits() as i64);
    cp_register(
        this,
        cp_box_string("data"),
        cp_box_ptr(data_thunk as *const u8),
    );

    let end_thunk = js_closure_alloc(cp_pipe_end_thunk as *const u8, 1);
    js_closure_set_capture_ptr(end_thunk, 0, dest.to_bits() as i64);
    cp_register(
        this,
        cp_box_string("end"),
        cp_box_ptr(end_thunk as *const u8),
    );

    dest
}

/// Pipe `data` forwarder: slot 0 = the destination; call `dest.write(chunk)`.
pub(crate) extern "C" fn cp_pipe_data_thunk(closure: *const ClosureHeader, chunk: f64) -> f64 {
    let dest = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    let write = cp_get_field(dest, b"write");
    if !crate::fs::extract_closure_ptr(write).is_null() {
        let prev = js_implicit_this_set(dest);
        let args = [chunk];
        unsafe {
            let _ = js_native_call_value(write, args.as_ptr(), args.len());
        }
        js_implicit_this_set(prev);
    }
    cp_undefined()
}

/// Pipe `end` forwarder: slot 0 = the destination; call `dest.end()` when the
/// destination has one (`process.stdout`/`process.stderr` do not — matching
/// Node's doEnd exclusion for them).
pub(crate) extern "C" fn cp_pipe_end_thunk(closure: *const ClosureHeader) -> f64 {
    let dest = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    let end = cp_get_field(dest, b"end");
    if !crate::fs::extract_closure_ptr(end).is_null() {
        let prev = js_implicit_this_set(dest);
        let args = [cp_undefined()];
        unsafe {
            let _ = js_native_call_value(end, args.as_ptr(), 0);
        }
        js_implicit_this_set(prev);
    }
    cp_undefined()
}
/// `child.stdin.write(chunk[, encoding][, callback])` — #1934. The `this` is
/// the stdin Writable; route the bytes to the live child's stdin via the
/// reactor. Returns `true` (Node's `write` returns whether the buffer can take
/// more — `true` for our synchronous pipe write).
pub(crate) extern "C" fn cp_method_write2(
    closure: *const ClosureHeader,
    chunk: f64,
    _enc: f64,
) -> f64 {
    let this = cp_this(closure);
    if let Some(handle) = cp_handle_of(this) {
        let bytes = cp_value_to_bytes(chunk);
        reactor::cp_live_stdin_write(handle, &bytes);
    }
    TAG_TRUE_F64
}

/// `child.send(message[, sendHandle][, options][, callback])` — serialize
/// `message` and write it to the IPC channel of a `fork()`ed child (#1933 /
/// #3316). The `this` is the ChildProcess.
///
/// Node semantics this matches (`subprocess.send.length === 4`):
/// - Returns `true` when the message was queued on an open channel, `false`
///   once the channel is closed (after `disconnect()`).
/// - The optional trailing `callback` fires asynchronously (on the next tick)
///   with `null` on success or an `Error [ERR_IPC_CHANNEL_CLOSED]`
///   (`message: "Channel closed"`) when the channel is closed.
///
/// The four value slots map to `message, sendHandle, options, callback`; the
/// callback is detected as the last *function* argument so the documented
/// optional `sendHandle` / `options` slots are skipped (those handle-/serialize-
/// option forms are otherwise no-ops here, matching the prior behavior).
pub(crate) extern "C" fn cp_method_send(
    closure: *const ClosureHeader,
    message: f64,
    a2: f64,
    a3: f64,
    a4: f64,
) -> f64 {
    let this = cp_this(closure);

    // The callback is the last argument when it is a function. dispatch pads
    // missing slots with `undefined`, so scan slots 4→2 for a closure.
    let callback = [a4, a3, a2]
        .into_iter()
        .find(|v| !crate::fs::extract_closure_ptr(*v).is_null());

    // A closed IPC channel (after `disconnect()`, or never connected) returns
    // `false` and reports `ERR_IPC_CHANNEL_CLOSED` to the callback.
    let connected = cp_get_field(this, b"connected");
    let channel_open = connected.to_bits() == TAG_TRUE_F64.to_bits();

    let ok = if channel_open {
        match cp_handle_of(this) {
            Some(handle) => reactor::cp_ipc_send(handle, message),
            None => false,
        }
    } else {
        false
    };

    if let Some(cb) = callback {
        cp_defer_send_callback(cb, ok);
    }

    if ok {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}

/// Schedule the `send` callback to fire on the next tick (Node delivers it
/// asynchronously). `ok` selects the argument: `null` on success, otherwise an
/// `Error [ERR_IPC_CHANNEL_CLOSED]` (`message: "Channel closed"`). The deferred
/// closure captures the callback in slot 0 and the success flag in slot 1.
pub(crate) fn cp_defer_send_callback(cb: f64, ok: bool) {
    let deferred = js_closure_alloc(cp_send_callback_thunk as *const u8, 2);
    js_closure_set_capture_ptr(deferred, 0, cb.to_bits() as i64);
    let flag = if ok { TAG_TRUE_F64 } else { TAG_FALSE_F64 };
    js_closure_set_capture_ptr(deferred, 1, flag.to_bits() as i64);
    crate::timer::js_set_immediate_callback(deferred as i64);
}

/// Deferred `send` callback body. Slot 0 = the user callback; slot 1 = the
/// success flag. Invokes `callback(null)` on success or `callback(err)` with a
/// Node-shaped `ERR_IPC_CHANNEL_CLOSED` error on failure.
pub(crate) extern "C" fn cp_send_callback_thunk(closure: *const ClosureHeader) -> f64 {
    let cb = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    if crate::fs::extract_closure_ptr(cb).is_null() {
        return cp_undefined();
    }
    let flag = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    let ok = flag.to_bits() == TAG_TRUE_F64.to_bits();
    let arg = if ok {
        TAG_NULL_F64
    } else {
        cp_channel_closed_error()
    };
    let args = [arg];
    unsafe { js_native_call_value(cb, args.as_ptr(), args.len()) };
    cp_undefined()
}

/// Build a Node-shaped `Error [ERR_IPC_CHANNEL_CLOSED]` value (`message:
/// "Channel closed"`, `code: "ERR_IPC_CHANNEL_CLOSED"`).
pub(crate) fn cp_channel_closed_error() -> f64 {
    let msg = js_string_from_bytes(b"Channel closed".as_ptr(), 14);
    crate::node_submodules::register_error_code_pub(msg, "ERR_IPC_CHANNEL_CLOSED");
    let err = crate::error::js_error_new_with_message(msg);
    crate::value::js_nanbox_pointer(err as i64)
}

/// `child.disconnect()` — close the IPC channel (#1933). Flips `connected` to
/// `false`, `channel` to `null`, and emits a `disconnect` event.
pub(crate) extern "C" fn cp_method_disconnect(closure: *const ClosureHeader) -> f64 {
    let this = cp_this(closure);
    if let Some(handle) = cp_handle_of(this) {
        reactor::cp_ipc_disconnect(handle);
    }
    cp_set_field(this, b"connected", TAG_FALSE_F64);
    cp_set_field(this, b"channel", TAG_NULL_F64);
    cp_emit(this, "disconnect", &[]);
    cp_undefined()
}

/// `child.stdin.end([chunk])` — write the optional final chunk, then close the
/// pipe so the child sees EOF (#1934). The `this` is the stdin Writable.
pub(crate) extern "C" fn cp_method_stdin_end(closure: *const ClosureHeader, chunk: f64) -> f64 {
    let this = cp_this(closure);
    if let Some(handle) = cp_handle_of(this) {
        // Optional final data chunk. Skip `undefined`, the `0.0` arg-padding
        // sentinel, and a callback argument (`end(cb)`).
        let bits = chunk.to_bits();
        if !JSValue::from_bits(bits).is_undefined()
            && bits != 0
            && crate::fs::extract_closure_ptr(chunk).is_null()
        {
            let bytes = cp_value_to_bytes(chunk);
            if !bytes.is_empty() {
                reactor::cp_live_stdin_write(handle, &bytes);
            }
        }
        reactor::cp_live_stdin_close(handle);
    }
    this
}

/// Read the reactor registry key (`__cpHandle`) off a ChildProcess / stdio
/// sub-object, set by `spawn`. `None` when absent (e.g. a buffered child).
pub(crate) fn cp_handle_of(this: f64) -> Option<u64> {
    let h = cp_get_field(this, b"__cpHandle");
    if JSValue::from_bits(h.to_bits()).is_undefined() {
        return None;
    }
    if h.is_finite() && h >= 0.0 {
        Some(h as u64)
    } else {
        None
    }
}
