use super::*;

use std::collections::HashMap;
use std::fs::File;
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

use sync_run::{
    cp_read_async_run_options, cp_read_spawn_sync_run_options, cp_read_sync_stdio_run_options,
    cp_run_to_completion, CpRun, CpRunError, CpRunOptions,
};

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr, js_native_call_value,
    js_register_closure_arity, ClosureHeader,
};
use crate::object::{
    js_implicit_this_get, js_implicit_this_set, js_object_alloc_with_shape,
    js_object_get_field_by_name_f64, js_object_set_field, js_object_set_field_by_name,
    ObjectHeader,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

// Shape-id band kept clear of node_stream (0x7FFF_FE60+), fs streams
// (0x7FFF_FE40), and weakref (0x7FFF_FE10+).
pub(crate) const CP_SHAPE_ID: u32 = 0x7FFF_FD00;
pub(crate) const CP_READABLE_SHAPE_ID: u32 = 0x7FFF_FD40;
pub(crate) const CP_WRITABLE_SHAPE_ID: u32 = 0x7FFF_FD80;

// ----- object construction -----

pub(crate) type CpFn = unsafe extern "C" fn();
#[allow(clippy::missing_transmute_annotations)]
pub(crate) fn cp_cast0(f: extern "C" fn(*const ClosureHeader) -> f64) -> CpFn {
    unsafe { std::mem::transmute(f) }
}
#[allow(clippy::missing_transmute_annotations)]
pub(crate) fn cp_cast1(f: extern "C" fn(*const ClosureHeader, f64) -> f64) -> CpFn {
    unsafe { std::mem::transmute(f) }
}
#[allow(clippy::missing_transmute_annotations)]
pub(crate) fn cp_cast2(f: extern "C" fn(*const ClosureHeader, f64, f64) -> f64) -> CpFn {
    unsafe { std::mem::transmute(f) }
}
#[allow(clippy::missing_transmute_annotations)]
pub(crate) fn cp_cast4(f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64) -> f64) -> CpFn {
    unsafe { std::mem::transmute(f) }
}

pub(crate) fn cp_register_arities() {
    js_register_closure_arity(cp_method_on as *const u8, 2);
    js_register_closure_arity(cp_method_emit as *const u8, 2);
    js_register_closure_arity(cp_method_this0 as *const u8, 0);
    js_register_closure_arity(cp_method_this1 as *const u8, 1);
    js_register_closure_arity(cp_method_remove_listener as *const u8, 2);
    js_register_closure_arity(cp_method_remove_all_listeners as *const u8, 1);
    js_register_closure_arity(cp_method_kill as *const u8, 1);
    js_register_closure_arity(cp_method_dispose as *const u8, 0);
    crate::closure::js_register_closure_length(cp_method_dispose as *const u8, 0);
    js_register_closure_arity(cp_method_read as *const u8, 1);
    js_register_closure_arity(cp_method_pipe as *const u8, 1);
    js_register_closure_arity(cp_method_write2 as *const u8, 2);
    js_register_closure_arity(cp_method_stdin_end as *const u8, 1);
    // #3316: `send(message, sendHandle, options, callback)` — dispatch with 4
    // padded slots so the trailing callback is visible regardless of call-site
    // arity, and report `child.send.length === 4` like Node.
    js_register_closure_arity(cp_method_send as *const u8, 4);
    crate::closure::js_register_closure_length(cp_method_send as *const u8, 4);
    js_register_closure_arity(cp_method_disconnect as *const u8, 0);
    // The deferred send-callback thunk takes no JS args.
    js_register_closure_arity(cp_send_callback_thunk as *const u8, 0);
}

/// Allocate a heap object whose method-name fields each hold a closure capturing
/// the object itself in slot 0 (so method bodies recover `this`).
pub(crate) fn cp_build_object(methods: &[(&str, CpFn)], shape_id: u32) -> *mut ObjectHeader {
    let mut packed: Vec<u8> = Vec::new();
    for (name, _) in methods {
        packed.extend_from_slice(name.as_bytes());
        packed.push(0);
    }
    let obj = js_object_alloc_with_shape(
        shape_id,
        methods.len() as u32,
        packed.as_ptr(),
        packed.len() as u32,
    );
    let this_bits = JSValue::pointer(obj as *const u8).bits();
    for (i, (_name, func)) in methods.iter().enumerate() {
        let closure = js_closure_alloc(*func as *const u8, 1);
        js_closure_set_capture_ptr(closure, 0, this_bits as i64);
        js_object_set_field(obj, i as u32, JSValue::pointer(closure as *const u8));
    }
    obj
}

