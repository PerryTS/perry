//! createReadStream / createWriteStream — real-file-backed streams.

use super::*;

// ============================================================
// createWriteStream / createReadStream — real implementation.
//
// Returns an ObjectHeader whose fields are NaN-boxed closure
// pointers keyed by method name (`write`, `end`, `on`, `once`,
// `close`). The closures capture a stream id in slot 0, which
// indexes into STREAM_REGISTRY for the in-memory buffer/state.
//
// The generic `js_native_call_method` dispatcher scans object
// keys and dispatches the matching closure via `js_native_call_value`,
// so `ws.write(x)` / `ws.on('finish', cb)` flow through unchanged.
//
// Stream semantics match Node's common `end(); on('finish', cb)`
// pattern: write() buffers, end() flushes to disk and marks the
// state finished, and on('finish', cb) fires cb inline if the
// stream is already finished (or stashes it otherwise).
// ============================================================
use std::cell::RefCell;
use std::collections::HashMap as StdHashMap;

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::object::{js_object_alloc_with_shape, js_object_set_field, ObjectHeader};
use crate::value::JSValue;

pub(crate) const TAG_UNDEFINED_STREAM: u64 = 0x7FFC_0000_0000_0001;
pub(crate) const STREAM_SHAPE_ID: u32 = 0x7FFF_FE40;

/// State for a single file stream (read OR write).
#[derive(Default)]
pub(crate) struct StreamState {
    /// Filesystem path the stream is bound to.
    path: String,
    /// In-memory buffer: for write streams this accumulates chunks
    /// until `end()` flushes them; for read streams it holds the
    /// pre-read file contents.
    buffer: Vec<u8>,
    /// True once `end()` has been called (write streams) or the
    /// initial read has happened (read streams).
    finished: bool,
    /// If an IO error occurred, this holds the error message.
    error_msg: Option<String>,
    /// If `on('finish', cb)` was registered BEFORE `end()` was
    /// called, the callback is stashed here and fired from end().
    pending_finish: Option<f64>,
    /// Write stream open flag (`w`, `a`, `wx`, ...). Read streams leave this empty.
    write_flag: String,
    /// Whether read streams should emit strings instead of Buffers.
    read_as_string: bool,
}

thread_local! {
    static STREAM_REGISTRY: RefCell<StdHashMap<usize, StreamState>> = RefCell::new(StdHashMap::new());
    static FS_STREAM_NEXT_ID: RefCell<usize> = const { RefCell::new(1) };
}

/// Allocate a new stream id and store the initial state.
pub(crate) fn alloc_stream(state: StreamState) -> usize {
    let id = FS_STREAM_NEXT_ID.with(|c| {
        let mut c = c.borrow_mut();
        let id = *c;
        *c += 1;
        id
    });
    STREAM_REGISTRY.with(|r| {
        r.borrow_mut().insert(id, state);
    });
    id
}

/// Extract a UTF-8 path from a NaN-boxed string value. Returns
/// empty string if the value isn't a string.
pub(crate) fn path_from_value(v: f64) -> String {
    unsafe { decode_path_value(v).unwrap_or_default() }
}

/// Extract raw UTF-8 bytes from a NaN-boxed string value.
pub(crate) fn bytes_from_value(v: f64) -> Vec<u8> {
    unsafe {
        if crate::buffer::js_buffer_is_buffer(v.to_bits() as i64) == 1 {
            let buf = buffer_ptr_from_value(v);
            if !buf.is_null() {
                let len = (*buf).length as usize;
                let data = crate::buffer::buffer_data(buf);
                return std::slice::from_raw_parts(data, len).to_vec();
            }
        }
        let bits = v.to_bits();
        let addr = if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            bits as usize
        };
        if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
            let ta = addr as *const crate::typedarray::TypedArrayHeader;
            if let Some(bytes) = crate::typedarray::typed_array_bytes(ta) {
                return bytes.to_vec();
            }
        }
        let ptr = extract_string_ptr(v);
        if ptr.is_null() {
            return Vec::new();
        }
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

/// Allocate a fresh ClosureHeader whose func_ptr is `func` and
/// whose slot 0 holds the given stream id.
pub(crate) fn make_stream_closure(func: extern "C" fn(), stream_id: usize) -> *mut ClosureHeader {
    let closure = js_closure_alloc(func as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, stream_id as i64);
    closure
}