pub(crate) fn cp_install_dispose(cp: f64) {
    let Some(obj) = cp_object_ptr(cp) else {
        return;
    };

    let closure = js_closure_alloc(cp_method_dispose as *const u8, 1);
    if closure.is_null() {
        return;
    }
    js_closure_set_capture_ptr(closure, 0, cp.to_bits() as i64);
    crate::object::set_bound_native_closure_name(closure, "");
    crate::object::set_builtin_closure_length(closure as usize, 0);
    let dispose_value = cp_box_ptr(closure as *const u8);

    let hidden_attrs = crate::object::PropertyAttrs::new(true, false, true);
    for key in ["__perry_dispose__", "@@__perry_wk_dispose"] {
        cp_set_field(cp, key.as_bytes(), dispose_value);
        crate::object::set_builtin_property_attrs(obj as usize, key.to_string(), hidden_attrs);
    }

    let dispose_sym = crate::symbol::well_known_symbol("dispose");
    if !dispose_sym.is_null() {
        let dispose_sym_value = cp_box_ptr(dispose_sym as *const u8);
        unsafe {
            crate::symbol::js_object_set_symbol_property(cp, dispose_sym_value, dispose_value);
        }
    }
}

/// Build a stdout/stderr Readable-shaped EventEmitter.
pub(crate) fn cp_build_readable() -> f64 {
    let methods: [(&str, CpFn); 13] = [
        ("on", cp_cast2(cp_method_on)),
        ("once", cp_cast2(cp_method_on)),
        ("addListener", cp_cast2(cp_method_on)),
        ("prependListener", cp_cast2(cp_method_on)),
        ("off", cp_cast2(cp_method_remove_listener)),
        ("removeListener", cp_cast2(cp_method_remove_listener)),
        ("emit", cp_cast2(cp_method_emit)),
        ("pause", cp_cast0(cp_method_this0)),
        ("resume", cp_cast0(cp_method_this0)),
        ("destroy", cp_cast0(cp_method_this0)),
        ("setEncoding", cp_cast1(cp_method_this1)),
        ("read", cp_cast1(cp_method_read)),
        ("pipe", cp_cast1(cp_method_pipe)),
    ];
    let obj = cp_build_object(&methods, CP_READABLE_SHAPE_ID + methods.len() as u32);
    let val = cp_box_ptr(obj as *const u8);
    cp_set_field(val, b"readable", TAG_TRUE_F64);
    cp_set_field(val, b"destroyed", TAG_FALSE_F64);
    // A child's `stdout`/`stderr` must be async-iterable, like Node's: both
    // `for await (const chunk of child.stdout)` and the `isAsyncIterable` probe
    // every stream-consuming library runs —
    // `typeof stream[Symbol.asyncIterator] === "function"` — depend on it.
    // Without the symbol, `get-stream` (used by execa) rejects the stream with
    // "The first argument must be a Readable, a ReadableStream, or an async
    // iterable", which silently aborted the background downloads of a large
    // esbuild-bundled CLI app.
    //
    // Reuse node:stream's iterator: it is event-driven (it attaches persistent
    // `data`/`end`/`error` listeners and settles a promise per pull), so it
    // drives this emitter-backed object as-is — `cp_emit` forwards to node:stream's
    // listener registry so those listeners fire.
    crate::node_stream::async_iterator::install_foreign_readable_async_iterator_symbol(val);
    val
}

/// Build a stdin Writable-shaped EventEmitter.
pub(crate) fn cp_build_writable() -> f64 {
    let methods: [(&str, CpFn); 11] = [
        ("on", cp_cast2(cp_method_on)),
        ("once", cp_cast2(cp_method_on)),
        ("addListener", cp_cast2(cp_method_on)),
        ("removeListener", cp_cast2(cp_method_remove_listener)),
        ("off", cp_cast2(cp_method_remove_listener)),
        ("emit", cp_cast2(cp_method_emit)),
        ("write", cp_cast2(cp_method_write2)),
        ("end", cp_cast1(cp_method_stdin_end)),
        ("destroy", cp_cast0(cp_method_this0)),
        ("cork", cp_cast0(cp_method_this0)),
        ("uncork", cp_cast0(cp_method_this0)),
    ];
    let obj = cp_build_object(&methods, CP_WRITABLE_SHAPE_ID + methods.len() as u32);
    let val = cp_box_ptr(obj as *const u8);
    cp_set_field(val, b"writable", TAG_TRUE_F64);
    cp_set_field(val, b"destroyed", TAG_FALSE_F64);
    val
}