/// Build the stream object: an ObjectHeader keyed by method names
/// whose values are NaN-boxed closure pointers. The caller provides
/// the per-method extern helper functions; each closure captures the
/// stream id in slot 0.
#[allow(clippy::type_complexity)]
pub(crate) fn build_stream_object(
    stream_id: usize,
    method_funcs: &[(&str, extern "C" fn())],
) -> *mut ObjectHeader {
    // Build a packed-keys byte sequence: "write\0end\0on\0once\0close\0"
    let mut packed: Vec<u8> = Vec::new();
    for (name, _) in method_funcs {
        packed.extend_from_slice(name.as_bytes());
        packed.push(0);
    }
    // Use a unique shape id per method-set so the SHAPE_CACHE doesn't
    // collide with other allocations. Since read/write use different
    // method sets, we use +0 for write, +1 for read (set by caller).
    let field_count = method_funcs.len() as u32;
    // NOTE: shape id uniqueness is on the caller side — pass the right
    // constant. We use STREAM_SHAPE_ID as base below.
    let obj = js_object_alloc_with_shape(
        STREAM_SHAPE_ID + method_funcs.len() as u32,
        field_count,
        packed.as_ptr(),
        packed.len() as u32,
    );
    for (i, (_name, func)) in method_funcs.iter().enumerate() {
        let closure = make_stream_closure(*func, stream_id);
        // Store as a NaN-boxed pointer (POINTER_TAG) so the dispatcher's
        // `field_val.is_pointer()` check succeeds.
        let val = JSValue::pointer(closure as *const u8);
        js_object_set_field(obj, i as u32, val);
    }
    obj
}

// ------------------------------------------------------------
// Write stream helpers.
// Each helper is an `extern "C" fn(*const ClosureHeader, ...)`
// matching the closure-call ABI. Slot 0 of the closure holds the
// stream id.
// ------------------------------------------------------------

/// Extract the stream id from the closure's capture slot 0.
#[inline]
pub(crate) fn stream_id_of(closure: *const ClosureHeader) -> usize {
    js_closure_get_capture_ptr(closure, 0) as usize
}

/// `ws.write(chunk)` — append chunk bytes to the in-memory buffer.
pub(crate) extern "C" fn write_stream_write_impl(closure: *const ClosureHeader, chunk: f64) -> f64 {
    let id = stream_id_of(closure);
    let chunk_bytes = bytes_from_value(chunk);
    STREAM_REGISTRY.with(|r| {
        if let Some(state) = r.borrow_mut().get_mut(&id) {
            state.buffer.extend_from_slice(&chunk_bytes);
        }
    });
    // Node returns `true` if the buffer is below the highWaterMark.
    // For our sync impl, always return true.
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    f64::from_bits(TAG_TRUE)
}

/// `ws.end()` — flush the buffer to disk, mark finished, and fire
/// any pending finish listener.
// #854: fs write-stream end helper retained for the stream subsystem
#[allow(dead_code)]
pub(crate) extern "C" fn write_stream_end0_impl(closure: *const ClosureHeader) -> f64 {
    write_stream_end_internal(closure, None)
}

/// `ws.end(finalChunk)` — write finalChunk, then flush.
pub(crate) extern "C" fn write_stream_end1_impl(closure: *const ClosureHeader, chunk: f64) -> f64 {
    write_stream_end_internal(closure, Some(chunk))
}

pub(crate) fn write_stream_end_internal(
    closure: *const ClosureHeader,
    final_chunk: Option<f64>,
) -> f64 {
    use crate::closure::js_closure_call0;
    let id = stream_id_of(closure);

    // Append optional final chunk.
    if let Some(chunk) = final_chunk {
        let bytes = bytes_from_value(chunk);
        STREAM_REGISTRY.with(|r| {
            if let Some(state) = r.borrow_mut().get_mut(&id) {
                state.buffer.extend_from_slice(&bytes);
            }
        });
    }

    // Flush to disk. Take the buffer out so we don't hold the
    // registry borrow across `fs::write`.
    let (path, buffer, flag) = STREAM_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        if let Some(state) = reg.get_mut(&id) {
            let p = state.path.clone();
            let b = std::mem::take(&mut state.buffer);
            let f = if state.write_flag.is_empty() {
                "w".to_string()
            } else {
                state.write_flag.clone()
            };
            (p, b, f)
        } else {
            (String::new(), Vec::new(), "w".to_string())
        }
    });

    let write_result = if path.is_empty() {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no path",
        ))
    } else {
        open_file_for_write_flag(&path, &flag).and_then(|mut file| file.write_all(&buffer))
    };

    // Mark finished / record error.
    let pending_finish = STREAM_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let state = reg.get_mut(&id);
        if let Some(state) = state {
            state.finished = true;
            if let Err(e) = &write_result {
                state.error_msg = Some(format!("{}", e));
            }
            state.pending_finish.take()
        } else {
            None
        }
    });

    // Fire any pending finish listener.
    if let Some(cb) = pending_finish {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            js_closure_call0(cb_ptr);
        }
    }

    f64::from_bits(TAG_UNDEFINED_STREAM)
}

/// `ws.on(event, cb)` — register a listener. For 'finish' this
/// fires synchronously if the stream is already finished; for
/// 'error' it checks for a recorded error. Unknown events noop.
pub(crate) extern "C" fn write_stream_on_impl(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    use crate::closure::{js_closure_call0, js_closure_call1};
    let id = stream_id_of(closure);
    let event_bytes = bytes_from_value(event);

    // Snapshot state under the borrow, then act without holding it.
    let (is_finished, err_msg, cb_is_finish) = STREAM_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(state) = reg.get_mut(&id) else {
            return (false, None, false);
        };
        match event_bytes.as_slice() {
            b"finish" | b"close" => {
                if state.finished && state.error_msg.is_none() {
                    (true, None, true)
                } else if !state.finished {
                    // Stash for later — will fire from end().
                    state.pending_finish = Some(cb);
                    (false, None, false)
                } else {
                    (false, None, false)
                }
            }
            b"error" => {
                if let Some(msg) = &state.error_msg {
                    (false, Some(msg.clone()), false)
                } else {
                    (false, None, false)
                }
            }
            _ => (false, None, false),
        }
    });

    if cb_is_finish && is_finished {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            js_closure_call0(cb_ptr);
        }
    }

    if let Some(msg) = err_msg {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            let msg_bytes = msg.as_bytes();
            let err_str = js_string_from_bytes(msg_bytes.as_ptr(), msg_bytes.len() as u32);
            let err_obj = crate::error::js_error_new_with_message(err_str);
            let err_val = crate::value::js_nanbox_pointer(err_obj as i64);
            js_closure_call1(cb_ptr, err_val);
        }
    }

    // `.on()` in Node returns the stream itself for chaining, but
    // we don't track the receiver inside the closure — return
    // undefined, which matches most practical uses since the test
    // pattern `stream.on('...', cb)` discards the return.
    f64::from_bits(TAG_UNDEFINED_STREAM)
}

/// `ws.close()` — noop; the stream is flushed on end().
pub(crate) extern "C" fn write_stream_close_impl(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED_STREAM)
}

// ------------------------------------------------------------
// Read stream helpers.
// ------------------------------------------------------------

/// `rs.on(event, cb)` — for 'data' fires cb(contents) once,
/// for 'end' fires cb() once (after all data), for 'error'
/// noops unless the file was unreadable.
pub(crate) extern "C" fn read_stream_on_impl(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    use crate::closure::{js_closure_call0, js_closure_call1};
    let id = stream_id_of(closure);
    let event_bytes = bytes_from_value(event);

    // Pull needed data out of the registry without holding the borrow
    // across the callback invocation.
    let (buffer_copy, err_msg, read_as_string) = STREAM_REGISTRY.with(|r| {
        let reg = r.borrow();
        match reg.get(&id) {
            Some(s) => (s.buffer.clone(), s.error_msg.clone(), s.read_as_string),
            None => (Vec::new(), None, false),
        }
    });

    match event_bytes.as_slice() {
        b"data" => {
            if err_msg.is_some() {
                return f64::from_bits(TAG_UNDEFINED_STREAM);
            }
            let cb_ptr = extract_closure_ptr(cb);
            if !cb_ptr.is_null() {
                let chunk_val = if read_as_string {
                    let chunk =
                        js_string_from_bytes(buffer_copy.as_ptr(), buffer_copy.len() as u32);
                    f64::from_bits(crate::value::js_nanbox_string(chunk as i64).to_bits())
                } else {
                    buffer_value_from_bytes(&buffer_copy)
                };
                js_closure_call1(cb_ptr, chunk_val);
            }
        }
        b"end" | b"close" => {
            if err_msg.is_some() {
                return f64::from_bits(TAG_UNDEFINED_STREAM);
            }
            let cb_ptr = extract_closure_ptr(cb);
            if !cb_ptr.is_null() {
                js_closure_call0(cb_ptr);
            }
        }
        b"error" => {
            if let Some(msg) = err_msg {
                let cb_ptr = extract_closure_ptr(cb);
                if !cb_ptr.is_null() {
                    let msg_bytes = msg.as_bytes();
                    let err_str = js_string_from_bytes(msg_bytes.as_ptr(), msg_bytes.len() as u32);
                    let err_obj = crate::error::js_error_new_with_message(err_str);
                    let err_val = crate::value::js_nanbox_pointer(err_obj as i64);
                    js_closure_call1(cb_ptr, err_val);
                }
            }
        }
        _ => {}
    }

    f64::from_bits(TAG_UNDEFINED_STREAM)
}

/// `rs.pipe(dest)` — not implemented beyond the noop signature.
pub(crate) extern "C" fn read_stream_pipe_impl(_closure: *const ClosureHeader, dest: f64) -> f64 {
    dest
}

/// `rs.close()` — noop.
pub(crate) extern "C" fn read_stream_close_impl(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED_STREAM)
}

// ------------------------------------------------------------
// Closure pointer extraction helper.
// ------------------------------------------------------------

/// Extract a raw ClosureHeader pointer from a NaN-boxed f64.
pub(crate) fn extract_closure_ptr(v: f64) -> *const ClosureHeader {
    let bits = v.to_bits();
    let top16 = bits >> 48;
    let raw = if (0x7FF8..=0x7FFF).contains(&top16) {
        // Tagged NaN-box — mask off the tag.
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 {
        bits as usize
    } else {
        return std::ptr::null();
    };
    if raw < 0x1000 || !crate::closure::is_closure_ptr(raw) {
        std::ptr::null()
    } else {
        raw as *const ClosureHeader
    }
}

// ------------------------------------------------------------
// Entry points: js_fs_create_write_stream / js_fs_create_read_stream
// ------------------------------------------------------------

/// Create a write stream bound to `path_value`. Returns a NaN-boxed
/// ObjectHeader pointer whose fields dispatch to the write-stream
/// helpers.
#[no_mangle]
pub extern "C" fn js_fs_create_write_stream(path_value: f64, options_value: f64) -> f64 {
    let path = path_from_value(path_value);
    let write_flag = file_options_flag(options_value, "w");
    let state = StreamState {
        path,
        write_flag,
        ..StreamState::default()
    };
    let id = alloc_stream(state);
    // Method table. Order is locked in — it determines the shape keys.
    // Using a unique method count (6) that differs from the read
    // stream's (5) so the shape cache doesn't alias.
    let method_funcs: [(&str, extern "C" fn()); 6] = [
        ("write", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                write_stream_write_impl,
            )
        }),
        ("end", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                write_stream_end1_impl,
            )
        }),
        ("on", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(write_stream_on_impl)
        }),
        ("once", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(write_stream_on_impl)
        }),
        ("close", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                write_stream_close_impl,
            )
        }),
        ("destroy", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                write_stream_close_impl,
            )
        }),
    ];
    let obj = build_stream_object(id, &method_funcs);
    // NaN-box as POINTER_TAG so the dispatcher's `is_pointer()` check
    // routes through the object-field scan in js_native_call_method.
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// Create a read stream: pre-read the file contents into the
/// registry buffer, then return an ObjectHeader whose `.on` fires
/// the data/end callbacks synchronously on first call.
#[no_mangle]
pub extern "C" fn js_fs_create_read_stream(path_value: f64, options_value: f64) -> f64 {
    let path = path_from_value(path_value);
    let mut state = StreamState {
        path: path.clone(),
        read_as_string: fs_encoding_option(options_value).is_some_and(|enc| enc != "buffer"),
        ..StreamState::default()
    };
    // Eagerly read the file so the data callback can fire synchronously.
    match std::fs::read(&path) {
        Ok(contents) => {
            let start = unsafe { options_number_field(options_value, b"start") }
                .map(|n| n.max(0.0) as usize)
                .unwrap_or(0);
            let end_inclusive = unsafe { options_number_field(options_value, b"end") }
                .map(|n| n.max(0.0) as usize)
                .unwrap_or_else(|| contents.len().saturating_sub(1));
            state.buffer = if start >= contents.len() {
                Vec::new()
            } else {
                let end_exclusive = end_inclusive.saturating_add(1).min(contents.len());
                contents[start..end_exclusive].to_vec()
            };
            state.finished = true;
        }
        Err(e) => {
            state.error_msg = Some(format!("{}", e));
        }
    }
    let id = alloc_stream(state);
    // Method set of length 5 to avoid shape-cache collision with write
    // streams (which have length 6).
    let method_funcs: [(&str, extern "C" fn()); 5] = [
        ("on", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(read_stream_on_impl)
        }),
        ("once", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(read_stream_on_impl)
        }),
        ("pipe", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                read_stream_pipe_impl,
            )
        }),
        ("close", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                read_stream_close_impl,
            )
        }),
        ("destroy", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                read_stream_close_impl,
            )
        }),
    ];
    let obj = build_stream_object(id, &method_funcs);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}
